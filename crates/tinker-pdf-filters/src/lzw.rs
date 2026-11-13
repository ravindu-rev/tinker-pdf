//! LZWDecode (7.4.4): 9→12-bit variable codes, MSB first.
//!
//! The table holds, for each code, the code of its string less the last
//! byte, that byte, and the string's length. So a code's string is written
//! from its last byte to its first, following the prefixes, straight into
//! room made at the output's end for exactly that many bytes: no byte is
//! gathered anywhere else first, copied or reversed, and each byte of the
//! output is written once. Codes come out of a 64-bit buffer refilled eight
//! bytes at a time.

use crate::{Limits, Tail, Warning, Warnings};

const CLEAR: u16 = 256;
const EOD: u16 = 257;
const FIRST: u16 = 258;
const MAX_ENTRIES: usize = 4096;

/// One code's string, as the table keeps it.
#[derive(Clone, Copy, Debug, Default)]
struct Entry {
    /// The code of the string less its last byte. A root has none, and its
    /// zero is never followed, since a root's string is its one byte.
    prefix: u16,
    /// The string's last byte.
    byte: u8,
    /// The string's length: a root's 1, and an entry's one more than its
    /// prefix's, so at most 3 839 with 3 838 entries above the roots.
    len: u16,
}

/// The string table, roots and entries. An entry's prefix is always a code
/// defined before it — the code read before the one that added it — so
/// following prefixes from any defined code reaches a root in at most its
/// length's steps.
struct Table {
    entries: Box<[Entry; MAX_ENTRIES]>,
}

impl Table {
    fn new() -> Self {
        let mut entries = Box::new([Entry::default(); MAX_ENTRIES]);
        for (byte, root) in (0..=255u8).zip(entries.iter_mut()) {
            *root = Entry {
                prefix: 0,
                byte,
                len: 1,
            };
        }
        Self { entries }
    }

    /// `code`'s entry. A code is at most twelve bits wide, so the mask
    /// changes none and keeps every index inside the table.
    fn get(&self, code: u16) -> Entry {
        self.entries[usize::from(code) & (MAX_ENTRIES - 1)]
    }

    fn set(&mut self, code: u16, entry: Entry) {
        self.entries[usize::from(code) & (MAX_ENTRIES - 1)] = entry;
    }

    /// Writes the string of `code` followed by `then`, cut to the bytes
    /// `room` holds, into `room` from its last byte to its first: what would
    /// fall past the room — `then`, and then the bytes of the code's string
    /// from its end — is passed over along the prefixes unwritten, and every
    /// byte of `room` is written once.
    fn place(&self, code: u16, then: Option<u8>, room: &mut [u8]) {
        let whole = usize::from(self.get(code).len) + usize::from(then.is_some());
        let mut skip = whole.saturating_sub(room.len());
        let mut slots = room.iter_mut().rev();
        if let Some(byte) = then {
            if skip > 0 {
                skip -= 1;
            } else if let Some(slot) = slots.next() {
                *slot = byte;
                wrote();
            }
        }
        let mut c = code;
        for _ in 0..skip {
            c = self.get(c).prefix;
        }
        for slot in slots {
            let entry = self.get(c);
            *slot = entry.byte;
            wrote();
            c = entry.prefix;
        }
    }
}

/// One byte of a string written into the output, counted under test.
fn wrote() {
    #[cfg(test)]
    tests::WRITES.with(|writes| writes.set(writes.get() + 1));
}

/// Codes read most significant bit first out of a 64-bit buffer: the next
/// code's bits stand at its top, `count` of them read and not yet taken.
struct Codes<'a> {
    input: &'a [u8],
    /// The next input byte not yet counted into the buffer.
    pos: usize,
    buf: u64,
    count: u32,
}

impl<'a> Codes<'a> {
    fn new(input: &'a [u8]) -> Self {
        Self {
            input,
            pos: 0,
            buf: 0,
            count: 0,
        }
    }

    /// The next `width`-bit code, or `None` where fewer bits than that are
    /// left.
    fn read(&mut self, width: u32) -> Option<u16> {
        if self.count < width {
            self.refill();
            if self.count < width {
                return None;
            }
        }
        let code = (self.buf >> (64 - width)) as u16;
        self.buf <<= width;
        self.count -= width;
        Some(code)
    }

    /// Tops the buffer up: with eight input bytes left, by one read of all
    /// eight, counting in the bytes that fit whole, so the bits under the
    /// count are the input's own next bits and reading those bytes again
    /// sets the same bits; nearer the end, a byte at a time. Only called
    /// with fewer than twelve bits in hand.
    fn refill(&mut self) {
        if let Some(word) = self
            .input
            .get(self.pos..)
            .and_then(|rest| rest.first_chunk::<8>())
        {
            self.buf |= u64::from_be_bytes(*word) >> self.count;
            let whole = (64 - self.count) / 8;
            self.pos += whole as usize;
            self.count += whole * 8;
            return;
        }
        while self.count <= 56 {
            let Some(&byte) = self.input.get(self.pos) else {
                break;
            };
            self.buf |= u64::from(byte) << (56 - self.count);
            self.count += 8;
            self.pos += 1;
        }
    }

    /// The input bytes the codes taken so far reach into: the bits after the
    /// last one in the byte it ends in are padding.
    fn bytes_taken(&self) -> usize {
        let bits = (self.pos as u64)
            .saturating_mul(8)
            .saturating_sub(u64::from(self.count));
        usize::try_from(bits.div_ceil(8)).unwrap_or(usize::MAX)
    }
}

/// LZWDecode's byte layer. `tail` says what may follow the EOD code.
pub(crate) fn lzw_bytes(
    input: &[u8],
    limits: &Limits,
    early_change: bool,
    tail: Tail,
    w: &mut Warnings,
) -> (Vec<u8>, bool) {
    let mut table = Table::new();
    let mut codes = Codes::new(input);
    let mut out = Vec::new();

    // 7.4.4.2: with /EarlyChange 1 (the default) the code width grows one
    // code before the table would otherwise require it.
    let early = u32::from(early_change);
    let mut width = 9u32;
    let mut next = FIRST;
    // The code read last and its string's first byte.
    let mut prev: Option<(u16, u8)> = None;

    loop {
        w.step_by(1);
        let Some(code) = codes.read(width) else {
            w.push(Warning::EarlyEod);
            return (out, false);
        };

        if code == CLEAR {
            next = FIRST;
            width = 9;
            prev = None;
            continue;
        }
        if code == EOD {
            // What followed the data starts at the next whole byte.
            if tail.garbage(input.get(codes.bytes_taken()..).unwrap_or_default()) {
                w.push(Warning::TrailingGarbage);
            }
            return (out, true);
        }

        // The string is the code's own, or, for the one code not yet
        // defined, the previous string and its first byte.
        let (base, then) = if code < 256 || (code >= FIRST && code < next) {
            (code, None)
        } else if code == next {
            // KwKwK: the encoder used an entry it defined with this very
            // code, so the string is the previous one plus its first byte.
            let Some((p, first)) = prev else {
                w.push(Warning::BadLzwCode);
                return (out, false);
            };
            (p, Some(first))
        } else {
            w.push(Warning::BadLzwCode);
            return (out, false);
        };

        let len = usize::from(table.get(base).len) + usize::from(then.is_some());
        w.step_len(len);
        let start = out.len();
        let n = len.min(limits.max_output.saturating_sub(start));
        out.resize(start + n, 0);
        if let Some(room) = out.get_mut(start..) {
            table.place(base, then, room);
        }
        if n < len {
            w.push(Warning::OutputCapHit);
            return (out, false);
        }

        let first = out.get(start).copied().unwrap_or_default();
        if let Some((p, _)) = prev {
            if usize::from(next) < MAX_ENTRIES {
                let entry = Entry {
                    prefix: p,
                    byte: first,
                    len: table.get(p).len.saturating_add(1),
                };
                table.set(next, entry);
                next += 1;
                if width < 12 && u32::from(next) + early >= 1u32 << width {
                    width += 1;
                }
            } else {
                // A full table without a Clear: freeze it and keep going,
                // which is what shipping readers do.
                w.push(Warning::LzwTableFull);
            }
        }
        prev = Some((code, first));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tiff::tests::pack_lzw;

    std::thread_local! {
        /// Every byte of a string [`Table::place`] has written on this
        /// thread.
        pub(super) static WRITES: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
    }

    const CAP: Limits = Limits::new(1 << 20);

    /// The decode as it was, kept to hold [`lzw_bytes`] to: each code read
    /// three bytes at a time, its string gathered from its last byte to its
    /// first into a scratch buffer, reversed and copied out. What it
    /// returns, how it ends, its warnings and the steps it counts.
    fn chained_expansion(
        input: &[u8],
        limits: &Limits,
        early_change: bool,
        tail: Tail,
    ) -> (Vec<u8>, bool, Vec<Warning>, u64) {
        let mut w = Warnings::default();
        let (data, complete) = chained_expansion_bytes(input, limits, early_change, tail, &mut w);
        let steps = w.steps();
        (data, complete, w.into_vec(), steps)
    }

    fn chained_expansion_bytes(
        input: &[u8],
        limits: &Limits,
        early_change: bool,
        tail: Tail,
        w: &mut Warnings,
    ) -> (Vec<u8>, bool) {
        let mut prefix = vec![0u16; MAX_ENTRIES];
        let mut suffix = vec![0u8; MAX_ENTRIES];
        let mut scratch: Vec<u8> = Vec::new();
        let mut out = Vec::new();
        let early = u32::from(early_change);
        let total_bits = input.len() as u64 * 8;
        let mut bitpos = 0u64;
        let mut width = 9u32;
        let mut next = FIRST;
        let mut prev: Option<u16> = None;
        loop {
            w.step_by(1);
            let Some(code) = read_code(input, bitpos, total_bits, width) else {
                w.push(Warning::EarlyEod);
                return (out, false);
            };
            bitpos += u64::from(width);
            if code == CLEAR {
                next = FIRST;
                width = 9;
                prev = None;
                continue;
            }
            if code == EOD {
                let after = usize::try_from(bitpos.div_ceil(8)).unwrap_or(usize::MAX);
                if tail.garbage(input.get(after..).unwrap_or_default()) {
                    w.push(Warning::TrailingGarbage);
                }
                return (out, true);
            }
            let known = code < 256 || (code >= FIRST && code < next);
            if known {
                if !expand(code, &prefix, &suffix, &mut scratch) {
                    w.push(Warning::BadLzwCode);
                    return (out, false);
                }
            } else if code == next {
                let Some(p) = prev else {
                    w.push(Warning::BadLzwCode);
                    return (out, false);
                };
                if !expand(p, &prefix, &suffix, &mut scratch) {
                    w.push(Warning::BadLzwCode);
                    return (out, false);
                }
                match scratch.first().copied() {
                    Some(f) => scratch.push(f),
                    None => {
                        w.push(Warning::BadLzwCode);
                        return (out, false);
                    }
                }
            } else {
                w.push(Warning::BadLzwCode);
                return (out, false);
            }
            w.step_len(scratch.len());
            let room = limits.max_output.saturating_sub(out.len());
            let n = scratch.len().min(room);
            out.extend_from_slice(&scratch[..n]);
            if n < scratch.len() {
                w.push(Warning::OutputCapHit);
                return (out, false);
            }
            if let (Some(p), Some(&f)) = (prev, scratch.first()) {
                if (next as usize) < MAX_ENTRIES {
                    prefix[next as usize] = p;
                    suffix[next as usize] = f;
                    next += 1;
                    if width < 12 && u32::from(next) + early >= 1u32 << width {
                        width += 1;
                    }
                } else {
                    w.push(Warning::LzwTableFull);
                }
            }
            prev = Some(code);
        }
    }

    /// Walks a code's chain into `scratch`, last byte first, and reverses
    /// it. `false` means the chain is broken or cyclic.
    fn expand(code: u16, prefix: &[u16], suffix: &[u8], scratch: &mut Vec<u8>) -> bool {
        scratch.clear();
        let mut c = code;
        loop {
            if scratch.len() > MAX_ENTRIES {
                return false;
            }
            if c < 256 {
                scratch.push(c as u8);
                break;
            }
            if c < FIRST {
                return false;
            }
            scratch.push(suffix[c as usize]);
            c = prefix[c as usize];
        }
        scratch.reverse();
        true
    }

    /// A code read out of the three bytes it can span.
    fn read_code(input: &[u8], bitpos: u64, total_bits: u64, width: u32) -> Option<u16> {
        if bitpos + u64::from(width) > total_bits {
            return None;
        }
        let byte = (bitpos / 8) as usize;
        let off = (bitpos % 8) as u32;
        let b0 = u32::from(*input.get(byte)?);
        let b1 = u32::from(input.get(byte + 1).copied().unwrap_or(0));
        let b2 = u32::from(input.get(byte + 2).copied().unwrap_or(0));
        let chunk = (b0 << 16) | (b1 << 8) | b2;
        let shift = 24 - off - width;
        Some(((chunk >> shift) & ((1u32 << width) - 1)) as u16)
    }

    /// A linear congruential generator's top bits, as `bound`-bounded draws.
    fn draws(seed: u64) -> impl FnMut(usize) -> usize {
        let mut state = seed;
        move |bound| {
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            ((state >> 33) as usize) % bound.max(1)
        }
    }

    /// `count` random codes a decoder can read after a Clear, then the EOD
    /// code: literals, entries anywhere in the table, the entry just added,
    /// the one code not yet defined (KwKwK), and, where `clears`, now and
    /// then a Clear. Without them the table fills at 4 096 codes and the
    /// stream goes on naming entries of the full table.
    fn random_codes(seed: u64, count: usize, clears: bool) -> Vec<u16> {
        drawn_codes(seed, count, clears).0
    }

    /// [`random_codes`], and how many of them were the code not yet defined.
    fn drawn_codes(seed: u64, count: usize, clears: bool) -> (Vec<u16>, usize) {
        let mut draw = draws(seed);
        let mut codes = vec![CLEAR];
        let mut next = usize::from(FIRST);
        let mut defined = false;
        let mut kwkwk = 0;
        for _ in 0..count {
            let roll = draw(100);
            let code = if clears && roll == 0 {
                CLEAR
            } else if defined && next < MAX_ENTRIES && roll < 5 {
                kwkwk += 1;
                next as u16
            } else if next > usize::from(FIRST) && roll < 15 {
                (next - 1) as u16
            } else if next > usize::from(FIRST) && roll < 50 {
                (usize::from(FIRST) + draw(next - usize::from(FIRST))) as u16
            } else {
                draw(256) as u16
            };
            codes.push(code);
            if code == CLEAR {
                next = usize::from(FIRST);
                defined = false;
            } else {
                if defined && next < MAX_ENTRIES {
                    next += 1;
                }
                defined = true;
            }
        }
        codes.push(EOD);
        (codes, kwkwk)
    }

    /// The longest strings a table holds: a literal and then, 3 838 times,
    /// the code not yet defined, each string one byte longer than the one
    /// before, up to 3 839 bytes at code 4 095, which fills the table; then
    /// that last entry three times over the full table, and the EOD code.
    fn longest_strings() -> Vec<u16> {
        let mut codes = vec![CLEAR, u16::from(b'x')];
        codes.extend(FIRST..MAX_ENTRIES as u16);
        codes.extend([MAX_ENTRIES as u16 - 1; 3]);
        codes.push(EOD);
        codes
    }

    /// **The longest strings are written whole.** [`longest_strings`] under
    /// both `/EarlyChange` values decodes as the decoder before it decoded
    /// it — bytes, outcome, warnings and steps — under a ceiling past its
    /// end and under ceilings that cut its last string at its last byte and
    /// 3 000 bytes from its end, cut a string near the middle, and fall
    /// between two strings; and whole, its strings are what the codes say,
    /// `x` repeated, one byte longer each, the table full.
    #[test]
    fn the_longest_strings_are_written_whole() {
        // Strings of 1 to 3 839 bytes, then the longest three times more.
        let total = (1..=3_839usize).sum::<usize>() + 3 * 3_839;
        for early_change in [true, false] {
            let stream = pack_lzw(&longest_strings(), true, early_change);
            for cap in [1usize << 23, total - 1, total - 3_000, 2_000_000, 300] {
                let limits = Limits::new(cap);
                let mut w = Warnings::default();
                let (data, complete) =
                    lzw_bytes(&stream, &limits, early_change, Tail::WhiteSpace, &mut w);
                let steps = w.steps();
                let ours = (data, complete, w.into_vec(), steps);
                assert_eq!(
                    ours,
                    chained_expansion(&stream, &limits, early_change, Tail::WhiteSpace),
                    "early change {early_change}, ceiling {cap}"
                );
                if cap > total {
                    assert_eq!(
                        (ours.0.len(), ours.1, ours.2),
                        (total, true, vec![Warning::LzwTableFull]),
                        "early change {early_change}"
                    );
                    assert!(ours.0.iter().all(|&b| b == b'x'));
                }
            }
        }
    }

    /// Code streams of every shape the two decoders could part on, packed
    /// by the crate's one LZW encoder for `early_change`: random ones with
    /// Clear codes, ones that fill the table and go on, 7.4.4.2's example,
    /// each with a bit flipped anywhere, cut short, and followed by bytes
    /// that are not its own.
    fn streams(early_change: bool) -> Vec<Vec<u8>> {
        let mut whole = Vec::new();
        for seed in 0..6u64 {
            whole.push(pack_lzw(
                &random_codes(seed, 3_000, true),
                true,
                early_change,
            ));
            whole.push(pack_lzw(
                &random_codes(seed + 50, 6_000, false),
                true,
                early_change,
            ));
        }
        whole.push(vec![0x80, 0x0b, 0x60, 0x50, 0x22, 0x0c, 0x0c, 0x85, 0x01]);
        let mut streams = whole.clone();
        for (index, stream) in whole.iter().enumerate() {
            let mut draw = draws(index as u64 + 900);
            for _ in 0..3 {
                let mut damaged = stream.clone();
                let bit = draw(damaged.len() * 8);
                damaged[bit / 8] ^= 1 << (bit % 8);
                streams.push(damaged);
            }
            streams.push(stream[..stream.len() * 2 / 3].to_vec());
            let mut followed = stream.clone();
            followed.extend_from_slice(b"\r\n\x00not this stream's");
            streams.push(followed);
        }
        streams
    }

    /// **Strings written backwards equal the chained expansion.**
    ///
    /// Every stream of [`streams`], under both `/EarlyChange` values, both
    /// tails, a ceiling that never binds and ceilings that cut a string
    /// short, decodes to what the decoder before it gave: the same bytes,
    /// the same outcome, the same warnings in the same order and the same
    /// steps. The random streams reach the code not yet defined, Clear codes
    /// and a full table, which the test asserts of the codes it draws and
    /// of the warnings, so the agreement covers each; [`longest_strings`]
    /// reaches the longest string a table holds.
    #[test]
    fn strings_written_backwards_equal_the_chained_expansion() {
        for seed in 0..6u64 {
            let (clears, kwkwk) = drawn_codes(seed, 3_000, true);
            assert!(
                clears[1..].contains(&CLEAR),
                "seed {seed}: a Clear past the first"
            );
            assert!(kwkwk > 0, "seed {seed}: the code not yet defined");
        }
        let mut full = 0;
        let mut whole = 0;
        for early_change in [true, false] {
            for (index, stream) in streams(early_change).iter().enumerate() {
                for cap in [1usize << 20, 5_000, 61, 1] {
                    for tail in [Tail::WhiteSpace, Tail::Nothing] {
                        let limits = Limits::new(cap);
                        let mut w = Warnings::default();
                        let (data, complete) =
                            lzw_bytes(stream, &limits, early_change, tail, &mut w);
                        let steps = w.steps();
                        let ours = (data, complete, w.into_vec(), steps);
                        let theirs = chained_expansion(stream, &limits, early_change, tail);
                        assert_eq!(
                            ours, theirs,
                            "stream {index}, early change {early_change}, ceiling {cap}, {tail:?}"
                        );
                        full += usize::from(ours.2.contains(&Warning::LzwTableFull));
                        whole += usize::from(ours.1 && cap == 1 << 20);
                    }
                }
            }
        }
        assert!(full > 0, "no stream filled the table");
        assert!(whole > 0, "no stream decoded whole");
    }

    /// **An output byte is written once.** Counted through [`WRITES`]: every
    /// byte the decode returns, from each random stream and from
    /// [`longest_strings`], under both `/EarlyChange` values, whole and under
    /// a ceiling one byte short of its end, which cuts its last string, is
    /// one write of [`Table::place`]'s, and nothing else is written: no byte
    /// is gathered first and copied, and none is reversed.
    #[test]
    fn an_output_byte_is_written_once() {
        let mut streams = vec![longest_strings()];
        for seed in 0..6u64 {
            streams.push(random_codes(seed, 3_000, true));
            streams.push(random_codes(seed + 50, 6_000, false));
        }
        for early_change in [true, false] {
            for (index, codes) in streams.iter().enumerate() {
                let stream = pack_lzw(codes, true, early_change);
                let case = format!("stream {index}, early change {early_change}");
                // No string is longer than 3 840 bytes and no stream has as
                // many as 7 000 codes, so none reaches this ceiling.
                let ((data, complete), writes) =
                    decode_counting_writes(&stream, 1 << 26, early_change);
                assert!(complete, "{case}");
                assert!(
                    data.len() > codes.len(),
                    "{case}: strings longer than a byte"
                );
                assert_eq!(writes, data.len() as u64, "{case}");
                let cap = data.len() - 1;
                let ((cut, complete), writes) = decode_counting_writes(&stream, cap, early_change);
                assert_eq!((cut.len(), complete), (cap, false), "{case}: the ceiling");
                assert_eq!(writes, cap as u64, "{case}, under the ceiling");
            }
        }
    }

    /// `stream` decoded under the ceiling `cap`: the bytes and whether they
    /// are complete, and the writes [`Table::place`] made.
    fn decode_counting_writes(
        stream: &[u8],
        cap: usize,
        early_change: bool,
    ) -> ((Vec<u8>, bool), u64) {
        WRITES.with(|writes| writes.set(0));
        let mut w = Warnings::default();
        let decoded = lzw_bytes(
            stream,
            &Limits::new(cap),
            early_change,
            Tail::WhiteSpace,
            &mut w,
        );
        (decoded, WRITES.with(std::cell::Cell::get))
    }

    /// **The buffer reads a code wherever the input ends.** A stream whose
    /// table fills, cut after each of its first 300 bytes, where its codes
    /// widen from nine bits to ten, and each of its last 300, where the full
    /// table's are twelve, so the buffer's last eight-byte read and the
    /// bytes after it fall at every place a code can: each cut decodes as the
    /// decoder before it decoded it, the same bytes, outcome, warnings and
    /// steps.
    #[test]
    fn codes_are_read_alike_wherever_the_input_ends() {
        let stream = pack_lzw(&random_codes(3, 5_000, false), true, true);
        let n = stream.len();
        assert!(n > 600, "{n} bytes");
        for len in (0..=300).chain(n - 300..=n) {
            let cut = &stream[..len];
            let mut w = Warnings::default();
            let (data, complete) = lzw_bytes(cut, &CAP, true, Tail::WhiteSpace, &mut w);
            let steps = w.steps();
            assert_eq!(
                (data, complete, w.into_vec(), steps),
                chained_expansion(cut, &CAP, true, Tail::WhiteSpace),
                "cut at {len}"
            );
        }
    }

    fn lzw(input: &[u8]) -> (Vec<u8>, bool, Vec<Warning>) {
        let mut w = Warnings::default();
        let (d, c) = lzw_bytes(input, &CAP, true, Tail::WhiteSpace, &mut w);
        (d, c, w.into_vec())
    }

    /// Packs (code, width) pairs MSB-first, zero-padded to a byte.
    fn pack(codes: &[(u16, u32)]) -> Vec<u8> {
        let mut bits: Vec<u8> = Vec::new();
        for &(c, wd) in codes {
            for i in (0..wd).rev() {
                bits.push(((u32::from(c) >> i) & 1) as u8);
            }
        }
        while bits.len() % 8 != 0 {
            bits.push(0);
        }
        bits.chunks(8)
            .map(|c| c.iter().fold(0u8, |a, &b| (a << 1) | b))
            .collect()
    }

    #[test]
    fn spec_example_round_trips() {
        // 7.4.4.2, Table 8: "-----A---B" encodes to these nine bytes, and
        // its third code is the KwKwK case.
        let enc = [0x80, 0x0b, 0x60, 0x50, 0x22, 0x0c, 0x0c, 0x85, 0x01];
        let (d, c, w) = lzw(&enc);
        assert_eq!(d, b"-----A---B");
        assert!(c);
        assert!(w.is_empty());
    }

    #[test]
    fn single_byte_codes_decode() {
        let enc = pack(&[
            (256, 9),
            (u16::from(b'A'), 9),
            (u16::from(b'B'), 9),
            (257, 9),
        ]);
        let (d, c, w) = lzw(&enc);
        assert_eq!(d, b"AB");
        assert!(c);
        assert!(w.is_empty());
    }

    #[test]
    fn a_clear_code_resets_the_table() {
        let enc = pack(&[
            (256, 9),
            (u16::from(b'A'), 9),
            (u16::from(b'B'), 9),
            (256, 9),
            (u16::from(b'C'), 9),
            (257, 9),
        ]);
        let (d, c, _) = lzw(&enc);
        assert_eq!(d, b"ABC");
        assert!(c);
    }

    #[test]
    fn missing_leading_clear_is_tolerated() {
        let enc = pack(&[(u16::from(b'A'), 9), (u16::from(b'B'), 9), (257, 9)]);
        let (d, c, _) = lzw(&enc);
        assert_eq!(d, b"AB");
        assert!(c);
    }

    #[test]
    fn out_of_range_code_stops_with_a_warning() {
        let enc = pack(&[(256, 9), (u16::from(b'A'), 9), (300, 9), (257, 9)]);
        let (d, c, w) = lzw(&enc);
        assert_eq!(d, b"A");
        assert!(!c);
        assert_eq!(w, vec![Warning::BadLzwCode]);
    }

    #[test]
    fn kwkwk_without_a_previous_code_is_rejected() {
        let enc = pack(&[(256, 9), (258, 9)]);
        let (d, c, w) = lzw(&enc);
        assert!(d.is_empty());
        assert!(!c);
        assert_eq!(w, vec![Warning::BadLzwCode]);
    }

    #[test]
    fn missing_eod_is_reported() {
        let enc = pack(&[(256, 9), (u16::from(b'A'), 9)]);
        let (d, c, w) = lzw(&enc);
        assert_eq!(d, b"A");
        assert!(!c);
        assert_eq!(w, vec![Warning::EarlyEod]);
    }

    #[test]
    fn truncating_the_spec_stream_keeps_the_prefix() {
        let enc = [0x80, 0x0b, 0x60, 0x50];
        let (d, c, w) = lzw(&enc);
        assert_eq!(d, b"---");
        assert!(!c);
        assert_eq!(w, vec![Warning::EarlyEod]);
    }

    #[test]
    fn empty_input_warns_once() {
        let (d, c, w) = lzw(b"");
        assert!(d.is_empty());
        assert!(!c);
        assert_eq!(w, vec![Warning::EarlyEod]);
    }

    #[test]
    fn bytes_after_the_eod_are_ignored() {
        let mut enc = pack(&[(256, 9), (u16::from(b'A'), 9), (257, 9)]);
        enc.extend_from_slice(b"junk");
        let (d, c, w) = lzw(&enc);
        assert_eq!(d, b"A");
        assert!(c);
        assert_eq!(w, vec![Warning::TrailingGarbage]);
    }

    /// The end-of-line 7.3.8.1 keeps out of `/Length`, counted in by the
    /// producer, follows the byte the EOD code ends in and is no garbage; a
    /// byte that is not white space after it still is.
    #[test]
    fn white_space_after_the_eod_is_the_streams_own() {
        let enc = pack(&[(256, 9), (u16::from(b'A'), 9), (257, 9)]);
        for tail in [&b"\n"[..], b"\r\n", b"\r", b" \t\x0c\r\n", b"\x00"] {
            let mut s = enc.clone();
            s.extend_from_slice(tail);
            let (d, c, w) = lzw(&s);
            assert_eq!(d, b"A", "{tail:?}");
            assert!(c, "{tail:?}");
            assert!(w.is_empty(), "{tail:?}: {w:?}");
        }
        let mut s = enc;
        s.extend_from_slice(b"\njunk");
        assert_eq!(lzw(&s).2, vec![Warning::TrailingGarbage]);
    }

    /// A TIFF strip ends where its data does, so a byte after its EOD code is
    /// garbage, white space or not.
    #[test]
    fn a_strip_keeps_white_space_after_its_eod_garbage() {
        let mut s = pack(&[(256, 9), (u16::from(b'A'), 9), (257, 9)]);
        s.push(b'\n');
        let mut w = Warnings::default();
        let (d, c) = lzw_bytes(&s, &CAP, true, Tail::Nothing, &mut w);
        assert_eq!((d.as_slice(), c), (&b"A"[..], true));
        assert_eq!(w.into_vec(), vec![Warning::TrailingGarbage]);
    }

    #[test]
    fn the_width_grows_one_code_early_by_default() {
        // 510 single-byte codes fill entries up to 511, which is where
        // /EarlyChange 1 switches to ten bits and 0 does not.
        let mut codes = vec![(256u16, 9u32)];
        for i in 0..254u16 {
            codes.push((i, 9));
        }
        // entries 258..=511 now exist: the next code is ten bits wide with
        // early change, still nine without.
        let early_on = {
            let mut c = codes.clone();
            c.push((u16::from(b'Z'), 10));
            c.push((257, 10));
            pack(&c)
        };
        let early_off = {
            let mut c = codes.clone();
            c.push((u16::from(b'Z'), 9));
            c.push((257, 10));
            pack(&c)
        };
        let mut w = Warnings::default();
        let (d, c) = lzw_bytes(&early_on, &CAP, true, Tail::WhiteSpace, &mut w);
        assert!(c, "{:?}", w.clone().into_vec());
        assert_eq!(d.last(), Some(&b'Z'));
        let mut w = Warnings::default();
        let (d, c) = lzw_bytes(&early_off, &CAP, false, Tail::WhiteSpace, &mut w);
        assert!(c);
        assert_eq!(d.last(), Some(&b'Z'));
    }

    #[test]
    fn a_full_table_without_a_clear_freezes_and_continues() {
        let mut codes = vec![(256u16, 9u32)];
        let mut width = 9u32;
        let mut next = FIRST;
        let mut first = true;
        for _ in 0..4000 {
            codes.push((0, width));
            if first {
                first = false;
            } else if (next as usize) < MAX_ENTRIES {
                next += 1;
                if width < 12 && u32::from(next) + 1 >= 1u32 << width {
                    width += 1;
                }
            }
        }
        codes.push((257, width));
        let (d, c, w) = lzw(&pack(&codes));
        assert_eq!(d.len(), 4000);
        assert!(d.iter().all(|&b| b == 0));
        assert!(c);
        assert_eq!(w, vec![Warning::LzwTableFull]);
    }

    #[test]
    fn output_cap_truncates() {
        let limits = Limits::new(3);
        let mut w = Warnings::default();
        let enc = [0x80, 0x0b, 0x60, 0x50, 0x22, 0x0c, 0x0c, 0x85, 0x01];
        let (d, c) = lzw_bytes(&enc, &limits, true, Tail::WhiteSpace, &mut w);
        assert_eq!(d, b"---");
        assert!(!c);
        assert_eq!(w.into_vec(), vec![Warning::OutputCapHit]);
    }
}
