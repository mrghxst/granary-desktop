//! Seek-bar previews for direct-played files without server trickplay (ADR-0009). One headless libmpv handle per
//! source, opened on first use and dropped when playback moves on. Matroska files are previewed from their keyframe
//! index (one small range request per frame, `granary_player::mkv`); frames are cached per keyframe, in memory only.
//!
//! Keyframes are downloaded outside the decoder lock, so several requests fetch at once while one decodes (software
//! decoding, about 0.1 s for a 4K keyframe; ADR-0009 revision 4).
//!
//! Three paths, cheapest first (ADR-0009 revision 3):
//! - `player_preview_near`: the nearest frame already decoded, answered from the cache without touching the decoder,
//!   so the hover tile never waits for the network;
//! - `player_preview`: the exact keyframe for the hovered time, ahead of everything else;
//! - `player_preview_warm`: a paced background fill of a coarse-to-fine grid of keyframes, only while the main
//!   player is healthy (playing or paused, not buffering) and never while a hover request waits.
use super::host::{Cmd, PlayerHost};
use crate::settings::SettingsStore;
use crate::silo::CmdResult;
use granary_player::Phase;
use serde::Serialize;
use std::collections::{BTreeMap, HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;
use tauri::{AppHandle, Manager, State};

/// Keyframe frames kept per source (about 10 KB each).
const CACHE: usize = 600;
/// Requested-time aliases kept before the map is cleared (each points at a cached keyframe).
const ALIASES: usize = 8000;
/// Background grid: frames per level, coarse to fine. The total bounds the extra traffic per file (one keyframe each,
/// typically 0.1–1.5 MB on 4K remuxes).
const LEVELS: [usize; 3] = [12, 48, 120];
/// Pause between two background frames, so the playing stream keeps the connection.
const PACE: Duration = Duration::from_millis(300);
/// Wait before the background fill starts, so the first seconds of playback get the bandwidth.
const WARM_DELAY: Duration = Duration::from_secs(4);
const OPEN_TIMEOUT: Duration = Duration::from_secs(20);
const FRAME_TIMEOUT: Duration = Duration::from_secs(15);
const BACKGROUND_FRAME_TIMEOUT: Duration = Duration::from_secs(8);

#[derive(Default)]
pub struct PreviewState {
    /// The decoder: one frame at a time.
    inner: Mutex<Decoder>,
    /// Keyframe downloads for the open source (index mode), used without holding the decoder.
    #[cfg(windows)]
    fetcher: Mutex<Option<(String, granary_player::thumbs::KeyframeFetcher)>>,
    /// Decoded frames: held only briefly, so the cache answers while a decode runs.
    frames: Mutex<Frames>,
    /// Hover requests waiting for the decoder: the background fill yields to them.
    hover: AtomicUsize,
    /// Bumped whenever the source changes; a background fill for an older source stops.
    generation: AtomicU64,
    warming: AtomicBool,
}

#[derive(Default)]
struct Decoder {
    source: String,
    #[cfg(windows)]
    thumb: Option<granary_player::thumbs::Thumbnailer>,
    /// The source could not be opened: do not retry it on every hover.
    failed: bool,
}

#[derive(Default)]
struct Frames {
    source: String,
    /// Keyframe time (ms) → JPEG data URL.
    keys: BTreeMap<u64, Arc<String>>,
    order: VecDeque<u64>,
    /// Requested time (ms) → keyframe time (ms) it shows.
    alias: HashMap<u64, u64>,
}

impl Frames {
    fn reset(&mut self, source: &str) {
        *self = Self {
            source: source.to_owned(),
            ..Self::default()
        };
    }

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

    fn alias(&mut self, requested: u64, key: u64) {
        if self.alias.len() >= ALIASES {
            self.alias.clear();
        }
        self.alias.insert(requested, key);
    }

    fn exact(&self, requested: u64) -> Option<Arc<String>> {
        let key = self.alias.get(&requested).copied().unwrap_or(requested);
        self.keys.get(&key).cloned()
    }

    /// The cached frame closest to `requested`, preferring the keyframe at or before it (what an exact preview shows).
    fn nearest(&self, requested: u64) -> Option<(u64, Arc<String>)> {
        let before = self.keys.range(..=requested).next_back();
        let after = self.keys.range(requested..).next();
        let pick = match (before, after) {
            (Some(b), Some(a)) => {
                // Same distance or closer: the earlier one (the frame the exact preview will show is at or before).
                if requested - b.0 <= a.0 - requested { b } else { a }
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

/// Cache key: a time in whole milliseconds.
fn bucket(seconds: f64) -> Option<u64> {
    // Finite and non-negative: the cast below cannot wrap (saturates at u64::MAX).
    (seconds.is_finite() && seconds >= 0.0).then(|| (seconds * 1000.0).round() as u64)
}

/// Background targets, coarse to fine: each level spreads its frames evenly over the file (centred in their slot), so
/// after the first level any hover has a frame within a twelfth of the file, then a 48th, then a 120th. Times already
/// covered by an earlier level are left out by the caller (per keyframe).
fn grid(duration: f64) -> Vec<f64> {
    if !duration.is_finite() || duration <= 0.0 {
        return Vec::new();
    }
    LEVELS
        .iter()
        .flat_map(|&n| (0..n).map(move |i| (i as f64 + 0.5) * duration / n as f64))
        .collect()
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

/// The stream previews may read, or `None` (previews off, transcode, stopped), releasing the decoder in that case.
async fn source(app: &AppHandle, host: &PlayerHost, settings: &SettingsStore) -> Option<String> {
    if !settings.current().player.preview_source_on("local") {
        return None;
    }
    let source = host.ask(Cmd::PreviewSource).await.flatten();
    if source.is_none() {
        let state = app.state::<PreviewState>();
        state.generation.fetch_add(1, Ordering::SeqCst);
        // A decode in flight keeps the decoder; the next source check resets it anyway.
        if let Ok(mut d) = state.inner.try_lock() {
            *d = Decoder::default();
        }
        #[cfg(windows)]
        {
            *lock(&state.fetcher) = None;
        }
        lock(&state.frames).reset("");
    }
    source
}

/// A JPEG data URL of the frame near `seconds` in the playing file, or `None` when previews are off, nothing
/// previewable is playing (transcode, stopped) or the frame could not be decoded.
#[tauri::command]
pub async fn player_preview(
    app: AppHandle,
    host: State<'_, PlayerHost>,
    settings: State<'_, SettingsStore>,
    seconds: f64,
) -> CmdResult<Option<String>> {
    let Some(b) = bucket(seconds) else { return Ok(None) };
    let Some(source) = self::source(&app, &host, &settings).await else {
        return Ok(None);
    };
    let at = b as f64 / 1000.0;
    let state = app.state::<PreviewState>();
    state.hover.fetch_add(1, Ordering::SeqCst);
    let worker = app.clone();
    let result = tauri::async_runtime::spawn_blocking(move || frame(&worker, &source, b, at, FRAME_TIMEOUT)).await;
    state.hover.fetch_sub(1, Ordering::SeqCst);
    Ok(result.ok().flatten().map(|s| (*s).clone()))
}

#[derive(Serialize)]
pub struct NearFrame {
    /// The time the frame shows, in seconds (a keyframe time in index mode).
    at: f64,
    url: String,
}

/// The decoded frame closest to `seconds`, straight from the cache (never decodes, never waits for the network).
#[tauri::command]
pub async fn player_preview_near(
    app: AppHandle,
    host: State<'_, PlayerHost>,
    settings: State<'_, SettingsStore>,
    seconds: f64,
) -> CmdResult<Option<NearFrame>> {
    let Some(b) = bucket(seconds) else { return Ok(None) };
    let Some(source) = self::source(&app, &host, &settings).await else {
        return Ok(None);
    };
    let state = app.state::<PreviewState>();
    let frames = lock(&state.frames);
    if frames.source != source {
        return Ok(None);
    }
    Ok(frames.nearest(b).map(|(key, url)| NearFrame {
        at: key as f64 / 1000.0,
        url: (*url).clone(),
    }))
}

/// Start the background fill for the playing file (once per source; later calls are no-ops).
#[tauri::command]
pub async fn player_preview_warm(
    app: AppHandle,
    host: State<'_, PlayerHost>,
    settings: State<'_, SettingsStore>,
) -> CmdResult<()> {
    let Some(source) = self::source(&app, &host, &settings).await else {
        return Ok(());
    };
    let state = app.state::<PreviewState>();
    if state.warming.swap(true, Ordering::SeqCst) {
        return Ok(());
    }
    let worker = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(WARM_DELAY).await;
        let app = worker.clone();
        let _ = tauri::async_runtime::spawn_blocking(move || warm(&worker, &source)).await;
        app.state::<PreviewState>().warming.store(false, Ordering::SeqCst);
    });
    Ok(())
}

/// Open the decoder for `source` if needed (resetting caches on a new source). False when previews cannot work.
#[cfg(windows)]
fn ensure_open(state: &PreviewState, d: &mut Decoder, source: &str) -> bool {
    use granary_player::thumbs::Thumbnailer;
    if d.source != source {
        state.generation.fetch_add(1, Ordering::SeqCst);
        *d = Decoder {
            source: source.to_owned(),
            ..Decoder::default()
        };
        *lock(&state.fetcher) = None;
        lock(&state.frames).reset(source);
    }
    if d.failed {
        return false;
    }
    if d.thumb.is_none() {
        match Thumbnailer::open(source, OPEN_TIMEOUT) {
            Ok(t) => {
                *lock(&state.fetcher) = t.fetcher().map(|f| (source.to_owned(), f));
                d.thumb = Some(t);
            }
            Err(e) => {
                tracing::warn!(error = %e, "seek previews unavailable for this file");
                d.failed = true;
                return false;
            }
        }
    }
    true
}

/// The keyframe fetcher for `source`, opening the decoder first if needed. `None` in seek mode or when the file
/// cannot be previewed.
#[cfg(windows)]
fn fetcher_for(state: &PreviewState, source: &str) -> Option<granary_player::thumbs::KeyframeFetcher> {
    if let Some((s, f)) = lock(&state.fetcher).as_ref()
        && s == source
    {
        return Some(f.clone());
    }
    let mut d = lock(&state.inner);
    if !ensure_open(state, &mut d, source) {
        return None;
    }
    d.thumb.as_ref().and_then(granary_player::thumbs::Thumbnailer::fetcher)
}

fn cached(state: &PreviewState, source: &str, key: u64) -> Option<Arc<String>> {
    let frames = lock(&state.frames);
    (frames.source == source)
        .then(|| frames.keys.get(&key).cloned())
        .flatten()
}

fn store(state: &PreviewState, source: &str, key: u64, jpeg: &[u8]) -> Arc<String> {
    let url = Arc::new(format!("data:image/jpeg;base64,{}", base64(jpeg)));
    let mut frames = lock(&state.frames);
    if frames.source == source {
        frames.remember(key, Arc::clone(&url));
    }
    url
}

/// The preview for `at`: from the cache, or fetched (in parallel with other requests) and decoded (one at a time).
#[cfg(windows)]
fn decode_at(state: &PreviewState, source: &str, at: f64, timeout: Duration) -> Option<(u64, Arc<String>)> {
    let Some(fetcher) = fetcher_for(state, source) else {
        // Seek mode: the stream is open in the decoder, so everything happens under its lock.
        let mut d = lock(&state.inner);
        if !ensure_open(state, &mut d, source) {
            return None;
        }
        let key = bucket(at)?;
        if let Some(hit) = cached(state, source, key) {
            return Some((key, hit));
        }
        let jpeg = d.thumb.as_mut()?.frame(at, timeout);
        return match jpeg {
            Ok(j) => Some((key, store(state, source, key, &j))),
            Err(e) => {
                tracing::debug!(error = %e, at, "seek preview frame failed");
                None
            }
        };
    };
    let key = fetcher.snap(at).and_then(bucket).or_else(|| bucket(at))?;
    if let Some(hit) = cached(state, source, key) {
        return Some((key, hit));
    }
    let file = match fetcher.fetch(at) {
        Ok((_, file)) => file,
        Err(e) => {
            tracing::debug!(error = %e, at, "seek preview keyframe not fetched");
            return None;
        }
    };
    let mut d = lock(&state.inner);
    // Another request may have decoded the same keyframe while this one downloaded.
    if let Some(hit) = cached(state, source, key) {
        return Some((key, hit));
    }
    if d.source != source {
        return None;
    }
    match d.thumb.as_mut()?.decode(file, timeout) {
        Ok(j) => Some((key, store(state, source, key, &j))),
        Err(e) => {
            tracing::debug!(error = %e, at, "seek preview frame failed");
            None
        }
    }
}

#[cfg(windows)]
fn frame(app: &AppHandle, source: &str, requested: u64, at: f64, timeout: Duration) -> Option<Arc<String>> {
    let state = app.state::<PreviewState>();
    {
        let frames = lock(&state.frames);
        if frames.source == source
            && let Some(hit) = frames.exact(requested)
        {
            return Some(hit);
        }
    }
    let (key, url) = decode_at(&state, source, at, timeout)?;
    let mut frames = lock(&state.frames);
    if frames.source == source {
        frames.alias(requested, key);
    }
    Some(url)
}

#[cfg(not(windows))]
fn frame(_app: &AppHandle, _source: &str, _requested: u64, _at: f64, _timeout: Duration) -> Option<Arc<String>> {
    None
}

/// Whether the main player can spare bandwidth: a file is playing or paused, not loading, seeking or buffering.
fn healthy(app: &AppHandle) -> bool {
    let host = app.state::<PlayerHost>();
    tauri::async_runtime::block_on(host.ask(Cmd::State))
        .is_some_and(|s| s.active && !s.buffering && matches!(s.phase, Some(Phase::Playing | Phase::Paused)))
}

/// Background fill (index mode only: seeking the stream for every grid point would compete with playback).
#[cfg(windows)]
fn warm(app: &AppHandle, source: &str) {
    let state = app.state::<PreviewState>();
    let Some(fetcher) = fetcher_for(&state, source) else {
        return;
    };
    let generation = state.generation.load(Ordering::SeqCst);
    let duration = {
        let d = lock(&state.inner);
        d.thumb
            .as_ref()
            .and_then(granary_player::thumbs::Thumbnailer::keyframes)
            .and_then(|k| k.last().copied())
            .unwrap_or(0.0)
    };
    let mut done = 0usize;
    for at in grid(duration) {
        loop {
            if state.generation.load(Ordering::SeqCst) != generation {
                return;
            }
            if state.hover.load(Ordering::SeqCst) == 0 && healthy(app) {
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        // Already cached (an earlier level or a hover hit the same keyframe): no traffic, no pause.
        if fetcher
            .snap(at)
            .and_then(bucket)
            .is_some_and(|k| cached(&state, source, k).is_some())
        {
            continue;
        }
        if decode_at(&state, source, at, BACKGROUND_FRAME_TIMEOUT).is_some() {
            done += 1;
        }
        std::thread::sleep(PACE);
    }
    tracing::debug!(frames = done, "seek preview background fill finished");
}

#[cfg(not(windows))]
fn warm(_app: &AppHandle, _source: &str) {}

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
        f.reset("x");
        assert!(f.keys.is_empty() && f.source == "x");
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
        f.alias(12_345, 10_000);
        assert_eq!(f.exact(12_345).as_deref().map(String::as_str), Some("a"));
        assert!(f.exact(12_346).is_none());
    }

    #[test]
    fn grid_goes_coarse_to_fine_inside_the_file() {
        let g = grid(1200.0);
        assert_eq!(g.len(), LEVELS.iter().sum::<usize>());
        assert!((g[0] - 50.0).abs() < 1e-9);
        assert!(g.iter().all(|&t| t > 0.0 && t < 1200.0));
        assert!(grid(0.0).is_empty() && grid(f64::NAN).is_empty());
    }
}
