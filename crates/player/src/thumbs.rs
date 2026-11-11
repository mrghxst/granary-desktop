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
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::sync::{Arc, Mutex};
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
        // Slice threads: a lone keyframe gains nothing from frame threads; 4K HEVC decodes about a fifth faster
        // (105 → 85 ms median, tests/preview_bench.rs).
        ("vd-lavc-threads", "4"),
        ("vd-lavc-o", "thread_type=slice"),
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

/// Fetches keyframes of an indexed source. Cheap to clone; each fetch opens its own connection, so several can run
/// at once and while the decoder is busy.
#[derive(Clone)]
pub struct KeyframeFetcher {
    index: Arc<MkvIndex>,
    source: String,
    timeout: Duration,
    /// Recent keyframe sizes, to size the first read (shared by the clones).
    sizes: Arc<Mutex<SizeGuess>>,
    /// Decoders for the slices of split keyframes (shared by the clones; started at the first such keyframe).
    strips: Arc<StripPool>,
}

/// Sizes the first read of a keyframe from the last ones: large enough that most keyframes take one request, small
/// enough that little is read in vain (the approach of AIOStreams' thumbnailer).
#[derive(Default)]
pub struct SizeGuess {
    recent: std::collections::VecDeque<u64>,
    /// The guess before any keyframe was read (0: the floor).
    start: u64,
}

impl SizeGuess {
    const FLOOR: u64 = 256 * 1024;
    const CEIL: u64 = 8 << 20;
    /// Upper bound of the first guess.
    const START_CEIL: u64 = 2 << 20;

    /// Starts from the file's average byte rate: a keyframe is taken to be a twelfth of a second of the file (as
    /// AIOStreams' thumbnailer does), so the first keyframes of a high-bitrate file take one request too.
    pub fn for_rate(bytes_per_second: u64) -> Self {
        Self {
            recent: std::collections::VecDeque::new(),
            start: (bytes_per_second / 12).clamp(Self::FLOOR, Self::START_CEIL),
        }
    }

    pub fn next(&self) -> u64 {
        if self.recent.len() < 4 {
            // Few samples: the largest seen plus a quarter, and not below the starting guess.
            return self
                .recent
                .iter()
                .max()
                .map_or(Self::FLOOR, |m| m * 5 / 4)
                .max(self.start)
                .clamp(Self::FLOOR, Self::CEIL);
        }
        let mut sorted: Vec<u64> = self.recent.iter().copied().collect();
        sorted.sort_unstable();
        (sorted[sorted.len() * 3 / 4] * 7 / 5).clamp(Self::FLOOR, Self::CEIL)
    }

    pub fn learn(&mut self, needed: u64) {
        if self.recent.len() == 32 {
            self.recent.pop_front();
        }
        self.recent.push_back(needed);
    }
}

impl KeyframeFetcher {
    fn new(index: MkvIndex, source: &str, timeout: Duration) -> Self {
        // The last keyframe sits near the end of the file: its position over its time is the average byte rate.
        let rate = index.cues().last().map_or(0, |c| {
            let seconds = index.seconds(c);
            if seconds >= 1.0 {
                (c.cluster as f64 / seconds) as u64
            } else {
                0
            }
        });
        Self {
            index: Arc::new(index),
            source: source.to_owned(),
            timeout,
            sizes: Arc::new(Mutex::new(SizeGuess::for_rate(rate))),
            strips: Arc::new(StripPool::new()),
        }
    }

    /// The keyframe time a preview for `seconds` shows.
    pub fn snap(&self, seconds: f64) -> Option<f64> {
        self.index.cue_for(seconds).map(|c| self.index.seconds(&c))
    }

    /// The time of the last keyframe (about the duration).
    pub fn last_keyframe(&self) -> Option<f64> {
        self.index.cues().last().map(|c| self.index.seconds(c))
    }

    /// Whether keyframes are decoded slice by slice: `None` before the first keyframe with several slices, then
    /// whether its strips matched the picture decoded whole.
    pub fn split(&self) -> Option<bool> {
        if self.strips.off.load(Ordering::Relaxed) {
            Some(false)
        } else {
            self.strips.checked.load(Ordering::Relaxed).then_some(true)
        }
    }

    /// The keyframe at or before `seconds`: its time and its block, for [`Thumbnailer::decode`].
    pub fn fetch(&self, seconds: f64) -> Result<(f64, Vec<u8>), PlayerError> {
        let bad = |m: String| PlayerError::InvalidArgument(format!("preview keyframe: {m}"));
        let cue = self.index.cue_for(seconds).ok_or_else(|| bad("no keyframe".into()))?;
        let mut src = range_source(&self.source, self.timeout).ok_or_else(|| bad("source not readable".into()))?;
        let first = self.sizes.lock().map_or(SizeGuess::FLOOR, |s| s.next());
        let f = self
            .index
            .frame_file_sized(&mut *src, &cue, first)
            .map_err(|e| bad(e.to_string()))?;
        if let Ok(mut s) = self.sizes.lock() {
            s.learn(f.needed);
        }
        tracing::trace!(first, needed = f.needed, reads = f.reads, "preview keyframe read");
        Ok((self.index.seconds(&cue), f.block))
    }
}

static SEQ: AtomicU64 = AtomicU64::new(0);

fn timed_out() -> PlayerError {
    PlayerError::InvalidArgument("preview timed out".into())
}

/// A headless libmpv handle that turns single-frame files held in memory into pictures.
struct Handle {
    mpv: Arc<Mpv>,
    events: Receiver<MpvEvent>,
    files: Arc<MemoryFiles>,
    /// The in-memory file mpv opened last (dropped once the next one is open).
    last: Option<String>,
    hdr_filter: bool,
    /// `video-params/gamma` was checked once (the track header may lack colour metadata).
    gamma_checked: bool,
    /// Decodes strips of a picture: their scaled height is rounded per strip, and the pixel aspect ratio that
    /// rounding leaves must not widen the picture mpv hands back.
    strip: bool,
}

impl Handle {
    fn new(hdr: bool, strip: bool) -> Result<Self, PlayerError> {
        let mpv = Mpv::new(&options())?;
        let files = Arc::new(MemoryFiles::default());
        mpv.add_memory_protocol(PROTOCOL, Arc::clone(&files))?;
        let (tx, rx) = channel();
        mpv.spawn_events(move |e| {
            let _ = tx.send(e);
        })
        .map_err(|e| PlayerError::Unavailable(format!("preview event thread: {e}")))?;
        let mut h = Self {
            mpv,
            events: rx,
            files,
            last: None,
            hdr_filter: false,
            gamma_checked: false,
            strip,
        };
        if strip {
            // A strip is one slice: the decoder has nothing to spread over threads.
            h.mpv.set_property("vd-lavc-threads", "1")?;
        }
        if hdr {
            h.set_hdr_filter()?;
        } else if strip {
            h.set_filter(SDR_FILTER)?;
        }
        Ok(h)
    }

    fn set_filter(&self, chain: &str) -> Result<(), PlayerError> {
        if self.strip {
            self.mpv.set_property("vf", &format!("{chain},setsar=1"))
        } else {
            self.mpv.set_property("vf", chain)
        }
    }

    fn set_hdr_filter(&mut self) -> Result<(), PlayerError> {
        self.set_filter(HDR_FILTER)?;
        self.hdr_filter = true;
        Ok(())
    }

    fn wait(&self, timeout: Duration, want: impl Fn(&MpvEvent) -> bool) -> Result<(), PlayerError> {
        let end = Instant::now() + timeout;
        loop {
            let left = end.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Err(timed_out());
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
                return Err(timed_out());
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

    /// The picture of a single-frame file, scaled and tone-mapped by the handle's filters.
    fn picture(&mut self, file: Vec<u8>, timeout: Duration) -> Result<RawImage, PlayerError> {
        let uri = format!("{PROTOCOL}://{}.mkv", SEQ.fetch_add(1, Ordering::Relaxed));
        self.files.insert(uri.clone(), Arc::from(file));
        let loaded = self.load_frame(&uri, timeout);
        // mpv closed the previous file when this one started; its reader holds its own reference until then.
        if let Some(old) = self.last.replace(uri.clone()) {
            self.files.remove(&old);
        }
        loaded?;
        if !std::mem::replace(&mut self.gamma_checked, true) && !self.hdr_filter {
            let gamma = self.mpv.get_property("video-params/gamma").unwrap_or_default();
            if needs_tonemap(&gamma) {
                self.set_hdr_filter()?;
                self.load_frame(&uri, timeout)?;
            }
        }
        self.mpv.screenshot_raw()
    }
}

impl Drop for Handle {
    fn drop(&mut self) {
        self.mpv.quit();
    }
}

struct StripJob {
    file: Vec<u8>,
    slot: usize,
    hdr: bool,
    timeout: Duration,
    done: Sender<(usize, Result<RawImage, PlayerError>)>,
}

/// Decoders for the slices of split HEVC keyframes ([`crate::hevc`]): a few handles, each decoding one strip at a
/// time, so the slices of a 4K keyframe decode side by side instead of one after the other. Nothing is started
/// until the first split keyframe, and everything stops when the last [`KeyframeFetcher`] of the source is dropped.
struct StripPool {
    workers: usize,
    jobs: Mutex<Option<Sender<StripJob>>>,
    /// A split keyframe failed to decode or did not match: the source's keyframes are decoded whole from then on.
    off: AtomicBool,
    /// The first split keyframe matched the picture decoded whole.
    checked: AtomicBool,
}

impl StripPool {
    fn new() -> Self {
        // Half the cores, so playback keeps the rest; two at least (the gain), four at most (the usual slice count).
        let cores = std::thread::available_parallelism().map_or(4, std::num::NonZero::get);
        Self {
            workers: (cores / 2).clamp(2, 4),
            jobs: Mutex::new(None),
            off: AtomicBool::new(false),
            checked: AtomicBool::new(false),
        }
    }

    fn sender(&self) -> Option<Sender<StripJob>> {
        let mut jobs = self.jobs.lock().ok()?;
        if jobs.is_none() {
            let (tx, rx) = channel::<StripJob>();
            let rx = Arc::new(Mutex::new(rx));
            for n in 0..self.workers {
                let rx = Arc::clone(&rx);
                let spawned = std::thread::Builder::new()
                    .name(format!("preview-strip-{n}"))
                    .spawn(move || strip_worker(&rx));
                if let Err(e) = spawned {
                    tracing::warn!(error = %e, "seek previews: fewer strip decoders");
                }
            }
            *jobs = Some(tx);
        }
        jobs.clone()
    }

    /// The decoded strips of `block`, top to bottom; `None` when the keyframe is to be decoded whole.
    fn pictures(&self, index: &MkvIndex, block: &[u8], hdr: bool, timeout: Duration) -> Option<Vec<RawImage>> {
        if self.off.load(Ordering::Relaxed) {
            return None;
        }
        let files = index.strip_files(block)?;
        let count = files.len();
        let (done, results) = channel();
        let sent = self.sender().is_some_and(|tx| {
            files.into_iter().enumerate().all(|(slot, file)| {
                tx.send(StripJob {
                    file,
                    slot,
                    hdr,
                    timeout,
                    done: done.clone(),
                })
                .is_ok()
            })
        });
        drop(done);
        let mut parts: Vec<Option<RawImage>> = (0..count).map(|_| None).collect();
        let mut failed = (!sent).then(|| "no strip decoder".to_owned());
        while failed.is_none() && parts.iter().any(Option::is_none) {
            match results.recv() {
                Ok((slot, Ok(picture))) => parts[slot] = Some(picture),
                Ok((_, Err(e))) => failed = Some(e.to_string()),
                Err(_) => failed = Some("strip decoders stopped".to_owned()),
            }
        }
        if let Some(error) = failed {
            tracing::warn!(
                error,
                "seek previews: slices did not decode on their own; decoding keyframes whole"
            );
            self.off.store(true, Ordering::Relaxed);
            return None;
        }
        parts.into_iter().collect()
    }
}

fn strip_worker(jobs: &Mutex<Receiver<StripJob>>) {
    let mut handle: Option<Handle> = None;
    loop {
        // Ends when the pool (the sender) is gone.
        let job = match jobs.lock().map(|rx| rx.recv()) {
            Ok(Ok(job)) => job,
            _ => return,
        };
        let picture = match handle.as_mut() {
            Some(h) => h.picture(job.file, job.timeout),
            None => Handle::new(job.hdr, true).and_then(|h| handle.insert(h).picture(job.file, job.timeout)),
        };
        let _ = job.done.send((job.slot, picture));
    }
}

/// Strips of one picture, stacked top to bottom.
fn stack(parts: &[RawImage]) -> Result<RawImage, PlayerError> {
    let bad = |m: &str| PlayerError::InvalidArgument(format!("preview strips: {m}"));
    let first = parts.first().ok_or_else(|| bad("none"))?;
    let row = first.width as usize * 4;
    let mut data = Vec::with_capacity(row * parts.iter().map(|p| p.height as usize).sum::<usize>());
    let mut height = 0;
    for p in parts {
        if p.width != first.width || p.format != first.format || p.stride < row {
            return Err(bad("sizes differ"));
        }
        for y in 0..p.height as usize {
            data.extend_from_slice(
                p.data
                    .get(y * p.stride..y * p.stride + row)
                    .ok_or_else(|| bad("short"))?,
            );
        }
        height += p.height;
    }
    Ok(RawImage {
        width: first.width,
        height,
        stride: row,
        format: first.format.clone(),
        data,
    })
}

/// Whether two pictures show the same thing: mean colours over a coarse grid, so the row or two by which stacked
/// strips may differ from the whole picture (each strip rounds its height) does not count, and a strip that decoded
/// to garbage does.
fn alike(a: &RawImage, b: &RawImage) -> bool {
    const GRID: usize = 8;
    let cells = |p: &RawImage| -> Option<Vec<[f64; 3]>> {
        let (w, h) = (p.width as usize, p.height as usize);
        if w < GRID || h < GRID {
            return None;
        }
        let mut out = Vec::with_capacity(GRID * GRID);
        for gy in 0..GRID {
            for gx in 0..GRID {
                let mut sum = [0u64; 3];
                let (x0, x1) = (gx * w / GRID, (gx + 1) * w / GRID);
                let (y0, y1) = (gy * h / GRID, (gy + 1) * h / GRID);
                for y in y0..y1 {
                    let row = p.data.get(y * p.stride + x0 * 4..y * p.stride + x1 * 4)?;
                    for px in row.as_chunks::<4>().0 {
                        for c in 0..3 {
                            sum[c] += u64::from(px[c]);
                        }
                    }
                }
                let n = ((x1 - x0) * (y1 - y0)) as f64;
                out.push(sum.map(|s| s as f64 / n));
            }
        }
        Some(out)
    };
    let (Some(a), Some(b)) = (cells(a), cells(b)) else {
        return false;
    };
    // Levels out of 255 per cell: rounding moves a cell's mean by a level or two, garbage by dozens.
    a.iter()
        .zip(&b)
        .all(|(x, y)| x.iter().zip(y).all(|(p, q)| (p - q).abs() <= 12.0))
}

/// One open preview source.
pub struct Thumbnailer {
    handle: Handle,
    source: String,
    /// Matroska with an index: one keyframe per preview, decoded from single-frame files in memory. Otherwise the
    /// stream is open in the handle and each preview is a keyframe seek.
    fetcher: Option<KeyframeFetcher>,
}

impl Thumbnailer {
    /// Open `source` (a URL or path mpv can read). Matroska files with an index are read through the index and
    /// nothing is opened in mpv yet; anything else is opened and waited for until it can seek.
    pub fn open(source: &str, timeout: Duration) -> Result<Self, PlayerError> {
        let indexed = range_source(source, timeout).and_then(|mut src| match MkvIndex::load(&mut *src) {
            Ok(index) => Some(index),
            Err(e) => {
                tracing::debug!(error = %e, "no keyframe index; previews seek in the stream");
                None
            }
        });
        if let Some(index) = indexed {
            tracing::info!(
                keyframes = index.cues().len(),
                "seek previews use the file's keyframe index"
            );
            return Self::for_index(KeyframeFetcher::new(index, source, timeout));
        }
        let mut handle = Handle::new(false, false)?;
        handle.mpv.command(&["loadfile", source])?;
        handle.wait(timeout, |e| matches!(e, MpvEvent::PlaybackRestart))?;
        handle.gamma_checked = true;
        let gamma = handle.mpv.get_property("video-params/gamma").unwrap_or_default();
        if needs_tonemap(&gamma) {
            handle.set_hdr_filter()?;
            // Re-seek so the first preview already goes through the new chain.
            handle.mpv.command(&["seek", "0", "absolute+keyframes"])?;
            handle.wait(timeout, |e| matches!(e, MpvEvent::PlaybackRestart))?;
        }
        Ok(Self {
            handle,
            source: source.to_owned(),
            fetcher: None,
        })
    }

    /// A decoder for the indexed source behind `fetcher`. Several can decode keyframes of one source side by side;
    /// each has its own libmpv handle and reads nothing from the network itself.
    pub fn for_index(fetcher: KeyframeFetcher) -> Result<Self, PlayerError> {
        Ok(Self {
            handle: Handle::new(fetcher.index.is_hdr(), false)?,
            source: fetcher.source.clone(),
            fetcher: Some(fetcher),
        })
    }

    pub fn source(&self) -> &str {
        &self.source
    }

    /// Whether previews come from the keyframe index (cheap) rather than stream seeks.
    pub fn indexed(&self) -> bool {
        self.fetcher.is_some()
    }

    /// A fetcher for keyframes of this source (index mode), to download outside the decoder.
    pub fn fetcher(&self) -> Option<KeyframeFetcher> {
        self.fetcher.clone()
    }

    /// The keyframe time a preview for `seconds` shows (index mode), so callers can cache per keyframe.
    pub fn snap(&self, seconds: f64) -> Option<f64> {
        self.fetcher.as_ref().and_then(|f| f.snap(seconds))
    }

    /// Every keyframe time in the index (index mode), for background prefetching; `None` in seek mode.
    pub fn keyframes(&self) -> Option<Vec<f64>> {
        let index = &self.fetcher.as_ref()?.index;
        Some(index.cues().iter().map(|c| index.seconds(c)).collect())
    }

    /// Decode a keyframe block from [`KeyframeFetcher::fetch`] (index mode) into a JPEG. An HEVC keyframe made of
    /// several slices is decoded slice by slice on the source's strip decoders, everything else whole on this
    /// handle.
    pub fn decode(&mut self, block: Vec<u8>, timeout: Duration) -> Result<Vec<u8>, PlayerError> {
        let Some(fetcher) = &self.fetcher else {
            return Err(PlayerError::InvalidArgument("preview source has no index".into()));
        };
        let pool = &fetcher.strips;
        let hdr = self.handle.hdr_filter;
        if pool.checked.load(Ordering::Relaxed) {
            if let Some(parts) = pool.pictures(&fetcher.index, &block, hdr, timeout) {
                return encode_jpeg(&stack(&parts)?);
            }
            return encode_jpeg(&self.handle.picture(fetcher.index.file(&block), timeout)?);
        }
        // Until a keyframe has shown that its strips give the same picture, it is also decoded whole (at the same
        // time, on this handle) and the two are compared: a stream the splitter misreads must not show garbage.
        let file = fetcher.index.file(&block);
        let handle = &mut self.handle;
        let (parts, whole) = std::thread::scope(|s| {
            let strips = s.spawn(|| pool.pictures(&fetcher.index, &block, hdr, timeout));
            let whole = handle.picture(file, timeout);
            (strips.join().ok().flatten(), whole)
        });
        let whole = whole?;
        if let Some(parts) = parts {
            if stack(&parts).is_ok_and(|stacked| alike(&stacked, &whole)) {
                tracing::info!(slices = parts.len(), "seek previews decode keyframes slice by slice");
                pool.checked.store(true, Ordering::Relaxed);
            } else {
                tracing::warn!("seek previews: slices decoded on their own do not match; decoding keyframes whole");
                pool.off.store(true, Ordering::Relaxed);
            }
        }
        encode_jpeg(&whole)
    }

    /// A JPEG of the keyframe at or before `seconds` (fetch and decode in one call).
    pub fn frame(&mut self, seconds: f64, timeout: Duration) -> Result<Vec<u8>, PlayerError> {
        if !seconds.is_finite() || seconds < 0.0 {
            return Err(PlayerError::InvalidArgument("preview time".into()));
        }
        let Some(fetcher) = self.fetcher.clone() else {
            // Drop stale events so the restart we wait for belongs to this seek.
            while self.handle.events.try_recv().is_ok() {}
            self.handle
                .mpv
                .command(&["seek", &format!("{seconds:.3}"), "absolute+keyframes"])?;
            self.handle.wait(timeout, |e| matches!(e, MpvEvent::PlaybackRestart))?;
            return encode_jpeg(&self.handle.mpv.screenshot_raw()?);
        };
        let (_, block) = fetcher.fetch(seconds)?;
        self.decode(block, timeout)
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
