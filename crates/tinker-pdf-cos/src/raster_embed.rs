//! A decoded BMP, GIF or WebP becomes an image XObject.
//!
//! [`crate::jxr_embed`]'s shape rather than [`crate::png_embed`]'s: none of
//! these three formats has a coding a `/Filter` name reads, so there is no
//! route to choose and the whole of this module is the arrangement of samples
//! the decoder already produced. What it shares with `png_embed` is the
//! reason the samples are re-deflated here rather than handed to the writer
//! raw — the buffer this value holds for a synthesised document's life is the
//! compressed one — and the `/SMask` split.
//!
//! # One type for three formats
//!
//! [`tinker_pdf_filters::ImagePixels`] is the one output shape the three
//! decoders share, deliberately, so there is one arrangement to get right:
//!
//! | Decoded layout | `/ColorSpace` | Transparency |
//! | --- | --- | --- |
//! | Indexed | `[/Indexed /DeviceRGB hival lookup]`, eight bits an index | a transparent index becomes 8.9.6.4's colour-key `/Mask [t t]` |
//! | RGB | `/DeviceRGB` | — |
//! | RGBA | `/DeviceRGB` | the alpha samples become an eight-bit `/SMask` |
//!
//! An indexed picture is **kept** indexed, which is the cost argument
//! `cbz.rs`'s module note makes: a GIF or a paletted BMP costs one byte a pixel
//! held rather than three, and `/Indexed` is exactly what the file said.
//!
//! # Bounds: none of its own
//!
//! `png_embed`'s argument, unchanged. The decoders refuse the geometry before
//! any buffer exists — `MAX_BMP_SAMPLES` and its two siblings, and the
//! caller's own [`tinker_pdf_filters::Limits::max_output`] under the caller's
//! number — and the split and the re-deflate are bounded by the raster those
//! already allowed. A palette is at most 256 entries by every one of the three
//! formats' own definitions.

use tinker_pdf_filters::{zlib_compress, ImagePixels, Warning as FilterWarning};

use crate::build::{
    CompressedImage, DeviceSpace, ImageColorSpace, ImageData, ImageFilter, SoftMask,
};

/// A decoded BMP, GIF or WebP, arranged for
/// [`crate::DocumentBuilder::add_image`].
///
/// It owns its buffers, so the value has to outlive the `add_image` call —
/// which is also what keeps the peak at one page's worth rather than the
/// document's.
pub struct RasterImageData {
    width: u32,
    height: u32,
    /// `None` for `/DeviceRGB`, the lookup table for `/Indexed`.
    lookup: Option<Vec<u8>>,
    /// The colour samples, zlib-compressed.
    data: Vec<u8>,
    color_key: Option<[(u32, u32); 1]>,
    /// The alpha samples, zlib-compressed.
    soft_mask: Option<Vec<u8>>,
    complete: bool,
    warnings: Vec<FilterWarning>,
}

impl RasterImageData {
    /// Arranges a decoder's output.
    pub(crate) fn new(
        width: u32,
        height: u32,
        pixels: ImagePixels,
        complete: bool,
        warnings: Vec<FilterWarning>,
    ) -> Self {
        let (lookup, samples, color_key, alpha) = match pixels {
            ImagePixels::Indexed {
                palette,
                indices,
                transparent,
            } => (
                Some(palette),
                indices,
                transparent.map(|t| [(u32::from(t), u32::from(t))]),
                None,
            ),
            ImagePixels::Rgb(data) => (None, data, None, None),
            ImagePixels::Rgba(data) => {
                let pixels = data.len() / 4;
                let mut colour = Vec::with_capacity(pixels * 3);
                let mut alpha = Vec::with_capacity(pixels);
                for pixel in data.chunks_exact(4) {
                    if let [r, g, b, a] = *pixel {
                        colour.extend_from_slice(&[r, g, b]);
                        alpha.push(a);
                    }
                }
                (None, colour, None, Some(alpha))
            }
        };
        Self {
            width,
            height,
            lookup,
            data: zlib_compress(&samples),
            color_key,
            soft_mask: alpha.map(|a| zlib_compress(&a)),
            complete,
            warnings,
        }
    }

    /// What to hand [`crate::DocumentBuilder::add_image`].
    #[must_use]
    pub fn image(&self) -> ImageData<'_> {
        ImageData::Compressed(CompressedImage {
            width: self.width,
            height: self.height,
            bits_per_component: 8,
            color_space: match &self.lookup {
                Some(lookup) => ImageColorSpace::Indexed {
                    base: DeviceSpace::Rgb,
                    lookup,
                },
                None => ImageColorSpace::DeviceRgb,
            },
            filter: Some(ImageFilter::Flate),
            data: &self.data,
            color_key_mask: self.color_key.as_ref().map(<[(u32, u32); 1]>::as_slice),
            soft_mask: self.soft_mask.as_ref().map(|data| SoftMask {
                width: self.width,
                height: self.height,
                bits_per_component: 8,
                filter: Some(ImageFilter::Flate),
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

    /// Whether the picture is kept as an `/Indexed` image.
    #[must_use]
    pub const fn is_indexed(&self) -> bool {
        self.lookup.is_some()
    }

    /// Whether nothing was degraded: the decoder's own `complete`.
    #[must_use]
    pub const fn complete(&self) -> bool {
        self.complete
    }

    /// Typed leniency records (ruling 10), straight from the decoder.
    #[must_use]
    pub fn warnings(&self) -> &[FilterWarning] {
        &self.warnings
    }
}

#[cfg(test)]
#[path = "raster_embed/tests.rs"]
mod tests;
