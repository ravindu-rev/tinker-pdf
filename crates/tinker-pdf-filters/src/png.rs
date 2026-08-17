//! PNG (ISO/IEC 15948) — a *container* decoder, and deliberately the fallback
//! rather than the default path.
//!
//! No PDF stream is ever a PNG file, so this is not a `/Filter` and does not
//! appear in [`crate::Filter`]. It is here because gap 29 needs one for CBZ,
//! and because everything it needs already lives in this crate: `inflate.rs`
//! for the zlib-wrapped IDAT, `predictors.rs` for the row filters, `crc32.rs`
//! for the per-chunk checksum.
//!
//! # This is the fallback, and that is a decision rather than an accident
//!
//! Gap 29's design section is emphatic about it. A non-interlaced PNG's IDAT
//! **is** a PDF `/FlateDecode` stream with `/Predictor 15`, byte for byte,
//! including the per-row filter tag and including the left-neighbour offset
//! being `ceil(colors x bpc / 8)` floored at one — because PDF's `/Predictor`
//! is PNG's specification, adopted wholesale. So the common case does not come
//! here at all: the compressed bytes are copied into a PDF image XObject
//! untouched, which is what keeps a 200-page archive's peak memory a small
//! multiple of the archive rather than *w x h x 3* per page — about 3.6 GB at
//! 2000 x 3000.
//!
//! What does come here is the set of PNGs that cannot pass through: Adam7
//! interlace, which `/Predictor` describes one raster and cannot express;
//! colour types 4 and 6, whose alpha is interleaved into the same scanlines
//! where PDF wants a separate `/SMask`; and a `tRNS` giving *partial* alpha to
//! palette entries, which PDF's colour-key `/Mask` is a range test rather than
//! a lookup for. [`png_scan`] exists for the pass-through side of that split:
//! it walks the chunks, checks every CRC and hands back the concatenated IDAT
//! **without inflating it**, which is what an embedder needs and what a decoder
//! must not be asked to do first.
//!
//! # What is reused, and why none of it is reimplemented here
//!
//! - **The IDAT is zlib-wrapped** (10.3), unlike a ZIP entry, which is raw
//!   DEFLATE by definition. So it goes through the sniffing door —
//!   [`crate::flate_decode`]'s byte layer — whose primary path is zlib with the
//!   Adler-32 verified, which is the check PNG's own text assumes a decoder
//!   performs. A [`Warning::RawDeflateFallback`] here therefore means
//!   something: the zlib header was unreadable and the bytes were tried raw
//!   anyway.
//! - **The row filters are `predictors.rs`.** `unfilter` implements PNG 9.2's
//!   five types with `paeth` under a doc comment already citing "PNG 9.4", and
//!   it is driven through [`crate::predictor_decode`] with `/Predictor 15`,
//!   `/Colors` the channel count, `/BitsPerComponent` the bit depth and
//!   `/Columns` the width. For an interlaced image it is called **once per
//!   pass**, with that pass's own width — which is precisely what 7.2 asks for
//!   and needs no new code at all.
//! - **The chunk CRC is `crc32.rs`'s resumable [`Crc32`]**, which was built for
//!   this caller: 5.3's checksum covers a chunk's *type* and its *data* and not
//!   its length, and those are two slices that are never adjacent in one
//!   buffer.
//!
//! # The output shape
//!
//! Ruling 8 keeps this crate PDF-free, so what comes out is samples and values:
//! an interleaved raster in one of four channel layouts at 8 or 16 bits per
//! component. Bit depths 1, 2 and 4 are expanded to 8 — by multiplication,
//! which 13.12's sample-depth scaling reduces to exactly, since 255/1, 255/3
//! and 255/15 are 255, 85 and 17 — and a palette is *applied* rather than
//! handed back, because the bounds check that makes a palette safe belongs
//! beside the palette. `tRNS` is applied too, in all three of its forms, so a
//! caller building an `/SMask` reads an alpha channel rather than re-deriving
//! one.
//!
//! # The caps that are not here
//!
//! [`MAX_PNG_SAMPLES`] is the only ceiling this decoder owns, and `limits.rs`
//! in `tinker-pdf-zip` is the precedent for writing down which ones were
//! considered and refused: a constant that can never fire is gap 18a milestone
//! 8's failure reached from the other direction.
//!
//! - **No cap on chunk count.** A chunk is at least twelve bytes and the walk
//!   allocates nothing per chunk, so the input length already bounds both the
//!   loop and the work. A constant would be decoration.
//! - **No cap on total IDAT bytes.** They are copied out of the input, so the
//!   input bounds them. What is unbounded is what they *inflate to*, and that
//!   is capped at the raster's own declared size rather than by a second
//!   constant that would have to be kept in step with the first.
//! - **No cap on palette size.** 11.2.3 caps it at `2^bit_depth` entries and
//!   this decoder enforces exactly that, so 256 entries is the format's own
//!   ceiling and not an invented one.
//! - **No cap on either dimension alone.** A 1 x 2^31 image is refused by
//!   [`MAX_PNG_SAMPLES`], because the sample count is the product; a separate
//!   dimension cap would refuse nothing the sample cap does not.

use crate::crc32::Crc32;
use crate::{inflate, predictor_decode, Limits, PredictorParams, Warning, Warnings};

// --- the budget ---------------------------------------------------------

/// Samples in the **output** raster — `width x height x components` — checked
/// with a saturating multiply before any buffer exists.
///
/// Charged at the *widest* layout the declared colour type can produce, since
/// that is what will be allocated and `tRNS` — which decides between three
/// components and four — is not known until later in the same file:
///
/// | Colour type | Charged components | Because |
/// | --- | --- | --- |
/// | 0 greyscale | 2 | a `tRNS` grey key adds an alpha channel |
/// | 2 truecolour | 4 | a `tRNS` colour key adds an alpha channel |
/// | 3 indexed | 4 | the palette expands to RGB, and `tRNS` to RGBA |
/// | 4 grey + alpha | 2 | already its widest |
/// | 6 truecolour + alpha | 4 | already its widest |
///
/// | | Samples |
/// | --- | --- |
/// | The most any fixture in this crate spends | 4 096 — a 32 x 32 RGBA |
/// | The most any PngSuite file spends | 6 400 — 40 x 40 x 4 |
/// | A comic page: 2000 x 3000 truecolour with alpha | 24 000 000 |
/// | **This cap** | **67 108 864** |
///
/// The margin over a real page is 2.8x, and the constant is the same `1 << 26`
/// as `jpx`'s `MAX_JPX_SAMPLES` for the same arithmetic: at 16 bits a component
/// it is 134 217 728 bytes, which is `tinker_pdf_cos::limits::MAX_DECODED_STREAM`
/// to the byte — the ceiling every other decoded object in this engine already
/// lives under.
///
/// **Reachable**, which is the property gap 18a milestone 8 found missing from
/// a constant written to avoid exactly this failure: IHDR carries width and
/// height as 32-bit fields, so **thirteen bytes** can ask for 2^63 samples, and
/// `an_image_past_the_sample_cap_is_refused_before_it_allocates` builds one.
pub const MAX_PNG_SAMPLES: u64 = 1 << 26;

/// 5.2: the eight bytes every PNG file begins with.
///
/// The middle six are not decoration. `\r\n` and `\x1a` are there so a file
/// mangled by a text-mode transfer stops matching, and `0x89` has its high bit
/// set so a transport that strips it does too — which is why PngSuite ships
/// four separate files corrupting one byte of this each.
pub const PNG_SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];

/// 5.3: a chunk's length "shall not exceed 2^31 - 1".
const MAX_CHUNK_LEN: u32 = 0x7FFF_FFFF;

/// 11.2.2: width and height are four-byte integers from 1 to 2^31 - 1.
const MAX_DIMENSION: u32 = 0x7FFF_FFFF;

// --- the refusals -------------------------------------------------------

/// Why an image was refused outright.
///
/// Refusals rather than warnings, because gap 17's finding was that **the
/// refusal is the feature**: a decoder that answered a malformed IHDR with a
/// plausible raster would produce a page indistinguishable from a correct one.
/// Every variant names what was wrong in terms a caller can show a human —
/// "colour type 1 does not exist" is a different sentence from "this is not a
/// PNG", and gap 29's page-level handling shows different things for them.
///
/// Damage that costs *pixels* rather than meaning is a [`Warning`] instead and
/// leaves a partial image (ruling 2): a truncated IDAT, a dropped ancillary
/// chunk, a palette index past the end of PLTE.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PngError {
    /// The first eight bytes are not [`PNG_SIGNATURE`].
    NotPng,
    /// The first chunk was not a readable thirteen-byte IHDR. 5.6 requires
    /// IHDR first, and there is nothing to interpret a later chunk against.
    MissingHeader,
    /// A **critical** chunk's CRC did not match its contents. 5.4 makes the
    /// case of a type's first letter say whether an image can be shown without
    /// the chunk, and for these it cannot.
    ChunkCrc(ChunkType),
    /// A chunk whose type is critical (5.4: an uppercase first letter) and
    /// which this build does not know. The specification's own rule: a decoder
    /// that cannot interpret a critical chunk must not present the image as
    /// though it had.
    UnknownCriticalChunk(ChunkType),
    /// Table 11.1 does not pair this colour type with this bit depth — which
    /// includes every colour type outside {0, 2, 3, 4, 6}. Both numbers are
    /// carried, because a caller reporting to a human wants to say which half
    /// was wrong.
    BadColourTypeDepth { colour_type: u8, bit_depth: u8 },
    /// A width or height of zero, or past 2^31 - 1 (11.2.2).
    BadDimensions { width: u32, height: u32 },
    /// IHDR's compression method byte was not 0. Only method 0 — the zlib
    /// deflate of 10.3 — has ever been defined.
    UnsupportedCompression(u8),
    /// IHDR's filter method byte was not 0. Only method 0 — the five adaptive
    /// filters of 9.2 — has ever been defined.
    UnsupportedFilterMethod(u8),
    /// IHDR's interlace byte was neither 0 (none) nor 1 (Adam7).
    UnsupportedInterlace(u8),
    /// Colour type 3 with no usable PLTE. 11.2.3 makes the palette mandatory
    /// there and there is nothing to fall back on: the samples are indices into
    /// a table that does not exist.
    MissingPalette,
    /// A PLTE whose length was not a multiple of three.
    PaletteLength(usize),
    /// A PLTE with more entries than the bit depth can index (11.2.3: the entry
    /// count "shall not exceed the range that can be represented in the image
    /// bit depth"). A 1-bit indexed image may name two colours.
    PaletteTooLarge { entries: usize, max: usize },
    /// Not one IDAT chunk. There is no image, as distinct from a damaged one.
    MissingImageData,
    /// [`MAX_PNG_SAMPLES`] would be spent. Refused before any buffer exists.
    TooManySamples { samples: u64, max: u64 },
    /// The raster would be larger than the caller's own [`Limits::max_output`].
    /// Separate from [`PngError::TooManySamples`] because one number is this
    /// crate's and the other is the caller's, and a caller that lowered its own
    /// wants to see its own back.
    ExceedsOutputLimit { bytes: u64, limit: usize },
}

impl core::fmt::Display for PngError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::NotPng => f.write_str("not a PNG: signature mismatch"),
            Self::MissingHeader => f.write_str("no readable IHDR chunk"),
            Self::ChunkCrc(t) => write!(f, "CRC mismatch on critical chunk {t}"),
            Self::UnknownCriticalChunk(t) => write!(f, "unknown critical chunk {t}"),
            Self::BadColourTypeDepth {
                colour_type,
                bit_depth,
            } => write!(
                f,
                "colour type {colour_type} with bit depth {bit_depth} is not in Table 11.1"
            ),
            Self::BadDimensions { width, height } => {
                write!(f, "image dimensions {width} x {height}")
            }
            Self::UnsupportedCompression(m) => write!(f, "compression method {m}"),
            Self::UnsupportedFilterMethod(m) => write!(f, "filter method {m}"),
            Self::UnsupportedInterlace(m) => write!(f, "interlace method {m}"),
            Self::MissingPalette => f.write_str("colour type 3 with no palette"),
            Self::PaletteLength(n) => write!(f, "palette of {n} bytes is not a multiple of three"),
            Self::PaletteTooLarge { entries, max } => {
                write!(f, "palette of {entries} entries, bit depth allows {max}")
            }
            Self::MissingImageData => f.write_str("no IDAT chunk"),
            Self::TooManySamples { samples, max } => {
                write!(f, "{samples} samples, ceiling is {max}")
            }
            Self::ExceedsOutputLimit { bytes, limit } => {
                write!(f, "{bytes} bytes of raster, caller's ceiling is {limit}")
            }
        }
    }
}

impl std::error::Error for PngError {}

/// A chunk's four-byte type, kept verbatim so a refusal can name it.
///
/// Four bytes rather than an enum, because the types that reach a refusal
/// include ones this build has never heard of — and "unknown critical chunk"
/// with no name is exactly the shape of message gap 17 was written against.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ChunkType(pub [u8; 4]);

impl ChunkType {
    /// 5.4: the ancillary bit is bit 5 of the first byte, so a lowercase first
    /// letter means an image can still be shown without the chunk.
    #[must_use]
    pub const fn is_critical(self) -> bool {
        self.0[0] & 0x20 == 0
    }
}

impl core::fmt::Display for ChunkType {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        for b in self.0 {
            if b.is_ascii_graphic() {
                write!(f, "{}", b as char)?;
            } else {
                write!(f, "\\x{b:02x}")?;
            }
        }
        Ok(())
    }
}

const IHDR: ChunkType = ChunkType(*b"IHDR");
const PLTE: ChunkType = ChunkType(*b"PLTE");
const IDAT: ChunkType = ChunkType(*b"IDAT");
const IEND: ChunkType = ChunkType(*b"IEND");
const TRNS: ChunkType = ChunkType(*b"tRNS");

// --- what the chunk walk found ------------------------------------------

/// IHDR (11.2.2), validated.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PngHeader {
    pub width: u32,
    pub height: u32,
    /// 1, 2, 4, 8 or 16, and only in a pairing Table 11.1 permits.
    pub bit_depth: u8,
    /// 0, 2, 3, 4 or 6.
    pub colour_type: u8,
    /// Interlace method 1, Adam7 (7.2). Method 0 is a single raster.
    pub interlaced: bool,
}

impl PngHeader {
    /// Samples per pixel in the *source* scanlines — Table 11.1's own column.
    ///
    /// Indexed is one: a colour type 3 scanline carries indices, and the three
    /// components a reader sees come from the palette rather than the data.
    #[must_use]
    pub const fn channels(&self) -> u32 {
        match self.colour_type {
            2 => 3,
            4 => 2,
            6 => 4,
            // 0 and 3. Nothing else can exist here: `parse_header` refuses it.
            _ => 1,
        }
    }

    /// Bytes in one unfiltered scanline of `width` pixels — 7.2's formula,
    /// which an interlace pass applies with its own reduced width.
    fn row_bytes(&self, width: u32) -> u64 {
        u64::from(width)
            .saturating_mul(u64::from(self.channels()))
            .saturating_mul(u64::from(self.bit_depth))
            .div_ceil(8)
    }
}

/// `tRNS` (11.3.2.1), in each of the three shapes the colour type selects.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PngTransparency {
    /// Colour type 0: one grey level, stored in two bytes but meaningful only
    /// over `0 ..= 2^bit_depth - 1` — so it is compared against the *raw*
    /// sample, before any depth scaling.
    Grey(u16),
    /// Colour type 2: one RGB triple, likewise at the image's own depth.
    Rgb(u16, u16, u16),
    /// Colour type 3: one alpha byte per palette entry, from entry zero.
    /// Entries past the end of this list are fully opaque.
    Palette(Vec<u8>),
}

/// The chunk walk's result: everything but the pixels.
///
/// Handed back whole so that the *pass-through* path — gap 29's default — can
/// have the header, the palette, the transparency and the compressed bytes
/// without any of them being inflated. [`PngScan::decode`] is the other half.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PngScan {
    pub header: PngHeader,
    /// PLTE, three bytes an entry, in file order. Empty when there was none.
    pub palette: Vec<u8>,
    pub transparency: Option<PngTransparency>,
    /// Every IDAT chunk's data concatenated in file order.
    ///
    /// One zlib stream spans them: 10.3 lets an encoder cut the compressed
    /// datastream at any byte, so a chunk boundary is not a stream boundary,
    /// and inflating chunks individually decodes the first and fails the rest.
    /// PngSuite ships `oi1n0g16`, `oi2n0g16`, `oi4n0g16` and `oi9n0g16` — the
    /// same image in one chunk, in two, in four unequal ones and in a run of
    /// length-one ones — precisely so a decoder can be caught doing that.
    pub idat: Vec<u8>,
    /// Typed leniency records (ruling 10), deduplicated.
    pub warnings: Vec<Warning>,
}

/// The channel layout of [`PngImage::data`].
///
/// Four rather than five: an indexed image has had its palette applied by the
/// time it is one of these, because the bounds check that makes a palette safe
/// belongs beside the palette rather than in every caller.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PngColour {
    Grey,
    GreyAlpha,
    Rgb,
    Rgba,
}

impl PngColour {
    #[must_use]
    pub const fn components(self) -> u32 {
        match self {
            Self::Grey => 1,
            Self::GreyAlpha => 2,
            Self::Rgb => 3,
            Self::Rgba => 4,
        }
    }
}

/// One decoded image.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PngImage {
    pub width: u32,
    pub height: u32,
    pub colour: PngColour,
    /// 8 or 16. Depths 1, 2 and 4 arrive here expanded to 8 (13.12); an indexed
    /// image is always 8, because a palette entry is.
    pub bits_per_component: u8,
    /// Interleaved, row-major, no row padding. 16-bit components are
    /// big-endian, which is PNG's byte order and not a choice made here.
    pub data: Vec<u8>,
    /// False when the raster came up short, the IDAT was damaged, or a chunk
    /// had to be dropped — the same contract as [`crate::Decoded::complete`].
    pub complete: bool,
    pub warnings: Vec<Warning>,
}

// --- Table 11.1 ---------------------------------------------------------

/// ISO/IEC 15948 Table 11.1, transcribed entry by entry.
///
/// Written as the table rather than as a predicate on purpose. "8 and 16
/// always, plus the small depths for colour types 0 and 3" is a plausible
/// reading that gets colour type 3 wrong in one place — it has no 16 — and a
/// predicate carrying that mistake reads exactly like a correct one.
const COLOUR_TYPE_DEPTHS: [(u8, &[u8]); 5] = [
    (0, &[1, 2, 4, 8, 16]),
    (2, &[8, 16]),
    (3, &[1, 2, 4, 8]),
    (4, &[8, 16]),
    (6, &[8, 16]),
];

/// Whether Table 11.1 pairs these two. Every other pair is refused by name.
#[must_use]
pub fn colour_type_depth_is_legal(colour_type: u8, bit_depth: u8) -> bool {
    COLOUR_TYPE_DEPTHS
        .iter()
        .any(|&(t, depths)| t == colour_type && depths.contains(&bit_depth))
}

/// The widest output the colour type can produce — see [`MAX_PNG_SAMPLES`].
const fn widest_components(colour_type: u8) -> u64 {
    match colour_type {
        0 | 4 => 2,
        _ => 4,
    }
}

// --- Adam7 --------------------------------------------------------------

/// Table 7.1's seven passes: **starting row, starting column, row increment,
/// column increment**.
///
/// Transcribed in the specification's own order — row first — and used that way
/// below, because the one mistake this table invites is a transposition, and a
/// transposition is a *bijection*: it permutes the pixels of a symmetric or
/// uniform test image into an arrangement no comparison can see. That is gap
/// 17's finding about two swapped JBIG2 context bits, one layer up, and it is
/// why `adam7_places_all_sixty_four_distinct_pixels` uses an image in which
/// every pixel differs from every other.
const ADAM7: [(u32, u32, u32, u32); 7] = [
    (0, 0, 8, 8),
    (0, 4, 8, 8),
    (4, 0, 8, 4),
    (0, 2, 4, 4),
    (2, 0, 4, 2),
    (0, 1, 2, 2),
    (1, 0, 2, 1),
];

/// One reduced image's extent, or `None` when the pass holds no pixels — which
/// is normal for a small image and means the pass contributes **no bytes at
/// all**, not a zero-height one.
fn pass_extent(pass: usize, width: u32, height: u32) -> Option<(u32, u32)> {
    let (ystart, xstart, ystep, xstep) = *ADAM7.get(pass)?;
    if width <= xstart || height <= ystart {
        return None;
    }
    Some((
        (width - xstart).div_ceil(xstep),
        (height - ystart).div_ceil(ystep),
    ))
}

// --- the chunk walk -----------------------------------------------------

/// Walks the signature and the chunk structure, checking every CRC, **without
/// inflating anything**.
///
/// The split matters: gap 29's pass-through embeds `scan.idat` into a PDF
/// stream verbatim, and asking a decoder for the header would mean inflating a
/// raster in order to throw it away — which is exactly the *w x h x 3* per page
/// the design exists to avoid.
///
/// # Errors
/// Any [`PngError`]. Damage that costs pixels rather than meaning warns instead
/// and leaves a partial scan (ruling 2).
pub fn png_scan(input: &[u8]) -> Result<PngScan, PngError> {
    if input.get(..8) != Some(&PNG_SIGNATURE[..]) {
        return Err(PngError::NotPng);
    }

    let mut w = Warnings::default();
    let mut header: Option<PngHeader> = None;
    let mut palette: Vec<u8> = Vec::new();
    let mut transparency: Option<PngTransparency> = None;
    let mut idat: Vec<u8> = Vec::new();
    let mut saw_idat = false;
    let mut saw_iend = false;
    let mut at = 8usize;

    while at < input.len() {
        let Some(chunk) = read_chunk(input, at) else {
            w.push(Warning::TruncatedInput);
            break;
        };
        at = chunk.next;

        // 5.3: the CRC covers the type and the data, and not the length.
        let mut c = Crc32::new();
        c.update(&chunk.kind.0);
        c.update(chunk.data);
        if c.finish() != chunk.declared_crc {
            if chunk.kind.is_critical() {
                return Err(PngError::ChunkCrc(chunk.kind));
            }
            w.push(Warning::PngChunkCrcMismatch);
            continue;
        }

        match chunk.kind {
            IHDR => {
                if header.is_some() {
                    w.push(Warning::PngChunkDropped);
                    continue;
                }
                header = Some(parse_header(chunk.data)?);
            }
            // 5.6 puts IHDR first. Anything before it leaves nothing to
            // interpret the rest against, and guessing is how a wrong picture
            // gets produced.
            _ if header.is_none() => return Err(PngError::MissingHeader),
            PLTE => {
                let h = header.ok_or(PngError::MissingHeader)?;
                if !palette.is_empty() || !palette_belongs(h.colour_type) {
                    w.push(Warning::PngChunkDropped);
                    continue;
                }
                if chunk.data.len() % 3 != 0 {
                    // Fatal for an indexed image, meaningless for the
                    // suggested-palette case, so the two answers differ.
                    if h.colour_type == 3 {
                        return Err(PngError::PaletteLength(chunk.data.len()));
                    }
                    w.push(Warning::PngChunkDropped);
                    continue;
                }
                let entries = chunk.data.len() / 3;
                if h.colour_type == 3 {
                    // 11.2.3: no more entries than the depth can index.
                    let max = 1usize << h.bit_depth;
                    if entries > max {
                        return Err(PngError::PaletteTooLarge { entries, max });
                    }
                }
                palette = chunk.data.to_vec();
            }
            TRNS => {
                let h = header.ok_or(PngError::MissingHeader)?;
                match read_transparency(h, chunk.data, &palette) {
                    Some(t) => transparency = Some(t),
                    None => w.push(Warning::PngChunkDropped),
                }
            }
            IDAT => {
                saw_idat = true;
                idat.extend_from_slice(chunk.data);
            }
            IEND => {
                saw_iend = true;
                if at < input.len() {
                    w.push(Warning::TrailingGarbage);
                }
                break;
            }
            other if other.is_critical() => return Err(PngError::UnknownCriticalChunk(other)),
            // Every other ancillary chunk: gAMA, sRGB, pHYs, tEXt, bKGD, sPLT,
            // hIST, tIME and the rest. Each is either advisory or about
            // presentation, so ignoring one is correct rather than lenient and
            // no warning is owed.
            _ => {}
        }
    }

    let header = header.ok_or(PngError::MissingHeader)?;
    if header.colour_type == 3 && palette.is_empty() {
        return Err(PngError::MissingPalette);
    }
    if !saw_idat {
        return Err(PngError::MissingImageData);
    }
    if !saw_iend {
        // IEND is PNG's end-of-data marker, so its absence is this crate's
        // existing `EarlyEod` rather than a second name for the same thing.
        w.push(Warning::EarlyEod);
    }

    Ok(PngScan {
        header,
        palette,
        transparency,
        idat,
        warnings: w.into_vec(),
    })
}

/// One chunk's fields, borrowed from the input.
struct Chunk<'a> {
    kind: ChunkType,
    data: &'a [u8],
    declared_crc: u32,
    /// Where the next chunk begins.
    next: usize,
}

/// 5.3's layout: length, type, data, CRC. `None` when the input cannot hold
/// one, which is a truncation rather than an error.
///
/// Every offset is a `checked_add`: `at` is bounded by the input but a declared
/// length is not, and `at + len` on a 32-bit target is one addition away from
/// wrapping into a slice that is inside the buffer (ruling 1).
fn read_chunk(input: &[u8], at: usize) -> Option<Chunk<'_>> {
    let head = input.get(at..at.checked_add(8)?)?;
    let len = u32::from_be_bytes([head[0], head[1], head[2], head[3]]);
    // A length above 5.3's ceiling cannot be walked past in any case: there is
    // no way to find the next chunk from it.
    if len > MAX_CHUNK_LEN {
        return None;
    }
    let kind = ChunkType([head[4], head[5], head[6], head[7]]);
    let body_at = at.checked_add(8)?;
    let crc_at = body_at.checked_add(len as usize)?;
    let data = input.get(body_at..crc_at)?;
    let next = crc_at.checked_add(4)?;
    let crc = input.get(crc_at..next)?;
    Some(Chunk {
        kind,
        data,
        declared_crc: u32::from_be_bytes([crc[0], crc[1], crc[2], crc[3]]),
        next,
    })
}

/// 11.2.2, and the only place a colour type or bit depth is ever accepted.
fn parse_header(data: &[u8]) -> Result<PngHeader, PngError> {
    let Some(d) = data.get(..13) else {
        return Err(PngError::MissingHeader);
    };
    let width = u32::from_be_bytes([d[0], d[1], d[2], d[3]]);
    let height = u32::from_be_bytes([d[4], d[5], d[6], d[7]]);
    let (bit_depth, colour_type, compression, filter, interlace) =
        (d[8], d[9], d[10], d[11], d[12]);

    if width == 0 || height == 0 || width > MAX_DIMENSION || height > MAX_DIMENSION {
        return Err(PngError::BadDimensions { width, height });
    }
    if !colour_type_depth_is_legal(colour_type, bit_depth) {
        return Err(PngError::BadColourTypeDepth {
            colour_type,
            bit_depth,
        });
    }
    if compression != 0 {
        return Err(PngError::UnsupportedCompression(compression));
    }
    if filter != 0 {
        return Err(PngError::UnsupportedFilterMethod(filter));
    }
    if interlace > 1 {
        return Err(PngError::UnsupportedInterlace(interlace));
    }

    // The sample ceiling, charged before a single buffer exists — which is the
    // whole point of charging it here rather than where the raster is built.
    let samples = u64::from(width)
        .saturating_mul(u64::from(height))
        .saturating_mul(widest_components(colour_type));
    if samples > MAX_PNG_SAMPLES {
        return Err(PngError::TooManySamples {
            samples,
            max: MAX_PNG_SAMPLES,
        });
    }

    Ok(PngHeader {
        width,
        height,
        bit_depth,
        colour_type,
        interlaced: interlace == 1,
    })
}

/// 11.2.3: PLTE is required for colour type 3, permitted as a *suggested*
/// palette for 2 and 6, and forbidden for 0 and 4.
const fn palette_belongs(colour_type: u8) -> bool {
    matches!(colour_type, 2 | 3 | 6)
}

/// 11.3.2.1, all three forms. `None` means the chunk does not belong to this
/// colour type, or is the wrong length or in the wrong place for it, and is
/// dropped.
fn read_transparency(h: PngHeader, data: &[u8], palette: &[u8]) -> Option<PngTransparency> {
    match h.colour_type {
        0 => {
            let d = data.get(..2)?;
            Some(PngTransparency::Grey(u16::from_be_bytes([d[0], d[1]])))
        }
        2 => {
            let d = data.get(..6)?;
            Some(PngTransparency::Rgb(
                u16::from_be_bytes([d[0], d[1]]),
                u16::from_be_bytes([d[2], d[3]]),
                u16::from_be_bytes([d[4], d[5]]),
            ))
        }
        3 => {
            // 5.6 puts tRNS after PLTE, and without the palette there is
            // nothing to index: an empty alpha list would silently make every
            // entry opaque, which reads as a successful decode of a file that
            // asked for transparency.
            let entries = palette.len() / 3;
            if entries == 0 {
                return None;
            }
            // "shall not contain more values than there are palette entries";
            // fewer is normal and the rest are opaque, more is a violation that
            // costs nothing to survive, so the tail is dropped.
            Some(PngTransparency::Palette(
                data.get(..entries.min(data.len()))?.to_vec(),
            ))
        }
        // 4 and 6 already carry an alpha channel; a tRNS there is meaningless
        // and 11.3.2.1 forbids it.
        _ => None,
    }
}

// --- decoding -----------------------------------------------------------

/// Signature, chunk walk, inflate, unfilter, de-interlace and expand.
///
/// # Errors
/// Any [`PngError`].
pub fn png_decode(input: &[u8], limits: &Limits) -> Result<PngImage, PngError> {
    png_scan(input)?.decode(limits)
}

/// The output raster, plus everything needed to address a pixel in it.
struct Raster<'a> {
    data: &'a mut [u8],
    /// Bytes per output row.
    stride: usize,
    colour: PngColour,
    /// 8 or 16.
    depth: u8,
    /// Bytes per output component: 1 or 2.
    bpc: usize,
}

impl PngScan {
    /// Turns a walked file into pixels.
    ///
    /// # Errors
    /// [`PngError::ExceedsOutputLimit`] when the raster would not fit the
    /// caller's ceiling. Damaged *data* never errors here: the IDAT has already
    /// been found, so what remains is degradation (ruling 2).
    pub fn decode(&self, limits: &Limits) -> Result<PngImage, PngError> {
        let h = self.header;
        let mut w = Warnings::default();
        for &warning in &self.warnings {
            w.push(warning);
        }

        let (colour, out_depth) = self.output_shape();
        let bpc = usize::from(out_depth / 8);
        let bytes = u64::from(h.width)
            .saturating_mul(u64::from(h.height))
            .saturating_mul(u64::from(colour.components()))
            .saturating_mul(out_depth as u64 / 8);
        if bytes > limits.max_output as u64 {
            return Err(PngError::ExceedsOutputLimit {
                bytes,
                limit: limits.max_output,
            });
        }
        // Below `limits.max_output`, itself a `usize`, so this cannot truncate.
        let raster_len = bytes as usize;

        // 7.2: one raster, or seven reduced ones. A pass's filtered length is
        // its own height times its own row width, plus one filter tag a row.
        let passes: Vec<(usize, u32, u32)> = if h.interlaced {
            (0..7)
                .filter_map(|p| pass_extent(p, h.width, h.height).map(|(pw, ph)| (p, pw, ph)))
                .collect()
        } else {
            vec![(usize::MAX, h.width, h.height)]
        };
        let filtered_len: u64 = passes
            .iter()
            .map(|&(_, pw, ph)| (h.row_bytes(pw) + 1).saturating_mul(u64::from(ph)))
            .fold(0u64, u64::saturating_add);

        // The inflate ceiling is the raster's own declared size and never more,
        // so a zlib bomb inside an IDAT stops at the row the image ends on
        // rather than at a second constant that would have to be kept in step
        // with this one. `filtered_len` is bounded by `MAX_PNG_SAMPLES`, because
        // the sample count bounds both dimensions.
        //
        // Because the ceiling *is* `filtered_len`, an over-long IDAT is reported
        // by [`Warning::OutputCapHit`] from the inflater and there is nothing
        // left here to check. The first version of this line compared
        // `filtered.len()` against `filtered_len` and pushed a
        // `TrailingGarbage` — a condition that cannot arise, since the inflater
        // is not allowed to produce it, so the branch was dead. Milestone 3's
        // injection matrix found it by disabling it and watching nothing fail,
        // which is gap 18a milestone 8's lesson arriving as a warning rather
        // than as a cap.
        let cap = Limits::new(usize::try_from(filtered_len).unwrap_or(usize::MAX));
        let (filtered, complete_inflate) = inflate::flate_bytes(&self.idat, &cap, &mut w);

        let mut data = vec![0u8; raster_len];
        let mut raster = Raster {
            data: &mut data,
            stride: (h.width as usize).saturating_mul(colour.components() as usize * bpc),
            colour,
            depth: out_depth,
            bpc,
        };
        let mut at = 0usize;
        let mut complete = complete_inflate;
        for &(pass, pw, ph) in &passes {
            let row_bytes = h.row_bytes(pw);
            let want = usize::try_from((row_bytes + 1).saturating_mul(u64::from(ph)))
                .unwrap_or(usize::MAX);
            let end = at.saturating_add(want).min(filtered.len());
            let Some(slice) = filtered.get(at..end).filter(|s| !s.is_empty()) else {
                complete = false;
                continue;
            };
            at = end;

            // `predictors.rs` does the unfiltering: PNG 9.2 and PDF Table 10
            // are one specification, which is why there is nothing to write
            // here. One call per pass, with the pass's own width as /Columns.
            let params = PredictorParams {
                predictor: 15,
                colors: h.channels(),
                bits_per_component: u32::from(h.bit_depth),
                columns: pw,
            };
            let row_cap = Limits::new(
                usize::try_from(row_bytes.saturating_mul(u64::from(ph))).unwrap_or(usize::MAX),
            );
            let Ok(rows) = predictor_decode(slice, &params, &row_cap) else {
                // Unreachable for a header this crate accepted: /Colors and
                // /Columns are both at least one and /BitsPerComponent is one
                // of Table 11.1's five. Degrading on a surprise is still
                // cheaper than an `unwrap` (ruling 1).
                complete = false;
                continue;
            };
            for warning in rows.warnings {
                w.push(warning);
            }
            complete &= rows.complete;
            self.place(pass, pw, ph, &rows.data, &mut raster, &mut w);
        }

        Ok(PngImage {
            width: h.width,
            height: h.height,
            colour,
            bits_per_component: out_depth,
            data,
            complete,
            warnings: w.into_vec(),
        })
    }

    /// The channel layout and component width the source produces.
    ///
    /// An indexed image is always eight bits out, whatever its indices were,
    /// because a palette entry is; everything else keeps 16 and widens 1, 2 and
    /// 4 to 8.
    fn output_shape(&self) -> (PngColour, u8) {
        let keyed = self.transparency.is_some();
        let depth = if self.header.colour_type != 3 && self.header.bit_depth == 16 {
            16
        } else {
            8
        };
        let colour = match self.header.colour_type {
            0 if keyed => PngColour::GreyAlpha,
            0 => PngColour::Grey,
            4 => PngColour::GreyAlpha,
            6 => PngColour::Rgba,
            // 2 and 3, whose palette has been applied by the time this matters.
            _ if keyed => PngColour::Rgba,
            _ => PngColour::Rgb,
        };
        (colour, depth)
    }

    /// Expands one pass's unfiltered rows into the output raster.
    ///
    /// `pass` is [`usize::MAX`] for a non-interlaced image, which
    /// [`ADAM7`]`.get` turns into the identity mapping `(0, 0, 1, 1)` — so the
    /// ordinary case is 7.2's placement with a step of one rather than a second
    /// code path an interlace bug could hide behind.
    fn place(
        &self,
        pass: usize,
        pass_width: u32,
        pass_height: u32,
        rows: &[u8],
        raster: &mut Raster<'_>,
        w: &mut Warnings,
    ) {
        let h = self.header;
        let (ystart, xstart, ystep, xstep) = ADAM7.get(pass).copied().unwrap_or((0, 0, 1, 1));
        let channels = h.channels() as usize;
        let row_bytes = usize::try_from(h.row_bytes(pass_width)).unwrap_or(usize::MAX);
        let pixel_stride = raster.colour.components() as usize * raster.bpc;

        for y in 0..pass_height {
            let off = (y as usize).saturating_mul(row_bytes);
            let end = off.saturating_add(row_bytes).min(rows.len());
            if off >= end {
                break;
            }
            let row = &rows[off..end];
            // A short final row keeps the pixels it does hold, and **only whole
            // ones**: the divisor is the bits in a pixel, not the bits in a
            // sample. A row that stops between the red and the green of an RGB
            // pixel has no green to place, and writing the red beside two
            // zeroes would put a saturated colour into a damaged image — a
            // wrong pixel where a missing one is the truth. The warning for the
            // short row came from `predictors.rs` already.
            let have = (row.len() * 8) / (channels * h.bit_depth as usize);
            let dst_y = (ystart as usize).saturating_add((y as usize) * ystep as usize);
            let wide = u32::try_from(have).unwrap_or(u32::MAX).min(pass_width);
            for x in 0..wide {
                let dst_x = (xstart as usize).saturating_add((x as usize) * xstep as usize);
                let base = dst_y
                    .saturating_mul(raster.stride)
                    .saturating_add(dst_x.saturating_mul(pixel_stride));
                self.pixel(row, (x as usize) * channels, base, raster, w);
            }
        }
    }

    /// One pixel: read the source samples, apply the palette and `tRNS`, scale
    /// to the output depth, and write.
    fn pixel(
        &self,
        row: &[u8],
        sample_at: usize,
        base: usize,
        raster: &mut Raster<'_>,
        w: &mut Warnings,
    ) {
        let depth = self.header.bit_depth;
        let wide = raster.depth == 16;
        let opaque: u16 = if wide { 0xFFFF } else { 0xFF };
        let widen = |v: u16| if wide { v } else { scale8(v, depth) };
        let mut out = [0u16; 4];

        match self.header.colour_type {
            0 => {
                let g = sample(row, sample_at, depth);
                out[0] = widen(g);
                if let Some(PngTransparency::Grey(key)) = self.transparency {
                    out[1] = if g == key { 0 } else { opaque };
                }
            }
            2 => {
                let r = sample(row, sample_at, depth);
                let g = sample(row, sample_at + 1, depth);
                let b = sample(row, sample_at + 2, depth);
                out[0] = widen(r);
                out[1] = widen(g);
                out[2] = widen(b);
                if let Some(PngTransparency::Rgb(kr, kg, kb)) = self.transparency {
                    out[3] = if (r, g, b) == (kr, kg, kb) { 0 } else { opaque };
                }
            }
            3 => {
                let index = sample(row, sample_at, depth) as usize;
                match self.palette.get(index * 3..index * 3 + 3) {
                    Some(rgb) => {
                        out[0] = u16::from(rgb[0]);
                        out[1] = u16::from(rgb[1]);
                        out[2] = u16::from(rgb[2]);
                    }
                    // 11.2.3 makes this an error; ruling 2 makes it a black
                    // pixel and a warning, because refusing the image would
                    // throw away every pixel that was fine.
                    None => w.push(Warning::PngPaletteIndexOutOfRange),
                }
                if let Some(PngTransparency::Palette(alpha)) = &self.transparency {
                    out[3] = u16::from(alpha.get(index).copied().unwrap_or(0xFF));
                }
            }
            4 => {
                out[0] = widen(sample(row, sample_at, depth));
                out[1] = widen(sample(row, sample_at + 1, depth));
            }
            // 6, and nothing else exists.
            _ => {
                for (k, slot) in out.iter_mut().enumerate() {
                    *slot = widen(sample(row, sample_at + k, depth));
                }
            }
        }

        let components = raster.colour.components() as usize;
        for (k, &value) in out.iter().enumerate().take(components) {
            if wide {
                let at = base + k * 2;
                if let Some(slot) = raster.data.get_mut(at..at + 2) {
                    slot.copy_from_slice(&value.to_be_bytes());
                }
            } else if let Some(slot) = raster.data.get_mut(base + k) {
                *slot = value as u8;
            }
        }
    }
}

/// The `index`-th sample of an unfiltered scanline, at the image's own depth.
///
/// Sub-byte samples are packed most significant first and never straddle a
/// byte, because 1, 2 and 4 all divide 8 (7.2). A read past the end yields zero
/// rather than panicking (ruling 1); a short row is already a warning by the
/// time one happens.
fn sample(row: &[u8], index: usize, depth: u8) -> u16 {
    match depth {
        16 => {
            let i = index * 2;
            match row.get(i..i + 2) {
                Some(b) => u16::from_be_bytes([b[0], b[1]]),
                None => 0,
            }
        }
        8 => u16::from(row.get(index).copied().unwrap_or(0)),
        d => {
            let per = 8 / d as usize;
            let byte = row.get(index / per).copied().unwrap_or(0);
            let shift = 8 - d as usize * (index % per + 1);
            u16::from((byte >> shift) & ((1u8 << d) - 1))
        }
    }
}

/// 13.12's sample-depth scaling, which for the depths that reach it is exactly
/// a multiplication: `v * (255 / (2^d - 1))` is `v * 255`, `v * 85`, `v * 17`
/// and `v * 1` for depths 1, 2, 4 and 8. Bit replication by another name, and
/// the reason a 1-bit white pixel comes out 255 rather than 1.
fn scale8(v: u16, depth: u8) -> u16 {
    let factor = match depth {
        1 => 255,
        2 => 85,
        4 => 17,
        _ => 1,
    };
    (v & 0xFF).saturating_mul(factor).min(0xFF)
}

#[cfg(test)]
mod tests;
