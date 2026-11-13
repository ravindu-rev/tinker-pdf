//! Own deflate encoder (RFC 1951), with the zlib wrapper of RFC 1950.
//!
//! Two encoders live here, both hash-chain LZ77 over one window.
//!
//! [`deflate`] and [`zlib_compress`] compress every stream the writer emits:
//! one fixed-Huffman block over the whole input, or stored blocks wherever
//! those are smaller.
//!
//! [`Deflater`] is the streaming one (`docs/design/performance.md`, P-04):
//! input is fed in pieces into a window buffer of two windows, and whole
//! blocks are appended to the caller's output as they complete, each written
//! stored or fixed-coded, whichever is fewer bits. It runs at one of two
//! levels ([`DeflateLevel`]): fast, greedy over a short chain, or ratio, over
//! a long chain with a lazy step. [`ZlibDeflater`] wraps it in RFC 1950's
//! header and a running Adler-32, and [`deflate_at`] and [`zlib_compress_at`]
//! feed it a whole input at once.
//!
//! **Arrival is not an input** (ruling 4). A block ends at
//! [`BLOCK_SYMBOLS`] symbols or at the end of the input, a position is
//! searched only with [`LOOKAHEAD`] bytes ahead of it or at the end, and the
//! buffer slides only when it is full and more input waits, so the bytes
//! written are a function of the input alone, never of how it was cut into
//! writes.
//!
//! Correctness is checked against this crate's own inflate, over generated
//! inputs rather than chosen ones. No other deflate decides it (ruling 13), so
//! what the round trip cannot catch is a bug the encoder and the decoder
//! share.

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
/// The bytes a position is searched with ahead of it, short of the input's
/// end: a match's reach forward, a hash's three, and the next position's.
const LOOKAHEAD: usize = MAX_MATCH + MIN_MATCH + 1;
/// The streaming deflater's window buffer: two windows, so a match may always
/// reach a whole window back from anywhere a position is searched.
const BUFFER: usize = 2 * WINDOW;
/// The symbols, literals and matches, a block holds before it is written.
const BLOCK_SYMBOLS: usize = 16_384;
/// How far back a three-byte match may be at the ratio level: further, its
/// distance's extra bits cost more than its three literals do.
const TOO_FAR: usize = 4_096;
/// The longest stored block (RFC 1951 §3.2.4): `LEN` is sixteen bits.
const STORED_MAX: usize = 65_535;

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
    let byte = |i: usize| data.get(at + i).copied().unwrap_or(0);
    hash(byte(0), byte(1), byte(2))
}

/// The bucket of the three bytes `a`, `b` and `c`.
fn hash(a: u8, b: u8, c: u8) -> usize {
    let (a, b, c) = (u32::from(a), u32::from(b), u32::from(c));
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
    let mut adler = Adler32::new();
    adler.update(data);
    adler.value()
}

/// RFC 1950 §9's checksum, kept as it runs: the two sums are reduced at least
/// every 5 552 bytes, the most that cannot overflow a `u32`, so pieces of any
/// size give the value the whole input gives.
#[derive(Clone, Copy)]
struct Adler32 {
    a: u32,
    b: u32,
}

impl Adler32 {
    const MOD: u32 = 65_521;

    fn new() -> Adler32 {
        Adler32 { a: 1, b: 0 }
    }

    fn update(&mut self, data: &[u8]) {
        for chunk in data.chunks(5552) {
            for &byte in chunk {
                self.a += u32::from(byte);
                self.b += self.a;
            }
            self.a %= Self::MOD;
            self.b %= Self::MOD;
        }
    }

    fn value(self) -> u32 {
        (self.b << 16) | self.a
    }
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

// ---- The streaming deflater -------------------------------------------------

/// How hard a [`Deflater`] looks for matches (`docs/design/performance.md`,
/// P-04, "The two levels"). Both levels are the same hash-chain LZ77 over the
/// same window, inserting every position a match covers; they differ in how
/// far a position searches and whether it looks one byte on. The numbers are
/// level 1's and level 6's published parameters, taken as numbers.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DeflateLevel {
    /// Greedy: four candidates a position, and a search ends at a match of 8.
    /// For output written once and read once, a rendered page among them.
    Fast,
    /// 128 candidates a position, a quarter of them once the match in hand
    /// reaches 8, and a search ends at a match of 128; a match shorter than
    /// 16 is held while the next position searches, and a longer match one
    /// byte on wins, the byte before it a literal. A three-byte match further
    /// back than 4 096 bytes is dropped. For documents, written once and read
    /// many times.
    Ratio,
}

/// A level's numbers.
#[derive(Clone, Copy)]
struct Params {
    /// The candidates a position tries.
    chain: usize,
    /// The match in hand at which a position tries a quarter of them.
    good: usize,
    /// The length that ends a search.
    nice: usize,
    /// A match in hand shorter than this waits while the next position
    /// searches; at none, the level is greedy.
    lazy: usize,
}

impl DeflateLevel {
    const fn params(self) -> Params {
        match self {
            Self::Fast => Params {
                chain: 4,
                good: MAX_MATCH,
                nice: 8,
                lazy: 0,
            },
            Self::Ratio => Params {
                chain: MAX_CHAIN,
                good: 8,
                nice: 128,
                lazy: 16,
            },
        }
    }
}

/// A match: its length and how far back it reaches.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Match {
    len: usize,
    dist: usize,
}

/// Each match length's index into [`LENGTH_TABLE`], from 3 to 258.
const LENGTH_INDEX: [u8; MAX_MATCH + 1] = length_index();

const fn length_index() -> [u8; MAX_MATCH + 1] {
    let mut index = [0u8; MAX_MATCH + 1];
    let mut i = 0;
    while i < LENGTH_TABLE.len() {
        let (_, extra, base) = LENGTH_TABLE[i];
        let mut length = base;
        // Symbol 284's extra bits reach 258, which is 285's alone: 285 comes
        // last and takes it.
        while length < base + (1 << extra) && length <= MAX_MATCH {
            index[length] = i as u8;
            length += 1;
        }
        i += 1;
    }
    index
}

/// Each distance from 1 to 256's index into [`DISTANCE_TABLE`], by
/// `distance - 1`.
const DISTANCE_LOW: [u8; 256] = distance_low();
/// Each distance past 256's index into [`DISTANCE_TABLE`], by
/// `(distance - 1) >> 7`: every code past 256 spans a multiple of 128.
const DISTANCE_HIGH: [u8; 256] = distance_high();

const fn distance_low() -> [u8; 256] {
    let mut index = [0u8; 256];
    let mut i = 0;
    while i < DISTANCE_TABLE.len() {
        let (_, extra, base) = DISTANCE_TABLE[i];
        let mut distance = base;
        while distance < base + (1 << extra) && distance <= 256 {
            index[distance - 1] = i as u8;
            distance += 1;
        }
        i += 1;
    }
    index
}

const fn distance_high() -> [u8; 256] {
    let mut index = [0u8; 256];
    let mut i = 0;
    while i < DISTANCE_TABLE.len() {
        let (_, extra, base) = DISTANCE_TABLE[i];
        if base > 256 {
            let mut distance = base;
            while distance < base + (1 << extra) {
                index[(distance - 1) >> 7] = i as u8;
                distance += 128;
            }
        }
        i += 1;
    }
    index
}

/// `distance`'s index into [`DISTANCE_TABLE`], for a distance from 1 to the
/// window.
fn distance_index(distance: usize) -> usize {
    let index = if distance <= 256 {
        DISTANCE_LOW[distance - 1]
    } else {
        DISTANCE_HIGH[(distance - 1) >> 7]
    };
    usize::from(index)
}

/// `code`'s low `length` bits in the other order: a Huffman code is packed
/// from its most significant bit (RFC 1951 §3.1.1), and the writer writes
/// from the least.
const fn reversed(code: u16, length: u8) -> u16 {
    let mut out = 0u16;
    let mut i = 0;
    while i < length {
        out |= ((code >> i) & 1) << (length - 1 - i);
        i += 1;
    }
    out
}

/// A code as the writer writes it: reversed, and its length.
type Code = (u16, u8);

/// The fixed literal/length code of RFC 1951 §3.2.6, reversed for writing.
const FIXED_LITERALS: [Code; 288] = fixed_literals();
/// The fixed distance code: five bits, reversed for writing.
const FIXED_DISTANCES: [Code; 30] = fixed_distances();

const fn fixed_literals() -> [Code; 288] {
    let mut codes = [(0u16, 0u8); 288];
    let mut symbol = 0u16;
    while symbol < 288 {
        let (code, length) = match symbol {
            0..=143 => (0b0011_0000 + symbol, 8),
            144..=255 => (0b1_1001_0000 + (symbol - 144), 9),
            256..=279 => (symbol - 256, 7),
            _ => (0b1100_0000 + (symbol - 280), 8),
        };
        codes[symbol as usize] = (reversed(code, length), length);
        symbol += 1;
    }
    codes
}

const fn fixed_distances() -> [Code; 30] {
    let mut codes = [(0u16, 0u8); 30];
    let mut symbol = 0;
    while symbol < 30 {
        codes[symbol] = (reversed(symbol as u16, 5), 5);
        symbol += 1;
    }
    codes
}

/// The streaming writer's bits, low bit first, into the caller's output: at
/// most 31 are held between writes, fewer than 8 between blocks.
#[derive(Default)]
struct Bits {
    held: u64,
    count: u32,
}

impl Bits {
    /// Writes the low `count` bits of `value`, `count` at most 16.
    fn put(&mut self, out: &mut Vec<u8>, value: u32, count: u32) {
        self.held |= (u64::from(value) & ((1u64 << count) - 1)) << self.count;
        self.count += count;
        if self.count >= 32 {
            out.extend_from_slice(&(self.held as u32).to_le_bytes());
            self.held >>= 32;
            self.count -= 32;
        }
    }

    /// Writes a code.
    fn code(&mut self, out: &mut Vec<u8>, (code, length): Code) {
        self.put(out, u32::from(code), u32::from(length));
    }

    /// Writes the whole bytes held, keeping fewer than eight bits.
    fn bytes(&mut self, out: &mut Vec<u8>) {
        while self.count >= 8 {
            out.push(self.held as u8);
            self.held >>= 8;
            self.count -= 8;
        }
    }

    /// Pads to a byte and writes everything held.
    fn align(&mut self, out: &mut Vec<u8>) {
        self.bytes(out);
        if self.count > 0 {
            out.push(self.held as u8);
            self.held = 0;
            self.count = 0;
        }
    }
}

/// The block being gathered: its symbols, their counts and the input it
/// covers.
struct Block {
    /// Each symbol, its value in the high half — a literal's byte or a
    /// match's length — and a match's distance in the low half, zero for a
    /// literal.
    symbols: Vec<u32>,
    /// Each literal/length symbol's count; 256, the end of the block, is
    /// counted when it is written.
    literals: [u32; 286],
    /// Each distance symbol's count.
    distances: [u32; 30],
    /// The position of the block's first byte.
    start: usize,
    /// The input bytes the block's symbols cover.
    len: usize,
}

impl Block {
    fn new() -> Block {
        Block {
            symbols: Vec::new(),
            literals: [0; 286],
            distances: [0; 30],
            start: 0,
            len: 0,
        }
    }

    fn literal(&mut self, byte: u8) {
        self.symbols.push(u32::from(byte) << 16);
        self.literals[usize::from(byte)] += 1;
        self.len += 1;
    }

    fn matched(&mut self, found: Match) {
        // A length is at most 258 and a distance at most 32 768: both fit
        // their sixteen bits.
        self.symbols
            .push(((found.len as u32) << 16) | found.dist as u32);
        self.literals[257 + usize::from(LENGTH_INDEX[found.len])] += 1;
        self.distances[distance_index(found.dist)] += 1;
        self.len += found.len;
    }

    fn full(&self) -> bool {
        self.symbols.len() == BLOCK_SYMBOLS
    }

    /// Empties the block, which starts where it ended.
    fn reset(&mut self) {
        self.symbols.clear();
        self.literals = [0; 286];
        self.distances = [0; 30];
        self.start += self.len;
        self.len = 0;
    }
}

/// The bits a block costs fixed-coded, header and end included.
fn fixed_bits(block: &Block) -> u64 {
    let mut bits = 3 + u64::from(FIXED_LITERALS[256].1);
    for (symbol, &count) in block.literals.iter().enumerate() {
        let extra = match symbol.checked_sub(257) {
            Some(index) => LENGTH_TABLE[index].1,
            None => 0,
        };
        bits += u64::from(count) * u64::from(u32::from(FIXED_LITERALS[symbol].1) + extra);
    }
    for (index, &count) in block.distances.iter().enumerate() {
        bits += u64::from(count) * u64::from(5 + DISTANCE_TABLE[index].1);
    }
    bits
}

/// The bits `len` bytes cost as stored blocks, written after `pending` bits
/// of a byte: each block's three header bits, its padding to a byte, its
/// `LEN` and `NLEN`, and its bytes.
fn stored_bits(len: usize, pending: u32) -> u64 {
    let blocks = len.div_ceil(STORED_MAX).max(1) as u64;
    let first = 3 + (8 - (pending + 3) % 8) % 8 + 32;
    u64::from(first) + (blocks - 1) * 40 + 8 * len as u64
}

/// Writes `data` as stored blocks, the last final if `last` is.
fn write_stored(bits: &mut Bits, out: &mut Vec<u8>, data: &[u8], last: bool) {
    let mut rest = data;
    loop {
        let (now, after) = rest.split_at(rest.len().min(STORED_MAX));
        bits.put(out, u32::from(last && after.is_empty()), 1);
        bits.put(out, 0b00, 2);
        bits.align(out);
        // At most `STORED_MAX`, so it fits.
        let len = now.len() as u16;
        out.extend_from_slice(&len.to_le_bytes());
        out.extend_from_slice(&(!len).to_le_bytes());
        out.extend_from_slice(now);
        rest = after;
        if rest.is_empty() {
            return;
        }
    }
}

/// Writes `block`'s symbols and its end in `literals` and `distances`.
fn write_symbols(
    bits: &mut Bits,
    out: &mut Vec<u8>,
    block: &Block,
    literals: &[Code],
    distances: &[Code],
) {
    for &symbol in &block.symbols {
        let (value, dist) = ((symbol >> 16) as usize, (symbol & 0xFFFF) as usize);
        if dist == 0 {
            bits.code(out, literals[value]);
            continue;
        }
        let index = usize::from(LENGTH_INDEX[value]);
        let (_, extra, base) = LENGTH_TABLE[index];
        bits.code(out, literals[257 + index]);
        if extra > 0 {
            bits.put(out, (value - base) as u32, extra);
        }
        let index = distance_index(dist);
        let (_, extra, base) = DISTANCE_TABLE[index];
        bits.code(out, distances[index]);
        if extra > 0 {
            bits.put(out, (dist - base) as u32, extra);
        }
    }
    bits.code(out, literals[256]);
}

/// The length of the run `here` and `there` share, both the same length.
fn common(here: &[u8], there: &[u8]) -> usize {
    let mut len = 0;
    for (a, b) in here.chunks_exact(8).zip(there.chunks_exact(8)) {
        let word = |bytes: &[u8]| <[u8; 8]>::try_from(bytes).map_or(0, u64::from_le_bytes);
        let differ = word(a) ^ word(b);
        if differ != 0 {
            // Little-endian: the first byte that differs is the lowest.
            return len + (differ.trailing_zeros() / 8) as usize;
        }
        len += 8;
    }
    len + here[len..]
        .iter()
        .zip(&there[len..])
        .take_while(|(a, b)| a == b)
        .count()
}

/// A raw deflate stream (RFC 1951), written as its input arrives.
///
/// [`Deflater::write`] takes input in pieces of any size and appends each
/// block to `out` as it completes; [`Deflater::finish`] writes the rest, the
/// last block final and the stream padded to a byte. The bytes written are
/// the same however the input is cut into writes. It keeps at most a window
/// buffer of two windows of input, 64 KiB, and a block of symbols beside the
/// finder's two tables; `out` is the caller's.
pub struct Deflater {
    level: DeflateLevel,
    params: Params,
    finder: Finder,
    /// The input from `start` on: the window behind the scan and what has
    /// arrived ahead of it, at most [`BUFFER`] bytes.
    buffer: Vec<u8>,
    /// The position of the buffer's first byte.
    start: usize,
    /// The next position to search.
    scan: usize,
    /// The ratio level's match found at `scan - 1`, held while `scan`
    /// searches.
    held: Option<Match>,
    /// The ratio level's byte at `scan - 1` is not written yet: a literal,
    /// or the start of the match held.
    waiting: bool,
    block: Block,
    bits: Bits,
}

impl Deflater {
    /// A deflater at `level`.
    #[must_use]
    pub fn new(level: DeflateLevel) -> Deflater {
        Deflater {
            level,
            params: level.params(),
            finder: Finder::new(),
            buffer: Vec::new(),
            start: 0,
            scan: 0,
            held: None,
            waiting: false,
            block: Block::new(),
            bits: Bits::default(),
        }
    }

    /// Takes `input`, appending to `out` each block it completes.
    pub fn write(&mut self, mut input: &[u8], out: &mut Vec<u8>) {
        while !input.is_empty() {
            if self.buffer.len() == BUFFER {
                self.slide();
            }
            let take = (BUFFER - self.buffer.len()).min(input.len());
            let (now, rest) = input.split_at(take);
            self.reserve(take);
            self.buffer.extend_from_slice(now);
            input = rest;
            self.run(out, false);
        }
    }

    /// Writes what is left: the last block, final, and the stream padded to
    /// a byte.
    pub fn finish(mut self, out: &mut Vec<u8>) {
        self.drain(out);
        self.flush(out, true);
        self.bits.align(out);
    }

    /// Searches every position left and writes the byte still waiting, so
    /// that every symbol is in a block.
    fn drain(&mut self, out: &mut Vec<u8>) {
        self.run(out, true);
        if self.waiting {
            self.waiting = false;
            let byte = self.buffer[self.scan - 1 - self.start];
            self.literal(out, byte);
        }
    }

    /// The position one past the last byte that has arrived.
    fn end(&self) -> usize {
        self.start + self.buffer.len()
    }

    /// Grows the buffer to hold `more` bytes, by powers of two up to
    /// [`BUFFER`], so a short input holds a short buffer.
    fn reserve(&mut self, more: usize) {
        let need = self.buffer.len() + more;
        if need > self.buffer.capacity() {
            let size = need.next_power_of_two().min(BUFFER);
            self.buffer.reserve_exact(size - self.buffer.len());
        }
    }

    /// Drops what is more than a window behind the scan. The buffer is full
    /// and the scan within [`LOOKAHEAD`] of its end, so more than a window is
    /// behind it and the drop makes room.
    fn slide(&mut self) {
        let keep = self.scan.saturating_sub(WINDOW).max(self.start);
        let drop = keep - self.start;
        self.buffer.copy_within(drop.., 0);
        self.buffer.truncate(self.buffer.len() - drop);
        self.start = keep;
    }

    /// Searches positions while each has [`LOOKAHEAD`] bytes ahead of it, or,
    /// `finishing`, any.
    fn run(&mut self, out: &mut Vec<u8>, finishing: bool) {
        let reach = if finishing { 1 } else { LOOKAHEAD };
        while self.end() - self.scan >= reach {
            match self.level {
                DeflateLevel::Fast => self.greedy(out),
                DeflateLevel::Ratio => self.lazy(out),
            }
        }
    }

    /// The bucket of the position `at`, unless too few bytes follow it.
    fn bucket(&self, at: usize) -> Option<usize> {
        let i = at - self.start;
        match self.buffer.get(i..i + MIN_MATCH)? {
            &[a, b, c] => Some(hash(a, b, c)),
            _ => None,
        }
    }

    /// Enters the positions from `from` up to `to` that a hash reaches.
    fn insert(&mut self, from: usize, to: usize) {
        for position in from..to {
            match self.bucket(position) {
                Some(bucket) => self.finder.insert(bucket, position),
                None => return,
            }
        }
    }

    /// One position at the fast level: its longest match, or its byte.
    fn greedy(&mut self, out: &mut Vec<u8>) {
        let at = self.scan;
        let found = self.bucket(at).and_then(|bucket| {
            let first = self.finder.first(bucket);
            let found = self.longest(first, at, MIN_MATCH - 1, self.params.chain);
            self.finder.insert(bucket, at);
            found
        });
        match found {
            Some(found) => {
                self.matched(out, found);
                self.insert(at + 1, at + found.len);
                self.scan = at + found.len;
            }
            None => {
                let byte = self.buffer[at - self.start];
                self.literal(out, byte);
                self.scan = at + 1;
            }
        }
    }

    /// One position at the ratio level: a match held from the position before
    /// is written unless this one finds a longer, which is held in its place
    /// and the byte before it written as a literal.
    fn lazy(&mut self, out: &mut Vec<u8>) {
        let at = self.scan;
        let in_hand = self.held.map_or(MIN_MATCH - 1, |held| held.len);
        let found = self.bucket(at).and_then(|bucket| {
            let found = if in_hand < self.params.lazy {
                let chain = if in_hand >= self.params.good {
                    self.params.chain / 4
                } else {
                    self.params.chain
                };
                let first = self.finder.first(bucket);
                self.longest(first, at, in_hand, chain)
                    .filter(|found| found.len > MIN_MATCH || found.dist <= TOO_FAR)
            } else {
                None
            };
            self.finder.insert(bucket, at);
            found
        });
        match (self.held, found) {
            (Some(held), None) => {
                // The match from `at - 1` stands; `at - 1` and `at` are in
                // the chain already.
                self.matched(out, held);
                let past = at - 1 + held.len;
                self.insert(at + 1, past);
                self.scan = past;
                self.held = None;
                self.waiting = false;
            }
            (_, found) => {
                if self.waiting {
                    let byte = self.buffer[at - 1 - self.start];
                    self.literal(out, byte);
                }
                self.waiting = true;
                self.held = found;
                self.scan = at + 1;
            }
        }
    }

    /// The longest match for the bytes at `at` longer than `best`, among at
    /// most `chain` candidates from `first` within the window; the search
    /// ends at the level's `nice` length. Of two the same length the nearer,
    /// which the chain visits first, is kept.
    fn longest(&self, first: Option<usize>, at: usize, best: usize, chain: usize) -> Option<Match> {
        let most = MAX_MATCH.min(self.end() - at);
        if most <= best {
            return None;
        }
        let nice = self.params.nice.min(most);
        let from = at - self.start;
        let here = &self.buffer[from..from + most];
        // The window behind the scan is held whole (`slide`), so every
        // candidate a walk accepts is in the buffer.
        let limit = at.saturating_sub(WINDOW);
        let (mut best_len, mut best_dist) = (best, 0);
        let mut next = first;
        let mut tries = 0;
        while let Some(candidate) = next.filter(|&candidate| candidate >= limit) {
            if tries == chain {
                break;
            }
            tries += 1;
            let from = candidate - self.start;
            let there = &self.buffer[from..from + most];
            // Only a candidate that agrees at `best_len` can be longer.
            if there[best_len] == here[best_len] {
                let len = common(here, there);
                if len > best_len {
                    best_len = len;
                    best_dist = at - candidate;
                    if len >= nice {
                        break;
                    }
                }
            }
            next = self.finder.next(candidate);
        }
        (best_dist != 0).then_some(Match {
            len: best_len,
            dist: best_dist,
        })
    }

    fn literal(&mut self, out: &mut Vec<u8>, byte: u8) {
        self.block.literal(byte);
        if self.block.full() {
            self.flush(out, false);
        }
    }

    fn matched(&mut self, out: &mut Vec<u8>, found: Match) {
        self.block.matched(found);
        if self.block.full() {
            self.flush(out, false);
        }
    }

    /// Writes the block stored or fixed-coded, whichever is fewer bits, stored
    /// on a tie and only while its input is still in the buffer; then the
    /// whole bytes of it.
    fn flush(&mut self, out: &mut Vec<u8>, last: bool) {
        let block = &self.block;
        let stored = block
            .start
            .checked_sub(self.start)
            .and_then(|from| self.buffer.get(from..from + block.len));
        let fixed = fixed_bits(block);
        match stored {
            Some(data) if stored_bits(data.len(), self.bits.count) <= fixed => {
                write_stored(&mut self.bits, out, data, last);
            }
            _ => {
                self.bits.put(out, u32::from(last), 1);
                self.bits.put(out, 0b01, 2);
                write_symbols(
                    &mut self.bits,
                    out,
                    block,
                    &FIXED_LITERALS,
                    &FIXED_DISTANCES,
                );
            }
        }
        self.bits.bytes(out);
        self.block.reset();
    }

    /// The bytes of input held.
    #[cfg(test)]
    fn retained(&self) -> usize {
        self.buffer.capacity()
    }
}

/// A zlib stream (RFC 1950): the two-byte header, a [`Deflater`]'s stream and
/// the input's Adler-32, kept as it arrives.
pub struct ZlibDeflater {
    deflater: Deflater,
    adler: Adler32,
    level: DeflateLevel,
    header: bool,
}

impl ZlibDeflater {
    /// A zlib stream at `level`.
    #[must_use]
    pub fn new(level: DeflateLevel) -> ZlibDeflater {
        ZlibDeflater {
            deflater: Deflater::new(level),
            adler: Adler32::new(),
            level,
            header: false,
        }
    }

    /// Takes `input`, appending to `out` the header first and each block the
    /// deflater completes.
    pub fn write(&mut self, input: &[u8], out: &mut Vec<u8>) {
        self.header(out);
        self.adler.update(input);
        self.deflater.write(input, out);
    }

    /// Writes the deflater's rest and the checksum.
    pub fn finish(mut self, out: &mut Vec<u8>) {
        self.header(out);
        self.deflater.finish(out);
        out.extend_from_slice(&self.adler.value().to_be_bytes());
    }

    /// CMF: deflate with a 32 KiB window. FLG: no dictionary, `FLEVEL` the
    /// fastest or the default, and check bits making the pair a multiple of
    /// 31 (RFC 1950 §2.2).
    fn header(&mut self, out: &mut Vec<u8>) {
        if !self.header {
            let flags = match self.level {
                DeflateLevel::Fast => 0x01,
                DeflateLevel::Ratio => 0x9C,
            };
            out.extend_from_slice(&[0x78, flags]);
            self.header = true;
        }
    }
}

/// `input` as a raw deflate stream at `level`, through a [`Deflater`].
#[must_use]
pub fn deflate_at(input: &[u8], level: DeflateLevel) -> Vec<u8> {
    let mut out = Vec::new();
    let mut deflater = Deflater::new(level);
    deflater.write(input, &mut out);
    deflater.finish(&mut out);
    out
}

/// `input` as a zlib stream at `level`, through a [`ZlibDeflater`].
#[must_use]
pub fn zlib_compress_at(input: &[u8], level: DeflateLevel) -> Vec<u8> {
    let mut out = Vec::new();
    let mut zlib = ZlibDeflater::new(level);
    zlib.write(input, &mut out);
    zlib.finish(&mut out);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{flate_decode, inflate_raw, Limits};
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
        let mut outs = vec![zlib_compress(b"anything")];
        for level in LEVELS {
            outs.push(zlib_compress_at(b"anything", level));
        }
        for out in outs {
            let header = (u16::from(out[0]) << 8) | u16::from(out[1]);
            assert_eq!(header % 31, 0);
            assert_eq!(out[0], 0x78, "deflate, a 32 KiB window");
            assert_eq!(out[1] & 0x20, 0, "no preset dictionary");
        }
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

    // ---- The streaming deflater ---------------------------------------------

    const LEVELS: [DeflateLevel; 2] = [DeflateLevel::Fast, DeflateLevel::Ratio];

    /// A seeded stream of numbers.
    fn seeded(mut state: u64) -> impl FnMut() -> u64 {
        move || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state
        }
    }

    /// `len` seeded bytes with structure at every reach: letters of a
    /// sixteen-letter alphabet, and copies of 3 to 300 bytes from as far back
    /// as 40 000, beyond the window as well as inside it.
    fn random(len: usize, seed: u64) -> Vec<u8> {
        let mut next = seeded(seed);
        let mut out = Vec::with_capacity(len);
        while out.len() < len {
            if next() % 3 == 0 && out.len() > 3 {
                let length = 3 + (next() % 298) as usize;
                let from = out.len() - 1 - (next() as usize % 40_000) % out.len();
                for i in 0..length {
                    let byte = out[from + i];
                    out.push(byte);
                }
            } else {
                out.push(b'a' + (next() % 16) as u8);
            }
        }
        out.truncate(len);
        out
    }

    /// Text repeated, a run of one byte longer than the buffer, and a
    /// window's noise repeated, whose matches reach back a whole window.
    fn repetitive() -> Vec<u8> {
        let mut out = b"the quick brown fox ".repeat(2_000);
        out.extend(std::iter::repeat_n(0u8, BUFFER + 1_000));
        out.extend(noise(WINDOW, 3).repeat(3));
        out
    }

    /// The three kinds of input the stream is fed.
    fn inputs() -> [(&'static str, Vec<u8>); 3] {
        [
            ("random", random(3 * WINDOW + 1_234, 7)),
            ("repetitive", repetitive()),
            (
                "incompressible",
                noise(3 * WINDOW + 999, 0x2545_F491_4F6C_DD1D),
            ),
        ]
    }

    /// Splits `input` at the sizes `sizes` gives, an empty piece among them
    /// where it gives none, the rest in one piece.
    fn pieces(input: &[u8], mut sizes: impl FnMut() -> usize) -> Vec<&[u8]> {
        let mut out = Vec::new();
        let mut rest = input;
        while !rest.is_empty() {
            let (now, after) = rest.split_at(sizes().min(rest.len()));
            out.push(now);
            rest = after;
        }
        out
    }

    /// Random piece sizes, most of them small and some empty.
    fn random_sizes(seed: u64) -> impl FnMut() -> usize {
        let mut next = seeded(seed);
        move || match next() % 4 {
            0 => 0,
            1 => (next() % 8) as usize,
            2 => (next() % 600) as usize,
            _ => (next() % 20_000) as usize,
        }
    }

    /// `pieces` fed to a deflater at `level`, one write each.
    fn fed(pieces: &[&[u8]], level: DeflateLevel) -> Vec<u8> {
        let mut deflater = Deflater::new(level);
        let mut out = Vec::new();
        for piece in pieces {
            deflater.write(piece, &mut out);
        }
        deflater.finish(&mut out);
        out
    }

    /// `pieces` fed to a zlib stream at `level`, one write each.
    fn fed_zlib(pieces: &[&[u8]], level: DeflateLevel) -> Vec<u8> {
        let mut zlib = ZlibDeflater::new(level);
        let mut out = Vec::new();
        for piece in pieces {
            zlib.write(piece, &mut out);
        }
        zlib.finish(&mut out);
        out
    }

    /// Random, repetitive and incompressible inputs, each fed whole, a byte at
    /// a time and in random pieces, empty ones among them, give the same bytes
    /// at both levels however they are fed, and inflate to themselves: raw,
    /// and in the zlib wrapper with its checksum read back. Block boundaries
    /// follow the symbols, never the writes (ruling 4).
    #[test]
    fn the_stream_round_trips_however_it_is_fed() {
        for (name, input) in inputs() {
            let feeds = [
                vec![&input[..]],
                pieces(&input, || 1),
                pieces(&input, random_sizes(input.len() as u64)),
            ];
            for level in LEVELS {
                let whole = deflate_at(&input, level);
                for (feed, pieces) in ["whole", "a byte at a time", "random pieces"]
                    .iter()
                    .zip(&feeds)
                {
                    assert_eq!(
                        fed(pieces, level),
                        whole,
                        "{name} at {level:?}, fed {feed}, wrote other bytes"
                    );
                    assert_eq!(
                        fed_zlib(pieces, level),
                        zlib_compress_at(&input, level),
                        "{name} at {level:?}, fed {feed} to the zlib wrapper"
                    );
                }
                let back = inflate_raw(&whole, &Limits::new(2 * input.len() + 64));
                assert!(back.complete, "{name} at {level:?} inflates whole");
                assert!(back.warnings.is_empty(), "{name}: {:?}", back.warnings);
                assert_eq!(back.data, input, "{name} at {level:?} inflates to itself");

                let zlib = zlib_compress_at(&input, level);
                let back = flate_decode(&zlib, &Limits::new(2 * input.len() + 64), None)
                    .expect("it decodes");
                assert!(back.complete, "{name} at {level:?} in zlib inflates whole");
                assert!(
                    back.warnings.is_empty(),
                    "{name} at {level:?} in zlib: {:?}",
                    back.warnings
                );
                assert_eq!(back.data, input);
            }
        }
        for level in LEVELS {
            let empty = deflate_at(b"", level);
            let back = inflate_raw(&empty, &Limits::new(16));
            assert!(back.complete && back.data.is_empty(), "{level:?}: empty");
        }
    }

    /// The input a deflater holds is its window buffer, 64 KiB, however much
    /// it is fed and however that is cut: the buffer reaches the bound and
    /// never passes it.
    #[test]
    fn the_window_is_all_the_deflater_keeps() {
        let mut input = random(1 << 20, 11);
        input.extend(noise(1 << 18, 5));
        for level in LEVELS {
            let mut deflater = Deflater::new(level);
            let mut out = Vec::new();
            let mut most = 0;
            for piece in pieces(&input, random_sizes(level as u64 + 1)) {
                deflater.write(piece, &mut out);
                most = most.max(deflater.retained());
                assert!(
                    deflater.block.symbols.len() < BLOCK_SYMBOLS,
                    "a full block is written"
                );
            }
            assert_eq!(most, BUFFER, "{level:?} held more or less than its buffer");
            deflater.finish(&mut out);
            let back = inflate_raw(&out, &Limits::new(2 * input.len()));
            assert_eq!(back.data, input, "{level:?} inflates to itself");
        }
    }

    /// A symbol of a block, as a test reads it.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum Symbol {
        Literal(u8),
        Matched { len: usize, dist: usize },
    }
    use Symbol::{Literal, Matched};

    /// Literals of each byte of `bytes`.
    fn literals(bytes: &[u8]) -> Vec<Symbol> {
        bytes.iter().map(|&byte| Literal(byte)).collect()
    }

    /// Each symbol a deflater at `level` finds in `input`, which is one
    /// block.
    fn symbols(input: &[u8], level: DeflateLevel) -> Vec<Symbol> {
        let mut deflater = Deflater::new(level);
        let mut out = Vec::new();
        deflater.write(input, &mut out);
        deflater.drain(&mut out);
        assert!(out.is_empty(), "the input is one block");
        deflater
            .block
            .symbols
            .iter()
            .map(|&symbol| match (symbol >> 16, symbol & 0xFFFF) {
                (byte, 0) => Literal(byte as u8),
                (len, dist) => Matched {
                    len: len as usize,
                    dist: dist as usize,
                },
            })
            .collect()
    }

    /// At `abcdefghijk`, `abc` matches 16 back and `bcdefghijk` 12 back, one
    /// byte on. The ratio level holds the three bytes, finds the ten, writes
    /// `a` as a literal and takes the ten; the fast level takes the three and
    /// then `defghijk`. No three bytes repeat before it.
    #[test]
    fn lazy_matching_takes_the_longer_match_one_byte_on() {
        let input = b"abcQ-bcdefghijk+abcdefghijk!";

        let mut ratio = literals(&input[..16]);
        ratio.extend([Literal(b'a'), Matched { len: 10, dist: 12 }, Literal(b'!')]);
        assert_eq!(symbols(input, DeflateLevel::Ratio), ratio);

        let mut fast = literals(&input[..16]);
        fast.extend([
            Matched { len: 3, dist: 16 },
            Matched { len: 8, dist: 12 },
            Literal(b'!'),
        ]);
        assert_eq!(symbols(input, DeflateLevel::Fast), fast);

        for level in LEVELS {
            let back = inflate_raw(&deflate_at(input, level), &Limits::new(64));
            assert_eq!(back.data, input, "{level:?}");
        }
    }

    /// The match a held match waits for must be longer, not as long: at
    /// `PQRST`, `PQRS` matches 10 back and `QRST` 6 back, one byte on, and
    /// the first stands.
    #[test]
    fn a_match_one_byte_on_as_long_leaves_the_first() {
        let input = b"PQRS-QRST+PQRST!";
        let mut want = literals(b"PQRS-");
        // `QRS` repeats 4 back inside the sources themselves.
        want.extend([Matched { len: 3, dist: 4 }]);
        want.extend(literals(b"T+"));
        want.extend([Matched { len: 4, dist: 10 }]);
        want.extend(literals(b"T!"));
        assert_eq!(symbols(input, DeflateLevel::Ratio), want);
    }

    /// At the ratio level a three-byte match further back than 4 096 bytes
    /// is dropped and a four-byte one kept; the fast level keeps both.
    #[test]
    fn the_ratio_level_drops_a_far_three_byte_match() {
        let filler = noise(TOO_FAR + 10, 9)
            .into_iter()
            .map(|byte| b'0' + byte % 10)
            .collect::<Vec<u8>>();
        for (head, len) in [(&b"XYZ"[..], 3), (&b"XYZW"[..], 4)] {
            let mut input = head.to_vec();
            input.push(b'+');
            input.extend(&filler);
            input.extend(head);
            input.push(b'!');
            let dist = head.len() + 1 + filler.len();
            for level in LEVELS {
                let found = symbols(&input, level).contains(&Matched { len, dist });
                let want = level == DeflateLevel::Fast || len > MIN_MATCH;
                assert_eq!(found, want, "{level:?}, a match of {len} {dist} back");
            }
        }
    }

    /// `len` seeded bytes in which no three in a row repeat, so that a finder
    /// finds no match in them.
    fn unrepeated(len: usize, seed: u64) -> Vec<u8> {
        let mut next = seeded(seed);
        let mut seen = std::collections::HashSet::new();
        let mut out: Vec<u8> = Vec::with_capacity(len);
        while out.len() < len {
            let byte = next() as u8;
            if let [.., a, b] = out[..] {
                if !seen.insert([a, b, byte]) {
                    continue;
                }
            }
            out.push(byte);
        }
        out
    }

    /// Seeded literals with no three bytes repeated are written as stored
    /// blocks, and a block's boundaries are its symbols': 16 384 literals a
    /// block, then the rest.
    #[test]
    fn literals_are_stored_a_block_of_symbols_at_a_time() {
        let tail = 1_000;
        let input = unrepeated(3 * BLOCK_SYMBOLS + tail, 21);
        for level in LEVELS {
            assert!(
                symbols(&input[..BLOCK_SYMBOLS - 1], level)
                    .iter()
                    .all(|symbol| matches!(symbol, Literal(_))),
                "{level:?} finds no match"
            );
            let out = deflate_at(&input, level);
            // Four stored blocks, each five bytes of header and its bytes.
            assert_eq!(out.len(), input.len() + 4 * 5, "{level:?}");
            let mut at = 0;
            for (index, len) in [BLOCK_SYMBOLS, BLOCK_SYMBOLS, BLOCK_SYMBOLS, tail]
                .into_iter()
                .enumerate()
            {
                assert_eq!(out[at], u8::from(index == 3), "{level:?}: BFINAL, BTYPE 00");
                assert_eq!(
                    u16::from_le_bytes([out[at + 1], out[at + 2]]) as usize,
                    len,
                    "{level:?}: block {index}'s LEN"
                );
                assert_eq!(&out[at + 5..at + 5 + len], &input[at - 5 * index..][..len]);
                at += 5 + len;
            }
        }
    }

    /// The tables a writer codes from: every length and distance reaches the
    /// symbol whose range holds it, and each fixed code is RFC 1951 3.2.6's
    /// reversed.
    #[test]
    fn the_symbol_tables_are_the_rfcs() {
        for length in MIN_MATCH..=MAX_MATCH {
            let (symbol, _, _) = length_symbol(length);
            assert_eq!(
                LENGTH_TABLE[usize::from(LENGTH_INDEX[length])].0,
                symbol,
                "length {length}"
            );
        }
        for distance in 1..=WINDOW {
            let (symbol, _, _) = distance_symbol(distance);
            assert_eq!(
                DISTANCE_TABLE[distance_index(distance)].0,
                symbol,
                "distance {distance}"
            );
        }
        for symbol in 0..288u16 {
            let (code, length) = fixed_literal_code(symbol);
            assert_eq!(
                FIXED_LITERALS[usize::from(symbol)],
                (reversed(code, length as u8), length as u8),
                "literal/length {symbol}"
            );
        }
        assert_eq!(reversed(0b0011_0000, 8), 0b0000_1100);
        assert_eq!(FIXED_DISTANCES[1], (0b10000, 5));
    }
}
