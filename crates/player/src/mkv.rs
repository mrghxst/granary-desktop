//! Matroska keyframe index for seek previews (ADR-0009, revised): read the container's own index (`Cues`) and the
//! video track header with a few HTTP range requests, then fetch exactly one keyframe block per preview and wrap it
//! in a tiny single-frame Matroska file that the preview handle decodes locally. A preview then costs one or two
//! small range requests instead of a demuxer seek that reads ahead through the remote file.
//!
//! Only what previews need is parsed: EBML header, `SeekHead`, `Info` (timestamp scale), `Tracks` (the first video
//! track, kept as raw bytes so codec private data, compression and colour metadata carry over unchanged), `Cues`, and
//! one `Cluster` child at a time. Anything unexpected is an error and the caller falls back to plain seeking.
use std::io;

const EBML: u32 = 0x1A45_DFA3;
const SEGMENT: u32 = 0x1853_8067;
const SEEK_HEAD: u32 = 0x114D_9B74;
const SEEK: u32 = 0x4DBB;
const SEEK_ID: u32 = 0x53AB;
const SEEK_POSITION: u32 = 0x53AC;
const INFO: u32 = 0x1549_A966;
const TIMESTAMP_SCALE: u32 = 0x2A_D7B1;
const TRACKS: u32 = 0x1654_AE6B;
const TRACK_ENTRY: u32 = 0xAE;
const TRACK_NUMBER: u32 = 0xD7;
const TRACK_TYPE: u32 = 0x83;
const VIDEO: u32 = 0xE0;
const COLOUR: u32 = 0x55B0;
const TRANSFER: u32 = 0x55BA;
const CUES: u32 = 0x1C53_BB6B;
const CUE_POINT: u32 = 0xBB;
const CUE_TIME: u32 = 0xB3;
const CUE_TRACK_POSITIONS: u32 = 0xB7;
const CUE_TRACK: u32 = 0xF7;
const CUE_CLUSTER_POSITION: u32 = 0xF1;
const CUE_RELATIVE_POSITION: u32 = 0xF0;
const CLUSTER: u32 = 0x1F43_B675;
const CLUSTER_TIMESTAMP: u32 = 0xE7;
const SIMPLE_BLOCK: u32 = 0xA3;
const BLOCK_GROUP: u32 = 0xA0;
const BLOCK: u32 = 0xA1;

/// Bytes read per range request when scanning (headers, a cluster's first children).
const CHUNK: u64 = 256 * 1024;
/// Largest `Cues`, `Tracks` or keyframe block accepted (a malformed size must not allocate gigabytes).
const MAX_ELEMENT: u64 = 48 * 1024 * 1024;
/// How far into a cluster to look for the keyframe when the index has no relative position.
const MAX_CLUSTER_SCAN: u64 = 24 * 1024 * 1024;
/// A keyframe this close to its cluster start is read in the same request as the cluster header.
const NEAR_BLOCK: u64 = 256 * 1024;

/// Random access to the bytes of a file: HTTP range requests in the app, a local file in tests.
pub trait RangeSource {
    fn read_at(&mut self, offset: u64, len: u64) -> io::Result<Vec<u8>>;
}

fn bad(what: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, format!("matroska: {what}"))
}

/// An element header: id (with its length marker), payload size (`None` = unknown) and header length.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Header {
    id: u32,
    size: Option<u64>,
    len: usize,
}

fn read_id(b: &[u8]) -> Option<(u32, usize)> {
    let first = *b.first()?;
    let n = first.leading_zeros() as usize + 1;
    if n > 4 || b.len() < n {
        return None;
    }
    Some((b[..n].iter().fold(0u32, |a, &x| (a << 8) | u32::from(x)), n))
}

/// A variable-size integer (marker removed); `None` value = all ones (unknown size).
fn read_vint(b: &[u8]) -> Option<(Option<u64>, usize)> {
    let first = *b.first()?;
    let n = first.leading_zeros() as usize + 1;
    if n > 8 || b.len() < n {
        return None;
    }
    let mask = if n == 8 { 0 } else { 0xFFu8 >> n };
    let mut v = u64::from(first & mask);
    let mut all_ones = (first & mask) == mask;
    for &x in &b[1..n] {
        v = (v << 8) | u64::from(x);
        all_ones &= x == 0xFF;
    }
    Some((if all_ones { None } else { Some(v) }, n))
}

fn header(b: &[u8]) -> Option<Header> {
    let (id, a) = read_id(b)?;
    let (size, c) = read_vint(&b[a..])?;
    Some(Header { id, size, len: a + c })
}

fn uint(b: &[u8]) -> u64 {
    b.iter().take(8).fold(0u64, |a, &x| (a << 8) | u64::from(x))
}

/// Children of a fully read master element's payload: (id, payload slice, whole element slice).
fn children(mut b: &[u8]) -> impl Iterator<Item = (u32, &[u8], &[u8])> {
    std::iter::from_fn(move || {
        let h = header(b)?;
        let size = usize::try_from(h.size?).ok()?;
        let end = h.len.checked_add(size)?;
        if end > b.len() {
            return None;
        }
        let whole = &b[..end];
        let payload = &b[h.len..end];
        b = &b[end..];
        Some((h.id, payload, whole))
    })
}

/// Reads through a window over the source: the first read is `chunk` bytes, and a request past the window's end
/// fetches only the missing tail, so a keyframe whose size was guessed a little short costs one more small read
/// rather than a second full one.
struct Window<'a, S: RangeSource + ?Sized> {
    src: &'a mut S,
    start: u64,
    buf: Vec<u8>,
    chunk: u64,
    /// The end of the furthest byte asked for (what a later read of the same element needs).
    reach: u64,
    /// Range requests made.
    reads: u32,
    /// The last read came back short: the source ends at the window.
    eof: bool,
}

impl<'a, S: RangeSource + ?Sized> Window<'a, S> {
    fn new(src: &'a mut S) -> Self {
        Self::with_chunk(src, CHUNK)
    }

    fn with_chunk(src: &'a mut S, chunk: u64) -> Self {
        Self {
            src,
            start: 0,
            buf: Vec::new(),
            chunk: chunk.max(4096),
            reach: 0,
            reads: 0,
            eof: false,
        }
    }

    /// `len` bytes at `off` (fewer only at the end of the file).
    fn get(&mut self, off: u64, len: u64) -> io::Result<&[u8]> {
        let end = off.checked_add(len).ok_or_else(|| bad("offset overflow"))?;
        self.reach = self.reach.max(end);
        let have = self.start + self.buf.len() as u64;
        if off >= self.start && end > have && (off <= have && !self.buf.is_empty()) {
            if !self.eof {
                // Extend: only the missing bytes (at least a quarter chunk, so headers that follow are covered).
                let want = (end - have).max(self.chunk / 4);
                let more = self.src.read_at(have, want)?;
                self.reads += 1;
                self.eof = (more.len() as u64) < want;
                self.buf.extend_from_slice(&more);
            }
        } else if off < self.start || end > have {
            let want = len.max(self.chunk);
            self.buf = self.src.read_at(off, want)?;
            self.reads += 1;
            self.eof = (self.buf.len() as u64) < want;
            self.start = off;
        }
        let from = usize::try_from(off - self.start).map_err(|_| bad("offset"))?;
        let to = usize::try_from(len)
            .map_err(|_| bad("length"))?
            .min(self.buf.len().saturating_sub(from));
        Ok(&self.buf[from..from + to])
    }

    fn header_at(&mut self, off: u64) -> io::Result<Header> {
        header(self.get(off, 12)?).ok_or_else(|| bad("element header"))
    }

    /// A whole element's payload, bounded by `MAX_ELEMENT`.
    fn payload_at(&mut self, off: u64) -> io::Result<(Header, Vec<u8>)> {
        let h = self.header_at(off)?;
        let size = h.size.ok_or_else(|| bad("unknown-size element"))?;
        if size > MAX_ELEMENT {
            return Err(bad("element too large"));
        }
        let data = self.get(off + h.len as u64, size)?.to_vec();
        if data.len() as u64 != size {
            return Err(bad("truncated element"));
        }
        Ok((h, data))
    }
}

/// One index entry of the video track.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cue {
    /// Timestamp in track ticks (multiply by `MkvIndex::scale_ns`).
    pub time: u64,
    /// Cluster position relative to the segment payload.
    pub cluster: u64,
    /// Block position relative to the cluster payload, when the muxer wrote it.
    pub relative: Option<u64>,
}

/// What previews need to know about a Matroska file.
#[derive(Debug, Clone)]
pub struct MkvIndex {
    segment_start: u64,
    scale_ns: u64,
    track_number: u64,
    /// The video `TrackEntry` element, verbatim.
    track_entry: Vec<u8>,
    /// `TransferCharacteristics` of the video track (16 = PQ, 18 = HLG), when present.
    pub transfer: Option<u64>,
    cues: Vec<Cue>,
}

impl MkvIndex {
    /// Read the header, track list and index. Fails for non-Matroska data or files without a usable index.
    pub fn load(src: &mut (impl RangeSource + ?Sized)) -> io::Result<Self> {
        let mut w = Window::new(src);
        let ebml = w.header_at(0)?;
        if ebml.id != EBML {
            return Err(bad("not EBML"));
        }
        let seg_off = ebml.len as u64 + ebml.size.ok_or_else(|| bad("EBML size"))?;
        let seg = w.header_at(seg_off)?;
        if seg.id != SEGMENT {
            return Err(bad("no segment"));
        }
        let segment_start = seg_off + seg.len as u64;

        // Top-level children before the first cluster; SeekHead entries point at the rest (Cues are usually at the end).
        let mut found: Vec<(u32, u64)> = Vec::new();
        let mut off = segment_start;
        for _ in 0..64 {
            let h = match w.header_at(off) {
                Ok(h) => h,
                Err(_) => break,
            };
            if h.id == CLUSTER {
                break;
            }
            found.push((h.id, off));
            if h.id == SEEK_HEAD {
                let (_, data) = w.payload_at(off)?;
                for (id, seek, _) in children(&data) {
                    if id != SEEK {
                        continue;
                    }
                    let mut target = None;
                    let mut pos = None;
                    for (cid, v, _) in children(seek) {
                        match cid {
                            SEEK_ID => target = read_id(v).map(|(x, _)| x),
                            SEEK_POSITION => pos = Some(uint(v)),
                            _ => {}
                        }
                    }
                    if let (Some(t), Some(p)) = (target, pos) {
                        found.push((t, segment_start + p));
                    }
                }
            }
            let Some(size) = h.size else { break };
            off += h.len as u64 + size;
        }
        let at = |id: u32| found.iter().find(|(i, _)| *i == id).map(|(_, o)| *o);

        let mut scale_ns = 1_000_000;
        if let Some(o) = at(INFO) {
            let (_, data) = w.payload_at(o)?;
            if let Some((_, v, _)) = children(&data).find(|(id, _, _)| *id == TIMESTAMP_SCALE) {
                scale_ns = uint(v).max(1);
            }
        }

        let (_, tracks) = w.payload_at(at(TRACKS).ok_or_else(|| bad("no Tracks"))?)?;
        let (track_number, track_entry, transfer) = children(&tracks)
            .filter(|(id, _, _)| *id == TRACK_ENTRY)
            .find_map(|(_, entry, whole)| {
                let mut number = None;
                let mut kind = None;
                let mut transfer = None;
                for (id, v, _) in children(entry) {
                    match id {
                        TRACK_NUMBER => number = Some(uint(v)),
                        TRACK_TYPE => kind = Some(uint(v)),
                        VIDEO => {
                            transfer = children(v)
                                .filter(|(i, _, _)| *i == COLOUR)
                                .flat_map(|(_, c, _)| children(c))
                                .find(|(i, _, _)| *i == TRANSFER)
                                .map(|(_, t, _)| uint(t));
                        }
                        _ => {}
                    }
                }
                (kind == Some(1)).then_some((number?, whole.to_vec(), transfer))
            })
            .ok_or_else(|| bad("no video track"))?;

        let (_, cues_data) = w.payload_at(at(CUES).ok_or_else(|| bad("no Cues"))?)?;
        let mut cues: Vec<Cue> = Vec::new();
        for (id, point, _) in children(&cues_data) {
            if id != CUE_POINT {
                continue;
            }
            let mut time = None;
            for (cid, v, _) in children(point) {
                match cid {
                    CUE_TIME => time = Some(uint(v)),
                    CUE_TRACK_POSITIONS => {
                        let mut track = None;
                        let mut cluster = None;
                        let mut relative = None;
                        for (pid, pv, _) in children(v) {
                            match pid {
                                CUE_TRACK => track = Some(uint(pv)),
                                CUE_CLUSTER_POSITION => cluster = Some(uint(pv)),
                                CUE_RELATIVE_POSITION => relative = Some(uint(pv)),
                                _ => {}
                            }
                        }
                        if let (Some(t), Some(c), Some(tr)) = (time, cluster, track)
                            && tr == track_number
                        {
                            cues.push(Cue {
                                time: t,
                                cluster: c,
                                relative,
                            });
                        }
                    }
                    _ => {}
                }
            }
        }
        if cues.is_empty() {
            return Err(bad("no video cues"));
        }
        cues.sort_by_key(|c| c.time);
        cues.dedup_by_key(|c| c.time);
        Ok(Self {
            segment_start,
            scale_ns,
            track_number,
            track_entry,
            transfer,
            cues,
        })
    }

    pub fn cues(&self) -> &[Cue] {
        &self.cues
    }

    pub fn seconds(&self, cue: &Cue) -> f64 {
        cue.time as f64 * self.scale_ns as f64 / 1e9
    }

    /// The index entry at or before `seconds` (the first one before the first entry).
    pub fn cue_for(&self, seconds: f64) -> Option<Cue> {
        if !seconds.is_finite() {
            return None;
        }
        let ticks = (seconds.max(0.0) * 1e9 / self.scale_ns as f64) as u64;
        let i = self.cues.partition_point(|c| c.time <= ticks);
        self.cues.get(i.saturating_sub(1)).copied()
    }

    /// Whether the video track is PQ or HLG (the preview needs tone mapping).
    pub fn is_hdr(&self) -> bool {
        matches!(self.transfer, Some(16 | 18))
    }

    /// The keyframe's block payload (track number vint + timestamp + flags + frame data) for `cue`.
    /// The keyframe block for `cue` and where it starts. `guess` sizes the block read (see [`Self::frame_file_sized`]).
    /// Like AIOStreams' reader: a block near the cluster start comes in one request with the cluster header; one deep
    /// inside the cluster (audio blocks first, common in large remuxes) is read at its own offset, so the bytes before
    /// it are never downloaded.
    fn keyframe(
        &self,
        w: &mut Window<'_, impl RangeSource + ?Sized>,
        cue: &Cue,
        guess: u64,
    ) -> io::Result<(Vec<u8>, u64)> {
        let cluster_off = self.segment_start + cue.cluster;
        w.chunk = match cue.relative {
            Some(rel) if rel < NEAR_BLOCK => 12 + rel + guess,
            Some(_) => 12,
            None => guess.max(CHUNK * 2),
        };
        let cluster = w.header_at(cluster_off)?;
        if cluster.id != CLUSTER {
            return Err(bad("cue does not point at a cluster"));
        }
        let data_start = cluster_off + cluster.len as u64;
        w.chunk = guess.max(4096);
        if let Some(rel) = cue.relative
            && let Ok(Some(block)) = self.block_at(w, data_start + rel)
        {
            return Ok((block, data_start + rel));
        }
        // No (usable) relative position: walk the cluster's children to the first keyframe of the video track.
        let end = cluster.size.map_or(u64::MAX, |s| data_start + s);
        let mut off = data_start;
        while off < end && off - data_start < MAX_CLUSTER_SCAN {
            let h = w.header_at(off)?;
            if h.id == CLUSTER {
                break;
            }
            if let Some(block) = self.block_at(w, off)? {
                return Ok((block, off));
            }
            off += h.len as u64 + h.size.ok_or_else(|| bad("unknown-size cluster child"))?;
        }
        Err(bad("keyframe not found in cluster"))
    }

    /// The block at `off` if it is a SimpleBlock or BlockGroup keyframe of the video track.
    fn block_at<S: RangeSource + ?Sized>(&self, w: &mut Window<'_, S>, off: u64) -> io::Result<Option<Vec<u8>>> {
        let h = w.header_at(off)?;
        let size = h.size.ok_or_else(|| bad("unknown-size block"))?;
        let payload_off = off + h.len as u64;
        let block = match h.id {
            SIMPLE_BLOCK => {
                // Peek at the track number before fetching a block that may be megabytes of another track.
                if !self.is_video_block(w.get(payload_off, 8)?) {
                    return Ok(None);
                }
                w.payload_at(off)?.1
            }
            BLOCK_GROUP => {
                if size > MAX_ELEMENT {
                    return Ok(None);
                }
                let (_, group) = w.payload_at(off)?;
                match children(&group).find(|(id, _, _)| *id == BLOCK) {
                    Some((_, b, _)) if self.is_video_block(b) => b.to_vec(),
                    _ => return Ok(None),
                }
            }
            CLUSTER_TIMESTAMP => return Ok(None),
            _ => return Ok(None),
        };
        let (_, n) = read_vint(&block).ok_or_else(|| bad("block track"))?;
        let flags = *block.get(n + 2).ok_or_else(|| bad("short block"))?;
        if flags & 0x06 != 0 {
            return Err(bad("laced video block"));
        }
        Ok(Some(block))
    }

    fn is_video_block(&self, b: &[u8]) -> bool {
        read_vint(b).is_some_and(|(n, _)| n == Some(self.track_number))
    }

    /// A complete single-frame Matroska file holding the keyframe for `cue`.
    pub fn frame_file(&self, src: &mut (impl RangeSource + ?Sized), cue: &Cue) -> io::Result<Vec<u8>> {
        self.frame_file_sized(src, cue, CHUNK).map(|f| f.file)
    }

    /// Like [`Self::frame_file`], reading `first` bytes of the keyframe block in its request (a guess of the
    /// keyframe's size, so most keyframes take one request), and reporting what the block really needed.
    pub fn frame_file_sized(
        &self,
        src: &mut (impl RangeSource + ?Sized),
        cue: &Cue,
        first: u64,
    ) -> io::Result<FrameFile> {
        let mut w = Window::with_chunk(src, first);
        let (block, at) = self.keyframe(&mut w, cue, first)?;
        let needed = w.reach.saturating_sub(at);
        let reads = w.reads;
        let (_, n) = read_vint(&block).ok_or_else(|| bad("block track"))?;
        let mut simple = Vec::with_capacity(block.len());
        simple.extend_from_slice(&block[..n]);
        simple.extend_from_slice(&[0, 0, 0x80]); // timestamp 0, keyframe
        simple.extend_from_slice(block.get(n + 3..).ok_or_else(|| bad("short block"))?);
        Ok(FrameFile {
            file: single_frame_file(self.scale_ns, &self.track_entry, &simple),
            needed,
            reads,
        })
    }
}

/// A single-frame file and what reading its keyframe cost.
pub struct FrameFile {
    pub file: Vec<u8>,
    /// Bytes the keyframe block needed from its own start (to size the next block read).
    pub needed: u64,
    /// Range requests made.
    pub reads: u32,
}

fn put_id(out: &mut Vec<u8>, id: u32) {
    let bytes = id.to_be_bytes();
    let skip = bytes.iter().take_while(|&&b| b == 0).count().min(3);
    out.extend_from_slice(&bytes[skip..]);
}

/// Element with an 8-byte size field (simple and always valid).
fn element(id: u32, payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(payload.len() + 12);
    put_id(&mut out, id);
    out.push(0x01);
    out.extend_from_slice(&(payload.len() as u64).to_be_bytes()[1..]);
    out.extend_from_slice(payload);
    out
}

fn uint_element(id: u32, v: u64) -> Vec<u8> {
    let bytes = v.to_be_bytes();
    let skip = bytes.iter().take_while(|&&b| b == 0).count().min(7);
    element(id, &bytes[skip..])
}

fn single_frame_file(scale_ns: u64, track_entry: &[u8], simple_block: &[u8]) -> Vec<u8> {
    let ebml = [
        uint_element(0x4286, 1),
        uint_element(0x42F7, 1),
        uint_element(0x42F2, 4),
        uint_element(0x42F3, 8),
        element(0x4282, b"matroska"),
        uint_element(0x4287, 4),
        uint_element(0x4285, 2),
    ]
    .concat();
    let info = [
        uint_element(TIMESTAMP_SCALE, scale_ns),
        element(0x4D80, b"granary"),
        element(0x5741, b"granary"),
    ]
    .concat();
    let cluster = [uint_element(CLUSTER_TIMESTAMP, 0), element(SIMPLE_BLOCK, simple_block)].concat();
    let segment = [
        element(INFO, &info),
        element(TRACKS, track_entry),
        element(CLUSTER, &cluster),
    ]
    .concat();
    [element(EBML, &ebml), element(SEGMENT, &segment)].concat()
}

#[cfg(test)]
#[allow(clippy::unwrap_used)] // test code
mod tests {
    use super::*;

    struct Mem(Vec<u8>, usize);

    impl RangeSource for Mem {
        fn read_at(&mut self, offset: u64, len: u64) -> io::Result<Vec<u8>> {
            self.1 += 1;
            let from = (offset as usize).min(self.0.len());
            let to = from.saturating_add(len as usize).min(self.0.len());
            Ok(self.0[from..to].to_vec())
        }
    }

    fn video_entry(number: u64, transfer: u64) -> Vec<u8> {
        let colour = element(COLOUR, &uint_element(TRANSFER, transfer));
        element(
            TRACK_ENTRY,
            &[
                uint_element(TRACK_NUMBER, number),
                uint_element(TRACK_TYPE, 1),
                element(VIDEO, &colour),
            ]
            .concat(),
        )
    }

    fn block(track: u8, payload: &[u8], key: bool) -> Vec<u8> {
        [&[0x80 | track, 0, 0, if key { 0x80 } else { 0 }][..], payload].concat()
    }

    /// A small file: audio track 1, video track 2, two clusters, Cues after the clusters, found via SeekHead.
    fn sample(relative: bool) -> Vec<u8> {
        sample_with(relative, b"aaaa")
    }

    /// Like [`sample`], with `audio` as the audio block before the first video keyframe.
    fn sample_with(relative: bool, audio_payload: &[u8]) -> Vec<u8> {
        let audio = element(
            TRACK_ENTRY,
            &[uint_element(TRACK_NUMBER, 1), uint_element(TRACK_TYPE, 2)].concat(),
        );
        let tracks = element(TRACKS, &[audio, video_entry(2, 16)].concat());
        let info = element(INFO, &uint_element(TIMESTAMP_SCALE, 1_000_000));
        let c1 = element(
            CLUSTER,
            &[
                uint_element(CLUSTER_TIMESTAMP, 0),
                element(SIMPLE_BLOCK, &block(1, audio_payload, true)),
                element(SIMPLE_BLOCK, &block(2, b"KEY-ONE", true)),
            ]
            .concat(),
        );
        let c2 = element(
            CLUSTER,
            &[
                uint_element(CLUSTER_TIMESTAMP, 5000),
                element(BLOCK_GROUP, &element(BLOCK, &block(2, b"KEY-TWO", false))),
            ]
            .concat(),
        );
        // Layout inside the segment payload: SeekHead, Info, Tracks, c1, c2, Cues.
        let build = |cues_pos: u64| {
            let entry = element(
                SEEK,
                &[
                    element(SEEK_ID, &CUES.to_be_bytes()),
                    element(SEEK_POSITION, &cues_pos.to_be_bytes()),
                ]
                .concat(),
            );
            element(SEEK_HEAD, &entry)
        };
        let head_len = build(0).len(); // fixed-width fields keep the length independent of the value
        let c1_pos = (head_len + info.len() + tracks.len()) as u64;
        let c2_pos = c1_pos + c1.len() as u64;
        let cues_pos = c2_pos + c2.len() as u64;
        let point = |t: u64, c: u64, rel: Option<u64>| {
            let mut pos = vec![uint_element(CUE_TRACK, 2), uint_element(CUE_CLUSTER_POSITION, c)];
            if let Some(r) = rel {
                pos.push(uint_element(CUE_RELATIVE_POSITION, r));
            }
            element(
                CUE_POINT,
                &[uint_element(CUE_TIME, t), element(CUE_TRACK_POSITIONS, &pos.concat())].concat(),
            )
        };
        // Relative positions: in c1 the video block follows the 9+1 byte timestamp and the audio block.
        let ts_len = uint_element(CLUSTER_TIMESTAMP, 0).len() as u64;
        let audio_len = element(SIMPLE_BLOCK, &block(1, audio_payload, true)).len() as u64;
        let cues = element(
            CUES,
            &[
                point(0, c1_pos, relative.then_some(ts_len + audio_len)),
                point(5000, c2_pos, None),
            ]
            .concat(),
        );
        let segment = [build(cues_pos), info, tracks, c1, c2, cues].concat();
        [element(EBML, &uint_element(0x4286, 1)), element(SEGMENT, &segment)].concat()
    }

    #[test]
    fn vints_and_ids() {
        assert_eq!(read_vint(&[0x81]), Some((Some(1), 1)));
        assert_eq!(read_vint(&[0x40, 0x02]), Some((Some(2), 2)));
        assert_eq!(read_vint(&[0xFF]), Some((None, 1)));
        assert_eq!(read_vint(&[0x01, 0, 0, 0, 0, 0, 1, 0]), Some((Some(256), 8)));
        assert_eq!(read_id(&[0x1A, 0x45, 0xDF, 0xA3]), Some((EBML, 4)));
        assert_eq!(read_id(&[0xA3]), Some((SIMPLE_BLOCK, 1)));
        assert_eq!(read_vint(&[0x00]), None);
    }

    #[test]
    fn window_extends_with_the_missing_tail_only() {
        let mut src = Mem((0..20_000u32).map(|i| i as u8).collect(), 0);
        let mut w = Window::with_chunk(&mut src, 4096);
        assert_eq!(w.get(0, 10).unwrap().len(), 10);
        assert_eq!(w.get(4000, 1000).unwrap()[0], (4000u32 % 256) as u8);
        assert_eq!((w.reads, w.buf.len()), (2, 4096 + 1024));
        w.get(100, 50).unwrap();
        assert_eq!(w.reads, 2, "inside the window: no request");
        assert_eq!(w.get(10_000, 10).unwrap()[0], (10_000u32 % 256) as u8);
        assert_eq!((w.reads, w.reach), (3, 10_010));
        let mut short = Mem(vec![7; 5000], 0);
        let mut w = Window::with_chunk(&mut short, 4096);
        w.get(0, 10).unwrap();
        assert_eq!(w.get(4090, 2000).unwrap().len(), 910);
        assert_eq!(w.get(4990, 100).unwrap().len(), 10);
        assert_eq!(w.reads, 2, "the end of the source is not asked for again");
    }

    #[test]
    fn index_and_keyframes_from_a_sample_file() {
        for relative in [true, false] {
            let mut src = Mem(sample(relative), 0);
            let idx = MkvIndex::load(&mut src).unwrap();
            assert_eq!(idx.track_number, 2);
            assert!(idx.is_hdr());
            assert_eq!(idx.cues().len(), 2);
            assert_eq!(idx.cue_for(3.0).unwrap().time, 0);
            assert_eq!(idx.cue_for(7.5).unwrap().time, 5000);
            assert_eq!(idx.seconds(&idx.cue_for(99.0).unwrap()), 5.0);
            assert_eq!(idx.cue_for(f64::NAN), None);
            for (t, want) in [(0.0, &b"KEY-ONE"[..]), (6.0, &b"KEY-TWO"[..])] {
                let cue = idx.cue_for(t).unwrap();
                let file = idx.frame_file(&mut src, &cue).unwrap();
                // The single-frame file parses as Matroska with the same video track and one keyframe block.
                let mut again = Mem(file.clone(), 0);
                let w = Window::new(&mut again).header_at(0).unwrap();
                assert_eq!(w.id, EBML);
                let tail = &file[file.len() - want.len() - 4..];
                assert_eq!(&tail[..4], &[0x82, 0, 0, 0x80]);
                assert_eq!(&tail[4..], want);
            }
        }
    }

    /// Counts the bytes a read returned.
    struct Counted(Mem, u64);

    impl RangeSource for Counted {
        fn read_at(&mut self, offset: u64, len: u64) -> io::Result<Vec<u8>> {
            let b = self.0.read_at(offset, len)?;
            self.1 += b.len() as u64;
            Ok(b)
        }
    }

    #[test]
    fn a_keyframe_deep_in_its_cluster_is_read_at_its_own_offset() {
        // 600 KiB of audio before the video keyframe: none of it is downloaded, and the size learned is the block's.
        let data = sample_with(true, &vec![b'a'; 600 * 1024]);
        let idx = MkvIndex::load(&mut Mem(data.clone(), 0)).unwrap();
        let cue = idx.cue_for(0.0).unwrap();
        let mut src = Counted(Mem(data, 0), 0);
        let f = idx.frame_file_sized(&mut src, &cue, 4096).unwrap();
        assert!(src.1 < 16 * 1024, "read {} bytes", src.1);
        assert!(f.needed < 64, "needed {}", f.needed);
        assert!(f.file.ends_with(b"KEY-ONE"));
    }

    #[test]
    fn rejects_other_data() {
        assert!(MkvIndex::load(&mut Mem(b"\x00\x00\x00\x18ftypisom".to_vec(), 0)).is_err());
        assert!(MkvIndex::load(&mut Mem(Vec::new(), 0)).is_err());
    }
}
