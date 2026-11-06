//! Own deflate encoder (RFC 1951), with the zlib wrapper of RFC 1950.
//!
//! The decoder has always been here; this is the other half, and without it
//! every byte the engine wrote was uncompressed — `WriteOptions::compress` had
//! no reader, and object streams, whose whole purpose is to compress many
//! small objects together, made files *larger* than not using them.
//!
//! The strategy is deliberately modest: a hash-chain LZ77 match finder feeding
//! fixed Huffman codes, with a stored block whenever that would be smaller.
//! Dynamic Huffman tables would buy perhaps another ten per cent and cost a
//! code-length encoder, a package-merge, and a second set of edge cases; the
//! ratio that matters is "much smaller than nothing", and fixed codes reach it.
//!
//! Correctness is what is not modest: every output is checked against this
//! crate's own inflate by proptest, over generated inputs rather than chosen
//! ones.
//!
//! It once said "and against zlib in CI (ruling 9)" as well. No such check
//! ever existed in any workflow — corrected August 2026 with the ruling-13
//! sweep, and worth recording rather than quietly deleting, because a claim
//! about a check that does not run is the failure mode that sweep exists to
//! find. Under ruling 13 no such check may be added: the round trip against
//! our own inflate is what there is, and what it cannot catch is a bug the
//! encoder and the decoder share.

/// How far back a match may reach (RFC 1951 §3.2.5).
const WINDOW: usize = 32_768;
/// The longest match the format can encode.
const MAX_MATCH: usize = 258;
/// The shortest match worth emitting: a shorter one costs more than the
/// literals it replaces.
const MIN_MATCH: usize = 3;
/// How many candidates to try per position.
///
/// The hash chain is ordered nearest-first, and the nearest match encodes in
/// fewer distance bits, so a cap trades a little ratio for a bound on the
/// worst case — which matters because the input is attacker-controlled.
const MAX_CHAIN: usize = 128;
/// Buckets in the match-finder's hash table.
const HASH_SIZE: usize = 1 << 15;
/// A table entry that names no position. No offset reaches it: offsets stay
/// below [`OFFSET_LIMIT`].
const EMPTY: u32 = u32::MAX;
/// The offset no position is held at: one about to reach it moves the base.
const OFFSET_LIMIT: usize = 1 << 31;

/// The match finder's tables: `head`, the newest position of each hash
/// bucket, and the ring, the position before each one in its bucket, at slot
/// `position mod WINDOW`. Both have a fixed length, so the finder's scratch is
/// the same whatever the input's length.
///
/// A slot is overwritten only by the position a whole window later, and a
/// walk stops at the first candidate further back than the window, so every
/// slot a walk reads holds the predecessor of the candidate it reached: the
/// ring visits the candidates a table of one entry an input byte visited, in
/// the same order.
///
/// A position is held as a `u32` offset from `base`. Before an offset would
/// reach [`OFFSET_LIMIT`], the base moves forward by that less one window:
/// every entry has the move subtracted, and one it takes below zero, older
/// than the window and so beyond any walk, becomes [`EMPTY`]. An input of any
/// length never wraps a position (`docs/design/performance.md`, P-04).
struct Finder {
    head: Vec<u32>,
    ring: Vec<u32>,
    base: usize,
}

impl Finder {
    fn new() -> Finder {
        Finder {
            head: vec![EMPTY; HASH_SIZE],
            ring: vec![EMPTY; WINDOW],
            base: 0,
        }
    }

    /// The position `offset` names, unless it names none.
    fn position(&self, offset: u32) -> Option<usize> {
        (offset != EMPTY).then(|| self.base + offset as usize)
    }

    /// The newest position in `bucket`.
    fn first(&self, bucket: usize) -> Option<usize> {
        self.position(self.head[bucket])
    }

    /// The position before `position` in its bucket.
    fn next(&self, position: usize) -> Option<usize> {
        self.position(self.ring[position % WINDOW])
    }

    /// Enters `position`, which hashed to `bucket`, at the front of its chain.
    fn insert(&mut self, bucket: usize, position: usize) {
        if position - self.base >= OFFSET_LIMIT {
            self.move_base();
        }
        self.ring[position % WINDOW] = self.head[bucket];
        self.head[bucket] = (position - self.base) as u32;
    }

    /// Moves the base forward by [`OFFSET_LIMIT`] less one window.
    fn move_base(&mut self) {
        let step = (OFFSET_LIMIT - WINDOW) as u32;
        for entry in self.head.iter_mut().chain(self.ring.iter_mut()) {
            *entry = match *entry {
                offset if offset == EMPTY || offset < step => EMPTY,
                offset => offset - step,
            };
        }
        self.base += step as usize;
    }

    /// The bytes the tables hold.
    #[cfg(test)]
    fn scratch(&self) -> usize {
        (self.head.capacity() + self.ring.capacity()) * core::mem::size_of::<u32>()
    }
}

/// Writes bits least-significant-first, as deflate requires.
struct BitWriter {
    out: Vec<u8>,
    bit_buffer: u32,
    bit_count: u32,
}

impl BitWriter {
    fn new() -> BitWriter {
        BitWriter {
            out: Vec::new(),
            bit_buffer: 0,
            bit_count: 0,
        }
    }

    /// Writes `count` bits of `value`, low bit first.
    fn write(&mut self, value: u32, count: u32) {
        self.bit_buffer |= (value & ((1u32 << count) - 1)) << self.bit_count;
        self.bit_count += count;
        while self.bit_count >= 8 {
            self.out.push((self.bit_buffer & 0xFF) as u8);
            self.bit_buffer >>= 8;
            self.bit_count -= 8;
        }
    }

    /// Writes a Huffman code, which is stored most-significant-bit-first and
    /// so must be reversed on the way out (RFC 1951 §3.1.1).
    fn write_code(&mut self, code: u16, length: u32) {
        let mut reversed = 0u32;
        for i in 0..length {
            reversed |= ((u32::from(code) >> i) & 1) << (length - 1 - i);
        }
        self.write(reversed, length);
    }

    fn align(&mut self) {
        if self.bit_count > 0 {
            self.out.push((self.bit_buffer & 0xFF) as u8);
            self.bit_buffer = 0;
            self.bit_count = 0;
        }
    }

    fn finish(mut self) -> Vec<u8> {
        self.align();
        self.out
    }
}

/// The fixed literal/length code of RFC 1951 §3.2.6.
fn fixed_literal_code(symbol: u16) -> (u16, u32) {
    match symbol {
        0..=143 => (0b0011_0000 + symbol, 8),
        144..=255 => (0b1_1001_0000 + (symbol - 144), 9),
        256..=279 => (symbol - 256, 7),
        _ => (0b1100_0000 + (symbol - 280), 8),
    }
}

/// `(code, extra bits, base length)` for each length symbol, 257..=285.
const LENGTH_TABLE: [(u16, u32, usize); 29] = [
    (257, 0, 3),
    (258, 0, 4),
    (259, 0, 5),
    (260, 0, 6),
    (261, 0, 7),
    (262, 0, 8),
    (263, 0, 9),
    (264, 0, 10),
    (265, 1, 11),
    (266, 1, 13),
    (267, 1, 15),
    (268, 1, 17),
    (269, 2, 19),
    (270, 2, 23),
    (271, 2, 27),
    (272, 2, 31),
    (273, 3, 35),
    (274, 3, 43),
    (275, 3, 51),
    (276, 3, 59),
    (277, 4, 67),
    (278, 4, 83),
    (279, 4, 99),
    (280, 4, 115),
    (281, 5, 131),
    (282, 5, 163),
    (283, 5, 195),
    (284, 5, 227),
    (285, 0, 258),
];

/// `(code, extra bits, base distance)` for each distance symbol.
const DISTANCE_TABLE: [(u16, u32, usize); 30] = [
    (0, 0, 1),
    (1, 0, 2),
    (2, 0, 3),
    (3, 0, 4),
    (4, 1, 5),
    (5, 1, 7),
    (6, 2, 9),
    (7, 2, 13),
    (8, 3, 17),
    (9, 3, 25),
    (10, 4, 33),
    (11, 4, 49),
    (12, 5, 65),
    (13, 5, 97),
    (14, 6, 129),
    (15, 6, 193),
    (16, 7, 257),
    (17, 7, 385),
    (18, 8, 513),
    (19, 8, 769),
    (20, 9, 1025),
    (21, 9, 1537),
    (22, 10, 2049),
    (23, 10, 3073),
    (24, 11, 4097),
    (25, 11, 6145),
    (26, 12, 8193),
    (27, 12, 12_289),
    (28, 13, 16_385),
    (29, 13, 24_577),
];

fn length_symbol(length: usize) -> (u16, u32, usize) {
    // Walk from the top so the largest base that fits is chosen.
    for entry in LENGTH_TABLE.iter().rev() {
        if length >= entry.2 {
            return *entry;
        }
    }
    LENGTH_TABLE[0]
}

fn distance_symbol(distance: usize) -> (u16, u32, usize) {
    for entry in DISTANCE_TABLE.iter().rev() {
        if distance >= entry.2 {
            return *entry;
        }
    }
    DISTANCE_TABLE[0]
}

/// Three bytes hashed into a bucket. Collisions are fine: a candidate is
/// always verified before it is used.
fn hash3(data: &[u8], at: usize) -> usize {
    let a = u32::from(data.get(at).copied().unwrap_or(0));
    let b = u32::from(data.get(at + 1).copied().unwrap_or(0));
    let c = u32::from(data.get(at + 2).copied().unwrap_or(0));
    (((a << 10) ^ (b << 5) ^ c).wrapping_mul(2_654_435_761) >> 17) as usize % HASH_SIZE
}

/// Compresses to a raw deflate stream (RFC 1951).
#[must_use]
pub fn deflate(input: &[u8]) -> Vec<u8> {
    // Incompressible input is common — an already-compressed image, a JPEG
    // passed through — and a stored block is the honest answer for it.
    let compressed = deflate_fixed(input);
    let stored = stored_blocks(input);
    if stored.len() <= compressed.len() {
        stored
    } else {
        compressed
    }
}

/// Wraps a deflate stream in the zlib header and Adler-32 of RFC 1950.
///
/// This is what a PDF `/FlateDecode` stream contains: the decoder accepts a
/// raw stream too, because real files contain both, but what we *write* is
/// the one the specification asks for.
#[must_use]
pub fn zlib_compress(input: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    // CMF: deflate, 32K window. FLG: no dictionary, default level, with the
    // check bits chosen so the pair is a multiple of 31.
    out.push(0x78);
    out.push(0x9C);
    out.extend_from_slice(&deflate(input));
    out.extend_from_slice(&adler32(input).to_be_bytes());
    out
}

/// RFC 1950 §9.
fn adler32(data: &[u8]) -> u32 {
    const MOD: u32 = 65_521;
    let (mut a, mut b) = (1u32, 0u32);
    for chunk in data.chunks(5552) {
        for byte in chunk {
            a += u32::from(*byte);
            b += a;
        }
        a %= MOD;
        b %= MOD;
    }
    (b << 16) | a
}

/// Stored (uncompressed) blocks, which cost five bytes per 65535.
fn stored_blocks(input: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    if input.is_empty() {
        // An empty final stored block, so the stream is still well formed.
        out.extend_from_slice(&[0x01, 0x00, 0x00, 0xFF, 0xFF]);
        return out;
    }

    let mut chunks = input.chunks(65_535).peekable();
    while let Some(chunk) = chunks.next() {
        let last = u8::from(chunks.peek().is_none());
        out.push(last);
        let len = chunk.len() as u16;
        out.extend_from_slice(&len.to_le_bytes());
        out.extend_from_slice(&(!len).to_le_bytes());
        out.extend_from_slice(chunk);
    }
    out
}

/// One fixed-Huffman block over the whole input.
fn deflate_fixed(input: &[u8]) -> Vec<u8> {
    let mut w = BitWriter::new();
    // Final block, fixed Huffman codes.
    w.write(1, 1);
    w.write(0b01, 2);

    // Each bucket's positions, a chain ordered nearest-first.
    let mut finder = Finder::new();
    #[cfg(test)]
    tests::note_scratch(&finder);

    let mut at = 0usize;
    while at < input.len() {
        let (mut best_len, mut best_dist) = (0usize, 0usize);

        if at + MIN_MATCH <= input.len() {
            let bucket = hash3(input, at);
            let mut next = finder.first(bucket);
            let limit = at.saturating_sub(WINDOW);
            let mut tries = 0usize;

            while let Some(candidate) = next.filter(|&c| c >= limit && tries < MAX_CHAIN) {
                tries += 1;
                let max_len = MAX_MATCH.min(input.len() - at);
                let mut len = 0usize;
                while len < max_len && input[candidate + len] == input[at + len] {
                    len += 1;
                }
                if len > best_len {
                    best_len = len;
                    best_dist = at - candidate;
                    // A longest-possible match cannot be improved on.
                    if len == max_len {
                        break;
                    }
                }
                next = finder.next(candidate);
            }
        }

        if best_len >= MIN_MATCH {
            let (sym, extra_bits, base) = length_symbol(best_len);
            let (code, bits) = fixed_literal_code(sym);
            w.write_code(code, bits);
            if extra_bits > 0 {
                w.write((best_len - base) as u32, extra_bits);
            }

            let (dsym, dextra, dbase) = distance_symbol(best_dist);
            // Distance codes are five-bit fixed, not the literal code.
            w.write_code(dsym, 5);
            if dextra > 0 {
                w.write((best_dist - dbase) as u32, dextra);
            }

            // Every position the match covers still has to enter the chain, or
            // later matches cannot reach back through it.
            for i in 0..best_len {
                let pos = at + i;
                if pos + MIN_MATCH <= input.len() {
                    finder.insert(hash3(input, pos), pos);
                }
            }
            at += best_len;
        } else {
            let (code, bits) = fixed_literal_code(u16::from(input[at]));
            w.write_code(code, bits);
            if at + MIN_MATCH <= input.len() {
                finder.insert(hash3(input, at), at);
            }
            at += 1;
        }
    }

    // End of block.
    let (code, bits) = fixed_literal_code(256);
    w.write_code(code, bits);
    w.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{flate_decode, Limits};
    use std::cell::Cell;

    thread_local! {
        /// The bytes the last finder this thread built holds.
        static SCRATCH: Cell<usize> = const { Cell::new(0) };
    }

    /// Notes what `finder` holds, for the test that reads it.
    pub(super) fn note_scratch(finder: &Finder) {
        SCRATCH.with(|scratch| scratch.set(finder.scratch()));
    }

    fn round_trip(input: &[u8]) {
        let compressed = zlib_compress(input);
        let decoded = flate_decode(&compressed, &Limits::new(1 << 24), None)
            .expect("it decodes")
            .data;
        assert_eq!(decoded, input, "round trip changed the bytes");
    }

    #[test]
    fn empty_input_round_trips() {
        round_trip(b"");
    }

    #[test]
    fn a_short_literal_run_round_trips() {
        round_trip(b"hello");
    }

    /// The case the encoder exists for: repetition compresses.
    #[test]
    fn repetition_round_trips_and_shrinks() {
        let input = b"the quick brown fox ".repeat(200);
        round_trip(&input);

        let compressed = zlib_compress(&input);
        assert!(
            compressed.len() < input.len() / 4,
            "4000 bytes of repetition compressed to {}",
            compressed.len()
        );
    }

    /// A match at the maximum length and one at the minimum both encode.
    #[test]
    fn extreme_match_lengths_round_trip() {
        round_trip(&[0x41u8; 300]);
        round_trip(b"abcabc");
    }

    /// A match at the far end of the window exercises the largest distance
    /// codes, which have thirteen extra bits.
    #[test]
    fn a_distant_match_round_trips() {
        let mut input = Vec::new();
        input.extend_from_slice(b"marker-sequence-here");
        input.extend_from_slice(&vec![b'x'; 30_000]);
        input.extend_from_slice(b"marker-sequence-here");
        round_trip(&input);
    }

    /// Incompressible input must not come out larger than a stored block.
    #[test]
    fn random_input_never_inflates_much() {
        // A deterministic pseudo-random sequence: no crate, no seed drift.
        let mut state = 0x2545_F491_4F6C_DD1Du64;
        let input: Vec<u8> = (0..4096)
            .map(|_| {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                state as u8
            })
            .collect();

        round_trip(&input);
        let compressed = zlib_compress(&input);
        assert!(
            compressed.len() <= input.len() + input.len() / 100 + 16,
            "{} bytes became {}",
            input.len(),
            compressed.len()
        );
    }

    #[test]
    fn every_byte_value_round_trips() {
        let input: Vec<u8> = (0..=255u8).collect();
        round_trip(&input);
    }

    /// RFC 1950's header must be one a decoder accepts: the two bytes are a
    /// multiple of 31.
    #[test]
    fn the_zlib_header_checksums() {
        let out = zlib_compress(b"anything");
        let header = (u16::from(out[0]) << 8) | u16::from(out[1]);
        assert_eq!(header % 31, 0);
    }

    /// The Adler-32 trailer is what tells a decoder the data survived.
    #[test]
    fn the_adler_trailer_matches() {
        // RFC 1950's own worked value for "Mark Adler".
        assert_eq!(adler32(b"Mark Adler"), 0x13070394);
    }

    #[test]
    fn long_input_round_trips() {
        let input: Vec<u8> = (0..100_000u32).map(|i| (i % 251) as u8).collect();
        round_trip(&input);
    }

    /// The encoder as it was before the ring, its chain one entry an input
    /// byte, kept to hold the ring to the bytes it gave.
    fn deflate_fixed_by_vector(input: &[u8]) -> Vec<u8> {
        let mut w = BitWriter::new();
        w.write(1, 1);
        w.write(0b01, 2);
        let mut head = vec![usize::MAX; HASH_SIZE];
        let mut prev = vec![usize::MAX; input.len()];
        let mut at = 0usize;
        while at < input.len() {
            let (mut best_len, mut best_dist) = (0usize, 0usize);
            if at + MIN_MATCH <= input.len() {
                let bucket = hash3(input, at);
                let mut candidate = head[bucket];
                let limit = at.saturating_sub(WINDOW);
                let mut tries = 0usize;
                while candidate != usize::MAX && candidate >= limit && tries < MAX_CHAIN {
                    tries += 1;
                    let max_len = MAX_MATCH.min(input.len() - at);
                    let mut len = 0usize;
                    while len < max_len && input[candidate + len] == input[at + len] {
                        len += 1;
                    }
                    if len > best_len {
                        best_len = len;
                        best_dist = at - candidate;
                        if len == max_len {
                            break;
                        }
                    }
                    candidate = prev[candidate];
                }
            }
            if best_len >= MIN_MATCH {
                let (sym, extra_bits, base) = length_symbol(best_len);
                let (code, bits) = fixed_literal_code(sym);
                w.write_code(code, bits);
                if extra_bits > 0 {
                    w.write((best_len - base) as u32, extra_bits);
                }
                let (dsym, dextra, dbase) = distance_symbol(best_dist);
                w.write_code(dsym, 5);
                if dextra > 0 {
                    w.write((best_dist - dbase) as u32, dextra);
                }
                for i in 0..best_len {
                    let pos = at + i;
                    if pos + MIN_MATCH <= input.len() {
                        let bucket = hash3(input, pos);
                        prev[pos] = head[bucket];
                        head[bucket] = pos;
                    }
                }
                at += best_len;
            } else {
                let (code, bits) = fixed_literal_code(u16::from(input[at]));
                w.write_code(code, bits);
                if at + MIN_MATCH <= input.len() {
                    let bucket = hash3(input, at);
                    prev[at] = head[bucket];
                    head[bucket] = at;
                }
                at += 1;
            }
        }
        let (code, bits) = fixed_literal_code(256);
        w.write_code(code, bits);
        w.finish()
    }

    /// `len` deterministic pseudo-random bytes.
    fn noise(len: usize, mut state: u64) -> Vec<u8> {
        (0..len)
            .map(|_| {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                state as u8
            })
            .collect()
    }

    /// The ring visits the candidates the vector visited, in its order, so the
    /// compressed bytes are the same: on noise, on repetition, and on blocks a
    /// window long, one less and one more, repeated to four windows, whose
    /// matches sit at the window's edge and just past it.
    #[test]
    fn the_ring_finds_what_the_vector_found() {
        let mut inputs = vec![
            noise(3 * WINDOW, 0x2545_F491_4F6C_DD1D),
            b"the quick brown fox ".repeat(10_000),
        ];
        for block in [WINDOW - 1, WINDOW, WINDOW + 1] {
            inputs.push(noise(block, block as u64).repeat(4 * WINDOW / block + 1));
        }
        for input in &inputs {
            let ring = deflate_fixed(input);
            assert_eq!(
                ring,
                deflate_fixed_by_vector(input),
                "{} bytes compressed differently",
                input.len()
            );
            round_trip(input);
        }
    }

    /// The finder's tables are the same size for 64 KiB of input and for
    /// 64 MiB: two tables of one window's entries, and nothing an input byte.
    #[test]
    fn the_match_finder_keeps_a_fixed_ring_whatever_the_input_length() {
        let mut held = Vec::new();
        for len in [64 << 10, 64 << 20] {
            let input: Vec<u8> = (0..len).map(|i| (i * 7 + i / 251) as u8).collect();
            let _ = deflate(&input);
            held.push(SCRATCH.with(Cell::get));
        }
        assert_eq!(held[0], held[1], "the scratch grew with the input");
        assert_eq!(held[0], 2 * WINDOW * 4, "two tables of a window's u32s");
    }

    /// A base that moves keeps every position within the window and empties
    /// what is older: positions entered either side of the offset limit read
    /// back as they were entered, and the oldest is gone.
    #[test]
    fn a_moved_base_keeps_the_window_and_empties_what_is_older() {
        let mut finder = Finder::new();
        let start = OFFSET_LIMIT - 8;
        let old = start - WINDOW;
        finder.insert(0, old);
        for position in start..start + 16 {
            finder.insert(0, position);
        }
        let mut seen = Vec::new();
        let mut next = finder.first(0);
        while let Some(position) = next {
            seen.push(position);
            next = finder.next(position);
        }
        let want: Vec<usize> = (start..start + 16).rev().collect();
        assert_eq!(
            seen, want,
            "the window's positions, newest first, and not the old one"
        );
        assert_eq!(finder.base, OFFSET_LIMIT - WINDOW, "the base moved once");
    }
}
