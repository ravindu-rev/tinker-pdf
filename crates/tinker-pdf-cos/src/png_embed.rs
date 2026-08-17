//! A PNG file becomes an image XObject (gap 29, milestone 4).
//!
//! This is the module that **chooses**, and the choice is the whole of gap 29's
//! memory argument. `build.rs` already reads a JPEG's frame header to fill an
//! image dictionary; this does the same job for the other format a comic
//! archive holds, and it has one more decision to take because PNG has a shape
//! PDF cannot always express.
//!
//! # Pass-through is the default and the decoder is the fallback
//!
//! A non-interlaced PNG's IDAT **is** a PDF `/FlateDecode` stream with
//! `/DecodeParms << /Predictor 15 /Colors n /BitsPerComponent d /Columns w >>`,
//! byte for byte, because PDF's `/Predictor` is PNG 9.2's row-filter
//! specification adopted wholesale — the per-row tag, the five filter types and
//! the `ceil(colors x bpc / 8)` left-neighbour offset floored at one. So the
//! common case copies bytes and reads a thirteen-byte header, and never builds
//! a raster at all.
//!
//! That is load-bearing rather than elegant. A CBZ is synthesised whole at
//! open, so whatever a page holds is held for the document's life; decoding
//! every page to samples costs *w x h x 3* each, about 3.6 GB for a 200-page
//! archive at 2000 x 3000. [`tinker_pdf_filters::png_scan`] exists for exactly
//! this reason — it walks the chunks, checks every CRC and hands back the
//! concatenated IDAT with nothing inflated.
//!
//! # What cannot pass through, and why each one cannot
//!
//! | PNG feature | Why | Route |
//! | --- | --- | --- |
//! | Adam7 interlace (7.2) | seven reduced images with their own filtered rows; `/Predictor` describes one raster and has no way to change width part way down | decode |
//! | Colour type 4, grey + alpha | the alpha is interleaved into the same scanlines and PDF has no `/DecodeParms` that says so | decode, split, `/SMask` |
//! | Colour type 6, RGBA | as above | decode, split, `/SMask` |
//! | `tRNS` giving *partial* alpha to palette entries | `/Mask` is a range test, not a lookup | decode, `/SMask` |
//! | `tRNS` transparent entries in two separate runs | one range cannot name two runs | decode, `/SMask` |
//! | Colour types 0, 2, 3 non-interlaced | — | **pass through** |
//! | ... with a `tRNS` naming one fully transparent colour, or one contiguous run of indices | 8.9.6.4's `/Mask` array expresses exactly a range test | **pass through**, with `/Mask` |
//!
//! The contiguous-run rule is slightly wider than gap 29's table, which says
//! "one fully transparent index". A run is what 8.9.6.4 actually expresses, one
//! index is the special case of a run of length one, and the wider rule sends
//! strictly fewer files to the decoder for the same output.
//!
//! # Bounds: this module owns none, and that is an argument rather than an
//! omission
//!
//! Gap 18a milestone 8's failure was a cap set above what its own inputs could
//! reach, so it could never fire; `tinker-pdf-zip`'s `limits.rs` and `png.rs`'s
//! module note are this repository's form for writing down what was considered
//! and refused. Four candidates, and every one of them would be decoration:
//!
//! - **Nothing on the pass-through.** Its allocation is a copy of the input's
//!   own IDAT bytes, so the input length bounds it; there is no raster.
//! - **Nothing on the decode.** [`tinker_pdf_filters::MAX_PNG_SAMPLES`] refuses
//!   the geometry before a buffer exists and the caller's own
//!   [`Limits::max_output`] refuses it again under the caller's number. A third
//!   ceiling here would have to be kept in step with two that already fire.
//! - **Nothing on the split or the re-deflate.** Both are bounded by the raster
//!   the two caps above already allowed: the split writes exactly as many bytes
//!   as it reads, and `zlib_compress` of *n* bytes is *n* plus a fixed header
//!   in the worst case.
//! - **Nothing on the palette.** 11.2.3 caps it at `2^bit_depth` entries and
//!   the scan enforces that, so 768 bytes is the format's own ceiling.

use tinker_pdf_filters::{
    png_scan, zlib_compress, Limits, PngColour, PngError, PngImage, PngScan, PngTransparency,
    Warning as FilterWarning,
};

use crate::build::{
    CompressedImage, DeviceSpace, ImageColorSpace, ImageData, ImageFilter, SoftMask,
};

/// Which of the two routes a PNG took.
///
/// Public because it is the claim worth testing. "The pass-through happened"
/// and "the picture is right" are different assertions, and a build that
/// quietly decoded everything would satisfy the second while throwing away the
/// reason this module exists.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PngRoute {
    /// The IDAT was placed unchanged, as `/FlateDecode` with `/Predictor 15`.
    PassedThrough,
    /// The file was decoded to samples and re-encoded.
    Decoded,
}

/// The `/ColorSpace` this module produces, holding its own palette.
enum OwnedSpace {
    Gray,
    Rgb,
    /// `[/Indexed /DeviceRGB hival lookup]`. PNG's PLTE is always RGB triples
    /// (11.2.3), so the base never varies.
    Indexed(Vec<u8>),
}

/// An `/SMask` sub-image, owned.
struct OwnedMask {
    width: u32,
    height: u32,
    bits_per_component: u8,
    filter: Option<ImageFilter>,
    data: Vec<u8>,
}

/// A PNG prepared for [`crate::DocumentBuilder::add_image`].
///
/// It owns its buffers because both routes produce them: the pass-through
/// concatenates IDAT chunks and the decode re-deflates samples, and neither is
/// a subslice of the file. [`PngImageData::image`] borrows from this, so the
/// value has to outlive the `add_image` call — which is also what keeps the
/// peak at one page's worth rather than the document's.
pub struct PngImageData {
    width: u32,
    height: u32,
    bits_per_component: u8,
    space: OwnedSpace,
    filter: Option<ImageFilter>,
    data: Vec<u8>,
    /// Empty when there is no colour key, which is not the same as a key that
    /// masks nothing.
    color_key_mask: Vec<(u32, u32)>,
    soft_mask: Option<OwnedMask>,
    route: PngRoute,
    complete: bool,
    warnings: Vec<FilterWarning>,
}

impl PngImageData {
    /// What to hand [`crate::DocumentBuilder::add_image`].
    #[must_use]
    pub fn image(&self) -> ImageData<'_> {
        ImageData::Compressed(CompressedImage {
            width: self.width,
            height: self.height,
            bits_per_component: self.bits_per_component,
            color_space: match &self.space {
                OwnedSpace::Gray => ImageColorSpace::DeviceGray,
                OwnedSpace::Rgb => ImageColorSpace::DeviceRgb,
                OwnedSpace::Indexed(lookup) => ImageColorSpace::Indexed {
                    base: DeviceSpace::Rgb,
                    lookup,
                },
            },
            filter: self.filter,
            data: &self.data,
            color_key_mask: (!self.color_key_mask.is_empty()).then_some(&self.color_key_mask),
            soft_mask: self.soft_mask.as_ref().map(|m| SoftMask {
                width: m.width,
                height: m.height,
                bits_per_component: m.bits_per_component,
                filter: m.filter,
                data: &m.data,
            }),
        })
    }

    /// Width in pixels, from IHDR.
    #[must_use]
    pub const fn width(&self) -> u32 {
        self.width
    }

    /// Height in pixels, from IHDR.
    #[must_use]
    pub const fn height(&self) -> u32 {
        self.height
    }

    /// Which route the file took.
    #[must_use]
    pub const fn route(&self) -> PngRoute {
        self.route
    }

    /// Whether nothing was degraded.
    ///
    /// On the decoded route this is [`PngImage::complete`] — a short raster or a
    /// damaged IDAT. On the pass-through it means the chunk walk reached IEND
    /// with the file intact, which is as much as can be known without
    /// inflating: the bytes are checked by their CRCs and then trusted, which
    /// is the cost of not building a raster and the reason the CRCs are
    /// compared rather than stored.
    #[must_use]
    pub const fn complete(&self) -> bool {
        self.complete
    }

    /// Typed leniency records (ruling 10), from the walk and from the decode.
    #[must_use]
    pub fn warnings(&self) -> &[FilterWarning] {
        &self.warnings
    }
}

/// Reads a PNG and prepares it for embedding, taking whichever route it needs.
///
/// `limits` bounds the *decoded* route only; the pass-through builds no raster
/// and so has nothing for a ceiling to stop.
///
/// # Errors
/// Any [`PngError`]: a bad signature, a CRC failure on a critical chunk, a
/// colour-type/bit-depth pair outside Table 11.1, and the rest of the refusals
/// [`png_scan`] and [`PngScan::decode`] already name.
pub fn png_image(bytes: &[u8], limits: &Limits) -> Result<PngImageData, PngError> {
    let scan = png_scan(bytes)?;
    match choose(&scan) {
        Route::Through(mask) => Ok(pass_through(scan, mask)),
        Route::Decode => decode(&scan, limits),
    }
}

/// The routing decision, and the colour key when there is one.
enum Route {
    /// Place the IDAT unchanged, with this `/Mask` if any.
    Through(Option<Vec<(u32, u32)>>),
    /// Build a raster.
    Decode,
}

/// What a palette `tRNS` can be expressed as under 8.9.6.4.
enum PaletteKey {
    /// Every entry is opaque, so there is nothing to mask.
    Opaque,
    /// The transparent entries are one contiguous run of indices, which is
    /// exactly the inclusive range `/Mask` tests.
    Range(u32, u32),
    /// Partial alpha, or transparency in two separate runs. `/Mask` is a range
    /// test and not a lookup, so this needs the decoder and an `/SMask`.
    Unexpressible,
}

/// Which route a scanned file takes. The whole of the table in the module note.
fn choose(scan: &PngScan) -> Route {
    // 7.2: seven reduced images, each with its own row width and its own filter
    // tags. `/Predictor` describes one raster and has no way to say "and now a
    // different width", so an interlaced file cannot pass through at all.
    if scan.header.interlaced {
        return Route::Decode;
    }

    let depth = scan.header.bit_depth;
    match (scan.header.colour_type, &scan.transparency) {
        // 4 and 6 interleave alpha into the same scanlines. PDF wants a
        // separate `/SMask` image, which means the samples have to be split.
        (4 | 6, _) => Route::Decode,
        (_, None) => Route::Through(None),
        // 11.3.2.1: for colour type 0 a tRNS names exactly one fully
        // transparent grey level, at the image's own bit depth.
        (0, Some(PngTransparency::Grey(key))) => {
            Route::Through(in_depth(u32::from(*key), depth).map(|k| vec![(k, k)]))
        }
        // And for colour type 2, one fully transparent triple.
        (2, Some(PngTransparency::Rgb(r, g, b))) => {
            match (
                in_depth(u32::from(*r), depth),
                in_depth(u32::from(*g), depth),
                in_depth(u32::from(*b), depth),
            ) {
                (Some(r), Some(g), Some(b)) => Route::Through(Some(vec![(r, r), (g, g), (b, b)])),
                // A component the depth cannot hold can never equal a sample,
                // so the file is opaque and a `/Mask` naming an out-of-range
                // value — which 8.9.6.4 forbids — would be a worse answer than
                // none.
                _ => Route::Through(None),
            }
        }
        (3, Some(PngTransparency::Palette(alpha))) => match palette_key(alpha) {
            PaletteKey::Opaque => Route::Through(None),
            PaletteKey::Range(lo, hi) => Route::Through(Some(vec![(lo, hi)])),
            PaletteKey::Unexpressible => Route::Decode,
        },
        // `png_scan` drops a tRNS whose form does not match its colour type, so
        // no other pairing can arrive here.
        _ => Route::Through(None),
    }
}

/// A `tRNS` key is stored in two bytes whatever the depth is (11.3.2.1), and is
/// compared against the *raw* sample — so a value the depth cannot hold matches
/// nothing.
const fn in_depth(value: u32, depth: u8) -> Option<u32> {
    // `depth` is one of Table 11.1's five, so the shift is at most 16 and
    // `1 << depth` is one past the largest sample it can hold.
    if value < 1u32 << depth {
        Some(value)
    } else {
        None
    }
}

/// 8.9.6.4 over a palette `tRNS`: one inclusive range, or nothing.
fn palette_key(alpha: &[u8]) -> PaletteKey {
    let mut first: Option<usize> = None;
    let mut last = 0usize;
    for (index, &value) in alpha.iter().enumerate() {
        match value {
            // Entries past the tRNS list are fully opaque (11.3.2.1), so the
            // run can never continue past the end of it.
            0xFF => {}
            0 => {
                if first.is_none() {
                    first = Some(index);
                }
                last = index;
            }
            // Anything between is partial alpha, which no range test expresses.
            _ => return PaletteKey::Unexpressible,
        }
    }

    match first {
        None => PaletteKey::Opaque,
        Some(lo) => {
            // Contiguous, or two runs with opaque entries between them.
            if alpha[lo..=last].iter().any(|&v| v != 0) {
                PaletteKey::Unexpressible
            } else {
                PaletteKey::Range(lo as u32, last as u32)
            }
        }
    }
}

/// The default route: the IDAT reaches the page as the bytes the file holds.
fn pass_through(scan: PngScan, mask: Option<Vec<(u32, u32)>>) -> PngImageData {
    let h = scan.header;
    let space = match h.colour_type {
        2 => OwnedSpace::Rgb,
        // The palette is the lookup table. `png_scan` has already enforced
        // 11.2.3's bound against the bit depth, which is the same bound
        // `add_image` re-checks against `/hival`.
        3 => OwnedSpace::Indexed(scan.palette),
        // 0. `choose` lets nothing else reach here.
        _ => OwnedSpace::Gray,
    };

    PngImageData {
        width: h.width,
        height: h.height,
        bits_per_component: h.bit_depth,
        space,
        filter: Some(ImageFilter::FlatePngPredictor {
            // Table 11.1's own channel count, which is 1 for an indexed image
            // because a scanline carries indices — and `/Colors` for an
            // `/Indexed` space is 1 for the same reason.
            colors: h.channels(),
            bits_per_component: u32::from(h.bit_depth),
            columns: h.width,
        }),
        data: scan.idat,
        color_key_mask: mask.unwrap_or_default(),
        soft_mask: None,
        route: PngRoute::PassedThrough,
        // The file ended where it said it would. Nothing here inflates, so the
        // chunk CRCs are the whole of what is known.
        complete: !scan
            .warnings
            .iter()
            .any(|w| matches!(w, FilterWarning::TruncatedInput | FilterWarning::EarlyEod)),
        warnings: scan.warnings,
    }
}

/// The fallback: build the raster, split the alpha off, re-deflate both.
///
/// Re-deflating is lossless — DEFLATE is — so [`ImageData::Jpeg`]'s
/// generational-loss argument does not transfer to this path and must not be
/// cited as though it did. It happens here rather than in the writer so the
/// buffer this value holds for the document's life is the compressed one.
fn decode(scan: &PngScan, limits: &Limits) -> Result<PngImageData, PngError> {
    let image = scan.decode(limits)?;
    let (space, samples, alpha) = split(&image);

    Ok(PngImageData {
        width: image.width,
        height: image.height,
        bits_per_component: image.bits_per_component,
        space,
        filter: Some(ImageFilter::Flate),
        data: zlib_compress(&samples),
        color_key_mask: Vec::new(),
        soft_mask: alpha.map(|a| OwnedMask {
            width: image.width,
            height: image.height,
            bits_per_component: image.bits_per_component,
            filter: Some(ImageFilter::Flate),
            data: zlib_compress(&a),
        }),
        route: PngRoute::Decoded,
        complete: image.complete,
        warnings: image.warnings,
    })
}

/// Separates an interleaved alpha channel from the colour it came with.
///
/// Returns the colour space, the colour samples and the alpha samples. A
/// layout with no alpha is returned whole, with no copy of the raster made.
fn split(image: &PngImage) -> (OwnedSpace, Vec<u8>, Option<Vec<u8>>) {
    // 8 or 16 bits, so one or two bytes a component.
    let unit = usize::from(image.bits_per_component / 8).max(1);
    let (colour_components, has_alpha) = match image.colour {
        PngColour::Grey => (1usize, false),
        PngColour::GreyAlpha => (1, true),
        PngColour::Rgb => (3, false),
        PngColour::Rgba => (3, true),
    };
    let space = if colour_components == 1 {
        OwnedSpace::Gray
    } else {
        OwnedSpace::Rgb
    };

    if !has_alpha {
        return (space, image.data.clone(), None);
    }

    let colour_bytes = colour_components * unit;
    let stride = colour_bytes + unit;
    let pixels = image.data.len() / stride;
    let mut colour = Vec::with_capacity(pixels * colour_bytes);
    let mut alpha = Vec::with_capacity(pixels * unit);
    for pixel in image.data.chunks_exact(stride) {
        colour.extend_from_slice(&pixel[..colour_bytes]);
        alpha.extend_from_slice(&pixel[colour_bytes..]);
    }
    (space, colour, Some(alpha))
}

#[cfg(test)]
mod tests;
