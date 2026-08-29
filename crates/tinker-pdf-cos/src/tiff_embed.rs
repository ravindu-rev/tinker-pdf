//! A TIFF file becomes an image XObject.
//!
//! `png_embed.rs`'s counterpart, and the same module in shape: this is the one
//! that **chooses**, and the choice is the whole reason TIFF is worth reading
//! at all in a PDF engine.
//!
//! # Four of TIFF's codings already have a `/Filter` name
//!
//! A scanned page is the case that matters, and a scanned page is a G4 fax or
//! an LZW or DEFLATE strip. Every one of those is a PDF stream filter already:
//!
//! | `Compression` | `/Filter` | What is written |
//! | --- | --- | --- |
//! | 2, 3, 4 | `/CCITTFaxDecode` (7.4.6) | Table 11 from the directory's own `T4Options` and geometry |
//! | 5 | `/LZWDecode` (7.4.4) | nothing: §13's LZW **is** 7.4.4's, `/EarlyChange 1` included |
//! | 7 | `/DCTDecode` (7.4.8) | nothing; `JPEGTables` is spliced in front, which is still not a raster |
//! | 8, 32946 | `/FlateDecode` (7.4.4) | `/Predictor 2` where the file used one, which Table 10 calls TIFF horizontal differencing |
//!
//! So the common case copies bytes, reads a directory and never builds a
//! raster — the same argument [`crate::png_embed`] makes for a PNG's IDAT, and
//! for the same reason: a comic archive or an XPS package is synthesised whole
//! at open, so whatever a page holds is held for the document's life.
//!
//! # What cannot pass through, and why each one cannot
//!
//! | TIFF feature | Why | Route |
//! | --- | --- | --- |
//! | More than one strip or tile | see below — this is the interesting one | decode |
//! | `Compression` 1, none | there is nothing to pass *through*; the writer compresses the samples either way | decode |
//! | `Compression` 32773, PackBits | 7.4.5 reads the byte 128 as end-of-data where §9 reads it as a no-op, so a PackBits strip is not a `/RunLengthDecode` stream and a strip that happens to contain no 128 only looks like one | decode |
//! | LZW in the pre-1993 bit order | `/LZWDecode` is MSB-first with `/EarlyChange 1`; the other packing decodes to noise in every reader | decode (transcoded) |
//! | `PhotometricInterpretation` 0 outside the fax codings | it needs `/Decode [1 0]`, which [`CompressedImage`] does not carry | decode (inverted) |
//! | `PlanarConfiguration` 2 | PDF has no planar image; the samples have to be interleaved | decode |
//! | `FillOrder` 2 | PDF has no equivalent, and the bytes would have to be reversed to mean anything | decode |
//! | 16-bit samples in an `II` file | 8.9.5.2's samples are big-endian; a little-endian file's are not | decode (swapped) |
//! | `ExtraSamples` | PDF wants a separate `/SMask` image, so the samples have to be split | decode, split, `/SMask` |
//! | Tiles | an edge tile is stored padded out to the tile size (p.67), so even one tile is not the raster its dictionary would declare | decode |
//! | Everything else | — | **pass through** |
//!
//! # The multi-strip caveat, decided per filter rather than waved at
//!
//! A PDF image is one stream. A TIFF is *n* strips. Concatenating them is only
//! legal where the concatenation is still one stream of that filter, and the
//! honest answer is that it never is:
//!
//! - **`/CCITTFaxDecode`**: each strip is an independent T.4/T.6 stream whose
//!   first row is coded against an imaginary all-white reference line. Joined,
//!   strip 2's first row would be decoded against strip 1's last, which is a
//!   different picture rather than a damaged one.
//! - **`/LZWDecode`**: each strip ends with code 257, EndOfInformation. A
//!   decoder stops there, so the second strip and everything after it is
//!   silently dropped.
//! - **`/FlateDecode`**: each strip is its own zlib stream with its own header
//!   and Adler-32. The second header would be read as compressed data.
//! - **`/DCTDecode`**: two JPEG datastreams end to end are not a JPEG.
//!
//! Four filters, four different reasons, and all four say no — so
//! [`TiffRoute::Placed`] means one strip and nothing else. That is not the
//! limitation it sounds like for the case this exists for: a fax and a scanned
//! page are routinely one strip, because `RowsPerStrip` defaults to 2^32-1
//! (p.39) and a writer that does not set it has written exactly one.
//!
//! # What the pass-through gives up, said plainly
//!
//! A PNG carries a CRC-32 on every chunk, so `png_embed`'s pass-through can
//! check the bytes it is about to copy and [`crate::PngImageData::complete`]
//! means something. **A TIFF carries no checksum at all.** The most this
//! module can know without decompressing is that the strip the directory
//! pointed at is inside the file and is not empty, and that is exactly what
//! [`TiffImageData::complete`] reports on the placed route. A corrupt G4 strip
//! placed unchanged reaches the page as a corrupt G4 strip, and the renderer's
//! own fax recovery is what handles it — which is the same recovery it would
//! have applied had this module decoded first, so nothing is lost by not
//! looking.

use tinker_pdf_filters::{
    tiff_scan, zlib_compress, CcittParams, Limits, TiffColour, TiffCompression, TiffError,
    TiffImage, TiffLayout, TiffPhotometric, TiffPlanar, TiffScan, Warning as FilterWarning,
};

use crate::build::{
    CompressedImage, DeviceSpace, ImageColorSpace, ImageData, ImageFilter, SoftMask,
};

/// Which of the three routes a TIFF took.
///
/// Public because it is the claim worth testing. "The pass-through happened"
/// and "the picture is right" are different assertions, and a build that
/// quietly decoded everything would satisfy the second while throwing away the
/// reason this module exists.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TiffRoute {
    /// The strip's own bytes reached the page, byte for byte, under the
    /// `/Filter` its `Compression` names.
    Placed,
    /// The strip's bytes and the file's `JPEGTables`, joined — TIFF Technical
    /// Note 2's two halves of one JPEG datastream. Still no raster: the only
    /// work is a concatenation of two subslices.
    Spliced,
    /// The file was decoded to samples and re-encoded.
    Decoded,
}

/// The `/ColorSpace` this module produces, holding its own palette.
enum OwnedSpace {
    Gray,
    Rgb,
    /// `[/Indexed /DeviceRGB hival lookup]`. A TIFF `ColorMap` arrives from
    /// `tiff_scan` already transposed out of p.23's three arrays and scaled to
    /// eight bits, which is exactly the layout 8.6.6.3 wants.
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

/// A TIFF prepared for [`crate::DocumentBuilder::add_image`].
///
/// It owns its buffers because two of the three routes produce them, and the
/// third — [`TiffRoute::Placed`] — copies one subslice rather than borrowing
/// it so that all three have one lifetime. [`TiffImageData::image`] borrows
/// from this, so the value has to outlive the `add_image` call, which is also
/// what keeps the peak at one page's worth rather than the document's.
pub struct TiffImageData {
    width: u32,
    height: u32,
    bits_per_component: u8,
    space: OwnedSpace,
    filter: Option<ImageFilter>,
    data: Vec<u8>,
    soft_mask: Option<OwnedMask>,
    route: TiffRoute,
    complete: bool,
    warnings: Vec<FilterWarning>,
}

impl TiffImageData {
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
            // A TIFF has no `tRNS`: transparency is `ExtraSamples`, which is a
            // per-sample opacity and therefore an `/SMask` rather than
            // 8.9.6.4's range test. So this is always `None`, and it is not a
            // gap — there is nothing in the format a colour key would express.
            color_key_mask: None,
            soft_mask: self.soft_mask.as_ref().map(|m| SoftMask {
                width: m.width,
                height: m.height,
                bits_per_component: m.bits_per_component,
                filter: m.filter,
                data: &m.data,
            }),
        })
    }

    /// Width in pixels, from `ImageWidth`.
    #[must_use]
    pub const fn width(&self) -> u32 {
        self.width
    }

    /// Height in pixels, from `ImageLength`.
    #[must_use]
    pub const fn height(&self) -> u32 {
        self.height
    }

    /// Which route the file took.
    #[must_use]
    pub const fn route(&self) -> TiffRoute {
        self.route
    }

    /// Whether nothing was degraded.
    ///
    /// On the decoded route this is [`TiffImage::complete`] — a short strip, a
    /// damaged coded stream, a segment that would not decode. On the placed
    /// and spliced routes it means the strip the directory pointed at is
    /// inside the file and is not empty, which is **the whole of what can be
    /// known without decompressing**: see the module note on the checksum a
    /// TIFF does not have.
    #[must_use]
    pub const fn complete(&self) -> bool {
        self.complete
    }

    /// Typed leniency records (ruling 10), from the directory walk and, on the
    /// decoded route, from the decode.
    #[must_use]
    pub fn warnings(&self) -> &[FilterWarning] {
        &self.warnings
    }
}

/// Reads a TIFF and prepares it for embedding, taking whichever route it needs.
///
/// `limits` bounds the *decoded* route only; the two pass-through routes build
/// no raster and so have nothing for a ceiling to stop.
///
/// # Errors
/// Any [`TiffError`]: a bad header, a `Compression` or
/// `PhotometricInterpretation` this build does not read, a geometry the
/// directory's own arrays cannot describe, and the rest of the refusals
/// [`tiff_scan`] already names.
pub fn tiff_image(bytes: &[u8], limits: &Limits) -> Result<TiffImageData, TiffError> {
    let scan = tiff_scan(bytes)?;
    match choose(&scan) {
        Some(placed) => Ok(place(&scan, placed)),
        None => decode(&scan, limits),
    }
}

/// What a placed strip needs beyond its own bytes.
struct Placed {
    filter: ImageFilter,
    bits_per_component: u8,
    space: PlacedSpace,
    /// `Some` for `Compression` 7 with a `JPEGTables`: the tables without
    /// their EOI, to be written in front of the strip without its SOI.
    tables: Option<Vec<u8>>,
}

enum PlacedSpace {
    Gray,
    Rgb,
    Indexed,
}

/// Whether a file can reach the page as its own bytes, and as what.
///
/// `None` sends it to the decoder. Every arm of this function is a row of the
/// module note's second table; nothing is refused here without a sentence
/// there saying why.
fn choose(scan: &TiffScan<'_>) -> Option<Placed> {
    // One stream in, one stream out. The module note works through why each of
    // the four filters refuses a concatenation, and they all do.
    if scan.segments_needed() != 1 || scan.segments.first().copied().is_none_or(<[u8]>::is_empty) {
        return None;
    }
    // p.67: an edge tile is stored padded out to the full tile, so a
    // single-tile image whose dimensions are not the tile's carries rows and
    // columns the dictionary would not declare.
    if !matches!(scan.layout, TiffLayout::Strips { .. }) {
        return None;
    }
    if scan.planar != TiffPlanar::Chunky || scan.fill_order != 1 {
        return None;
    }
    // `ExtraSamples` is a per-sample opacity, which PDF wants as a separate
    // image; splitting it means having the samples.
    if u32::from(scan.samples_per_pixel) > scan.photometric.colour_channels() {
        return None;
    }
    // 8.9.5.2: a 16-bit sample is big-endian. A `II` file's is not.
    if scan.bits_per_sample == 16 && scan.little_endian {
        return None;
    }
    if !matches!(scan.bits_per_sample, 1 | 2 | 4 | 8 | 16) {
        return None;
    }

    let predictor = |colors: u32, flate: bool| -> Option<ImageFilter> {
        match scan.predictor {
            1 if flate => Some(ImageFilter::Flate),
            1 => Some(ImageFilter::Lzw),
            2 if flate => Some(ImageFilter::FlateTiffPredictor {
                colors,
                bits_per_component: u32::from(scan.bits_per_sample),
                columns: scan.width,
            }),
            2 => Some(ImageFilter::LzwTiffPredictor {
                colors,
                bits_per_component: u32::from(scan.bits_per_sample),
                columns: scan.width,
            }),
            _ => None,
        }
    };

    match scan.compression {
        TiffCompression::ModifiedHuffman | TiffCompression::CcittG3 | TiffCompression::CcittG4 => {
            if scan.bits_per_sample != 1 || scan.samples_per_pixel != 1 || scan.predictor != 1 {
                return None;
            }
            let fax = scan.ccitt?;
            let photometric = match scan.photometric {
                TiffPhotometric::WhiteIsZero => false,
                TiffPhotometric::BlackIsZero => true,
                _ => return None,
            };
            Some(Placed {
                filter: ImageFilter::CcittFax(CcittParams {
                    k: fax.k,
                    columns: scan.width,
                    rows: scan.height,
                    // The one parameter that carries the photometric, and the
                    // reason no `/Decode` array is needed. `/BlackIs1 false`
                    // makes the filter emit 0 for a black run, and
                    // `/DeviceGray` renders 0 as black — which is what
                    // `WhiteIsZero` asked for, since its sample 1 is black.
                    // `BlackIsZero` reverses both halves and so reverses this.
                    black_is_1: photometric,
                    byte_align: fax.byte_align,
                    // A TIFF strip is not required to carry either, and
                    // `/Rows` above is the authority for where it ends.
                    end_of_line: false,
                    end_of_block: false,
                }),
                bits_per_component: 1,
                space: PlacedSpace::Gray,
                tables: None,
            })
        }
        TiffCompression::Jpeg => {
            if scan.bits_per_sample != 8 || scan.predictor != 1 {
                return None;
            }
            let space = match (scan.photometric, scan.samples_per_pixel) {
                (TiffPhotometric::BlackIsZero, 1) => PlacedSpace::Gray,
                // 7.4.8 and JFIF agree with T.81: a three-component JPEG is
                // YCbCr and `/DCTDecode` converts it, which is the same thing
                // TIFF §21 asked the reader to do.
                (TiffPhotometric::Rgb | TiffPhotometric::YCbCr, 3) => PlacedSpace::Rgb,
                _ => return None,
            };
            // Technical Note 2: `SOI tables EOI` and `SOI frame EOI` join into
            // one datastream as the first without its EOI and the second
            // without its SOI.
            let tables = match scan.jpeg_tables {
                Some(t) if t.len() > 4 && t.starts_with(&[0xFF, 0xD8]) => {
                    Some(t.get(..t.len() - 2)?.to_vec())
                }
                _ => None,
            };
            Some(Placed {
                filter: ImageFilter::Dct,
                bits_per_component: 8,
                space,
                tables,
            })
        }
        TiffCompression::Lzw | TiffCompression::Deflate => {
            let flate = scan.compression == TiffCompression::Deflate;
            if !flate && !scan.lzw_bit_order_is_standard() {
                return None;
            }
            let (space, colors) = match (scan.photometric, scan.samples_per_pixel) {
                (TiffPhotometric::BlackIsZero, 1) => (PlacedSpace::Gray, 1),
                (TiffPhotometric::Rgb, 3) => (PlacedSpace::Rgb, 3),
                // 8.6.6.3 caps `/hival` at 255, so an indexed image is at most
                // eight bits whatever TIFF would allow.
                (TiffPhotometric::Palette, 1) if scan.bits_per_sample <= 8 => {
                    (PlacedSpace::Indexed, 1)
                }
                _ => return None,
            };
            Some(Placed {
                filter: predictor(colors, flate)?,
                bits_per_component: scan.bits_per_sample as u8,
                space,
                tables: None,
            })
        }
        // Nothing to pass through, and the module note's second table says why
        // for each.
        TiffCompression::None | TiffCompression::PackBits => None,
    }
}

/// The default route: the strip reaches the page as the bytes the file holds.
fn place(scan: &TiffScan<'_>, placed: Placed) -> TiffImageData {
    let strip = scan.segments.first().copied().unwrap_or(&[]);
    let (data, route) = match &placed.tables {
        Some(tables) if strip.starts_with(&[0xFF, 0xD8]) => (
            [tables.as_slice(), strip.get(2..).unwrap_or(strip)].concat(),
            TiffRoute::Spliced,
        ),
        _ => (strip.to_vec(), TiffRoute::Placed),
    };

    TiffImageData {
        width: scan.width,
        height: scan.height,
        bits_per_component: placed.bits_per_component,
        space: match placed.space {
            PlacedSpace::Gray => OwnedSpace::Gray,
            PlacedSpace::Rgb => OwnedSpace::Rgb,
            PlacedSpace::Indexed => OwnedSpace::Indexed(scan.color_map.clone()),
        },
        filter: Some(placed.filter),
        data,
        soft_mask: None,
        route,
        // Everything a directory walk can know. See the module note: there is
        // no checksum in a TIFF to compare, so this is a claim about the file's
        // structure and not about its bytes.
        complete: !strip.is_empty()
            && !scan
                .warnings
                .iter()
                .any(|w| matches!(w, FilterWarning::TiffDirectoryCycle)),
        warnings: scan.warnings.clone(),
    }
}

/// The fallback: build the raster, split the alpha off, re-deflate both.
///
/// Re-deflating is lossless — DEFLATE is — so [`ImageData::Jpeg`]'s
/// generational-loss argument does not transfer to this path. It happens here
/// rather than in the writer so the buffer this value holds for the document's
/// life is the compressed one.
fn decode(scan: &TiffScan<'_>, limits: &Limits) -> Result<TiffImageData, TiffError> {
    let image = scan.decode(limits)?;
    let (space, samples, alpha) = split(&image);

    Ok(TiffImageData {
        width: image.width,
        height: image.height,
        bits_per_component: image.bits_per_component,
        space,
        filter: Some(ImageFilter::Flate),
        data: zlib_compress(&samples),
        soft_mask: alpha.map(|a| OwnedMask {
            width: image.width,
            height: image.height,
            bits_per_component: image.bits_per_component,
            filter: Some(ImageFilter::Flate),
            data: zlib_compress(&a),
        }),
        route: TiffRoute::Decoded,
        complete: image.complete,
        warnings: image.warnings,
    })
}

/// Separates an interleaved alpha channel from the colour it came with.
///
/// `png_embed`'s `split` over the same four layouts, which is why
/// [`TiffColour`] has exactly [`tinker_pdf_filters::PngColour`]'s four: two
/// decoders feed one `/SMask` split, and a second layout to learn would be a
/// second place to get it wrong.
fn split(image: &TiffImage) -> (OwnedSpace, Vec<u8>, Option<Vec<u8>>) {
    // 8 or 16 bits, so one or two bytes a component.
    let unit = usize::from(image.bits_per_component / 8).max(1);
    let (colour_components, has_alpha) = match image.colour {
        TiffColour::Grey => (1usize, false),
        TiffColour::GreyAlpha => (1, true),
        TiffColour::Rgb => (3, false),
        TiffColour::Rgba => (3, true),
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
