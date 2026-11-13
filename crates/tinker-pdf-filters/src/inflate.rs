//! Inflate (RFC 1951) and the zlib wrapper (RFC 1950).
//!
//! The core is a resumable state machine: every state either completes or
//! reports `NeedInput` without discarding the bits it already read, so the
//! one-shot wrapper below and the streamed read in [`zlib_reads_clean`] share
//! the same decoder. Inside a block, while the input and the output have room
//! for any symbol, [`Inflate::fast`] decodes symbols in a loop of its own and
//! hands the state machine back exactly where it stopped, wherever the input
//! was cut. Back-references read the output `Vec` directly: the one-shot read
//! decodes to memory, so its output doubles as the 32 KB window, and the
//! streamed one keeps that window and lets the rest go.

use crate::{Limits, Tail, Warning, Warnings};

const PRIMARY_BITS: u32 = 9;
const PRIMARY_SIZE: usize = 1 << PRIMARY_BITS;

// Table entry layout. Bit 31 marks a subtable pointer: bits 24..28 its index
// width and bits 0..24 its offset. Any other entry is a code: bits 0..16 its
// value, bits 16..21 its length, bits 21..25 the extra bits after it and bits
// 25..28 what it means, so a code's whole meaning is one read. Zero is a hole.
const ENTRY_SUB: u32 = 1 << 31;
const SUB_BITS_SHIFT: u32 = 24;
const OFFSET_MASK: u32 = 0x00ff_ffff;
const LEN_SHIFT: u32 = 16;
const LEN_MASK: u32 = 0x1f;
const VALUE_MASK: u32 = 0xffff;
const EXTRA_SHIFT: u32 = 21;
const EXTRA_MASK: u32 = 0xf;
const KIND_SHIFT: u32 = 25;
const KIND_MASK: u32 = 0x7;

// What an entry's code means. None is zero, so no code is a hole.
const KIND_LITERAL: u32 = 1;
const KIND_END: u32 = 2;
const KIND_LENGTH: u32 = 3;
const KIND_DISTANCE: u32 = 4;
const KIND_SYMBOL: u32 = 5;
const KIND_UNUSED: u32 = 6;

// 3.2.7: the code-length alphabet arrives in this order.
const CLEN_ORDER: [usize; 19] = [
    16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15,
];

// 3.2.5, Table: length codes 257..=285 and distance codes 0..=29.
const LEN_BASE: [u16; 29] = [
    3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131,
    163, 195, 227, 258,
];
const LEN_EXTRA: [u8; 29] = [
    0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0,
];
const DIST_BASE: [u16; 30] = [
    1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537,
    2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577,
];
const DIST_EXTRA: [u8; 30] = [
    0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13,
    13,
];

/// The alphabet a table decodes (3.2.5, 3.2.7), which says what
/// [`Huffman::build_as`] writes into each code's entry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Alphabet {
    /// Literals, end-of-block and lengths: a code's entry holds the literal
    /// byte, or the length's base and its extra bits.
    LiteralLength,
    /// Distances: a code's entry holds the distance's base and extra bits.
    Distance,
    /// The symbols as they are: the code-length alphabet.
    Symbol,
}

impl Alphabet {
    /// What `symbol` means here, as an entry's value, extra bits and kind.
    ///
    /// Literal/length 286 and 287 and distance 30 and 31 are in their
    /// alphabets and never used (3.2.5), so their codes are entries that say
    /// so, and reading one is a corrupt stream.
    fn meaning(self, symbol: u16) -> (u32, u32, u32) {
        let unused = (0, 0, KIND_UNUSED);
        match self {
            Alphabet::LiteralLength => match symbol {
                0..=255 => (u32::from(symbol), 0, KIND_LITERAL),
                256 => (0, 0, KIND_END),
                _ => {
                    let index = usize::from(symbol) - 257;
                    match (LEN_BASE.get(index), LEN_EXTRA.get(index)) {
                        (Some(&base), Some(&extra)) => {
                            (u32::from(base), u32::from(extra), KIND_LENGTH)
                        }
                        _ => unused,
                    }
                }
            },
            Alphabet::Distance => {
                let index = usize::from(symbol);
                match (DIST_BASE.get(index), DIST_EXTRA.get(index)) {
                    (Some(&base), Some(&extra)) => {
                        (u32::from(base), u32::from(extra), KIND_DISTANCE)
                    }
                    _ => unused,
                }
            }
            Alphabet::Symbol => (u32::from(symbol), 0, KIND_SYMBOL),
        }
    }
}

/// A code read through a table: what its entry says it means.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Code {
    Literal(u8),
    EndOfBlock,
    /// A length's base and the extra bits that follow its code.
    Length(u16, u32),
    /// A distance's base and the extra bits that follow its code.
    Distance(u16, u32),
    /// A symbol of an [`Alphabet::Symbol`] table.
    Symbol(u16),
    /// 3.2.5's literal/length 286 or 287, or distance 30 or 31.
    Unused,
}

impl Code {
    /// The meaning a code entry holds.
    fn of(entry: u32) -> Code {
        let value = entry & VALUE_MASK;
        let extra = (entry >> EXTRA_SHIFT) & EXTRA_MASK;
        match (entry >> KIND_SHIFT) & KIND_MASK {
            KIND_LITERAL => Code::Literal(value as u8),
            KIND_END => Code::EndOfBlock,
            KIND_LENGTH => Code::Length(value as u16, extra),
            KIND_DISTANCE => Code::Distance(value as u16, extra),
            KIND_SYMBOL => Code::Symbol(value as u16),
            _ => Code::Unused,
        }
    }
}

/// A canonical Huffman decoding table: one 9-bit primary lookup plus
/// subtables for the codes that do not fit in it.
#[derive(Clone, Debug)]
struct Huffman {
    primary: Vec<u32>,
    sub: Vec<u32>,
}

impl Huffman {
    fn empty() -> Self {
        Self {
            primary: vec![0; PRIMARY_SIZE],
            sub: Vec::new(),
        }
    }

    /// A table of the symbols themselves, [`Alphabet::Symbol`]'s: the
    /// code-length alphabet's.
    fn build(lengths: &[u8]) -> Option<Self> {
        Self::build_as(lengths, Alphabet::Symbol)
    }

    /// A table whose every code's entry holds what its symbol means in
    /// `alphabet` — the literal, or the length's or the distance's base and
    /// extra bits — so a code resolves in the one read that finds it, and a
    /// read of a code longer than the primary table in the one more its
    /// subtable takes.
    ///
    /// `None` when the lengths over-subscribe the code space. Incomplete
    /// codes are accepted: their holes decode as invalid, which is what
    /// makes a one-distance-code block work.
    fn build_as(lengths: &[u8], alphabet: Alphabet) -> Option<Self> {
        let mut count = [0u16; 16];
        for &l in lengths {
            let l = l as usize;
            if l > 15 {
                return None;
            }
            if l != 0 {
                count[l] += 1;
            }
        }

        let mut left: i32 = 1;
        for c in count.iter().skip(1) {
            left <<= 1;
            left -= i32::from(*c);
            if left < 0 {
                return None;
            }
        }

        let total: usize = count.iter().map(|&c| c as usize).sum();
        let mut sorted = vec![0u16; total];
        let mut offs = [0usize; 16];
        let mut acc = 0usize;
        for len in 1..16 {
            offs[len] = acc;
            acc += count[len] as usize;
        }
        for (sym, &l) in lengths.iter().enumerate() {
            if l == 0 {
                continue;
            }
            let li = l as usize;
            if let (Some(slot), Ok(s)) = (offs.get_mut(li), u16::try_from(sym)) {
                if let Some(dst) = sorted.get_mut(*slot) {
                    *dst = s;
                }
                *slot += 1;
            }
        }

        // 3.2.2: canonical codes, assigned in (length, symbol) order.
        let mut codes: Vec<(u16, u8, u32)> = Vec::with_capacity(total);
        let mut code: u32 = 0;
        let mut k = 0usize;
        for len in 1..16u32 {
            code = (code + u32::from(count[len as usize - 1])) << 1;
            // Written as a counted range rather than a `for _` with a `c += 1`
            // inside it: clippy 1.98 refuses the second form (`explicit_counter_loop`)
            // and this checkout's 1.93 does not, which is how a lint nobody here
            // can reproduce locally turned every CI leg red for five runs.
            for c in (code..).take(count[len as usize] as usize) {
                if let Some(&sym) = sorted.get(k) {
                    codes.push((sym, len as u8, reverse_bits(c, len)));
                }
                k += 1;
            }
        }

        let mut primary = vec![0u32; PRIMARY_SIZE];
        let mut sub = Vec::new();

        let mut submax = [0u8; PRIMARY_SIZE];
        for &(_, len, rev) in &codes {
            if u32::from(len) > PRIMARY_BITS {
                let p = (rev as usize) & (PRIMARY_SIZE - 1);
                if let Some(m) = submax.get_mut(p) {
                    if len > *m {
                        *m = len;
                    }
                }
            }
        }
        for (p, &m) in submax.iter().enumerate() {
            if m == 0 {
                continue;
            }
            let bits = u32::from(m) - PRIMARY_BITS;
            let off = sub.len();
            if off > OFFSET_MASK as usize {
                return None;
            }
            sub.resize(off + (1usize << bits), 0);
            if let Some(e) = primary.get_mut(p) {
                *e = ENTRY_SUB | (bits << SUB_BITS_SHIFT) | (off as u32);
            }
        }

        for &(sym, len, rev) in &codes {
            let l = u32::from(len);
            let (value, extra, kind) = alphabet.meaning(sym);
            let entry = (kind << KIND_SHIFT) | (extra << EXTRA_SHIFT) | (l << LEN_SHIFT) | value;
            if l <= PRIMARY_BITS {
                let step = 1usize << l;
                let mut j = rev as usize;
                while j < PRIMARY_SIZE {
                    if let Some(e) = primary.get_mut(j) {
                        *e = entry;
                    }
                    j += step;
                }
            } else {
                let p = (rev as usize) & (PRIMARY_SIZE - 1);
                let head = match primary.get(p) {
                    Some(&h) if h & ENTRY_SUB != 0 => h,
                    _ => continue,
                };
                let bits = (head >> SUB_BITS_SHIFT) & 0xf;
                let off = (head & OFFSET_MASK) as usize;
                let size = 1usize << bits;
                let step = 1usize << (l - PRIMARY_BITS);
                let mut j = (rev >> PRIMARY_BITS) as usize;
                while j < size {
                    if let Some(e) = sub.get_mut(off + j) {
                        *e = entry;
                    }
                    j += step;
                }
            }
        }

        Some(Self { primary, sub })
    }
}

fn reverse_bits(v: u32, n: u32) -> u32 {
    let mut r = 0;
    for i in 0..n {
        r |= ((v >> i) & 1) << (n - 1 - i);
    }
    r
}

// 3.2.6: the fixed literal/length and distance code lengths.
fn fixed_literal_lengths() -> [u8; 288] {
    let mut lens = [0u8; 288];
    for (i, l) in lens.iter_mut().enumerate() {
        *l = match i {
            0..=143 => 8,
            144..=255 => 9,
            256..=279 => 7,
            _ => 8,
        };
    }
    lens
}

fn fixed_literal() -> Huffman {
    Huffman::build_as(&fixed_literal_lengths(), Alphabet::LiteralLength)
        .unwrap_or_else(Huffman::empty)
}

fn fixed_distance() -> Huffman {
    Huffman::build_as(&[5u8; 30], Alphabet::Distance).unwrap_or_else(Huffman::empty)
}

/// Position in the caller's byte slice. Split from the bit accumulator so a
/// streaming wrapper can hand the same decoder successive slices.
struct Bits<'a> {
    input: &'a [u8],
    pos: usize,
}

#[derive(Clone, Copy, Debug, Default)]
struct BitState {
    buf: u64,
    cnt: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Sym {
    Val(Code),
    NeedInput,
    Invalid,
}

/// One read of a table's entries, counted under test.
fn entry(entries: &[u32], index: usize) -> Option<u32> {
    #[cfg(test)]
    tests::READS.with(|reads| reads.set(reads.get() + 1));
    entries.get(index).copied()
}

impl BitState {
    fn fill(&mut self, src: &mut Bits<'_>) {
        while self.cnt <= 56 {
            match src.input.get(src.pos) {
                Some(&b) => {
                    self.buf |= u64::from(b) << self.cnt;
                    self.cnt += 8;
                    src.pos += 1;
                }
                None => break,
            }
        }
    }

    /// Tops the accumulator up to at least 56 bits with one read of the next
    /// eight input bytes, where [`BitState::fill`] takes a byte a turn. Only
    /// with eight bytes left.
    ///
    /// Only the bytes that fit whole are counted in, so the bits past the
    /// count are the input's own next bits rather than zeros, and reading the
    /// same bytes again ORs the same bits into them. [`Inflate::fast`] clears
    /// them on its way out, because every other reader of the accumulator
    /// wants zeros there: a stored block takes its bytes past the count
    /// straight from the input.
    fn refill(&mut self, src: &mut Bits<'_>) {
        if self.cnt >= 56 {
            return;
        }
        let Some(&word) = src
            .input
            .get(src.pos..)
            .and_then(|rest| rest.first_chunk::<8>())
        else {
            return;
        };
        self.buf |= u64::from_le_bytes(word) << self.cnt;
        let whole = (63 - self.cnt) / 8;
        src.pos += whole as usize;
        self.cnt += whole * 8;
    }

    /// The code at the head of the accumulator through `t`, and its length,
    /// without taking it; `None` for a hole. Needs fifteen bits in hand.
    fn peek(&self, t: &Huffman) -> Option<(Code, u32)> {
        #[cfg(test)]
        tests::DECODES.with(|decodes| decodes.set(decodes.get() + 1));
        let mut e = entry(&t.primary, (self.buf as usize) & (PRIMARY_SIZE - 1))?;
        if e & ENTRY_SUB != 0 {
            let bits = (e >> SUB_BITS_SHIFT) & 0xf;
            let off = (e & OFFSET_MASK) as usize;
            let j = ((self.buf >> PRIMARY_BITS) & ((1u64 << bits) - 1)) as usize;
            e = entry(&t.sub, off + j)?;
        }
        (e != 0).then(|| (Code::of(e), (e >> LEN_SHIFT) & LEN_MASK))
    }

    fn need(&mut self, n: u32, src: &mut Bits<'_>) -> bool {
        if self.cnt < n {
            self.fill(src);
        }
        self.cnt >= n
    }

    fn take(&mut self, n: u32) -> u32 {
        let v = if n == 0 {
            0
        } else {
            (self.buf & ((1u64 << n) - 1)) as u32
        };
        self.buf >>= n;
        self.cnt = self.cnt.saturating_sub(n);
        v
    }

    fn decode(&mut self, t: &Huffman, src: &mut Bits<'_>) -> Sym {
        #[cfg(test)]
        tests::DECODES.with(|decodes| decodes.set(decodes.get() + 1));
        self.fill(src);
        let idx = (self.buf as usize) & (PRIMARY_SIZE - 1);
        let Some(e) = entry(&t.primary, idx) else {
            return Sym::Invalid;
        };
        if e == 0 {
            // A hole. With fewer than 9 bits left the pattern is zero
            // padding, not a real code, so more input may still resolve it.
            return if self.cnt < PRIMARY_BITS {
                Sym::NeedInput
            } else {
                Sym::Invalid
            };
        }
        if e & ENTRY_SUB == 0 {
            let len = (e >> LEN_SHIFT) & LEN_MASK;
            if self.cnt < len {
                return Sym::NeedInput;
            }
            self.take(len);
            return Sym::Val(Code::of(e));
        }
        let bits = (e >> SUB_BITS_SHIFT) & 0xf;
        let off = (e & OFFSET_MASK) as usize;
        let j = ((self.buf >> PRIMARY_BITS) & ((1u64 << bits) - 1)) as usize;
        let Some(se) = entry(&t.sub, off + j) else {
            return Sym::Invalid;
        };
        if se == 0 {
            return if self.cnt < PRIMARY_BITS + bits {
                Sym::NeedInput
            } else {
                Sym::Invalid
            };
        }
        let len = (se >> LEN_SHIFT) & LEN_MASK;
        if self.cnt < len {
            return Sym::NeedInput;
        }
        self.take(len);
        Sym::Val(Code::of(se))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum State {
    Header,
    StoredLen,
    StoredCopy,
    DynHeader,
    DynClens,
    DynLens,
    Symbol,
    /// A length's base and its extra bits.
    LenExtra(u16, u32),
    /// The length decoded.
    DistSym(u16),
    /// The length, and the distance's base and extra bits.
    DistExtra(u16, u16, u32),
    Copy(u16, u16),
    Done,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Progress {
    Finished,
    NeedInput,
    Corrupt,
    OutputCap,
}

/// The resumable RFC 1951 decoder. Private on purpose: the crate exposes the
/// one-shot reads, and [`zlib_reads_clean`] is the streamed one inside it.
struct Inflate {
    bits: BitState,
    state: State,
    last: bool,
    lit: Huffman,
    dist: Huffman,
    clen: Huffman,
    hlit: usize,
    hdist: usize,
    hclen: usize,
    clen_lens: [u8; 19],
    idx: usize,
    lens: [u8; 320],
    rep_kind: u8,
    stored_left: u32,
    /// Turns of the state machine: a header field, a code length or a symbol
    /// read each. [`Inflate::fast`] counts the turns it saves the state
    /// machine, so the count is the state machine's alone.
    steps: u64,
}

impl Inflate {
    fn new() -> Self {
        Self {
            bits: BitState::default(),
            state: State::Header,
            last: false,
            lit: Huffman::empty(),
            dist: Huffman::empty(),
            clen: Huffman::empty(),
            hlit: 0,
            hdist: 0,
            hclen: 0,
            clen_lens: [0; 19],
            idx: 0,
            lens: [0; 320],
            rep_kind: 0,
            stored_left: 0,
            steps: 0,
        }
    }

    /// **The fast path**: literals, lengths, distances and copies decoded in
    /// one loop, while at least eight input bytes and 258 bytes of output
    /// room remain — enough for any symbol whatever it turns out to be, a
    /// match's 48 bits of codes and extra bits and its 258 bytes — with the
    /// accumulator refilled eight bytes at a time and no state dispatch a
    /// symbol. Ruling 1's bound on the output is the margin, one check a
    /// symbol rather than one a byte.
    ///
    /// It stops at the first symbol it would not finish: end-of-block, a hole
    /// or an unused code, a distance before the output's start, or a margin
    /// run out. A code not yet taken is left for [`Inflate::run`]'s symbol
    /// state and a match is left in the state it reached, so the state
    /// machine reads what it would have read itself, and the stream's tail,
    /// `NeedInput` and the output cap stay its own — the same bytes out, the
    /// same outcome and the same `end`.
    ///
    /// It counts the turns the state machine would have taken for what it
    /// decodes, a literal one and a match five, so [`Inflate::steps`] is the
    /// same with it as without it. The turn that runs it counts the symbol
    /// the state machine reads after it, or the match it stops inside.
    fn fast(&mut self, src: &mut Bits<'_>, out: &mut Vec<u8>, max_output: usize) {
        while src.input.len().saturating_sub(src.pos) >= 8
            && max_output.saturating_sub(out.len()) >= 258
        {
            self.bits.refill(src);
            let Some((code, length)) = self.bits.peek(&self.lit) else {
                break;
            };
            match code {
                Code::Literal(byte) => {
                    self.bits.take(length);
                    out.push(byte);
                    self.turned(1);
                }
                Code::Length(base, extra) => {
                    self.bits.take(length);
                    let len = u32::from(base) + self.bits.take(extra);
                    let Some((Code::Distance(base, extra), length)) = self.bits.peek(&self.dist)
                    else {
                        // The length's extra bits were the state machine's
                        // turn; the turn that reads the distance is its own.
                        self.turned(1);
                        self.state = State::DistSym(len as u16);
                        break;
                    };
                    self.bits.take(length);
                    let distance = u32::from(base) + self.bits.take(extra);
                    if distance == 0 || distance as usize > out.len() {
                        // The length's extra bits, the distance and its extra
                        // bits; the copy's turn refuses it.
                        self.turned(3);
                        self.state = State::Copy(len as u16, distance as u16);
                        break;
                    }
                    copy_back(out, out.len() - distance as usize, len as usize);
                    // The length, its extra bits, the distance, its extra
                    // bits and the copy.
                    self.turned(5);
                }
                _ => break,
            }
        }
        if self.bits.cnt < 64 {
            self.bits.buf &= (1u64 << self.bits.cnt) - 1;
        }
    }

    /// `turns` of the state machine taken without it.
    fn turned(&mut self, turns: u64) {
        self.steps = self.steps.saturating_add(turns);
    }

    #[allow(clippy::too_many_lines)]
    fn run(&mut self, src: &mut Bits<'_>, out: &mut Vec<u8>, max_output: usize) -> Progress {
        loop {
            self.steps = self.steps.saturating_add(1);
            #[cfg(test)]
            tests::DISPATCHES.with(|dispatches| dispatches.set(dispatches.get() + 1));
            match self.state {
                State::Done => return Progress::Finished,

                State::Header => {
                    if !self.bits.need(3, src) {
                        return Progress::NeedInput;
                    }
                    self.last = self.bits.take(1) == 1;
                    match self.bits.take(2) {
                        0 => {
                            // 3.2.4: stored blocks resume on a byte boundary.
                            let r = self.bits.cnt % 8;
                            self.bits.take(r);
                            self.state = State::StoredLen;
                        }
                        1 => {
                            self.lit = fixed_literal();
                            self.dist = fixed_distance();
                            self.state = State::Symbol;
                        }
                        2 => self.state = State::DynHeader,
                        // 3.2.3: type 3 is reserved.
                        _ => return Progress::Corrupt,
                    }
                }

                State::StoredLen => {
                    if !self.bits.need(32, src) {
                        return Progress::NeedInput;
                    }
                    let len = self.bits.take(16);
                    let nlen = self.bits.take(16);
                    if len ^ 0xffff != nlen {
                        return Progress::Corrupt;
                    }
                    self.stored_left = len;
                    self.state = State::StoredCopy;
                }

                State::StoredCopy => {
                    // Whole bytes already pulled into the accumulator belong
                    // to the stored run and must be drained first.
                    while self.stored_left > 0 && self.bits.cnt >= 8 {
                        if out.len() >= max_output {
                            return Progress::OutputCap;
                        }
                        out.push(self.bits.take(8) as u8);
                        self.stored_left -= 1;
                    }
                    if self.stored_left > 0 {
                        let avail = src.input.len() - src.pos;
                        let room = max_output.saturating_sub(out.len());
                        if room == 0 {
                            return Progress::OutputCap;
                        }
                        let n = (self.stored_left as usize).min(avail).min(room);
                        if let Some(s) = src.input.get(src.pos..src.pos + n) {
                            out.extend_from_slice(s);
                        }
                        src.pos += n;
                        self.stored_left -= n as u32;
                        if self.stored_left > 0 {
                            return if n == room {
                                Progress::OutputCap
                            } else {
                                Progress::NeedInput
                            };
                        }
                    }
                    self.state = if self.last {
                        State::Done
                    } else {
                        State::Header
                    };
                }

                State::DynHeader => {
                    if !self.bits.need(14, src) {
                        return Progress::NeedInput;
                    }
                    self.hlit = self.bits.take(5) as usize + 257;
                    self.hdist = self.bits.take(5) as usize + 1;
                    self.hclen = self.bits.take(4) as usize + 4;
                    self.clen_lens = [0; 19];
                    self.idx = 0;
                    self.state = State::DynClens;
                }

                State::DynClens => {
                    while self.idx < self.hclen {
                        if !self.bits.need(3, src) {
                            return Progress::NeedInput;
                        }
                        let v = self.bits.take(3) as u8;
                        if let Some(slot) = CLEN_ORDER
                            .get(self.idx)
                            .and_then(|&o| self.clen_lens.get_mut(o))
                        {
                            *slot = v;
                        }
                        self.idx += 1;
                    }
                    match Huffman::build(&self.clen_lens) {
                        Some(t) => self.clen = t,
                        None => return Progress::Corrupt,
                    }
                    self.lens = [0; 320];
                    self.idx = 0;
                    self.rep_kind = 0;
                    self.state = State::DynLens;
                }

                State::DynLens => {
                    let total = self.hlit + self.hdist;
                    loop {
                        if self.rep_kind != 0 {
                            let (extra, base) = match self.rep_kind {
                                16 => (2u32, 3usize),
                                17 => (3, 3),
                                _ => (7, 11),
                            };
                            if !self.bits.need(extra, src) {
                                return Progress::NeedInput;
                            }
                            let n = base + self.bits.take(extra) as usize;
                            let val = if self.rep_kind == 16 {
                                match self.idx.checked_sub(1).and_then(|i| self.lens.get(i)) {
                                    Some(&v) => v,
                                    None => return Progress::Corrupt,
                                }
                            } else {
                                0
                            };
                            // Leniency: a repeat running past the declared
                            // count is clamped rather than failing the block.
                            let end = self.idx.saturating_add(n).min(total);
                            while self.idx < end {
                                if let Some(s) = self.lens.get_mut(self.idx) {
                                    *s = val;
                                }
                                self.idx += 1;
                            }
                            self.rep_kind = 0;
                            continue;
                        }
                        if self.idx >= total {
                            break;
                        }
                        let sym = match self.bits.decode(&self.clen, src) {
                            Sym::Val(Code::Symbol(s)) => s,
                            Sym::NeedInput => return Progress::NeedInput,
                            Sym::Val(_) | Sym::Invalid => return Progress::Corrupt,
                        };
                        if sym < 16 {
                            if let Some(s) = self.lens.get_mut(self.idx) {
                                *s = sym as u8;
                            }
                            self.idx += 1;
                        } else if sym <= 18 {
                            self.rep_kind = sym as u8;
                        } else {
                            return Progress::Corrupt;
                        }
                    }
                    let lit = match self
                        .lens
                        .get(..self.hlit)
                        .and_then(|lens| Huffman::build_as(lens, Alphabet::LiteralLength))
                    {
                        Some(t) => t,
                        None => return Progress::Corrupt,
                    };
                    let dist = match self
                        .lens
                        .get(self.hlit..total)
                        .and_then(|lens| Huffman::build_as(lens, Alphabet::Distance))
                    {
                        Some(t) => t,
                        None => return Progress::Corrupt,
                    };
                    self.lit = lit;
                    self.dist = dist;
                    self.state = State::Symbol;
                }

                State::Symbol => {
                    if fast_path() {
                        self.fast(src, out, max_output);
                        if self.state != State::Symbol {
                            // It stopped inside a match, which the state
                            // machine refuses from where it stopped.
                            continue;
                        }
                    }
                    match self.bits.decode(&self.lit, src) {
                        Sym::Val(Code::Literal(byte)) => {
                            if out.len() >= max_output {
                                return Progress::OutputCap;
                            }
                            out.push(byte);
                        }
                        Sym::Val(Code::EndOfBlock) => {
                            self.state = if self.last {
                                State::Done
                            } else {
                                State::Header
                            };
                        }
                        Sym::Val(Code::Length(base, extra)) => {
                            self.state = State::LenExtra(base, extra);
                        }
                        Sym::NeedInput => return Progress::NeedInput,
                        // 286 and 287 are in the alphabet but never used.
                        Sym::Val(_) | Sym::Invalid => return Progress::Corrupt,
                    }
                }

                State::LenExtra(base, extra) => {
                    if !self.bits.need(extra, src) {
                        return Progress::NeedInput;
                    }
                    let len = u32::from(base) + self.bits.take(extra);
                    self.state = State::DistSym(len as u16);
                }

                State::DistSym(len) => match self.bits.decode(&self.dist, src) {
                    Sym::Val(Code::Distance(base, extra)) => {
                        self.state = State::DistExtra(len, base, extra);
                    }
                    Sym::NeedInput => return Progress::NeedInput,
                    // As are distances 30 and 31.
                    Sym::Val(_) | Sym::Invalid => return Progress::Corrupt,
                },

                State::DistExtra(len, base, extra) => {
                    if !self.bits.need(extra, src) {
                        return Progress::NeedInput;
                    }
                    let d = u32::from(base) + self.bits.take(extra);
                    self.state = State::Copy(len, d as u16);
                }

                State::Copy(len, dist) => {
                    let d = dist as usize;
                    if d == 0 || d > out.len() {
                        return Progress::Corrupt;
                    }
                    let start = out.len() - d;
                    let room = max_output.saturating_sub(out.len());
                    let want = len as usize;
                    let n = want.min(room);
                    copy_back(out, start, n);
                    if n < want {
                        return Progress::OutputCap;
                    }
                    self.state = State::Symbol;
                }
            }
        }
    }
}

/// Appends the `n` bytes of `out` from `start` on, a byte at a time, so a
/// byte the copy itself wrote is copied again where the distance is shorter
/// than the length (3.2.3). `start` is inside `out`, which only grows, so
/// every byte read is there.
fn copy_back(out: &mut Vec<u8>, start: usize, n: usize) {
    for k in 0..n {
        if let Some(&byte) = out.get(start + k) {
            out.push(byte);
        }
    }
}

/// Whether [`Inflate::fast`] runs. Always, but under the test that turns it
/// off to hold it to the state machine alone.
fn fast_path() -> bool {
    #[cfg(test)]
    {
        !tests::STATE_MACHINE_ONLY.with(std::cell::Cell::get)
    }
    #[cfg(not(test))]
    {
        true
    }
}

struct Raw {
    data: Vec<u8>,
    complete: bool,
    /// The output ceiling, not the data, ended this decode.
    capped: bool,
    /// Bytes consumed, counted to the end of the final block.
    end: usize,
}

/// What [`inflate_raw`] produced, and how far into the input it reached.
///
/// Not [`crate::Decoded`], which is what a `/Filter` chain returns, and the
/// difference is the reason this type exists. A caller here is not running a
/// PDF filter; it is reading a *container* whose payload happens to be RFC
/// 1951, and it needs two things a filter never asks for: whether the ceiling
/// rather than the data ended the decode, and where the stream stopped.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RawInflated {
    /// The bytes produced, truncated at [`Limits::max_output`].
    pub data: Vec<u8>,
    /// False when the input ended early, was damaged, or hit the ceiling.
    pub complete: bool,
    /// The output ceiling, not the data, ended this decode. Implies
    /// `!complete`: the stream had more to give and was not allowed to.
    pub capped: bool,
    /// Input bytes consumed, counted to the end of the final block — so
    /// `input[..end]` is the stream and `input[end..]` is whatever the
    /// container put after it.
    ///
    /// DEFLATE ends on a *bit* boundary and every container that carries it
    /// resumes on a byte one, so a final block ending mid-byte counts that
    /// whole byte. `end` is therefore a ceiling, and `input[..end]` is a
    /// stream rather than a prefix of one.
    ///
    /// **Only meaningful when `complete`.** A truncated, corrupt or capped
    /// decode stopped somewhere the format did not choose, and `end` there
    /// records where this decoder gave up rather than where the stream ends.
    ///
    /// This is the field a ZIP entry with general-purpose bit 3 set has no
    /// substitute for: its compressed size is not written ahead of the data,
    /// so the only way to find the data descriptor that follows is to inflate
    /// and see where the stream stopped.
    pub end: usize,
    /// Typed leniency records (ruling 10), deduplicated, in first-occurrence
    /// order — the same contract as [`crate::Decoded::warnings`].
    pub warnings: Vec<Warning>,
}

/// Inflate a raw RFC 1951 stream, with no zlib wrapper and none looked for.
///
/// [`crate::flate_decode`] is the door for a PDF stream named `/FlateDecode`,
/// where the bytes may be either and sniffing is right. This is the door for a
/// caller that **knows a priori** it holds raw DEFLATE — a ZIP entry stored
/// with method 8 is RFC 1951 with no wrapper by definition (APPNOTE 4.4.5) —
/// and going through the sniff instead would cost it three things:
///
/// - a [`Warning::RawDeflateFallback`] on every single entry, because the
///   sniffing path treats raw DEFLATE as a *fallback* where here it is the
///   norm. Ruling 10 exists so that "it opened" and "it opened cleanly" stay
///   distinguishable, and a warning raised once per entry for something that
///   is not leniency at all destroys that distinction for the whole format;
/// - a [`Warning::TrailingGarbage`] on every entry whose size was streamed,
///   because what follows such an entry is its data descriptor and then the
///   next local header — structure, not garbage;
/// - and, rarely but constructibly, a wrong answer. A raw stream beginning
///   with a non-final stored block whose discarded pad bit is set presents the
///   low nibble a zlib header wants; `08 1D` passes both of the sniff's tests
///   (`0x081D` is 31 x 67), and the retry that would recover only runs when
///   the wrapped read produced *nothing at all*. `tests/containers.rs` commits
///   such a string and walks it through both doors.
///
/// Bounded on any input (ruling 1): output stops at `limits.max_output` with
/// [`Warning::OutputCapHit`] and `capped` set, so a bomb is refused by its own
/// warning rather than by how long it took.
#[must_use]
pub fn inflate_raw(input: &[u8], limits: &Limits) -> RawInflated {
    let mut w = Warnings::default();
    let r = raw_bytes(input, limits, &mut w);
    RawInflated {
        data: r.data,
        complete: r.complete,
        capped: r.capped,
        end: r.end,
        warnings: w.into_vec(),
    }
}

/// The byte layer, shared by [`inflate_raw`] and [`flate_bytes`] — named for
/// the convention every other filter in this crate uses for its own.
fn raw_bytes(input: &[u8], limits: &Limits, w: &mut Warnings) -> Raw {
    let mut inf = Inflate::new();
    let mut src = Bits { input, pos: 0 };
    let mut out = Vec::new();
    let p = inf.run(&mut src, &mut out, limits.max_output);
    w.step_by(inf.steps);
    match p {
        Progress::NeedInput => w.push(Warning::TruncatedInput),
        Progress::Corrupt => w.push(Warning::CorruptDeflateStream),
        Progress::OutputCap => w.push(Warning::OutputCapHit),
        Progress::Finished => {}
    }
    // Whole bytes still in the accumulator were never part of the stream.
    let end = src.pos.saturating_sub(inf.bits.cnt as usize / 8);
    Raw {
        data: out,
        complete: p == Progress::Finished,
        capped: p == Progress::OutputCap,
        end,
    }
}

/// 2.2: a zlib header is CM 8, CINFO <= 7 and a 31-modular check byte.
fn zlib_header(input: &[u8]) -> Option<bool> {
    let (&cmf, &flg) = match (input.first(), input.get(1)) {
        (Some(c), Some(f)) => (c, f),
        _ => return None,
    };
    if cmf & 0x0f != 8 || cmf >> 4 > 7 {
        return None;
    }
    if (u16::from(cmf) * 256 + u16::from(flg)) % 31 != 0 {
        return None;
    }
    Some(flg & 0x20 != 0)
}

fn adler32(data: &[u8]) -> u32 {
    adler32_from(1, data)
}

/// Adler-32 carried on from `adler`, the checksum of the bytes before `data`,
/// so a stream read a piece at a time is summed the same as one read whole.
fn adler32_from(adler: u32, data: &[u8]) -> u32 {
    let (mut a, mut b) = (u64::from(adler & 0xffff), u64::from(adler >> 16));
    for chunk in data.chunks(5552) {
        for &x in chunk {
            a += u64::from(x);
            b += a;
        }
        a %= 65521;
        b %= 65521;
    }
    ((b << 16) | a) as u32
}

/// FlateDecode's byte layer: a zlib stream when the header says so, raw
/// deflate when it does not. `tail` says what may follow the stream.
pub(crate) fn flate_bytes(
    input: &[u8],
    limits: &Limits,
    tail: Tail,
    w: &mut Warnings,
) -> (Vec<u8>, bool) {
    if let Some(fdict) = zlib_header(input) {
        if fdict {
            w.push(Warning::FdictIgnored);
        }
        // 2.2: DICTID follows the two header bytes when FDICT is set.
        let start = if fdict { 6 } else { 2 };
        let body = input.get(start..).unwrap_or(&[]);
        let mut aw = Warnings::default();
        let r = raw_bytes(body, limits, &mut aw);
        // Retrying as raw only makes sense when the wrapped bytes yielded
        // nothing at all; a ceiling hit is our decision, not bad data.
        if !r.data.is_empty() || r.complete || r.capped {
            w.merge(&aw);
            let mut complete = r.complete;
            if complete {
                let rest = input.get(start + r.end..).unwrap_or_default();
                // A missing checksum costs no data, so its absence is not
                // itself a leniency worth recording. Four bytes after the
                // data are the checksum, whatever they are.
                if let Some((checksum, after)) = rest.split_first_chunk::<4>() {
                    if !fdict && u32::from_be_bytes(*checksum) != adler32(&r.data) {
                        w.push(Warning::AdlerMismatch);
                        complete = false;
                    }
                    if tail.garbage(after) {
                        w.push(Warning::TrailingGarbage);
                    }
                }
            }
            return (r.data, complete);
        }
    }

    // Header absent, invalid, or wrapped bytes that produced nothing: real
    // files carry raw deflate under a FlateDecode name often enough that
    // this retry is required, not optional.
    w.push(Warning::RawDeflateFallback);
    let r = raw_bytes(input, limits, w);
    if r.complete && tail.garbage(input.get(r.end..).unwrap_or_default()) {
        w.push(Warning::TrailingGarbage);
    }
    (r.data, r.complete)
}

/// The largest distance a back-reference reaches (3.2.5), and so all of the
/// output a streamed read has to keep.
const WINDOW: usize = 32 * 1024;

/// Input handed to the decoder at a time by [`zlib_reads_clean`], which bounds
/// each piece of output it yields to this times DEFLATE's largest expansion,
/// whatever the stream holds: a 258-byte copy coded in as few as two bits, so
/// about a thousand to one.
const STEP: usize = 4 * 1024;

/// [`flate_bytes`] read as a stream, for a caller that looks at the data
/// without holding it: the decoded bytes reach `sink` in order, a piece at a
/// time, and the read keeps only a window of them, the 32 KiB back-references
/// copy from, let go in runs of three times that.
///
/// True when the read is clean and `sink` took every piece, which is exactly
/// when [`flate_bytes`] with no output ceiling returns the bytes complete and
/// pushes no warning: a zlib header with no preset dictionary, a stream that
/// ends, an Adler-32 that matches what it held, and nothing `tail` calls
/// garbage after it. `sink` returns false to stop the read, which is then not
/// clean. With no ceiling, the work is bounded by the input, a thousand bytes
/// of data at most for each byte of code, and a caller that knows how much
/// data to expect stops the read from `sink` once it is past that.
pub(crate) fn zlib_reads_clean(
    input: &[u8],
    tail: Tail,
    sink: &mut dyn FnMut(&[u8]) -> bool,
) -> bool {
    // A preset dictionary is ignored with a warning, and bytes that are not a
    // zlib header are retried as raw DEFLATE with one: neither read is clean.
    if zlib_header(input) != Some(false) {
        return false;
    }
    let body = input.get(2..).unwrap_or_default();
    let mut inf = Inflate::new();
    let mut out = Vec::new();
    // `out[..handed]` has been through `sink` and the checksum.
    let mut handed = 0usize;
    let mut adler = 1u32;
    let mut pos = 0usize;
    let mut fed = 0usize;
    loop {
        fed = body.len().min(fed.saturating_add(STEP));
        let mut src = Bits {
            input: body.get(..fed).unwrap_or_default(),
            pos,
        };
        let progress = inf.run(&mut src, &mut out, usize::MAX);
        pos = src.pos;
        let fresh = out.get(handed..).unwrap_or_default();
        adler = adler32_from(adler, fresh);
        if !fresh.is_empty() && !sink(fresh) {
            return false;
        }
        // Drained in long runs, so the window is moved once for every few
        // times its own length decoded rather than once a piece.
        if out.len() >= 4 * WINDOW {
            out.drain(..out.len() - WINDOW);
        }
        handed = out.len();
        match progress {
            Progress::NeedInput if fed < body.len() => {}
            Progress::Finished => {
                // As in `raw_bytes`: whole bytes still in the accumulator were
                // never part of the stream.
                let end = pos.saturating_sub(inf.bits.cnt as usize / 8);
                let rest = body.get(end..).unwrap_or_default();
                // And as in `flate_bytes`: four bytes after the data are the
                // checksum, and fewer are a checksum missing, which costs no
                // data and is no repair.
                return match rest.split_first_chunk::<4>() {
                    Some((checksum, after)) => {
                        u32::from_be_bytes(*checksum) == adler && !tail.garbage(after)
                    }
                    None => true,
                };
            }
            // Cut short, corrupt, or a ceiling, which this read does not set.
            Progress::NeedInput | Progress::Corrupt | Progress::OutputCap => return false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    std::thread_local! {
        /// Every read of a table's entries this thread has made, so a test can
        /// say what a code costs to decode — a property of the code rather
        /// than of the machine.
        pub(super) static READS: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
        /// Every code this thread has decoded, likewise.
        pub(super) static DECODES: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
        /// Every turn of [`Inflate::run`]'s state dispatch, likewise.
        pub(super) static DISPATCHES: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
        /// Set, [`Inflate::fast`] does not run and every symbol goes through
        /// the state machine.
        pub(super) static STATE_MACHINE_ONLY: std::cell::Cell<bool> =
            const { std::cell::Cell::new(false) };
    }

    const CAP: Limits = Limits::new(1 << 20);

    fn raw(input: &[u8]) -> (Vec<u8>, bool, Vec<Warning>) {
        let mut w = Warnings::default();
        let r = raw_bytes(input, &CAP, &mut w);
        (r.data, r.complete, w.into_vec())
    }

    fn flate(input: &[u8]) -> (Vec<u8>, bool, Vec<Warning>) {
        let mut w = Warnings::default();
        let (d, c) = flate_bytes(input, &CAP, Tail::WhiteSpace, &mut w);
        (d, c, w.into_vec())
    }

    #[test]
    fn reverse_bits_is_msb_first() {
        assert_eq!(reverse_bits(0b1, 3), 0b100);
        assert_eq!(reverse_bits(0b1011, 4), 0b1101);
        assert_eq!(reverse_bits(0, 15), 0);
    }

    #[test]
    fn fixed_tables_build() {
        let l = fixed_literal();
        assert_eq!(l.primary.len(), PRIMARY_SIZE);
        // 144..=255 are 9-bit codes, so nothing spills into a subtable.
        assert!(l.sub.is_empty());
        assert!(!fixed_distance().primary.is_empty());
    }

    #[test]
    fn over_subscribed_lengths_are_rejected() {
        assert!(Huffman::build(&[1, 1, 1]).is_none());
        assert!(Huffman::build(&[1, 1]).is_some());
        assert!(Huffman::build(&[16]).is_none());
    }

    #[test]
    fn long_codes_reach_a_subtable() {
        // Fifteen-bit codes cannot live in the 9-bit primary table.
        let mut lens = [0u8; 16];
        for (i, l) in lens.iter_mut().enumerate() {
            *l = if i < 15 { i as u8 + 1 } else { 15 };
        }
        let t = Huffman::build(&lens).expect("kraft-complete");
        assert!(!t.sub.is_empty());
    }

    /// The decode as it was: the symbol out of a table of symbols, then its
    /// meaning out of 3.2.5's tables, a lookup for the base and one for the
    /// extra bits.
    fn symbol_then_base(alphabet: Alphabet, symbol: u16) -> Code {
        let i = usize::from(symbol);
        match alphabet {
            Alphabet::LiteralLength if i < 256 => Code::Literal(symbol as u8),
            Alphabet::LiteralLength if i == 256 => Code::EndOfBlock,
            Alphabet::LiteralLength => match (LEN_BASE.get(i - 257), LEN_EXTRA.get(i - 257)) {
                (Some(&base), Some(&extra)) => Code::Length(base, u32::from(extra)),
                _ => Code::Unused,
            },
            Alphabet::Distance => match (DIST_BASE.get(i), DIST_EXTRA.get(i)) {
                (Some(&base), Some(&extra)) => Code::Distance(base, u32::from(extra)),
                _ => Code::Unused,
            },
            Alphabet::Symbol => Code::Symbol(symbol),
        }
    }

    /// The code at the head of `pattern`, read through `table` with 32 bits
    /// in hand, and how many of them it took.
    fn read(table: &Huffman, pattern: u64) -> (Sym, u32) {
        let mut state = BitState {
            buf: pattern,
            cnt: 32,
        };
        let mut src = Bits { input: &[], pos: 0 };
        let sym = state.decode(table, &mut src);
        (sym, 32 - state.cnt)
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

    /// One deflate token: a literal byte, or a match of a length from a
    /// distance back.
    #[derive(Clone, Copy, Debug)]
    enum Token {
        Literal(u8),
        Match(usize, usize),
    }

    /// Tokens for about `len` bytes of page-like text: about a third of them
    /// matches of 3 to 258 bytes from anywhere up to 32 768 back, and
    /// literals of a small alphabet between.
    fn tokens(len: usize, seed: u64) -> Vec<Token> {
        let mut draw = draws(seed);
        let mut tokens = Vec::new();
        let mut made = 0usize;
        while made < len {
            if made > 40 && draw(3) == 0 {
                let length = 3 + draw(256);
                tokens.push(Token::Match(length, 1 + draw(made.min(32_768))));
                made += length;
            } else {
                tokens.push(Token::Literal(b"etaoin shrdlu"[draw(13)]));
                made += 1;
            }
        }
        tokens
    }

    /// Bits gathered as deflate packs them, from each byte's least
    /// significant bit up.
    #[derive(Default)]
    struct BitSink {
        bytes: Vec<u8>,
        used: u32,
    }

    impl BitSink {
        fn bit(&mut self, bit: u32) {
            if self.used == 0 {
                self.bytes.push(0);
            }
            if let Some(last) = self.bytes.last_mut() {
                *last |= ((bit & 1) as u8) << self.used;
            }
            self.used = (self.used + 1) % 8;
        }

        /// A field: `value`'s low `count` bits, least significant first.
        fn field(&mut self, value: usize, count: u8) {
            for i in 0..u32::from(count) {
                self.bit((value >> i) as u32);
            }
        }

        /// A Huffman code, most significant bit first (3.1.1).
        fn code(&mut self, (code, length): (u32, u32)) {
            for i in (0..length).rev() {
                self.bit(code >> i);
            }
        }
    }

    /// 3.2.6's fixed code for a literal/length symbol, as code and length.
    fn fixed_code(symbol: usize) -> (u32, u32) {
        let s = symbol as u32;
        match symbol {
            0..=143 => (0x30 + s, 8),
            144..=255 => (0x190 + s - 144, 9),
            256..=279 => (s - 256, 7),
            _ => (0xC0 + s - 280, 8),
        }
    }

    /// One final block of 3.2.6's fixed codes carrying `tokens`, written bit
    /// by bit from the clauses rather than by the crate's encoder, which
    /// chooses its own codes; and the bytes the tokens make.
    fn fixed_block(tokens: &[Token]) -> (Vec<u8>, Vec<u8>) {
        let mut sink = BitSink::default();
        // BFINAL, then BTYPE 01.
        sink.field(0b011, 3);
        let mut data = Vec::new();
        for &token in tokens {
            match token {
                Token::Literal(byte) => {
                    sink.code(fixed_code(usize::from(byte)));
                    data.push(byte);
                }
                Token::Match(length, distance) => {
                    let at = LEN_BASE
                        .iter()
                        .rposition(|&base| usize::from(base) <= length)
                        .unwrap_or(0);
                    sink.code(fixed_code(257 + at));
                    sink.field(length - usize::from(LEN_BASE[at]), LEN_EXTRA[at]);
                    let back = DIST_BASE
                        .iter()
                        .rposition(|&base| usize::from(base) <= distance)
                        .unwrap_or(0);
                    sink.code((back as u32, 5));
                    sink.field(distance - usize::from(DIST_BASE[back]), DIST_EXTRA[back]);
                    let start = data.len() - distance;
                    for k in 0..length {
                        data.push(data[start + k]);
                    }
                }
            }
        }
        sink.code(fixed_code(256));
        (sink.bytes, data)
    }

    /// The lengths of a complete prefix code of `count` codes, none longer
    /// than fifteen bits, given to distinct symbols of `symbols`: leaves
    /// split from a root, a random one at a time and every third time the
    /// deepest still splittable, so codes too long for the primary table
    /// come too.
    fn complete_lengths(symbols: usize, count: usize, seed: u64) -> Vec<u8> {
        let mut draw = draws(seed);
        let mut leaves = vec![0u8];
        let mut turn = 0;
        while leaves.len() < count.min(symbols) {
            let splittable: Vec<usize> = (0..leaves.len()).filter(|&i| leaves[i] < 15).collect();
            if splittable.is_empty() {
                break;
            }
            turn += 1;
            let at = if turn % 3 == 0 {
                splittable
                    .iter()
                    .copied()
                    .max_by_key(|&i| leaves[i])
                    .unwrap_or(0)
            } else {
                splittable[draw(splittable.len())]
            };
            let depth = leaves.swap_remove(at) + 1;
            leaves.extend([depth, depth]);
        }
        let mut order: Vec<usize> = (0..symbols).collect();
        for i in (1..symbols).rev() {
            order.swap(i, draw(i + 1));
        }
        let mut lengths = vec![0u8; symbols];
        for (&symbol, &depth) in order.iter().zip(&leaves) {
            lengths[symbol] = depth;
        }
        lengths
    }

    /// The fixed tables, and random complete and incomplete dynamic ones of
    /// both alphabets — as many literal/length symbols as `HLIT` can name,
    /// 286 and 287 among them, and distance 30 and 31 — with codes up to
    /// fifteen bits.
    fn tables() -> Vec<(Alphabet, Vec<u8>)> {
        let mut tables = vec![
            (Alphabet::LiteralLength, fixed_literal_lengths().to_vec()),
            (Alphabet::Distance, vec![5; 30]),
            // 3.2.7's one distance code of one bit.
            (Alphabet::Distance, vec![1]),
        ];
        for seed in 0..12u64 {
            let mut draw = draws(seed + 100);
            let hlit = 257 + draw(32);
            let hdist = 1 + draw(32);
            for (alphabet, symbols) in
                [(Alphabet::LiteralLength, hlit), (Alphabet::Distance, hdist)]
            {
                let count = 2 + draw(symbols - 1);
                let complete = complete_lengths(symbols, count, seed);
                let mut incomplete = complete.clone();
                for _ in 0..1 + draw(4) {
                    let at = draw(symbols);
                    incomplete[at] = 0;
                }
                tables.push((alphabet, complete));
                tables.push((alphabet, incomplete));
            }
        }
        tables
    }

    /// **The combined tables decode what the separate lookups did.**
    ///
    /// Every fifteen-bit pattern through each table of [`tables`] — every
    /// code with every suffix, the subtables' and the holes' included —
    /// gives the meaning, code length and extra bits through the combined
    /// table that the table of symbols and [`symbol_then_base`] give, and
    /// the same answer where there is no code.
    #[test]
    fn the_combined_tables_decode_what_the_separate_lookups_did() {
        let mut long = 0usize;
        for (index, (alphabet, lengths)) in tables().iter().enumerate() {
            let combined = Huffman::build_as(lengths, *alphabet).expect("a code the test drew");
            let symbols = Huffman::build(lengths).expect("the same code");
            for pattern in 0..1u64 << 15 {
                let (ours, taken) = read(&combined, pattern);
                let (theirs, their_taken) = match read(&symbols, pattern) {
                    (Sym::Val(Code::Symbol(symbol)), taken) => {
                        (Sym::Val(symbol_then_base(*alphabet, symbol)), taken)
                    }
                    other => other,
                };
                assert_eq!(
                    (ours, taken),
                    (theirs, their_taken),
                    "table {index}, {alphabet:?}, pattern {pattern:015b}"
                );
                long += usize::from(matches!(ours, Sym::Val(_)) && taken > PRIMARY_BITS);
            }
        }
        assert!(long > 0, "no code was longer than the primary table");
    }

    /// **A length or a distance resolves in one table read**, two for a
    /// code longer than the primary table — and the read is the whole of it,
    /// the base and the extra bits coming with the code. Counted through
    /// [`READS`] over every code of every table of [`tables`], and over a
    /// whole stream of fixed codes, where every symbol is one read.
    #[test]
    fn a_length_or_distance_resolves_in_one_table_read() {
        let mut seen = [0usize; 2];
        for (alphabet, lengths) in tables() {
            if alphabet == Alphabet::Symbol {
                continue;
            }
            let table = Huffman::build_as(&lengths, alphabet).expect("a code the test drew");
            for pattern in 0..1u64 << 15 {
                READS.with(|reads| reads.set(0));
                let (sym, taken) = read(&table, pattern);
                let reads = READS.with(std::cell::Cell::get);
                let kind = match sym {
                    Sym::Val(Code::Length(..)) => 0,
                    Sym::Val(Code::Distance(..)) => 1,
                    _ => continue,
                };
                seen[kind] += 1;
                let expected = if taken <= PRIMARY_BITS { 1 } else { 2 };
                assert_eq!(reads, expected, "{alphabet:?}, pattern {pattern:015b}");
            }
        }
        assert!(seen[0] > 0 && seen[1] > 0, "{seen:?}");

        // A stream of fixed codes, literals and matches: the fixed codes all
        // fit the primary table, so each symbol is one read, and the matches
        // are why there are fewer symbols than bytes.
        let (coded, data) = fixed_block(&tokens(20_000, 5));
        READS.with(|reads| reads.set(0));
        DECODES.with(|decodes| decodes.set(0));
        let mut inf = Inflate::new();
        let mut src = Bits {
            input: &coded,
            pos: 0,
        };
        let mut out = Vec::new();
        assert_eq!(inf.run(&mut src, &mut out, 1 << 20), Progress::Finished);
        assert_eq!(out, data);
        let reads = READS.with(std::cell::Cell::get);
        let decodes = DECODES.with(std::cell::Cell::get);
        assert_eq!(reads, decodes, "{reads} reads for {decodes} symbols");
        assert!(
            decodes > 0 && decodes < data.len() as u64,
            "{decodes} symbols for {} bytes: the stream coded no match",
            data.len()
        );
    }

    /// `stream` decoded by one state machine, its first `split` bytes in one
    /// call and the rest in a second as a streaming caller would hand them
    /// on, under the output ceiling `cap`, with [`Inflate::fast`] on or off:
    /// what came out, how it ended, `end` as [`raw_bytes`] counts it, and the
    /// steps it counted.
    fn decode_split(
        stream: &[u8],
        split: usize,
        cap: usize,
        fast: bool,
    ) -> (Vec<u8>, Progress, usize, u64) {
        STATE_MACHINE_ONLY.with(|only| only.set(!fast));
        let mut inf = Inflate::new();
        let mut out = Vec::new();
        let mut src = Bits {
            input: &stream[..split.min(stream.len())],
            pos: 0,
        };
        let mut progress = inf.run(&mut src, &mut out, cap);
        if progress == Progress::NeedInput && split < stream.len() {
            src = Bits {
                input: stream,
                pos: src.pos,
            };
            progress = inf.run(&mut src, &mut out, cap);
        }
        STATE_MACHINE_ONLY.with(|only| only.set(false));
        let end = src.pos.saturating_sub(inf.bits.cnt as usize / 8);
        (out, progress, end, inf.steps)
    }

    /// Page-like bytes: text-like runs and repeats at every distance range,
    /// so a coded stream has literals, short and long matches and far ones.
    fn text_with_repeats(len: usize, seed: u64) -> Vec<u8> {
        let mut draw = draws(seed);
        let mut out = Vec::with_capacity(len);
        while out.len() < len {
            if out.len() > 40 && draw(3) == 0 {
                let back = 1 + draw(out.len().min(30_000));
                let start = out.len() - back;
                for k in 0..(3 + draw(60)).min(len - out.len()) {
                    out.push(out[start + k]);
                }
            } else {
                out.push(b"etaoin shrdlu"[draw(13)]);
            }
        }
        out
    }

    /// Streams of every shape the two paths could part on: a block of fixed
    /// codes, the encoder's at the ratio and the fast level, a stored block,
    /// a stored block before a fixed one, each damaged by a bit flipped
    /// anywhere in its body, cut short, and followed by bytes that are not
    /// its own.
    fn streams() -> Vec<Vec<u8>> {
        let text = text_with_repeats(1_500, 3);
        let noise: Vec<u8> = {
            let mut draw = draws(9);
            (0..400).map(|_| draw(256) as u8).collect()
        };
        // 3.2.4's stored block: a byte holding BFINAL and BTYPE 00, then LEN
        // and NLEN.
        let stored = |data: &[u8], last: bool| {
            let len = data.len() as u16;
            let mut out = vec![u8::from(last)];
            out.extend_from_slice(&len.to_le_bytes());
            out.extend_from_slice(&(!len).to_le_bytes());
            out.extend_from_slice(data);
            out
        };
        let mut mixed = stored(&noise[..100], false);
        mixed.extend_from_slice(&fixed_block(&tokens(600, 8)).0);
        let mut streams = vec![
            fixed_block(&tokens(1_500, 3)).0,
            crate::deflate_at(&text, crate::DeflateLevel::Ratio),
            crate::deflate_at(&text, crate::DeflateLevel::Fast),
            stored(&noise, true),
            mixed,
            crate::deflate_at(&text_with_repeats(300, 4), crate::DeflateLevel::Ratio),
        ];
        let whole = streams.clone();
        for (index, stream) in whole.iter().enumerate() {
            let mut draw = draws(index as u64 + 50);
            for _ in 0..3 {
                let mut damaged = stream.clone();
                let bit = draw(damaged.len() * 8);
                damaged[bit / 8] ^= 1 << (bit % 8);
                streams.push(damaged);
            }
            streams.push(stream[..stream.len() * 2 / 3].to_vec());
            let mut followed = stream.clone();
            followed.extend_from_slice(b"\x00\xffnot this stream's");
            streams.push(followed);
        }
        streams
    }

    /// **The fast loop and the state machine agree at every split.**
    ///
    /// Each stream of [`streams`] decoded with the fast loop, whole and
    /// resumed after every one of its bytes, and with the state machine
    /// alone, under a ceiling that never binds and under one that cuts the
    /// output short: the same bytes out, the same outcome — and so the same
    /// warnings, which [`raw_bytes`] reads off the outcome alone — and the
    /// same `end`, which a ZIP entry's data descriptor is found by. Split at
    /// the same byte, the two count the same steps too, since the fast loop
    /// counts the turns it saves; a split itself costs the turns that
    /// returned for more input, so steps are compared split for split.
    #[test]
    fn the_fast_loop_and_the_state_machine_agree_at_every_split() {
        for (index, stream) in streams().iter().enumerate() {
            for cap in [1usize << 20, 700] {
                let alone = decode_split(stream, stream.len(), cap, false);
                for split in 0..=stream.len() {
                    let case = format!("stream {index}, ceiling {cap}, split at {split}");
                    let fast = decode_split(stream, split, cap, true);
                    assert_eq!(
                        fast,
                        decode_split(stream, split, cap, false),
                        "{case}: the fast loop and the state machine alone"
                    );
                    assert_eq!(
                        (&fast.0, fast.1, fast.2),
                        (&alone.0, alone.1, alone.2),
                        "{case}: resumed and whole"
                    );
                }
                let limits = Limits::new(cap);
                let mut w = Warnings::default();
                let whole = raw_bytes(stream, &limits, &mut w);
                assert_eq!(
                    (whole.data, whole.end),
                    (alone.0.clone(), alone.2),
                    "stream {index}, ceiling {cap}, through raw_bytes"
                );
                assert_eq!(
                    w.steps(),
                    alone.3,
                    "stream {index}, ceiling {cap}: raw_bytes counts the state machine's steps"
                );
            }
        }
    }

    /// **The streamed read takes the fast loop and reads what the whole read
    /// does.** [`zlib_reads_clean`] hands the decoder its input a piece at a
    /// time and lets its output go behind the window, so the fast loop stops
    /// short of every piece's end and carries on in the next. Each stream of
    /// [`streams`], wrapped in zlib with the checksum of what it decodes to,
    /// gives the same pieces and the same verdict with the fast loop as with
    /// the state machine alone, and the pieces joined are the bytes the
    /// one-shot read decodes, whether the stream ends, is cut short or is
    /// damaged.
    #[test]
    fn the_streamed_read_takes_the_fast_loop_and_reads_what_the_whole_read_does() {
        for (index, stream) in streams().iter().enumerate() {
            let whole = decode_split(stream, stream.len(), usize::MAX, false);
            let mut wrapped = vec![0x78, 0x01];
            wrapped.extend_from_slice(stream);
            wrapped.extend_from_slice(&adler32(&whole.0).to_be_bytes());
            STATE_MACHINE_ONLY.with(|only| only.set(true));
            let alone = streamed(&wrapped, Tail::WhiteSpace);
            STATE_MACHINE_ONLY.with(|only| only.set(false));
            let fast = streamed(&wrapped, Tail::WhiteSpace);
            assert_eq!(
                fast, alone,
                "stream {index}: the fast loop and the state machine alone"
            );
            assert_eq!(
                fast.0, whole.0,
                "stream {index}: the pieces and the one-shot read"
            );
        }
    }

    /// **A symbol on the fast path costs no state dispatch.**
    ///
    /// One fixed-code block of page-like data, followed by sixteen bytes so
    /// that every symbol has eight input bytes ahead of the furthest the
    /// accumulator can have read, which is eight past the symbol, takes the
    /// same three turns of the dispatch — the header, the symbols, done —
    /// whether it holds a kilobyte or sixty-four, counted through
    /// [`DISPATCHES`]; the state machine alone takes a turn a symbol and
    /// more.
    #[test]
    fn a_symbol_on_the_fast_path_costs_no_state_dispatch() {
        let mut turns = Vec::new();
        for len in [1_000usize, 64_000] {
            let (mut stream, data) = fixed_block(&tokens(len, 11));
            assert_eq!(
                stream.first().map(|b| b & 0b111),
                Some(0b011),
                "one final fixed block"
            );
            stream.extend_from_slice(&[0; 16]);
            for fast in [true, false] {
                DISPATCHES.with(|dispatches| dispatches.set(0));
                DECODES.with(|decodes| decodes.set(0));
                let (out, progress, _, _) = decode_split(&stream, stream.len(), 1 << 20, fast);
                assert_eq!((progress, out == data), (Progress::Finished, true));
                let dispatches = DISPATCHES.with(std::cell::Cell::get);
                let symbols = DECODES.with(std::cell::Cell::get);
                if fast {
                    turns.push(dispatches);
                } else {
                    assert!(
                        dispatches >= symbols,
                        "{dispatches} turns for {symbols} symbols"
                    );
                }
            }
        }
        assert_eq!(turns, [3, 3], "the turns grew with the symbols");
    }

    #[test]
    fn stored_block_round_trips() {
        // BFINAL=1 BTYPE=00, LEN=5, NLEN=~5, "Hello".
        let s = b"\x01\x05\x00\xfa\xffHello";
        let (d, c, w) = raw(s);
        assert_eq!(d, b"Hello");
        assert!(c);
        assert!(w.is_empty());
    }

    #[test]
    fn stored_block_with_bad_nlen_is_corrupt() {
        let s = b"\x01\x05\x00\x00\x00Hello";
        let (d, c, w) = raw(s);
        assert!(d.is_empty());
        assert!(!c);
        assert_eq!(w, vec![Warning::CorruptDeflateStream]);
    }

    #[test]
    fn empty_input_needs_input() {
        let (d, c, w) = raw(b"");
        assert!(d.is_empty());
        assert!(!c);
        assert_eq!(w, vec![Warning::TruncatedInput]);
    }

    #[test]
    fn reserved_block_type_is_corrupt() {
        let (_, c, w) = raw(&[0b111]);
        assert!(!c);
        assert_eq!(w, vec![Warning::CorruptDeflateStream]);
    }

    #[test]
    fn zlib_header_recognises_the_common_headers() {
        assert_eq!(zlib_header(&[0x78, 0x9c]), Some(false));
        assert_eq!(zlib_header(&[0x78, 0x01]), Some(false));
        assert_eq!(zlib_header(&[0x78, 0xda]), Some(false));
        assert_eq!(zlib_header(&[0x78, 0xf9]), Some(true));
        assert_eq!(zlib_header(&[0x78, 0x9d]), None);
        assert_eq!(zlib_header(&[0x08]), None);
        assert_eq!(zlib_header(&[]), None);
    }

    #[test]
    fn adler32_matches_the_rfc_definition() {
        assert_eq!(adler32(b""), 1);
        assert_eq!(adler32(b"a"), 0x0062_0062);
        assert_eq!(adler32(b"Wikipedia"), 0x11E6_0398);
    }

    #[test]
    fn zlib_wrapped_stored_block() {
        let s = b"\x78\x01\x01\x05\x00\xfa\xffHello\x05\x8c\x01\xf5";
        let (d, c, w) = flate(s);
        assert_eq!(d, b"Hello");
        assert!(c, "warnings: {w:?}");
        assert!(w.is_empty(), "{w:?}");
    }

    #[test]
    fn adler_mismatch_warns_but_keeps_the_data() {
        let s = b"\x78\x01\x01\x05\x00\xfa\xffHello\x00\x00\x00\x00";
        let (d, c, w) = flate(s);
        assert_eq!(d, b"Hello");
        assert!(!c);
        assert_eq!(w, vec![Warning::AdlerMismatch]);
    }

    #[test]
    fn trailing_garbage_after_the_checksum_is_ignored() {
        let s = b"\x78\x01\x01\x05\x00\xfa\xffHello\x05\x8c\x01\xf5junk";
        let (d, c, w) = flate(s);
        assert_eq!(d, b"Hello");
        assert!(c);
        assert_eq!(w, vec![Warning::TrailingGarbage]);
    }

    /// "Hello", stored, in a zlib wrapper with its checksum.
    const HELLO: &[u8] = b"\x78\x01\x01\x05\x00\xfa\xffHello\x05\x8c\x01\xf5";

    /// The end-of-line 7.3.8.1 keeps out of `/Length`, which a producer
    /// counted in, is the last of a stream's bytes and no garbage: in each of
    /// its three forms, after blanks, and as several blank lines.
    #[test]
    fn white_space_after_the_checksum_is_the_streams_own() {
        for tail in [&b"\n"[..], b"\r\n", b"\r", b" \t\r\n", b"\x00\x0c\n\n"] {
            let mut s = HELLO.to_vec();
            s.extend_from_slice(tail);
            let (d, c, w) = flate(&s);
            assert_eq!(d, b"Hello", "{tail:?}");
            assert!(c, "{tail:?}");
            assert!(w.is_empty(), "{tail:?}: {w:?}");
        }
        // Anything else after it is garbage still, white space before it or
        // not.
        let mut s = HELLO.to_vec();
        s.extend_from_slice(b"\r\nx");
        assert_eq!(flate(&s).2, vec![Warning::TrailingGarbage]);
    }

    /// A stream with no checksum whose `/Length` counts its end-of-line:
    /// fewer than four bytes follow the data, so there is no checksum to
    /// read, and its absence costs no data.
    #[test]
    fn an_end_of_line_with_no_checksum_before_it_is_silent() {
        let bare = &HELLO[..HELLO.len() - 4];
        for tail in [&b"\n"[..], b"\r\n", b"\r"] {
            let mut s = bare.to_vec();
            s.extend_from_slice(tail);
            let (d, c, w) = flate(&s);
            assert_eq!(d, b"Hello", "{tail:?}");
            assert!(c, "{tail:?}");
            assert!(w.is_empty(), "{tail:?}: {w:?}");
        }
    }

    /// A container's coded data ends where its data does: a PNG's image data
    /// or a TIFF strip keeps any byte after the checksum garbage.
    #[test]
    fn a_containers_data_keeps_white_space_after_it_garbage() {
        let mut w = Warnings::default();
        let mut s = HELLO.to_vec();
        s.push(b'\n');
        let (d, c) = flate_bytes(&s, &CAP, Tail::Nothing, &mut w);
        assert_eq!((d.as_slice(), c), (&b"Hello"[..], true));
        assert_eq!(w.into_vec(), vec![Warning::TrailingGarbage]);
    }

    /// The raw fallback reads what follows its data by the same rule.
    #[test]
    fn white_space_after_raw_deflate_is_the_streams_own() {
        let (d, c, w) = flate(b"\x01\x05\x00\xfa\xffHello\r\n");
        assert_eq!(d, b"Hello");
        assert!(c);
        assert_eq!(w, vec![Warning::RawDeflateFallback]);
        let (_, _, w) = flate(b"\x01\x05\x00\xfa\xffHello\r\njunk");
        assert_eq!(
            w,
            vec![Warning::RawDeflateFallback, Warning::TrailingGarbage]
        );
    }

    #[test]
    fn raw_deflate_under_a_flate_name_falls_back() {
        let s = b"\x01\x05\x00\xfa\xffHello";
        let (d, c, w) = flate(s);
        assert_eq!(d, b"Hello");
        assert!(c);
        assert_eq!(w, vec![Warning::RawDeflateFallback]);
    }

    #[test]
    fn valid_header_that_decodes_to_nothing_retries_as_raw() {
        // 0x78 0x01 is a valid zlib header, but here it is the first two
        // bytes of a raw stored block whose payload is "xy".
        let mut s = vec![0x01, 0x02, 0x00, 0xfd, 0xff, b'x', b'y'];
        assert_eq!(zlib_header(&s), None);
        s.clear();
        // A stream whose zlib body is garbage: fall back and still fail.
        s.extend_from_slice(&[0x78, 0x9c, 0xff, 0xff]);
        let (d, c, w) = flate(&s);
        assert!(d.is_empty());
        assert!(!c);
        assert!(w.contains(&Warning::RawDeflateFallback));
    }

    #[test]
    fn fdict_is_warned_and_skipped() {
        // Header with FDICT, a four-byte DICTID, then a stored "Hi".
        let s = b"\x78\xf9\x00\x00\x00\x00\x01\x02\x00\xfd\xffHi";
        let (d, c, w) = flate(s);
        assert_eq!(d, b"Hi");
        assert!(c);
        assert!(w.contains(&Warning::FdictIgnored));
    }

    #[test]
    fn truncation_mid_stored_block_keeps_the_prefix() {
        let s = b"\x01\x05\x00\xfa\xffHel";
        let (d, c, w) = raw(s);
        assert_eq!(d, b"Hel");
        assert!(!c);
        assert_eq!(w, vec![Warning::TruncatedInput]);
    }

    #[test]
    fn output_cap_stops_a_stored_block() {
        let limits = Limits::new(2);
        let mut w = Warnings::default();
        let r = raw_bytes(b"\x01\x05\x00\xfa\xffHello", &limits, &mut w);
        assert_eq!(r.data, b"He");
        assert!(!r.complete);
        assert_eq!(w.into_vec(), vec![Warning::OutputCapHit]);
    }

    /// The first two bytes of `tests/containers.rs`'s witness, which is where
    /// the rest of the claim is made. `zlib_header` is private, so the half of
    /// the hazard that is a *sniff* result rather than a decode result is
    /// asserted here, next to the function it is about.
    ///
    /// `0x08` is CM 8 with CINFO 0 — and, read as DEFLATE, BFINAL 0 with BTYPE
    /// 00 and bit 3 set, which is the pad bit a stored block discards. `0x081D`
    /// is 2 077, which is 31 x 67, so the modulo-31 check passes too. Neither
    /// test is weak; they simply do not know what the bytes are for.
    #[test]
    fn a_raw_stored_block_can_present_a_valid_zlib_header() {
        assert_eq!(zlib_header(&[0x08, 0x1D]), Some(false));
        assert_eq!(0x081D_u32 % 31, 0);
    }

    #[test]
    fn resuming_across_slices_gives_the_same_bytes() {
        // The state machine is fed one byte at a time; a streaming surface
        // would do exactly this with a carry buffer.
        let whole = b"\x01\x05\x00\xfa\xffHello";
        let mut inf = Inflate::new();
        let mut out = Vec::new();
        let mut p = Progress::NeedInput;
        for end in 1..=whole.len() {
            let mut src = Bits {
                input: &whole[..end],
                pos: end - 1,
            };
            p = inf.run(&mut src, &mut out, 64);
            if p == Progress::Finished {
                break;
            }
            assert_eq!(p, Progress::NeedInput);
        }
        assert_eq!(p, Progress::Finished);
        assert_eq!(out, b"Hello");
    }

    /// A streamed read's pieces, joined, and whether it was clean.
    fn streamed(input: &[u8], tail: Tail) -> (Vec<u8>, bool) {
        let mut seen = Vec::new();
        let clean = zlib_reads_clean(input, tail, &mut |piece: &[u8]| {
            seen.extend_from_slice(piece);
            true
        });
        (seen, clean)
    }

    /// Words from a small vocabulary, with now and then a run repeated from
    /// most of a window back: it compresses, but to many steps of input, and
    /// its back-references reach across every drain of the streamed window.
    fn page_like(len: usize) -> Vec<u8> {
        let words: Vec<Vec<u8>> = (0..64u32)
            .map(|i| {
                (0..5 + i % 8)
                    .map(|k| b'a' + ((i * 7 + k * 3) % 26) as u8)
                    .collect()
            })
            .collect();
        let mut x = 0x2545_f491_u32;
        let mut out = Vec::with_capacity(len + 512);
        while out.len() < len {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            if x % 40 == 0 && out.len() > WINDOW {
                let from = out.len() - WINDOW + (x as usize >> 20) % 1024;
                out.extend_from_within(from..from + 300);
            } else {
                out.extend_from_slice(&words[(x >> 8) as usize % words.len()]);
                out.push(b' ');
            }
        }
        out.truncate(len);
        out
    }

    /// What [`flate_bytes`] says of a stream with a ceiling it cannot reach.
    fn whole(input: &[u8], tail: Tail) -> (Vec<u8>, bool) {
        let mut w = Warnings::default();
        let (data, complete) = flate_bytes(input, &Limits::new(1 << 26), tail, &mut w);
        (data, complete && w.into_vec().is_empty())
    }

    #[test]
    fn a_streamed_read_hands_on_the_bytes_a_whole_read_returns() {
        for len in [0, 1, 1000, 4 * WINDOW, 600_000] {
            let data = page_like(len);
            let coded = crate::deflate::zlib_compress(&data);
            if len == 600_000 {
                assert!(coded.len() > 8 * STEP, "{} bytes of code", coded.len());
            }
            let (seen, clean) = streamed(&coded, Tail::WhiteSpace);
            assert!(clean, "{len} bytes read clean");
            assert_eq!(seen, data, "{len} bytes handed on in order");
        }
    }

    /// Clean exactly when the whole read is complete with no warning, over a
    /// stream cut, flipped, extended, given a dictionary and stripped of its
    /// header; and where both are clean, the same bytes.
    #[test]
    fn a_streamed_read_is_clean_exactly_when_a_whole_read_needs_no_repair() {
        let coded = crate::deflate::zlib_compress(&page_like(200_000));
        let n = coded.len();
        let mut cases: Vec<Vec<u8>> = vec![coded.clone()];
        for cut in [0, 1, 2, 3, n / 2, n - 6, n - 5, n - 4, n - 3, n - 1] {
            cases.push(coded[..cut].to_vec());
        }
        for at in [0, 1, 2, 3, 10, n / 3, n / 2, n - 6, n - 5, n - 4, n - 1] {
            for bit in [0x01, 0x80] {
                let mut flipped = coded.clone();
                flipped[at] ^= bit;
                cases.push(flipped);
            }
        }
        for tail in [&b"\n"[..], b" \r\n", b"x", b"\nx", b"\0\0"] {
            let mut longer = coded.clone();
            longer.extend_from_slice(tail);
            cases.push(longer);
        }
        // FDICT set (0x7820 is 31 x 992) and a dictionary id, then the same
        // body; and the body alone, raw DEFLATE under no header at all.
        let mut fdict = vec![0x78, 0x20, 0, 0, 0, 1];
        fdict.extend_from_slice(&coded[2..]);
        cases.push(fdict);
        cases.push(coded[2..n - 4].to_vec());

        for (i, case) in cases.iter().enumerate() {
            for tail in [Tail::WhiteSpace, Tail::Nothing] {
                let (data, expected) = whole(case, tail);
                let (seen, clean) = streamed(case, tail);
                assert_eq!(clean, expected, "case {i} under {tail:?}");
                if clean {
                    assert_eq!(seen, data, "case {i} under {tail:?}");
                }
            }
        }
        // Both answers occur, so the agreement is not vacuous: the stream as
        // written reads clean, a white-space tail only where white space is
        // allowed, and half a stream not at all.
        assert!(streamed(&coded, Tail::Nothing).1);
        let mut newline = coded.clone();
        newline.push(b'\n');
        assert!(streamed(&newline, Tail::WhiteSpace).1);
        assert!(!streamed(&newline, Tail::Nothing).1);
        assert!(!streamed(&coded[..n / 2], Tail::WhiteSpace).1);
    }

    #[test]
    fn a_sink_that_stops_the_read_ends_it() {
        // A megabyte of zeros in about a kilobyte of code: the sink has seen
        // enough after the first piece, and the read goes no further.
        let coded = crate::deflate::zlib_compress(&vec![0u8; 1 << 20]);
        let mut pieces = 0;
        let clean = zlib_reads_clean(&coded, Tail::WhiteSpace, &mut |_: &[u8]| {
            pieces += 1;
            false
        });
        assert!(!clean);
        assert_eq!(pieces, 1);
    }

    #[test]
    fn the_checksum_is_carried_across_pieces() {
        let data = page_like(100_000);
        for split in [0, 1, 5551, 5552, 5553, 65_521, 99_999] {
            let (a, b) = data.split_at(split);
            assert_eq!(
                adler32_from(adler32(a), b),
                adler32(&data),
                "split at {split}"
            );
        }
    }
}
