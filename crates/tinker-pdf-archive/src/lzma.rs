//! LZMA and LZMA2, hand-rolled: the range decoder, the probability model and
//! the LZ77 window.
//!
//! Written for `sevenz`, which needs **both**: 7-Zip compresses a `.7z`'s own
//! header with plain LZMA (coder `030101`) and its file data with LZMA2 (coder
//! `21`), so a reader that had only one of the two could not list the archive
//! it could decompress, or the reverse. They are one engine and two front ends
//! rather than two decoders — [`decode_lzma2`] is a chunk framing over exactly
//! the `State` this module already had — which is why they share a file.
//!
//! # No `lzma-rs`, and the reason is written down in `deny.toml`
//!
//! CONTRIBUTING rule 1. The range decoder below, the eleven probability arrays
//! and the distance model are transcribed from the format, and `deny.toml`
//! names `lzma-rs`, `xz2` and `liblzma` so that reaching for one is a build
//! failure rather than a judgement call.
//!
//! # The window is the output, and that is deliberate
//!
//! A general LZMA decoder keeps a circular dictionary because it streams. This
//! one does not stream: every caller here knows the unpacked size before it
//! starts, because 7z records it in `kCodersUnpackSize` and LZMA2 records it
//! per chunk. So the "dictionary" is simply the output written so far, a match
//! is a copy from earlier in the same `Vec`, and a distance that reaches past
//! the start is [`Error::DistanceTooFar`] rather than a wrap-around to
//! whatever the circular buffer happened to hold.
//!
//! That removes the whole class of bug this decoder would otherwise be most
//! likely to have — a wrong modulus on the window, which produces plausible
//! bytes rather than an error — and it costs the peak memory that
//! [`Limits::max_unpacked`] exists to bound.
//!
//! # Untrusted bytes
//!
//! Ruling 1. Every read is checked, every arithmetic operation on a
//! file-derived number is checked or saturating, and an input that runs out
//! mid-symbol is [`Error::Truncated`] rather than a panic or a silent zero
//! run. `#![forbid(unsafe_code)]` is on the crate.

/// LZMA's probability model: 11-bit fixed point, moving 1/32 of the way to the
/// answer each time.
const PROB_BITS: u32 = 11;
/// Probabilities start at exactly one half.
const PROB_INIT: u16 = (1 << PROB_BITS) / 2;
/// The adaptation rate. Both constants are the format's, not a tuning knob:
/// an encoder used them and a decoder that used others diverges on the first
/// bit.
const MOVE_BITS: u32 = 5;
/// The range decoder renormalises below this.
const TOP: u32 = 1 << 24;

/// States 7 and above mean "the last thing decoded was a match", which is what
/// switches the literal decoder into its matched mode.
const STATES: usize = 12;
/// Lengths 2, 3, 4 and 5-or-more each get their own distance-slot tree.
const LEN_TO_POS_STATES: usize = 4;
/// Past this slot the middle bits are decoded as raw, unmodelled bits.
const END_POS_MODEL_INDEX: u32 = 14;
/// `1 << (END_POS_MODEL_INDEX / 2)`: the distances the modelled path covers.
const FULL_DISTANCES: u32 = 1 << (END_POS_MODEL_INDEX >> 1);
/// The bottom four bits of a long distance, modelled separately because they
/// carry the alignment of the copy.
const ALIGN_BITS: u32 = 4;
/// A match is at least two bytes; the length coder counts from here.
const MATCH_MIN_LEN: u32 = 2;

/// Why a stream did not decode.
///
/// Every variant is a fact about the *input*. There is no "internal error"
/// arm, and there is nothing here a caller can retry.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Error {
    /// The input ended in the middle of a symbol.
    Truncated,
    /// The declared unpacked size is past [`Limits::max_unpacked`].
    TooLarge,
    /// A match reaches further back than the output is long. In a circular
    /// dictionary this would silently read whatever was there; here it is an
    /// error, which is the whole argument for not having one.
    DistanceTooFar,
    /// `lc + lp` past 4, or `pb` past 4 — outside what the format allows, and
    /// what a property byte holding rubbish decodes to.
    BadProperties,
    /// The five bytes that start a range-coded stream are not a valid start:
    /// the first must be zero.
    BadRangeStart,
    /// An LZMA2 control byte outside the four shapes the format defines, or a
    /// chunk that asks for properties before any have been given.
    BadChunk,
    /// The stream decoded fewer bytes than it declared, and ended cleanly
    /// saying so.
    ShortOutput,
}

impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(match self {
            Error::Truncated => "the compressed stream ends mid-symbol",
            Error::TooLarge => "a declared unpacked size past this build's cap",
            Error::DistanceTooFar => "a match reaching back past the start of the output",
            Error::BadProperties => "an LZMA property byte outside the format's range",
            Error::BadRangeStart => "a range-coded stream with a bad first five bytes",
            Error::BadChunk => "an LZMA2 control byte the format does not define",
            Error::ShortOutput => "a stream that ended before its declared length",
        })
    }
}

impl std::error::Error for Error {}

/// What one decompression may spend.
///
/// One number, and the shortness is the same argument `tar::Limits` makes from
/// the other direction: LZMA's *only* unbounded allocation is the output, and
/// the probability arrays are a fixed 15 KiB whatever the input says. A cap on
/// anything else here would be a constant no input can reach.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    /// The most bytes one stream may decode to. See
    /// [`limits::MAX_LZMA_UNPACKED`](crate::sevenz::limits::MAX_LZMA_UNPACKED).
    pub max_unpacked: usize,
}

/// LZMA's three tuning parameters, as the one-byte property field encodes
/// them.
///
/// `lc` literal context bits, `lp` literal position bits, `pb` position bits,
/// packed as `(pb * 5 + lp) * 9 + lc`. The multiply-and-divide is the format's
/// and is why a byte past 224 is not a valid property byte.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Properties {
    lc: u32,
    lp: u32,
    pb: u32,
}

impl Properties {
    /// Unpacks the one-byte encoding.
    ///
    /// # Errors
    /// [`Error::BadProperties`] for a byte outside the format's range, and for
    /// the `lc + lp > 4` combination — which encodes but would allocate a
    /// literal table of 3 MiB and is refused by every real implementation.
    pub fn from_byte(byte: u8) -> Result<Self, Error> {
        let mut value = u32::from(byte);
        if value >= 9 * 5 * 5 {
            return Err(Error::BadProperties);
        }
        let lc = value % 9;
        value /= 9;
        let lp = value % 5;
        let pb = value / 5;
        if lc + lp > 4 {
            return Err(Error::BadProperties);
        }
        Ok(Properties { lc, lp, pb })
    }
}

/// The range decoder: LZMA's arithmetic coder, reading.
///
/// Carries `exhausted` rather than erroring inside `next`, because a
/// renormalisation happens deep inside a bit decode and threading a `Result`
/// through every one of them would put a `?` on the hottest path in this
/// crate. The flag is checked at every symbol boundary instead, which is at
/// most 273 bytes of output later.
struct Range<'a> {
    input: &'a [u8],
    at: usize,
    range: u32,
    code: u32,
    exhausted: bool,
}

impl<'a> Range<'a> {
    /// The five-byte preamble: a zero byte, then the initial code, big-endian.
    fn new(input: &'a [u8]) -> Result<Self, Error> {
        let Some(head) = input.get(..5) else {
            return Err(Error::Truncated);
        };
        // The format fixes the first byte at zero. It is the only structural
        // check a range-coded stream has, so it is the only place a wrong
        // offset into the archive is caught before it becomes noise.
        if head.first() != Some(&0) {
            return Err(Error::BadRangeStart);
        }
        let mut code = 0u32;
        for &byte in head.get(1..5).unwrap_or_default() {
            code = (code << 8) | u32::from(byte);
        }
        Ok(Range {
            input,
            at: 5,
            range: u32::MAX,
            code,
            exhausted: false,
        })
    }

    fn next_byte(&mut self) -> u8 {
        match self.input.get(self.at) {
            Some(&byte) => {
                self.at += 1;
                byte
            }
            None => {
                self.exhausted = true;
                0
            }
        }
    }

    fn normalize(&mut self) {
        if self.range < TOP {
            self.range <<= 8;
            self.code = (self.code << 8) | u32::from(self.next_byte());
        }
    }

    fn decode_bit(&mut self, prob: &mut u16) -> u32 {
        let bound = (self.range >> PROB_BITS).wrapping_mul(u32::from(*prob));
        let bit = if self.code < bound {
            *prob += ((1 << PROB_BITS) - *prob) >> MOVE_BITS;
            self.range = bound;
            0
        } else {
            *prob -= *prob >> MOVE_BITS;
            self.range -= bound;
            self.code -= bound;
            1
        };
        self.normalize();
        bit
    }

    /// Bits the model does not track, used for the middle of a long distance.
    fn decode_direct(&mut self, count: u32) -> u32 {
        let mut result = 0u32;
        for _ in 0..count {
            self.range >>= 1;
            self.code = self.code.wrapping_sub(self.range);
            let t = 0u32.wrapping_sub(self.code >> 31);
            self.code = self.code.wrapping_add(self.range & t);
            result = (result << 1).wrapping_add(t.wrapping_add(1));
            self.normalize();
        }
        result
    }

    /// A bit tree, most significant bit first.
    fn bit_tree(&mut self, probs: &mut [u16], bits: u32) -> u32 {
        let mut m = 1usize;
        for _ in 0..bits {
            let Some(prob) = probs.get_mut(m) else {
                self.exhausted = true;
                return 0;
            };
            let bit = self.decode_bit(prob);
            m = (m << 1) | bit as usize;
        }
        (m as u32).wrapping_sub(1 << bits)
    }

    /// A bit tree, least significant bit first, which is how a distance's low
    /// bits and its alignment are coded.
    fn bit_tree_reverse(&mut self, probs: &mut [u16], base: usize, bits: u32) -> u32 {
        let mut m = 1usize;
        let mut symbol = 0u32;
        for i in 0..bits {
            let Some(prob) = probs.get_mut(base.saturating_add(m)) else {
                self.exhausted = true;
                return symbol;
            };
            let bit = self.decode_bit(prob);
            m = (m << 1) | bit as usize;
            symbol |= bit << i;
        }
        symbol
    }
}

/// The length coder: three ranges of lengths, each its own tree.
///
/// 2-9 from `low`, 10-17 from `mid`, 18-273 from `high`, chosen by two bits.
/// The split exists because short matches are overwhelmingly the common case
/// and the encoder spends fewer bits on them.
struct Len {
    choice: u16,
    choice2: u16,
    low: [[u16; 8]; 1 << 4],
    mid: [[u16; 8]; 1 << 4],
    high: [u16; 256],
}

impl Len {
    fn new() -> Self {
        Len {
            choice: PROB_INIT,
            choice2: PROB_INIT,
            low: [[PROB_INIT; 8]; 1 << 4],
            mid: [[PROB_INIT; 8]; 1 << 4],
            high: [PROB_INIT; 256],
        }
    }

    fn reset(&mut self) {
        *self = Len::new();
    }

    fn decode(&mut self, rc: &mut Range<'_>, pos_state: usize) -> u32 {
        if rc.decode_bit(&mut self.choice) == 0 {
            let probs = self.low.get_mut(pos_state).map_or(&mut [][..], |p| p);
            return rc.bit_tree(probs, 3);
        }
        if rc.decode_bit(&mut self.choice2) == 0 {
            let probs = self.mid.get_mut(pos_state).map_or(&mut [][..], |p| p);
            return 8 + rc.bit_tree(probs, 3);
        }
        16 + rc.bit_tree(&mut self.high, 8)
    }
}

/// Everything an LZMA stream carries between symbols.
///
/// Public in the crate because LZMA2 resets *parts* of it per chunk — state
/// without the dictionary, or properties without the state — and a front end
/// that could only reset all of it would decode chunk two as though chunk one
/// had not happened.
pub(crate) struct State {
    props: Properties,
    /// `0x300` probabilities per literal context.
    literal: Vec<u16>,
    is_match: [[u16; 1 << 4]; STATES],
    is_rep: [u16; STATES],
    is_rep_g0: [u16; STATES],
    is_rep_g1: [u16; STATES],
    is_rep_g2: [u16; STATES],
    is_rep0_long: [[u16; 1 << 4]; STATES],
    pos_slot: [[u16; 1 << 6]; LEN_TO_POS_STATES],
    /// `1 + FULL_DISTANCES - END_POS_MODEL_INDEX`, which is exactly the range
    /// `bit_tree_reverse` can reach for every legal slot below
    /// [`END_POS_MODEL_INDEX`].
    spec_pos: [u16; 1 + (FULL_DISTANCES - END_POS_MODEL_INDEX) as usize],
    align: [u16; 1 << ALIGN_BITS],
    len: Len,
    rep_len: Len,
    state: usize,
    reps: [u32; 4],
}

impl State {
    pub(crate) fn new(props: Properties) -> Self {
        let mut state = State {
            props,
            literal: Vec::new(),
            is_match: [[PROB_INIT; 1 << 4]; STATES],
            is_rep: [PROB_INIT; STATES],
            is_rep_g0: [PROB_INIT; STATES],
            is_rep_g1: [PROB_INIT; STATES],
            is_rep_g2: [PROB_INIT; STATES],
            is_rep0_long: [[PROB_INIT; 1 << 4]; STATES],
            pos_slot: [[PROB_INIT; 1 << 6]; LEN_TO_POS_STATES],
            spec_pos: [PROB_INIT; 1 + (FULL_DISTANCES - END_POS_MODEL_INDEX) as usize],
            align: [PROB_INIT; 1 << ALIGN_BITS],
            len: Len::new(),
            rep_len: Len::new(),
            state: 0,
            reps: [0; 4],
        };
        state.reset_probabilities();
        state
    }

    /// Everything except the dictionary. LZMA2's control bits 5-6 ask for
    /// exactly this.
    pub(crate) fn reset_probabilities(&mut self) {
        let size = 0x300usize << (self.props.lc + self.props.lp);
        self.literal.clear();
        self.literal.resize(size, PROB_INIT);
        self.is_match = [[PROB_INIT; 1 << 4]; STATES];
        self.is_rep = [PROB_INIT; STATES];
        self.is_rep_g0 = [PROB_INIT; STATES];
        self.is_rep_g1 = [PROB_INIT; STATES];
        self.is_rep_g2 = [PROB_INIT; STATES];
        self.is_rep0_long = [[PROB_INIT; 1 << 4]; STATES];
        self.pos_slot = [[PROB_INIT; 1 << 6]; LEN_TO_POS_STATES];
        self.spec_pos = [PROB_INIT; 1 + (FULL_DISTANCES - END_POS_MODEL_INDEX) as usize];
        self.align = [PROB_INIT; 1 << ALIGN_BITS];
        self.len.reset();
        self.rep_len.reset();
        self.state = 0;
        self.reps = [0; 4];
    }

    pub(crate) fn set_properties(&mut self, props: Properties) {
        self.props = props;
        self.reset_probabilities();
    }

    /// Decodes until `out` reaches `until`, or the stream ends.
    ///
    /// `dict_start` is where this chunk's dictionary begins inside `out`: for
    /// plain LZMA it is zero, and for LZMA2 it moves only on a dictionary
    /// reset, which is what lets one `Vec` serve as the window for a whole
    /// multi-chunk stream.
    fn run(
        &mut self,
        rc: &mut Range<'_>,
        out: &mut Vec<u8>,
        until: usize,
        dict_start: usize,
    ) -> Result<(), Error> {
        let pb_mask = (1u32 << self.props.pb) - 1;
        let lp_mask = (1u32 << self.props.lp) - 1;
        let lc = self.props.lc;

        while out.len() < until {
            if rc.exhausted {
                return Err(Error::Truncated);
            }
            let total = (out.len() - dict_start) as u32;
            let pos_state = (total & pb_mask) as usize;

            let is_match = &mut self.is_match[self.state][pos_state];
            if rc.decode_bit(is_match) == 0 {
                // A literal.
                //
                // **The byte before the dictionary starts is a zero, not the
                // byte before the reset.** LZMA2 resets the dictionary mid
                // stream, and a decoder that reached back past the reset for
                // its literal context would pick a different probability slot
                // from the one the encoder used -- for one byte, after which
                // the two models diverge and every byte after it is wrong.
                let prev = if out.len() == dict_start {
                    0
                } else {
                    out.last().copied().unwrap_or(0)
                };
                let context =
                    (((total & lp_mask) << lc) + (u32::from(prev) >> (8 - lc).min(8))) as usize;
                let base = context.saturating_mul(0x300);
                let Some(probs) = self.literal.get_mut(base..base + 0x300) else {
                    return Err(Error::BadProperties);
                };
                let mut symbol = 1usize;
                if self.state >= 7 {
                    // "Matched" literal: the byte at the last distance is used
                    // as context, one bit at a time, until the two disagree.
                    let Some(at) = out
                        .len()
                        .checked_sub(self.reps[0] as usize)
                        .and_then(|n| n.checked_sub(1))
                        .filter(|n| *n >= dict_start)
                    else {
                        return Err(Error::DistanceTooFar);
                    };
                    let mut matched = out.get(at).copied().unwrap_or(0);
                    while symbol < 0x100 {
                        let match_bit = usize::from(matched >> 7);
                        matched <<= 1;
                        let index = ((1 + match_bit) << 8) + symbol;
                        let Some(prob) = probs.get_mut(index) else {
                            return Err(Error::BadProperties);
                        };
                        let bit = rc.decode_bit(prob) as usize;
                        symbol = (symbol << 1) | bit;
                        if match_bit != bit {
                            break;
                        }
                    }
                }
                while symbol < 0x100 {
                    let Some(prob) = probs.get_mut(symbol) else {
                        return Err(Error::BadProperties);
                    };
                    symbol = (symbol << 1) | rc.decode_bit(prob) as usize;
                }
                out.push(symbol as u8);
                // Literal-after-match decays back towards the literal states.
                self.state = match self.state {
                    0..=3 => 0,
                    4..=9 => self.state - 3,
                    _ => self.state - 6,
                };
                continue;
            }

            let len;
            if rc.decode_bit(&mut self.is_rep[self.state]) != 0 {
                // One of the four remembered distances.
                if out.len() == dict_start {
                    // A repeat before anything has been written has no
                    // distance to repeat.
                    return Err(Error::DistanceTooFar);
                }
                if rc.decode_bit(&mut self.is_rep_g0[self.state]) == 0 {
                    if rc.decode_bit(&mut self.is_rep0_long[self.state][pos_state]) == 0 {
                        // A single byte at the last distance.
                        self.state = if self.state < 7 { 9 } else { 11 };
                        let Some(at) = out
                            .len()
                            .checked_sub(self.reps[0] as usize)
                            .and_then(|n| n.checked_sub(1))
                            .filter(|n| *n >= dict_start)
                        else {
                            return Err(Error::DistanceTooFar);
                        };
                        let byte = out.get(at).copied().unwrap_or(0);
                        out.push(byte);
                        continue;
                    }
                } else {
                    let dist;
                    if rc.decode_bit(&mut self.is_rep_g1[self.state]) == 0 {
                        dist = self.reps[1];
                    } else if rc.decode_bit(&mut self.is_rep_g2[self.state]) == 0 {
                        dist = self.reps[2];
                        self.reps[2] = self.reps[1];
                    } else {
                        dist = self.reps[3];
                        self.reps[3] = self.reps[2];
                        self.reps[2] = self.reps[1];
                    }
                    self.reps[1] = self.reps[0];
                    self.reps[0] = dist;
                }
                len = self.rep_len.decode(rc, pos_state) + MATCH_MIN_LEN;
                self.state = if self.state < 7 { 8 } else { 11 };
            } else {
                // A new distance.
                self.reps[3] = self.reps[2];
                self.reps[2] = self.reps[1];
                self.reps[1] = self.reps[0];
                let raw_len = self.len.decode(rc, pos_state);
                len = raw_len + MATCH_MIN_LEN;
                self.state = if self.state < 7 { 7 } else { 10 };
                self.reps[0] = self.decode_distance(rc, raw_len);
                if self.reps[0] == u32::MAX {
                    // The end-of-stream marker. A stream that declares its
                    // length need not carry one, and one that does ends here.
                    return Ok(());
                }
            }

            let dist = self.reps[0] as usize;
            let Some(from) = out
                .len()
                .checked_sub(dist)
                .and_then(|n| n.checked_sub(1))
                .filter(|n| *n >= dict_start)
            else {
                return Err(Error::DistanceTooFar);
            };
            // Byte at a time on purpose: an LZMA match may overlap its own
            // output — `dist` of 1 and `len` of 100 is a run of one byte — so
            // a block copy would be wrong as well as unavailable.
            let want = (len as usize).min(until.saturating_sub(out.len()));
            for step in 0..want {
                let byte = out.get(from + step).copied().unwrap_or(0);
                out.push(byte);
            }
        }
        Ok(())
    }

    /// The distance model: a six-bit slot, then the middle bits, then the
    /// four alignment bits.
    fn decode_distance(&mut self, rc: &mut Range<'_>, raw_len: u32) -> u32 {
        let slot_state = (raw_len as usize).min(LEN_TO_POS_STATES - 1);
        let probs = self.pos_slot.get_mut(slot_state).map_or(&mut [][..], |p| p);
        let pos_slot = rc.bit_tree(probs, 6);
        if pos_slot < 4 {
            return pos_slot;
        }
        let direct_bits = (pos_slot >> 1) - 1;
        let mut dist = (2 | (pos_slot & 1)) << direct_bits;
        if pos_slot < END_POS_MODEL_INDEX {
            // `dist - pos_slot` is the format's own offset into one shared
            // array; the sizing note on `spec_pos` is why this cannot leave it.
            let base = (dist as usize).saturating_sub(pos_slot as usize);
            dist += rc.bit_tree_reverse(&mut self.spec_pos, base, direct_bits);
        } else {
            dist += rc.decode_direct(direct_bits - ALIGN_BITS) << ALIGN_BITS;
            dist += rc.bit_tree_reverse(&mut self.align, 0, ALIGN_BITS);
        }
        dist
    }
}

/// Plain LZMA, as 7z coder `030101` stores it: properties and a size the
/// container already knows, then a range-coded stream with no header of its
/// own.
///
/// # Errors
/// [`Error`], one variant per way the input is not this.
pub fn decode(input: &[u8], props: u8, unpacked: usize, limits: &Limits) -> Result<Vec<u8>, Error> {
    if unpacked > limits.max_unpacked {
        return Err(Error::TooLarge);
    }
    let props = Properties::from_byte(props)?;
    let mut state = State::new(props);
    let mut rc = Range::new(input)?;
    let mut out: Vec<u8> = Vec::with_capacity(unpacked.min(1 << 20));
    state.run(&mut rc, &mut out, unpacked, 0)?;
    if out.len() < unpacked {
        return Err(Error::ShortOutput);
    }
    Ok(out)
}

/// LZMA2, as 7z coder `21` stores it: the same engine under a chunk framing
/// that can reset the state, the properties and the dictionary independently.
///
/// The framing exists so a compressor can bound how much a corrupt chunk
/// costs and can switch to storing incompressible data. Both matter here: a
/// comic archive is mostly PNG and JPEG, and 7-Zip emits **uncompressed
/// chunks** for a good deal of it — so the uncompressed arm below is the
/// common path for this crate's own corpus rather than an edge case.
///
/// # Errors
/// [`Error`], one variant per way the input is not this.
pub fn decode_lzma2(input: &[u8], unpacked: usize, limits: &Limits) -> Result<Vec<u8>, Error> {
    if unpacked > limits.max_unpacked {
        return Err(Error::TooLarge);
    }
    let mut out: Vec<u8> = Vec::with_capacity(unpacked.min(1 << 20));
    let mut state: Option<State> = None;
    let mut dict_start = 0usize;
    let mut at = 0usize;

    loop {
        let Some(&control) = input.get(at) else {
            return Err(Error::Truncated);
        };
        at += 1;
        if control == 0 {
            break;
        }
        if control < 3 {
            // An uncompressed chunk: `01` also resets the dictionary, `02`
            // does not. Nothing else in 0x00..0x80 is defined.
            let Some(size) = be16(input, at) else {
                return Err(Error::Truncated);
            };
            at += 2;
            let size = size as usize + 1;
            let Some(chunk) = input.get(at..at.saturating_add(size)) else {
                return Err(Error::Truncated);
            };
            at += size;
            if out.len().saturating_add(size) > limits.max_unpacked {
                return Err(Error::TooLarge);
            }
            if control == 1 {
                dict_start = out.len();
            }
            out.extend_from_slice(chunk);
            // An uncompressed chunk invalidates the probability model but not
            // the properties: the next LZMA chunk must ask for a state reset,
            // and the format guarantees it does.
            if let Some(state) = state.as_mut() {
                state.reset_probabilities();
            }
            continue;
        }
        if control < 0x80 {
            return Err(Error::BadChunk);
        }

        let unpack_high = u32::from(control & 0x1F) << 16;
        let (Some(unpack_low), Some(pack_low)) = (be16(input, at), be16(input, at + 2)) else {
            return Err(Error::Truncated);
        };
        at += 4;
        let chunk_unpacked = (unpack_high | u32::from(unpack_low)) as usize + 1;
        let chunk_packed = pack_low as usize + 1;
        let reset = (control >> 5) & 3;

        if reset >= 2 {
            let Some(&byte) = input.get(at) else {
                return Err(Error::Truncated);
            };
            at += 1;
            let props = Properties::from_byte(byte)?;
            match state.as_mut() {
                Some(state) => state.set_properties(props),
                None => state = Some(State::new(props)),
            }
        }
        let Some(state) = state.as_mut() else {
            // Control asked to continue a stream that never had properties.
            return Err(Error::BadChunk);
        };
        if reset == 3 {
            dict_start = out.len();
        }
        if reset == 1 {
            state.reset_probabilities();
        }

        let Some(chunk) = input.get(at..at.saturating_add(chunk_packed)) else {
            return Err(Error::Truncated);
        };
        at += chunk_packed;
        let until = out.len().saturating_add(chunk_unpacked);
        if until > limits.max_unpacked {
            return Err(Error::TooLarge);
        }
        let mut rc = Range::new(chunk)?;
        state.run(&mut rc, &mut out, until, dict_start)?;
        if out.len() < until {
            return Err(Error::ShortOutput);
        }
    }

    if out.len() < unpacked {
        return Err(Error::ShortOutput);
    }
    out.truncate(unpacked);
    Ok(out)
}

/// Two bytes, big-endian — LZMA2's chunk sizes, which are the one big-endian
/// field in a format that is otherwise little-endian throughout.
fn be16(input: &[u8], at: usize) -> Option<u16> {
    let hi = *input.get(at)?;
    let lo = *input.get(at.checked_add(1)?)?;
    Some((u16::from(hi) << 8) | u16::from(lo))
}

#[cfg(test)]
mod tests;
