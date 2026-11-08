//! Seek-bar previews for files without server trickplay (ADR-0009): a second, headless libmpv handle (`vo=null`,
//! no audio) opens the same stream, seeks to the nearest keyframe and writes a small JPEG with
//! `screenshot-to-file`. HDR sources are tone-mapped to SDR by libavfilter (zscale + tonemap) inside that handle, so
//! the main player's GPU pipeline is never touched.
use crate::PlayerError;
use crate::backend::PlayerBackend;
use crate::events::MpvEvent;
use crate::mpv::Mpv;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, channel};
use std::time::{Duration, Instant};

/// Width of a preview frame in pixels (height follows the aspect ratio).
pub const WIDTH: u32 = 320;
const SDR_FILTER: &str = "scale=w=320:h=-2";
/// PQ/HLG to SDR: linearise, convert to BT.709 primaries, Hable tone curve, back to a BT.709 transfer.
const HDR_FILTER: &str = "zscale=w=320:h=-2:t=linear:npl=100,format=gbrpf32le,zscale=p=bt709,\
tonemap=tonemap=hable:desat=0,zscale=t=bt709:m=bt709:r=tv,format=yuv420p";

/// Options for the preview handle: no window, no audio, no scripts or user config, cheap keyframe seeks.
pub fn options() -> Vec<(String, String)> {
    [
        ("vo", "null"),
        ("ao", "null"),
        ("audio", "no"),
        ("sub", "no"),
        ("pause", "yes"),
        ("hr-seek", "no"),
        ("hwdec", "auto-copy"),
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

/// One open preview source.
pub struct Thumbnailer {
    mpv: Arc<Mpv>,
    events: Receiver<MpvEvent>,
    source: String,
    dir: PathBuf,
}

static SEQ: AtomicU64 = AtomicU64::new(0);

impl Thumbnailer {
    /// Open `source` (a URL or path mpv can read) and wait until it is ready to seek.
    pub fn open(source: &str, dir: &Path, timeout: Duration) -> Result<Self, PlayerError> {
        std::fs::create_dir_all(dir).map_err(|e| PlayerError::InvalidArgument(format!("preview dir: {e}")))?;
        let mpv = Mpv::new(&options())?;
        let (tx, rx) = channel();
        mpv.spawn_events(move |e| {
            let _ = tx.send(e);
        })
        .map_err(|e| PlayerError::Unavailable(format!("preview event thread: {e}")))?;
        let t = Self {
            mpv,
            events: rx,
            source: source.to_owned(),
            dir: dir.to_owned(),
        };
        t.mpv.command(&["loadfile", source])?;
        t.wait(timeout, |e| matches!(e, MpvEvent::PlaybackRestart))?;
        let gamma = t.mpv.get_property("video-params/gamma").unwrap_or_default();
        if needs_tonemap(&gamma) {
            t.mpv.set_property("vf", HDR_FILTER)?;
            // Re-seek so the first preview already goes through the new chain.
            t.mpv.command(&["seek", "0", "absolute+keyframes"])?;
            t.wait(timeout, |e| matches!(e, MpvEvent::PlaybackRestart))?;
        }
        Ok(t)
    }

    pub fn source(&self) -> &str {
        &self.source
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
                Ok(MpvEvent::Shutdown) | Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                    return Err(PlayerError::Unavailable("preview core shut down".into()));
                }
                Ok(e) if want(&e) => return Ok(()),
                Ok(_) | Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            }
        }
    }

    /// A JPEG of the keyframe at or before `seconds`.
    pub fn frame(&self, seconds: f64, timeout: Duration) -> Result<Vec<u8>, PlayerError> {
        if !seconds.is_finite() || seconds < 0.0 {
            return Err(PlayerError::InvalidArgument("preview time".into()));
        }
        // Drop stale events so the restart we wait for belongs to this seek.
        while self.events.try_recv().is_ok() {}
        self.mpv
            .command(&["seek", &format!("{seconds:.3}"), "absolute+keyframes"])?;
        self.wait(timeout, |e| matches!(e, MpvEvent::PlaybackRestart))?;
        let n = SEQ.fetch_add(1, Ordering::Relaxed);
        let path = self.dir.join(format!("p{}-{n}.jpg", std::process::id()));
        let shot = path.to_string_lossy().into_owned();
        self.mpv.command(&["screenshot-to-file", &shot, "video"])?;
        let bytes = std::fs::read(&path).map_err(|e| PlayerError::InvalidArgument(format!("preview read: {e}")));
        let _ = std::fs::remove_file(&path);
        bytes
    }
}

impl Drop for Thumbnailer {
    fn drop(&mut self) {
        self.mpv.quit();
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
    }
}
