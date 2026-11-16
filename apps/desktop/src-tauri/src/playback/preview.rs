//! Seek-bar previews for direct-played files without server trickplay (ADR-0009, revisions 5 to 8). One session per
//! source, opened in the background on the first request, so asking never blocks:
//! - the page says which time the pointer is over ([`player_preview_want`]) and numbers its requests; the newest
//!   wins, also when an older one arrives after it;
//! - Matroska files are read through their keyframe index, one range request per keyframe that ends as soon as the
//!   keyframe is complete. The server is asked for little ([`Sched::take_job`]): at most [`SLOTS`] downloads in the
//!   air, the keyframe under the pointer first and at once, nothing for spots the pointer only passes, a second
//!   request when the first is not answered, and downloads the pointer has left are given up when their slot is
//!   needed;
//! - one thread decodes, the wanted keyframe first, else the newest fetched, and pushes each frame newer than the one
//!   on screen to the page ([`EVENT_PREVIEW`]); the page shows the latest it got until the exact one arrives; a second
//!   decoder joins only while the pointer is on the bar and the first is busy; HEVC keyframes of several slices are
//!   decoded slice by slice on the source's strip decoders (`granary_player::hevc`);
//! - while nobody hovers and the main player is healthy, a paced background fill decodes a grid, coarse to fine, so
//!   the bar has a frame close to any spot at once;
//! - playback comes first ([`Room`]): while the main player is short of buffered video, hovering fetches one keyframe
//!   at a time and nothing ahead, and the background fill waits.
//!
//! Other files seek in a stream opened in the preview handle, newest request first. Frames are kept in memory only.
//! Newest-request-wins and the parallel fetch follow AIOStreams' thumbnailer (`thumbs/src/session.rs@87dee2c` of
//! AIOStreams/media); decoding stays in a second libmpv handle because our FFmpeg is linked into libmpv (ADR-0009).
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
/// Fetch threads. One more than [`SLOTS`]: the thread of a download given up needs a moment to come back.
const FETCHERS: usize = 3;
/// Downloads in the air at once. A remote link answers one request for a far-off part of the file in well under a
/// second when it is left alone and takes several seconds when a handful arrive together (measured through a
/// server that reads from remote storage: 0.02 to 0.7 s alone, 2 to 5 s with three others, and after a burst of
/// ten in a second nothing for 20 s). Two is the pointer's download and its second request, or one the pointer
/// just left.
const SLOTS: usize = 2;
/// With a download in the air, the pointer has to stay on a keyframe this long before its download starts: a
/// sweep along the bar shows the frames already made and asks the server for nothing it will not use.
const SETTLE: Duration = Duration::from_millis(120);
/// At most this many downloads for the pointer are started within [`BURST_WINDOW`], however the pointer moves: a
/// download given up still keeps the server busy until it would have answered.
const BURST: usize = 4;
const BURST_WINDOW: Duration = Duration::from_secs(2);
/// A download the server has not started to answer after this long is given up and asked for again.
const PATIENCE: Duration = Duration::from_secs(8);
/// Background grid: frames per level, coarse to fine. The total bounds the extra traffic per file (one keyframe each,
/// typically 0.1–1.5 MB on 4K remuxes). The last level puts a frame within about half a percent of the file of any
/// spot, so on a link that takes seconds per request a hover still has something close to show at once.
const LEVELS: [usize; 3] = [12, 48, 144];
/// A download for the pointer that the server has not started to answer after this long gets a second request
/// beside it; the first to arrive is used. Answer times on a remote link vary from a tenth of a second to several
/// seconds for the same kind of request.
const HEDGE: Duration = Duration::from_millis(1200);
/// A download of a spot the pointer has left may be given up for the pointer's while the server has not answered
/// it, or when it has been receiving for longer than this.
const STALLED: Duration = Duration::from_secs(3);
/// A keyframe under the pointer that failed is tried again after this long, this many times in all.
const RETRY_FRAME: Duration = Duration::from_millis(600);
const TRIES: u32 = 3;
/// With the pointer resting on a spot whose frame is not made yet, the hover counts as going on for this long.
const HOVER_WAIT: Duration = Duration::from_secs(30);
/// Fetched keyframes of spots the pointer has left that wait for the decoder; older ones are dropped.
const LEFT_KEPT: usize = 6;
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
/// Neighbours in flight at once.
const NEAR_AT_ONCE: usize = 1;
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
    /// Since when the pointer has been on this keyframe.
    since: Instant,
}

struct Fetched {
    /// The request it answers; 0 for the background fill.
    asked: u64,
    key: u64,
    /// The keyframe block as fetched.
    block: Vec<u8>,
}

/// What a download is for.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Kind {
    /// The keyframe under the pointer (possibly a second request for it).
    Hover,
    /// A neighbour of it.
    Near,
    /// The background grid.
    Warm,
}

/// A keyframe download in progress.
struct Flight {
    id: u64,
    at: f64,
    key: u64,
    asked: u64,
    kind: Kind,
    started: Instant,
    cancel: Arc<Cancel>,
}

/// The read handle of a download. Without libmpv (other platforms) there are no downloads.
#[cfg(windows)]
type Cancel = granary_player::thumbs::Cancel;
#[cfg(not(windows))]
#[derive(Default)]
struct Cancel;
#[cfg(not(windows))]
impl Cancel {
    fn cancel(&self) {}
    fn answered(&self) -> bool {
        false
    }
}

/// A download handed to a fetch thread.
struct Job {
    id: u64,
    at: f64,
    key: u64,
    kind: Kind,
    cancel: Arc<Cancel>,
}

struct Failure {
    tries: u32,
    at: Instant,
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
    failed: std::collections::HashMap<u64, Failure>,
    /// Downloads in progress. One given up is removed at once; its thread returns within moments.
    flights: Vec<Flight>,
    flight_ids: u64,
    /// When the last downloads for the pointer were started ([`BURST`]).
    starts: VecDeque<Instant>,
    /// The newest request number the page sent. Requests are answered on several threads and can arrive out of
    /// order; an older one must not move the pointer back.
    seq: u64,
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
    /// The first decoder is in a decode.
    decoding: bool,
    /// When the source failed to open.
    failed_at: Option<Instant>,
    /// What the last keyframe download and the last decode cost (shown in the stats overlay).
    fetch_cost: Option<(Duration, usize)>,
    decode_cost: Option<Duration>,
    closed: bool,
}

impl Sched {
    fn new() -> Self {
        Self {
            mode: Mode::Opening,
            asked: 0,
            wanted: None,
            busy: HashSet::new(),
            failed: std::collections::HashMap::new(),
            flights: Vec::new(),
            flight_ids: 0,
            starts: VecDeque::new(),
            seq: 0,
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
            decoding: false,
            failed_at: None,
            fetch_cost: None,
            decode_cost: None,
            closed: false,
        }
    }

    fn fail(&mut self) {
        self.mode = Mode::Failed;
        self.failed_at = Some(Instant::now());
    }

    /// The pointer is on the bar: it asked lately, or it rests on a spot whose frame is still being made.
    fn hovering(&self, now: Instant) -> bool {
        let Some(t) = self.last_hover else { return false };
        let since = now.duration_since(t);
        since < HOVER_QUIET || (since < HOVER_WAIT && self.wanted_open())
    }

    /// The frame under the pointer is not made yet.
    fn wanted_open(&self) -> bool {
        self.wanted
            .as_ref()
            .and_then(|w| w.key)
            .is_some_and(|k| !self.frames.keys.contains_key(&k))
    }

    /// The pointer is over `seconds`. Returns the keyframe it shows and whether this is the newest request (an
    /// older one that arrives late changes nothing).
    fn want(&mut self, seconds: f64, seq: Option<u64>, now: Instant) -> (Option<u64>, bool) {
        let key = self.key_for(seconds);
        if seq.is_some_and(|q| q < self.seq) {
            return (key, false);
        }
        self.seq = seq.unwrap_or(self.seq);
        self.asked += 1;
        let mut since = now;
        if let Some(w) = &self.wanted {
            if w.at != seconds {
                self.dir = if seconds > w.at { 1.0 } else { -1.0 };
            }
            if w.key == key {
                since = w.since;
            } else {
                self.failed.clear();
            }
        }
        self.wanted = Some(Want {
            at: seconds,
            key,
            asked: self.asked,
            since,
        });
        self.last_hover = Some(now);
        self.clear_warm_flights();
        if key.is_some_and(|k| self.frames.keys.contains_key(&k)) {
            self.shown = self.asked;
        }
        (key, true)
    }

    /// Another download for the pointer may start now ([`BURST`]).
    fn may_start(&mut self, now: Instant) -> bool {
        while self
            .starts
            .front()
            .is_some_and(|t| now.duration_since(*t) >= BURST_WINDOW)
        {
            self.starts.pop_front();
        }
        self.starts.len() < BURST
    }

    /// Start a download for the pointer or next to it.
    fn start(&mut self, job: (f64, u64, u64), kind: Kind, now: Instant) -> Job {
        self.starts.push_back(now);
        self.fly(job, kind, now)
    }

    fn fly(&mut self, (at, key, asked): (f64, u64, u64), kind: Kind, now: Instant) -> Job {
        self.flight_ids += 1;
        let cancel = Arc::new(Cancel::default());
        self.flights.push(Flight {
            id: self.flight_ids,
            at,
            key,
            asked,
            kind,
            started: now,
            cancel: Arc::clone(&cancel),
        });
        Job {
            id: self.flight_ids,
            at,
            key,
            kind,
            cancel,
        }
    }

    /// Give up the download at `i`: its key is open again and a background target goes back to its queue.
    fn give_up(&mut self, i: usize) {
        let f = self.flights.swap_remove(i);
        f.cancel.cancel();
        if !self.flights.iter().any(|o| o.key == f.key) {
            self.busy.remove(&f.key);
        }
        if f.kind == Kind::Warm {
            self.warm.push_front((f.at, f.key));
        }
    }

    /// Give up the download with this id, when it is still going.
    fn give_up_id(&mut self, id: u64) {
        if let Some(i) = self.flights.iter().position(|f| f.id == id) {
            self.give_up(i);
        }
    }

    /// The pointer asked: background downloads the server has not answered yet make way.
    fn clear_warm_flights(&mut self) {
        while let Some(i) = self
            .flights
            .iter()
            .position(|f| f.kind == Kind::Warm && !f.cancel.answered())
        {
            self.give_up(i);
        }
    }

    /// The download to give up for the keyframe under the pointer: one for another keyframe that the server has
    /// not answered or that has been receiving for long; background first, then neighbours, the oldest first.
    fn victim(&self, key: u64, now: Instant) -> Option<usize> {
        self.flights
            .iter()
            .enumerate()
            .filter(|(_, f)| f.key != key && (!f.cancel.answered() || now.duration_since(f.started) >= STALLED))
            .min_by_key(|(_, f)| {
                let rank = match f.kind {
                    Kind::Warm => 0,
                    Kind::Near => 1,
                    Kind::Hover => 2,
                };
                (rank, f.started)
            })
            .map(|(i, _)| i)
    }

    /// The next download for a fetch thread, at most [`SLOTS`] in the air:
    /// 1. the keyframe under the pointer: at once when nothing is in the air, else once the pointer has stayed on
    ///    it for [`SETTLE`]; when the slots are taken, a download the pointer has left is given up for it;
    /// 2. a second request for it when the server has not answered the first after [`HEDGE`];
    /// 3. a keyframe next to it;
    /// 4. the background grid.
    ///
    /// While the main player is short of buffer and the pointer is on the bar: one download at a time, only the
    /// first kind.
    fn take_job(&mut self, now: Instant) -> Option<Job> {
        while let Some(i) = self
            .flights
            .iter()
            .position(|f| !f.cancel.answered() && now.duration_since(f.started) >= PATIENCE)
        {
            self.give_up(i);
        }
        let tight = self.room == Room::Tight && self.hovering(now);
        let slots = if tight { 1 } else { SLOTS };
        let wanted = self
            .wanted
            .as_ref()
            .and_then(|w| w.key.map(|k| ((w.at, k, w.asked), w.since)));
        if let Some((w, since)) = wanted
            && self.open_for(w.1, now, true)
        {
            let settled = self.flights.is_empty() || now.duration_since(since) >= SETTLE;
            if !settled || !self.may_start(now) {
                return None;
            }
            if self.flights.len() >= slots {
                let victim = self.victim(w.1, now)?;
                self.give_up(victim);
            }
            self.busy.insert(w.1);
            return Some(self.start(w, Kind::Hover, now));
        }
        if tight || self.flights.len() >= slots {
            return None;
        }
        if let Some((w, _)) = wanted {
            let mut mine = self.flights.iter().filter(|f| f.key == w.1);
            if let (Some(f), None) = (mine.next(), mine.next())
                && !f.cancel.answered()
                && now.duration_since(f.started) >= HEDGE
                && self.may_start(now)
            {
                return Some(self.start(w, Kind::Hover, now));
            }
        }
        if self.may_start(now)
            && let Some((at, key)) = self.take_near(now)
        {
            return Some(self.start((at, key, 0), Kind::Near, now));
        }
        if let Some((at, key)) = self.take_warm(now) {
            return Some(self.fly((at, key, 0), Kind::Warm, now));
        }
        None
    }

    /// A download ended, with the keyframe block or without. The block is queued for the decoders and a second
    /// request for the same keyframe is given up; a failure counts only when no second request is still going.
    fn land(&mut self, id: u64, block: Option<Vec<u8>>, now: Instant) {
        // Given up meanwhile: whatever it brought is not wanted.
        let Some(i) = self.flights.iter().position(|f| f.id == id) else {
            return;
        };
        let f = self.flights.swap_remove(i);
        match block {
            Some(block) => {
                self.flights.retain(|o| {
                    if o.key == f.key {
                        o.cancel.cancel();
                    }
                    o.key != f.key
                });
                self.fetched.push(Fetched {
                    asked: f.asked,
                    key: f.key,
                    block,
                });
            }
            None if self.flights.iter().any(|o| o.key == f.key) => {}
            None => {
                self.busy.remove(&f.key);
                self.fail_key(f.key, now);
            }
        }
    }

    fn fail_key(&mut self, key: u64, now: Instant) {
        let e = self.failed.entry(key).or_insert(Failure { tries: 0, at: now });
        e.tries += 1;
        e.at = now;
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

    /// `key` may be worked on: not made, not in work, and not failed. With `retry` (the keyframe under the
    /// pointer) a failed one is open again after [`RETRY_FRAME`], [`TRIES`] times in all.
    fn open_for(&self, key: u64, now: Instant, retry: bool) -> bool {
        !self.frames.keys.contains_key(&key)
            && !self.busy.contains(&key)
            && self
                .failed
                .get(&key)
                .is_none_or(|f| retry && f.tries < TRIES && now.duration_since(f.at) >= RETRY_FRAME)
    }

    /// The wanted key when it still needs decoding (seek mode); marks it busy.
    fn take_wanted(&mut self, now: Instant) -> Option<(f64, u64, u64)> {
        let w = self.wanted.as_ref()?;
        let key = w.key?;
        if !self.open_for(key, now, true) {
            return None;
        }
        let job = (w.at, key, w.asked);
        self.busy.insert(key);
        Some(job)
    }

    /// A keyframe next to the one under the pointer, once that one is made and while the pointer is still around:
    /// the spots a small move lands on are then answered from the cache. Ahead in the direction of travel first.
    /// Marks it busy.
    fn take_near(&mut self, now: Instant) -> Option<(f64, u64)> {
        let w = self.wanted.as_ref()?;
        let (at, key) = (w.at, w.key?);
        if self.step <= 0.0
            || !self.frames.keys.contains_key(&key)
            || self.flights.iter().filter(|f| f.kind == Kind::Near).count() >= NEAR_AT_ONCE
            || !self.hovering(now)
        {
            return None;
        }
        for n in NEAR {
            let t = at + n * self.dir * self.step;
            if let Some(k) = self.key_for(t)
                && k != key
                && self.open_for(k, now, false)
            {
                self.busy.insert(k);
                return Some((t, k));
            }
        }
        None
    }

    /// The next background target, when nobody hovers and nothing else is in flight; marks it busy.
    fn take_warm(&mut self, now: Instant) -> Option<(f64, u64)> {
        if !self.warm_wanted
            || now < self.warm_after
            || self.hovering(now)
            || !self.busy.is_empty()
            || !self.fetched.is_empty()
        {
            return None;
        }
        while let Some((at, key)) = self.warm.pop_front() {
            if self.open_for(key, now, false) {
                self.busy.insert(key);
                return Some((at, key));
            }
        }
        None
    }

    /// The fetched keyframe to decode next: the wanted one, else the newest request. Keyframes of spots the pointer
    /// has left are decoded too when the decoders get to them (their download was the expensive part, and the
    /// pointer often comes back), the newest [`LEFT_KEPT`] of them.
    fn take_fetched(&mut self) -> Option<Fetched> {
        let wanted = self.wanted.as_ref().and_then(|w| w.key);
        let pick = (0..self.fetched.len())
            .find(|&i| Some(self.fetched[i].key) == wanted)
            .or_else(|| (0..self.fetched.len()).max_by_key(|&i| self.fetched[i].asked))?;
        let next = self.fetched.swap_remove(pick);
        self.fetched.sort_by_key(|f| std::cmp::Reverse(f.asked));
        let (mut left, mut dropped) = (0, Vec::new());
        self.fetched.retain(|f| {
            if f.asked == 0 || Some(f.key) == wanted {
                return true;
            }
            left += 1;
            if left > LEFT_KEPT {
                dropped.push(f.key);
            }
            left <= LEFT_KEPT
        });
        for key in dropped {
            self.busy.remove(&key);
        }
        Some(next)
    }

    /// Store a decoded frame; whether the page should get it (newer than what it shows, or the wanted one).
    fn finish(&mut self, key: u64, asked: u64, url: Option<Arc<String>>) -> bool {
        self.busy.remove(&key);
        let Some(url) = url else {
            self.fail_key(key, Instant::now());
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

    /// Rows for the stats overlay: how previews are made for the playing file and what the last one cost, so a
    /// slow link and a slow decode can be told apart.
    pub fn summary(&self) -> Vec<(&'static str, String)> {
        let Some(session) = lock(&self.session).clone() else {
            return Vec::new();
        };
        let s = lock(&session.shared.state);
        let how = match &s.mode {
            Mode::Opening => "opening",
            #[cfg(windows)]
            Mode::Index(f) if f.split() == Some(true) => "keyframe index, slice by slice",
            #[cfg(windows)]
            Mode::Index(_) => "keyframe index",
            Mode::Seek => "stream seeks",
            Mode::Failed => "unavailable",
        };
        let mut rows = vec![("preview", format!("{how}, {} frames kept", s.frames.keys.len()))];
        if let Some((took, bytes)) = s.fetch_cost {
            rows.push((
                "preview-fetch",
                format!("{} ms, {:.2} MB", took.as_millis(), bytes as f64 / 1e6),
            ));
        }
        if let Some(took) = s.decode_cost {
            rows.push(("preview-decode", format!("{} ms", took.as_millis())));
        }
        rows
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
/// previews are off or nothing previewable is playing (transcode, stopped). `seq` rises with every request of the
/// page: requests are served on several threads, and one that arrives after a newer one only reads the cache.
#[tauri::command]
pub async fn player_preview_want(
    app: AppHandle,
    host: State<'_, PlayerHost>,
    settings: State<'_, SettingsStore>,
    seconds: f64,
    seq: Option<u64>,
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
    let (key, _newest) = s.want(seconds, seq, Instant::now());
    let hit = key.and_then(|k| s.frames.keys.get(&k).cloned());
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
    took: Duration,
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
        if url.is_some() {
            s.decode_cost = Some(took);
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
                    s.take_fetched().map(|f| (f.key, f.asked, Ok(f.block)))
                } else {
                    s.take_wanted(Instant::now())
                        .map(|(at, key, asked)| (key, asked, Err(at)))
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
        let started = Instant::now();
        let jpeg = match work {
            Ok(file) => thumb.decode(file, FRAME_TIMEOUT),
            Err(at) => thumb.frame(at, FRAME_TIMEOUT),
        };
        let took = started.elapsed();
        tracing::debug!(key, ms = took.as_millis() as u64, "seek preview decoded");
        deliver(app, shared, generation, (key, asked), jpeg, took, true);
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
        let started = Instant::now();
        let jpeg = t.decode(job.block, FRAME_TIMEOUT);
        let took = started.elapsed();
        tracing::debug!(
            key = job.key,
            ms = took.as_millis() as u64,
            "seek preview decoded (second decoder)"
        );
        deliver(app, shared, generation, (job.key, job.asked), jpeg, took, false);
    }
}

#[cfg(not(windows))]
fn run(_app: &AppHandle, shared: &Arc<Shared>, _source: &str, _generation: u64) {
    lock(&shared.state).fail();
}

/// A fetch thread (index mode): takes downloads from [`Sched::take_job`] until the session closes.
#[cfg(windows)]
fn fetch_loop(app: &AppHandle, shared: &Shared, fetcher: &granary_player::thumbs::KeyframeFetcher) {
    loop {
        let job = {
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
                if let Some(job) = s.take_job(now) {
                    break job;
                }
                // Short while the pointer waits (a second request may be due) or background work waits on a timer;
                // otherwise a request wakes us.
                let nap = if s.hovering(now) {
                    Duration::from_millis(150)
                } else if s.warm_wanted && !s.warm.is_empty() {
                    Duration::from_millis(250)
                } else {
                    Duration::from_secs(10)
                };
                s = wait(shared, s, nap);
            }
        };
        let warm = job.kind == Kind::Warm;
        if warm && room(app) != Room::Plenty {
            // The main player needs the bandwidth: put the target back and look again later.
            let mut s = lock(&shared.state);
            s.give_up_id(job.id);
            s.warm_after = Instant::now() + Duration::from_millis(500);
            continue;
        }
        let started = Instant::now();
        let fetched = fetcher.fetch_with(job.at, Some(&job.cancel));
        let took = started.elapsed();
        let mut s = lock(&shared.state);
        let now = Instant::now();
        match fetched {
            Ok((_, block)) => {
                tracing::debug!(
                    key = job.key,
                    kind = ?job.kind,
                    ms = took.as_millis() as u64,
                    answer_ms = job.cancel.answer_time().map_or(0, |t| t.as_millis() as u64),
                    bytes = block.len(),
                    others = s.flights.len().saturating_sub(1),
                    given_up = job.cancel.is_cancelled(),
                    "seek preview keyframe fetched"
                );
                s.fetch_cost = Some((took, block.len()));
                s.land(job.id, Some(block), now);
            }
            Err(e) => {
                tracing::debug!(
                    error = %e,
                    key = job.key,
                    kind = ?job.kind,
                    ms = took.as_millis() as u64,
                    given_up = job.cancel.is_cancelled(),
                    "seek preview keyframe not fetched"
                );
                s.land(job.id, None, now);
            }
        }
        if warm {
            s.warm_after = now + PACE;
        }
        drop(s);
        shared.wake.notify_all();
    }
}

#[cfg(test)]
#[allow(clippy::expect_used)] // test code
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

    /// The pointer has been on `at` for a while (see [`SETTLE`]).
    fn want(s: &mut Sched, at: f64) {
        let now = Instant::now();
        s.asked += 1;
        s.wanted = Some(Want {
            at,
            key: bucket(at),
            asked: s.asked,
            since: now.checked_sub(SETTLE * 2).unwrap_or(now),
        });
    }

    fn url(s: &str) -> Option<Arc<String>> {
        Some(Arc::new(s.into()))
    }

    /// The download `job` arrived and its frame was decoded.
    fn made(s: &mut Sched, job: &Job, now: Instant) {
        s.land(job.id, Some(Vec::new()), now);
        let f = s.take_fetched().expect("fetched");
        assert_eq!(f.key, job.key);
        s.finish(f.key, f.asked, url("x"));
    }

    #[test]
    fn the_newest_request_wins() {
        let mut s = Sched::new();
        s.mode = Mode::Seek;
        let now = Instant::now();
        want(&mut s, 10.0);
        assert_eq!(s.take_wanted(now).map(|j| j.1), Some(10_000));
        assert!(s.take_wanted(now).is_none(), "busy: not fetched twice");
        // The pointer moves on twice while 10 s loads; only the spot under it is fetched next.
        want(&mut s, 20.0);
        want(&mut s, 30.0);
        assert_eq!(s.take_wanted(now).map(|j| (j.1, j.2)), Some((30_000, 3)));
        // Both arrive; the wanted one decodes first, the one the pointer left after it.
        s.fetched.push(Fetched {
            asked: 1,
            key: 10_000,
            block: Vec::new(),
        });
        s.fetched.push(Fetched {
            asked: 3,
            key: 30_000,
            block: Vec::new(),
        });
        assert_eq!(s.take_fetched().map(|f| f.key), Some(30_000));
        assert!(s.finish(30_000, 3, url("c")), "the wanted frame is pushed");
        assert_eq!(s.take_fetched().map(|f| f.key), Some(10_000));
        // An older frame finishing after a newer one was shown is cached but not pushed.
        assert!(!s.finish(10_000, 1, url("a")));
        assert!(s.frames.keys.contains_key(&10_000));
        // A failed keyframe under the pointer is tried again after a pause, a few times in all.
        want(&mut s, 40.0);
        let _ = s.take_wanted(Instant::now());
        assert!(!s.finish(40_000, 4, None));
        assert!(s.take_wanted(Instant::now()).is_none(), "not at once");
        for _ in 1..TRIES {
            assert!(s.take_wanted(Instant::now() + RETRY_FRAME).is_some());
            s.finish(40_000, 4, None);
        }
        assert!(s.take_wanted(Instant::now() + RETRY_FRAME).is_none(), "given up");
    }

    #[test]
    fn only_so_many_left_keyframes_wait_for_the_decoder() {
        let mut s = Sched::new();
        s.mode = Mode::Seek;
        for i in 1..=(LEFT_KEPT as u64 + 3) {
            s.busy.insert(i);
            s.fetched.push(Fetched {
                asked: i,
                key: i,
                block: Vec::new(),
            });
        }
        // The newest is decoded; of the rest the newest few are kept, the oldest two are open again.
        assert_eq!(s.take_fetched().map(|f| f.key), Some(LEFT_KEPT as u64 + 3));
        assert_eq!(s.fetched.len(), LEFT_KEPT);
        assert!(!s.busy.contains(&1) && !s.busy.contains(&2) && s.busy.contains(&3));
    }

    #[test]
    fn an_older_frame_is_pushed_while_nothing_newer_is_on_screen() {
        let mut s = Sched::new();
        s.mode = Mode::Seek;
        want(&mut s, 10.0);
        want(&mut s, 20.0);
        // The frame for the first request lands while the second still loads: the page gets it as a stand-in.
        assert!(s.finish(10_000, 1, url("a")));
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
        let exact = s.take_job(now).expect("the exact frame comes first");
        assert_eq!((exact.key, exact.kind), (100_000, Kind::Hover));
        assert!(s.take_job(now).is_none(), "nothing beside it yet");
        made(&mut s, &exact, now);
        // Ahead first, then behind, one at a time.
        let ahead = s.take_job(now).expect("ahead");
        assert_eq!((ahead.key, ahead.kind), (110_000, Kind::Near));
        assert!(s.take_job(now).is_none());
        made(&mut s, &ahead, now);
        let behind = s.take_job(now).expect("behind");
        assert_eq!(behind.key, 90_000);
        made(&mut s, &behind, now);
        assert_eq!(s.take_job(now).map(|j| j.key), Some(120_000));
        assert!(s.take_job(now + HOVER_QUIET).is_none(), "the pointer is gone");
        // Moving back turns the order around.
        let mut s = Sched::new();
        s.mode = Mode::Seek;
        s.step = 10.0;
        s.dir = -1.0;
        want(&mut s, 100.0);
        s.last_hover = Some(now);
        s.frames.remember(100_000, Arc::new("a".into()));
        assert_eq!(s.take_job(now).map(|j| j.key), Some(90_000));
        assert_eq!(spot_step(7200.0), 10.0);
        assert_eq!(spot_step(600.0), 2.0);
    }

    #[test]
    fn the_pointer_never_waits_for_downloads_it_has_left() {
        let mut s = Sched::new();
        s.mode = Mode::Seek;
        let now = Instant::now();
        s.last_hover = Some(now);
        // The pointer paused here and there: every slot has a download.
        let mut jobs = Vec::new();
        for i in 1..=SLOTS {
            want(&mut s, i as f64 * 10.0);
            jobs.push(s.take_job(now).expect("a free thread"));
        }
        // The pointer stops somewhere else: the oldest of them is given up for it.
        want(&mut s, 500.0);
        let job = s.take_job(now).expect("the pointer's download starts at once");
        assert_eq!(job.key, 500_000);
        assert_eq!(s.flights.len(), SLOTS);
        assert!(!s.flights.iter().any(|f| f.key == 10_000) && !s.busy.contains(&10_000));
        // What the given-up download still brings is not used, and it is no failure.
        s.land(jobs[0].id, Some(Vec::new()), now);
        s.land(jobs[0].id, None, now);
        assert!(s.fetched.is_empty() && s.failed.is_empty());
    }

    #[test]
    fn a_late_request_does_not_move_the_pointer_back() {
        let mut s = Sched::new();
        s.mode = Mode::Seek;
        let now = Instant::now();
        // The page asked for 10 s, 20 s, 30 s; the request for 20 s arrives last.
        assert_eq!(s.want(10.0, Some(1), now), (Some(10_000), true));
        assert_eq!(s.want(30.0, Some(3), now), (Some(30_000), true));
        assert_eq!(s.want(20.0, Some(2), now), (Some(20_000), false));
        assert_eq!(s.wanted.as_ref().and_then(|w| w.key), Some(30_000));
        assert_eq!(s.take_job(now).map(|j| j.key), Some(30_000));
        // Without a number (older pages) every request counts.
        assert!(s.want(40.0, None, now).1);
    }

    #[test]
    fn a_sweep_asks_the_server_for_little() {
        let mut s = Sched::new();
        s.mode = Mode::Seek;
        let t0 = Instant::now();
        // The first spot starts at once; the spots the pointer only passes do not.
        s.want(10.0, Some(1), t0);
        assert_eq!(s.take_job(t0).map(|j| j.key), Some(10_000));
        for i in 2..10u64 {
            let t = t0 + Duration::from_millis(30 * i);
            s.want(i as f64 * 10.0, Some(i), t);
            assert!(s.take_job(t).is_none(), "passing by");
        }
        // It stops: after a moment on the spot its download starts.
        let stop = t0 + Duration::from_millis(300);
        s.want(500.0, Some(10), stop);
        assert!(s.take_job(stop + SETTLE / 2).is_none());
        // Moving within the same keyframe does not start the wait again.
        s.want(500.0, Some(11), stop + SETTLE / 2);
        assert_eq!(s.take_job(stop + SETTLE).map(|j| j.key), Some(500_000));
        assert_eq!(s.flights.len(), 2);
        // However it moves, only so many downloads start in a short time.
        for i in 0..10u64 {
            let t = stop + SETTLE * (2 + i as u32);
            s.want(1000.0 + i as f64 * 10.0, Some(20 + i), t);
            let _ = s.take_job(t + SETTLE);
        }
        assert_eq!(s.starts.len(), BURST);
        // Later it may again.
        let later = stop + BURST_WINDOW * 2;
        s.want(2000.0, Some(40), later);
        assert_eq!(s.take_job(later + SETTLE).map(|j| j.key), Some(2_000_000));
    }

    #[test]
    fn a_download_the_server_never_answers_is_asked_for_again() {
        let mut s = Sched::new();
        s.mode = Mode::Seek;
        let now = Instant::now();
        s.last_hover = Some(now);
        want(&mut s, 100.0);
        let first = s.take_job(now).expect("first");
        let second = s.take_job(now + HEDGE).expect("second request");
        // Neither is answered: both are given up and the keyframe is asked for anew.
        let third = s.take_job(now + HEDGE + PATIENCE).expect("asked again");
        assert_eq!(third.key, 100_000);
        assert!(third.id != first.id && third.id != second.id);
        assert_eq!(s.flights.len(), 1);
    }

    #[test]
    fn a_slow_answer_gets_a_second_request() {
        let mut s = Sched::new();
        s.mode = Mode::Seek;
        let now = Instant::now();
        s.last_hover = Some(now);
        want(&mut s, 100.0);
        let first = s.take_job(now).expect("first");
        assert!(s.take_job(now + HEDGE / 2).is_none(), "not yet");
        let second = s.take_job(now + HEDGE).expect("second request");
        assert_eq!((second.key, second.kind), (100_000, Kind::Hover));
        assert!(s.take_job(now + HEDGE * 3).is_none(), "two at most");
        // One of them failing is no failure while the other is going.
        s.land(first.id, None, now);
        assert!(s.failed.is_empty() && s.busy.contains(&100_000));
        s.land(second.id, Some(Vec::new()), now);
        assert_eq!(s.fetched.len(), 1);
        assert!(s.flights.is_empty());
        // The first to arrive ends the other.
        want(&mut s, 200.0);
        let first = s.take_job(now).expect("first");
        let second = s.take_job(now + HEDGE).expect("second");
        s.land(second.id, Some(Vec::new()), now);
        assert!(s.flights.is_empty());
        s.land(first.id, Some(Vec::new()), now);
        assert_eq!(s.fetched.len(), 2, "the late twin is dropped");
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
        let first = s.take_job(now).expect("one download");
        assert!(s.take_job(now + HEDGE).is_none(), "no second request");
        // The pointer moves on while the server has not answered: that download makes way, one stays in flight.
        want(&mut s, 200.0);
        let second = s.take_job(now).expect("the pointer's download");
        assert_eq!(second.key, 200_000);
        assert_eq!(s.flights.len(), 1);
        s.land(first.id, Some(Vec::new()), now);
        made(&mut s, &second, now);
        assert!(s.take_job(now).is_none(), "nothing is fetched ahead");
        // With buffer to spare the neighbours follow.
        s.room = Room::Normal;
        assert_eq!(s.take_job(now).map(|j| (j.key, j.kind)), Some((210_000, Kind::Near)));
    }

    #[test]
    fn the_second_decoder_only_helps_a_busy_first_one_during_a_hover() {
        let mut s = Sched::new();
        s.mode = Mode::Seek;
        let now = Instant::now();
        s.fetched.push(Fetched {
            asked: 0,
            key: 1,
            block: Vec::new(),
        });
        s.last_hover = Some(now);
        assert!(s.take_fetched_second(now).is_none(), "the first decoder is free");
        s.decoding = true;
        assert!(s.take_fetched_second(now + HOVER_QUIET).is_none(), "nobody hovers");
        assert_eq!(s.take_fetched_second(now).map(|f| f.key), Some(1));
    }

    #[test]
    fn a_pointer_resting_on_an_unmade_spot_still_hovers() {
        let mut s = Sched::new();
        s.mode = Mode::Seek;
        let now = Instant::now();
        s.last_hover = Some(now);
        want(&mut s, 100.0);
        assert!(s.hovering(now + HOVER_QUIET * 2), "its frame is still being made");
        assert!(!s.hovering(now + HOVER_WAIT));
        s.frames.remember(100_000, Arc::new("a".into()));
        assert!(!s.hovering(now + HOVER_QUIET * 2));
    }

    #[test]
    fn the_background_fill_waits_for_quiet_and_makes_way() {
        let mut s = Sched::new();
        s.mode = Mode::Seek;
        let now = Instant::now();
        s.warm.push_back((5.0, 5_000));
        assert!(s.take_job(now).is_none(), "not asked for");
        s.warm_wanted = true;
        assert!(s.take_job(now).is_none(), "start delay");
        s.warm_after = now;
        s.last_hover = Some(now);
        assert!(s.take_job(now).is_none(), "someone hovers");
        let later = now + HOVER_QUIET;
        let job = s.take_job(later).expect("background download");
        assert_eq!((job.key, job.kind), (5_000, Kind::Warm));
        // The pointer comes to the bar before the server answered: the download is given up and queued again.
        s.clear_warm_flights();
        assert!(s.flights.is_empty() && s.busy.is_empty());
        assert_eq!(s.warm.front(), Some(&(5.0, 5_000)));
        // Background frames fill the cache but are never pushed.
        let job = s.take_job(later).expect("again");
        s.land(job.id, Some(Vec::new()), later);
        assert_eq!(s.take_fetched().map(|f| f.key), Some(5_000));
        assert!(!s.finish(5_000, 0, url("w")));
        assert!(s.frames.keys.contains_key(&5_000));
    }
}
