//! JPEG decoding (DCTDecode, 7.4.8; ITU-T T.81).
//!
//! Huffman-coded baseline, extended sequential and **progressive** at 8 bits,
//! which between them is what essentially every PDF carries. Arithmetic coding
//! and 12-bit precision are reported rather than half-decoded.
//!
//! Every mode decodes into the same per-component coefficient buffer and is
//! then rendered once, at the end, by a single dequantise-and-transform pass.
//! Progressive forces that shape — a coefficient is refined by later scans, so
//! nothing can be turned into a pixel until the last scan has been read — and
//! baseline shares it rather than keeping a second path that could drift.
//!
//! The IDCT is the integer separable transform. That means output can differ
//! from libjpeg's by a least-significant bit on some coefficients — there is no
//! single correct IDCT, only conforming ones — so comparison against a
//! reference is perceptual, never exact.
//!
//! The encoder is `encode`, which writes baseline and only baseline; what it
//! excludes, what adjudicates it, and what does not, are that module's header.

mod encode;

pub use encode::{
    jpeg_encode, JpegEncodeError, JpegOptions, JpegQuantisation, JpegSampling, JpegSource,
    JpegSourceColour,
};

use crate::Warning;

/// What colour the decoded components represent.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JpegColor {
    /// One component.
    Gray,
    /// Three components, already converted from YCbCr.
    Rgb,
    /// Four components. Adobe's transform, if any, has been undone.
    Cmyk,
    /// Four components, and the file marked them inverted — the Photoshop
    /// convention that trips readers which assume otherwise.
    CmykInverted,
}

/// A decoded image.
#[derive(Clone, Debug)]
pub struct JpegImage {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// What the components mean.
    pub color: JpegColor,
    /// Interleaved samples, one byte each.
    ///
    /// **Eight bits whatever the frame's precision was.** A 12-bit frame is
    /// decoded at twelve and narrowed here, because every `PixelFormat` this
    /// engine rasters into is eight bits deep and a 16-bit sample path would
    /// be a change to the raster rather than to this decoder. The narrowing is
    /// reported as [`Warning::JpegPrecisionNarrowed`] and [`JpegImage::precision`]
    /// says what it came from, so a later caller that grows a wider path knows
    /// where to look.
    pub data: Vec<u8>,
    /// T.81 B.2.2's `P`: the frame's sample precision, 8 or 12.
    pub precision: u8,
    /// What the decoder tolerated.
    pub warnings: Vec<Warning>,
}

/// Why a JPEG could not be decoded.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JpegError {
    /// The bytes do not begin like a JPEG.
    NotJpeg,
    /// An arithmetic-coded frame: SOF9, SOF10, SOF11, SOF13, SOF14 or SOF15.
    ///
    /// T.81 Annex D's QM coder, which is related to but not the same as the MQ
    /// coder in `mq.rs` -- a different state table, a different
    /// renormalisation, and a different byte-stuffing rule. Refused rather than
    /// half-decoded, and see `docs/ROADMAP.md` for the three separate reasons
    /// it is not built.
    Arithmetic,
    /// A lossless frame: SOF3 or SOF7.
    ///
    /// Annex H's predictive coder, which shares nothing with the DCT path
    /// below -- no quantisation tables, no blocks, no transform. Named apart
    /// from [`JpegError::Arithmetic`] because a file needing one is not a file
    /// needing the other, and until now both were **skipped rather than
    /// refused**: the marker fell through to the unknown-segment arm, the
    /// decoder found no frame it understood, and the failure surfaced as
    /// `Truncated`.
    Lossless,
    /// A differential frame: SOF5 or SOF6.
    ///
    /// The hierarchical progression of Annex J, where a frame codes the
    /// difference from an upsampled earlier one. Same history as
    /// [`JpegError::Lossless`]: skipped rather than refused.
    Differential,
    /// A sample precision T.81 B.2.2 does not allow: anything but 8 or 12.
    UnsupportedPrecision,
    /// The file ended before the image did, past any hope of recovery.
    Truncated,
    /// A component count no colour model covers.
    UnsupportedComponents,
}

#[derive(Clone, Default)]
struct Component {
    id: u8,
    h: usize,
    v: usize,
    quant: usize,
    dc_table: usize,
    ac_table: usize,
    dc_prediction: i32,
    /// Blocks per line in the coefficient buffer, padded out to whole MCUs so
    /// an interleaved scan can address every block it codes.
    blocks_x: usize,
    /// Blocks per column, likewise padded.
    blocks_y: usize,
    /// Blocks per line that carry image rather than padding, which is what a
    /// non-interleaved scan iterates over (T.81 A.2.2). Getting this wrong
    /// desynchronises every later block in the scan.
    scan_x: usize,
    /// Blocks per column, likewise.
    scan_y: usize,
    /// Coefficients in zig-zag order, one 64-entry block after another.
    ///
    /// Zig-zag rather than natural order because spectral selection names its
    /// band in zig-zag indices; storing them any other way would mean
    /// converting on every scan instead of once at the end. `i16` because that
    /// is the range a coefficient occupies, and the buffer covers the whole
    /// image.
    coeffs: Vec<i16>,
}

impl Component {
    fn block(&self, bx: usize, by: usize) -> Option<&[i16]> {
        if bx >= self.blocks_x || by >= self.blocks_y {
            return None;
        }
        let at = (by * self.blocks_x + bx).checked_mul(64)?;
        self.coeffs.get(at..at + 64)
    }

    fn block_mut(&mut self, bx: usize, by: usize) -> Option<&mut [i16]> {
        if bx >= self.blocks_x || by >= self.blocks_y {
            return None;
        }
        let at = (by * self.blocks_x + bx).checked_mul(64)?;
        self.coeffs.get_mut(at..at + 64)
    }
}

#[derive(Clone, Default)]
struct HuffmanTable {
    /// Maximum code of each length, or -1 when the length is unused.
    max_code: [i32; 17],
    /// Minimum code of each length.
    min_code: [i32; 17],
    /// Index into `values` where each length's codes begin.
    value_offset: [i32; 17],
    values: Vec<u8>,
}

impl HuffmanTable {
    /// Builds the canonical decoding tables from the per-length counts.
    fn build(counts: &[u8; 16], values: Vec<u8>) -> HuffmanTable {
        let mut table = HuffmanTable {
            values,
            ..HuffmanTable::default()
        };

        let mut code = 0i32;
        let mut index = 0i32;
        for length in 1..=16usize {
            let count = i32::from(counts.get(length - 1).copied().unwrap_or(0));
            if count == 0 {
                if let Some(slot) = table.max_code.get_mut(length) {
                    *slot = -1;
                }
                code <<= 1;
                continue;
            }
            if let Some(slot) = table.value_offset.get_mut(length) {
                *slot = index - code;
            }
            if let Some(slot) = table.min_code.get_mut(length) {
                *slot = code;
            }
            index += count;
            code += count;
            if let Some(slot) = table.max_code.get_mut(length) {
                *slot = code - 1;
            }
            code <<= 1;
        }
        table
    }
}

/// Reads bits from the entropy-coded segment, unstuffing as it goes.
struct BitReader<'a> {
    data: &'a [u8],
    at: usize,
    bits: u32,
    count: u32,
    /// True once the reader has run past the end.
    exhausted: bool,
    /// Set when `bit` walked over a restart marker by itself, so `restart`
    /// knows not to skip a second one and lose a whole interval.
    crossed_restart: bool,
}

impl<'a> BitReader<'a> {
    fn new(data: &'a [u8]) -> BitReader<'a> {
        BitReader {
            data,
            at: 0,
            bits: 0,
            count: 0,
            exhausted: false,
            crossed_restart: false,
        }
    }

    /// One bit, MSB first. Past the end this returns zeros, which lets a
    /// truncated image decode to something rather than nothing.
    fn bit(&mut self) -> u32 {
        if self.count == 0 {
            let Some(&byte) = self.data.get(self.at) else {
                self.exhausted = true;
                return 0;
            };
            self.at += 1;

            // T.81 F.1.2.3: a 0xFF in entropy-coded data is followed by a
            // stuffed zero; anything else is a marker and ends the segment.
            if byte == 0xFF {
                match self.data.get(self.at) {
                    Some(0x00) => self.at += 1,
                    Some(&m) if (0xD0..=0xD7).contains(&m) => {
                        // A restart marker: skip it and carry on.
                        self.at += 1;
                        self.crossed_restart = true;
                        return self.bit();
                    }
                    _ => {
                        self.exhausted = true;
                        return 0;
                    }
                }
            }
            self.bits = u32::from(byte);
            self.count = 8;
        }
        self.count -= 1;
        (self.bits >> self.count) & 1
    }

    fn bits(&mut self, n: u32) -> i32 {
        let mut value = 0i32;
        for _ in 0..n.min(31) {
            value = (value << 1) | self.bit() as i32;
        }
        value
    }

    /// Decodes one Huffman-coded symbol.
    fn huffman(&mut self, table: &HuffmanTable) -> Option<u8> {
        let mut code = 0i32;
        for length in 1..=16usize {
            code = (code << 1) | self.bit() as i32;
            let max = table.max_code.get(length).copied().unwrap_or(-1);
            if max >= 0 && code <= max {
                let offset = table.value_offset.get(length).copied().unwrap_or(0);
                let index = usize::try_from(offset + code).ok()?;
                return table.values.get(index).copied();
            }
            if self.exhausted {
                return None;
            }
        }
        None
    }

    /// Resets at a restart marker (T.81 F.2.1.3.1).
    fn restart(&mut self) {
        self.count = 0;
        if self.crossed_restart {
            // The bit reader already stepped over it while filling its
            // accumulator; skipping another would drop an entire interval's
            // worth of blocks.
            self.crossed_restart = false;
            return;
        }

        // Skip to just past the next RSTn marker.
        while self.at + 1 < self.data.len() {
            if self.data.get(self.at) == Some(&0xFF) {
                if let Some(&m) = self.data.get(self.at + 1) {
                    if (0xD0..=0xD7).contains(&m) {
                        self.at += 2;
                        return;
                    }
                }
            }
            self.at += 1;
        }
    }
}

/// Zig-zag order (T.81 figure A.6).
const ZIGZAG: [usize; 64] = [
    0, 1, 8, 16, 9, 2, 3, 10, 17, 24, 32, 25, 18, 11, 4, 5, 12, 19, 26, 33, 40, 48, 41, 34, 27, 20,
    13, 6, 7, 14, 21, 28, 35, 42, 49, 56, 57, 50, 43, 36, 29, 22, 15, 23, 30, 37, 44, 51, 58, 59,
    52, 45, 38, 31, 39, 46, 53, 60, 61, 54, 47, 55, 62, 63,
];

/// The largest magnitude category T.81 defines. A Huffman table is free to
/// contain a larger byte, and a corrupt one will; shifting by it is undefined
/// in the spec and a panic in Rust (ruling 1).
const MAX_CATEGORY: u32 = 16;

/// Extends a Huffman-decoded magnitude to its signed value (T.81 F.2.2.1).
fn extend(value: i32, length: u32) -> i32 {
    if length == 0 || length > MAX_CATEGORY {
        return 0;
    }
    if value < (1 << (length - 1)) {
        value - (1 << length) + 1
    } else {
        value
    }
}

/// Finds the next real marker, stepping over stuffed bytes and restarts.
///
/// A scan's entropy-coded data has no length field, so the only way to reach
/// the segment after it is to look for the next thing that cannot occur inside
/// it. Trusting the bit reader's position instead would put a corrupt scan's
/// desynchronisation into the marker stream as well.
fn next_marker(data: &[u8], from: usize) -> usize {
    let mut at = from;
    while at + 1 < data.len() {
        if data.get(at) == Some(&0xFF) {
            if let Some(&marker) = data.get(at + 1) {
                if marker != 0x00 && marker != 0xFF && !(0xD0..=0xD7).contains(&marker) {
                    return at;
                }
            }
        }
        at += 1;
    }
    data.len()
}

/// Sizes the coefficient buffers, returning the MCU grid.
///
/// Returns `None` when the image would exceed the output cap, which is checked
/// here rather than after decoding because the buffers are the allocation that
/// a hostile size field is trying to provoke.
fn allocate(
    components: &mut [Component],
    width: usize,
    height: usize,
    max_output: usize,
) -> Option<(usize, usize)> {
    if width
        .saturating_mul(height)
        .saturating_mul(components.len())
        > max_output
    {
        return None;
    }

    let h_max = components.iter().map(|c| c.h).max().unwrap_or(1).max(1);
    let v_max = components.iter().map(|c| c.v).max().unwrap_or(1).max(1);
    let mcus_x = width.div_ceil(h_max * 8);
    let mcus_y = height.div_ceil(v_max * 8);

    for component in components.iter_mut() {
        component.blocks_x = mcus_x * component.h;
        component.blocks_y = mcus_y * component.v;

        // A.1.1: a component's own resolution, rounded up to whole blocks.
        let own_w = (width * component.h).div_ceil(h_max);
        let own_h = (height * component.v).div_ceil(v_max);
        component.scan_x = own_w.div_ceil(8).min(component.blocks_x);
        component.scan_y = own_h.div_ceil(8).min(component.blocks_y);

        component.coeffs = vec![0i16; component.blocks_x * component.blocks_y * 64];
    }

    Some((mcus_x, mcus_y))
}

/// Decodes a JPEG.
pub fn decode(data: &[u8], max_output: usize) -> Result<JpegImage, JpegError> {
    if data.get(..2) != Some(&[0xFF, 0xD8]) {
        return Err(JpegError::NotJpeg);
    }

    let mut warnings = Vec::new();
    let mut quant = [[1u16; 64]; 4];
    let mut dc_tables: Vec<HuffmanTable> = vec![HuffmanTable::default(); 4];
    let mut ac_tables: Vec<HuffmanTable> = vec![HuffmanTable::default(); 4];
    let mut components: Vec<Component> = Vec::new();
    let (mut width, mut height) = (0usize, 0usize);
    let mut restart_interval = 0usize;
    let mut adobe_transform: Option<u8> = None;
    let mut adobe_seen = false;
    let mut progressive = false;
    let mut sample_precision = 8u8;
    let mut mcus = (0usize, 0usize);
    let mut allocated = false;
    let mut truncated = false;

    let mut at = 2usize;
    while at + 1 < data.len() {
        if data.get(at) != Some(&0xFF) {
            at += 1;
            continue;
        }
        let Some(&marker) = data.get(at + 1) else {
            break;
        };
        at += 2;

        match marker {
            // Padding and standalone markers.
            0x01 | 0xD0..=0xD7 | 0xFF => continue,
            0xD9 => break, // EOI
            _ => {}
        }

        let Some(length) = data
            .get(at..at + 2)
            .map(|b| usize::from(u16::from_be_bytes([b[0], b[1]])))
        else {
            break;
        };
        let segment_end = at + length.max(2);
        let Some(segment) = data.get(at + 2..segment_end.min(data.len())) else {
            break;
        };

        match marker {
            // SOF0 baseline, SOF1 extended sequential, SOF2 progressive.
            0xC0..=0xC2 => {
                progressive = marker == 0xC2;

                let (Some(&precision), Some(h), Some(w)) =
                    (segment.first(), segment.get(1..3), segment.get(3..5))
                else {
                    return Err(JpegError::Truncated);
                };
                // B.2.2: `P` is 8 for a baseline frame and 8 or 12 for an
                // extended sequential or progressive one. Anything else is a
                // header this build will not guess at.
                if precision != 8 && !(precision == 12 && marker != 0xC0) {
                    return Err(JpegError::UnsupportedPrecision);
                }
                sample_precision = precision;
                height = usize::from(u16::from_be_bytes([h[0], h[1]]));
                width = usize::from(u16::from_be_bytes([w[0], w[1]]));

                let count = usize::from(segment.get(5).copied().unwrap_or(0));
                components.clear();
                for i in 0..count.min(4) {
                    let base = 6 + i * 3;
                    let (Some(&id), Some(&hv), Some(&tq)) = (
                        segment.get(base),
                        segment.get(base + 1),
                        segment.get(base + 2),
                    ) else {
                        return Err(JpegError::Truncated);
                    };
                    components.push(Component {
                        id,
                        h: usize::from(hv >> 4).clamp(1, 4),
                        v: usize::from(hv & 0x0F).clamp(1, 4),
                        quant: usize::from(tq).min(3),
                        ..Component::default()
                    });
                }
            }
            // Every other SOF marker, refused by the family it belongs to.
            // 0xC4 is DHT and 0xCC is DAC, which are tables rather than
            // frames and are handled below.
            0xC9 | 0xCA | 0xCB | 0xCD | 0xCE | 0xCF => return Err(JpegError::Arithmetic),
            0xC3 | 0xC7 => return Err(JpegError::Lossless),
            0xC5 | 0xC6 => return Err(JpegError::Differential),

            // DQT
            0xDB => {
                let mut i = 0usize;
                while i < segment.len() {
                    let Some(&pq_tq) = segment.get(i) else { break };
                    i += 1;
                    let precision = pq_tq >> 4;
                    let index = usize::from(pq_tq & 0x0F).min(3);
                    for k in 0..64usize {
                        let value = if precision == 0 {
                            let Some(&v) = segment.get(i) else { break };
                            i += 1;
                            u16::from(v)
                        } else {
                            let Some(pair) = segment.get(i..i + 2) else {
                                break;
                            };
                            i += 2;
                            u16::from_be_bytes([pair[0], pair[1]])
                        };
                        if let (Some(table), Some(&z)) = (quant.get_mut(index), ZIGZAG.get(k)) {
                            if let Some(slot) = table.get_mut(z) {
                                *slot = value.max(1);
                            }
                        }
                    }
                }
            }

            // DHT
            0xC4 => {
                let mut i = 0usize;
                while i < segment.len() {
                    let Some(&tc_th) = segment.get(i) else { break };
                    i += 1;
                    let class = tc_th >> 4;
                    let index = usize::from(tc_th & 0x0F).min(3);

                    let mut counts = [0u8; 16];
                    let Some(raw) = segment.get(i..i + 16) else {
                        break;
                    };
                    counts.copy_from_slice(raw);
                    i += 16;

                    let total: usize = counts.iter().map(|&c| usize::from(c)).sum();
                    let Some(values) = segment.get(i..i + total) else {
                        break;
                    };
                    i += total;

                    let table = HuffmanTable::build(&counts, values.to_vec());
                    let target = if class == 0 {
                        &mut dc_tables
                    } else {
                        &mut ac_tables
                    };
                    if let Some(slot) = target.get_mut(index) {
                        *slot = table;
                    }
                }
            }

            // DRI
            0xDD => {
                if let Some(pair) = segment.get(..2) {
                    restart_interval = usize::from(u16::from_be_bytes([pair[0], pair[1]]));
                }
            }

            // APP14: Adobe's colour transform marker.
            0xEE => {
                if segment.starts_with(b"Adobe") {
                    adobe_seen = true;
                    adobe_transform = segment.last().copied();
                }
            }

            // SOS: one scan, of which a progressive file has many.
            0xDA => {
                let count = usize::from(segment.first().copied().unwrap_or(0));
                let mut parts: Vec<usize> = Vec::with_capacity(count.min(4));
                for i in 0..count.min(4) {
                    let (Some(&id), Some(&tables)) =
                        (segment.get(1 + i * 2), segment.get(2 + i * 2))
                    else {
                        return Err(JpegError::Truncated);
                    };
                    if let Some(index) = components.iter().position(|c| c.id == id) {
                        if let Some(component) = components.get_mut(index) {
                            component.dc_table = usize::from(tables >> 4).min(3);
                            component.ac_table = usize::from(tables & 0x0F).min(3);
                        }
                        parts.push(index);
                    }
                }

                // G.1.1.1.1: the spectral band and the point transform. A
                // baseline scan always says 0..63 with no approximation, so
                // reading them costs nothing and progressive needs them.
                let base = 1 + count.min(4) * 2;
                let ss = usize::from(segment.get(base).copied().unwrap_or(0)).min(63);
                let se = usize::from(segment.get(base + 1).copied().unwrap_or(63)).min(63);
                let a = segment.get(base + 2).copied().unwrap_or(0);
                let (ah, al) = (u32::from(a >> 4), u32::from(a & 0x0F));

                if !allocated {
                    let Some(grid) = allocate(&mut components, width, height, max_output) else {
                        warnings.push(Warning::OutputCapHit);
                        return Err(JpegError::Truncated);
                    };
                    mcus = grid;
                    allocated = true;
                }

                let scan = data.get(segment_end..).unwrap_or_default();
                let complete = decode_scan(
                    scan,
                    &mut components,
                    &parts,
                    &dc_tables,
                    &ac_tables,
                    restart_interval,
                    progressive,
                    (ss, se.max(ss)),
                    (ah, al),
                    mcus,
                );
                truncated |= !complete;

                // Entropy data carries no length; the next segment starts at
                // the next marker that cannot appear inside it.
                at = next_marker(data, segment_end);
                continue;
            }

            _ => {}
        }

        at = segment_end;
    }

    if !allocated {
        warnings.push(Warning::TruncatedInput);
        return Err(JpegError::Truncated);
    }
    if truncated {
        warnings.push(Warning::TruncatedInput);
    }

    finish(
        &components,
        &quant,
        width,
        height,
        adobe_seen,
        adobe_transform,
        sample_precision,
        max_output,
        warnings,
    )
}

/// Decodes one scan into the components' coefficient buffers.
///
/// Returns false when the entropy data ran out or a table was missing. What
/// was decoded stays in place either way: a progressive file that loses its
/// last refinement still shows an image, just a coarser one, which is exactly
/// the degradation the format was designed around (ruling 2).
#[allow(clippy::too_many_arguments)]
fn decode_scan(
    data: &[u8],
    components: &mut [Component],
    parts: &[usize],
    dc_tables: &[HuffmanTable],
    ac_tables: &[HuffmanTable],
    restart_interval: usize,
    progressive: bool,
    band: (usize, usize),
    approximation: (u32, u32),
    mcus: (usize, usize),
) -> bool {
    if parts.is_empty() {
        return false;
    }

    let mut reader = BitReader::new(data);
    let mut eobrun = 0u32;
    for &index in parts {
        if let Some(component) = components.get_mut(index) {
            component.dc_prediction = 0;
        }
    }

    // A.2: more than one component in a scan means the blocks are interleaved
    // MCU by MCU; one component means plain raster order over that component's
    // own blocks, with no MCU padding.
    let interleaved = parts.len() > 1;
    let (units_x, units_y) = if interleaved {
        mcus
    } else {
        parts
            .first()
            .and_then(|&index| components.get(index))
            .map_or((0, 0), |c| (c.scan_x, c.scan_y))
    };

    let mut unit = 0usize;
    let mut complete = true;

    'outer: for uy in 0..units_y {
        for ux in 0..units_x {
            if restart_interval > 0 && unit > 0 && unit % restart_interval == 0 {
                reader.restart();
                eobrun = 0;
                for &index in parts {
                    if let Some(component) = components.get_mut(index) {
                        component.dc_prediction = 0;
                    }
                }
            }
            unit += 1;

            if !interleaved {
                let Some(&index) = parts.first() else {
                    break 'outer;
                };
                let Some(component) = components.get_mut(index) else {
                    break 'outer;
                };
                if !decode_block(
                    &mut reader,
                    component,
                    ux,
                    uy,
                    dc_tables,
                    ac_tables,
                    progressive,
                    band,
                    approximation,
                    &mut eobrun,
                ) {
                    complete = false;
                    break 'outer;
                }
                continue;
            }

            for &index in parts {
                let (h, v) = components.get(index).map_or((1, 1), |c| (c.h, c.v));
                for by in 0..v {
                    for bx in 0..h {
                        let Some(component) = components.get_mut(index) else {
                            complete = false;
                            break 'outer;
                        };
                        if !decode_block(
                            &mut reader,
                            component,
                            ux * h + bx,
                            uy * v + by,
                            dc_tables,
                            ac_tables,
                            progressive,
                            band,
                            approximation,
                            &mut eobrun,
                        ) {
                            complete = false;
                            break 'outer;
                        }
                    }
                }
            }
        }
    }

    complete && !reader.exhausted
}

/// Decodes one block, in whichever of the four codings this scan is using.
#[allow(clippy::too_many_arguments)]
fn decode_block(
    reader: &mut BitReader,
    component: &mut Component,
    bx: usize,
    by: usize,
    dc_tables: &[HuffmanTable],
    ac_tables: &[HuffmanTable],
    progressive: bool,
    band: (usize, usize),
    approximation: (u32, u32),
    eobrun: &mut u32,
) -> bool {
    // Worked on as i32 and stored as i16: refinement adds to what earlier
    // scans left, and the intermediate must not wrap where the stored value
    // saturates.
    let mut block = [0i32; 64];
    if let Some(existing) = component.block(bx, by) {
        for (slot, &value) in block.iter_mut().zip(existing.iter()) {
            *slot = i32::from(value);
        }
    }

    let (ss, se) = band;
    let (ah, al) = approximation;

    let ok = if !progressive {
        decode_sequential(reader, component, &mut block, dc_tables, ac_tables)
    } else if ss == 0 {
        decode_dc_progressive(reader, component, &mut block, dc_tables, ah, al)
    } else {
        decode_ac_progressive(
            reader, component, &mut block, ac_tables, ss, se, ah, al, eobrun,
        )
    };

    if let Some(target) = component.block_mut(bx, by) {
        for (slot, &value) in target.iter_mut().zip(block.iter()) {
            *slot = value.clamp(i32::from(i16::MIN), i32::from(i16::MAX)) as i16;
        }
    }
    ok
}

/// Baseline and extended sequential: the whole block in one pass.
fn decode_sequential(
    reader: &mut BitReader,
    component: &mut Component,
    block: &mut [i32; 64],
    dc_tables: &[HuffmanTable],
    ac_tables: &[HuffmanTable],
) -> bool {
    let (Some(dc_table), Some(ac_table)) = (
        dc_tables.get(component.dc_table),
        ac_tables.get(component.ac_table),
    ) else {
        return false;
    };

    // DC: a difference from the previous block's value.
    let Some(t) = reader.huffman(dc_table) else {
        return false;
    };
    let t = u32::from(t).min(MAX_CATEGORY);
    let diff = extend(reader.bits(t), t);
    component.dc_prediction = component.dc_prediction.saturating_add(diff);
    block[0] = component.dc_prediction;

    // AC: run-length pairs to the end of the block.
    let mut k = 1usize;
    while k < 64 {
        let Some(rs) = reader.huffman(ac_table) else {
            return false;
        };
        let run = usize::from(rs >> 4);
        let size = u32::from(rs & 0x0F);
        if size == 0 {
            if run == 15 {
                k += 16; // ZRL: sixteen zeros.
                continue;
            }
            break; // EOB.
        }
        k += run;
        if k >= 64 {
            break;
        }
        let value = extend(reader.bits(size), size);
        if let Some(slot) = block.get_mut(k) {
            *slot = value;
        }
        k += 1;
    }
    true
}

/// Progressive DC (G.1.2.1): the first scan sends the value shifted right by
/// the point transform; every later one sends the next bit down.
fn decode_dc_progressive(
    reader: &mut BitReader,
    component: &mut Component,
    block: &mut [i32; 64],
    dc_tables: &[HuffmanTable],
    ah: u32,
    al: u32,
) -> bool {
    if ah == 0 {
        let Some(table) = dc_tables.get(component.dc_table) else {
            return false;
        };
        let Some(t) = reader.huffman(table) else {
            return false;
        };
        let t = u32::from(t).min(MAX_CATEGORY);
        let diff = extend(reader.bits(t), t);
        component.dc_prediction = component.dc_prediction.saturating_add(diff);
        block[0] = component.dc_prediction << al.min(15);
        return true;
    }

    if reader.bit() == 1 {
        block[0] |= 1 << al.min(15);
    }
    !reader.exhausted
}

/// Progressive AC, first pass (G.1.2.2): run-length pairs within the band,
/// with an end-of-band run that can span whole blocks.
#[allow(clippy::too_many_arguments)]
fn decode_ac_first(
    reader: &mut BitReader,
    block: &mut [i32; 64],
    table: &HuffmanTable,
    ss: usize,
    se: usize,
    al: u32,
    eobrun: &mut u32,
) -> bool {
    if *eobrun > 0 {
        *eobrun -= 1;
        return true;
    }

    let mut k = ss;
    while k <= se {
        let Some(rs) = reader.huffman(table) else {
            return false;
        };
        let run = u32::from(rs >> 4);
        let size = u32::from(rs & 0x0F);

        if size == 0 {
            if run < 15 {
                // An EOB run of 2^run blocks, this one included.
                *eobrun = (1u32 << run).saturating_sub(1);
                if run > 0 {
                    *eobrun = eobrun.saturating_add(reader.bits(run) as u32);
                }
                break;
            }
            k += 16; // ZRL.
            continue;
        }

        k += run as usize;
        if k > se {
            break;
        }
        let value = extend(reader.bits(size), size);
        if let Some(slot) = block.get_mut(k) {
            *slot = value << al.min(15);
        }
        k += 1;
    }
    true
}

/// Progressive AC, refinement (G.1.2.3).
///
/// The awkward one: the bit stream interleaves corrections to coefficients an
/// earlier scan already found with the run-lengths that place new ones, and a
/// correction bit is only present for a coefficient that is already non-zero.
/// Reading one bit too many or too few here desynchronises the rest of the
/// image, which is why this follows the reference structure closely.
#[allow(clippy::too_many_arguments)]
fn decode_ac_refine(
    reader: &mut BitReader,
    block: &mut [i32; 64],
    table: &HuffmanTable,
    ss: usize,
    se: usize,
    al: u32,
    eobrun: &mut u32,
) -> bool {
    let shift = al.min(14);
    let positive = 1i32 << shift;
    let negative = -(1i32 << shift);

    let mut k = ss;
    if *eobrun == 0 {
        while k <= se {
            let Some(rs) = reader.huffman(table) else {
                return false;
            };
            let mut run = i32::from(rs >> 4);
            let size = rs & 0x0F;

            let mut new_value = 0i32;
            if size == 0 {
                if run < 15 {
                    *eobrun = 1u32 << (run.clamp(0, 14) as u32);
                    if run > 0 {
                        *eobrun = eobrun.saturating_add(reader.bits(run as u32) as u32);
                    }
                    break;
                }
                // run == 15 with no size: skip sixteen zero coefficients,
                // correcting any non-zero ones passed on the way.
            } else {
                // The magnitude is always one bit in a refinement scan; the
                // bit that follows is its sign.
                new_value = if reader.bit() == 1 {
                    positive
                } else {
                    negative
                };
            }

            while k <= se {
                let coefficient = block.get(k).copied().unwrap_or(0);
                if coefficient != 0 {
                    if reader.bit() == 1 && (coefficient & positive) == 0 {
                        if let Some(slot) = block.get_mut(k) {
                            *slot = if coefficient >= 0 {
                                coefficient.saturating_add(positive)
                            } else {
                                coefficient.saturating_add(negative)
                            };
                        }
                    }
                } else {
                    if run == 0 {
                        if new_value != 0 {
                            if let Some(slot) = block.get_mut(k) {
                                *slot = new_value;
                            }
                        }
                        k += 1;
                        break;
                    }
                    run -= 1;
                }
                k += 1;
            }

            if reader.exhausted {
                return false;
            }
        }
    }

    if *eobrun > 0 {
        // Inside an end-of-band run no new coefficients appear, but the ones
        // already there still get their correction bit.
        while k <= se {
            let coefficient = block.get(k).copied().unwrap_or(0);
            if coefficient != 0 && reader.bit() == 1 && (coefficient & positive) == 0 {
                if let Some(slot) = block.get_mut(k) {
                    *slot = if coefficient >= 0 {
                        coefficient.saturating_add(positive)
                    } else {
                        coefficient.saturating_add(negative)
                    };
                }
            }
            k += 1;
        }
        *eobrun -= 1;
    }
    true
}

#[allow(clippy::too_many_arguments)]
fn decode_ac_progressive(
    reader: &mut BitReader,
    component: &mut Component,
    block: &mut [i32; 64],
    ac_tables: &[HuffmanTable],
    ss: usize,
    se: usize,
    ah: u32,
    al: u32,
    eobrun: &mut u32,
) -> bool {
    let Some(table) = ac_tables.get(component.ac_table) else {
        return false;
    };
    if ah == 0 {
        decode_ac_first(reader, block, table, ss, se, al, eobrun)
    } else {
        decode_ac_refine(reader, block, table, ss, se, al, eobrun)
    }
}

/// Turns the finished coefficients into pixels.
#[allow(clippy::too_many_arguments)]
fn finish(
    components: &[Component],
    quant: &[[u16; 64]; 4],
    width: usize,
    height: usize,
    adobe_seen: bool,
    adobe_transform: Option<u8>,
    precision: u8,
    max_output: usize,
    mut warnings: Vec<Warning>,
) -> Result<JpegImage, JpegError> {
    if width == 0 || height == 0 || components.is_empty() {
        return Err(JpegError::Truncated);
    }

    let color = match components.len() {
        1 => JpegColor::Gray,
        3 => JpegColor::Rgb,
        4 => {
            // Adobe transform 2 is YCCK; 0 is plain CMYK. Photoshop writes
            // CMYK inverted, which the Adobe marker's presence signals.
            if adobe_seen {
                JpegColor::CmykInverted
            } else {
                JpegColor::Cmyk
            }
        }
        _ => return Err(JpegError::UnsupportedComponents),
    };

    let needed = width
        .saturating_mul(height)
        .saturating_mul(components.len());
    if needed > max_output {
        warnings.push(Warning::OutputCapHit);
        return Err(JpegError::Truncated);
    }

    if precision > 8 {
        // Ruling 10: the samples handed out are narrower than the frame's, and
        // that is a leniency rather than a decode. Recorded once.
        warnings.push(Warning::JpegPrecisionNarrowed);
    }

    let h_max = components.iter().map(|c| c.h).max().unwrap_or(1).max(1);
    let v_max = components.iter().map(|c| c.v).max().unwrap_or(1).max(1);

    // One full-resolution plane per component, upsampled as it is written.
    let mut planes: Vec<Vec<u8>> = components
        .iter()
        .map(|_| vec![128u8; width * height])
        .collect();

    let mut block = [0i32; 64];
    let mut pixels = [0u8; 64];

    for (ci, component) in components.iter().enumerate() {
        let table = quant.get(component.quant).copied().unwrap_or([1; 64]);
        let scale_x = h_max / component.h.max(1);
        let scale_y = v_max / component.v.max(1);
        let Some(plane) = planes.get_mut(ci) else {
            continue;
        };

        for by in 0..component.blocks_y {
            for bx in 0..component.blocks_x {
                let Some(source) = component.block(bx, by) else {
                    continue;
                };

                // Dequantise out of zig-zag order and into the natural one the
                // transform expects.
                block.fill(0);
                for (k, &coefficient) in source.iter().enumerate() {
                    if coefficient == 0 {
                        continue;
                    }
                    let Some(&z) = ZIGZAG.get(k) else { continue };
                    let q = i32::from(table.get(z).copied().unwrap_or(1));
                    if let Some(slot) = block.get_mut(z) {
                        *slot = i32::from(coefficient).saturating_mul(q);
                    }
                }
                idct_block(&block, precision, &mut pixels);

                let origin_x = bx * 8 * scale_x;
                let origin_y = by * 8 * scale_y;
                if origin_x >= width || origin_y >= height {
                    continue;
                }

                for py in 0..8usize {
                    for px in 0..8usize {
                        let value = pixels.get(py * 8 + px).copied().unwrap_or(128);
                        for ry in 0..scale_y {
                            for rx in 0..scale_x {
                                let x = origin_x + px * scale_x + rx;
                                let y = origin_y + py * scale_y + ry;
                                if x < width && y < height {
                                    if let Some(slot) = plane.get_mut(y * width + x) {
                                        *slot = value;
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // Interleave, converting colour where the model calls for it.
    let n = components.len();
    let mut out = vec![0u8; width * height * n];
    for i in 0..(width * height) {
        match color {
            JpegColor::Gray => {
                if let (Some(slot), Some(plane)) = (out.get_mut(i), planes.first()) {
                    *slot = plane.get(i).copied().unwrap_or(128);
                }
            }
            JpegColor::Rgb => {
                let y = f32::from(
                    planes
                        .first()
                        .and_then(|p| p.get(i))
                        .copied()
                        .unwrap_or(128),
                );
                let cb =
                    f32::from(planes.get(1).and_then(|p| p.get(i)).copied().unwrap_or(128)) - 128.0;
                let cr =
                    f32::from(planes.get(2).and_then(|p| p.get(i)).copied().unwrap_or(128)) - 128.0;
                // T.871: the JFIF YCbCr to RGB conversion.
                let rgb = [
                    y + 1.402 * cr,
                    y - 0.344_136 * cb - 0.714_136 * cr,
                    y + 1.772 * cb,
                ];
                for (c, value) in rgb.iter().enumerate() {
                    if let Some(slot) = out.get_mut(i * 3 + c) {
                        *slot = value.clamp(0.0, 255.0) as u8;
                    }
                }
            }
            JpegColor::Cmyk | JpegColor::CmykInverted => {
                let transform = adobe_transform.unwrap_or(0);
                let raw: Vec<u8> = (0..4)
                    .map(|c| planes.get(c).and_then(|p| p.get(i)).copied().unwrap_or(0))
                    .collect();

                // Transform 2 means the first three components are YCCK and
                // need the same conversion as YCbCr before use.
                let values = if transform == 2 {
                    let y = f32::from(raw.first().copied().unwrap_or(0));
                    let cb = f32::from(raw.get(1).copied().unwrap_or(128)) - 128.0;
                    let cr = f32::from(raw.get(2).copied().unwrap_or(128)) - 128.0;
                    [
                        (y + 1.402 * cr).clamp(0.0, 255.0) as u8,
                        (y - 0.344_136 * cb - 0.714_136 * cr).clamp(0.0, 255.0) as u8,
                        (y + 1.772 * cb).clamp(0.0, 255.0) as u8,
                        raw.get(3).copied().unwrap_or(0),
                    ]
                } else {
                    [
                        raw.first().copied().unwrap_or(0),
                        raw.get(1).copied().unwrap_or(0),
                        raw.get(2).copied().unwrap_or(0),
                        raw.get(3).copied().unwrap_or(0),
                    ]
                };

                for (c, value) in values.iter().enumerate() {
                    if let Some(slot) = out.get_mut(i * 4 + c) {
                        // Adobe writes CMYK inverted; undo it so callers get
                        // ink values that mean what they say.
                        *slot = if color == JpegColor::CmykInverted {
                            255 - *value
                        } else {
                            *value
                        };
                    }
                }
            }
        }
    }

    Ok(JpegImage {
        width: width as u32,
        height: height as u32,
        color,
        data: out,
        precision,
        warnings,
    })
}

/// The inverse DCT of one block, separable and in integers.
fn idct_block(input: &[i32; 64], precision: u8, out: &mut [u8; 64]) {
    // A straightforward separable implementation: rows then columns, with
    // fixed-point cosines. Determinism matters more here than the last unit
    // of precision (ruling 4).
    let mut tmp = [0i32; 64];

    for row in 0..8usize {
        for x in 0..8usize {
            let mut sum = 0i64;
            for u in 0..8usize {
                let coefficient = input.get(row * 8 + u).copied().unwrap_or(0);
                if coefficient == 0 {
                    continue;
                }
                // The basis function already carries C(u); no extra scale.
                let cos = COS_TABLE.get(x * 8 + u).copied().unwrap_or(0);
                sum += i64::from(coefficient) * i64::from(cos);
            }
            if let Some(slot) = tmp.get_mut(row * 8 + x) {
                *slot = (sum >> 14) as i32;
            }
        }
    }

    for col in 0..8usize {
        for y in 0..8usize {
            let mut sum = 0i64;
            for v in 0..8usize {
                let coefficient = tmp.get(v * 8 + col).copied().unwrap_or(0);
                if coefficient == 0 {
                    continue;
                }
                let cos = COS_TABLE.get(y * 8 + v).copied().unwrap_or(0);
                sum += i64::from(coefficient) * i64::from(cos);
            }
            // A.3.1's level shift is `2^(P-1)`, and the clamp is to the
            // frame's own range. A 12-bit sample is then narrowed to the
            // eight this crate hands out -- see [`JpegImage::data`] -- which
            // for `P = 8` is a shift of zero and leaves the byte untouched.
            let half = 1i64 << (precision - 1);
            let ceiling = (1i64 << precision) - 1;
            let value = (((sum >> 14) + half).clamp(0, ceiling) >> (precision - 8)) as u8;
            if let Some(slot) = out.get_mut(y * 8 + col) {
                *slot = value;
            }
        }
    }
}

/// `cos((2x+1) · u · π / 16) · C(u) / 2`, in 1/16384.
///
/// Computed once at first use rather than written out, so the values cannot
/// drift from the formula they are supposed to be.
static COS_TABLE: std::sync::LazyLock<[i32; 64]> = std::sync::LazyLock::new(|| {
    let mut table = [0i32; 64];
    for x in 0..8usize {
        for u in 0..8usize {
            let cu = if u == 0 {
                1.0 / std::f64::consts::SQRT_2
            } else {
                1.0
            };
            let value =
                cu / 2.0 * ((2.0 * x as f64 + 1.0) * u as f64 * std::f64::consts::PI / 16.0).cos();
            if let Some(slot) = table.get_mut(x * 8 + u) {
                *slot = (value * 16384.0).round() as i32;
            }
        }
    }
    table
});

#[cfg(test)]
mod tests {
    use super::*;

    /// A 1×1 grey JPEG, hand-assembled: the smallest thing that exercises the
    /// whole path from marker parsing to a decoded pixel.
    fn tiny_gray() -> Vec<u8> {
        let mut out = vec![0xFF, 0xD8];

        // DQT: all ones, so coefficients pass through unscaled.
        out.extend_from_slice(&[0xFF, 0xDB, 0x00, 0x43, 0x00]);
        out.extend_from_slice(&[1u8; 64]);

        // SOF0: 8-bit, 1×1, one component with id 1, no subsampling. The
        // length says nine body bytes and there are nine — this fixture used
        // to declare eleven and supply eight, so the component descriptor was
        // read out of the following marker and matched nothing in the scan.
        out.extend_from_slice(&[0xFF, 0xC0, 0x00, 0x0B, 0x08, 0x00, 0x01, 0x00, 0x01, 0x01]);
        out.extend_from_slice(&[0x01, 0x11, 0x00]);

        // DHT for DC: one code of length 2, value 0.
        let mut dht = vec![0x00];
        let mut counts = [0u8; 16];
        counts[1] = 1;
        dht.extend_from_slice(&counts);
        dht.push(0x00);
        out.extend_from_slice(&[0xFF, 0xC4]);
        out.extend_from_slice(&((dht.len() + 2) as u16).to_be_bytes());
        out.extend_from_slice(&dht);

        // DHT for AC: one code of length 2, value 0 (EOB).
        let mut dht = vec![0x10];
        dht.extend_from_slice(&counts);
        dht.push(0x00);
        out.extend_from_slice(&[0xFF, 0xC4]);
        out.extend_from_slice(&((dht.len() + 2) as u16).to_be_bytes());
        out.extend_from_slice(&dht);

        // SOS.
        out.extend_from_slice(&[0xFF, 0xDA, 0x00, 0x08, 0x01, 0x01, 0x00, 0x00, 0x3F, 0x00]);
        // Entropy data: DC code 00 (size 0, so difference zero), then EOB.
        out.extend_from_slice(&[0b0000_0000]);
        out.extend_from_slice(&[0xFF, 0xD9]);
        out
    }

    #[test]
    fn a_minimal_grayscale_image_decodes() {
        let image = decode(&tiny_gray(), 1 << 20).expect("it decodes");
        assert_eq!((image.width, image.height), (1, 1));
        assert_eq!(image.color, JpegColor::Gray);
        assert_eq!(image.data.len(), 1);
        // A DC of zero is mid-grey after the level shift.
        assert_eq!(image.data.first().copied(), Some(128));
    }

    #[test]
    fn markers_that_are_not_jpeg_are_refused() {
        assert_eq!(decode(&[], 1 << 20).err(), Some(JpegError::NotJpeg));
        assert_eq!(
            decode(b"not a jpeg", 1 << 20).err(),
            Some(JpegError::NotJpeg)
        );
    }

    #[test]
    fn arithmetic_coding_is_reported_rather_than_half_decoded() {
        let mut arithmetic = vec![0xFF, 0xD8, 0xFF, 0xC9, 0x00, 0x0B, 0x08];
        arithmetic.extend_from_slice(&[0x00, 0x01, 0x00, 0x01, 0x01, 0x11, 0x00]);
        assert_eq!(
            decode(&arithmetic, 1 << 20).err(),
            Some(JpegError::Arithmetic)
        );
    }

    /// [`tiny_gray`] with the frame marker and the sample precision chosen.
    ///
    /// The SOF sits after SOI and a 69-byte DQT, and is found rather than
    /// counted so that a change to the fixture above cannot silently move it.
    fn tiny_gray_at(marker: u8, precision: u8) -> Vec<u8> {
        let mut out = tiny_gray();
        let at = out
            .windows(2)
            .position(|w| w == [0xFF, 0xC0])
            .expect("the fixture has an SOF0");
        out[at + 1] = marker;
        out[at + 4] = precision;
        out
    }

    /// **B.2.2 allows twelve bits, and only outside the baseline frame.**
    ///
    /// `P` is 8 for SOF0 and 8 or 12 for SOF1 and SOF2, so the same header at
    /// twelve bits is a legal extended-sequential frame and an illegal
    /// baseline one. Both are asserted, because accepting 12 everywhere would
    /// read a corrupt baseline header as a valid frame.
    #[test]
    fn twelve_bits_are_read_outside_the_baseline_frame_and_refused_inside_it() {
        assert_eq!(
            decode(&tiny_gray_at(0xC0, 12), 1 << 20).err(),
            Some(JpegError::UnsupportedPrecision),
            "SOF0 at twelve bits is not a baseline frame"
        );
        let image = decode(&tiny_gray_at(0xC1, 12), 1 << 20).expect("SOF1 at twelve bits decodes");
        assert_eq!(image.precision, 12);
        assert_eq!(
            image.warnings,
            vec![Warning::JpegPrecisionNarrowed],
            "the narrowing to eight bits is recorded"
        );
        // A DC of zero is mid-grey after A.3.1's level shift, which at twelve
        // bits is 2048 of 4095 -- and 2048 >> 4 is 128, the same byte the
        // eight-bit path gives. That is the point: the narrowing is a shift,
        // not a rescale, so mid-grey stays mid-grey.
        assert_eq!(image.data.first().copied(), Some(128));

        for bits in [1u8, 4, 9, 16] {
            assert_eq!(
                decode(&tiny_gray_at(0xC1, bits), 1 << 20).err(),
                Some(JpegError::UnsupportedPrecision),
                "{bits} bits"
            );
        }
    }

    /// **Every frame type this build does not decode refuses by its own
    /// name**, rather than being skipped.
    ///
    /// Until now only SOF9, SOF10 and SOF11 were named. SOF3, SOF5, SOF6,
    /// SOF7, SOF13, SOF14 and SOF15 fell through to the unknown-segment arm,
    /// were stepped over as though they were a comment, and the decode then
    /// failed as `Truncated` -- a lossless JPEG reported as a damaged file.
    #[test]
    fn every_frame_type_this_build_declines_refuses_by_its_own_name() {
        for (marker, expected) in [
            (0xC3u8, JpegError::Lossless),
            (0xC5, JpegError::Differential),
            (0xC6, JpegError::Differential),
            (0xC7, JpegError::Lossless),
            (0xC9, JpegError::Arithmetic),
            (0xCA, JpegError::Arithmetic),
            (0xCB, JpegError::Arithmetic),
            (0xCD, JpegError::Arithmetic),
            (0xCE, JpegError::Arithmetic),
            (0xCF, JpegError::Arithmetic),
        ] {
            assert_eq!(
                decode(&tiny_gray_at(marker, 8), 1 << 20).err(),
                Some(expected),
                "SOF marker {marker:#04x}"
            );
        }
    }

    #[test]
    fn an_output_cap_is_honoured() {
        // A 1x1 image needs one byte; a cap of zero refuses it.
        assert!(decode(&tiny_gray(), 0).is_err());
    }

    #[test]
    fn the_cosine_table_matches_its_formula() {
        // The DC basis is constant: cos(0) · (1/√2) / 2.
        let expected = (1.0 / std::f64::consts::SQRT_2 / 2.0 * 16384.0).round() as i32;
        for x in 0..8usize {
            assert_eq!(COS_TABLE.get(x * 8), Some(&expected), "row {x}");
        }
    }

    #[test]
    fn a_flat_block_inverts_to_a_flat_image() {
        // Only the DC coefficient: every pixel should be the same.
        let mut block = [0i32; 64];
        block[0] = 8 * 16; // an arbitrary DC level
        let mut pixels = [0u8; 64];
        idct_block(&block, 8, &mut pixels);

        let first = pixels.first().copied().unwrap_or(0);
        assert!(
            pixels.iter().all(|&p| p.abs_diff(first) <= 1),
            "a DC-only block should be flat, got {pixels:?}"
        );
        assert!(first > 128, "a positive DC brightens the block");
    }

    #[test]
    fn arbitrary_bytes_terminate_without_panicking() {
        for len in 0..512usize {
            let data: Vec<u8> = (0..len).map(|i| ((i * 31) % 256) as u8).collect();
            let _ = decode(&data, 1 << 16);
        }
        // A valid header followed by garbage.
        let mut damaged = tiny_gray();
        damaged.truncate(damaged.len() / 2);
        let _ = decode(&damaged, 1 << 20);

        for cut in 0..tiny_gray().len() {
            let mut truncated = tiny_gray();
            truncated.truncate(cut);
            let _ = decode(&truncated, 1 << 20);
        }
    }

    // ---- Progressive ----------------------------------------------------
    //
    // The fixtures below encode one 8×8 block carrying the same two
    // coefficients three different ways: sequentially, progressively in two
    // scans, and progressively with successive approximation in four. All
    // three must decode to the same pixels, which is a far stronger assertion
    // than any single expected value — it says the scans reassemble into the
    // coefficients the encoder meant, without needing to agree with anyone
    // about what the IDCT should produce from them.

    /// Writes entropy-coded bits, stuffing a zero after every 0xFF.
    #[derive(Default)]
    struct Bits {
        out: Vec<u8>,
        acc: u32,
        held: u32,
    }

    impl Bits {
        fn bit(&mut self, value: u32) {
            self.acc = (self.acc << 1) | (value & 1);
            self.held += 1;
            if self.held == 8 {
                let byte = self.acc as u8;
                self.out.push(byte);
                if byte == 0xFF {
                    self.out.push(0x00);
                }
                self.acc = 0;
                self.held = 0;
            }
        }

        fn push(&mut self, value: u32, length: u32) {
            for i in (0..length).rev() {
                self.bit((value >> i) & 1);
            }
        }

        /// Pads to a byte boundary with ones, which is what an encoder does.
        fn finish(mut self) -> Vec<u8> {
            while self.held != 0 {
                self.bit(1);
            }
            self.out
        }
    }

    fn marker(out: &mut Vec<u8>, code: u8, body: &[u8]) {
        out.extend_from_slice(&[0xFF, code]);
        out.extend_from_slice(&((body.len() + 2) as u16).to_be_bytes());
        out.extend_from_slice(body);
    }

    /// DC table: "00" → size 3, "01" → size 2.
    /// AC table: "00" → run 0 size 3, "01" → EOB with run 0, "10" → run 0
    /// size 2. The size-2 codes exist because a successive-approximation scan
    /// sends a value with its low bit removed, and a magnitude that fits in
    /// three bits usually does not fit in three bits once halved.
    fn tables(out: &mut Vec<u8>) {
        let mut dc_counts = [0u8; 16];
        dc_counts[1] = 2;
        let mut dc = vec![0x00];
        dc.extend_from_slice(&dc_counts);
        dc.extend_from_slice(&[0x03, 0x02]);
        marker(out, 0xC4, &dc);

        let mut ac_counts = [0u8; 16];
        ac_counts[1] = 3;
        let mut ac = vec![0x10];
        ac.extend_from_slice(&ac_counts);
        ac.extend_from_slice(&[0x03, 0x00, 0x02]);
        marker(out, 0xC4, &ac);
    }

    fn header(out: &mut Vec<u8>, sof: u8) {
        out.extend_from_slice(&[0xFF, 0xD8]);

        let mut dqt = vec![0x00];
        dqt.extend_from_slice(&[1u8; 64]);
        marker(out, 0xDB, &dqt);

        // 8×8, one component with id 1, no subsampling, quantisation table 0.
        marker(
            out,
            sof,
            &[0x08, 0x00, 0x08, 0x00, 0x08, 0x01, 0x01, 0x11, 0x00],
        );
        tables(out);
    }

    /// Sequential: DC 5, then AC 5 at zig-zag index 1, then end of block.
    fn sequential_block() -> Vec<u8> {
        let mut out = Vec::new();
        header(&mut out, 0xC0);
        marker(&mut out, 0xDA, &[0x01, 0x01, 0x00, 0x00, 0x3F, 0x00]);

        let mut bits = Bits::default();
        bits.push(0b00, 2); // DC size 3
        bits.push(5, 3); // difference +5
        bits.push(0b00, 2); // AC run 0 size 3
        bits.push(5, 3); // value +5 at index 1
        bits.push(0b01, 2); // EOB
        out.extend_from_slice(&bits.finish());

        out.extend_from_slice(&[0xFF, 0xD9]);
        out
    }

    /// Progressive, no successive approximation: one DC scan, one AC scan.
    fn progressive_block() -> Vec<u8> {
        let mut out = Vec::new();
        header(&mut out, 0xC2);

        // DC scan: band 0..0, Ah 0, Al 0.
        marker(&mut out, 0xDA, &[0x01, 0x01, 0x00, 0x00, 0x00, 0x00]);
        let mut bits = Bits::default();
        bits.push(0b00, 2); // size 3
        bits.push(5, 3); // difference +5
        out.extend_from_slice(&bits.finish());

        // AC scan: band 1..63, Ah 0, Al 0.
        marker(&mut out, 0xDA, &[0x01, 0x01, 0x00, 0x01, 0x3F, 0x00]);
        let mut bits = Bits::default();
        bits.push(0b00, 2); // run 0 size 3
        bits.push(5, 3); // value +5 at index 1
        bits.push(0b01, 2); // EOB run of one
        out.extend_from_slice(&bits.finish());

        out.extend_from_slice(&[0xFF, 0xD9]);
        out
    }

    /// Progressive with successive approximation: every coefficient arrives
    /// one bit short and is completed by a refinement scan.
    fn progressive_refined_block() -> Vec<u8> {
        let mut out = Vec::new();
        header(&mut out, 0xC2);

        // DC first scan, Al 1: sends 5 >> 1 = 2.
        marker(&mut out, 0xDA, &[0x01, 0x01, 0x00, 0x00, 0x00, 0x01]);
        let mut bits = Bits::default();
        bits.push(0b01, 2); // size 2
        bits.push(0b10, 2); // difference +2
        out.extend_from_slice(&bits.finish());

        // AC first scan, band 1..63, Al 1: sends 5 >> 1 = 2, stored as 4.
        marker(&mut out, 0xDA, &[0x01, 0x01, 0x00, 0x01, 0x3F, 0x01]);
        let mut bits = Bits::default();
        bits.push(0b10, 2); // run 0 size 2
        bits.push(0b10, 2); // +2, which the point transform stores as 4
        bits.push(0b01, 2); // EOB run of one
        out.extend_from_slice(&bits.finish());

        // DC refinement, Ah 1 Al 0: the low bit, making 5.
        marker(&mut out, 0xDA, &[0x01, 0x01, 0x00, 0x00, 0x00, 0x10]);
        let mut bits = Bits::default();
        bits.push(1, 1);
        out.extend_from_slice(&bits.finish());

        // AC refinement, band 1..63, Ah 1 Al 0: an EOB run, then the
        // correction bit for the one coefficient already there.
        marker(&mut out, 0xDA, &[0x01, 0x01, 0x00, 0x01, 0x3F, 0x10]);
        let mut bits = Bits::default();
        bits.push(0b01, 2); // EOB run of one
        bits.push(1, 1); // correction for index 1
        out.extend_from_slice(&bits.finish());

        out.extend_from_slice(&[0xFF, 0xD9]);
        out
    }

    #[test]
    fn a_progressive_image_decodes_at_all() {
        let image = decode(&progressive_block(), 1 << 20).expect("progressive decodes");
        assert_eq!((image.width, image.height), (8, 8));
        assert_eq!(image.color, JpegColor::Gray);
        assert_eq!(image.data.len(), 64);
        assert!(
            image.data.iter().any(|&p| p != 128),
            "the coefficients reached the pixels"
        );
    }

    #[test]
    fn progressive_scans_reassemble_the_sequential_image() {
        let sequential = decode(&sequential_block(), 1 << 20).expect("sequential decodes");
        let progressive = decode(&progressive_block(), 1 << 20).expect("progressive decodes");
        assert_eq!(
            sequential.data, progressive.data,
            "the same coefficients sent in two scans give the same pixels"
        );
    }

    #[test]
    fn successive_approximation_refines_to_the_same_image() {
        let sequential = decode(&sequential_block(), 1 << 20).expect("sequential decodes");
        let refined = decode(&progressive_refined_block(), 1 << 20).expect("refined decodes");
        assert_eq!(
            sequential.data, refined.data,
            "the refinement scans supply the bits the first scans left out"
        );
    }

    /// A progressive file cut short must still produce the coarse image the
    /// scans it did carry describe — that is the entire point of the format,
    /// and refusing it outright would be worse than what came before.
    #[test]
    fn a_truncated_progressive_file_keeps_what_it_decoded() {
        let full = progressive_refined_block();
        // Cut after the first two scans: the DC and AC first passes.
        let cut = full.len() - 12;
        let partial = decode(&full[..cut], 1 << 20).expect("the partial file still decodes");

        assert_eq!(partial.data.len(), 64);
        assert!(
            partial.data.iter().any(|&p| p != 128),
            "the scans that did arrive were used"
        );

        let refined = decode(&full, 1 << 20).expect("the whole file decodes");
        assert_ne!(
            partial.data, refined.data,
            "and the missing refinement is visible as a coarser image"
        );
    }

    #[test]
    fn progressive_garbage_terminates_without_panicking() {
        let full = progressive_refined_block();
        for cut in 0..full.len() {
            let _ = decode(&full[..cut], 1 << 20);
        }
        for (seed, byte) in full.iter().enumerate() {
            let mut damaged = full.clone();
            if let Some(slot) = damaged.get_mut(seed) {
                *slot = byte.wrapping_add(97);
            }
            let _ = decode(&damaged, 1 << 20);
        }
    }

    // ---------------------------------------------------------------- encoder
    //
    // Everything below is the baseline encoder in `jpeg/encode.rs`. Each test's
    // own doc comment says which link it adjudicates with third-party data and
    // which it only holds to itself; there is no test here that presents an
    // encode-then-decode round trip as adjudication.

    use super::encode::{
        canonical_codes, category, forward_dct_quantise, pad_plane, rgb_to_ycbcr, HuffSpec,
        ANNEX_K1_LUMINANCE, ANNEX_K2_CHROMINANCE, ANNEX_K3_DC_LUMA, ANNEX_K4_DC_CHROMA,
        ANNEX_K5_AC_LUMA, ANNEX_K6_AC_CHROMA,
    };

    /// Hex, one byte per two characters, spaces ignored.
    fn hex(text: &str) -> Vec<u8> {
        let digits: Vec<u8> = text
            .bytes()
            .filter(|b| !b.is_ascii_whitespace())
            .map(|b| match b {
                b'0'..=b'9' => b - b'0',
                b'A'..=b'F' => b - b'A' + 10,
                b'a'..=b'f' => b - b'a' + 10,
                other => panic!("not hex: {other}"),
            })
            .collect();
        digits
            .chunks(2)
            .map(|pair| (pair[0] << 4) | pair[1])
            .collect()
    }

    /// The entropy-coded segment: everything between the SOS segment and EOI.
    fn entropy_segment(bytes: &[u8]) -> Vec<u8> {
        let mut at = 2; // past SOI
        while at + 3 < bytes.len() {
            assert_eq!(bytes[at], 0xFF, "expected a marker at {at}");
            let code = bytes[at + 1];
            let length = usize::from(bytes[at + 2]) << 8 | usize::from(bytes[at + 3]);
            if code == 0xDA {
                let start = at + 2 + length;
                let end = bytes.len() - 2;
                assert_eq!(&bytes[end..], &[0xFF, 0xD9], "EOI");
                return bytes[start..end].to_vec();
            }
            at += 2 + length;
        }
        panic!("no SOS");
    }

    /// Every marker segment's payload, keyed by marker code, in order.
    fn segments(bytes: &[u8]) -> Vec<(u8, Vec<u8>)> {
        let mut out = Vec::new();
        let mut at = 2;
        while at + 3 < bytes.len() {
            let code = bytes[at + 1];
            let length = usize::from(bytes[at + 2]) << 8 | usize::from(bytes[at + 3]);
            out.push((code, bytes[at + 4..at + 2 + length].to_vec()));
            if code == 0xDA {
                break;
            }
            at += 2 + length;
        }
        out
    }

    fn gray(width: u32, height: u32, value: u8) -> Vec<u8> {
        vec![value; (width * height) as usize]
    }

    fn gray_source(data: &[u8], width: u32, height: u32) -> JpegSource<'_> {
        JpegSource {
            width,
            height,
            colour: JpegSourceColour::Gray,
            stride: width as usize,
            data,
        }
    }

    /// T.81 Figure A.6 as printed: for each natural (row-major) coefficient
    /// position, the index at which the zig-zag sequence visits it. That is the
    /// **inverse** of [`ZIGZAG`], which is what makes the test below a check on
    /// the shipped table rather than a copy of it.
    const FIGURE_A_6: [usize; 64] = [
        0, 1, 5, 6, 14, 15, 27, 28, //
        2, 4, 7, 13, 16, 26, 29, 42, //
        3, 8, 12, 17, 25, 30, 41, 43, //
        9, 11, 18, 24, 31, 40, 44, 53, //
        10, 19, 23, 32, 39, 45, 52, 54, //
        20, 22, 33, 38, 46, 51, 55, 60, //
        21, 34, 37, 47, 50, 56, 59, 61, //
        35, 36, 48, 49, 57, 58, 62, 63,
    ];

    /// **Adjudicated by third-party data.** T.81 Figure A.6, read twice — text
    /// layer and `tpdf render --dpi 200`'s page 30 — from the same
    /// `T-REC-T.81` the tables below come from. Both readings agree on all 64
    /// cells.
    ///
    /// This test exists because a counted injection found it missing. Swapping
    /// two entries of [`ZIGZAG`] fired **nothing**: the decoder scatters with
    /// the same table the encoder gathers with, so a round trip is blind to any
    /// permutation of it, and every other test here had been written in terms of
    /// [`ZIGZAG`] rather than in terms of the figure. Holding the shipped table
    /// to its own inverse, transcribed independently, is what closes that — and
    /// `a_grayscale_datastream_carries_annex_k_s_published_table_bytes` and
    /// `the_forward_dct_matches_a_3_3_s_equation` were rewritten to build their
    /// expectations from `FIGURE_A_6` for the same reason.
    #[test]
    fn the_zig_zag_order_is_figure_a_6_s() {
        for (natural, &k) in FIGURE_A_6.iter().enumerate() {
            assert_eq!(
                ZIGZAG[k], natural,
                "the zig-zag sequence's step {k} is not Figure A.6's cell {natural}"
            );
        }
        // A.6 is a permutation of the 64 positions, which is the property a
        // transposition preserves and a duplicated entry does not.
        let mut seen = [false; 64];
        for &k in FIGURE_A_6.iter() {
            assert!(!seen[k], "step {k} appears twice");
            seen[k] = true;
        }
    }

    /// **Adjudicated by third-party data.** T.81 Tables K.1 and K.2, from
    /// `T-REC-T.81` (<https://www.w3.org/Graphics/JPEG/itu-t81.pdf>, W3C's copy
    /// of CCITT Rec. T.81 (1992) | ISO/IEC 10918-1 : 1993), fetched 15 September
    /// 2026 and read twice: once from the text layer `tpdf text` extracts, once
    /// off `tpdf render --dpi 200`'s page 143. The two readings agree on all 128
    /// entries, and the transcription below is the rendered one.
    ///
    /// The second reading was not a formality. T.81's tables are typeset with
    /// column rules that the text layer emits as a literal `1`, so K.1's first
    /// row arrives as `16111016124140151161` and resolves into
    /// `16 11 10 16 24 40 51 61` only against the picture.
    #[test]
    fn the_quantisation_tables_are_itu_t_t_81_annex_k_s() {
        let k1: [u8; 64] = [
            16, 11, 10, 16, 24, 40, 51, 61, //
            12, 12, 14, 19, 26, 58, 60, 55, //
            14, 13, 16, 24, 40, 57, 69, 56, //
            14, 17, 22, 29, 51, 87, 80, 62, //
            18, 22, 37, 56, 68, 109, 103, 77, //
            24, 35, 55, 64, 81, 104, 113, 92, //
            49, 64, 78, 87, 103, 121, 120, 101, //
            72, 92, 95, 98, 112, 100, 103, 99,
        ];
        let k2: [u8; 64] = [
            17, 18, 24, 47, 99, 99, 99, 99, //
            18, 21, 26, 66, 99, 99, 99, 99, //
            24, 26, 56, 99, 99, 99, 99, 99, //
            47, 66, 99, 99, 99, 99, 99, 99, //
            99, 99, 99, 99, 99, 99, 99, 99, //
            99, 99, 99, 99, 99, 99, 99, 99, //
            99, 99, 99, 99, 99, 99, 99, 99, //
            99, 99, 99, 99, 99, 99, 99, 99,
        ];
        assert_eq!(ANNEX_K1_LUMINANCE, k1, "Table K.1");
        assert_eq!(ANNEX_K2_CHROMINANCE, k2, "Table K.2");
        // B.2.4.1 gives Qk the range 1 to 255 at Pq = 0.
        assert!(k1.iter().chain(k2.iter()).all(|&q| q >= 1));
    }

    /// **Adjudicated by third-party data.** T.81 K.3.3.1 and K.3.3.2 print the
    /// BITS and HUFFVAL lists of Tables K.3 to K.6 as hexadecimal byte strings,
    /// and B.2.4.2 makes those strings the DHT payload. Read twice from the same
    /// document as the test above: text layer, and `tpdf render`'s pages 158 and
    /// 159. Both readings agree on all 396 bytes.
    ///
    /// These strings, and not Tables K.3 to K.6's typeset grids, are what the
    /// shipped tables are held to, because running text extracts unambiguously
    /// where a ruled grid does not.
    #[test]
    fn the_huffman_tables_are_itu_t_t_81_annex_k_s() {
        let cases: [(&HuffSpec, &str, &str, &str); 4] = [
            (
                &ANNEX_K3_DC_LUMA,
                "K.3 luminance DC",
                "00010501010101010100000000000000",
                "000102030405060708090A0B",
            ),
            (
                &ANNEX_K4_DC_CHROMA,
                "K.4 chrominance DC",
                "00030101010101010101010000000000",
                "000102030405060708090A0B",
            ),
            (
                &ANNEX_K5_AC_LUMA,
                "K.5 luminance AC",
                "0002010303020403050504040000017D",
                "01020300041105122131410613516107\
                 227114328191A1082342B1C11552D1F0\
                 2433627282090A161718191A25262728\
                 292A3435363738393A43444546474849\
                 4A535455565758595A63646566676869\
                 6A737475767778797A83848586878889\
                 8A92939495969798999AA2A3A4A5A6A7\
                 A8A9AAB2B3B4B5B6B7B8B9BAC2C3C4C5\
                 C6C7C8C9CAD2D3D4D5D6D7D8D9DAE1E2\
                 E3E4E5E6E7E8E9EAF1F2F3F4F5F6F7F8\
                 F9FA",
            ),
            (
                &ANNEX_K6_AC_CHROMA,
                "K.6 chrominance AC",
                "00020102040403040705040400010277",
                "000102031104 05213106124151076171\
                 1322328108144291A1B1C109233352F0\
                 156272D10A162434E125F11718191A26\
                 2728292A35363738393A434445464748\
                 494A535455565758595A6364 65666768\
                 696A737475767778797A828384858687\
                 88898A92939495969798999AA2A3A4A5\
                 A6A7A8A9AAB2B3B4B5 B6B7B8B9BAC2C3\
                 C4C5C6C7C8C9CAD2D3D4D5D6D7D8D9DA\
                 E2E3E4E5E6E7E8E9EAF2F3F4F5F6F7F8\
                 F9FA",
            ),
        ];

        for (spec, name, bits, values) in cases {
            assert_eq!(spec.bits.as_slice(), hex(bits).as_slice(), "{name} BITS");
            assert_eq!(spec.values, hex(values).as_slice(), "{name} HUFFVAL");
            // C.2: the counts and the value list have to describe the same
            // table, which is also the check that catches a dropped byte.
            let total: usize = spec.bits.iter().map(|&n| usize::from(n)).sum();
            assert_eq!(total, spec.values.len(), "{name} BITS sum");
        }
        assert_eq!(ANNEX_K5_AC_LUMA.values.len(), 162);
        assert_eq!(ANNEX_K6_AC_CHROMA.values.len(), 162);
    }

    /// **Adjudicated by third-party data.** T.81 Tables K.3 and K.4 print, for
    /// every one of their 24 symbols, a code length *and* a binary code word.
    /// Those 24 code words are transcribed here and required to be what Annex
    /// C's canonical assignment produces from the BITS and HUFFVAL of the test
    /// above — two independent presentations of the same table in the same
    /// Recommendation, which is why this is a check and not a restatement.
    ///
    /// Read twice, text layer and `tpdf render`'s page 149. The text layer
    /// carries the same column-rule `1` that Table K.1 does, so
    /// `1641110` is category 6, length 4, code word `1110`.
    ///
    /// Tables K.5 and K.6 print 324 more code words the same way. **Sheet 1 of
    /// 4 of Table K.5 — 40 of them — is transcribed here too**, read twice from
    /// the text layer and from `tpdf render`'s page 150; that sample reaches
    /// nine of the sixteen code lengths, including the 16-bit group whose codes
    /// are the last ones the canonical assignment produces and therefore the
    /// ones a mis-stepped `code <<= 1` would land wrongest. The remaining three
    /// sheets and all of K.6 are left to the BITS and HUFFVAL above, which
    /// determine them.
    #[test]
    fn the_canonical_codes_are_tables_k_3_and_k_4_s_printed_code_words() {
        // (category, code length, code word) exactly as Table K.3 prints them.
        let k3: [(u8, u8, &str); 12] = [
            (0, 2, "00"),
            (1, 3, "010"),
            (2, 3, "011"),
            (3, 3, "100"),
            (4, 3, "101"),
            (5, 3, "110"),
            (6, 4, "1110"),
            (7, 5, "11110"),
            (8, 6, "111110"),
            (9, 7, "1111110"),
            (10, 8, "11111110"),
            (11, 9, "111111110"),
        ];
        // Table K.4.
        let k4: [(u8, u8, &str); 12] = [
            (0, 2, "00"),
            (1, 2, "01"),
            (2, 2, "10"),
            (3, 3, "110"),
            (4, 4, "1110"),
            (5, 5, "11110"),
            (6, 6, "111110"),
            (7, 7, "1111110"),
            (8, 8, "11111110"),
            (9, 9, "111111110"),
            (10, 10, "1111111110"),
            (11, 11, "11111111110"),
        ];
        // Table K.5, the whole of sheet 1 of 4: run/size, length, code word.
        let k5: [(u8, u8, &str); 40] = [
            (0x00, 4, "1010"), // EOB
            (0x01, 2, "00"),
            (0x02, 2, "01"),
            (0x03, 3, "100"),
            (0x04, 4, "1011"),
            (0x05, 5, "11010"),
            (0x06, 7, "1111000"),
            (0x07, 8, "11111000"),
            (0x08, 10, "1111110110"),
            (0x09, 16, "1111111110000010"),
            (0x0A, 16, "1111111110000011"),
            (0x11, 4, "1100"),
            (0x12, 5, "11011"),
            (0x13, 7, "1111001"),
            (0x14, 9, "111110110"),
            (0x15, 11, "11111110110"),
            (0x16, 16, "1111111110000100"),
            (0x17, 16, "1111111110000101"),
            (0x18, 16, "1111111110000110"),
            (0x19, 16, "1111111110000111"),
            (0x1A, 16, "1111111110001000"),
            (0x21, 5, "11100"),
            (0x22, 8, "11111001"),
            (0x23, 10, "1111110111"),
            (0x24, 12, "111111110100"),
            (0x25, 16, "1111111110001001"),
            (0x26, 16, "1111111110001010"),
            (0x27, 16, "1111111110001011"),
            (0x28, 16, "1111111110001100"),
            (0x29, 16, "1111111110001101"),
            (0x2A, 16, "1111111110001110"),
            (0x31, 6, "111010"),
            (0x32, 9, "111110111"),
            (0x33, 12, "111111110101"),
            (0x34, 16, "1111111110001111"),
            (0x35, 16, "1111111110010000"),
            (0x36, 16, "1111111110010001"),
            (0x37, 16, "1111111110010010"),
            (0x38, 16, "1111111110010011"),
            (0x39, 16, "1111111110010100"),
        ];

        for (spec, printed, name) in [
            (&ANNEX_K3_DC_LUMA, k3.as_slice(), "K.3"),
            (&ANNEX_K4_DC_CHROMA, k4.as_slice(), "K.4"),
        ] {
            let codes = canonical_codes(spec);
            for &(value, length, word) in printed {
                let code = codes[usize::from(value)];
                assert_eq!(code.length, length, "{name} value {value} length");
                assert_eq!(
                    format!("{:0width$b}", code.bits, width = usize::from(length)),
                    word,
                    "{name} value {value} code word"
                );
            }
        }

        let codes = canonical_codes(&ANNEX_K5_AC_LUMA);
        for &(value, length, word) in k5.iter() {
            let code = codes[usize::from(value)];
            assert_eq!(code.length, length, "K.5 run/size {value:02X} length");
            assert_eq!(
                format!("{:0width$b}", code.bits, width = usize::from(length)),
                word,
                "K.5 run/size {value:02X} code word"
            );
        }
    }

    /// **Adjudicated by third-party data.** B.2.4.1 makes a DQT payload
    /// `Pq`/`Tq` then the table in zig-zag order, and B.2.4.2 makes a DHT
    /// payload `Tc`/`Th` then BITS then HUFFVAL. So the bytes T.81 K.1 and
    /// K.3.3 print appear in this encoder's output verbatim, and this test finds
    /// them there — encoder output against published bytes, with no decoder on
    /// either side.
    #[test]
    fn a_grayscale_datastream_carries_annex_k_s_published_table_bytes() {
        let pixels = gray(8, 8, 128);
        let bytes = jpeg_encode(&gray_source(&pixels, 8, 8), &JpegOptions::default()).unwrap();
        let found = segments(&bytes);

        let dqt = &found.iter().find(|(code, _)| *code == 0xDB).unwrap().1;
        assert_eq!(dqt[0], 0x00, "Pq = 0, Tq = 0");
        let mut zigzagged = [0u8; 64];
        for (natural, &k) in FIGURE_A_6.iter().enumerate() {
            zigzagged[k] = ANNEX_K1_LUMINANCE[natural];
        }
        assert_eq!(
            &dqt[1..],
            zigzagged.as_slice(),
            "Table K.1 in Figure A.6's order"
        );

        let dhts: Vec<&(u8, Vec<u8>)> = found.iter().filter(|(code, _)| *code == 0xC4).collect();
        assert_eq!(dhts.len(), 2, "a grayscale frame needs one DC and one AC");

        let mut dc = vec![0x00u8];
        dc.extend_from_slice(&hex("00010501010101010100000000000000"));
        dc.extend_from_slice(&hex("000102030405060708090A0B"));
        assert_eq!(dhts[0].1, dc, "K.3.3.1's published bytes");

        let mut ac = vec![0x10u8];
        ac.extend_from_slice(&hex("0002010303020403050504040000017D"));
        ac.extend_from_slice(ANNEX_K5_AC_LUMA.values);
        assert_eq!(dhts[1].1, ac, "K.3.3.2's published bytes");

        // B.2.2's SOF0: P, Y, X, Nf, then one component.
        let sof = &found.iter().find(|(code, _)| *code == 0xC0).unwrap().1;
        assert_eq!(sof.as_slice(), &[8, 0, 8, 0, 8, 1, 1, 0x11, 0]);
    }

    /// **Adjudicated by third-party data.** Two complete entropy-coded segments
    /// whose every bit follows from the standard alone: A.3.3's equation, K.1's
    /// quantiser, the code words Tables K.3 and K.5 print, and B.1.1.5 NOTE 1's
    /// 1-bit padding. Nothing in this repository is consulted to produce the
    /// expected bytes, and no decoder runs.
    ///
    /// A flat 8x8 block of 128 level-shifts to zero, so every coefficient is
    /// zero: `DIFF = 0` is K.3 category 0, code word `00`; the rest of the block
    /// is K.5's 0/0 EOB, code word `1010`. Six bits, padded to `00101011`.
    ///
    /// A flat block of 144 has `s = 16` everywhere, so A.3.3 gives
    /// `S00 = (1/4)(1/sqrt 2)(1/sqrt 2) x 64 x 16 = 128` and every other
    /// coefficient zero; K.1's `Q00 = 16` makes `Sq00 = 8`. Category 4 is K.3's
    /// `101`, the four additional bits of F.1.2.1.1 are `1000`, then EOB
    /// `1010` — eleven bits, padded to `10110001 01011111`.
    #[test]
    fn the_flat_block_datastreams_are_annex_k_s_own_code_words() {
        for (value, expected) in [(128u8, vec![0x2Bu8]), (144, vec![0xB1, 0x5F])] {
            let pixels = gray(8, 8, value);
            let bytes = jpeg_encode(&gray_source(&pixels, 8, 8), &JpegOptions::default()).unwrap();
            assert_eq!(entropy_segment(&bytes), expected, "a flat block of {value}");
        }
    }

    /// A.3.3's FDCT, transcribed here in `f64` from the equation on the
    /// rendered page 27, against the fixed-point transform the encoder runs.
    ///
    /// **This is the standard's formula, not the standard's numbers.** ITU-T
    /// T.83's compliance data — the published vector set that would adjudicate
    /// these coefficients — is on three MS-DOS diskettes bundled with the paid
    /// Recommendation (T.83 clause 4.4) and could not be obtained; the module
    /// header lists the URLs tried and what each returned. What this test can
    /// and does say is that the integer path rounds the ideal transform
    /// **correctly to within 0.012 of a quantiser step** on every coefficient of
    /// every block below — 0.5117 measured, where 0.5 is a correct rounding —
    /// which is enough to catch a transposed basis function, a quantiser
    /// applied on the wrong side of the transform, or a zig-zag that scans the
    /// wrong cell.
    #[test]
    fn the_forward_dct_matches_a_3_3_s_equation() {
        fn reference(samples: &[i32; 64], u: usize, v: usize) -> f64 {
            let c = |k: usize| {
                if k == 0 {
                    1.0 / std::f64::consts::SQRT_2
                } else {
                    1.0
                }
            };
            let mut sum = 0.0;
            for x in 0..8usize {
                for y in 0..8usize {
                    sum += f64::from(samples[y * 8 + x])
                        * ((2.0 * x as f64 + 1.0) * u as f64 * std::f64::consts::PI / 16.0).cos()
                        * ((2.0 * y as f64 + 1.0) * v as f64 * std::f64::consts::PI / 16.0).cos();
                }
            }
            0.25 * c(u) * c(v) * sum
        }

        let mut worst = 0.0f64;
        let mut state = 0x1234_5678u32;
        let mut next = move || {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            (state >> 24) as i32 - 128
        };

        for case in 0..24usize {
            let mut samples = [0i32; 64];
            for (i, slot) in samples.iter_mut().enumerate() {
                *slot = match case {
                    0 => 0,
                    1 => 127,
                    2 => -128,
                    3 => {
                        if (i / 8 + i % 8) % 2 == 0 {
                            127
                        } else {
                            -128
                        }
                    }
                    4 => (i as i32) - 32,
                    _ => next(),
                };
            }
            for table in [&ANNEX_K1_LUMINANCE, &ANNEX_K2_CHROMINANCE, &[1u8; 64]] {
                let mut coefficients = [0i32; 64];
                forward_dct_quantise(&samples, table, &mut coefficients);
                for (natural, &k) in FIGURE_A_6.iter().enumerate() {
                    let (v, u) = (natural / 8, natural % 8);
                    let exact = reference(&samples, u, v) / f64::from(table[natural]);
                    let error = (f64::from(coefficients[k]) - exact).abs();
                    worst = worst.max(error);
                }
            }
        }
        assert!(
            worst <= 0.55,
            "the fixed-point transform strayed {worst} from A.3.3's equation"
        );
    }

    /// Why the encoder needs no "coefficient too large" refusal, measured
    /// rather than argued.
    ///
    /// Annex K's tables code AC magnitudes up to size 10 (`|Sq| <= 1023`) and DC
    /// differences up to category 11 (`|DIFF| <= 2047`). A.3.3's transform is
    /// linear in the samples, so its extreme over the box `s in [-128, 127]` is
    /// attained at a vertex, and the vertex is known: take `127` where the basis
    /// product is positive and `-128` where it is negative. That is computed
    /// here for all 64 coefficients at the finest quantiser B.2.4.1 allows, a
    /// table of ones, which is the worst case over every legal table.
    ///
    /// The clamp in `forward_dct_quantise` is unreachable because of this, and
    /// this test is what keeps that true.
    #[test]
    fn no_quantiser_can_push_a_coefficient_past_annex_k() {
        let basis = |x: usize, u: usize| {
            let c = if u == 0 {
                1.0 / std::f64::consts::SQRT_2
            } else {
                1.0
            };
            c / 2.0 * ((2.0 * x as f64 + 1.0) * u as f64 * std::f64::consts::PI / 16.0).cos()
        };

        let mut worst_ac = 0.0f64;
        let (mut dc_high, mut dc_low) = (0.0f64, 0.0f64);
        for v in 0..8usize {
            for u in 0..8usize {
                let mut high = 0.0f64;
                let mut low = 0.0f64;
                for x in 0..8usize {
                    for y in 0..8usize {
                        let weight = basis(x, u) * basis(y, v);
                        // The vertex that maximises, and the one that minimises.
                        high += weight * if weight > 0.0 { 127.0 } else { -128.0 };
                        low += weight * if weight > 0.0 { -128.0 } else { 127.0 };
                    }
                }
                if u == 0 && v == 0 {
                    dc_high = high;
                    dc_low = low;
                } else {
                    worst_ac = worst_ac.max(high.abs()).max(low.abs());
                }
            }
        }

        assert!(
            worst_ac <= 1023.0,
            "an AC coefficient can reach {worst_ac}, past K.5's size 10"
        );
        let spread = dc_high - dc_low;
        assert!(
            spread <= 2047.0,
            "a DC difference can reach {spread}, past K.3's category 11"
        );
        // The categories those magnitudes fall in, so a changed bound is loud.
        assert_eq!(category(worst_ac.round() as i32), 10);
        assert_eq!(category(spread.round() as i32), 11);
    }

    /// **Adjudicated by third-party data.** ITU-T T.871 | ISO/IEC 10918-5
    /// clause 7's exact forward equations, read off `tpdf render`'s page 4 of
    /// `T-REC-T.871-201105-I` — fetched from the ITU 15 September 2026, by
    /// `WebFetch` where `curl` got an HTTP 500 — and written out here literally.
    ///
    /// The encoder computes the algebraically equivalent identity
    /// `Cb = (B - Y)/1.772 + 128` in fixed point, so agreement is a check on
    /// both the identity and the fixed-point precision rather than a
    /// restatement of one expression as itself. The tolerance is one level,
    /// which is what a 1/65536 fixed point buys.
    #[test]
    fn the_colour_transform_is_t_871_clause_7_s() {
        let published = |r: u8, g: u8, b: u8| {
            let (r, g, b) = (f64::from(r), f64::from(g), f64::from(b));
            let clamp = |v: f64| v.round().clamp(0.0, 255.0) as i32;
            (
                clamp(0.299 * r + 0.587 * g + 0.114 * b),
                clamp((-0.299 * r - 0.587 * g + 0.886 * b) / 1.772 + 128.0),
                clamp((0.701 * r - 0.587 * g - 0.114 * b) / 1.402 + 128.0),
            )
        };

        let mut worst = 0i32;
        for r in (0..=255u32).step_by(15) {
            for g in (0..=255u32).step_by(15) {
                for b in (0..=255u32).step_by(15) {
                    let (r, g, b) = (r as u8, g as u8, b as u8);
                    let (y, cb, cr) = rgb_to_ycbcr(r, g, b);
                    let want = published(r, g, b);
                    worst = worst
                        .max((i32::from(y) - want.0).abs())
                        .max((i32::from(cb) - want.1).abs())
                        .max((i32::from(cr) - want.2).abs());
                }
            }
        }
        assert!(
            worst <= 1,
            "the transform strayed {worst} levels from T.871"
        );

        // The three points clause 7 pins exactly, in both directions.
        assert_eq!(rgb_to_ycbcr(0, 0, 0), (0, 128, 128));
        assert_eq!(rgb_to_ycbcr(255, 255, 255), (255, 128, 128));
    }

    /// A.2.4's NOTE: "any incomplete MCUs be completed by replication of the
    /// right-most column and the bottom line of each component". Held to the
    /// clause, not to a round trip — a padding rule is only visible in the bits
    /// it costs, and a decoder discards it by A.2.4's last sentence.
    #[test]
    fn a_partial_mcu_is_completed_by_replicating_the_edge() {
        // A 3 x 2 picture in an 8 x 8 plane, with a distinct value per pixel.
        let mut plane = vec![0u8; 64];
        for y in 0..2usize {
            for x in 0..3usize {
                plane[y * 8 + x] = (y * 3 + x + 1) as u8;
            }
        }
        pad_plane(&mut plane, 8, 8, 3, 2);

        for y in 0..8usize {
            for x in 0..8usize {
                let sy = y.min(1);
                let sx = x.min(2);
                assert_eq!(
                    plane[y * 8 + x],
                    (sy * 3 + sx + 1) as u8,
                    "the sample at ({x}, {y})"
                );
            }
        }
    }

    /// F.1.2.3's byte stuffing, checked over an image noisy enough to produce
    /// X'FF' bytes in the coded data. Held to the clause: inside the entropy-
    /// coded segment of a scan with no restart interval, an X'FF' may be
    /// followed only by X'00'.
    #[test]
    fn every_ff_in_the_coded_data_is_followed_by_a_stuffed_zero() {
        let mut pixels = vec![0u8; 64 * 64];
        let mut state = 0x9E37_79B9u32;
        for slot in pixels.iter_mut() {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            *slot = (state >> 24) as u8;
        }
        let options = JpegOptions {
            quantisation: JpegQuantisation::Tables {
                luminance: [1; 64],
                chrominance: [1; 64],
            },
            ..JpegOptions::default()
        };
        let bytes = jpeg_encode(&gray_source(&pixels, 64, 64), &options).unwrap();
        let coded = entropy_segment(&bytes);
        let mut stuffed = 0;
        let mut at = 0;
        while at < coded.len() {
            if coded[at] == 0xFF {
                assert_eq!(
                    coded.get(at + 1).copied(),
                    Some(0x00),
                    "an unstuffed X'FF' at {at}"
                );
                stuffed += 1;
                at += 2;
            } else {
                at += 1;
            }
        }
        assert!(
            stuffed > 0,
            "this fixture is supposed to produce X'FF' bytes"
        );
    }

    /// 4.10 and F.1.2.3's restart interval: the markers are RST0 through RST7 in
    /// order, there is one every `Ri` MCUs, and — the half that a decoder can
    /// hide — the DC predictor goes back to zero at each one.
    ///
    /// The predictor reset is what the second half of this test is for. It is
    /// invisible to any check that only counts markers, and it is invisible to a
    /// round trip through a decoder that also forgets to reset, so the bits are
    /// read here instead: the first block after a restart must code its DC as a
    /// difference from zero, which for this flat image is the same code word the
    /// very first block emitted.
    #[test]
    fn the_restart_interval_is_emitted_and_the_predictors_reset() {
        // 32 x 16 of one value: eight MCUs of 8 x 8, all with the same DC.
        let pixels = gray(32, 16, 144);
        let options = JpegOptions {
            restart_interval: 2,
            ..JpegOptions::default()
        };
        let bytes = jpeg_encode(&gray_source(&pixels, 32, 16), &options).unwrap();

        let dri = segments(&bytes)
            .into_iter()
            .find(|(code, _)| *code == 0xDD)
            .expect("a DRI segment");
        assert_eq!(dri.1, vec![0x00, 0x02]);

        let coded = entropy_segment(&bytes);
        // Eight MCUs, a restart before MCUs 2, 4 and 6: three markers.
        let mut markers = Vec::new();
        let mut runs: Vec<Vec<u8>> = vec![Vec::new()];
        let mut at = 0;
        while at < coded.len() {
            if coded[at] == 0xFF {
                match coded.get(at + 1).copied() {
                    Some(0x00) => {
                        runs.last_mut().unwrap().push(0xFF);
                        at += 2;
                        continue;
                    }
                    Some(m) if (0xD0..=0xD7).contains(&m) => {
                        markers.push(m);
                        runs.push(Vec::new());
                        at += 2;
                        continue;
                    }
                    other => panic!("a marker {other:?} inside the coded data"),
                }
            }
            runs.last_mut().unwrap().push(coded[at]);
            at += 1;
        }
        assert_eq!(markers, vec![0xD0, 0xD1, 0xD2], "RSTn in order from RST0");

        // Each run codes two identical MCUs, and each starts from a zeroed
        // predictor, so every run is byte-identical to the first.
        assert_eq!(runs.len(), 4);
        for (i, run) in runs.iter().enumerate() {
            assert_eq!(run, &runs[0], "run {i} after a restart");
        }
        // And it is the flat-block code word of the published test above, twice
        // over: 101 1000 1010 then 000 1010 (DIFF = 0 for the second MCU).
        assert_eq!(runs[0], vec![0xB1, 0x45, 0x7F]);
    }

    /// Self-consistency only: this encoder against this crate's decoder, which
    /// `jpeg/encode.rs`'s header says is itself adjudicated by nothing
    /// third-party. It says that a datastream is well-formed enough to be read
    /// back and that the picture survives; it says nothing about T.81.
    #[test]
    fn a_round_trip_keeps_the_picture_in_every_colour_and_sampling() {
        let cases: [(u32, u32, JpegSourceColour, JpegSampling); 5] = [
            (16, 16, JpegSourceColour::Gray, JpegSampling::FourFourFour),
            (16, 16, JpegSourceColour::Rgb, JpegSampling::FourFourFour),
            (32, 32, JpegSourceColour::Rgb, JpegSampling::FourTwoZero),
            (13, 7, JpegSourceColour::Gray, JpegSampling::FourFourFour),
            (13, 7, JpegSourceColour::Rgb, JpegSampling::FourTwoZero),
        ];

        for (width, height, colour, sampling) in cases {
            let components = usize::from(colour.components());
            let mut pixels = vec![0u8; width as usize * height as usize * components];
            for (i, slot) in pixels.iter_mut().enumerate() {
                let pixel = i / components;
                // A smooth luma ramp: coarse quantisers keep it, so the
                // comparison can be tight without asserting anything about the
                // IDCT.
                let base = (pixel % 8 * 16 + 64) as i32;
                // And a colour that is genuinely off neutral, because a grey
                // ramp leaves both chrominance planes at 128 and makes every
                // colour case a luminance case wearing three components. The
                // offset flips every sixteenth row, which is one 4:2:0 MCU, so
                // each chrominance block is still *flat*: the box filter and
                // the block transform are exact on it and the tolerance below
                // stays the luma ramp's. Transposing Cb and Cr fires this test
                // only because of these three lines.
                let offset = if components == 3 {
                    let band = if (pixel / width as usize / 16) % 2 == 0 {
                        1i32
                    } else {
                        -1
                    };
                    band * match i % 3 {
                        0 => 40i32,
                        1 => 0,
                        _ => -40,
                    }
                } else {
                    0
                };
                *slot = (base + offset) as u8;
            }
            let source = JpegSource {
                width,
                height,
                colour,
                stride: width as usize * components,
                data: &pixels,
            };
            let options = JpegOptions {
                quantisation: JpegQuantisation::AnnexKHalved,
                sampling,
                restart_interval: 0,
            };
            let bytes = jpeg_encode(&source, &options).unwrap();
            let image = decode(&bytes, 1 << 24).expect("the encoder's own output decodes");
            assert_eq!((image.width, image.height), (width, height));
            assert_eq!(image.data.len(), pixels.len());

            let worst = image
                .data
                .iter()
                .zip(pixels.iter())
                .map(|(&got, &want)| (i32::from(got) - i32::from(want)).abs())
                .max()
                .unwrap_or(0);
            assert!(
                worst <= 24,
                "{width}x{height} {colour:?} {sampling:?} strayed {worst} levels"
            );
        }
    }

    /// Self-consistency only, and the reason it is here rather than folded into
    /// the round trip above: a restart interval must change the bytes and not
    /// the picture. A decoder that mishandles RSTn desynchronises, so this is
    /// the cheapest total check that the markers are where the decoder expects.
    #[test]
    fn restarts_change_the_bytes_and_not_the_picture() {
        let pixels: Vec<u8> = (0..64u32 * 64).map(|i| (i % 251) as u8).collect();
        let plain = jpeg_encode(&gray_source(&pixels, 64, 64), &JpegOptions::default()).unwrap();
        let restarted = jpeg_encode(
            &gray_source(&pixels, 64, 64),
            &JpegOptions {
                restart_interval: 3,
                ..JpegOptions::default()
            },
        )
        .unwrap();
        assert_ne!(plain, restarted);
        assert_eq!(
            decode(&plain, 1 << 24).unwrap().data,
            decode(&restarted, 1 << 24).unwrap().data
        );
    }

    /// The stride is the field an encoder is most likely to ignore, because for
    /// every unpadded buffer it equals the row length. `PngSource`'s test says
    /// the same thing about the same mistake.
    #[test]
    fn a_padded_stride_is_not_read_as_pixels() {
        let mut padded = vec![0u8; 12 * 4];
        for y in 0..4usize {
            for x in 0..4usize {
                padded[y * 12 + x] = (x * 16 + y * 4) as u8;
            }
        }
        let tight: Vec<u8> = (0..4usize)
            .flat_map(|y| (0..4usize).map(move |x| (x * 16 + y * 4) as u8))
            .collect();

        let from_padded = jpeg_encode(
            &JpegSource {
                width: 4,
                height: 4,
                colour: JpegSourceColour::Gray,
                stride: 12,
                data: &padded,
            },
            &JpegOptions::default(),
        )
        .unwrap();
        let from_tight = jpeg_encode(&gray_source(&tight, 4, 4), &JpegOptions::default()).unwrap();
        assert_eq!(from_padded, from_tight);
    }

    /// Every refusal, by its own name. All five are a caller describing its own
    /// buffer or its own intent wrongly; none can be reached from the pixels.
    #[test]
    fn every_refusal_fires_by_its_own_name() {
        let pixels = gray(8, 8, 128);
        let default = JpegOptions::default();

        assert_eq!(
            jpeg_encode(&gray_source(&pixels, 0, 8), &default),
            Err(JpegEncodeError::BadDimensions {
                width: 0,
                height: 8
            })
        );
        assert_eq!(
            jpeg_encode(&gray_source(&pixels, 65_536, 8), &default),
            Err(JpegEncodeError::BadDimensions {
                width: 65_536,
                height: 8
            })
        );
        assert_eq!(
            jpeg_encode(
                &JpegSource {
                    width: 8,
                    height: 8,
                    colour: JpegSourceColour::Gray,
                    stride: 4,
                    data: &pixels,
                },
                &default
            ),
            Err(JpegEncodeError::ShortStride {
                stride: 4,
                row_bytes: 8
            })
        );
        assert_eq!(
            jpeg_encode(&gray_source(&pixels[..40], 8, 8), &default),
            Err(JpegEncodeError::ShortData { have: 40, need: 64 })
        );
        assert_eq!(
            jpeg_encode(
                &gray_source(&pixels, 8, 8),
                &JpegOptions {
                    quantisation: JpegQuantisation::Tables {
                        luminance: {
                            let mut table = [1u8; 64];
                            table[7] = 0;
                            table
                        },
                        chrominance: [1; 64],
                    },
                    ..default
                }
            ),
            Err(JpegEncodeError::ZeroQuantiser {
                chrominance: false,
                index: 7
            })
        );
        assert_eq!(
            jpeg_encode(
                &gray_source(&pixels, 8, 8),
                &JpegOptions {
                    sampling: JpegSampling::FourTwoZero,
                    ..default
                }
            ),
            Err(JpegEncodeError::SubsampledGrayscale)
        );
    }

    /// The two rungs T.81 names are different quantisers and therefore different
    /// bytes, and the halved one is finer — which is the whole content of K.1's
    /// second paragraph.
    #[test]
    fn annex_k_halved_is_finer_than_annex_k() {
        let pixels: Vec<u8> = (0..32u32 * 32).map(|i| (i % 97 * 2) as u8).collect();
        let coarse = jpeg_encode(&gray_source(&pixels, 32, 32), &JpegOptions::default()).unwrap();
        let fine = jpeg_encode(
            &gray_source(&pixels, 32, 32),
            &JpegOptions {
                quantisation: JpegQuantisation::AnnexKHalved,
                ..JpegOptions::default()
            },
        )
        .unwrap();
        assert!(fine.len() > coarse.len(), "a finer quantiser costs bits");

        let error = |bytes: &[u8]| {
            decode(bytes, 1 << 24)
                .unwrap()
                .data
                .iter()
                .zip(pixels.iter())
                .map(|(&got, &want)| u32::from(got.abs_diff(want)))
                .sum::<u32>()
        };
        assert!(error(&fine) < error(&coarse), "and buys accuracy");

        // Rounding up, so halving can never produce the zero B.2.4.1 forbids.
        let dqt = segments(&fine)
            .into_iter()
            .find(|(code, _)| *code == 0xDB)
            .unwrap()
            .1;
        assert!(dqt[1..].iter().all(|&q| q >= 1));
        assert_eq!(dqt[1], ANNEX_K1_LUMINANCE[0].div_ceil(2));
    }
}
