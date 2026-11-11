//! Splits an HEVC keyframe made of several independent slices into one small picture per slice (ADR-0009,
//! revision 7).
//!
//! FFmpeg decodes the slices of a picture one after the other unless the stream uses wavefronts, so a 4K keyframe of
//! a disc remux (four slices, 1–4 MB) takes several hundred milliseconds on one core however many threads the
//! decoder has. Intra prediction never crosses a slice boundary, though: a slice that starts at the left edge of a
//! CTB row is, bit for bit, the slice data of a picture that begins at that row. So each slice gets a sequence
//! parameter set with its own height and a slice header that says "first slice of the picture", and the strips are
//! decoded side by side and stacked. With the in-loop filters off (they do cross slice boundaries, and previews skip
//! them anyway) the samples are the same as in the whole picture.
//!
//! The idea is the one AIOStreams' thumbnailer uses; this is an independent implementation from the syntax tables of
//! ITU-T H.265 (07/2024): 7.3.1.2 (NAL unit header), 7.3.2.2 (SPS), 7.3.2.3 (PPS), 7.3.4 (scaling lists), 7.3.6.1
//! (slice segment header) and 7.3.7 (short-term reference picture sets).
//!
//! Anything this code does not fully understand (tiles, wavefronts, dependent slices, separate colour planes,
//! multilayer or screen-content extensions, slices that do not start at a row) makes [`split`] return `None`, and the
//! caller decodes the picture whole.

const NAL_VPS: u8 = 32;
const NAL_SPS: u8 = 33;
const NAL_PPS: u8 = 34;
/// How much of a slice NAL is unescaped to read its header (an intra slice header is a few dozen bytes).
const HEADER_WINDOW: usize = 1024;

/// Parameter sets of a track and the size of its NAL length prefixes (`hvcC`, ISO/IEC 14496-15 8.3.3.1.2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParamSets {
    length_size: usize,
    /// VPS, SPS and PPS NAL units (with header, escaped), in file order.
    units: Vec<Vec<u8>>,
}

impl ParamSets {
    pub fn from_hvcc(b: &[u8]) -> Option<Self> {
        let length_size = usize::from(*b.get(21)? & 3) + 1;
        let arrays = *b.get(22)?;
        let mut at = 23;
        let mut units = Vec::new();
        for _ in 0..arrays {
            let count = u16::from_be_bytes([*b.get(at + 1)?, *b.get(at + 2)?]);
            at += 3;
            for _ in 0..count {
                let len = usize::from(u16::from_be_bytes([*b.get(at)?, *b.get(at + 1)?]));
                at += 2;
                let unit = b.get(at..at + len)?;
                at += len;
                if matches!(nal_type(unit), Some(NAL_VPS | NAL_SPS | NAL_PPS)) {
                    units.push(unit.to_vec());
                }
            }
        }
        Some(Self { length_size, units })
    }
}

/// One slice as a picture of its own.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Strip {
    /// Luma rows the strip shows (after the conformance window).
    pub rows: u32,
    /// Parameter sets and the slice, with the track's NAL length prefixes: a frame the track's decoder accepts.
    pub frame: Vec<u8>,
}

/// `nal_unit_type` of a base-layer NAL unit.
fn nal_type(unit: &[u8]) -> Option<u8> {
    let (a, b) = (*unit.first()?, *unit.get(1)?);
    // forbidden_zero_bit, nal_unit_type(6), nuh_layer_id(6), nuh_temporal_id_plus1(3)
    let layer = ((a & 1) << 5) | (b >> 3);
    (a & 0x80 == 0 && layer == 0).then_some((a >> 1) & 0x3F)
}

/// The NAL units of a length-prefixed frame.
fn units(frame: &[u8], length_size: usize) -> Option<Vec<&[u8]>> {
    let mut out = Vec::new();
    let mut rest = frame;
    while !rest.is_empty() {
        let prefix = rest.get(..length_size)?;
        let len = prefix.iter().fold(0usize, |a, &x| (a << 8) | usize::from(x));
        out.push(rest.get(length_size..length_size + len)?);
        rest = &rest[length_size + len..];
    }
    Some(out)
}

/// Removes emulation prevention bytes; also the position in `unit` of every byte kept.
fn unescape(unit: &[u8]) -> (Vec<u8>, Vec<usize>) {
    let mut bytes = Vec::with_capacity(unit.len());
    let mut origin = Vec::with_capacity(unit.len());
    let mut zeros = 0u32;
    for (i, &b) in unit.iter().enumerate() {
        if zeros >= 2 && b == 3 {
            zeros = 0;
            continue;
        }
        zeros = if b == 0 { zeros + 1 } else { 0 };
        bytes.push(b);
        origin.push(i);
    }
    (bytes, origin)
}

/// Appends `rbsp` with emulation prevention bytes.
fn escape_into(out: &mut Vec<u8>, rbsp: &[u8]) {
    let mut zeros = 0u32;
    for &b in rbsp {
        if zeros >= 2 && b <= 3 {
            out.push(3);
            zeros = 0;
        }
        zeros = if b == 0 { zeros + 1 } else { 0 };
        out.push(b);
    }
}

struct Bits<'a> {
    data: &'a [u8],
    at: usize,
}

impl<'a> Bits<'a> {
    /// Positioned after the two-byte NAL unit header.
    fn after_header(data: &'a [u8]) -> Self {
        Self { data, at: 16 }
    }

    fn flag(&mut self) -> Option<bool> {
        let byte = *self.data.get(self.at / 8)?;
        let bit = byte >> (7 - self.at % 8) & 1;
        self.at += 1;
        Some(bit == 1)
    }

    fn bits(&mut self, n: u32) -> Option<u32> {
        (0..n).try_fold(0u32, |v, _| Some(v << 1 | u32::from(self.flag()?)))
    }

    fn skip(&mut self, n: usize) -> Option<()> {
        self.at = self.at.checked_add(n)?;
        (self.at <= self.data.len() * 8).then_some(())
    }

    /// ue(v), 9.2.
    fn ue(&mut self) -> Option<u32> {
        let mut zeros = 0;
        while !self.flag()? {
            zeros += 1;
            if zeros > 31 {
                return None;
            }
        }
        let tail = self.bits(zeros)?;
        u32::try_from((1u64 << zeros) - 1 + u64::from(tail)).ok()
    }

    /// se(v): only ever skipped here.
    fn se(&mut self) -> Option<()> {
        self.ue().map(|_| ())
    }
}

#[derive(Default)]
struct BitWriter {
    out: Vec<u8>,
    len: usize,
}

impl BitWriter {
    fn flag(&mut self, v: bool) {
        if self.len.is_multiple_of(8) {
            self.out.push(0);
        }
        if v && let Some(last) = self.out.last_mut() {
            *last |= 0x80 >> (self.len % 8);
        }
        self.len += 1;
    }

    fn ue(&mut self, v: u32) {
        let x = u64::from(v) + 1;
        let width = 64 - x.leading_zeros();
        for _ in 1..width {
            self.flag(false);
        }
        for i in (0..width).rev() {
            self.flag(x >> i & 1 == 1);
        }
    }

    /// Bits `from..to` of `src`.
    fn copy(&mut self, src: &[u8], from: usize, to: usize) {
        for p in from..to {
            self.flag(src[p / 8] >> (7 - p % 8) & 1 == 1);
        }
    }

    /// A one bit, then zeros to the byte boundary (`rbsp_trailing_bits` and `byte_alignment`).
    fn finish(mut self) -> Vec<u8> {
        self.flag(true);
        while !self.len.is_multiple_of(8) {
            self.flag(false);
        }
        self.out
    }
}

/// Bits needed to code values below `n` (`Ceil(Log2(n))`).
fn width_for(n: u32) -> u32 {
    if n <= 1 { 0 } else { 32 - (n - 1).leading_zeros() }
}

/// scaling_list_data(), 7.3.4.
fn skip_scaling_lists(r: &mut Bits) -> Option<()> {
    for size_id in 0..4u32 {
        let step = if size_id == 3 { 3 } else { 1 };
        for _ in (0..6).step_by(step) {
            if !r.flag()? {
                r.ue()?; // scaling_list_pred_matrix_id_delta
                continue;
            }
            if size_id > 1 {
                r.se()?; // scaling_list_dc_coef_minus8
            }
            for _ in 0..64.min(1u32 << (4 + (size_id << 1))) {
                r.se()?;
            }
        }
    }
    Some(())
}

/// st_ref_pic_set(idx), 7.3.7: skips it and returns its number of entries (`NumDeltaPocs`), which a later set
/// predicted from it needs. `earlier` holds the entry counts of the sets before it.
fn skip_ref_pic_set(r: &mut Bits, idx: usize, total: usize, earlier: &[u32]) -> Option<u32> {
    let predicted = idx != 0 && r.flag()?;
    if !predicted {
        let negative = r.ue()?;
        let positive = r.ue()?;
        for _ in 0..negative.checked_add(positive)? {
            r.ue()?; // delta_poc_s*_minus1
            r.flag()?; // used_by_curr_pic_s*_flag
        }
        return Some(negative + positive);
    }
    let back = if idx == total { r.ue()? as usize + 1 } else { 1 };
    r.flag()?; // delta_rps_sign
    r.ue()?; // abs_delta_rps_minus1
    let reference = *earlier.get(idx.checked_sub(back)?)?;
    let mut entries = 0;
    for _ in 0..=reference {
        let used = r.flag()?;
        // use_delta_flag is 1 when absent.
        if used || r.flag()? {
            entries += 1;
        }
    }
    Some(entries)
}

/// What the splitter needs from a sequence parameter set.
struct Sps {
    id: u32,
    rbsp: Vec<u8>,
    /// Bit positions of `pic_width_in_luma_samples` and of the first bit after the conformance window.
    size_at: usize,
    window_end: usize,
    width: u32,
    height: u32,
    /// Conformance window offsets (left, right, top, bottom) in chroma sample units.
    window: [u32; 4],
    /// Luma rows per unit of a vertical window offset (`SubHeightC`).
    sub_height: u32,
    ctb: u32,
    separate_planes: bool,
    chroma_array_type: u32,
    poc_bits: u32,
    ref_pic_sets: Vec<u32>,
    long_term: bool,
    long_term_in_sps: u32,
    temporal_mvp: bool,
    sao: bool,
}

/// seq_parameter_set_rbsp(), 7.3.2.2, up to `sps_temporal_mvp_enabled_flag`.
fn parse_sps(rbsp: Vec<u8>) -> Option<Sps> {
    let mut r = Bits::after_header(&rbsp);
    r.bits(4)?; // sps_video_parameter_set_id
    let sub_layers = r.bits(3)? as usize; // sps_max_sub_layers_minus1
    r.flag()?; // sps_temporal_id_nesting_flag
    // profile_tier_level(1, sub_layers), 7.3.3: 88 bits of general profile, 8 of level.
    r.skip(96)?;
    let mut present = Vec::with_capacity(sub_layers);
    for _ in 0..sub_layers {
        present.push((r.flag()?, r.flag()?));
    }
    if sub_layers > 0 {
        r.skip(2 * (8 - sub_layers))?;
    }
    for (profile, level) in present {
        r.skip(usize::from(profile) * 88 + usize::from(level) * 8)?;
    }
    let id = r.ue()?;
    let chroma_format = r.ue()?;
    let separate_planes = chroma_format == 3 && r.flag()?;
    let size_at = r.at;
    let width = r.ue()?;
    let height = r.ue()?;
    let mut window = [0u32; 4];
    if r.flag()? {
        for w in &mut window {
            *w = r.ue()?;
        }
    }
    let window_end = r.at;
    r.ue()?; // bit_depth_luma_minus8
    r.ue()?; // bit_depth_chroma_minus8
    let poc_bits = r.ue()?.checked_add(4).filter(|&b| b <= 16)?;
    let every_layer = r.flag()?; // sps_sub_layer_ordering_info_present_flag
    for _ in (if every_layer { 0 } else { sub_layers })..=sub_layers {
        r.ue()?;
        r.ue()?;
        r.ue()?;
    }
    let min_cb_log2 = r.ue()?.checked_add(3)?;
    let ctb_log2 = min_cb_log2.checked_add(r.ue()?).filter(|&l| (4..=6).contains(&l))?;
    for _ in 0..4 {
        r.ue()?; // transform block sizes and hierarchy depths
    }
    if r.flag()? && r.flag()? {
        skip_scaling_lists(&mut r)?;
    }
    r.flag()?; // amp_enabled_flag
    let sao = r.flag()?;
    if r.flag()? {
        // pcm: two 4-bit depths, two block sizes, loop filter flag
        r.bits(8)?;
        r.ue()?;
        r.ue()?;
        r.flag()?;
    }
    let count = r.ue()? as usize;
    if count > 64 {
        return None;
    }
    let mut ref_pic_sets = Vec::with_capacity(count);
    for i in 0..count {
        let entries = skip_ref_pic_set(&mut r, i, count, &ref_pic_sets)?;
        ref_pic_sets.push(entries);
    }
    let long_term = r.flag()?;
    let mut long_term_in_sps = 0;
    if long_term {
        long_term_in_sps = r.ue()?;
        for _ in 0..long_term_in_sps {
            r.bits(poc_bits)?;
            r.flag()?;
        }
    }
    let temporal_mvp = r.flag()?;
    if width == 0 || height == 0 {
        return None;
    }
    Some(Sps {
        id,
        size_at,
        window_end,
        width,
        height,
        window,
        sub_height: if chroma_format == 1 { 2 } else { 1 },
        ctb: 1 << ctb_log2,
        separate_planes,
        chroma_array_type: if separate_planes { 0 } else { chroma_format },
        poc_bits,
        ref_pic_sets,
        long_term,
        long_term_in_sps,
        temporal_mvp,
        sao,
        rbsp,
    })
}

impl Sps {
    /// The same parameter set for a picture `height` luma rows tall with the given window offsets at its top and
    /// bottom; everything after the window is copied bit for bit. Returns the NAL unit (escaped).
    fn for_strip(&self, height: u32, top: u32, bottom: u32) -> Option<Vec<u8>> {
        let mut w = BitWriter::default();
        w.copy(&self.rbsp, 0, self.size_at);
        w.ue(self.width);
        w.ue(height);
        let window = [self.window[0], self.window[1], top, bottom];
        w.flag(window != [0; 4]);
        if window != [0; 4] {
            for v in window {
                w.ue(v);
            }
        }
        // Up to the stop bit of rbsp_trailing_bits (the last one bit).
        let last = self.rbsp.iter().rposition(|&b| b != 0)?;
        let stop = last * 8 + 7 - self.rbsp[last].trailing_zeros() as usize;
        w.copy(&self.rbsp, self.window_end, stop.max(self.window_end));
        let mut unit = Vec::with_capacity(self.rbsp.len() + 8);
        escape_into(&mut unit, &w.finish());
        Some(unit)
    }
}

/// What the splitter needs from a picture parameter set.
struct Pps {
    id: u32,
    sps: u32,
    dependent_slices: bool,
    output_flag: bool,
    extra_header_bits: u32,
    chroma_qp_offsets: bool,
    loop_filter_across_slices: bool,
    deblocking_override: bool,
    deblocking_disabled: bool,
    header_extension: bool,
    chroma_qp_offset_list: bool,
}

/// pic_parameter_set_rbsp(), 7.3.2.3. `None` for tiles, wavefronts and extensions other than the range extension.
fn parse_pps(rbsp: &[u8]) -> Option<Pps> {
    let mut r = Bits::after_header(rbsp);
    let id = r.ue()?;
    let sps = r.ue()?;
    let dependent_slices = r.flag()?;
    let output_flag = r.flag()?;
    let extra_header_bits = r.bits(3)?;
    r.flag()?; // sign_data_hiding_enabled_flag
    r.flag()?; // cabac_init_present_flag
    r.ue()?; // num_ref_idx_l0_default_active_minus1
    r.ue()?; // num_ref_idx_l1_default_active_minus1
    r.se()?; // init_qp_minus26
    r.flag()?; // constrained_intra_pred_flag
    let transform_skip = r.flag()?;
    if r.flag()? {
        r.ue()?; // diff_cu_qp_delta_depth
    }
    r.se()?; // pps_cb_qp_offset
    r.se()?; // pps_cr_qp_offset
    let chroma_qp_offsets = r.flag()?;
    r.flag()?; // weighted_pred_flag
    r.flag()?; // weighted_bipred_flag
    r.flag()?; // transquant_bypass_enabled_flag
    let tiles = r.flag()?;
    let wavefronts = r.flag()?;
    if tiles || wavefronts {
        return None;
    }
    let loop_filter_across_slices = r.flag()?;
    let (mut deblocking_override, mut deblocking_disabled) = (false, false);
    if r.flag()? {
        deblocking_override = r.flag()?;
        deblocking_disabled = r.flag()?;
        if !deblocking_disabled {
            r.se()?;
            r.se()?;
        }
    }
    if r.flag()? {
        skip_scaling_lists(&mut r)?;
    }
    r.flag()?; // lists_modification_present_flag
    r.ue()?; // log2_parallel_merge_level_minus2
    let header_extension = r.flag()?;
    let mut chroma_qp_offset_list = false;
    if r.flag()? {
        let range = r.flag()?;
        let others = r.bits(3)?; // multilayer, 3D, screen content
        r.bits(4)?;
        if others != 0 {
            return None;
        }
        if range {
            if transform_skip {
                r.ue()?; // log2_max_transform_skip_block_size_minus2
            }
            r.flag()?; // cross_component_prediction_enabled_flag
            chroma_qp_offset_list = r.flag()?;
        }
    }
    Some(Pps {
        id,
        sps,
        dependent_slices,
        output_flag,
        extra_header_bits,
        chroma_qp_offsets,
        loop_filter_across_slices,
        deblocking_override,
        deblocking_disabled,
        header_extension,
        chroma_qp_offset_list,
    })
}

/// A slice rewritten as the first slice of a picture, and the CTB row it started at.
struct Slice<'a> {
    row: u32,
    pps: &'a [u8],
    unit: Vec<u8>,
}

/// slice_segment_header(), 7.3.6.1, for an intra slice of an IRAP picture: drops the slice address and sets
/// `first_slice_segment_in_pic_flag`.
fn rewrite_slice<'a>(
    unit: &[u8],
    kind: u8,
    spss: &'a [Sps],
    ppss: &'a [(Pps, &'a [u8])],
    used: &mut Option<u32>,
) -> Option<(Slice<'a>, &'a Sps)> {
    let (rbsp, origin) = unescape(&unit[..unit.len().min(HEADER_WINDOW)]);
    let mut r = Bits::after_header(&rbsp);
    let first = r.flag()?;
    r.flag()?; // no_output_of_prior_pics_flag (IRAP)
    let pps_id = r.ue()?;
    let (pps, pps_unit) = ppss.iter().find(|(p, _)| p.id == pps_id)?;
    let sps = spss.iter().find(|s| s.id == pps.sps)?;
    if sps.separate_planes || used.is_some_and(|id| id != sps.id) {
        return None;
    }
    *used = Some(sps.id);
    let across = sps.width.div_ceil(sps.ctb);
    let mut address = 0;
    if !first {
        if pps.dependent_slices && r.flag()? {
            return None;
        }
        address = r.bits(width_for(across.checked_mul(sps.height.div_ceil(sps.ctb))?))?;
    }
    if !address.is_multiple_of(across) {
        return None;
    }
    let rest = r.at;
    r.bits(pps.extra_header_bits)?;
    if r.ue()? != 2 {
        return None; // not an I slice
    }
    if pps.output_flag {
        r.flag()?;
    }
    // IDR_W_RADL and IDR_N_LP carry no picture order count or reference sets.
    if kind != 19 && kind != 20 {
        r.bits(sps.poc_bits)?;
        let sets = sps.ref_pic_sets.len();
        if !r.flag()? {
            skip_ref_pic_set(&mut r, sets, sets, &sps.ref_pic_sets)?;
        } else if sets > 1 {
            r.bits(width_for(u32::try_from(sets).ok()?))?;
        }
        if sps.long_term {
            let from_sps = if sps.long_term_in_sps > 0 { r.ue()? } else { 0 };
            let own = r.ue()?;
            for i in 0..from_sps.checked_add(own)? {
                if i < from_sps {
                    r.bits(width_for(sps.long_term_in_sps))?;
                } else {
                    r.bits(sps.poc_bits)?;
                    r.flag()?;
                }
                if r.flag()? {
                    r.ue()?; // delta_poc_msb_cycle_lt
                }
            }
        }
        if sps.temporal_mvp {
            r.flag()?;
        }
    }
    let mut sao = false;
    if sps.sao {
        sao = r.flag()?;
        if sps.chroma_array_type != 0 {
            sao |= r.flag()?;
        }
    }
    r.se()?; // slice_qp_delta
    if pps.chroma_qp_offsets {
        r.se()?;
        r.se()?;
    }
    if pps.chroma_qp_offset_list {
        r.flag()?;
    }
    let mut deblocking_disabled = pps.deblocking_disabled;
    if pps.deblocking_override && r.flag()? {
        deblocking_disabled = r.flag()?;
        if !deblocking_disabled {
            r.se()?;
            r.se()?;
        }
    }
    if pps.loop_filter_across_slices && (sao || !deblocking_disabled) {
        r.flag()?;
    }
    if pps.header_extension {
        let bytes = r.ue()? as usize;
        r.skip(bytes.checked_mul(8)?)?;
    }
    let header_end = r.at;
    // byte_alignment(): a one bit, then zeros; the slice data starts at the next byte.
    let data = (header_end + 1).div_ceil(8);
    // Re-escape the new header together with the slice data up to its first non-zero byte: after a non-zero byte
    // the escaping of what follows no longer depends on what came before, so the rest is copied as it is.
    let anchor = (data..rbsp.len()).find(|&i| rbsp[i] != 0)?;
    let mut w = BitWriter::default();
    w.copy(&rbsp, 0, 16);
    w.flag(true);
    w.copy(&rbsp, 17, 18);
    w.ue(pps_id);
    w.copy(&rbsp, rest, header_end);
    let mut head = w.finish();
    head.extend_from_slice(&rbsp[data..=anchor]);
    let tail = unit.get(origin.get(anchor)? + 1..)?;
    let mut out = Vec::with_capacity(head.len() + tail.len() + 8);
    escape_into(&mut out, &head);
    out.extend_from_slice(tail);
    Some((
        Slice {
            row: address / across,
            pps: pps_unit,
            unit: out,
        },
        sps,
    ))
}

/// One [`Strip`] per slice of the keyframe `frame` (NAL units with length prefixes), top to bottom; `None` when
/// the picture has a single slice or cannot be split safely.
pub fn split(sets: &ParamSets, frame: &[u8]) -> Option<Vec<Strip>> {
    let in_band = units(frame, sets.length_size)?;
    let mut vps: Vec<&[u8]> = Vec::new();
    let mut spss: Vec<Sps> = Vec::new();
    let mut ppss: Vec<(Pps, &[u8])> = Vec::new();
    let mut slices: Vec<(u8, &[u8])> = Vec::new();
    // Parameter sets in the frame replace those of the track header with the same id (they come later).
    for unit in sets.units.iter().map(Vec::as_slice).chain(in_band.iter().copied()) {
        match nal_type(unit) {
            Some(NAL_VPS) => {
                vps.retain(|v| v != &unit);
                vps.push(unit);
            }
            Some(NAL_SPS) => {
                let sps = parse_sps(unescape(unit).0)?;
                spss.retain(|s| s.id != sps.id);
                spss.push(sps);
            }
            Some(NAL_PPS) => {
                let pps = parse_pps(&unescape(unit).0)?;
                ppss.retain(|(p, _)| p.id != pps.id);
                ppss.push((pps, unit));
            }
            // IRAP pictures only (BLA, IDR, CRA).
            Some(t @ 16..=21) => slices.push((t, unit)),
            Some(0..=31) => return None,
            _ => {}
        }
    }
    if slices.len() < 2 {
        return None;
    }
    let mut used = None;
    let mut parts = Vec::with_capacity(slices.len());
    let mut sps = None;
    for (kind, unit) in slices {
        let (slice, s) = rewrite_slice(unit, kind, &spss, &ppss, &mut used)?;
        sps = Some(s);
        parts.push(slice);
    }
    let sps = sps?;
    if parts[0].row != 0 {
        return None;
    }
    let count = parts.len();
    let mut strips = Vec::with_capacity(count);
    for (i, part) in parts.iter().enumerate() {
        let y = part.row.checked_mul(sps.ctb)?;
        // The last strip keeps the picture's own bottom edge: CTBs that cross it are coded differently.
        let end = match parts.get(i + 1) {
            Some(next) => next.row.checked_mul(sps.ctb)?,
            None => sps.height,
        };
        if end <= y || end > sps.height {
            return None;
        }
        let top = if i == 0 { sps.window[2] } else { 0 };
        let bottom = if i + 1 == count { sps.window[3] } else { 0 };
        let height = end - y;
        let rows = height.checked_sub(sps.sub_height.checked_mul(top.checked_add(bottom)?)?)?;
        if rows == 0 {
            return None;
        }
        let sps_unit = sps.for_strip(height, top, bottom)?;
        let mut frame = Vec::with_capacity(part.unit.len() + sps_unit.len() + 256);
        let mut put = |unit: &[u8]| -> Option<()> {
            let len = unit.len().to_be_bytes();
            let prefix = &len[len.len() - sets.length_size..];
            if len[..len.len() - sets.length_size].iter().any(|&b| b != 0) {
                return None;
            }
            frame.extend_from_slice(prefix);
            frame.extend_from_slice(unit);
            Some(())
        };
        for v in &vps {
            put(v)?;
        }
        put(&sps_unit)?;
        put(part.pps)?;
        put(&part.unit)?;
        strips.push(Strip { rows, frame });
    }
    Some(strips)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)] // test code
mod tests {
    use super::*;

    /// `tests/fixtures/hevc_slices.mkv`: 320x180 (coded 320x184, bottom window offset 2), 64-pixel CTBs, keyframes
    /// of three slices that start at CTB rows 0, 1 and 2.
    #[test]
    fn a_three_slice_keyframe_becomes_three_pictures() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/hevc_slices.mkv");
        let mut src = crate::range::FileRange::open(&path).unwrap();
        let index = crate::mkv::MkvIndex::load(&mut src).unwrap();
        let block = index
            .frame_file_sized(&mut src, &index.cues()[0], 1 << 20)
            .unwrap()
            .block;
        let sets = index.hevc_sets().unwrap();
        // Track number (one byte), timestamp, flags; then the frame.
        let frame = &block[4..];
        let strips = split(sets, frame).unwrap();
        assert_eq!(strips.iter().map(|s| s.rows).collect::<Vec<_>>(), [64, 64, 52]);
        for (strip, (height, bottom)) in strips.iter().zip([(64, 0), (64, 0), (56, 2)]) {
            let parts = units(&strip.frame, sets.length_size).unwrap();
            let sps = parts.iter().find(|u| nal_type(u) == Some(NAL_SPS)).unwrap();
            let sps = parse_sps(unescape(sps).0).unwrap();
            assert_eq!((sps.width, sps.height, sps.window), (320, height, [0, 0, 0, bottom]));
            assert_eq!(sps.ctb, 64);
            let slice = parts.last().unwrap();
            assert!(matches!(nal_type(slice), Some(16..=21)));
            assert_eq!(slice[2] & 0x80, 0x80, "first_slice_segment_in_pic_flag");
        }
        // The slice data is carried over byte for byte.
        let original = units(frame, sets.length_size).unwrap();
        let last = original.last().unwrap();
        let rewritten = units(&strips[2].frame, sets.length_size).unwrap();
        assert!(rewritten.last().unwrap().ends_with(&last[last.len() - 64..]));
        // A picture of one slice is left alone.
        assert!(split(sets, &strips[0].frame).is_none());
        assert!(
            split(sets, &frame[..frame.len() / 2]).is_none(),
            "a truncated frame is not split"
        );
    }

    #[test]
    fn exp_golomb_round_trips() {
        let mut w = BitWriter::default();
        let values = [0u32, 1, 2, 3, 7, 8, 255, 2159, 3840, 65_535, u32::MAX - 1];
        for v in values {
            w.ue(v);
        }
        let bytes = w.finish();
        let mut r = Bits { data: &bytes, at: 0 };
        for v in values {
            assert_eq!(r.ue(), Some(v));
        }
        // The stop bit follows.
        assert_eq!(r.flag(), Some(true));
    }

    #[test]
    fn escaping_round_trips() {
        let rbsp = [1, 0, 0, 0, 0, 1, 0, 0, 3, 0, 0, 2, 5, 0, 0];
        let mut escaped = Vec::new();
        escape_into(&mut escaped, &rbsp);
        assert_eq!(escaped, [1, 0, 0, 3, 0, 0, 3, 1, 0, 0, 3, 3, 0, 0, 3, 2, 5, 0, 0]);
        let (back, origin) = unescape(&escaped);
        assert_eq!(back, rbsp);
        assert_eq!(origin[3], 4);
    }

    #[test]
    fn widths_and_types() {
        assert_eq!(width_for(1), 0);
        assert_eq!(width_for(2), 1);
        assert_eq!(width_for(2040), 11);
        assert_eq!(width_for(2048), 11);
        assert_eq!(width_for(2049), 12);
        // IDR_W_RADL, layer 0, temporal id 1.
        assert_eq!(nal_type(&[19 << 1, 1]), Some(19));
        // The same in an enhancement layer is not ours.
        assert_eq!(nal_type(&[19 << 1, 1 | 1 << 3]), None);
    }

    #[test]
    fn reads_the_parameter_sets_of_a_track_header() {
        let mut hvcc = vec![0u8; 23];
        hvcc[21] = 0xFC | 3;
        hvcc[22] = 2;
        // One SPS-typed and one SEI-typed unit: only parameter sets are kept.
        hvcc.extend_from_slice(&[0x80 | NAL_SPS, 0, 1, 0, 3, NAL_SPS << 1, 1, 9]);
        hvcc.extend_from_slice(&[39, 0, 1, 0, 2, 39 << 1, 1]);
        let sets = ParamSets::from_hvcc(&hvcc).unwrap();
        assert_eq!(sets.length_size, 4);
        assert_eq!(sets.units, vec![vec![NAL_SPS << 1, 1, 9]]);
        assert!(ParamSets::from_hvcc(&hvcc[..25]).is_none());
        assert_eq!(
            units(&[0, 0, 0, 2, 7, 8, 0, 0, 0, 1, 9], 4).unwrap(),
            vec![&[7u8, 8][..], &[9][..]]
        );
        assert!(units(&[0, 0, 0, 9, 1], 4).is_none());
    }
}
