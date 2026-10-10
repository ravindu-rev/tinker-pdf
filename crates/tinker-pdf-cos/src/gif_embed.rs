//! A GIF file becomes an image XObject.
//!
//! [`crate::bmp_embed`]'s sibling and as short, for the same reason: **a GIF
//! has no pass-through route.** Its LZW is not `/LZWDecode`'s — the root set
//! is sized by the file, the codes are packed least significant bit first
//! into 255-byte sub-blocks, and the width grows one code later — so no
//! `/Filter` in ISO 32000-2 Table 6 reads a GIF's image data, and every GIF is
//! decoded.
//!
//! What it keeps is the palette. A GIF is indexed by definition, and
//! [`crate::raster_embed::RasterImageData`] places it as `/Indexed` over the
//! table it was decoded against, with the graphic control extension's
//! transparent index as 8.9.6.4's colour-key `/Mask` — one index, one range,
//! no second image. The one GIF that cannot stay indexed is a first image
//! smaller than its screen that brings its own table, and `gif.rs` expands
//! that one to RGBA before it arrives here.

use tinker_pdf_filters::{gif_decode, GifError, Limits};

use crate::raster_embed::RasterImageData;

/// Reads a GIF's first image and prepares it for embedding.
///
/// # Errors
/// Any [`GifError`].
pub fn gif_image(bytes: &[u8], limits: &Limits) -> Result<RasterImageData, GifError> {
    let image = gif_decode(bytes, limits)?;
    Ok(RasterImageData::new(
        image.width,
        image.height,
        image.pixels,
        image.complete,
        image.warnings,
    ))
}
