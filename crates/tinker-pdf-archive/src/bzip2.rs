//! bzip2, hand-rolled: the block format, its Huffman tables and selectors, the
//! move-to-front and zero-run coding, the inverse Burrows–Wheeler transform and
//! the run-length pass in front of it.
//!
//! Two containers carry it: ZIP method 12 (APPNOTE 4.4.5) and 7z coder
//! `040202`. Both store a whole bzip2 stream — `BZh`, a level digit, blocks,
//! an end-of-stream marker — and both declare the unpacked size, so the output
//! is bounded by the caller before a bit is read.
//!
//! # Where it was written from
//!
//! bzip2 has no published specification; its format is what its reference
//! implementation reads. This decoder is written from that format as the
//! bzip2/libbzip2 1.0.x `decompress.c` validates it (read at the
//! `libarchive/bzip2` mirror on GitHub, 26 September 2026, for the checks it
//! makes: the level digit `1`–`9`, two to six Huffman groups, at least one
//! selector and at most 18 002 kept, code lengths 1 to 20, a zero-run weight
//! below 2^21, a block of at most `100 000 × level` symbols, and an origin
//! pointer inside the block). No line of it is copied — its licence is not one
//! `deny.toml` allows, and the shapes differ (a canonical decoder by counts
//! where it uses limit/base/perm tables; one pass per stage where it streams).
//!
//! **What adjudicates it is two checksums, and neither is this crate's.**
//! bzip2 carries a CRC-32 per block over the block's output and a combined CRC
//! over the stream, both checked here before a byte is handed back; ZIP and 7z
//! then check their own CRC-32 over the entry. A decoder wrong about one symbol
//! fails all three. The fixtures are CPython's `zipfile` (`ZIP_BZIP2`) and
//! py7zr's `FILTER_BZIP2`, both over libbzip2 1.0.8, in
//! `tests/coders/`, and their expected answer is the file that went in.
//!
//! # Refused by name
//!
//! **Randomised blocks** ([`Error::Randomised`]). bzip2 0.9.0 could XOR a
//! block with a fixed pseudo-random table to dodge its sorter's worst case;
//! 0.9.5 (1999) stopped writing them, and every later encoder sets the bit to
//! zero. Decoding one needs the 512-entry table, which this repository has no
//! first-party source for, and no writer on hand can produce a fixture.
//!
//! # Untrusted bytes
//!
//! Ruling 1. The one allocation a stream sizes is a block's symbols, and the
//! format bounds it at 900 000 (level 9); the output is bounded by
//! [`Limits::max_unpacked`], checked before every byte is written. A run that
//! would expand past either is an error, not a truncation.

/// What one decode may produce.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    /// The most bytes the whole input may decode to. Callers here pass the
    /// entry's declared size, already bounded by their own container's cap.
    pub max_unpacked: usize,
}

/// Why a stream did not decode. Every variant is a fact about the input.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Error {
    /// No `BZh` followed by a level digit `1`–`9` at the start.
    NotBzip2,
    /// The input ended inside a block or before the end-of-stream marker.
    Truncated,
    /// Neither a block's 48-bit magic (`0x314159265359`) nor the end of
    /// stream's (`0x177245385090`) where one had to be.
    BadMagic,
    /// A randomised block. See this module's header.
    Randomised,
    /// A block's tables are not the format: no symbol in use, a group count
    /// outside 2–6, no selectors, a selector naming a group that does not
    /// exist, or a code length outside 1–20.
    BadTables,
    /// A bit pattern no code in the current table has.
    BadCode,
    /// More symbols than the stream's level allows in one block.
    BlockTooLarge,
    /// A zero run whose weight reached 2^21, which no encoder writes.
    RunTooLong,
    /// The origin pointer is not inside the block.
    BadOrigin,
    /// A block's CRC-32 does not match what it decoded to.
    BlockCrcMismatch,
    /// The stream's combined CRC does not match its blocks'.
    StreamCrcMismatch,
    /// The output would pass [`Limits::max_unpacked`].
    TooLarge,
}

impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(match self {
            Error::NotBzip2 => "not a bzip2 stream",
            Error::Truncated => "a bzip2 stream that ends early",
            Error::BadMagic => "neither a block nor an end of stream where one must be",
            Error::Randomised => "a randomised bzip2 block, which this build does not read",
            Error::BadTables => "a bzip2 block whose tables are not the format",
            Error::BadCode => "a bit pattern no Huffman code in the table has",
            Error::BlockTooLarge => "a block larger than its stream's level allows",
            Error::RunTooLong => "a zero run longer than any encoder writes",
            Error::BadOrigin => "a block whose origin pointer is outside it",
            Error::BlockCrcMismatch => "a block whose CRC-32 does not match",
            Error::StreamCrcMismatch => "a stream whose combined CRC does not match",
            Error::TooLarge => "an output past this build's cap",
        })
    }
}

impl std::error::Error for Error {}

/// A block's 48-bit magic, the BCD digits of pi.
const BLOCK_MAGIC: u64 = 0x3141_5926_5359;
/// The end of stream's, the BCD digits of the square root of pi.
const END_MAGIC: u64 = 0x1772_4538_5090;
/// Symbols per selector: every fifty, the Huffman table may change.
const GROUP_SIZE: usize = 50;
/// The most selectors bzip2 keeps: `2 + 900 000 / 50`. A stream may declare
/// more (the field is fifteen bits) and 1.0.8 reads and discards the excess,
/// which some encoders rely on, so this does too.
const MAX_SELECTORS: usize = 2 + 900_000 / GROUP_SIZE;
/// Code lengths are 1 to 20 bits.
const MAX_CODE_LEN: usize = 20;
/// The weight at which a zero run is refused: bzip2's own `2 * 1024 * 1024`.
const MAX_RUN_WEIGHT: u32 = 1 << 21;

/// CRC-32 as bzip2 computes it: polynomial `04C11DB7` shifted in most
/// significant bit first, which is **not** the reflected CRC-32 ZIP, PNG and
/// `tinker_pdf_filters::crc32` share. Same polynomial, other bit order.
const CRC_TABLE: [u32; 256] = {
    let mut table = [0u32; 256];
    let mut i = 0;
    while i < 256 {
        let mut c = (i as u32) << 24;
        let mut bit = 0;
        while bit < 8 {
            c = if c & 0x8000_0000 != 0 {
                (c << 1) ^ 0x04C1_1DB7
            } else {
                c << 1
            };
            bit += 1;
        }
        table[i] = c;
        i += 1;
    }
    table
};

fn crc_update(crc: u32, byte: u8) -> u32 {
    (crc << 8) ^ CRC_TABLE[((crc >> 24) as u8 ^ byte) as usize]
}

/// The bit reader: most significant bit of each byte first.
struct Bits<'a> {
    input: &'a [u8],
    /// The next byte not yet in `buffer`.
    at: usize,
    buffer: u64,
    held: u32,
}

impl<'a> Bits<'a> {
    fn new(input: &'a [u8], at: usize) -> Self {
        Bits {
            input,
            at,
            buffer: 0,
            held: 0,
        }
    }

    fn refill(&mut self) {
        while self.held <= 56 {
            let Some(&byte) = self.input.get(self.at) else {
                return;
            };
            self.buffer = (self.buffer << 8) | u64::from(byte);
            self.held += 8;
            self.at += 1;
        }
    }

    /// `n` bits, `n` at most 32.
    fn bits(&mut self, n: u32) -> Result<u32, Error> {
        if self.held < n {
            self.refill();
            if self.held < n {
                return Err(Error::Truncated);
            }
        }
        self.held -= n;
        let value = (self.buffer >> self.held) & ((1u64 << n) - 1);
        Ok(value as u32)
    }

    fn bit(&mut self) -> Result<bool, Error> {
        Ok(self.bits(1)? == 1)
    }

    /// The byte a following stream would start at: the bits left in the
    /// current byte are padding.
    fn next_byte(&self) -> usize {
        self.at - (self.held / 8) as usize
    }
}

/// A canonical Huffman code: codes are handed out in order of length and,
/// within a length, of symbol — which is how bzip2's encoder assigns them.
struct Code {
    /// Codes of each length, 1 to 20.
    count: [u16; MAX_CODE_LEN + 1],
    /// Symbols sorted by (length, symbol).
    symbols: Vec<u16>,
}

impl Code {
    fn new(lengths: &[u8]) -> Code {
        let mut count = [0u16; MAX_CODE_LEN + 1];
        for &len in lengths {
            if let Some(slot) = count.get_mut(usize::from(len)) {
                *slot += 1;
            }
        }
        let mut symbols = Vec::with_capacity(lengths.len());
        for len in 1..=MAX_CODE_LEN {
            for (symbol, &l) in lengths.iter().enumerate() {
                if usize::from(l) == len {
                    symbols.push(symbol as u16);
                }
            }
        }
        Code { count, symbols }
    }

    fn decode(&self, bits: &mut Bits<'_>) -> Result<u16, Error> {
        let mut code: u32 = 0;
        let mut first: u32 = 0;
        let mut index: u32 = 0;
        for len in 1..=MAX_CODE_LEN {
            code |= bits.bits(1)?;
            let count = u32::from(self.count[len]);
            if code < first + count {
                let at = (index + code - first) as usize;
                return self.symbols.get(at).copied().ok_or(Error::BadCode);
            }
            index += count;
            first = (first + count) << 1;
            code <<= 1;
        }
        Err(Error::BadCode)
    }
}

/// Decodes every bzip2 stream at the start of `input`, one after another.
///
/// A stream ends at its end-of-stream marker, padded to a byte; a second
/// stream may follow (a parallel compressor writes one per chunk), and one
/// is read when the next bytes are `BZh` and a level digit. Anything else
/// after a complete stream ends the decode: the caller holds the output to
/// the length and the CRC-32 its container declared.
///
/// # Errors
/// [`Error`], one variant per way the input is not a bzip2 stream this build
/// reads.
pub fn decode(input: &[u8], limits: &Limits) -> Result<Vec<u8>, Error> {
    let mut out: Vec<u8> = Vec::with_capacity(limits.max_unpacked.min(1 << 20));
    let mut at = stream(input, 0, &mut out, limits)?;
    while let Some(rest) = input.get(at..) {
        if !is_stream_start(rest) {
            break;
        }
        at = stream(input, at, &mut out, limits)?;
    }
    Ok(out)
}

fn is_stream_start(bytes: &[u8]) -> bool {
    matches!(bytes, [b'B', b'Z', b'h', b'1'..=b'9', ..])
}

/// One stream from `at`; returns where the next byte after it is.
fn stream(input: &[u8], at: usize, out: &mut Vec<u8>, limits: &Limits) -> Result<usize, Error> {
    let head = input.get(at..).unwrap_or_default();
    if !is_stream_start(head) {
        return Err(Error::NotBzip2);
    }
    // `is_stream_start` matched four bytes, the fourth a digit 1-9.
    let level = usize::from(head.get(3).copied().unwrap_or(b'1') - b'0');
    let block_max = level * 100_000;
    let mut bits = Bits::new(input, at + 4);
    let mut combined: u32 = 0;
    let mut tt: Vec<u32> = Vec::new();
    loop {
        let magic = (u64::from(bits.bits(24)?) << 24) | u64::from(bits.bits(24)?);
        let crc = bits.bits(32)?;
        match magic {
            BLOCK_MAGIC => {
                let got = block(&mut bits, block_max, &mut tt, out, limits)?;
                if got != crc {
                    return Err(Error::BlockCrcMismatch);
                }
                combined = combined.rotate_left(1) ^ crc;
            }
            END_MAGIC => {
                if crc != combined {
                    return Err(Error::StreamCrcMismatch);
                }
                return Ok(bits.next_byte());
            }
            _ => return Err(Error::BadMagic),
        }
    }
}

/// One block, appended to `out`; returns the CRC of what it appended.
fn block(
    bits: &mut Bits<'_>,
    block_max: usize,
    tt: &mut Vec<u32>,
    out: &mut Vec<u8>,
    limits: &Limits,
) -> Result<u32, Error> {
    if bits.bit()? {
        return Err(Error::Randomised);
    }
    let origin = bits.bits(24)? as usize;

    // The symbol map: sixteen bits saying which ranges of sixteen byte values
    // appear, then sixteen bits for each range that does.
    let ranges = bits.bits(16)?;
    let mut in_use: Vec<u8> = Vec::with_capacity(256);
    for range in 0..16u32 {
        if ranges & (0x8000 >> range) != 0 {
            let values = bits.bits(16)?;
            for value in 0..16u32 {
                if values & (0x8000 >> value) != 0 {
                    in_use.push((range * 16 + value) as u8);
                }
            }
        }
    }
    if in_use.is_empty() {
        return Err(Error::BadTables);
    }
    // RUNA, RUNB, the move-to-front indices 1 .. in_use - 1, and end of block.
    let alphabet = in_use.len() + 2;
    let end_of_block = (in_use.len() + 1) as u16;

    let groups = bits.bits(3)? as usize;
    if !(2..=6).contains(&groups) {
        return Err(Error::BadTables);
    }
    let selector_count = bits.bits(15)? as usize;
    if selector_count == 0 {
        return Err(Error::BadTables);
    }
    // Each selector is a move-to-front index over the groups, in unary.
    let mut order: Vec<u8> = (0..groups as u8).collect();
    let mut selectors: Vec<u8> = Vec::with_capacity(selector_count.min(MAX_SELECTORS));
    for i in 0..selector_count {
        let mut index = 0usize;
        while bits.bit()? {
            index += 1;
            if index >= groups {
                return Err(Error::BadTables);
            }
        }
        if i < MAX_SELECTORS {
            let group = order.remove(index);
            order.insert(0, group);
            selectors.push(group);
        }
    }

    // Code lengths, one table per group, each length a delta from the last.
    let mut codes: Vec<Code> = Vec::with_capacity(groups);
    for _ in 0..groups {
        let mut length = bits.bits(5)? as i32;
        let mut lengths = vec![0u8; alphabet];
        for slot in &mut lengths {
            loop {
                if !(1..=MAX_CODE_LEN as i32).contains(&length) {
                    return Err(Error::BadTables);
                }
                if !bits.bit()? {
                    break;
                }
                length += if bits.bit()? { -1 } else { 1 };
            }
            *slot = length as u8;
        }
        codes.push(Code::new(&lengths));
    }

    // The symbols: move-to-front indices, with runs of the front symbol coded
    // in bijective base two by RUNA (1) and RUNB (2).
    tt.clear();
    let mut front: Vec<u8> = (0..=255u8).collect();
    let mut run: u32 = 0;
    let mut weight: u32 = 1;
    let mut group_left = 0usize;
    let mut group_at = 0usize;
    let mut code: Option<&Code> = None;
    loop {
        if group_left == 0 {
            let selector = *selectors.get(group_at).ok_or(Error::BadTables)?;
            group_at += 1;
            code = codes.get(usize::from(selector));
            group_left = GROUP_SIZE;
        }
        group_left -= 1;
        let symbol = code.ok_or(Error::BadTables)?.decode(bits)?;
        if symbol <= 1 {
            run += (u32::from(symbol) + 1) * weight;
            weight <<= 1;
            if weight >= MAX_RUN_WEIGHT {
                return Err(Error::RunTooLong);
            }
            continue;
        }
        if run > 0 {
            let head = usize::from(front.first().copied().unwrap_or(0));
            let value = u32::from(*in_use.get(head).ok_or(Error::BadTables)?);
            if tt.len() + run as usize > block_max {
                return Err(Error::BlockTooLarge);
            }
            tt.extend(core::iter::repeat_n(value, run as usize));
            run = 0;
            weight = 1;
        }
        if symbol == end_of_block {
            break;
        }
        let index = usize::from(symbol - 1);
        if index >= in_use.len() {
            return Err(Error::BadTables);
        }
        let value = front.remove(index);
        front.insert(0, value);
        if tt.len() >= block_max {
            return Err(Error::BlockTooLarge);
        }
        tt.push(u32::from(
            *in_use.get(usize::from(value)).ok_or(Error::BadTables)?,
        ));
    }

    let length = tt.len();
    if origin >= length {
        return Err(Error::BadOrigin);
    }

    // The inverse transform. Each entry's low byte is its symbol; the high
    // bits become the index of the entry that follows it in the original.
    let mut next = [0u32; 256];
    for &entry in tt.iter() {
        next[(entry & 0xFF) as usize] += 1;
    }
    let mut sum = 0u32;
    for slot in next.iter_mut() {
        let count = *slot;
        *slot = sum;
        sum += count;
    }
    for i in 0..length {
        let symbol = (tt.get(i).copied().unwrap_or(0) & 0xFF) as usize;
        let to = next[symbol] as usize;
        next[symbol] += 1;
        if let Some(entry) = tt.get_mut(to) {
            *entry |= (i as u32) << 8;
        }
    }

    // Walk it, undoing the run-length pass as the bytes come out: four equal
    // bytes are followed by a count of how many more.
    let mut crc = u32::MAX;
    let mut position = (tt.get(origin).copied().unwrap_or(0) >> 8) as usize;
    let mut last: Option<u8> = None;
    let mut same = 0u32;
    for _ in 0..length {
        let entry = *tt.get(position).ok_or(Error::BadOrigin)?;
        position = (entry >> 8) as usize;
        let byte = entry as u8;
        if same == 4 {
            let repeat = usize::from(byte);
            let value = last.unwrap_or(0);
            if out.len() + repeat > limits.max_unpacked {
                return Err(Error::TooLarge);
            }
            for _ in 0..repeat {
                out.push(value);
                crc = crc_update(crc, value);
            }
            same = 0;
            continue;
        }
        if last == Some(byte) {
            same += 1;
        } else {
            last = Some(byte);
            same = 1;
        }
        if out.len() >= limits.max_unpacked {
            return Err(Error::TooLarge);
        }
        out.push(byte);
        crc = crc_update(crc, byte);
    }
    Ok(!crc)
}

#[cfg(test)]
mod tests;
