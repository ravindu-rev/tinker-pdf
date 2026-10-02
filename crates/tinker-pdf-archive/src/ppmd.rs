//! PPMd variant H with 7-Zip's range coder: 7z coder `030401`.
//!
//! Prediction by partial matching. There is no dictionary and no match: every
//! byte is coded arithmetically from a model of the contexts that preceded it,
//! from the longest the model holds down to the empty one, escaping one order
//! at a time when the byte has not been seen after the longer context. The
//! model is rebuilt identically by encoder and decoder as they go, so the
//! decoder has to make **exactly** the encoder's decisions — every frequency
//! update, every rescale, every context it creates, and every allocation in a
//! fixed arena whose exhaustion throws the whole model away and starts again.
//!
//! # Where it was written from
//!
//! Dmitry Shkarin's PPMd var.H (2001), as 7-Zip carries it: `C/Ppmd7.c` (the
//! model and its allocator), `C/Ppmd7Dec.c` (the 7z range decoder and symbol
//! decoding) and `C/Ppmd.h`, `C/Ppmd7.h`, read at `ip7z/7zip` 26.02 on
//! 26 September 2026. All four headers put the files in the public domain —
//! "Igor Pavlov : Public domain", "PPMd var.H (2001): Dmitry Shkarin : Public
//! domain" — which is why it was read at all; 7-Zip's `PpmdDecoder.cpp`
//! (LGPL) was read only for the property check (at least five bytes, order 2
//! to 64, memory 2 KiB to `2^32 - 37`). There is no other specification.
//! What follows is a Rust transcription of that algorithm into a byte arena
//! addressed by `u32` offsets, which is what the C does too when its pointers
//! are 64-bit; the state layout inside the arena is kept byte for byte,
//! because the allocator's free-block gluing reads the first two bytes of
//! every 12-byte unit to tell a free block from a live one, and a layout that
//! differed would glue differently and restart the model at a different byte.
//!
//! **What adjudicates it is the 7z archive's CRC-32 over the original
//! bytes**, as for every coder in this crate. The fixtures are py7zr's
//! `FILTER_PPMD` over files made here (`tests/coders/`), at order 6 with
//! 16 MiB and at order 32 with **64 KiB**, the second small enough that the
//! arena fills and the model restarts again and again — the path a roomy
//! default never takes.
//!
//! # Untrusted bytes
//!
//! Ruling 1. The arena is the one allocation the archive sizes, bounded by
//! [`Limits::max_memory`] before it is made; the output by
//! [`Limits::max_unpacked`]. Every arena access is checked, and a model that
//! would step outside its arena — which a correct model never does, whatever
//! the input — is [`Error::Corrupt`] rather than a panic. The input only
//! chooses among the symbols the model offers, so it cannot corrupt the
//! model's structure; the checks are there so that belief is not load-bearing.

/// Resource bounds for one decode.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    /// The most bytes one stream may decode to.
    pub max_unpacked: usize,
    /// The largest model arena a stream's properties may ask for. 7z callers
    /// pass their folder cap,
    /// [`MAX_7Z_UNPACKED`](crate::sevenz::limits::MAX_7Z_UNPACKED). The arena
    /// lives only while its own stream decodes, so a folder holds one at a
    /// time; what else it holds meanwhile — the outputs of the coders already
    /// decoded for the same consumer — is bounded by that consumer's declared
    /// output where it is a filter (`sevenz`'s `feeders_fit`), and by the cap
    /// apiece where it is not.
    pub max_memory: usize,
}

/// Why a stream did not decode.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Error {
    /// Fewer than five property bytes, a model order outside 2–64, or an
    /// arena outside 2 KiB to `2^32 - 37` bytes — the ranges 7-Zip accepts.
    BadProperties,
    /// The properties ask for an arena past [`Limits::max_memory`], or the
    /// declared output is past [`Limits::max_unpacked`].
    TooLarge,
    /// The range coder's first byte is not zero, or its initial code is
    /// `FFFFFFFF`.
    BadRangeStart,
    /// The input ran out before the declared output did.
    Truncated,
    /// The stream decoded to a symbol the model cannot hold: a count past its
    /// context's total, or the end-of-data escape before the declared length.
    Corrupt,
}

impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(match self {
            Error::BadProperties => "PPMd properties outside the ranges 7-Zip accepts",
            Error::TooLarge => "a PPMd model or output past this build's cap",
            Error::BadRangeStart => "a range-coded stream with a bad first five bytes",
            Error::Truncated => "a PPMd stream that ends early",
            Error::Corrupt => "a PPMd stream the model cannot decode",
        })
    }
}

impl std::error::Error for Error {}

const MIN_ORDER: u32 = 2;
const MAX_ORDER: u32 = 64;
const MIN_MEM: u32 = 1 << 11;
const MAX_MEM: u32 = u32::MAX - 12 * 3;

const INT_BITS: u32 = 7;
const PERIOD_BITS: u32 = 7;
const BIN_SCALE: u32 = 1 << (INT_BITS + PERIOD_BITS);
const MAX_FREQ: u32 = 124;
const UNIT: u32 = 12;
/// `PPMD_N1 + PPMD_N2 + PPMD_N3 + PPMD_N4`: 4 + 4 + 4 + 26 block sizes.
const NUM_INDEXES: usize = 38;
const TOP: u32 = 1 << 24;

const EXP_ESCAPE: [u8; 16] = [25, 14, 9, 7, 5, 5, 4, 4, 4, 3, 3, 3, 2, 2, 2, 2];
const INIT_BIN_ESC: [u16; 8] = [
    0x3CDD, 0x1F3F, 0x59BF, 0x48F3, 0x64A1, 0x5ABC, 0x6632, 0x6051,
];

/// Secondary escape estimation: an adaptive escape frequency for contexts
/// whose longer relatives have been masked.
#[derive(Clone, Copy)]
struct See {
    summ: u16,
    shift: u8,
    count: u8,
}

impl See {
    fn update(&mut self) {
        if u32::from(self.shift) < PERIOD_BITS {
            self.count = self.count.wrapping_sub(1);
            if self.count == 0 {
                self.summ = self.summ.wrapping_shl(1);
                self.count = (3u32 << self.shift) as u8;
                self.shift += 1;
            }
        }
    }
}

/// The range decoder, 7z's variant: a zero byte, then a big-endian code.
struct Range<'a> {
    input: &'a [u8],
    at: usize,
    range: u32,
    code: u32,
    overrun: bool,
}

impl<'a> Range<'a> {
    fn new(input: &'a [u8]) -> Result<Self, Error> {
        let head = input.get(..5).ok_or(Error::Truncated)?;
        if head.first() != Some(&0) {
            return Err(Error::BadRangeStart);
        }
        let mut code = 0u32;
        for &byte in head.get(1..).unwrap_or_default() {
            code = (code << 8) | u32::from(byte);
        }
        if code == u32::MAX {
            return Err(Error::BadRangeStart);
        }
        Ok(Range {
            input,
            at: 5,
            range: u32::MAX,
            code,
            overrun: false,
        })
    }

    fn byte(&mut self) -> u32 {
        match self.input.get(self.at) {
            Some(&b) => {
                self.at += 1;
                u32::from(b)
            }
            None => {
                self.overrun = true;
                0
            }
        }
    }

    fn normalize_once(&mut self) {
        if self.range < TOP {
            self.code = (self.code << 8) | self.byte();
            self.range <<= 8;
        }
    }

    /// 7-Zip normalises at most twice after a decode, and that is the whole
    /// of it: a second byte is always enough for totals below 2^16.
    fn normalize(&mut self) {
        self.normalize_once();
        self.normalize_once();
    }

    fn threshold(&mut self, total: u32) -> Result<u32, Error> {
        self.range = self.range.checked_div(total).ok_or(Error::Corrupt)?;
        self.code.checked_div(self.range).ok_or(Error::Corrupt)
    }

    fn decode(&mut self, start: u32, size: u32) {
        self.code = self.code.wrapping_sub(start.wrapping_mul(self.range));
        self.range = self.range.wrapping_mul(size);
    }

    fn decode_final(&mut self, start: u32, size: u32) {
        self.decode(start, size);
        self.normalize();
    }
}

/// The model: its arena, its allocator and its adaptive tables.
///
/// Arena records, as the C lays them out (little-endian):
///
/// - a **state** is six bytes: symbol, frequency, successor (`u32`);
/// - a **context** is one twelve-byte unit: number of states (`u16`), then
///   either the summed frequency (`u16`) and a reference to its states
///   (`u32`), or — when it has one state — that state itself, from byte 2;
///   then its suffix (`u32`);
/// - a **free block** carries its free-list link in bytes 0–3, and while the
///   allocator glues, a stamp (`u16`, zero), its size in units (`u16`) and the
///   next block (`u32`).
struct Model {
    mem: Vec<u8>,
    size: u32,
    align_offset: u32,
    text: u32,
    units_start: u32,
    lo_unit: u32,
    hi_unit: u32,
    glue_count: u32,
    free_list: [u32; NUM_INDEXES],
    indx2units: [u8; NUM_INDEXES],
    units2indx: [u8; 128],
    ns2indx: [u8; 256],
    ns2bs_indx: [u8; 256],
    see: [[See; 16]; 25],
    dummy_see: See,
    bin_summ: [[u16; 64]; 128],
    min_context: u32,
    max_context: u32,
    found_state: u32,
    order_fall: u32,
    init_esc: u32,
    prev_success: u32,
    max_order: u32,
    hi_bits_flag: u32,
    run_length: i32,
    init_rl: i32,
    /// How many times the model has been built: once to start, and once per
    /// exhaustion of the arena. Counted so a fixture named for restarts can be
    /// held to having them.
    builds: u32,
}

/// Which escape estimator a masked context used: one of the table's, or the
/// dummy the order-0 context uses.
#[derive(Clone, Copy)]
enum SeeAt {
    Table(usize, usize),
    Dummy,
}

fn hi_bits_3(symbol: u8) -> u32 {
    ((u32::from(symbol) + 0xC0) >> 5) & 8
}

fn hi_bits_4(symbol: u8) -> u32 {
    ((u32::from(symbol) + 0xC0) >> 4) & 16
}

impl Model {
    fn new(order: u32, size: u32) -> Model {
        let align_offset = 4u32.wrapping_sub(size) & 3;
        let mut model = Model {
            mem: vec![0u8; (align_offset as usize).saturating_add(size as usize)],
            size,
            align_offset,
            text: 0,
            units_start: 0,
            lo_unit: 0,
            hi_unit: 0,
            glue_count: 0,
            free_list: [0; NUM_INDEXES],
            indx2units: [0; NUM_INDEXES],
            units2indx: [0; 128],
            ns2indx: [0; 256],
            ns2bs_indx: [0; 256],
            see: [[See {
                summ: 0,
                shift: 0,
                count: 0,
            }; 16]; 25],
            dummy_see: See {
                summ: 0,
                shift: 0,
                count: 0,
            },
            bin_summ: [[0; 64]; 128],
            min_context: 0,
            max_context: 0,
            found_state: 0,
            order_fall: 0,
            init_esc: 0,
            prev_success: 0,
            max_order: order,
            hi_bits_flag: 0,
            run_length: 0,
            init_rl: 0,
            builds: 0,
        };
        // Block sizes in units: 1-4 by one, 6-12 by two, 15-24 by three, then
        // by four to 128.
        let mut k = 0usize;
        for i in 0..NUM_INDEXES {
            let step = if i >= 12 { 4 } else { (i >> 2) + 1 };
            for _ in 0..step {
                if let Some(slot) = model.units2indx.get_mut(k) {
                    *slot = i as u8;
                }
                k += 1;
            }
            model.indx2units[i] = k as u8;
        }
        model.ns2bs_indx[0] = 0;
        model.ns2bs_indx[1] = 2;
        for slot in &mut model.ns2bs_indx[2..11] {
            *slot = 4;
        }
        for slot in &mut model.ns2bs_indx[11..] {
            *slot = 6;
        }
        for i in 0..3 {
            model.ns2indx[i] = i as u8;
        }
        let (mut m, mut step) = (3u32, 1u32);
        for slot in &mut model.ns2indx[3..] {
            *slot = m as u8;
            step -= 1;
            if step == 0 {
                m += 1;
                step = m - 2;
            }
        }
        model.restart();
        model
    }

    // ---- The arena --------------------------------------------------------

    fn u8_at(&self, at: u32) -> u8 {
        self.mem.get(at as usize).copied().unwrap_or(0)
    }

    fn set_u8(&mut self, at: u32, value: u8) {
        if let Some(slot) = self.mem.get_mut(at as usize) {
            *slot = value;
        }
    }

    fn u16_at(&self, at: u32) -> u16 {
        u16::from(self.u8_at(at)) | (u16::from(self.u8_at(at.wrapping_add(1))) << 8)
    }

    fn set_u16(&mut self, at: u32, value: u16) {
        self.set_u8(at, value as u8);
        self.set_u8(at.wrapping_add(1), (value >> 8) as u8);
    }

    fn u32_at(&self, at: u32) -> u32 {
        u32::from(self.u16_at(at)) | (u32::from(self.u16_at(at.wrapping_add(2))) << 16)
    }

    fn set_u32(&mut self, at: u32, value: u32) {
        self.set_u16(at, value as u16);
        self.set_u16(at.wrapping_add(2), (value >> 16) as u16);
    }

    /// Copies `units` twelve-byte units; the two ranges never overlap.
    fn copy_units(&mut self, to: u32, from: u32, units: u32) {
        let (to, from, len) = (to as usize, from as usize, (units * UNIT) as usize);
        if from.saturating_add(len) <= self.mem.len() && to.saturating_add(len) <= self.mem.len() {
            self.mem.copy_within(from..from + len, to);
        }
    }

    // State fields.
    fn symbol(&self, s: u32) -> u8 {
        self.u8_at(s)
    }
    fn freq(&self, s: u32) -> u32 {
        u32::from(self.u8_at(s.wrapping_add(1)))
    }
    fn set_freq(&mut self, s: u32, freq: u32) {
        self.set_u8(s.wrapping_add(1), freq as u8);
    }
    fn successor(&self, s: u32) -> u32 {
        self.u32_at(s.wrapping_add(2))
    }
    fn set_successor(&mut self, s: u32, v: u32) {
        self.set_u32(s.wrapping_add(2), v);
    }
    fn copy_state(&mut self, to: u32, from: u32) {
        for k in 0..6 {
            let b = self.u8_at(from.wrapping_add(k));
            self.set_u8(to.wrapping_add(k), b);
        }
    }
    fn swap_states(&mut self, a: u32, b: u32) {
        for k in 0..6 {
            let x = self.u8_at(a.wrapping_add(k));
            let y = self.u8_at(b.wrapping_add(k));
            self.set_u8(a.wrapping_add(k), y);
            self.set_u8(b.wrapping_add(k), x);
        }
    }

    // Context fields.
    fn num_stats(&self, c: u32) -> u32 {
        u32::from(self.u16_at(c))
    }
    fn set_num_stats(&mut self, c: u32, n: u32) {
        self.set_u16(c, n as u16);
    }
    fn summ_freq(&self, c: u32) -> u32 {
        u32::from(self.u16_at(c.wrapping_add(2)))
    }
    fn set_summ_freq(&mut self, c: u32, v: u32) {
        self.set_u16(c.wrapping_add(2), v as u16);
    }
    fn stats(&self, c: u32) -> u32 {
        self.u32_at(c.wrapping_add(4))
    }
    fn set_stats(&mut self, c: u32, v: u32) {
        self.set_u32(c.wrapping_add(4), v);
    }
    fn suffix(&self, c: u32) -> u32 {
        self.u32_at(c.wrapping_add(8))
    }
    fn set_suffix(&mut self, c: u32, v: u32) {
        self.set_u32(c.wrapping_add(8), v);
    }
    /// A one-state context's state lives inside the context, from byte 2.
    fn one_state(c: u32) -> u32 {
        c.wrapping_add(2)
    }

    /// The state in `c`'s list whose symbol is `symbol`. A correct model
    /// always has one; a search that runs off the list is a corrupt model.
    fn find(&self, c: u32, symbol: u8) -> Result<u32, Error> {
        let stats = self.stats(c);
        (0..self.num_stats(c))
            .map(|i| stats.wrapping_add(i * 6))
            .find(|&s| self.symbol(s) == symbol)
            .ok_or(Error::Corrupt)
    }

    // ---- The allocator ----------------------------------------------------

    fn i2u(&self, index: usize) -> u32 {
        u32::from(self.indx2units.get(index).copied().unwrap_or(0))
    }

    fn u2i(&self, units: u32) -> usize {
        usize::from(
            self.units2indx
                .get((units as usize).wrapping_sub(1))
                .copied()
                .unwrap_or(0),
        )
    }

    fn insert_node(&mut self, node: u32, index: usize) {
        let head = self.free_list.get(index).copied().unwrap_or(0);
        self.set_u32(node, head);
        if let Some(slot) = self.free_list.get_mut(index) {
            *slot = node;
        }
    }

    fn remove_node(&mut self, index: usize) -> u32 {
        let node = self.free_list.get(index).copied().unwrap_or(0);
        let next = self.u32_at(node);
        if let Some(slot) = self.free_list.get_mut(index) {
            *slot = next;
        }
        node
    }

    fn split_block(&mut self, ptr: u32, old_index: usize, new_index: usize) {
        let nu = self.i2u(old_index).wrapping_sub(self.i2u(new_index));
        let ptr = ptr.wrapping_add(self.i2u(new_index) * UNIT);
        let mut i = self.u2i(nu);
        if self.i2u(i) != nu {
            i = i.wrapping_sub(1);
            let k = self.i2u(i);
            self.insert_node(ptr.wrapping_add(k * UNIT), (nu - k - 1) as usize);
        }
        self.insert_node(ptr, i);
    }

    fn glue_free_blocks(&mut self) {
        self.glue_count = 255;
        // A guard where the free gap starts, so a merge stops there.
        if self.lo_unit != self.hi_unit {
            self.set_u16(self.lo_unit, 1);
        }
        // Every free block onto one list, stamped free with its size.
        let mut n: u32 = 0;
        for i in 0..NUM_INDEXES {
            let nu = self.i2u(i) as u16;
            let mut next = self.free_list[i];
            self.free_list[i] = 0;
            while next != 0 {
                let node = next;
                next = self.u32_at(node);
                self.set_u16(node, 0);
                self.set_u16(node.wrapping_add(2), nu);
                self.set_u32(node.wrapping_add(4), n);
                n = node;
            }
        }
        let head = n;
        // Glue each block to the free blocks that follow it in the arena.
        let mut prev: Option<u32> = None;
        let mut first = head;
        let mut n = head;
        let mut steps = 0usize;
        while n != 0 && steps <= self.mem.len() {
            steps += 1;
            let node = n;
            let mut nu = u32::from(self.u16_at(node.wrapping_add(2)));
            n = self.u32_at(node.wrapping_add(4));
            if nu == 0 {
                match prev {
                    Some(p) => self.set_u32(p.wrapping_add(4), n),
                    None => first = n,
                }
                continue;
            }
            prev = Some(node);
            loop {
                let node2 = node.wrapping_add(nu * UNIT);
                nu += u32::from(self.u16_at(node2.wrapping_add(2)));
                if self.u16_at(node2) != 0 || nu >= 0x10000 {
                    break;
                }
                self.set_u16(node.wrapping_add(2), nu as u16);
                self.set_u16(node2.wrapping_add(2), 0);
            }
        }
        // And back onto the lists by size, 128 units at a time.
        let mut n = first;
        let mut steps = 0usize;
        while n != 0 && steps <= self.mem.len() {
            steps += 1;
            let mut node = n;
            let mut nu = u32::from(self.u16_at(node.wrapping_add(2)));
            n = self.u32_at(node.wrapping_add(4));
            if nu == 0 {
                continue;
            }
            while nu > 128 {
                self.insert_node(node, NUM_INDEXES - 1);
                nu -= 128;
                node = node.wrapping_add(128 * UNIT);
            }
            let mut i = self.u2i(nu);
            if self.i2u(i) != nu {
                i -= 1;
                let k = self.i2u(i);
                self.insert_node(node.wrapping_add(k * UNIT), (nu - k - 1) as usize);
            }
            self.insert_node(node, i);
        }
    }

    fn alloc_units_rare(&mut self, index: usize) -> Option<u32> {
        if self.glue_count == 0 {
            self.glue_free_blocks();
            if self.free_list.get(index).copied().unwrap_or(0) != 0 {
                return Some(self.remove_node(index));
            }
        }
        let mut i = index;
        loop {
            i += 1;
            if i == NUM_INDEXES {
                let bytes = self.i2u(index) * UNIT;
                self.glue_count = self.glue_count.wrapping_sub(1);
                return if self.units_start.wrapping_sub(self.text) > bytes {
                    self.units_start -= bytes;
                    Some(self.units_start)
                } else {
                    None
                };
            }
            if self.free_list[i] != 0 {
                break;
            }
        }
        let block = self.remove_node(i);
        self.split_block(block, i, index);
        Some(block)
    }

    fn alloc_units(&mut self, index: usize) -> Option<u32> {
        if self.free_list.get(index).copied().unwrap_or(0) != 0 {
            return Some(self.remove_node(index));
        }
        let bytes = self.i2u(index) * UNIT;
        let lo = self.lo_unit;
        if self.hi_unit.wrapping_sub(lo) >= bytes {
            self.lo_unit = lo + bytes;
            return Some(lo);
        }
        self.alloc_units_rare(index)
    }

    fn alloc_context(&mut self) -> Option<u32> {
        if self.hi_unit != self.lo_unit {
            self.hi_unit -= UNIT;
            Some(self.hi_unit)
        } else if self.free_list[0] != 0 {
            Some(self.remove_node(0))
        } else {
            self.alloc_units_rare(0)
        }
    }

    // ---- The model ----------------------------------------------------------

    fn restart(&mut self) {
        self.builds = self.builds.saturating_add(1);
        self.free_list = [0; NUM_INDEXES];
        self.text = self.align_offset;
        self.hi_unit = self.text + self.size;
        self.units_start = self.hi_unit - self.size / 8 / UNIT * 7 * UNIT;
        self.lo_unit = self.units_start;
        self.glue_count = 0;

        self.order_fall = self.max_order;
        self.init_rl = -(self.max_order.min(12) as i32) - 1;
        self.run_length = self.init_rl;
        self.prev_success = 0;

        self.hi_unit -= UNIT;
        let mc = self.hi_unit;
        let s = self.lo_unit;
        self.lo_unit += 128 * UNIT;
        self.max_context = mc;
        self.min_context = mc;
        self.found_state = s;
        self.set_num_stats(mc, 256);
        self.set_summ_freq(mc, 257);
        self.set_stats(mc, s);
        self.set_suffix(mc, 0);
        for i in 0..256u32 {
            let state = s + i * 6;
            self.set_u8(state, i as u8);
            self.set_freq(state, 1);
            self.set_successor(state, 0);
        }

        for (i, row) in self.bin_summ.iter_mut().enumerate() {
            for (k, &esc) in INIT_BIN_ESC.iter().enumerate() {
                let value = (BIN_SCALE - u32::from(esc) / (i as u32 + 2)) as u16;
                for m in (0..64).step_by(8) {
                    row[k + m] = value;
                }
            }
        }
        for (i, row) in self.see.iter_mut().enumerate() {
            let summ = ((5 * i as u32 + 10) << (PERIOD_BITS - 4)) as u16;
            for see in row.iter_mut() {
                *see = See {
                    summ,
                    shift: (PERIOD_BITS - 4) as u8,
                    count: 4,
                };
            }
        }
        self.dummy_see = See {
            summ: 0,
            shift: PERIOD_BITS as u8,
            count: 64,
        };
    }

    /// Creates the contexts a raw successor stands for. `Ok(None)` is an
    /// arena with no room, which restarts the model.
    fn create_successors(&mut self) -> Result<Option<u32>, Error> {
        let mut c = self.min_context;
        let mut up_branch = self.successor(self.found_state);
        let symbol = self.symbol(self.found_state);
        let mut ps: Vec<u32> = Vec::with_capacity(MAX_ORDER as usize);
        if self.order_fall != 0 {
            ps.push(self.found_state);
        }
        while self.suffix(c) != 0 {
            c = self.suffix(c);
            let s = if self.num_stats(c) != 1 {
                self.find(c, symbol)?
            } else {
                Self::one_state(c)
            };
            let successor = self.successor(s);
            if successor != up_branch {
                c = successor;
                if ps.is_empty() {
                    return Ok(Some(c));
                }
                break;
            }
            if ps.len() >= MAX_ORDER as usize {
                return Err(Error::Corrupt);
            }
            ps.push(s);
        }

        let new_symbol = self.u8_at(up_branch);
        up_branch = up_branch.wrapping_add(1);
        let new_freq = if self.num_stats(c) == 1 {
            self.freq(Self::one_state(c))
        } else {
            let s = self.find(c, new_symbol)?;
            let cf = self.freq(s).wrapping_sub(1);
            let s0 = self
                .summ_freq(c)
                .wrapping_sub(self.num_stats(c))
                .wrapping_sub(cf);
            1 + if 2 * cf <= s0 {
                u32::from(5 * cf > s0)
            } else {
                (2 * cf + s0 - 1)
                    .checked_div(2 * s0)
                    .ok_or(Error::Corrupt)?
                    + 1
            }
        };

        while let Some(state) = ps.pop() {
            let Some(c1) = self.alloc_context() else {
                return Ok(None);
            };
            self.set_num_stats(c1, 1);
            let one = Self::one_state(c1);
            self.set_u8(one, new_symbol);
            self.set_freq(one, new_freq);
            self.set_successor(one, up_branch);
            self.set_suffix(c1, c);
            self.set_successor(state, c1);
            c = c1;
        }
        Ok(Some(c))
    }

    fn update_model(&mut self) -> Result<(), Error> {
        let fs = self.found_state;
        let fs_symbol = self.symbol(fs);
        let fs_freq = self.freq(fs);

        if fs_freq < MAX_FREQ / 4 && self.suffix(self.min_context) != 0 {
            let c = self.suffix(self.min_context);
            if self.num_stats(c) == 1 {
                let s = Self::one_state(c);
                if self.freq(s) < 32 {
                    self.set_freq(s, self.freq(s) + 1);
                }
            } else {
                let mut s = self.stats(c);
                if self.symbol(s) != fs_symbol {
                    s = self.find(c, fs_symbol)?;
                    if self.freq(s) >= self.freq(s - 6) {
                        self.swap_states(s, s - 6);
                        s -= 6;
                    }
                }
                if self.freq(s) < MAX_FREQ - 9 {
                    self.set_freq(s, self.freq(s) + 2);
                    self.set_summ_freq(c, self.summ_freq(c) + 2);
                }
            }
        }

        if self.order_fall == 0 {
            match self.create_successors()? {
                Some(c) => {
                    self.min_context = c;
                    self.max_context = c;
                    self.set_successor(self.found_state, c);
                }
                None => self.restart(),
            }
            return Ok(());
        }

        self.set_u8(self.text, fs_symbol);
        self.text += 1;
        if self.text >= self.units_start {
            self.restart();
            return Ok(());
        }
        let mut max_successor = self.text;
        let mut min_successor = self.successor(self.found_state);

        if min_successor != 0 {
            if min_successor <= max_successor {
                match self.create_successors()? {
                    Some(cs) => min_successor = cs,
                    None => {
                        self.restart();
                        return Ok(());
                    }
                }
            }
            self.order_fall -= 1;
            if self.order_fall == 0 {
                max_successor = min_successor;
                if self.max_context != self.min_context {
                    self.text -= 1;
                }
            }
        } else {
            self.set_successor(self.found_state, max_successor);
            min_successor = self.min_context;
        }

        let mc = self.min_context;
        let mut c = self.max_context;
        self.max_context = min_successor;
        self.min_context = min_successor;
        if c == mc {
            return Ok(());
        }

        let ns = self.num_stats(mc);
        let s0 = self
            .summ_freq(mc)
            .wrapping_sub(ns)
            .wrapping_sub(self.freq(self.found_state).wrapping_sub(1));

        let mut steps = 0u32;
        loop {
            steps += 1;
            if steps > MAX_ORDER + 1 {
                return Err(Error::Corrupt);
            }
            let ns1 = self.num_stats(c);
            let mut sum;
            if ns1 != 1 {
                if ns1 & 1 == 0 {
                    let old_nu = ns1 >> 1;
                    let i = self.u2i(old_nu);
                    if i != self.u2i(old_nu + 1) {
                        let Some(ptr) = self.alloc_units(i + 1) else {
                            self.restart();
                            return Ok(());
                        };
                        let old = self.stats(c);
                        self.copy_units(ptr, old, old_nu);
                        self.insert_node(old, i);
                        self.set_stats(c, ptr);
                    }
                }
                sum = self.summ_freq(c);
                sum += u32::from(2 * ns1 < ns)
                    + 2 * (u32::from(4 * ns1 <= ns) & u32::from(sum <= 8 * ns1));
            } else {
                let Some(s) = self.alloc_units(0) else {
                    self.restart();
                    return Ok(());
                };
                let one = Self::one_state(c);
                let mut freq = self.freq(one);
                self.copy_state(s, one);
                self.set_stats(c, s);
                freq = if freq < MAX_FREQ / 4 - 1 {
                    freq << 1
                } else {
                    MAX_FREQ - 4
                };
                self.set_freq(s, freq);
                sum = freq + self.init_esc + u32::from(ns > 3);
            }

            let s = self.stats(c) + ns1 * 6;
            let mut cf = 2 * (sum + 6) * self.freq(self.found_state);
            let sf = s0.wrapping_add(sum);
            self.set_u8(s, fs_symbol);
            self.set_num_stats(c, ns1 + 1);
            self.set_successor(s, max_successor);
            if cf < 6 * sf {
                cf = 1 + u32::from(cf > sf) + u32::from(cf >= 4 * sf);
                sum += 3;
            } else {
                cf = 4
                    + u32::from(cf >= 9 * sf)
                    + u32::from(cf >= 12 * sf)
                    + u32::from(cf >= 15 * sf);
                sum += cf;
            }
            self.set_summ_freq(c, sum);
            self.set_freq(s, cf);
            c = self.suffix(c);
            if c == mc {
                return Ok(());
            }
        }
    }

    fn rescale(&mut self) {
        let mc = self.min_context;
        let stats = self.stats(mc);
        let mut s = self.found_state;
        // The found state to the front.
        if s != stats {
            let mut tmp = [0u8; 6];
            for (k, b) in tmp.iter_mut().enumerate() {
                *b = self.u8_at(s + k as u32);
            }
            while s != stats {
                self.copy_state(s, s - 6);
                s -= 6;
            }
            for (k, &b) in tmp.iter().enumerate() {
                self.set_u8(stats + k as u32, b);
            }
        }
        let mut sum_freq = self.freq(stats);
        let mut esc_freq = self.summ_freq(mc).wrapping_sub(sum_freq);
        let adder = u32::from(self.order_fall != 0);
        sum_freq = (sum_freq + 4 + adder) >> 1;
        let count = self.num_stats(mc).saturating_sub(1);
        self.set_freq(stats, sum_freq);
        let mut s = stats;
        for _ in 0..count {
            s += 6;
            let mut freq = self.freq(s);
            esc_freq = esc_freq.wrapping_sub(freq);
            freq = (freq + adder) >> 1;
            sum_freq += freq;
            self.set_freq(s, freq);
            if freq > self.freq(s - 6) {
                let mut tmp = [0u8; 6];
                for (k, b) in tmp.iter_mut().enumerate() {
                    *b = self.u8_at(s + k as u32);
                }
                let mut s1 = s;
                loop {
                    self.copy_state(s1, s1 - 6);
                    s1 -= 6;
                    if s1 == stats || freq <= self.freq(s1 - 6) {
                        break;
                    }
                }
                for (k, &b) in tmp.iter().enumerate() {
                    self.set_u8(s1 + k as u32, b);
                }
            }
        }

        if self.freq(s) == 0 {
            let mut removed = 0u32;
            loop {
                removed += 1;
                s -= 6;
                if self.freq(s) != 0 || s == stats {
                    break;
                }
            }
            esc_freq += removed;
            let num_stats = self.num_stats(mc);
            let new_stats = num_stats - removed;
            self.set_num_stats(mc, new_stats);
            let n0 = (num_stats + 1) >> 1;
            if new_stats == 1 {
                let mut freq = self.freq(stats);
                loop {
                    esc_freq >>= 1;
                    freq = (freq + 1) >> 1;
                    if esc_freq <= 1 {
                        break;
                    }
                }
                let one = Self::one_state(mc);
                self.copy_state(one, stats);
                self.set_freq(one, freq);
                self.found_state = one;
                let index = self.u2i(n0);
                self.insert_node(stats, index);
                return;
            }
            let n1 = (new_stats + 1) >> 1;
            if n0 != n1 {
                let i0 = self.u2i(n0);
                let i1 = self.u2i(n1);
                if i0 != i1 {
                    if self.free_list[i1] != 0 {
                        let ptr = self.remove_node(i1);
                        self.set_stats(mc, ptr);
                        self.copy_units(ptr, stats, n1);
                        self.insert_node(stats, i0);
                    } else {
                        self.split_block(stats, i0, i1);
                    }
                }
            }
        }
        self.set_summ_freq(mc, sum_freq + esc_freq - (esc_freq >> 1));
        self.found_state = self.stats(mc);
    }

    fn make_esc_freq(&mut self, num_masked: u32) -> (SeeAt, u32) {
        let mc = self.min_context;
        let num_stats = self.num_stats(mc);
        if num_stats == 256 {
            return (SeeAt::Dummy, 1);
        }
        let non_masked = num_stats.wrapping_sub(num_masked);
        let row = usize::from(
            self.ns2indx
                .get(non_masked.wrapping_sub(1) as usize)
                .copied()
                .unwrap_or(0),
        );
        let column =
            u32::from(non_masked < self.num_stats(self.suffix(mc)).wrapping_sub(num_stats))
                + 2 * u32::from(self.summ_freq(mc) < 11 * num_stats)
                + 4 * u32::from(num_masked > non_masked)
                + self.hi_bits_flag;
        let at = SeeAt::Table(row, column as usize);
        let see = self.see_mut(at);
        let summ = u32::from(see.summ);
        let r = summ >> see.shift;
        see.summ = (summ - r) as u16;
        (at, r + u32::from(r == 0))
    }

    fn see_mut(&mut self, at: SeeAt) -> &mut See {
        match at {
            SeeAt::Table(row, column) => self
                .see
                .get_mut(row)
                .and_then(|r| r.get_mut(column))
                .unwrap_or(&mut self.dummy_see),
            SeeAt::Dummy => &mut self.dummy_see,
        }
    }

    fn next_context(&mut self) -> Result<(), Error> {
        let c = self.successor(self.found_state);
        if self.order_fall == 0 && c > self.text {
            self.min_context = c;
            self.max_context = c;
            Ok(())
        } else {
            self.update_model()
        }
    }

    fn update1(&mut self) -> Result<(), Error> {
        let s = self.found_state;
        let freq = self.freq(s) + 4;
        let mc = self.min_context;
        self.set_summ_freq(mc, self.summ_freq(mc) + 4);
        self.set_freq(s, freq);
        if freq > self.freq(s - 6) {
            self.swap_states(s, s - 6);
            self.found_state = s - 6;
            if freq > MAX_FREQ {
                self.rescale();
            }
        }
        self.next_context()
    }

    fn update1_0(&mut self) -> Result<(), Error> {
        let s = self.found_state;
        let mc = self.min_context;
        let freq = self.freq(s);
        let summ = self.summ_freq(mc);
        self.prev_success = u32::from(2 * freq > summ);
        self.run_length += self.prev_success as i32;
        self.set_summ_freq(mc, summ + 4);
        self.set_freq(s, freq + 4);
        if freq + 4 > MAX_FREQ {
            self.rescale();
        }
        self.next_context()
    }

    fn update2(&mut self) -> Result<(), Error> {
        let s = self.found_state;
        let freq = self.freq(s) + 4;
        let mc = self.min_context;
        self.run_length = self.init_rl;
        self.set_summ_freq(mc, self.summ_freq(mc) + 4);
        self.set_freq(s, freq);
        if freq > MAX_FREQ {
            self.rescale();
        }
        self.update_model()
    }

    /// One symbol. `Ok(None)` is the end-of-data escape (an escape out of
    /// the order-0 context), which 7z's writers may put after the last byte.
    fn decode_symbol(&mut self, rc: &mut Range<'_>) -> Result<Option<u8>, Error> {
        let mut mask = [true; 256];
        let mc = self.min_context;
        if self.num_stats(mc) != 1 {
            let mut s = self.stats(mc);
            let summ = self.summ_freq(mc);
            let mut count = rc.threshold(summ)?;
            let hi = count;
            count = count.wrapping_sub(self.freq(s));
            if (count as i32) < 0 {
                rc.decode_final(0, self.freq(s));
                self.found_state = s;
                let symbol = self.symbol(s);
                self.update1_0()?;
                return Ok(Some(symbol));
            }
            self.prev_success = 0;
            for _ in 1..self.num_stats(mc) {
                s += 6;
                count = count.wrapping_sub(self.freq(s));
                if (count as i32) < 0 {
                    let freq = self.freq(s);
                    rc.decode_final(hi.wrapping_sub(count).wrapping_sub(freq), freq);
                    self.found_state = s;
                    let symbol = self.symbol(s);
                    self.update1()?;
                    return Ok(Some(symbol));
                }
            }
            if hi >= summ {
                return Err(Error::Corrupt);
            }
            let hi = hi.wrapping_sub(count);
            rc.decode(hi, summ - hi);
            self.hi_bits_flag = hi_bits_3(self.symbol(self.found_state));
            let stats = self.stats(mc);
            for i in 0..self.num_stats(mc) {
                mask[usize::from(self.symbol(stats + i * 6))] = false;
            }
        } else {
            let s = Self::one_state(mc);
            let freq = self.freq(s) as usize;
            let suffix_stats = self.num_stats(self.suffix(mc)) as usize;
            self.hi_bits_flag = hi_bits_3(self.symbol(self.found_state));
            let row = freq.wrapping_sub(1);
            let column = self.prev_success as usize
                + ((self.run_length >> 26) & 0x20) as usize
                + usize::from(
                    self.ns2bs_indx
                        .get(suffix_stats.wrapping_sub(1))
                        .copied()
                        .unwrap_or(0),
                )
                + hi_bits_4(self.symbol(s)) as usize
                + self.hi_bits_flag as usize;
            let prob = self
                .bin_summ
                .get_mut(row)
                .and_then(|r| r.get_mut(column))
                .ok_or(Error::Corrupt)?;
            let mut pr = u32::from(*prob);
            let size0 = (rc.range >> 14).wrapping_mul(pr);
            pr = pr - ((pr + (1 << (PERIOD_BITS - 2))) >> PERIOD_BITS);
            if rc.code < size0 {
                *prob = (pr + (1 << INT_BITS)) as u16;
                rc.range = size0;
                rc.normalize_once();
                let freq = self.freq(s);
                let c = self.successor(s);
                let symbol = self.symbol(s);
                self.found_state = s;
                self.prev_success = 1;
                self.run_length += 1;
                self.set_freq(s, freq + u32::from(freq < 128));
                if self.order_fall == 0 && c > self.text {
                    self.min_context = c;
                    self.max_context = c;
                } else {
                    self.update_model()?;
                }
                return Ok(Some(symbol));
            }
            *prob = pr as u16;
            self.init_esc = u32::from(EXP_ESCAPE[(pr >> 10) as usize & 15]);
            rc.code = rc.code.wrapping_sub(size0);
            rc.range = rc.range.wrapping_sub(size0);
            mask[usize::from(self.symbol(s))] = false;
            self.prev_success = 0;
        }

        loop {
            rc.normalize();
            let mut mc = self.min_context;
            let num_masked = self.num_stats(mc);
            loop {
                self.order_fall += 1;
                if self.suffix(mc) == 0 {
                    return Ok(None);
                }
                mc = self.suffix(mc);
                if self.num_stats(mc) != num_masked {
                    break;
                }
            }
            let stats = self.stats(mc);
            let n = self.num_stats(mc);
            let mut hi = 0u32;
            for i in 0..n {
                let s = stats + i * 6;
                if mask[usize::from(self.symbol(s))] {
                    hi += self.freq(s);
                }
            }
            self.min_context = mc;
            let (see, esc) = self.make_esc_freq(num_masked);
            let total = esc + hi;
            let mut count = rc.threshold(total)?;
            if count < hi {
                let target = count;
                let mut s = stats;
                let mut found = None;
                for i in 0..n {
                    s = stats + i * 6;
                    if mask[usize::from(self.symbol(s))] {
                        count = count.wrapping_sub(self.freq(s));
                        if (count as i32) < 0 {
                            found = Some(s);
                            break;
                        }
                    }
                }
                let s = found.ok_or(Error::Corrupt).map(|_| s)?;
                let freq = self.freq(s);
                rc.decode_final(target.wrapping_sub(count).wrapping_sub(freq), freq);
                self.see_mut(see).update();
                self.found_state = s;
                let symbol = self.symbol(s);
                self.update2()?;
                return Ok(Some(symbol));
            }
            if count >= total {
                return Err(Error::Corrupt);
            }
            rc.decode(hi, total - hi);
            let see = self.see_mut(see);
            see.summ = see.summ.wrapping_add(total as u16);
            for i in 0..n {
                mask[usize::from(self.symbol(stats + i * 6))] = false;
            }
        }
    }
}

/// Decodes a 7z PPMd stream (coder `030401`) of `unpacked` bytes.
///
/// `props` is the coder's property blob: the model order, then the arena
/// size, little-endian — five bytes, and 7-Zip accepts more and reads the
/// five (py7zr writes seven).
///
/// # Errors
/// [`Error`], one variant per way the input is not this.
pub fn decode(
    input: &[u8],
    props: &[u8],
    unpacked: usize,
    limits: &Limits,
) -> Result<Vec<u8>, Error> {
    decode_counting(input, props, unpacked, limits).map(|(out, _)| out)
}

/// [`decode`], and how many times the arena filled and the model restarted.
pub(crate) fn decode_counting(
    input: &[u8],
    props: &[u8],
    unpacked: usize,
    limits: &Limits,
) -> Result<(Vec<u8>, u32), Error> {
    let [order, m0, m1, m2, m3, ..] = *props else {
        return Err(Error::BadProperties);
    };
    let order = u32::from(order);
    let memory = u32::from_le_bytes([m0, m1, m2, m3]);
    if !(MIN_ORDER..=MAX_ORDER).contains(&order) || !(MIN_MEM..=MAX_MEM).contains(&memory) {
        return Err(Error::BadProperties);
    }
    if memory as usize > limits.max_memory || unpacked > limits.max_unpacked {
        return Err(Error::TooLarge);
    }
    let mut rc = Range::new(input)?;
    let mut model = Model::new(order, memory);
    let mut out = Vec::with_capacity(unpacked.min(1 << 20));
    while out.len() < unpacked {
        match model.decode_symbol(&mut rc)? {
            Some(symbol) => out.push(symbol),
            None => return Err(Error::Corrupt),
        }
        if rc.overrun {
            return Err(Error::Truncated);
        }
    }
    Ok((out, model.builds.saturating_sub(1)))
}

#[cfg(test)]
mod tests;
