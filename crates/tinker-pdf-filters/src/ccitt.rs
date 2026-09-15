//! CCITT Group 3 and Group 4 fax decoding (7.4.6; ITU-T T.4, T.6).
//!
//! Scanned documents are mostly this. A row is coded as runs of white and
//! black, either by their lengths against a Huffman table (one-dimensional,
//! T.4) or by where their changes sit relative to the row above
//! (two-dimensional, T.6) — which is what makes a fax of mostly-blank paper
//! so small.
//!
//! Output is packed one bit per pixel, most significant bit first, each row
//! padded out to a byte boundary — the shape `/BitsPerComponent 1` describes,
//! so the caller reads it with the same sample loop it reads every other
//! image with, and `/ImageMask`, `/Decode` and `/ColorSpace` apply to a fax
//! the way they apply to anything else.
//!
//! Polarity is a parameter rather than a convention. T.4 codes runs of white
//! and black with no bit values involved at all; `/BlackIs1` (7.4.6, Table 11)
//! says which bit value the black ones take, and its default of false means
//! **0 is black** — which is also what 0 means in a one-bit DeviceGray
//! sample. So the default composes into an upright image, and a file that
//! sets `/BlackIs1 true` produces a negative unless it also says
//! `/Decode [1 0]`, exactly as the specification describes.

use crate::Warning;

mod encode;

pub use encode::{g4_encode, CcittEncodeError, CcittSource};

/// How the data is coded (7.4.6, Table 11).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CcittParams {
    /// `/K`: negative is pure two-dimensional (G4), zero pure
    /// one-dimensional (G3), positive mixed — each line announcing its own
    /// mode with the tag bit T.4 §4.2.1.3.1 hangs off the EOL.
    pub k: i32,
    /// `/Columns`, the pixels per row.
    pub columns: u32,
    /// `/Rows`; zero means "until the data runs out".
    pub rows: u32,
    /// `/BlackIs1`: whether a 1 bit means black.
    pub black_is_1: bool,
    /// `/EncodedByteAlign`: whether each row starts on a byte boundary.
    pub byte_align: bool,
    /// `/EndOfLine`: whether the encoding is *required* to carry an EOL
    /// before every line. An EOL is honoured wherever it appears whatever
    /// this says; true additionally makes a missing one worth reporting.
    pub end_of_line: bool,
    /// `/EndOfBlock`: whether the data ends with an EOFB pattern. True, the
    /// default, means the pattern terminates the image and `/Rows` does not;
    /// false means `/Rows` is the authority and nothing past it is read.
    pub end_of_block: bool,
}

impl Default for CcittParams {
    fn default() -> Self {
        CcittParams {
            k: 0,
            columns: 1728,
            rows: 0,
            black_is_1: false,
            byte_align: false,
            end_of_line: false,
            end_of_block: true,
        }
    }
}

/// Reads bits most-significant first.
struct Bits<'a> {
    data: &'a [u8],
    at: usize,
}

impl<'a> Bits<'a> {
    fn new(data: &'a [u8]) -> Bits<'a> {
        Bits { data, at: 0 }
    }

    fn exhausted(&self) -> bool {
        self.at >= self.data.len() * 8
    }

    fn peek(&self, count: u32) -> u32 {
        let mut value = 0u32;
        for i in 0..count.min(24) {
            let index = self.at + i as usize;
            let bit = self
                .data
                .get(index / 8)
                .map_or(0, |byte| (byte >> (7 - (index % 8))) & 1);
            value = (value << 1) | u32::from(bit);
        }
        value
    }

    fn skip(&mut self, count: u32) {
        self.at = self.at.saturating_add(count as usize);
    }

    /// Whether an EOL sits at the cursor (T.4 §4.1.2: eleven zeros then a one).
    ///
    /// A stream may pad with extra zeros to byte-align the code that follows,
    /// so any run of at least eleven zeros ending in a one is an EOL.
    fn at_eol(&self) -> bool {
        let mut zeros = 0u32;
        let mut index = self.at;
        let end = self.data.len() * 8;
        while index < end && zeros < 64 {
            let bit = self
                .data
                .get(index / 8)
                .map_or(0, |byte| (byte >> (7 - (index % 8))) & 1);
            if bit == 1 {
                return zeros >= 11;
            }
            zeros += 1;
            index += 1;
        }
        false
    }

    /// Steps past an EOL, however much zero padding it carries.
    fn skip_eol(&mut self) {
        let end = self.data.len() * 8;
        while self.at < end {
            let bit = self
                .data
                .get(self.at / 8)
                .map_or(0, |byte| (byte >> (7 - (self.at % 8))) & 1);
            self.at += 1;
            if bit == 1 {
                return;
            }
        }
    }

    /// Whether the remaining bits are all zero.
    ///
    /// Trailing zero padding is how a stream ends when it carries no EOFB, and
    /// reading it as data yields a spurious truncation warning on a file that
    /// is perfectly well formed.
    fn only_padding_left(&self) -> bool {
        let end = self.data.len() * 8;
        (self.at..end).all(|index| {
            self.data
                .get(index / 8)
                .is_none_or(|byte| (byte >> (7 - (index % 8))) & 1 == 0)
        })
    }

    fn bit(&mut self) -> u32 {
        let value = self.peek(1);
        self.skip(1);
        value
    }

    /// Advances to the next byte boundary.
    fn align(&mut self) {
        self.at = self.at.div_ceil(8) * 8;
    }
}

/// `(bit length, code, run length)` for terminating white codes (T.4 table 2).
const WHITE_TERM: [(u32, u32, u16); 64] = [
    (8, 0x35, 0),
    (6, 0x07, 1),
    (4, 0x07, 2),
    (4, 0x08, 3),
    (4, 0x0B, 4),
    (4, 0x0C, 5),
    (4, 0x0E, 6),
    (4, 0x0F, 7),
    (5, 0x13, 8),
    (5, 0x14, 9),
    (5, 0x07, 10),
    (5, 0x08, 11),
    (6, 0x08, 12),
    (6, 0x03, 13),
    (6, 0x34, 14),
    (6, 0x35, 15),
    (6, 0x2A, 16),
    (6, 0x2B, 17),
    (7, 0x27, 18),
    (7, 0x0C, 19),
    (7, 0x08, 20),
    (7, 0x17, 21),
    (7, 0x03, 22),
    (7, 0x04, 23),
    (7, 0x28, 24),
    (7, 0x2B, 25),
    (7, 0x13, 26),
    (7, 0x24, 27),
    (7, 0x18, 28),
    (8, 0x02, 29),
    (8, 0x03, 30),
    (8, 0x1A, 31),
    (8, 0x1B, 32),
    (8, 0x12, 33),
    (8, 0x13, 34),
    (8, 0x14, 35),
    (8, 0x15, 36),
    (8, 0x16, 37),
    (8, 0x17, 38),
    (8, 0x28, 39),
    (8, 0x29, 40),
    (8, 0x2A, 41),
    (8, 0x2B, 42),
    (8, 0x2C, 43),
    (8, 0x2D, 44),
    (8, 0x04, 45),
    (8, 0x05, 46),
    (8, 0x0A, 47),
    (8, 0x0B, 48),
    (8, 0x52, 49),
    (8, 0x53, 50),
    (8, 0x54, 51),
    (8, 0x55, 52),
    (8, 0x24, 53),
    (8, 0x25, 54),
    (8, 0x58, 55),
    (8, 0x59, 56),
    (8, 0x5A, 57),
    (8, 0x5B, 58),
    (8, 0x4A, 59),
    (8, 0x4B, 60),
    (8, 0x32, 61),
    (8, 0x33, 62),
    (8, 0x34, 63),
];

/// Make-up codes for white runs of 64 and above (T.4 table 3).
const WHITE_MAKEUP: [(u32, u32, u16); 27] = [
    (5, 0x1B, 64),
    (5, 0x12, 128),
    (6, 0x17, 192),
    (7, 0x37, 256),
    (8, 0x36, 320),
    (8, 0x37, 384),
    (8, 0x64, 448),
    (8, 0x65, 512),
    (8, 0x68, 576),
    (8, 0x67, 640),
    (9, 0xCC, 704),
    (9, 0xCD, 768),
    (9, 0xD2, 832),
    (9, 0xD3, 896),
    (9, 0xD4, 960),
    (9, 0xD5, 1024),
    (9, 0xD6, 1088),
    (9, 0xD7, 1152),
    (9, 0xD8, 1216),
    (9, 0xD9, 1280),
    (9, 0xDA, 1344),
    (9, 0xDB, 1408),
    (9, 0x98, 1472),
    (9, 0x99, 1536),
    (9, 0x9A, 1600),
    (6, 0x18, 1664),
    (9, 0x9B, 1728),
];

/// Terminating black codes (T.4 table 2).
const BLACK_TERM: [(u32, u32, u16); 64] = [
    (10, 0x37, 0),
    (3, 0x02, 1),
    (2, 0x03, 2),
    (2, 0x02, 3),
    (3, 0x03, 4),
    (4, 0x03, 5),
    (4, 0x02, 6),
    (5, 0x03, 7),
    (6, 0x05, 8),
    (6, 0x04, 9),
    (7, 0x04, 10),
    (7, 0x05, 11),
    (7, 0x07, 12),
    (8, 0x04, 13),
    (8, 0x07, 14),
    (9, 0x18, 15),
    (10, 0x17, 16),
    (10, 0x18, 17),
    (10, 0x08, 18),
    (11, 0x67, 19),
    (11, 0x68, 20),
    (11, 0x6C, 21),
    (11, 0x37, 22),
    (11, 0x28, 23),
    (11, 0x17, 24),
    (11, 0x18, 25),
    (12, 0xCA, 26),
    (12, 0xCB, 27),
    (12, 0xCC, 28),
    (12, 0xCD, 29),
    (12, 0x68, 30),
    (12, 0x69, 31),
    (12, 0x6A, 32),
    (12, 0x6B, 33),
    (12, 0xD2, 34),
    (12, 0xD3, 35),
    (12, 0xD4, 36),
    (12, 0xD5, 37),
    (12, 0xD6, 38),
    (12, 0xD7, 39),
    (12, 0x6C, 40),
    (12, 0x6D, 41),
    (12, 0xDA, 42),
    (12, 0xDB, 43),
    (12, 0x54, 44),
    (12, 0x55, 45),
    (12, 0x56, 46),
    (12, 0x57, 47),
    (12, 0x64, 48),
    (12, 0x65, 49),
    (12, 0x52, 50),
    (12, 0x53, 51),
    (12, 0x24, 52),
    (12, 0x37, 53),
    (12, 0x38, 54),
    (12, 0x27, 55),
    (12, 0x28, 56),
    (12, 0x58, 57),
    (12, 0x59, 58),
    (12, 0x2B, 59),
    (12, 0x2C, 60),
    (12, 0x5A, 61),
    (12, 0x66, 62),
    (12, 0x67, 63),
];

/// Make-up codes for black runs (T.4 table 3).
const BLACK_MAKEUP: [(u32, u32, u16); 27] = [
    (10, 0x0F, 64),
    (12, 0xC8, 128),
    (12, 0xC9, 192),
    (12, 0x5B, 256),
    (12, 0x33, 320),
    (12, 0x34, 384),
    (12, 0x35, 448),
    (13, 0x6C, 512),
    (13, 0x6D, 576),
    (13, 0x4A, 640),
    (13, 0x4B, 704),
    (13, 0x4C, 768),
    (13, 0x4D, 832),
    (13, 0x72, 896),
    (13, 0x73, 960),
    (13, 0x74, 1024),
    (13, 0x75, 1088),
    (13, 0x76, 1152),
    (13, 0x77, 1216),
    (13, 0x52, 1280),
    (13, 0x53, 1344),
    (13, 0x54, 1408),
    (13, 0x55, 1472),
    (13, 0x5A, 1536),
    (13, 0x5B, 1600),
    (13, 0x64, 1664),
    (13, 0x65, 1728),
];

/// Extended make-up codes, shared by both colours (T.4 table 3b).
const EXT_MAKEUP: [(u32, u32, u16); 13] = [
    (11, 0x08, 1792),
    (11, 0x0C, 1856),
    (11, 0x0D, 1920),
    (12, 0x12, 1984),
    (12, 0x13, 2048),
    (12, 0x14, 2112),
    (12, 0x15, 2176),
    (12, 0x16, 2240),
    (12, 0x17, 2304),
    (12, 0x1C, 2368),
    (12, 0x1D, 2432),
    (12, 0x1E, 2496),
    (12, 0x1F, 2560),
];

/// One entry of a run-length table: `(bit length, code, run)`.
type RunCode = (u32, u32, u16);

/// Reads one run length, following make-up codes until a terminating one.
fn read_run(bits: &mut Bits, white: bool) -> Option<u32> {
    let mut total = 0u32;
    // A run is at most one terminating code plus a chain of make-ups; the
    // bound stops a corrupt stream looping.
    for _ in 0..64 {
        let (term, makeup): (&[RunCode], &[RunCode]) = if white {
            (&WHITE_TERM, &WHITE_MAKEUP)
        } else {
            (&BLACK_TERM, &BLACK_MAKEUP)
        };

        let mut matched = None;
        // Shorter codes first: the tables are prefix-free, so the first
        // length that matches is the right one.
        for length in 2..=14u32 {
            let candidate = bits.peek(length);
            if let Some((_, _, run)) = term
                .iter()
                .find(|(l, code, _)| *l == length && *code == candidate)
            {
                matched = Some((length, *run, true));
                break;
            }
            if let Some((_, _, run)) = makeup
                .iter()
                .chain(EXT_MAKEUP.iter())
                .find(|(l, code, _)| *l == length && *code == candidate)
            {
                matched = Some((length, *run, false));
                break;
            }
        }

        let (length, run, terminating) = matched?;
        bits.skip(length);
        total = total.saturating_add(u32::from(run));
        if terminating {
            return Some(total);
        }
        if bits.exhausted() {
            return Some(total);
        }
    }
    Some(total)
}

/// Records a leniency once. The crate's contract is one entry per condition
/// per decode, not one per occurrence.
fn note(warnings: &mut Vec<Warning>, warning: Warning) {
    if !warnings.contains(&warning) {
        warnings.push(warning);
    }
}

/// Bytes in one packed row of `columns` pixels.
fn row_bytes(columns: usize) -> usize {
    columns.div_ceil(8)
}

/// Sets `from..to` to the black bit value.
fn paint(row: &mut [u8], from: usize, to: usize, black_is_1: bool) {
    for index in from..to {
        if let Some(byte) = row.get_mut(index / 8) {
            let mask = 0x80u8 >> (index % 8);
            if black_is_1 {
                *byte |= mask;
            } else {
                *byte &= !mask;
            }
        }
    }
}

/// Packs one row's changing elements into `row`, most significant bit first.
///
/// A row starts white and alternates colour at each changing element (T.4
/// §4.1). `black_is_1` chooses the sense; the padding bits past `columns` in
/// the final byte take the white value, so a caller re-laying rows at a
/// different width never picks up a black edge that is not in the image.
fn pack_row(changes: &[usize], columns: usize, black_is_1: bool, row: &mut [u8]) {
    let bytes = row_bytes(columns);
    let white = if black_is_1 { 0x00 } else { 0xFF };
    for byte in row.iter_mut().take(bytes) {
        *byte = white;
    }

    let mut at = 0usize;
    let mut color_white = true;
    for &change in changes {
        let change = change.min(columns);
        if !color_white {
            paint(row, at.min(change), change, black_is_1);
        }
        at = change;
        color_white = !color_white;
    }
    if !color_white {
        paint(row, at, columns, black_is_1);
    }
}

/// Decodes CCITT data into packed one-bit-per-pixel rows.
///
/// Rows are `columns.div_ceil(8)` bytes each, most significant bit first, and
/// `max_output` bounds the whole result in bytes.
#[must_use]
pub fn decode(data: &[u8], params: &CcittParams, max_output: usize) -> (Vec<u8>, Vec<Warning>) {
    let mut warnings = Vec::new();
    let columns = params.columns.clamp(1, 1 << 16) as usize;
    let stride = row_bytes(columns);
    let mut out: Vec<u8> = Vec::new();
    let mut row = vec![0u8; stride];
    let mut bits = Bits::new(data);

    // The row above, as the positions where colour changes. A first row is
    // decoded against an imaginary all-white one.
    let mut reference: Vec<usize> = vec![columns, columns];
    let mut rows_done = 0u32;

    // 7.4.6: `/K` decides the coding. Negative is pure two-dimensional and
    // zero pure one-dimensional, so for those the mode is fixed for the whole
    // stream. Positive is mixed, and a mixed stream *announces* each line's
    // mode with the tag bit read below; until something announces otherwise
    // the mode is one-dimensional, which is what T.4 §4.2.1.3.1 requires of
    // the first line of a page.
    let mut two_dimensional = params.k < 0;

    while !bits.exhausted() {
        // 7.4.6, Table 11: `/EndOfBlock` says whether the data is terminated
        // by an EOFB pattern, "overriding the Rows parameter". Read at its
        // most literal that would let a stream with trailing bytes and no
        // EOFB decode past its own declared height, bounded only by
        // `max_output` — a quarter of a gigabyte of rows a caller that knows
        // the image is `/Height` tall will throw away. Ruling 1 asks for
        // bounded work on untrusted input, so `/Rows` stays a ceiling either
        // way and the override runs the other direction: an EOFB *before*
        // `/Rows` ends the image early, which is the case the spec is
        // actually describing. What `/EndOfBlock false` adds is that nothing
        // past the last row is read at all, not even looking for a pattern
        // the file has said is not there.
        if params.rows > 0 && rows_done >= params.rows {
            break;
        }
        if out.len().saturating_add(stride) > max_output {
            note(&mut warnings, Warning::OutputCapHit);
            break;
        }
        // T.4 §4.1.2: rows may be separated by an EOL, and a stream may open
        // with one. Two in a row is EOFB (T.6) or RTC (T.4) — the end of the
        // image, not a damaged row. Reading an EOL as data aborted the whole
        // image, which is what happened to every fax stream that carried them.
        if bits.at_eol() {
            bits.skip_eol();
            if bits.at_eol() {
                // EOFB/RTC: whatever follows is not image data.
                break;
            }
            if params.k > 0 && !bits.exhausted() {
                // T.4 §4.2.1.3.1: in mixed mode the EOL carries one more bit
                // — 1 says the line that follows is one-dimensionally coded,
                // 0 says two-dimensionally. The bit belongs to the EOL, so it
                // is read here and nowhere else; a row that arrives without
                // one keeps the mode last announced, because there is nothing
                // in the data to change it and the alternative is spending a
                // bit of that row's first code.
                two_dimensional = bits.bit() == 0;
                if bits.at_eol() {
                    // RTC in its mixed-mode shape is six EOL-plus-tag pairs
                    // (T.4 §4.1.2), so the second EOL only comes into view
                    // once the first tag is out of the way. Without this the
                    // terminator was decoded as a row and reported as damage.
                    break;
                }
            }
        } else if params.end_of_line && !bits.only_padding_left() {
            // Table 11 again: `/EndOfLine true` says the encoding carries one
            // before every line. It does not, here — and the bits left are
            // real rather than the zero padding a stream ends with. Decode the
            // row from the cursor anyway, because leniency is the house
            // policy, but ruling 10 wants the tolerance named rather than
            // absorbed: a stream whose EOLs went missing is a stream whose row
            // boundaries are only as good as its run lengths.
            note(&mut warnings, Warning::MissingEndOfLine);
        }
        if bits.exhausted() || bits.only_padding_left() {
            break;
        }

        let Some(changes) = decode_row(&mut bits, &reference, columns, two_dimensional) else {
            // A row that will not decode is one row, not the rest of the page.
            // Repeating the row above is what every fax decoder does with a
            // damaged line: it keeps the image legible and localises the loss.
            note(&mut warnings, Warning::TruncatedInput);
            if rows_done > 0 && out.len() >= stride && params.rows > rows_done {
                let previous: Vec<u8> = out[out.len() - stride..].to_vec();
                out.extend_from_slice(&previous);
                rows_done += 1;
                // Nothing else can be decoded from a broken bit position.
            }
            break;
        };

        pack_row(&changes, columns, params.black_is_1, &mut row);
        out.extend_from_slice(&row);
        rows_done += 1;

        reference = changes;
        reference.push(columns);
        reference.push(columns);

        if params.byte_align {
            bits.align();
        }
    }

    if params.rows > 0 && rows_done < params.rows {
        note(&mut warnings, Warning::TruncatedInput);
        // A short image is padded white rather than left ragged, so the
        // caller's row arithmetic still works.
        let missing = (params.rows - rows_done) as usize * stride;
        if out.len() + missing <= max_output {
            out.resize(
                out.len() + missing,
                if params.black_is_1 { 0x00 } else { 0xFF },
            );
        }
    }

    (out, warnings)
}

/// A two-dimensional (T.6) row decoder that resumes from a bit offset.
///
/// [`decode`] is whole-stream: it owns everything 7.4.6 wraps around the
/// coding — `/K`, `/Rows`, byte alignment, EOL and EOFB, the padding of a
/// short image. An MMR-coded region (T.88 6.2.6, which is how JBIG2 codes a
/// bilevel region without arithmetic coding) is the *same* T.6 coding with
/// none of that around it: it begins at an arbitrary bit position inside a
/// larger segment, runs for a height the segment header already gave, and the
/// reader has to know which bit it ended on to carry on parsing.
///
/// So this is the seam, and it is deliberately narrow: construct at a bit
/// offset, ask for rows one at a time into a caller-owned buffer, read the
/// bit position back. Rows are packed exactly as [`decode`] packs them,
/// except that here **1 is black** unconditionally — JBIG2's own sense, so
/// its caller inverts at the boundary rather than the decoder guessing.
///
/// Both this and [`decode`] decode a row through the same `decode_row`, so
/// the mode codes have one implementation and one set of tests.
pub struct T6Rows<'a> {
    bits: Bits<'a>,
    reference: Vec<usize>,
    columns: usize,
}

impl<'a> T6Rows<'a> {
    /// Starts at `bit_offset` bits into `data`, decoding rows of `columns`
    /// pixels against an imaginary all-white reference line (T.6 §2.2.1).
    #[must_use]
    pub fn new(data: &'a [u8], bit_offset: usize, columns: u32) -> T6Rows<'a> {
        let columns = columns.clamp(1, 1 << 16) as usize;
        T6Rows {
            bits: Bits {
                data,
                at: bit_offset,
            },
            reference: vec![columns, columns],
            columns,
        }
    }

    /// Bytes one row occupies, which is the least `next_row` will accept.
    #[must_use]
    pub fn row_bytes(&self) -> usize {
        row_bytes(self.columns)
    }

    /// The bit position after everything decoded so far.
    #[must_use]
    pub fn bit_position(&self) -> usize {
        self.bits.at
    }

    /// Decodes the next row into `row`, 1 for black, most significant bit
    /// first.
    ///
    /// False means no further row could be read — the data ran out, or the
    /// bits at the cursor are not a mode code. Nothing is written in that
    /// case and [`Self::bit_position`] still points at the failure, which is
    /// what lets a caller report where a region went wrong rather than only
    /// that it did.
    pub fn next_row(&mut self, row: &mut [u8]) -> bool {
        if row.len() < self.row_bytes() || self.bits.exhausted() {
            return false;
        }
        let Some(changes) = decode_row(&mut self.bits, &self.reference, self.columns, true) else {
            return false;
        };
        pack_row(&changes, self.columns, true, row);
        self.reference = changes;
        self.reference.push(self.columns);
        self.reference.push(self.columns);
        true
    }
}

/// Decodes one row, returning the positions where its colour changes.
fn decode_row(
    bits: &mut Bits,
    reference: &[usize],
    columns: usize,
    two_dimensional: bool,
) -> Option<Vec<usize>> {
    let mut changes: Vec<usize> = Vec::new();
    let mut a0: isize = -1;
    let mut white = true;

    while (a0 as usize) < columns || a0 < 0 {
        if bits.exhausted() {
            return (!changes.is_empty()).then_some(changes);
        }

        if two_dimensional {
            // T.6 mode codes, longest first so a prefix cannot shadow one.
            let b1 = next_change(reference, a0, white, columns);
            let b2 = following(reference, b1, columns);

            if bits.peek(4) == 0b0001 {
                // Pass: the run continues past b2.
                bits.skip(4);
                a0 = b2 as isize;
                continue;
            }
            if bits.peek(3) == 0b001 {
                // Horizontal: two explicit runs follow.
                bits.skip(3);
                let first = read_run(bits, white)?;
                let second = read_run(bits, !white)?;
                let start = if a0 < 0 { 0 } else { a0 as usize };
                let one = (start + first as usize).min(columns);
                let two = (one + second as usize).min(columns);
                changes.push(one);
                changes.push(two);
                a0 = two as isize;
                continue;
            }

            // Vertical modes: the change sits within three pixels of b1.
            let delta = if bits.peek(1) == 1 {
                bits.skip(1);
                Some(0i32)
            } else if bits.peek(3) == 0b011 {
                bits.skip(3);
                Some(1)
            } else if bits.peek(3) == 0b010 {
                bits.skip(3);
                Some(-1)
            } else if bits.peek(6) == 0b000011 {
                bits.skip(6);
                Some(2)
            } else if bits.peek(6) == 0b000010 {
                bits.skip(6);
                Some(-2)
            } else if bits.peek(7) == 0b0000011 {
                bits.skip(7);
                Some(3)
            } else if bits.peek(7) == 0b0000010 {
                bits.skip(7);
                Some(-3)
            } else {
                None
            };

            let Some(delta) = delta else {
                // An end-of-line or an unrecognized code ends the row.
                return (!changes.is_empty()).then_some(changes);
            };

            let a1 = (b1 as i64 + i64::from(delta)).clamp(0, columns as i64) as usize;
            changes.push(a1);
            a0 = a1 as isize;
            white = !white;
        } else {
            let run = read_run(bits, white)?;
            let start = if a0 < 0 { 0 } else { a0 as usize };
            let next = (start + run as usize).min(columns);
            changes.push(next);
            a0 = next as isize;
            white = !white;
            if next >= columns {
                break;
            }
        }
    }

    Some(changes)
}

/// b1: the first change on the reference line right of `a0` with the opposite
/// colour of `a0` (T.4 figure 1).
fn next_change(reference: &[usize], a0: isize, white: bool, columns: usize) -> usize {
    let mut index = 0usize;
    // Changes alternate colour starting from white, so the parity of the
    // index says which colour a change begins.
    while let Some(&position) = reference.get(index) {
        if (position as isize) > a0 && (index % 2 == 0) == white {
            return position.min(columns);
        }
        index += 1;
    }
    columns
}

fn following(reference: &[usize], b1: usize, columns: usize) -> usize {
    reference
        .iter()
        .copied()
        .find(|&position| position > b1)
        .unwrap_or(columns)
        .min(columns)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds a bit string from a pattern of '0' and '1', padded with zeros.
    fn bits_from(pattern: &str) -> Vec<u8> {
        let mut out = Vec::new();
        let mut byte = 0u8;
        let mut count = 0u32;
        for c in pattern.chars().filter(|c| *c == '0' || *c == '1') {
            byte = (byte << 1) | u8::from(c == '1');
            count += 1;
            if count % 8 == 0 {
                out.push(byte);
                byte = 0;
            }
        }
        if count % 8 != 0 {
            out.push(byte << (8 - count % 8));
        }
        out
    }

    /// T.4 §4.1.2: an EOL is eleven zeros and a one. Reading it as data
    /// aborted the entire image, which is what happened to every fax stream
    /// that carried them — and carrying them is the norm for `/K > 0`.
    #[test]
    fn an_end_of_line_code_is_recognised() {
        let plain = bits_from("00000000000 1 0101");
        assert!(Bits::new(&plain).at_eol());

        // Extra zero padding before the one is legal and still an EOL.
        let padded = bits_from("0000000000000000 1");
        assert!(Bits::new(&padded).at_eol());

        // Ten zeros is not enough.
        let short = bits_from("0000000000 1");
        assert!(!Bits::new(&short).at_eol());
    }

    #[test]
    fn skipping_an_end_of_line_lands_after_it() {
        let data = bits_from("00000000000 1 1101");
        let mut reader = Bits::new(&data);
        reader.skip_eol();
        // The first data bit after the EOL is a one.
        assert_eq!(reader.peek(1), 1);
    }

    /// Trailing zeros are padding, not a truncated row. Reading them as data
    /// warned about a file that was perfectly well formed.
    #[test]
    fn trailing_padding_is_not_mistaken_for_data() {
        let data = bits_from("1101 0000000000000000");
        let mut reader = Bits::new(&data);
        reader.skip(4);
        assert!(reader.only_padding_left());
    }

    /// An all-white row, then an EOL, then another: the EOL must not end the
    /// image or corrupt the row after it.
    #[test]
    fn rows_separated_by_end_of_line_codes_both_decode() {
        // 0x35 is the white run-length code for 1728, the standard width.
        let mut data = Vec::new();
        data.extend_from_slice(&bits_from("00110101"));
        data.extend_from_slice(&bits_from("00000000000 1"));
        data.extend_from_slice(&bits_from("00110101"));

        let params = CcittParams {
            k: 0,
            columns: 1728,
            rows: 2,
            ..CcittParams::default()
        };
        let (out, _) = decode(&data, &params, 1 << 20);
        assert_eq!(out.len(), (1728 / 8) * 2, "both rows decoded");
        assert!(out.iter().all(|p| *p == 0xFF), "and both are white");
    }

    /// Packs a sequence of `(length, code)` pairs into bytes.
    fn pack(codes: &[(u32, u32)]) -> Vec<u8> {
        let mut out = Vec::new();
        let mut current = 0u8;
        let mut filled = 0u32;
        for &(length, code) in codes {
            for i in (0..length).rev() {
                let bit = ((code >> i) & 1) as u8;
                current = (current << 1) | bit;
                filled += 1;
                if filled == 8 {
                    out.push(current);
                    current = 0;
                    filled = 0;
                }
            }
        }
        if filled > 0 {
            out.push(current << (8 - filled));
        }
        out
    }

    /// The output shape, stated as an assertion: one bit per pixel, most
    /// significant bit first, 0 for black by default (7.4.6, Table 11).
    ///
    /// Byte-per-pixel output was the old contract, and the caller compensated
    /// for it. Both halves changed in one commit; this is the half that says
    /// what the bytes now mean.
    #[test]
    fn a_one_dimensional_row_decodes_its_runs() {
        // Four white, then four black, in an eight-pixel row.
        let data = pack(&[(4, 0x0B), (4, 0x03)]);
        let params = CcittParams {
            k: 0,
            columns: 8,
            rows: 1,
            ..CcittParams::default()
        };
        let (pixels, _) = decode(&data, &params, 1 << 20);

        assert_eq!(pixels.len(), 1, "eight pixels is one byte, not eight");
        assert_eq!(pixels[0], 0b1111_0000, "white is 1, black is 0, MSB first");
    }

    /// `/BlackIs1` chooses the bit value, and nothing else about the image.
    #[test]
    fn black_is_1_inverts_the_result() {
        let data = pack(&[(4, 0x0B), (4, 0x03)]);
        let params = CcittParams {
            k: 0,
            columns: 8,
            rows: 1,
            black_is_1: true,
            ..CcittParams::default()
        };
        let (pixels, _) = decode(&data, &params, 1 << 20);
        assert_eq!(pixels[0], 0b0000_1111, "inverted");
    }

    /// A row that is not a whole number of bytes still ends on a byte
    /// boundary, and the bits past its width are white rather than whatever
    /// the last run left there.
    #[test]
    fn a_row_is_padded_to_a_byte_boundary() {
        // Ten pixels: two white, then eight black.
        let data = pack(&[(4, 0x07), (6, 0x05)]);
        let params = CcittParams {
            k: 0,
            columns: 10,
            rows: 1,
            ..CcittParams::default()
        };
        let (pixels, _) = decode(&data, &params, 1 << 20);

        assert_eq!(pixels.len(), 2, "ten pixels take two bytes");
        assert_eq!(pixels[0], 0b1100_0000, "two white then black");
        assert_eq!(
            pixels[1], 0b0011_1111,
            "the last two pixels are black and the six padding bits are white"
        );
    }

    /// 7.4.6 Table 11: `/EndOfBlock false` says the data carries no EOFB, so
    /// `/Rows` is the authority on where the image ends. Without that, the
    /// decoder read whatever followed the last row and grew the image past
    /// its declared height -- and because the extra rows decode from real
    /// codes rather than from padding, the result is plausible rather than
    /// obviously wrong.
    #[test]
    fn end_of_block_false_stops_at_rows_with_bytes_to_spare() {
        // Three identical rows of eight black pixels, but /Rows says one.
        let row = [(4, 0x0Bu32), (6, 0x0Du32)];
        let mut codes = Vec::new();
        for _ in 0..3 {
            codes.extend_from_slice(&row);
        }
        let data = pack(&codes);

        let stopping = CcittParams {
            k: 0,
            columns: 8,
            rows: 1,
            end_of_block: false,
            ..CcittParams::default()
        };
        let (pixels, _) = decode(&data, &stopping, 1 << 20);
        assert_eq!(
            pixels.len(),
            1,
            "/Rows is the authority, so the trailing rows are never read"
        );

        // And the trailing bytes really were decodable rows rather than
        // padding the decoder would have stopped on anyway -- otherwise this
        // test would pass against a decoder that simply ran out of input.
        let all_three = CcittParams {
            rows: 3,
            ..stopping
        };
        let (more, _) = decode(&data, &all_three, 1 << 20);
        assert_eq!(more.len(), 3, "the bytes past row one decode to two rows");
    }

    /// Table 11 again: `/EndOfLine true` says the encoding carries an EOL
    /// before every line. Honouring one wherever it appears is right and
    /// already worked; what was missing is noticing that a *required* one is
    /// absent. Ruling 10 wants the tolerance named -- a stream whose EOLs
    /// went missing has row boundaries only as good as its run lengths.
    #[test]
    fn a_missing_required_end_of_line_is_reported_rather_than_absorbed() {
        let data = pack(&[(4, 0x0B), (6, 0x0D)]);
        let params = CcittParams {
            k: 0,
            columns: 8,
            rows: 1,
            end_of_line: true,
            ..CcittParams::default()
        };
        let (pixels, warnings) = decode(&data, &params, 1 << 20);

        assert_eq!(pixels.len(), 1, "the row is still decoded, leniently");
        assert!(
            warnings.contains(&Warning::MissingEndOfLine),
            "and the absent EOL is named: {warnings:?}"
        );

        // The same bytes without the flag are simply a well-formed stream,
        // so the warning tracks the declaration rather than the data.
        let quiet = CcittParams {
            end_of_line: false,
            ..params
        };
        let (_, warnings) = decode(&data, &quiet, 1 << 20);
        assert!(
            !warnings.contains(&Warning::MissingEndOfLine),
            "nothing is missing when nothing was promised: {warnings:?}"
        );
    }

    /// T.4 §4.2.1.3.1: in mixed mode the EOL carries one more bit — 1 for a
    /// one-dimensionally coded line, 0 for a two-dimensionally coded one.
    ///
    /// This is the shape every mixed-mode encoder emits, and it decoded
    /// correctly before this commit too: with an EOL before every row,
    /// "the bit after the EOL" and "the bit at the top of the row" are the
    /// same bit. So this test is the guard rather than the evidence — a fix
    /// that only moved the tag read into the EOL branch would pass it, and
    /// the two tests below are the ones that say whether the fix is whole.
    #[test]
    fn mixed_mode_rows_are_coded_the_way_their_tag_bit_says() {
        let data = bits_from(concat!(
            // Row 0, tagged one-dimensional: white 4 (1011), black 4 (011).
            "000000000001 1 1011 011 ",
            // Row 1, tagged two-dimensional: two V(0)s repeat row 0 exactly.
            "000000000001 0 1 1 ",
            // Row 2, tagged two-dimensional: VR(1) moves the edge one pixel
            // right, then V(0) for the end of the row.
            "000000000001 0 011 1 ",
            // Row 3, tagged one-dimensional again: white 8 (10011).
            "000000000001 1 10011",
        ));

        let params = CcittParams {
            k: 1,
            columns: 8,
            rows: 4,
            end_of_line: true,
            ..CcittParams::default()
        };
        let (pixels, warnings) = decode(&data, &params, 1 << 20);

        assert_eq!(
            pixels,
            vec![0b1111_0000, 0b1111_0000, 0b1111_1000, 0b1111_1111],
            "each row decodes the way its own tag bit said it was coded"
        );
        assert!(
            warnings.is_empty(),
            "every line has the EOL it promised: {warnings:?}"
        );
    }

    /// The tag bit is part of the EOL, so a row that arrives without an EOL
    /// has no tag bit to read.
    ///
    /// Reading one anyway spends the first bit of that row's first code, and
    /// the row — and every row after it, since the reference line is now
    /// wrong too — decodes to noise. `/EndOfLine false` is the default, so
    /// this is not an exotic stream.
    #[test]
    fn a_mixed_mode_row_without_an_end_of_line_keeps_its_first_data_bit() {
        // One EOL at the head and none after it, which is what an encoder
        // that opens with a sync and then stops emitting them produces.
        let data = bits_from(concat!(
            "000000000001 1 1011 011 ", // row 0: white 4, black 4
            "0111 0010 ",               // row 1: white 2, black 6
            "1110 11 ",                 // row 2: white 6, black 2
            "10011",                    // row 3: white 8
        ));

        let params = CcittParams {
            k: 1,
            columns: 8,
            rows: 4,
            ..CcittParams::default()
        };
        let (pixels, warnings) = decode(&data, &params, 1 << 20);

        assert_eq!(
            pixels,
            vec![0b1111_0000, 0b1100_0000, 0b1111_1100, 0b1111_1111],
            "the rows after the first are their own runs, not noise"
        );
        assert!(warnings.is_empty(), "and nothing was damaged: {warnings:?}");

        // The same four rows with no EOL anywhere, which loses the *first*
        // row as well. T.4 §4.2.1.3.1 says a page begins one-dimensionally,
        // and with nothing to announce otherwise it stays that way.
        let bare = bits_from("1011 011 0111 0010 1110 11 10011");
        let (pixels, warnings) = decode(&bare, &params, 1 << 20);
        assert_eq!(
            pixels,
            vec![0b1111_0000, 0b1100_0000, 0b1111_1100, 0b1111_1111],
            "a mixed-mode stream with no EOL at all is one-dimensional"
        );
        assert!(warnings.is_empty(), "{warnings:?}");
    }

    /// What a row without an EOL is coded as, given there is no tag bit to
    /// tell it: the mode last announced.
    ///
    /// This is the half a fix could plausibly leave out. Defaulting an
    /// untagged row to one-dimensional passes the test above — every row
    /// there is one-dimensional — and turns a two-dimensionally coded
    /// continuation into run lengths read from mode codes, which is noise
    /// again, from a different offset.
    #[test]
    fn a_two_dimensional_announcement_outlives_a_row_without_an_end_of_line() {
        let data = bits_from(concat!(
            "000000000001 1 1011 011 ", // row 0: EOL, tagged 1D
            "000000000001 0 1 1 ",      // row 1: EOL, tagged 2D, two V(0)s
            "011 1 ",                   // row 2: no EOL — still 2D: VR(1), V(0)
            "1 1",                      // row 3: no EOL — still 2D: two V(0)s
        ));

        let params = CcittParams {
            k: 1,
            columns: 8,
            rows: 4,
            ..CcittParams::default()
        };
        let (pixels, warnings) = decode(&data, &params, 1 << 20);

        assert_eq!(
            pixels,
            vec![0b1111_0000, 0b1111_0000, 0b1111_1000, 0b1111_1000],
            "rows 2 and 3 are two-dimensional because row 1's EOL said so"
        );
        assert!(warnings.is_empty(), "{warnings:?}");
    }

    /// RTC (T.4 §4.1.2) is six EOL-and-tag pairs, so in mixed mode the second
    /// EOL only comes into view once the first tag bit is out of the way.
    /// Looking for it before that read the terminator as a row, failed on it
    /// and called a perfectly well-formed stream damaged.
    #[test]
    fn a_mixed_mode_return_to_control_ends_the_image_without_complaint() {
        let mut pattern = String::from(concat!(
            "000000000001 1 1011 011 ", // row 0: white 4, black 4
            "000000000001 1 10011 ",    // row 1: white 8
        ));
        for _ in 0..6 {
            pattern.push_str("000000000001 1 ");
        }
        let data = bits_from(&pattern);

        let params = CcittParams {
            k: 1,
            columns: 8,
            // Nothing declares a height, so only the RTC says where to stop.
            rows: 0,
            ..CcittParams::default()
        };
        let (pixels, warnings) = decode(&data, &params, 1 << 20);

        assert_eq!(pixels, vec![0b1111_0000, 0b1111_1111], "two rows, then RTC");
        assert!(
            warnings.is_empty(),
            "a terminator is not a damaged row: {warnings:?}"
        );
    }

    #[test]
    fn a_short_image_is_padded_and_reported() {
        let data = pack(&[(4, 0x0B), (4, 0x03)]);
        let params = CcittParams {
            k: 0,
            columns: 8,
            rows: 4,
            ..CcittParams::default()
        };
        let (pixels, warnings) = decode(&data, &params, 1 << 20);

        assert_eq!(pixels.len(), 4, "every declared row is present");
        assert_eq!(
            &pixels[1..],
            &[0xFF, 0xFF, 0xFF],
            "and the padding is white"
        );
        assert!(warnings.contains(&Warning::TruncatedInput));
        assert_eq!(
            warnings
                .iter()
                .filter(|w| **w == Warning::TruncatedInput)
                .count(),
            1,
            "one entry per condition per decode, however many rows were short"
        );
    }

    #[test]
    fn the_output_cap_is_honoured() {
        let data = pack(&[(4, 0x0B), (4, 0x03)]);
        let params = CcittParams {
            k: 0,
            columns: 64,
            rows: 0,
            ..CcittParams::default()
        };
        let (pixels, warnings) = decode(&data, &params, 4);
        assert!(pixels.len() <= 4, "the cap bounds the output");
        assert!(warnings.contains(&Warning::OutputCapHit));
    }

    #[test]
    fn run_lengths_read_from_their_tables() {
        // A white run of 2 is four bits, 0x07.
        let data = pack(&[(4, 0x07)]);
        let mut bits = Bits::new(&data);
        assert_eq!(read_run(&mut bits, true), Some(2));

        // A black run of 2 is two bits, 0x03.
        let data = pack(&[(2, 0x03)]);
        let mut bits = Bits::new(&data);
        assert_eq!(read_run(&mut bits, false), Some(2));

        // A make-up of 64 white followed by a terminating 0 gives 64.
        let data = pack(&[(5, 0x1B), (8, 0x35)]);
        let mut bits = Bits::new(&data);
        assert_eq!(read_run(&mut bits, true), Some(64));
    }

    #[test]
    fn arbitrary_bytes_terminate_without_panicking() {
        for len in 0..256usize {
            let data: Vec<u8> = (0..len).map(|i| ((i * 37) % 256) as u8).collect();
            for k in [-1i32, 0, 1] {
                let params = CcittParams {
                    k,
                    columns: 16,
                    rows: 8,
                    ..CcittParams::default()
                };
                let _ = decode(&data, &params, 1 << 16);
            }
        }
    }

    /// The seam gap 17's MMR path decodes through: rows one at a time, from
    /// a bit offset that
    /// is not a byte offset, with the position readable afterwards.
    #[test]
    fn two_dimensional_rows_resume_from_a_bit_offset() {
        // Three bits of something else, then the two G4 rows of the
        // end-to-end fixture: horizontal white 0 / black 4 then V(0), and
        // three V(0)s repeating it.
        let data = bits_from("101 001 00110101 011 1 111");
        let mut rows = T6Rows::new(&data, 3, 8);
        assert_eq!(rows.row_bytes(), 1);

        let mut row = [0u8; 1];
        assert!(rows.next_row(&mut row), "the first row decodes");
        assert_eq!(
            row[0], 0b1111_0000,
            "four black, then four white — 1 is black"
        );
        let after_first = rows.bit_position();
        assert_eq!(after_first, 3 + 15, "and it consumed exactly its own codes");

        assert!(rows.next_row(&mut row), "the second row decodes against it");
        assert_eq!(row[0], 0b1111_0000, "identically");
        assert_eq!(rows.bit_position(), after_first + 3, "three V(0) codes");

        assert!(!rows.next_row(&mut row), "and then the data is spent");
    }

    /// A buffer too small for a row is refused rather than half-filled.
    #[test]
    fn a_short_row_buffer_is_refused() {
        let data = bits_from("001 00110101 011 1");
        let mut rows = T6Rows::new(&data, 0, 24);
        let mut row = [0u8; 2];
        assert!(!rows.next_row(&mut row), "three bytes are needed");
        assert_eq!(rows.bit_position(), 0, "and nothing was consumed");
    }

    #[test]
    fn a_degenerate_column_count_does_not_divide_by_zero() {
        let params = CcittParams {
            columns: 0,
            rows: 1,
            ..CcittParams::default()
        };
        let (pixels, _) = decode(&[0xFF, 0x00], &params, 1 << 16);
        assert!(pixels.len() <= 1);
    }

    // ---- The encoder (T.6 §2.2) ---------------------------------------

    /// **ITU-T T.4 Table 2, transcribed from the Recommendation and not from
    /// `ccitt.rs`.** The terminating codes for white runs of 0 to 63.
    ///
    /// The Recommendation was fetched from the ITU in September 2026 and every
    /// entry below was read **twice**: once out of the PDF's text layer, once
    /// off the rendered page at 170 dpi. The two readings agree on all 195
    /// entries of the five tables here, which is the check the tables needed —
    /// one measurement in this repository found 37 wrong entries out of 182 in
    /// tables drafted in a single pass.
    ///
    /// These exist as bit *strings* rather than as `(length, code)` pairs on
    /// purpose. The pair form is [`WHITE_TERM`]'s own, and a transcription in
    /// the same shape invites copying rather than reading: a leading zero is
    /// invisible in `0x07` and decisive in `000111`.
    #[rustfmt::skip]
    const T4_WHITE_TERM: [&str; 64] = [
        "00110101", "000111", "0111", "1000", "1011", "1100", "1110", "1111", "10011", "10100",
        "00111", "01000", "001000", "000011", "110100", "110101", "101010", "101011", "0100111",
        "0001100", "0001000", "0010111", "0000011", "0000100", "0101000", "0101011", "0010011",
        "0100100", "0011000", "00000010", "00000011", "00011010", "00011011", "00010010",
        "00010011", "00010100", "00010101", "00010110", "00010111", "00101000", "00101001",
        "00101010", "00101011", "00101100", "00101101", "00000100", "00000101", "00001010",
        "00001011", "01010010", "01010011", "01010100", "01010101", "00100100", "00100101",
        "01011000", "01011001", "01011010", "01011011", "01001010", "01001011", "00110010",
        "00110011", "00110100",
    ];

    /// ITU-T T.4 Table 2, the black column.
    #[rustfmt::skip]
    const T4_BLACK_TERM: [&str; 64] = [
        "0000110111", "010", "11", "10", "011", "0011", "0010", "00011", "000101", "000100",
        "0000100", "0000101", "0000111", "00000100", "00000111", "000011000", "0000010111",
        "0000011000", "0000001000", "00001100111", "00001101000", "00001101100", "00000110111",
        "00000101000", "00000010111", "00000011000", "000011001010", "000011001011",
        "000011001100", "000011001101", "000001101000", "000001101001", "000001101010",
        "000001101011", "000011010010", "000011010011", "000011010100", "000011010101",
        "000011010110", "000011010111", "000001101100", "000001101101", "000011011010",
        "000011011011", "000001010100", "000001010101", "000001010110", "000001010111",
        "000001100100", "000001100101", "000001010010", "000001010011", "000000100100",
        "000000110111", "000000111000", "000000100111", "000000101000", "000001011000",
        "000001011001", "000000101011", "000000101100", "000001011010", "000001100110",
        "000001100111",
    ];

    /// ITU-T T.4 Table 3a, the white column: make-up codes for 64 to 1 728.
    #[rustfmt::skip]
    const T4_WHITE_MAKEUP: [&str; 27] = [
        "11011", "10010", "010111", "0110111", "00110110", "00110111", "01100100", "01100101",
        "01101000", "01100111", "011001100", "011001101", "011010010", "011010011", "011010100",
        "011010101", "011010110", "011010111", "011011000", "011011001", "011011010", "011011011",
        "010011000", "010011001", "010011010", "011000", "010011011",
    ];

    /// ITU-T T.4 Table 3a, the black column.
    #[rustfmt::skip]
    const T4_BLACK_MAKEUP: [&str; 27] = [
        "0000001111", "000011001000", "000011001001", "000001011011", "000000110011",
        "000000110100", "000000110101", "0000001101100", "0000001101101", "0000001001010",
        "0000001001011", "0000001001100", "0000001001101", "0000001110010", "0000001110011",
        "0000001110100", "0000001110101", "0000001110110", "0000001110111", "0000001010010",
        "0000001010011", "0000001010100", "0000001010101", "0000001011010", "0000001011011",
        "0000001100100", "0000001100101",
    ];

    /// ITU-T T.4 Table 3b: make-up codes for 1 792 to 2 560, shared by both
    /// colours.
    #[rustfmt::skip]
    const T4_EXT_MAKEUP: [&str; 13] = [
        "00000001000", "00000001100", "00000001101", "000000010010", "000000010011",
        "000000010100", "000000010101", "000000010110", "000000010111", "000000011100",
        "000000011101", "000000011110", "000000011111",
    ];

    /// The bit string a `(length, code, run)` row of one of this module's
    /// tables spells, so the two forms can be compared at all.
    fn spelled(entry: RunCode) -> String {
        let (length, code, _) = entry;
        (0..length)
            .rev()
            .map(|index| if (code >> index) & 1 == 1 { '1' } else { '0' })
            .collect()
    }

    /// **The tables both directions share are ITU-T T.4's own.**
    ///
    /// This is the test that makes sharing them defensible. `encode.rs` emits
    /// from the same five arrays [`read_run`] reads, which is the right
    /// engineering — the two directions must agree entry for entry or nothing
    /// round-trips, and a second copy is a second thing to get wrong — but it
    /// means a wrong entry cancels between them and every round trip in this
    /// file still passes. Nothing in the tree notices except this, which
    /// compares against a transcription of the Recommendation rather than
    /// against either implementation.
    ///
    /// The run *lengths* are asserted too, and not only the code words: a
    /// table whose rows are right but shifted by one is the defect a spot
    /// check misses.
    #[test]
    fn the_run_tables_are_itu_t_t_4_s_own() {
        for (run, expected) in T4_WHITE_TERM.iter().enumerate() {
            assert_eq!(
                &spelled(WHITE_TERM[run]),
                expected,
                "white terminating {run}"
            );
            assert_eq!(
                WHITE_TERM[run].2 as usize, run,
                "white terminating row {run}"
            );
        }
        for (run, expected) in T4_BLACK_TERM.iter().enumerate() {
            assert_eq!(
                &spelled(BLACK_TERM[run]),
                expected,
                "black terminating {run}"
            );
            assert_eq!(
                BLACK_TERM[run].2 as usize, run,
                "black terminating row {run}"
            );
        }
        for (index, expected) in T4_WHITE_MAKEUP.iter().enumerate() {
            assert_eq!(
                &spelled(WHITE_MAKEUP[index]),
                expected,
                "white make-up {index}"
            );
            assert_eq!(WHITE_MAKEUP[index].2 as usize, (index + 1) * 64);
        }
        for (index, expected) in T4_BLACK_MAKEUP.iter().enumerate() {
            assert_eq!(
                &spelled(BLACK_MAKEUP[index]),
                expected,
                "black make-up {index}"
            );
            assert_eq!(BLACK_MAKEUP[index].2 as usize, (index + 1) * 64);
        }
        for (index, expected) in T4_EXT_MAKEUP.iter().enumerate() {
            assert_eq!(
                &spelled(EXT_MAKEUP[index]),
                expected,
                "extended make-up {index}"
            );
            assert_eq!(EXT_MAKEUP[index].2 as usize, 1792 + index * 64);
        }
    }

    /// Packs `pattern` — '0' and '1', anything else ignored — as one row of
    /// pixels, 1 for black, and codes it.
    fn encode_rows(rows: &[&str], end_of_block: bool) -> Vec<u8> {
        let columns = rows[0].len();
        let stride = row_bytes(columns);
        let mut data = vec![0u8; stride * rows.len()];
        for (y, row) in rows.iter().enumerate() {
            for (x, cell) in row.chars().enumerate() {
                if cell == '#' {
                    data[y * stride + (x >> 3)] |= 0x80 >> (x & 7);
                }
            }
        }
        g4_encode(&CcittSource {
            columns: columns as u32,
            rows: rows.len() as u32,
            black_is_1: true,
            stride,
            end_of_block,
            data: &data,
        })
        .expect("a well-formed raster")
    }

    /// One row of `columns` pixels: `white` white pixels, then black to the
    /// end, coded with no end-of-block.
    fn encode_one_run(columns: usize, white: usize) -> Vec<u8> {
        let row: String = (0..columns)
            .map(|x| if x < white { '.' } else { '#' })
            .collect();
        encode_rows(&[&row], false)
    }

    /// The bits `codes` spell, packed most significant bit first and padded
    /// with zeros — what [`g4_encode`] should have produced.
    fn expect_bits(codes: &[&str]) -> Vec<u8> {
        bits_from(&codes.concat())
    }

    /// **T.6 §2.2.4 step 2 iii), against T.4's tables directly.**
    ///
    /// A run longer than 63 is a make-up code plus a terminating code, and the
    /// make-up is the one "nearest, not longer" — three separate rules, each of
    /// which has a plausible wrong version that a round trip cannot see,
    /// because this crate's decoder sums make-ups and terminating codes in
    /// whatever order they arrive.
    ///
    /// Every expected code word below comes from [`T4_WHITE_TERM`] and its
    /// neighbours, which are the Recommendation's; nothing here reads the
    /// tables the encoder emits from.
    ///
    /// The four cases are the four ranges the clause distinguishes:
    /// - 64 exactly: a make-up and a **terminating code for zero**, which is
    ///   the one an encoder is most likely to drop, because dropping it
    ///   changes nothing about runs that are not multiples of 64.
    /// - 1 000: a make-up out of Table 3a and a remainder.
    /// - 2 000: a make-up out of Table **3b**, which a version that only knew
    ///   its own colour's table would answer with 1 728 and a remainder of 272
    ///   — a run length no terminating code can express.
    /// - 3 000: past 2 623, so Table 3b's note applies and the code opens with
    ///   a 2 560.
    #[test]
    fn a_run_past_sixty_three_is_a_make_up_code_and_a_terminating_code() {
        // Sixty-four white then black to 200. The black run is 136, which is a
        // make-up of 128 and a terminating 8.
        assert_eq!(
            encode_one_run(200, 64),
            expect_bits(&[
                "001",
                T4_WHITE_MAKEUP[0],
                T4_WHITE_TERM[0],
                T4_BLACK_MAKEUP[1],
                T4_BLACK_TERM[8],
            ]),
            "a run of exactly 64 owes a terminating code for a run of zero"
        );

        // 1 000 white then 1 000 black in a 2 000 pixel row. 960 is the
        // largest make-up not longer than 1 000.
        assert_eq!(
            encode_one_run(2000, 1000),
            expect_bits(&[
                "001",
                T4_WHITE_MAKEUP[14],
                T4_WHITE_TERM[40],
                T4_BLACK_MAKEUP[14],
                T4_BLACK_TERM[40],
            ]),
        );

        // 2 000 white then 2 000 black in a 4 000 pixel row. 1 984 is in
        // Table 3b and 1 728 is the last row of Table 3a.
        assert_eq!(
            encode_one_run(4000, 2000),
            expect_bits(&[
                "001",
                T4_EXT_MAKEUP[3],
                T4_WHITE_TERM[16],
                T4_EXT_MAKEUP[3],
                T4_BLACK_TERM[16],
            ]),
            "1 984 is nearer to 2 000 than 1 728 and Table 3b is shared"
        );

        // 3 000 white then 2 000 black in a 5 000 pixel row. 3 000 is past
        // 2 623, so it opens with 2 560 and codes the remaining 440 by the
        // ordinary rule: a make-up of 384 and a terminating 56.
        assert_eq!(
            encode_one_run(5000, 3000),
            expect_bits(&[
                "001",
                T4_EXT_MAKEUP[12],
                T4_WHITE_MAKEUP[5],
                T4_WHITE_TERM[56],
                T4_EXT_MAKEUP[3],
                T4_BLACK_TERM[16],
            ]),
        );
    }

    /// Runs of every length up to a full 65 536-pixel row survive the decoder.
    ///
    /// This one *is* a self round trip and says so: it proves the encoder and
    /// this decoder agree, which is worth having because the composition rule
    /// for a long run has more arithmetic in it than any code word does, and
    /// worth nothing at all about T.4 — which is what
    /// [`Self::a_run_past_sixty_three_is_a_make_up_code_and_a_terminating_code`]
    /// is for.
    #[test]
    fn long_runs_survive_the_round_trip() {
        for white in [
            0, 1, 63, 64, 65, 127, 1727, 1728, 1729, 2559, 2560, 2623, 2624, 5121,
        ] {
            let columns = 65536usize;
            let coded = encode_one_run(columns, white);
            let params = CcittParams {
                k: -1,
                columns: columns as u32,
                rows: 1,
                black_is_1: true,
                ..CcittParams::default()
            };
            let (pixels, warnings) = decode(&coded, &params, 1 << 20);
            assert!(warnings.is_empty(), "{white}: {warnings:?}");
            let expected: Vec<usize> = if white == 0 { vec![0] } else { vec![white] };
            let stride = row_bytes(columns);
            let mut want = vec![0u8; stride];
            pack_row(&expected, columns, true, &mut want);
            assert_eq!(pixels, want, "a white run of {white}");
        }
    }

    /// **The picture comes back**, through every mode T.6 §2.2.3 has.
    ///
    /// A self round trip, and its whole value is breadth rather than depth:
    /// the pattern below reaches pass mode, horizontal mode and all seven
    /// vertical codes, which the two published fixtures between them do not.
    /// What adjudicates the coding is
    /// `jbig2::tests::annex_h_mmr_region_re_encodes_to_the_published_bytes`.
    #[test]
    fn every_two_dimensional_mode_round_trips() {
        #[rustfmt::skip]
        let rows = [
            "................................",
            "###############.................",
            "..###############...............",
            "....############................",
            "#..#..#..#..#..#..#..#..#..#..#.",
            ".#..#..#..#..#..#..#..#..#..#..#",
            "################################",
            "................................",
            "#..............................#",
            "###.........................####",
            "..#############################.",
            "###############################.",
        ];
        let coded = encode_rows(&rows, true);
        let params = CcittParams {
            k: -1,
            columns: 32,
            rows: rows.len() as u32,
            black_is_1: true,
            ..CcittParams::default()
        };
        let (pixels, warnings) = decode(&coded, &params, 1 << 20);
        assert!(warnings.is_empty(), "{warnings:?}");

        let picture: Vec<String> = pixels
            .chunks(4)
            .map(|row| {
                (0..32)
                    .map(|x| {
                        if (row[x >> 3] >> (7 - (x & 7))) & 1 == 1 {
                            '#'
                        } else {
                            '.'
                        }
                    })
                    .collect()
            })
            .collect();
        assert_eq!(picture, rows);
    }

    /// `/EndOfBlock` writes T.6 §2.4.1.1's twenty-four bits and nothing else
    /// changes.
    ///
    /// The two encodings are byte-identical up to the terminator, which is the
    /// half that would be missed by comparing only the decoded pictures: an
    /// EOFB written *instead of* the last row's codes rather than after them
    /// decodes to the same image with `/Rows` set.
    #[test]
    fn the_end_of_block_is_two_end_of_line_codes_after_the_last_row() {
        let rows = ["##..####", "..######"];
        let plain = encode_rows(&rows, false);
        let terminated = encode_rows(&rows, true);

        assert!(terminated.starts_with(&plain[..plain.len() - 1]));
        // The rows occupy 22 bits here, so the EOFB starts mid-byte and the
        // comparison has to be on bits rather than on bytes.
        let mut expected = bits_from(&format!(
            "{}{}",
            (0..plain.len() * 8)
                .map(|at| {
                    let bit = (plain[at / 8] >> (7 - (at % 8))) & 1;
                    if bit == 1 {
                        '1'
                    } else {
                        '0'
                    }
                })
                .collect::<String>()
                .trim_end_matches('0'),
            "000000000001000000000001"
        ));
        expected.truncate(terminated.len());
        assert_eq!(terminated, expected);
    }

    /// `/BlackIs1` chooses which bit value the encoder reads as black, and
    /// nothing else — the same two pictures, coded identically.
    #[test]
    fn black_is_1_chooses_the_polarity_the_encoder_reads() {
        let rows = ["##....##", "..####.."];
        let stride = 1usize;
        let mut ones = vec![0u8; 2];
        for (y, row) in rows.iter().enumerate() {
            for (x, cell) in row.chars().enumerate() {
                if cell == '#' {
                    ones[y * stride + (x >> 3)] |= 0x80 >> (x & 7);
                }
            }
        }
        let zeros: Vec<u8> = ones.iter().map(|byte| !byte).collect();

        let as_ones = g4_encode(&CcittSource {
            columns: 8,
            rows: 2,
            black_is_1: true,
            stride,
            end_of_block: true,
            data: &ones,
        })
        .expect("well formed");
        let as_zeros = g4_encode(&CcittSource {
            columns: 8,
            rows: 2,
            black_is_1: false,
            stride,
            end_of_block: true,
            data: &zeros,
        })
        .expect("well formed");
        assert_eq!(as_ones, as_zeros);
    }

    /// A stride wider than a row is padding and is not read as pixels.
    ///
    /// `png/encode.rs` records why this test has to be built rather than
    /// found: for every unpadded buffer the stride equals the row length, so a
    /// version that read the raster as one contiguous run produces identical
    /// output and nothing in the engine would notice.
    #[test]
    fn a_padded_stride_is_not_read_as_pixels() {
        let packed = [0b1010_0000u8, 0b0101_0000];
        let padded = [0b1010_0000u8, 0xFF, 0xFF, 0b0101_0000, 0xFF, 0xFF];
        let tight = g4_encode(&CcittSource {
            columns: 4,
            rows: 2,
            black_is_1: true,
            stride: 1,
            end_of_block: true,
            data: &packed,
        })
        .expect("well formed");
        let loose = g4_encode(&CcittSource {
            columns: 4,
            rows: 2,
            black_is_1: true,
            stride: 3,
            end_of_block: true,
            data: &padded,
        })
        .expect("well formed");
        assert_eq!(tight, loose, "the three padding bytes a row are not pixels");
    }

    /// The three refusals, each on the buffer that earns it.
    #[test]
    fn a_raster_that_is_not_an_image_is_refused_rather_than_coded() {
        let data = [0u8; 64];
        let base = CcittSource {
            columns: 8,
            rows: 2,
            black_is_1: true,
            stride: 1,
            end_of_block: true,
            data: &data,
        };
        assert_eq!(
            g4_encode(&CcittSource { columns: 0, ..base }),
            Err(CcittEncodeError::BadDimensions {
                columns: 0,
                rows: 2
            })
        );
        assert_eq!(
            g4_encode(&CcittSource { rows: 0, ..base }),
            Err(CcittEncodeError::BadDimensions {
                columns: 8,
                rows: 0
            })
        );
        assert_eq!(
            g4_encode(&CcittSource {
                columns: (1 << 16) + 1,
                ..base
            }),
            Err(CcittEncodeError::BadDimensions {
                columns: (1 << 16) + 1,
                rows: 2
            }),
            "past what `decode` will clamp `/Columns` to, so it could not read it back"
        );
        assert_eq!(
            g4_encode(&CcittSource {
                columns: 32,
                stride: 3,
                ..base
            }),
            Err(CcittEncodeError::ShortStride {
                stride: 3,
                row_bytes: 4
            })
        );
        let short = [0u8; 1];
        assert_eq!(
            g4_encode(&CcittSource {
                data: &short,
                ..base
            }),
            Err(CcittEncodeError::ShortData { have: 1, need: 2 })
        );
        // And a buffer that stops exactly at the last pixel is enough.
        let exact = [0u8; 2];
        assert!(g4_encode(&CcittSource {
            data: &exact,
            ..base
        })
        .is_ok());
    }
}
