//! Inflate (RFC 1951) and the zlib wrapper (RFC 1950).
//!
//! The core is a resumable state machine: every state either completes or
//! reports `NeedInput` without discarding the bits it already read, so the
//! one-shot wrapper below and a future streaming surface share the same
//! decoder. Back-references read the output `Vec` directly — decoding is to
//! memory, so the output doubles as the 32 KB window.

use crate::{Limits, Warning, Warnings};

const PRIMARY_BITS: u32 = 9;
const PRIMARY_SIZE: usize = 1 << PRIMARY_BITS;

// Table entry layout. Bit 31 marks a subtable pointer; otherwise bits 16..21
// hold the code length and bits 0..16 the symbol.
const ENTRY_SUB: u32 = 1 << 31;
const SUB_BITS_SHIFT: u32 = 24;
const OFFSET_MASK: u32 = 0x00ff_ffff;
const LEN_SHIFT: u32 = 16;
const LEN_MASK: u32 = 0x1f;
const SYM_MASK: u32 = 0xffff;

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

    /// `None` when the lengths over-subscribe the code space. Incomplete
    /// codes are accepted: their holes decode as invalid, which is what
    /// makes a one-distance-code block work.
    fn build(lengths: &[u8]) -> Option<Self> {
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
            let mut c = code;
            for _ in 0..count[len as usize] {
                if let Some(&sym) = sorted.get(k) {
                    codes.push((sym, len as u8, reverse_bits(c, len)));
                }
                k += 1;
                c += 1;
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
            let entry = (l << LEN_SHIFT) | u32::from(sym);
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
fn fixed_literal() -> Huffman {
    let mut lens = [0u8; 288];
    for (i, l) in lens.iter_mut().enumerate() {
        *l = match i {
            0..=143 => 8,
            144..=255 => 9,
            256..=279 => 7,
            _ => 8,
        };
    }
    Huffman::build(&lens).unwrap_or_else(Huffman::empty)
}

fn fixed_distance() -> Huffman {
    Huffman::build(&[5u8; 30]).unwrap_or_else(Huffman::empty)
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

enum Sym {
    Val(u16),
    NeedInput,
    Invalid,
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
        self.fill(src);
        let idx = (self.buf as usize) & (PRIMARY_SIZE - 1);
        let e = match t.primary.get(idx) {
            Some(&e) => e,
            None => return Sym::Invalid,
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
            return Sym::Val((e & SYM_MASK) as u16);
        }
        let bits = (e >> SUB_BITS_SHIFT) & 0xf;
        let off = (e & OFFSET_MASK) as usize;
        let j = ((self.buf >> PRIMARY_BITS) & ((1u64 << bits) - 1)) as usize;
        let se = match t.sub.get(off + j) {
            Some(&s) => s,
            None => return Sym::Invalid,
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
        Sym::Val((se & SYM_MASK) as u16)
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
    LenExtra(u16),
    DistSym(u16),
    DistExtra(u16, u16),
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

/// The resumable RFC 1951 decoder. Private on purpose: wave 1 exposes only
/// the one-shot API, and this is the shape a streaming surface would take.
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
        }
    }

    #[allow(clippy::too_many_lines)]
    fn run(&mut self, src: &mut Bits<'_>, out: &mut Vec<u8>, max_output: usize) -> Progress {
        loop {
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
                            Sym::Val(s) => s,
                            Sym::NeedInput => return Progress::NeedInput,
                            Sym::Invalid => return Progress::Corrupt,
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
                    let lit = match self.lens.get(..self.hlit).and_then(Huffman::build) {
                        Some(t) => t,
                        None => return Progress::Corrupt,
                    };
                    let dist = match self.lens.get(self.hlit..total).and_then(Huffman::build) {
                        Some(t) => t,
                        None => return Progress::Corrupt,
                    };
                    self.lit = lit;
                    self.dist = dist;
                    self.state = State::Symbol;
                }

                State::Symbol => {
                    let sym = match self.bits.decode(&self.lit, src) {
                        Sym::Val(s) => s,
                        Sym::NeedInput => return Progress::NeedInput,
                        Sym::Invalid => return Progress::Corrupt,
                    };
                    if sym < 256 {
                        if out.len() >= max_output {
                            return Progress::OutputCap;
                        }
                        out.push(sym as u8);
                    } else if sym == 256 {
                        self.state = if self.last {
                            State::Done
                        } else {
                            State::Header
                        };
                    } else {
                        let i = sym as usize - 257;
                        if i >= LEN_BASE.len() {
                            // 286 and 287 are in the alphabet but never used.
                            return Progress::Corrupt;
                        }
                        self.state = State::LenExtra(i as u16);
                    }
                }

                State::LenExtra(i) => {
                    let i = i as usize;
                    let (base, extra) = match (LEN_BASE.get(i), LEN_EXTRA.get(i)) {
                        (Some(&b), Some(&e)) => (b, u32::from(e)),
                        _ => return Progress::Corrupt,
                    };
                    if !self.bits.need(extra, src) {
                        return Progress::NeedInput;
                    }
                    let len = u32::from(base) + self.bits.take(extra);
                    self.state = State::DistSym(len as u16);
                }

                State::DistSym(len) => {
                    let sym = match self.bits.decode(&self.dist, src) {
                        Sym::Val(s) => s,
                        Sym::NeedInput => return Progress::NeedInput,
                        Sym::Invalid => return Progress::Corrupt,
                    };
                    if sym as usize >= DIST_BASE.len() {
                        return Progress::Corrupt;
                    }
                    self.state = State::DistExtra(len, sym);
                }

                State::DistExtra(len, sym) => {
                    let i = sym as usize;
                    let (base, extra) = match (DIST_BASE.get(i), DIST_EXTRA.get(i)) {
                        (Some(&b), Some(&e)) => (b, u32::from(e)),
                        _ => return Progress::Corrupt,
                    };
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
                    for k in 0..n {
                        match out.get(start + k) {
                            Some(&b) => out.push(b),
                            None => return Progress::Corrupt,
                        }
                    }
                    if n < want {
                        return Progress::OutputCap;
                    }
                    self.state = State::Symbol;
                }
            }
        }
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
    let (mut a, mut b) = (1u64, 0u64);
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
/// deflate when it does not.
pub(crate) fn flate_bytes(input: &[u8], limits: &Limits, w: &mut Warnings) -> (Vec<u8>, bool) {
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
                let end = start + r.end;
                // A missing checksum costs no data, so its absence is not
                // itself a leniency worth recording.
                if let Some([c0, c1, c2, c3]) = input.get(end..end + 4) {
                    let want = u32::from_be_bytes([*c0, *c1, *c2, *c3]);
                    if !fdict && want != adler32(&r.data) {
                        w.push(Warning::AdlerMismatch);
                        complete = false;
                    }
                    if input.len() > end + 4 {
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
    if r.complete && r.end < input.len() {
        w.push(Warning::TrailingGarbage);
    }
    (r.data, r.complete)
}

#[cfg(test)]
mod tests {
    use super::*;

    const CAP: Limits = Limits::new(1 << 20);

    fn raw(input: &[u8]) -> (Vec<u8>, bool, Vec<Warning>) {
        let mut w = Warnings::default();
        let r = raw_bytes(input, &CAP, &mut w);
        (r.data, r.complete, w.into_vec())
    }

    fn flate(input: &[u8]) -> (Vec<u8>, bool, Vec<Warning>) {
        let mut w = Warnings::default();
        let (d, c) = flate_bytes(input, &CAP, &mut w);
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
}
