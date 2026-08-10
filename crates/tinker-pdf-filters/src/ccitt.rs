//! CCITT Group 3 and Group 4 fax decoding (7.4.6; ITU-T T.4, T.6).
//!
//! Scanned documents are mostly this. A row is coded as runs of white and
//! black, either by their lengths against a Huffman table (one-dimensional,
//! T.4) or by where their changes sit relative to the row above
//! (two-dimensional, T.6) — which is what makes a fax of mostly-blank paper
//! so small.
//!
//! Output is one byte per pixel, 0 for black and 255 for white, which the
//! caller maps into whatever colour space the image declares.

use crate::Warning;

/// How the data is coded (7.4.6, Table 11).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CcittParams {
    /// `/K`: negative is pure two-dimensional (G4), zero pure
    /// one-dimensional (G3), positive mixed.
    pub k: i32,
    /// `/Columns`, the pixels per row.
    pub columns: u32,
    /// `/Rows`; zero means "until the data runs out".
    pub rows: u32,
    /// `/BlackIs1`: whether a 1 bit means black.
    pub black_is_1: bool,
    /// `/EncodedByteAlign`: whether each row starts on a byte boundary.
    pub byte_align: bool,
}

impl Default for CcittParams {
    fn default() -> Self {
        CcittParams {
            k: 0,
            columns: 1728,
            rows: 0,
            black_is_1: false,
            byte_align: false,
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

/// Decodes CCITT data into one byte per pixel.
#[must_use]
pub fn decode(data: &[u8], params: &CcittParams, max_output: usize) -> (Vec<u8>, Vec<Warning>) {
    let mut warnings = Vec::new();
    let columns = params.columns.clamp(1, 1 << 16) as usize;
    let mut out: Vec<u8> = Vec::new();
    let mut bits = Bits::new(data);

    // The row above, as the positions where colour changes. A first row is
    // decoded against an imaginary all-white one.
    let mut reference: Vec<usize> = vec![columns, columns];
    let mut rows_done = 0u32;

    while !bits.exhausted() {
        if params.rows > 0 && rows_done >= params.rows {
            break;
        }
        if out.len().saturating_add(columns) > max_output {
            warnings.push(Warning::OutputCapHit);
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
        }
        if bits.exhausted() || bits.only_padding_left() {
            break;
        }

        // 7.4.6: /K decides the mode. A positive K means each row announces
        // its own mode with a bit after the EOL, which this reads when the
        // data provides it and otherwise assumes one-dimensional.
        let two_dimensional = if params.k < 0 {
            true
        } else if params.k > 0 {
            bits.bit() == 0
        } else {
            false
        };

        let Some(changes) = decode_row(&mut bits, &reference, columns, two_dimensional) else {
            // A row that will not decode is one row, not the rest of the page.
            // Repeating the row above is what every fax decoder does with a
            // damaged line: it keeps the image legible and localises the loss.
            warnings.push(Warning::TruncatedInput);
            if rows_done > 0 && out.len() >= columns && params.rows > rows_done {
                let previous: Vec<u8> = out[out.len() - columns..].to_vec();
                out.extend_from_slice(&previous);
                rows_done += 1;
                // Nothing else can be decoded from a broken bit position.
            }
            break;
        };

        // Turn the change positions into pixels: a row starts white and
        // alternates at each change.
        let mut row = vec![255u8; columns];
        let mut color_white = true;
        let mut at = 0usize;
        for &change in &changes {
            let change = change.min(columns);
            if !color_white {
                for pixel in row.get_mut(at..change).unwrap_or_default() {
                    *pixel = 0;
                }
            }
            at = change;
            color_white = !color_white;
        }
        if !color_white {
            for pixel in row.get_mut(at..).unwrap_or_default() {
                *pixel = 0;
            }
        }

        if params.black_is_1 {
            // The samples are inverted relative to the default reading.
            for pixel in &mut row {
                *pixel = 255 - *pixel;
            }
        }

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
        warnings.push(Warning::TruncatedInput);
        // A short image is padded white rather than left ragged, so the
        // caller's row arithmetic still works.
        let missing = (params.rows - rows_done) as usize * columns;
        if out.len() + missing <= max_output {
            out.resize(out.len() + missing, if params.black_is_1 { 0 } else { 255 });
        }
    }

    (out, warnings)
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
            black_is_1: false,
            byte_align: false,
        };
        let (out, _) = decode(&data, &params, 1 << 20);
        assert_eq!(out.len(), 1728 * 2, "both rows decoded");
        assert!(out.iter().all(|p| *p == 255), "and both are white");
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

        assert_eq!(pixels.len(), 8);
        assert_eq!(&pixels[..4], &[255, 255, 255, 255], "white first");
        assert_eq!(&pixels[4..], &[0, 0, 0, 0], "then black");
    }

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
        assert_eq!(&pixels[..4], &[0, 0, 0, 0], "inverted");
        assert_eq!(&pixels[4..], &[255, 255, 255, 255]);
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

        assert_eq!(pixels.len(), 32, "every declared row is present");
        assert!(warnings.contains(&Warning::TruncatedInput));
    }

    #[test]
    fn the_output_cap_is_honoured() {
        let data = pack(&[(4, 0x0B), (4, 0x03)]);
        let params = CcittParams {
            k: 0,
            columns: 8,
            rows: 0,
            ..CcittParams::default()
        };
        let (pixels, _) = decode(&data, &params, 4);
        assert!(pixels.len() <= 8, "the cap bounds the output");
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
}
