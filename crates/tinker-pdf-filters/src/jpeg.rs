//! JPEG decoding (DCTDecode, 7.4.8; ITU-T T.81).
//!
//! Baseline, extended sequential and **progressive**, each with either entropy
//! coder — Annex C's Huffman codes or Annex D's arithmetic ones — at 8 or 12
//! bits. That is SOF0, SOF1, SOF2, SOF9 and SOF10. The lossless frames of
//! Annex H and the differential frames of Annex J are reported rather than
//! half-decoded, each by the annex it needs.
//!
//! # The arithmetic frames, and what stands behind them
//!
//! SOF9 and SOF10 landed in September 2026. The coder is `crate::qm`, which
//! T.81 K.4.1's published test sequence adjudicates outright. The statistical
//! models that pick its contexts are in [`arith`], and **nothing published
//! adjudicates those** — the search that establishes it is below, because a
//! failed fixture hunt that is not written down gets repeated.
//!
//! **What was searched, on 20 September 2026.** *T.81 itself* —
//! <https://www.w3.org/Graphics/JPEG/itu-t81.pdf>, the W3C's copy of CCITT
//! Rec. T.81 (1992) | ISO/IEC 10918-1 : 1993, **HTTP 200** to `curl` that day,
//! 1 058 883 bytes, SHA-256
//! `631031d4ba56b06abee3e312a0f235b9422da9c7267d1c8f7604418795768bf0`, byte for
//! byte the file `encode.rs`'s header names from 15 September. (That header
//! records a 403 from `curl`; it answers 200 now, which is what bot protection
//! does and is the fifth time a "not obtainable" here has needed retesting.)
//! Searched: the contents list, then Annex K section by section. K.1 and K.2 are quantisation
//! tables; K.3 is Huffman tables and K.3.3 their byte lists; **K.4.1 is the
//! arithmetic coder test sequence — 256 decisions, the 32 bytes they encode
//! to, and Tables K.7 and K.8's symbol-by-symbol encoder and decoder traces**;
//! K.4 has no other subsection; K.5 to K.10 are downsampling filters,
//! applicability guidance, AC prediction and a point-transform example, with
//! no data. A sweep of the whole document for hexadecimal runs finds three
//! places only: K.3.3's Huffman byte lists, K.4.1's two sequences, and
//! `X'FFFF0000'` inside D.1. **So T.81 publishes an arithmetic *coder* vector
//! and no arithmetic-coded *image*.**
//!
//! *The other two parts*: T.83 | ISO/IEC 10918-2 is the compliance-testing
//! document and its clause 4.4 says its test data "are available on 3
//! diskettes and are included with the copy of this ITU-T Recommendation"
//! rather than inside it; T.84 | ISO/IEC 10918-3, fetched whole on 20
//! September 2026, says in clause 4.2.1 that compliance test data is
//! "available from ISO and ITU to parties who wish to determine compliance"
//! and its Annex G specifies the *structure* of those streams without
//! printing one. URLs and what each returned are in `encode.rs`'s header for
//! T.83 and in [`arith`]'s for the rest.
//!
//! So the models are transcribed from F.1.4.4, Tables F.4 and F.5, and Table
//! G.2, checked against the document a second time, and pinned against
//! hand-derived decision sequences. [`arith`] says what that is worth.
//!
//! Pixels are made a strip at a time — one MCU row of each component
//! dequantised, transformed, replicated and colour-converted — and handed to a
//! [`JpegSink`] or painted into the image ([`decode_strips`], [`decode`];
//! `docs/design/jpeg-speed.md`). Most frames decode every scan into one
//! per-component coefficient buffer covering the whole image and paint it at
//! the end. Progressive forces that shape — a coefficient is refined by later
//! scans, so nothing can be turned into a pixel until the last scan has been
//! read — and so does a sequential frame in several scans. **A sequential
//! frame whose components all arrive in its one scan streams**: it is
//! entropy-decoded one MCU row at a time into one MCU row of coefficients,
//! each row painted as it completes, when nothing after the scan changes what
//! the end of the decode reads ([`streamable`], [`the_rest_is_inert`]). The
//! two paths cannot drift apart unseen: the test build decodes every
//! sequential stream both ways and holds them equal.
//!
//! **A strip decode takes a scale** ([`JpegScale`]). At 1/2, 1/4 and 1/8 each
//! block's top-left 4 × 4, 2 × 2 or 1 × 1 coefficients are transformed by the
//! reduced transforms ([`reduced_block`]) into an image of `⌈W·N/8⌉` by
//! `⌈H·N/8⌉`, a subsampled component reduced by the same factor before it is
//! replicated, for every frame type, both coders and both precisions. A block
//! whose kept coefficients have no AC term, and every block at 1/8, takes the
//! full transform's DC level ([`dc_level`]), so a flat block is the same level
//! at every scale. [`decode`] is the full-size decode.
//!
//! The IDCT is the integer separable transform. That means output can differ
//! from libjpeg's by a least-significant bit on some coefficients — there is no
//! single correct IDCT, only conforming ones — so comparison against a
//! reference is perceptual, never exact.
//!
//! The encoder is `encode`, which writes baseline and only baseline; what it
//! excludes, what adjudicates it, and what does not, are that module's header.

mod arith;
#[cfg(test)]
mod direct;
mod encode;
#[cfg(test)]
mod streams;

pub use encode::{
    jpeg_encode, JpegEncodeError, JpegOptions, JpegQuantisation, JpegSampling, JpegSource,
    JpegSourceColour,
};

use crate::qm::QmDecoder;
use crate::Warning;

/// What colour the decoded components represent.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JpegColor {
    /// One component.
    Gray,
    /// Three components, already converted from YCbCr.
    Rgb,
    /// Four components. Adobe's transform, if any, has been undone.
    Cmyk,
    /// Four components, and the file marked them inverted — the Photoshop
    /// convention that trips readers which assume otherwise.
    CmykInverted,
}

/// A decoded image.
#[derive(Clone, Debug)]
pub struct JpegImage {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// What the components mean.
    pub color: JpegColor,
    /// Interleaved samples, one byte each.
    ///
    /// **Eight bits whatever the frame's precision was.** A 12-bit frame is
    /// decoded at twelve and narrowed here, because every `PixelFormat` this
    /// engine rasters into is eight bits deep and a 16-bit sample path would
    /// be a change to the raster rather than to this decoder. The narrowing is
    /// reported as [`Warning::JpegPrecisionNarrowed`] and [`JpegImage::precision`]
    /// says what it came from, so a later caller that grows a wider path knows
    /// where to look.
    pub data: Vec<u8>,
    /// T.81 B.2.2's `P`: the frame's sample precision, 8 or 12.
    pub precision: u8,
    /// What the decoder tolerated.
    pub warnings: Vec<Warning>,
}

impl JpegColor {
    /// Samples a pixel has: one, three or four.
    #[must_use]
    pub fn components(self) -> usize {
        match self {
            JpegColor::Gray => 1,
            JpegColor::Rgb => 3,
            JpegColor::Cmyk | JpegColor::CmykInverted => 4,
        }
    }
}

/// The size a strip decode makes its image at: the frame's own, or a half, a
/// quarter or an eighth of it each way, reduced in the DCT domain
/// (`docs/design/jpeg-speed.md`, "The reduced transforms").
///
/// At `N/8` each block's top-left `N × N` coefficients are transformed into
/// `N × N` samples, so a `W × H` frame gives a `⌈W·N/8⌉ × ⌈H·N/8⌉` image whose
/// sample `X` covers the frame's columns `8X/N` up to `8(X+1)/N`, the last cut
/// where the frame ends.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum JpegScale {
    /// Every sample: `N` is 8.
    #[default]
    Full,
    /// Four samples a block side, from its top-left 4 × 4 coefficients.
    Half,
    /// Two samples a block side, from its top-left 2 × 2 coefficients.
    Quarter,
    /// One sample a block: its DC level.
    Eighth,
}

impl JpegScale {
    /// Every scale, the full size first and then each smaller one: the order
    /// the `jpeg` fuzz target's input length picks them in.
    pub const ALL: [JpegScale; 4] = [
        JpegScale::Full,
        JpegScale::Half,
        JpegScale::Quarter,
        JpegScale::Eighth,
    ];

    /// `N` of `N/8`: the samples a block gives along each side.
    #[must_use]
    pub fn block_samples(self) -> u32 {
        match self {
            JpegScale::Full => 8,
            JpegScale::Half => 4,
            JpegScale::Quarter => 2,
            JpegScale::Eighth => 1,
        }
    }

    /// A length of `full` samples at this scale: `⌈full·N/8⌉`.
    #[must_use]
    pub fn length(self, full: u32) -> u32 {
        let scaled = (u64::from(full) * u64::from(self.block_samples())).div_ceil(8);
        // At most `full`, since `N` is at most 8.
        u32::try_from(scaled).unwrap_or(full)
    }
}

/// A decoded image's geometry and colour: what a [`JpegSink`] is told before
/// its first strip.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct JpegLayout {
    /// Width in pixels: the frame's at full size, `⌈X·N/8⌉` of its `X` at
    /// `N/8`.
    pub width: u32,
    /// Height in pixels, likewise.
    pub height: u32,
    /// What the components mean.
    pub color: JpegColor,
    /// T.81 B.2.2's `P`: the frame's sample precision, 8 or 12. The samples
    /// are eight bits whatever it was, as [`JpegImage::data`]'s are.
    pub precision: u8,
    /// The scale the image is decoded at.
    pub scale: JpegScale,
    /// T.81 B.2.2's `X`: the frame's own width, which the image covers at
    /// every scale, its last column cut where the frame ends.
    pub full_width: u32,
    /// T.81 B.2.2's `Y`: the frame's own height, likewise.
    pub full_height: u32,
}

/// Where [`decode_strips`] hands a decoded image, a strip at a time.
///
/// A strip is one MCU row of the image — `N · Vmax` rows at the scale's `N`,
/// `8 · Vmax` at full size, the last cut at the image's height — and the
/// strips arrive top to bottom.
pub trait JpegSink {
    /// The image's geometry and colour: once, before the first strip.
    fn begin(&mut self, layout: JpegLayout);
    /// Rows `y` to `y + rows` of the image, whole: interleaved samples, one
    /// byte each, as [`JpegImage::data`]'s rows hold them.
    fn strip(&mut self, y: u32, rows: u32, data: &[u8]);
}

/// The bytes a strip decode held at once in buffers sized from the image,
/// beside the image it handed out.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct JpegHeld {
    /// Coefficients: the whole image's, or one MCU row's for a frame that
    /// streams — a sequential frame whose components all arrive in its one
    /// scan.
    pub coefficients: usize,
    /// Component samples: the block rows of each component one strip reads,
    /// replicated across the image's width.
    pub planes: usize,
    /// The strip handed to the sink.
    pub strip: usize,
}

impl JpegHeld {
    /// All of it.
    #[must_use]
    pub fn total(&self) -> usize {
        self.coefficients
            .saturating_add(self.planes)
            .saturating_add(self.strip)
    }
}

/// What a strip decode says of the image beside its strips.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JpegFrame {
    /// The geometry and colour the sink was told.
    pub layout: JpegLayout,
    /// What the decoder tolerated, as [`JpegImage::warnings`].
    pub warnings: Vec<Warning>,
    /// What it held while it decoded.
    pub held: JpegHeld,
    /// Samples the inverse transforms output, every component's: `N × N` for
    /// each block transformed at the scale's `N`, before any is replicated.
    pub transformed: u64,
}

/// Why a JPEG could not be decoded.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JpegError {
    /// The bytes do not begin like a JPEG.
    NotJpeg,
    /// A lossless frame: SOF3, SOF7, SOF11 or SOF15.
    ///
    /// Annex H's predictive coder, which shares nothing with the DCT path
    /// below -- no quantisation tables, no blocks, no transform. Both entropy
    /// coders are here because the predictor is what is missing either way:
    /// SOF3 and SOF7 are Huffman-coded and SOF11 and SOF15 arithmetic, and
    /// `crate::qm` decodes the latter's decisions perfectly well with nothing
    /// to hand them to.
    ///
    /// SOF3 and SOF7 were once **skipped rather than refused** -- the marker
    /// fell through to the unknown-segment arm, no frame was ever found, and
    /// the failure surfaced as `Truncated`, so a lossless JPEG looked like a
    /// damaged file.
    Lossless,
    /// A differential frame: SOF5, SOF6, SOF13 or SOF14.
    ///
    /// The hierarchical progression of Annex J, where a frame codes the
    /// difference from an upsampled earlier one. Same shape as
    /// [`JpegError::Lossless`]: the entropy coder is not the gap, so the
    /// Huffman pair and the arithmetic pair report together.
    ///
    /// **There was a third variant here, `Arithmetic`, and it is gone.** It
    /// refused SOF9, SOF10, SOF11, SOF13, SOF14 and SOF15 together on the
    /// grounds that Annex D's coder was not built. It is built (`crate::qm`),
    /// SOF9 and SOF10 decode, and the remaining four are refused by the annex
    /// they actually need rather than by the coder they happen to use -- which
    /// is what this enum was always for.
    Differential,
    /// A sample precision T.81 B.2.2 does not allow: anything but 8 or 12.
    UnsupportedPrecision,
    /// The file ended before the image did, past any hope of recovery.
    Truncated,
    /// A component count no colour model covers.
    UnsupportedComponents,
}

#[derive(Clone, Default)]
struct Component {
    id: u8,
    h: usize,
    v: usize,
    quant: usize,
    dc_table: usize,
    ac_table: usize,
    dc_prediction: i32,
    /// T.81 F.1.4.4.1.2's `Da`: the difference coded for the previous block of
    /// this component, which conditions the next block's first three
    /// arithmetic decisions. Zero at a scan and at every restart
    /// (F.1.4.4.1.5), and untouched by the Huffman path.
    da: i32,
    /// Blocks per line in the coefficient buffer, padded out to whole MCUs so
    /// an interleaved scan can address every block it codes.
    blocks_x: usize,
    /// Blocks per column, likewise padded.
    blocks_y: usize,
    /// Blocks per line that carry image rather than padding, which is what a
    /// non-interleaved scan iterates over (T.81 A.2.2). Getting this wrong
    /// desynchronises every later block in the scan.
    scan_x: usize,
    /// Blocks per column, likewise.
    scan_y: usize,
    /// Coefficients in zig-zag order, one 64-entry block after another.
    ///
    /// Zig-zag rather than natural order because spectral selection names its
    /// band in zig-zag indices; storing them any other way would mean
    /// converting on every scan instead of once at the end. `i16` because that
    /// is the range a coefficient occupies. The buffer covers the whole image,
    /// or, for a frame that streams, the one MCU row being decoded.
    coeffs: Vec<i16>,
    /// The first block row `coeffs` holds, and how many it holds: zero and
    /// `blocks_y` for a whole image, `V` rows from the MCU row being decoded
    /// for a frame that streams.
    row_base: usize,
    rows_held: usize,
}

impl Component {
    /// Where block `(bx, by)` starts in `coeffs`, when `coeffs` holds it.
    fn at(&self, bx: usize, by: usize) -> Option<usize> {
        if bx >= self.blocks_x || by >= self.blocks_y || by < self.row_base {
            return None;
        }
        let row = by - self.row_base;
        if row >= self.rows_held {
            return None;
        }
        (row * self.blocks_x + bx).checked_mul(64)
    }

    fn block(&self, bx: usize, by: usize) -> Option<&[i16]> {
        let at = self.at(bx, by)?;
        self.coeffs.get(at..at + 64)
    }

    fn block_mut(&mut self, bx: usize, by: usize) -> Option<&mut [i16]> {
        let at = self.at(bx, by)?;
        self.coeffs.get_mut(at..at + 64)
    }

    /// Clears a streaming frame's coefficients for the next MCU row.
    fn next_row(&mut self) {
        self.coeffs.fill(0);
        self.row_base += self.rows_held;
    }
}

/// Bits the first level of a Huffman lookup is indexed by.
const LOOKUP_BITS: u32 = 9;

/// Bits a second level is indexed by: the rest of the sixteen a code may run
/// to (T.81 C.1).
const SECOND_BITS: u32 = 16 - LOOKUP_BITS;

/// A lookup entry that points at a second level, whose 128 entries start at
/// the offset in its low bits.
const LONG: u32 = 1 << 31;

/// A lookup entry that names a symbol, held in its low byte. An entry without
/// it is a search that ends in nothing, reading the length it carries.
const HAS_SYMBOL: u32 = 1 << 16;

/// A lookup entry: a search that reads `length` bits and ends in `symbol`.
fn lookup_entry(length: u32, symbol: Option<u8>) -> u32 {
    (length << 8) | symbol.map_or(0, |symbol| HAS_SYMBOL | u32::from(symbol))
}

#[derive(Clone, Default)]
struct HuffmanTable {
    /// Maximum code of each length, or -1 when the length is unused.
    max_code: [i32; 17],
    /// Minimum code of each length.
    min_code: [i32; 17],
    /// Index into `values` where each length's codes begin.
    value_offset: [i32; 17],
    values: Vec<u8>,
    /// The first level of the lookup, indexed by the next [`LOOKUP_BITS`]
    /// bits: what [`HuffmanTable::search`] answers over them, or a pointer
    /// into `second`. Empty for a table no `DHT` defined, whose every search
    /// the bit-at-a-time path makes.
    first: Vec<u32>,
    /// The second levels, [`SECOND_BITS`] wide, one for each first-level
    /// prefix that begins a longer code.
    second: Vec<u32>,
}

impl HuffmanTable {
    /// Builds the canonical decoding tables from the per-length counts.
    fn build(counts: &[u8; 16], values: Vec<u8>) -> HuffmanTable {
        let mut table = HuffmanTable {
            values,
            ..HuffmanTable::default()
        };

        let mut code = 0i32;
        let mut index = 0i32;
        for length in 1..=16usize {
            let count = i32::from(counts.get(length - 1).copied().unwrap_or(0));
            if count == 0 {
                if let Some(slot) = table.max_code.get_mut(length) {
                    *slot = -1;
                }
                code <<= 1;
                continue;
            }
            if let Some(slot) = table.value_offset.get_mut(length) {
                *slot = index - code;
            }
            if let Some(slot) = table.min_code.get_mut(length) {
                *slot = code;
            }
            index += count;
            code += count;
            if let Some(slot) = table.max_code.get_mut(length) {
                *slot = code - 1;
            }
            code <<= 1;
        }
        table.build_lookup();
        table
    }

    /// The search the bit-at-a-time path makes, over the `bits` most
    /// significant bits of `window`: the first length whose largest code the
    /// bits so far do not pass, and the value it names. `None` when no length
    /// up to `bits` matched.
    ///
    /// A matched code whose value the table does not hold names nothing, as
    /// [`BitReader::huffman_bitwise`] answers it.
    fn search(&self, window: u32, bits: u32) -> Option<(u32, Option<u8>)> {
        for length in 1..=bits {
            let code = (window >> (bits - length)) as i32;
            let max = self.max_code.get(length as usize).copied().unwrap_or(-1);
            if max >= 0 && code <= max {
                let offset = self.value_offset.get(length as usize).copied().unwrap_or(0);
                let value = usize::try_from(offset + code)
                    .ok()
                    .and_then(|index| self.values.get(index).copied());
                return Some((length, value));
            }
        }
        None
    }

    /// Whether a code longer than the first level begins with `prefix`: the
    /// smallest code of some longer length that starts with it is within
    /// that length's largest. A prefix no code begins with reads sixteen bits
    /// and names nothing, and needs no second level to say so.
    fn reaches_past(&self, prefix: u32) -> bool {
        (LOOKUP_BITS + 1..=16).any(|length| {
            let max = self.max_code.get(length as usize).copied().unwrap_or(-1);
            max >= 0 && i64::from(prefix << (length - LOOKUP_BITS)) <= i64::from(max)
        })
    }

    /// Both levels of the lookup, each entry the answer [`Self::search`]
    /// gives over the bits it is indexed by, so the lookup answers what the
    /// search does for any table: complete, incomplete or oversubscribed.
    fn build_lookup(&mut self) {
        let mut first = Vec::with_capacity(1 << LOOKUP_BITS);
        let mut second = Vec::new();
        for prefix in 0..(1u32 << LOOKUP_BITS) {
            let entry = match self.search(prefix, LOOKUP_BITS) {
                Some((length, symbol)) => lookup_entry(length, symbol),
                None if self.reaches_past(prefix) => {
                    let at = second.len() as u32;
                    for suffix in 0..(1u32 << SECOND_BITS) {
                        let window = (prefix << SECOND_BITS) | suffix;
                        second.push(match self.search(window, 16) {
                            Some((length, symbol)) => lookup_entry(length, symbol),
                            None => lookup_entry(16, None),
                        });
                    }
                    LONG | at
                }
                None => lookup_entry(16, None),
            };
            first.push(entry);
        }
        self.first = first;
        self.second = second;
    }
}

/// Reads bits from the entropy-coded segment, unstuffing as it goes, through
/// a word of bits read ahead (`docs/design/jpeg-speed.md`, "The look-ahead
/// Huffman reader").
///
/// **The word holds data bytes and nothing else.** The refill stops at any
/// `X'FF'` not followed by a stuffed zero, and at the end, without reading
/// it; every such event — a restart marker stepped over, a marker, the end —
/// is met by [`BitReader::fill`] when the word is empty and a bit is asked
/// for, which is where a reader of one byte at a time meets it. So `exhausted`
/// and `crossed_restart` are set when that reader would set them, and
/// [`BitReader::position`] says where it would be.
struct BitReader<'a> {
    data: &'a [u8],
    /// The next byte the word is filled from.
    at: usize,
    /// The bits read ahead, the next one the most significant.
    word: u64,
    /// How many of `word`'s top bits are read ahead.
    count: u32,
    /// True once the reader has run past the end.
    exhausted: bool,
    /// Set when `bit` walked over a restart marker by itself, so `restart`
    /// knows not to skip a second one and lose a whole interval.
    crossed_restart: bool,
    /// The steps this reader took (`docs/design/resource-exhaustion.md`, "The
    /// counters"): each symbol, field and bit it was asked for, and each byte
    /// a restart's search examined. Added to the decode's count when the scan
    /// ends; nothing here reads it.
    steps: u64,
    /// Lookup entries read, for the probe count's test.
    #[cfg(test)]
    probes: u64,
    /// Symbols the bit-at-a-time search decoded, for the same test.
    #[cfg(test)]
    searched: u64,
}

impl<'a> BitReader<'a> {
    fn new(data: &'a [u8]) -> BitReader<'a> {
        BitReader {
            data,
            at: 0,
            word: 0,
            count: 0,
            exhausted: false,
            crossed_restart: false,
            steps: 0,
            #[cfg(test)]
            probes: 0,
            #[cfg(test)]
            searched: 0,
        }
    }

    /// Reads data bytes into the word while it has room for one.
    ///
    /// T.81 F.1.2.3: a `X'FF'` in entropy-coded data is followed by a stuffed
    /// zero, which is consumed here; an `X'FF'` followed by anything else, or
    /// by nothing, is not data, and the refill stops in front of it.
    fn refill(&mut self) {
        while self.count <= 56 {
            let Some(&byte) = self.data.get(self.at) else {
                return;
            };
            if byte == 0xFF {
                if self.data.get(self.at + 1) != Some(&0x00) {
                    return;
                }
                self.at += 2;
            } else {
                self.at += 1;
            }
            self.word |= u64::from(byte) << (56 - self.count);
            self.count += 8;
        }
    }

    /// Makes a bit available, as a reader of one byte at a time does when it
    /// has none: data read into the word, a restart marker stepped over, or a
    /// marker or the end met, `exhausted` set and false returned.
    ///
    /// Past a marker the reader's position is the marker's code, which the
    /// next fill reads as data, and past the end every fill fails again — the
    /// zeros that let a truncated image decode to something rather than
    /// nothing.
    fn fill(&mut self) -> bool {
        loop {
            self.refill();
            if self.count > 0 {
                return true;
            }
            // The refill stopped in front of an event: the end, or an X'FF'
            // that is not data.
            if self.data.get(self.at).is_none() {
                self.exhausted = true;
                return false;
            }
            self.at += 1;
            match self.data.get(self.at) {
                Some(&m) if (0xD0..=0xD7).contains(&m) => {
                    // A restart marker: skip it and carry on.
                    self.at += 1;
                    self.crossed_restart = true;
                }
                _ => {
                    self.exhausted = true;
                    return false;
                }
            }
        }
    }

    /// One bit, MSB first. Past the end this returns zeros.
    fn bit(&mut self) -> u32 {
        self.steps = self.steps.saturating_add(1);
        if self.count == 0 && !self.fill() {
            return 0;
        }
        let bit = (self.word >> 63) as u32;
        self.word <<= 1;
        self.count -= 1;
        bit
    }

    /// `n` bits, MSB first, at most 31: from the word when it holds them, and
    /// a bit at a time when it does not.
    fn bits(&mut self, n: u32) -> i32 {
        let n = n.min(31);
        if n == 0 {
            return 0;
        }
        self.steps = self.steps.saturating_add(1);
        if self.count < n {
            self.refill();
        }
        if self.count >= n {
            let value = (self.word >> (64 - n)) as i32;
            self.word <<= n;
            self.count -= n;
            return value;
        }
        let mut value = 0i32;
        for _ in 0..n {
            value = (value << 1) | self.bit() as i32;
        }
        value
    }

    /// Decodes one Huffman-coded symbol.
    ///
    /// The lookup answers when every bit the search could read is a data bit
    /// in the word: with sixteen read ahead and `exhausted` clear, in one
    /// probe, or two for a code longer than [`LOOKUP_BITS`]; with fewer, when
    /// the first level's entry fits in what is read ahead. Otherwise the
    /// search runs a bit at a time, which is also the only path once
    /// `exhausted` is set, because that search stops at the first length that
    /// fails past the end.
    fn huffman(&mut self, table: &HuffmanTable) -> Option<u8> {
        self.steps = self.steps.saturating_add(1);
        if self.count < 16 {
            self.refill();
        }
        if !self.exhausted && self.count >= LOOKUP_BITS {
            let index = (self.word >> (64 - LOOKUP_BITS)) as usize;
            if let Some(&first) = table.first.get(index) {
                let entry = if first & LONG == 0 {
                    Some(first)
                } else if self.count >= 16 {
                    let suffix = (self.word >> (64 - 16)) as usize & ((1 << SECOND_BITS) - 1);
                    table.second.get((first & !LONG) as usize + suffix).copied()
                } else {
                    None
                };
                if let Some(entry) = entry {
                    let length = (entry >> 8) & 0x1F;
                    if length <= self.count {
                        #[cfg(test)]
                        {
                            self.probes += if first & LONG == 0 { 1 } else { 2 };
                        }
                        self.word <<= length;
                        self.count -= length;
                        return (entry & HAS_SYMBOL != 0).then_some(entry as u8);
                    }
                }
            }
        }
        #[cfg(test)]
        {
            self.searched += 1;
        }
        self.huffman_bitwise(table)
    }

    /// The search a bit at a time: each length in turn against its largest
    /// code (T.81 F.2.2.3).
    fn huffman_bitwise(&mut self, table: &HuffmanTable) -> Option<u8> {
        let mut code = 0i32;
        for length in 1..=16usize {
            code = (code << 1) | self.bit() as i32;
            let max = table.max_code.get(length).copied().unwrap_or(-1);
            if max >= 0 && code <= max {
                let offset = table.value_offset.get(length).copied().unwrap_or(0);
                let index = usize::try_from(offset + code).ok()?;
                return table.values.get(index).copied();
            }
            if self.exhausted {
                return None;
            }
        }
        None
    }

    /// Where a reader of one byte at a time would be: past the byte the next
    /// bit is in, or past the last byte read when no part of one is left.
    /// The word's whole bytes are ones it had not read, and each is given
    /// back, two for an `X'FF'` and its stuffed zero.
    fn position(&self) -> usize {
        let partial = self.count % 8;
        let mut at = self.at;
        for k in 0..self.count / 8 {
            let byte = (self.word >> (64 - partial - 8 * (k + 1))) as u8;
            at = at.saturating_sub(if byte == 0xFF { 2 } else { 1 });
        }
        at
    }

    /// Resets at a restart marker (T.81 F.2.1.3.1).
    fn restart(&mut self) {
        // What is left of the current byte is dropped, and the bytes read
        // ahead of it go back, so the search below starts where it always
        // did: it stops one byte short of the end, so where it starts decides
        // what a stream that ends without its marker reads next.
        self.at = self.position();
        self.word = 0;
        self.count = 0;
        if self.crossed_restart {
            // The bit reader already stepped over it while filling its
            // accumulator; skipping another would drop an entire interval's
            // worth of blocks.
            self.crossed_restart = false;
            return;
        }

        // Skip to just past the next RSTn marker.
        while self.at + 1 < self.data.len() {
            self.steps = self.steps.saturating_add(1);
            if self.data.get(self.at) == Some(&0xFF) {
                if let Some(&m) = self.data.get(self.at + 1) {
                    if (0xD0..=0xD7).contains(&m) {
                        self.at += 2;
                        return;
                    }
                }
            }
            self.at += 1;
        }
    }
}

// Zig-zag order (T.81 figure A.6).
const ZIGZAG: [usize; 64] = [
    0, 1, 8, 16, 9, 2, 3, 10, 17, 24, 32, 25, 18, 11, 4, 5, 12, 19, 26, 33, 40, 48, 41, 34, 27, 20,
    13, 6, 7, 14, 21, 28, 35, 42, 49, 56, 57, 50, 43, 36, 29, 22, 15, 23, 30, 37, 44, 51, 58, 59,
    52, 45, 38, 31, 39, 46, 53, 60, 61, 54, 47, 55, 62, 63,
];

/// The largest magnitude category T.81 defines. A Huffman table is free to
/// contain a larger byte, and a corrupt one will; shifting by it is undefined
/// in the spec and a panic in Rust (ruling 1).
const MAX_CATEGORY: u32 = 16;

/// Extends a Huffman-decoded magnitude to its signed value (T.81 F.2.2.1).
fn extend(value: i32, length: u32) -> i32 {
    if length == 0 || length > MAX_CATEGORY {
        return 0;
    }
    if value < (1 << (length - 1)) {
        value - (1 << length) + 1
    } else {
        value
    }
}

/// Finds the next real marker, stepping over stuffed bytes and restarts.
///
/// A scan's entropy-coded data has no length field, so the only way to reach
/// the segment after it is to look for the next thing that cannot occur inside
/// it. Trusting the bit reader's position instead would put a corrupt scan's
/// desynchronisation into the marker stream as well. Each byte examined is a
/// step.
fn next_marker(data: &[u8], from: usize, steps: &mut u64) -> usize {
    let mut at = from;
    while at + 1 < data.len() {
        *steps = steps.saturating_add(1);
        if data.get(at) == Some(&0xFF) {
            if let Some(&marker) = data.get(at + 1) {
                if marker != 0x00 && marker != 0xFF && !(0xD0..=0xD7).contains(&marker) {
                    return at;
                }
            }
        }
        at += 1;
    }
    data.len()
}

/// Sizes the coefficient buffers, returning the MCU grid: the whole image's,
/// or one MCU row's when the frame `streams`.
///
/// Returns `None` when the image would exceed the output cap, which is checked
/// here rather than after decoding because the buffers are the allocation that
/// a hostile size field is trying to provoke.
fn allocate(
    components: &mut [Component],
    width: usize,
    height: usize,
    max_output: usize,
    streams: bool,
) -> Option<(usize, usize)> {
    if width
        .saturating_mul(height)
        .saturating_mul(components.len())
        > max_output
    {
        return None;
    }

    let h_max = components.iter().map(|c| c.h).max().unwrap_or(1).max(1);
    let v_max = components.iter().map(|c| c.v).max().unwrap_or(1).max(1);
    let mcus_x = width.div_ceil(h_max * 8);
    let mcus_y = height.div_ceil(v_max * 8);

    for component in components.iter_mut() {
        component.blocks_x = mcus_x * component.h;
        component.blocks_y = mcus_y * component.v;

        // A.1.1: a component's own resolution, rounded up to whole blocks.
        let own_w = (width * component.h).div_ceil(h_max);
        let own_h = (height * component.v).div_ceil(v_max);
        component.scan_x = own_w.div_ceil(8).min(component.blocks_x);
        component.scan_y = own_h.div_ceil(8).min(component.blocks_y);

        component.row_base = 0;
        component.rows_held = if streams {
            component.v.min(component.blocks_y)
        } else {
            component.blocks_y
        };
        component.coeffs = vec![0i16; component.blocks_x * component.rows_held * 64];
    }

    Some((mcus_x, mcus_y))
}

/// Whether a frame can stream from its first scan, which names `parts`
/// (`docs/design/jpeg-speed.md`, "Which frames stream"): a sequential frame
/// that [`finish`] accepts, every component in the scan once, every sampling
/// ratio dividing the largest. Whether the rest of the stream lets it is
/// [`the_rest_is_inert`]'s.
fn streamable(
    components: &[Component],
    parts: &[usize],
    progressive: bool,
    width: usize,
    height: usize,
) -> bool {
    let n = components.len();
    if progressive || width == 0 || height == 0 || !matches!(n, 1 | 3 | 4) {
        return false;
    }
    if parts.len() != n || !(0..n).all(|index| parts.contains(&index)) {
        return false;
    }
    let h_max = components.iter().map(|c| c.h).max().unwrap_or(1).max(1);
    let v_max = components.iter().map(|c| c.v).max().unwrap_or(1).max(1);
    components
        .iter()
        .all(|c| h_max % c.h.max(1) == 0 && v_max % c.v.max(1) == 0)
}

/// Whether nothing from `at` on changes what the end of the decode reads:
/// the marker walk [`scan_frame`] makes, run ahead of it from where it will
/// resume, meets no frame header, no further scan, no quantisation table and
/// no Adobe marker before EOI or the end. A later `DQT` would change the
/// dequantisation and a later Adobe marker the colour, both of which the end
/// of a decode reads, and a later scan or frame anything at all. Each byte or
/// segment the walk steps over is a step.
fn the_rest_is_inert(data: &[u8], mut at: usize, steps: &mut u64) -> bool {
    while at + 1 < data.len() {
        *steps = steps.saturating_add(1);
        if data.get(at) != Some(&0xFF) {
            at += 1;
            continue;
        }
        let Some(&marker) = data.get(at + 1) else {
            break;
        };
        at += 2;
        match marker {
            0x01 | 0xD0..=0xD7 | 0xFF => continue,
            0xD9 => return true,
            _ => {}
        }
        let Some(length) = data
            .get(at..at + 2)
            .map(|b| usize::from(u16::from_be_bytes([b[0], b[1]])))
        else {
            return true;
        };
        let segment_end = at + length.max(2);
        let Some(segment) = data.get(at + 2..segment_end.min(data.len())) else {
            return true;
        };
        match marker {
            0xC0..=0xC3 | 0xC5..=0xC7 | 0xC9..=0xCB | 0xCD..=0xCF | 0xDA | 0xDB => {
                return false;
            }
            0xEE if segment.starts_with(b"Adobe") => return false,
            _ => {}
        }
        at = segment_end;
    }
    true
}

/// The colour a frame's components mean, or `None` for a count no colour
/// model covers.
fn colour_of(components: usize, adobe_seen: bool) -> Option<JpegColor> {
    match components {
        1 => Some(JpegColor::Gray),
        3 => Some(JpegColor::Rgb),
        // Adobe transform 2 is YCCK; 0 is plain CMYK. Photoshop writes CMYK
        // inverted, which the Adobe marker's presence signals.
        4 if adobe_seen => Some(JpegColor::CmykInverted),
        4 => Some(JpegColor::Cmyk),
        _ => None,
    }
}

/// Decodes a JPEG, at full size.
pub fn decode(data: &[u8], max_output: usize) -> Result<JpegImage, JpegError> {
    decode_counted(data, max_output).0
}

/// [`decode`], and the steps it took (`docs/design/resource-exhaustion.md`,
/// "The counters"): each byte or segment the marker walk and its searches
/// examined, each Huffman table entry built, each Huffman symbol, field and
/// bit read, each arithmetic decision decoded, each block decoded, each
/// sample the inverse transforms output and each byte of the image returned.
///
/// A scan visits every block it names whatever its bytes say, so a frame of
/// many scans over a large image costs blocks times scans: the block count is
/// where that repeats, and it is counted there.
pub fn decode_counted(data: &[u8], max_output: usize) -> (Result<JpegImage, JpegError>, u64) {
    let mut steps = 0u64;
    let mut image = Collected::default();
    let decoded = scan_frame_counted(
        data,
        max_output,
        JpegScale::Full,
        Some(&mut image),
        &mut steps,
    )
    .and_then(|scanned| finish(scanned, max_output, &mut image));
    let decoded = decoded.map(|frame| {
        let returned = u64::try_from(image.data.len()).unwrap_or(u64::MAX);
        steps = steps
            .saturating_add(frame.transformed)
            .saturating_add(returned);
        JpegImage {
            width: frame.layout.width,
            height: frame.layout.height,
            color: frame.layout.color,
            data: image.data,
            precision: frame.layout.precision,
            warnings: frame.warnings,
        }
    });
    (decoded, steps)
}

/// Decodes a JPEG a strip at a time, at `scale` (`docs/design/jpeg-speed.md`,
/// "Strips" and "The reduced transforms"): `sink` is told the image's
/// layout, then handed every strip, top to bottom, and the image is never
/// held whole.
///
/// At full size the strips are [`decode`]'s image, row for row, and the frame
/// its geometry, colour and warnings; at a reduced scale they are the image
/// the reduced transforms make, `⌈W·N/8⌉` by `⌈H·N/8⌉`. `max_output` bounds
/// the frame's own samples at every scale, since the coefficients a decode
/// holds are the frame's. **A decode that hands out a strip succeeds**: every
/// check is made before the first one.
pub fn decode_strips(
    data: &[u8],
    max_output: usize,
    scale: JpegScale,
    sink: &mut dyn JpegSink,
) -> Result<JpegFrame, JpegError> {
    let mut forwarded = Forwarded {
        sink,
        strip: Vec::new(),
        held: 0,
    };
    let scanned = scan_frame(data, max_output, scale, Some(&mut forwarded))?;
    finish(scanned, max_output, &mut forwarded)
}

/// What the marker walk leaves for the end of a decode.
struct Scanned {
    components: Vec<Component>,
    quant: [[u16; 64]; 4],
    width: usize,
    height: usize,
    adobe_seen: bool,
    adobe_transform: Option<u8>,
    /// T.81 B.2.2's `P`.
    precision: u8,
    /// The scale the image is painted at.
    scale: JpegScale,
    /// What the scans tolerated.
    warnings: Vec<Warning>,
    /// What a frame that streamed left: its strips were painted as its scan
    /// was decoded.
    streamed: Option<Streamed>,
}

/// A streamed frame's layout, what it held and the samples it transformed.
struct Streamed {
    layout: JpegLayout,
    held: JpegHeld,
    transformed: u64,
}

/// The layout of a `width` by `height` frame's image at `scale`.
fn layout_at(
    width: usize,
    height: usize,
    color: JpegColor,
    precision: u8,
    scale: JpegScale,
) -> JpegLayout {
    // B.2.2's `X` and `Y` are sixteen bits.
    let (full_width, full_height) = (width as u32, height as u32);
    JpegLayout {
        width: scale.length(full_width),
        height: scale.length(full_height),
        color,
        precision,
        scale,
        full_width,
        full_height,
    }
}

/// Reads every marker segment and decodes every scan into the frame's
/// coefficients, or, for a frame that streams and a `stream` to paint into,
/// paints each MCU row at `scale` as it is decoded
/// (`docs/design/jpeg-speed.md`, "Which frames stream").
fn scan_frame(
    data: &[u8],
    max_output: usize,
    scale: JpegScale,
    stream: Option<&mut dyn Rows>,
) -> Result<Scanned, JpegError> {
    scan_frame_counted(data, max_output, scale, stream, &mut 0)
}

/// [`scan_frame`], adding the steps it took to `steps`: what
/// [`decode_counted`] names, but for the transform and the image, which the
/// end of a decode counts.
fn scan_frame_counted(
    data: &[u8],
    max_output: usize,
    scale: JpegScale,
    mut stream: Option<&mut dyn Rows>,
    steps: &mut u64,
) -> Result<Scanned, JpegError> {
    if data.get(..2) != Some(&[0xFF, 0xD8]) {
        return Err(JpegError::NotJpeg);
    }

    let mut warnings = Vec::new();
    let mut quant = [[1u16; 64]; 4];
    let mut dc_tables: Vec<HuffmanTable> = vec![HuffmanTable::default(); 4];
    let mut ac_tables: Vec<HuffmanTable> = vec![HuffmanTable::default(); 4];
    let mut components: Vec<Component> = Vec::new();
    let (mut width, mut height) = (0usize, 0usize);
    let mut restart_interval = 0usize;
    let mut adobe_transform: Option<u8> = None;
    let mut adobe_seen = false;
    let mut progressive = false;
    let mut arithmetic = false;
    let mut conditioning = arith::Conditioning::default();
    let mut arith_stats = arith::Stats::new();
    let mut sample_precision = 8u8;
    let mut mcus = (0usize, 0usize);
    let mut allocated = false;
    let mut truncated = false;
    let mut painting: Option<(Painter, JpegLayout)> = None;

    let mut at = 2usize;
    while at + 1 < data.len() {
        *steps = steps.saturating_add(1);
        if data.get(at) != Some(&0xFF) {
            at += 1;
            continue;
        }
        let Some(&marker) = data.get(at + 1) else {
            break;
        };
        at += 2;

        match marker {
            // Padding and standalone markers.
            0x01 | 0xD0..=0xD7 | 0xFF => continue,
            0xD9 => break, // EOI
            _ => {}
        }

        let Some(length) = data
            .get(at..at + 2)
            .map(|b| usize::from(u16::from_be_bytes([b[0], b[1]])))
        else {
            break;
        };
        let segment_end = at + length.max(2);
        let Some(segment) = data.get(at + 2..segment_end.min(data.len())) else {
            break;
        };

        match marker {
            // SOF0 baseline, SOF1 extended sequential, SOF2 progressive, and
            // SOF9 and SOF10, which are the same two DCT processes with
            // Annex D's entropy coder in place of Annex C's.
            0xC0..=0xC2 | 0xC9 | 0xCA => {
                progressive = matches!(marker, 0xC2 | 0xCA);
                arithmetic = matches!(marker, 0xC9 | 0xCA);

                let (Some(&precision), Some(h), Some(w)) =
                    (segment.first(), segment.get(1..3), segment.get(3..5))
                else {
                    return Err(JpegError::Truncated);
                };
                // B.2.2: `P` is 8 for a baseline frame and 8 or 12 for an
                // extended sequential or progressive one. Anything else is a
                // header this build will not guess at.
                if precision != 8 && !(precision == 12 && marker != 0xC0) {
                    return Err(JpegError::UnsupportedPrecision);
                }
                sample_precision = precision;
                height = usize::from(u16::from_be_bytes([h[0], h[1]]));
                width = usize::from(u16::from_be_bytes([w[0], w[1]]));

                let count = usize::from(segment.get(5).copied().unwrap_or(0));
                components.clear();
                for i in 0..count.min(4) {
                    let base = 6 + i * 3;
                    let (Some(&id), Some(&hv), Some(&tq)) = (
                        segment.get(base),
                        segment.get(base + 1),
                        segment.get(base + 2),
                    ) else {
                        return Err(JpegError::Truncated);
                    };
                    components.push(Component {
                        id,
                        h: usize::from(hv >> 4).clamp(1, 4),
                        v: usize::from(hv & 0x0F).clamp(1, 4),
                        quant: usize::from(tq).min(3),
                        ..Component::default()
                    });
                }
            }
            // Every other SOF marker, refused by the annex it needs rather
            // than by the entropy coder it uses: SOF3 and SOF11 are Annex H
            // lossless, SOF7 and SOF15 differential lossless, SOF5, SOF6,
            // SOF13 and SOF14 Annex J's hierarchical progression. 0xC4 is DHT
            // and 0xCC is DAC, which are tables rather than frames and have
            // their own arms below.
            0xC3 | 0xC7 | 0xCB | 0xCF => return Err(JpegError::Lossless),
            0xC5 | 0xC6 | 0xCD | 0xCE => return Err(JpegError::Differential),

            // DQT
            0xDB => {
                let mut i = 0usize;
                while i < segment.len() {
                    let Some(&pq_tq) = segment.get(i) else { break };
                    i += 1;
                    let precision = pq_tq >> 4;
                    let index = usize::from(pq_tq & 0x0F).min(3);
                    for k in 0..64usize {
                        let value = if precision == 0 {
                            let Some(&v) = segment.get(i) else { break };
                            i += 1;
                            u16::from(v)
                        } else {
                            let Some(pair) = segment.get(i..i + 2) else {
                                break;
                            };
                            i += 2;
                            u16::from_be_bytes([pair[0], pair[1]])
                        };
                        if let (Some(table), Some(&z)) = (quant.get_mut(index), ZIGZAG.get(k)) {
                            if let Some(slot) = table.get_mut(z) {
                                *slot = value.max(1);
                            }
                        }
                    }
                }
            }

            // DHT
            0xC4 => {
                let mut i = 0usize;
                while i < segment.len() {
                    let Some(&tc_th) = segment.get(i) else { break };
                    i += 1;
                    let class = tc_th >> 4;
                    let index = usize::from(tc_th & 0x0F).min(3);

                    let mut counts = [0u8; 16];
                    let Some(raw) = segment.get(i..i + 16) else {
                        break;
                    };
                    counts.copy_from_slice(raw);
                    i += 16;

                    let total: usize = counts.iter().map(|&c| usize::from(c)).sum();
                    let Some(values) = segment.get(i..i + total) else {
                        break;
                    };
                    i += total;

                    let table = HuffmanTable::build(&counts, values.to_vec());
                    // A table's lookup is built whole whatever its segment
                    // holds, so each entry is a step.
                    let entries = table.first.len().saturating_add(table.second.len());
                    *steps = steps.saturating_add(u64::try_from(entries).unwrap_or(u64::MAX));
                    let target = if class == 0 {
                        &mut dc_tables
                    } else {
                        &mut ac_tables
                    };
                    if let Some(slot) = target.get_mut(index) {
                        *slot = table;
                    }
                }
            }

            // DAC: the arithmetic conditioning tables (B.2.4.3).
            //
            // **This arm did not exist until September 2026**, and the comment
            // beside the SOF refusals above said it did -- "0xCC is DAC, which
            // [is a table] rather than [a frame] and [is] handled below". It
            // was not: 0xCC fell through to the wildcard and was stepped over
            // as though it were a comment. It did not matter while every
            // arithmetic frame was refused before a DAC could be reached, but
            // it is exactly the shape of the defect this decoder has already
            // been caught by once, when SOF3, SOF5, SOF6 and SOF7 were skipped
            // rather than refused and a lossless JPEG surfaced as a damaged
            // file.
            0xCC => conditioning.define(segment),

            // DRI
            0xDD => {
                if let Some(pair) = segment.get(..2) {
                    restart_interval = usize::from(u16::from_be_bytes([pair[0], pair[1]]));
                }
            }

            // APP14: Adobe's colour transform marker.
            0xEE => {
                if segment.starts_with(b"Adobe") {
                    adobe_seen = true;
                    adobe_transform = segment.last().copied();
                }
            }

            // SOS: one scan, of which a progressive file has many.
            0xDA => {
                let count = usize::from(segment.first().copied().unwrap_or(0));
                let mut parts: Vec<usize> = Vec::with_capacity(count.min(4));
                for i in 0..count.min(4) {
                    let (Some(&id), Some(&tables)) =
                        (segment.get(1 + i * 2), segment.get(2 + i * 2))
                    else {
                        return Err(JpegError::Truncated);
                    };
                    if let Some(index) = components.iter().position(|c| c.id == id) {
                        if let Some(component) = components.get_mut(index) {
                            component.dc_table = usize::from(tables >> 4).min(3);
                            component.ac_table = usize::from(tables & 0x0F).min(3);
                        }
                        parts.push(index);
                    }
                }

                // G.1.1.1.1: the spectral band and the point transform. A
                // baseline scan always says 0..63 with no approximation, so
                // reading them costs nothing and progressive needs them.
                let base = 1 + count.min(4) * 2;
                let ss = usize::from(segment.get(base).copied().unwrap_or(0)).min(63);
                let se = usize::from(segment.get(base + 1).copied().unwrap_or(63)).min(63);
                let a = segment.get(base + 2).copied().unwrap_or(0);
                let (ah, al) = (u32::from(a >> 4), u32::from(a & 0x0F));

                if !allocated {
                    let streams = stream.is_some()
                        && streamable(&components, &parts, progressive, width, height)
                        && the_rest_is_inert(data, next_marker(data, segment_end, steps), steps);
                    let Some(grid) = allocate(&mut components, width, height, max_output, streams)
                    else {
                        warnings.push(Warning::OutputCapHit);
                        return Err(JpegError::Truncated);
                    };
                    mcus = grid;
                    allocated = true;
                    let colour = colour_of(components.len(), adobe_seen);
                    if let (true, Some(colour), Some(target)) =
                        (streams, colour, stream.as_deref_mut())
                    {
                        let layout = layout_at(width, height, colour, sample_precision, scale);
                        target.begin(layout);
                        let painter =
                            Painter::new(&components, layout, adobe_transform.unwrap_or(0));
                        painting = Some((painter, layout));
                    }
                }

                let scan = data.get(segment_end..).unwrap_or_default();
                let entropy = Entropy {
                    arithmetic,
                    dc_tables: &dc_tables,
                    ac_tables: &ac_tables,
                    conditioning: &conditioning,
                    stats: &mut arith_stats,
                };
                let band = (ss, se.max(ss));
                let complete = match (painting.as_mut(), stream.as_deref_mut()) {
                    (Some((painter, _)), Some(target)) => {
                        // One MCU row of coefficients at a time: a row is
                        // painted when its last unit is decoded, and the rows
                        // a scan that stopped never reached are painted from
                        // what they hold, as the whole image's buffer held it.
                        let alone = parts.len() == 1;
                        let units_y = match parts.first().and_then(|&part| components.get(part)) {
                            Some(component) if alone => component.scan_y,
                            _ => mcus.1,
                        };
                        let v = components.first().map_or(1, |c| c.v.max(1));
                        let mut next = 0usize;
                        let complete = {
                            let quant = &quant;
                            let mut row = |uy: usize, components: &mut [Component]| {
                                let done = if !alone {
                                    Some(uy)
                                } else if (uy + 1) % v == 0 || uy + 1 == units_y {
                                    Some(uy / v)
                                } else {
                                    None
                                };
                                if let Some(strip) = done {
                                    painter.paint(strip, components, quant, &mut *target);
                                    components.iter_mut().for_each(Component::next_row);
                                    next = strip + 1;
                                }
                            };
                            decode_scan(
                                scan,
                                &mut components,
                                &parts,
                                entropy,
                                restart_interval,
                                progressive,
                                band,
                                (ah, al),
                                mcus,
                                Some(&mut row),
                                steps,
                            )
                        };
                        for strip in next..painter.strips() {
                            painter.paint(strip, &components, &quant, &mut *target);
                            components.iter_mut().for_each(Component::next_row);
                        }
                        complete
                    }
                    _ => decode_scan(
                        scan,
                        &mut components,
                        &parts,
                        entropy,
                        restart_interval,
                        progressive,
                        band,
                        (ah, al),
                        mcus,
                        None,
                        steps,
                    ),
                };
                truncated |= !complete;

                // Entropy data carries no length; the next segment starts at
                // the next marker that cannot appear inside it.
                at = next_marker(data, segment_end, steps);
                continue;
            }

            _ => {}
        }

        at = segment_end;
    }

    if !allocated {
        warnings.push(Warning::TruncatedInput);
        return Err(JpegError::Truncated);
    }
    if truncated {
        warnings.push(Warning::TruncatedInput);
    }

    let streamed = match (painting, stream) {
        (Some((painter, layout)), Some(target)) => {
            let coefficients = components
                .iter()
                .map(|component| component.coeffs.len().saturating_mul(2))
                .fold(0, usize::saturating_add);
            let held = JpegHeld {
                coefficients,
                planes: painter.held(),
                strip: target.held(),
            };
            Some(Streamed {
                layout,
                held,
                transformed: painter.transformed,
            })
        }
        _ => None,
    };

    Ok(Scanned {
        components,
        quant,
        width,
        height,
        adobe_seen,
        adobe_transform,
        precision: sample_precision,
        scale,
        warnings,
        streamed,
    })
}

/// Everything a scan's entropy decoding needs that is not the scan's bytes.
///
/// It is one struct rather than five parameters because the frame picks one
/// half of it and never both: a Huffman frame reads `dc_tables` and
/// `ac_tables` and leaves the conditioning alone, and an arithmetic frame does
/// the reverse.
struct Entropy<'a> {
    /// The frame was SOF9 or SOF10, so the scan is coded by Annex D rather
    /// than Annex C.
    arithmetic: bool,
    dc_tables: &'a [HuffmanTable],
    ac_tables: &'a [HuffmanTable],
    conditioning: &'a arith::Conditioning,
    stats: &'a mut arith::Stats,
}

/// What a scan calls when every unit of a row is decoded: the row, and the
/// components it was decoded into.
type RowDone<'a> = &'a mut dyn FnMut(usize, &mut [Component]);

/// The live entropy decoder for one scan: one or the other, never both.
enum Coder<'a> {
    /// Annex C, through [`BitReader`].
    Huffman(BitReader<'a>),
    /// Annex D, through [`crate::qm::QmDecoder`].
    Arithmetic(QmDecoder<'a>),
}

/// Decodes one scan into the components' coefficient buffers.
///
/// Returns false when the entropy data ran out, a table was missing, or the
/// arithmetic decoder met T.81 F.2.4.4 b)'s "physically impossible data". What
/// was decoded stays in place either way: a progressive file that loses its
/// last refinement still shows an image, just a coarser one, which is exactly
/// the degradation the format was designed around (ruling 2).
///
/// Adds to `steps` each block the scan decoded and the steps its entropy
/// decoder took: a scan visits every block it names, so this is where a
/// frame of many scans repeats its work.
#[allow(clippy::too_many_arguments)]
fn decode_scan(
    data: &[u8],
    components: &mut [Component],
    parts: &[usize],
    entropy: Entropy<'_>,
    restart_interval: usize,
    progressive: bool,
    band: (usize, usize),
    approximation: (u32, u32),
    mcus: (usize, usize),
    mut row_done: Option<RowDone<'_>>,
    steps: &mut u64,
) -> bool {
    if parts.is_empty() {
        return false;
    }

    let Entropy {
        arithmetic,
        dc_tables,
        ac_tables,
        conditioning,
        stats,
    } = entropy;

    // E.2.3 and F.2.4: the entropy coder is initialised at the start of every
    // scan, and for the arithmetic coder F.1.4.4.1.5 and F.1.4.4.2.2 return
    // every statistics bin to Annex D's initial state at the same moment.
    let mut coder = if arithmetic {
        stats.reset();
        Coder::Arithmetic(QmDecoder::new(data))
    } else {
        Coder::Huffman(BitReader::new(data))
    };
    let mut eobrun = 0u32;
    for &index in parts {
        if let Some(component) = components.get_mut(index) {
            component.dc_prediction = 0;
            component.da = 0;
        }
    }

    // A.2: more than one component in a scan means the blocks are interleaved
    // MCU by MCU; one component means plain raster order over that component's
    // own blocks, with no MCU padding.
    let interleaved = parts.len() > 1;
    let (units_x, units_y) = if interleaved {
        mcus
    } else {
        parts
            .first()
            .and_then(|&index| components.get(index))
            .map_or((0, 0), |c| (c.scan_x, c.scan_y))
    };

    let mut unit = 0usize;
    let mut complete = true;
    let mut blocks = 0u64;

    'outer: for uy in 0..units_y {
        for ux in 0..units_x {
            if restart_interval > 0 && unit > 0 && unit % restart_interval == 0 {
                match &mut coder {
                    Coder::Huffman(reader) => reader.restart(),
                    Coder::Arithmetic(qm) => {
                        // F.2.4.4: the RSTm markers "can be located without
                        // decoding", and E.2.4 restarts the coder after each.
                        // F.1.4.4.1.5 and F.1.4.4.2.2 reset the statistics at
                        // "the beginning of each restart interval" too, which
                        // is the difference from the Huffman path -- there is
                        // no Huffman table to reset.
                        qm.restart();
                        stats.reset();
                    }
                }
                eobrun = 0;
                for &index in parts {
                    if let Some(component) = components.get_mut(index) {
                        component.dc_prediction = 0;
                        component.da = 0;
                    }
                }
            }
            unit += 1;

            if !interleaved {
                let Some(&index) = parts.first() else {
                    break 'outer;
                };
                let Some(component) = components.get_mut(index) else {
                    break 'outer;
                };
                blocks += 1;
                if !decode_block(
                    &mut coder,
                    conditioning,
                    stats,
                    component,
                    ux,
                    uy,
                    dc_tables,
                    ac_tables,
                    progressive,
                    band,
                    approximation,
                    &mut eobrun,
                ) {
                    complete = false;
                    break 'outer;
                }
                continue;
            }

            for &index in parts {
                let (h, v) = components.get(index).map_or((1, 1), |c| (c.h, c.v));
                for by in 0..v {
                    for bx in 0..h {
                        let Some(component) = components.get_mut(index) else {
                            complete = false;
                            break 'outer;
                        };
                        blocks += 1;
                        if !decode_block(
                            &mut coder,
                            conditioning,
                            stats,
                            component,
                            ux * h + bx,
                            uy * v + by,
                            dc_tables,
                            ac_tables,
                            progressive,
                            band,
                            approximation,
                            &mut eobrun,
                        ) {
                            complete = false;
                            break 'outer;
                        }
                    }
                }
            }
        }
        // Every unit of row `uy` is decoded.
        if let Some(row_done) = row_done.as_mut() {
            row_done(uy, components);
        }
    }

    let coded = match &coder {
        Coder::Huffman(reader) => reader.steps,
        Coder::Arithmetic(qm) => qm.steps(),
    };
    *steps = steps.saturating_add(blocks).saturating_add(coded);

    complete
        && match &coder {
            Coder::Huffman(reader) => !reader.exhausted,
            // The scan's slice runs to the end of the file, so a healthy
            // arithmetic scan stops at the marker that follows its
            // entropy-coded segment (D.21) with bytes to spare. Running off
            // the end instead means no marker was ever found, which is the
            // same truncation `exhausted` reports on the other side.
            Coder::Arithmetic(qm) => !qm.overran(),
        }
}

/// Decodes one block, in whichever of the codings this scan is using.
///
/// The coding model -- sequential, progressive DC first or refined,
/// progressive AC first or refined -- is chosen by the scan header and is the
/// same on both sides of `coder`. Only the entropy coding differs, which is
/// why the split is here and not higher up.
#[allow(clippy::too_many_arguments)]
fn decode_block(
    coder: &mut Coder,
    conditioning: &arith::Conditioning,
    stats: &mut arith::Stats,
    component: &mut Component,
    bx: usize,
    by: usize,
    dc_tables: &[HuffmanTable],
    ac_tables: &[HuffmanTable],
    progressive: bool,
    band: (usize, usize),
    approximation: (u32, u32),
    eobrun: &mut u32,
) -> bool {
    // Worked on as i32 and stored as i16: refinement adds to what earlier
    // scans left, and the intermediate must not wrap where the stored value
    // saturates.
    let mut block = [0i32; 64];
    if let Some(existing) = component.block(bx, by) {
        for (slot, &value) in block.iter_mut().zip(existing.iter()) {
            *slot = i32::from(value);
        }
    }

    let (ss, se) = band;
    let (ah, al) = approximation;

    let ok = match coder {
        Coder::Arithmetic(qm) => arith::decode_block(
            qm,
            stats,
            conditioning,
            (component.dc_table, component.ac_table),
            &mut component.dc_prediction,
            &mut component.da,
            &mut block,
            progressive,
            (ss, se),
            (ah, al),
        ),
        Coder::Huffman(reader) => {
            if !progressive {
                decode_sequential(reader, component, &mut block, dc_tables, ac_tables)
            } else if ss == 0 {
                decode_dc_progressive(reader, component, &mut block, dc_tables, ah, al)
            } else {
                decode_ac_progressive(
                    reader, component, &mut block, ac_tables, ss, se, ah, al, eobrun,
                )
            }
        }
    };

    if let Some(target) = component.block_mut(bx, by) {
        for (slot, &value) in target.iter_mut().zip(block.iter()) {
            *slot = value.clamp(i32::from(i16::MIN), i32::from(i16::MAX)) as i16;
        }
    }
    ok
}

/// Baseline and extended sequential: the whole block in one pass.
fn decode_sequential(
    reader: &mut BitReader,
    component: &mut Component,
    block: &mut [i32; 64],
    dc_tables: &[HuffmanTable],
    ac_tables: &[HuffmanTable],
) -> bool {
    let (Some(dc_table), Some(ac_table)) = (
        dc_tables.get(component.dc_table),
        ac_tables.get(component.ac_table),
    ) else {
        return false;
    };

    // DC: a difference from the previous block's value.
    let Some(t) = reader.huffman(dc_table) else {
        return false;
    };
    let t = u32::from(t).min(MAX_CATEGORY);
    let diff = extend(reader.bits(t), t);
    component.dc_prediction = component.dc_prediction.saturating_add(diff);
    block[0] = component.dc_prediction;

    // AC: run-length pairs to the end of the block.
    let mut k = 1usize;
    while k < 64 {
        let Some(rs) = reader.huffman(ac_table) else {
            return false;
        };
        let run = usize::from(rs >> 4);
        let size = u32::from(rs & 0x0F);
        if size == 0 {
            if run == 15 {
                k += 16; // ZRL: sixteen zeros.
                continue;
            }
            break; // EOB.
        }
        k += run;
        if k >= 64 {
            break;
        }
        let value = extend(reader.bits(size), size);
        if let Some(slot) = block.get_mut(k) {
            *slot = value;
        }
        k += 1;
    }
    true
}

/// Progressive DC (G.1.2.1): the first scan sends the value shifted right by
/// the point transform; every later one sends the next bit down.
fn decode_dc_progressive(
    reader: &mut BitReader,
    component: &mut Component,
    block: &mut [i32; 64],
    dc_tables: &[HuffmanTable],
    ah: u32,
    al: u32,
) -> bool {
    if ah == 0 {
        let Some(table) = dc_tables.get(component.dc_table) else {
            return false;
        };
        let Some(t) = reader.huffman(table) else {
            return false;
        };
        let t = u32::from(t).min(MAX_CATEGORY);
        let diff = extend(reader.bits(t), t);
        component.dc_prediction = component.dc_prediction.saturating_add(diff);
        block[0] = component.dc_prediction << al.min(15);
        return true;
    }

    if reader.bit() == 1 {
        block[0] |= 1 << al.min(15);
    }
    !reader.exhausted
}

/// Progressive AC, first pass (G.1.2.2): run-length pairs within the band,
/// with an end-of-band run that can span whole blocks.
#[allow(clippy::too_many_arguments)]
fn decode_ac_first(
    reader: &mut BitReader,
    block: &mut [i32; 64],
    table: &HuffmanTable,
    ss: usize,
    se: usize,
    al: u32,
    eobrun: &mut u32,
) -> bool {
    if *eobrun > 0 {
        *eobrun -= 1;
        return true;
    }

    let mut k = ss;
    while k <= se {
        let Some(rs) = reader.huffman(table) else {
            return false;
        };
        let run = u32::from(rs >> 4);
        let size = u32::from(rs & 0x0F);

        if size == 0 {
            if run < 15 {
                // An EOB run of 2^run blocks, this one included.
                *eobrun = (1u32 << run).saturating_sub(1);
                if run > 0 {
                    *eobrun = eobrun.saturating_add(reader.bits(run) as u32);
                }
                break;
            }
            k += 16; // ZRL.
            continue;
        }

        k += run as usize;
        if k > se {
            break;
        }
        let value = extend(reader.bits(size), size);
        if let Some(slot) = block.get_mut(k) {
            *slot = value << al.min(15);
        }
        k += 1;
    }
    true
}

/// Progressive AC, refinement (G.1.2.3).
///
/// The awkward one: the bit stream interleaves corrections to coefficients an
/// earlier scan already found with the run-lengths that place new ones, and a
/// correction bit is only present for a coefficient that is already non-zero.
/// Reading one bit too many or too few here desynchronises the rest of the
/// image, which is why this follows the reference structure closely.
#[allow(clippy::too_many_arguments)]
fn decode_ac_refine(
    reader: &mut BitReader,
    block: &mut [i32; 64],
    table: &HuffmanTable,
    ss: usize,
    se: usize,
    al: u32,
    eobrun: &mut u32,
) -> bool {
    let shift = al.min(14);
    let positive = 1i32 << shift;
    let negative = -(1i32 << shift);

    let mut k = ss;
    if *eobrun == 0 {
        while k <= se {
            let Some(rs) = reader.huffman(table) else {
                return false;
            };
            let mut run = i32::from(rs >> 4);
            let size = rs & 0x0F;

            let mut new_value = 0i32;
            if size == 0 {
                if run < 15 {
                    *eobrun = 1u32 << (run.clamp(0, 14) as u32);
                    if run > 0 {
                        *eobrun = eobrun.saturating_add(reader.bits(run as u32) as u32);
                    }
                    break;
                }
                // run == 15 with no size: skip sixteen zero coefficients,
                // correcting any non-zero ones passed on the way.
            } else {
                // The magnitude is always one bit in a refinement scan; the
                // bit that follows is its sign.
                new_value = if reader.bit() == 1 {
                    positive
                } else {
                    negative
                };
            }

            while k <= se {
                let coefficient = block.get(k).copied().unwrap_or(0);
                if coefficient != 0 {
                    if reader.bit() == 1 && (coefficient & positive) == 0 {
                        if let Some(slot) = block.get_mut(k) {
                            *slot = if coefficient >= 0 {
                                coefficient.saturating_add(positive)
                            } else {
                                coefficient.saturating_add(negative)
                            };
                        }
                    }
                } else {
                    if run == 0 {
                        if new_value != 0 {
                            if let Some(slot) = block.get_mut(k) {
                                *slot = new_value;
                            }
                        }
                        k += 1;
                        break;
                    }
                    run -= 1;
                }
                k += 1;
            }

            if reader.exhausted {
                return false;
            }
        }
    }

    if *eobrun > 0 {
        // Inside an end-of-band run no new coefficients appear, but the ones
        // already there still get their correction bit.
        while k <= se {
            let coefficient = block.get(k).copied().unwrap_or(0);
            if coefficient != 0 && reader.bit() == 1 && (coefficient & positive) == 0 {
                if let Some(slot) = block.get_mut(k) {
                    *slot = if coefficient >= 0 {
                        coefficient.saturating_add(positive)
                    } else {
                        coefficient.saturating_add(negative)
                    };
                }
            }
            k += 1;
        }
        *eobrun -= 1;
    }
    true
}

#[allow(clippy::too_many_arguments)]
fn decode_ac_progressive(
    reader: &mut BitReader,
    component: &mut Component,
    block: &mut [i32; 64],
    ac_tables: &[HuffmanTable],
    ss: usize,
    se: usize,
    ah: u32,
    al: u32,
    eobrun: &mut u32,
) -> bool {
    let Some(table) = ac_tables.get(component.ac_table) else {
        return false;
    };
    if ah == 0 {
        decode_ac_first(reader, block, table, ss, se, al, eobrun)
    } else {
        decode_ac_refine(reader, block, table, ss, se, al, eobrun)
    }
}

/// Where a [`Painter`]'s rows go: straight into a whole image, or a strip at
/// a time to a [`JpegSink`].
trait Rows {
    /// The image's layout, once, before the first rows are asked for.
    fn begin(&mut self, layout: JpegLayout);
    /// The buffer `count` rows of `row_bytes` from row `y` are painted into.
    fn rows(&mut self, y: usize, count: usize, row_bytes: usize) -> &mut [u8];
    /// The rows [`Rows::rows`] last handed out are painted.
    fn painted(&mut self, y: usize, count: usize, row_bytes: usize);
    /// Bytes held beside the image the rows go to.
    fn held(&self) -> usize;
}

/// The rows painted into the whole image [`decode`] returns.
#[derive(Default)]
struct Collected {
    data: Vec<u8>,
}

impl Rows for Collected {
    fn begin(&mut self, layout: JpegLayout) {
        let samples = (layout.width as usize)
            .saturating_mul(layout.height as usize)
            .saturating_mul(layout.color.components());
        self.data = vec![0u8; samples];
    }

    fn rows(&mut self, y: usize, count: usize, row_bytes: usize) -> &mut [u8] {
        let start = y.saturating_mul(row_bytes);
        let end = start.saturating_add(count.saturating_mul(row_bytes));
        self.data.get_mut(start..end).unwrap_or_default()
    }

    fn painted(&mut self, _: usize, _: usize, _: usize) {}

    fn held(&self) -> usize {
        0
    }
}

/// The rows painted into one strip and handed to a sink, by
/// [`decode_strips`].
struct Forwarded<'s> {
    sink: &'s mut dyn JpegSink,
    strip: Vec<u8>,
    /// The most bytes `strip` has held.
    held: usize,
}

impl Rows for Forwarded<'_> {
    fn begin(&mut self, layout: JpegLayout) {
        self.sink.begin(layout);
    }

    fn rows(&mut self, _: usize, count: usize, row_bytes: usize) -> &mut [u8] {
        let bytes = count.saturating_mul(row_bytes);
        self.strip.resize(bytes, 0);
        self.held = self.held.max(bytes);
        &mut self.strip
    }

    fn painted(&mut self, y: usize, count: usize, row_bytes: usize) {
        let bytes = count.saturating_mul(row_bytes);
        let strip = self.strip.get(..bytes).unwrap_or_default();
        self.sink.strip(y as u32, count as u32, strip);
    }

    fn held(&self) -> usize {
        self.held
    }
}

/// Turns the finished coefficients into pixels at the scan's scale, one MCU
/// row at a time (`docs/design/jpeg-speed.md`, "Strips"), into `target`.
///
/// Every check this makes comes before the first strip, so a target that is
/// handed a strip is handed the whole image. The ceiling is held to the
/// frame's own samples at every scale.
fn finish(
    scanned: Scanned,
    max_output: usize,
    target: &mut dyn Rows,
) -> Result<JpegFrame, JpegError> {
    let Scanned {
        components,
        quant,
        width,
        height,
        adobe_seen,
        adobe_transform,
        precision,
        scale,
        mut warnings,
        streamed,
    } = scanned;
    if let Some(Streamed {
        layout,
        held,
        transformed,
    }) = streamed
    {
        // Painted as its scan was decoded, every check below made first.
        if precision > 8 {
            warnings.push(Warning::JpegPrecisionNarrowed);
        }
        return Ok(JpegFrame {
            layout,
            warnings,
            held,
            transformed,
        });
    }
    if width == 0 || height == 0 || components.is_empty() {
        return Err(JpegError::Truncated);
    }

    let Some(color) = colour_of(components.len(), adobe_seen) else {
        return Err(JpegError::UnsupportedComponents);
    };

    let needed = width
        .saturating_mul(height)
        .saturating_mul(components.len());
    if needed > max_output {
        warnings.push(Warning::OutputCapHit);
        return Err(JpegError::Truncated);
    }

    if precision > 8 {
        // Ruling 10: the samples handed out are narrower than the frame's, and
        // that is a leniency rather than a decode. Recorded once.
        warnings.push(Warning::JpegPrecisionNarrowed);
    }

    let layout = layout_at(width, height, color, precision, scale);
    target.begin(layout);
    let mut painter = Painter::new(&components, layout, adobe_transform.unwrap_or(0));
    for strip in 0..painter.strips() {
        painter.paint(strip, &components, &quant, target);
    }

    let coefficients = components
        .iter()
        .map(|component| component.coeffs.len().saturating_mul(2))
        .fold(0, usize::saturating_add);
    Ok(JpegFrame {
        layout,
        warnings,
        held: JpegHeld {
            coefficients,
            planes: painter.held(),
            strip: target.held(),
        },
        transformed: painter.transformed,
    })
}

/// One component's samples for the strip being painted.
struct Plane {
    /// Image columns one of this component's samples covers, and image rows:
    /// T.81 A.1.1's `Hmax / H` and `Vmax / V`, rounded down as a ratio that
    /// does not divide leaves them, at every scale.
    sx: usize,
    sy: usize,
    /// The component's blocks a row, and its block rows.
    blocks_x: usize,
    blocks_y: usize,
    /// `N`: the samples a block gives along each side at the painter's scale.
    n: usize,
    /// The first block row the plane holds for the strip being painted, and
    /// how many it holds.
    first: usize,
    held: usize,
    /// Lines of the image's width, `N` a block row the plane has room for,
    /// each sample replicated across the image columns it covers as it is
    /// written. A column no block covers — only a ratio that does not divide
    /// leaves one — keeps the 128 the plane was filled with.
    samples: Vec<u8>,
}

impl Plane {
    /// The line image row `y` reads, or `None` when no block row of this
    /// component covers it.
    fn line(&self, y: usize, width: usize) -> Option<&[u8]> {
        let row = y / self.sy;
        let block_row = row / self.n;
        if block_row < self.first || block_row >= self.first + self.held {
            return None;
        }
        let at = ((block_row - self.first) * self.n + row % self.n) * width;
        self.samples.get(at..at + width)
    }
}

/// The block rows of a component the image rows of strip `strip` read, each
/// block row `n` of the component's own rows: `first..last`, empty when none
/// of them is a row the component has.
fn block_rows(
    strip: usize,
    strip_rows: usize,
    height: usize,
    sy: usize,
    n: usize,
    blocks_y: usize,
) -> (usize, usize) {
    let top = strip * strip_rows;
    let bottom = (top + strip_rows).min(height);
    if top >= bottom {
        return (0, 0);
    }
    let first = top / sy / n;
    let last = ((bottom - 1) / sy / n + 1).min(blocks_y);
    (first, last.max(first))
}

/// Paints a frame's strips from its coefficients at a scale: each component's
/// block rows a strip reads, dequantised, transformed and replicated into its
/// plane, then each image row converted from the planes into the strip.
struct Painter {
    /// The image's width and height: the frame's at full size, `⌈X·N/8⌉` and
    /// `⌈Y·N/8⌉` at `N/8`.
    width: usize,
    height: usize,
    color: JpegColor,
    /// The Adobe marker's transform, 2 for YCCK.
    transform: u8,
    precision: u8,
    /// The scale the blocks are transformed at.
    scale: JpegScale,
    /// The zig-zag positions a block's top-left `N × N` coefficients reach.
    span: usize,
    /// Image rows a strip has: one MCU row, `N · Vmax`.
    strip_rows: usize,
    planes: Vec<Plane>,
    /// A line of 128s, for an image row no block row of a component covers,
    /// which only a ratio that does not divide leaves.
    grey: Vec<u8>,
    /// Samples the inverse transforms have output, every component's.
    transformed: u64,
}

impl Painter {
    /// The painter of `layout`'s image from `components`, `transform` being
    /// the Adobe marker's.
    fn new(components: &[Component], layout: JpegLayout, transform: u8) -> Painter {
        let (width, height) = (layout.width as usize, layout.height as usize);
        let n = layout.scale.block_samples() as usize;
        let h_max = components.iter().map(|c| c.h).max().unwrap_or(1).max(1);
        let v_max = components.iter().map(|c| c.v).max().unwrap_or(1).max(1);
        let strip_rows = n * v_max;
        let strips = height.div_ceil(strip_rows);
        let mut uncovered = false;
        let planes = components
            .iter()
            .map(|component| {
                let sy = v_max / component.v.max(1);
                let most = (0..strips)
                    .map(|strip| {
                        let (first, last) =
                            block_rows(strip, strip_rows, height, sy, n, component.blocks_y);
                        last - first
                    })
                    .max()
                    .unwrap_or(0);
                uncovered |= component.blocks_y.saturating_mul(n).saturating_mul(sy) < height;
                Plane {
                    sx: h_max / component.h.max(1),
                    sy,
                    blocks_x: component.blocks_x,
                    blocks_y: component.blocks_y,
                    n,
                    first: 0,
                    held: 0,
                    samples: vec![128u8; most.saturating_mul(n).saturating_mul(width)],
                }
            })
            .collect();
        Painter {
            width,
            height,
            color: layout.color,
            transform,
            precision: layout.precision,
            scale: layout.scale,
            span: kept_span(n),
            strip_rows,
            planes,
            grey: if uncovered {
                vec![128u8; width]
            } else {
                Vec::new()
            },
            transformed: 0,
        }
    }

    /// How many strips the image is.
    fn strips(&self) -> usize {
        self.height.div_ceil(self.strip_rows)
    }

    /// Bytes of planes held.
    fn held(&self) -> usize {
        self.planes
            .iter()
            .map(|plane| plane.samples.len())
            .fold(self.grey.len(), usize::saturating_add)
    }

    /// Paints strip `strip` into `target`.
    fn paint(
        &mut self,
        strip: usize,
        components: &[Component],
        quant: &[[u16; 64]; 4],
        target: &mut dyn Rows,
    ) {
        let top = strip * self.strip_rows;
        let bottom = (top + self.strip_rows).min(self.height);
        if top >= bottom {
            return;
        }
        let (width, height, strip_rows) = (self.width, self.height, self.strip_rows);
        let (precision, scale, span) = (self.precision, self.scale, self.span);
        for (plane, component) in self.planes.iter_mut().zip(components) {
            let (first, last) =
                block_rows(strip, strip_rows, height, plane.sy, plane.n, plane.blocks_y);
            plane.first = first;
            plane.held = last - first;
            let table = quant.get(component.quant).copied().unwrap_or([1; 64]);
            for by in first..last {
                let row = BlockRow {
                    plane: &mut *plane,
                    component,
                    table: &table,
                    by,
                    width,
                    precision,
                };
                self.transformed += match scale {
                    JpegScale::Full => transform_row(row),
                    JpegScale::Half => transform_row_reduced(row, span, &REDUCED.four),
                    JpegScale::Quarter => transform_row_reduced(row, span, &REDUCED.two),
                    // Every block at 1/8 takes its DC level, which reads no
                    // cosine.
                    JpegScale::Eighth => transform_row_reduced::<1>(row, span, &[[0]]),
                };
            }
        }

        let row_bytes = width * self.color.components();
        let rows = bottom - top;
        let out = target.rows(top, rows, row_bytes);
        for (line, y) in out.chunks_exact_mut(row_bytes).zip(top..bottom) {
            let mut lines: [&[u8]; 4] = [&[]; 4];
            for (slot, plane) in lines.iter_mut().zip(&self.planes) {
                *slot = plane.line(y, width).unwrap_or(self.grey.as_slice());
            }
            convert(self.color, self.transform, &lines, line);
        }
        target.painted(top, rows, row_bytes);
    }
}

/// Dequantises a block out of zig-zag order and into the natural one the
/// transform expects.
fn dequantise(source: &[i16], table: &[u16; 64], block: &mut [i32; 64]) {
    block.fill(0);
    for (&coefficient, &z) in source.iter().zip(ZIGZAG.iter()) {
        if coefficient == 0 {
            continue;
        }
        let q = i32::from(table.get(z).copied().unwrap_or(1));
        if let Some(slot) = block.get_mut(z) {
            *slot = i32::from(coefficient).saturating_mul(q);
        }
    }
}

/// One block row a transform paints.
struct BlockRow<'r> {
    /// The plane it is painted into.
    plane: &'r mut Plane,
    /// The component its blocks are read from, and its quantisation table.
    component: &'r Component,
    table: &'r [u16; 64],
    /// The block row.
    by: usize,
    /// The image's width.
    width: usize,
    /// The frame's precision, 8 or 12.
    precision: u8,
}

/// Transforms a block row of a component into its place in its plane at
/// full size, returning the samples the transform output.
fn transform_row(row: BlockRow<'_>) -> u64 {
    let BlockRow {
        plane,
        component,
        table,
        by,
        width,
        precision,
    } = row;
    let base = (by - plane.first) * 8;
    let mut block = [0i32; 64];
    let mut pixels = [0u8; 64];
    let mut transformed = 0u64;
    for bx in 0..plane.blocks_x {
        let origin = bx * 8 * plane.sx;
        if origin >= width {
            // Every block further right is past the image too.
            break;
        }
        match component.block(bx, by) {
            Some(source) => {
                dequantise(source, table, &mut block);
                idct_block(&block, precision, &mut pixels);
                transformed += 64;
            }
            // A block the buffer does not hold leaves the 128 a plane starts
            // as.
            None => pixels = [128; 64],
        }
        for (py, samples) in pixels.chunks_exact(8).enumerate() {
            let at = (base + py) * width;
            if let Some(line) = plane.samples.get_mut(at..at + width) {
                replicate(line, samples, origin, plane.sx);
            }
        }
    }
    transformed
}

/// Transforms a block row of a component into its place in its plane at
/// `N/8`: each block's top-left `N × N` coefficients, which the zig-zag
/// positions below `span` hold, into `N × N` samples by `cosines`
/// ([`reduced_block`]), returning the samples the transform output.
fn transform_row_reduced<const N: usize>(
    row: BlockRow<'_>,
    span: usize,
    cosines: &[[i64; N]; N],
) -> u64 {
    let BlockRow {
        plane,
        component,
        table,
        by,
        width,
        precision,
    } = row;
    let base = (by - plane.first) * N;
    let mut transformed = 0u64;
    for bx in 0..plane.blocks_x {
        let origin = bx * N * plane.sx;
        if origin >= width {
            // Every block further right is past the image too.
            break;
        }
        let samples = match component.block(bx, by) {
            Some(source) => {
                transformed += (N * N) as u64;
                reduced_block(&kept(source, table, span), cosines, precision)
            }
            // A block the buffer does not hold leaves the 128 a plane starts
            // as.
            None => [[128; N]; N],
        };
        for (py, line_samples) in samples.iter().enumerate() {
            let at = (base + py) * width;
            if let Some(line) = plane.samples.get_mut(at..at + width) {
                replicate(line, line_samples, origin, plane.sx);
            }
        }
    }
    transformed
}

/// The top-left `N × N` of a block's coefficients, `[v][u]`, out of zig-zag
/// order and dequantised as [`dequantise`] dequantises them: the zig-zag
/// positions below `span` hold every one of them.
fn kept<const N: usize>(source: &[i16], table: &[u16; 64], span: usize) -> [[i32; N]; N] {
    let mut kept = [[0i32; N]; N];
    for (&coefficient, &z) in source.iter().zip(ZIGZAG.iter()).take(span) {
        if coefficient == 0 {
            continue;
        }
        if let Some(slot) = kept.get_mut(z / 8).and_then(|row| row.get_mut(z % 8)) {
            let q = i32::from(table.get(z).copied().unwrap_or(1));
            *slot = i32::from(coefficient).saturating_mul(q);
        }
    }
    kept
}

/// How many zig-zag positions a block's top-left `n × n` coefficients reach:
/// one past the last of them in T.81 Figure A.6's order.
fn kept_span(n: usize) -> usize {
    ZIGZAG
        .iter()
        .rposition(|&z| z / 8 < n && z % 8 < n)
        .map_or(0, |k| k + 1)
}

/// Writes one block row's samples into `line` from column `origin`, each
/// across `sx` columns, as far as the image's width.
fn replicate(line: &mut [u8], row: &[u8], origin: usize, sx: usize) {
    if sx == 1 {
        let end = (origin + row.len()).min(line.len());
        if let (Some(target), Some(source)) = (line.get_mut(origin..end), row.get(..end - origin)) {
            target.copy_from_slice(source);
        }
        return;
    }
    for (px, &value) in row.iter().enumerate() {
        let start = origin + px * sx;
        let end = (start + sx).min(line.len());
        if start >= end {
            break;
        }
        if let Some(target) = line.get_mut(start..end) {
            target.fill(value);
        }
    }
}

/// T.871: the JFIF YCbCr to RGB conversion, in `f32`, each expression in its
/// order of operations.
fn ycc_to_rgb(y: u8, cb: u8, cr: u8) -> [u8; 3] {
    let y = f32::from(y);
    let cb = f32::from(cb) - 128.0;
    let cr = f32::from(cr) - 128.0;
    [
        (y + 1.402 * cr).clamp(0.0, 255.0) as u8,
        (y - 0.344_136 * cb - 0.714_136 * cr).clamp(0.0, 255.0) as u8,
        (y + 1.772 * cb).clamp(0.0, 255.0) as u8,
    ]
}

/// Interleaves one image row from the components' lines into `out`,
/// converting colour where the model calls for it.
fn convert(color: JpegColor, transform: u8, lines: &[&[u8]; 4], out: &mut [u8]) {
    let [first, second, third, fourth] = *lines;
    match color {
        JpegColor::Gray => {
            for (slot, &y) in out.iter_mut().zip(first) {
                *slot = y;
            }
        }
        JpegColor::Rgb => {
            for (pixel, ((&y, &cb), &cr)) in out
                .chunks_exact_mut(3)
                .zip(first.iter().zip(second).zip(third))
            {
                pixel.copy_from_slice(&ycc_to_rgb(y, cb, cr));
            }
        }
        JpegColor::Cmyk | JpegColor::CmykInverted => {
            for (pixel, (((&a, &b), &c), &k)) in out
                .chunks_exact_mut(4)
                .zip(first.iter().zip(second).zip(third).zip(fourth))
            {
                // Transform 2 means the first three components are YCCK and
                // need the same conversion as YCbCr before use.
                let values = if transform == 2 {
                    let [r, g, b] = ycc_to_rgb(a, b, c);
                    [r, g, b, k]
                } else {
                    [a, b, c, k]
                };
                for (slot, value) in pixel.iter_mut().zip(values) {
                    // Adobe writes CMYK inverted; undo it so callers get ink
                    // values that mean what they say.
                    *slot = if color == JpegColor::CmykInverted {
                        255 - value
                    } else {
                        value
                    };
                }
            }
        }
    }
}

/// The largest dequantised coefficient, in magnitude, whose block the
/// transform sums in `i32` (`docs/design/jpeg-speed.md`, "The lane
/// transform").
///
/// Every row of [`COS_TABLE`] sums to 43 284 in magnitude, so with every
/// coefficient at most this a row pass's sums stay within 18 779 × 43 284 =
/// 812 830 236, its floored results within 49 612, and a column pass's sums
/// within 49 612 × 43 284 = 2 147 405 808, all under 2^31.
/// `the_lane_bound_keeps_every_sum_inside_i32` derives it from the table.
const LANE_BOUND: u32 = 18_779;

/// Which rows of a dequantised block hold what, a bit a row. The transform's
/// shortcuts read nothing else, and each is exact: what one skips is a
/// product of zero.
#[derive(Clone, Copy, Debug)]
struct Profile {
    /// The largest coefficient, in magnitude.
    largest: u32,
    /// Rows holding any coefficient.
    rows: u8,
    /// Rows holding an AC term, `u` at least 1.
    ac: u8,
    /// Rows holding a term in their upper half, `u` at least 4.
    high: u8,
}

impl Profile {
    fn of(input: &[i32; 64]) -> Profile {
        let mut profile = Profile {
            largest: 0,
            rows: 0,
            ac: 0,
            high: 0,
        };
        for (row, coefficients) in input.chunks_exact(8).enumerate() {
            let (mut any, mut ac, mut high) = (0u32, 0u32, 0u32);
            for (u, &value) in coefficients.iter().enumerate() {
                let magnitude = value.unsigned_abs();
                profile.largest = profile.largest.max(magnitude);
                any |= magnitude;
                if u >= 1 {
                    ac |= magnitude;
                }
                if u >= 4 {
                    high |= magnitude;
                }
            }
            let bit = 1u8 << row;
            if any != 0 {
                profile.rows |= bit;
            }
            if ac != 0 {
                profile.ac |= bit;
            }
            if high != 0 {
                profile.high |= bit;
            }
        }
        profile
    }
}

/// The [`COS_TABLE`] entries an eight-point pass multiplies by.
///
/// The table holds `COS_TABLE[7 − x][u] = (−1)^u · COS_TABLE[x][u]`, so
/// outputs `x` and `7 − x` share every product and differ in the odd half's
/// sign; and within the even half `COS_TABLE[x][0]` is `k0` for every `x`,
/// `COS_TABLE[x][4]` is `±k4` and `COS_TABLE[x][2]` and `COS_TABLE[x][6]` are
/// each `±k2` or `±k6`. `the_cosine_table_has_the_symmetry_the_lanes_share`
/// holds the table to all of it.
struct Kernel {
    k0: i32,
    k2: i32,
    k4: i32,
    k6: i32,
    /// `COS_TABLE[x][u]` for `x` below four and the odd `u`, in order.
    odd: [[i32; 4]; 4],
}

static KERNEL: std::sync::LazyLock<Kernel> = std::sync::LazyLock::new(|| {
    let entry = |x: usize, u: usize| COS_TABLE.get(x * 8 + u).copied().unwrap_or(0);
    let odd = |x: usize| [entry(x, 1), entry(x, 3), entry(x, 5), entry(x, 7)];
    Kernel {
        k0: entry(0, 0),
        k2: entry(0, 2),
        k4: entry(0, 4),
        k6: entry(0, 6),
        odd: [odd(0), odd(1), odd(2), odd(3)],
    }
});

/// What an eight-point pass sums in: `i32` within [`LANE_BOUND`], `i64`
/// past it, and [`Lanes`] for the column pass's eight columns at once.
trait Sum:
    Copy + core::ops::Add<Output = Self> + core::ops::Sub<Output = Self> + core::ops::Mul<Output = Self>
{
    /// A table entry, as this type.
    fn of(entry: i32) -> Self;
}

impl Sum for i32 {
    fn of(entry: i32) -> i32 {
        entry
    }
}

impl Sum for i64 {
    fn of(entry: i32) -> i64 {
        i64::from(entry)
    }
}

/// Eight `i32` lanes, a column each: the column pass, one operation for all
/// eight columns.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Lanes([i32; 8]);

impl Lanes {
    fn with(self, other: Lanes, f: impl Fn(i32, i32) -> i32) -> Lanes {
        let mut out = [0i32; 8];
        for ((slot, a), b) in out.iter_mut().zip(self.0).zip(other.0) {
            *slot = f(a, b);
        }
        Lanes(out)
    }
}

impl core::ops::Add for Lanes {
    type Output = Lanes;

    fn add(self, other: Lanes) -> Lanes {
        self.with(other, |a, b| a + b)
    }
}

impl core::ops::Sub for Lanes {
    type Output = Lanes;

    fn sub(self, other: Lanes) -> Lanes {
        self.with(other, |a, b| a - b)
    }
}

impl core::ops::Mul for Lanes {
    type Output = Lanes;

    fn mul(self, other: Lanes) -> Lanes {
        self.with(other, |a, b| a * b)
    }
}

impl Sum for Lanes {
    fn of(entry: i32) -> Lanes {
        Lanes([entry; 8])
    }
}

/// One eight-point pass's sums, before its floor: outputs `x` and `7 − x` as
/// `E(x) ± O(x)`, from six even products and sixteen odd ones, or, when the
/// upper half of the input is zero (`half`), three and eight.
///
/// Integer sums are the same in any grouping, so each output is the sum
/// today's transform makes, product for product.
#[inline(always)]
fn butterfly<T: Sum>(v: &[T; 8], kernel: &Kernel, half: bool) -> [T; 8] {
    let [v0, v1, v2, v3, v4, v5, v6, v7] = *v;
    let p0 = v0 * T::of(kernel.k0);
    let a = v2 * T::of(kernel.k2);
    let b = v2 * T::of(kernel.k6);
    let (e0, e1, e2, e3) = if half {
        (p0 + a, p0 + b, p0 - b, p0 - a)
    } else {
        let p4 = v4 * T::of(kernel.k4);
        let c = v6 * T::of(kernel.k6);
        let d = v6 * T::of(kernel.k2);
        (
            p0 + p4 + a + c,
            p0 - p4 + b - d,
            p0 - p4 - b + d,
            p0 + p4 - a - c,
        )
    };
    let odd = |entries: &[i32; 4]| {
        let low = v1 * T::of(entries[0]) + v3 * T::of(entries[1]);
        if half {
            low
        } else {
            low + v5 * T::of(entries[2]) + v7 * T::of(entries[3])
        }
    };
    let [o0, o1, o2, o3] = [
        odd(&kernel.odd[0]),
        odd(&kernel.odd[1]),
        odd(&kernel.odd[2]),
        odd(&kernel.odd[3]),
    ];
    [
        e0 + o0,
        e1 + o1,
        e2 + o2,
        e3 + o3,
        e3 - o3,
        e2 - o2,
        e1 - o1,
        e0 - o0,
    ]
}

// Multiplies the transform made on this thread, for the count's test.
#[cfg(test)]
thread_local! {
    static MULTIPLIES: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

/// Counts `n` multiplies, in the test build.
#[inline(always)]
fn multiplied(n: u64) {
    #[cfg(test)]
    MULTIPLIES.with(|count| count.set(count.get() + n));
    let _ = n;
}

/// The multiplies a pass over the rows a `rows` bit set holds makes, in the
/// shape the column pass takes.
fn column_shape(rows: u8) -> (bool, bool) {
    // Only row 0: each column is its constant. Nothing in rows 4 to 7: the
    // half shape.
    (rows == 1, rows & 0xF0 == 0)
}

/// The inverse DCT of one block, separable and in integers: rows then
/// columns, with fixed-point cosines, each pass floored by `>> 14`.
///
/// Every product is the one a direct sum makes, made once
/// (`docs/design/jpeg-speed.md`, "The lane transform"): `direct::idct_block`
/// is that direct sum, and `the_lane_transform_is_the_direct_transform_on_every_block`
/// holds this to it. Determinism matters more here than the last unit of
/// precision (ruling 4).
fn idct_block(input: &[i32; 64], precision: u8, out: &mut [u8; 64]) {
    // A.3.1's level shift is `2^(P-1)`, and the clamp is to the frame's own
    // range. A 12-bit sample is then narrowed to the eight this crate hands
    // out -- see [`JpegImage::data`] -- which for `P = 8` is a shift of zero
    // and leaves the byte untouched.
    let half = 1i64 << (precision - 1);
    let ceiling = (1i64 << precision) - 1;
    let narrow = precision - 8;
    let level = |sum: i64| (((sum >> 14) + half).clamp(0, ceiling) >> narrow) as u8;

    let profile = Profile::of(input);
    let kernel = &*KERNEL;
    if profile.rows == 0 {
        out.fill(level(0));
        return;
    }
    if profile.rows == 1 && profile.ac & 1 == 0 {
        // No AC term: one row of one product, then one column of one.
        multiplied(2);
        out.fill(dc_level(input.first().copied().unwrap_or(0), precision));
        return;
    }
    if profile.largest <= LANE_BOUND {
        lane_transform(input, profile, kernel, &level, out);
    } else {
        wide_transform(input, profile, kernel, &level, out);
    }
}

/// The row pass's sums for one row, in the shape its profile allows.
#[inline(always)]
fn row_sums<T: Sum>(v: &[T; 8], kernel: &Kernel, ac: bool, high: bool) -> [T; 8] {
    if !ac {
        // No AC term: every output is the DC's one product.
        multiplied(1);
        return [v[0] * T::of(kernel.k0); 8];
    }
    multiplied(if high { 22 } else { 11 });
    butterfly(v, kernel, !high)
}

/// [`idct_block`] for a block within [`LANE_BOUND`]: rows in `i32`, then the
/// eight columns as lanes.
fn lane_transform(
    input: &[i32; 64],
    profile: Profile,
    kernel: &Kernel,
    level: &impl Fn(i64) -> u8,
    out: &mut [u8; 64],
) {
    let mut rows = [Lanes::default(); 8];
    for (index, (row, coefficients)) in rows.iter_mut().zip(input.chunks_exact(8)).enumerate() {
        let bit = 1u8 << index;
        if profile.rows & bit == 0 {
            continue;
        }
        let v = <[i32; 8]>::try_from(coefficients).unwrap_or([0; 8]);
        let sums = row_sums(&v, kernel, profile.ac & bit != 0, profile.high & bit != 0);
        *row = Lanes(sums.map(|sum| sum >> 14));
    }

    let (constant, half) = column_shape(profile.rows);
    let sums = if constant {
        multiplied(8);
        [rows[0] * Lanes::of(kernel.k0); 8]
    } else {
        multiplied(if half { 88 } else { 176 });
        butterfly(&rows, kernel, half)
    };
    for (line, sum) in out.chunks_exact_mut(8).zip(sums) {
        for (slot, sum) in line.iter_mut().zip(sum.0) {
            *slot = level(i64::from(sum));
        }
    }
}

/// [`idct_block`] for a block past [`LANE_BOUND`]: the same shapes in `i64`,
/// the row pass's floored sums truncated to `i32` as the direct transform's
/// are.
fn wide_transform(
    input: &[i32; 64],
    profile: Profile,
    kernel: &Kernel,
    level: &impl Fn(i64) -> u8,
    out: &mut [u8; 64],
) {
    let mut rows = [[0i32; 8]; 8];
    for (index, (row, coefficients)) in rows.iter_mut().zip(input.chunks_exact(8)).enumerate() {
        let bit = 1u8 << index;
        if profile.rows & bit == 0 {
            continue;
        }
        let v = <[i32; 8]>::try_from(coefficients)
            .unwrap_or([0; 8])
            .map(i64::from);
        let sums = row_sums(&v, kernel, profile.ac & bit != 0, profile.high & bit != 0);
        *row = sums.map(|sum| (sum >> 14) as i32);
    }

    let (constant, half) = column_shape(profile.rows);
    multiplied(if constant {
        8
    } else if half {
        88
    } else {
        176
    });
    for column in 0..8 {
        let v = rows.map(|row| row.get(column).copied().map_or(0, i64::from));
        let sums = if constant {
            [v[0] * i64::from(kernel.k0); 8]
        } else {
            butterfly(&v, kernel, half)
        };
        for (y, sum) in sums.iter().enumerate() {
            if let Some(slot) = out.get_mut(y * 8 + column) {
                *slot = level(*sum);
            }
        }
    }
}

/// The level every sample of a block with no AC term takes: the full
/// transform's own constant, `S(0, 0)` times [`COS_TABLE`]'s DC entry floored
/// by `>> 14`, times it again and floored again, then level-shifted by
/// `2^(P−1)`, clamped to the frame's range and narrowed to eight bits.
///
/// [`idct_block`] takes it for a block whose only coefficient is its DC, and
/// [`reduced_block`] for one whose kept coefficients have no AC term, as every
/// block at 1/8 has none: so a flat block is the same level at every scale
/// (`docs/design/jpeg-speed.md`, "The reduced transforms").
fn dc_level(dc: i32, precision: u8) -> u8 {
    let k0 = i64::from(KERNEL.k0);
    let half = 1i64 << (precision - 1);
    let ceiling = (1i64 << precision) - 1;
    let row = (i64::from(dc) * k0) >> 14;
    ((((row * k0) >> 14) + half).clamp(0, ceiling) >> (precision - 8)) as u8
}

/// The largest magnitude a kept coefficient enters a reduced transform with:
/// `2^20`, so that each pass's sums stay inside `i64` — a row's four terms of
/// at most `2^20 · 2^15`, a column's four of `2^37 · 2^15`.
const REDUCED_BOUND: i32 = 1 << 20;

/// The reduced transforms' cosines, `K_N[x][u]`, in 1/65536.
struct Reduced {
    /// `K_4`, for 1/2.
    four: [[i64; 4]; 4],
    /// `K_2`, for 1/4.
    two: [[i64; 2]; 2],
}

/// `K_N[x][u] = round(2^16 · C(u)/2 · cos((2x+1)uπ/2N))` for `N` of 4 and 2
/// (`docs/design/jpeg-speed.md`, "The reduced transforms").
///
/// Computed once at first use rather than written out, as [`COS_TABLE`] is,
/// so the values cannot drift from their formula. Every entry is far enough
/// from a rounding boundary that every target's `cos` rounds it alike, which
/// `the_reduced_cosines_match_their_formula` holds.
static REDUCED: std::sync::LazyLock<Reduced> = std::sync::LazyLock::new(|| Reduced {
    four: cosines(),
    two: cosines(),
});

/// `K_N`, for the `N` its type names.
fn cosines<const N: usize>() -> [[i64; N]; N] {
    let mut table = [[0i64; N]; N];
    for (x, row) in table.iter_mut().enumerate() {
        for (u, slot) in row.iter_mut().enumerate() {
            let cu = if u == 0 {
                1.0 / std::f64::consts::SQRT_2
            } else {
                1.0
            };
            let angle = (2.0 * x as f64 + 1.0) * u as f64 * std::f64::consts::PI / (2.0 * N as f64);
            *slot = (cu / 2.0 * angle.cos() * 65536.0).round() as i64;
        }
    }
    table
}

/// A block's top-left `N × N` dequantised coefficients, `kept[v][u]`, as
/// `N × N` samples, `[y][x]`: the reduced transform at `N/8`
/// (`docs/design/jpeg-speed.md`, "The reduced transforms").
///
/// Each kept coefficient is clamped to [`REDUCED_BOUND`]; the row pass sums
/// `S · K_N` exactly in `i64`, the column pass sums `K_N` times those exactly,
/// and the one rounding is the last — add `2^31`, floor by `>> 32` — before
/// the level shift, the clamp to the frame's range and the narrowing to eight
/// bits. A block whose kept coefficients have no AC term takes [`dc_level`],
/// and at 1/8 every block is one.
fn reduced_block<const N: usize>(
    kept: &[[i32; N]; N],
    cosines: &[[i64; N]; N],
    precision: u8,
) -> [[u8; N]; N] {
    let dc = kept
        .first()
        .and_then(|row| row.first())
        .copied()
        .unwrap_or(0);
    let ac = kept
        .iter()
        .flatten()
        .skip(1)
        .any(|&coefficient| coefficient != 0);
    if !ac {
        return [[dc_level(dc, precision); N]; N];
    }
    let half = 1i64 << (precision - 1);
    let ceiling = (1i64 << precision) - 1;
    let narrow = precision - 8;

    // The row pass: `rows[v][x]` is the sum over `u` of `S(u, v) · K_N[x][u]`.
    let mut rows = [[0i64; N]; N];
    for (sums, coefficients) in rows.iter_mut().zip(kept) {
        for (sum, cosine) in sums.iter_mut().zip(cosines) {
            *sum = coefficients
                .iter()
                .zip(cosine)
                .map(|(&s, &k)| i64::from(s.clamp(-REDUCED_BOUND, REDUCED_BOUND)) * k)
                .sum();
        }
    }

    // The column pass, `[y][x]` the sum over `v` of `rows[v][x] · K_N[y][v]`,
    // and the one rounding, out of 2^32.
    let mut out = [[0u8; N]; N];
    for (line, cosine) in out.iter_mut().zip(cosines) {
        for (x, slot) in line.iter_mut().enumerate() {
            let sum: i64 = rows
                .iter()
                .zip(cosine)
                .map(|(row, &k)| row.get(x).copied().unwrap_or(0) * k)
                .sum();
            let level = ((sum + (1 << 31)) >> 32) + half;
            *slot = (level.clamp(0, ceiling) >> narrow) as u8;
        }
    }
    out
}

/// `cos((2x+1) · u · π / 16) · C(u) / 2`, in 1/16384.
///
/// Computed once at first use rather than written out, so the values cannot
/// drift from the formula they are supposed to be.
static COS_TABLE: std::sync::LazyLock<[i32; 64]> = std::sync::LazyLock::new(|| {
    let mut table = [0i32; 64];
    for x in 0..8usize {
        for u in 0..8usize {
            let cu = if u == 0 {
                1.0 / std::f64::consts::SQRT_2
            } else {
                1.0
            };
            let value =
                cu / 2.0 * ((2.0 * x as f64 + 1.0) * u as f64 * std::f64::consts::PI / 16.0).cos();
            if let Some(slot) = table.get_mut(x * 8 + u) {
                *slot = (value * 16384.0).round() as i32;
            }
        }
    }
    table
});

#[cfg(test)]
mod tests {
    use super::*;

    /// A 1×1 grey JPEG, hand-assembled: the smallest thing that exercises the
    /// whole path from marker parsing to a decoded pixel.
    fn tiny_gray() -> Vec<u8> {
        let mut out = vec![0xFF, 0xD8];

        // DQT: all ones, so coefficients pass through unscaled.
        out.extend_from_slice(&[0xFF, 0xDB, 0x00, 0x43, 0x00]);
        out.extend_from_slice(&[1u8; 64]);

        // SOF0: 8-bit, 1×1, one component with id 1, no subsampling. The
        // length says nine body bytes and there are nine — this fixture used
        // to declare eleven and supply eight, so the component descriptor was
        // read out of the following marker and matched nothing in the scan.
        out.extend_from_slice(&[0xFF, 0xC0, 0x00, 0x0B, 0x08, 0x00, 0x01, 0x00, 0x01, 0x01]);
        out.extend_from_slice(&[0x01, 0x11, 0x00]);

        // DHT for DC: one code of length 2, value 0.
        let mut dht = vec![0x00];
        let mut counts = [0u8; 16];
        counts[1] = 1;
        dht.extend_from_slice(&counts);
        dht.push(0x00);
        out.extend_from_slice(&[0xFF, 0xC4]);
        out.extend_from_slice(&((dht.len() + 2) as u16).to_be_bytes());
        out.extend_from_slice(&dht);

        // DHT for AC: one code of length 2, value 0 (EOB).
        let mut dht = vec![0x10];
        dht.extend_from_slice(&counts);
        dht.push(0x00);
        out.extend_from_slice(&[0xFF, 0xC4]);
        out.extend_from_slice(&((dht.len() + 2) as u16).to_be_bytes());
        out.extend_from_slice(&dht);

        // SOS.
        out.extend_from_slice(&[0xFF, 0xDA, 0x00, 0x08, 0x01, 0x01, 0x00, 0x00, 0x3F, 0x00]);
        // Entropy data: DC code 00 (size 0, so difference zero), then EOB.
        out.extend_from_slice(&[0b0000_0000]);
        out.extend_from_slice(&[0xFF, 0xD9]);
        out
    }

    #[test]
    fn a_minimal_grayscale_image_decodes() {
        let image = decode(&tiny_gray(), 1 << 20).expect("it decodes");
        assert_eq!((image.width, image.height), (1, 1));
        assert_eq!(image.color, JpegColor::Gray);
        assert_eq!(image.data.len(), 1);
        // A DC of zero is mid-grey after the level shift.
        assert_eq!(image.data.first().copied(), Some(128));
    }

    #[test]
    fn markers_that_are_not_jpeg_are_refused() {
        assert_eq!(decode(&[], 1 << 20).err(), Some(JpegError::NotJpeg));
        assert_eq!(
            decode(b"not a jpeg", 1 << 20).err(),
            Some(JpegError::NotJpeg)
        );
    }

    /// [`tiny_gray`] with the frame marker and the sample precision chosen.
    ///
    /// The SOF sits after SOI and a 69-byte DQT, and is found rather than
    /// counted so that a change to the fixture above cannot silently move it.
    fn tiny_gray_at(marker: u8, precision: u8) -> Vec<u8> {
        let mut out = tiny_gray();
        let at = out
            .windows(2)
            .position(|w| w == [0xFF, 0xC0])
            .expect("the fixture has an SOF0");
        out[at + 1] = marker;
        out[at + 4] = precision;
        out
    }

    /// **B.2.2 allows twelve bits, and only outside the baseline frame.**
    ///
    /// `P` is 8 for SOF0 and 8 or 12 for SOF1 and SOF2, so the same header at
    /// twelve bits is a legal extended-sequential frame and an illegal
    /// baseline one. Both are asserted, because accepting 12 everywhere would
    /// read a corrupt baseline header as a valid frame.
    #[test]
    fn twelve_bits_are_read_outside_the_baseline_frame_and_refused_inside_it() {
        assert_eq!(
            decode(&tiny_gray_at(0xC0, 12), 1 << 20).err(),
            Some(JpegError::UnsupportedPrecision),
            "SOF0 at twelve bits is not a baseline frame"
        );
        let image = decode(&tiny_gray_at(0xC1, 12), 1 << 20).expect("SOF1 at twelve bits decodes");
        assert_eq!(image.precision, 12);
        assert_eq!(
            image.warnings,
            vec![Warning::JpegPrecisionNarrowed],
            "the narrowing to eight bits is recorded"
        );
        // A DC of zero is mid-grey after A.3.1's level shift, which at twelve
        // bits is 2048 of 4095 -- and 2048 >> 4 is 128, the same byte the
        // eight-bit path gives. That is the point: the narrowing is a shift,
        // not a rescale, so mid-grey stays mid-grey.
        assert_eq!(image.data.first().copied(), Some(128));

        for bits in [1u8, 4, 9, 16] {
            assert_eq!(
                decode(&tiny_gray_at(0xC1, bits), 1 << 20).err(),
                Some(JpegError::UnsupportedPrecision),
                "{bits} bits"
            );
        }
    }

    /// **Every frame type this build does not decode refuses by its own
    /// name**, rather than being skipped.
    ///
    /// Once only SOF9, SOF10 and SOF11 were named. SOF3, SOF5, SOF6, SOF7,
    /// SOF13, SOF14 and SOF15 fell through to the unknown-segment arm, were
    /// stepped over as though they were a comment, and the decode then failed
    /// as `Truncated` -- a lossless JPEG reported as a damaged file.
    ///
    /// **The four arithmetic refusals moved in September 2026**, when SOF9 and
    /// SOF10 started decoding. They are refused by the annex they need rather
    /// than by the entropy coder they use: SOF11 and SOF15 need Annex H's
    /// predictor and SOF13 and SOF14 Annex J's hierarchical progression, and
    /// `crate::qm` decodes all four of their decision streams perfectly well
    /// with nothing to hand them to. The two that decode are listed here too,
    /// so the boundary is one table rather than two.
    #[test]
    fn every_frame_type_this_build_declines_refuses_by_its_own_name() {
        for (marker, expected) in [
            (0xC3u8, JpegError::Lossless),
            (0xC5, JpegError::Differential),
            (0xC6, JpegError::Differential),
            (0xC7, JpegError::Lossless),
            (0xCB, JpegError::Lossless),
            (0xCD, JpegError::Differential),
            (0xCE, JpegError::Differential),
            (0xCF, JpegError::Lossless),
        ] {
            assert_eq!(
                decode(&tiny_gray_at(marker, 8), 1 << 20).err(),
                Some(expected),
                "SOF marker {marker:#04x}"
            );
        }

        // SOF9 and SOF10 are frames now. `tiny_gray` carries a Huffman-coded
        // scan, so relabelling its SOF does not make a decodable file -- what
        // is asserted is only that neither marker is refused for its coder.
        for marker in [0xC9u8, 0xCA] {
            assert!(
                !matches!(
                    decode(&tiny_gray_at(marker, 8), 1 << 20).err(),
                    Some(JpegError::Lossless) | Some(JpegError::Differential)
                ),
                "SOF marker {marker:#04x} is a frame this build decodes"
            );
        }
    }

    #[test]
    fn an_output_cap_is_honoured() {
        // A 1x1 image needs one byte; a cap of zero refuses it.
        assert!(decode(&tiny_gray(), 0).is_err());
    }

    #[test]
    fn the_cosine_table_matches_its_formula() {
        // The DC basis is constant: cos(0) · (1/√2) / 2.
        let expected = (1.0 / std::f64::consts::SQRT_2 / 2.0 * 16384.0).round() as i32;
        for x in 0..8usize {
            assert_eq!(COS_TABLE.get(x * 8), Some(&expected), "row {x}");
        }
    }

    #[test]
    fn a_flat_block_inverts_to_a_flat_image() {
        // Only the DC coefficient: every pixel should be the same.
        let mut block = [0i32; 64];
        block[0] = 8 * 16; // an arbitrary DC level
        let mut pixels = [0u8; 64];
        idct_block(&block, 8, &mut pixels);

        let first = pixels.first().copied().unwrap_or(0);
        assert!(
            pixels.iter().all(|&p| p.abs_diff(first) <= 1),
            "a DC-only block should be flat, got {pixels:?}"
        );
        assert!(first > 128, "a positive DC brightens the block");
    }

    #[test]
    fn arbitrary_bytes_terminate_without_panicking() {
        for len in 0..512usize {
            let data: Vec<u8> = (0..len).map(|i| ((i * 31) % 256) as u8).collect();
            let _ = decode(&data, 1 << 16);
        }
        // A valid header followed by garbage.
        let mut damaged = tiny_gray();
        damaged.truncate(damaged.len() / 2);
        let _ = decode(&damaged, 1 << 20);

        for cut in 0..tiny_gray().len() {
            let mut truncated = tiny_gray();
            truncated.truncate(cut);
            let _ = decode(&truncated, 1 << 20);
        }
    }

    // ---- Progressive ----------------------------------------------------
    //
    // The fixtures below encode one 8×8 block carrying the same two
    // coefficients three different ways: sequentially, progressively in two
    // scans, and progressively with successive approximation in four. All
    // three must decode to the same pixels, which is a far stronger assertion
    // than any single expected value — it says the scans reassemble into the
    // coefficients the encoder meant, without needing to agree with anyone
    // about what the IDCT should produce from them.

    /// Writes entropy-coded bits, stuffing a zero after every 0xFF.
    #[derive(Default)]
    struct Bits {
        out: Vec<u8>,
        acc: u32,
        held: u32,
    }

    impl Bits {
        fn bit(&mut self, value: u32) {
            self.acc = (self.acc << 1) | (value & 1);
            self.held += 1;
            if self.held == 8 {
                let byte = self.acc as u8;
                self.out.push(byte);
                if byte == 0xFF {
                    self.out.push(0x00);
                }
                self.acc = 0;
                self.held = 0;
            }
        }

        fn push(&mut self, value: u32, length: u32) {
            for i in (0..length).rev() {
                self.bit((value >> i) & 1);
            }
        }

        /// Pads to a byte boundary with ones, which is what an encoder does.
        fn finish(mut self) -> Vec<u8> {
            while self.held != 0 {
                self.bit(1);
            }
            self.out
        }
    }

    fn marker(out: &mut Vec<u8>, code: u8, body: &[u8]) {
        out.extend_from_slice(&[0xFF, code]);
        out.extend_from_slice(&((body.len() + 2) as u16).to_be_bytes());
        out.extend_from_slice(body);
    }

    /// DC table: "00" → size 3, "01" → size 2.
    /// AC table: "00" → run 0 size 3, "01" → EOB with run 0, "10" → run 0
    /// size 2. The size-2 codes exist because a successive-approximation scan
    /// sends a value with its low bit removed, and a magnitude that fits in
    /// three bits usually does not fit in three bits once halved.
    fn tables(out: &mut Vec<u8>) {
        let mut dc_counts = [0u8; 16];
        dc_counts[1] = 2;
        let mut dc = vec![0x00];
        dc.extend_from_slice(&dc_counts);
        dc.extend_from_slice(&[0x03, 0x02]);
        marker(out, 0xC4, &dc);

        let mut ac_counts = [0u8; 16];
        ac_counts[1] = 3;
        let mut ac = vec![0x10];
        ac.extend_from_slice(&ac_counts);
        ac.extend_from_slice(&[0x03, 0x00, 0x02]);
        marker(out, 0xC4, &ac);
    }

    fn header(out: &mut Vec<u8>, sof: u8) {
        out.extend_from_slice(&[0xFF, 0xD8]);

        let mut dqt = vec![0x00];
        dqt.extend_from_slice(&[1u8; 64]);
        marker(out, 0xDB, &dqt);

        // 8×8, one component with id 1, no subsampling, quantisation table 0.
        marker(
            out,
            sof,
            &[0x08, 0x00, 0x08, 0x00, 0x08, 0x01, 0x01, 0x11, 0x00],
        );
        tables(out);
    }

    /// Sequential: DC 5, then AC 5 at zig-zag index 1, then end of block.
    fn sequential_block() -> Vec<u8> {
        let mut out = Vec::new();
        header(&mut out, 0xC0);
        marker(&mut out, 0xDA, &[0x01, 0x01, 0x00, 0x00, 0x3F, 0x00]);

        let mut bits = Bits::default();
        bits.push(0b00, 2); // DC size 3
        bits.push(5, 3); // difference +5
        bits.push(0b00, 2); // AC run 0 size 3
        bits.push(5, 3); // value +5 at index 1
        bits.push(0b01, 2); // EOB
        out.extend_from_slice(&bits.finish());

        out.extend_from_slice(&[0xFF, 0xD9]);
        out
    }

    /// Progressive, no successive approximation: one DC scan, one AC scan.
    fn progressive_block() -> Vec<u8> {
        let mut out = Vec::new();
        header(&mut out, 0xC2);

        // DC scan: band 0..0, Ah 0, Al 0.
        marker(&mut out, 0xDA, &[0x01, 0x01, 0x00, 0x00, 0x00, 0x00]);
        let mut bits = Bits::default();
        bits.push(0b00, 2); // size 3
        bits.push(5, 3); // difference +5
        out.extend_from_slice(&bits.finish());

        // AC scan: band 1..63, Ah 0, Al 0.
        marker(&mut out, 0xDA, &[0x01, 0x01, 0x00, 0x01, 0x3F, 0x00]);
        let mut bits = Bits::default();
        bits.push(0b00, 2); // run 0 size 3
        bits.push(5, 3); // value +5 at index 1
        bits.push(0b01, 2); // EOB run of one
        out.extend_from_slice(&bits.finish());

        out.extend_from_slice(&[0xFF, 0xD9]);
        out
    }

    /// Progressive with successive approximation: every coefficient arrives
    /// one bit short and is completed by a refinement scan.
    fn progressive_refined_block() -> Vec<u8> {
        let mut out = Vec::new();
        header(&mut out, 0xC2);

        // DC first scan, Al 1: sends 5 >> 1 = 2.
        marker(&mut out, 0xDA, &[0x01, 0x01, 0x00, 0x00, 0x00, 0x01]);
        let mut bits = Bits::default();
        bits.push(0b01, 2); // size 2
        bits.push(0b10, 2); // difference +2
        out.extend_from_slice(&bits.finish());

        // AC first scan, band 1..63, Al 1: sends 5 >> 1 = 2, stored as 4.
        marker(&mut out, 0xDA, &[0x01, 0x01, 0x00, 0x01, 0x3F, 0x01]);
        let mut bits = Bits::default();
        bits.push(0b10, 2); // run 0 size 2
        bits.push(0b10, 2); // +2, which the point transform stores as 4
        bits.push(0b01, 2); // EOB run of one
        out.extend_from_slice(&bits.finish());

        // DC refinement, Ah 1 Al 0: the low bit, making 5.
        marker(&mut out, 0xDA, &[0x01, 0x01, 0x00, 0x00, 0x00, 0x10]);
        let mut bits = Bits::default();
        bits.push(1, 1);
        out.extend_from_slice(&bits.finish());

        // AC refinement, band 1..63, Ah 1 Al 0: an EOB run, then the
        // correction bit for the one coefficient already there.
        marker(&mut out, 0xDA, &[0x01, 0x01, 0x00, 0x01, 0x3F, 0x10]);
        let mut bits = Bits::default();
        bits.push(0b01, 2); // EOB run of one
        bits.push(1, 1); // correction for index 1
        out.extend_from_slice(&bits.finish());

        out.extend_from_slice(&[0xFF, 0xD9]);
        out
    }

    #[test]
    fn a_progressive_image_decodes_at_all() {
        let image = decode(&progressive_block(), 1 << 20).expect("progressive decodes");
        assert_eq!((image.width, image.height), (8, 8));
        assert_eq!(image.color, JpegColor::Gray);
        assert_eq!(image.data.len(), 64);
        assert!(
            image.data.iter().any(|&p| p != 128),
            "the coefficients reached the pixels"
        );
    }

    #[test]
    fn progressive_scans_reassemble_the_sequential_image() {
        let sequential = decode(&sequential_block(), 1 << 20).expect("sequential decodes");
        let progressive = decode(&progressive_block(), 1 << 20).expect("progressive decodes");
        assert_eq!(
            sequential.data, progressive.data,
            "the same coefficients sent in two scans give the same pixels"
        );
    }

    #[test]
    fn successive_approximation_refines_to_the_same_image() {
        let sequential = decode(&sequential_block(), 1 << 20).expect("sequential decodes");
        let refined = decode(&progressive_refined_block(), 1 << 20).expect("refined decodes");
        assert_eq!(
            sequential.data, refined.data,
            "the refinement scans supply the bits the first scans left out"
        );
    }

    /// A progressive file cut short must still produce the coarse image the
    /// scans it did carry describe — that is the entire point of the format,
    /// and refusing it outright would be worse than what came before.
    #[test]
    fn a_truncated_progressive_file_keeps_what_it_decoded() {
        let full = progressive_refined_block();
        // Cut after the first two scans: the DC and AC first passes.
        let cut = full.len() - 12;
        let partial = decode(&full[..cut], 1 << 20).expect("the partial file still decodes");

        assert_eq!(partial.data.len(), 64);
        assert!(
            partial.data.iter().any(|&p| p != 128),
            "the scans that did arrive were used"
        );

        let refined = decode(&full, 1 << 20).expect("the whole file decodes");
        assert_ne!(
            partial.data, refined.data,
            "and the missing refinement is visible as a coarser image"
        );
    }

    #[test]
    fn progressive_garbage_terminates_without_panicking() {
        let full = progressive_refined_block();
        for cut in 0..full.len() {
            let _ = decode(&full[..cut], 1 << 20);
        }
        for (seed, byte) in full.iter().enumerate() {
            let mut damaged = full.clone();
            if let Some(slot) = damaged.get_mut(seed) {
                *slot = byte.wrapping_add(97);
            }
            let _ = decode(&damaged, 1 << 20);
        }
    }

    // ---------------------------------------------------------------- encoder
    //
    // Everything below is the baseline encoder in `jpeg/encode.rs`. Each test's
    // own doc comment says which link it adjudicates with third-party data and
    // which it only holds to itself; there is no test here that presents an
    // encode-then-decode round trip as adjudication.

    use super::encode::{
        canonical_codes, category, forward_dct_quantise, pad_plane, rgb_to_ycbcr, HuffSpec,
        ANNEX_K1_LUMINANCE, ANNEX_K2_CHROMINANCE, ANNEX_K3_DC_LUMA, ANNEX_K4_DC_CHROMA,
        ANNEX_K5_AC_LUMA, ANNEX_K6_AC_CHROMA,
    };

    /// Hex, one byte per two characters, spaces ignored.
    fn hex(text: &str) -> Vec<u8> {
        let digits: Vec<u8> = text
            .bytes()
            .filter(|b| !b.is_ascii_whitespace())
            .map(|b| match b {
                b'0'..=b'9' => b - b'0',
                b'A'..=b'F' => b - b'A' + 10,
                b'a'..=b'f' => b - b'a' + 10,
                other => panic!("not hex: {other}"),
            })
            .collect();
        digits
            .chunks(2)
            .map(|pair| (pair[0] << 4) | pair[1])
            .collect()
    }

    /// The entropy-coded segment: everything between the SOS segment and EOI.
    fn entropy_segment(bytes: &[u8]) -> Vec<u8> {
        let mut at = 2; // past SOI
        while at + 3 < bytes.len() {
            assert_eq!(bytes[at], 0xFF, "expected a marker at {at}");
            let code = bytes[at + 1];
            let length = usize::from(bytes[at + 2]) << 8 | usize::from(bytes[at + 3]);
            if code == 0xDA {
                let start = at + 2 + length;
                let end = bytes.len() - 2;
                assert_eq!(&bytes[end..], &[0xFF, 0xD9], "EOI");
                return bytes[start..end].to_vec();
            }
            at += 2 + length;
        }
        panic!("no SOS");
    }

    /// Every marker segment's payload, keyed by marker code, in order.
    fn segments(bytes: &[u8]) -> Vec<(u8, Vec<u8>)> {
        let mut out = Vec::new();
        let mut at = 2;
        while at + 3 < bytes.len() {
            let code = bytes[at + 1];
            let length = usize::from(bytes[at + 2]) << 8 | usize::from(bytes[at + 3]);
            out.push((code, bytes[at + 4..at + 2 + length].to_vec()));
            if code == 0xDA {
                break;
            }
            at += 2 + length;
        }
        out
    }

    fn gray(width: u32, height: u32, value: u8) -> Vec<u8> {
        vec![value; (width * height) as usize]
    }

    fn gray_source(data: &[u8], width: u32, height: u32) -> JpegSource<'_> {
        JpegSource {
            width,
            height,
            colour: JpegSourceColour::Gray,
            stride: width as usize,
            data,
        }
    }

    /// T.81 Figure A.6 as printed: for each natural (row-major) coefficient
    /// position, the index at which the zig-zag sequence visits it. That is the
    /// **inverse** of [`ZIGZAG`], which is what makes the test below a check on
    /// the shipped table rather than a copy of it.
    const FIGURE_A_6: [usize; 64] = [
        0, 1, 5, 6, 14, 15, 27, 28, //
        2, 4, 7, 13, 16, 26, 29, 42, //
        3, 8, 12, 17, 25, 30, 41, 43, //
        9, 11, 18, 24, 31, 40, 44, 53, //
        10, 19, 23, 32, 39, 45, 52, 54, //
        20, 22, 33, 38, 46, 51, 55, 60, //
        21, 34, 37, 47, 50, 56, 59, 61, //
        35, 36, 48, 49, 57, 58, 62, 63,
    ];

    /// **Adjudicated by third-party data.** T.81 Figure A.6, read twice — text
    /// layer and `tpdf render --dpi 200`'s page 30 — from the same
    /// `T-REC-T.81` the tables below come from. Both readings agree on all 64
    /// cells.
    ///
    /// This test exists because a counted injection found it missing. Swapping
    /// two entries of [`ZIGZAG`] fired **nothing**: the decoder scatters with
    /// the same table the encoder gathers with, so a round trip is blind to any
    /// permutation of it, and every other test here had been written in terms of
    /// [`ZIGZAG`] rather than in terms of the figure. Holding the shipped table
    /// to its own inverse, transcribed independently, is what closes that — and
    /// `a_grayscale_datastream_carries_annex_k_s_published_table_bytes` and
    /// `the_forward_dct_matches_a_3_3_s_equation` were rewritten to build their
    /// expectations from `FIGURE_A_6` for the same reason.
    #[test]
    fn the_zig_zag_order_is_figure_a_6_s() {
        for (natural, &k) in FIGURE_A_6.iter().enumerate() {
            assert_eq!(
                ZIGZAG[k], natural,
                "the zig-zag sequence's step {k} is not Figure A.6's cell {natural}"
            );
        }
        // A.6 is a permutation of the 64 positions, which is the property a
        // transposition preserves and a duplicated entry does not.
        let mut seen = [false; 64];
        for &k in FIGURE_A_6.iter() {
            assert!(!seen[k], "step {k} appears twice");
            seen[k] = true;
        }
    }

    /// **Adjudicated by third-party data.** T.81 Tables K.1 and K.2, from
    /// `T-REC-T.81` (<https://www.w3.org/Graphics/JPEG/itu-t81.pdf>, W3C's copy
    /// of CCITT Rec. T.81 (1992) | ISO/IEC 10918-1 : 1993), fetched 15 September
    /// 2026 and read twice: once from the text layer `tpdf text` extracts, once
    /// off `tpdf render --dpi 200`'s page 143. The two readings agree on all 128
    /// entries, and the transcription below is the rendered one.
    ///
    /// The second reading was not a formality. T.81's tables are typeset with
    /// column rules that the text layer emits as a literal `1`, so K.1's first
    /// row arrives as `16111016124140151161` and resolves into
    /// `16 11 10 16 24 40 51 61` only against the picture.
    #[test]
    fn the_quantisation_tables_are_itu_t_t_81_annex_k_s() {
        let k1: [u8; 64] = [
            16, 11, 10, 16, 24, 40, 51, 61, //
            12, 12, 14, 19, 26, 58, 60, 55, //
            14, 13, 16, 24, 40, 57, 69, 56, //
            14, 17, 22, 29, 51, 87, 80, 62, //
            18, 22, 37, 56, 68, 109, 103, 77, //
            24, 35, 55, 64, 81, 104, 113, 92, //
            49, 64, 78, 87, 103, 121, 120, 101, //
            72, 92, 95, 98, 112, 100, 103, 99,
        ];
        let k2: [u8; 64] = [
            17, 18, 24, 47, 99, 99, 99, 99, //
            18, 21, 26, 66, 99, 99, 99, 99, //
            24, 26, 56, 99, 99, 99, 99, 99, //
            47, 66, 99, 99, 99, 99, 99, 99, //
            99, 99, 99, 99, 99, 99, 99, 99, //
            99, 99, 99, 99, 99, 99, 99, 99, //
            99, 99, 99, 99, 99, 99, 99, 99, //
            99, 99, 99, 99, 99, 99, 99, 99,
        ];
        assert_eq!(ANNEX_K1_LUMINANCE, k1, "Table K.1");
        assert_eq!(ANNEX_K2_CHROMINANCE, k2, "Table K.2");
        // B.2.4.1 gives Qk the range 1 to 255 at Pq = 0.
        assert!(k1.iter().chain(k2.iter()).all(|&q| q >= 1));
    }

    /// **Adjudicated by third-party data.** T.81 K.3.3.1 and K.3.3.2 print the
    /// BITS and HUFFVAL lists of Tables K.3 to K.6 as hexadecimal byte strings,
    /// and B.2.4.2 makes those strings the DHT payload. Read twice from the same
    /// document as the test above: text layer, and `tpdf render`'s pages 158 and
    /// 159. Both readings agree on all 396 bytes.
    ///
    /// These strings, and not Tables K.3 to K.6's typeset grids, are what the
    /// shipped tables are held to, because running text extracts unambiguously
    /// where a ruled grid does not.
    #[test]
    fn the_huffman_tables_are_itu_t_t_81_annex_k_s() {
        let cases: [(&HuffSpec, &str, &str, &str); 4] = [
            (
                &ANNEX_K3_DC_LUMA,
                "K.3 luminance DC",
                "00010501010101010100000000000000",
                "000102030405060708090A0B",
            ),
            (
                &ANNEX_K4_DC_CHROMA,
                "K.4 chrominance DC",
                "00030101010101010101010000000000",
                "000102030405060708090A0B",
            ),
            (
                &ANNEX_K5_AC_LUMA,
                "K.5 luminance AC",
                "0002010303020403050504040000017D",
                "01020300041105122131410613516107\
                 227114328191A1082342B1C11552D1F0\
                 2433627282090A161718191A25262728\
                 292A3435363738393A43444546474849\
                 4A535455565758595A63646566676869\
                 6A737475767778797A83848586878889\
                 8A92939495969798999AA2A3A4A5A6A7\
                 A8A9AAB2B3B4B5B6B7B8B9BAC2C3C4C5\
                 C6C7C8C9CAD2D3D4D5D6D7D8D9DAE1E2\
                 E3E4E5E6E7E8E9EAF1F2F3F4F5F6F7F8\
                 F9FA",
            ),
            (
                &ANNEX_K6_AC_CHROMA,
                "K.6 chrominance AC",
                "00020102040403040705040400010277",
                "000102031104 05213106124151076171\
                 1322328108144291A1B1C109233352F0\
                 156272D10A162434E125F11718191A26\
                 2728292A35363738393A434445464748\
                 494A535455565758595A6364 65666768\
                 696A737475767778797A828384858687\
                 88898A92939495969798999AA2A3A4A5\
                 A6A7A8A9AAB2B3B4B5 B6B7B8B9BAC2C3\
                 C4C5C6C7C8C9CAD2D3D4D5D6D7D8D9DA\
                 E2E3E4E5E6E7E8E9EAF2F3F4F5F6F7F8\
                 F9FA",
            ),
        ];

        for (spec, name, bits, values) in cases {
            assert_eq!(spec.bits.as_slice(), hex(bits).as_slice(), "{name} BITS");
            assert_eq!(spec.values, hex(values).as_slice(), "{name} HUFFVAL");
            // C.2: the counts and the value list have to describe the same
            // table, which is also the check that catches a dropped byte.
            let total: usize = spec.bits.iter().map(|&n| usize::from(n)).sum();
            assert_eq!(total, spec.values.len(), "{name} BITS sum");
        }
        assert_eq!(ANNEX_K5_AC_LUMA.values.len(), 162);
        assert_eq!(ANNEX_K6_AC_CHROMA.values.len(), 162);
    }

    /// **Adjudicated by third-party data.** T.81 Tables K.3 and K.4 print, for
    /// every one of their 24 symbols, a code length *and* a binary code word.
    /// Those 24 code words are transcribed here and required to be what Annex
    /// C's canonical assignment produces from the BITS and HUFFVAL of the test
    /// above — two independent presentations of the same table in the same
    /// Recommendation, which is why this is a check and not a restatement.
    ///
    /// Read twice, text layer and `tpdf render`'s page 149. The text layer
    /// carries the same column-rule `1` that Table K.1 does, so
    /// `1641110` is category 6, length 4, code word `1110`.
    ///
    /// Tables K.5 and K.6 print 324 more code words the same way. **Sheet 1 of
    /// 4 of Table K.5 — 40 of them — is transcribed here too**, read twice from
    /// the text layer and from `tpdf render`'s page 150; that sample reaches
    /// nine of the sixteen code lengths, including the 16-bit group whose codes
    /// are the last ones the canonical assignment produces and therefore the
    /// ones a mis-stepped `code <<= 1` would land wrongest. The remaining three
    /// sheets and all of K.6 are left to the BITS and HUFFVAL above, which
    /// determine them.
    #[test]
    fn the_canonical_codes_are_tables_k_3_and_k_4_s_printed_code_words() {
        // (category, code length, code word) exactly as Table K.3 prints them.
        let k3: [(u8, u8, &str); 12] = [
            (0, 2, "00"),
            (1, 3, "010"),
            (2, 3, "011"),
            (3, 3, "100"),
            (4, 3, "101"),
            (5, 3, "110"),
            (6, 4, "1110"),
            (7, 5, "11110"),
            (8, 6, "111110"),
            (9, 7, "1111110"),
            (10, 8, "11111110"),
            (11, 9, "111111110"),
        ];
        // Table K.4.
        let k4: [(u8, u8, &str); 12] = [
            (0, 2, "00"),
            (1, 2, "01"),
            (2, 2, "10"),
            (3, 3, "110"),
            (4, 4, "1110"),
            (5, 5, "11110"),
            (6, 6, "111110"),
            (7, 7, "1111110"),
            (8, 8, "11111110"),
            (9, 9, "111111110"),
            (10, 10, "1111111110"),
            (11, 11, "11111111110"),
        ];
        // Table K.5, the whole of sheet 1 of 4: run/size, length, code word.
        let k5: [(u8, u8, &str); 40] = [
            (0x00, 4, "1010"), // EOB
            (0x01, 2, "00"),
            (0x02, 2, "01"),
            (0x03, 3, "100"),
            (0x04, 4, "1011"),
            (0x05, 5, "11010"),
            (0x06, 7, "1111000"),
            (0x07, 8, "11111000"),
            (0x08, 10, "1111110110"),
            (0x09, 16, "1111111110000010"),
            (0x0A, 16, "1111111110000011"),
            (0x11, 4, "1100"),
            (0x12, 5, "11011"),
            (0x13, 7, "1111001"),
            (0x14, 9, "111110110"),
            (0x15, 11, "11111110110"),
            (0x16, 16, "1111111110000100"),
            (0x17, 16, "1111111110000101"),
            (0x18, 16, "1111111110000110"),
            (0x19, 16, "1111111110000111"),
            (0x1A, 16, "1111111110001000"),
            (0x21, 5, "11100"),
            (0x22, 8, "11111001"),
            (0x23, 10, "1111110111"),
            (0x24, 12, "111111110100"),
            (0x25, 16, "1111111110001001"),
            (0x26, 16, "1111111110001010"),
            (0x27, 16, "1111111110001011"),
            (0x28, 16, "1111111110001100"),
            (0x29, 16, "1111111110001101"),
            (0x2A, 16, "1111111110001110"),
            (0x31, 6, "111010"),
            (0x32, 9, "111110111"),
            (0x33, 12, "111111110101"),
            (0x34, 16, "1111111110001111"),
            (0x35, 16, "1111111110010000"),
            (0x36, 16, "1111111110010001"),
            (0x37, 16, "1111111110010010"),
            (0x38, 16, "1111111110010011"),
            (0x39, 16, "1111111110010100"),
        ];

        for (spec, printed, name) in [
            (&ANNEX_K3_DC_LUMA, k3.as_slice(), "K.3"),
            (&ANNEX_K4_DC_CHROMA, k4.as_slice(), "K.4"),
        ] {
            let codes = canonical_codes(spec);
            for &(value, length, word) in printed {
                let code = codes[usize::from(value)];
                assert_eq!(code.length, length, "{name} value {value} length");
                assert_eq!(
                    format!("{:0width$b}", code.bits, width = usize::from(length)),
                    word,
                    "{name} value {value} code word"
                );
            }
        }

        let codes = canonical_codes(&ANNEX_K5_AC_LUMA);
        for &(value, length, word) in k5.iter() {
            let code = codes[usize::from(value)];
            assert_eq!(code.length, length, "K.5 run/size {value:02X} length");
            assert_eq!(
                format!("{:0width$b}", code.bits, width = usize::from(length)),
                word,
                "K.5 run/size {value:02X} code word"
            );
        }
    }

    /// **Adjudicated by third-party data.** B.2.4.1 makes a DQT payload
    /// `Pq`/`Tq` then the table in zig-zag order, and B.2.4.2 makes a DHT
    /// payload `Tc`/`Th` then BITS then HUFFVAL. So the bytes T.81 K.1 and
    /// K.3.3 print appear in this encoder's output verbatim, and this test finds
    /// them there — encoder output against published bytes, with no decoder on
    /// either side.
    #[test]
    fn a_grayscale_datastream_carries_annex_k_s_published_table_bytes() {
        let pixels = gray(8, 8, 128);
        let bytes = jpeg_encode(&gray_source(&pixels, 8, 8), &JpegOptions::default()).unwrap();
        let found = segments(&bytes);

        let dqt = &found.iter().find(|(code, _)| *code == 0xDB).unwrap().1;
        assert_eq!(dqt[0], 0x00, "Pq = 0, Tq = 0");
        let mut zigzagged = [0u8; 64];
        for (natural, &k) in FIGURE_A_6.iter().enumerate() {
            zigzagged[k] = ANNEX_K1_LUMINANCE[natural];
        }
        assert_eq!(
            &dqt[1..],
            zigzagged.as_slice(),
            "Table K.1 in Figure A.6's order"
        );

        let dhts: Vec<&(u8, Vec<u8>)> = found.iter().filter(|(code, _)| *code == 0xC4).collect();
        assert_eq!(dhts.len(), 2, "a grayscale frame needs one DC and one AC");

        let mut dc = vec![0x00u8];
        dc.extend_from_slice(&hex("00010501010101010100000000000000"));
        dc.extend_from_slice(&hex("000102030405060708090A0B"));
        assert_eq!(dhts[0].1, dc, "K.3.3.1's published bytes");

        let mut ac = vec![0x10u8];
        ac.extend_from_slice(&hex("0002010303020403050504040000017D"));
        ac.extend_from_slice(ANNEX_K5_AC_LUMA.values);
        assert_eq!(dhts[1].1, ac, "K.3.3.2's published bytes");

        // B.2.2's SOF0: P, Y, X, Nf, then one component.
        let sof = &found.iter().find(|(code, _)| *code == 0xC0).unwrap().1;
        assert_eq!(sof.as_slice(), &[8, 0, 8, 0, 8, 1, 1, 0x11, 0]);
    }

    /// **Adjudicated by third-party data.** Two complete entropy-coded segments
    /// whose every bit follows from the standard alone: A.3.3's equation, K.1's
    /// quantiser, the code words Tables K.3 and K.5 print, and B.1.1.5 NOTE 1's
    /// 1-bit padding. Nothing in this repository is consulted to produce the
    /// expected bytes, and no decoder runs.
    ///
    /// A flat 8x8 block of 128 level-shifts to zero, so every coefficient is
    /// zero: `DIFF = 0` is K.3 category 0, code word `00`; the rest of the block
    /// is K.5's 0/0 EOB, code word `1010`. Six bits, padded to `00101011`.
    ///
    /// A flat block of 144 has `s = 16` everywhere, so A.3.3 gives
    /// `S00 = (1/4)(1/sqrt 2)(1/sqrt 2) x 64 x 16 = 128` and every other
    /// coefficient zero; K.1's `Q00 = 16` makes `Sq00 = 8`. Category 4 is K.3's
    /// `101`, the four additional bits of F.1.2.1.1 are `1000`, then EOB
    /// `1010` — eleven bits, padded to `10110001 01011111`.
    #[test]
    fn the_flat_block_datastreams_are_annex_k_s_own_code_words() {
        for (value, expected) in [(128u8, vec![0x2Bu8]), (144, vec![0xB1, 0x5F])] {
            let pixels = gray(8, 8, value);
            let bytes = jpeg_encode(&gray_source(&pixels, 8, 8), &JpegOptions::default()).unwrap();
            assert_eq!(entropy_segment(&bytes), expected, "a flat block of {value}");
        }
    }

    /// A.3.3's FDCT, transcribed here in `f64` from the equation on the
    /// rendered page 27, against the fixed-point transform the encoder runs.
    ///
    /// **This is the standard's formula, not the standard's numbers.** ITU-T
    /// T.83's compliance data — the published vector set that would adjudicate
    /// these coefficients — is on three MS-DOS diskettes bundled with the paid
    /// Recommendation (T.83 clause 4.4) and could not be obtained; the module
    /// header lists the URLs tried and what each returned. What this test can
    /// and does say is that the integer path rounds the ideal transform
    /// **correctly to within 0.012 of a quantiser step** on every coefficient of
    /// every block below — 0.5117 measured, where 0.5 is a correct rounding —
    /// which is enough to catch a transposed basis function, a quantiser
    /// applied on the wrong side of the transform, or a zig-zag that scans the
    /// wrong cell.
    #[test]
    fn the_forward_dct_matches_a_3_3_s_equation() {
        fn reference(samples: &[i32; 64], u: usize, v: usize) -> f64 {
            let c = |k: usize| {
                if k == 0 {
                    1.0 / std::f64::consts::SQRT_2
                } else {
                    1.0
                }
            };
            let mut sum = 0.0;
            for x in 0..8usize {
                for y in 0..8usize {
                    sum += f64::from(samples[y * 8 + x])
                        * ((2.0 * x as f64 + 1.0) * u as f64 * std::f64::consts::PI / 16.0).cos()
                        * ((2.0 * y as f64 + 1.0) * v as f64 * std::f64::consts::PI / 16.0).cos();
                }
            }
            0.25 * c(u) * c(v) * sum
        }

        let mut worst = 0.0f64;
        let mut state = 0x1234_5678u32;
        let mut next = move || {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            (state >> 24) as i32 - 128
        };

        for case in 0..24usize {
            let mut samples = [0i32; 64];
            for (i, slot) in samples.iter_mut().enumerate() {
                *slot = match case {
                    0 => 0,
                    1 => 127,
                    2 => -128,
                    3 => {
                        if (i / 8 + i % 8) % 2 == 0 {
                            127
                        } else {
                            -128
                        }
                    }
                    4 => (i as i32) - 32,
                    _ => next(),
                };
            }
            for table in [&ANNEX_K1_LUMINANCE, &ANNEX_K2_CHROMINANCE, &[1u8; 64]] {
                let mut coefficients = [0i32; 64];
                forward_dct_quantise(&samples, table, &mut coefficients);
                for (natural, &k) in FIGURE_A_6.iter().enumerate() {
                    let (v, u) = (natural / 8, natural % 8);
                    let exact = reference(&samples, u, v) / f64::from(table[natural]);
                    let error = (f64::from(coefficients[k]) - exact).abs();
                    worst = worst.max(error);
                }
            }
        }
        assert!(
            worst <= 0.55,
            "the fixed-point transform strayed {worst} from A.3.3's equation"
        );
    }

    /// Why the encoder needs no "coefficient too large" refusal, measured
    /// rather than argued.
    ///
    /// Annex K's tables code AC magnitudes up to size 10 (`|Sq| <= 1023`) and DC
    /// differences up to category 11 (`|DIFF| <= 2047`). A.3.3's transform is
    /// linear in the samples, so its extreme over the box `s in [-128, 127]` is
    /// attained at a vertex, and the vertex is known: take `127` where the basis
    /// product is positive and `-128` where it is negative. That is computed
    /// here for all 64 coefficients at the finest quantiser B.2.4.1 allows, a
    /// table of ones, which is the worst case over every legal table.
    ///
    /// The clamp in `forward_dct_quantise` is unreachable because of this, and
    /// this test is what keeps that true.
    #[test]
    fn no_quantiser_can_push_a_coefficient_past_annex_k() {
        let basis = |x: usize, u: usize| {
            let c = if u == 0 {
                1.0 / std::f64::consts::SQRT_2
            } else {
                1.0
            };
            c / 2.0 * ((2.0 * x as f64 + 1.0) * u as f64 * std::f64::consts::PI / 16.0).cos()
        };

        let mut worst_ac = 0.0f64;
        let (mut dc_high, mut dc_low) = (0.0f64, 0.0f64);
        for v in 0..8usize {
            for u in 0..8usize {
                let mut high = 0.0f64;
                let mut low = 0.0f64;
                for x in 0..8usize {
                    for y in 0..8usize {
                        let weight = basis(x, u) * basis(y, v);
                        // The vertex that maximises, and the one that minimises.
                        high += weight * if weight > 0.0 { 127.0 } else { -128.0 };
                        low += weight * if weight > 0.0 { -128.0 } else { 127.0 };
                    }
                }
                if u == 0 && v == 0 {
                    dc_high = high;
                    dc_low = low;
                } else {
                    worst_ac = worst_ac.max(high.abs()).max(low.abs());
                }
            }
        }

        assert!(
            worst_ac <= 1023.0,
            "an AC coefficient can reach {worst_ac}, past K.5's size 10"
        );
        let spread = dc_high - dc_low;
        assert!(
            spread <= 2047.0,
            "a DC difference can reach {spread}, past K.3's category 11"
        );
        // The categories those magnitudes fall in, so a changed bound is loud.
        assert_eq!(category(worst_ac.round() as i32), 10);
        assert_eq!(category(spread.round() as i32), 11);
    }

    /// **Adjudicated by third-party data.** ITU-T T.871 | ISO/IEC 10918-5
    /// clause 7's exact forward equations, read off `tpdf render`'s page 4 of
    /// `T-REC-T.871-201105-I` — fetched from the ITU 15 September 2026, by
    /// `WebFetch` where `curl` got an HTTP 500 — and written out here literally.
    ///
    /// The encoder computes the algebraically equivalent identity
    /// `Cb = (B - Y)/1.772 + 128` in fixed point, so agreement is a check on
    /// both the identity and the fixed-point precision rather than a
    /// restatement of one expression as itself. The tolerance is one level,
    /// which is what a 1/65536 fixed point buys.
    #[test]
    fn the_colour_transform_is_t_871_clause_7_s() {
        let published = |r: u8, g: u8, b: u8| {
            let (r, g, b) = (f64::from(r), f64::from(g), f64::from(b));
            let clamp = |v: f64| v.round().clamp(0.0, 255.0) as i32;
            (
                clamp(0.299 * r + 0.587 * g + 0.114 * b),
                clamp((-0.299 * r - 0.587 * g + 0.886 * b) / 1.772 + 128.0),
                clamp((0.701 * r - 0.587 * g - 0.114 * b) / 1.402 + 128.0),
            )
        };

        let mut worst = 0i32;
        for r in (0..=255u32).step_by(15) {
            for g in (0..=255u32).step_by(15) {
                for b in (0..=255u32).step_by(15) {
                    let (r, g, b) = (r as u8, g as u8, b as u8);
                    let (y, cb, cr) = rgb_to_ycbcr(r, g, b);
                    let want = published(r, g, b);
                    worst = worst
                        .max((i32::from(y) - want.0).abs())
                        .max((i32::from(cb) - want.1).abs())
                        .max((i32::from(cr) - want.2).abs());
                }
            }
        }
        assert!(
            worst <= 1,
            "the transform strayed {worst} levels from T.871"
        );

        // The three points clause 7 pins exactly, in both directions.
        assert_eq!(rgb_to_ycbcr(0, 0, 0), (0, 128, 128));
        assert_eq!(rgb_to_ycbcr(255, 255, 255), (255, 128, 128));
    }

    /// A.2.4's NOTE: "any incomplete MCUs be completed by replication of the
    /// right-most column and the bottom line of each component". Held to the
    /// clause, not to a round trip — a padding rule is only visible in the bits
    /// it costs, and a decoder discards it by A.2.4's last sentence.
    #[test]
    fn a_partial_mcu_is_completed_by_replicating_the_edge() {
        // A 3 x 2 picture in an 8 x 8 plane, with a distinct value per pixel.
        let mut plane = vec![0u8; 64];
        for y in 0..2usize {
            for x in 0..3usize {
                plane[y * 8 + x] = (y * 3 + x + 1) as u8;
            }
        }
        pad_plane(&mut plane, 8, 8, 3, 2);

        for y in 0..8usize {
            for x in 0..8usize {
                let sy = y.min(1);
                let sx = x.min(2);
                assert_eq!(
                    plane[y * 8 + x],
                    (sy * 3 + sx + 1) as u8,
                    "the sample at ({x}, {y})"
                );
            }
        }
    }

    /// F.1.2.3's byte stuffing, checked over an image noisy enough to produce
    /// X'FF' bytes in the coded data. Held to the clause: inside the entropy-
    /// coded segment of a scan with no restart interval, an X'FF' may be
    /// followed only by X'00'.
    #[test]
    fn every_ff_in_the_coded_data_is_followed_by_a_stuffed_zero() {
        let mut pixels = vec![0u8; 64 * 64];
        let mut state = 0x9E37_79B9u32;
        for slot in pixels.iter_mut() {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            *slot = (state >> 24) as u8;
        }
        let options = JpegOptions {
            quantisation: JpegQuantisation::Tables {
                luminance: [1; 64],
                chrominance: [1; 64],
            },
            ..JpegOptions::default()
        };
        let bytes = jpeg_encode(&gray_source(&pixels, 64, 64), &options).unwrap();
        let coded = entropy_segment(&bytes);
        let mut stuffed = 0;
        let mut at = 0;
        while at < coded.len() {
            if coded[at] == 0xFF {
                assert_eq!(
                    coded.get(at + 1).copied(),
                    Some(0x00),
                    "an unstuffed X'FF' at {at}"
                );
                stuffed += 1;
                at += 2;
            } else {
                at += 1;
            }
        }
        assert!(
            stuffed > 0,
            "this fixture is supposed to produce X'FF' bytes"
        );
    }

    /// 4.10 and F.1.2.3's restart interval: the markers are RST0 through RST7 in
    /// order, there is one every `Ri` MCUs, and — the half that a decoder can
    /// hide — the DC predictor goes back to zero at each one.
    ///
    /// The predictor reset is what the second half of this test is for. It is
    /// invisible to any check that only counts markers, and it is invisible to a
    /// round trip through a decoder that also forgets to reset, so the bits are
    /// read here instead: the first block after a restart must code its DC as a
    /// difference from zero, which for this flat image is the same code word the
    /// very first block emitted.
    #[test]
    fn the_restart_interval_is_emitted_and_the_predictors_reset() {
        // 32 x 16 of one value: eight MCUs of 8 x 8, all with the same DC.
        let pixels = gray(32, 16, 144);
        let options = JpegOptions {
            restart_interval: 2,
            ..JpegOptions::default()
        };
        let bytes = jpeg_encode(&gray_source(&pixels, 32, 16), &options).unwrap();

        let dri = segments(&bytes)
            .into_iter()
            .find(|(code, _)| *code == 0xDD)
            .expect("a DRI segment");
        assert_eq!(dri.1, vec![0x00, 0x02]);

        let coded = entropy_segment(&bytes);
        // Eight MCUs, a restart before MCUs 2, 4 and 6: three markers.
        let mut markers = Vec::new();
        let mut runs: Vec<Vec<u8>> = vec![Vec::new()];
        let mut at = 0;
        while at < coded.len() {
            if coded[at] == 0xFF {
                match coded.get(at + 1).copied() {
                    Some(0x00) => {
                        runs.last_mut().unwrap().push(0xFF);
                        at += 2;
                        continue;
                    }
                    Some(m) if (0xD0..=0xD7).contains(&m) => {
                        markers.push(m);
                        runs.push(Vec::new());
                        at += 2;
                        continue;
                    }
                    other => panic!("a marker {other:?} inside the coded data"),
                }
            }
            runs.last_mut().unwrap().push(coded[at]);
            at += 1;
        }
        assert_eq!(markers, vec![0xD0, 0xD1, 0xD2], "RSTn in order from RST0");

        // Each run codes two identical MCUs, and each starts from a zeroed
        // predictor, so every run is byte-identical to the first.
        assert_eq!(runs.len(), 4);
        for (i, run) in runs.iter().enumerate() {
            assert_eq!(run, &runs[0], "run {i} after a restart");
        }
        // And it is the flat-block code word of the published test above, twice
        // over: 101 1000 1010 then 000 1010 (DIFF = 0 for the second MCU).
        assert_eq!(runs[0], vec![0xB1, 0x45, 0x7F]);
    }

    /// Self-consistency only: this encoder against this crate's decoder, which
    /// `jpeg/encode.rs`'s header says is itself adjudicated by nothing
    /// third-party. It says that a datastream is well-formed enough to be read
    /// back and that the picture survives; it says nothing about T.81.
    #[test]
    fn a_round_trip_keeps_the_picture_in_every_colour_and_sampling() {
        let cases: [(u32, u32, JpegSourceColour, JpegSampling); 5] = [
            (16, 16, JpegSourceColour::Gray, JpegSampling::FourFourFour),
            (16, 16, JpegSourceColour::Rgb, JpegSampling::FourFourFour),
            (32, 32, JpegSourceColour::Rgb, JpegSampling::FourTwoZero),
            (13, 7, JpegSourceColour::Gray, JpegSampling::FourFourFour),
            (13, 7, JpegSourceColour::Rgb, JpegSampling::FourTwoZero),
        ];

        for (width, height, colour, sampling) in cases {
            let components = usize::from(colour.components());
            let mut pixels = vec![0u8; width as usize * height as usize * components];
            for (i, slot) in pixels.iter_mut().enumerate() {
                let pixel = i / components;
                // A smooth luma ramp: coarse quantisers keep it, so the
                // comparison can be tight without asserting anything about the
                // IDCT.
                let base = (pixel % 8 * 16 + 64) as i32;
                // And a colour that is genuinely off neutral, because a grey
                // ramp leaves both chrominance planes at 128 and makes every
                // colour case a luminance case wearing three components. The
                // offset flips every sixteenth row, which is one 4:2:0 MCU, so
                // each chrominance block is still *flat*: the box filter and
                // the block transform are exact on it and the tolerance below
                // stays the luma ramp's. Transposing Cb and Cr fires this test
                // only because of these three lines.
                let offset = if components == 3 {
                    let band = if (pixel / width as usize / 16) % 2 == 0 {
                        1i32
                    } else {
                        -1
                    };
                    band * match i % 3 {
                        0 => 40i32,
                        1 => 0,
                        _ => -40,
                    }
                } else {
                    0
                };
                *slot = (base + offset) as u8;
            }
            let source = JpegSource {
                width,
                height,
                colour,
                stride: width as usize * components,
                data: &pixels,
            };
            let options = JpegOptions {
                quantisation: JpegQuantisation::AnnexKHalved,
                sampling,
                restart_interval: 0,
            };
            let bytes = jpeg_encode(&source, &options).unwrap();
            let image = decode(&bytes, 1 << 24).expect("the encoder's own output decodes");
            assert_eq!((image.width, image.height), (width, height));
            assert_eq!(image.data.len(), pixels.len());

            let worst = image
                .data
                .iter()
                .zip(pixels.iter())
                .map(|(&got, &want)| (i32::from(got) - i32::from(want)).abs())
                .max()
                .unwrap_or(0);
            assert!(
                worst <= 24,
                "{width}x{height} {colour:?} {sampling:?} strayed {worst} levels"
            );
        }
    }

    /// Self-consistency only, and the reason it is here rather than folded into
    /// the round trip above: a restart interval must change the bytes and not
    /// the picture. A decoder that mishandles RSTn desynchronises, so this is
    /// the cheapest total check that the markers are where the decoder expects.
    #[test]
    fn restarts_change_the_bytes_and_not_the_picture() {
        let pixels: Vec<u8> = (0..64u32 * 64).map(|i| (i % 251) as u8).collect();
        let plain = jpeg_encode(&gray_source(&pixels, 64, 64), &JpegOptions::default()).unwrap();
        let restarted = jpeg_encode(
            &gray_source(&pixels, 64, 64),
            &JpegOptions {
                restart_interval: 3,
                ..JpegOptions::default()
            },
        )
        .unwrap();
        assert_ne!(plain, restarted);
        assert_eq!(
            decode(&plain, 1 << 24).unwrap().data,
            decode(&restarted, 1 << 24).unwrap().data
        );
    }

    /// The stride is the field an encoder is most likely to ignore, because for
    /// every unpadded buffer it equals the row length. `PngSource`'s test says
    /// the same thing about the same mistake.
    #[test]
    fn a_padded_stride_is_not_read_as_pixels() {
        let mut padded = vec![0u8; 12 * 4];
        for y in 0..4usize {
            for x in 0..4usize {
                padded[y * 12 + x] = (x * 16 + y * 4) as u8;
            }
        }
        let tight: Vec<u8> = (0..4usize)
            .flat_map(|y| (0..4usize).map(move |x| (x * 16 + y * 4) as u8))
            .collect();

        let from_padded = jpeg_encode(
            &JpegSource {
                width: 4,
                height: 4,
                colour: JpegSourceColour::Gray,
                stride: 12,
                data: &padded,
            },
            &JpegOptions::default(),
        )
        .unwrap();
        let from_tight = jpeg_encode(&gray_source(&tight, 4, 4), &JpegOptions::default()).unwrap();
        assert_eq!(from_padded, from_tight);
    }

    /// Every refusal, by its own name. All five are a caller describing its own
    /// buffer or its own intent wrongly; none can be reached from the pixels.
    #[test]
    fn every_refusal_fires_by_its_own_name() {
        let pixels = gray(8, 8, 128);
        let default = JpegOptions::default();

        assert_eq!(
            jpeg_encode(&gray_source(&pixels, 0, 8), &default),
            Err(JpegEncodeError::BadDimensions {
                width: 0,
                height: 8
            })
        );
        assert_eq!(
            jpeg_encode(&gray_source(&pixels, 65_536, 8), &default),
            Err(JpegEncodeError::BadDimensions {
                width: 65_536,
                height: 8
            })
        );
        assert_eq!(
            jpeg_encode(
                &JpegSource {
                    width: 8,
                    height: 8,
                    colour: JpegSourceColour::Gray,
                    stride: 4,
                    data: &pixels,
                },
                &default
            ),
            Err(JpegEncodeError::ShortStride {
                stride: 4,
                row_bytes: 8
            })
        );
        assert_eq!(
            jpeg_encode(&gray_source(&pixels[..40], 8, 8), &default),
            Err(JpegEncodeError::ShortData { have: 40, need: 64 })
        );
        assert_eq!(
            jpeg_encode(
                &gray_source(&pixels, 8, 8),
                &JpegOptions {
                    quantisation: JpegQuantisation::Tables {
                        luminance: {
                            let mut table = [1u8; 64];
                            table[7] = 0;
                            table
                        },
                        chrominance: [1; 64],
                    },
                    ..default
                }
            ),
            Err(JpegEncodeError::ZeroQuantiser {
                chrominance: false,
                index: 7
            })
        );
        assert_eq!(
            jpeg_encode(
                &gray_source(&pixels, 8, 8),
                &JpegOptions {
                    sampling: JpegSampling::FourTwoZero,
                    ..default
                }
            ),
            Err(JpegEncodeError::SubsampledGrayscale)
        );
    }

    /// The two rungs T.81 names are different quantisers and therefore different
    /// bytes, and the halved one is finer — which is the whole content of K.1's
    /// second paragraph.
    #[test]
    fn annex_k_halved_is_finer_than_annex_k() {
        let pixels: Vec<u8> = (0..32u32 * 32).map(|i| (i % 97 * 2) as u8).collect();
        let coarse = jpeg_encode(&gray_source(&pixels, 32, 32), &JpegOptions::default()).unwrap();
        let fine = jpeg_encode(
            &gray_source(&pixels, 32, 32),
            &JpegOptions {
                quantisation: JpegQuantisation::AnnexKHalved,
                ..JpegOptions::default()
            },
        )
        .unwrap();
        assert!(fine.len() > coarse.len(), "a finer quantiser costs bits");

        let error = |bytes: &[u8]| {
            decode(bytes, 1 << 24)
                .unwrap()
                .data
                .iter()
                .zip(pixels.iter())
                .map(|(&got, &want)| u32::from(got.abs_diff(want)))
                .sum::<u32>()
        };
        assert!(error(&fine) < error(&coarse), "and buys accuracy");

        // Rounding up, so halving can never produce the zero B.2.4.1 forbids.
        let dqt = segments(&fine)
            .into_iter()
            .find(|(code, _)| *code == 0xDB)
            .unwrap()
            .1;
        assert!(dqt[1..].iter().all(|&q| q >= 1));
        assert_eq!(dqt[1], ANNEX_K1_LUMINANCE[0].div_ceil(2));
    }
    // ---- SOF9 and SOF10: the arithmetic frames ------------------------------

    /// Encodes a hand-derived decision list into an entropy-coded segment.
    ///
    /// The same shape as `arith`'s own test helper and for the same reason:
    /// what goes in is the test's reading of T.81's figures, and the coder it
    /// goes through is pinned at both ends by K.4.1's published data.
    fn arith_segment(decisions: &[(arith::Bin, u8)]) -> Vec<u8> {
        let mut encoder = crate::qm::encoder::QmEncoder::new();
        let mut dc = [crate::qm::QmContext::default(); 49];
        let mut ac = [crate::qm::QmContext::default(); 245];
        for &(bin, d) in decisions {
            match bin {
                arith::Bin::Dc(at) => encoder.encode(&mut dc[at], d),
                arith::Bin::Ac(at) => encoder.encode(&mut ac[at], d),
                arith::Bin::Fixed => encoder.encode_fixed(d),
            }
        }
        encoder.finish()
    }

    /// A one-component arithmetic JPEG of `width` by `height` around already
    /// entropy-coded scans.
    ///
    /// The quantisation table is all ones so a coefficient reaches the
    /// transform unchanged and the expected pixels can be worked out from
    /// A.3.3 alone. Each scan is `(Ss, Se, Ah/Al, bytes)`.
    fn arithmetic_gray(
        marker: u8,
        width: u16,
        height: u16,
        restart: Option<u16>,
        dac: Option<&[u8]>,
        scans: &[(u8, u8, u8, Vec<u8>)],
    ) -> Vec<u8> {
        let mut out = vec![0xFF, 0xD8];

        out.extend_from_slice(&[0xFF, 0xDB, 0x00, 0x43, 0x00]);
        out.extend_from_slice(&[1u8; 64]);

        if let Some(segment) = dac {
            let length = u16::try_from(segment.len() + 2).expect("a short DAC segment");
            out.extend_from_slice(&[0xFF, 0xCC]);
            out.extend_from_slice(&length.to_be_bytes());
            out.extend_from_slice(segment);
        }
        if let Some(interval) = restart {
            out.extend_from_slice(&[0xFF, 0xDD, 0x00, 0x04]);
            out.extend_from_slice(&interval.to_be_bytes());
        }

        // B.2.2: Lf = 8 + 3 * Nf.
        out.extend_from_slice(&[0xFF, marker, 0x00, 0x0B, 0x08]);
        out.extend_from_slice(&height.to_be_bytes());
        out.extend_from_slice(&width.to_be_bytes());
        out.extend_from_slice(&[0x01, 0x01, 0x11, 0x00]);

        for (ss, se, a, data) in scans {
            // B.2.3: Ls = 6 + 2 * Ns.
            out.extend_from_slice(&[0xFF, 0xDA, 0x00, 0x08, 0x01, 0x01, 0x00, *ss, *se, *a]);
            out.extend_from_slice(data);
        }

        out.extend_from_slice(&[0xFF, 0xD9]);
        out
    }

    /// The decisions for `DIFF = 8`, derived from Figures F.19 to F.24 with
    /// `Da = 0`.
    ///
    /// `V = 8` means `Sz = 7`, so the category runs `X1`, `X2`, `X3` and stops
    /// at "Sz < 8", and the two magnitude bits at `M3 = X3 + 14 = 36` are both
    /// 1, making `Sz = 4 | 2 | 1`.
    fn diff_of_eight() -> Vec<(arith::Bin, u8)> {
        vec![
            (arith::Bin::Dc(0), 1),
            (arith::Bin::Dc(1), 0),
            (arith::Bin::Dc(2), 1),
            (arith::Bin::Dc(20), 1),
            (arith::Bin::Dc(21), 1),
            (arith::Bin::Dc(22), 0),
            (arith::Bin::Dc(36), 1),
            (arith::Bin::Dc(36), 1),
        ]
    }

    /// The decisions for `DIFF = 128`.
    ///
    /// `V = 128` means `Sz = 127`, so the category climbs `X1` to `X7` — bins
    /// 20 to 26 — before "Sz < 128" stops it, and the six magnitude bits at
    /// `M7 = X7 + 14 = 40` are all 1.
    ///
    /// 128 rather than 8 because this engine's integer IDCT reaches a byte
    /// through two truncating shifts and a DC of 8 does not survive them: the
    /// exact sample is `8 / 8 = 1` and the transform yields 0, so mid-grey
    /// would be the answer whether or not this decoded at all. 128 is sixteen
    /// levels, which it cannot swallow.
    fn diff_of_128() -> Vec<(arith::Bin, u8)> {
        let mut decisions = vec![
            (arith::Bin::Dc(0), 1),
            (arith::Bin::Dc(1), 0),
            (arith::Bin::Dc(2), 1),
        ];
        for bin in 20..=25usize {
            decisions.push((arith::Bin::Dc(bin), 1));
        }
        decisions.push((arith::Bin::Dc(26), 0));
        for _ in 0..6 {
            decisions.push((arith::Bin::Dc(40), 1));
        }
        decisions
    }

    /// A flat grayscale JPEG of `value`, Huffman-coded by this crate's own
    /// encoder against a quantisation table of ones.
    ///
    /// A.3.3's FDCT of a flat block leaves `S00 = 8 x (value - 128)` and every
    /// other coefficient zero, and a quantiser of one passes that through — so
    /// the caller knows which coefficient the file carries without decoding
    /// it.
    fn huffman_flat_gray(width: u32, height: u32, value: u8, restart_interval: u16) -> Vec<u8> {
        let raster = vec![value; (width * height) as usize];
        jpeg_encode(
            &JpegSource {
                width,
                height,
                colour: JpegSourceColour::Gray,
                stride: width as usize,
                data: &raster,
            },
            &JpegOptions {
                quantisation: JpegQuantisation::Tables {
                    luminance: [1; 64],
                    chrominance: [1; 64],
                },
                sampling: JpegSampling::FourFourFour,
                restart_interval,
            },
        )
        .expect("a flat grayscale raster encodes")
    }

    /// **A SOF9 frame and a Huffman frame carrying the same coefficients
    /// decode to the same image** — which is T.81 F.1.4's own claim about the
    /// two entropy coders, tested rather than assumed:
    ///
    /// > As with the Huffman coding technique, the binary arithmetic coding
    /// > technique is lossless. It is possible to transcode between the two
    /// > systems without either FDCT or IDCT computations, and without
    /// > modification of the reconstructed image.
    ///
    /// The Huffman side is this crate's baseline encoder over a flat raster of
    /// 144, which A.3.3 puts at `S00 = 8 x 16 = 128` with every other
    /// coefficient zero; the arithmetic side is [`diff_of_128`], hand-derived
    /// from Figures F.19 to F.24. Nothing is shared between the two paths
    /// below the coefficient buffer, so agreement is a statement about the
    /// entropy decoding rather than about the transform.
    #[test]
    fn a_sof9_frame_and_a_huffman_frame_carry_the_same_block_t_81_f_1_4() {
        let mut decisions = diff_of_128();
        decisions.push((arith::Bin::Ac(0), 1));
        let arithmetic = arithmetic_gray(
            0xC9,
            8,
            8,
            None,
            None,
            &[(0x00, 0x3F, 0x00, arith_segment(&decisions))],
        );

        let image = decode(&arithmetic, 1 << 20).expect("a SOF9 frame decodes");
        assert_eq!((image.width, image.height), (8, 8));
        assert_eq!(image.color, JpegColor::Gray);
        assert_eq!(
            image.warnings,
            Vec::new(),
            "a clean frame warns about nothing"
        );

        let huffman =
            decode(&huffman_flat_gray(8, 8, 144, 0), 1 << 20).expect("the Huffman frame decodes");
        assert_eq!(image.data, huffman.data);

        // And it is not vacuously equal: both are the flat block the
        // coefficient describes, well away from the mid-grey an undecoded
        // frame would give.
        assert!(image.data.iter().all(|&s| s == image.data[0]));
        assert_ne!(image.data[0], 128);
    }

    /// **The DC predictor runs across blocks and `Da` conditions the next
    /// one** (F.2.1.3.1 and F.1.4.4.1.2), over a two-MCU frame.
    ///
    /// The second block's difference is zero, so its DC coefficient is the
    /// first block's 128 carried forward and both blocks come out at the same
    /// grey. Its `Da` is 128, "large positive" under the default `L = 0`,
    /// `U = 1`, so its one decision is at bin 12 — a bin nothing has touched —
    /// and not at bin 0, which the first block left adapted.
    #[test]
    fn the_dc_predictor_and_da_carry_across_blocks_in_a_sof9_frame() {
        let mut decisions = diff_of_128();
        decisions.push((arith::Bin::Ac(0), 1));
        decisions.push((arith::Bin::Dc(12), 0));
        decisions.push((arith::Bin::Ac(0), 1));

        let data = arithmetic_gray(
            0xC9,
            16,
            8,
            None,
            None,
            &[(0x00, 0x3F, 0x00, arith_segment(&decisions))],
        );

        let image = decode(&data, 1 << 20).expect("a two-block SOF9 frame decodes");
        assert_eq!((image.width, image.height), (16, 8));
        let huffman =
            decode(&huffman_flat_gray(16, 8, 144, 0), 1 << 20).expect("the Huffman frame decodes");
        assert_eq!(image.data, huffman.data);
    }

    /// **A restart interval re-runs `Initdec` and resets the statistics**
    /// (E.2.4, F.1.4.4.1.5, F.1.4.4.2.2).
    ///
    /// Two blocks with `Ri = 1`, so an `RSTn` sits between them and the second
    /// starts from scratch: its DC predictor is zero again, its `Da` is zero
    /// again, and every statistics bin is back at Table D.3's index 0. So the
    /// second interval's bytes are byte for byte the first's, which is what
    /// lets this tell a reset from a missing one — a decoder that carried
    /// either the predictor or the statistics across would decode the second
    /// block to something else.
    #[test]
    fn a_restart_interval_resets_the_coder_and_the_statistics() {
        let mut decisions = diff_of_128();
        decisions.push((arith::Bin::Ac(0), 1));
        let interval = arith_segment(&decisions);

        let mut scan = interval.clone();
        scan.extend_from_slice(&[0xFF, 0xD0]);
        scan.extend_from_slice(&interval);

        let data = arithmetic_gray(0xC9, 16, 8, Some(1), None, &[(0x00, 0x3F, 0x00, scan)]);

        let image = decode(&data, 1 << 20).expect("a restarting SOF9 frame decodes");
        let huffman =
            decode(&huffman_flat_gray(16, 8, 144, 1), 1 << 20).expect("the Huffman frame decodes");
        assert_eq!(image.data, huffman.data);
    }

    /// **The DAC marker is read, not stepped over.**
    ///
    /// Until September 2026 `X'FFCC'` fell through to the unknown-segment arm
    /// while a comment beside the SOF refusals claimed it was "handled below".
    /// It was invisible because every arithmetic frame was refused before a
    /// DAC could matter.
    ///
    /// The block below has coefficients at `K = 3` and `K = 6`, and its
    /// decisions were derived with `Kx = 6` — so both use the `X2` bin at 189
    /// and the second inherits the adaptation the first left there. Under the
    /// default `Kx = 5` the second would use 217 instead, which no decision
    /// has touched, and the stream decodes to something else. So the two
    /// decodes differing is the marker being parsed; what the conditioning
    /// then *means* is `arith`'s own tests, which assert the bins directly.
    #[test]
    fn the_dac_marker_changes_what_a_sof9_frame_decodes_to() {
        let decisions = vec![
            (arith::Bin::Dc(0), 0),
            (arith::Bin::Ac(0), 0),
            (arith::Bin::Ac(1), 0),
            (arith::Bin::Ac(4), 0),
            (arith::Bin::Ac(7), 1),
            (arith::Bin::Fixed, 0),
            (arith::Bin::Ac(8), 1),
            (arith::Bin::Ac(8), 1),
            (arith::Bin::Ac(189), 0),
            (arith::Bin::Ac(203), 0),
            (arith::Bin::Ac(9), 0),
            (arith::Bin::Ac(10), 0),
            (arith::Bin::Ac(13), 0),
            (arith::Bin::Ac(16), 1),
            (arith::Bin::Fixed, 0),
            (arith::Bin::Ac(17), 1),
            (arith::Bin::Ac(17), 1),
            (arith::Bin::Ac(189), 0),
            (arith::Bin::Ac(203), 1),
            (arith::Bin::Ac(18), 1),
        ];
        let scans = [(0x00u8, 0x3Fu8, 0x00u8, arith_segment(&decisions))];

        // B.2.4.3: Tc = 1 selects an AC conditioning table, Tb = 0 the
        // destination, and Cs is Kx.
        let with_dac = arithmetic_gray(0xC9, 8, 8, None, Some(&[0x10, 0x06]), &scans);
        let without = arithmetic_gray(0xC9, 8, 8, None, None, &scans);

        let read = decode(&with_dac, 1 << 20).expect("with DAC");
        let ignored = decode(&without, 1 << 20).expect("without DAC");
        assert_ne!(
            read.data, ignored.data,
            "the DAC segment made no difference, so it was skipped"
        );
    }

    /// **A SOF10 frame decodes across three scans** — a DC first scan, a DC
    /// refinement and an AC band — which is the shape G.1.3 describes.
    ///
    /// The DC coefficient arrives in two pieces: the first scan sends 8 at
    /// `Al = 4`, giving 128, and the refinement adds the bit below it at
    /// `Al = 3`, giving 136. The AC band places nothing. A Huffman frame over
    /// a flat raster of 145 carries that same `S00 = 8 x 17 = 136`, so
    /// F.1.4's transcoding claim applies to it as much as to the sequential
    /// frame above — and it is the refinement that makes the two match, since
    /// the first scan alone would leave 128 and a visibly different grey.
    #[test]
    fn a_sof10_frame_decodes_across_three_scans() {
        let first = arith_segment(&diff_of_eight());
        // G.1.3.1: one decision at the fixed estimate, per block, per scan.
        let refine = arith_segment(&[(arith::Bin::Fixed, 1)]);
        // An AC band that is immediately at its end of band.
        let band = arith_segment(&[(arith::Bin::Ac(0), 1)]);

        let data = arithmetic_gray(
            0xCA,
            8,
            8,
            None,
            None,
            &[
                (0x00, 0x00, 0x04, first),
                (0x00, 0x00, 0x43, refine),
                (0x01, 0x3F, 0x03, band),
            ],
        );

        let image = decode(&data, 1 << 20).expect("a SOF10 frame decodes");
        assert_eq!((image.width, image.height), (8, 8));
        let huffman =
            decode(&huffman_flat_gray(8, 8, 145, 0), 1 << 20).expect("the Huffman frame decodes");
        assert_eq!(image.data, huffman.data);
        assert_ne!(
            image.data,
            decode(&huffman_flat_gray(8, 8, 144, 0), 1 << 20)
                .expect("the Huffman frame decodes")
                .data,
            "without the refinement scan the DC would still be 128"
        );
    }

    /// The two seeds `fuzz/corpus/jpeg/` carries for the arithmetic frames,
    /// written from the same helpers as the tests above so the two cannot
    /// drift apart.
    ///
    /// Worth having because the corpus held four seeds before this and every
    /// one of them was Huffman-coded: `qm.rs` and `jpeg/arith.rs` together are
    /// roughly a thousand lines that **no fuzz target could reach**, and a
    /// mutator starting from a SOF0 file does not stumble into a SOF9 one —
    /// it would have to invent a valid arithmetic segment from nothing. This
    /// is ruling 1's enforcement arm being given a door.
    ///
    /// The seeds are **reachable by construction rather than measured**: the
    /// assertions below prove each one decodes, and to the block it was built
    /// from, before a single byte is mutated.
    ///
    /// Run with `--ignored` when the fixtures change; the corpus is committed,
    /// and a run that rewrites it is a diff to look at rather than apply
    /// blindly.
    #[test]
    #[ignore = "writes into fuzz/corpus/jpeg, which is committed"]
    fn write_the_arithmetic_fuzz_seeds() {
        let mut sequential = diff_of_128();
        sequential.push((arith::Bin::Ac(0), 0));
        sequential.push((arith::Bin::Ac(1), 1));
        sequential.push((arith::Bin::Fixed, 1));
        sequential.push((arith::Bin::Ac(2), 1));
        sequential.push((arith::Bin::Ac(2), 1));
        sequential.push((arith::Bin::Ac(189), 0));
        sequential.push((arith::Bin::Ac(203), 0));
        sequential.push((arith::Bin::Ac(3), 1));
        let sequential = arithmetic_gray(
            0xC9,
            16,
            8,
            None,
            Some(&[0x10, 0x06, 0x00, 0x31]),
            &[(0x00, 0x3F, 0x00, arith_segment(&sequential))],
        );

        let progressive = arithmetic_gray(
            0xCA,
            8,
            8,
            None,
            None,
            &[
                (0x00, 0x00, 0x04, arith_segment(&diff_of_eight())),
                (0x00, 0x00, 0x43, arith_segment(&[(arith::Bin::Fixed, 1)])),
                (
                    0x01,
                    0x3F,
                    0x03,
                    arith_segment(&[
                        (arith::Bin::Ac(0), 0),
                        (arith::Bin::Ac(1), 1),
                        (arith::Bin::Fixed, 0),
                        (arith::Bin::Ac(2), 1),
                        (arith::Bin::Ac(2), 0),
                        (arith::Bin::Ac(3), 1),
                    ]),
                ),
                (0x01, 0x3F, 0x30, arith_segment(&[(arith::Bin::Ac(2), 1)])),
            ],
        );

        // A seed that no longer reaches what it was chosen for is worse than
        // no seed, because it looks like coverage.
        for (label, bytes) in [
            ("arithmetic-sequential", &sequential),
            ("arithmetic-progressive", &progressive),
        ] {
            let image = decode(bytes, 1 << 20).unwrap_or_else(|e| panic!("{label}: {e:?}"));
            assert_eq!(image.warnings, Vec::new(), "{label}");
            assert!(
                image.data.iter().any(|&s| s != 128),
                "{label}: nothing decoded"
            );
        }

        let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fuzz/corpus/jpeg");
        std::fs::write(base.join("arithmetic-sequential.jpg"), &sequential)
            .expect("the corpus directory is there");
        std::fs::write(base.join("arithmetic-progressive.jpg"), &progressive)
            .expect("the corpus directory is there");
    }

    /// An arithmetic scan that stops in the middle is reported as truncated
    /// rather than silently completed (ruling 10), and what decoded stays
    /// (ruling 2).
    #[test]
    fn a_truncated_arithmetic_scan_warns_rather_than_failing() {
        let mut data = arithmetic_gray(
            0xC9,
            16,
            8,
            None,
            None,
            &[(0x00, 0x3F, 0x00, arith_segment(&diff_of_eight()))],
        );
        // Drop the EOI, so the coder runs off the end of the segment instead
        // of stopping at a marker.
        data.truncate(data.len() - 2);

        let image = decode(&data, 1 << 20).expect("what decoded is kept");
        assert!(image.warnings.contains(&Warning::TruncatedInput));
    }

    // The restart interval `tests/complexity.rs` repeats, committed beside
    // that test because this module's QM encoder is the only one that writes
    // it.
    include!("../tests/complexity/arithmetic.rs");

    /// **The committed interval is this encoder's output**
    /// (`docs/design/resource-exhaustion.md`, "The families"): the flat
    /// block's two decisions in fresh contexts code to exactly the bytes
    /// `tests/complexity/arithmetic.rs` holds, so the complexity test's shape
    /// cannot drift from the coder that made it. And two such intervals
    /// between a restart marker decode to two flat mid-grey blocks, clean, as
    /// that test reads hundreds of them.
    #[test]
    fn the_complexity_shapes_arithmetic_interval_is_the_qm_encoders_flat_block() {
        let coded = arith_segment(&[(arith::Bin::Dc(0), 0), (arith::Bin::Ac(0), 1)]);
        assert_eq!(coded, FLAT_INTERVAL, "the committed interval has drifted");

        let mut scan = FLAT_INTERVAL.to_vec();
        scan.extend_from_slice(&[0xFF, 0xD0]);
        scan.extend_from_slice(FLAT_INTERVAL);
        let data = arithmetic_gray(0xC9, 16, 8, Some(1), None, &[(0x00, 0x3F, 0x00, scan)]);
        let image = decode(&data, 1 << 20).expect("two flat intervals decode");
        assert_eq!((image.width, image.height), (16, 8));
        assert_eq!(
            image.warnings,
            Vec::new(),
            "a clean frame warns about nothing"
        );
        assert!(
            image.data.iter().all(|&s| s == 128),
            "a zero DC difference after every restart is mid-grey"
        );
    }

    /// The counted door answers what the uncounted one does, on every frame
    /// type, and counts something for each.
    #[test]
    fn the_counted_door_answers_as_the_uncounted_one() {
        let frames = [
            tiny_gray(),
            huffman_flat_gray(16, 16, 144, 1),
            arithmetic_gray(
                0xC9,
                16,
                8,
                None,
                None,
                &[(0x00, 0x3F, 0x00, arith_segment(&diff_of_eight()))],
            ),
        ];
        for frame in &frames {
            let (counted, steps) = decode_counted(frame, 1 << 20);
            let plain = decode(frame, 1 << 20);
            assert_eq!(
                counted.as_ref().map(|i| (&i.data, &i.warnings)),
                plain.as_ref().map(|i| (&i.data, &i.warnings)),
            );
            assert!(steps > 0, "a decode that ran counted nothing");
        }
        // A refusal is the same refusal through both doors.
        let refused = [0xFF, 0xD8, 0xFF, 0xC3, 0x00, 0x02];
        assert_eq!(
            decode_counted(&refused, 1 << 20).0.err(),
            Some(JpegError::Lossless)
        );
        assert_eq!(
            decode_counted(b"not a jpeg", 1 << 20).0.err(),
            Some(JpegError::NotJpeg)
        );
    }

    /// **A scan visits every block it names, and the count sees each visit**:
    /// the same frame with its one band scanned again — an AC scan that is all
    /// end-of-band, which changes no coefficient — costs at least one step
    /// more for every block, though its image is the same.
    #[test]
    fn a_scan_repeated_costs_a_step_a_block_more() {
        let first = arith_segment(&diff_of_eight());
        let refine = arith_segment(&[(arith::Bin::Fixed, 1)]);
        let band = arith_segment(&[(arith::Bin::Ac(0), 1)]);
        let once = arithmetic_gray(
            0xCA,
            8,
            8,
            None,
            None,
            &[
                (0x00, 0x00, 0x04, first.clone()),
                (0x00, 0x00, 0x43, refine.clone()),
                (0x01, 0x3F, 0x03, band.clone()),
            ],
        );
        let twice = arithmetic_gray(
            0xCA,
            8,
            8,
            None,
            None,
            &[
                (0x00, 0x00, 0x04, first),
                (0x00, 0x00, 0x43, refine),
                (0x01, 0x3F, 0x03, band.clone()),
                (0x01, 0x3F, 0x03, band),
            ],
        );
        let (a, once_steps) = decode_counted(&once, 1 << 20);
        let (b, twice_steps) = decode_counted(&twice, 1 << 20);
        assert_eq!(
            a.expect("three scans decode").data,
            b.expect("four scans decode").data,
            "an end-of-band scan changes no coefficient"
        );
        // One block, so at least one step for its visit and one decision.
        assert!(
            twice_steps >= once_steps + 2,
            "{twice_steps} steps for four scans against {once_steps} for three"
        );
    }

    // ---- The look-ahead reader ------------------------------------------
    //
    // `BitReader` against `direct::BitwiseReader`, the reader a byte at a
    // time, over tables and streams no encoder would write as well as ones it
    // would (`docs/design/jpeg-speed.md`, "The look-ahead Huffman reader").

    use super::streams::Noise;

    /// What kind of Huffman table a case draws.
    #[derive(Clone, Copy, Debug)]
    enum Shape {
        /// Every code of the longest length used: C.2's tables as an encoder
        /// writes them.
        Complete,
        /// Codes left unassigned, which no lookup entry may invent a symbol
        /// for.
        Incomplete,
        /// More codes of some length than the lengths before leave room for,
        /// which C.2 does not allow and a damaged `DHT` carries.
        Oversubscribed,
        /// No `DHT` at all for the slot a scan names.
        Undefined,
    }

    /// A table of `shape` whose codes run to `longest` bits, from `noise`.
    fn random_table(noise: &mut Noise, shape: Shape, longest: usize) -> HuffmanTable {
        if let Shape::Undefined = shape {
            return HuffmanTable::default();
        }
        let mut counts = [0u8; 16];
        // The codes free at each length, kept below 256 so that the longest
        // length can take them all.
        let mut free = 2u64;
        for length in 1..=longest {
            let count = if length == longest {
                match shape {
                    Shape::Incomplete => noise.below(free),
                    _ => free,
                }
            } else {
                let least = free.saturating_sub(127);
                least + noise.below(free - least)
            };
            counts[length - 1] = count as u8;
            free = 2 * (free - count.min(free));
        }
        if let Shape::Oversubscribed = shape {
            let at = noise.below(longest as u64) as usize;
            counts[at] = counts[at].saturating_add(1 + noise.below(3) as u8);
        }
        let total: usize = counts.iter().map(|&c| usize::from(c)).sum();
        let values = (0..total).map(|_| noise.next() as u8).collect();
        HuffmanTable::build(&counts, values)
    }

    /// Entropy-coded bytes with the events a scan meets: stuffed `X'FF00'`,
    /// RSTn, markers inside a code, fill bytes, a raw `X'FF'` with whatever
    /// follows it, and an end anywhere, an `X'FF'` last included.
    fn random_stream(noise: &mut Noise) -> Vec<u8> {
        let length = noise.below(160) as usize;
        let mut out = Vec::with_capacity(length + 2);
        while out.len() < length {
            match noise.below(48) {
                0 => out.extend_from_slice(&[0xFF, 0x00]),
                1 => out.extend_from_slice(&[0xFF, 0xD0 + noise.below(8) as u8]),
                2 => {
                    let marker = [0xD9, 0xC4, 0xDA, 0xDD, 0x01][noise.below(5) as usize];
                    out.extend_from_slice(&[0xFF, marker]);
                }
                3 => out.extend_from_slice(&[0xFF, 0xFF]),
                4 => out.push(0xFF),
                _ => out.push(noise.next() as u8),
            }
        }
        if noise.below(4) == 0 {
            out.push(0xFF);
        }
        out
    }

    /// **The look-ahead reader answers every question the reader of a byte
    /// at a time answers, and is left where that reader is left.**
    ///
    /// Each case draws three tables — complete, incomplete, oversubscribed or
    /// never defined, codes to sixteen bits — and a stream with stuffing,
    /// restart markers mid-symbol and on a byte boundary, markers inside a
    /// code and an end anywhere, and runs a random sequence of symbols,
    /// extra-bits reads, single bits and restarts through both readers. After
    /// every operation the answer, `exhausted`, `crossed_restart`, the
    /// position and the bits left of the current byte must agree, which is
    /// the whole state a scan reads.
    #[test]
    fn the_lookahead_reader_decodes_what_the_bitwise_reader_did() {
        let mut noise = Noise(0x9E37_79B9_7F4A_7C15);
        let shapes = [
            Shape::Complete,
            Shape::Complete,
            Shape::Incomplete,
            Shape::Oversubscribed,
            Shape::Undefined,
        ];
        let mut symbols = 0usize;
        for case in 0..1500 {
            let tables: Vec<HuffmanTable> = (0..3)
                .map(|_| {
                    let shape = shapes[noise.below(shapes.len() as u64) as usize];
                    let longest = 1 + noise.below(16) as usize;
                    random_table(&mut noise, shape, longest)
                })
                .collect();
            let stream = random_stream(&mut noise);
            let mut fast = BitReader::new(&stream);
            let mut slow = direct::BitwiseReader::new(&stream);
            for step in 0..200 {
                let what = match noise.below(12) {
                    0..=6 => {
                        let table = &tables[noise.below(3) as usize];
                        let (got, want) = (fast.huffman(table), slow.huffman(table));
                        assert_eq!(got, want, "case {case} step {step}: the symbol");
                        symbols += 1;
                        "a symbol"
                    }
                    7 | 8 => {
                        let n = if noise.below(8) == 0 {
                            17 + noise.below(15) as u32
                        } else {
                            noise.below(17) as u32
                        };
                        assert_eq!(
                            fast.bits(n),
                            slow.bits(n),
                            "case {case} step {step}: {n} bits"
                        );
                        "extra bits"
                    }
                    9 | 10 => {
                        assert_eq!(fast.bit(), slow.bit(), "case {case} step {step}: a bit");
                        "a bit"
                    }
                    _ => {
                        fast.restart();
                        slow.restart();
                        "a restart"
                    }
                };
                assert_eq!(
                    (
                        fast.exhausted,
                        fast.crossed_restart,
                        fast.position(),
                        fast.count % 8
                    ),
                    (slow.exhausted, slow.crossed_restart, slow.at, slow.count),
                    "case {case} step {step}: the state after {what}"
                );
            }
        }
        assert!(symbols > 100_000, "the sweep decoded {symbols} symbols");
    }

    /// **A symbol costs one table probe, or two for a code longer than the
    /// first level** — counted over streams of every table shape an encoder
    /// writes, codes to sixteen bits, with the bytes after the last symbol
    /// that let every symbol be read from the word.
    #[test]
    fn a_symbol_costs_at_most_two_table_probes() {
        let mut noise = Noise(0x2545_F491_4F6C_DD1D);
        let mut long_codes = 0u64;
        for case in 0..96 {
            let longest = if case % 2 == 0 {
                10 + noise.below(7) as usize
            } else {
                1 + noise.below(16) as usize
            };
            let table = random_table(&mut noise, Shape::Complete, longest);
            // C.2's canonical codes, read back off the table: each length's
            // run from its smallest code to its largest.
            let mut codes = Vec::new();
            for length in 1..=16usize {
                if table.max_code[length] < 0 {
                    continue;
                }
                for code in table.min_code[length]..=table.max_code[length] {
                    let value = table.values[(table.value_offset[length] + code) as usize];
                    codes.push((code as u32, length as u32, value));
                }
            }
            let mut bits = Bits::default();
            let mut expected = Vec::new();
            for _ in 0..400 {
                let (code, length, value) = codes[noise.below(codes.len() as u64) as usize];
                bits.push(code, length);
                expected.push((value, length));
            }
            let mut data = bits.finish();
            data.extend_from_slice(&[0; 8]);

            let mut reader = BitReader::new(&data);
            let mut probes = 0u64;
            for (index, &(value, length)) in expected.iter().enumerate() {
                assert_eq!(
                    reader.huffman(&table),
                    Some(value),
                    "case {case} symbol {index}"
                );
                probes += if length > LOOKUP_BITS { 2 } else { 1 };
                long_codes += u64::from(length > LOOKUP_BITS);
            }
            assert_eq!(reader.probes, probes, "case {case}: the probes");
            assert!(reader.probes <= 2 * expected.len() as u64);
            assert_eq!(
                reader.searched, 0,
                "case {case}: no symbol was searched for"
            );
        }
        assert!(
            long_codes > 1_000,
            "the second level was reached {long_codes} times"
        );
    }

    // ---- The lane transform ---------------------------------------------
    //
    // `idct_block` against `direct::idct_block`, every product made, over
    // blocks of every shape and magnitude a dequantiser can hand it
    // (`docs/design/jpeg-speed.md`, "The lane transform").

    /// **The table has the symmetry the transform's shared products rest
    /// on**, and every entry is far enough from a rounding boundary that
    /// every target's `cos` rounds it to the same integer.
    #[test]
    fn the_cosine_table_has_the_symmetry_the_lanes_share() {
        let entry = |x: usize, u: usize| COS_TABLE[x * 8 + u];
        for x in 0..8 {
            for u in 0..8 {
                let mirrored = if u % 2 == 0 {
                    entry(x, u)
                } else {
                    -entry(x, u)
                };
                assert_eq!(entry(7 - x, u), mirrored, "({x}, {u})");
            }
        }
        let kernel = &*KERNEL;
        for x in 0..4 {
            assert_eq!(entry(x, 0), kernel.k0, "({x}, 0)");
            assert_eq!(
                entry(x, 4),
                [kernel.k4, -kernel.k4, -kernel.k4, kernel.k4][x]
            );
            assert_eq!(
                entry(x, 2),
                [kernel.k2, kernel.k6, -kernel.k6, -kernel.k2][x]
            );
            assert_eq!(
                entry(x, 6),
                [kernel.k6, -kernel.k2, kernel.k2, -kernel.k6][x]
            );
            assert_eq!(
                kernel.odd[x],
                [entry(x, 1), entry(x, 3), entry(x, 5), entry(x, 7)]
            );
        }
        for x in 0..8 {
            for u in 0..8 {
                let cu = if u == 0 {
                    1.0 / std::f64::consts::SQRT_2
                } else {
                    1.0
                };
                let exact = cu / 2.0
                    * ((2.0 * x as f64 + 1.0) * u as f64 * std::f64::consts::PI / 16.0).cos()
                    * 16384.0;
                let from_half = (exact - exact.floor() - 0.5).abs();
                assert!(
                    from_half > 0.07,
                    "({x}, {u}) is {from_half} from rounding the other way"
                );
            }
        }
    }

    /// **[`LANE_BOUND`] is the largest magnitude whose sums all fit in
    /// `i32`**, derived from the table: a row's sum, its floored result and a
    /// column's sum of those, each at its largest.
    #[test]
    fn the_lane_bound_keeps_every_sum_inside_i32() {
        let row_sum = |x: usize| {
            (0..8)
                .map(|u| i64::from(COS_TABLE[x * 8 + u].abs()))
                .sum::<i64>()
        };
        let span = row_sum(0);
        for x in 0..8 {
            assert_eq!(row_sum(x), span, "row {x}");
        }
        assert_eq!(span, 43_284);
        // The floor of a negative sum rounds away from zero, so the largest
        // floored magnitude is the ceiling.
        let column = |bound: i64| (bound * span + 16_383) / 16_384 * span;
        let bound = i64::from(LANE_BOUND);
        assert!(bound * span <= i64::from(i32::MAX));
        assert!(column(bound) <= i64::from(i32::MAX));
        assert!(
            column(bound + 1) > i64::from(i32::MAX),
            "the bound could be larger"
        );
    }

    /// A dequantised block of `shape` whose coefficients are at most
    /// `magnitude`, some exactly it.
    fn random_block(noise: &mut Noise, shape: u64, magnitude: i64) -> [i32; 64] {
        let value = |noise: &mut Noise| -> i32 {
            let v = if noise.below(8) == 0 {
                magnitude
            } else {
                (noise.next() % (magnitude as u64 + 1)) as i64
            };
            let v = if noise.below(2) == 0 { v } else { -v };
            v as i32
        };
        let mut block = [0i32; 64];
        match shape {
            // A single coefficient anywhere.
            0 => block[noise.below(64) as usize] = value(noise),
            // A few, anywhere.
            1 => {
                for _ in 0..1 + noise.below(6) {
                    block[noise.below(64) as usize] = value(noise);
                }
            }
            // The low rows only, as a photograph's blocks mostly are.
            2 => {
                for slot in block.iter_mut().take(32) {
                    *slot = value(noise);
                }
            }
            // Every row's lower half only.
            3 => {
                for (index, slot) in block.iter_mut().enumerate() {
                    if index % 8 < 4 {
                        *slot = value(noise);
                    }
                }
            }
            // Row 0 only, or column 0 only.
            4 => {
                for index in 0..8 {
                    let at = if noise.below(2) == 0 {
                        index
                    } else {
                        index * 8
                    };
                    block[at] = value(noise);
                }
            }
            // Every coefficient.
            _ => {
                for slot in block.iter_mut() {
                    *slot = value(noise);
                }
            }
        }
        block
    }

    /// **The lane transform gives the bytes the direct transform gives, on
    /// every block**: at 8 and 12 bits, sparse and dense, small, at
    /// [`LANE_BOUND`], one past it and as large as an `i16` coefficient times
    /// a 16-bit quantiser makes, where the direct transform's row pass wraps.
    #[test]
    fn the_lane_transform_is_the_direct_transform_on_every_block() {
        let mut noise = Noise(0x0123_4567_89AB_CDEF);
        let bound = i64::from(LANE_BOUND);
        let magnitudes = [
            64,
            1 << 11,
            1 << 14,
            bound,
            bound + 1,
            1 << 20,
            32_768 * 65_535,
        ];
        let mut past = 0;
        for case in 0..30_000 {
            let precision = if case % 3 == 0 { 12 } else { 8 };
            let magnitude = magnitudes[noise.below(magnitudes.len() as u64) as usize];
            let shape = noise.below(6);
            let block = random_block(&mut noise, shape, magnitude);
            past += usize::from(Profile::of(&block).largest > LANE_BOUND);
            let (mut lanes, mut direct) = ([0u8; 64], [0u8; 64]);
            idct_block(&block, precision, &mut lanes);
            direct::idct_block(&block, precision, &mut direct);
            assert_eq!(lanes, direct, "case {case}: {block:?} at {precision} bits");
        }
        assert!(past > 5_000, "{past} blocks took the wide sums");

        // The edges themselves, every coefficient at once.
        for value in [bound, bound + 1, 32_768 * 65_535] {
            for sign in [1i64, -1] {
                let mut block = [(sign * value) as i32; 64];
                for (index, slot) in block.iter_mut().enumerate() {
                    if index % 3 == 0 {
                        *slot = -*slot;
                    }
                }
                for precision in [8, 12] {
                    let (mut lanes, mut direct) = ([0u8; 64], [0u8; 64]);
                    idct_block(&block, precision, &mut lanes);
                    direct::idct_block(&block, precision, &mut direct);
                    assert_eq!(lanes, direct, "{value} x {sign} at {precision} bits");
                }
            }
        }
    }

    /// **A block costs the products it shares, and no more**: twenty-two
    /// multiplies an eight-point pass where the direct sum makes sixty-four,
    /// eleven for a pass whose upper half is zero, and one for a row or a
    /// column with nothing but its first term — counted.
    #[test]
    fn a_block_costs_the_shared_products() {
        fn multiplies(block: &[i32; 64]) -> u64 {
            MULTIPLIES.with(|count| count.set(0));
            let mut out = [0u8; 64];
            idct_block(block, 8, &mut out);
            MULTIPLIES.with(|count| count.get())
        }
        let filled = |keep: &dyn Fn(usize, usize) -> bool, value: i32| {
            let mut block = [0i32; 64];
            for (index, slot) in block.iter_mut().enumerate() {
                if keep(index / 8, index % 8) {
                    *slot = value + (index as i32 % 5);
                }
            }
            block
        };
        let every = |_: usize, _: usize| true;
        let bound = LANE_BOUND as i32;
        for (name, block, expected) in [
            ("a zero block", [0; 64], 0),
            ("a DC-only block", filled(&|r, u| r == 0 && u == 0, 40), 2),
            ("a dense block", filled(&every, 40), 8 * 22 + 8 * 22),
            (
                "a dense block past the bound",
                filled(&every, bound),
                8 * 22 + 8 * 22,
            ),
            ("row 0 alone", filled(&|r, _| r == 0, 40), 22 + 8),
            ("column 0 alone", filled(&|_, u| u == 0, 40), 8 + 8 * 22),
            ("rows 0 to 3", filled(&|r, _| r < 4, 40), 4 * 22 + 8 * 11),
            (
                "every row's lower half",
                filled(&|_, u| u < 4, 40),
                8 * 11 + 8 * 22,
            ),
            (
                "the top-left quarter",
                filled(&|r, u| r < 4 && u < 4, 40),
                4 * 11 + 8 * 11,
            ),
        ] {
            assert_eq!(multiplies(&block), expected, "{name}");
        }
        assert!(
            multiplies(&filled(&every, 40)) * 2 < 1024,
            "a dense block's direct sum makes 1 024"
        );
    }

    // ---- Strips -------------------------------------------------------------
    //
    // The strip painter against `direct::finish_planes`, the whole-image
    // planes, over the streams `streams` writes (`docs/design/jpeg-speed.md`,
    // "Strips").

    /// The ceiling every equality test decodes under.
    const CEILING: usize = 1 << 24;

    /// [`decode`] with no frame streaming: every frame's whole-image
    /// coefficients, painted a strip at a time at the end.
    fn decode_buffered(data: &[u8], max_output: usize) -> Result<JpegImage, JpegError> {
        decode_buffered_at(data, max_output, JpegScale::Full)
    }

    /// [`decode_buffered`] at `scale`.
    fn decode_buffered_at(
        data: &[u8],
        max_output: usize,
        scale: JpegScale,
    ) -> Result<JpegImage, JpegError> {
        let mut image = Collected::default();
        let scanned = scan_frame(data, max_output, scale, None)?;
        assert!(
            scanned.streamed.is_none(),
            "nothing streams with nowhere to"
        );
        let frame = finish(scanned, max_output, &mut image)?;
        Ok(JpegImage {
            width: frame.layout.width,
            height: frame.layout.height,
            color: frame.layout.color,
            data: image.data,
            precision: frame.layout.precision,
            warnings: frame.warnings,
        })
    }

    /// [`decode`] at `scale`, a frame that streams streaming: the whole image
    /// the strips paint.
    fn decode_at(data: &[u8], max_output: usize, scale: JpegScale) -> Result<JpegImage, JpegError> {
        let mut image = Collected::default();
        let scanned = scan_frame(data, max_output, scale, Some(&mut image))?;
        let frame = finish(scanned, max_output, &mut image)?;
        Ok(JpegImage {
            width: frame.layout.width,
            height: frame.layout.height,
            color: frame.layout.color,
            data: image.data,
            precision: frame.layout.precision,
            warnings: frame.warnings,
        })
    }

    /// [`decode`] with whole-image coefficients and whole-image planes in
    /// place of the strips.
    fn decode_through_planes(data: &[u8], max_output: usize) -> Result<JpegImage, JpegError> {
        decode_through_planes_at(data, max_output, JpegScale::Full)
    }

    /// [`decode_through_planes`] at `scale`.
    fn decode_through_planes_at(
        data: &[u8],
        max_output: usize,
        scale: JpegScale,
    ) -> Result<JpegImage, JpegError> {
        let scanned = scan_frame(data, max_output, scale, None)?;
        direct::finish_planes(
            &scanned.components,
            &scanned.quant,
            scanned.width,
            scanned.height,
            scanned.adobe_seen,
            scanned.adobe_transform,
            scanned.precision,
            max_output,
            scanned.warnings,
            scale,
        )
    }

    /// A sink that keeps what it is handed, and holds the order it is handed
    /// it in.
    #[derive(Default)]
    struct Kept {
        layouts: Vec<JpegLayout>,
        strips: Vec<(u32, u32)>,
        data: Vec<u8>,
    }

    impl JpegSink for Kept {
        fn begin(&mut self, layout: JpegLayout) {
            assert!(self.strips.is_empty(), "the layout comes first");
            self.layouts.push(layout);
        }

        fn strip(&mut self, y: u32, rows: u32, data: &[u8]) {
            let next: u32 = self.strips.iter().map(|&(_, rows)| rows).sum();
            assert_eq!(y, next, "the strips arrive in order, whole");
            let layout = self.layouts.first().expect("the layout came first");
            assert_eq!(
                data.len(),
                rows as usize * layout.width as usize * layout.color.components()
            );
            self.strips.push((y, rows));
            self.data.extend_from_slice(data);
        }
    }

    /// Two decodes of one stream that must agree: the same image, warnings
    /// included, or the same refusal.
    fn same(label: &str, got: &Result<JpegImage, JpegError>, want: &Result<JpegImage, JpegError>) {
        match (got, want) {
            (Ok(got), Ok(want)) => {
                assert_eq!(
                    (got.width, got.height, got.color, got.precision),
                    (want.width, want.height, want.color, want.precision),
                    "{label}"
                );
                assert_eq!(got.warnings, want.warnings, "{label}");
                assert!(got.data == want.data, "{label}: the samples differ");
            }
            (Err(got), Err(want)) => assert_eq!(got, want, "{label}"),
            _ => panic!(
                "{label}: {:?} against {:?}",
                got.as_ref().map(|image| image.width),
                want.as_ref().map(|image| image.width)
            ),
        }
    }

    /// Every stream the equality tests run over, with the recipe of each
    /// that has one.
    fn every_stream(noise: &mut Noise) -> Vec<(String, Vec<u8>, Option<streams::Recipe>)> {
        let mut out = Vec::new();
        for _ in 0..800 {
            let recipe = streams::random_recipe(noise);
            let bytes = streams::stream(&recipe, noise);
            out.push((format!("{recipe:?}"), bytes, Some(recipe)));
        }
        let fixed = streams::encoded()
            .into_iter()
            .chain(streams::seeds())
            .chain(streams::tails(noise))
            .chain([
                ("tiny gray".to_string(), tiny_gray()),
                ("one sequential block".to_string(), sequential_block()),
                ("one progressive block".to_string(), progressive_block()),
                ("one refined block".to_string(), progressive_refined_block()),
            ]);
        for (name, bytes) in fixed {
            out.push((name, bytes, None));
        }
        // Cut at every length.
        let cuttable = [
            ("one refined block", progressive_refined_block()),
            ("a restarting 32 x 16", huffman_flat_gray(32, 16, 144, 2)),
        ];
        for (name, bytes) in cuttable {
            for cut in 0..bytes.len() {
                out.push((format!("{name} cut at {cut}"), bytes[..cut].to_vec(), None));
            }
        }
        out
    }

    /// **A strip decode gives the image the whole-image decode gives**, row
    /// for row: through the sink and through [`decode`], against the
    /// whole-image planes, and a frame that streams against the same frame's
    /// whole-image coefficients, for every frame type and both coders, 8 and 12
    /// bits, every sampling the generator draws — 4:4:4, 4:2:2, 4:2:0, 4:1:1,
    /// 4:4:0 and ratios that do not divide — odd sizes, Gray, YCbCr, CMYK,
    /// YCCK and inverted CMYK, restarts and cuts, beside the encoder's
    /// streams, the fuzz seeds and every cut of two small streams. A stream
    /// that fails fails alike, and hands the sink nothing.
    #[test]
    fn a_strip_decode_is_the_whole_image_decode() {
        let mut noise = Noise(0x5EED_0F57_1295_D00D);
        let mut images = 0usize;
        let mut reached = std::collections::BTreeMap::<String, usize>::new();
        for (label, bytes, recipe) in every_stream(&mut noise) {
            let planes = decode_through_planes(&bytes, CEILING);
            let whole = decode(&bytes, CEILING);
            same(&label, &whole, &planes);
            // The drift guard: a frame that streams decodes to what its
            // whole-image coefficients give.
            same(&label, &decode_buffered(&bytes, CEILING), &whole);

            let mut kept = Kept::default();
            let frame = decode_strips(&bytes, CEILING, JpegScale::Full, &mut kept);
            match (&planes, &frame) {
                (Ok(image), Ok(frame)) => {
                    images += 1;
                    assert_eq!(kept.layouts, vec![frame.layout], "{label}");
                    assert_eq!(
                        (
                            frame.layout.width,
                            frame.layout.height,
                            frame.layout.color,
                            frame.layout.precision
                        ),
                        (image.width, image.height, image.color, image.precision),
                        "{label}"
                    );
                    assert_eq!(frame.warnings, image.warnings, "{label}");
                    assert!(
                        kept.data == image.data,
                        "{label}: the strips are not the image"
                    );
                    // Every strip but the last is one MCU row.
                    if let Some((&(_, rows), rest)) = kept.strips.split_first() {
                        for (index, &(_, more)) in rest.iter().enumerate() {
                            if index + 1 < rest.len() {
                                assert_eq!(more, rows, "{label}: strip {index}");
                            } else {
                                assert!(more <= rows, "{label}: the last strip");
                            }
                        }
                    }
                    if let Some(recipe) = recipe {
                        let truncated = image.warnings.contains(&Warning::TruncatedInput);
                        for class in [
                            format!("frame {:#04x}", recipe.frame),
                            format!("{} bits", recipe.precision),
                            format!("{:?}", image.color),
                            format!("{:?}", recipe.sampling),
                            format!("{:?}", recipe.scans),
                            format!("divides {}", recipe.divides()),
                            format!("restart {}", recipe.restart > 0),
                            format!("cut {}", recipe.cut.is_some()),
                            format!("truncated {truncated}"),
                            format!("adobe {:?}", recipe.adobe),
                        ] {
                            *reached.entry(class).or_default() += 1;
                        }
                    }
                }
                (Err(want), Err(got)) => {
                    assert_eq!(got, want, "{label}");
                    assert!(
                        kept.layouts.is_empty() && kept.strips.is_empty(),
                        "{label}: a decode that failed handed out a strip"
                    );
                }
                _ => panic!("{label}: the strip decode and the planes disagree on success"),
            }
        }
        assert!(images > 600, "{images} streams decoded to an image");
        for class in [
            "frame 0xc0",
            "frame 0xc1",
            "frame 0xc2",
            "frame 0xc9",
            "frame 0xca",
            "8 bits",
            "12 bits",
            "Gray",
            "Rgb",
            "Cmyk",
            "CmykInverted",
            "[(1, 1), (1, 1), (1, 1)]",
            "[(2, 1), (1, 1), (1, 1)]",
            "[(2, 2), (1, 1), (1, 1)]",
            "[(4, 1), (1, 1), (1, 1)]",
            "Interleaved",
            "EachAlone",
            "Split",
            "Progressive",
            "divides false",
            "restart true",
            "cut true",
            "truncated true",
            "truncated false",
            "adobe Some(2)",
        ] {
            let count = reached.get(class).copied().unwrap_or(0);
            assert!(count >= 10, "{class}: {count} images, {reached:?}");
        }
    }

    /// **No decode holds a whole-image plane**: the planes a strip decode
    /// holds are at most one MCU row of the image's bytes, and less than the
    /// image once it is taller than two MCU rows; the strip buffer is one
    /// strip.
    #[test]
    fn a_decode_holds_no_whole_image_plane() {
        let mut noise = Noise(0x0DEC_0DE5_7219_0001);
        let mut taller = 0usize;
        for _ in 0..400 {
            let mut recipe = streams::random_recipe(&mut noise);
            recipe.cut = None;
            let bytes = streams::stream(&recipe, &mut noise);
            let mut kept = Kept::default();
            let Ok(frame) = decode_strips(&bytes, CEILING, JpegScale::Full, &mut kept) else {
                continue;
            };
            let layout = frame.layout;
            let row = layout.width as usize * layout.color.components();
            let mcu_row = 8 * recipe.v_max() * row;
            if recipe.divides() {
                assert!(
                    frame.held.planes <= mcu_row,
                    "{recipe:?}: {} plane bytes, and one MCU row is {mcu_row}",
                    frame.held.planes
                );
            }
            if layout.height as usize > 2 * 8 * recipe.v_max() {
                taller += 1;
                assert!(
                    frame.held.planes < layout.height as usize * row,
                    "{recipe:?}: the planes hold the whole image"
                );
            }
            let largest = kept
                .strips
                .iter()
                .map(|&(_, rows)| rows as usize * row)
                .max()
                .unwrap_or(0);
            assert_eq!(frame.held.strip, largest, "{recipe:?}: the strip buffer");
        }
        assert!(taller > 30, "{taller} images were taller than two MCU rows");
    }

    // ---- Streaming ------------------------------------------------------------

    /// **A single-scan sequential frame holds one MCU row of coefficients**,
    /// Huffman-coded and arithmetic-coded alike, where every other frame — a
    /// progressive one, one in several scans, one whose ratios do not divide
    /// — holds the whole image's (`docs/design/jpeg-speed.md`, "Which frames
    /// stream"). What each decodes to is the strip test's: the same frame
    /// with its whole-image coefficients.
    #[test]
    fn a_single_scan_sequential_frame_holds_one_mcu_row_of_coefficients() {
        let mut noise = Noise(0x57AE_A111_0FF0_0001);
        let (mut huffman, mut arithmetic, mut whole_images) = (0usize, 0usize, 0usize);
        for _ in 0..600 {
            let recipe = streams::random_recipe(&mut noise);
            let bytes = streams::stream(&recipe, &mut noise);
            let mut kept = Kept::default();
            let Ok(frame) = decode_strips(&bytes, CEILING, JpegScale::Full, &mut kept) else {
                continue;
            };
            let h_max = usize::from(recipe.sampling.iter().map(|s| s.0).max().unwrap_or(1));
            let v_max = recipe.v_max();
            let mcus_x = usize::from(recipe.width).div_ceil(8 * h_max);
            let mcus_y = usize::from(recipe.height).div_ceil(8 * v_max);
            let row: usize = recipe
                .sampling
                .iter()
                .map(|&(h, v)| mcus_x * usize::from(h) * usize::from(v) * 64 * 2)
                .sum();
            let streams = recipe.sequential()
                && recipe.scans == streams::Scans::Interleaved
                && recipe.divides();
            if streams {
                assert_eq!(
                    frame.held.coefficients, row,
                    "{recipe:?}: one MCU row of coefficients"
                );
                if recipe.frame == 0xC9 {
                    arithmetic += 1;
                } else {
                    huffman += 1;
                }
            } else {
                assert_eq!(
                    frame.held.coefficients,
                    row * mcus_y,
                    "{recipe:?}: the whole image's coefficients"
                );
                whole_images += 1;
            }
        }
        assert!(huffman > 60, "{huffman} Huffman frames streamed");
        assert!(arithmetic > 20, "{arithmetic} arithmetic frames streamed");
        assert!(
            whole_images > 60,
            "{whole_images} frames kept the whole image"
        );
    }

    // ---- The reduced transforms ---------------------------------------------
    //
    // 1/2, 1/4 and 1/8 in the DCT domain (`docs/design/jpeg-speed.md`, "The
    // reduced transforms"): new output, held to its own definition.

    /// The reduced scales, each with its `N`.
    const REDUCED_SCALES: [(JpegScale, usize); 3] = [
        (JpegScale::Half, 4),
        (JpegScale::Quarter, 2),
        (JpegScale::Eighth, 1),
    ];

    /// A dequantised block's top-left `n × n` through the strip decode's own
    /// reduced transform, its samples row-major.
    fn reduce(block: &[i32; 64], n: usize, precision: u8) -> Vec<u8> {
        fn at<const N: usize>(
            block: &[i32; 64],
            cosines: &[[i64; N]; N],
            precision: u8,
        ) -> Vec<u8> {
            let mut kept = [[0i32; N]; N];
            for (v, row) in kept.iter_mut().enumerate() {
                for (u, slot) in row.iter_mut().enumerate() {
                    *slot = block[v * 8 + u];
                }
            }
            reduced_block(&kept, cosines, precision)
                .iter()
                .flatten()
                .copied()
                .collect()
        }
        match n {
            4 => at(block, &REDUCED.four, precision),
            2 => at(block, &REDUCED.two, precision),
            _ => at::<1>(block, &[[0]], precision),
        }
    }

    /// The level P-09.1's definition gives sample `(x, y)` of a block at
    /// `n/8`, evaluated in `f64`, rounded, level-shifted, clamped and narrowed
    /// as a decoded sample is.
    fn defined_level(block: &[i32; 64], n: usize, precision: u8, x: usize, y: usize) -> u8 {
        let c = |u: usize| {
            if u == 0 {
                1.0 / std::f64::consts::SQRT_2
            } else {
                1.0
            }
        };
        let basis = |position: usize, frequency: usize| {
            ((2.0 * position as f64 + 1.0) * frequency as f64 * std::f64::consts::PI
                / (2.0 * n as f64))
                .cos()
        };
        let mut sum = 0.0f64;
        for v in 0..n {
            for u in 0..n {
                sum += c(u) * c(v) * f64::from(block[v * 8 + u]) * basis(x, u) * basis(y, v);
            }
        }
        let half = f64::from(1u32 << (precision - 1));
        let ceiling = f64::from((1u32 << precision) - 1);
        let level = ((sum / 4.0).round() + half).clamp(0.0, ceiling);
        ((level as u32) >> (precision - 8)) as u8
    }

    /// The level P-09.1's definition gives a block with no AC term:
    /// `S(0, 0)/8`, rounded, level-shifted, clamped and narrowed.
    fn defined_dc_level(dc: i32, precision: u8) -> u8 {
        let half = f64::from(1u32 << (precision - 1));
        let ceiling = f64::from((1u32 << precision) - 1);
        let level = ((f64::from(dc) / 8.0).round() + half).clamp(0.0, ceiling);
        ((level as u32) >> (precision - 8)) as u8
    }

    /// **Each scale is its fraction of a length**, rounded up, and the scales
    /// run full size first, as the `jpeg` fuzz target's length picks them.
    #[test]
    fn the_scales_are_their_fractions_of_a_length() {
        assert_eq!(
            JpegScale::ALL,
            [
                JpegScale::Full,
                JpegScale::Half,
                JpegScale::Quarter,
                JpegScale::Eighth
            ]
        );
        assert_eq!(JpegScale::default(), JpegScale::Full);
        let n: Vec<u32> = JpegScale::ALL.iter().map(|s| s.block_samples()).collect();
        assert_eq!(n, [8, 4, 2, 1]);
        for full in [
            0u32,
            1,
            2,
            3,
            7,
            8,
            9,
            15,
            16,
            17,
            1240,
            1754,
            65_535,
            u32::MAX,
        ] {
            for scale in JpegScale::ALL {
                let n = u64::from(scale.block_samples());
                let expected = (u64::from(full) * n + 7) / 8;
                assert_eq!(
                    u64::from(scale.length(full)),
                    expected,
                    "{full} at {scale:?}"
                );
            }
            assert_eq!(JpegScale::Full.length(full), full);
        }
        // The pairs the design names: a quarter of 1240 x 1754 is 310 x 439,
        // and one sample an eighth of a pixel wide still covers it.
        assert_eq!(
            (
                JpegScale::Quarter.length(1240),
                JpegScale::Quarter.length(1754)
            ),
            (310, 439)
        );
        assert_eq!(JpegScale::Eighth.length(1), 1);
    }

    /// **The reduced transforms' cosines are their formula**, and every entry
    /// is far enough from a rounding boundary that every target's `cos`
    /// rounds it to the same integer.
    #[test]
    fn the_reduced_cosines_match_their_formula() {
        fn check<const N: usize>(table: &[[i64; N]; N]) {
            for (x, row) in table.iter().enumerate() {
                for (u, &entry) in row.iter().enumerate() {
                    let cu = if u == 0 {
                        1.0 / std::f64::consts::SQRT_2
                    } else {
                        1.0
                    };
                    let exact = cu / 2.0
                        * ((2.0 * x as f64 + 1.0) * u as f64 * std::f64::consts::PI
                            / (2.0 * N as f64))
                            .cos()
                        * 65536.0;
                    assert_eq!(entry, exact.round() as i64, "K_{N}[{x}][{u}]");
                    let from_half = (exact - exact.floor() - 0.5).abs();
                    assert!(
                        from_half > 0.02,
                        "K_{N}[{x}][{u}] is {from_half} from rounding the other way"
                    );
                    // C.2's symmetry: x and N - 1 - x differ by (-1)^u.
                    let mirrored = table[N - 1 - x][u];
                    assert_eq!(mirrored, if u % 2 == 0 { entry } else { -entry });
                }
            }
            // The DC column is 2^16 / (2 sqrt 2) everywhere.
            assert!(
                table.iter().all(|row| row[0] == 23_170),
                "K_{N}'s DC column"
            );
        }
        check(&REDUCED.four);
        check(&REDUCED.two);
        assert_eq!(kept_span(8), 64);
        assert_eq!(kept_span(4), 25);
        assert_eq!(kept_span(2), 5);
        assert_eq!(kept_span(1), 1);
    }

    /// **A flat block is the same level at every scale, exactly**: a block
    /// whose one coefficient is its DC gives `dc_level` at every sample of the
    /// full transform and of each reduced one, at both precisions and for
    /// every DC a frame can carry — and a block whose kept coefficients have no
    /// AC term, whatever lies outside them, gives the same level at its scale.
    /// Through the decode, flat images from the encoder and a flat
    /// arithmetic-coded frame decode at every scale to the full decode's one
    /// colour.
    #[test]
    fn a_flat_block_is_the_same_level_at_every_scale() {
        let mut noise = streams::Noise(0xF1A7_B10C_0000_0001);
        let mut dcs: Vec<i32> = (-16_384..=16_384).step_by(7).collect();
        dcs.extend([
            i32::MIN,
            i32::MIN + 1,
            -(1 << 20) - 1,
            -(1 << 20),
            -1,
            0,
            1,
            1 << 20,
            (1 << 20) + 1,
            i32::MAX,
        ]);
        for precision in [8u8, 12] {
            for &dc in &dcs {
                let mut block = [0i32; 64];
                block[0] = dc;
                let level = dc_level(dc, precision);
                let mut full = [0u8; 64];
                idct_block(&block, precision, &mut full);
                assert!(
                    full.iter().all(|&s| s == level),
                    "{dc} at {precision} bits, full size"
                );
                for (_, n) in REDUCED_SCALES {
                    let reduced = reduce(&block, n, precision);
                    assert_eq!(reduced.len(), n * n);
                    assert!(
                        reduced.iter().all(|&s| s == level),
                        "{dc} at {precision} bits, {n}/8"
                    );
                }
                // AC terms outside the kept square change nothing at its
                // scale.
                for (_, n) in REDUCED_SCALES {
                    let mut outside = block;
                    for (index, slot) in outside.iter_mut().enumerate() {
                        let (v, u) = (index / 8, index % 8);
                        if (v >= n || u >= n) && noise.below(3) == 0 {
                            *slot = noise.next() as i32 >> 8;
                        }
                    }
                    let reduced = reduce(&outside, n, precision);
                    assert!(
                        reduced.iter().all(|&s| s == level),
                        "{dc} with AC outside the {n} x {n} at {precision} bits"
                    );
                }
            }
        }

        // Through the decode: every frame is flat, so each scale's image is
        // the full decode's one colour, sample for sample.
        let mut flat: Vec<(String, Vec<u8>)> = Vec::new();
        for (width, height) in [(1u32, 1u32), (8, 8), (13, 7), (17, 33), (64, 48)] {
            for value in [0u8, 31, 128, 144, 255] {
                flat.push((
                    format!("flat gray {width}x{height} of {value}"),
                    huffman_flat_gray(width, height, value, 0),
                ));
                flat.push((
                    format!("flat gray {width}x{height} of {value}, restarting"),
                    huffman_flat_gray(width, height, value, 2),
                ));
            }
            for (sampling, colour) in [
                (JpegSampling::FourFourFour, [200u8, 40, 90]),
                (JpegSampling::FourTwoZero, [12, 230, 128]),
            ] {
                let raster: Vec<u8> = (0..width * height).flat_map(|_| colour).collect();
                let bytes = jpeg_encode(
                    &JpegSource {
                        width,
                        height,
                        colour: JpegSourceColour::Rgb,
                        stride: width as usize * 3,
                        data: &raster,
                    },
                    &JpegOptions {
                        quantisation: JpegQuantisation::AnnexKHalved,
                        sampling,
                        restart_interval: 0,
                    },
                )
                .expect("a flat raster encodes");
                flat.push((format!("flat {sampling:?} {width}x{height}"), bytes));
            }
        }
        let mut decisions = diff_of_128();
        decisions.push((arith::Bin::Ac(0), 1));
        flat.push((
            "a flat SOF9 block".to_string(),
            arithmetic_gray(
                0xC9,
                8,
                8,
                None,
                None,
                &[(0x00, 0x3F, 0x00, arith_segment(&decisions))],
            ),
        ));
        flat.push((
            "a twelve-bit flat pixel".to_string(),
            tiny_gray_at(0xC1, 12),
        ));
        for (label, bytes) in &flat {
            let full = decode(bytes, CEILING).unwrap_or_else(|e| panic!("{label}: {e:?}"));
            let pixel = full.color.components();
            let first = &full.data[..pixel];
            assert!(
                full.data.chunks(pixel).all(|p| p == first),
                "{label}: the full decode is flat"
            );
            for (scale, _) in REDUCED_SCALES {
                let mut kept = Kept::default();
                let frame = decode_strips(bytes, CEILING, scale, &mut kept)
                    .unwrap_or_else(|e| panic!("{label} at {scale:?}: {e:?}"));
                assert_eq!(
                    (frame.layout.width, frame.layout.height),
                    (scale.length(full.width), scale.length(full.height)),
                    "{label} at {scale:?}"
                );
                assert_eq!(frame.warnings, full.warnings, "{label} at {scale:?}");
                assert!(
                    kept.data.chunks(pixel).all(|p| p == first),
                    "{label} at {scale:?}: a flat image is the full decode's level"
                );
            }
        }
    }

    /// **An eighth-scale decode is each block's DC level**: every sample of
    /// each component is `dc_level` of the block it lies in, read from the
    /// frame's own coefficients, replicated by its sampling and converted as
    /// a full-size sample is — or the 128 a ratio that does not divide leaves
    /// a sample no block covers — for every frame type, both coders, both
    /// precisions and every sampling and colour the generator draws.
    #[test]
    fn an_eighth_scale_decode_is_each_blocks_dc_level() {
        let mut noise = streams::Noise(0xE16E_7D0C_0000_0008);
        let mut images = 0usize;
        let mut reached = std::collections::BTreeMap::<String, usize>::new();
        let mut inputs: Vec<(String, Vec<u8>, Option<streams::Recipe>)> = Vec::new();
        for _ in 0..500 {
            let recipe = streams::random_recipe(&mut noise);
            let bytes = streams::stream(&recipe, &mut noise);
            inputs.push((format!("{recipe:?}"), bytes, Some(recipe)));
        }
        for (label, bytes) in streams::encoded().into_iter().chain(streams::seeds()) {
            inputs.push((label, bytes, None));
        }
        for (label, bytes, recipe) in inputs {
            let mut kept = Kept::default();
            let decoded = decode_strips(&bytes, CEILING, JpegScale::Eighth, &mut kept);
            let Ok(scanned) = scan_frame(&bytes, CEILING, JpegScale::Eighth, None) else {
                assert!(decoded.is_err(), "{label}: the marker walk failed");
                continue;
            };
            let Ok(frame) = decoded else {
                assert!(
                    kept.strips.is_empty(),
                    "{label}: a failure handed out a strip"
                );
                continue;
            };
            images += 1;
            let layout = frame.layout;
            let (width, height) = (layout.width as usize, layout.height as usize);
            assert_eq!(
                (layout.full_width, layout.full_height),
                (scanned.width as u32, scanned.height as u32),
                "{label}"
            );
            assert_eq!(
                (width, height),
                (scanned.width.div_ceil(8), scanned.height.div_ceil(8)),
                "{label}"
            );
            let h_max = scanned.components.iter().map(|c| c.h).max().unwrap_or(1);
            let v_max = scanned.components.iter().map(|c| c.v).max().unwrap_or(1);
            let planes: Vec<Vec<u8>> = scanned
                .components
                .iter()
                .map(|component| {
                    let (sx, sy) = (h_max / component.h, v_max / component.v);
                    let q = i32::from(scanned.quant[component.quant][0]);
                    let mut plane = vec![128u8; width * height];
                    for y in 0..height {
                        for x in 0..width {
                            if let Some(block) = component.block(x / sx, y / sy) {
                                let dc = i32::from(block[0]).saturating_mul(q);
                                plane[y * width + x] = dc_level(dc, scanned.precision);
                            }
                        }
                    }
                    plane
                })
                .collect();
            let row_bytes = width * layout.color.components();
            let mut expected = vec![0u8; row_bytes * height];
            for (y, line) in expected.chunks_exact_mut(row_bytes).enumerate() {
                let mut lines: [&[u8]; 4] = [&[]; 4];
                for (slot, plane) in lines.iter_mut().zip(&planes) {
                    *slot = &plane[y * width..(y + 1) * width];
                }
                convert(
                    layout.color,
                    scanned.adobe_transform.unwrap_or(0),
                    &lines,
                    line,
                );
            }
            assert!(
                kept.data == expected,
                "{label}: the eighth-scale image is not each block's DC level"
            );
            assert_eq!(
                frame.transformed,
                planes_blocks(&scanned) as u64,
                "{label}: one sample a block"
            );
            if let Some(recipe) = recipe {
                for class in [
                    format!("frame {:#04x}", recipe.frame),
                    format!("{} bits", recipe.precision),
                    format!("{:?}", layout.color),
                    format!("divides {}", recipe.divides()),
                ] {
                    *reached.entry(class).or_default() += 1;
                }
            }
        }
        assert!(images > 300, "{images} streams decoded to an image");
        for class in [
            "frame 0xc0",
            "frame 0xc1",
            "frame 0xc2",
            "frame 0xc9",
            "frame 0xca",
            "8 bits",
            "12 bits",
            "Gray",
            "Rgb",
            "Cmyk",
            "CmykInverted",
            "divides false",
        ] {
            let count = reached.get(class).copied().unwrap_or(0);
            assert!(count >= 5, "{class}: {count} images, {reached:?}");
        }
    }

    /// The blocks a decode of `scanned` transforms: each component's that
    /// the buffer holds and that some sample of the image lies in.
    fn planes_blocks(scanned: &Scanned) -> usize {
        let h_max = scanned.components.iter().map(|c| c.h).max().unwrap_or(1);
        let v_max = scanned.components.iter().map(|c| c.v).max().unwrap_or(1);
        scanned
            .components
            .iter()
            .map(|component| {
                let (sx, sy) = (h_max / component.h, v_max / component.v);
                let across = scanned.width.div_ceil(8 * sx).min(component.blocks_x);
                let down = scanned.height.div_ceil(8 * sy).min(component.blocks_y);
                (0..down)
                    .flat_map(|by| (0..across).map(move |bx| (bx, by)))
                    .filter(|&(bx, by)| component.block(bx, by).is_some())
                    .count()
            })
            .sum()
    }

    /// **The reduced transforms are within one level of their definition**:
    /// random blocks, at 1/2, 1/4 and 1/8, at eight bits with kept
    /// coefficients as large as an eight-bit frame's samples make them — 1 152
    /// once dequantised — and at twelve with sixteen times that, against
    /// P-09.1's formula evaluated in `f64`; a block with no kept AC term
    /// against `S(0, 0)/8`. The rounded cosines move a sample by at most
    /// `2^-17` of the kept coefficients' summed magnitude and the last
    /// rounding by half a level, which keeps every sample within one.
    #[test]
    fn the_reduced_transforms_match_their_definition() {
        let mut noise = streams::Noise(0x00DE_F1AE_D0BE_EF00);
        let (mut transformed, mut constant) = (0usize, 0usize);
        for case in 0..60_000 {
            let precision = if case % 3 == 0 { 12 } else { 8 };
            let largest: i64 = if precision == 8 { 1_152 } else { 18_432 };
            let (_, n) = REDUCED_SCALES[noise.below(3) as usize];
            let mut block = [0i32; 64];
            let density = noise.below(4);
            for v in 0..n {
                for u in 0..n {
                    let keep = match density {
                        0 => (u, v) == (0, 0) || noise.below(8) == 0,
                        1 => noise.below(2) == 0,
                        _ => true,
                    };
                    if keep {
                        let magnitude = if noise.below(8) == 0 {
                            largest
                        } else {
                            (noise.next() % (largest as u64 + 1)) as i64
                        };
                        block[v * 8 + u] = (if noise.below(2) == 0 {
                            magnitude
                        } else {
                            -magnitude
                        }) as i32;
                    }
                }
            }
            // What lies outside the kept square is never read.
            for (index, slot) in block.iter_mut().enumerate() {
                if (index / 8 >= n || index % 8 >= n) && noise.below(4) == 0 {
                    *slot = (noise.next() % 2048) as i32 - 1024;
                }
            }
            let ac = (0..n).any(|v| (0..n).any(|u| (u, v) != (0, 0) && block[v * 8 + u] != 0));
            let reduced = reduce(&block, n, precision);
            for y in 0..n {
                for x in 0..n {
                    let want = if ac {
                        defined_level(&block, n, precision, x, y)
                    } else {
                        defined_dc_level(block[0], precision)
                    };
                    let got = reduced[y * n + x];
                    assert!(
                        got.abs_diff(want) <= 1,
                        "case {case}: ({x}, {y}) at {n}/8 and {precision} bits is {got}, \
                         the definition {want}: {block:?}"
                    );
                }
            }
            // The direct double sum is the same integers.
            let mut direct = [0u8; 16];
            direct::idct_reduced(&block, n, precision, &mut direct);
            assert_eq!(
                &direct[..n * n],
                &reduced[..],
                "case {case}: the double sum"
            );
            if ac {
                transformed += 1;
            } else {
                constant += 1;
            }
        }
        assert!(
            transformed > 25_000,
            "{transformed} blocks took a transform"
        );
        assert!(constant > 15_000, "{constant} blocks took the DC level");
    }

    /// **A reduced transform takes any coefficient a dequantiser can hand it**
    /// — every magnitude to `i32`'s ends, every kept position, both
    /// precisions — without leaving `i64`, and the direct double sum agrees.
    #[test]
    fn a_reduced_transform_takes_any_coefficient() {
        let mut noise = streams::Noise(0x0BAD_C0EF_F1C1_E175);
        let extremes = [
            i32::MIN,
            i32::MIN + 1,
            -(1 << 20) - 1,
            -(1 << 20),
            (1 << 20) - 1,
            1 << 20,
            (1 << 20) + 1,
            i32::MAX,
        ];
        for case in 0..20_000 {
            let precision = if case % 2 == 0 { 8 } else { 12 };
            let n = [4usize, 2][noise.below(2) as usize];
            let mut block = [0i32; 64];
            for v in 0..n {
                for u in 0..n {
                    block[v * 8 + u] = if noise.below(2) == 0 {
                        extremes[noise.below(extremes.len() as u64) as usize]
                    } else {
                        noise.next() as i32
                    };
                }
            }
            let reduced = reduce(&block, n, precision);
            let mut direct = [0u8; 16];
            direct::idct_reduced(&block, n, precision, &mut direct);
            assert_eq!(&direct[..n * n], &reduced[..], "case {case}: {block:?}");
        }
        // Every coefficient at the clamp or past it, both signs: a block of
        // `2^20` everywhere is a block of `2^20 + 1` everywhere.
        for n in [4usize, 2] {
            for precision in [8u8, 12] {
                for sign in [1i32, -1] {
                    let mut at = [0i32; 64];
                    let mut past = [0i32; 64];
                    for v in 0..n {
                        for u in 0..n {
                            at[v * 8 + u] = sign * (1 << 20);
                            past[v * 8 + u] = if sign > 0 { i32::MAX } else { i32::MIN };
                        }
                    }
                    assert_eq!(
                        reduce(&at, n, precision),
                        reduce(&past, n, precision),
                        "{n}/8 at {precision} bits, sign {sign}"
                    );
                }
            }
        }
    }

    /// **A quarter-scale decode transforms a sixteenth of the samples**,
    /// counted: the transforms of every frame output `N × N` samples a block
    /// at `N/8`, the same blocks at every scale, so a half-scale decode
    /// outputs a quarter of the full decode's, a quarter-scale one a
    /// sixteenth and an eighth-scale one a sixty-fourth — and the full decode
    /// sixty-four for each block some sample of the image lies in.
    #[test]
    fn a_quarter_scale_decode_transforms_a_sixteenth_of_the_samples() {
        let mut noise = streams::Noise(0x0016_7A45_F0E0_0004);
        let mut inputs: Vec<(String, Vec<u8>)> = Vec::new();
        for _ in 0..300 {
            let recipe = streams::random_recipe(&mut noise);
            let bytes = streams::stream(&recipe, &mut noise);
            inputs.push((format!("{recipe:?}"), bytes));
        }
        inputs.extend(streams::encoded());
        inputs.extend(streams::seeds());
        let mut counted = 0usize;
        for (label, bytes) in &inputs {
            let mut kept = Kept::default();
            let Ok(full) = decode_strips(bytes, CEILING, JpegScale::Full, &mut kept) else {
                continue;
            };
            let scanned = scan_frame(bytes, CEILING, JpegScale::Full, None)
                .unwrap_or_else(|e| panic!("{label}: {e:?}"));
            assert_eq!(
                full.transformed,
                64 * planes_blocks(&scanned) as u64,
                "{label}: sixty-four samples a block at full size"
            );
            for (scale, divisor) in [
                (JpegScale::Half, 4u64),
                (JpegScale::Quarter, 16),
                (JpegScale::Eighth, 64),
            ] {
                let mut kept = Kept::default();
                let frame = decode_strips(bytes, CEILING, scale, &mut kept)
                    .unwrap_or_else(|e| panic!("{label} at {scale:?}: {e:?}"));
                assert_eq!(
                    frame.transformed * divisor,
                    full.transformed,
                    "{label} at {scale:?}"
                );
            }
            counted += 1;
        }
        assert!(counted > 200, "{counted} streams were counted");

        // One image whose count is known by hand: 64 x 48 at 4:2:0 is 8 x 6
        // luma blocks and 4 x 3 of each chroma, 72 in all.
        let raster = vec![77u8; 64 * 48 * 3];
        let bytes = jpeg_encode(
            &JpegSource {
                width: 64,
                height: 48,
                colour: JpegSourceColour::Rgb,
                stride: 64 * 3,
                data: &raster,
            },
            &JpegOptions {
                quantisation: JpegQuantisation::AnnexKHalved,
                sampling: JpegSampling::FourTwoZero,
                restart_interval: 0,
            },
        )
        .expect("it encodes");
        let mut kept = Kept::default();
        let quarter = decode_strips(&bytes, CEILING, JpegScale::Quarter, &mut kept)
            .expect("it decodes at a quarter");
        assert_eq!(quarter.transformed, 72 * 4);
        let full = decode_strips(&bytes, CEILING, JpegScale::Full, &mut Kept::default())
            .expect("it decodes");
        assert_eq!(full.transformed, 72 * 64);
        assert_eq!((quarter.layout.width, quarter.layout.height), (16, 12));
    }

    /// **A reduced strip decode is the whole-image reduced decode**, row for
    /// row: at 1/2, 1/4 and 1/8, through the sink, through the whole image a
    /// streaming frame paints and through the buffered coefficients, against
    /// whole-image planes made by the reduced transform's double sum, over
    /// every stream P-09.4's equality runs over — every frame type and both
    /// coders, 8 and 12 bits, every sampling the generator draws, ratios that
    /// do not divide, odd sizes, every colour model, restarts and cuts. The
    /// layout is the scale's fraction of the frame and names both; every strip
    /// but the last is `N · Vmax` rows; the planes are at most one MCU row; a
    /// stream that fails fails alike at every scale and hands the sink
    /// nothing.
    #[test]
    fn a_reduced_strip_decode_is_the_whole_image_reduced_decode() {
        let mut noise = streams::Noise(0x5CA1_ED57_1295_0002);
        let mut images = 0usize;
        let mut reached = std::collections::BTreeMap::<String, usize>::new();
        for (label, bytes, recipe) in every_stream(&mut noise) {
            let full = decode_through_planes(&bytes, CEILING);
            for (scale, n) in REDUCED_SCALES {
                let label = format!("{label} at {scale:?}");
                let planes = decode_through_planes_at(&bytes, CEILING, scale);
                let painted = decode_at(&bytes, CEILING, scale);
                same(&label, &painted, &planes);
                same(&label, &decode_buffered_at(&bytes, CEILING, scale), &planes);

                let mut kept = Kept::default();
                let frame = decode_strips(&bytes, CEILING, scale, &mut kept);
                match (&planes, &frame, &full) {
                    (Ok(image), Ok(frame), Ok(full)) => {
                        images += 1;
                        let layout = frame.layout;
                        assert_eq!(kept.layouts, vec![layout], "{label}");
                        assert_eq!(
                            (layout.width, layout.height, layout.color, layout.precision),
                            (image.width, image.height, image.color, image.precision),
                            "{label}"
                        );
                        assert_eq!(
                            (layout.scale, layout.full_width, layout.full_height),
                            (scale, full.width, full.height),
                            "{label}"
                        );
                        assert_eq!(
                            (layout.width, layout.height),
                            (scale.length(full.width), scale.length(full.height)),
                            "{label}"
                        );
                        assert_eq!(frame.warnings, image.warnings, "{label}");
                        assert_eq!(frame.warnings, full.warnings, "{label}");
                        assert!(
                            kept.data == image.data,
                            "{label}: the strips are not the image"
                        );
                        if let Some(recipe) = &recipe {
                            let strip_rows = (n * recipe.v_max()) as u32;
                            if let Some((last, rest)) = kept.strips.split_last() {
                                for &(_, rows) in rest {
                                    assert_eq!(rows, strip_rows, "{label}");
                                }
                                assert!(last.1 <= strip_rows, "{label}: the last strip");
                            }
                            if recipe.divides() {
                                let row = layout.width as usize * layout.color.components();
                                assert!(
                                    frame.held.planes <= strip_rows as usize * row,
                                    "{label}: {} plane bytes",
                                    frame.held.planes
                                );
                            }
                            for class in [
                                format!("{scale:?} frame {:#04x}", recipe.frame),
                                format!("{scale:?} {} bits", recipe.precision),
                                format!("{scale:?} {:?}", image.color),
                                format!("{scale:?} {:?}", recipe.scans),
                                format!("{scale:?} divides {}", recipe.divides()),
                                format!("{scale:?} restart {}", recipe.restart > 0),
                            ] {
                                *reached.entry(class).or_default() += 1;
                            }
                        }
                    }
                    (Err(want), Err(got), Err(at_full)) => {
                        assert_eq!(got, want, "{label}");
                        assert_eq!(got, at_full, "{label}: a failure fails at every scale");
                        assert!(
                            kept.layouts.is_empty() && kept.strips.is_empty(),
                            "{label}: a decode that failed handed out a strip"
                        );
                    }
                    _ => panic!("{label}: the scales disagree on success"),
                }
            }
        }
        assert!(images > 1_800, "{images} reduced images decoded");
        for scale in ["Half", "Quarter", "Eighth"] {
            for class in [
                "frame 0xc0",
                "frame 0xc1",
                "frame 0xc2",
                "frame 0xc9",
                "frame 0xca",
                "8 bits",
                "12 bits",
                "Gray",
                "Rgb",
                "Cmyk",
                "CmykInverted",
                "Interleaved",
                "EachAlone",
                "Split",
                "Progressive",
                "divides false",
                "restart true",
            ] {
                let key = format!("{scale} {class}");
                let count = reached.get(&key).copied().unwrap_or(0);
                assert!(count >= 10, "{key}: {count} images");
            }
        }
    }

    /// **A twelve-bit frame is narrowed at every scale, and says so**: the
    /// same warning at each, and mid-grey stays mid-grey.
    #[test]
    fn every_scale_narrows_a_twelve_bit_frame_and_says_so() {
        for scale in JpegScale::ALL {
            let mut kept = Kept::default();
            let frame = decode_strips(&tiny_gray_at(0xC1, 12), 1 << 20, scale, &mut kept)
                .expect("SOF1 at twelve bits decodes");
            assert_eq!(frame.layout.precision, 12, "{scale:?}");
            assert_eq!(
                frame.warnings,
                vec![Warning::JpegPrecisionNarrowed],
                "{scale:?}"
            );
            assert_eq!(kept.data, vec![128], "{scale:?}");
        }
    }

    /// **The ceiling bounds the frame's own samples at every scale**, since
    /// the coefficients a decode holds are the frame's: a frame one sample
    /// past it is refused at every scale, whatever the reduced image would
    /// hold, and one at it decodes at every scale.
    #[test]
    fn the_ceiling_bounds_the_frame_at_every_scale() {
        let bytes = huffman_flat_gray(64, 48, 99, 0);
        for scale in JpegScale::ALL {
            let mut kept = Kept::default();
            assert_eq!(
                decode_strips(&bytes, 64 * 48 - 1, scale, &mut kept).err(),
                Some(JpegError::Truncated),
                "{scale:?}"
            );
            assert!(
                kept.layouts.is_empty() && kept.strips.is_empty(),
                "{scale:?}"
            );
            let frame = decode_strips(&bytes, 64 * 48, scale, &mut Kept::default())
                .unwrap_or_else(|e| panic!("{scale:?}: {e:?}"));
            assert_eq!(
                (frame.layout.width, frame.layout.height),
                (scale.length(64), scale.length(48))
            );
            assert!(decode_strips(&tiny_gray(), 0, scale, &mut Kept::default()).is_err());
        }
    }

    /// **Arbitrary bytes terminate at every scale**, as they do at full size:
    /// the same sweep as `arbitrary_bytes_terminate_without_panicking`, every
    /// cut of a small frame included.
    #[test]
    fn arbitrary_bytes_terminate_at_every_scale() {
        for (scale, _) in REDUCED_SCALES {
            for len in 0..512usize {
                let data: Vec<u8> = (0..len).map(|i| ((i * 31) % 256) as u8).collect();
                let _ = decode_strips(&data, 1 << 16, scale, &mut Kept::default());
            }
            for cut in 0..tiny_gray().len() {
                let mut truncated = tiny_gray();
                truncated.truncate(cut);
                let _ = decode_strips(&truncated, 1 << 20, scale, &mut Kept::default());
            }
            for cut in 0..progressive_refined_block().len() {
                let mut truncated = progressive_refined_block();
                truncated.truncate(cut);
                let _ = decode_strips(&truncated, 1 << 20, scale, &mut Kept::default());
            }
        }
    }

    /// **The kept coefficients are a block's top-left square**, out of
    /// zig-zag order and dequantised as the full transform's are, whatever
    /// the rest of the block holds: the positions below `kept_span` reach
    /// every one of them.
    #[test]
    fn the_kept_coefficients_are_the_top_left_square() {
        fn square<const N: usize>(natural: &[i32; 64], kept: [[i32; N]; N], case: usize) {
            for (v, row) in kept.iter().enumerate() {
                for (u, &value) in row.iter().enumerate() {
                    assert_eq!(value, natural[v * 8 + u], "case {case}: ({u}, {v}) of {N}");
                }
            }
        }
        let mut noise = streams::Noise(0x4EE7_0000_5A11_0001);
        for case in 0..5_000 {
            let source: Vec<i16> = (0..64)
                .map(|_| {
                    if noise.below(3) == 0 {
                        0
                    } else {
                        noise.next() as i16
                    }
                })
                .collect();
            let mut table = [0u16; 64];
            for slot in table.iter_mut() {
                *slot = 1 + noise.below(65_535) as u16;
            }
            let mut natural = [0i32; 64];
            dequantise(&source, &table, &mut natural);
            square(&natural, kept::<4>(&source, &table, kept_span(4)), case);
            square(&natural, kept::<2>(&source, &table, kept_span(2)), case);
            square(&natural, kept::<1>(&source, &table, kept_span(1)), case);
        }
    }
}
