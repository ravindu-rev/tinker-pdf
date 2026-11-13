//! A JPEG 2000 codestream, generated (ITU-T T.800 Annexes A to D): SIZ, COD,
//! COC, QCD, QCC, POC and tile-parts that agree with one another, packets in
//! the order the progression they declare gives, packet headers coded as
//! Annex B codes them — tag trees, pass counts, `Lblock` and one length per
//! codeword segment — and code-block data that is either the input's own
//! bytes or Annex D's coding of generated coefficients, for `jpx_codestream`
//! to decode.
//!
//! Everything is spelled from the clauses it cites. The arithmetic coder
//! (C.2), the bypass's raw bits (D.6) and the context model (D.3) are this
//! module's own, so a coefficient the decoder reads otherwise than this
//! writes it shows as a cleanup pass whose segmentation symbol (D.5) is not
//! `1010`.
//!
//! One field may be broken on a choice the generator records ([`Break`]): set
//! to a value the standard rules out, with every other field left as it was,
//! so a decoder that refuses the codestream has the one field to name.

use std::collections::BTreeSet;

use crate::Choices;

/// The widest and the tallest image, in samples of the reference grid.
pub const MAX_SIZE: i64 = 40;
/// The most components.
pub const MAX_COMPONENTS: usize = 4;
/// The most decomposition levels.
pub const MAX_LEVELS: u8 = 5;
/// The most quality layers.
pub const MAX_LAYERS: u16 = 4;
/// The most tiles across the image, and down it.
pub const MAX_TILES_ACROSS: i64 = 3;
/// The most coding passes one code-block carries.
pub const MAX_PASSES: u32 = 13;
/// The most magnitude bit-planes a subband's coefficients have, `Mb` (E-2).
pub const MAX_MB: i64 = 12;
/// The most bit-planes a code-block codes.
pub const MAX_PLANES: i64 = 5;
/// The most bytes of the input one piece of a code-block's raw data takes.
pub const MAX_PIECE: usize = 12;
/// The most tile-parts a tile is cut into.
pub const MAX_PARTS: usize = 3;
/// The most progression order volumes one POC describes before the volume
/// that covers the rest.
pub const MAX_VOLUMES: usize = 3;

// Table A.2.
const SOC: u16 = 0xFF4F;
const SOT: u16 = 0xFF90;
const SOD: u16 = 0xFF93;
const EOC: u16 = 0xFFD9;
const SIZ: u16 = 0xFF51;
const COD: u16 = 0xFF52;
const COC: u16 = 0xFF53;
const QCD: u16 = 0xFF5C;
const QCC: u16 = 0xFF5D;
const POC: u16 = 0xFF5F;
const PPM: u16 = 0xFF60;
const PPT: u16 = 0xFF61;
const SOP: u16 = 0xFF91;
const EPH: u16 = 0xFF92;
const CRG: u16 = 0xFF63;
const COM: u16 = 0xFF64;

/// Table A.19's code-block styles.
pub const BYPASS: u8 = 0x01;
/// Contexts reset at every coding pass.
pub const RESET: u8 = 0x02;
/// Every coding pass terminated.
pub const TERMALL: u8 = 0x04;
/// Vertically causal context formation (D.7).
pub const CAUSAL: u8 = 0x08;
/// Predictable termination (D.4.2).
pub const PREDICTABLE: u8 = 0x10;
/// Segmentation symbols (D.5).
pub const SEGMENTATION: u8 = 0x20;

/// A field set to a value T.800 rules out.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Break {
    /// SIZ's `Rsiz` past Table A.10's three values.
    Rsiz,
    /// SIZ's `XTsiz` or `YTsiz` of zero (Table A.9).
    TileSize,
    /// SIZ's `XOsiz` or `YOsiz` at or past `Xsiz` or `Ysiz`.
    ImageOffset,
    /// SIZ's `XTOsiz` or `YTOsiz` past the image offset (B-3).
    TileOffset,
    /// A first tile that misses the image (B-4).
    TileGrid,
    /// SIZ's `Csiz` disagreeing with `Lsiz` (A-1).
    Csiz,
    /// A component's `Ssiz` past Table A.11's 38 bits.
    Ssiz,
    /// A component's `XRsiz` or `YRsiz` of zero (Table A.9).
    Separation,
    /// A bit of COD's `Scod` Table A.13 reserves.
    Scod,
    /// A progression order Table A.16 reserves.
    Progression,
    /// No quality layers (Table A.14).
    Layers,
    /// A multiple component transformation Table A.17 reserves.
    Mct,
    /// More decomposition levels than Table A.15's 32.
    Levels,
    /// A code-block size Table A.18 does not allow.
    CodeBlockSize,
    /// A code-block style bit Table A.19 reserves.
    CodeBlockStyle,
    /// A transformation Table A.20 reserves.
    Wavelet,
    /// A precinct exponent of zero above resolution 0 (Table A.21).
    Precinct,
    /// A bit of COC's `Scoc` Table A.23 reserves.
    Scoc,
    /// A COC naming a component SIZ does not declare (Table A.22).
    Ccoc,
    /// A quantization style Table A.28 reserves.
    Sqcd,
    /// A QCC naming a component SIZ does not declare (Table A.31).
    Cqcc,
    /// A POC progression outside Table A.32's ranges.
    Poc,
    /// An SOT whose `Lsot` is not Table A.5's 10.
    Lsot,
    /// An SOT naming a tile outside the grid.
    Isot,
    /// A `Psot` shorter than its own tile-part header (A.4.2).
    Psot,
    /// A tile's parts out of `TPsot` order (A.4.2).
    TPsot,
    /// Two parts of a tile declaring different numbers of parts (Table A.6).
    TNsot,
    /// A marker code Table A.2 does not define.
    Marker,
    /// A second COD or QCD in the main header (A.6.1, A.6.4).
    Second,
}

impl Break {
    const ALL: [Break; 29] = [
        Break::Rsiz,
        Break::TileSize,
        Break::ImageOffset,
        Break::TileOffset,
        Break::TileGrid,
        Break::Csiz,
        Break::Ssiz,
        Break::Separation,
        Break::Scod,
        Break::Progression,
        Break::Layers,
        Break::Mct,
        Break::Levels,
        Break::CodeBlockSize,
        Break::CodeBlockStyle,
        Break::Wavelet,
        Break::Precinct,
        Break::Scoc,
        Break::Ccoc,
        Break::Sqcd,
        Break::Cqcc,
        Break::Poc,
        Break::Lsot,
        Break::Isot,
        Break::Psot,
        Break::TPsot,
        Break::TNsot,
        Break::Marker,
        Break::Second,
    ];

    /// The words a refusal of this field may name it by: its marker, the
    /// field as T.800's tables spell it, and the words for what it is.
    pub fn names(self) -> &'static [&'static str] {
        match self {
            Break::Rsiz => &["Rsiz"],
            Break::TileSize => &["tile size", "XTsiz", "YTsiz"],
            Break::ImageOffset => &["image offset", "XOsiz", "YOsiz"],
            Break::TileOffset => &["tile offset", "XTOsiz", "YTOsiz"],
            Break::TileGrid => &["tile grid", "XTsiz", "YTsiz"],
            Break::Csiz => &["Csiz", "SIZ", "components"],
            Break::Ssiz => &["Ssiz", "precision"],
            Break::Separation => &["separation", "XRsiz", "YRsiz"],
            Break::Scod => &["Scod"],
            Break::Progression => &["progression"],
            Break::Layers => &["layers"],
            Break::Mct => &["component transform"],
            Break::Levels => &["decomposition levels"],
            Break::CodeBlockSize => &["code-block size"],
            Break::CodeBlockStyle => &["code-block style"],
            Break::Wavelet => &["wavelet", "transformation"],
            Break::Precinct => &["precinct"],
            Break::Scoc => &["Scoc"],
            Break::Ccoc => &["COC"],
            Break::Sqcd => &["quantisation style", "quantization style", "Sqcd"],
            Break::Cqcc => &["QCC"],
            Break::Poc => &["POC", "progression"],
            Break::Lsot => &["SOT", "Lsot"],
            Break::Isot => &["SOT", "Isot"],
            Break::Psot => &["Psot"],
            Break::TPsot => &["tile-part", "TPsot"],
            Break::TNsot => &["TNsot"],
            Break::Marker => &["marker"],
            Break::Second => &["COD", "QCD"],
        }
    }
}

/// What a refusal says, in words a [`Break`]'s names can be found in.
pub fn describe(refusal: &tinker_pdf_filters::JpxRefusal) -> String {
    use tinker_pdf_filters::JpxRefusal as R;
    match refusal {
        R::Marker(s) | R::Structure(s) | R::Feature(s) | R::Truncated(s) | R::Budget(s) => {
            (*s).to_string()
        }
        R::UnknownMarker(code) => format!("a marker Table A.2 does not define, {code:#06x}"),
        R::Precision(bits) => format!("a component precision of {bits} bits"),
        R::PacketLength => "a packet that does not end where the next begins".to_string(),
        R::SegmentationSymbol => "a cleanup pass whose segmentation symbol is not 1010".to_string(),
    }
}

/// Whether a refusal names the field `broken` broke: whether what it says
/// holds one of the field's names, letter case aside.
pub fn names(broken: Break, refusal: &tinker_pdf_filters::JpxRefusal) -> bool {
    let said = describe(refusal).to_lowercase();
    broken
        .names()
        .iter()
        .any(|name| said.contains(&name.to_lowercase()))
}

/// How the packet headers travel (B.10, A.7.4, A.7.5).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Headers {
    /// Each before its packet's body.
    InStream,
    /// In PPT segments of the tile-part headers, for some tiles.
    Ppt,
    /// In PPM segments of the main header.
    Ppm,
}

/// A generated codestream and what the generator knows of it.
pub struct Codestream {
    /// The codestream, SOC to EOC.
    pub bytes: Vec<u8>,
    /// The image's width, `Xsiz - XOsiz`.
    pub width: u32,
    /// The image's height, `Ysiz - YOsiz`.
    pub height: u32,
    /// `Csiz`.
    pub components: u8,
    /// Every component's sample precision.
    pub precision: u8,
    /// The field broken, if one is.
    pub broken: Option<Break>,
    /// The progression orders some tile's packets follow, one bit per Table
    /// A.16 value.
    pub orders: u8,
    /// Tiles.
    pub tiles: usize,
    /// Tile-parts.
    pub parts: usize,
    /// Whether some tile's parts are interleaved with another's.
    pub interleaved: bool,
    /// Progression order volumes in the main header's POC.
    pub volumes: usize,
    /// Tiles with a POC of their own.
    pub tile_pocs: usize,
    /// Tiles with a COD of their own.
    pub tile_cods: usize,
    /// COC and QCC segments, in any header.
    pub overrides: usize,
    /// Every code-block style some tile-component is coded with, or'd.
    pub styles: u8,
    /// Whether some resolution is cut into more than one precinct.
    pub precincts: bool,
    /// The most layers a tile has.
    pub layers: u16,
    /// The most decomposition levels a tile-component has.
    pub levels: u8,
    /// Whether some component is subsampled.
    pub subsampled: bool,
    /// Whether some tile applies the reversible component transform.
    pub rct: bool,
    /// Whether some tile applies the irreversible one.
    pub ict: bool,
    /// Whether the code-block data is Annex D's coding of generated
    /// coefficients rather than the input's bytes.
    pub encoded: bool,
    /// Packets.
    pub packets: usize,
    /// Packets with a code-block contribution.
    pub filled: usize,
    /// Whether SOP and EPH are signalled.
    pub sop: bool,
    /// Whether EPH is signalled.
    pub eph: bool,
    /// Where the packet headers travel.
    pub headers: Headers,
    /// Whether the last tile-part's `Psot` is zero.
    pub psot_zero: bool,
    /// Whether some tile-part leaves `TNsot` unstated.
    pub tnsot_zero: bool,
    /// Whether some component's samples are signed.
    pub signed: bool,
    /// Code-blocks whose passes reach the bypass's raw ones (Table D.9).
    pub raw: usize,
}

// --- arithmetic ----------------------------------------------------------

fn ceil_div(a: i64, b: i64) -> i64 {
    a.div_euclid(b) + i64::from(a.rem_euclid(b) != 0)
}

fn floor_div(a: i64, b: i64) -> i64 {
    a.div_euclid(b)
}

/// The bits a length needs.
fn bit_length(value: usize) -> u32 {
    usize::BITS - value.leading_zeros()
}

fn floor_log2(value: u32) -> u32 {
    31 - value.max(1).leading_zeros()
}

fn be16(out: &mut Vec<u8>, value: u16) {
    out.extend_from_slice(&value.to_be_bytes());
}

fn be32(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_be_bytes());
}

/// A marker segment: its code, `L` (which counts itself), and its body.
fn segment(code: u16, body: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(body.len() + 4);
    be16(&mut out, code);
    be16(&mut out, (body.len() + 2) as u16);
    out.extend_from_slice(body);
    out
}

/// A pseudo-random sequence for coefficients, so that a code-block's samples
/// cost a few bytes of the input rather than one each.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 32) as u32
    }
}

// --- the arithmetic coder (Annex C) --------------------------------------

/// Table C.2: `Qe`, `NMPS`, `NLPS` and `SWITCH`, state by state.
const QE: [(u32, u8, u8, bool); 47] = [
    (0x5601, 1, 1, true),
    (0x3401, 2, 6, false),
    (0x1801, 3, 9, false),
    (0x0AC1, 4, 12, false),
    (0x0521, 5, 29, false),
    (0x0221, 38, 33, false),
    (0x5601, 7, 6, true),
    (0x5401, 8, 14, false),
    (0x4801, 9, 14, false),
    (0x3801, 10, 14, false),
    (0x3001, 11, 17, false),
    (0x2401, 12, 18, false),
    (0x1C01, 13, 20, false),
    (0x1601, 29, 21, false),
    (0x5601, 15, 14, true),
    (0x5401, 16, 14, false),
    (0x5101, 17, 15, false),
    (0x4801, 18, 16, false),
    (0x3801, 19, 17, false),
    (0x3401, 20, 18, false),
    (0x3001, 21, 19, false),
    (0x2801, 22, 19, false),
    (0x2401, 23, 20, false),
    (0x2201, 24, 21, false),
    (0x1C01, 25, 22, false),
    (0x1801, 26, 23, false),
    (0x1601, 27, 24, false),
    (0x1401, 28, 25, false),
    (0x1201, 29, 26, false),
    (0x1101, 30, 27, false),
    (0x0AC1, 31, 28, false),
    (0x09C1, 32, 29, false),
    (0x08A1, 33, 30, false),
    (0x0521, 34, 31, false),
    (0x0441, 35, 32, false),
    (0x02A1, 36, 33, false),
    (0x0221, 37, 34, false),
    (0x0141, 38, 35, false),
    (0x0111, 39, 36, false),
    (0x0085, 40, 37, false),
    (0x0049, 41, 38, false),
    (0x0025, 42, 39, false),
    (0x0015, 43, 40, false),
    (0x0009, 44, 41, false),
    (0x0005, 45, 42, false),
    (0x0001, 45, 43, false),
    (0x5601, 46, 46, false),
];

/// Annex D's nineteen contexts: Table D.1's nine, D.3's five sign contexts,
/// D.4's three refinement contexts, the run-length context and UNIFORM.
const CONTEXTS: usize = 19;
const SIGN_BASE: usize = 9;
const RUN: usize = 17;
const UNIFORM: usize = 18;

/// Each context's state index and MPS.
#[derive(Clone)]
struct States([(u8, u8); CONTEXTS]);

impl States {
    /// Table D.7.
    fn initial() -> States {
        let mut states = [(0u8, 0u8); CONTEXTS];
        states[0] = (4, 0);
        states[RUN] = (3, 0);
        states[UNIFORM] = (46, 0);
        States(states)
    }
}

/// The encoder of C.2: the A and C registers, the bit counter, and the
/// bytes so far, behind one byte that stands before the first (C.2.8's
/// `BPST - 1`).
struct Mq {
    out: Vec<u8>,
    a: u32,
    c: u64,
    ct: u32,
}

impl Mq {
    /// INITENC (C.2.8). The byte before the segment is no 0xFF, so `CT` is
    /// 12.
    fn new() -> Mq {
        Mq {
            out: vec![0],
            a: 0x8000,
            c: 0,
            ct: 12,
        }
    }

    /// The bytes written so far.
    fn len(&self) -> usize {
        self.out.len() - 1
    }

    /// ENCODE (C.2.2), with CODEMPS and CODELPS (C.2.4).
    fn encode(&mut self, states: &mut States, cx: usize, d: u8) {
        let (index, mps) = states.0[cx];
        let (qe, nmps, nlps, switch) = QE[usize::from(index)];
        self.a -= qe;
        if d == mps {
            if self.a & 0x8000 == 0 {
                if self.a < qe {
                    self.a = qe;
                } else {
                    self.c += u64::from(qe);
                }
                states.0[cx].0 = nmps;
                self.renorm();
            } else {
                self.c += u64::from(qe);
            }
        } else {
            if self.a < qe {
                self.c += u64::from(qe);
            } else {
                self.a = qe;
            }
            if switch {
                states.0[cx].1 = 1 - mps;
            }
            states.0[cx].0 = nlps;
            self.renorm();
        }
    }

    /// RENORME (C.2.6).
    fn renorm(&mut self) {
        loop {
            self.a <<= 1;
            self.c <<= 1;
            self.ct -= 1;
            if self.ct == 0 {
                self.byte_out();
            }
            if self.a & 0x8000 != 0 {
                break;
            }
        }
    }

    /// BYTEOUT (C.2.7), with its carry and its bit stuffed after 0xFF.
    fn byte_out(&mut self) {
        let last = self.out.len() - 1;
        if self.out[last] == 0xFF {
            self.out.push((self.c >> 20) as u8);
            self.c &= 0xF_FFFF;
            self.ct = 7;
        } else if self.c < 0x800_0000 {
            self.out.push((self.c >> 19) as u8);
            self.c &= 0x7_FFFF;
            self.ct = 8;
        } else {
            self.out[last] += 1;
            if self.out[last] == 0xFF {
                self.c &= 0x7FF_FFFF;
                self.out.push((self.c >> 20) as u8);
                self.c &= 0xF_FFFF;
                self.ct = 7;
            } else {
                self.out.push((self.c >> 19) as u8);
                self.c &= 0x7_FFFF;
                self.ct = 8;
            }
        }
    }

    /// The segment's bytes: FLUSH (C.2.9), or D.4.2's predictable
    /// termination, and in both the last byte dropped where it is 0xFF.
    fn finish(mut self, predictable: bool) -> Vec<u8> {
        if predictable {
            let mut k = (11 - self.ct as i64) + 1;
            while k > 0 {
                self.c <<= self.ct;
                self.ct = 0;
                self.byte_out();
                k -= i64::from(self.ct);
            }
        } else {
            let temp = self.c + u64::from(self.a);
            self.c |= 0xFFFF;
            if self.c >= temp {
                self.c -= 0x8000;
            }
            self.c <<= self.ct;
            self.byte_out();
            self.c <<= self.ct;
            self.byte_out();
        }
        if self.out.len() > 1 && self.out.last() == Some(&0xFF) {
            self.out.pop();
        }
        self.out.remove(0);
        self.out
    }
}

/// D.6's raw bits: packed from the most significant, a zero bit stuffed
/// after every 0xFF byte.
struct Raw {
    out: Vec<u8>,
    byte: u8,
    bits: u8,
    room: u8,
}

impl Raw {
    fn new() -> Raw {
        Raw {
            out: Vec::new(),
            byte: 0,
            bits: 0,
            room: 8,
        }
    }

    fn put(&mut self, bit: u8) {
        self.byte = (self.byte << 1) | (bit & 1);
        self.bits += 1;
        if self.bits == self.room {
            self.out.push(self.byte);
            self.room = if self.byte == 0xFF { 7 } else { 8 };
            self.byte = 0;
            self.bits = 0;
        }
    }

    fn len(&self) -> usize {
        self.out.len()
    }

    /// The pass's bytes, padded to the byte boundary — with D.6's
    /// alternating 0 and 1 from a 0 under predictable termination — and
    /// never ending in 0xFF.
    fn finish(mut self, predictable: bool) -> Vec<u8> {
        if self.bits > 0 {
            for i in 0..self.room - self.bits {
                let pad = if predictable { i & 1 } else { 0 };
                self.byte = (self.byte << 1) | pad;
            }
            self.out.push(self.byte);
        } else if self.out.last() == Some(&0xFF) {
            if predictable {
                // D.6, note 2: the byte after the 0xFF, its stuffed zero bit
                // then the alternating pad.
                self.out.push(0x2A);
            } else {
                self.out.pop();
            }
        }
        self.out
    }
}

/// Packet header bits (B.10.1): from the most significant bit, a zero bit
/// stuffed after every 0xFF byte, and never ending in 0xFF.
struct Bits {
    out: Vec<u8>,
    byte: u8,
    bits: u8,
    room: u8,
}

impl Bits {
    fn new() -> Bits {
        Bits {
            out: Vec::new(),
            byte: 0,
            bits: 0,
            room: 8,
        }
    }

    fn put(&mut self, bit: bool) {
        self.byte = (self.byte << 1) | u8::from(bit);
        self.bits += 1;
        if self.bits == self.room {
            self.out.push(self.byte);
            self.room = if self.byte == 0xFF { 7 } else { 8 };
            self.byte = 0;
            self.bits = 0;
        }
    }

    fn put_value(&mut self, value: u32, width: u32) {
        for i in (0..width).rev() {
            self.put((value >> i) & 1 == 1);
        }
    }

    fn finish(mut self) -> Vec<u8> {
        if self.bits > 0 {
            self.byte <<= self.room - self.bits;
            self.out.push(self.byte);
        } else if self.out.last() == Some(&0xFF) {
            // The byte that carries the zero bit stuffed after the 0xFF.
            self.out.push(0);
        }
        self.out
    }
}

/// A tag tree (B.10.2) over a grid of code-blocks, with each node's value,
/// its current value and whether it is known.
struct TagTree {
    /// `(width, height)` of each level, the leaves first.
    levels: Vec<(usize, usize)>,
    /// Where each level's nodes start in the arrays below.
    starts: Vec<usize>,
    value: Vec<u32>,
    low: Vec<u32>,
    known: Vec<bool>,
}

impl TagTree {
    fn new(width: usize, height: usize, leaves: &[u32]) -> TagTree {
        let mut levels = vec![(width, height)];
        while levels.last().is_some_and(|&(w, h)| w > 1 || h > 1) {
            let (w, h) = *levels.last().unwrap_or(&(1, 1));
            levels.push((w.div_ceil(2), h.div_ceil(2)));
        }
        let mut starts = Vec::with_capacity(levels.len());
        let mut total = 0;
        for &(w, h) in &levels {
            starts.push(total);
            total += w * h;
        }
        let mut value = vec![u32::MAX; total];
        value[..leaves.len()].copy_from_slice(leaves);
        for level in 1..levels.len() {
            let (w, h) = levels[level];
            let (cw, ch) = levels[level - 1];
            for y in 0..h {
                for x in 0..w {
                    let mut min = u32::MAX;
                    for (cx, cy) in [
                        (2 * x, 2 * y),
                        (2 * x + 1, 2 * y),
                        (2 * x, 2 * y + 1),
                        (2 * x + 1, 2 * y + 1),
                    ] {
                        if cx < cw && cy < ch {
                            min = min.min(value[starts[level - 1] + cy * cw + cx]);
                        }
                    }
                    value[starts[level] + y * w + x] = min;
                }
            }
        }
        TagTree {
            low: vec![0; total],
            known: vec![false; total],
            levels,
            starts,
            value,
        }
    }

    /// Codes as much of leaf `(x, y)` as a threshold asks: the questions from
    /// the top node down, a 0 for each value passed and a 1 where a node's
    /// value is reached, none asked of a node already known, and none past
    /// the threshold. Says whether the leaf's value is below the threshold.
    fn encode(&mut self, x: usize, y: usize, threshold: u32, bits: &mut Bits) -> bool {
        let mut path = Vec::with_capacity(self.levels.len());
        let (mut px, mut py) = (x, y);
        for level in 0..self.levels.len() {
            let (w, _) = self.levels[level];
            path.push(self.starts[level] + py * w + px);
            px /= 2;
            py /= 2;
        }
        let mut low = 0u32;
        for &node in path.iter().rev() {
            if self.low[node] < low {
                self.low[node] = low;
            } else {
                low = self.low[node];
            }
            while low < threshold {
                if low >= self.value[node] {
                    if !self.known[node] {
                        bits.put(true);
                        self.known[node] = true;
                    }
                    break;
                }
                bits.put(false);
                low += 1;
            }
            self.low[node] = low;
        }
        self.value[path[0]] < threshold
    }
}

// --- Annex D: the code-block coder ---------------------------------------

/// A subband's orientation (F.3.1), which picks Table D.1's column.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Band {
    Ll,
    Hl,
    Lh,
    Hh,
}

/// Table D.1.
fn zero_context(h: u32, v: u32, d: u32, band: Band) -> usize {
    // The LL and LH column; HL is the same with H and V exchanged.
    let primary = |h: u32, v: u32, d: u32| -> usize {
        match (h, v, d) {
            (2.., _, _) => 8,
            (1, 1.., _) => 7,
            (1, 0, 1..) => 6,
            (1, 0, 0) => 5,
            (0, 2.., _) => 4,
            (0, 1, _) => 3,
            (0, 0, 2..) => 2,
            (0, 0, 1) => 1,
            _ => 0,
        }
    };
    match band {
        Band::Ll | Band::Lh => primary(h, v, d),
        Band::Hl => primary(v, h, d),
        Band::Hh => match (h + v, d) {
            (_, 3..) => 8,
            (1.., 2) => 7,
            (0, 2) => 6,
            (2.., 1) => 5,
            (1, 1) => 4,
            (0, 1) => 3,
            (2.., 0) => 2,
            (1, 0) => 1,
            _ => 0,
        },
    }
}

/// Table D.3: the sign context and its XOR bit for the horizontal and
/// vertical contributions of Table D.2.
fn sign_context(h: i32, v: i32) -> (usize, u8) {
    let (label, xor) = match (h, v) {
        (1, 1) => (13, 0),
        (1, 0) => (12, 0),
        (1, -1) => (11, 0),
        (0, 1) => (10, 0),
        (0, 0) => (9, 0),
        (0, -1) => (10, 1),
        (-1, 1) => (11, 1),
        (-1, 0) => (12, 1),
        _ => (13, 1),
    };
    (label, xor)
}

/// One codeword segment of a coded code-block: its bytes, the passes it
/// holds, and the bytes needed after each of them (D.4.3).
#[derive(Clone, Debug)]
struct Coded {
    bytes: Vec<u8>,
    first: u32,
    passes: u32,
    ends: Vec<usize>,
}

/// Which coding pass a pass index is: 0 the cleanup of the first bit-plane,
/// then significance propagation, magnitude refinement and cleanup in turn.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Pass {
    Significance,
    Refinement,
    Cleanup,
}

fn pass_kind(k: u32) -> Pass {
    if k == 0 {
        return Pass::Cleanup;
    }
    match (k - 1) % 3 {
        0 => Pass::Significance,
        1 => Pass::Refinement,
        _ => Pass::Cleanup,
    }
}

/// Whether pass `k` is raw under the bypass (Table D.9): the significance
/// propagation and refinement passes from the fifth bit-plane down.
fn raw_pass(style: u8, k: u32) -> bool {
    style & BYPASS != 0 && k >= 10 && pass_kind(k) != Pass::Cleanup
}

/// Whether Tables D.8 and D.9 terminate pass `k` (the code-block's last pass
/// apart, which ends its data however it is coded).
fn terminates(style: u8, k: u32) -> bool {
    if style & TERMALL != 0 {
        return true;
    }
    if style & BYPASS != 0 {
        return k == 9 || (k >= 10 && pass_kind(k) != Pass::Significance);
    }
    false
}

/// The coder of one code-block (Annex D) over its coefficients.
struct BlockCoder<'a> {
    width: usize,
    height: usize,
    magnitude: &'a [u32],
    negative: &'a [bool],
    band: Band,
    causal: bool,
    sigma: Vec<bool>,
    visited: Vec<bool>,
    refined: Vec<bool>,
}

/// A coder the passes write into: the arithmetic coder or raw bits.
enum Sink {
    Mq(Mq),
    Raw(Raw),
}

impl Sink {
    fn len(&self) -> usize {
        match self {
            Sink::Mq(mq) => mq.len(),
            Sink::Raw(raw) => raw.len(),
        }
    }

    fn finish(self, predictable: bool) -> Vec<u8> {
        match self {
            Sink::Mq(mq) => mq.finish(predictable),
            Sink::Raw(raw) => raw.finish(predictable),
        }
    }

    /// A decision in a context, or the bit itself when the pass is raw.
    fn code(&mut self, states: &mut States, cx: usize, bit: u8) {
        match self {
            Sink::Mq(mq) => mq.encode(states, cx, bit),
            Sink::Raw(raw) => raw.put(bit),
        }
    }
}

impl BlockCoder<'_> {
    fn at(&self, x: usize, y: usize) -> usize {
        y * self.width + x
    }

    /// Whether `(x + dx, y + dy)` is significant, as context formation sees
    /// it from `(x, y)`: outside the block is insignificant (D.3), and so,
    /// under D.7, is the next stripe.
    fn significant(&self, x: usize, y: usize, dx: isize, dy: isize) -> Option<bool> {
        if dy > 0 && self.causal && y % 4 == 3 {
            return None;
        }
        let nx = x.checked_add_signed(dx)?;
        let ny = y.checked_add_signed(dy)?;
        if nx >= self.width || ny >= self.height {
            return None;
        }
        let i = self.at(nx, ny);
        self.sigma[i].then_some(self.negative[i])
    }

    /// `(ΣH, ΣV, ΣD)` of Table D.1.
    fn sums(&self, x: usize, y: usize) -> (u32, u32, u32) {
        let s = |dx, dy| u32::from(self.significant(x, y, dx, dy).is_some());
        (
            s(-1, 0) + s(1, 0),
            s(0, -1) + s(0, 1),
            s(-1, -1) + s(1, -1) + s(-1, 1) + s(1, 1),
        )
    }

    fn zero(&self, x: usize, y: usize) -> usize {
        let (h, v, d) = self.sums(x, y);
        zero_context(h, v, d, self.band)
    }

    /// Codes the sign of `(x, y)`, which has just become significant.
    fn sign(&mut self, sink: &mut Sink, states: &mut States, x: usize, y: usize) {
        let negative = self.negative[self.at(x, y)];
        if let Sink::Raw(raw) = sink {
            // (D-2).
            raw.put(u8::from(negative));
            return;
        }
        // Table D.2: each pair's contribution is the sign of their sum.
        let contribution = |a: Option<bool>, b: Option<bool>| -> i32 {
            let one = |s: Option<bool>| match s {
                None => 0,
                Some(true) => -1,
                Some(false) => 1,
            };
            (one(a) + one(b)).clamp(-1, 1)
        };
        let h = contribution(self.significant(x, y, -1, 0), self.significant(x, y, 1, 0));
        let v = contribution(self.significant(x, y, 0, -1), self.significant(x, y, 0, 1));
        let (label, xor) = sign_context(h, v);
        debug_assert!((SIGN_BASE..SIGN_BASE + 5).contains(&label));
        sink.code(states, label, u8::from(negative) ^ xor);
    }

    fn bit(&self, i: usize, plane: u32) -> u8 {
        ((self.magnitude[i] >> plane) & 1) as u8
    }

    /// D.3.1.
    fn significance(&mut self, sink: &mut Sink, states: &mut States, plane: u32) {
        for top in (0..self.height).step_by(4) {
            for x in 0..self.width {
                for y in top..(top + 4).min(self.height) {
                    let i = self.at(x, y);
                    if self.sigma[i] {
                        continue;
                    }
                    let cx = self.zero(x, y);
                    if cx == 0 {
                        continue;
                    }
                    let bit = self.bit(i, plane);
                    sink.code(states, cx, bit);
                    self.visited[i] = true;
                    if bit == 1 {
                        self.sign(sink, states, x, y);
                        self.sigma[i] = true;
                    }
                }
            }
        }
    }

    /// D.3.3.
    fn refinement(&mut self, sink: &mut Sink, states: &mut States, plane: u32) {
        for top in (0..self.height).step_by(4) {
            for x in 0..self.width {
                for y in top..(top + 4).min(self.height) {
                    let i = self.at(x, y);
                    if !self.sigma[i] || self.visited[i] {
                        continue;
                    }
                    let cx = if self.refined[i] {
                        16
                    } else {
                        let (h, v, d) = self.sums(x, y);
                        if h + v + d > 0 {
                            15
                        } else {
                            14
                        }
                    };
                    let bit = self.bit(i, plane);
                    sink.code(states, cx, bit);
                    self.refined[i] = true;
                }
            }
        }
    }

    /// D.3.4, with Table D.5's run-length coding.
    fn cleanup(&mut self, sink: &mut Sink, states: &mut States, plane: u32, symbols: bool) {
        for top in (0..self.height).step_by(4) {
            let rows = (self.height - top).min(4);
            for x in 0..self.width {
                let mut from = top;
                let run = rows == 4
                    && (top..top + 4).all(|y| {
                        let i = self.at(x, y);
                        !self.sigma[i] && !self.visited[i] && self.zero(x, y) == 0
                    });
                if run {
                    let first = (0..4).find(|r| self.bit(self.at(x, top + r), plane) == 1);
                    match first {
                        None => {
                            sink.code(states, RUN, 0);
                            continue;
                        }
                        Some(r) => {
                            sink.code(states, RUN, 1);
                            sink.code(states, UNIFORM, (r >> 1) as u8);
                            sink.code(states, UNIFORM, (r & 1) as u8);
                            let y = top + r;
                            self.sign(sink, states, x, y);
                            let i = self.at(x, y);
                            self.sigma[i] = true;
                            from = y + 1;
                        }
                    }
                }
                for y in from..top + rows {
                    let i = self.at(x, y);
                    if self.sigma[i] || self.visited[i] {
                        continue;
                    }
                    let cx = self.zero(x, y);
                    let bit = self.bit(i, plane);
                    sink.code(states, cx, bit);
                    if bit == 1 {
                        self.sign(sink, states, x, y);
                        self.sigma[i] = true;
                    }
                }
            }
        }
        self.visited.iter_mut().for_each(|v| *v = false);
        if symbols {
            // D.5: 1010 in UNIFORM.
            for bit in [1, 0, 1, 0] {
                sink.code(states, UNIFORM, bit);
            }
        }
    }
}

/// Codes `passes` passes of a code-block whose coefficients have `planes`
/// magnitude bit-planes, into the codeword segments Tables D.8 and D.9 make
/// of them under `style`.
fn code_block(
    coefficients: &[i32],
    width: usize,
    height: usize,
    band: Band,
    style: u8,
    planes: u32,
    passes: u32,
) -> Vec<Coded> {
    let magnitude: Vec<u32> = coefficients.iter().map(|c| c.unsigned_abs()).collect();
    let negative: Vec<bool> = coefficients.iter().map(|c| *c < 0).collect();
    let mut coder = BlockCoder {
        width,
        height,
        magnitude: &magnitude,
        negative: &negative,
        band,
        causal: style & CAUSAL != 0,
        sigma: vec![false; width * height],
        visited: vec![false; width * height],
        refined: vec![false; width * height],
    };
    let predictable = style & PREDICTABLE != 0;
    let mut states = States::initial();
    let mut out = Vec::new();
    let mut sink: Option<Sink> = None;
    let mut first = 0u32;
    let mut ends = Vec::new();
    for k in 0..passes {
        // Pass 0 codes the top plane, and each three after it the next.
        let plane = planes - 1 - k.div_ceil(3);
        let current = sink.get_or_insert_with(|| {
            first = k;
            ends.clear();
            if raw_pass(style, k) {
                Sink::Raw(Raw::new())
            } else {
                Sink::Mq(Mq::new())
            }
        });
        match pass_kind(k) {
            Pass::Significance => coder.significance(current, &mut states, plane),
            Pass::Refinement => coder.refinement(current, &mut states, plane),
            Pass::Cleanup => {
                coder.cleanup(current, &mut states, plane, style & SEGMENTATION != 0);
            }
        }
        ends.push(current.len());
        if terminates(style, k) || k + 1 == passes {
            if let Some(done) = sink.take() {
                let bytes = done.finish(predictable);
                let total = bytes.len();
                out.push(Coded {
                    bytes,
                    first,
                    passes: k - first + 1,
                    ends: ends.iter().map(|e| (*e).min(total)).collect(),
                });
            }
        }
        if style & RESET != 0 {
            states = States::initial();
        }
    }
    out
}

// --- the model -----------------------------------------------------------

/// Table A.16.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Order {
    Lrcp,
    Rlcp,
    Rpcl,
    Pcrl,
    Cprl,
}

impl Order {
    fn byte(self) -> u8 {
        self as u8
    }
}

/// SPcod or SPcoc (Table A.15), with whether `Scod` or `Scoc` defines the
/// precincts.
#[derive(Clone, Debug)]
struct Style {
    levels: u8,
    xcb: u8,
    ycb: u8,
    cb_style: u8,
    reversible: bool,
    precincts: Option<Vec<(u8, u8)>>,
}

impl Style {
    fn precinct(&self, r: usize) -> (u8, u8) {
        self.precincts
            .as_ref()
            .and_then(|p| p.get(r).copied())
            .unwrap_or((15, 15))
    }
}

/// QCD or QCC's `Sqcd` and steps (Tables A.28 to A.30).
#[derive(Clone, Debug)]
struct Quant {
    /// 0 none, 1 scalar derived, 2 scalar expounded.
    style: u8,
    guard: u8,
    steps: Vec<(u8, u16)>,
}

impl Quant {
    /// Whether this quantization can serve a tile-component coded with
    /// `style`: the arithmetic Table A.28 pairs it with, and the steps its
    /// decomposition levels ask for.
    fn fits(&self, style: &Style) -> bool {
        let none = self.style == 0;
        let count = if self.style == 1 {
            1
        } else {
            1 + 3 * usize::from(style.levels)
        };
        none == style.reversible && self.steps.len() == count
    }

    /// `Mb` (E-2) of subband `band` — 0 the LL band, then HL, LH and HH of
    /// each resolution in turn — for `levels` decomposition levels.
    fn mb(&self, band: usize, levels: u8) -> i64 {
        let exponent = if self.style == 1 {
            // (E-5): ε_b = ε_0 - N_L + n_b.
            let resolution = if band == 0 { 0 } else { (band - 1) / 3 + 1 };
            let nb = if resolution == 0 {
                i64::from(levels)
            } else {
                i64::from(levels) - resolution as i64 + 1
            };
            i64::from(self.steps[0].0) - i64::from(levels) + nb
        } else {
            i64::from(self.steps[band].0)
        };
        i64::from(self.guard) + exponent - 1
    }
}

/// SGcod (Table A.14), `Scod`'s SOP and EPH bits, and the style.
#[derive(Clone, Debug)]
struct Cod {
    order: Order,
    layers: u16,
    mct: bool,
    sop: bool,
    eph: bool,
    style: Style,
}

/// One POC progression (Table A.32).
#[derive(Clone, Copy, Debug)]
struct Poc {
    rs: u8,
    cs: u16,
    lye: u16,
    re: u8,
    ce: u16,
    order: Order,
}

/// SIZ's per-component fields.
#[derive(Clone, Copy, Debug)]
struct Component {
    precision: u8,
    signed: bool,
    dx: u8,
    dy: u8,
}

/// SIZ (Table A.9).
#[derive(Clone, Debug)]
struct Siz {
    rsiz: u16,
    xsiz: i64,
    ysiz: i64,
    xosiz: i64,
    yosiz: i64,
    xtsiz: i64,
    ytsiz: i64,
    xtosiz: i64,
    ytosiz: i64,
    components: Vec<Component>,
}

impl Siz {
    fn tiles_x(&self) -> i64 {
        ceil_div(self.xsiz - self.xtosiz, self.xtsiz)
    }

    fn tiles_y(&self) -> i64 {
        ceil_div(self.ysiz - self.ytosiz, self.ytsiz)
    }

    /// B.3's tile bounds.
    fn tile(&self, t: usize) -> (i64, i64, i64, i64) {
        let p = t as i64 % self.tiles_x();
        let q = t as i64 / self.tiles_x();
        (
            (self.xtosiz + p * self.xtsiz).max(self.xosiz),
            (self.ytosiz + q * self.ytsiz).max(self.yosiz),
            (self.xtosiz + (p + 1) * self.xtsiz).min(self.xsiz),
            (self.ytosiz + (q + 1) * self.ytsiz).min(self.ysiz),
        )
    }
}

/// The marker segments one header carries, beside SIZ.
#[derive(Clone, Debug, Default)]
struct Header {
    cod: Option<Cod>,
    coc: Vec<(usize, Style)>,
    qcd: Option<Quant>,
    qcc: Vec<(usize, Quant)>,
    poc: Option<Vec<Poc>>,
}

// --- geometry (B.3 to B.7) and the code-blocks ---------------------------

/// What one code-block contributes: per layer, its pieces — the bytes of
/// one signalled length and the passes it closes over — in order.
struct Block {
    zero_planes: u32,
    /// The layer it is first included in, or the layer count where never.
    first_layer: u32,
    layers: Vec<Vec<(Vec<u8>, u32)>>,
    included: bool,
    lblock: u32,
}

/// One precinct as confined to one subband: its code-block grid, the
/// blocks in raster order, and its two tag trees.
struct PrecinctBand {
    width: usize,
    blocks: Vec<Block>,
    inclusion: TagTree,
    zero: TagTree,
}

/// One resolution of a tile-component.
struct Resolution {
    x0: i64,
    y0: i64,
    ppx: u8,
    ppy: u8,
    across: i64,
    down: i64,
    /// Per precinct, per subband of the resolution: `None` where the
    /// precinct holds none of that subband's coefficients.
    precincts: Vec<Vec<Option<PrecinctBand>>>,
}

struct TileComponent {
    levels: u8,
    resolutions: Vec<Resolution>,
}

struct Tile {
    bounds: Bounds,
    cod: Cod,
    pocs: Option<Vec<Poc>>,
    components: Vec<TileComponent>,
    /// Whether its packet headers travel in PPT segments.
    ppt: bool,
}

/// How a generated code-block gets its data.
#[derive(Clone, Copy)]
struct Data {
    encoded: bool,
}

/// The pieces of a contribution of `count` passes from pass `first`: one
/// per pass the schedule terminates, and one closing at the last.
fn pieces(style: u8, first: u32, count: u32) -> Vec<u32> {
    let mut out = Vec::new();
    let mut start = first;
    for p in first..first + count {
        if terminates(style, p) || p + 1 == first + count {
            out.push(p + 1 - start);
            start = p + 1;
        }
    }
    out
}

/// The input's own bytes for one piece of raw data, kept to what an
/// arithmetic coder can emit: no byte after 0xFF above 0x8F (D.4's marker
/// range never appears, C.2.7's stuffing never lets it), and no 0xFF last
/// (B.10.7).
fn raw_piece(c: &mut Choices) -> Vec<u8> {
    let mut bytes = c.bytes(MAX_PIECE);
    for i in 1..bytes.len() {
        if bytes[i - 1] == 0xFF {
            bytes[i] &= 0x7F;
        }
    }
    if bytes.last() == Some(&0xFF) {
        bytes.pop();
    }
    bytes
}

/// A code-block: how many bit-planes it codes, how many passes, and how the
/// passes fall across the layers, with the data that codes them.
#[allow(clippy::too_many_arguments)]
fn block(
    c: &mut Choices,
    width: usize,
    height: usize,
    band: Band,
    style: u8,
    mb: i64,
    layers: u16,
    data: Data,
) -> Block {
    let planes = c.range(0, mb.min(MAX_PLANES)) as u32;
    let zero_planes = (mb as u32).saturating_sub(planes);
    let most = if planes == 0 {
        0
    } else {
        (1 + 3 * (planes - 1)).min(MAX_PASSES)
    };
    let total = c.range(0, i64::from(most)) as u32;
    let layers = u32::from(layers);
    let mut per_layer = vec![0u32; layers as usize];
    let first_layer = if total == 0 {
        layers
    } else {
        let first = c.range(0, i64::from(layers) - 1) as u32;
        let mut left = total;
        for l in first..layers {
            let n = if l + 1 == layers {
                left
            } else if l == first {
                c.range(1, i64::from(left)) as u32
            } else {
                c.range(0, i64::from(left)) as u32
            };
            per_layer[l as usize] = n;
            left -= n;
        }
        first
    };

    let mut out = vec![Vec::new(); layers as usize];
    if total > 0 {
        if data.encoded {
            let density = c.below(4) as u32;
            let mut rng = Rng(c.range(1, i64::from(u32::MAX)) as u64 | 1 << 40);
            let top = 1u32 << planes;
            let coefficients: Vec<i32> = (0..width * height)
                .map(|_| {
                    let pick = rng.next();
                    if pick % 4 >= density {
                        return 0;
                    }
                    let magnitude = (rng.next() % top) as i32;
                    if rng.next() & 1 == 1 {
                        -magnitude
                    } else {
                        magnitude
                    }
                })
                .collect();
            let coded = code_block(&coefficients, width, height, band, style, planes, total);
            // Each layer's pieces take their bytes from the segment the
            // passes are in, up to the bytes the last pass needs, with a 0xFF
            // that would end a piece moved to the next (B.10.7).
            let mut segment = 0usize;
            let mut taken = 0usize;
            let mut pass = 0u32;
            for (l, &n) in per_layer.iter().enumerate() {
                if n == 0 {
                    continue;
                }
                for count in pieces(style, pass, n) {
                    let last = pass + count - 1;
                    while coded[segment].first + coded[segment].passes <= pass {
                        segment += 1;
                        taken = 0;
                    }
                    let seg = &coded[segment];
                    let mut end = if last + 1 == seg.first + seg.passes {
                        seg.bytes.len()
                    } else {
                        seg.ends[(last - seg.first) as usize].max(taken)
                    };
                    if end > taken && end < seg.bytes.len() && seg.bytes[end - 1] == 0xFF {
                        end -= 1;
                    }
                    out[l].push((seg.bytes[taken..end].to_vec(), count));
                    taken = end;
                    pass += count;
                }
            }
        } else {
            let mut pass = 0u32;
            for (l, &n) in per_layer.iter().enumerate() {
                if n == 0 {
                    continue;
                }
                for count in pieces(style, pass, n) {
                    out[l].push((raw_piece(c), count));
                    pass += count;
                }
            }
        }
    }
    Block {
        zero_planes,
        first_layer,
        layers: out,
        included: false,
        lblock: 3,
    }
}

/// A rectangle `(x0, y0, x1, y1)`, the far edges outside it.
type Bounds = (i64, i64, i64, i64);

/// B.5's subband bounds within tile-component `(x0, y0, x1, y1)` at
/// decomposition level `nb`, for a subband `(xob, yob)` from its corner.
fn subband(bounds: Bounds, nb: u32, xob: i64, yob: i64) -> Bounds {
    let (x0, y0, x1, y1) = bounds;
    let scale = 1i64 << nb;
    let half = if nb == 0 { 0 } else { 1i64 << (nb - 1) };
    (
        ceil_div(x0 - half * xob, scale),
        ceil_div(y0 - half * yob, scale),
        ceil_div(x1 - half * xob, scale),
        ceil_div(y1 - half * yob, scale),
    )
}

/// Builds one tile-component's resolutions, precincts and code-blocks.
#[allow(clippy::too_many_arguments)]
fn tile_component(
    c: &mut Choices,
    bounds: Bounds,
    style: &Style,
    quant: &Quant,
    layers: u16,
    data: Data,
    seen: &mut Codestream,
) -> TileComponent {
    let levels = style.levels;
    let nl = u32::from(levels);
    let mut resolutions = Vec::with_capacity(usize::from(levels) + 1);
    for r in 0..=nl {
        let shift = 1i64 << (nl - r);
        let (rx0, ry0, rx1, ry1) = (
            ceil_div(bounds.0, shift),
            ceil_div(bounds.1, shift),
            ceil_div(bounds.2, shift),
            ceil_div(bounds.3, shift),
        );
        let (ppx, ppy) = style.precinct(r as usize);
        let (across, down) = if rx1 > rx0 && ry1 > ry0 {
            (
                ceil_div(rx1, 1 << ppx) - floor_div(rx0, 1 << ppx),
                ceil_div(ry1, 1 << ppy) - floor_div(ry0, 1 << ppy),
            )
        } else {
            (0, 0)
        };
        if across * down > 1 {
            seen.precincts = true;
        }
        // The subbands of the resolution, with their orientation, their
        // index in the quantization's steps, and their bounds.
        let bands: Vec<(Band, usize, Bounds)> = if r == 0 {
            vec![(Band::Ll, 0, subband(bounds, nl, 0, 0))]
        } else {
            let nb = nl - r + 1;
            let base = 1 + 3 * (r as usize - 1);
            vec![
                (Band::Hl, base, subband(bounds, nb, 1, 0)),
                (Band::Lh, base + 1, subband(bounds, nb, 0, 1)),
                (Band::Hh, base + 2, subband(bounds, nb, 1, 1)),
            ]
        };
        // B.6 and B.7: a precinct covers 2^PPx of the resolution, so half as
        // much of each subband above resolution 0, and the code-blocks are
        // no larger than that.
        let (ex, ey) = if r == 0 {
            (u32::from(ppx), u32::from(ppy))
        } else {
            (u32::from(ppx) - 1, u32::from(ppy) - 1)
        };
        let (xcb, ycb) = (u32::from(style.xcb).min(ex), u32::from(style.ycb).min(ey));
        let mut precincts = Vec::with_capacity((across * down) as usize);
        for k in 0..across * down {
            let px = floor_div(rx0, 1 << ppx) + k % across.max(1);
            let py = floor_div(ry0, 1 << ppy) + k / across.max(1);
            let mut per_band = Vec::with_capacity(bands.len());
            for &(band, index, (bx0, by0, bx1, by1)) in &bands {
                let x0 = (px << ex).max(bx0);
                let y0 = (py << ey).max(by0);
                let x1 = ((px + 1) << ex).min(bx1);
                let y1 = ((py + 1) << ey).min(by1);
                if x1 <= x0 || y1 <= y0 {
                    per_band.push(None);
                    continue;
                }
                let cx0 = floor_div(x0, 1 << xcb);
                let cy0 = floor_div(y0, 1 << ycb);
                let cx1 = ceil_div(x1, 1 << xcb);
                let cy1 = ceil_div(y1, 1 << ycb);
                let width = (cx1 - cx0) as usize;
                let height = (cy1 - cy0) as usize;
                let mb = quant.mb(index, levels);
                let mut blocks = Vec::with_capacity(width * height);
                for gy in cy0..cy1 {
                    for gx in cx0..cx1 {
                        let w = (((gx + 1) << xcb).min(x1) - (gx << xcb).max(x0)) as usize;
                        let h = (((gy + 1) << ycb).min(y1) - (gy << ycb).max(y0)) as usize;
                        let made = block(c, w, h, band, style.cb_style, mb, layers, data);
                        let passes: u32 = made.layers.iter().flatten().map(|(_, n)| *n).sum();
                        if style.cb_style & BYPASS != 0 && passes > 10 {
                            seen.raw += 1;
                        }
                        blocks.push(made);
                    }
                }
                let inclusion: Vec<u32> = blocks.iter().map(|b| b.first_layer).collect();
                let zero: Vec<u32> = blocks.iter().map(|b| b.zero_planes).collect();
                per_band.push(Some(PrecinctBand {
                    width,
                    inclusion: TagTree::new(width, height, &inclusion),
                    zero: TagTree::new(width, height, &zero),
                    blocks,
                }));
            }
            precincts.push(per_band);
        }
        resolutions.push(Resolution {
            x0: rx0,
            y0: ry0,
            ppx,
            ppy,
            across,
            down,
            precincts,
        });
    }
    TileComponent {
        levels,
        resolutions,
    }
}

/// B.12's packet sequence for a tile, over B.12.2's volumes: `(layer,
/// resolution, component, precinct)`, each packet once.
fn packet_order(siz: &Siz, tile: &Tile) -> Vec<(u16, usize, usize, usize)> {
    let components = tile.components.len();
    let max_res = tile
        .components
        .iter()
        .map(|tc| usize::from(tc.levels) + 1)
        .max()
        .unwrap_or(0);
    let volumes: Vec<Poc> = match &tile.pocs {
        Some(pocs) => pocs.clone(),
        None => vec![Poc {
            rs: 0,
            cs: 0,
            lye: tile.cod.layers,
            re: max_res as u8,
            ce: components as u16,
            order: tile.cod.order,
        }],
    };
    let mut out = Vec::new();
    let mut sent = BTreeSet::new();
    let mut emit = |l: u16, r: usize, i: usize, k: usize, out: &mut Vec<_>| {
        if sent.insert((l, r, i, k)) {
            out.push((l, r, i, k));
        }
    };
    let (tx0, ty0, tx1, ty1) = tile.bounds;
    // B.12.1.3's test: whether `(x, y)` is a precinct's origin of component
    // `i` at resolution `r`, and which precinct.
    let origin = |i: usize, r: usize, x: i64, y: i64| -> Option<usize> {
        let tc = &tile.components[i];
        let res = tc.resolutions.get(r)?;
        if res.across == 0 || res.down == 0 {
            return None;
        }
        let e = u32::from(tc.levels) - r as u32;
        let comp = &siz.components[i];
        let (ppx, ppy) = (u32::from(res.ppx), u32::from(res.ppy));
        let on_y = y % (i64::from(comp.dy) << (ppy + e)) == 0
            || (y == ty0 && (res.y0 << e) % (1i64 << (ppy + e)) != 0);
        let on_x = x % (i64::from(comp.dx) << (ppx + e)) == 0
            || (x == tx0 && (res.x0 << e) % (1i64 << (ppx + e)) != 0);
        if !on_x || !on_y {
            return None;
        }
        // (B-20).
        let kx =
            floor_div(ceil_div(x, i64::from(comp.dx) << e), 1 << ppx) - floor_div(res.x0, 1 << ppx);
        let ky =
            floor_div(ceil_div(y, i64::from(comp.dy) << e), 1 << ppy) - floor_div(res.y0, 1 << ppy);
        if kx < 0 || ky < 0 || kx >= res.across || ky >= res.down {
            return None;
        }
        Some((ky * res.across + kx) as usize)
    };
    let precincts = |i: usize, r: usize| -> usize {
        tile.components[i]
            .resolutions
            .get(r)
            .map_or(0, |res| (res.across * res.down) as usize)
    };
    for v in &volumes {
        let (r0, r1) = (usize::from(v.rs), usize::from(v.re).min(max_res));
        let ce = if v.ce == 0 { 256 } else { usize::from(v.ce) };
        let (c0, c1) = (usize::from(v.cs).min(components), ce.min(components));
        let layers = v.lye.min(tile.cod.layers);
        match v.order {
            Order::Lrcp => {
                for l in 0..layers {
                    for r in r0..r1 {
                        for i in c0..c1 {
                            for k in 0..precincts(i, r) {
                                emit(l, r, i, k, &mut out);
                            }
                        }
                    }
                }
            }
            Order::Rlcp => {
                for r in r0..r1 {
                    for l in 0..layers {
                        for i in c0..c1 {
                            for k in 0..precincts(i, r) {
                                emit(l, r, i, k, &mut out);
                            }
                        }
                    }
                }
            }
            Order::Rpcl => {
                for r in r0..r1 {
                    for y in ty0..ty1 {
                        for x in tx0..tx1 {
                            for i in c0..c1 {
                                if let Some(k) = origin(i, r, x, y) {
                                    for l in 0..layers {
                                        emit(l, r, i, k, &mut out);
                                    }
                                }
                            }
                        }
                    }
                }
            }
            Order::Pcrl => {
                for y in ty0..ty1 {
                    for x in tx0..tx1 {
                        for i in c0..c1 {
                            for r in r0..r1 {
                                if let Some(k) = origin(i, r, x, y) {
                                    for l in 0..layers {
                                        emit(l, r, i, k, &mut out);
                                    }
                                }
                            }
                        }
                    }
                }
            }
            Order::Cprl => {
                for i in c0..c1 {
                    for y in ty0..ty1 {
                        for x in tx0..tx1 {
                            for r in r0..r1 {
                                if let Some(k) = origin(i, r, x, y) {
                                    for l in 0..layers {
                                        emit(l, r, i, k, &mut out);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    out
}

/// One packet's header (B.10.8) and body (B.9), which also updates the
/// precinct's tag trees and its code-blocks' `Lblock`.
fn packet(
    c: &mut Choices,
    bands: &mut [Option<PrecinctBand>],
    layer: u16,
) -> (Vec<u8>, Vec<u8>, bool) {
    let l = usize::from(layer);
    let any_blocks = bands.iter().flatten().any(|b| !b.blocks.is_empty());
    let filled = bands
        .iter()
        .flatten()
        .any(|b| b.blocks.iter().any(|block| !block.layers[l].is_empty()));
    let mut bits = Bits::new();
    let mut body = Vec::new();
    // B.10.3. A packet with nothing in it may still say so code-block by
    // code-block, where it has code-blocks to speak of (B.9's note 2).
    let nonempty = filled || (any_blocks && c.below(4) == 3);
    bits.put(nonempty);
    if nonempty {
        for band in bands.iter_mut().flatten() {
            let width = band.width;
            for i in 0..band.blocks.len() {
                let (gx, gy) = (i % width, i / width);
                let contributes = !band.blocks[i].layers[l].is_empty();
                if band.blocks[i].included {
                    bits.put(contributes);
                } else {
                    let now = band
                        .inclusion
                        .encode(gx, gy, u32::from(layer) + 1, &mut bits);
                    debug_assert_eq!(now, contributes);
                }
                if !contributes {
                    continue;
                }
                let block = &mut band.blocks[i];
                if !block.included {
                    // B.10.5: the zero bit-planes, to their value.
                    band.zero.encode(gx, gy, u32::MAX, &mut bits);
                    block.included = true;
                }
                let pieces = &block.layers[l];
                let passes: u32 = pieces.iter().map(|(_, n)| *n).sum();
                // Table B.4.
                match passes {
                    1 => bits.put_value(0, 1),
                    2 => bits.put_value(0b10, 2),
                    3..=5 => bits.put_value(0b1100 | (passes - 3), 4),
                    6..=36 => {
                        bits.put_value(0b1111, 4);
                        bits.put_value(passes - 6, 5);
                    }
                    _ => {
                        bits.put_value(0b1_1111_1111, 9);
                        bits.put_value(passes - 37, 7);
                    }
                }
                // B.10.7: Lblock raised once for every length, by as much as
                // the longest needs, and now and then by more (note 2).
                let needed = pieces
                    .iter()
                    .map(|(bytes, n)| {
                        bit_length(bytes.len()).saturating_sub(block.lblock + floor_log2(*n))
                    })
                    .max()
                    .unwrap_or(0);
                let raise = needed + u32::from(c.below(8) == 7);
                for _ in 0..raise {
                    bits.put(true);
                }
                bits.put(false);
                block.lblock += raise;
                for (bytes, n) in pieces {
                    bits.put_value(bytes.len() as u32, block.lblock + floor_log2(*n));
                    body.extend_from_slice(bytes);
                }
            }
        }
    }
    (bits.finish(), body, filled)
}

// --- the marker segments -------------------------------------------------

/// What the writer breaks, where it can.
struct Breaking {
    wanted: Option<Break>,
    done: Option<Break>,
}

impl Breaking {
    /// Whether `which` is the break asked for, recording that it was made.
    fn take(&mut self, which: Break) -> bool {
        if self.wanted == Some(which) && self.done.is_none() {
            self.done = Some(which);
            return true;
        }
        false
    }

    fn wants(&self, which: Break) -> bool {
        self.wanted == Some(which) && self.done.is_none()
    }
}

fn write_siz(siz: &Siz, b: &mut Breaking, c: &mut Choices) -> Vec<u8> {
    let mut body = Vec::new();
    let rsiz = if b.take(Break::Rsiz) {
        c.range(3, 0xFFFF) as u16
    } else {
        siz.rsiz
    };
    be16(&mut body, rsiz);
    let (xsiz, ysiz) = (siz.xsiz, siz.ysiz);
    let (mut xosiz, mut yosiz) = (siz.xosiz, siz.yosiz);
    let (mut xtsiz, mut ytsiz) = (siz.xtsiz, siz.ytsiz);
    let (mut xtosiz, mut ytosiz) = (siz.xtosiz, siz.ytosiz);
    let across = c.flag();
    if b.take(Break::TileSize) {
        if across {
            xtsiz = 0;
        } else {
            ytsiz = 0;
        }
    }
    if b.take(Break::ImageOffset) {
        if across {
            xosiz = xsiz + c.range(0, 3);
            xtosiz = xtosiz.min(xosiz);
        } else {
            yosiz = ysiz + c.range(0, 3);
            ytosiz = ytosiz.min(yosiz);
        }
    }
    if b.take(Break::TileOffset) {
        if across {
            xtosiz = xosiz + c.range(1, 3);
        } else {
            ytosiz = yosiz + c.range(1, 3);
        }
    }
    if b.wants(Break::TileGrid) && (xosiz > xtosiz || yosiz > ytosiz) {
        b.take(Break::TileGrid);
        if xosiz > xtosiz && (across || yosiz <= ytosiz) {
            xtsiz = xosiz - xtosiz;
        } else {
            ytsiz = yosiz - ytosiz;
        }
    }
    for value in [xsiz, ysiz, xosiz, yosiz, xtsiz, ytsiz, xtosiz, ytosiz] {
        be32(&mut body, value as u32);
    }
    let count = siz.components.len() as u16;
    let csiz = if b.take(Break::Csiz) {
        let wrong = c.range(0, i64::from(count) + 3) as u16;
        if wrong == count {
            count + 1
        } else {
            wrong
        }
    } else {
        count
    };
    be16(&mut body, csiz);
    let broken_ssiz = b.take(Break::Ssiz).then(|| c.below(siz.components.len()));
    let broken_sep = b
        .take(Break::Separation)
        .then(|| c.below(siz.components.len()));
    for (i, comp) in siz.components.iter().enumerate() {
        let precision = if broken_ssiz == Some(i) {
            c.range(38, 127) as u8
        } else {
            comp.precision - 1
        };
        body.push(precision | if comp.signed { 0x80 } else { 0 });
        let (mut dx, mut dy) = (comp.dx, comp.dy);
        if broken_sep == Some(i) {
            if across {
                dx = 0;
            } else {
                dy = 0;
            }
        }
        body.push(dx);
        body.push(dy);
    }
    segment(SIZ, &body)
}

/// SPcod or SPcoc (Table A.15) and the precinct sizes.
fn write_style(style: &Style, body: &mut Vec<u8>, b: &mut Breaking, c: &mut Choices) {
    let levels = if b.take(Break::Levels) {
        c.range(33, 255) as u8
    } else {
        style.levels
    };
    body.push(levels);
    let (mut xcb, mut ycb) = (style.xcb - 2, style.ycb - 2);
    if b.take(Break::CodeBlockSize) {
        // Table A.18: each value 0 to 8, and `xcb + ycb` at most 12.
        match c.below(3) {
            0 => xcb = c.range(9, 255) as u8,
            1 => ycb = c.range(9, 255) as u8,
            _ => {
                xcb = c.range(1, 8) as u8;
                ycb = c.range(9 - i64::from(xcb), 8) as u8;
            }
        }
    }
    body.push(xcb);
    body.push(ycb);
    let cb_style = if b.take(Break::CodeBlockStyle) {
        style.cb_style | if c.flag() { 0x40 } else { 0x80 }
    } else {
        style.cb_style
    };
    body.push(cb_style);
    let transform = if b.take(Break::Wavelet) {
        c.range(2, 255) as u8
    } else {
        u8::from(style.reversible)
    };
    body.push(transform);
    if let Some(precincts) = &style.precincts {
        let broken = (precincts.len() > 1 && b.take(Break::Precinct))
            .then(|| 1 + c.below(precincts.len() - 1));
        for (r, &(ppx, ppy)) in precincts.iter().enumerate() {
            let (mut ppx, mut ppy) = (ppx, ppy);
            if broken == Some(r) {
                if c.flag() {
                    ppx = 0;
                } else {
                    ppy = 0;
                }
            }
            body.push(ppy << 4 | ppx);
        }
    }
}

fn write_cod(cod: &Cod, b: &mut Breaking, c: &mut Choices) -> Vec<u8> {
    let mut scod = u8::from(cod.style.precincts.is_some())
        | if cod.sop { 0x02 } else { 0 }
        | if cod.eph { 0x04 } else { 0 };
    if b.take(Break::Scod) {
        scod |= 0x08 << c.below(5);
    }
    let mut body = vec![scod];
    body.push(if b.take(Break::Progression) {
        c.range(5, 255) as u8
    } else {
        cod.order.byte()
    });
    be16(
        &mut body,
        if b.take(Break::Layers) { 0 } else { cod.layers },
    );
    body.push(if b.take(Break::Mct) {
        c.range(2, 255) as u8
    } else {
        u8::from(cod.mct)
    });
    write_style(&cod.style, &mut body, b, c);
    segment(COD, &body)
}

fn component_index(body: &mut Vec<u8>, index: usize) {
    // Tables A.22 and A.31: one byte below 257 components; never more here.
    body.push(index as u8);
}

fn write_coc(index: usize, style: &Style, b: &mut Breaking, c: &mut Choices) -> Vec<u8> {
    let mut body = Vec::new();
    component_index(&mut body, index);
    let mut scoc = u8::from(style.precincts.is_some());
    if b.take(Break::Scoc) {
        scoc |= 0x02 << c.below(7);
    }
    body.push(scoc);
    write_style(style, &mut body, b, c);
    segment(COC, &body)
}

/// QCD's or QCC's `Sqcd` and steps.
fn write_quant(quant: &Quant, body: &mut Vec<u8>, b: &mut Breaking, c: &mut Choices) {
    let style = if b.take(Break::Sqcd) {
        c.range(3, 31) as u8
    } else {
        quant.style
    };
    body.push(quant.guard << 5 | style);
    for &(exponent, mantissa) in &quant.steps {
        if quant.style == 0 {
            body.push(exponent << 3);
        } else {
            be16(body, u16::from(exponent) << 11 | mantissa);
        }
    }
}

fn write_qcd(quant: &Quant, b: &mut Breaking, c: &mut Choices) -> Vec<u8> {
    let mut body = Vec::new();
    write_quant(quant, &mut body, b, c);
    segment(QCD, &body)
}

fn write_qcc(index: usize, quant: &Quant, b: &mut Breaking, c: &mut Choices) -> Vec<u8> {
    let mut body = Vec::new();
    component_index(&mut body, index);
    write_quant(quant, &mut body, b, c);
    segment(QCC, &body)
}

fn write_poc(pocs: &[Poc], b: &mut Breaking, c: &mut Choices) -> Vec<u8> {
    let broken = b
        .take(Break::Poc)
        .then(|| (c.below(pocs.len()), c.below(5)));
    let mut body = Vec::new();
    for (n, poc) in pocs.iter().enumerate() {
        let mut p = *poc;
        let mut order = p.order.byte();
        if let Some((at, how)) = broken {
            if at == n {
                match how {
                    0 => p.rs = c.range(33, 255) as u8,
                    1 => p.re = p.rs,
                    2 => p.lye = 0,
                    3 => {
                        p.ce = p.cs.max(1);
                        p.cs = p.ce;
                    }
                    _ => order = c.range(5, 255) as u8,
                }
            }
        }
        body.push(p.rs);
        body.push(p.cs as u8);
        be16(&mut body, p.lye);
        body.push(p.re);
        body.push(p.ce as u8);
        body.push(order);
    }
    segment(POC, &body)
}

// --- the generator -------------------------------------------------------

fn order(c: &mut Choices, positional: bool) -> Order {
    let all = [
        Order::Lrcp,
        Order::Rlcp,
        Order::Rpcl,
        Order::Pcrl,
        Order::Cprl,
    ];
    let n = if positional { 5 } else { 2 };
    all[c.below(n)]
}

fn style(c: &mut Choices, reversible: bool, encoded: bool) -> Style {
    let levels = c.below(usize::from(MAX_LEVELS) + 1) as u8;
    let xcb = 2 + c.below(5) as u8;
    let ycb = 2 + c.below(5) as u8;
    let mut cb_style = c.byte() & 0x3F;
    if !encoded {
        // Segmentation symbols are written by Annex D's coder, never by the
        // input's own bytes.
        cb_style &= !SEGMENTATION;
    }
    let precincts = c.flag().then(|| {
        (0..=levels)
            .map(|r| {
                let sizes: &[u8] = if r == 0 {
                    &[15, 2, 0, 1, 3, 4, 5]
                } else {
                    &[15, 2, 1, 3, 4, 5]
                };
                (*c.pick(sizes), *c.pick(sizes))
            })
            .collect()
    });
    Style {
        levels,
        xcb,
        ycb,
        cb_style,
        reversible,
        precincts,
    }
}

/// A quantization that fits `style` (A.6.4), with `Mb` of every subband in
/// `1..=MAX_MB` whatever the decomposition levels it is read with.
fn quant(c: &mut Choices, style: &Style) -> Quant {
    let guard = c.range(1, 3) as u8;
    let g = i64::from(guard);
    let (lo, hi) = ((2 - g).max(0), MAX_MB + 1 - g);
    let kind = if style.reversible {
        0
    } else if c.flag() {
        2
    } else {
        1
    };
    let steps = if kind == 1 {
        let lo = (i64::from(MAX_LEVELS) + 1 - g).max(0);
        vec![(c.range(lo, hi) as u8, c.range(0, 2047) as u16)]
    } else {
        (0..1 + 3 * usize::from(style.levels))
            .map(|_| {
                let exponent = c.range(lo, hi) as u8;
                let mantissa = if kind == 2 {
                    c.range(0, 2047) as u16
                } else {
                    0
                };
                (exponent, mantissa)
            })
            .collect()
    };
    Quant {
        style: kind,
        guard,
        steps,
    }
}

/// POC progressions over `components`, `resolutions` and `layers`: some
/// volumes, then one that covers what they left, so every packet is in one.
fn pocs(
    c: &mut Choices,
    components: usize,
    resolutions: usize,
    layers: u16,
    positional: bool,
) -> Vec<Poc> {
    let count = 1 + c.below(MAX_VOLUMES);
    let mut out = Vec::with_capacity(count + 1);
    for _ in 0..count {
        let rs = c.range(0, resolutions as i64 - 1) as u8;
        let re = c.range(i64::from(rs) + 1, resolutions as i64) as u8;
        let cs = c.range(0, components as i64 - 1) as u16;
        let ce = c.range(i64::from(cs) + 1, components as i64) as u16;
        let lye = c.range(1, i64::from(layers)) as u16;
        out.push(Poc {
            rs,
            cs,
            lye,
            re,
            ce,
            order: order(c, positional),
        });
    }
    // The last: everything, written as an encoder asking for "the rest"
    // writes it — sometimes with Table A.32's ceilings.
    let wide = c.flag();
    out.push(Poc {
        rs: 0,
        cs: 0,
        lye: if wide { u16::MAX } else { layers },
        re: if wide { 33 } else { resolutions as u8 },
        ce: if wide { 0 } else { components as u16 },
        order: order(c, positional),
    });
    out
}

/// One tile-part as written: its tile, its index, its header segments and
/// its packets (the bit stream's share and the packed header's share).
struct Part {
    tile: usize,
    index: usize,
    header: Vec<Vec<u8>>,
    data: Vec<u8>,
    packed: Vec<Vec<u8>>,
}

/// A codestream built from `c`.
pub fn generate(c: &mut Choices) -> Codestream {
    // The break first, so a codestream with one and without are the same
    // codestream otherwise.
    let pick = c.below(Break::ALL.len() * 2);
    let mut breaking = Breaking {
        wanted: (pick >= Break::ALL.len()).then(|| Break::ALL[pick - Break::ALL.len()]),
        done: None,
    };
    let encoded = c.flag();
    let data = Data { encoded };

    // SIZ.
    let count = 1 + c.below(MAX_COMPONENTS);
    let precision = *c.pick(&[8u8, 12, 16, 1, 4, 10, 5, 7, 2, 3, 6, 9, 11, 13, 14, 15]);
    let mct_anywhere = count >= 3 && c.flag();
    let reversible = !c.flag();
    // B.12.1.3 to B.12.1.5 need each separation a power of two; a
    // separation of 3 keeps every progression to the first two orders.
    let odd = c.below(8) == 7;
    let positional = !odd;
    let separations: &[u8] = if odd { &[1, 1, 2, 4, 3] } else { &[1, 1, 2, 4] };
    let mut components: Vec<Component> = Vec::with_capacity(count);
    for i in 0..count {
        let signed = c.flag();
        // G.2: the components a transform reads share a separation and a
        // bit depth.
        let (dx, dy) = if mct_anywhere && (1..3).contains(&i) {
            (components[0].dx, components[0].dy)
        } else {
            (*c.pick(separations), *c.pick(separations))
        };
        components.push(Component {
            precision,
            signed,
            dx,
            dy,
        });
    }
    let width = c.range(1, MAX_SIZE);
    let height = c.range(1, MAX_SIZE);
    let xosiz = c.range(0, 20);
    let yosiz = c.range(0, 20);
    let xtosiz = c.range(0, xosiz);
    let ytosiz = c.range(0, yosiz);
    let (xsiz, ysiz) = (xosiz + width, yosiz + height);
    let tile_size = |c: &mut Choices, size: i64, offset: i64, tile_offset: i64| -> i64 {
        let span = size - tile_offset;
        if c.below(4) == 3 {
            return span + c.range(1, 8);
        }
        let least = (offset - tile_offset + 1).max(ceil_div(span, MAX_TILES_ACROSS));
        span - c.below((span - least + 1) as usize) as i64
    };
    let xtsiz = tile_size(c, xsiz, xosiz, xtosiz);
    let ytsiz = tile_size(c, ysiz, yosiz, ytosiz);
    let siz = Siz {
        // Table A.10's profiles restrict what this writes; it claims none.
        rsiz: 0,
        xsiz,
        ysiz,
        xosiz,
        yosiz,
        xtsiz,
        ytsiz,
        xtosiz,
        ytosiz,
        components,
    };
    let tiles = (siz.tiles_x() * siz.tiles_y()) as usize;

    // A transform reads components 0 to 2, so where one may run they keep
    // the wavelet the defaults give.
    let flips = |i: usize| !mct_anywhere || i >= 3;

    // The main header.
    let mut main = Header {
        cod: Some(Cod {
            order: order(c, positional),
            layers: 1 + c.below(usize::from(MAX_LAYERS)) as u16,
            mct: mct_anywhere && c.flag(),
            sop: c.flag(),
            eph: c.flag(),
            style: style(c, reversible, encoded),
        }),
        ..Header::default()
    };
    for i in 0..count {
        if c.below(4) == 3 {
            let rev = if flips(i) && c.below(4) == 3 {
                !reversible
            } else {
                reversible
            };
            main.coc.push((i, style(c, rev, encoded)));
        }
    }
    let main_cod = main.cod.clone().unwrap_or_else(|| unreachable!());

    // The tiles' own headers.
    let mut tile_headers: Vec<Header> = Vec::with_capacity(tiles);
    for _ in 0..tiles {
        let mut header = Header::default();
        if c.below(4) == 3 {
            header.cod = Some(Cod {
                order: order(c, positional),
                layers: 1 + c.below(usize::from(MAX_LAYERS)) as u16,
                mct: mct_anywhere && c.flag(),
                sop: c.flag(),
                eph: c.flag(),
                style: style(c, reversible, encoded),
            });
        }
        for i in 0..count {
            if c.below(6) == 5 {
                let rev = if flips(i) && c.below(4) == 3 {
                    !reversible
                } else {
                    reversible
                };
                header.coc.push((i, style(c, rev, encoded)));
            }
        }
        tile_headers.push(header);
    }

    // A.6's precedence: tile COC, tile COD, main COC, main COD.
    let style_for = |main: &Header, tile: &Header, i: usize| -> Style {
        if let Some((_, s)) = tile.coc.iter().find(|(j, _)| *j == i) {
            return s.clone();
        }
        if let Some(cod) = &tile.cod {
            return cod.style.clone();
        }
        if let Some((_, s)) = main.coc.iter().find(|(j, _)| *j == i) {
            return s.clone();
        }
        main_cod.style.clone()
    };

    // Quantization that fits each tile-component, by the same precedence:
    // a QCD for its COD, a QCC wherever what is in force does not fit.
    main.qcd = Some(quant(c, &main_cod.style));
    for i in 0..count {
        let coc = main
            .coc
            .iter()
            .find(|(j, _)| *j == i)
            .map(|(_, s)| s.clone());
        let wanted = coc.unwrap_or_else(|| main_cod.style.clone());
        let fits = main.qcd.as_ref().is_some_and(|q| q.fits(&wanted));
        if !fits || c.below(6) == 5 {
            main.qcc.push((i, quant(c, &wanted)));
        }
    }
    for tile in &mut tile_headers {
        if let Some(cod) = tile.cod.clone() {
            if c.flag() {
                tile.qcd = Some(quant(c, &cod.style));
            }
        }
        for i in 0..count {
            let wanted = style_for(&main, tile, i);
            let in_force = tile
                .qcd
                .as_ref()
                .or_else(|| main.qcc.iter().find(|(j, _)| *j == i).map(|(_, q)| q))
                .or(main.qcd.as_ref());
            let fits = in_force.is_some_and(|q| q.fits(&wanted));
            if !fits || c.below(8) == 7 {
                let q = quant(c, &wanted);
                tile.qcc.push((i, q));
            }
        }
    }

    // Progression order changes.
    let resolutions_in = |main: &Header, tile: &Header| -> usize {
        (0..count)
            .map(|i| usize::from(style_for(main, tile, i).levels) + 1)
            .max()
            .unwrap_or(1)
    };
    let layers_in = |tile: &Header| tile.cod.as_ref().unwrap_or(&main_cod).layers;
    if c.below(4) == 3 {
        let resolutions = (0..tiles)
            .map(|t| resolutions_in(&main, &tile_headers[t]))
            .max()
            .unwrap_or(1);
        let layers = (0..tiles)
            .map(|t| layers_in(&tile_headers[t]))
            .max()
            .unwrap_or(1);
        main.poc = Some(pocs(c, count, resolutions, layers, positional));
    }
    for tile in &mut tile_headers {
        if c.below(6) == 5 {
            let resolutions = resolutions_in(&main, tile);
            let layers = layers_in(tile);
            tile.poc = Some(pocs(c, count, resolutions, layers, positional));
        }
    }

    let mut seen = Codestream {
        bytes: Vec::new(),
        width: width as u32,
        height: height as u32,
        components: count as u8,
        precision,
        broken: None,
        orders: 0,
        tiles,
        parts: 0,
        interleaved: false,
        volumes: main.poc.as_ref().map_or(0, Vec::len),
        tile_pocs: tile_headers.iter().filter(|h| h.poc.is_some()).count(),
        tile_cods: tile_headers.iter().filter(|h| h.cod.is_some()).count(),
        overrides: main.coc.len()
            + main.qcc.len()
            + tile_headers
                .iter()
                .map(|h| h.coc.len() + h.qcc.len())
                .sum::<usize>(),
        styles: 0,
        precincts: false,
        layers: 0,
        levels: 0,
        subsampled: siz.components.iter().any(|k| k.dx > 1 || k.dy > 1),
        rct: false,
        ict: false,
        encoded,
        packets: 0,
        filled: 0,
        sop: false,
        eph: false,
        headers: Headers::InStream,
        psot_zero: false,
        tnsot_zero: false,
        signed: siz.components.iter().any(|k| k.signed),
        raw: 0,
    };

    // Where the packet headers travel.
    let headers = match c.below(4) {
        2 => Headers::Ppt,
        3 => Headers::Ppm,
        _ => Headers::InStream,
    };

    // Every tile: its geometry, its code-blocks, its packets.
    let mut built: Vec<Tile> = Vec::with_capacity(tiles);
    for (t, header) in tile_headers.iter().enumerate() {
        let cod = header.cod.clone().unwrap_or_else(|| main_cod.clone());
        let bounds = siz.tile(t);
        let mut comps = Vec::with_capacity(count);
        for i in 0..count {
            let s = style_for(&main, header, i);
            let q = header
                .qcc
                .iter()
                .find(|(j, _)| *j == i)
                .map(|(_, q)| q.clone())
                .or_else(|| header.qcd.clone())
                .or_else(|| {
                    main.qcc
                        .iter()
                        .find(|(j, _)| *j == i)
                        .map(|(_, q)| q.clone())
                })
                .or_else(|| main.qcd.clone())
                .unwrap_or_else(|| unreachable!());
            seen.styles |= s.cb_style;
            seen.levels = seen.levels.max(s.levels);
            let comp = siz.components[i];
            let (dx, dy) = (i64::from(comp.dx), i64::from(comp.dy));
            let tc_bounds = (
                ceil_div(bounds.0, dx),
                ceil_div(bounds.1, dy),
                ceil_div(bounds.2, dx),
                ceil_div(bounds.3, dy),
            );
            comps.push(tile_component(
                c, tc_bounds, &s, &q, cod.layers, data, &mut seen,
            ));
        }
        if cod.mct {
            if reversible {
                seen.rct = true;
            } else {
                seen.ict = true;
            }
        }
        seen.layers = seen.layers.max(cod.layers);
        seen.sop |= cod.sop;
        seen.eph |= cod.eph;
        let pocs = header.poc.clone().or_else(|| main.poc.clone());
        match &pocs {
            Some(volumes) => {
                for v in volumes {
                    seen.orders |= 1 << v.order.byte();
                }
            }
            None => seen.orders |= 1 << cod.order.byte(),
        }
        built.push(Tile {
            bounds,
            cod,
            pocs,
            components: comps,
            ppt: headers == Headers::Ppt && c.flag(),
        });
    }

    // Each tile's packets, cut into tile-parts at packet boundaries.
    let mut parts_of: Vec<Vec<Part>> = Vec::with_capacity(tiles);
    for (t, tile) in built.iter_mut().enumerate() {
        let order = packet_order(&siz, tile);
        let packed = tile.ppt || headers == Headers::Ppm;
        let mut packets: Vec<(Vec<u8>, Vec<u8>)> = Vec::with_capacity(order.len());
        for (n, &(l, r, i, k)) in order.iter().enumerate() {
            let bands = &mut tile.components[i].resolutions[r].precincts[k];
            let (mut head, body, filled) = packet(c, bands, l);
            seen.packets += 1;
            seen.filled += usize::from(filled);
            if tile.cod.eph {
                be16(&mut head, EPH);
            }
            let mut stream = Vec::new();
            if tile.cod.sop {
                be16(&mut stream, SOP);
                be16(&mut stream, 4);
                be16(&mut stream, (n % 0x1_0000) as u16);
            }
            if packed {
                stream.extend_from_slice(&body);
                packets.push((head, stream));
            } else {
                stream.extend_from_slice(&head);
                stream.extend_from_slice(&body);
                packets.push((Vec::new(), stream));
            }
        }
        // Where the tile is cut, at packet boundaries; two cuts in one place
        // make a part with no packets, which B.11 allows.
        let mut bounds: Vec<usize> = (0..c.below(MAX_PARTS))
            .map(|_| c.below(packets.len() + 1))
            .collect();
        bounds.sort_unstable();
        bounds.insert(0, 0);
        bounds.push(packets.len());
        let mut parts = Vec::with_capacity(bounds.len() - 1);
        for (index, span) in bounds.windows(2).enumerate() {
            let mut part = Part {
                tile: t,
                index,
                header: Vec::new(),
                data: Vec::new(),
                packed: Vec::new(),
            };
            for (head, stream) in &packets[span[0]..span[1]] {
                if packed {
                    part.packed.push(head.clone());
                }
                part.data.extend_from_slice(stream);
            }
            parts.push(part);
        }
        parts_of.push(parts);
    }

    // The tile-part headers: the first part of each tile carries the tile's
    // COD, COC, QCD, QCC and POC, any part a comment, in an order chosen.
    for (t, parts) in parts_of.iter_mut().enumerate() {
        let header = &tile_headers[t];
        let mut first: Vec<Vec<u8>> = Vec::new();
        if let Some(cod) = &header.cod {
            first.push(write_cod(cod, &mut breaking, c));
        }
        for (i, s) in &header.coc {
            first.push(write_coc(*i, s, &mut breaking, c));
        }
        if let Some(q) = &header.qcd {
            first.push(write_qcd(q, &mut breaking, c));
        }
        for (i, q) in &header.qcc {
            first.push(write_qcc(*i, q, &mut breaking, c));
        }
        if let Some(p) = &header.poc {
            first.push(write_poc(p, &mut breaking, c));
        }
        if let Some(part) = parts.first_mut() {
            part.header = first;
        }
        for part in parts.iter_mut() {
            if c.below(8) == 7 {
                part.header.push(segment(COM, &[0, 1, b'g']));
            }
            rotate(&mut part.header, c);
        }
    }

    // The order the tile-parts are written in: each tile's in order, the
    // tiles' interleaved as the input chooses (B.11).
    let mut queues: Vec<std::collections::VecDeque<Part>> = parts_of
        .into_iter()
        .map(|p| p.into_iter().collect())
        .collect();
    let mut sequence: Vec<Part> = Vec::new();
    let mut last_tile: Option<usize> = None;
    loop {
        let open: Vec<usize> = (0..queues.len())
            .filter(|t| !queues[*t].is_empty())
            .collect();
        if open.is_empty() {
            break;
        }
        let t = open[c.below(open.len())];
        if let Some(prev) = last_tile {
            if prev != t && !queues[prev].is_empty() {
                seen.interleaved = true;
            }
        }
        last_tile = Some(t);
        if let Some(part) = queues[t].pop_front() {
            sequence.push(part);
        }
    }
    seen.parts = sequence.len();
    let mut totals = vec![0usize; tiles];
    for part in &sequence {
        totals[part.tile] += 1;
    }

    // The packed headers.
    let mut ppm: Vec<Vec<u8>> = Vec::new();
    if headers == Headers::Ppm && seen.packets > 0 {
        seen.headers = Headers::Ppm;
        // The Nppm and Ippm series, cut into segments only where a packet
        // header ends (A.7.4), each of at least Table A.38's length.
        let mut stream = Vec::new();
        let mut cuts = Vec::new();
        for part in &sequence {
            let total: usize = part.packed.iter().map(Vec::len).sum();
            be32(&mut stream, total as u32);
            for head in &part.packed {
                stream.extend_from_slice(head);
                cuts.push(stream.len());
            }
        }
        ppm = packed_segments(&stream, &cuts, 4, c);
    } else if headers == Headers::Ppt && built.iter().any(|t| t.ppt) {
        seen.headers = Headers::Ppt;
        for part in &mut sequence {
            if part.packed.is_empty() {
                continue;
            }
            let mut stream = Vec::new();
            let mut cuts = Vec::new();
            for head in &part.packed {
                stream.extend_from_slice(head);
                cuts.push(stream.len());
            }
            for (z, run) in packed_segments(&stream, &cuts, 1, c).iter().enumerate() {
                let mut body = vec![z as u8];
                body.extend_from_slice(run);
                part.header.push(segment(PPT, &body));
            }
        }
    }

    // The main header.
    let mut segments: Vec<Vec<u8>> = Vec::new();
    segments.push(write_cod(&main_cod, &mut breaking, c));
    for (i, s) in &main.coc {
        segments.push(write_coc(*i, s, &mut breaking, c));
    }
    if let Some(q) = &main.qcd {
        segments.push(write_qcd(q, &mut breaking, c));
    }
    for (i, q) in &main.qcc {
        segments.push(write_qcc(*i, q, &mut breaking, c));
    }
    if let Some(p) = &main.poc {
        segments.push(write_poc(p, &mut breaking, c));
    }
    for (z, run) in ppm.iter().enumerate() {
        let mut body = vec![z as u8];
        body.extend_from_slice(run);
        segments.push(segment(PPM, &body));
    }
    if c.below(8) == 7 {
        // A.9.1: one registration offset pair per component.
        let mut body = Vec::new();
        for _ in 0..count {
            be16(&mut body, c.range(0, 0xFFFF) as u16);
            be16(&mut body, c.range(0, 0xFFFF) as u16);
        }
        segments.push(segment(CRG, &body));
    }
    if c.below(8) == 7 {
        let mut body = vec![0, 1];
        body.extend_from_slice(b"generated");
        segments.push(segment(COM, &body));
    }
    // The breaks that add a segment rather than change a field.
    if breaking.take(Break::Ccoc) {
        segments.push(write_coc(count, &main_cod.style, &mut breaking, c));
    }
    if breaking.take(Break::Cqcc) {
        let q = main.qcd.clone().unwrap_or_else(|| unreachable!());
        segments.push(write_qcc(count, &q, &mut breaking, c));
    }
    if breaking.wants(Break::Scoc) {
        let index = main.coc.first().map_or(0, |(i, _)| *i);
        if main.coc.is_empty() {
            segments.push(write_coc(index, &main_cod.style, &mut breaking, c));
        }
    }
    if breaking.wants(Break::Poc) && main.poc.is_none() {
        let p = pocs(c, count, 1, 1, positional);
        segments.push(write_poc(&p, &mut breaking, c));
    }
    if breaking.take(Break::Marker) {
        let code = 0xFF70 + c.below(16) as u16;
        segments.push(segment(code, &[0, 0]));
    }
    if breaking.take(Break::Second) {
        if c.flag() {
            segments.push(write_cod(&main_cod, &mut breaking, c));
        } else {
            let q = main.qcd.clone().unwrap_or_else(|| unreachable!());
            segments.push(write_qcd(&q, &mut breaking, c));
        }
    }
    rotate(&mut segments, c);

    let mut out = Vec::new();
    be16(&mut out, SOC);
    out.extend_from_slice(&write_siz(&siz, &mut breaking, c));
    for s in &segments {
        out.extend_from_slice(s);
    }

    // The tile-parts. A break of a tile's part order or count takes a tile
    // of two parts or more; one of an SOT's own fields takes any part.
    let split_tile = (0..tiles).find(|t| totals[*t] > 1);
    let broken_part = c.below(sequence.len());
    let last = sequence.len() - 1;
    // A.4.2: only the last tile-part may run to EOC with a Psot of zero.
    let zero_last = c.below(4) == 3;
    for (n, part) in sequence.iter().enumerate() {
        let mut lsot = 10u16;
        let mut isot = part.tile as u16;
        let mut tpsot = part.index as u8;
        let mut tnsot = if c.below(4) == 3 {
            seen.tnsot_zero = true;
            0
        } else {
            totals[part.tile] as u8
        };
        let mut short_psot = false;
        if split_tile == Some(part.tile) {
            if part.index == 0 && breaking.take(Break::TPsot) {
                tpsot = 1;
            }
            if breaking.wants(Break::TNsot) {
                // The first part states the count and the last another.
                if part.index == 0 {
                    tnsot = totals[part.tile] as u8;
                } else if part.index + 1 == totals[part.tile] {
                    breaking.take(Break::TNsot);
                    tnsot = totals[part.tile] as u8 + 1;
                }
            }
        }
        if n == broken_part {
            if breaking.take(Break::Lsot) {
                lsot = if c.flag() {
                    10 + c.range(1, 6) as u16
                } else {
                    c.range(0, 9) as u16
                };
            }
            if breaking.take(Break::Isot) {
                isot = tiles as u16 + c.below(4) as u16;
            }
            if breaking.take(Break::Psot) {
                short_psot = true;
            }
        }
        let mut header = Vec::new();
        for s in &part.header {
            header.extend_from_slice(s);
        }
        let header_len = 12 + header.len() + 2;
        let psot = if short_psot {
            c.range(1, header_len as i64 - 1) as u32
        } else if n == last && zero_last {
            seen.psot_zero = true;
            0
        } else {
            (header_len + part.data.len()) as u32
        };
        be16(&mut out, SOT);
        be16(&mut out, lsot);
        be16(&mut out, isot);
        be32(&mut out, psot);
        out.push(tpsot);
        out.push(tnsot);
        out.extend_from_slice(&header);
        be16(&mut out, SOD);
        out.extend_from_slice(&part.data);
    }
    be16(&mut out, EOC);

    seen.bytes = out;
    seen.broken = breaking.done;
    seen
}

/// A list of segments in an order the input chooses (A.3: beside SOC, SOT,
/// SOD, EOC and SIZ, marker segments may come in any order).
fn rotate(segments: &mut [Vec<u8>], c: &mut Choices) {
    if segments.len() > 1 {
        let by = c.below(segments.len());
        segments.rotate_left(by);
    }
}

/// A packed header stream cut at some of the places a packet header ends,
/// each run at least `least` bytes, the last ending the stream.
fn packed_segments(stream: &[u8], ends: &[usize], least: usize, c: &mut Choices) -> Vec<Vec<u8>> {
    // Tables A.38 and A.39 index the segments with one byte, so no more than
    // 256 of them: every run but the last is at least a 255th of the stream.
    let most = (4 + c.below(60)).max(stream.len().div_ceil(255));
    let mut out = Vec::new();
    let mut start = 0usize;
    let mut cut_at = ends
        .iter()
        .copied()
        .filter(|e| *e < stream.len())
        .collect::<Vec<_>>();
    cut_at.push(stream.len());
    for &end in &cut_at {
        if end == stream.len() {
            break;
        }
        if end - start >= least.max(most) && stream.len() - end >= least {
            out.push(stream[start..end].to_vec());
            start = end;
        }
    }
    out.push(stream[start..].to_vec());
    out
}

#[cfg(test)]
mod tests;
