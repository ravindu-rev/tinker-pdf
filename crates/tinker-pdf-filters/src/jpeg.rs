//! Baseline JPEG decoding (DCTDecode, 7.4.8; ITU-T T.81).
//!
//! Huffman-coded baseline and extended sequential at 8 bits, which is what
//! essentially every PDF carries. Progressive is a named gap: it needs a
//! second coefficient pass with spectral selection and successive
//! approximation, and until it exists a progressive image is reported rather
//! than half-decoded.
//!
//! The IDCT is the integer AAN-style separable transform. That means output
//! can differ from libjpeg's by a least-significant bit on some coefficients —
//! there is no single correct IDCT, only conforming ones — so comparison
//! against a reference is perceptual, never exact.

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
    /// Progressive JPEG, which is not yet implemented.
    Progressive,
    /// Arithmetic coding, which is deferred behind a capability.
    Arithmetic,
    /// A sample precision other than 8 bits.
    UnsupportedPrecision,
    /// The file ended before the image did, past any hope of recovery.
    Truncated,
    /// A component count no colour model covers.
    UnsupportedComponents,
}

#[derive(Clone, Copy, Default)]
struct Component {
    id: u8,
    h: usize,
    v: usize,
    quant: usize,
    dc_table: usize,
    ac_table: usize,
    dc_prediction: i32,
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
}

impl<'a> BitReader<'a> {
    fn new(data: &'a [u8]) -> BitReader<'a> {
        BitReader {
            data,
            at: 0,
            bits: 0,
            count: 0,
            exhausted: false,
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
        for _ in 0..n.min(32) {
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

/// Extends a Huffman-decoded magnitude to its signed value (T.81 F.2.2.1).
fn extend(value: i32, length: u32) -> i32 {
    if length == 0 {
        return 0;
    }
    if value < (1 << (length - 1)) {
        value - (1 << length) + 1
    } else {
        value
    }
}

/// Decodes a baseline JPEG.
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
            // SOF0 baseline, SOF1 extended sequential.
            0xC0 | 0xC1 => {
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
            0xC2 => return Err(JpegError::Progressive),
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

            // SOS: the scan itself.
            0xDA => {
                let count = usize::from(segment.first().copied().unwrap_or(0));
                for i in 0..count.min(4) {
                    let (Some(&id), Some(&tables)) =
                        (segment.get(1 + i * 2), segment.get(2 + i * 2))
                    else {
                        return Err(JpegError::Truncated);
                    };
                    if let Some(component) = components.iter_mut().find(|c| c.id == id) {
                        component.dc_table = usize::from(tables >> 4).min(3);
                        component.ac_table = usize::from(tables & 0x0F).min(3);
                    }
                }

                let scan = data.get(segment_end..).unwrap_or_default();
                return finish(
                    scan,
                    width,
                    height,
                    &mut components,
                    &quant,
                    &dc_tables,
                    &ac_tables,
                    restart_interval,
                    adobe_seen,
                    adobe_transform,
                    max_output,
                    warnings,
                );
            }

            _ => {}
        }

        at = segment_end;
    }

    warnings.push(Warning::TruncatedInput);
    Err(JpegError::Truncated)
}

/// Decodes the scan and assembles the image.
#[allow(clippy::too_many_arguments)]
fn finish(
    scan: &[u8],
    width: usize,
    height: usize,
    components: &mut [Component],
    quant: &[[u16; 64]; 4],
    dc_tables: &[HuffmanTable],
    ac_tables: &[HuffmanTable],
    restart_interval: usize,
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
    let mcu_w = h_max * 8;
    let mcu_h = v_max * 8;
    let mcus_x = width.div_ceil(mcu_w);
    let mcus_y = height.div_ceil(mcu_h);

    // One full-resolution plane per component, upsampled as it is written.
    let mut planes: Vec<Vec<u8>> = components
        .iter()
        .map(|_| vec![128u8; width * height])
        .collect();

    let mut reader = BitReader::new(scan);
    let mut block = [0i32; 64];
    let mut pixels = [0u8; 64];
    let mut mcu_index = 0usize;

    'outer: for my in 0..mcus_y {
        for mx in 0..mcus_x {
            if restart_interval > 0 && mcu_index > 0 && mcu_index % restart_interval == 0 {
                reader.restart();
                for component in components.iter_mut() {
                    component.dc_prediction = 0;
                }
            }
            mcu_index += 1;

            for (ci, component) in components.iter_mut().enumerate() {
                for by in 0..component.v {
                    for bx in 0..component.h {
                        block.fill(0);

                        let dc_table = dc_tables.get(component.dc_table);
                        let ac_table = ac_tables.get(component.ac_table);
                        let (Some(dc_table), Some(ac_table)) = (dc_table, ac_table) else {
                            break 'outer;
                        };

                        // DC: a difference from the previous block's value.
                        let Some(t) = reader.huffman(dc_table) else {
                            break 'outer;
                        };
                        let diff = extend(reader.bits(u32::from(t)), u32::from(t));
                        component.dc_prediction = component.dc_prediction.saturating_add(diff);
                        block[0] = component.dc_prediction;

                        // AC: run-length pairs to the end of the block.
                        let mut k = 1usize;
                        while k < 64 {
                            let Some(rs) = reader.huffman(ac_table) else {
                                break 'outer;
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
                            if let Some(&z) = ZIGZAG.get(k) {
                                if let Some(slot) = block.get_mut(z) {
                                    *slot = value;
                                }
                            }
                            k += 1;
                        }

                        let table = quant.get(component.quant).copied().unwrap_or([1; 64]);
                        for (value, q) in block.iter_mut().zip(table.iter()) {
                            *value = value.saturating_mul(i32::from(*q));
                        }
                        idct_block(&block, &mut pixels);

                        // Write the block into its plane, upsampled to full
                        // resolution by pixel replication.
                        let scale_x = h_max / component.h.max(1);
                        let scale_y = v_max / component.v.max(1);
                        let origin_x = mx * mcu_w + bx * 8 * scale_x;
                        let origin_y = my * mcu_h + by * 8 * scale_y;

                        let Some(plane) = planes.get_mut(ci) else {
                            break 'outer;
                        };
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
        }
    }

    if reader.exhausted {
        warnings.push(Warning::TruncatedInput);
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

        // SOF0: 8-bit, 1×1, one component, no subsampling.
        out.extend_from_slice(&[0xFF, 0xC0, 0x00, 0x0B, 0x08, 0x00, 0x01, 0x00, 0x01, 0x01]);
        out.extend_from_slice(&[0x11, 0x00]);

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
    fn progressive_and_arithmetic_are_reported_rather_than_half_decoded() {
        let mut progressive = vec![0xFF, 0xD8, 0xFF, 0xC2, 0x00, 0x0B, 0x08];
        progressive.extend_from_slice(&[0x00, 0x01, 0x00, 0x01, 0x01, 0x11, 0x00]);
        assert_eq!(
            decode(&progressive, 1 << 20).err(),
            Some(JpegError::Progressive)
        );

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
}
