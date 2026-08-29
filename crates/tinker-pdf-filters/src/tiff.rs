//! TIFF 6.0 — a *container* decoder, and the second one this crate holds.
//!
//! No PDF stream is ever a TIFF file, so this is not a `/Filter` and does not
//! appear in [`crate::Filter`]. It is here for the same reason `png.rs` is:
//! XPS and CBZ hand out image files rather than PDF streams, and **everything
//! a TIFF is made of already lives in this crate**. That is not a convenience,
//! it is the whole argument for the format landing here rather than in a
//! reader:
//!
//! | TIFF `Compression` | What decodes it | Written for |
//! | --- | --- | --- |
//! | 1 none | this module | — |
//! | 2 modified Huffman, 3 G3, 4 G4 | `ccitt.rs` | `/CCITTFaxDecode` (7.4.6) |
//! | 5 LZW | `lzw.rs` | `/LZWDecode` (7.4.4) |
//! | 7 JPEG | `jpeg.rs` | `/DCTDecode` (7.4.8) |
//! | 8, 32946 Deflate | `inflate.rs` | `/FlateDecode` (7.4.4) |
//! | 32773 PackBits | `packbits.rs` | TIFF 6.0 §9 |
//! | `Predictor` 2 | `predictors.rs` | `/Predictor 2`, Table 10 |
//!
//! Six of the seven were written for a PDF filter and are reached here
//! unchanged. Only the directory walk, the strip and tile geometry and the
//! photometric interpretations are new — which is roughly the top third of
//! this file, and the reason the rest of it is short for a format with this
//! much surface.
//!
//! # The two quirks that are not in TIFF 6.0
//!
//! **Old-style LZW.** TIFF 6.0 §13 packs LZW codes most-significant bit first
//! and switches code width one code early — the same "off by one" PDF spells
//! `/EarlyChange 1`. A generation of encoders wrote the codes **least**
//! significant bit first and switched one code *late*, and files from them are
//! still in circulation. They are detected the way every reader detects them,
//! by the only two bytes that can tell them apart: a stream that opens with
//! the Clear code 256 begins `0x80` when the codes are MSB-first and `0x00`
//! with bit 0 of the next byte set when they are LSB-first. Such a stream is
//! **transcoded** into the ordinary form and handed to `lzw.rs` — the
//! dictionary is not reimplemented, only the packing, and the two width rules
//! are modelled side by side in [`transcode_old_style_lzw`] so they cannot
//! drift.
//!
//! **`ColorMap` written at 8 bits.** TIFF 6.0 p.23 says the map holds
//! `SHORT`s from 0 to 65535. A great many writers store 0 to 255 in the same
//! `SHORT`s. A map whose every entry is at or below 255 is therefore read as
//! an 8-bit one, with a warning — the alternative is a palette image that is
//! uniformly, almost perfectly black, which reads as a decoder bug rather than
//! as the file's.
//!
//! # The output shape
//!
//! Ruling 8 keeps this crate PDF-free, so what comes out is samples and
//! values: an interleaved raster in one of four channel layouts at 8 or 16
//! bits, exactly [`crate::PngImage`]'s shape and for the same reason — the
//! consumer that splits an alpha channel into an `/SMask` should not have to
//! learn two layouts. Depths 1, 2 and 4 are expanded to 8 by multiplication,
//! a `ColorMap` is *applied* rather than handed back, and
//! `PhotometricInterpretation` 0 is inverted here so that a caller never has
//! to ask which end of the range is black.
//!
//! # What is refused, and why each one is refused rather than guessed at
//!
//! [`TiffError`] names all of it. The pattern is `png.rs`'s: damage that costs
//! *pixels* is a [`Warning`] and leaves a partial raster (ruling 2), and
//! anything that would make this module produce a plausible picture of the
//! wrong thing is an error. A `PhotometricInterpretation` of 5 (CMYK) decoded
//! as RGB is not a degraded image, it is a different one.

use crate::{
    ccitt, inflate, jpeg, lzw, packbits, predictors, CcittParams, Limits, PredictorParams, Warning,
    Warnings,
};

// --- the budgets --------------------------------------------------------

/// Samples in the **output** raster — `width x height x components` — checked
/// with a saturating multiply before any buffer exists.
///
/// The same `1 << 26` as [`crate::MAX_PNG_SAMPLES`] and `jpx`'s
/// `MAX_JPX_SAMPLES`, for the same arithmetic: at 16 bits a component it is
/// 134 217 728 bytes, which is `tinker_pdf_cos::limits::MAX_DECODED_STREAM` to
/// the byte.
///
/// **Reachable.** `ImageWidth` and `ImageLength` are 32-bit fields, so a
/// twenty-six byte file can ask for 2^62 samples, and
/// `an_image_past_the_sample_cap_is_refused_before_it_allocates` builds one.
pub const MAX_TIFF_SAMPLES: u64 = 1 << 26;

/// Image file directories followed down the `NextIFD` chain.
///
/// A depth bound *as well as* the cycle guard, because the two catch different
/// files: the guard catches an IFD that points at itself or at an earlier one,
/// and this catches a long descending chain that never repeats. Sixteen would
/// cover every multi-page fax anyone has sent; 64 is the round number above
/// it, and both are far below the point where the walk costs anything.
const MAX_TIFF_IFDS: usize = 64;

/// Strips or tiles in one image.
///
/// **Reachable**, and by the cheapest possible file: `StripOffsets` is a
/// `LONG` array, so four bytes of tag data buys one more segment and a four
/// megabyte file can declare a million of them. The cap is charged against the
/// *declared* count before any of the arrays are read.
const MAX_TIFF_SEGMENTS: usize = 1 << 20;

/// Values read from one tag.
///
/// `Count` is a 32-bit field and this module reads whole arrays into `Vec`s —
/// `StripOffsets`, `StripByteCounts`, `ColorMap`, `BitsPerSample`. The largest
/// legitimate one is a `ColorMap` at 3 x 65536 entries, so the cap is the
/// round number above that.
const MAX_TIFF_TAG_VALUES: usize = 1 << 20;

// --- what the header says -----------------------------------------------

/// TIFF 6.0 p.13: `II` little-endian, `MM` big-endian, and no third.
const LITTLE_ENDIAN: [u8; 2] = *b"II";
const BIG_ENDIAN: [u8; 2] = *b"MM";

/// p.13's "arbitrary but carefully chosen number (42)".
const TIFF_MAGIC: u16 = 42;

/// BigTIFF's magic, which is not TIFF 6.0's and is refused by name.
const BIGTIFF_MAGIC: u16 = 43;

// --- the tags -----------------------------------------------------------

const TAG_IMAGE_WIDTH: u16 = 256;
const TAG_IMAGE_LENGTH: u16 = 257;
const TAG_BITS_PER_SAMPLE: u16 = 258;
const TAG_COMPRESSION: u16 = 259;
const TAG_PHOTOMETRIC: u16 = 262;
const TAG_FILL_ORDER: u16 = 266;
const TAG_STRIP_OFFSETS: u16 = 273;
const TAG_SAMPLES_PER_PIXEL: u16 = 277;
const TAG_ROWS_PER_STRIP: u16 = 278;
const TAG_STRIP_BYTE_COUNTS: u16 = 279;
const TAG_X_RESOLUTION: u16 = 282;
const TAG_Y_RESOLUTION: u16 = 283;
const TAG_PLANAR_CONFIGURATION: u16 = 284;
const TAG_T4_OPTIONS: u16 = 292;
const TAG_RESOLUTION_UNIT: u16 = 296;
const TAG_PREDICTOR: u16 = 317;
const TAG_COLOR_MAP: u16 = 320;
const TAG_TILE_WIDTH: u16 = 322;
const TAG_TILE_LENGTH: u16 = 323;
const TAG_TILE_OFFSETS: u16 = 324;
const TAG_TILE_BYTE_COUNTS: u16 = 325;
const TAG_EXTRA_SAMPLES: u16 = 338;
const TAG_SAMPLE_FORMAT: u16 = 339;
const TAG_JPEG_TABLES: u16 = 347;

// --- the refusals -------------------------------------------------------

/// Why an image was refused outright.
///
/// Every variant names what was wrong in terms a caller can show a human, for
/// `png.rs`'s reason: "this is not a TIFF" and "this is a CMYK TIFF" are
/// different sentences and a page-level handler shows different things for
/// them. Damage that costs *pixels* rather than meaning is a [`Warning`]
/// instead and leaves a partial raster (ruling 2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TiffError {
    /// The first four bytes are neither `II*\0` nor `MM\0*`.
    NotTiff,
    /// The magic number was 43: BigTIFF, whose offsets are eight bytes wide
    /// and whose directory layout is a different format wearing the same two
    /// order bytes.
    BigTiff,
    /// The first directory's offset does not land inside the file, or the
    /// directory is cut off inside its own entry array.
    UnreadableDirectory,
    /// A tag with no default that the image cannot be read without.
    MissingTag(u16),
    /// A zero width or height, or a product past `u32`.
    BadDimensions { width: u32, height: u32 },
    /// A `Compression` value this build does not decode — 6 (old-style JPEG,
    /// withdrawn by TIFF Technical Note 2), 34712 (JPEG 2000) and the rest.
    UnsupportedCompression(u16),
    /// A `PhotometricInterpretation` outside {0, 1, 2, 3} — and 6, which is
    /// read only when `Compression` is 7 and the JPEG has already undone it.
    UnsupportedPhotometric(u16),
    /// A `BitsPerSample` outside {1, 2, 4, 8, 16}.
    UnsupportedBitDepth(u16),
    /// `BitsPerSample` gave different depths for different samples. TIFF 6.0
    /// permits it; nothing in this decoder's sample path carries two depths at
    /// once, and half-expanding one would be worse than saying so.
    UnequalBitDepths,
    /// `SampleFormat` other than 1 (unsigned integer). Two's-complement and
    /// IEEE floating-point samples are a different number line, and reading
    /// them as unsigned produces a picture rather than a refusal.
    UnsupportedSampleFormat(u16),
    /// `Predictor` other than 1 or 2. Value 3 is floating-point predictor
    /// (TIFF Technical Note 3), which travels with float samples.
    UnsupportedPredictor(u16),
    /// `PlanarConfiguration` other than 1 or 2.
    UnsupportedPlanarConfiguration(u16),
    /// `PhotometricInterpretation` 3 with no usable `ColorMap`. The samples
    /// are indices into a table that does not exist.
    MissingColorMap,
    /// Neither `StripOffsets` nor `TileOffsets`. There is no image, as
    /// distinct from a damaged one.
    NoImageData,
    /// The offsets array and the byte-counts array are different lengths, or
    /// there are not enough of either for the geometry. Two independent
    /// length systems over one image, and neither validates the other.
    InconsistentSegments,
    /// A tile geometry TIFF 6.0 p.67 forbids: a zero dimension, or one that is
    /// not a multiple of 16.
    BadTileGeometry { width: u32, height: u32 },
    /// [`MAX_TIFF_SAMPLES`] would be spent. Refused before any buffer exists.
    TooManySamples { samples: u64, max: u64 },
    /// [`MAX_TIFF_SEGMENTS`] would be spent.
    TooManySegments { segments: u64, max: usize },
    /// The raster would be larger than the caller's own [`Limits::max_output`].
    /// Separate from [`TiffError::TooManySamples`] because one number is this
    /// crate's and the other is the caller's.
    ExceedsOutputLimit { bytes: u64, limit: usize },
}

impl core::fmt::Display for TiffError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::NotTiff => f.write_str("not a TIFF: no II*\\0 or MM\\0* header"),
            Self::BigTiff => f.write_str("BigTIFF (magic 43), which is a different format"),
            Self::UnreadableDirectory => f.write_str("no readable image file directory"),
            Self::MissingTag(t) => write!(f, "tag {t} is absent and has no default"),
            Self::BadDimensions { width, height } => {
                write!(f, "image dimensions {width} x {height}")
            }
            Self::UnsupportedCompression(c) => write!(f, "Compression {c}"),
            Self::UnsupportedPhotometric(p) => write!(f, "PhotometricInterpretation {p}"),
            Self::UnsupportedBitDepth(b) => write!(f, "BitsPerSample {b}"),
            Self::UnequalBitDepths => f.write_str("BitsPerSample gave two depths for one image"),
            Self::UnsupportedSampleFormat(s) => write!(f, "SampleFormat {s}"),
            Self::UnsupportedPredictor(p) => write!(f, "Predictor {p}"),
            Self::UnsupportedPlanarConfiguration(p) => write!(f, "PlanarConfiguration {p}"),
            Self::MissingColorMap => f.write_str("a palette image with no ColorMap"),
            Self::NoImageData => f.write_str("neither StripOffsets nor TileOffsets"),
            Self::InconsistentSegments => {
                f.write_str("the offsets and the byte counts do not describe one image")
            }
            Self::BadTileGeometry { width, height } => {
                write!(f, "tile geometry {width} x {height}")
            }
            Self::TooManySamples { samples, max } => {
                write!(f, "{samples} samples, ceiling is {max}")
            }
            Self::TooManySegments { segments, max } => {
                write!(f, "{segments} strips or tiles, ceiling is {max}")
            }
            Self::ExceedsOutputLimit { bytes, limit } => {
                write!(f, "{bytes} bytes of raster, caller's ceiling is {limit}")
            }
        }
    }
}

impl std::error::Error for TiffError {}

// --- what the directory said --------------------------------------------

/// The `Compression` values this build decodes, by the names TIFF 6.0 uses.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TiffCompression {
    /// 1: the samples are the bytes.
    None,
    /// 2: CCITT Group 3 one-dimensional, byte-aligned per row, no EOLs
    /// (TIFF 6.0 §10).
    ModifiedHuffman,
    /// 3: CCITT Group 3 (T.4), one- or two-dimensional per `T4Options`.
    CcittG3,
    /// 4: CCITT Group 4 (T.6).
    CcittG4,
    /// 5: LZW (§13), in whichever of the two bit orders the strip turns out to
    /// use.
    Lzw,
    /// 7: JPEG, the form TIFF Technical Note 2 defines with `JPEGTables`.
    Jpeg,
    /// 8 or 32946: DEFLATE in a zlib wrapper. Two codes for one coding — 32946
    /// was Adobe's private registration and 8 is its adopted form, and files
    /// carrying either are identical inside.
    Deflate,
    /// 32773: PackBits (§9).
    PackBits,
}

impl TiffCompression {
    fn from_code(code: u16) -> Option<Self> {
        Some(match code {
            1 => Self::None,
            2 => Self::ModifiedHuffman,
            3 => Self::CcittG3,
            4 => Self::CcittG4,
            5 => Self::Lzw,
            7 => Self::Jpeg,
            8 | 32946 => Self::Deflate,
            32773 => Self::PackBits,
            _ => return None,
        })
    }

    /// Whether the coded bytes are a bit stream rather than a byte stream,
    /// which is the only case in which `FillOrder` means anything.
    const fn is_bit_oriented(self) -> bool {
        matches!(
            self,
            Self::None | Self::ModifiedHuffman | Self::CcittG3 | Self::CcittG4 | Self::PackBits
        )
    }
}

/// `PhotometricInterpretation` (tag 262), for the values this build reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TiffPhotometric {
    /// 0: 0 is white. The fax default, and the one that has to be inverted.
    WhiteIsZero,
    /// 1: 0 is black.
    BlackIsZero,
    /// 2: RGB.
    Rgb,
    /// 3: the sample is an index into `ColorMap`.
    Palette,
    /// 6: YCbCr — **only** with `Compression` 7, where T.81's own colour
    /// transform has already run by the time this module sees a sample. TIFF
    /// 6.0 §21's subsampled YCbCr over any other compression is refused, since
    /// nothing here would undo the subsampling.
    YCbCr,
}

/// `PlanarConfiguration` (tag 284).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TiffPlanar {
    /// 1: the samples of a pixel are adjacent.
    Chunky,
    /// 2: each sample has its own strips or tiles, all of one before any of
    /// the next.
    Planar,
}

/// How the image is cut up.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TiffLayout {
    /// Full-width bands of `rows_per_strip` rows.
    ///
    /// TIFF 6.0 p.39 defaults `RowsPerStrip` to 2^32-1, "which is effectively
    /// infinity" — one strip holding the whole image.
    Strips { rows_per_strip: u32 },
    /// A grid of tiles, each `width` x `height`, both multiples of 16 (p.67).
    /// The right and bottom edges are padded out to the tile size and the
    /// padding is discarded here.
    Tiles { width: u32, height: u32 },
}

/// The channel layout of [`TiffImage::data`] — [`crate::PngColour`]'s four,
/// deliberately, so a consumer that splits an alpha channel learns one shape.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TiffColour {
    Grey,
    GreyAlpha,
    Rgb,
    Rgba,
}

impl TiffColour {
    #[must_use]
    pub const fn components(self) -> u32 {
        match self {
            Self::Grey => 1,
            Self::GreyAlpha => 2,
            Self::Rgb => 3,
            Self::Rgba => 4,
        }
    }

    /// Whether the last component is an opacity rather than a colour.
    #[must_use]
    pub const fn has_alpha(self) -> bool {
        matches!(self, Self::GreyAlpha | Self::Rgba)
    }
}

/// `XResolution` / `YResolution` (282, 283) and `ResolutionUnit` (296).
///
/// Carried rather than acted on: a resolution is a statement about physical
/// size, and every consumer this repository has decided its own page geometry
/// before TIFF arrived. It is read because a `RATIONAL` that does not parse is
/// evidence about the file, and because the tags are in the brief's list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TiffResolution {
    /// Numerator and denominator, exactly as the `RATIONAL` holds them. Not
    /// divided here: a zero denominator is the file's business to have written
    /// and not this module's to turn into a division.
    pub x: (u32, u32),
    pub y: (u32, u32),
    /// 1 none, 2 inch, 3 centimetre (p.38). Defaults to 2.
    pub unit: u16,
}

/// The CCITT parameters the directory implies, for compressions 2, 3 and 4.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TiffCcitt {
    /// `/K`'s sense: 0 one-dimensional, positive mixed, negative G4.
    pub k: i32,
    /// Whether each row starts on a byte boundary — always for compression 2
    /// (§10), and for compression 3 when `T4Options` bit 2 is set.
    pub byte_align: bool,
}

/// The directory walk's result: everything but the pixels.
///
/// Handed back whole so the *pass-through* path can have the geometry, the
/// palette and the coded bytes of every strip without any of them being
/// decompressed — which is `png_scan`'s split, for `png_scan`'s reason.
/// [`TiffScan::decode`] is the other half.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TiffScan<'a> {
    pub width: u32,
    pub height: u32,
    /// One depth for every sample; [`TiffError::UnequalBitDepths`] otherwise.
    pub bits_per_sample: u16,
    /// `SamplesPerPixel`, colour channels **and** extras.
    pub samples_per_pixel: u16,
    pub compression: TiffCompression,
    pub photometric: TiffPhotometric,
    pub planar: TiffPlanar,
    /// 1 or 2; anything else is refused.
    pub predictor: u16,
    /// 1 MSB-first, 2 LSB-first (p.32). Honoured for bit-oriented codings and
    /// reported and ignored for the rest — see the module note.
    pub fill_order: u16,
    pub layout: TiffLayout,
    /// The `ColorMap`, **already scaled to eight bits and interleaved into RGB
    /// triples**. TIFF 6.0 p.23 stores it as three consecutive arrays — every
    /// red, then every green, then every blue — which is not how PNG's PLTE or
    /// PDF's `/Indexed` lookup is laid out, and the transposition is done here
    /// so it is done once.
    pub color_map: Vec<u8>,
    /// `ExtraSamples` (338), one value per sample past the colour channels.
    pub extra_samples: Vec<u16>,
    pub resolution: Option<TiffResolution>,
    /// Present exactly when `compression` is one of the three CCITT ones.
    pub ccitt: Option<TiffCcitt>,
    /// `JPEGTables` (347): an abbreviated table-specification datastream, for
    /// `Compression` 7's "new-style" form.
    pub jpeg_tables: Option<&'a [u8]>,
    /// Every strip or tile, in the order the offsets array gave them, each
    /// already checked to lie inside the file.
    pub segments: Vec<&'a [u8]>,
    /// True for `II`. Load-bearing for 16-bit samples, which arrive in the
    /// file's own order and leave in big-endian.
    pub little_endian: bool,
    /// Directories on the `NextIFD` chain, bounded by [`MAX_TIFF_IFDS`] and by
    /// the cycle guard. Only the first is decoded.
    pub pages: u32,
    /// Typed leniency records (ruling 10), deduplicated.
    pub warnings: Vec<Warning>,
}

/// One decoded image.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TiffImage {
    pub width: u32,
    pub height: u32,
    pub colour: TiffColour,
    /// 8 or 16. Depths 1, 2 and 4 arrive here expanded to 8; a palette image
    /// is always 8, because a `ColorMap` entry is scaled to one.
    pub bits_per_component: u8,
    /// Interleaved, row-major from the top, no row padding. 16-bit components
    /// are big-endian whatever the file's own order was.
    pub data: Vec<u8>,
    /// False when a strip came up short, a coded stream was damaged, or a
    /// segment had to be dropped — [`crate::Decoded::complete`]'s contract.
    pub complete: bool,
    pub warnings: Vec<Warning>,
}

// --- the entry points ---------------------------------------------------

/// Header, directory walk, decompression, prediction and colour.
///
/// # Errors
/// Any [`TiffError`].
pub fn tiff_decode(bytes: &[u8], limits: &Limits) -> Result<TiffImage, TiffError> {
    tiff_scan(bytes)?.decode(limits)
}

/// Reads the header and the first image file directory, and finds every strip
/// or tile, **without decompressing any of them**.
///
/// # Errors
/// Every [`TiffError`] except [`TiffError::ExceedsOutputLimit`], which belongs
/// to a ceiling this half never reaches.
pub fn tiff_scan(bytes: &[u8]) -> Result<TiffScan<'_>, TiffError> {
    let mut w = Warnings::default();

    let order = bytes.get(..2).ok_or(TiffError::NotTiff)?;
    let little = if order == LITTLE_ENDIAN {
        true
    } else if order == BIG_ENDIAN {
        false
    } else {
        return Err(TiffError::NotTiff);
    };
    let file = Cursor {
        bytes,
        little_endian: little,
    };
    match file.u16_at(2) {
        Some(TIFF_MAGIC) => {}
        Some(BIGTIFF_MAGIC) => return Err(TiffError::BigTiff),
        _ => return Err(TiffError::NotTiff),
    }
    let first = file.u32_at(4).ok_or(TiffError::UnreadableDirectory)? as usize;

    let fields = read_directory(&file, first).ok_or(TiffError::UnreadableDirectory)?;
    let pages = count_directories(&file, first, &mut w);

    build_scan(&file, &fields, pages, w)
}

impl TiffScan<'_> {
    /// Turns a walked directory into pixels.
    ///
    /// # Errors
    /// [`TiffError::ExceedsOutputLimit`] when the raster would not fit the
    /// caller's ceiling. Damaged *data* never errors here: the segments have
    /// been found, so what remains is degradation (ruling 2).
    pub fn decode(&self, limits: &Limits) -> Result<TiffImage, TiffError> {
        let mut w = Warnings::default();
        for &warning in &self.warnings {
            w.push(warning);
        }

        let colour = self.colour();
        let out_depth: u8 = if self.bits_per_sample == 16 && self.photometric.is_direct() {
            16
        } else {
            8
        };
        let unit = usize::from(out_depth / 8);
        let bytes = u64::from(self.width)
            .saturating_mul(u64::from(self.height))
            .saturating_mul(u64::from(colour.components()))
            .saturating_mul(unit as u64);
        if bytes > limits.max_output as u64 {
            return Err(TiffError::ExceedsOutputLimit {
                bytes,
                limit: limits.max_output,
            });
        }
        // Below `limits.max_output`, itself a `usize`, so this cannot truncate.
        let mut raster = Raster {
            width: self.width as usize,
            height: self.height as usize,
            components: colour.components() as usize,
            unit,
            data: vec![0u8; bytes as usize],
        };

        let mut complete = true;
        for (index, segment) in self.segments.iter().enumerate() {
            let Some(place) = self.placement(index) else {
                // More segments than the geometry needs. Not damage: TIFF 6.0
                // lets a writer pad the arrays, and a strip nothing addresses
                // is simply never read.
                continue;
            };
            let ok = self.segment_into(&mut raster, segment, &place, limits, &mut w);
            complete &= ok;
        }

        Ok(TiffImage {
            width: self.width,
            height: self.height,
            colour,
            bits_per_component: out_depth,
            data: raster.data,
            complete,
            warnings: w.into_vec(),
        })
    }

    /// The channel layout the directory implies.
    #[must_use]
    pub fn colour(&self) -> TiffColour {
        let alpha = self.alpha_sample().is_some();
        match (self.photometric, alpha) {
            (TiffPhotometric::WhiteIsZero | TiffPhotometric::BlackIsZero, false) => {
                TiffColour::Grey
            }
            (TiffPhotometric::WhiteIsZero | TiffPhotometric::BlackIsZero, true) => {
                TiffColour::GreyAlpha
            }
            (_, false) => TiffColour::Rgb,
            (_, true) => TiffColour::Rgba,
        }
    }

    /// Which sample index carries alpha, and whether it is premultiplied.
    ///
    /// TIFF 6.0 p.31: `ExtraSamples` 1 is "associated alpha", which is
    /// premultiplied into the colour, and 2 is unassociated. A 0
    /// ("unspecified") is treated as unassociated, which is what the tag is
    /// for when a writer had an extra channel and no opinion about it.
    fn alpha_sample(&self) -> Option<(usize, bool)> {
        let colour_channels = self.photometric.colour_channels() as usize;
        let extras = usize::from(self.samples_per_pixel).saturating_sub(colour_channels);
        if extras == 0 {
            return None;
        }
        let kind = self.extra_samples.first().copied().unwrap_or(0);
        Some((colour_channels, kind == 1))
    }

    /// Whether every LZW strip is packed the way §13 describes.
    ///
    /// The pass-through's question, and it has to be asked before a strip is
    /// decompressed: a `/LZWDecode` stream is MSB-first with `/EarlyChange 1`,
    /// so a pre-1993 strip placed unchanged would decode to noise in every
    /// reader including this one. `false` sends the file to the decoder, which
    /// transcodes it.
    #[must_use]
    pub fn lzw_bit_order_is_standard(&self) -> bool {
        self.compression != TiffCompression::Lzw || !self.segments.iter().any(|s| old_style_lzw(s))
    }

    /// Segments the geometry addresses, which is not always how many the
    /// arrays hold: TIFF 6.0 lets a writer pad them, and the pass-through's
    /// "one strip" question is about the geometry rather than the array.
    #[must_use]
    pub fn segments_needed(&self) -> u64 {
        let planes = match self.planar {
            TiffPlanar::Chunky => 1u64,
            TiffPlanar::Planar => u64::from(self.samples_per_pixel).max(1),
        };
        match self.layout {
            TiffLayout::Strips { rows_per_strip } => {
                u64::from(self.height).div_ceil(u64::from(rows_per_strip.max(1)))
            }
            TiffLayout::Tiles { width, height } => {
                u64::from(self.width).div_ceil(u64::from(width.max(1)))
                    * u64::from(self.height).div_ceil(u64::from(height.max(1)))
            }
        }
        .saturating_mul(planes)
    }

    /// Samples per segment: every sample of a pixel for a chunky image, one
    /// for a planar one (TIFF 6.0 p.38).
    fn segment_samples(&self) -> usize {
        match self.planar {
            TiffPlanar::Chunky => usize::from(self.samples_per_pixel),
            TiffPlanar::Planar => 1,
        }
    }

    /// Where segment `index` lands, or `None` when the geometry does not
    /// address it.
    fn placement(&self, index: usize) -> Option<Placement> {
        let planes = match self.planar {
            TiffPlanar::Chunky => 1usize,
            TiffPlanar::Planar => usize::from(self.samples_per_pixel).max(1),
        };
        match self.layout {
            TiffLayout::Strips { rows_per_strip } => {
                let per_strip = rows_per_strip.max(1) as usize;
                let down = (self.height as usize).div_ceil(per_strip);
                if down == 0 {
                    return None;
                }
                let plane = index / down;
                if plane >= planes {
                    return None;
                }
                let strip = index % down;
                let y = strip.checked_mul(per_strip)?;
                Some(Placement {
                    x: 0,
                    y,
                    width: self.width as usize,
                    // p.39: the last strip holds whatever is left over.
                    height: per_strip.min((self.height as usize).saturating_sub(y)),
                    stored_width: self.width as usize,
                    plane,
                })
            }
            TiffLayout::Tiles { width, height } => {
                let (tw, th) = (width.max(1) as usize, height.max(1) as usize);
                let across = (self.width as usize).div_ceil(tw);
                let down = (self.height as usize).div_ceil(th);
                let per_plane = across.checked_mul(down)?;
                if per_plane == 0 {
                    return None;
                }
                let plane = index / per_plane;
                if plane >= planes {
                    return None;
                }
                let within = index % per_plane;
                let x = (within % across).checked_mul(tw)?;
                let y = (within / across).checked_mul(th)?;
                Some(Placement {
                    x,
                    y,
                    width: tw.min((self.width as usize).saturating_sub(x)),
                    height: th.min((self.height as usize).saturating_sub(y)),
                    // p.67: an edge tile is stored full size and padded.
                    stored_width: tw,
                    plane,
                })
            }
        }
    }

    /// Bytes one stored row of a segment occupies, rounded up (p.39: "each row
    /// begins on a byte boundary").
    fn segment_row_bytes(&self, place: &Placement) -> usize {
        let bits = place
            .stored_width
            .saturating_mul(self.segment_samples())
            .saturating_mul(usize::from(self.bits_per_sample));
        bits.div_ceil(8)
    }

    /// Decompresses one segment and writes its pixels into the raster.
    /// Returns whether nothing was degraded.
    fn segment_into(
        &self,
        raster: &mut Raster,
        coded: &[u8],
        place: &Placement,
        limits: &Limits,
        w: &mut Warnings,
    ) -> bool {
        let row_bytes = self.segment_row_bytes(place);
        // The stored height, which for a tile is the whole tile even where the
        // image ends part way down it.
        let stored_rows = match self.layout {
            TiffLayout::Strips { .. } => place.height,
            TiffLayout::Tiles { height, .. } => height.max(1) as usize,
        };
        let expected = row_bytes.saturating_mul(stored_rows);

        // A segment cannot be asked to produce more than the raster it lands
        // in, and never more than the caller's own ceiling.
        let segment_limits = Limits::new(expected.min(limits.max_output).max(1));

        let reversed;
        let coded = if self.fill_order == 2 && self.compression.is_bit_oriented() {
            reversed = coded.iter().map(|b| b.reverse_bits()).collect::<Vec<u8>>();
            &reversed
        } else {
            coded
        };

        // A strip the directory pointed outside the file, or one it declared
        // zero bytes long. The rows it covers stay at zero and this says so:
        // several codings pad a short image out to its declared height, so
        // without this the raster would come back the right *size* and be
        // reported complete.
        if coded.is_empty() && expected > 0 {
            w.push(Warning::TiffSegmentUndecodable);
            return false;
        }

        let (mut packed, mut ok, samples_here) = match self.compression {
            TiffCompression::Jpeg => match self.jpeg_segment(coded, expected, w) {
                Some((data, samples)) => (data, true, samples),
                None => return false,
            },
            _ => (
                self.byte_segment(coded, place, expected, &segment_limits, w),
                true,
                self.segment_samples(),
            ),
        };
        if packed.len() < expected {
            ok = false;
        }

        // TIFF 6.0 p.64: `Predictor` 2 differences horizontally, across the
        // samples of a row, in the file's own sample order. Sixteen-bit
        // samples are swapped to big-endian first, which makes the shared
        // `/Predictor 2` implementation — big-endian, because PDF is — the
        // right arithmetic for both byte orders rather than only for `MM`.
        if self.bits_per_sample == 16 && self.little_endian {
            for pair in packed.chunks_exact_mut(2) {
                pair.swap(0, 1);
            }
        }
        if self.predictor == 2 && self.compression != TiffCompression::Jpeg {
            let params = PredictorParams {
                predictor: 2,
                colors: samples_here as u32,
                bits_per_component: u32::from(self.bits_per_sample),
                columns: place.stored_width as u32,
            };
            match predictors::unpredict(&packed, &params, &Limits::new(packed.len().max(1)), w) {
                Ok((out, row_ok)) => {
                    packed = out;
                    ok &= row_ok;
                }
                // `Geometry::new` refuses only parameters `build_scan` has
                // already rejected, so this arm is unreachable from a scanned
                // file; it is degradation rather than a panic either way.
                Err(_) => return false,
            }
        }

        self.place_samples(raster, &packed, place, row_bytes, samples_here);
        ok
    }

    /// Everything but `Compression` 7, which does not produce packed samples.
    fn byte_segment(
        &self,
        coded: &[u8],
        place: &Placement,
        expected: usize,
        limits: &Limits,
        w: &mut Warnings,
    ) -> Vec<u8> {
        match self.compression {
            TiffCompression::None => {
                let n = coded.len().min(expected);
                coded.get(..n).unwrap_or(coded).to_vec()
            }
            TiffCompression::PackBits => packbits::packbits_bytes(coded, expected, limits, w).0,
            TiffCompression::Deflate => inflate::flate_bytes(coded, limits, w).0,
            TiffCompression::Lzw => {
                if old_style_lzw(coded) {
                    w.push(Warning::TiffOldStyleLzw);
                    let repacked = transcode_old_style_lzw(coded);
                    lzw::lzw_bytes(&repacked, limits, true, w).0
                } else {
                    lzw::lzw_bytes(coded, limits, true, w).0
                }
            }
            TiffCompression::ModifiedHuffman
            | TiffCompression::CcittG3
            | TiffCompression::CcittG4 => {
                let ccitt = self.ccitt.unwrap_or(TiffCcitt {
                    k: 0,
                    byte_align: false,
                });
                // A tile's rows are its own width, not the image's, and a
                // strip's are the image's. `stored_width` is already that
                // distinction.
                let stride = place.stored_width.div_ceil(8).max(1);
                let params = CcittParams {
                    k: ccitt.k,
                    columns: place.stored_width as u32,
                    rows: (expected / stride) as u32,
                    // TIFF's decompressed convention is 1 = black (§10 pairs
                    // the coding with `PhotometricInterpretation` 0, where the
                    // *sample* 0 is white). `photometric` is applied once, at
                    // the end, for every compression alike — so the fax path
                    // hands over TIFF's own sense and nothing here has to know
                    // which end of the range the file meant.
                    black_is_1: true,
                    byte_align: ccitt.byte_align,
                    end_of_line: false,
                    // The strip's own row count is the authority: TIFF cuts a
                    // fax into strips and an EOFB, where there is one at all,
                    // is at the end of the last one.
                    end_of_block: false,
                };
                let (data, warnings) = ccitt::decode(coded, &params, limits.max_output);
                for warning in warnings {
                    w.push(warning);
                }
                data
            }
            // Returned above.
            TiffCompression::Jpeg => Vec::new(),
        }
    }

    /// `Compression` 7: the strip is an abbreviated JPEG datastream and
    /// `JPEGTables` holds the tables it names.
    ///
    /// Returns the interleaved eight-bit samples and how many components they
    /// carry, which is the JPEG's own count rather than `SamplesPerPixel` —
    /// they agree in every file worth decoding and a disagreement is the
    /// JPEG's to win, since it is the thing that produced the bytes.
    fn jpeg_segment(
        &self,
        coded: &[u8],
        expected: usize,
        w: &mut Warnings,
    ) -> Option<(Vec<u8>, usize)> {
        let spliced;
        let stream = match self.jpeg_tables {
            // TIFF Technical Note 2: the tables are `SOI tables EOI` and the
            // strip is `SOI frame EOI`, so one complete datastream is the
            // first without its EOI followed by the second without its SOI.
            Some(tables)
                if tables.len() > 4
                    && tables.starts_with(&[0xFF, 0xD8])
                    && coded.starts_with(&[0xFF, 0xD8]) =>
            {
                let head = tables.get(..tables.len() - 2).unwrap_or(tables);
                let tail = coded.get(2..).unwrap_or(coded);
                spliced = [head, tail].concat();
                &spliced[..]
            }
            _ => coded,
        };
        match jpeg::decode(stream, expected.max(1).saturating_mul(4)) {
            Ok(image) => {
                let components = match image.color {
                    jpeg::JpegColor::Gray => 1usize,
                    jpeg::JpegColor::Rgb => 3,
                    jpeg::JpegColor::Cmyk | jpeg::JpegColor::CmykInverted => 4,
                };
                for warning in image.warnings {
                    w.push(warning);
                }
                Some((image.data, components))
            }
            Err(_) => {
                w.push(Warning::TiffSegmentUndecodable);
                None
            }
        }
    }

    /// Reads one segment's samples out of its packed rows and into the raster.
    fn place_samples(
        &self,
        raster: &mut Raster,
        packed: &[u8],
        place: &Placement,
        row_bytes: usize,
        samples_here: usize,
    ) {
        let depth = self.bits_per_sample;
        let colour_channels = self.photometric.colour_channels() as usize;
        let alpha = self.alpha_sample();
        let out_components = raster.components;

        for row in 0..place.height {
            let Some(src) = packed.get(row * row_bytes..) else {
                break;
            };
            let src = src.get(..row_bytes).unwrap_or(src);
            let y = place.y + row;
            if y >= raster.height {
                break;
            }
            for col in 0..place.width {
                let x = place.x + col;
                if x >= raster.width {
                    break;
                }
                for sample in 0..samples_here {
                    let Some(raw) = sample_at(src, col * samples_here + sample, depth) else {
                        continue;
                    };
                    let channel = match self.planar {
                        TiffPlanar::Chunky => sample,
                        TiffPlanar::Planar => place.plane,
                    };
                    if let Some((index, _)) = alpha {
                        if channel == index {
                            let a = self.normalise(raw, raster.unit, false);
                            raster.put(x, y, out_components - 1, a);
                            continue;
                        }
                    }
                    if channel >= colour_channels {
                        // A second or third extra sample. TIFF 6.0 p.31 allows
                        // any number; PDF has one `/SMask`, so the rest are
                        // read past rather than kept.
                        continue;
                    }
                    if self.photometric == TiffPhotometric::Palette {
                        let base = (raw as usize).saturating_mul(3);
                        for c in 0..3usize {
                            let v = self.color_map.get(base + c).copied().unwrap_or(0);
                            raster.put(x, y, c, u32::from(v));
                        }
                    } else {
                        let v = self.normalise(
                            raw,
                            raster.unit,
                            self.photometric == TiffPhotometric::WhiteIsZero,
                        );
                        raster.put(x, y, channel, v);
                    }
                }
            }
        }

        // p.31: associated alpha is premultiplied into the colour, and PDF's
        // `/SMask` is not. Undone in integer arithmetic, which is exact on
        // every target (ruling 4).
        if raster.components > 1 && matches!(alpha, Some((_, true))) {
            raster.unpremultiply();
        }
    }

    /// Scales a raw sample to the output depth, and inverts it for
    /// `WhiteIsZero`.
    ///
    /// 1, 2 and 4 bits are scaled by multiplication, which is exactly the
    /// division 255/1, 255/3 and 255/15 reduce to — 255, 85 and 17.
    fn normalise(&self, raw: u32, unit: usize, invert: bool) -> u32 {
        let max = if unit == 2 { 0xFFFFu32 } else { 0xFFu32 };
        let scaled = match (self.bits_per_sample, unit) {
            (16, 2) | (8, 1) => raw,
            (16, 1) => raw >> 8,
            (8, 2) => raw * 257,
            (4, _) => raw * 17 * if unit == 2 { 257 } else { 1 },
            (2, _) => raw * 85 * if unit == 2 { 257 } else { 1 },
            // 1 bit.
            (_, _) => raw * max,
        };
        let scaled = scaled.min(max);
        if invert {
            max - scaled
        } else {
            scaled
        }
    }
}

impl TiffPhotometric {
    fn from_code(code: u16) -> Option<Self> {
        Some(match code {
            0 => Self::WhiteIsZero,
            1 => Self::BlackIsZero,
            2 => Self::Rgb,
            3 => Self::Palette,
            6 => Self::YCbCr,
            _ => return None,
        })
    }

    /// Colour channels before any `ExtraSamples`.
    #[must_use]
    pub const fn colour_channels(self) -> u32 {
        match self {
            Self::WhiteIsZero | Self::BlackIsZero | Self::Palette => 1,
            Self::Rgb | Self::YCbCr => 3,
        }
    }

    /// Whether a sample *is* an intensity, rather than an index into a table
    /// of eight-bit entries.
    const fn is_direct(self) -> bool {
        !matches!(self, Self::Palette)
    }
}

// --- the raster ---------------------------------------------------------

/// Where one segment lands in the image.
struct Placement {
    x: usize,
    y: usize,
    /// Pixels of this segment that are inside the image.
    width: usize,
    /// Rows of this segment that are inside the image.
    height: usize,
    /// Pixels the segment's rows are *stored* at, which for an edge tile is
    /// wider than `width`.
    stored_width: usize,
    /// Which sample this segment carries, under `PlanarConfiguration` 2.
    plane: usize,
}

struct Raster {
    width: usize,
    height: usize,
    components: usize,
    /// Bytes per output component: 1 or 2.
    unit: usize,
    data: Vec<u8>,
}

impl Raster {
    fn put(&mut self, x: usize, y: usize, component: usize, value: u32) {
        if component >= self.components || x >= self.width || y >= self.height {
            return;
        }
        let at = ((y * self.width + x) * self.components + component) * self.unit;
        if self.unit == 2 {
            if let Some(s) = self.data.get_mut(at..at + 2) {
                s.copy_from_slice(&(value as u16).to_be_bytes());
            }
        } else if let Some(b) = self.data.get_mut(at) {
            *b = value as u8;
        }
    }

    /// Divides the colour components of every pixel by its own alpha.
    fn unpremultiply(&mut self) {
        let colour = self.components - 1;
        let stride = self.components * self.unit;
        let max = if self.unit == 2 { 0xFFFFu32 } else { 0xFFu32 };
        for pixel in self.data.chunks_exact_mut(stride) {
            let a = read_component(pixel, colour, self.unit);
            if a == 0 || a >= max {
                continue;
            }
            for c in 0..colour {
                let v = read_component(pixel, c, self.unit);
                // Rounded, and clamped: a premultiplied sample above its own
                // alpha is out of range and the file wrote it that way.
                let out = (v.saturating_mul(max) + a / 2) / a;
                write_component(pixel, c, self.unit, out.min(max));
            }
        }
    }
}

fn read_component(pixel: &[u8], index: usize, unit: usize) -> u32 {
    let at = index * unit;
    if unit == 2 {
        match (pixel.get(at), pixel.get(at + 1)) {
            (Some(&h), Some(&l)) => u32::from(u16::from_be_bytes([h, l])),
            _ => 0,
        }
    } else {
        u32::from(pixel.get(at).copied().unwrap_or(0))
    }
}

fn write_component(pixel: &mut [u8], index: usize, unit: usize, value: u32) {
    let at = index * unit;
    if unit == 2 {
        if let Some(s) = pixel.get_mut(at..at + 2) {
            s.copy_from_slice(&(value as u16).to_be_bytes());
        }
    } else if let Some(b) = pixel.get_mut(at) {
        *b = value as u8;
    }
}

/// One sample out of a packed row, most significant bit first.
fn sample_at(row: &[u8], index: usize, bits: u16) -> Option<u32> {
    match bits {
        8 => row.get(index).map(|&b| u32::from(b)),
        16 => {
            let at = index.checked_mul(2)?;
            match (row.get(at), row.get(at + 1)) {
                (Some(&h), Some(&l)) => Some(u32::from(u16::from_be_bytes([h, l]))),
                _ => None,
            }
        }
        // 1, 2 and 4. p.29: "the bits are packed into bytes, high-order bit
        // first", and a row is padded out to a byte.
        b @ (1 | 2 | 4) => {
            let bit = index.checked_mul(usize::from(b))?;
            let byte = row.get(bit / 8)?;
            let shift = 8 - (bit % 8) - usize::from(b);
            let mask = (1u32 << b) - 1;
            Some((u32::from(*byte) >> shift) & mask)
        }
        _ => None,
    }
}

// --- reading the file ---------------------------------------------------

/// The file, plus which end of a number comes first.
struct Cursor<'a> {
    bytes: &'a [u8],
    little_endian: bool,
}

impl<'a> Cursor<'a> {
    fn u16_at(&self, at: usize) -> Option<u16> {
        let s = self.bytes.get(at..at.checked_add(2)?)?;
        let pair = [*s.first()?, *s.get(1)?];
        Some(if self.little_endian {
            u16::from_le_bytes(pair)
        } else {
            u16::from_be_bytes(pair)
        })
    }

    fn u32_at(&self, at: usize) -> Option<u32> {
        let s = self.bytes.get(at..at.checked_add(4)?)?;
        let quad = [*s.first()?, *s.get(1)?, *s.get(2)?, *s.get(3)?];
        Some(if self.little_endian {
            u32::from_le_bytes(quad)
        } else {
            u32::from_be_bytes(quad)
        })
    }

    fn slice(&self, at: usize, len: usize) -> Option<&'a [u8]> {
        self.bytes.get(at..at.checked_add(len)?)
    }
}

/// One directory entry, with its values already located.
#[derive(Clone, Copy)]
struct Field {
    tag: u16,
    kind: u16,
    count: u32,
    /// Where the values are: the entry's own bytes 8..12 when they fit there,
    /// and the offset those four bytes hold otherwise.
    at: usize,
}

/// TIFF 6.0 p.15's Table 2, by type code. `None` is a type this build has
/// never heard of, which p.16 says to skip.
const fn type_size(kind: u16) -> Option<usize> {
    Some(match kind {
        1 | 2 | 6 | 7 => 1,
        3 | 8 => 2,
        4 | 9 | 11 => 4,
        5 | 10 | 12 => 8,
        _ => return None,
    })
}

/// Reads one directory's entries. `None` means the directory is not there.
fn read_directory(file: &Cursor<'_>, at: usize) -> Option<Vec<Field>> {
    let count = usize::from(file.u16_at(at)?);
    // Every entry has to be inside the file for the directory to be one.
    let bytes = count.checked_mul(12)?;
    let first = at.checked_add(2)?;
    file.slice(first, bytes)?;

    let mut fields = Vec::with_capacity(count.min(4096));
    for index in 0..count {
        let entry = first + index * 12;
        let (Some(tag), Some(kind), Some(n)) = (
            file.u16_at(entry),
            file.u16_at(entry + 2),
            file.u32_at(entry + 4),
        ) else {
            break;
        };
        let Some(size) = type_size(kind) else {
            // p.16: "a field whose type is unknown should be skipped".
            continue;
        };
        let Some(span) = (n as usize).checked_mul(size) else {
            continue;
        };
        let values_at = if span <= 4 {
            // p.15: a value that fits is stored in the entry itself, and is
            // left-justified — it begins at the offset field's first byte
            // whatever the byte order is.
            entry + 8
        } else {
            match file.u32_at(entry + 8) {
                Some(offset) => offset as usize,
                None => continue,
            }
        };
        if file.slice(values_at, span).is_none() {
            // The values are not in the file. The field is dropped rather than
            // the directory refused: one unreadable tag is not an unreadable
            // image (ruling 2).
            continue;
        }
        fields.push(Field {
            tag,
            kind,
            count: n,
            at: values_at,
        });
    }
    Some(fields)
}

/// Follows the `NextIFD` chain, and is the reason this file has a cycle guard.
///
/// A directory whose next pointer is its own offset is one twelve-byte edit
/// away from any real file, and the chain is walked before any of it is
/// trusted.
fn count_directories(file: &Cursor<'_>, first: usize, w: &mut Warnings) -> u32 {
    let mut seen: Vec<usize> = Vec::with_capacity(8);
    let mut at = first;
    let mut pages = 0u32;
    while (pages as usize) < MAX_TIFF_IFDS {
        if seen.contains(&at) {
            w.push(Warning::TiffDirectoryCycle);
            break;
        }
        seen.push(at);
        let Some(count) = file.u16_at(at) else {
            break;
        };
        pages = pages.saturating_add(1);
        let next_at = match usize::from(count)
            .checked_mul(12)
            .and_then(|n| at.checked_add(2 + n))
        {
            Some(v) => v,
            None => break,
        };
        match file.u32_at(next_at) {
            Some(0) | None => break,
            Some(next) => at = next as usize,
        }
    }
    if pages as usize >= MAX_TIFF_IFDS {
        w.push(Warning::TiffDirectoryCycle);
    }
    pages
}

/// The values of one field, as unsigned integers.
fn integers(file: &Cursor<'_>, field: &Field) -> Vec<u32> {
    let n = (field.count as usize).min(MAX_TIFF_TAG_VALUES);
    let mut out = Vec::with_capacity(n.min(4096));
    for index in 0..n {
        let value = match field.kind {
            1 | 2 | 6 | 7 => file.bytes.get(field.at + index).map(|&b| u32::from(b)),
            3 | 8 => file.u16_at(field.at + index * 2).map(u32::from),
            4 | 9 => file.u32_at(field.at + index * 4),
            _ => None,
        };
        match value {
            Some(v) => out.push(v),
            None => break,
        }
    }
    out
}

fn first_integer(fields: &[Field], file: &Cursor<'_>, tag: u16) -> Option<u32> {
    let field = fields.iter().find(|f| f.tag == tag)?;
    integers(file, field).first().copied()
}

fn rational(fields: &[Field], file: &Cursor<'_>, tag: u16) -> Option<(u32, u32)> {
    let field = fields.iter().find(|f| f.tag == tag)?;
    if field.kind != 5 && field.kind != 10 {
        return None;
    }
    Some((file.u32_at(field.at)?, file.u32_at(field.at + 4)?))
}

/// Turns a directory into a [`TiffScan`], refusing everything it cannot mean.
fn build_scan<'a>(
    file: &Cursor<'a>,
    fields: &[Field],
    pages: u32,
    mut w: Warnings,
) -> Result<TiffScan<'a>, TiffError> {
    let width = first_integer(fields, file, TAG_IMAGE_WIDTH)
        .ok_or(TiffError::MissingTag(TAG_IMAGE_WIDTH))?;
    let height = first_integer(fields, file, TAG_IMAGE_LENGTH)
        .ok_or(TiffError::MissingTag(TAG_IMAGE_LENGTH))?;
    if width == 0 || height == 0 {
        return Err(TiffError::BadDimensions { width, height });
    }

    let compression_code = first_integer(fields, file, TAG_COMPRESSION).unwrap_or(1);
    let compression = u16::try_from(compression_code)
        .ok()
        .and_then(TiffCompression::from_code)
        .ok_or(TiffError::UnsupportedCompression(
            compression_code.min(u32::from(u16::MAX)) as u16,
        ))?;

    // p.38: no default, "there is no default; the field is required".
    let photometric_code = first_integer(fields, file, TAG_PHOTOMETRIC)
        .ok_or(TiffError::MissingTag(TAG_PHOTOMETRIC))?;
    let photometric = u16::try_from(photometric_code)
        .ok()
        .and_then(TiffPhotometric::from_code)
        .ok_or(TiffError::UnsupportedPhotometric(
            photometric_code.min(u32::from(u16::MAX)) as u16,
        ))?;
    if photometric == TiffPhotometric::YCbCr && compression != TiffCompression::Jpeg {
        // §21's subsampled YCbCr needs `YCbCrSubSampling` and the
        // `ReferenceBlackWhite` transform, neither of which is decoded here.
        // Under compression 7 the JPEG has already done all of it.
        return Err(TiffError::UnsupportedPhotometric(6));
    }

    let samples_per_pixel =
        u16::try_from(first_integer(fields, file, TAG_SAMPLES_PER_PIXEL).unwrap_or(1)).unwrap_or(1);
    let colour_channels = photometric.colour_channels() as usize;
    let samples_per_pixel = if usize::from(samples_per_pixel) < colour_channels {
        // A file that says RGB and one sample cannot mean both. The photometric
        // is the stronger claim — it decides how many arrays `ColorMap` has and
        // what a strip is — so the count follows it (ruling 2).
        w.push(Warning::TiffSamplesPerPixelWrong);
        colour_channels as u16
    } else {
        samples_per_pixel
    };

    // p.29: "BitsPerSample ... the default is 1".
    let depths = fields
        .iter()
        .find(|f| f.tag == TAG_BITS_PER_SAMPLE)
        .map(|f| integers(file, f))
        .unwrap_or_default();
    let bits_per_sample = match depths.first().copied() {
        None => 1u32,
        Some(first) => {
            if depths
                .iter()
                .take(usize::from(samples_per_pixel))
                .any(|&d| d != first)
            {
                return Err(TiffError::UnequalBitDepths);
            }
            first
        }
    };
    let bits_per_sample = u16::try_from(bits_per_sample).unwrap_or(0);
    if !matches!(bits_per_sample, 1 | 2 | 4 | 8 | 16) {
        return Err(TiffError::UnsupportedBitDepth(bits_per_sample));
    }

    // p.80: 1 unsigned, 2 two's complement, 3 IEEE float, 4 undefined.
    if let Some(field) = fields.iter().find(|f| f.tag == TAG_SAMPLE_FORMAT) {
        for value in integers(file, field)
            .into_iter()
            .take(usize::from(samples_per_pixel))
        {
            if value != 1 && value != 4 {
                return Err(TiffError::UnsupportedSampleFormat(
                    value.min(u32::from(u16::MAX)) as u16,
                ));
            }
        }
    }

    let predictor =
        u16::try_from(first_integer(fields, file, TAG_PREDICTOR).unwrap_or(1)).unwrap_or(u16::MAX);
    if predictor != 1 && predictor != 2 {
        return Err(TiffError::UnsupportedPredictor(predictor));
    }

    let planar = match first_integer(fields, file, TAG_PLANAR_CONFIGURATION).unwrap_or(1) {
        1 => TiffPlanar::Chunky,
        2 => TiffPlanar::Planar,
        other => {
            return Err(TiffError::UnsupportedPlanarConfiguration(
                other.min(u32::from(u16::MAX)) as u16,
            ))
        }
    };

    let fill_order =
        u16::try_from(first_integer(fields, file, TAG_FILL_ORDER).unwrap_or(1)).unwrap_or(1);
    if fill_order == 2 && !compression.is_bit_oriented() {
        // p.32's bit order is about a *bit* stream. Reversing the bytes of an
        // LZW, JPEG or DEFLATE strip would destroy it, and no encoder has ever
        // meant that by this tag.
        w.push(Warning::TiffFillOrderIgnored);
    }

    let color_map = read_color_map(file, fields, photometric, bits_per_sample, &mut w)?;
    let extra_samples: Vec<u16> = fields
        .iter()
        .find(|f| f.tag == TAG_EXTRA_SAMPLES)
        .map(|f| {
            integers(file, f)
                .into_iter()
                .map(|v| u16::try_from(v).unwrap_or(0))
                .collect()
        })
        .unwrap_or_default();

    let resolution = rational(fields, file, TAG_X_RESOLUTION).map(|x| TiffResolution {
        x,
        y: rational(fields, file, TAG_Y_RESOLUTION).unwrap_or(x),
        unit: u16::try_from(first_integer(fields, file, TAG_RESOLUTION_UNIT).unwrap_or(2))
            .unwrap_or(2),
    });

    let ccitt = ccitt_options(fields, file, compression);
    let jpeg_tables = fields
        .iter()
        .find(|f| f.tag == TAG_JPEG_TABLES)
        .and_then(|f| file.slice(f.at, f.count as usize));

    let (layout, segments) = read_segments(file, fields, width, height, samples_per_pixel, planar)?;

    // The sample cap is charged on the widest raster the directory can produce
    // — a palette expands one sample into three — and before any of it exists.
    let widest = if photometric == TiffPhotometric::Palette {
        4u64
    } else {
        u64::from(colour_channels as u32).saturating_add(1)
    };
    let samples = u64::from(width)
        .saturating_mul(u64::from(height))
        .saturating_mul(widest);
    if samples > MAX_TIFF_SAMPLES {
        return Err(TiffError::TooManySamples {
            samples,
            max: MAX_TIFF_SAMPLES,
        });
    }

    if pages > 1 {
        w.push(Warning::TiffExtraPagesIgnored);
    }

    Ok(TiffScan {
        width,
        height,
        bits_per_sample,
        samples_per_pixel,
        compression,
        photometric,
        planar,
        predictor,
        fill_order,
        layout,
        color_map,
        extra_samples,
        resolution,
        ccitt,
        jpeg_tables,
        segments,
        little_endian: file.little_endian,
        pages,
        warnings: w.into_vec(),
    })
}

/// `T4Options` (292) and `T6Options` (293), turned into the two things the
/// decoder needs.
fn ccitt_options(
    fields: &[Field],
    file: &Cursor<'_>,
    compression: TiffCompression,
) -> Option<TiffCcitt> {
    match compression {
        // §10: one-dimensional coding with no EOLs, each row byte-aligned.
        TiffCompression::ModifiedHuffman => Some(TiffCcitt {
            k: 0,
            byte_align: true,
        }),
        TiffCompression::CcittG3 => {
            let options = first_integer(fields, file, TAG_T4_OPTIONS).unwrap_or(0);
            Some(TiffCcitt {
                // p.30: bit 0 set means two-dimensional coding is *allowed*,
                // which is T.4's mixed mode — every line announces itself, so
                // this is `/K` positive rather than `/K` negative.
                k: i32::from(options & 1 != 0) * 4,
                // Bit 2: "fill bits have been added as necessary before EOL
                // codes such that EOL always ends on a byte boundary".
                byte_align: options & 4 != 0,
            })
        }
        // `T6Options` (293) has exactly one defined bit — T.6's uncompressed
        // mode, which no encoder in circulation emits and which this decoder
        // would have to implement rather than configure. There is nothing to
        // read out of it, so it is not read.
        TiffCompression::CcittG4 => Some(TiffCcitt {
            k: -1,
            byte_align: false,
        }),
        _ => None,
    }
}

/// `ColorMap` (320), transposed into RGB triples and scaled to eight bits.
fn read_color_map(
    file: &Cursor<'_>,
    fields: &[Field],
    photometric: TiffPhotometric,
    bits_per_sample: u16,
    w: &mut Warnings,
) -> Result<Vec<u8>, TiffError> {
    if photometric != TiffPhotometric::Palette {
        return Ok(Vec::new());
    }
    let field = fields
        .iter()
        .find(|f| f.tag == TAG_COLOR_MAP)
        .ok_or(TiffError::MissingColorMap)?;
    let values = integers(file, field);
    // p.23: "3 * (2**BitsPerSample) values", all reds, then all greens, then
    // all blues.
    let entries = values.len() / 3;
    if entries == 0 {
        return Err(TiffError::MissingColorMap);
    }
    // The 8-bit-map defect: a map whose every value fits in a byte was written
    // by an encoder that forgot to scale, and reading it as 16-bit makes every
    // colour at most 1/257 of what it should be.
    let eight_bit = values.iter().all(|&v| v <= 255);
    if eight_bit {
        w.push(Warning::TiffColorMapIsEightBit);
    }
    let wanted = 1usize << bits_per_sample;
    let mut out = Vec::with_capacity(wanted * 3);
    for index in 0..wanted {
        for channel in 0..3usize {
            let v = values.get(channel * entries + index).copied().unwrap_or(0);
            out.push(if eight_bit {
                (v & 0xFF) as u8
            } else {
                // 65535 -> 255, rounded.
                ((v.min(0xFFFF) * 255 + 32767) / 65535) as u8
            });
        }
    }
    Ok(out)
}

/// `StripOffsets`/`StripByteCounts` or `TileOffsets`/`TileByteCounts`, checked
/// against each other and against the file.
fn read_segments<'a>(
    file: &Cursor<'a>,
    fields: &[Field],
    width: u32,
    height: u32,
    samples_per_pixel: u16,
    planar: TiffPlanar,
) -> Result<(TiffLayout, Vec<&'a [u8]>), TiffError> {
    let tiled = fields.iter().any(|f| f.tag == TAG_TILE_OFFSETS);
    let (layout, offsets_tag, counts_tag) = if tiled {
        let tw = first_integer(fields, file, TAG_TILE_WIDTH)
            .ok_or(TiffError::MissingTag(TAG_TILE_WIDTH))?;
        let th = first_integer(fields, file, TAG_TILE_LENGTH)
            .ok_or(TiffError::MissingTag(TAG_TILE_LENGTH))?;
        // p.67: "TileWidth and TileLength must be a multiple of 16".
        if tw == 0 || th == 0 || tw % 16 != 0 || th % 16 != 0 {
            return Err(TiffError::BadTileGeometry {
                width: tw,
                height: th,
            });
        }
        (
            TiffLayout::Tiles {
                width: tw,
                height: th,
            },
            TAG_TILE_OFFSETS,
            TAG_TILE_BYTE_COUNTS,
        )
    } else {
        // p.39: "the default is 2**32 - 1, which is effectively infinity" —
        // one strip holding the whole image.
        let rows = first_integer(fields, file, TAG_ROWS_PER_STRIP)
            .unwrap_or(u32::MAX)
            .max(1);
        (
            TiffLayout::Strips {
                rows_per_strip: rows.min(height),
            },
            TAG_STRIP_OFFSETS,
            TAG_STRIP_BYTE_COUNTS,
        )
    };

    let offsets_field = fields
        .iter()
        .find(|f| f.tag == offsets_tag)
        .ok_or(TiffError::NoImageData)?;
    let declared = u64::from(offsets_field.count);
    if declared > MAX_TIFF_SEGMENTS as u64 {
        return Err(TiffError::TooManySegments {
            segments: declared,
            max: MAX_TIFF_SEGMENTS,
        });
    }
    let offsets = integers(file, offsets_field);
    let counts = fields
        .iter()
        .find(|f| f.tag == counts_tag)
        .map(|f| integers(file, f))
        .unwrap_or_default();
    if offsets.is_empty() || counts.len() < offsets.len() {
        return Err(TiffError::InconsistentSegments);
    }

    // How many the geometry needs. A file with fewer is refused rather than
    // decoded into a half-blank page: the two arrays and the geometry are
    // three claims about one image, and this is the only place they meet.
    let planes = match planar {
        TiffPlanar::Chunky => 1usize,
        TiffPlanar::Planar => usize::from(samples_per_pixel).max(1),
    };
    let needed = match layout {
        TiffLayout::Strips { rows_per_strip } => (height as usize)
            .div_ceil(rows_per_strip.max(1) as usize)
            .saturating_mul(planes),
        TiffLayout::Tiles {
            width: tw,
            height: th,
        } => (width as usize)
            .div_ceil(tw as usize)
            .saturating_mul((height as usize).div_ceil(th as usize))
            .saturating_mul(planes),
    };
    if offsets.len() < needed {
        return Err(TiffError::InconsistentSegments);
    }

    let mut segments = Vec::with_capacity(offsets.len().min(4096));
    for (index, &offset) in offsets.iter().enumerate() {
        let len = counts.get(index).copied().unwrap_or(0) as usize;
        // A strip that is not inside the file is an empty one, not a refusal:
        // the rows it would have carried stay at zero and `complete` says so.
        segments.push(file.slice(offset as usize, len).unwrap_or(&[]));
    }
    Ok((layout, segments))
}

// --- the LZW that TIFF 6.0 does not describe ----------------------------

/// Whether a strip carries the pre-1993 bit order.
///
/// Two bytes decide it, and they are the only two that can: §13 requires a
/// stream to open with the Clear code 256, which in nine bits is `1 0000
/// 0000`. Packed most significant bit first that is `0x80` and then a zero
/// bit; packed least significant bit first it is `0x00` and then a set bit.
fn old_style_lzw(coded: &[u8]) -> bool {
    matches!((coded.first(), coded.get(1)), (Some(0), Some(b)) if b & 1 != 0)
}

/// Repacks least-significant-bit-first codes as most-significant-bit-first
/// ones, so `lzw.rs` decodes both and the dictionary exists once.
///
/// The two width rules are the whole of the difference and they are written
/// side by side below. `lzw.rs` grows its code width when
/// `next + early >= 1 << width`, so `early = 1` widens at 511 — TIFF 6.0
/// §13's own "switch at 511, 1023, 2047" — and `early = 0` widens at 512,
/// which is what the old encoders did. Reading with one rule and writing with
/// the other is a value-preserving repack: the new width is never narrower
/// than the old one, so no code is ever truncated.
fn transcode_old_style_lzw(coded: &[u8]) -> Vec<u8> {
    let mut out = MsbWriter::default();
    let total_bits = (coded.len() as u64).saturating_mul(8);
    let mut bitpos = 0u64;
    let mut next = 258u32;
    let mut seen_since_clear = false;

    loop {
        let read_width = width_for(next, 0);
        let Some(code) = read_lsb(coded, bitpos, total_bits, read_width) else {
            break;
        };
        bitpos += u64::from(read_width);
        out.push(code, width_for(next, 1));

        match code {
            256 => {
                next = 258;
                seen_since_clear = false;
            }
            257 => break,
            _ => {
                if seen_since_clear {
                    next = next.saturating_add(1).min(4096);
                } else {
                    seen_since_clear = true;
                }
            }
        }
    }
    out.finish()
}

/// The code width `lzw.rs` would be using with this table size and this
/// `/EarlyChange`.
const fn width_for(next: u32, early: u32) -> u32 {
    let mut width = 9u32;
    while width < 12 && next + early >= 1u32 << width {
        width += 1;
    }
    width
}

/// A code read least significant bit first, GIF's packing and the old TIFF
/// encoders'.
fn read_lsb(input: &[u8], bitpos: u64, total_bits: u64, width: u32) -> Option<u16> {
    if bitpos + u64::from(width) > total_bits {
        return None;
    }
    let byte = (bitpos / 8) as usize;
    let off = (bitpos % 8) as u32;
    let b0 = u32::from(*input.get(byte)?);
    let b1 = u32::from(input.get(byte + 1).copied().unwrap_or(0));
    let b2 = u32::from(input.get(byte + 2).copied().unwrap_or(0));
    // width <= 12 and off <= 7, so a code never spans more than three bytes.
    let chunk = b0 | (b1 << 8) | (b2 << 16);
    Some(((chunk >> off) & ((1u32 << width) - 1)) as u16)
}

/// A code written most significant bit first, which is what `lzw.rs` reads.
#[derive(Default)]
struct MsbWriter {
    out: Vec<u8>,
    acc: u32,
    bits: u32,
}

impl MsbWriter {
    fn push(&mut self, code: u16, width: u32) {
        self.acc = (self.acc << width) | u32::from(code);
        self.bits += width;
        while self.bits >= 8 {
            self.bits -= 8;
            self.out.push((self.acc >> self.bits) as u8);
        }
    }

    fn finish(mut self) -> Vec<u8> {
        if self.bits > 0 {
            self.out.push((self.acc << (8 - self.bits)) as u8);
        }
        self.out
    }
}

#[cfg(test)]
mod tests;
