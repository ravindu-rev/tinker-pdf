//! A WebP file becomes an image XObject.
//!
//! [`crate::gif_embed`]'s sibling and as short, for the same reason: **a WebP
//! has no pass-through route.** Neither of its bitstreams is a `/Filter` in
//! ISO 32000-2 Table 6 — the lossless one is prefix codes over transformed
//! ARGB, not DEFLATE — so every WebP is decoded.
//!
//! What comes back is RGB, or RGBA when any pixel is less than opaque, and
//! [`crate::raster_embed::RasterImageData`] places the alpha as an 8-bit
//! `/SMask`. A WebP is never indexed on the way out: a colour-indexing
//! transform is how the file was *compressed*, not a promise that the picture
//! fits a palette once its other transforms are undone.

use tinker_pdf_filters::{webp_decode, Limits, WebpError};

use crate::raster_embed::RasterImageData;

/// Reads a WebP's picture — the first frame of an animation — and prepares
/// it for embedding.
///
/// # Errors
/// Any [`WebpError`].
pub fn webp_image(bytes: &[u8], limits: &Limits) -> Result<RasterImageData, WebpError> {
    let image = webp_decode(bytes, limits)?;
    Ok(RasterImageData::new(
        image.width,
        image.height,
        image.pixels,
        image.complete,
        image.warnings,
    ))
}
