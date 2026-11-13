//! The direct forms of the decoder's fast paths, kept in the test build to
//! hold those paths to (`docs/design/jpeg-speed.md`, "The invariant").
//!
//! Each is the straightforward reading of its clause — a bit at a time, every
//! product made, every plane whole — and each fast path in `super` is tested
//! against it over inputs that reach every branch of both, so that a
//! full-size decode stays byte-identical for every stream.

use super::{Component, HuffmanTable, JpegColor, JpegError, JpegImage, COS_TABLE, ZIGZAG};
use crate::Warning;

/// Reads bits from the entropy-coded segment a byte at a time, unstuffing as
/// it goes: [`super::BitReader`] without its word.
pub(super) struct BitwiseReader<'a> {
    data: &'a [u8],
    pub(super) at: usize,
    bits: u32,
    pub(super) count: u32,
    /// True once the reader has run past the end.
    pub(super) exhausted: bool,
    /// Set when `bit` walked over a restart marker by itself, so `restart`
    /// knows not to skip a second one and lose a whole interval.
    pub(super) crossed_restart: bool,
}

impl<'a> BitwiseReader<'a> {
    pub(super) fn new(data: &'a [u8]) -> BitwiseReader<'a> {
        BitwiseReader {
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
    pub(super) fn bit(&mut self) -> u32 {
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

    pub(super) fn bits(&mut self, n: u32) -> i32 {
        let mut value = 0i32;
        for _ in 0..n.min(31) {
            value = (value << 1) | self.bit() as i32;
        }
        value
    }

    /// Decodes one Huffman-coded symbol.
    pub(super) fn huffman(&mut self, table: &HuffmanTable) -> Option<u8> {
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
    pub(super) fn restart(&mut self) {
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

/// The inverse DCT of one block, separable and in integers, every product
/// made: [`super::idct_block`] without its shared products, its lanes or its
/// shortcuts.
pub(super) fn idct_block(input: &[i32; 64], precision: u8, out: &mut [u8; 64]) {
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
            // eight this crate hands out -- see [`super::JpegImage::data`] --
            // which for `P = 8` is a shift of zero and leaves the byte
            // untouched.
            let half = 1i64 << (precision - 1);
            let ceiling = (1i64 << precision) - 1;
            let value = (((sum >> 14) + half).clamp(0, ceiling) >> (precision - 8)) as u8;
            if let Some(slot) = out.get_mut(y * 8 + col) {
                *slot = value;
            }
        }
    }
}
/// Turns the finished coefficients into pixels: one full-resolution plane
/// a component, upsampled as it is written, then interleaved —
/// [`super::finish`] without its strips.
#[allow(clippy::too_many_arguments)]
pub(super) fn finish_planes(
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
