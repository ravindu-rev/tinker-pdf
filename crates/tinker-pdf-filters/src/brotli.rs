//! Brotli (RFC 7932), decompression only.
//!
//! Feature documentation: `docs/features/filters.md`.
//!
//! This is here rather than in `tinker-pdf-font` for the reason `inflate_raw`
//! and `crc32` are here: it is a decompressor, and a container that needs one
//! reaches *down* for it. WOFF2 is the caller — a whole-file Brotli stream
//! wrapped around a transformed sfnt — and the `font → filters` edge that
//! carries the CMap tables already exists, so nothing in the crate graph
//! moves. Putting it in the font crate would have given that crate a second
//! compression implementation for no reason.
//!
//! # What a Brotli stream is, in one paragraph
//!
//! A stream is a window size and then meta-blocks (§9). Each meta-block
//! carries its own prefix codes and decodes to a run of commands, each an
//! `<insert length, copy length>` pair: some literal bytes, then a copy from
//! earlier output. Three things make it not-DEFLATE. The prefix code used for
//! a literal depends on a **context** — the previous two output bytes, mapped
//! through one of four context modes (§7.1) and a per-meta-block context map;
//! the code used for anything can change mid-meta-block through
//! **block-switch commands** (§6); and a copy whose distance reaches past the
//! start of the output is not an error but a reference into a **122 KiB
//! static dictionary** (§8), 121 transformations of which are addressable.
//!
//! # The dictionary is vendored data, not a table somebody typed out
//!
//! `data/brotli/dictionary.bin` is Appendix A's `DICT` array, declared in
//! `THIRDPARTY.md` and licensed BSD-3-Clause as an IETF Trust Code Component.
//! It is embedded with `include_bytes!` unconditionally: a Brotli decoder that
//! cannot resolve a dictionary reference is not a Brotli decoder, it is one
//! that works until it meets a real file. 120 KiB is the price, and it is
//! recorded rather than hidden behind a feature nothing could safely turn off.
//!
//! Four of the tables here carry the RFC's own published CRC-32 check values —
//! the dictionary, Appendix B's transformations, and §7.1's three context
//! lookups. Every one is asserted in a test below, so a transcription slip is
//! caught by the specification rather than by a font that renders wrong.
//!
//! # Never panics, and never spins
//!
//! Ruling 1. Every read is bounds-checked, every length is checked or
//! saturating, and output is capped by [`Limits::max_output`]. Two loops in
//! this format can be made to consume nothing: a prefix code with one symbol
//! reads **no bits** (§3.4, §3.5), and a static-dictionary transform can
//! produce an **empty** word (`OmitFirst9` of a four-byte word). A command
//! that did both would neither advance the reader nor grow the output, so
//! [`BrotliError::Malformed`] names that case explicitly rather than trusting
//! the input to make progress.

use crate::Limits;

/// Appendix A's `DICT`, vendored verbatim (`THIRDPARTY.md`).
const DICTIONARY: &[u8] = include_bytes!("../data/brotli/dictionary.bin");

/// §8: the bit-depth array that says how many words there are of each length.
const NDBITS: [u8; 25] = [
    0, 0, 0, 0, 10, 10, 11, 11, 10, 10, 10, 10, 10, 9, 9, 8, 7, 7, 8, 7, 7, 6, 6, 5, 5,
];

/// §8's `DOFFSET` recursion, evaluated once at compile time.
const DOFFSET: [u32; 25] = {
    let mut out = [0u32; 25];
    let mut length = 0usize;
    while length < 24 {
        let words = if length < 4 {
            0
        } else {
            1u32 << NDBITS[length]
        };
        out[length + 1] = out[length] + (length as u32) * words;
        length += 1;
    }
    out
};

/// The longest code any prefix code in this format may have (§3.5's
/// `32768 >> code length` bookkeeping bottoms out at 15).
const MAX_CODE_BITS: u32 = 15;

/// §8: a static-dictionary copy length outside this range is invalid.
const MIN_DICT_WORD: usize = 4;
const MAX_DICT_WORD: usize = 24;

/// Why a Brotli stream did not decode.
///
/// Three variants and no more, because a caller can only do three things:
/// ask for more bytes, refuse the file, or raise its ceiling.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum BrotliError {
    /// The stream ended before the last meta-block completed (§10: "If the
    /// stream ends before the completion of the last meta-block, then the
    /// stream should be rejected as invalid").
    Truncated,
    /// A rule the format states was broken. The string names the rule, not
    /// the byte, because the byte is rarely where the damage started.
    Malformed(&'static str),
    /// [`Limits::max_output`] was reached. A refusal rather than a truncation:
    /// half a font program is not a font, and the caller asked for a ceiling
    /// precisely so it would hear about this.
    ExceedsOutputLimit { limit: usize },
}

impl core::fmt::Display for BrotliError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            BrotliError::Truncated => write!(f, "the brotli stream ended mid-meta-block"),
            BrotliError::Malformed(why) => write!(f, "malformed brotli stream: {why}"),
            BrotliError::ExceedsOutputLimit { limit } => {
                write!(f, "the brotli stream decodes to more than {limit} bytes")
            }
        }
    }
}

/// Decodes one Brotli stream (RFC 7932 §10).
///
/// `limits` bounds the output, and it is not advisory: a few hundred bytes of
/// Brotli can name gigabytes through the copy machinery, which is a
/// denial-of-service primitive without a ceiling.
///
/// # Errors
///
/// [`BrotliError`] — truncated input, a rule of the format broken, or the
/// output ceiling reached.
pub fn brotli_decode(input: &[u8], limits: &Limits) -> Result<Vec<u8>, BrotliError> {
    Decoder::new(input, limits).run()
}

// ---- the bit reader ---------------------------------------------------------

/// §1.5.1: elements are packed from the least significant bit of each byte
/// upward, integers least-significant-bit first, prefix codes most-significant
/// bit of the code first (which is what makes `read_bit` the natural unit for
/// [`PrefixCode::decode`] and `read_bits` the natural unit for everything
/// else).
struct Bits<'a> {
    data: &'a [u8],
    /// Position in **bits** from the start of `data`.
    at: usize,
}

impl<'a> Bits<'a> {
    fn new(data: &'a [u8]) -> Bits<'a> {
        Bits { data, at: 0 }
    }

    fn read_bit(&mut self) -> Result<u32, BrotliError> {
        let byte = *self.data.get(self.at >> 3).ok_or(BrotliError::Truncated)?;
        let bit = (byte >> (self.at & 7)) & 1;
        self.at += 1;
        Ok(u32::from(bit))
    }

    /// `n` integer bits, least significant first. `n` is at most 24 anywhere
    /// in this format, which is what lets the accumulator be four bytes wide.
    fn read_bits(&mut self, n: u32) -> Result<u32, BrotliError> {
        debug_assert!(n <= 24, "no field in RFC 7932 is wider than 24 bits");
        if n == 0 {
            return Ok(0);
        }
        let first = self.at >> 3;
        let shift = (self.at & 7) as u32;
        let bytes = (shift + n).div_ceil(8) as usize;
        let mut acc = 0u64;
        for i in 0..bytes {
            let byte = *self.data.get(first + i).ok_or(BrotliError::Truncated)?;
            acc |= u64::from(byte) << (8 * i);
        }
        self.at += n as usize;
        Ok(((acc >> shift) & ((1u64 << n) - 1)) as u32)
    }

    /// Skips to the next byte boundary. §9.2 requires the skipped bits to be
    /// zero in every place this is called, and says so three times.
    fn align(&mut self, why: &'static str) -> Result<(), BrotliError> {
        while self.at & 7 != 0 {
            if self.read_bit()? != 0 {
                return Err(BrotliError::Malformed(why));
            }
        }
        Ok(())
    }

    /// `n` whole bytes from a byte-aligned position.
    fn read_bytes(&mut self, n: usize) -> Result<&'a [u8], BrotliError> {
        debug_assert!(self.at & 7 == 0, "read_bytes is only called after align");
        let from = self.at >> 3;
        let to = from.checked_add(n).ok_or(BrotliError::Truncated)?;
        let out = self.data.get(from..to).ok_or(BrotliError::Truncated)?;
        self.at += n * 8;
        Ok(out)
    }
}

// ---- prefix codes -----------------------------------------------------------

/// A canonical prefix code (§3.2), stored as the counts-and-symbols form that
/// decodes with one comparison per bit and no allocation per symbol.
struct PrefixCode {
    /// `counts[len]` is how many symbols have a code of that length.
    counts: [u16; MAX_CODE_BITS as usize + 1],
    /// Symbols ordered by `(code length, symbol)`, which *is* the canonical
    /// order (§3.2: codes of a length are consecutive, in symbol order).
    symbols: Vec<u16>,
    /// §3.4 and §3.5 both admit a code whose single symbol has a **zero
    /// length**: no bits are emitted and none are read.
    single: Option<u16>,
}

impl PrefixCode {
    fn single(symbol: u16) -> PrefixCode {
        PrefixCode {
            counts: [0; MAX_CODE_BITS as usize + 1],
            symbols: Vec::new(),
            single: Some(symbol),
        }
    }

    /// Builds the canonical code from a length per symbol.
    ///
    /// Rejects an incomplete or over-subscribed code, which §3.5 states as the
    /// `sum(32768 >> length) == 32768` rule.
    fn from_lengths(lengths: &[u8]) -> Result<PrefixCode, BrotliError> {
        let mut counts = [0u16; MAX_CODE_BITS as usize + 1];
        let mut used = 0u32;
        for &len in lengths {
            if len == 0 {
                continue;
            }
            if u32::from(len) > MAX_CODE_BITS {
                return Err(BrotliError::Malformed("a code length exceeded 15 bits"));
            }
            counts[len as usize] = counts[len as usize].saturating_add(1);
            used += 1;
        }
        if used == 0 {
            return Err(BrotliError::Malformed("a prefix code with no symbols"));
        }

        // §3.5: "the sum of (32768 >> code length) ... must be equal to
        // 32768". Under-full is ambiguous and over-full is unrepresentable;
        // both are rejected rather than decoded into something.
        let mut space = 0u32;
        for len in 1..=MAX_CODE_BITS {
            space += u32::from(counts[len as usize]) << (MAX_CODE_BITS - len);
        }
        if space != 1 << MAX_CODE_BITS {
            return Err(BrotliError::Malformed(
                "a prefix code that is not exactly full",
            ));
        }

        let mut offsets = [0usize; MAX_CODE_BITS as usize + 2];
        for len in 1..=MAX_CODE_BITS as usize {
            offsets[len + 1] = offsets[len] + counts[len] as usize;
        }
        let mut symbols = vec![0u16; used as usize];
        let mut next = offsets;
        for (symbol, &len) in lengths.iter().enumerate() {
            if len == 0 {
                continue;
            }
            let slot = next[len as usize];
            // The slot is in range because `used` counted exactly these
            // symbols and `offsets` partitioned that many places.
            if let Some(cell) = symbols.get_mut(slot) {
                *cell = symbol as u16;
            }
            next[len as usize] = slot + 1;
        }

        Ok(PrefixCode {
            counts,
            symbols,
            single: None,
        })
    }

    /// Reads one symbol, most significant bit of the code first (§1.5.1).
    fn decode(&self, bits: &mut Bits<'_>) -> Result<u16, BrotliError> {
        if let Some(symbol) = self.single {
            return Ok(symbol);
        }
        let mut code = 0u32;
        let mut first = 0u32;
        let mut index = 0usize;
        for len in 1..=MAX_CODE_BITS as usize {
            code |= bits.read_bit()?;
            let count = u32::from(self.counts[len]);
            if code - first < count {
                let at = index + (code - first) as usize;
                return self.symbols.get(at).copied().ok_or(BrotliError::Malformed(
                    "a prefix code index ran off its table",
                ));
            }
            index += count as usize;
            first = (first + count) << 1;
            code <<= 1;
        }
        Err(BrotliError::Malformed("a prefix code overran 15 bits"))
    }
}

/// §3.4's `ALPHABET_BITS`: the smallest width that can hold every symbol.
fn alphabet_bits(size: usize) -> u32 {
    match size.checked_sub(1) {
        None | Some(0) => 0,
        Some(top) => usize::BITS - top.leading_zeros(),
    }
}

/// Reads one prefix code: simple (§3.4) or complex (§3.5), told apart by the
/// first two bits.
fn read_prefix_code(bits: &mut Bits<'_>, alphabet: usize) -> Result<PrefixCode, BrotliError> {
    if alphabet == 0 {
        return Err(BrotliError::Malformed(
            "a prefix code over an empty alphabet",
        ));
    }
    let hskip = bits.read_bits(2)?;
    if hskip == 1 {
        return read_simple_prefix_code(bits, alphabet);
    }
    read_complex_prefix_code(bits, alphabet, hskip)
}

fn read_simple_prefix_code(
    bits: &mut Bits<'_>,
    alphabet: usize,
) -> Result<PrefixCode, BrotliError> {
    let width = alphabet_bits(alphabet);
    let nsym = bits.read_bits(2)? as usize + 1;
    let mut chosen = [0u16; 4];
    for i in 0..nsym {
        let symbol = bits.read_bits(width)? as usize;
        // §3.4: "If the integer value is greater than or equal to the alphabet
        // size, or the value is identical to a previous value, then the stream
        // should be rejected as invalid."
        if symbol >= alphabet {
            return Err(BrotliError::Malformed(
                "a simple prefix code named a symbol outside its alphabet",
            ));
        }
        if chosen.iter().take(i).any(|&s| usize::from(s) == symbol) {
            return Err(BrotliError::Malformed(
                "a simple prefix code repeated a symbol",
            ));
        }
        chosen[i] = symbol as u16;
    }

    if nsym == 1 {
        return Ok(PrefixCode::single(chosen[0]));
    }
    // §3.4's four shapes. The lengths are per symbol *in the order decoded*,
    // and `from_lengths` re-sorts them into canonical order, which is the
    // "prefix codes of the same bit length must be assigned to the symbols in
    // sorted order" rule two paragraphs above.
    let lengths: [u8; 4] = match nsym {
        2 => [1, 1, 0, 0],
        3 => [1, 2, 2, 0],
        _ => {
            if bits.read_bit()? == 0 {
                [2, 2, 2, 2]
            } else {
                [1, 2, 3, 3]
            }
        }
    };

    let mut per_symbol = vec![0u8; alphabet];
    for i in 0..nsym {
        if let Some(cell) = per_symbol.get_mut(usize::from(chosen[i])) {
            *cell = lengths[i];
        }
    }
    PrefixCode::from_lengths(&per_symbol)
}

/// §3.5: the order the code-length alphabet's own lengths arrive in.
const CODE_LENGTH_ORDER: [usize; 18] =
    [1, 2, 3, 4, 0, 5, 17, 6, 16, 7, 8, 9, 10, 11, 12, 13, 14, 15];

/// §3.5's fixed variable-length code over code lengths 0..5, written as the
/// tree it is. The specification gives it as bit patterns "parsed from right
/// to left", which is exactly first-bit-read-is-rightmost:
///
/// ```text
/// 0 -> 00     1 -> 0111   2 -> 011
/// 3 -> 10     4 -> 01     5 -> 1111
/// ```
fn read_code_length_length(bits: &mut Bits<'_>) -> Result<u8, BrotliError> {
    if bits.read_bit()? == 0 {
        // "00" and "10".
        return Ok(if bits.read_bit()? == 0 { 0 } else { 3 });
    }
    if bits.read_bit()? == 0 {
        // "01".
        return Ok(4);
    }
    if bits.read_bit()? == 0 {
        // "011".
        return Ok(2);
    }
    // "0111" and "1111".
    Ok(if bits.read_bit()? == 0 { 1 } else { 5 })
}

fn read_complex_prefix_code(
    bits: &mut Bits<'_>,
    alphabet: usize,
    hskip: u32,
) -> Result<PrefixCode, BrotliError> {
    // The lengths of the code-length code, in §3.5's order, stopping as soon
    // as the code is full — "any trailing zero code lengths are omitted".
    let mut cl_lengths = [0u8; 18];
    let mut space = 32i32;
    let mut nonzero = 0u32;
    let mut i = hskip as usize;
    while i < CODE_LENGTH_ORDER.len() && space > 0 {
        let len = read_code_length_length(bits)?;
        if len != 0 {
            nonzero += 1;
            space -= 32 >> len;
        }
        cl_lengths[CODE_LENGTH_ORDER[i]] = len;
        i += 1;
    }
    if nonzero != 1 && space != 0 {
        return Err(BrotliError::Malformed(
            "the code-length code is not exactly full",
        ));
    }
    let cl_code = if nonzero == 1 {
        // One non-zero length: §3.5's degenerate case, a code whose single
        // symbol costs no bits.
        let only = cl_lengths
            .iter()
            .position(|&l| l != 0)
            .ok_or(BrotliError::Malformed("a code-length code with no symbols"))?;
        PrefixCode::single(only as u16)
    } else {
        PrefixCode::from_lengths(&cl_lengths)?
    };

    // §3.5's repeat machinery. `prev` starts at 8 — "The previous length is
    // taken to be 8 before any code length code lengths are read."
    let mut lengths = vec![0u8; alphabet];
    let mut written = 0usize;
    let mut prev = 8u8;
    let mut repeat = 0u32;
    let mut repeat_length = 0u8;
    let mut space = 1i64 << MAX_CODE_BITS;
    let mut symbols_used = 0u32;

    while written < alphabet && space > 0 {
        let symbol = cl_code.decode(bits)?;
        if symbol < 16 {
            let len = symbol as u8;
            lengths[written] = len;
            written += 1;
            if len != 0 {
                prev = len;
                symbols_used += 1;
                space -= 1i64 << (MAX_CODE_BITS - u32::from(len));
            }
            repeat = 0;
            continue;
        }

        // 16 repeats the previous non-zero length, 17 repeats a zero; either
        // one following itself *extends* the count rather than starting a new
        // run, and a 16 after a 17 (or the reverse) starts over.
        let (extra, new_length) = if symbol == 16 {
            (2u32, prev)
        } else {
            (3u32, 0u8)
        };
        if repeat_length != new_length {
            repeat = 0;
            repeat_length = new_length;
        }
        let previous = repeat;
        if repeat > 0 {
            repeat = repeat.saturating_sub(2).saturating_mul(1 << extra);
        }
        repeat = repeat
            .saturating_add(bits.read_bits(extra)?)
            .saturating_add(3);
        let run = repeat.saturating_sub(previous) as usize;
        // §3.5: "If the number of times to repeat ... would result in more
        // lengths in total than the number of symbols in the alphabet, then
        // the stream should be rejected as invalid."
        if run > alphabet - written {
            return Err(BrotliError::Malformed(
                "a code-length repeat ran past the alphabet",
            ));
        }
        for _ in 0..run {
            lengths[written] = repeat_length;
            written += 1;
        }
        if repeat_length != 0 {
            symbols_used += run as u32;
            space -= (run as i64) << (MAX_CODE_BITS - u32::from(repeat_length));
        }
    }

    if symbols_used == 1 {
        let only = lengths
            .iter()
            .position(|&l| l != 0)
            .ok_or(BrotliError::Malformed("a prefix code with no symbols"))?;
        return Ok(PrefixCode::single(only as u16));
    }
    if space != 0 {
        return Err(BrotliError::Malformed(
            "a prefix code that is not exactly full",
        ));
    }
    PrefixCode::from_lengths(&lengths)
}

// ---- length and count tables ------------------------------------------------

/// §5's insert length code alphabet: `(extra bits, first length)`.
const INSERT_LENGTHS: [(u32, u32); 24] = [
    (0, 0),
    (0, 1),
    (0, 2),
    (0, 3),
    (0, 4),
    (0, 5),
    (1, 6),
    (1, 8),
    (2, 10),
    (2, 14),
    (3, 18),
    (3, 26),
    (4, 34),
    (4, 50),
    (5, 66),
    (5, 98),
    (6, 130),
    (7, 194),
    (8, 322),
    (9, 578),
    (10, 1090),
    (12, 2114),
    (14, 6210),
    (24, 22594),
];

/// §5's copy length code alphabet: `(extra bits, first length)`.
const COPY_LENGTHS: [(u32, u32); 24] = [
    (0, 2),
    (0, 3),
    (0, 4),
    (0, 5),
    (0, 6),
    (0, 7),
    (0, 8),
    (0, 9),
    (1, 10),
    (1, 12),
    (2, 14),
    (2, 18),
    (3, 22),
    (3, 30),
    (4, 38),
    (4, 54),
    (5, 70),
    (5, 102),
    (6, 134),
    (7, 198),
    (8, 326),
    (9, 582),
    (10, 1094),
    (24, 2118),
];

/// §6's block count code alphabet: `(extra bits, first count)`.
const BLOCK_COUNTS: [(u32, u32); 26] = [
    (2, 1),
    (2, 5),
    (2, 9),
    (2, 13),
    (3, 17),
    (3, 25),
    (3, 33),
    (3, 41),
    (4, 49),
    (4, 65),
    (4, 81),
    (4, 97),
    (5, 113),
    (5, 145),
    (5, 177),
    (5, 209),
    (6, 241),
    (6, 305),
    (7, 369),
    (8, 497),
    (9, 753),
    (10, 1265),
    (11, 2289),
    (12, 4337),
    (13, 8433),
    (24, 16625),
];

/// §5's insert-and-copy table, as the `(insert code base, copy code base,
/// distance is an implicit zero)` of each 64-symbol cell.
const INSERT_AND_COPY: [(u16, u16, bool); 11] = [
    (0, 0, true),
    (0, 8, true),
    (0, 0, false),
    (0, 8, false),
    (8, 0, false),
    (8, 8, false),
    (0, 16, false),
    (16, 0, false),
    (8, 16, false),
    (16, 8, false),
    (16, 16, false),
];

// ---- the decoder ------------------------------------------------------------

/// One literal block type's context mode (§7.1), as the integers §7.1 assigns.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ContextMode {
    Lsb6,
    Msb6,
    Utf8,
    Signed,
}

impl ContextMode {
    fn from_bits(value: u32) -> ContextMode {
        match value & 3 {
            0 => ContextMode::Lsb6,
            1 => ContextMode::Msb6,
            2 => ContextMode::Utf8,
            _ => ContextMode::Signed,
        }
    }

    fn context(self, p1: u8, p2: u8) -> usize {
        let id = match self {
            ContextMode::Lsb6 => p1 & 0x3f,
            ContextMode::Msb6 => p1 >> 2,
            ContextMode::Utf8 => LUT0[p1 as usize] | LUT1[p2 as usize],
            ContextMode::Signed => (LUT2[p1 as usize] << 3) | LUT2[p2 as usize],
        };
        usize::from(id)
    }
}

/// One block category's switching state (§6).
struct Blocks {
    types: usize,
    type_code: Option<PrefixCode>,
    count_code: Option<PrefixCode>,
    current: usize,
    previous: usize,
    remaining: u32,
}

impl Blocks {
    fn single() -> Blocks {
        Blocks {
            types: 1,
            type_code: None,
            count_code: None,
            current: 0,
            previous: 1,
            // §10: with one block type the count is set to 16777216, which is
            // the largest meta-block, so it never expires.
            remaining: 1 << 24,
        }
    }

    /// Consumes one element of this category, reading a block-switch command
    /// first if the current block is spent (§9.3).
    fn step(&mut self, bits: &mut Bits<'_>) -> Result<(), BrotliError> {
        if self.remaining == 0 {
            let (Some(types), Some(counts)) = (&self.type_code, &self.count_code) else {
                return Err(BrotliError::Malformed(
                    "a block ran out with no block-switch code to renew it",
                ));
            };
            let symbol = usize::from(types.decode(bits)?);
            // §6: symbol 0 is "the block type before this one", 1 is "one
            // more, wrapping", and 2.. are the types themselves.
            let next = match symbol {
                0 => self.previous,
                1 => {
                    if self.current + 1 >= self.types {
                        0
                    } else {
                        self.current + 1
                    }
                }
                other => other
                    .checked_sub(2)
                    .ok_or(BrotliError::Malformed("a block type below zero"))?,
            };
            if next >= self.types {
                return Err(BrotliError::Malformed(
                    "a block type outside the declared range",
                ));
            }
            self.previous = self.current;
            self.current = next;
            self.remaining = read_block_count(bits, counts)?;
            if self.remaining == 0 {
                return Err(BrotliError::Malformed("a block count of zero"));
            }
        }
        self.remaining -= 1;
        Ok(())
    }
}

fn read_block_count(bits: &mut Bits<'_>, code: &PrefixCode) -> Result<u32, BrotliError> {
    let symbol = usize::from(code.decode(bits)?);
    let (extra, base) = *BLOCK_COUNTS
        .get(symbol)
        .ok_or(BrotliError::Malformed("a block count code out of range"))?;
    Ok(base.saturating_add(bits.read_bits(extra)?))
}

/// §9.2's `1..11 bits` count, used for `NBLTYPES*` and `NTREES*`.
fn read_type_count(bits: &mut Bits<'_>) -> Result<usize, BrotliError> {
    if bits.read_bit()? == 0 {
        return Ok(1);
    }
    let n = bits.read_bits(3)?;
    if n == 0 {
        return Ok(2);
    }
    let extra = bits.read_bits(n)?;
    Ok((1usize << n) + 1 + extra as usize)
}

struct Decoder<'a> {
    bits: Bits<'a>,
    out: Vec<u8>,
    limit: usize,
    window: usize,
    /// §4's ring buffer of the four most recent distances, most recent first.
    last: [i64; 4],
}

impl<'a> Decoder<'a> {
    fn new(input: &'a [u8], limits: &Limits) -> Decoder<'a> {
        Decoder {
            bits: Bits::new(input),
            out: Vec::new(),
            limit: limits.max_output,
            window: 0,
            // §4: "initialized by the values 16, 15, 11, and 4 ... at the
            // beginning of the *stream*".
            last: [4, 11, 15, 16],
        }
    }

    fn run(mut self) -> Result<Vec<u8>, BrotliError> {
        self.window = read_window_size(&mut self.bits)?;
        loop {
            let last = self.bits.read_bit()? == 1;
            if last && self.bits.read_bit()? == 1 {
                // ISLASTEMPTY: the stream ends here.
                return Ok(self.out);
            }
            let nibbles = match self.bits.read_bits(2)? {
                0 => 4,
                1 => 5,
                2 => 6,
                _ => 0,
            };
            if nibbles == 0 {
                if last {
                    return Err(BrotliError::Malformed(
                        "the last meta-block declared itself metadata",
                    ));
                }
                self.skip_metadata()?;
                continue;
            }
            let raw = self.bits.read_bits(nibbles * 4)?;
            // §9.2: "if MNIBBLES is greater than 4, and the last nibble is all
            // zeros, then the stream should be rejected as invalid".
            if nibbles > 4 && raw >> ((nibbles - 1) * 4) == 0 {
                return Err(BrotliError::Malformed(
                    "a meta-block length padded with a zero nibble",
                ));
            }
            let mlen = raw as usize + 1;

            if !last && self.bits.read_bit()? == 1 {
                self.bits
                    .align("the pad before an uncompressed meta-block was not zero")?;
                let body = self.bits.read_bytes(mlen)?;
                self.push_literals(body)?;
                continue;
            }

            self.meta_block(mlen)?;
            if last {
                return Ok(self.out);
            }
        }
    }

    /// §9.2's empty meta-block, which may carry metadata bytes that are not
    /// part of the output *or* of the sliding window.
    fn skip_metadata(&mut self) -> Result<(), BrotliError> {
        if self.bits.read_bit()? != 0 {
            return Err(BrotliError::Malformed(
                "the reserved bit of an empty meta-block was set",
            ));
        }
        let count = self.bits.read_bits(2)? as usize;
        let skip = if count == 0 {
            0usize
        } else {
            let raw = self.bits.read_bits((count * 8) as u32)?;
            if count > 1 && raw >> ((count - 1) * 8) == 0 {
                return Err(BrotliError::Malformed(
                    "a metadata length padded with a zero byte",
                ));
            }
            raw as usize + 1
        };
        self.bits
            .align("the pad before a metadata block was not zero")?;
        let _ = self.bits.read_bytes(skip)?;
        Ok(())
    }

    fn push_literals(&mut self, bytes: &[u8]) -> Result<(), BrotliError> {
        if self.out.len().saturating_add(bytes.len()) > self.limit {
            return Err(BrotliError::ExceedsOutputLimit { limit: self.limit });
        }
        self.out.extend_from_slice(bytes);
        Ok(())
    }

    fn push_byte(&mut self, byte: u8) -> Result<(), BrotliError> {
        if self.out.len() >= self.limit {
            return Err(BrotliError::ExceedsOutputLimit { limit: self.limit });
        }
        self.out.push(byte);
        Ok(())
    }

    /// One compressed meta-block: the header of §9.2, then the commands of
    /// §9.3.
    #[allow(clippy::too_many_lines)]
    fn meta_block(&mut self, mlen: usize) -> Result<(), BrotliError> {
        let mut literal_blocks = self.read_blocks()?;
        let mut command_blocks = self.read_blocks()?;
        let mut distance_blocks = self.read_blocks()?;

        let npostfix = self.bits.read_bits(2)?;
        let ndirect = self.bits.read_bits(4)? << npostfix;

        let mut modes = Vec::with_capacity(literal_blocks.types);
        for _ in 0..literal_blocks.types {
            modes.push(ContextMode::from_bits(self.bits.read_bits(2)?));
        }

        let literal_trees = read_type_count(&mut self.bits)?;
        let literal_map = self.read_context_map(64 * literal_blocks.types, literal_trees)?;
        let distance_trees = read_type_count(&mut self.bits)?;
        let distance_map = self.read_context_map(4 * distance_blocks.types, distance_trees)?;

        let mut literal_codes = Vec::with_capacity(literal_trees);
        for _ in 0..literal_trees {
            literal_codes.push(read_prefix_code(&mut self.bits, 256)?);
        }
        let mut command_codes = Vec::with_capacity(command_blocks.types);
        for _ in 0..command_blocks.types {
            command_codes.push(read_prefix_code(&mut self.bits, 704)?);
        }
        // §3.3: the distance alphabet is 16 + NDIRECT + (48 << NPOSTFIX).
        let distance_alphabet = 16 + ndirect as usize + (48usize << npostfix);
        let mut distance_codes = Vec::with_capacity(distance_trees);
        for _ in 0..distance_trees {
            distance_codes.push(read_prefix_code(&mut self.bits, distance_alphabet)?);
        }

        let start = self.out.len();
        while self.out.len() - start < mlen {
            let before = self.bits.at;
            let produced = self.out.len();

            command_blocks.step(&mut self.bits)?;
            let command = command_codes
                .get(command_blocks.current)
                .ok_or(BrotliError::Malformed("no prefix code for a block type"))?;
            let symbol = usize::from(command.decode(&mut self.bits)?);
            let (insert_base, copy_base, implicit_zero) =
                *INSERT_AND_COPY
                    .get(symbol >> 6)
                    .ok_or(BrotliError::Malformed(
                        "an insert-and-copy code out of range",
                    ))?;
            let insert_code = usize::from(insert_base) + ((symbol >> 3) & 7);
            let copy_code = usize::from(copy_base) + (symbol & 7);
            let (insert_extra, insert_first) = *INSERT_LENGTHS
                .get(insert_code)
                .ok_or(BrotliError::Malformed("an insert length code out of range"))?;
            let (copy_extra, copy_first) = *COPY_LENGTHS
                .get(copy_code)
                .ok_or(BrotliError::Malformed("a copy length code out of range"))?;
            let insert = insert_first.saturating_add(self.bits.read_bits(insert_extra)?) as usize;
            let copy = copy_first.saturating_add(self.bits.read_bits(copy_extra)?) as usize;

            // §9.3: the insert may not carry the meta-block past MLEN.
            if insert > mlen - (self.out.len() - start) {
                return Err(BrotliError::Malformed(
                    "an insert length ran past the meta-block",
                ));
            }
            for _ in 0..insert {
                literal_blocks.step(&mut self.bits)?;
                let mode = modes
                    .get(literal_blocks.current)
                    .copied()
                    .ok_or(BrotliError::Malformed("no context mode for a block type"))?;
                let len = self.out.len();
                let p1 = if len >= 1 { self.out[len - 1] } else { 0 };
                let p2 = if len >= 2 { self.out[len - 2] } else { 0 };
                let context = mode.context(p1, p2);
                let tree = literal_map
                    .get(64 * literal_blocks.current + context)
                    .copied()
                    .ok_or(BrotliError::Malformed("a literal context map index"))?;
                let code = literal_codes
                    .get(usize::from(tree))
                    .ok_or(BrotliError::Malformed("a literal context map entry"))?;
                let byte = code.decode(&mut self.bits)? as u8;
                self.push_byte(byte)?;
            }

            if self.out.len() - start == mlen {
                // §9.3: the copy of the final command is ignored outright.
                break;
            }

            let distance = if implicit_zero {
                self.last[0]
            } else {
                distance_blocks.step(&mut self.bits)?;
                // §7.2: the distance context is the copy length, saturated at
                // "more than 4".
                let context = match copy {
                    0..=2 => 0usize,
                    3 => 1,
                    4 => 2,
                    _ => 3,
                };
                let tree = distance_map
                    .get(4 * distance_blocks.current + context)
                    .copied()
                    .ok_or(BrotliError::Malformed("a distance context map index"))?;
                let code = distance_codes
                    .get(usize::from(tree))
                    .ok_or(BrotliError::Malformed("a distance context map entry"))?;
                let symbol = usize::from(code.decode(&mut self.bits)?);
                let last = self.last;
                distance_from_symbol(&mut self.bits, &last, symbol, npostfix, ndirect)?
            };

            let max_backward = self.window.min(self.out.len()) as i64;
            if distance <= max_backward {
                if distance <= 0 {
                    // §4: "If a special distance symbol resolves to a zero or
                    // negative value, the stream should be rejected".
                    return Err(BrotliError::Malformed("a distance of zero or less"));
                }
                if !implicit_zero {
                    self.push_distance(distance);
                }
                if copy > mlen - (self.out.len() - start) {
                    return Err(BrotliError::Malformed(
                        "a copy length ran past the meta-block",
                    ));
                }
                self.copy_backward(distance as usize, copy)?;
            } else {
                let word = self.dictionary_word(distance - max_backward - 1, copy)?;
                if word.len() > mlen - (self.out.len() - start) {
                    return Err(BrotliError::Malformed(
                        "a dictionary word ran past the meta-block",
                    ));
                }
                self.push_literals(&word)?;
            }

            if self.bits.at == before && self.out.len() == produced {
                // A command with a zero-length code for everything and an
                // empty dictionary word consumes nothing and produces nothing.
                // Nothing in RFC 7932 forbids writing one; a decoder that took
                // it at face value would spin forever.
                return Err(BrotliError::Malformed(
                    "a command that consumed no input and produced no output",
                ));
            }
        }
        Ok(())
    }

    fn push_distance(&mut self, distance: i64) {
        self.last = [distance, self.last[0], self.last[1], self.last[2]];
    }

    fn copy_backward(&mut self, distance: usize, length: usize) -> Result<(), BrotliError> {
        if self.out.len().saturating_add(length) > self.limit {
            return Err(BrotliError::ExceedsOutputLimit { limit: self.limit });
        }
        let start = self
            .out
            .len()
            .checked_sub(distance)
            .ok_or(BrotliError::Malformed("a copy reached before the output"))?;
        // §10: "the referenced string may overlap the current position", so
        // this is a byte-at-a-time copy on purpose rather than a slice move —
        // <length 5, distance 2> over `XY` has to produce `XYXYX`.
        for from in start..start + length {
            let byte = *self
                .out
                .get(from)
                .ok_or(BrotliError::Malformed("a copy reached past the output"))?;
            self.out.push(byte);
        }
        Ok(())
    }

    /// §8: a distance past the window names a transformed dictionary word.
    fn dictionary_word(&self, word_id: i64, length: usize) -> Result<Vec<u8>, BrotliError> {
        if !(MIN_DICT_WORD..=MAX_DICT_WORD).contains(&length) {
            return Err(BrotliError::Malformed(
                "a dictionary reference outside the 4..24 length range",
            ));
        }
        let word_id = u32::try_from(word_id)
            .map_err(|_| BrotliError::Malformed("a dictionary word identifier out of range"))?;
        let bits = u32::from(NDBITS[length]);
        let words = 1u32 << bits;
        let index = word_id % words;
        let transform = (word_id >> bits) as usize;
        // §8: "If transform_id is greater than 120 ... the compressed stream
        // should be rejected as invalid."
        if transform >= TRANSFORMS.len() {
            return Err(BrotliError::Malformed("a dictionary transform above 120"));
        }
        let at = DOFFSET[length] as usize + index as usize * length;
        let base = DICTIONARY
            .get(at..at + length)
            .ok_or(BrotliError::Malformed("a dictionary word off the end"))?;
        Ok(apply_transform(base, transform))
    }

    fn read_blocks(&mut self) -> Result<Blocks, BrotliError> {
        let types = read_type_count(&mut self.bits)?;
        if types == 1 {
            return Ok(Blocks::single());
        }
        // §3.3: the block type alphabet is NBLTYPES + 2.
        let type_code = read_prefix_code(&mut self.bits, types + 2)?;
        let count_code = read_prefix_code(&mut self.bits, BLOCK_COUNTS.len())?;
        let remaining = read_block_count(&mut self.bits, &count_code)?;
        if remaining == 0 {
            return Err(BrotliError::Malformed("a first block count of zero"));
        }
        Ok(Blocks {
            types,
            type_code: Some(type_code),
            count_code: Some(count_code),
            current: 0,
            previous: 1,
            remaining,
        })
    }

    /// §7.3's context map: a prefix code, run-length coding for zeros, and an
    /// optional inverse move-to-front pass.
    fn read_context_map(&mut self, size: usize, trees: usize) -> Result<Vec<u8>, BrotliError> {
        if trees < 2 {
            return Ok(vec![0u8; size]);
        }
        let rlemax = if self.bits.read_bit()? == 0 {
            0u32
        } else {
            1 + self.bits.read_bits(4)?
        };
        let code = read_prefix_code(&mut self.bits, trees + rlemax as usize)?;
        let mut map = Vec::with_capacity(size.min(1 << 16));
        while map.len() < size {
            let symbol = u32::from(code.decode(&mut self.bits)?);
            if symbol == 0 {
                map.push(0);
                continue;
            }
            if symbol <= rlemax {
                // Symbol k repeats a zero (1 << k) to (1 << (k+1)) - 1 times.
                let run = (1usize << symbol) + self.bits.read_bits(symbol)? as usize;
                if run > size - map.len() {
                    return Err(BrotliError::Malformed("a context map run ran past the map"));
                }
                map.resize(map.len() + run, 0);
                continue;
            }
            let value = symbol - rlemax;
            let value = u8::try_from(value)
                .map_err(|_| BrotliError::Malformed("a context map value above 255"))?;
            map.push(value);
        }
        if self.bits.read_bit()? == 1 {
            inverse_move_to_front(&mut map);
        }
        // §7.3: "NTREES must equal the number of different values in the
        // context map", so a value at or above it is a stream that lied.
        if map.iter().any(|&v| usize::from(v) >= trees) {
            return Err(BrotliError::Malformed(
                "a context map named a prefix code that was not declared",
            ));
        }
        Ok(map)
    }
}

/// §4's distance short codes, the NDIRECT block beneath them, and the
/// extra-bit formula beneath that.
///
/// A free function rather than a method because it needs the reader mutably
/// and the ring buffer by value, and the ring buffer is four words of `Copy`.
fn distance_from_symbol(
    bits: &mut Bits<'_>,
    last: &[i64; 4],
    symbol: usize,
    npostfix: u32,
    ndirect: u32,
) -> Result<i64, BrotliError> {
    if symbol < 16 {
        // §4's sixteen short codes: four straight references to the ring
        // buffer, then -1, +1, -2, +2, -3, +3 against each of the first two.
        let (slot, delta) = match symbol {
            0..=3 => (symbol, 0i64),
            other => {
                let slot = (other - 4) / 6;
                let step = (other - 4) % 6;
                let magnitude = (step as i64 / 2) + 1;
                let sign = if step % 2 == 0 { -1 } else { 1 };
                (slot, sign * magnitude)
            }
        };
        let base = *last
            .get(slot)
            .ok_or(BrotliError::Malformed("a distance short code out of range"))?;
        return Ok(base + delta);
    }

    let offset = symbol - 16;
    if offset < ndirect as usize {
        // §4: "the next NDIRECT distance symbols ... represent distances from
        // 1 to NDIRECT", with no extra bits.
        return Ok(offset as i64 + 1);
    }
    let code = (offset - ndirect as usize) as u32;
    let nbits = 1 + (code >> (npostfix + 1));
    // §4: "The maximum number of extra bits is 24". The alphabet size makes
    // this unreachable from a well-formed header; a malformed one is what the
    // check is for.
    if nbits > 24 {
        return Err(BrotliError::Malformed("a distance code wider than 24 bits"));
    }
    let hcode = code >> npostfix;
    let lcode = code & ((1u32 << npostfix) - 1);
    let extra = bits.read_bits(nbits)?;
    let base = ((2 + i64::from(hcode & 1)) << nbits) - 4;
    Ok(((base + i64::from(extra)) << npostfix) + i64::from(lcode) + i64::from(ndirect) + 1)
}

/// §9.1's variable-length window size.
fn read_window_size(bits: &mut Bits<'_>) -> Result<usize, BrotliError> {
    let wbits = if bits.read_bit()? == 0 {
        16u32
    } else {
        let n = bits.read_bits(3)?;
        if n != 0 {
            17 + n
        } else {
            let m = bits.read_bits(3)?;
            match m {
                0 => 17,
                // §9.1: "Note that bit pattern 0010001 is invalid".
                1 => return Err(BrotliError::Malformed("the reserved window-size pattern")),
                other => 8 + other,
            }
        }
    };
    Ok((1usize << wbits) - 16)
}

/// §7.3's inverse move-to-front transform, transcribed from the C in the
/// specification.
fn inverse_move_to_front(values: &mut [u8]) {
    let mut mtf = [0u8; 256];
    for (i, slot) in mtf.iter_mut().enumerate() {
        *slot = i as u8;
    }
    for slot in values.iter_mut() {
        let index = usize::from(*slot);
        let value = mtf[index];
        *slot = value;
        let mut i = index;
        while i > 0 {
            mtf[i] = mtf[i - 1];
            i -= 1;
        }
        mtf[0] = value;
    }
}

/// §8's `Ferment`, transcribed from the C in the specification. Returns how
/// many bytes of `word` the call consumed.
fn ferment(word: &mut [u8], at: usize) -> usize {
    let Some(&byte) = word.get(at) else {
        return 1;
    };
    if byte < 192 {
        if (97..=122).contains(&byte) {
            word[at] = byte ^ 32;
        }
        1
    } else if byte < 224 {
        if at + 1 < word.len() {
            word[at + 1] ^= 32;
        }
        2
    } else {
        if at + 2 < word.len() {
            word[at + 2] ^= 5;
        }
        3
    }
}

/// One of Appendix B's 121 transformations: `prefix + T(word) + suffix`.
fn apply_transform(word: &[u8], transform: usize) -> Vec<u8> {
    let Some(&(prefix, elementary, suffix)) = TRANSFORMS.get(transform) else {
        return Vec::new();
    };
    let mut middle: Vec<u8> = match elementary {
        0 => word.to_vec(),
        1 => {
            let mut w = word.to_vec();
            if !w.is_empty() {
                ferment(&mut w, 0);
            }
            w
        }
        2 => {
            let mut w = word.to_vec();
            let mut i = 0usize;
            while i < w.len() {
                i += ferment(&mut w, i);
            }
            w
        }
        // 3..=11 are OmitFirst1..OmitFirst9, 12..=20 are OmitLast1..OmitLast9.
        3..=11 => {
            let k = usize::from(elementary) - 2;
            if word.len() < k {
                Vec::new()
            } else {
                word[k..].to_vec()
            }
        }
        12..=20 => {
            let k = usize::from(elementary) - 11;
            if word.len() < k {
                Vec::new()
            } else {
                word[..word.len() - k].to_vec()
            }
        }
        _ => word.to_vec(),
    };
    let mut out = Vec::with_capacity(prefix.len() + middle.len() + suffix.len());
    out.extend_from_slice(prefix);
    out.append(&mut middle);
    out.extend_from_slice(suffix);
    out
}

/// Appendix B's 121 word transformations, as `(prefix, elementary transform,
/// suffix)`. The elementary transform keeps Appendix B's own numbering: 0
/// Identity, 1 FermentFirst, 2 FermentAll, 3..=11 OmitFirst1..9, 12..=20
/// OmitLast1..9.
///
/// Transcribed from the RFC and checked against it: Appendix B says how to
/// re-encode this table as a byte sequence and publishes that sequence's
/// CRC-32, which `the_transform_table_matches_the_published_crc32` recomputes.
const TRANSFORMS: [(&[u8], u8, &[u8]); 121] = [
    (b"", 0, b""),              // Identity
    (b"", 0, b" "),             // Identity
    (b" ", 0, b" "),            // Identity
    (b"", 3, b""),              // OmitFirst1
    (b"", 1, b" "),             // FermentFirst
    (b"", 0, b" the "),         // Identity
    (b" ", 0, b""),             // Identity
    (b"s ", 0, b" "),           // Identity
    (b"", 0, b" of "),          // Identity
    (b"", 1, b""),              // FermentFirst
    (b"", 0, b" and "),         // Identity
    (b"", 4, b""),              // OmitFirst2
    (b"", 12, b""),             // OmitLast1
    (b", ", 0, b" "),           // Identity
    (b"", 0, b", "),            // Identity
    (b" ", 1, b" "),            // FermentFirst
    (b"", 0, b" in "),          // Identity
    (b"", 0, b" to "),          // Identity
    (b"e ", 0, b" "),           // Identity
    (b"", 0, b"\""),            // Identity
    (b"", 0, b"."),             // Identity
    (b"", 0, b"\">"),           // Identity
    (b"", 0, b"\x0A"),          // Identity
    (b"", 14, b""),             // OmitLast3
    (b"", 0, b"]"),             // Identity
    (b"", 0, b" for "),         // Identity
    (b"", 5, b""),              // OmitFirst3
    (b"", 13, b""),             // OmitLast2
    (b"", 0, b" a "),           // Identity
    (b"", 0, b" that "),        // Identity
    (b" ", 1, b""),             // FermentFirst
    (b"", 0, b". "),            // Identity
    (b".", 0, b""),             // Identity
    (b" ", 0, b", "),           // Identity
    (b"", 6, b""),              // OmitFirst4
    (b"", 0, b" with "),        // Identity
    (b"", 0, b"'"),             // Identity
    (b"", 0, b" from "),        // Identity
    (b"", 0, b" by "),          // Identity
    (b"", 7, b""),              // OmitFirst5
    (b"", 8, b""),              // OmitFirst6
    (b" the ", 0, b""),         // Identity
    (b"", 15, b""),             // OmitLast4
    (b"", 0, b". The "),        // Identity
    (b"", 2, b""),              // FermentAll
    (b"", 0, b" on "),          // Identity
    (b"", 0, b" as "),          // Identity
    (b"", 0, b" is "),          // Identity
    (b"", 18, b""),             // OmitLast7
    (b"", 12, b"ing "),         // OmitLast1
    (b"", 0, b"\x0A\x09"),      // Identity
    (b"", 0, b":"),             // Identity
    (b" ", 0, b". "),           // Identity
    (b"", 0, b"ed "),           // Identity
    (b"", 11, b""),             // OmitFirst9
    (b"", 9, b""),              // OmitFirst7
    (b"", 17, b""),             // OmitLast6
    (b"", 0, b"("),             // Identity
    (b"", 1, b", "),            // FermentFirst
    (b"", 19, b""),             // OmitLast8
    (b"", 0, b" at "),          // Identity
    (b"", 0, b"ly "),           // Identity
    (b" the ", 0, b" of "),     // Identity
    (b"", 16, b""),             // OmitLast5
    (b"", 20, b""),             // OmitLast9
    (b" ", 1, b", "),           // FermentFirst
    (b"", 1, b"\""),            // FermentFirst
    (b".", 0, b"("),            // Identity
    (b"", 2, b" "),             // FermentAll
    (b"", 1, b"\">"),           // FermentFirst
    (b"", 0, b"=\""),           // Identity
    (b" ", 0, b"."),            // Identity
    (b".com/", 0, b""),         // Identity
    (b" the ", 0, b" of the "), // Identity
    (b"", 1, b"'"),             // FermentFirst
    (b"", 0, b". This "),       // Identity
    (b"", 0, b","),             // Identity
    (b".", 0, b" "),            // Identity
    (b"", 1, b"("),             // FermentFirst
    (b"", 1, b"."),             // FermentFirst
    (b"", 0, b" not "),         // Identity
    (b" ", 0, b"=\""),          // Identity
    (b"", 0, b"er "),           // Identity
    (b" ", 2, b" "),            // FermentAll
    (b"", 0, b"al "),           // Identity
    (b" ", 2, b""),             // FermentAll
    (b"", 0, b"='"),            // Identity
    (b"", 2, b"\""),            // FermentAll
    (b"", 1, b". "),            // FermentFirst
    (b" ", 0, b"("),            // Identity
    (b"", 0, b"ful "),          // Identity
    (b" ", 1, b". "),           // FermentFirst
    (b"", 0, b"ive "),          // Identity
    (b"", 0, b"less "),         // Identity
    (b"", 2, b"'"),             // FermentAll
    (b"", 0, b"est "),          // Identity
    (b" ", 1, b"."),            // FermentFirst
    (b"", 2, b"\">"),           // FermentAll
    (b" ", 0, b"='"),           // Identity
    (b"", 1, b","),             // FermentFirst
    (b"", 0, b"ize "),          // Identity
    (b"", 2, b"."),             // FermentAll
    (b"\xC2\xA0", 0, b""),      // Identity
    (b" ", 0, b","),            // Identity
    (b"", 1, b"=\""),           // FermentFirst
    (b"", 2, b"=\""),           // FermentAll
    (b"", 0, b"ous "),          // Identity
    (b"", 2, b", "),            // FermentAll
    (b"", 1, b"='"),            // FermentFirst
    (b" ", 1, b","),            // FermentFirst
    (b" ", 2, b"=\""),          // FermentAll
    (b" ", 2, b", "),           // FermentAll
    (b"", 2, b","),             // FermentAll
    (b"", 2, b"("),             // FermentAll
    (b"", 2, b". "),            // FermentAll
    (b" ", 2, b"."),            // FermentAll
    (b"", 2, b"='"),            // FermentAll
    (b" ", 2, b". "),           // FermentAll
    (b" ", 1, b"=\""),          // FermentFirst
    (b" ", 2, b"='"),           // FermentAll
    (b" ", 1, b"='"),           // FermentFirst
];

/// §7.1's `Lut0`: the UTF8 context mode's table for the most recent byte.
const LUT0: [u8; 256] = [
    0, 0, 0, 0, 0, 0, 0, 0, 0, 4, 4, 0, 0, 4, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    8, 12, 16, 12, 12, 20, 12, 16, 24, 28, 12, 12, 32, 12, 36, 12, 44, 44, 44, 44, 44, 44, 44, 44,
    44, 44, 32, 32, 24, 40, 28, 12, 12, 48, 52, 52, 52, 48, 52, 52, 52, 48, 52, 52, 52, 52, 52, 48,
    52, 52, 52, 52, 52, 48, 52, 52, 52, 52, 52, 24, 12, 28, 12, 12, 12, 56, 60, 60, 60, 56, 60, 60,
    60, 56, 60, 60, 60, 60, 60, 56, 60, 60, 60, 60, 60, 56, 60, 60, 60, 60, 60, 24, 12, 28, 12, 0,
    0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1,
    0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1,
    2, 3, 2, 3, 2, 3, 2, 3, 2, 3, 2, 3, 2, 3, 2, 3, 2, 3, 2, 3, 2, 3, 2, 3, 2, 3, 2, 3, 2, 3, 2, 3,
    2, 3, 2, 3, 2, 3, 2, 3, 2, 3, 2, 3, 2, 3, 2, 3, 2, 3, 2, 3, 2, 3, 2, 3, 2, 3, 2, 3, 2, 3, 2, 3,
];

/// §7.1's `Lut1`: the UTF8 context mode's table for the byte before that.
const LUT1: [u8; 256] = [
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 1, 1, 1, 1, 1, 1,
    1, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 1, 1, 1, 1, 1,
    1, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 1, 1, 1, 1, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2,
];

/// §7.1's `Lut2`: the Signed context mode's table, used for both bytes.
const LUT2: [u8; 256] = [
    0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2,
    2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2,
    3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3,
    3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3,
    4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4,
    4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4,
    5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5,
    5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 7,
];

#[cfg(test)]
mod tests;
