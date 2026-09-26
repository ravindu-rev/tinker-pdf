//! A JPEG XR file becomes an image XObject.
//!
//! [`crate::png_embed`]'s and [`crate::tiff_embed`]'s counterpart, and the
//! short one, because **JPEG XR has no pass-through route and never will**.
//!
//! Those two modules exist mostly to decide: a non-interlaced PNG's IDAT
//! already *is* a `/FlateDecode` stream with `/Predictor 15`, and four of
//! TIFF 6.0's codings already have a `/Filter` name, so the interesting work
//! there is proving when the bytes may be copied rather than decoded. No
//! `/Filter` in ISO 32000-2 Table 6 reads an ITU-T T.832 codestream — not
//! `/JPXDecode`, which is JPEG 2000 — so there is no route to choose. Every
//! JPEG XR image is decoded to samples and embedded as raw ones, and the
//! whole of this module is the arrangement of those samples.
//!
//! # What that costs, stated plainly
//!
//! A page's peak is *w × h × 3* for the colour and another *w × h* for a soft
//! mask, where a PNG or a single-strip TIFF costs a multiple of the *part*.
//! The writer's ordinary compression then applies, so the *file* is not
//! three bytes a pixel — but the peak is, and a caller synthesising a
//! two-hundred-page document from JPEG XR pages should know that.
//!
//! # Three arrangements the format needs and PDF does not have
//!
//! - **Channel order.** Table A.6 has `24bppBGR` and `32bppBGRA` as well as
//!   the RGB rows; `/DeviceRGB` has one order, so BGR is permuted here.
//! - **Byte order.** A.7.3 makes the container little-endian and
//!   [`tinker_pdf_filters::JxrImage`] follows it; ISO 32000-2 8.9.5.2 makes a
//!   16-bit image sample **big-endian**. Every 16-bit sample is swapped.
//! - **Alpha.** A.3.2's alpha is a separate image plane, which arrives
//!   interleaved in the decoded raster; 11.6.5.3 wants it as its own
//!   `/DeviceGray` image under `/SMask`. So the samples are split.
//!
//! None of the three is optional and none is a leniency: each is a different
//! spelling of the same picture, and getting one wrong gives a page that is
//! recognisable and wrong — blue where it should be red, or noise where a
//! 16-bit gradient should be.

use tinker_pdf_filters::{jxr_decode, JxrChannels, JxrError, JxrWarning, Limits};

use crate::build::{CompressedImage, ImageColorSpace, ImageData, SoftMask};

/// A decoded JPEG XR, arranged for embedding.
pub struct JxrImageData {
    width: u32,
    height: u32,
    bits_per_component: u8,
    /// `/DeviceGray` when false — Table A.6's grey rows, which stay one
    /// channel rather than being widened to three.
    rgb: bool,
    data: Vec<u8>,
    soft_mask: Option<Vec<u8>>,
    complete: bool,
    warnings: Vec<JxrWarning>,
    resolution: Option<(f32, f32)>,
}

impl JxrImageData {
    /// The image XObject.
    #[must_use]
    pub fn image(&self) -> ImageData<'_> {
        ImageData::Compressed(CompressedImage {
            width: self.width,
            height: self.height,
            bits_per_component: self.bits_per_component,
            color_space: if self.rgb {
                ImageColorSpace::DeviceRgb
            } else {
                ImageColorSpace::DeviceGray
            },
            // Raw samples: there is nothing already-encoded to declare, so the
            // writer's ordinary compression applies.
            filter: None,
            data: &self.data,
            // 8.9.6.4's range test cannot express a per-sample opacity, and a
            // per-sample opacity is the only transparency JPEG XR has. So this
            // is always `None`, and it is not a gap — there is nothing in the
            // format a colour key would say.
            color_key_mask: None,
            soft_mask: self.soft_mask.as_ref().map(|data| SoftMask {
                width: self.width,
                height: self.height,
                bits_per_component: self.bits_per_component,
                filter: None,
                data,
            }),
        })
    }

    /// Width in pixels.
    #[must_use]
    pub const fn width(&self) -> u32 {
        self.width
    }

    /// Height in pixels.
    #[must_use]
    pub const fn height(&self) -> u32 {
        self.height
    }

    /// Whether nothing was degraded: no tile dropped to zero, no alpha plane
    /// lost. The same contract [`crate::tiff_embed::TiffImageData::complete`]
    /// has.
    #[must_use]
    pub const fn complete(&self) -> bool {
        self.complete
    }

    /// Dots per inch, from Annex A's `WIDTH_RESOLUTION` and
    /// `HEIGHT_RESOLUTION`.
    ///
    /// `None` when the file states none. Nothing here defaults to 96 or to
    /// 72: what an absent resolution means is the caller's question, and XPS
    /// 13.4.1 answers it differently from a comic archive — the same division
    /// of labour [`crate::tiff_embed::TiffImageData::dpi`] draws.
    #[must_use]
    pub fn dpi(&self) -> Option<(f64, f64)> {
        let (x, y) = self.resolution?;
        Some((f64::from(x), f64::from(y)))
    }

    /// Typed leniency records (ruling 10), straight from the decoder.
    #[must_use]
    pub fn warnings(&self) -> &[JxrWarning] {
        &self.warnings
    }
}

/// Reads a JPEG XR and prepares it for embedding.
///
/// # Errors
/// Any [`JxrError`]. Every variant is a decision rather than data — see that
/// enum's documentation and `docs/features/filters.md`'s refusal table.
pub fn jxr_image(bytes: &[u8], limits: &Limits) -> Result<JxrImageData, JxrError> {
    let decoded = jxr_decode(bytes, limits)?;
    let channels = decoded.format.channels;
    let bits = decoded.bits_per_component();
    // 8 or 16 by construction: every other depth is refused by name.
    let per = usize::from(bits / 8);
    let source_channels = usize::from(channels.count());
    // Which source channel each output channel takes, in `/DeviceRGB` order.
    // The decoder reports Table A.6's own order, so BGR rows are permuted
    // here and RGB rows are not.
    let order: &[usize] = match channels {
        JxrChannels::Gray => &[0],
        JxrChannels::Rgb | JxrChannels::Rgba => &[0, 1, 2],
        JxrChannels::Bgr | JxrChannels::Bgra => &[2, 1, 0],
    };
    // A.3.2's alpha is the last channel of the interleaved raster.
    let alpha = channels.has_alpha().then(|| source_channels - 1);

    let pixels = decoded.data.len() / (source_channels * per).max(1);
    let mut data = Vec::with_capacity(pixels * order.len() * per);
    let mut mask = alpha.map(|_| Vec::with_capacity(pixels * per));

    for pixel in decoded.data.chunks_exact(source_channels * per) {
        for &channel in order {
            push_sample(&mut data, pixel, channel, per);
        }
        if let (Some(at), Some(mask)) = (alpha, mask.as_mut()) {
            push_sample(mask, pixel, at, per);
        }
    }

    Ok(JxrImageData {
        width: decoded.width,
        height: decoded.height,
        bits_per_component: bits,
        rgb: order.len() == 3,
        data,
        soft_mask: mask,
        complete: decoded.complete,
        warnings: decoded.warnings,
        resolution: decoded.resolution,
    })
}

/// Appends one sample, **big-endian**, which is what 8.9.5.2 wants and the
/// opposite of what A.7.3 gave.
fn push_sample(out: &mut Vec<u8>, pixel: &[u8], channel: usize, per: usize) {
    let at = channel * per;
    match pixel.get(at..at + per) {
        // A.7.3's little-endian pair, reversed.
        Some([low, high]) => out.extend_from_slice(&[*high, *low]),
        Some([byte]) => out.push(*byte),
        // Unreachable: `chunks_exact` guarantees the width, and `per` is 1 or
        // 2. A short pixel becomes a black sample rather than a panic.
        _ => out.extend(std::iter::repeat_n(0u8, per)),
    }
}

#[cfg(test)]
#[path = "jxr_embed/tests.rs"]
mod tests;
