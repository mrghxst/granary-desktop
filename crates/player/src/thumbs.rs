//! Seek-bar previews for files without server trickplay (ADR-0009). A second, headless libmpv handle (`vo=null`,
//! no audio) decodes small JPEGs with `screenshot-to-file`; HDR sources are tone-mapped to SDR by libavfilter
//! (zscale + tonemap) inside that handle, so the main player's GPU pipeline is never touched.
//!
//! Matroska files with a keyframe index (Cues) are read through the index ([`crate::mkv`]): each preview fetches one
//! keyframe with a byte-range request, wraps it in a single-frame file and decodes only that. Other files fall back
//! to opening the stream in the preview handle and seeking to keyframes.
use crate::PlayerError;
use crate::backend::PlayerBackend;
use crate::events::MpvEvent;
use crate::mkv::{MkvIndex, RangeSource};
use crate::mpv::Mpv;
use crate::range::{FileRange, HttpRange};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, channel};
use std::time::{Duration, Instant};

/// Width of a preview frame in pixels (height follows the aspect ratio).
pub const WIDTH: u32 = 320;
const SDR_FILTER: &str = "scale=w=320:h=-2";
/// PQ/HLG to SDR: linearise, convert to BT.709 primaries, Hable tone curve, back to a BT.709 transfer.
const HDR_FILTER: &str = "zscale=w=320:h=-2:t=linear:npl=100,format=gbrpf32le,zscale=p=bt709,\
tonemap=tonemap=hable:desat=0,zscale=t=bt709:m=bt709:r=tv,format=yuv420p";

/// Options for the preview handle: no window, no audio, no scripts or user config, cheap keyframe decodes that
/// leave the CPU to the main player.
pub fn options() -> Vec<(String, String)> {
    [
        ("vo", "null"),
        ("ao", "null"),
        ("audio", "no"),
        ("sub", "no"),
        ("pause", "yes"),
        ("keep-open", "yes"),
        ("hr-seek", "no"),
        ("hwdec", "auto-copy"),
        ("vd-lavc-threads", "2"),
        ("vd-lavc-skiploopfilter", "all"),
        ("vd-lavc-fast", "yes"),
        ("vf", SDR_FILTER),
        ("screenshot-format", "jpg"),
        ("screenshot-jpeg-quality", "78"),
        ("cache", "no"),
        ("demuxer-readahead-secs", "0"),
        ("config", "no"),
        ("load-scripts", "no"),
        ("ytdl", "no"),
        ("input-default-bindings", "no"),
        ("terminal", "no"),
    ]
    .iter()
    .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
    .collect()
}

/// The tone-mapping filter chain is needed for this `video-params/gamma` value.
pub fn needs_tonemap(gamma: &str) -> bool {
    matches!(gamma, "pq" | "hlg")
}

/// How frames are found.
enum Mode {
    /// Matroska with an index: one keyframe per preview, decoded from a single-frame local file.
    Index {
        index: MkvIndex,
        src: Box<dyn RangeSource + Send>,
        /// The single-frame file mpv opened last (deleted once mpv has moved on).
        last: Option<PathBuf>,
        /// `video-params/gamma` was checked once (the track header may lack colour metadata).
        gamma_checked: bool,
    },
    /// Anything else: the stream is open in mpv and each preview is a keyframe seek.
    Seek,
}

/// One open preview source.
pub struct Thumbnailer {
    mpv: Arc<Mpv>,
    events: Receiver<MpvEvent>,
    source: String,
    dir: PathBuf,
    mode: Mode,
    hdr_filter: bool,
}

static SEQ: AtomicU64 = AtomicU64::new(0);

/// Byte access to `source` for the index: the loopback relay over HTTP, or a local path.
fn range_source(source: &str, timeout: Duration) -> Option<Box<dyn RangeSource + Send>> {
    if source.starts_with("http://") {
        return HttpRange::new(source, timeout).map(|h| Box::new(h) as Box<dyn RangeSource + Send>);
    }
    if source.contains("://") {
        return None;
    }
    FileRange::open(Path::new(source))
        .ok()
        .map(|f| Box::new(f) as Box<dyn RangeSource + Send>)
}

impl Thumbnailer {
    /// Open `source` (a URL or path mpv can read). Matroska files with an index are read through the index and
    /// nothing is opened in mpv yet; anything else is opened and waited for until it can seek.
    pub fn open(source: &str, dir: &Path, timeout: Duration) -> Result<Self, PlayerError> {
        std::fs::create_dir_all(dir).map_err(|e| PlayerError::InvalidArgument(format!("preview dir: {e}")))?;
        let mpv = Mpv::new(&options())?;
        let (tx, rx) = channel();
        mpv.spawn_events(move |e| {
            let _ = tx.send(e);
        })
        .map_err(|e| PlayerError::Unavailable(format!("preview event thread: {e}")))?;
        let indexed = range_source(source, timeout).and_then(|mut src| match MkvIndex::load(&mut *src) {
            Ok(index) => Some((index, src)),
            Err(e) => {
                tracing::debug!(error = %e, "no keyframe index; previews seek in the stream");
                None
            }
        });
        let mut t = Self {
            mpv,
            events: rx,
            source: source.to_owned(),
            dir: dir.to_owned(),
            mode: Mode::Seek,
            hdr_filter: false,
        };
        if let Some((index, src)) = indexed {
            tracing::info!(
                keyframes = index.cues().len(),
                "seek previews use the file's keyframe index"
            );
            if index.is_hdr() {
                t.set_hdr_filter()?;
            }
            t.mode = Mode::Index {
                index,
                src,
                last: None,
                gamma_checked: false,
            };
            return Ok(t);
        }
        t.mpv.command(&["loadfile", source])?;
        t.wait(timeout, |e| matches!(e, MpvEvent::PlaybackRestart))?;
        let gamma = t.mpv.get_property("video-params/gamma").unwrap_or_default();
        if needs_tonemap(&gamma) {
            t.set_hdr_filter()?;
            // Re-seek so the first preview already goes through the new chain.
            t.mpv.command(&["seek", "0", "absolute+keyframes"])?;
            t.wait(timeout, |e| matches!(e, MpvEvent::PlaybackRestart))?;
        }
        Ok(t)
    }

    fn set_hdr_filter(&mut self) -> Result<(), PlayerError> {
        self.mpv.set_property("vf", HDR_FILTER)?;
        self.hdr_filter = true;
        Ok(())
    }

    pub fn source(&self) -> &str {
        &self.source
    }

    /// Whether previews come from the keyframe index (cheap) rather than stream seeks.
    pub fn indexed(&self) -> bool {
        matches!(self.mode, Mode::Index { .. })
    }

    /// The keyframe time a preview for `seconds` shows (index mode), so callers can cache per keyframe.
    pub fn snap(&self, seconds: f64) -> Option<f64> {
        match &self.mode {
            Mode::Index { index, .. } => index.cue_for(seconds).map(|c| index.seconds(&c)),
            Mode::Seek => None,
        }
    }

    fn wait(&self, timeout: Duration, want: impl Fn(&MpvEvent) -> bool) -> Result<(), PlayerError> {
        let end = Instant::now() + timeout;
        loop {
            let left = end.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Err(PlayerError::InvalidArgument("preview timed out".into()));
            }
            match self.events.recv_timeout(left) {
                Ok(MpvEvent::EndFile { reason }) => {
                    return Err(PlayerError::InvalidArgument(format!(
                        "preview source ended: {reason:?}"
                    )));
                }
                Ok(MpvEvent::Shutdown) | Err(RecvTimeoutError::Disconnected) => {
                    return Err(PlayerError::Unavailable("preview core shut down".into()));
                }
                Ok(e) if want(&e) => return Ok(()),
                Ok(_) | Err(RecvTimeoutError::Timeout) => {}
            }
        }
    }

    /// Load a local single-frame file and wait for its frame. The previous file's end arrives first and is expected.
    fn load_frame_file(&self, path: &Path, timeout: Duration) -> Result<(), PlayerError> {
        while self.events.try_recv().is_ok() {}
        self.mpv.command(&["loadfile", &path.to_string_lossy()])?;
        let end = Instant::now() + timeout;
        let mut started = false;
        loop {
            let left = end.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Err(PlayerError::InvalidArgument("preview timed out".into()));
            }
            match self.events.recv_timeout(left) {
                Ok(MpvEvent::StartFile) => started = true,
                Ok(MpvEvent::EndFile { reason }) if started => {
                    return Err(PlayerError::InvalidArgument(format!(
                        "preview frame failed: {reason:?}"
                    )));
                }
                Ok(MpvEvent::PlaybackRestart) if started => return Ok(()),
                Ok(MpvEvent::Shutdown) | Err(RecvTimeoutError::Disconnected) => {
                    return Err(PlayerError::Unavailable("preview core shut down".into()));
                }
                Ok(_) | Err(RecvTimeoutError::Timeout) => {}
            }
        }
    }

    fn screenshot(&self) -> Result<Vec<u8>, PlayerError> {
        let n = SEQ.fetch_add(1, Ordering::Relaxed);
        let path = self.dir.join(format!("p{}-{n}.jpg", std::process::id()));
        let shot = path.to_string_lossy().into_owned();
        self.mpv.command(&["screenshot-to-file", &shot, "video"])?;
        let bytes = std::fs::read(&path).map_err(|e| PlayerError::InvalidArgument(format!("preview read: {e}")));
        let _ = std::fs::remove_file(&path);
        bytes
    }

    /// A JPEG of the keyframe at or before `seconds`.
    pub fn frame(&mut self, seconds: f64, timeout: Duration) -> Result<Vec<u8>, PlayerError> {
        if !seconds.is_finite() || seconds < 0.0 {
            return Err(PlayerError::InvalidArgument("preview time".into()));
        }
        let Mode::Index { index, src, .. } = &mut self.mode else {
            // Drop stale events so the restart we wait for belongs to this seek.
            while self.events.try_recv().is_ok() {}
            self.mpv
                .command(&["seek", &format!("{seconds:.3}"), "absolute+keyframes"])?;
            self.wait(timeout, |e| matches!(e, MpvEvent::PlaybackRestart))?;
            return self.screenshot();
        };
        let cue = index
            .cue_for(seconds)
            .ok_or_else(|| PlayerError::InvalidArgument("no keyframe".into()))?;
        let file = index
            .frame_file(&mut **src, &cue)
            .map_err(|e| PlayerError::InvalidArgument(format!("preview keyframe: {e}")))?;
        let n = SEQ.fetch_add(1, Ordering::Relaxed);
        let path = self.dir.join(format!("f{}-{n}.mkv", std::process::id()));
        std::fs::write(&path, &file).map_err(|e| PlayerError::InvalidArgument(format!("preview write: {e}")))?;
        let loaded = self.load_frame_file(&path, timeout);
        let mut first = false;
        if let Mode::Index {
            last, gamma_checked, ..
        } = &mut self.mode
        {
            if let Some(old) = last.replace(path.clone()) {
                // mpv closed it when the new file started.
                let _ = std::fs::remove_file(old);
            }
            first = !std::mem::replace(gamma_checked, true);
        }
        loaded?;
        if first && !self.hdr_filter {
            let gamma = self.mpv.get_property("video-params/gamma").unwrap_or_default();
            if needs_tonemap(&gamma) {
                self.set_hdr_filter()?;
                self.load_frame_file(&path, timeout)?;
            }
        }
        self.screenshot()
    }
}

impl Drop for Thumbnailer {
    fn drop(&mut self) {
        self.mpv.quit();
        if let Mode::Index { last: Some(path), .. } = &self.mode {
            let _ = std::fs::remove_file(path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tonemap_only_for_hdr_transfers() {
        assert!(needs_tonemap("pq"));
        assert!(needs_tonemap("hlg"));
        assert!(!needs_tonemap("bt.1886"));
        assert!(!needs_tonemap(""));
    }

    #[test]
    fn preview_handle_is_headless() {
        let o = options();
        let get = |k: &str| o.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str());
        assert_eq!(get("vo"), Some("null"));
        assert_eq!(get("audio"), Some("no"));
        assert_eq!(get("load-scripts"), Some("no"));
        assert_eq!(get("config"), Some("no"));
        assert_eq!(get("keep-open"), Some("yes"));
    }

    #[test]
    fn only_loopback_or_paths_are_indexed() {
        let t = Duration::from_secs(1);
        assert!(range_source("https://example.com/a.mkv", t).is_none());
        assert!(range_source("http://example.com/a.mkv", t).is_none());
        assert!(range_source("http://127.0.0.1:9/r/x/a.mkv", t).is_some());
    }
}
