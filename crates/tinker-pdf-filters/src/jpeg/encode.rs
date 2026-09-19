//! Baseline JPEG encoding (ITU-T T.81 Annex F.1.2) — the writer half of
//! [`super`].
//!
//! # What "baseline" bounds, and what is excluded by name
//!
//! T.81 4.11 names one process "baseline": the **sequential DCT, Huffman
//! coding, 8-bit sample precision**, with at most two DC and two AC table
//! destinations and at most four components in a frame. That is what this
//! writes — an SOF0 frame, one scan, `Ss = 0`, `Se = 63`, `Ah = Al = 0` — and
//! everything else T.81 defines is excluded rather than half-built:
//!
//! - **Progressive (SOF2)** and **extended sequential (SOF1)**. The decoder
//!   beside this reads both; nothing here writes them. A progressive scan is
//!   not a different entropy coder but a different *schedule* — spectral
//!   selection and successive approximation across many scans — and choosing
//!   that schedule is a policy decision belonging to whoever wants it.
//! - **Arithmetic coding** (SOF9 SOF10 SOF11 SOF13 SOF14 SOF15). Annex D's QM
//!   coder. **The decoder beside this reads SOF9 and SOF10 as of 20 September
//!   2026** — `crate::qm` is the coder and `super::arith` the models — so what
//!   is missing here is not the coder but an encoder-side model: Figures F.4
//!   to F.6 and G.9 to G.11 are a second transcription of Annex F and Annex G,
//!   and nothing published would adjudicate it any more than it adjudicates
//!   the decoder's (ruling 13, and that file's header).
//! - **Lossless (SOF3, SOF7)** and **hierarchical/differential (SOF5, SOF6,
//!   SOF13..SOF15)**. Annexes H and J share no machinery with the DCT path.
//! - **12-bit precision.** T.81 B.2.2 allows `P = 12` for the extended
//!   processes only; baseline is `P = 8`, and this takes a byte per sample.
//! - **Four-component frames — CMYK and YCCK.** T.81 has no opinion about what
//!   four components mean; the meaning a reader would need comes from Adobe's
//!   APP14, which is not in T.81 or T.871, and inventing an APP14 here would be
//!   writing a convention rather than a standard. [`JpegSourceColour`] carries
//!   the two cases whose interpretation *is* published.
//! - **Optimised Huffman tables.** K.2's procedure builds a table from an
//!   image's own statistics; this writes Annex K's typical tables instead, and
//!   K.3.3's argument for that is below.
//! - **DNL**, multi-scan non-interleaved frames, and the abbreviated formats of
//!   B.4 (tables without a frame, or a frame without tables). One
//!   self-contained interchange-format datastream, SOI to EOI.
//!
//! # The three things this had to settle, each with its argument
//!
//! ## Colour transforms, and how a component's sampling factors are chosen
//!
//! **T.81 specifies no colour space at all** — it codes numbered components and
//! says nothing about what they mean (A.1: "the source image ... is not
//! restricted to a particular colour space"). The meaning comes from outside,
//! and the published outside is **ITU-T T.871 | ISO/IEC 10918-5 clause 7**,
//! whose YCbCr the decoder beside this already inverts. So exactly two source
//! colours are written, both of them ones T.871 defines:
//!
//! - [`JpegSourceColour::Gray`] — one component, no transform, no APP0
//!   ambiguity to resolve.
//! - [`JpegSourceColour::Rgb`] — three components through T.871 clause 7's
//!   forward equations, with the JFIF APP0 segment T.871 clause 10.1 requires
//!   so that a reader is *told* the components are that YCbCr rather than
//!   having to guess from the component count.
//!
//! The transform used is T.871's **exact** form, not the four-decimal
//! approximation printed under it, and it is arranged as the identity the
//! exact form is built on: `Cb = (B - Y) / 1.772 + 128` and
//! `Cr = (R - Y) / 1.402 + 128`, with `Y` unrounded. That is algebraically the
//! same as the printed `-0.299 R - 0.587 G + 0.886 B` over 1.772, and writing
//! it the other way is what makes
//! `the_colour_transform_is_t_871_clause_7_s` a check rather
//! than a tautology: the test computes the published expression literally, the
//! code computes the identity in fixed point, and the two have to meet.
//!
//! **Sampling factors are named by the caller and never inferred.**
//! [`JpegSampling`] offers 4:4:4 and 4:2:0 and the default is 4:4:4. Three
//! reasons, in order of weight. T.81 recommends no factors — Annex K's
//! quantisation tables mention "2:1 horizontal subsampling" in passing and that
//! is the whole of its advice. Choosing factors from the pixels would make the
//! output depend on image content in a way the caller cannot predict, which is
//! the opposite of what ruling 4 asks of this engine. And subsampling is a
//! *lossy colour decision*: `finish` upsamples by replicating a sample
//! over the block it covers, so a caller who did not ask for 4:2:0 should not
//! silently get chroma back through a box filter and a replicate.
//!
//! ## The quantisation tables, and the caller who wants a specific quality
//!
//! **T.81 publishes exactly two quantiser settings, and this offers exactly
//! those two plus the caller's own.** Tables K.1 and K.2 are the first;
//! K.1's own second paragraph is the second — "if these quantization values are
//! divided by 2, the resulting reconstructed image is usually nearly
//! indistinguishable from the source image". [`JpegQuantisation`] is those two
//! rungs and a `Tables` variant.
//!
//! There is deliberately **no `quality: u8` from 1 to 100**, and the caller who
//! wants one should read this paragraph rather than the roadmap. Every such
//! scale in circulation is some *program's* private convention — a linear
//! ramp, a reciprocal above 50, a floor at 1, a ceiling at 255, each choice
//! arbitrary and none of them in T.81, T.83 or T.871. Shipping one would put a
//! number in this crate's public API that no published document adjudicates,
//! and then every later change to it would be a silent change to every
//! caller's bytes. What the format actually carries is the table, so a caller
//! who wants a specific quality supplies the table and gets exactly it. T.83
//! clause 4.3.3 makes the same point from the other end: "required accuracy is
//! a function of the quantization tables used in these tests ... an encoder
//! which passes the test with a moderately coarse quantization table will not
//! be guaranteed to perform as well, with a finer quantization table".
//!
//! A quantiser value of zero is refused ([`JpegEncodeError::ZeroQuantiser`]):
//! B.2.4.1 gives `Qk` the range 1 to 255 at `Pq = 0`, and a zero is a division
//! the decoder cannot undo.
//!
//! ## An image whose dimensions are not a multiple of the MCU size
//!
//! **A.2.4, and its NOTE.** The clause requires the encoder to extend the
//! columns to complete the right-most blocks and the lines to complete the
//! bottom block-row, and — when the component is interleaved — to extend
//! further so the block count is a multiple of `Hi` and the block-row count a
//! multiple of `Vi`. The NOTE recommends how: "any incomplete MCUs be completed
//! by replication of the right-most column and the bottom line of each
//! component". That is what `pad_plane` does, and the last sentence of A.2.4
//! is why nothing else needs to happen: "Any sample added by an encoding
//! process to complete partial MCUs shall be removed by the decoding process",
//! so the padding never reaches a caller of the decoder.
//!
//! Replication is chosen over the two obvious alternatives for a reason that
//! outlives the NOTE. Zero-fill puts a step of up to 128 levels inside the edge
//! block, which costs bits in every AC coefficient and, after a lossy round
//! trip, rings back across the boundary into pixels the caller *can* see.
//! Mirroring costs no more than replication but is a different picture from the
//! one the standard recommends, for no gain. Replication puts a constant in the
//! padding, so the padding contributes to `S00` and to nothing else.
//!
//! # What adjudicates this, by name — and what does not
//!
//! ## Published tables, read twice
//!
//! `T-REC-T.81` — <https://www.w3.org/Graphics/JPEG/itu-t81.pdf>, the W3C's
//! copy of CCITT Rec. T.81 (1992) | ISO/IEC 10918-1 : 1993, fetched 15
//! September 2026 — was read **twice**, once out of the text layer `tpdf text`
//! extracts and once off `tpdf render`'s pages at 200 dpi with a face supplied.
//! The two readings agree on every one of the 604 entries the tables below are
//! built from:
//!
//! | Read | Entries | Where |
//! | --- | --- | --- |
//! | Figure A.6, the zig-zag sequence | 64 | rendered p. 30, text layer |
//! | Table K.1, luminance quantisation | 64 | rendered p. 143, text layer |
//! | Table K.2, chrominance quantisation | 64 | rendered p. 143, text layer |
//! | K.3.3.1, BITS and HUFFVAL for Tables K.3 and K.4 | 2 x (16 + 12) | rendered p. 158, text layer |
//! | K.3.3.2, BITS and HUFFVAL for Tables K.5 and K.6 | 2 x (16 + 162) | rendered pp. 158-159, text layer |
//!
//! The text layer needed the second reading rather than merely confirming the
//! first. T.81's tables are typeset with column rules that `tpdf text` emits as
//! a literal `1`, so Table K.1's first row arrives as `16111016124140151161`
//! and is only resolvable into `16 11 10 16 24 40 51 61` against the picture.
//! K.3.3's byte lists are running text and came out clean, which is why they,
//! and not Tables K.3 to K.6's typeset grids, are what
//! `the_huffman_tables_are_itu_t_t_81_annex_k_s` asserts
//! against.
//!
//! ## Published bytes, not a round trip
//!
//! Two of them, and both are output rather than input:
//!
//! - **Every DHT segment this writes is K.3.3's own byte list.** B.2.4.2's
//!   payload after `Tc`/`Th` is exactly BITS followed by HUFFVAL, so the bytes
//!   K.3.3.1 and K.3.3.2 print — `X'00010501010101010100000000000000'` and the
//!   rest — appear in the encoder's output verbatim. `a_grayscale_datastream_
//!   carries_annex_k_s_published_table_bytes` finds them there. A table
//!   transcribed wrong changes those bytes and the test fails; no decoder is
//!   involved on either side.
//! - **Two whole entropy-coded segments are derived from the published code
//!   words rather than from this crate.** Tables K.3 and K.5 print a code
//!   length and a binary code word per symbol, and for two images the coded
//!   bits follow from them and from A.3.3 with no implementation in the middle:
//!   a flat 8x8 mid-grey block is `DIFF = 0` then EOB, which K.3 category 0
//!   (`00`, 2 bits) and K.5 0/0 (`1010`, 4 bits) fix at `001010`, padded with
//!   1-bits per B.1.1.5 NOTE 1 to `X'2B'`; a flat block of 144 has
//!   `S00 = 8 x 16 = 128` by A.3.3 and `Sq = 128/16 = 8` by K.1, so category 4
//!   (`101`) then the four bits `1000` then EOB, giving `X'B15F'`. See
//!   `the_flat_block_datastreams_are_annex_k_s_own_code_words`.
//!
//! ## What is *not* adjudicated, said plainly
//!
//! - **ITU-T T.83 | ISO/IEC 10918-2, the compliance-test data published to
//!   adjudicate exactly this, could not be obtained, and it is not merely
//!   paywalled — the data is not in the document.** T.83 clause 4.4 says so
//!   itself: "The compliance test data for the encoder compliance tests and the
//!   generic decoder compliance tests are available on 3 diskettes and are
//!   included with the copy of this ITU-T Recommendation | ISO/IEC
//!   International Standard", "of the 1.4 M-byte high-density double-sided 96
//!   tracks per inch MS-DOS format". What was tried, on 15 September 2026 and
//!   again on 16 September with the same answers:
//!   `https://www.itu.int/rec/dologin_pub.asp?lang=e&id=T-REC-T.83-199411-I!!PDF-E&type=items`
//!   returns **HTTP 500** with a 1 208-byte IIS error body, to `curl` and to
//!   `WebFetch` alike, and the `-S` id returns HTTP 200 with the body
//!   `<title>Document Not Found</title>`;
//!   <https://www.itu.int/rec/T-REC-T.83/en> answers 200 and says the document
//!   "is only available through payment"; <https://www.iso.org/standard/20689.html>
//!   returns **HTTP 403** to `curl` and to `WebFetch` alike, so the price on it
//!   is not a number this file will quote. The Internet Archive holds nothing:
//!   its full-text search finds no copy, its availability API answered 429 on
//!   16 September, and a direct Wayback fetch of the preview below is a 404.
//!
//!   `https://cdn.standards.iteh.ai/samples/20689/c18c6efaec8c4aeab1373fd6e4f09ccf/ISO-IEC-10918-2-1995.pdf`
//!   **did** return a document, and it is the standards-preview extract, read
//!   here with `tpdf`: 15 PDF pages carrying the front matter and the
//!   Recommendation's **numbered pages 1 to 11**, ending mid-sentence in clause
//!   5.2.1. Clause 3's definitions, clause 4's rationale and clause 4.4 quoted
//!   above are in it. Its own contents page puts clause 6's encoder compliance
//!   tests on p. 19, Annex B's compliance quantisation tables on p. 28, Annex
//!   C's compressed test data stream structure on p. 30 and Annex E's
//!   greater-accuracy data on p. 53 — every one of them past where the preview
//!   stops. (An earlier draft of this header called the preview "11 of 58
//!   pages"; 58 is where Annex H *starts* in that contents list, not the
//!   document's length, and the number is struck rather than re-guessed.)
//!
//!   So **no published DCT vector set adjudicates the coefficients this
//!   produces.** `the_forward_dct_matches_a_3_3_s_equation`
//!   holds the fixed-point transform to A.3.3's equation recomputed in `f64` in
//!   the test, which is the standard's *formula* and not the standard's
//!   *numbers*; it is a real check on the implementation, and it is not T.83.
//!   The roadmap row and `docs/features/filters.md` say the same in the same
//!   words.
//!
//! - **The decoder beside this is not itself adjudicated by anything
//!   third-party, so "held to this decoder" is worth less than it sounds.**
//!   This was checked rather than assumed. Every fixture in `jpeg.rs`'s test
//!   module is built by that module's own `BitWriter` and `marker` helpers;
//!   `fuzz/corpus/jpeg/` is four files written out of those same fixtures;
//!   `crates/tinker-pdf/tests/cbz/source/page3.jpg` is 169 bytes and is one of
//!   them; `crates/tinker-pdf/tests/jpeg_census.rs` walks 10 606 real streams
//!   but counts *frame types* and never compares a pixel; and there is no JPEG
//!   equivalent of the PngSuite files that adjudicate `png.rs` in both
//!   directions. The round-trip tests here therefore say "round trip" in their
//!   own doc comments and claim nothing else.
//!
//! # The shape, and the two rulings that fix it
//!
//! [`JpegSource`] is [`crate::PngSource`] with a different colour enum, for the
//! reason `ccitt/encode.rs` gives at more length: a borrowed raster with an
//! explicit stride, plain numbers beside it, a free function returning
//! `Result<Vec<u8>, _>`, and a refusal enum whose variants are all a caller
//! mis-describing its own buffer. **Ruling 8** is satisfied — six integers, two
//! enums and a byte slice cross the boundary, and no COS type or PDF name is
//! among them. **Ruling 11** does not reach here: it makes `tinker_pdf` the
//! surface for a *document*, and a raster is not one; `png_encode` is projected
//! by the facade as `Bitmap::to_png` because a facade caller holds a rendered
//! page, and if one ever wants a JPEG of a page that projection is where it
//! belongs, not a re-export.
//!
//! # Who calls this
//!
//! **Nothing in this repository outside `jpeg.rs`'s own tests**, which is where
//! `ccitt/encode.rs` and `jbig2/encode.rs` also stand. The writer does not
//! re-encode image bytes by contract, and promoting a coder does not change a
//! contract. What would call it is a writer building a `/DCTDecode` image
//! XObject from a raster, and partial image redaction — both of which need a
//! decision about when a lossy coding is acceptable that belongs to whoever
//! writes them.

use super::ZIGZAG;

/// What the caller's interleaved raster holds.
///
/// Two, because these are the two whose interpretation is published: T.871
/// clause 7 says what three components mean and clause 8 what one does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JpegSourceColour {
    /// One byte per pixel, written as a single-component frame with no
    /// transform.
    Gray,
    /// Three bytes per pixel, R then G then B, written as T.871 clause 7's
    /// YCbCr with a JFIF APP0 segment saying so.
    Rgb,
}

impl JpegSourceColour {
    /// Bytes per pixel in the caller's raster.
    pub fn components(self) -> u8 {
        match self {
            JpegSourceColour::Gray => 1,
            JpegSourceColour::Rgb => 3,
        }
    }
}

/// The sampling factors written into SOF0, named by the caller.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum JpegSampling {
    /// Every component at `Hi = Vi = 1`: one 8 x 8 block each per MCU.
    #[default]
    FourFourFour,
    /// Luminance at `H = V = 2`, chrominance at 1: a 16 x 16 MCU of four
    /// luminance blocks and one of each chrominance. Refused for
    /// [`JpegSourceColour::Gray`], which has no chrominance to subsample.
    FourTwoZero,
}

/// Which quantisation tables to write.
///
/// The module header argues at length for why there is no 1-to-100 quality
/// number here.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum JpegQuantisation {
    /// T.81 Tables K.1 and K.2 exactly as printed.
    #[default]
    AnnexK,
    /// Those tables halved, rounding up, which is the one other setting T.81
    /// names: K.1's "if these quantization values are divided by 2, the
    /// resulting reconstructed image is usually nearly indistinguishable from
    /// the source image".
    AnnexKHalved,
    /// The caller's own tables, in natural row-major order — *not* zig-zag;
    /// the zig-zag B.2.4.1 requires in the DQT segment is applied here.
    ///
    /// Every element must be 1 to 255. A grayscale source uses `luminance` and
    /// ignores `chrominance`.
    Tables {
        luminance: [u8; 64],
        chrominance: [u8; 64],
    },
}

/// Everything about the encode that is not the pixels.
#[derive(Clone, Copy, Debug, Default)]
pub struct JpegOptions {
    pub quantisation: JpegQuantisation,
    pub sampling: JpegSampling,
    /// T.81 4.10's restart interval, in MCUs. Zero writes no DRI segment and
    /// no RST markers.
    pub restart_interval: u16,
}

/// An interleaved 8-bit raster, and where its rows begin.
///
/// Borrowed for [`crate::PngSource`]'s reason: the caller already holds the
/// pixels.
#[derive(Clone, Copy, Debug)]
pub struct JpegSource<'a> {
    pub width: u32,
    pub height: u32,
    pub colour: JpegSourceColour,
    /// Bytes from the start of one row to the start of the next. At least
    /// `width x colour.components()`; more means the buffer is padded and the
    /// tail of each row is not read.
    pub stride: usize,
    pub data: &'a [u8],
}

/// Why a raster could not be encoded. Every variant is the caller describing
/// its own buffer or its own intent wrongly.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JpegEncodeError {
    /// Zero, or past B.2.2's `X`/`Y` ceiling of 65 535.
    BadDimensions { width: u32, height: u32 },
    /// The stride is shorter than one row of pixels.
    ShortStride { stride: usize, row_bytes: u64 },
    /// The buffer ends before the last row does.
    ShortData { have: usize, need: u64 },
    /// A quantisation table element outside B.2.4.1's 1 to 255 at `Pq = 0`.
    ZeroQuantiser { chrominance: bool, index: usize },
    /// 4:2:0 asked of a one-component image, which has nothing to subsample.
    SubsampledGrayscale,
}

impl core::fmt::Display for JpegEncodeError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::BadDimensions { width, height } => {
                write!(f, "{width} x {height} is not an encodable image")
            }
            Self::ShortStride { stride, row_bytes } => {
                write!(f, "a stride of {stride} for rows of {row_bytes} bytes")
            }
            Self::ShortData { have, need } => {
                write!(f, "{have} bytes of raster, {need} needed")
            }
            Self::ZeroQuantiser { chrominance, index } => {
                let which = if *chrominance {
                    "chrominance"
                } else {
                    "luminance"
                };
                write!(f, "{which} quantiser element {index} is zero")
            }
            Self::SubsampledGrayscale => {
                write!(f, "4:2:0 asked of a single-component image")
            }
        }
    }
}

impl std::error::Error for JpegEncodeError {}

/// B.2.2's ceiling on `X` and `Y`, which are two bytes each.
const MAX_DIMENSION: u32 = 65_535;

/// T.81 Table K.1, the luminance quantisation table, in natural order.
///
/// Read twice from `T-REC-T.81`; see the module header's table.
pub(super) const ANNEX_K1_LUMINANCE: [u8; 64] = [
    16, 11, 10, 16, 24, 40, 51, 61, //
    12, 12, 14, 19, 26, 58, 60, 55, //
    14, 13, 16, 24, 40, 57, 69, 56, //
    14, 17, 22, 29, 51, 87, 80, 62, //
    18, 22, 37, 56, 68, 109, 103, 77, //
    24, 35, 55, 64, 81, 104, 113, 92, //
    49, 64, 78, 87, 103, 121, 120, 101, //
    72, 92, 95, 98, 112, 100, 103, 99,
];

/// T.81 Table K.2, the chrominance quantisation table, in natural order.
pub(super) const ANNEX_K2_CHROMINANCE: [u8; 64] = [
    17, 18, 24, 47, 99, 99, 99, 99, //
    18, 21, 26, 66, 99, 99, 99, 99, //
    24, 26, 56, 99, 99, 99, 99, 99, //
    47, 66, 99, 99, 99, 99, 99, 99, //
    99, 99, 99, 99, 99, 99, 99, 99, //
    99, 99, 99, 99, 99, 99, 99, 99, //
    99, 99, 99, 99, 99, 99, 99, 99, //
    99, 99, 99, 99, 99, 99, 99, 99,
];

/// A Huffman table as B.2.4.2 carries it: sixteen code-length counts, then the
/// values, longest-code last. These are literally K.3.3's published byte lists.
pub(super) struct HuffSpec {
    pub(super) bits: [u8; 16],
    pub(super) values: &'static [u8],
}

/// T.81 Table K.3, luminance DC. K.3.3.1's first byte list.
pub(super) const ANNEX_K3_DC_LUMA: HuffSpec = HuffSpec {
    bits: [0, 1, 5, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0, 0, 0],
    values: &[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11],
};

/// T.81 Table K.4, chrominance DC. K.3.3.1's second byte list.
pub(super) const ANNEX_K4_DC_CHROMA: HuffSpec = HuffSpec {
    bits: [0, 3, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0],
    values: &[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11],
};

/// T.81 Table K.5, luminance AC. K.3.3.2's first byte list, 162 values.
pub(super) const ANNEX_K5_AC_LUMA: HuffSpec = HuffSpec {
    bits: [0, 2, 1, 3, 3, 2, 4, 3, 5, 5, 4, 4, 0, 0, 1, 0x7D],
    values: &[
        0x01, 0x02, 0x03, 0x00, 0x04, 0x11, 0x05, 0x12, 0x21, 0x31, 0x41, 0x06, 0x13, 0x51, 0x61,
        0x07, 0x22, 0x71, 0x14, 0x32, 0x81, 0x91, 0xA1, 0x08, 0x23, 0x42, 0xB1, 0xC1, 0x15, 0x52,
        0xD1, 0xF0, 0x24, 0x33, 0x62, 0x72, 0x82, 0x09, 0x0A, 0x16, 0x17, 0x18, 0x19, 0x1A, 0x25,
        0x26, 0x27, 0x28, 0x29, 0x2A, 0x34, 0x35, 0x36, 0x37, 0x38, 0x39, 0x3A, 0x43, 0x44, 0x45,
        0x46, 0x47, 0x48, 0x49, 0x4A, 0x53, 0x54, 0x55, 0x56, 0x57, 0x58, 0x59, 0x5A, 0x63, 0x64,
        0x65, 0x66, 0x67, 0x68, 0x69, 0x6A, 0x73, 0x74, 0x75, 0x76, 0x77, 0x78, 0x79, 0x7A, 0x83,
        0x84, 0x85, 0x86, 0x87, 0x88, 0x89, 0x8A, 0x92, 0x93, 0x94, 0x95, 0x96, 0x97, 0x98, 0x99,
        0x9A, 0xA2, 0xA3, 0xA4, 0xA5, 0xA6, 0xA7, 0xA8, 0xA9, 0xAA, 0xB2, 0xB3, 0xB4, 0xB5, 0xB6,
        0xB7, 0xB8, 0xB9, 0xBA, 0xC2, 0xC3, 0xC4, 0xC5, 0xC6, 0xC7, 0xC8, 0xC9, 0xCA, 0xD2, 0xD3,
        0xD4, 0xD5, 0xD6, 0xD7, 0xD8, 0xD9, 0xDA, 0xE1, 0xE2, 0xE3, 0xE4, 0xE5, 0xE6, 0xE7, 0xE8,
        0xE9, 0xEA, 0xF1, 0xF2, 0xF3, 0xF4, 0xF5, 0xF6, 0xF7, 0xF8, 0xF9, 0xFA,
    ],
};

/// T.81 Table K.6, chrominance AC. K.3.3.2's second byte list, 162 values.
pub(super) const ANNEX_K6_AC_CHROMA: HuffSpec = HuffSpec {
    bits: [0, 2, 1, 2, 4, 4, 3, 4, 7, 5, 4, 4, 0, 1, 2, 0x77],
    values: &[
        0x00, 0x01, 0x02, 0x03, 0x11, 0x04, 0x05, 0x21, 0x31, 0x06, 0x12, 0x41, 0x51, 0x07, 0x61,
        0x71, 0x13, 0x22, 0x32, 0x81, 0x08, 0x14, 0x42, 0x91, 0xA1, 0xB1, 0xC1, 0x09, 0x23, 0x33,
        0x52, 0xF0, 0x15, 0x62, 0x72, 0xD1, 0x0A, 0x16, 0x24, 0x34, 0xE1, 0x25, 0xF1, 0x17, 0x18,
        0x19, 0x1A, 0x26, 0x27, 0x28, 0x29, 0x2A, 0x35, 0x36, 0x37, 0x38, 0x39, 0x3A, 0x43, 0x44,
        0x45, 0x46, 0x47, 0x48, 0x49, 0x4A, 0x53, 0x54, 0x55, 0x56, 0x57, 0x58, 0x59, 0x5A, 0x63,
        0x64, 0x65, 0x66, 0x67, 0x68, 0x69, 0x6A, 0x73, 0x74, 0x75, 0x76, 0x77, 0x78, 0x79, 0x7A,
        0x82, 0x83, 0x84, 0x85, 0x86, 0x87, 0x88, 0x89, 0x8A, 0x92, 0x93, 0x94, 0x95, 0x96, 0x97,
        0x98, 0x99, 0x9A, 0xA2, 0xA3, 0xA4, 0xA5, 0xA6, 0xA7, 0xA8, 0xA9, 0xAA, 0xB2, 0xB3, 0xB4,
        0xB5, 0xB6, 0xB7, 0xB8, 0xB9, 0xBA, 0xC2, 0xC3, 0xC4, 0xC5, 0xC6, 0xC7, 0xC8, 0xC9, 0xCA,
        0xD2, 0xD3, 0xD4, 0xD5, 0xD6, 0xD7, 0xD8, 0xD9, 0xDA, 0xE2, 0xE3, 0xE4, 0xE5, 0xE6, 0xE7,
        0xE8, 0xE9, 0xEA, 0xF2, 0xF3, 0xF4, 0xF5, 0xF6, 0xF7, 0xF8, 0xF9, 0xFA,
    ],
};

/// One symbol's code word: the code in the low `length` bits.
#[derive(Clone, Copy, Default)]
pub(super) struct Code {
    pub(super) bits: u16,
    pub(super) length: u8,
}

/// The canonical code words of a table, indexed by symbol value.
///
/// This is Annex C's procedure — Figures C.1 to C.3 — run forwards: codes are
/// assigned in increasing length, and within a length in HUFFVAL order,
/// starting at zero and shifting left at each length boundary. It is the same
/// assignment `HuffmanTable::build` inverts, written out here rather
/// than shared with it because the two want opposite indexings and a shared
/// intermediate would be a third representation to keep honest.
pub(super) fn canonical_codes(spec: &HuffSpec) -> [Code; 256] {
    let mut table = [Code::default(); 256];
    let mut code = 0u16;
    let mut index = 0usize;
    for length in 1..=16u8 {
        let count = usize::from(spec.bits.get(usize::from(length) - 1).copied().unwrap_or(0));
        for _ in 0..count {
            if let Some(&value) = spec.values.get(index) {
                if let Some(slot) = table.get_mut(usize::from(value)) {
                    *slot = Code { bits: code, length };
                }
            }
            index += 1;
            code = code.wrapping_add(1);
        }
        code <<= 1;
    }
    table
}

/// T.871 clause 7's exact forward transform, in 1/65536.
///
/// `[0..3]` are `0.299`, `0.587` and `0.114`; `[3]` is `1 / 1.772` and `[4]` is
/// `1 / 1.402`. Computed from the published decimals rather than written out,
/// for `COS_TABLE`'s reason: a constant that is derived cannot drift
/// from the formula it is supposed to be.
static YCBCR: std::sync::LazyLock<[i64; 5]> = std::sync::LazyLock::new(|| {
    let scale = 65_536.0f64;
    [
        (0.299 * scale).round() as i64,
        (0.587 * scale).round() as i64,
        (0.114 * scale).round() as i64,
        (scale / 1.772).round() as i64,
        (scale / 1.402).round() as i64,
    ]
});

/// Rounds `value / 2^bits` to nearest, ties away from zero.
fn round_shift(value: i64, bits: u32) -> i64 {
    let half = 1i64 << (bits - 1);
    if value >= 0 {
        (value + half) >> bits
    } else {
        -((-value + half) >> bits)
    }
}

/// Rounds `value / divisor` to nearest, ties away from zero. `divisor > 0`.
fn round_div(value: i64, divisor: i64) -> i64 {
    if value >= 0 {
        (value + divisor / 2) / divisor
    } else {
        -((-value + divisor / 2) / divisor)
    }
}

/// One pixel's `(Y, Cb, Cr)` by T.871 clause 7.
pub(super) fn rgb_to_ycbcr(r: u8, g: u8, b: u8) -> (u8, u8, u8) {
    let k = &*YCBCR;
    let (kr, kg, kb) = (
        k.first().copied().unwrap_or(0),
        k.get(1).copied().unwrap_or(0),
        k.get(2).copied().unwrap_or(0),
    );
    // `luma` is Y at 1/65536, unrounded: the two chrominance equations are
    // built on the unrounded Y, which is what makes them the exact form rather
    // than the four-decimal one.
    let luma = kr * i64::from(r) + kg * i64::from(g) + kb * i64::from(b);
    let y = round_shift(luma, 16).clamp(0, 255) as u8;

    let cb_num = (i64::from(b) << 16) - luma;
    let cr_num = (i64::from(r) << 16) - luma;
    let cb = (round_shift(cb_num * k.get(3).copied().unwrap_or(0), 32) + 128).clamp(0, 255) as u8;
    let cr = (round_shift(cr_num * k.get(4).copied().unwrap_or(0), 32) + 128).clamp(0, 255) as u8;
    (y, cb, cr)
}

/// A.3.3's FDCT followed by A.3.4's quantisation, in one rounding.
///
/// `samples` are already level-shifted by A.3.1. The output is in **zig-zag
/// order** — `out[k]` is the coefficient A.6's scan visits `k`th — which is the
/// order F.1.2 codes in and the order `finish` reads back.
///
/// The cosines are `COS_TABLE`, which already carries `C(u)/2`, so
/// `S_vu` is the plain separable product: `(1/4) Cu Cv` is `(Cu/2)(Cv/2)`. Two
/// passes at 1/16384 leave the sum at 1/2^28, and the quantiser divides that
/// once rather than twice, so there is exactly one rounding between the
/// equation and the coded integer.
pub(super) fn forward_dct_quantise(samples: &[i32; 64], quant: &[u8; 64], out: &mut [i32; 64]) {
    let cos = &*super::COS_TABLE;

    // Pass one, over x: `rows[y][u] = sum_x s_yx T[x][u]`, at 1/16384.
    let mut rows = [0i64; 64];
    for y in 0..8usize {
        for u in 0..8usize {
            let mut sum = 0i64;
            for x in 0..8usize {
                let sample = samples.get(y * 8 + x).copied().unwrap_or(0);
                if sample == 0 {
                    continue;
                }
                let c = cos.get(x * 8 + u).copied().unwrap_or(0);
                sum += i64::from(sample) * i64::from(c);
            }
            if let Some(slot) = rows.get_mut(y * 8 + u) {
                *slot = sum;
            }
        }
    }

    // Pass two, over y, straight into zig-zag order and through the quantiser.
    for (k, &natural) in ZIGZAG.iter().enumerate() {
        let (v, u) = (natural / 8, natural % 8);
        let mut sum = 0i64;
        for y in 0..8usize {
            let value = rows.get(y * 8 + u).copied().unwrap_or(0);
            if value == 0 {
                continue;
            }
            let c = cos.get(y * 8 + v).copied().unwrap_or(0);
            sum += value * i64::from(c);
        }
        // 2^28 from the two 1/16384 passes, times the quantiser. `q` is at
        // least 1: `quantisation_tables` refuses a zero before anything is
        // coded.
        let q = i64::from(quant.get(natural).copied().unwrap_or(1)).max(1);
        let coefficient = round_div(sum, q << 28);
        if let Some(slot) = out.get_mut(k) {
            // Clamped to what Annex K's tables can code. **This clamp is
            // unreachable**: `no_quantiser_can_push_a_coefficient_past_annex_k`
            // enumerates the worst-case sample pattern for every one of the 64
            // coefficients at the finest legal quantiser and finds the largest
            // magnitude is 1 020 for an AC coefficient and 1 024 for the DC
            // one, inside K.5's size 10 and K.3's category 11. It is here so
            // that a later 12-bit path degrades to a coarse coefficient rather
            // than to a code word that does not exist (ruling 1).
            *slot = coefficient.clamp(-32_767, 32_767) as i32;
        }
    }
}

/// The magnitude category of F.1.2.1.1 and Table F.1: how many bits `|value|`
/// needs. Zero for zero.
pub(super) fn category(value: i32) -> u8 {
    if value == 0 {
        0
    } else {
        (32 - value.unsigned_abs().leading_zeros()) as u8
    }
}

/// F.1.2.1.1's additional bits: the low `size` bits of `value` when it is
/// positive, of `value - 1` when it is negative.
fn magnitude_bits(value: i32, size: u8) -> u16 {
    let raw = if value >= 0 { value } else { value - 1 };
    let mask = if size >= 16 {
        0xFFFFu32
    } else {
        (1u32 << size) - 1
    };
    ((raw as u32) & mask) as u16
}

/// Accumulates entropy-coded bits, stuffing per F.1.2.3 as it goes.
struct BitWriter {
    out: Vec<u8>,
    accumulator: u32,
    count: u32,
}

impl BitWriter {
    fn new() -> BitWriter {
        BitWriter {
            out: Vec::new(),
            accumulator: 0,
            count: 0,
        }
    }

    fn push(&mut self, bits: u16, length: u8) {
        for i in (0..length).rev() {
            let bit = (u32::from(bits) >> i) & 1;
            self.accumulator = (self.accumulator << 1) | bit;
            self.count += 1;
            if self.count == 8 {
                let byte = (self.accumulator & 0xFF) as u8;
                self.out.push(byte);
                // F.1.2.3: an X'FF' in the entropy-coded segment is followed by
                // a stuffed zero, so that no marker can be read out of it.
                if byte == 0xFF {
                    self.out.push(0x00);
                }
                self.accumulator = 0;
                self.count = 0;
            }
        }
    }

    fn code(&mut self, code: Code) {
        self.push(code.bits, code.length);
    }

    /// B.1.1.5 NOTE 1: pad the final byte of a segment with 1-bits. A byte of
    /// all ones so produced is itself stuffed, which [`Self::push`] does.
    fn pad_to_byte(&mut self) {
        while self.count != 0 {
            self.push(1, 1);
        }
    }
}

/// One component's plane, padded to whole blocks, plus its sampling factors.
struct Plane {
    /// Component identifier written into SOF0 and SOS.
    id: u8,
    h: usize,
    v: usize,
    /// Quantisation and Huffman destination: 0 for luminance, 1 otherwise.
    table: u8,
    width: usize,
    data: Vec<u8>,
}

/// A.2.4's completion of partial MCUs, by the NOTE's replication.
///
/// The source occupies the top-left `src_w x src_h` of a `dst_w x dst_h` plane;
/// every column past `src_w` repeats column `src_w - 1`, and every row past
/// `src_h` repeats row `src_h - 1`. Corner samples therefore come from the
/// bottom-right source pixel, which is what replicating a column and then a
/// line gives.
pub(super) fn pad_plane(data: &mut [u8], dst_w: usize, dst_h: usize, src_w: usize, src_h: usize) {
    if src_w == 0 || src_h == 0 {
        return;
    }
    for y in 0..src_h {
        let edge = data.get(y * dst_w + src_w - 1).copied().unwrap_or(0);
        for x in src_w..dst_w {
            if let Some(slot) = data.get_mut(y * dst_w + x) {
                *slot = edge;
            }
        }
    }
    for y in src_h..dst_h {
        // The bottom line is the last *source* line, already column-padded.
        let (head, tail) = data.split_at_mut(y * dst_w);
        let Some(source) = head.get((src_h - 1) * dst_w..(src_h - 1) * dst_w + dst_w) else {
            continue;
        };
        if let Some(row) = tail.get_mut(..dst_w) {
            row.copy_from_slice(source);
        }
    }
}

/// The two tables, natural order, checked against B.2.4.1's range.
fn quantisation_tables(
    quantisation: &JpegQuantisation,
) -> Result<([u8; 64], [u8; 64]), JpegEncodeError> {
    let halve = |table: &[u8; 64]| {
        let mut out = [0u8; 64];
        for (slot, &value) in out.iter_mut().zip(table.iter()) {
            // Round up, so no element can reach zero from a legal one.
            *slot = value.div_ceil(2);
        }
        out
    };
    let (luma, chroma) = match quantisation {
        JpegQuantisation::AnnexK => (ANNEX_K1_LUMINANCE, ANNEX_K2_CHROMINANCE),
        JpegQuantisation::AnnexKHalved => {
            (halve(&ANNEX_K1_LUMINANCE), halve(&ANNEX_K2_CHROMINANCE))
        }
        JpegQuantisation::Tables {
            luminance,
            chrominance,
        } => (*luminance, *chrominance),
    };
    for (index, &value) in luma.iter().enumerate() {
        if value == 0 {
            return Err(JpegEncodeError::ZeroQuantiser {
                chrominance: false,
                index,
            });
        }
    }
    for (index, &value) in chroma.iter().enumerate() {
        if value == 0 {
            return Err(JpegEncodeError::ZeroQuantiser {
                chrominance: true,
                index,
            });
        }
    }
    Ok((luma, chroma))
}

fn marker(out: &mut Vec<u8>, code: u8, body: &[u8]) {
    out.push(0xFF);
    out.push(code);
    let length = body.len() + 2;
    out.push((length >> 8) as u8);
    out.push((length & 0xFF) as u8);
    out.extend_from_slice(body);
}

/// B.2.4.1's DQT: `Pq = 0`, `Tq`, then 64 elements **in zig-zag order**.
fn dqt(out: &mut Vec<u8>, destination: u8, table: &[u8; 64]) {
    let mut body = Vec::with_capacity(65);
    body.push(destination & 0x0F);
    for &natural in ZIGZAG.iter() {
        body.push(table.get(natural).copied().unwrap_or(1));
    }
    marker(out, 0xDB, &body);
}

/// B.2.4.2's DHT: `Tc`/`Th`, then BITS, then HUFFVAL — which for these tables
/// is K.3.3's published byte list verbatim.
fn dht(out: &mut Vec<u8>, class: u8, destination: u8, spec: &HuffSpec) {
    let mut body = Vec::with_capacity(1 + 16 + spec.values.len());
    body.push(((class & 0x0F) << 4) | (destination & 0x0F));
    body.extend_from_slice(&spec.bits);
    body.extend_from_slice(spec.values);
    marker(out, 0xC4, &body);
}

/// Writes one complete baseline JPEG interchange datastream.
///
/// # Errors
/// Any [`JpegEncodeError`]; all of them are a caller describing its own buffer
/// or its own intent wrongly, and none can arise from the pixels.
pub fn jpeg_encode(
    source: &JpegSource<'_>,
    options: &JpegOptions,
) -> Result<Vec<u8>, JpegEncodeError> {
    let (width, height) = (source.width, source.height);
    if width == 0 || height == 0 || width > MAX_DIMENSION || height > MAX_DIMENSION {
        return Err(JpegEncodeError::BadDimensions { width, height });
    }
    if options.sampling == JpegSampling::FourTwoZero && source.colour == JpegSourceColour::Gray {
        return Err(JpegEncodeError::SubsampledGrayscale);
    }

    // In `u64` throughout and compared before anything is converted, for
    // `png_encode`'s reason: on a 32-bit target `width x components` overflows
    // a `usize` while still being a width B.2.2 permits.
    let components = u64::from(source.colour.components());
    let row_bytes = u64::from(width) * components;
    if (source.stride as u64) < row_bytes {
        return Err(JpegEncodeError::ShortStride {
            stride: source.stride,
            row_bytes,
        });
    }
    let need = (source.stride as u64)
        .saturating_mul(u64::from(height) - 1)
        .saturating_add(row_bytes);
    if (source.data.len() as u64) < need {
        return Err(JpegEncodeError::ShortData {
            have: source.data.len(),
            need,
        });
    }

    let (luma_quant, chroma_quant) = quantisation_tables(&options.quantisation)?;

    let (mcu_w, mcu_h) = match options.sampling {
        JpegSampling::FourFourFour => (8usize, 8usize),
        JpegSampling::FourTwoZero => (16usize, 16usize),
    };
    let src_w = width as usize;
    let src_h = height as usize;
    let padded_w = src_w.div_ceil(mcu_w) * mcu_w;
    let padded_h = src_h.div_ceil(mcu_h) * mcu_h;

    let planes = build_planes(source, options, padded_w, padded_h, src_w, src_h);

    let mut out = Vec::new();
    out.extend_from_slice(&[0xFF, 0xD8]); // SOI

    // T.871 clause 10.1's APP0, written only where it says something: a
    // three-component frame whose components are that Recommendation's YCbCr.
    // Version 1.01, no density units, no thumbnail.
    if source.colour == JpegSourceColour::Rgb {
        marker(
            &mut out,
            0xE0,
            &[
                b'J', b'F', b'I', b'F', 0x00, // identifier
                0x01, 0x01, // version 1.01
                0x00, // units: none, the ratio only
                0x00, 0x01, 0x00, 0x01, // 1 : 1
                0x00, 0x00, // no thumbnail
            ],
        );
    }

    dqt(&mut out, 0, &luma_quant);
    if source.colour == JpegSourceColour::Rgb {
        dqt(&mut out, 1, &chroma_quant);
    }

    // B.2.2's SOF0.
    let mut frame = Vec::with_capacity(6 + 3 * planes.len());
    frame.push(8); // P
    frame.push((height >> 8) as u8);
    frame.push((height & 0xFF) as u8);
    frame.push((width >> 8) as u8);
    frame.push((width & 0xFF) as u8);
    frame.push(planes.len() as u8);
    for plane in &planes {
        frame.push(plane.id);
        frame.push(((plane.h as u8) << 4) | (plane.v as u8));
        frame.push(plane.table);
    }
    marker(&mut out, 0xC0, &frame);

    dht(&mut out, 0, 0, &ANNEX_K3_DC_LUMA);
    dht(&mut out, 1, 0, &ANNEX_K5_AC_LUMA);
    if source.colour == JpegSourceColour::Rgb {
        dht(&mut out, 0, 1, &ANNEX_K4_DC_CHROMA);
        dht(&mut out, 1, 1, &ANNEX_K6_AC_CHROMA);
    }

    if options.restart_interval != 0 {
        marker(
            &mut out,
            0xDD,
            &[
                (options.restart_interval >> 8) as u8,
                (options.restart_interval & 0xFF) as u8,
            ],
        );
    }

    // B.2.3's SOS. One scan, every component, the whole spectrum, no
    // successive approximation -- which is what makes it baseline.
    let mut scan = Vec::with_capacity(4 + 2 * planes.len());
    scan.push(planes.len() as u8);
    for plane in &planes {
        scan.push(plane.id);
        scan.push((plane.table << 4) | plane.table);
    }
    scan.push(0); // Ss
    scan.push(63); // Se
    scan.push(0); // Ah / Al
    marker(&mut out, 0xDA, &scan);

    let dc_codes = [
        canonical_codes(&ANNEX_K3_DC_LUMA),
        canonical_codes(&ANNEX_K4_DC_CHROMA),
    ];
    let ac_codes = [
        canonical_codes(&ANNEX_K5_AC_LUMA),
        canonical_codes(&ANNEX_K6_AC_CHROMA),
    ];

    let mut writer = BitWriter::new();
    let mut predictors = [0i32; 3];
    let mut samples = [0i32; 64];
    let mut coefficients = [0i32; 64];
    let mcus_x = padded_w / mcu_w;
    let mcus_y = padded_h / mcu_h;
    let mut restarts = 0u32;

    for mcu in 0..(mcus_x * mcus_y) {
        if options.restart_interval != 0
            && mcu != 0
            && mcu % usize::from(options.restart_interval) == 0
        {
            // F.1.2.3 and 4.10: the segment ends on a byte boundary, the marker
            // follows, and every DC predictor goes back to zero.
            writer.pad_to_byte();
            writer.out.push(0xFF);
            writer.out.push(0xD0 + (restarts % 8) as u8);
            restarts += 1;
            predictors = [0i32; 3];
        }

        let (mx, my) = (mcu % mcus_x, mcu / mcus_x);
        for (ci, plane) in planes.iter().enumerate() {
            let quant = if plane.table == 0 {
                &luma_quant
            } else {
                &chroma_quant
            };
            for bv in 0..plane.v {
                for bh in 0..plane.h {
                    let x0 = (mx * plane.h + bh) * 8;
                    let y0 = (my * plane.v + bv) * 8;
                    for y in 0..8usize {
                        for x in 0..8usize {
                            let value = plane
                                .data
                                .get((y0 + y) * plane.width + x0 + x)
                                .copied()
                                .unwrap_or(128);
                            if let Some(slot) = samples.get_mut(y * 8 + x) {
                                // A.3.1's level shift, `2^(P-1)` with `P = 8`.
                                *slot = i32::from(value) - 128;
                            }
                        }
                    }
                    forward_dct_quantise(&samples, quant, &mut coefficients);

                    let destination = usize::from(plane.table).min(1);
                    let dc = dc_codes.get(destination);
                    let ac = ac_codes.get(destination);
                    let predictor = predictors.get(ci).copied().unwrap_or(0);
                    let dc_value = coefficients.first().copied().unwrap_or(0);
                    if let Some(slot) = predictors.get_mut(ci) {
                        *slot = dc_value;
                    }
                    encode_block(&mut writer, &coefficients, dc_value - predictor, dc, ac);
                }
            }
        }
    }

    writer.pad_to_byte();
    out.extend_from_slice(&writer.out);
    out.extend_from_slice(&[0xFF, 0xD9]); // EOI
    Ok(out)
}

/// F.1.2's coding of one block: the DC difference, then the AC coefficients in
/// zig-zag order as run/size pairs.
fn encode_block(
    writer: &mut BitWriter,
    coefficients: &[i32; 64],
    diff: i32,
    dc: Option<&[Code; 256]>,
    ac: Option<&[Code; 256]>,
) {
    let Some(dc) = dc else { return };
    let Some(ac) = ac else { return };

    let size = category(diff);
    if let Some(&code) = dc.get(usize::from(size)) {
        writer.code(code);
    }
    if size != 0 {
        writer.push(magnitude_bits(diff, size), size);
    }

    let mut run = 0u8;
    for k in 1..64usize {
        let value = coefficients.get(k).copied().unwrap_or(0);
        if value == 0 {
            run += 1;
            continue;
        }
        // F.1.2.2.1: X'F0' is ZRL, sixteen zeros with no coefficient after it.
        while run > 15 {
            if let Some(&code) = ac.get(0xF0) {
                writer.code(code);
            }
            run -= 16;
        }
        let size = category(value);
        if let Some(&code) = ac.get(usize::from((run << 4) | (size & 0x0F))) {
            writer.code(code);
        }
        writer.push(magnitude_bits(value, size), size);
        run = 0;
    }
    // F.1.2.2.1: EOB, when every remaining coefficient is zero.
    if run > 0 {
        if let Some(&code) = ac.first() {
            writer.code(code);
        }
    }
}

/// The component planes, colour-transformed, padded by A.2.4 and subsampled if
/// the caller asked.
fn build_planes(
    source: &JpegSource<'_>,
    options: &JpegOptions,
    padded_w: usize,
    padded_h: usize,
    src_w: usize,
    src_h: usize,
) -> Vec<Plane> {
    let stride = source.stride;
    match source.colour {
        JpegSourceColour::Gray => {
            let mut data = vec![0u8; padded_w * padded_h];
            for y in 0..src_h {
                for x in 0..src_w {
                    let value = source.data.get(y * stride + x).copied().unwrap_or(0);
                    if let Some(slot) = data.get_mut(y * padded_w + x) {
                        *slot = value;
                    }
                }
            }
            pad_plane(&mut data, padded_w, padded_h, src_w, src_h);
            vec![Plane {
                id: 1,
                h: 1,
                v: 1,
                table: 0,
                width: padded_w,
                data,
            }]
        }
        JpegSourceColour::Rgb => {
            let mut luma = vec![0u8; padded_w * padded_h];
            let mut cb = vec![0u8; padded_w * padded_h];
            let mut cr = vec![0u8; padded_w * padded_h];
            for y in 0..src_h {
                for x in 0..src_w {
                    let at = y * stride + x * 3;
                    let (y0, cb0, cr0) = rgb_to_ycbcr(
                        source.data.get(at).copied().unwrap_or(0),
                        source.data.get(at + 1).copied().unwrap_or(0),
                        source.data.get(at + 2).copied().unwrap_or(0),
                    );
                    let at = y * padded_w + x;
                    if let Some(slot) = luma.get_mut(at) {
                        *slot = y0;
                    }
                    if let Some(slot) = cb.get_mut(at) {
                        *slot = cb0;
                    }
                    if let Some(slot) = cr.get_mut(at) {
                        *slot = cr0;
                    }
                }
            }
            // Padded *before* subsampling, so the box filter never averages a
            // written sample with an unwritten zero.
            for plane in [&mut luma, &mut cb, &mut cr] {
                pad_plane(plane, padded_w, padded_h, src_w, src_h);
            }

            match options.sampling {
                JpegSampling::FourFourFour => vec![
                    Plane {
                        id: 1,
                        h: 1,
                        v: 1,
                        table: 0,
                        width: padded_w,
                        data: luma,
                    },
                    Plane {
                        id: 2,
                        h: 1,
                        v: 1,
                        table: 1,
                        width: padded_w,
                        data: cb,
                    },
                    Plane {
                        id: 3,
                        h: 1,
                        v: 1,
                        table: 1,
                        width: padded_w,
                        data: cr,
                    },
                ],
                JpegSampling::FourTwoZero => {
                    let (half_w, half_h) = (padded_w / 2, padded_h / 2);
                    let shrink = |plane: &[u8]| {
                        let mut out = vec![0u8; half_w * half_h];
                        for y in 0..half_h {
                            for x in 0..half_w {
                                // T.871 clause 9 puts a subsampled sample at the
                                // centre of the samples it covers; the average of
                                // the four is the sample at that centre.
                                let mut sum = 0u32;
                                for dy in 0..2usize {
                                    for dx in 0..2usize {
                                        sum += u32::from(
                                            plane
                                                .get((y * 2 + dy) * padded_w + x * 2 + dx)
                                                .copied()
                                                .unwrap_or(128),
                                        );
                                    }
                                }
                                if let Some(slot) = out.get_mut(y * half_w + x) {
                                    *slot = ((sum + 2) / 4) as u8;
                                }
                            }
                        }
                        out
                    };
                    vec![
                        Plane {
                            id: 1,
                            h: 2,
                            v: 2,
                            table: 0,
                            width: padded_w,
                            data: luma,
                        },
                        Plane {
                            id: 2,
                            h: 1,
                            v: 1,
                            table: 1,
                            width: half_w,
                            data: shrink(&cb),
                        },
                        Plane {
                            id: 3,
                            h: 1,
                            v: 1,
                            table: 1,
                            width: half_w,
                            data: shrink(&cr),
                        },
                    ]
                }
            }
        }
    }
}
