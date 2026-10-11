//! A BMP file becomes an image XObject.
//!
//! The short one of the `*_embed` doors, because **a BMP has no pass-through
//! route and never will**: no `/Filter` in ISO 32000-2 Table 6 reads
//! `BI_RLE8`, `BI_RLE4` or a bottom-up pixel array padded to four bytes a row,
//! and an uncompressed BMP is not even an uncompressed PDF image — its rows
//! are upside down and its samples blue first. So every bitmap is decoded, and
//! [`crate::raster_embed::RasterImageData`] is the whole of the arrangement:
//! an indexed BMP stays `/Indexed`, a direct one is `/DeviceRGB`, and an alpha
//! mask becomes an `/SMask`.
//!
//! What `bmp.rs` decided on the way — that the fourth byte of a 32-bit
//! `BI_RGB` pixel is not alpha, that an RLE delta's undefined pixels are index
//! 0 — reaches the page unchanged, and its warnings with it (ruling 10).

use tinker_pdf_filters::{bmp_decode, BmpError, Limits};

use crate::raster_embed::RasterImageData;

/// Reads a BMP and prepares it for embedding.
///
/// # Errors
/// Any [`BmpError`] — every one of them a decision about the header rather
/// than damage in the pixels, which degrades instead.
pub fn bmp_image(bytes: &[u8], limits: &Limits) -> Result<RasterImageData, BmpError> {
    let image = bmp_decode(bytes, limits)?;
    Ok(RasterImageData::new(
        image.width,
        image.height,
        image.pixels,
        image.complete,
        image.warnings,
    ))
}
