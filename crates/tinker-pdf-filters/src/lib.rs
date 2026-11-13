//! Hand-rolled PDF stream filters. Bytes in, bytes out; no PDF types.
//!
//! Feature documentation: `docs/features/filters.md`.
//!
//! Wave 1 is the byte-filter set: Flate (RFC 1950/1951), LZW (7.4.4),
//! ASCIIHex (7.4.2), ASCII85 (7.4.3), RunLength (7.4.5) and the predictors of
//! Table 10. The image codecs (DCTDecode, CCITTFaxDecode) land in wave 2 and
//! are terminal in a filter chain until then: [`apply_chain`] hands their
//! still-encoded bytes back to the caller.
//!
//! The output contract, stated once: **corrupt input truncates and warns; it
//! never errors.** [`Decoded::complete`] is false whenever the input ended
//! early, was damaged, or hit [`Limits::max_output`], and every tolerated
//! condition leaves a typed [`Warning`]. [`FilterError`] is reserved for the
//! caller holding the API wrong ([`FilterError::BadParams`]) and for deferred
//! capabilities ([`FilterError::Unsupported`]) — decisions, not data.
//!
//! Three entry points here are not stream filters at all, and are exported for
//! the *container* formats gap 29 begins — a CBZ is a ZIP of images, and no
//! PDF stream is either. [`inflate_raw`] is RFC 1951 with no wrapper and none
//! looked for, which is what a caller that knows what it holds needs instead
//! of [`flate_decode`]'s sniff; [`crc32`] is the checksum ZIP puts on an entry
//! and PNG puts on a chunk; [`png_decode`] is the PNG decoder gap 29 needs for
//! the images its pass-through cannot carry. All three live here because this
//! is where inflate, the PNG row filters and the checksum already are, and none
//! of them changes anything a `/Filter` name reaches. [`xxh64`] is a hash
//! two crates above this one share, the Zstandard frame checksum and the
//! bucket deduplication sorts streams into, and no filter reads it.
//!
//! [`png_encode`] is the fourth and the only one that *writes*. It is here for
//! the same reason and more sharply: a PNG file is a zlib stream inside chunks
//! carrying a CRC-32, with 9.2's row filters in front of it, and all three of
//! those already live in this crate and in no other. `tinker_pdf::Bitmap::to_png`
//! is a projection over it — it maps a `PixelFormat` onto one of PNG's colour
//! types and calls this — because ruling 11 makes the facade the public surface
//! for a *document* and a rendered page is what a caller has.
//!
//! [`bmp_decode`] and [`gif_decode`] are container decoders of the tier-4
//! archive row's, the first with no coding a `/Filter` shares. Each hands back
//! [`ImagePixels`] — indexed or direct, eight bits — which is the one output
//! shape the archive row's decoders share, so the embedder learns it once.

#![forbid(unsafe_code)]

mod ascii;
mod bmp;
mod brotli;
mod ccitt;
mod crc32;
pub mod deflate;
mod gif;
mod inflate;
mod jbig2;
mod jpeg;
mod jpx;
mod jxr;
mod lzw;
pub mod mq;
mod packbits;
mod png;
mod predictors;
mod qm;
mod raster;
mod runlength;
mod tiff;
mod webp;
mod xxhash;

use core::fmt;

pub use bmp::{bmp_decode, bmp_decode_counted, BmpError, BmpImage, MAX_BMP_SAMPLES};
pub use brotli::{brotli_decode, brotli_decode_counted, BrotliError};
pub use ccitt::{
    decode as ccitt_decode, decode_counted as ccitt_decode_counted, g4_encode as ccitt_g4_encode,
    CcittEncodeError, CcittParams, CcittSource, T6Rows,
};
pub use crc32::{crc32, Crc32};
pub use deflate::{
    deflate, deflate_at, zlib_compress, zlib_compress_at, DeflateLevel, Deflater, ZlibDeflater,
};
pub use gif::{gif_decode, gif_decode_counted, GifError, GifImage, MAX_GIF_SAMPLES};
pub use inflate::{inflate_raw, RawInflated};
pub use jbig2::{
    decode as jbig2_decode, decode_attributed as jbig2_decode_attributed,
    decode_measured as jbig2_decode_measured, generic_encode as jbig2_generic_encode,
    generic_region_segment as jbig2_generic_region_segment, Jbig2EncodeError, Jbig2GenericSource,
    Jbig2Params, Jbig2Refusal, Jbig2SymbolExtent, JBIG2_ELEMENT_COST, MAX_JBIG2_SYMBOLS,
    MAX_JBIG2_SYMBOL_PAGE_MULTIPLE, MAX_JBIG2_SYMBOL_PIXELS, MAX_JBIG2_TABLE_LINES,
    MAX_JBIG2_TEXT_INSTANCES, MAX_JBIG2_WORK_MULTIPLE,
};
pub use jpeg::{
    decode as jpeg_decode, decode_strips as jpeg_decode_strips, jpeg_encode, JpegColor,
    JpegEncodeError, JpegError, JpegFrame, JpegHeld, JpegImage, JpegLayout, JpegOptions,
    JpegQuantisation, JpegSampling, JpegScale, JpegSink, JpegSource, JpegSourceColour,
};
pub use jpx::{
    jpx_decode, jpx_decode_attributed, jpx_decode_request, jpx_header, JpxColour, JpxCounts,
    JpxHeader, JpxImage, JpxRequest, JpxWindow, Refusal as JpxRefusal,
};
pub use jxr::{
    jxr_decode, JxrChannels, JxrError, JxrImage, JxrPixelFormat, JxrRefusal, JxrWarning,
};
pub use mq::{encoder::MqEncoder, MqContext, MqContexts, MqDecoder};
pub use png::{
    colour_type_depth_is_legal, png_decode, png_decode_counted, png_encode, png_encode_counted,
    png_scan, ChunkType, PngColour, PngEncodeError, PngError, PngHeader, PngHeld, PngImage,
    PngScan, PngSource, PngTransparency, MAX_PNG_SAMPLES, PNG_SIGNATURE,
};
pub use predictors::PredictorParams;
pub use raster::ImagePixels;
pub use tiff::{
    tiff_decode, tiff_decode_counted, tiff_scan, tiff_scan_directory, TiffCcitt, TiffColour,
    TiffCompression, TiffError, TiffImage, TiffLayout, TiffPhotometric, TiffPlanar, TiffResolution,
    TiffSampleFormat, TiffSampleRange, TiffScan, MAX_TIFF_SAMPLES,
};
pub use webp::{webp_decode, WebpError, WebpImage, MAX_WEBP_SAMPLES};
pub use xxhash::xxh64;

/// Resource ceilings. Mandatory: a 1 KB flate stream can legally expand to
/// gigabytes, and a lenient decoder without a ceiling is a denial-of-service
/// primitive.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    /// Hard ceiling on produced bytes. Hitting it truncates and warns.
    pub max_output: usize,
}

impl Limits {
    #[must_use]
    pub const fn new(max_output: usize) -> Self {
        Self { max_output }
    }
}

/// The result of one decode: what came out, whether it came out whole, and
/// what had to be tolerated to get it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Decoded {
    pub data: Vec<u8>,
    /// false: input ended early, was damaged, or hit `max_output`.
    pub complete: bool,
    /// Typed leniency records (ruling 10). This crate reports *what* it
    /// tolerated; the caller attaches *which object* it happened in.
    pub warnings: Vec<Warning>,
}

/// The two conditions that are decisions rather than data.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FilterError {
    /// Parameters that cannot describe any stream (zero `/Columns`, a
    /// `/BitsPerComponent` outside 1/2/4/8/16, an unknown `/Predictor`).
    BadParams(&'static str),
    /// A codec deferred behind a capability gate (ruling 3).
    Unsupported(Capability),
}

impl fmt::Display for FilterError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BadParams(m) => write!(f, "bad filter parameters: {m}"),
            Self::Unsupported(c) => write!(f, "unsupported codec: {c:?}"),
        }
    }
}

impl std::error::Error for FilterError {}

/// Codecs that exist behind a capability gate rather than in the crate.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Capability {
    Jbig2,
    Jpx,
}

/// Every leniency this crate performs, as data rather than a log line.
///
/// The set is closed and each variant is recorded at most once per decode:
/// a stream of a million bad bytes yields one `NonHexByte`, not a million.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Warning {
    /// The zlib header was absent or invalid; the same bytes were decoded as
    /// raw deflate (RFC 1951 with no wrapper).
    RawDeflateFallback,
    /// FDICT was set. Preset dictionaries do not occur in PDFs; the
    /// dictionary id was skipped and decoding attempted anyway.
    FdictIgnored,
    /// The zlib Adler-32 did not match the decoded bytes.
    AdlerMismatch,
    /// Input ended in the middle of a code, block or run.
    TruncatedInput,
    /// The stream ended before its EOD marker (`>`, `~>`, `128`, code 257 or
    /// a final deflate block) appeared.
    EarlyEod,
    /// Bytes remained after the stream's end; they were ignored.
    ///
    /// White space after a `/Filter`'s end of data is not this. 7.3.8.1 puts
    /// an end-of-line marker between a stream's data and `endstream` and
    /// keeps it out of `/Length`, and a producer that counts it in hands the
    /// filter that marker as the last bytes of its input, where it holds no
    /// data. A container's coded data — a PNG's image data, a TIFF strip —
    /// ends where its data does, and any byte after it is this.
    TrailingGarbage,
    /// `Limits::max_output` was reached; the output is truncated there.
    OutputCapHit,
    /// A deflate block was structurally invalid (over-subscribed Huffman
    /// table, reserved block type, distance past the window); decoding
    /// stopped at that point.
    CorruptDeflateStream,
    /// ASCIIHex: an odd number of digits; a trailing `0` was implied.
    OddHexDigit,
    /// ASCIIHex: a byte that is neither a hex digit nor white space was
    /// skipped.
    NonHexByte,
    /// ASCII85: a byte outside `!`..=`u` (`y` included — 7.4.3 has no `y`
    /// shortcut) was skipped.
    BadAscii85Byte,
    /// ASCII85: `z` appeared inside a group rather than at its start; it was
    /// skipped.
    Ascii85ZInGroup,
    /// ASCII85: a group decoded above 2^32-1; it was clamped.
    Ascii85Overflow,
    /// LZW: a code outside the current table; decoding stopped there.
    BadLzwCode,
    /// LZW: the table filled without a Clear code; it was frozen and
    /// decoding continued.
    LzwTableFull,
    /// Predictor: the last row was short; the bytes present were unfiltered.
    PredictorRowShort,
    /// Predictor: a PNG row tag above 4; the row was taken unfiltered.
    BadPredictorTag,
    /// A chain entry followed an image codec; the tail was ignored.
    ChainTailIgnored,
    /// CCITT: `/EndOfLine true` says every line is preceded by an EOL and a
    /// line was not; it was decoded from the cursor anyway.
    MissingEndOfLine,
    /// JBIG2: a segment this build does not decode was skipped, and anything
    /// it would have drawn is missing from the page.
    ///
    /// Overwhelmingly the symbol-dictionary and text-region lineage (T.88
    /// 6.4, 6.5), which is what OCR pipelines emit. A page that ends with no
    /// region on it is refused outright — this warning is what says *why*,
    /// since the refusal itself is an error and carries no detail.
    Jbig2SegmentSkipped,
    /// JBIG2: a region or page declared more pixels than the output ceiling
    /// allows, so it was refused rather than allocated.
    Jbig2RegionTooLarge,
    /// JBIG2: the decode as a whole spent `MAX_JBIG2_WORK_MULTIPLE` times
    /// what its caller's ceiling holds, and stopped at the segment that ran
    /// the budget out. The page keeps what was drawn before it.
    ///
    /// Not [`Warning::Jbig2RegionTooLarge`], which it was reported as until
    /// October 2026: nothing here was too large. Every item was inside every
    /// bound and there were too many of them, which is
    /// [`Warning::JpxBudgetSpent`]'s shape and wants its own name for the same
    /// reason.
    Jbig2BudgetSpent,
    /// JBIG2: a segment this build decodes, refused for a reference that does
    /// not resolve — a missing or wrong-shaped retained context, a custom
    /// table the segment does not refer to, a text region whose dictionary is
    /// absent or refused — or for a reserved table selector or a refinement
    /// with no refinement tables.
    ///
    /// Distinct from [`Warning::Jbig2SegmentSkipped`] on purpose. That one
    /// means a segment this build does not decode; this one means a file that
    /// contradicts itself inside one it does
    /// (`docs/design/jbig2-symbol-text.md`, *Refusals*). Folding the two
    /// together would make a broken reference look like a capability nobody
    /// built.
    Jbig2VariantSkipped,
    /// JBIG2: a symbol dictionary asked for more symbols, or more pixels of
    /// them, than the bounds allow — or promised a count its own data did not
    /// keep to.
    Jbig2SymbolLimitHit,

    /// PackBits (TIFF 6.0 §9): the -128 tag, which §9 calls a no-op and 7.4.5
    /// calls an end marker. It was skipped, not obeyed.
    PackBitsNoOp,
    /// PackBits: a literal or replicate run reached past the byte count the
    /// caller expected; the excess was dropped.
    PackBitsRunOverruns,

    // ---- JPEG 2000 (T.800) -----------------------------------------------
    //
    // Ten, and each of them is one *class* of refusal rather than one
    // condition, because [`Warning`] is a closed set recorded at most once
    // per decode and a variant per marker would make it neither. What the
    // refusal was called in full — which marker, which constraint — lives in
    // `jpx::Refusal`, which the tests assert against.
    //
    // Nine of the ten are paired with `FilterError::Unsupported`, so the
    // caller draws the placeholder. That is not a partial success: a wrong
    // JPEG 2000 decode is a plausible photograph, and this codec has no
    // half-measure that is better than the honest grey rectangle. The tenth,
    // `JpxCoefficientClamped`, is the one leniency rather than a refusal.
    //
    // Every one of the ten is reachable, and `jpx::tests::refusals` asserts
    // it by decoding a codestream that produces each. Milestone 8 found three
    // that were not: `JpxStageNotBuilt` was removed with the last stage it
    // could name, and `JpxPacketLength` and `JpxSegmentationSymbol` were
    // being reported as `JpxStructureInvalid`, so the two integrity checks
    // the plan calls the cheapest real defence in the decoder were invisible
    // from outside the crate.
    /// JPX: a marker T.800 Table A.2 defines, in a place the clause that
    /// defines it does not put — SOP or EPH in a header rather than in the
    /// bit stream (A.8).
    ///
    /// **No Table A.2 marker is refused as a capability.** RGN, PPM, PPT and
    /// POC were all here and all left, the last on 21 September 2026, once
    /// each was implemented where its clause puts it: a skipped RGN draws a
    /// bright rectangle and a skipped POC mis-parses every packet after it,
    /// which is why none of them was ever stepped over instead.
    JpxMarkerUnsupported,
    /// JPX: a marker code Table A.2 does not define at all, which is where
    /// every ISO/IEC 15444-2 marker lands.
    JpxMarkerUnknown,
    /// JPX: a structural constraint was violated — A.5.1's tile grid, a
    /// marker segment length, marker ordering, a JP2 box past its parent.
    JpxStructureInvalid,
    /// JPX: a coding feature this build does not implement — a progression
    /// order, a Table A.19 code-block style bit, a quantisation style, an
    /// `Rsiz` capability, a `colr` method.
    JpxFeatureUnsupported,
    /// JPX: component precision above 16 bits, which is refused rather than
    /// truncated.
    JpxPrecisionUnsupported,
    /// JPX: the codestream ended inside a marker segment, a packet header or
    /// a code-block segment.
    JpxTruncated,
    /// JPX: **an integrity check, not a capability check.** A packet's
    /// declared length did not land where the next packet begins, so tier-2
    /// has gone wrong — and it is caught before any pixel exists, because a
    /// mis-parsed packet header almost never lands on a boundary by accident.
    JpxPacketLength,
    /// JPX: **an integrity check, not a capability check.** A cleanup pass
    /// ended with a segmentation symbol that was not `1010` (T.800 D.5), so
    /// the arithmetic decoder is out of step with the encoder.
    JpxSegmentationSymbol,
    /// JPX: one of `MAX_JPX_SAMPLES`, `MAX_JPX_CODE_BLOCKS` or
    /// `MAX_JPX_WORK` was spent, or the caller's own output ceiling was.
    JpxBudgetSpent,
    /// JPX: **a leniency, not a refusal.** A dequantised coefficient asked
    /// for a magnitude past T.800 E.1's nominal dynamic range plus one guard
    /// bit and was clamped to it before it reached a plane.
    ///
    /// The exponent and mantissa of a QCD marker are attacker-controlled, so
    /// without the clamp there is no bound on the fixed-point arithmetic that
    /// follows and the overflow proof behind the 9/7's `i64` products becomes
    /// an assumption (ruling 1). Nothing legitimate reaches it — an 8-bit
    /// image's coefficients live three bits below — so this says the file
    /// declared a step size its own samples cannot justify.
    JpxCoefficientClamped,

    // ---- JPEG ------------------------------------------------------------
    /// JPEG: the frame's sample precision was twelve bits (T.81 B.2.2) and the
    /// samples handed out are eight.
    ///
    /// The decode itself is at twelve; the narrowing is A.3.1's level shift
    /// followed by a four-bit shift right, which is where the information
    /// goes. Every `PixelFormat` this engine rasters into is eight bits deep,
    /// so a wider `JpegImage` would be a change to the raster rather than to
    /// the codec — `JpegImage::precision` records what it came from.
    JpegPrecisionNarrowed,

    // ---- TIFF 6.0 --------------------------------------------------------
    //
    // Seven, and every one of them is a *leniency* rather than a refusal:
    // anything a TIFF can get wrong that costs meaning rather than pixels is a
    // [`tiff::TiffError`], for the reason `png.rs` gives one line below.
    /// TIFF: the LZW strip was packed least significant bit first and widened
    /// its codes one code late — the pre-1993 encoders' form, which §13 does
    /// not describe. It was repacked and decoded.
    TiffOldStyleLzw,
    /// TIFF: a strip or tile would not decode at all, so the rows it covers
    /// are missing from the raster.
    TiffSegmentUndecodable,
    /// TIFF: the `NextIFD` chain pointed back at a directory already read, or
    /// ran past the depth bound. The chain was cut there.
    TiffDirectoryCycle,
    /// TIFF: `SamplesPerPixel` was smaller than the
    /// `PhotometricInterpretation` needs. The photometric won.
    TiffSamplesPerPixelWrong,
    /// TIFF: `FillOrder` 2 on a coding whose strips are a byte stream rather
    /// than a bit stream. Reversing them would destroy the strip, so the tag
    /// was ignored.
    TiffFillOrderIgnored,
    /// TIFF: every `ColorMap` value was at or below 255, so the map was read
    /// as an 8-bit one rather than as the 16-bit one p.23 describes.
    TiffColorMapIsEightBit,
    /// TIFF: the file holds more than one image file directory and was read
    /// through `tiff_scan`, which takes the first. `tiff_scan_directory` reads
    /// the others by index, and a caller that pages uses it instead.
    TiffExtraPagesIgnored,

    // ---- PNG (ISO/IEC 15948) ---------------------------------------------
    //
    // Three, and every one of them is a *leniency* rather than a refusal.
    // Everything a PNG can get wrong that costs meaning rather than pixels is
    // a [`png::PngError`] instead — because gap 17's finding was that naming
    // the refusal is the feature, and an error can carry the offending chunk's
    // four bytes where a closed warning set cannot.
    /// PNG: an **ancillary** chunk's CRC-32 did not match, so the chunk was
    /// dropped and whatever it said is missing.
    ///
    /// A critical chunk failing the same check refuses the image, which is
    /// 5.4's own rule: the case of a type's first letter says whether the image
    /// can be shown without it.
    PngChunkCrcMismatch,
    /// PNG: a chunk whose CRC was fine was dropped for a structural reason — a
    /// second IHDR or PLTE, a PLTE where the colour type forbids one, a `tRNS`
    /// of the wrong length for its colour type, or one that arrived before the
    /// palette it indexes.
    PngChunkDropped,
    /// PNG: a sample indexed past the end of PLTE. 11.2.3 makes that an error;
    /// ruling 2 makes it a black pixel here, because refusing the image would
    /// throw away every pixel that was fine.
    PngPaletteIndexOutOfRange,

    // ---- BMP -------------------------------------------------------------
    //
    // Three, and every one of them is a *leniency*: what a bitmap can get
    // wrong that costs meaning is a [`bmp::BmpError`], for `png.rs`'s reason.
    /// BMP: a pixel indexed past the end of the colour table the file
    /// supplied. The table is padded with black to the depth's own size, so the
    /// pixel is black — [`Warning::PngPaletteIndexOutOfRange`]'s bargain.
    BmpPaletteIndexOutOfRange,
    /// BMP: an RLE delta or an early end-of-line or end-of-bitmap left pixels
    /// the stream never defined. The documentation does not say what they
    /// are, and they are index 0 here.
    BmpRleUndefinedPixels,
    /// BMP: an RLE run or literal reached past the end of its row or of the
    /// image. The excess was dropped rather than wrapped onto the next row.
    BmpRleOverrun,

    // ---- GIF -------------------------------------------------------------
    //
    // Three, and each a *leniency*; LZW damage reuses [`Warning::BadLzwCode`]
    // and [`Warning::TruncatedInput`], which already say it.
    /// GIF: an index past the end of the active colour table. The table is
    /// padded with black to 256 entries, so the pixel is black.
    GifPaletteIndexOutOfRange,
    /// GIF: the file holds more than one image. The first is the picture; the
    /// rest are not decoded — an animation drawn as its first frame.
    GifFramesIgnored,
    /// GIF: the first image reaches past its logical screen, and the part
    /// outside it was clipped.
    GifFrameOutsideScreen,

    // ---- WebP (RFC 9649) -------------------------------------------------
    //
    // Three leniencies; a cut-off stream reuses [`Warning::TruncatedInput`].
    /// WebP: a back-reference reached before the image or past its end, or a
    /// colour-cache index past the cache — data that is well formed and
    /// impossible. The image stops there and the rest is transparent black.
    WebpCorruptData,
    /// WebP: an animation's frames after the first are not decoded.
    WebpFramesIgnored,
    /// WebP: an `ALPH` chunk would not decode, so the picture is opaque.
    WebpAlphaDropped,
}

impl Warning {
    /// A short stable identifier, suitable for a debug dump or a test name.
    /// Callers that re-export these as their own typed warnings need a name
    /// that does not change when the prose does.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::RawDeflateFallback => "raw-deflate-fallback",
            Self::FdictIgnored => "fdict-ignored",
            Self::AdlerMismatch => "adler-mismatch",
            Self::TruncatedInput => "truncated-input",
            Self::EarlyEod => "early-eod",
            Self::TrailingGarbage => "trailing-garbage",
            Self::OutputCapHit => "output-cap-hit",
            Self::CorruptDeflateStream => "corrupt-deflate-stream",
            Self::OddHexDigit => "odd-hex-digit",
            Self::NonHexByte => "non-hex-byte",
            Self::BadAscii85Byte => "bad-ascii85-byte",
            Self::Ascii85ZInGroup => "ascii85-z-in-group",
            Self::Ascii85Overflow => "ascii85-overflow",
            Self::BadLzwCode => "bad-lzw-code",
            Self::LzwTableFull => "lzw-table-full",
            Self::PredictorRowShort => "predictor-row-short",
            Self::BadPredictorTag => "bad-predictor-tag",
            Self::ChainTailIgnored => "chain-tail-ignored",
            Self::MissingEndOfLine => "missing-end-of-line",
            Self::Jbig2SegmentSkipped => "jbig2-segment-skipped",
            Self::Jbig2RegionTooLarge => "jbig2-region-too-large",
            Self::Jbig2BudgetSpent => "jbig2-budget-spent",
            Self::Jbig2VariantSkipped => "jbig2-variant-skipped",
            Self::Jbig2SymbolLimitHit => "jbig2-symbol-limit-hit",
            Self::PackBitsNoOp => "packbits-no-op",
            Self::PackBitsRunOverruns => "packbits-run-overruns",
            Self::TiffOldStyleLzw => "tiff-old-style-lzw",
            Self::TiffSegmentUndecodable => "tiff-segment-undecodable",
            Self::TiffDirectoryCycle => "tiff-directory-cycle",
            Self::TiffSamplesPerPixelWrong => "tiff-samples-per-pixel-wrong",
            Self::TiffFillOrderIgnored => "tiff-fill-order-ignored",
            Self::TiffColorMapIsEightBit => "tiff-color-map-is-eight-bit",
            Self::TiffExtraPagesIgnored => "tiff-extra-pages-ignored",
            Self::JpxMarkerUnsupported => "jpx-marker-unsupported",
            Self::JpxMarkerUnknown => "jpx-marker-unknown",
            Self::JpxStructureInvalid => "jpx-structure-invalid",
            Self::JpxFeatureUnsupported => "jpx-feature-unsupported",
            Self::JpxPrecisionUnsupported => "jpx-precision-unsupported",
            Self::JpxTruncated => "jpx-truncated",
            Self::JpxPacketLength => "jpx-packet-length",
            Self::JpxSegmentationSymbol => "jpx-segmentation-symbol",
            Self::JpxBudgetSpent => "jpx-budget-spent",
            Self::JpxCoefficientClamped => "jpx-coefficient-clamped",
            Self::JpegPrecisionNarrowed => "jpeg-precision-narrowed",
            Self::PngChunkCrcMismatch => "png-chunk-crc-mismatch",
            Self::PngChunkDropped => "png-chunk-dropped",
            Self::PngPaletteIndexOutOfRange => "png-palette-index-out-of-range",
            Self::BmpPaletteIndexOutOfRange => "bmp-palette-index-out-of-range",
            Self::BmpRleUndefinedPixels => "bmp-rle-undefined-pixels",
            Self::BmpRleOverrun => "bmp-rle-overrun",
            Self::GifPaletteIndexOutOfRange => "gif-palette-index-out-of-range",
            Self::GifFramesIgnored => "gif-frames-ignored",
            Self::GifFrameOutsideScreen => "gif-frame-outside-screen",
            Self::WebpCorruptData => "webp-corrupt-data",
            Self::WebpFramesIgnored => "webp-frames-ignored",
            Self::WebpAlphaDropped => "webp-alpha-dropped",
        }
    }
}

impl fmt::Display for Warning {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Self::RawDeflateFallback => "zlib header invalid, decoded as raw deflate",
            Self::FdictIgnored => "zlib preset dictionary ignored",
            Self::AdlerMismatch => "zlib Adler-32 mismatch",
            Self::TruncatedInput => "input ended mid-symbol",
            Self::EarlyEod => "stream ended before its EOD marker",
            Self::TrailingGarbage => "bytes after end of stream ignored",
            Self::OutputCapHit => "output limit reached, decode truncated",
            Self::CorruptDeflateStream => "corrupt deflate block",
            Self::OddHexDigit => "odd hex digit count, trailing zero implied",
            Self::NonHexByte => "non-hex byte skipped",
            Self::BadAscii85Byte => "invalid ASCII85 byte skipped",
            Self::Ascii85ZInGroup => "ASCII85 'z' inside a group skipped",
            Self::Ascii85Overflow => "ASCII85 group above 2^32-1 clamped",
            Self::BadLzwCode => "LZW code out of range",
            Self::LzwTableFull => "LZW table full without a clear code",
            Self::PredictorRowShort => "short predictor row",
            Self::BadPredictorTag => "unknown PNG predictor tag",
            Self::ChainTailIgnored => "filters after an image codec ignored",
            Self::MissingEndOfLine => "expected end-of-line code absent",
            Self::Jbig2SegmentSkipped => "JBIG2 segment type not decoded",
            Self::Jbig2RegionTooLarge => "JBIG2 region larger than the output ceiling",
            Self::Jbig2BudgetSpent => "JBIG2 decode work budget spent",
            Self::Jbig2VariantSkipped => "JBIG2 coding variant not decoded here",
            Self::Jbig2SymbolLimitHit => "JBIG2 symbol dictionary past its bounds",
            Self::PackBitsNoOp => "PackBits no-op tag skipped",
            Self::PackBitsRunOverruns => "PackBits run past the expected byte count",
            Self::TiffOldStyleLzw => "TIFF LZW strip in the pre-1993 bit order, repacked",
            Self::TiffSegmentUndecodable => "TIFF strip or tile could not be decoded",
            Self::TiffDirectoryCycle => "TIFF directory chain cycled or ran past its bound",
            Self::TiffSamplesPerPixelWrong => "TIFF SamplesPerPixel below the photometric's need",
            Self::TiffFillOrderIgnored => "TIFF FillOrder 2 on a byte-oriented coding, ignored",
            Self::TiffColorMapIsEightBit => "TIFF ColorMap written at 8 bits rather than 16",
            Self::TiffExtraPagesIgnored => "TIFF directories after the first are not read",
            Self::JpxMarkerUnsupported => "JPX marker defined by T.800 but not decoded here",
            Self::JpxMarkerUnknown => "JPX marker not defined by T.800 Table A.2",
            Self::JpxStructureInvalid => "JPX codestream or box structure invalid",
            Self::JpxFeatureUnsupported => "JPX coding feature not implemented here",
            Self::JpxPrecisionUnsupported => "JPX component precision above 16 bits",
            Self::JpxTruncated => "JPX codestream ended mid-structure",
            Self::JpxPacketLength => "JPX packet length does not reach the next packet",
            Self::JpxSegmentationSymbol => "JPX segmentation symbol was not 1010",
            Self::JpxBudgetSpent => "JPX decode budget spent",
            Self::JpxCoefficientClamped => "JPX coefficient clamped to E.1's dynamic range",
            Self::JpegPrecisionNarrowed => "JPEG twelve-bit samples narrowed to eight",
            Self::PngChunkCrcMismatch => "PNG ancillary chunk CRC-32 mismatch, chunk dropped",
            Self::PngChunkDropped => "PNG chunk dropped as misplaced or malformed",
            Self::PngPaletteIndexOutOfRange => "PNG sample indexed past the end of PLTE",
            Self::BmpPaletteIndexOutOfRange => "BMP pixel indexed past the end of its colour table",
            Self::BmpRleUndefinedPixels => "BMP RLE stream left pixels undefined; index 0 used",
            Self::BmpRleOverrun => "BMP RLE run past the end of its row, clipped",
            Self::GifPaletteIndexOutOfRange => "GIF pixel indexed past the end of its colour table",
            Self::GifFramesIgnored => "GIF images after the first are not decoded",
            Self::GifFrameOutsideScreen => "GIF image reaches past its logical screen, clipped",
            Self::WebpCorruptData => "WebP pixel data refers outside the image; decode stopped",
            Self::WebpFramesIgnored => "WebP animation frames after the first are not decoded",
            Self::WebpAlphaDropped => "WebP alpha chunk undecodable; picture drawn opaque",
        };
        f.write_str(s)
    }
}

/// 7.2.3, Table 1: the six white-space characters.
pub(crate) const fn is_white(b: u8) -> bool {
    matches!(b, 0x00 | 0x09 | 0x0a | 0x0c | 0x0d | 0x20)
}

/// What may follow the data a byte layer decodes without being
/// [`Warning::TrailingGarbage`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Tail {
    /// A stream's bytes as a `/Filter` reads them, at any stage of its
    /// chain: they may end in the end-of-line marker 7.3.8.1 keeps out of
    /// `/Length`, which a producer counted in, or in blanks before it — and a
    /// stage handed what an earlier one decoded may meet the same marker
    /// re-encoded with the data. White space after the end of data is no
    /// garbage.
    WhiteSpace,
    /// A container's coded data, which ends where its data does: a PNG's
    /// image data, a TIFF strip. Any byte after it is garbage.
    Nothing,
}

impl Tail {
    /// Whether `rest`, the bytes after the end of data, are garbage.
    pub(crate) fn garbage(self, rest: &[u8]) -> bool {
        match self {
            Self::WhiteSpace => rest.iter().any(|&b| !is_white(b)),
            Self::Nothing => !rest.is_empty(),
        }
    }
}

/// Deduplicating warning sink. Keeps first-occurrence order and bounds the
/// vector by the size of the `Warning` set, so a pathological stream cannot
/// turn leniency into an allocation attack.
///
/// It is the decode path's one context, so it also counts the steps the
/// decode took (`docs/design/resource-exhaustion.md`, "The counters"): each
/// decoder adds a step where it reads a symbol, a code, a row or a byte, and
/// a counted door hands the total back with what it decoded.
#[derive(Clone, Debug, Default)]
pub(crate) struct Warnings {
    list: Vec<Warning>,
    steps: u64,
}

impl Warnings {
    pub(crate) fn push(&mut self, w: Warning) {
        if !self.list.contains(&w) {
            self.list.push(w);
        }
    }

    /// Both lists of warnings, and both counts of steps: a decode run on a
    /// sink of its own is this one's work all the same.
    pub(crate) fn merge(&mut self, other: &Self) {
        for w in &other.list {
            self.push(*w);
        }
        self.step_by(other.steps);
    }

    /// `n` steps more, saturating.
    pub(crate) fn step_by(&mut self, n: u64) {
        self.steps = self.steps.saturating_add(n);
    }

    /// `n` bytes or entries more, as steps.
    pub(crate) fn step_len(&mut self, n: usize) {
        self.step_by(u64::try_from(n).unwrap_or(u64::MAX));
    }

    /// The steps counted so far.
    pub(crate) fn steps(&self) -> u64 {
        self.steps
    }

    pub(crate) fn into_vec(self) -> Vec<Warning> {
        self.list
    }
}

/// One filter of a `/Filter` chain. Image codecs are members so a chain can
/// name them, but they terminate the chain — see [`ChainOutput`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Filter {
    Flate,
    Lzw,
    AsciiHex,
    Ascii85,
    RunLength,
    Dct,
    Ccitt,
    Jbig2,
    Jpx,
}

impl Filter {
    /// `Some` when this filter produces an image rather than bytes.
    #[must_use]
    pub const fn image_codec(self) -> Option<ImageCodec> {
        match self {
            Self::Dct => Some(ImageCodec::Dct),
            Self::Ccitt => Some(ImageCodec::Ccitt),
            Self::Jbig2 => Some(ImageCodec::Jbig2),
            Self::Jpx => Some(ImageCodec::Jpx),
            _ => None,
        }
    }
}

/// The filters whose output is an image, not bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ImageCodec {
    Dct,
    Ccitt,
    Jbig2,
    Jpx,
}

impl ImageCodec {
    /// The capability gate this codec sits behind, if it has one. `None`
    /// means the codec is decoded in-crate; `Some` means a decode **may** be
    /// refused, and this is the capability the caller names when it
    /// substitutes the neutral placeholder (ruling 2).
    ///
    /// "May", for JBIG2, since gap 17. It used to mean the bytes would never
    /// be decoded at all. The generic-region lineage now decodes and the
    /// symbol-dictionary lineage does not, so the gate has become a statement
    /// about a *decode* rather than about a *format* — see
    /// [`crate::jbig2_decode`], whose error is this same capability. Plan 02's
    /// milestone 4 records the change; the assertion below is not a weaker
    /// version of the old one, it is a claim about a different thing.
    #[must_use]
    pub const fn capability(self) -> Option<Capability> {
        match self {
            Self::Jbig2 => Some(Capability::Jbig2),
            Self::Jpx => Some(Capability::Jpx),
            Self::Dct | Self::Ccitt => None,
        }
    }
}

/// A chain entry: which filter, plus the parameters COS decoded for it.
///
/// Parameters that do not apply to the named filter are ignored, so a caller
/// can forward a `/DecodeParms` entry without first deciding whether it is
/// meaningful.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FilterSpec {
    pub filter: Filter,
    /// Flate and LZW only.
    pub predictor: Option<PredictorParams>,
    /// LZW only. `/EarlyChange`, whose PDF default is 1 (true).
    pub early_change: bool,
}

impl FilterSpec {
    /// A filter with PDF-default parameters.
    #[must_use]
    pub const fn new(filter: Filter) -> Self {
        Self {
            filter,
            predictor: None,
            early_change: true,
        }
    }

    #[must_use]
    pub const fn with_predictor(mut self, p: PredictorParams) -> Self {
        self.predictor = Some(p);
        self
    }

    #[must_use]
    pub const fn with_early_change(mut self, early_change: bool) -> Self {
        self.early_change = early_change;
        self
    }
}

/// What a chain produced: bytes, or the still-encoded payload of an image
/// codec that terminated it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ChainOutput {
    Bytes(Decoded),
    EncodedImage {
        kind: ImageCodec,
        /// The bytes as the codec wants them: every byte filter ahead of the
        /// codec has been applied, the codec itself has not.
        data: Vec<u8>,
        warnings: Vec<Warning>,
    },
}

/// FlateDecode (7.4.4): zlib or raw deflate, then the predictor if one was
/// given.
///
/// # Errors
/// [`FilterError::BadParams`] if the predictor parameters cannot describe a
/// row. Damaged *data* never errors.
pub fn flate_decode(
    input: &[u8],
    limits: &Limits,
    predictor: Option<&PredictorParams>,
) -> Result<Decoded, FilterError> {
    flate_decode_counted(input, limits, predictor).0
}

/// [`flate_decode`], and the steps it took
/// (`docs/design/resource-exhaustion.md`, "The counters"): a step for each
/// symbol, header field and code length the inflater read, each byte the
/// predictor unfiltered, and each byte the decode returned.
pub fn flate_decode_counted(
    input: &[u8],
    limits: &Limits,
    predictor: Option<&PredictorParams>,
) -> (Result<Decoded, FilterError>, u64) {
    let mut w = Warnings::default();
    let (data, complete) = inflate::flate_bytes(input, limits, Tail::WhiteSpace, &mut w);
    finish_counted(data, complete, predictor, limits, w)
}

/// LZWDecode (7.4.4), then the predictor if one was given.
///
/// `early_change` is `/EarlyChange`, whose default is 1 (`true`).
///
/// # Errors
/// [`FilterError::BadParams`] if the predictor parameters cannot describe a
/// row. Damaged *data* never errors.
pub fn lzw_decode(
    input: &[u8],
    limits: &Limits,
    early_change: bool,
    predictor: Option<&PredictorParams>,
) -> Result<Decoded, FilterError> {
    lzw_decode_counted(input, limits, early_change, predictor).0
}

/// [`lzw_decode`], and the steps it took: a step for each code read, each
/// byte of the string it expands to, each byte the predictor unfiltered, and
/// each byte the decode returned.
pub fn lzw_decode_counted(
    input: &[u8],
    limits: &Limits,
    early_change: bool,
    predictor: Option<&PredictorParams>,
) -> (Result<Decoded, FilterError>, u64) {
    let mut w = Warnings::default();
    let (data, complete) = lzw::lzw_bytes(input, limits, early_change, Tail::WhiteSpace, &mut w);
    finish_counted(data, complete, predictor, limits, w)
}

/// ASCIIHexDecode (7.4.2).
#[must_use]
pub fn ascii_hex_decode(input: &[u8], limits: &Limits) -> Decoded {
    ascii_hex_decode_counted(input, limits).0
}

/// [`ascii_hex_decode`], and the steps it took: a step for each byte read
/// and each byte returned.
pub fn ascii_hex_decode_counted(input: &[u8], limits: &Limits) -> (Decoded, u64) {
    let mut w = Warnings::default();
    let (data, complete) = ascii::hex_bytes(input, limits, &mut w);
    decoded_counted(data, complete, w)
}

/// What a decode returned, and its steps with a step for each byte of it.
fn decoded_counted(data: Vec<u8>, complete: bool, mut w: Warnings) -> (Decoded, u64) {
    w.step_len(data.len());
    let steps = w.steps();
    (
        Decoded {
            data,
            complete,
            warnings: w.into_vec(),
        },
        steps,
    )
}

/// ASCII85Decode (7.4.3).
#[must_use]
pub fn ascii85_decode(input: &[u8], limits: &Limits) -> Decoded {
    ascii85_decode_counted(input, limits).0
}

/// [`ascii85_decode`], and the steps it took: a step for each byte read and
/// each byte returned.
pub fn ascii85_decode_counted(input: &[u8], limits: &Limits) -> (Decoded, u64) {
    let mut w = Warnings::default();
    let (data, complete) = ascii::a85_bytes(input, limits, &mut w);
    decoded_counted(data, complete, w)
}

/// RunLengthDecode (7.4.5).
#[must_use]
pub fn run_length_decode(input: &[u8], limits: &Limits) -> Decoded {
    run_length_decode_counted(input, limits).0
}

/// [`run_length_decode`], and the steps it took: a step for each length byte
/// read and each byte returned.
pub fn run_length_decode_counted(input: &[u8], limits: &Limits) -> (Decoded, u64) {
    let mut w = Warnings::default();
    let (data, complete) = runlength::rle_bytes(input, limits, &mut w);
    decoded_counted(data, complete, w)
}

/// PackBits (TIFF 6.0 §9), the run-length scheme TIFF compression 32773 names.
///
/// **Not `/Filter` RunLengthDecode**, though the two are the same Macintosh
/// scheme: 7.4.5 makes the byte 128 an end-of-data marker and §9 makes it a
/// no-op, and §9 stops on a byte count where 7.4.5 stops on the marker. So
/// `expected` — the bytes one strip is declared to hold — is a parameter here
/// and does not exist there. See [`run_length_decode`] for the other one.
#[must_use]
pub fn packbits_decode(input: &[u8], expected: usize, limits: &Limits) -> Decoded {
    let mut w = Warnings::default();
    let (data, complete) = packbits::packbits_bytes(input, expected, limits, &mut w);
    Decoded {
        data,
        complete,
        warnings: w.into_vec(),
    }
}

/// The Table 10 predictors, applied on their own. Flate and LZW streams
/// normally reach this through their own entry points.
///
/// # Errors
/// [`FilterError::BadParams`] if the parameters cannot describe a row.
pub fn predictor_decode(
    input: &[u8],
    params: &PredictorParams,
    limits: &Limits,
) -> Result<Decoded, FilterError> {
    predictor_decode_counted(input, params, limits).0
}

/// [`predictor_decode`], and the steps it took: a step for each byte a row
/// unfiltered and each byte returned.
pub fn predictor_decode_counted(
    input: &[u8],
    params: &PredictorParams,
    limits: &Limits,
) -> (Result<Decoded, FilterError>, u64) {
    let mut w = Warnings::default();
    match predictors::unpredict(input, params, limits, &mut w) {
        Ok((data, complete)) => {
            let (decoded, steps) = decoded_counted(data, complete, w);
            (Ok(decoded), steps)
        }
        Err(e) => (Err(e), w.steps()),
    }
}

/// Runs a `/Filter` chain in order.
///
/// An image codec terminates the chain: its input bytes come back tagged
/// with [`ImageCodec`] for the caller to hand to a codec entry point. A
/// filter named after an image codec is nonsense, so the tail is dropped
/// with [`Warning::ChainTailIgnored`].
///
/// # Errors
/// [`FilterError::BadParams`] if a stage's predictor parameters cannot
/// describe a row.
pub fn apply_chain(
    input: &[u8],
    chain: &[FilterSpec],
    limits: &Limits,
) -> Result<ChainOutput, FilterError> {
    let mut w = Warnings::default();
    let mut complete = true;
    let mut carried: Option<Vec<u8>> = None;

    for (i, spec) in chain.iter().enumerate() {
        let cur: &[u8] = match carried.as_deref() {
            Some(d) => d,
            None => input,
        };

        if let Some(kind) = spec.filter.image_codec() {
            if i + 1 < chain.len() {
                w.push(Warning::ChainTailIgnored);
            }
            let (data, _) = cap_slice(cur, limits, &mut w);
            return Ok(ChainOutput::EncodedImage {
                kind,
                data,
                warnings: w.into_vec(),
            });
        }

        let (data, ok) = match spec.filter {
            Filter::Flate => {
                let (d, ok) = inflate::flate_bytes(cur, limits, Tail::WhiteSpace, &mut w);
                stage_predictor(d, ok, spec.predictor.as_ref(), limits, &mut w)?
            }
            Filter::Lzw => {
                let (d, ok) =
                    lzw::lzw_bytes(cur, limits, spec.early_change, Tail::WhiteSpace, &mut w);
                stage_predictor(d, ok, spec.predictor.as_ref(), limits, &mut w)?
            }
            Filter::AsciiHex => ascii::hex_bytes(cur, limits, &mut w),
            Filter::Ascii85 => ascii::a85_bytes(cur, limits, &mut w),
            Filter::RunLength => runlength::rle_bytes(cur, limits, &mut w),
            // Image codecs returned above.
            Filter::Dct | Filter::Ccitt | Filter::Jbig2 | Filter::Jpx => (cur.to_vec(), true),
        };

        complete &= ok;
        carried = Some(data);
    }

    let data = match carried {
        Some(d) => d,
        // An empty chain is the identity, and the identity is still bounded.
        None => {
            let (d, ok) = cap_slice(input, limits, &mut w);
            complete &= ok;
            d
        }
    };
    Ok(ChainOutput::Bytes(Decoded {
        data,
        complete,
        warnings: w.into_vec(),
    }))
}

fn cap_slice(data: &[u8], limits: &Limits, w: &mut Warnings) -> (Vec<u8>, bool) {
    match data.get(..limits.max_output) {
        Some(head) if head.len() < data.len() => {
            w.push(Warning::OutputCapHit);
            (head.to_vec(), false)
        }
        _ => (data.to_vec(), true),
    }
}

fn stage_predictor(
    data: Vec<u8>,
    complete: bool,
    predictor: Option<&PredictorParams>,
    limits: &Limits,
    w: &mut Warnings,
) -> Result<(Vec<u8>, bool), FilterError> {
    match predictor {
        Some(p) if p.is_active() => {
            let (out, ok) = predictors::unpredict(&data, p, limits, w)?;
            Ok((out, complete && ok))
        }
        _ => Ok((data, complete)),
    }
}

/// What a decode returned, its predictor applied, and the steps of both, a
/// step for each byte returned among them.
fn finish_counted(
    data: Vec<u8>,
    complete: bool,
    predictor: Option<&PredictorParams>,
    limits: &Limits,
    mut w: Warnings,
) -> (Result<Decoded, FilterError>, u64) {
    match stage_predictor(data, complete, predictor, limits, &mut w) {
        Ok((data, complete)) => {
            let (decoded, steps) = decoded_counted(data, complete, w);
            (Ok(decoded), steps)
        }
        Err(e) => (Err(e), w.steps()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn warnings_dedup_and_keep_order() {
        let mut w = Warnings::default();
        w.push(Warning::EarlyEod);
        w.push(Warning::NonHexByte);
        w.push(Warning::EarlyEod);
        assert_eq!(w.into_vec(), vec![Warning::EarlyEod, Warning::NonHexByte]);
    }

    #[test]
    fn warning_merge_dedups_across_sinks() {
        let mut a = Warnings::default();
        a.push(Warning::OutputCapHit);
        let mut b = Warnings::default();
        b.push(Warning::OutputCapHit);
        b.push(Warning::TruncatedInput);
        a.merge(&b);
        assert_eq!(
            a.into_vec(),
            vec![Warning::OutputCapHit, Warning::TruncatedInput]
        );
    }

    #[test]
    fn empty_chain_is_identity() {
        let out = apply_chain(b"abc", &[], &Limits::new(64)).expect("no params to reject");
        match out {
            ChainOutput::Bytes(d) => {
                assert_eq!(d.data, b"abc");
                assert!(d.complete);
                assert!(d.warnings.is_empty());
            }
            ChainOutput::EncodedImage { .. } => panic!("no codec in chain"),
        }
    }

    #[test]
    fn white_space_is_garbage_only_after_a_containers_data() {
        for rest in [&b""[..], b"\n", b"\r\n", b"\r", b" \t\x0c\x00"] {
            assert!(!Tail::WhiteSpace.garbage(rest), "{rest:?}");
        }
        assert!(Tail::WhiteSpace.garbage(b"\r\nx"));
        assert!(!Tail::Nothing.garbage(b""));
        assert!(Tail::Nothing.garbage(b"\n"));
    }

    /// A stream re-encoded whole, the end-of-line its `/Length` counted
    /// with it: each stage of the chain meets white space after its own end
    /// of data, and neither is garbage.
    #[test]
    fn white_space_after_each_stage_of_a_chain_is_the_streams_own() {
        let chain = [
            FilterSpec::new(Filter::AsciiHex),
            FilterSpec::new(Filter::Flate),
        ];
        // "Hello", stored in a zlib wrapper, its checksum and a line feed,
        // in hex, and the hex stream's own end-of-line.
        let coded = b"7801010500faff48656c6c6f058c01f50a>\r\n";
        match apply_chain(coded, &chain, &Limits::new(64)).expect("no params to reject") {
            ChainOutput::Bytes(d) => {
                assert_eq!(d.data, b"Hello");
                assert!(d.complete);
                assert!(d.warnings.is_empty(), "{:?}", d.warnings);
            }
            ChainOutput::EncodedImage { .. } => panic!("no codec in chain"),
        }
    }

    #[test]
    fn image_codec_terminates_chain() {
        // ASCIIHex then DCT: the hex is decoded, the JPEG bytes come back raw.
        let chain = [
            FilterSpec::new(Filter::AsciiHex),
            FilterSpec::new(Filter::Dct),
        ];
        let out = apply_chain(b"FFD8FFE0>", &chain, &Limits::new(64)).expect("params fine");
        match out {
            ChainOutput::EncodedImage {
                kind,
                data,
                warnings,
            } => {
                assert_eq!(kind, ImageCodec::Dct);
                assert_eq!(data, vec![0xFF, 0xD8, 0xFF, 0xE0]);
                assert!(warnings.is_empty());
            }
            ChainOutput::Bytes(_) => panic!("codec should terminate"),
        }
    }

    #[test]
    fn chain_tail_after_codec_is_dropped_with_a_warning() {
        let chain = [
            FilterSpec::new(Filter::Jpx),
            FilterSpec::new(Filter::AsciiHex),
        ];
        let out = apply_chain(b"\x00\x01", &chain, &Limits::new(64)).expect("params fine");
        match out {
            ChainOutput::EncodedImage {
                kind,
                data,
                warnings,
            } => {
                assert_eq!(kind, ImageCodec::Jpx);
                assert_eq!(data, vec![0, 1]);
                assert_eq!(warnings, vec![Warning::ChainTailIgnored]);
            }
            ChainOutput::Bytes(_) => panic!("codec should terminate"),
        }
    }

    /// Plan 02's milestone 4 pins this, and what it pins moved in gap 17.
    ///
    /// JBIG2 still answers `Some`, because a JBIG2 decode can still be
    /// refused and this is the name it is refused under. What it no longer
    /// means is "these bytes will never be decoded" — the generic-region
    /// lineage decodes. The plan file says so in its own words rather than
    /// this assertion being quietly reinterpreted to keep passing.
    #[test]
    fn capability_gates_name_the_deferred_codecs() {
        assert_eq!(ImageCodec::Jbig2.capability(), Some(Capability::Jbig2));
        assert_eq!(ImageCodec::Jpx.capability(), Some(Capability::Jpx));
        assert_eq!(ImageCodec::Dct.capability(), None);
        assert_eq!(ImageCodec::Ccitt.capability(), None);
        assert_eq!(Filter::Flate.image_codec(), None);
        assert_eq!(Filter::Ccitt.image_codec(), Some(ImageCodec::Ccitt));
    }

    #[test]
    fn filter_error_displays() {
        assert_eq!(
            FilterError::BadParams("bad").to_string(),
            "bad filter parameters: bad"
        );
        assert_eq!(
            FilterError::Unsupported(Capability::Jpx).to_string(),
            "unsupported codec: Jpx"
        );
        assert_eq!(
            Warning::EarlyEod.to_string(),
            "stream ended before its EOD marker"
        );
    }
}
