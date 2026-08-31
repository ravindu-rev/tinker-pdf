//! ITU-T T.832 9.10: output formatting.
//!
//! The last stage: the samples come out of 9.9 in an internal colour format,
//! in an extended geometry, biased around zero and scaled by a power of two.
//! This turns them into the channels and the byte layout Table A.6's pixel
//! format names.
//!
//! # The six sub-processes, and the four this build needs
//!
//! 9.10.2 lists `SamplingConversion`, `ConvertInternalToOutputClrFmt`,
//! `AddBias`, `ComputeScaling`, `PostscalingProcess` and
//! `ClippingAndPackingStage`. The first is a no-op here — every internal
//! colour format this build accepts is already 4:4:4, and 9.10.3's chroma
//! upsampling belongs to the subsampled ones, which are refused by name. The
//! rest all run.
//!
//! # Two things the clause makes easy to get backwards
//!
//! **The bias is added before the scaling, not after.** 9.10.5 adds
//! `iBias << iScale` and 9.10.6 then shifts the sum right by `iScale` with a
//! rounding term. Adding an unshifted bias after the scaling is off by a
//! factor of eight on a scaled image and looks like a contrast problem rather
//! than a bug.
//!
//! **`SHIFT_BITS` moves the bias down and the sample up.** 9.10.5 computes
//! `iBias >> SHIFT_BITS` and 9.10.7.2 computes `sample << SHIFT_BITS`, so the
//! two are not a matched pair that cancels — the bias is applied in the
//! narrowed domain and the whole result is widened afterwards.
//!
//! # The colour transform is exact, not a matrix
//!
//! 9.10.4.3's inverse is three integer lifting steps, so an RGB image that
//! was converted to the internal format losslessly comes back **bit-exact**.
//! That is what makes the lossless identity in
//! `crates/tinker-pdf-filters/tests/jxr_fixtures.rs` a total check rather
//! than a tolerance, and it is why `Floor` and `Ceiling` below are written as
//! shifts with their rounding made explicit rather than as a division.

#![deny(clippy::float_arithmetic)]

use super::coefficients::Planes;
use super::container::{JxrChannels, JxrPixelFormat};
use super::headers::{
    CodedImageHeaders, InternalClrFmt, OutputBitdepth, OutputClrFmt, PlaneHeader,
};
use super::JxrError;

/// The samples of one decoded image plane, taken through 9.10.4 to 9.10.7 and
/// ready for 9.10.8's clip and crop.
struct Finished {
    components: Vec<Vec<i32>>,
    width: usize,
    height: usize,
}

/// 9.10.2's `OutputFormatting( )`, plus the interleave into the byte layout
/// Table A.6's row names.
///
/// `alpha` is A.3.2's separate alpha image plane, already decoded as a
/// `CODED_IMAGE( )` of its own — it has its own header, its own
/// `SCALED_FLAG` and its own `SHIFT_BITS`, so it is finished separately and
/// only joins the primary plane at the interleave.
///
/// # Errors
/// [`JxrError::BadDimensions`] if the declared output geometry does not fit
/// the reconstructed one, which a codestream can only reach by disagreeing
/// with itself.
pub(crate) fn format_output(
    planes: &Planes,
    h: &CodedImageHeaders,
    format: JxrPixelFormat,
    alpha: Option<(&Planes, &PlaneHeader)>,
) -> Result<Vec<u8>, JxrError> {
    let width = usize::try_from(h.image.width).map_err(|_| JxrError::BadDimensions)?;
    let height = usize::try_from(h.image.height).map_err(|_| JxrError::BadDimensions)?;
    let primary = finish(planes, h, &h.primary, output_components(h))?;
    let alpha = alpha.map(|(p, ph)| finish(p, h, ph, 1)).transpose()?;

    let channels = format.channels;
    let bits = format.bits_per_component;
    let per_sample = usize::from(bits / 8);
    let out_len = width
        .checked_mul(height)
        .and_then(|n| n.checked_mul(usize::from(channels.count())))
        .and_then(|n| n.checked_mul(per_sample))
        .ok_or(JxrError::BadDimensions)?;
    let mut out = Vec::with_capacity(out_len);

    // 9.10.8.1's crop: the output window starts at (LEFT_MARGIN, TOP_MARGIN),
    // and a non-zero top or left margin is refused by name in `super`, so the
    // origin is (0, 0) and only the bottom and right padding is dropped.
    for y in 0..height {
        for x in 0..width {
            for &plane in channel_order(channels) {
                let sample = if plane == ALPHA_CHANNEL {
                    alpha
                        .as_ref()
                        .and_then(|a| a.at(0, x, y))
                        // 9.10.8.2's ceiling is opaque, which is what an
                        // absent alpha plane means.
                        .unwrap_or(opaque(bits))
                } else {
                    primary.at(plane, x, y).unwrap_or(0)
                };
                let clipped = clipping_basic(sample, bits);
                if bits == 16 {
                    // A.7.3: this container's byte order is little-endian, so
                    // 16-bit samples are too — unlike PNG's.
                    out.extend_from_slice(&(clipped as u16).to_le_bytes());
                } else {
                    // `clipping_basic` bounded this to 0..=255.
                    out.push(clipped as u8);
                }
            }
        }
    }
    Ok(out)
}

/// The index [`channel_order`] uses for the alpha channel, which does not
/// come from the primary plane.
const ALPHA_CHANNEL: usize = usize::MAX;

/// Which of the finished planes each output channel takes, in order.
///
/// After 9.10.4 the primary plane is R, G, B in components 0, 1 and 2 — so
/// every one of Table A.6's channel orders is a permutation of a prefix of
/// that, plus alpha.
const fn channel_order(channels: JxrChannels) -> &'static [usize] {
    match channels {
        JxrChannels::Gray => &[0],
        JxrChannels::Rgb => &[0, 1, 2],
        JxrChannels::Bgr => &[2, 1, 0],
        JxrChannels::Bgra => &[2, 1, 0, ALPHA_CHANNEL],
        JxrChannels::Rgba => &[0, 1, 2, ALPHA_CHANNEL],
    }
}

/// The value a fully opaque alpha sample takes at `bits`.
const fn opaque(bits: u8) -> i32 {
    if bits == 16 {
        65_535
    } else {
        255
    }
}

/// 9.10.2's `outputArrays`: three for an RGB or YUV output, otherwise the
/// component count.
fn output_components(h: &CodedImageHeaders) -> usize {
    match h.image.output_clr_fmt {
        OutputClrFmt::Rgb => 3,
        _ => usize::try_from(h.primary.num_components).unwrap_or(1),
    }
}

impl Finished {
    fn at(&self, component: usize, x: usize, y: usize) -> Option<i32> {
        if x >= self.width || y >= self.height {
            return None;
        }
        self.components
            .get(component)
            .and_then(|p| p.get(y * self.width + x))
            .copied()
    }
}

/// 9.10.4 to 9.10.7 for one image plane.
fn finish(
    planes: &Planes,
    h: &CodedImageHeaders,
    plane: &PlaneHeader,
    out_components: usize,
) -> Result<Finished, JxrError> {
    let width = usize::try_from(planes.width).map_err(|_| JxrError::BadDimensions)?;
    let height = usize::try_from(planes.height).map_err(|_| JxrError::BadDimensions)?;
    let len = width.checked_mul(height).ok_or(JxrError::BadDimensions)?;
    let mut components: Vec<Vec<i32>> = Vec::with_capacity(out_components);
    for i in 0..out_components {
        // 9.10.4.2's `InvColorFmtConvert1( )` is a replication, so a YONLY
        // plane feeding an RGB output supplies all three channels from one.
        let source = planes.samples.get(i).or_else(|| planes.samples.first());
        let mut c = source.cloned().unwrap_or_else(|| vec![0; len]);
        c.resize(len, 0);
        components.push(c);
    }
    // 9.10.4.3's `InvColorFmtConvert2( )`.
    if h.image.output_clr_fmt == OutputClrFmt::Rgb
        && plane.internal_clr_fmt == InternalClrFmt::Yuv444
        && components.len() >= 3
    {
        // Split rather than index: the conversion reads all three planes and
        // writes all three, so the three borrows have to coexist.
        let (first, rest) = components.split_at_mut(1);
        let (second, third) = rest.split_at_mut(1);
        for ((y, u), v) in first[0]
            .iter_mut()
            .zip(second[0].iter_mut())
            .zip(third[0].iter_mut())
        {
            let (r, g, b) = inv_colour_convert2(*y, *u, *v);
            *y = r;
            *u = g;
            *v = b;
        }
    }
    // 9.10.5's `AddBias( )` and 9.10.6's `ComputeScaling( )`, in that order —
    // see the module docs for why the order is load-bearing.
    let scaled = plane.scaled;
    let shift_bits = plane.shift_bits.min(31);
    let depth = h.image.output_bitdepth;
    let bias = add_bias_amount(depth, shift_bits, scaled);
    let (scale, rounding) = compute_scaling(depth, scaled);
    let post = matches!(depth, OutputBitdepth::Bd16);
    for c in &mut components {
        for v in c.iter_mut() {
            let mut s = v.wrapping_add(bias);
            s = s.wrapping_add(rounding) >> scale;
            if post {
                // 9.10.7.2's `PostScalingInt( )`.
                s = s.wrapping_shl(shift_bits);
            }
            *v = s;
        }
    }
    Ok(Finished {
        components,
        width,
        height,
    })
}

/// 9.10.4.3's `InvColorFmtConvert2( )`.
///
/// `Floor(t / 2)` is an arithmetic shift and `Ceiling(v / 2)` is
/// `(v + 1) >> 1`; both are exact for negative operands, which matters
/// because the internal U and V are centred on zero and are negative for
/// half of every image.
fn inv_colour_convert2(y: i32, u: i32, v: i32) -> (i32, i32, i32) {
    let t = u.wrapping_neg();
    let g = y.wrapping_sub(t >> 1);
    let r = t.wrapping_add(g).wrapping_sub((v.wrapping_add(1)) >> 1);
    let b = v.wrapping_add(r);
    (r, g, b)
}

/// 9.10.5's bias, already shifted by `iScale` so the caller adds one number.
fn add_bias_amount(depth: OutputBitdepth, shift_bits: u32, scaled: bool) -> i32 {
    let mut bias: i32 = match depth {
        OutputBitdepth::Bd8 => 1 << 7,
        OutputBitdepth::Bd16 => 1 << 15,
        // Every other depth is refused by name in `super`; 9.10.5 gives them
        // a bias of zero, which is what this arm would do anyway.
        _ => 0,
    };
    if matches!(
        depth,
        OutputBitdepth::Bd16 | OutputBitdepth::Bd16S | OutputBitdepth::Bd32S
    ) {
        bias >>= shift_bits.min(31);
    }
    let scale = if scaled { 3 } else { 0 };
    bias.wrapping_shl(scale)
}

/// 9.10.6's `iScale` and `iRoundingFactor`.
///
/// The rounding factor is 3 for most depths and **4 for BD16**, which is the
/// clause's own asymmetry and not a slip: the two differ by where the
/// half-way case lands, and using 3 for a 16-bit output loses the top bit's
/// rounding on every sample.
const fn compute_scaling(depth: OutputBitdepth, scaled: bool) -> (u32, i32) {
    if !scaled {
        return (0, 0);
    }
    let rounding = match depth {
        OutputBitdepth::Bd16 | OutputBitdepth::Bd1White1 | OutputBitdepth::Bd1Black1 => 4,
        _ => 3,
    };
    (3, rounding)
}

/// 9.10.8.2's `ClippingBasic( )`.
const fn clipping_basic(sample: i32, bits: u8) -> i32 {
    let (low, high) = if bits == 16 { (0, 65_535) } else { (0, 255) };
    if sample < low {
        low
    } else if sample > high {
        high
    } else {
        sample
    }
}

#[cfg(test)]
#[path = "tests/colour.rs"]
mod tests;
