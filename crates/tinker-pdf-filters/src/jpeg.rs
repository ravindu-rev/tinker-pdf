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
    pub data: Vec<u8>,
    /// What the decoder tolerated.
    pub warnings: Vec<Warning>,
}

/// Why a JPEG could not be decoded.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JpegError {
    /// The bytes do not begin like a JPEG.
    NotJpeg,
    /// Arithmetic coding, which is deferred behind a capability.
    Arithmetic,
    /// A sample precision other than 8 bits.
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
                if precision != 8 {
                    return Err(JpegError::UnsupportedPrecision);
                }
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
            0xC9..=0xCB => return Err(JpegError::Arithmetic),

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
                idct_block(&block, &mut pixels);

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
        warnings,
    })
}

/// The inverse DCT of one block, separable and in integers.
fn idct_block(input: &[i32; 64], out: &mut [u8; 64]) {
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
            let value = ((sum >> 14) + 128).clamp(0, 255) as u8;
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

    #[test]
    fn a_twelve_bit_image_is_refused_rather_than_misread() {
        let mut twelve = vec![0xFF, 0xD8, 0xFF, 0xC0, 0x00, 0x0B, 0x0C];
        twelve.extend_from_slice(&[0x00, 0x01, 0x00, 0x01, 0x01, 0x11, 0x00]);
        assert_eq!(
            decode(&twelve, 1 << 20).err(),
            Some(JpegError::UnsupportedPrecision)
        );
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
        idct_block(&block, &mut pixels);

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
}
