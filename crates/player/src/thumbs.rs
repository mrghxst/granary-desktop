//! Seek-bar previews for files without server trickplay (ADR-0009). A second, headless libmpv handle (`vo=null`,
//! no audio, software decoding) turns keyframes into small JPEGs; HDR sources are tone-mapped to SDR by libavfilter
//! (zscale + tonemap) inside that handle, so the main player's GPU pipeline is never touched.
//!
//! Matroska files with a keyframe index (Cues) are read through the index ([`crate::mkv`]): each preview fetches one
//! keyframe with a byte-range request ([`KeyframeFetcher`], usable from other threads while the decoder works), wraps
//! it in a single-frame file held in memory and decodes only that. Other files fall back to opening the stream in the
//! preview handle and seeking to keyframes. Nothing touches the disk: frames reach mpv through a custom stream
//! protocol and come back with `screenshot-raw`, encoded to JPEG here.
use crate::PlayerError;
use crate::backend::PlayerBackend;
use crate::events::MpvEvent;
use crate::mkv::{MkvIndex, RangeSource};
use crate::mpv::{MemoryFiles, Mpv, RawImage};
use crate::range::{FileRange, HttpRange};
use std::path::Path;
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
/// Protocol of the in-memory single-frame files.
const PROTOCOL: &str = "granary-frame";
const JPEG_QUALITY: u8 = 78;

/// Options for the preview handle: no window, no audio, no scripts or user config, cheap keyframe decodes that
/// leave the CPU to the main player.
///
/// Software decoding: a single keyframe decodes in about 0.1 s even for 4K HEVC Main10, while setting up a hardware
/// decoder again for every single-frame file costs about 0.4 s more (measured, ADR-0009 revision 4) and would
/// compete with the main player for the GPU.
pub fn options() -> Vec<(String, String)> {
    [
        ("vo", "null"),
        ("ao", "null"),
        ("audio", "no"),
        ("sub", "no"),
        ("pause", "yes"),
        ("keep-open", "yes"),
        ("hr-seek", "no"),
        ("hwdec", "no"),
        ("vd-lavc-threads", "2"),
        ("vd-lavc-skiploopfilter", "all"),
        ("vd-lavc-fast", "yes"),
        ("vf", SDR_FILTER),
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

/// A `screenshot-raw` frame (`bgr0`) as JPEG.
pub fn encode_jpeg(raw: &RawImage) -> Result<Vec<u8>, PlayerError> {
    let bad = |m: &str| PlayerError::InvalidArgument(format!("preview frame: {m}"));
    if raw.format != "bgr0" && raw.format != "bgra" {
        return Err(bad(&format!("unexpected pixel format {}", raw.format)));
    }
    let (w, h) = (
        u16::try_from(raw.width).map_err(|_| bad("too wide"))?,
        u16::try_from(raw.height).map_err(|_| bad("too tall"))?,
    );
    let row = usize::from(w) * 4;
    if raw.stride < row || raw.data.len() < raw.stride * usize::from(h).saturating_sub(1) + row {
        return Err(bad("short pixel data"));
    }
    let packed: std::borrow::Cow<'_, [u8]> = if raw.stride == row {
        std::borrow::Cow::Borrowed(&raw.data[..row * usize::from(h)])
    } else {
        std::borrow::Cow::Owned(
            raw.data
                .chunks(raw.stride)
                .take(usize::from(h))
                .flat_map(|r| &r[..row])
                .copied()
                .collect(),
        )
    };
    let mut out = Vec::with_capacity(16 * 1024);
    jpeg_encoder::Encoder::new(&mut out, JPEG_QUALITY)
        .encode(&packed, w, h, jpeg_encoder::ColorType::Bgra)
        .map_err(|e| bad(&e.to_string()))?;
    Ok(out)
}

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

/// Fetches single-keyframe files of an indexed source. Cheap to clone; each fetch opens its own connection, so
/// several can run at once and while the decoder is busy.
#[derive(Clone)]
pub struct KeyframeFetcher {
    index: Arc<MkvIndex>,
    source: String,
    timeout: Duration,
}

impl KeyframeFetcher {
    /// The keyframe time a preview for `seconds` shows.
    pub fn snap(&self, seconds: f64) -> Option<f64> {
        self.index.cue_for(seconds).map(|c| self.index.seconds(&c))
    }

    /// The keyframe time and a complete single-frame Matroska file for the keyframe at or before `seconds`.
    pub fn fetch(&self, seconds: f64) -> Result<(f64, Vec<u8>), PlayerError> {
        let bad = |m: String| PlayerError::InvalidArgument(format!("preview keyframe: {m}"));
        let cue = self.index.cue_for(seconds).ok_or_else(|| bad("no keyframe".into()))?;
        let mut src = range_source(&self.source, self.timeout).ok_or_else(|| bad("source not readable".into()))?;
        let file = self.index.frame_file(&mut *src, &cue).map_err(|e| bad(e.to_string()))?;
        Ok((self.index.seconds(&cue), file))
    }
}

/// How frames are found.
enum Mode {
    /// Matroska with an index: one keyframe per preview, decoded from a single-frame file in memory.
    Index {
        fetcher: KeyframeFetcher,
        /// The in-memory file mpv opened last (dropped once the next one is open).
        last: Option<String>,
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
    files: Arc<MemoryFiles>,
    source: String,
    mode: Mode,
    hdr_filter: bool,
}

static SEQ: AtomicU64 = AtomicU64::new(0);

impl Thumbnailer {
    /// Open `source` (a URL or path mpv can read). Matroska files with an index are read through the index and
    /// nothing is opened in mpv yet; anything else is opened and waited for until it can seek.
    pub fn open(source: &str, timeout: Duration) -> Result<Self, PlayerError> {
        let mpv = Mpv::new(&options())?;
        let files = Arc::new(MemoryFiles::default());
        mpv.add_memory_protocol(PROTOCOL, Arc::clone(&files))?;
        let (tx, rx) = channel();
        mpv.spawn_events(move |e| {
            let _ = tx.send(e);
        })
        .map_err(|e| PlayerError::Unavailable(format!("preview event thread: {e}")))?;
        let indexed = range_source(source, timeout).and_then(|mut src| match MkvIndex::load(&mut *src) {
            Ok(index) => Some(index),
            Err(e) => {
                tracing::debug!(error = %e, "no keyframe index; previews seek in the stream");
                None
            }
        });
        let mut t = Self {
            mpv,
            events: rx,
            files,
            source: source.to_owned(),
            mode: Mode::Seek,
            hdr_filter: false,
        };
        if let Some(index) = indexed {
            tracing::info!(
                keyframes = index.cues().len(),
                "seek previews use the file's keyframe index"
            );
            if index.is_hdr() {
                t.set_hdr_filter()?;
            }
            t.mode = Mode::Index {
                fetcher: KeyframeFetcher {
                    index: Arc::new(index),
                    source: source.to_owned(),
                    timeout,
                },
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

    /// A fetcher for keyframes of this source (index mode), to download outside the decoder.
    pub fn fetcher(&self) -> Option<KeyframeFetcher> {
        match &self.mode {
            Mode::Index { fetcher, .. } => Some(fetcher.clone()),
            Mode::Seek => None,
        }
    }

    /// The keyframe time a preview for `seconds` shows (index mode), so callers can cache per keyframe.
    pub fn snap(&self, seconds: f64) -> Option<f64> {
        match &self.mode {
            Mode::Index { fetcher, .. } => fetcher.snap(seconds),
            Mode::Seek => None,
        }
    }

    /// Every keyframe time in the index (index mode), for background prefetching; `None` in seek mode.
    pub fn keyframes(&self) -> Option<Vec<f64>> {
        match &self.mode {
            Mode::Index { fetcher, .. } => {
                Some(fetcher.index.cues().iter().map(|c| fetcher.index.seconds(c)).collect())
            }
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

    /// Load an in-memory single-frame file and wait for its frame. The previous file's end arrives first and is
    /// expected.
    fn load_frame(&self, uri: &str, timeout: Duration) -> Result<(), PlayerError> {
        while self.events.try_recv().is_ok() {}
        self.mpv.command(&["loadfile", uri])?;
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

    fn jpeg(&self) -> Result<Vec<u8>, PlayerError> {
        encode_jpeg(&self.mpv.screenshot_raw()?)
    }

    /// Decode a single-frame file from [`KeyframeFetcher::fetch`] (index mode) into a JPEG.
    pub fn decode(&mut self, file: Vec<u8>, timeout: Duration) -> Result<Vec<u8>, PlayerError> {
        if !self.indexed() {
            return Err(PlayerError::InvalidArgument("preview source has no index".into()));
        }
        let uri = format!("{PROTOCOL}://{}.mkv", SEQ.fetch_add(1, Ordering::Relaxed));
        self.files.insert(uri.clone(), Arc::from(file));
        let loaded = self.load_frame(&uri, timeout);
        let mut first = false;
        if let Mode::Index {
            last, gamma_checked, ..
        } = &mut self.mode
        {
            // mpv closed the previous file when this one started; its reader holds its own reference until then.
            if let Some(old) = last.replace(uri.clone()) {
                self.files.remove(&old);
            }
            first = !std::mem::replace(gamma_checked, true);
        }
        loaded?;
        if first && !self.hdr_filter {
            let gamma = self.mpv.get_property("video-params/gamma").unwrap_or_default();
            if needs_tonemap(&gamma) {
                self.set_hdr_filter()?;
                self.load_frame(&uri, timeout)?;
            }
        }
        self.jpeg()
    }

    /// A JPEG of the keyframe at or before `seconds` (fetch and decode in one call).
    pub fn frame(&mut self, seconds: f64, timeout: Duration) -> Result<Vec<u8>, PlayerError> {
        if !seconds.is_finite() || seconds < 0.0 {
            return Err(PlayerError::InvalidArgument("preview time".into()));
        }
        let Some(fetcher) = self.fetcher() else {
            // Drop stale events so the restart we wait for belongs to this seek.
            while self.events.try_recv().is_ok() {}
            self.mpv
                .command(&["seek", &format!("{seconds:.3}"), "absolute+keyframes"])?;
            self.wait(timeout, |e| matches!(e, MpvEvent::PlaybackRestart))?;
            return self.jpeg();
        };
        let (_, file) = fetcher.fetch(seconds)?;
        self.decode(file, timeout)
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
    fn preview_handle_is_headless_and_decodes_in_software() {
        let o = options();
        let get = |k: &str| o.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str());
        assert_eq!(get("vo"), Some("null"));
        assert_eq!(get("audio"), Some("no"));
        assert_eq!(get("load-scripts"), Some("no"));
        assert_eq!(get("config"), Some("no"));
        assert_eq!(get("keep-open"), Some("yes"));
        assert_eq!(get("hwdec"), Some("no"));
    }

    #[test]
    fn only_loopback_or_paths_are_indexed() {
        let t = Duration::from_secs(1);
        assert!(range_source("https://example.com/a.mkv", t).is_none());
        assert!(range_source("http://example.com/a.mkv", t).is_none());
        assert!(range_source("http://127.0.0.1:9/r/x/a.mkv", t).is_some());
    }

    #[test]
    fn jpeg_from_padded_rows() {
        // 2x2 grey, rows padded to 16 bytes as mpv does for alignment.
        let mut data = vec![0u8; 32];
        for row in 0..2 {
            for px in 0..2 {
                let i = row * 16 + px * 4;
                data[i..i + 4].copy_from_slice(&[128, 128, 128, 0]);
            }
        }
        let raw = RawImage {
            width: 2,
            height: 2,
            stride: 16,
            format: "bgr0".into(),
            data,
        };
        let jpg = encode_jpeg(&raw).unwrap_or_default();
        assert!(jpg.starts_with(&[0xFF, 0xD8]) && jpg.ends_with(&[0xFF, 0xD9]));
        let short = RawImage {
            data: vec![0; 8],
            ..raw
        };
        assert!(encode_jpeg(&short).is_err());
    }
}
