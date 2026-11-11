//! Seek-bar previews for direct-played files without server trickplay (ADR-0009, revision 5). One session per source,
//! opened in the background on the first request, so asking never blocks:
//! - the page says which time the pointer is over ([`player_preview_want`]); the newest request wins;
//! - Matroska files are read through their keyframe index: [`FETCHERS`] threads fetch the wanted keyframe (one range
//!   request each, sized from the keyframes before it), so a link slow to answer keeps up with the pointer;
//! - one thread decodes, the wanted keyframe first, else the newest fetched, and pushes each frame newer than the one
//!   on screen to the page ([`EVENT_PREVIEW`]); the page shows the latest it got until the exact one arrives; a second
//!   decoder joins only while the pointer is on the bar and the first is busy;
//! - while nobody hovers and the main player is healthy, a paced background fill decodes a coarse grid, so a far jump
//!   has a stand-in at once;
//! - playback comes first ([`Room`]): while the main player is short of buffered video, hovering fetches one keyframe
//!   at a time and nothing ahead, and the background fill waits.
//!
//! Other files seek in a stream opened in the preview handle, newest request first. Frames are kept in memory only.
//! The scheduling follows AIOStreams' thumbnailer (`thumbs/src/session.rs@87dee2c` of AIOStreams/media); decoding stays
//! in a second libmpv handle because our FFmpeg is linked into libmpv (ADR-0009).
use super::host::{Cmd, PlayerHost};
use crate::settings::SettingsStore;
use crate::silo::CmdResult;
use granary_player::Phase;
use serde::Serialize;
use std::collections::{BTreeMap, HashSet, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager, State};

/// A decoded preview pushed to the page.
pub const EVENT_PREVIEW: &str = "player://preview";
/// Keyframe frames kept per source (about 10 KB each).
const CACHE: usize = 600;
/// Keyframes fetched at once.
const FETCHERS: usize = 3;
/// Background grid: frames per level, coarse to fine. The total bounds the extra traffic per file (one keyframe each,
/// typically 0.1–1.5 MB on 4K remuxes).
const LEVELS: [usize; 2] = [12, 48];
/// Pause between two background frames, so the playing stream keeps the connection.
const PACE: Duration = Duration::from_millis(300);
/// Wait before the background fill starts, so the first seconds of playback get the bandwidth.
const WARM_DELAY: Duration = Duration::from_secs(4);
/// The background fill waits this long after the last hover request.
const HOVER_QUIET: Duration = Duration::from_millis(1500);
/// The background fill waits until the main player has this many seconds buffered (or its buffer is full). Low
/// enough that a link only a little faster than the file still gets its grid.
const WARM_CACHE: f64 = 10.0;
/// Below this many buffered seconds the main player is close to stalling: previews take the least they can.
const LOW_CACHE: f64 = 4.0;
/// How long a reading of the main player's buffer is used while hovering.
const ROOM_TTL: Duration = Duration::from_millis(500);
/// A source that failed to open is tried again after this long (a slow or busy link at the start of playback).
const RETRY: Duration = Duration::from_secs(15);
/// Neighbours of the keyframe under the pointer fetched ahead, in steps of the page's preview spots (negative:
/// behind the direction of travel).
const NEAR: [f64; 3] = [1.0, -1.0, 2.0];
/// Neighbours in flight at once; one fetch thread stays free for the pointer.
const NEAR_AT_ONCE: usize = FETCHERS - 1;
const OPEN_TIMEOUT: Duration = Duration::from_secs(20);
const FRAME_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Default)]
pub struct PreviewState {
    session: Mutex<Option<Arc<Session>>>,
    generation: AtomicU64,
}

/// The previews of one source. Dropping the last handle stops its threads after the read or decode they are in.
struct Session {
    source: String,
    generation: u64,
    shared: Arc<Shared>,
}

impl Drop for Session {
    fn drop(&mut self) {
        lock(&self.shared.state).closed = true;
        self.shared.wake.notify_all();
    }
}

struct Shared {
    state: Mutex<Sched>,
    wake: Condvar,
}

/// How much of the connection previews may take, from the main player's buffer.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Room {
    /// Loading, seeking, stalled or nearly out of buffered video: one keyframe at a time, only for the pointer.
    Tight,
    /// Playing with some buffer: hovering works at full speed; the background fill waits.
    Normal,
    /// Well buffered: the background fill may run too.
    Plenty,
}

enum Mode {
    Opening,
    #[cfg(windows)]
    Index(granary_player::thumbs::KeyframeFetcher),
    Seek,
    Failed,
}

struct Want {
    at: f64,
    /// The cache key (keyframe time in ms in index mode); unknown while the source opens.
    key: Option<u64>,
    asked: u64,
}

struct Fetched {
    /// The request it answers; 0 for the background fill.
    asked: u64,
    key: u64,
    file: Vec<u8>,
}

/// Scheduling state of one session.
struct Sched {
    mode: Mode,
    /// Requests so far; the wanted keyframe carries the count at its request.
    asked: u64,
    wanted: Option<Want>,
    /// Keys being fetched or decoded.
    busy: HashSet<u64>,
    /// Keys that failed since the pointer last moved to another.
    failed: HashSet<u64>,
    /// Fetched and not yet decoded.
    fetched: Vec<Fetched>,
    /// The request the last frame pushed to the page answered.
    shown: u64,
    frames: Frames,
    /// Background targets `(time, key)`, coarse to fine.
    warm: VecDeque<(f64, u64)>,
    warm_wanted: bool,
    warm_after: Instant,
    last_hover: Option<Instant>,
    /// Seconds between the page's preview spots (index mode; 0 until the source is open).
    step: f64,
    /// Direction the pointer last moved in: 1 forward, -1 back.
    dir: f64,
    /// The last reading of the main player's buffer, and when it was taken.
    room: Room,
    room_at: Option<Instant>,
    /// Keyframe downloads in flight.
    fetching: usize,
    /// The first decoder is in a decode.
    decoding: bool,
    /// When the source failed to open.
    failed_at: Option<Instant>,
    closed: bool,
}

impl Sched {
    fn new() -> Self {
        Self {
            mode: Mode::Opening,
            asked: 0,
            wanted: None,
            busy: HashSet::new(),
            failed: HashSet::new(),
            fetched: Vec::new(),
            shown: 0,
            frames: Frames::default(),
            warm: VecDeque::new(),
            warm_wanted: false,
            warm_after: Instant::now() + WARM_DELAY,
            last_hover: None,
            step: 0.0,
            dir: 1.0,
            room: Room::Normal,
            room_at: None,
            fetching: 0,
            decoding: false,
            failed_at: None,
            closed: false,
        }
    }

    fn fail(&mut self) {
        self.mode = Mode::Failed;
        self.failed_at = Some(Instant::now());
    }

    fn hovering(&self, now: Instant) -> bool {
        self.last_hover.is_some_and(|t| now.duration_since(t) < HOVER_QUIET)
    }

    /// A download for the pointer: the keyframe under it, else one next to it. While the main player is short of
    /// buffer, one at a time and nothing ahead. Counts the download as in flight.
    fn take_hover(&mut self, now: Instant) -> Option<(f64, u64, u64)> {
        let tight = self.room == Room::Tight;
        if self.fetching >= if tight { 1 } else { FETCHERS } {
            return None;
        }
        let job = self
            .take_wanted()
            .or_else(|| if tight { None } else { self.take_near(now) })?;
        self.fetching += 1;
        Some(job)
    }

    /// Work for the second decoder: only while the pointer is on the bar and the first decoder is busy, so an idle
    /// bar and the background fill cost one core's worth of decoding at most.
    fn take_fetched_second(&mut self, now: Instant) -> Option<Fetched> {
        if !self.decoding || !self.hovering(now) {
            return None;
        }
        self.take_fetched()
    }

    fn key_for(&self, at: f64) -> Option<u64> {
        match &self.mode {
            #[cfg(windows)]
            Mode::Index(f) => f.snap(at).and_then(bucket),
            Mode::Seek => bucket(at),
            Mode::Opening | Mode::Failed => None,
        }
    }

    fn open_for(&self, key: u64) -> bool {
        !self.frames.keys.contains_key(&key) && !self.busy.contains(&key) && !self.failed.contains(&key)
    }

    /// The wanted key when it still needs fetching (index mode) or decoding (seek mode); marks it busy.
    fn take_wanted(&mut self) -> Option<(f64, u64, u64)> {
        let w = self.wanted.as_ref()?;
        let key = w.key?;
        if !self.open_for(key) {
            return None;
        }
        let job = (w.at, key, w.asked);
        self.busy.insert(key);
        Some(job)
    }

    /// A keyframe next to the one under the pointer, once that one is made and while the pointer is still around:
    /// the spots a small move lands on are then answered from the cache. Ahead in the direction of travel first.
    /// Marks it busy.
    fn take_near(&mut self, now: Instant) -> Option<(f64, u64, u64)> {
        let w = self.wanted.as_ref()?;
        let (at, key) = (w.at, w.key?);
        if self.step <= 0.0
            || !self.frames.keys.contains_key(&key)
            || self.busy.len() >= NEAR_AT_ONCE
            || !self.hovering(now)
        {
            return None;
        }
        for n in NEAR {
            let t = at + n * self.dir * self.step;
            if let Some(k) = self.key_for(t)
                && k != key
                && self.open_for(k)
            {
                self.busy.insert(k);
                return Some((t, k, 0));
            }
        }
        None
    }

    /// The next background target, when nobody hovers and nothing else is in flight; marks it busy.
    fn take_warm(&mut self, now: Instant) -> Option<(f64, u64, u64)> {
        if !self.warm_wanted
            || now < self.warm_after
            || self.hovering(now)
            || !self.busy.is_empty()
            || !self.fetched.is_empty()
        {
            return None;
        }
        while let Some((at, key)) = self.warm.pop_front() {
            if self.open_for(key) {
                self.busy.insert(key);
                return Some((at, key, 0));
            }
        }
        None
    }

    /// The fetched keyframe to decode next: the wanted one, else the newest request.
    fn take_fetched(&mut self) -> Option<Fetched> {
        let wanted = self.wanted.as_ref().and_then(|w| w.key);
        let pick = (0..self.fetched.len())
            .find(|&i| Some(self.fetched[i].key) == wanted)
            .or_else(|| (0..self.fetched.len()).max_by_key(|&i| self.fetched[i].asked))?;
        let next = self.fetched.swap_remove(pick);
        // Hover keyframes the pointer has left are not decoded: that would only delay the one it is on (AIOStreams does
        // the same). Background ones are kept; filling the cache is their purpose.
        let (keep, stale): (Vec<_>, Vec<_>) = std::mem::take(&mut self.fetched)
            .into_iter()
            .partition(|f| f.asked == 0 || Some(f.key) == wanted);
        self.fetched = keep;
        for f in stale {
            self.busy.remove(&f.key);
        }
        Some(next)
    }

    /// Store a decoded frame; whether the page should get it (newer than what it shows, or the wanted one).
    fn finish(&mut self, key: u64, asked: u64, url: Option<Arc<String>>) -> bool {
        self.busy.remove(&key);
        let Some(url) = url else {
            self.failed.insert(key);
            return false;
        };
        self.frames.remember(key, url);
        let show = asked > 0 && (asked > self.shown || self.wanted.as_ref().is_some_and(|w| w.key == Some(key)));
        if show {
            self.shown = self.shown.max(asked);
        }
        show
    }

    fn fill_warm(&mut self) {
        #[cfg(windows)]
        if let Mode::Index(f) = &self.mode
            && self.warm.is_empty()
        {
            let mut seen = HashSet::new();
            let duration = f.last_keyframe().unwrap_or(0.0);
            self.warm = grid(duration)
                .into_iter()
                .filter_map(|at| f.snap(at).and_then(bucket).map(|k| (at, k)))
                .filter(|(_, k)| seen.insert(*k))
                .collect();
        }
    }
}

#[derive(Default)]
struct Frames {
    /// Keyframe time (ms) → JPEG data URL.
    keys: BTreeMap<u64, Arc<String>>,
    order: VecDeque<u64>,
}

impl Frames {
    fn remember(&mut self, key: u64, url: Arc<String>) {
        if self.keys.insert(key, url).is_none() {
            self.order.push_back(key);
            while self.order.len() > CACHE {
                if let Some(old) = self.order.pop_front() {
                    self.keys.remove(&old);
                }
            }
        }
    }

    /// The cached frame closest to `requested`, preferring the keyframe at or before it (what an exact preview shows).
    fn nearest(&self, requested: u64) -> Option<(u64, Arc<String>)> {
        let before = self.keys.range(..=requested).next_back();
        let after = self.keys.range(requested..).next();
        let pick = match (before, after) {
            (Some(b), Some(a)) => {
                if requested - b.0 <= a.0 - requested {
                    b
                } else {
                    a
                }
            }
            (Some(b), None) => b,
            (None, Some(a)) => a,
            (None, None) => return None,
        };
        Some((*pick.0, Arc::clone(pick.1)))
    }
}

/// Scratch folder versions up to 0.4.1 used for single-frame files and screenshots. Previews no longer write
/// anything; the folder is only swept.
pub fn temp_dir() -> std::path::PathBuf {
    std::env::temp_dir().join("granary-previews")
}

/// Startup: remove scratch files an older version left behind (crash, power loss). Files are named
/// `<kind><pid>-<n>.<ext>`; a file of this process is never touched. The folder itself goes when it is empty.
pub fn sweep_temp() {
    sweep_dir(&temp_dir(), std::process::id());
}

fn sweep_dir(dir: &std::path::Path, own_pid: u32) {
    let Ok(list) = std::fs::read_dir(dir) else { return };
    for e in list.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        let pid = name
            .get(1..)
            .and_then(|r| r.split('-').next())
            .and_then(|n| n.parse::<u32>().ok());
        if pid != Some(own_pid) {
            let _ = std::fs::remove_file(e.path());
        }
    }
    // Fails while other files remain, which is fine.
    let _ = std::fs::remove_dir(dir);
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    // A poisoned lock only means an earlier decode panicked; the data is still consistent.
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

fn wait<'a>(shared: &'a Shared, g: MutexGuard<'a, Sched>, timeout: Duration) -> MutexGuard<'a, Sched> {
    match shared.wake.wait_timeout(g, timeout) {
        Ok((g, _)) => g,
        Err(p) => p.into_inner().0,
    }
}

/// Cache key: a time in whole milliseconds.
fn bucket(seconds: f64) -> Option<u64> {
    // Finite and non-negative: the cast below cannot wrap (saturates at u64::MAX).
    (seconds.is_finite() && seconds >= 0.0).then(|| (seconds * 1000.0).round() as u64)
}

/// Background targets, coarse to fine: each level spreads its frames evenly over the file (centred in their slot), so
/// after the first level any hover has a frame within a twelfth of the file, then a 48th.
fn grid(duration: f64) -> Vec<f64> {
    if !duration.is_finite() || duration <= 0.0 {
        return Vec::new();
    }
    LEVELS
        .iter()
        .flat_map(|&n| (0..n).map(move |i| (i as f64 + 0.5) * duration / n as f64))
        .collect()
}

/// Seconds between the page's preview spots (`previewInterval` in `useLocalPreview.ts`).
fn spot_step(duration: f64) -> f64 {
    if !duration.is_finite() || duration <= 0.0 {
        return 10.0;
    }
    (duration / 400.0).ceil().clamp(2.0, 10.0)
}

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

fn base64(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for (i, shift) in [18u32, 12, 6, 0].into_iter().enumerate() {
            if i <= chunk.len() {
                out.push(char::from(B64[((n >> shift) & 63) as usize]));
            } else {
                out.push('=');
            }
        }
    }
    out
}

fn data_url(jpeg: &[u8]) -> Arc<String> {
    Arc::new(format!("data:image/jpeg;base64,{}", base64(jpeg)))
}

/// The stream previews may read, or `None` (previews off, transcode, stopped), closing the session in that case.
async fn source(app: &AppHandle, host: &PlayerHost, settings: &SettingsStore) -> Option<String> {
    if !settings.current().player.preview_source_on("local") {
        return None;
    }
    let source = host.ask(Cmd::PreviewSource).await.flatten();
    if source.is_none() {
        *lock(&app.state::<PreviewState>().session) = None;
    }
    source
}

impl PreviewState {
    /// Drop the open session (playback stopped): its threads end after the read or decode they are in.
    pub fn close(&self) {
        *lock(&self.session) = None;
    }

    /// The session for `source`, opening it (in the background) when the source changed.
    fn session_for(&self, app: &AppHandle, source: &str) -> Arc<Session> {
        let mut current = lock(&self.session);
        if let Some(s) = current.as_ref()
            && s.source == source
            && lock(&s.shared.state).failed_at.is_none_or(|t| t.elapsed() < RETRY)
        {
            return Arc::clone(s);
        }
        let session = Arc::new(Session {
            source: source.to_owned(),
            generation: self.generation.fetch_add(1, Ordering::SeqCst) + 1,
            shared: Arc::new(Shared {
                state: Mutex::new(Sched::new()),
                wake: Condvar::new(),
            }),
        });
        let (app, shared, source, generation) = (
            app.clone(),
            Arc::clone(&session.shared),
            source.to_owned(),
            session.generation,
        );
        let spawned = std::thread::Builder::new()
            .name("preview-render".into())
            .spawn(move || run(&app, &shared, &source, generation));
        if let Err(e) = spawned {
            tracing::warn!(error = %e, "seek previews: no render thread");
            lock(&session.shared.state).fail();
        }
        *current = Some(Arc::clone(&session));
        session
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NearFrame {
    /// The time the frame shows, in seconds (a keyframe time in index mode).
    at: f64,
    url: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WantReply {
    /// Identifies the source; pushed frames of another generation belong to an earlier file.
    generation: u64,
    /// The time the exact preview shows (the keyframe at or before the request), once the source is open.
    key: Option<f64>,
    /// The exact preview, when it was made before.
    url: Option<String>,
    /// Otherwise the closest frame made so far, as a stand-in.
    near: Option<NearFrame>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct PushedFrame {
    generation: u64,
    at: f64,
    url: Arc<String>,
}

/// Ask for the preview at `seconds` in the playing file. Answers at once: with the frame when it was made before,
/// otherwise with the nearest one made so far; the exact frame follows as an [`EVENT_PREVIEW`] event. `None` when
/// previews are off or nothing previewable is playing (transcode, stopped).
#[tauri::command]
pub async fn player_preview_want(
    app: AppHandle,
    host: State<'_, PlayerHost>,
    settings: State<'_, SettingsStore>,
    seconds: f64,
) -> CmdResult<Option<WantReply>> {
    if bucket(seconds).is_none() {
        return Ok(None);
    }
    let Some(source) = self::source(&app, &host, &settings).await else {
        return Ok(None);
    };
    let session = app.state::<PreviewState>().session_for(&app, &source);
    let mut s = lock(&session.shared.state);
    if matches!(s.mode, Mode::Failed) {
        return Ok(None);
    }
    s.asked += 1;
    let asked = s.asked;
    let key = s.key_for(seconds);
    if let Some(w) = &s.wanted
        && w.at != seconds
    {
        s.dir = if seconds > w.at { 1.0 } else { -1.0 };
    }
    if s.wanted.as_ref().is_none_or(|w| w.key != key) {
        s.failed.clear();
    }
    s.wanted = Some(Want {
        at: seconds,
        key,
        asked,
    });
    s.last_hover = Some(Instant::now());
    let hit = key.and_then(|k| s.frames.keys.get(&k).cloned());
    if hit.is_some() {
        s.shown = asked;
    }
    let near = if hit.is_none() {
        key.or_else(|| bucket(seconds))
            .and_then(|k| s.frames.nearest(k))
            .map(|(at, url)| NearFrame {
                at: at as f64 / 1000.0,
                url: (*url).clone(),
            })
    } else {
        None
    };
    drop(s);
    session.shared.wake.notify_all();
    Ok(Some(WantReply {
        generation: session.generation,
        key: key.map(|k| k as f64 / 1000.0),
        url: hit.map(|u| (*u).clone()),
        near,
    }))
}

/// Open the playing file's previews ahead of the first hover and start the paced background fill.
#[tauri::command]
pub async fn player_preview_warm(
    app: AppHandle,
    host: State<'_, PlayerHost>,
    settings: State<'_, SettingsStore>,
) -> CmdResult<()> {
    let Some(source) = self::source(&app, &host, &settings).await else {
        return Ok(());
    };
    let session = app.state::<PreviewState>().session_for(&app, &source);
    let mut s = lock(&session.shared.state);
    if !s.warm_wanted {
        s.warm_wanted = true;
        s.fill_warm();
    }
    drop(s);
    session.shared.wake.notify_all();
    Ok(())
}

/// How much of the connection the main player can spare: [`Room::Tight`] unless a file is playing or paused (not
/// loading, seeking or stalled) with [`LOW_CACHE`] buffered ahead; [`Room::Plenty`] from [`WARM_CACHE`] on, when the
/// buffer is full, or when the player reports no buffer (a local file).
fn room(app: &AppHandle) -> Room {
    let host = app.state::<PlayerHost>();
    let steady = tauri::async_runtime::block_on(host.ask(Cmd::State))
        .is_some_and(|s| s.active && !s.buffering && matches!(s.phase, Some(Phase::Playing | Phase::Paused)));
    if !steady {
        return Room::Tight;
    }
    match tauri::async_runtime::block_on(host.ask(Cmd::CacheAhead)).flatten() {
        Some(ahead) if ahead < LOW_CACHE => Room::Tight,
        Some(ahead) if ahead < WARM_CACHE => Room::Normal,
        _ => Room::Plenty,
    }
}

/// Store a finished decode and push it to the page when it is newer than what the page shows.
#[cfg(windows)]
fn deliver(
    app: &AppHandle,
    shared: &Shared,
    generation: u64,
    (key, asked): (u64, u64),
    jpeg: Result<Vec<u8>, granary_player::PlayerError>,
    first: bool,
) {
    let url = match jpeg {
        Ok(j) => Some(data_url(&j)),
        Err(e) => {
            tracing::debug!(error = %e, key, "seek preview frame failed");
            None
        }
    };
    let show = {
        let mut s = lock(&shared.state);
        if first {
            s.decoding = false;
        }
        s.finish(key, asked, url.clone())
    };
    shared.wake.notify_all();
    if show && let Some(url) = url {
        push(app, generation, key, url);
    }
}

fn push(app: &AppHandle, generation: u64, key: u64, url: Arc<String>) {
    let frame = PushedFrame {
        generation,
        at: key as f64 / 1000.0,
        url,
    };
    if let Err(e) = app.emit(EVENT_PREVIEW, frame) {
        tracing::debug!(error = %e, "preview event not sent");
    }
}

/// The session's decoder thread: opens the source, starts the fetchers (index mode) and decodes until closed.
#[cfg(windows)]
fn run(app: &AppHandle, shared: &Arc<Shared>, source: &str, generation: u64) {
    use granary_player::thumbs::Thumbnailer;
    let mut thumb = match Thumbnailer::open(source, OPEN_TIMEOUT) {
        Ok(t) => t,
        Err(e) => {
            tracing::warn!(error = %e, "seek previews unavailable for this file");
            lock(&shared.state).fail();
            return;
        }
    };
    {
        let mut s = lock(&shared.state);
        if s.closed {
            return;
        }
        s.mode = match thumb.fetcher() {
            Some(f) => Mode::Index(f),
            None => Mode::Seek,
        };
        // A request made while the source opened gets its key now.
        let key = s.wanted.as_ref().and_then(|w| s.key_for(w.at));
        if let Some(w) = s.wanted.as_mut() {
            w.key = key;
        }
        if s.warm_wanted {
            s.fill_warm();
        }
        if let Mode::Index(f) = &s.mode {
            s.step = spot_step(f.last_keyframe().unwrap_or(0.0));
        }
        if let Mode::Index(f) = &s.mode {
            for n in 0..FETCHERS {
                let (app, shared, f) = (app.clone(), Arc::clone(shared), f.clone());
                let spawned = std::thread::Builder::new()
                    .name(format!("preview-fetch-{n}"))
                    .spawn(move || fetch_loop(&app, &shared, &f));
                if let Err(e) = spawned {
                    tracing::warn!(error = %e, "seek previews: fewer fetch threads");
                }
            }
            let (app, shared, f) = (app.clone(), Arc::clone(shared), f.clone());
            let spawned = std::thread::Builder::new()
                .name("preview-render-2".into())
                .spawn(move || second_decoder(&app, &shared, f, generation));
            if let Err(e) = spawned {
                tracing::warn!(error = %e, "seek previews: one decoder only");
            }
        }
    }
    shared.wake.notify_all();
    let indexed = thumb.indexed();
    loop {
        let job = {
            let mut s = lock(&shared.state);
            loop {
                if s.closed {
                    return;
                }
                let next = if indexed {
                    s.take_fetched().map(|f| (f.key, f.asked, Ok(f.file)))
                } else {
                    s.take_wanted().map(|(at, key, asked)| (key, asked, Err(at)))
                };
                if let Some(job) = next {
                    s.decoding = true;
                    break job;
                }
                s = wait(shared, s, Duration::from_secs(10));
            }
        };
        // The second decoder may take what else is fetched.
        shared.wake.notify_all();
        let (key, asked, work) = job;
        let jpeg = match work {
            Ok(file) => thumb.decode(file, FRAME_TIMEOUT),
            Err(at) => thumb.frame(at, FRAME_TIMEOUT),
        };
        deliver(app, shared, generation, (key, asked), jpeg, true);
    }
}

/// The second decoder (index mode): its libmpv handle is made at the first job, so files that are never swept
/// quickly do not pay for it.
#[cfg(windows)]
fn second_decoder(app: &AppHandle, shared: &Shared, fetcher: granary_player::thumbs::KeyframeFetcher, generation: u64) {
    use granary_player::thumbs::Thumbnailer;
    let mut thumb: Option<Thumbnailer> = None;
    loop {
        let job = {
            let mut s = lock(&shared.state);
            loop {
                if s.closed {
                    return;
                }
                if let Some(job) = s.take_fetched_second(Instant::now()) {
                    break job;
                }
                s = wait(shared, s, Duration::from_secs(10));
            }
        };
        if thumb.is_none() {
            match Thumbnailer::for_index(fetcher.clone()) {
                Ok(t) => thumb = Some(t),
                Err(e) => {
                    tracing::warn!(error = %e, "seek previews: one decoder only");
                    // Hand the keyframe back to the first decoder.
                    lock(&shared.state).fetched.push(job);
                    shared.wake.notify_all();
                    return;
                }
            }
        }
        let Some(t) = thumb.as_mut() else { return };
        let jpeg = t.decode(job.file, FRAME_TIMEOUT);
        deliver(app, shared, generation, (job.key, job.asked), jpeg, false);
    }
}

#[cfg(not(windows))]
fn run(_app: &AppHandle, shared: &Arc<Shared>, _source: &str, _generation: u64) {
    lock(&shared.state).fail();
}

/// A fetch thread (index mode): the wanted keyframe, else one next to it, else (when idle) the next background one.
#[cfg(windows)]
fn fetch_loop(app: &AppHandle, shared: &Shared, fetcher: &granary_player::thumbs::KeyframeFetcher) {
    loop {
        let ((at, key, asked), warm) = {
            let mut s = lock(&shared.state);
            loop {
                if s.closed {
                    return;
                }
                let now = Instant::now();
                if s.hovering(now) && s.room_at.is_none_or(|t| now.duration_since(t) >= ROOM_TTL) {
                    // Claim the reading, so the other fetch threads do not ask too.
                    s.room_at = Some(now);
                    drop(s);
                    let r = room(app);
                    s = lock(&shared.state);
                    s.room = r;
                    continue;
                }
                if let Some(job) = s.take_hover(now) {
                    break (job, false);
                }
                if let Some(job) = s.take_warm(now) {
                    s.fetching += 1;
                    break (job, true);
                }
                // Short when background work waits on a timer or the pointer waits for room; otherwise a request
                // wakes us.
                let nap = if (s.warm_wanted && !s.warm.is_empty()) || s.hovering(now) {
                    Duration::from_millis(250)
                } else {
                    Duration::from_secs(10)
                };
                s = wait(shared, s, nap);
            }
        };
        if warm && room(app) != Room::Plenty {
            // The main player needs the bandwidth: put the target back and look again later.
            let mut s = lock(&shared.state);
            s.fetching -= 1;
            s.busy.remove(&key);
            s.warm.push_front((at, key));
            s.warm_after = Instant::now() + Duration::from_millis(500);
            continue;
        }
        let fetched = fetcher.fetch(at);
        let mut s = lock(&shared.state);
        s.fetching -= 1;
        if warm {
            s.warm_after = Instant::now() + PACE;
        }
        match fetched {
            Ok((_, file)) => s.fetched.push(Fetched { asked, key, file }),
            Err(e) => {
                tracing::debug!(error = %e, at, "seek preview keyframe not fetched");
                s.busy.remove(&key);
                s.failed.insert(key);
            }
        }
        drop(s);
        shared.wake.notify_all();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[allow(clippy::unwrap_used)] // test code
    fn sweep_removes_only_other_runs_files() {
        let d = std::env::temp_dir().join(format!("granary-previews-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        for n in ["p1-0.jpg", "f1-3.mkv", "p42-1.jpg", "junk"] {
            std::fs::write(d.join(n), b"x").unwrap();
        }
        sweep_dir(&d, 42);
        let mut left: Vec<String> = std::fs::read_dir(&d)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        left.sort();
        assert_eq!(left, vec!["p42-1.jpg"]);
        sweep_dir(&d, 7);
        assert!(!d.exists(), "an empty folder is removed");
    }

    #[test]
    fn base64_matches_rfc4648_vectors() {
        for (input, want) in [
            ("", ""),
            ("f", "Zg=="),
            ("fo", "Zm8="),
            ("foo", "Zm9v"),
            ("foob", "Zm9vYg=="),
            ("fooba", "Zm9vYmE="),
            ("foobar", "Zm9vYmFy"),
        ] {
            assert_eq!(base64(input.as_bytes()), want);
        }
    }

    #[test]
    fn buckets_and_cache_bound() {
        assert_eq!(bucket(0.0), Some(0));
        assert_eq!(bucket(90.0), Some(90_000));
        assert_eq!(bucket(3.9996), Some(4000));
        assert_eq!(bucket(-1.0), None);
        assert_eq!(bucket(f64::NAN), None);
        let mut f = Frames::default();
        for i in 0..(CACHE as u64 + 10) {
            f.remember(i, Arc::new(String::new()));
        }
        assert_eq!(f.keys.len(), CACHE);
        assert!(!f.keys.contains_key(&0));
    }

    #[test]
    fn nearest_prefers_the_keyframe_before_on_ties() {
        let mut f = Frames::default();
        assert!(f.nearest(5).is_none());
        f.remember(10_000, Arc::new("a".into()));
        f.remember(20_000, Arc::new("b".into()));
        assert_eq!(f.nearest(0).map(|x| x.0), Some(10_000));
        assert_eq!(f.nearest(15_000).map(|x| x.0), Some(10_000));
        assert_eq!(f.nearest(15_001).map(|x| x.0), Some(20_000));
        assert_eq!(f.nearest(99_000).map(|x| x.0), Some(20_000));
    }

    #[test]
    fn grid_goes_coarse_to_fine_inside_the_file() {
        let g = grid(1200.0);
        assert_eq!(g.len(), LEVELS.iter().sum::<usize>());
        assert!((g[0] - 50.0).abs() < 1e-9);
        assert!(g.iter().all(|&t| t > 0.0 && t < 1200.0));
        assert!(grid(0.0).is_empty() && grid(f64::NAN).is_empty());
    }

    fn want(s: &mut Sched, at: f64) {
        s.asked += 1;
        s.wanted = Some(Want {
            at,
            key: bucket(at),
            asked: s.asked,
        });
    }

    #[test]
    fn the_newest_request_wins() {
        let mut s = Sched::new();
        s.mode = Mode::Seek;
        want(&mut s, 10.0);
        assert_eq!(s.take_wanted().map(|j| j.1), Some(10_000));
        assert!(s.take_wanted().is_none(), "busy: not fetched twice");
        // The pointer moves on twice while 10 s loads; only the spot under it is fetched next.
        want(&mut s, 20.0);
        want(&mut s, 30.0);
        assert_eq!(s.take_wanted().map(|j| (j.1, j.2)), Some((30_000, 3)));
        // Both arrive; the wanted one decodes first, and the one the pointer left is dropped (not decoded, not busy).
        s.fetched.push(Fetched {
            asked: 1,
            key: 10_000,
            file: Vec::new(),
        });
        s.fetched.push(Fetched {
            asked: 3,
            key: 30_000,
            file: Vec::new(),
        });
        let first = s.take_fetched().map(|f| f.key);
        assert_eq!(first, Some(30_000));
        assert!(
            s.finish(30_000, 3, Some(Arc::new("c".into()))),
            "the wanted frame is pushed"
        );
        assert!(s.take_fetched().is_none());
        assert!(!s.busy.contains(&10_000));
        // An older frame finishing after a newer one was shown is cached but not pushed.
        assert!(!s.finish(10_000, 1, Some(Arc::new("a".into()))));
        assert!(s.frames.keys.contains_key(&10_000));
        // A failed key is not retried until the pointer moves to another key.
        want(&mut s, 40.0);
        let _ = s.take_wanted();
        assert!(!s.finish(40_000, 4, None));
        assert!(s.take_wanted().is_none());
    }

    #[test]
    fn an_older_frame_is_pushed_while_nothing_newer_is_on_screen() {
        let mut s = Sched::new();
        s.mode = Mode::Seek;
        want(&mut s, 10.0);
        want(&mut s, 20.0);
        // The frame for the first request lands while the second still loads: the page gets it as a stand-in.
        assert!(s.finish(10_000, 1, Some(Arc::new("a".into()))));
        assert_eq!(s.shown, 1);
    }

    #[test]
    fn neighbours_are_fetched_once_the_spot_under_the_pointer_is_made() {
        let mut s = Sched::new();
        s.mode = Mode::Seek;
        s.step = 10.0;
        let now = Instant::now();
        want(&mut s, 100.0);
        s.last_hover = Some(now);
        assert!(s.take_near(now).is_none(), "the exact frame comes first");
        let _ = s.take_wanted();
        s.finish(100_000, 1, Some(Arc::new("a".into())));
        // Ahead first, then behind; never more than leave a fetch thread free for the pointer.
        assert_eq!(s.take_near(now).map(|j| (j.1, j.2)), Some((110_000, 0)));
        assert_eq!(s.take_near(now).map(|j| j.1), Some(90_000));
        assert!(s.take_near(now).is_none());
        s.finish(110_000, 0, Some(Arc::new("b".into())));
        assert_eq!(s.take_near(now).map(|j| j.1), Some(120_000));
        s.busy.clear();
        assert!(s.take_near(now + HOVER_QUIET).is_none(), "the pointer is gone");
        // Moving back turns the order around.
        s.dir = -1.0;
        s.frames.keys.clear();
        s.frames.remember(100_000, Arc::new("a".into()));
        assert_eq!(s.take_near(now).map(|j| j.1), Some(90_000));
        assert_eq!(spot_step(7200.0), 10.0);
        assert_eq!(spot_step(600.0), 2.0);
    }

    #[test]
    fn playback_short_of_buffer_gets_the_line() {
        let mut s = Sched::new();
        s.mode = Mode::Seek;
        s.step = 10.0;
        let now = Instant::now();
        s.last_hover = Some(now);
        s.room = Room::Tight;
        want(&mut s, 100.0);
        assert_eq!(s.take_hover(now).map(|j| j.1), Some(100_000));
        // The pointer moves on while that download runs: the next one waits for it.
        want(&mut s, 200.0);
        assert!(s.take_hover(now).is_none());
        s.fetching -= 1;
        assert_eq!(s.take_hover(now).map(|j| j.1), Some(200_000));
        s.fetching -= 1;
        s.finish(200_000, 2, Some(Arc::new("a".into())));
        assert!(s.take_hover(now).is_none(), "nothing is fetched ahead");
        // With buffer to spare the neighbours follow.
        s.room = Room::Normal;
        assert_eq!(s.take_hover(now).map(|j| j.1), Some(210_000));
        assert_eq!(s.fetching, 1);
    }

    #[test]
    fn the_second_decoder_only_helps_a_busy_first_one_during_a_hover() {
        let mut s = Sched::new();
        s.mode = Mode::Seek;
        let now = Instant::now();
        s.fetched.push(Fetched {
            asked: 0,
            key: 1,
            file: Vec::new(),
        });
        s.last_hover = Some(now);
        assert!(s.take_fetched_second(now).is_none(), "the first decoder is free");
        s.decoding = true;
        assert!(s.take_fetched_second(now + HOVER_QUIET).is_none(), "nobody hovers");
        assert_eq!(s.take_fetched_second(now).map(|f| f.key), Some(1));
    }

    #[test]
    fn the_background_fill_waits_for_quiet() {
        let mut s = Sched::new();
        s.mode = Mode::Seek;
        let now = Instant::now();
        s.warm.push_back((5.0, 5_000));
        assert!(s.take_warm(now).is_none(), "not asked for");
        s.warm_wanted = true;
        assert!(s.take_warm(now).is_none(), "start delay");
        s.warm_after = now;
        s.last_hover = Some(now);
        assert!(s.take_warm(now).is_none(), "someone hovers");
        let later = now + HOVER_QUIET;
        assert_eq!(s.take_warm(later).map(|j| (j.1, j.2)), Some((5_000, 0)));
        // Background frames fill the cache but are never pushed.
        assert!(!s.finish(5_000, 0, Some(Arc::new("w".into()))));
        assert!(s.frames.keys.contains_key(&5_000));
    }
}
