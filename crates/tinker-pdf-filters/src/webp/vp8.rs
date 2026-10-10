//! VP8 key frames — RFC 6386, "VP8 Data Format and Decoding Guide" — the
//! lossy bitstream of a WebP.
//!
//! Section numbers below are RFC 6386's. A still WebP's lossy picture is one
//! VP8 key frame, so this is the intra half of the format and nothing else:
//! the boolean entropy decoder (§7), the frame header (§9), the per-macroblock
//! modes (§11), the DCT tokens (§13), dequantization (§14), the inverse WHT
//! and DCT (§14.3, §14.4), intra prediction (§12) and the loop filter (§15).
//! Where the prose and the reference decoder (§20, "dixie") differ in how a
//! thing is said, the reference decoder is followed, because it is what the
//! test vectors were checked against; each place is marked where it matters.
//!
//! # What comes out
//!
//! The decoded planes, cropped to the frame's size: Y at full resolution, U
//! and V at half in each direction rounded up. [`to_argb`] turns them into
//! pixels the way libwebp does — its "fancy" chroma upsampling and its
//! fixed-point BT.601 conversion — because a WebP is a picture people have
//! seen in a browser, and every browser draws it through libwebp. Both are
//! integer arithmetic, so the picture is the same on every machine.

use super::WebpError;
use crate::{Warning, Warnings};

mod tables;
use tables::{AC_Q, COEFF_UPDATE_PROBS, DC_Q, DEFAULT_COEFF_PROBS, KF_B_MODE_PROBS};

// --- the boolean decoder (§7) ---------------------------------------------

/// §7.3's decoder, exactly as §20's `bool_decoder.h` writes it: two bytes of
/// lookahead, a byte shifted in every eight bits.
///
/// Past the end of its partition it shifts in zeros and counts them. Two are
/// the lookahead any decoder holds at the end of a well-formed partition;
/// more mean the partition ended before the data it codes did.
struct BoolDecoder<'a> {
    data: &'a [u8],
    pos: usize,
    value: u32,
    range: u32,
    bit_count: u32,
    phantom: u32,
}

impl<'a> BoolDecoder<'a> {
    fn new(data: &'a [u8]) -> Self {
        let mut d = Self {
            data,
            pos: 0,
            value: 0,
            range: 255,
            bit_count: 0,
            phantom: 0,
        };
        d.value = (d.next_byte() << 8) | d.next_byte();
        d
    }

    fn next_byte(&mut self) -> u32 {
        match self.data.get(self.pos) {
            Some(&b) => {
                self.pos += 1;
                u32::from(b)
            }
            None => {
                self.phantom = self.phantom.saturating_add(1);
                0
            }
        }
    }

    /// One boolean whose probability of being zero is `prob / 256`.
    fn get(&mut self, prob: u8) -> bool {
        let split = 1 + (((self.range - 1) * u32::from(prob)) >> 8);
        let big_split = split << 8;
        let bit = if self.value >= big_split {
            self.range -= split;
            self.value -= big_split;
            true
        } else {
            self.range = split;
            false
        };
        while self.range < 128 {
            self.value <<= 1;
            self.range <<= 1;
            self.bit_count += 1;
            if self.bit_count == 8 {
                self.bit_count = 0;
                self.value |= self.next_byte();
            }
        }
        bit
    }

    fn bit(&mut self) -> bool {
        self.get(128)
    }

    /// §9's unsigned `L(n)`, most significant bit first.
    fn literal(&mut self, n: u32) -> u32 {
        (0..n).fold(0, |v, _| (v << 1) | u32::from(self.bit()))
    }

    /// A magnitude of `n` bits, then a sign.
    fn signed(&mut self, n: u32) -> i32 {
        let magnitude = self.literal(n) as i32;
        if self.bit() {
            -magnitude
        } else {
            magnitude
        }
    }

    /// A flag, then [`BoolDecoder::signed`] when it is set: §9's optional
    /// signed fields.
    fn maybe_signed(&mut self, n: u32) -> i32 {
        if self.bit() {
            self.signed(n)
        } else {
            0
        }
    }

    /// §8.1's tree walk: an index into `tree` per boolean, a leaf stored as
    /// its value negated (so a leaf of value 0 is 0, which no branch is).
    fn tree(&mut self, tree: &[i8], probs: &[u8]) -> u8 {
        let mut i = 0usize;
        loop {
            let prob = probs.get(i >> 1).copied().unwrap_or(128);
            let next = tree
                .get(i + usize::from(self.get(prob)))
                .copied()
                .unwrap_or(0);
            if next <= 0 {
                return next.unsigned_abs();
            }
            i = next as usize;
        }
    }

    fn exhausted(&self) -> bool {
        self.phantom > 2
    }
}

// --- modes (§11) ------------------------------------------------------------

/// §11.2's macroblock modes, numbered as §20 numbers them.
const DC_PRED: u8 = 0;
const V_PRED: u8 = 1;
const H_PRED: u8 = 2;
const TM_PRED: u8 = 3;
const B_PRED: u8 = 4;

/// §11.2's subblock modes.
const B_DC_PRED: u8 = 0;
const B_TM_PRED: u8 = 1;
const B_VE_PRED: u8 = 2;
const B_HE_PRED: u8 = 3;
const B_LD_PRED: u8 = 4;
const B_RD_PRED: u8 = 5;
const B_VR_PRED: u8 = 6;
const B_VL_PRED: u8 = 7;
const B_HD_PRED: u8 = 8;
const B_HU_PRED: u8 = 9;

const KF_Y_MODE_TREE: [i8; 8] = [-4, 2, 4, 6, 0, -1, -2, -3];
const KF_Y_MODE_PROBS: [u8; 4] = [145, 156, 163, 128];
const UV_MODE_TREE: [i8; 6] = [0, 2, -1, 4, -2, -3];
const KF_UV_MODE_PROBS: [u8; 3] = [142, 114, 183];
const B_MODE_TREE: [i8; 18] = [
    0, 2, -1, 4, -2, 6, 8, 12, -3, 10, -5, -6, -4, 14, -7, 16, -8, -9,
];

/// §11.3: the subblock mode a whole-macroblock mode stands for, as context
/// for the subblocks of the macroblock below or to the right.
const fn implied_b_mode(y_mode: u8) -> u8 {
    match y_mode {
        V_PRED => B_VE_PRED,
        H_PRED => B_HE_PRED,
        TM_PRED => B_TM_PRED,
        _ => B_DC_PRED,
    }
}

/// One macroblock's header, from the first partition.
#[derive(Clone, Copy)]
struct Macroblock {
    y_mode: u8,
    uv_mode: u8,
    b_modes: [u8; 16],
    segment: usize,
    skip: bool,
}

// --- tokens (§13) -----------------------------------------------------------

const ZIGZAG: [usize; 16] = [0, 1, 4, 8, 5, 2, 3, 6, 9, 12, 13, 10, 7, 11, 14, 15];
const BANDS: [usize; 16] = [0, 1, 2, 3, 6, 4, 5, 6, 6, 6, 6, 6, 6, 6, 6, 7];

/// §13.2's extra-bit probabilities for `DCT_CAT1` to `DCT_CAT6`, most
/// significant bit first, and the smallest value of each category.
const CAT_PROBS: [&[u8]; 6] = [
    &[159],
    &[165, 145],
    &[173, 148, 140],
    &[176, 155, 140, 135],
    &[180, 157, 141, 134, 130],
    &[254, 254, 243, 230, 196, 177, 153, 140, 133, 130, 129],
];
const CAT_BASE: [i32; 6] = [5, 7, 11, 19, 35, 67];

type BlockProbs = [[[u8; 11]; 3]; 8];

fn category(bd: &mut BoolDecoder<'_>, cat: usize) -> i32 {
    CAT_PROBS[cat]
        .iter()
        .fold(0, |v, &p| (v << 1) | i32::from(bd.get(p)))
        + CAT_BASE[cat]
}

/// One block's tokens, dequantized into `out` in raster order (§13.3).
///
/// Returns whether any token was read before the end of block — the
/// "non-zero" context §13.3 hands to the neighbours, and the fact the loop
/// filter's inner edges depend on. A coefficient is stored as the 16 bits
/// §20 stores it in.
fn read_block(
    bd: &mut BoolDecoder<'_>,
    probs: &BlockProbs,
    ctx: usize,
    first: usize,
    dq: [i32; 2],
    out: &mut [i16],
) -> bool {
    let mut c = first;
    let mut p = &probs[BANDS[c]][ctx.min(2)];
    if !bd.get(p[0]) {
        return false;
    }
    loop {
        if !bd.get(p[1]) {
            // DCT_0. No end of block may follow a zero, so the next token
            // starts at the zero test; a zero in the last position is a
            // malformed block, and §20 ends it there.
            if c == 15 {
                return true;
            }
            c += 1;
            p = &probs[BANDS[c]][0];
            continue;
        }
        let (value, next) = if !bd.get(p[2]) {
            (1, 1)
        } else {
            let v = if !bd.get(p[3]) {
                if !bd.get(p[4]) {
                    2
                } else if !bd.get(p[5]) {
                    3
                } else {
                    4
                }
            } else if !bd.get(p[6]) {
                let cat = usize::from(bd.get(p[7]));
                category(bd, cat)
            } else if !bd.get(p[8]) {
                let cat = 2 + usize::from(bd.get(p[9]));
                category(bd, cat)
            } else {
                let cat = 4 + usize::from(bd.get(p[10]));
                category(bd, cat)
            };
            (v, 2)
        };
        let signed = if bd.bit() { -value } else { value };
        if let Some(slot) = out.get_mut(ZIGZAG[c]) {
            *slot = (signed * dq[usize::from(c > 0)]) as i16;
        }
        if c == 15 {
            return true;
        }
        c += 1;
        p = &probs[BANDS[c]][next];
        if !bd.get(p[0]) {
            return true;
        }
    }
}

/// §14.1's factors for one segment: Y, Y2 and chroma, each DC then AC.
#[derive(Clone, Copy, Default)]
struct Dequant {
    y1: [i32; 2],
    y2: [i32; 2],
    uv: [i32; 2],
}

fn dc_q(q: i32) -> i32 {
    DC_Q[q.clamp(0, 127) as usize]
}

fn ac_q(q: i32) -> i32 {
    AC_Q[q.clamp(0, 127) as usize]
}

/// The non-zero contexts of §13.3: four luma columns or rows, two each for
/// U and V, and the Y2 block.
type Contexts = [bool; 9];

/// A macroblock's 25 blocks (§13): sixteen luma, four U, four V, then Y2
/// when the luma mode has one. Returns whether any block held a token.
fn read_macroblock_tokens(
    bd: &mut BoolDecoder<'_>,
    probs: &[BlockProbs; 4],
    left: &mut Contexts,
    above: &mut Contexts,
    has_y2: bool,
    dq: &Dequant,
    coeffs: &mut [i16; 400],
) -> bool {
    let mut any = false;
    let (first, luma_type) = if has_y2 {
        let ctx = usize::from(left[8]) + usize::from(above[8]);
        let nz = read_block(bd, &probs[1], ctx, 0, dq.y2, &mut coeffs[384..]);
        left[8] = nz;
        above[8] = nz;
        any |= nz;
        (1, 0)
    } else {
        (0, 3)
    };
    for i in 0..16 {
        let (l, a) = (i >> 2, i & 3);
        let ctx = usize::from(left[l]) + usize::from(above[a]);
        let nz = read_block(
            bd,
            &probs[luma_type],
            ctx,
            first,
            dq.y1,
            &mut coeffs[i * 16..],
        );
        left[l] = nz;
        above[a] = nz;
        any |= nz;
    }
    for i in 0..8 {
        // U's two rows and columns are contexts 4 and 5, V's 6 and 7.
        let l = 4 + (i >> 1);
        let a = 4 + (i & 1) + 2 * (i >> 2);
        let ctx = usize::from(left[l]) + usize::from(above[a]);
        let nz = read_block(bd, &probs[2], ctx, 0, dq.uv, &mut coeffs[(16 + i) * 16..]);
        left[l] = nz;
        above[a] = nz;
        any |= nz;
    }
    any
}

// --- the transforms (§14.3, §14.4) -----------------------------------------

/// §14.3's inverse Walsh-Hadamard transform, §20's `vp8_dixie_walsh`, with
/// its 16-bit intermediate.
fn inverse_wht(input: &[i16], out: &mut [i16; 16]) {
    for i in 0..4 {
        let (i0, i1, i2, i3) = (
            i32::from(input[i]),
            i32::from(input[4 + i]),
            i32::from(input[8 + i]),
            i32::from(input[12 + i]),
        );
        let (a1, b1, c1, d1) = (i0 + i3, i1 + i2, i1 - i2, i0 - i3);
        out[i] = (a1 + b1) as i16;
        out[4 + i] = (c1 + d1) as i16;
        out[8 + i] = (a1 - b1) as i16;
        out[12 + i] = (d1 - c1) as i16;
    }
    for r in 0..4 {
        let row = &mut out[4 * r..4 * r + 4];
        let (i0, i1, i2, i3) = (
            i32::from(row[0]),
            i32::from(row[1]),
            i32::from(row[2]),
            i32::from(row[3]),
        );
        let (a1, b1, c1, d1) = (i0 + i3, i1 + i2, i1 - i2, i0 - i3);
        row[0] = ((a1 + b1 + 3) >> 3) as i16;
        row[1] = ((c1 + d1 + 3) >> 3) as i16;
        row[2] = ((a1 - b1 + 3) >> 3) as i16;
        row[3] = ((d1 - c1 + 3) >> 3) as i16;
    }
}

const COS_MINUS_1: i32 = 20091; // cos(pi/8) * sqrt(2) - 1, in 16-bit fixed point
const SIN: i32 = 35468; // sin(pi/8) * sqrt(2)

/// §14.4's inverse DCT added to the prediction in place, §20's
/// `vp8_dixie_idct_add`: columns first into 16 bits, then rows.
fn idct_add(buf: &mut [u8], at: usize, stride: usize, coeffs: &[i16]) {
    let mut tmp = [0i16; 16];
    for i in 0..4 {
        let (i0, i4, i8, i12) = (
            i32::from(coeffs[i]),
            i32::from(coeffs[4 + i]),
            i32::from(coeffs[8 + i]),
            i32::from(coeffs[12 + i]),
        );
        let a1 = i0 + i8;
        let b1 = i0 - i8;
        let c1 = ((i4 * SIN) >> 16) - (i12 + ((i12 * COS_MINUS_1) >> 16));
        let d1 = (i4 + ((i4 * COS_MINUS_1) >> 16)) + ((i12 * SIN) >> 16);
        tmp[i] = (a1 + d1) as i16;
        tmp[12 + i] = (a1 - d1) as i16;
        tmp[4 + i] = (b1 + c1) as i16;
        tmp[8 + i] = (b1 - c1) as i16;
    }
    for r in 0..4 {
        let (t0, t1, t2, t3) = (
            i32::from(tmp[4 * r]),
            i32::from(tmp[4 * r + 1]),
            i32::from(tmp[4 * r + 2]),
            i32::from(tmp[4 * r + 3]),
        );
        let a1 = t0 + t2;
        let b1 = t0 - t2;
        let c1 = ((t1 * SIN) >> 16) - (t3 + ((t3 * COS_MINUS_1) >> 16));
        let d1 = (t1 + ((t1 * COS_MINUS_1) >> 16)) + ((t3 * SIN) >> 16);
        let row = at + r * stride;
        for (k, residual) in [a1 + d1, b1 + c1, b1 - c1, a1 - d1].into_iter().enumerate() {
            let p = &mut buf[row + k];
            *p = (i32::from(*p) + ((residual + 4) >> 3)).clamp(0, 255) as u8;
        }
    }
}

// --- the frame buffer and prediction (§12) ----------------------------------

/// One plane of the macroblock grid, with a column of border to its left, a
/// row above, and four columns to its right for the subblock modes that
/// read above and to the right (§12.3).
///
/// Every index below is a macroblock's pixel or one of those border pixels:
/// a block at `(x, y)` of the grid reaches from `x - 4` to `x + 19` and from
/// `y - 4` to `y + 15`, and the loop filter's left and top edges, which reach
/// four pixels back, are filtered only where a macroblock lies behind them.
struct Plane {
    buf: Vec<u8>,
    stride: usize,
}

impl Plane {
    fn new(width: usize, height: usize) -> Self {
        let stride = width + 5;
        Self {
            buf: vec![0; stride * (height + 1)],
            stride,
        }
    }

    /// The index of pixel `(x, y)` of the grid.
    fn at(&self, x: usize, y: usize) -> usize {
        (y + 1) * self.stride + x + 1
    }
}

/// §12.2's out-of-frame left column — 129 — written the way §20's
/// `fixup_left` writes it: a DC-predicted macroblock below the first row
/// copies its above row there instead, which makes §12.2's "average of the
/// edges that exist" the average of both.
fn fixup_left(p: &mut Plane, at: usize, n: usize, row: usize, mode: u8) {
    let s = p.stride;
    if mode == DC_PRED && row > 0 {
        for i in 0..n {
            p.buf[at + i * s - 1] = p.buf[at - s + i];
        }
    } else {
        for i in 0..=n {
            p.buf[at + i * s - s - 1] = 129;
        }
    }
}

/// §12.2's out-of-frame above row — 127 — §20's `fixup_above`, the same
/// way, and four more 127s above and to the right for the subblock modes.
fn fixup_above(p: &mut Plane, at: usize, n: usize, col: usize, mode: u8) {
    let s = p.stride;
    let above = at - s;
    if mode == DC_PRED && col > 0 {
        for i in 0..n {
            p.buf[above + i] = p.buf[at + i * s - 1];
        }
    } else {
        for v in &mut p.buf[above - 1..above + n] {
            *v = 127;
        }
    }
    for v in &mut p.buf[above + n..above + n + 4] {
        *v = 127;
    }
}

/// §12.2's whole-block predictors, for the 16 x 16 luma, the 8 x 8 chroma
/// and — DC and TM — the 4 x 4 subblocks.
fn predict_block(p: &mut Plane, at: usize, n: usize, mode: u8) {
    let s = p.stride;
    match mode {
        V_PRED => {
            for r in 0..n {
                for c in 0..n {
                    p.buf[at + r * s + c] = p.buf[at - s + c];
                }
            }
        }
        H_PRED => {
            for r in 0..n {
                let l = p.buf[at + r * s - 1];
                for c in 0..n {
                    p.buf[at + r * s + c] = l;
                }
            }
        }
        TM_PRED => {
            let corner = i32::from(p.buf[at - s - 1]);
            for r in 0..n {
                let l = i32::from(p.buf[at + r * s - 1]);
                for c in 0..n {
                    let a = i32::from(p.buf[at - s + c]);
                    p.buf[at + r * s + c] = (l + a - corner).clamp(0, 255) as u8;
                }
            }
        }
        _ => {
            let sum: u32 = (0..n)
                .map(|i| u32::from(p.buf[at - s + i]) + u32::from(p.buf[at + i * s - 1]))
                .sum();
            let dc = ((sum + n as u32) >> (n.trailing_zeros() + 1)) as u8;
            for r in 0..n {
                for c in 0..n {
                    p.buf[at + r * s + c] = dc;
                }
            }
        }
    }
}

/// §12.3's ten subblock predictors, §20's `predict_*_4x4`.
fn predict_subblock(p: &mut Plane, at: usize, mode: u8) {
    let s = p.stride;
    // The row above, from the corner to four pixels past the block.
    let a = |k: usize| i32::from(p.buf[at - s - 1 + k]); // a(0) is the corner
    let l = |k: usize| i32::from(p.buf[at + k * s - 1]);
    let avg3 = |x: i32, y: i32, z: i32| ((x + 2 * y + z + 2) >> 2) as u8;
    let avg2 = |x: i32, y: i32| ((x + y + 1) >> 1) as u8;
    let (e, a0, a1, a2, a3, a4, a5, a6, a7) =
        (a(0), a(1), a(2), a(3), a(4), a(5), a(6), a(7), a(8));
    let (l0, l1, l2, l3) = (l(0), l(1), l(2), l(3));
    let out: [[u8; 4]; 4] = match mode {
        B_VE_PRED => {
            let row = [
                avg3(e, a0, a1),
                avg3(a0, a1, a2),
                avg3(a1, a2, a3),
                avg3(a2, a3, a4),
            ];
            [row; 4]
        }
        B_HE_PRED => [
            [avg3(e, l0, l1); 4],
            [avg3(l0, l1, l2); 4],
            [avg3(l1, l2, l3); 4],
            [avg3(l2, l3, l3); 4],
        ],
        B_LD_PRED => {
            let d = [
                avg3(a0, a1, a2),
                avg3(a1, a2, a3),
                avg3(a2, a3, a4),
                avg3(a3, a4, a5),
                avg3(a4, a5, a6),
                avg3(a5, a6, a7),
                avg3(a6, a7, a7),
            ];
            core::array::from_fn(|r| core::array::from_fn(|c| d[r + c]))
        }
        B_RD_PRED => {
            // Indexed by column minus row, plus three.
            let d = [
                avg3(l3, l2, l1),
                avg3(l2, l1, l0),
                avg3(l1, l0, e),
                avg3(l0, e, a0),
                avg3(e, a0, a1),
                avg3(a0, a1, a2),
                avg3(a1, a2, a3),
            ];
            core::array::from_fn(|r| core::array::from_fn(|c| d[3 + c - r]))
        }
        B_VR_PRED => [
            [avg2(e, a0), avg2(a0, a1), avg2(a1, a2), avg2(a2, a3)],
            [
                avg3(l0, e, a0),
                avg3(e, a0, a1),
                avg3(a0, a1, a2),
                avg3(a1, a2, a3),
            ],
            [avg3(l1, l0, e), avg2(e, a0), avg2(a0, a1), avg2(a1, a2)],
            [
                avg3(l2, l1, l0),
                avg3(l0, e, a0),
                avg3(e, a0, a1),
                avg3(a0, a1, a2),
            ],
        ],
        B_VL_PRED => [
            [avg2(a0, a1), avg2(a1, a2), avg2(a2, a3), avg2(a3, a4)],
            [
                avg3(a0, a1, a2),
                avg3(a1, a2, a3),
                avg3(a2, a3, a4),
                avg3(a3, a4, a5),
            ],
            [avg2(a1, a2), avg2(a2, a3), avg2(a3, a4), avg3(a4, a5, a6)],
            [
                avg3(a1, a2, a3),
                avg3(a2, a3, a4),
                avg3(a3, a4, a5),
                avg3(a5, a6, a7),
            ],
        ],
        B_HD_PRED => [
            [
                avg2(l0, e),
                avg3(l0, e, a0),
                avg3(e, a0, a1),
                avg3(a0, a1, a2),
            ],
            [avg2(l1, l0), avg3(l1, l0, e), avg2(l0, e), avg3(l0, e, a0)],
            [
                avg2(l2, l1),
                avg3(l2, l1, l0),
                avg2(l1, l0),
                avg3(l1, l0, e),
            ],
            [
                avg2(l3, l2),
                avg3(l3, l2, l1),
                avg2(l2, l1),
                avg3(l2, l1, l0),
            ],
        ],
        B_HU_PRED => [
            [
                avg2(l0, l1),
                avg3(l0, l1, l2),
                avg2(l1, l2),
                avg3(l1, l2, l3),
            ],
            [
                avg2(l1, l2),
                avg3(l1, l2, l3),
                avg2(l2, l3),
                avg3(l2, l3, l3),
            ],
            [avg2(l2, l3), avg3(l2, l3, l3), l3 as u8, l3 as u8],
            [l3 as u8; 4],
        ],
        B_TM_PRED => {
            predict_block(p, at, 4, TM_PRED);
            return;
        }
        _ => {
            predict_block(p, at, 4, DC_PRED);
            return;
        }
    };
    for (r, row) in out.iter().enumerate() {
        p.buf[at + r * s..at + r * s + 4].copy_from_slice(row);
    }
}

/// One macroblock predicted and its residual added, §20's
/// `predict_intra_luma` and `predict_intra_chroma`.
fn reconstruct(
    planes: &mut [Plane; 3],
    mbx: usize,
    mby: usize,
    mb: &Macroblock,
    coeffs: &mut [i16; 400],
) {
    let [y, u, v] = planes;
    let at = y.at(mbx * 16, mby * 16);
    let s = y.stride;
    if mb.y_mode == B_PRED {
        // §12.3: the subblocks on the right edge below the first row take
        // their above-right pixels from the macroblock row above, not from
        // the macroblock to the right, which is not decoded yet; §20 copies
        // them down beside the block.
        for k in 1..4 {
            for i in 0..4 {
                y.buf[at + (4 * k - 1) * s + 16 + i] = y.buf[at - s + 16 + i];
            }
        }
        for (i, &mode) in mb.b_modes.iter().enumerate() {
            let b = at + (i >> 2) * 4 * s + (i & 3) * 4;
            predict_subblock(y, b, mode);
            idct_add(&mut y.buf, b, s, &coeffs[i * 16..i * 16 + 16]);
        }
    } else {
        predict_block(y, at, 16, mb.y_mode);
        let mut dc = [0i16; 16];
        inverse_wht(&coeffs[384..400], &mut dc);
        for (i, &d) in dc.iter().enumerate() {
            coeffs[i * 16] = d;
        }
        for i in 0..16 {
            let b = at + (i >> 2) * 4 * s + (i & 3) * 4;
            idct_add(&mut y.buf, b, s, &coeffs[i * 16..i * 16 + 16]);
        }
    }
    for (k, plane) in [u, v].into_iter().enumerate() {
        let at = plane.at(mbx * 8, mby * 8);
        let s = plane.stride;
        predict_block(plane, at, 8, mb.uv_mode);
        for i in 0..4 {
            let b = at + (i >> 1) * 4 * s + (i & 1) * 4;
            let block = (16 + 4 * k + i) * 16;
            idct_add(&mut plane.buf, b, s, &coeffs[block..block + 16]);
        }
    }
}

// --- the loop filter (§15) ----------------------------------------------------

/// What §15.2 needs to know of a macroblock once it is decoded.
#[derive(Clone, Copy)]
struct FilterInfo {
    segment: usize,
    b_pred: bool,
    /// Any token in any block: §20's `eob_mask`, which is what decides
    /// whether the inner edges are filtered — not the skip flag as coded.
    coded: bool,
}

/// §15.1 to §15.3's thresholds for one macroblock: the edge limit (the
/// filter level), the interior limit and the high-edge-variance threshold.
struct Strength {
    level: i32,
    interior: i32,
    hev: i32,
}

struct FilterHeader {
    simple: bool,
    level: i32,
    sharpness: i32,
    ref_delta: i32,
    b_pred_delta: i32,
    segment_levels: Option<([i32; 4], bool)>,
}

impl FilterHeader {
    /// §20's `calculate_filter_parameters`, for an intra macroblock of a key
    /// frame.
    ///
    /// **Followed from §20, not §15's prose**: the segment's level is
    /// clamped to 0 to 63 *before* the deltas are added, as the reference
    /// decoder and libvpx both do.
    fn strength(&self, info: &FilterInfo) -> Strength {
        let mut level = self.level;
        if let Some((levels, absolute)) = self.segment_levels {
            let seg = levels.get(info.segment).copied().unwrap_or(0);
            level = if absolute { seg } else { level + seg };
        }
        level = level.clamp(0, 63);
        level += self.ref_delta;
        if info.b_pred {
            level += self.b_pred_delta;
        }
        let level = level.clamp(0, 63);
        let mut interior = level;
        if self.sharpness > 0 {
            interior >>= if self.sharpness > 4 { 2 } else { 1 };
            interior = interior.min(9 - self.sharpness);
        }
        let interior = interior.max(1);
        let hev = i32::from(level >= 15) + i32::from(level >= 40);
        Strength {
            level,
            interior,
            hev,
        }
    }
}

fn s8(v: i32) -> i32 {
    v.clamp(-128, 127)
}

fn u8c(v: i32) -> u8 {
    v.clamp(0, 255) as u8
}

/// The eight pixels across one edge position: `p3 p2 p1 p0 | q0 q1 q2 q3`,
/// `step` apart, `q0` at `at`.
struct Edge<'a> {
    buf: &'a mut [u8],
    at: usize,
    step: usize,
}

impl Edge<'_> {
    fn get(&self, k: isize) -> i32 {
        i32::from(self.buf[(self.at as isize + k * self.step as isize) as usize])
    }

    fn set(&mut self, k: isize, v: u8) {
        self.buf[(self.at as isize + k * self.step as isize) as usize] = v;
    }

    fn simple_threshold(&self, limit: i32) -> bool {
        (self.get(-1) - self.get(0)).abs() * 2 + ((self.get(-2) - self.get(1)).abs() >> 1) <= limit
    }

    fn normal_threshold(&self, edge: i32, interior: i32) -> bool {
        self.simple_threshold(2 * edge + interior)
            && (self.get(-4) - self.get(-3)).abs() <= interior
            && (self.get(-3) - self.get(-2)).abs() <= interior
            && (self.get(-2) - self.get(-1)).abs() <= interior
            && (self.get(3) - self.get(2)).abs() <= interior
            && (self.get(2) - self.get(1)).abs() <= interior
            && (self.get(1) - self.get(0)).abs() <= interior
    }

    fn high_variance(&self, threshold: i32) -> bool {
        (self.get(-2) - self.get(-1)).abs() > threshold
            || (self.get(1) - self.get(0)).abs() > threshold
    }

    /// §15.2's `common_adjust`, and the subblock filter's outer taps when
    /// `outer` is false.
    fn common(&mut self, outer: bool) {
        let (p1, p0, q0, q1) = (self.get(-2), self.get(-1), self.get(0), self.get(1));
        let mut a = 3 * (q0 - p0);
        if outer {
            a += s8(p1 - q1);
        }
        let a = s8(a);
        let f1 = (a + 4).min(127) >> 3;
        let f2 = (a + 3).min(127) >> 3;
        self.set(-1, u8c(p0 + f2));
        self.set(0, u8c(q0 - f1));
        if !outer {
            let a = (f1 + 1) >> 1;
            self.set(-2, u8c(p1 + a));
            self.set(1, u8c(q1 - a));
        }
    }

    /// §15.3's macroblock-edge filter.
    fn macroblock(&mut self) {
        let (p2, p1, p0, q0, q1, q2) = (
            self.get(-3),
            self.get(-2),
            self.get(-1),
            self.get(0),
            self.get(1),
            self.get(2),
        );
        let w = s8(s8(p1 - q1) + 3 * (q0 - p0));
        let a = (27 * w + 63) >> 7;
        self.set(-1, u8c(p0 + a));
        self.set(0, u8c(q0 - a));
        let a = (18 * w + 63) >> 7;
        self.set(-2, u8c(p1 + a));
        self.set(1, u8c(q1 - a));
        let a = (9 * w + 63) >> 7;
        self.set(-3, u8c(p2 + a));
        self.set(2, u8c(q2 - a));
    }
}

/// Which of §15's three filters an edge takes.
#[derive(Clone, Copy)]
enum EdgeKind {
    Macroblock,
    Subblock,
    Simple,
}

/// One edge of `len` pixels: `along` steps between positions on the edge,
/// `across` between the pixels either side of it.
fn filter_edge(
    buf: &mut [u8],
    start: usize,
    along: usize,
    across: usize,
    len: usize,
    kind: EdgeKind,
    st: &Strength,
) {
    for i in 0..len {
        let mut e = Edge {
            buf: &mut *buf,
            at: start + i * along,
            step: across,
        };
        match kind {
            EdgeKind::Simple => {
                if e.simple_threshold(st.level) {
                    e.common(true);
                }
            }
            EdgeKind::Macroblock => {
                if e.normal_threshold(st.level + 2, st.interior) {
                    if e.high_variance(st.hev) {
                        e.common(true);
                    } else {
                        e.macroblock();
                    }
                }
            }
            EdgeKind::Subblock => {
                if e.normal_threshold(st.level, st.interior) {
                    let hev = e.high_variance(st.hev);
                    e.common(hev);
                }
            }
        }
    }
}

/// The inner edges of a luma and of a chroma macroblock.
const LUMA_INNER: [usize; 3] = [4, 8, 12];
const CHROMA_INNER: [usize; 1] = [4];

/// §15's loop filter over the decoded macroblocks, in raster order, each
/// macroblock's left edge, inner vertical edges, top edge and inner
/// horizontal edges in that order — §20's `filter_row_normal` and
/// `filter_row_simple`.
fn loop_filter(planes: &mut [Plane; 3], hdr: &FilterHeader, mbc: usize, infos: &[FilterInfo]) {
    for (index, info) in infos.iter().enumerate() {
        let (mbx, mby) = (index % mbc, index / mbc);
        let st = hdr.strength(info);
        if st.level == 0 {
            continue;
        }
        let inner = info.coded || info.b_pred;
        if hdr.simple {
            let y = &mut planes[0];
            let (s, at) = (y.stride, y.at(mbx * 16, mby * 16));
            let mb = Strength {
                level: (st.level + 2) * 2 + st.interior,
                interior: 0,
                hev: 0,
            };
            let b = Strength {
                level: st.level * 2 + st.interior,
                interior: 0,
                hev: 0,
            };
            if mbx > 0 {
                filter_edge(&mut y.buf, at, s, 1, 16, EdgeKind::Simple, &mb);
            }
            if inner {
                for k in [4, 8, 12] {
                    filter_edge(&mut y.buf, at + k, s, 1, 16, EdgeKind::Simple, &b);
                }
            }
            if mby > 0 {
                filter_edge(&mut y.buf, at, 1, s, 16, EdgeKind::Simple, &mb);
            }
            if inner {
                for k in [4, 8, 12] {
                    filter_edge(&mut y.buf, at + k * s, 1, s, 16, EdgeKind::Simple, &b);
                }
            }
            continue;
        }
        let geometry = |p: usize| -> (usize, &'static [usize]) {
            if p == 0 {
                (16, &LUMA_INNER)
            } else {
                (8, &CHROMA_INNER)
            }
        };
        if mbx > 0 {
            for (n, plane) in planes.iter_mut().enumerate() {
                let (size, _) = geometry(n);
                let (s, at) = (plane.stride, plane.at(mbx * size, mby * size));
                filter_edge(&mut plane.buf, at, s, 1, size, EdgeKind::Macroblock, &st);
            }
        }
        if inner {
            for (n, plane) in planes.iter_mut().enumerate() {
                let (size, inner_edges) = geometry(n);
                let (s, at) = (plane.stride, plane.at(mbx * size, mby * size));
                for &k in inner_edges {
                    filter_edge(&mut plane.buf, at + k, s, 1, size, EdgeKind::Subblock, &st);
                }
            }
        }
        if mby > 0 {
            for (n, plane) in planes.iter_mut().enumerate() {
                let (size, _) = geometry(n);
                let (s, at) = (plane.stride, plane.at(mbx * size, mby * size));
                filter_edge(&mut plane.buf, at, 1, s, size, EdgeKind::Macroblock, &st);
            }
        }
        if inner {
            for (n, plane) in planes.iter_mut().enumerate() {
                let (size, inner_edges) = geometry(n);
                let (s, at) = (plane.stride, plane.at(mbx * size, mby * size));
                for &k in inner_edges {
                    filter_edge(
                        &mut plane.buf,
                        at + k * s,
                        1,
                        s,
                        size,
                        EdgeKind::Subblock,
                        &st,
                    );
                }
            }
        }
    }
}

// --- the frame ----------------------------------------------------------------

/// A decoded key frame, cropped: Y is `width x height`, U and V are
/// `(width + 1) / 2 x (height + 1) / 2`.
pub(super) struct Picture {
    pub(super) width: usize,
    pub(super) height: usize,
    pub(super) y: Vec<u8>,
    pub(super) u: Vec<u8>,
    pub(super) v: Vec<u8>,
    /// False when a partition ended before the macroblocks it codes; those
    /// left undecoded are black.
    pub(super) complete: bool,
}

fn u24(b: &[u8], at: usize) -> Option<usize> {
    let s = b.get(at..at + 3)?;
    Some(usize::from(s[0]) | (usize::from(s[1]) << 8) | (usize::from(s[2]) << 16))
}

/// A VP8 key frame, §9's header first. `check` charges the picture before
/// any buffer exists.
pub(super) fn decode(
    data: &[u8],
    w: &mut Warnings,
    check: impl FnOnce(usize, usize) -> Result<(), WebpError>,
) -> Result<Picture, WebpError> {
    // §9.1: the frame tag.
    let tag = u24(data, 0).ok_or(WebpError::Truncated)?;
    if tag & 1 != 0 {
        return Err(WebpError::Lossy(
            "an inter frame, which a still image is not",
        ));
    }
    if (tag >> 1) & 7 > 3 {
        return Err(WebpError::Lossy("a version past 3"));
    }
    if (tag >> 4) & 1 == 0 {
        // libwebp refuses one too: a frame its encoder said not to show.
        return Err(WebpError::Lossy("a key frame marked not to be shown"));
    }
    let first_size = tag >> 5;
    // §9.2: the start code, then fourteen bits of each dimension and two of
    // a scaling hint, which says how to display the picture, not decode it.
    let header = data.get(3..10).ok_or(WebpError::Truncated)?;
    if header[..3] != [0x9d, 0x01, 0x2a] {
        return Err(WebpError::Lossy("no key frame start code"));
    }
    let width = usize::from(u16::from_le_bytes([header[3], header[4]]) & 0x3fff);
    let height = usize::from(u16::from_le_bytes([header[5], header[6]]) & 0x3fff);
    if width == 0 || height == 0 {
        return Err(WebpError::BadDimensions {
            width: width as u32,
            height: height as u32,
        });
    }
    check(width, height)?;

    let mut complete = true;
    let body = &data[10..];
    let first = match body.get(..first_size) {
        Some(f) => f,
        None => {
            complete = false;
            body
        }
    };
    let mut bd = BoolDecoder::new(first);

    // §9.2: colour space and clamping. The first is reserved and the second
    // a promise that clamping is not needed; clamping regardless gives the
    // same pixels when the promise is kept.
    let _colour_space = bd.bit();
    let _clamping = bd.bit();

    // §9.3: segmentation.
    let segmentation = bd.bit();
    let (mut update_map, mut absolute) = (false, false);
    let mut seg_quant = [0i32; 4];
    let mut seg_level = [0i32; 4];
    let mut tree_probs = [255u8; 3];
    if segmentation {
        update_map = bd.bit();
        if bd.bit() {
            absolute = bd.bit();
            for q in &mut seg_quant {
                *q = bd.maybe_signed(7);
            }
            for l in &mut seg_level {
                *l = bd.maybe_signed(6);
            }
        }
        if update_map {
            for p in &mut tree_probs {
                *p = if bd.bit() { bd.literal(8) as u8 } else { 255 };
            }
        }
    }

    // §9.6: the loop filter.
    let simple = bd.bit();
    let level = bd.literal(6) as i32;
    let sharpness = bd.literal(3) as i32;
    let (mut ref_delta, mut b_pred_delta) = (0, 0);
    if bd.bit() && bd.bit() {
        let mut refs = [0i32; 4];
        for d in &mut refs {
            *d = bd.maybe_signed(6);
        }
        let mut modes = [0i32; 4];
        for d in &mut modes {
            *d = bd.maybe_signed(6);
        }
        // An intra macroblock's reference is the current frame (0), and
        // B_PRED is mode delta 0.
        ref_delta = refs[0];
        b_pred_delta = modes[0];
    }

    // §9.5: the token partitions.
    let partition_count = 1usize << bd.literal(2);

    // §9.6: the quantizer.
    let q_index = bd.literal(7) as i32;
    let y1_dc = bd.maybe_signed(4);
    let y2_dc = bd.maybe_signed(4);
    let y2_ac = bd.maybe_signed(4);
    let uv_dc = bd.maybe_signed(4);
    let uv_ac = bd.maybe_signed(4);

    // §9.7 and §9.8: a key frame refreshes every reference, so the one
    // flag left is whether these probabilities persist — which, for the
    // only frame there is, they need not.
    let _refresh_entropy = bd.bit();

    // §9.9 and §13.4: the coefficient probability updates.
    let mut probs = DEFAULT_COEFF_PROBS;
    for (i, types) in probs.iter_mut().enumerate() {
        for (j, bands) in types.iter_mut().enumerate() {
            for (k, ctxs) in bands.iter_mut().enumerate() {
                for (l, p) in ctxs.iter_mut().enumerate() {
                    if bd.get(COEFF_UPDATE_PROBS[i][j][k][l]) {
                        *p = bd.literal(8) as u8;
                    }
                }
            }
        }
    }
    // §9.10 and §9.11.
    let skip_prob = if bd.bit() {
        Some(bd.literal(8) as u8)
    } else {
        None
    };

    // §9.5: partition sizes, three bytes each but the last, after the first
    // partition; the last takes the rest. One that says it is longer than
    // the data is read as far as the data goes.
    let rest = body.get(first_size..).unwrap_or(&[]);
    let sizes = 3 * (partition_count - 1);
    let mut partitions = Vec::with_capacity(partition_count);
    let mut at = sizes.min(rest.len());
    if rest.len() < sizes {
        complete = false;
    }
    for i in 0..partition_count {
        let size = if i + 1 < partition_count {
            u24(rest, 3 * i).unwrap_or(0)
        } else {
            rest.len().saturating_sub(at)
        };
        let end = at.saturating_add(size);
        if end > rest.len() {
            complete = false;
        }
        let end = end.min(rest.len());
        partitions.push(BoolDecoder::new(rest.get(at..end).unwrap_or(&[])));
        at = end;
    }

    // §14.1: the factors of each segment.
    //
    // **Followed from §20 and libwebp**: a segment's quantizer index is not
    // clamped until each delta has been added to it.
    let mut dequant = [Dequant::default(); 4];
    for (s, dq) in dequant.iter_mut().enumerate() {
        let q = if !segmentation {
            q_index
        } else if absolute {
            seg_quant[s]
        } else {
            q_index + seg_quant[s]
        };
        *dq = Dequant {
            y1: [dc_q(q + y1_dc), ac_q(q)],
            y2: [dc_q(q + y2_dc) * 2, (ac_q(q + y2_ac) * 155 / 100).max(8)],
            uv: [dc_q(q + uv_dc).min(132), ac_q(q + uv_ac)],
        };
    }

    let filter = FilterHeader {
        simple,
        level,
        sharpness,
        ref_delta,
        b_pred_delta,
        segment_levels: segmentation.then_some((seg_level, absolute)),
    };

    let mbc = width.div_ceil(16);
    let mbr = height.div_ceil(16);
    let mut planes = [
        Plane::new(mbc * 16, mbr * 16),
        Plane::new(mbc * 8, mbr * 8),
        Plane::new(mbc * 8, mbr * 8),
    ];
    let mut above_modes = vec![[B_DC_PRED; 4]; mbc];
    let mut above_ctx = vec![[false; 9]; mbc];
    let mut infos: Vec<FilterInfo> = Vec::with_capacity(mbc * mbr);
    let mut coeffs = [0i16; 400];
    let mut row = Vec::with_capacity(mbc);

    'rows: for mby in 0..mbr {
        // §19.3: the modes of a whole row, from the first partition.
        row.clear();
        let mut left_modes = [B_DC_PRED; 4];
        for above in above_modes.iter_mut() {
            let segment = if update_map {
                if bd.get(tree_probs[0]) {
                    2 + usize::from(bd.get(tree_probs[2]))
                } else {
                    usize::from(bd.get(tree_probs[1]))
                }
            } else {
                0
            };
            let skip = skip_prob.is_some_and(|p| bd.get(p));
            let y_mode = bd.tree(&KF_Y_MODE_TREE, &KF_Y_MODE_PROBS);
            let mut b_modes = [implied_b_mode(y_mode); 16];
            if y_mode == B_PRED {
                for i in 0..16 {
                    let a = if i < 4 { above[i] } else { b_modes[i - 4] };
                    let l = if i & 3 == 0 {
                        left_modes[i >> 2]
                    } else {
                        b_modes[i - 1]
                    };
                    let probs = &KF_B_MODE_PROBS[usize::from(a).min(9)][usize::from(l).min(9)];
                    b_modes[i] = bd.tree(&B_MODE_TREE, probs);
                }
            }
            *above = [b_modes[12], b_modes[13], b_modes[14], b_modes[15]];
            left_modes = [b_modes[3], b_modes[7], b_modes[11], b_modes[15]];
            let uv_mode = bd.tree(&UV_MODE_TREE, &KF_UV_MODE_PROBS);
            row.push(Macroblock {
                y_mode,
                uv_mode,
                b_modes,
                segment,
                skip,
            });
        }
        if bd.exhausted() {
            complete = false;
            break 'rows;
        }

        // §13: each macroblock's tokens, from the partition this row is in,
        // and the macroblock rebuilt from them.
        let part = &mut partitions[mby % partition_count];
        let mut left_ctx = [false; 9];
        for (mbx, mb) in row.iter().enumerate() {
            let has_y2 = mb.y_mode != B_PRED;
            coeffs.fill(0);
            let above = &mut above_ctx[mbx];
            let coded = if mb.skip {
                for c in left_ctx.iter_mut().take(8).chain(above.iter_mut().take(8)) {
                    *c = false;
                }
                if has_y2 {
                    left_ctx[8] = false;
                    above[8] = false;
                }
                false
            } else {
                read_macroblock_tokens(
                    part,
                    &probs,
                    &mut left_ctx,
                    above,
                    has_y2,
                    &dequant[mb.segment],
                    &mut coeffs,
                )
            };
            if part.exhausted() {
                complete = false;
                break 'rows;
            }
            if mbx == 0 {
                let [y, u, v] = &mut planes;
                let at = y.at(0, mby * 16);
                fixup_left(y, at, 16, mby, mb.y_mode);
                let at = u.at(0, mby * 8);
                fixup_left(u, at, 8, mby, mb.uv_mode);
                let at = v.at(0, mby * 8);
                fixup_left(v, at, 8, mby, mb.uv_mode);
            }
            if mby == 0 {
                let [y, u, v] = &mut planes;
                let at = y.at(mbx * 16, 0);
                fixup_above(y, at, 16, mbx, mb.y_mode);
                let at = u.at(mbx * 8, 0);
                fixup_above(u, at, 8, mbx, mb.uv_mode);
                let at = v.at(mbx * 8, 0);
                fixup_above(v, at, 8, mbx, mb.uv_mode);
            }
            reconstruct(&mut planes, mbx, mby, mb, &mut coeffs);
            infos.push(FilterInfo {
                segment: mb.segment,
                b_pred: !has_y2,
                coded,
            });
        }
        // §20: the last macroblock's above-right pixels, for the row below,
        // are its own bottom-right pixel four times.
        let y = &mut planes[0];
        let at = y.at(mbc * 16, mby * 16 + 15);
        let edge = y.buf[at - 1];
        for v in &mut y.buf[at..at + 4] {
            *v = edge;
        }
    }

    if level > 0 {
        loop_filter(&mut planes, &filter, mbc, &infos);
    }
    if !complete {
        w.push(Warning::TruncatedInput);
        // What was not decoded is black: Y 0 under neutral chroma.
        for index in infos.len()..mbc * mbr {
            let (mbx, mby) = (index % mbc, index / mbc);
            for (n, plane) in planes.iter_mut().enumerate() {
                let (size, value) = if n == 0 { (16, 0) } else { (8, 128) };
                for r in 0..size {
                    let at = plane.at(mbx * size, mby * size + r);
                    for p in &mut plane.buf[at..at + size] {
                        *p = value;
                    }
                }
            }
        }
    }

    let crop = |plane: &Plane, w: usize, h: usize| -> Vec<u8> {
        let mut out = Vec::with_capacity(w * h);
        for r in 0..h {
            let at = plane.at(0, r);
            out.extend_from_slice(&plane.buf[at..at + w]);
        }
        out
    };
    let (uw, uh) = (width.div_ceil(2), height.div_ceil(2));
    Ok(Picture {
        width,
        height,
        y: crop(&planes[0], width, height),
        u: crop(&planes[1], uw, uh),
        v: crop(&planes[2], uw, uh),
        complete,
    })
}

// --- to pixels ----------------------------------------------------------------

/// libwebp's `MultHi`, the fixed-point multiply its conversion is built on.
const fn mult_hi(v: i32, coeff: i32) -> i32 {
    (v * coeff) >> 8
}

/// libwebp's `VP8Clip8`: six fractional bits off, clamped to a byte.
const fn clip8(v: i32) -> u32 {
    if v & !16383 == 0 {
        (v >> 6) as u32
    } else if v < 0 {
        0
    } else {
        255
    }
}

/// libwebp's `VP8YuvToRgb`: BT.601's limited-range matrix in 14-bit fixed
/// point, as `0x00RRGGBB`.
const fn yuv_to_rgb(y: u8, u: u8, v: u8) -> u32 {
    let (y, u, v) = (y as i32, u as i32, v as i32);
    let r = clip8(mult_hi(y, 19077) + mult_hi(v, 26149) - 14234);
    let g = clip8(mult_hi(y, 19077) - mult_hi(u, 6419) - mult_hi(v, 13320) + 8708);
    let b = clip8(mult_hi(y, 19077) + mult_hi(u, 33050) - 17685);
    (r << 16) | (g << 8) | b
}

/// The chroma of a pixel in the first or last column, which has one chroma
/// column beside it: the `near` row weighted 3:1 against the `far` one.
const fn fancy_edge(near: u8, far: u8) -> u8 {
    let (n, f) = (near as u32, far as u32);
    ((3 * n + f + 2) >> 2) as u8
}

/// The chroma of the two pixels that sit between chroma columns `x - 1` and
/// `x`: each is the four chroma samples around it weighted 9:3:3:1, nearest
/// first — the left pixel nearest `n0`, the right nearest `n1` — in libwebp's
/// two-step integer form, which is within one of the exact weighting.
const fn fancy_pair(n0: u8, n1: u8, f0: u8, f1: u8) -> (u8, u8) {
    let (n0, n1, f0, f1) = (n0 as u32, n1 as u32, f0 as u32, f1 as u32);
    let avg = n0 + n1 + f0 + f1 + 8;
    let left = (((avg + 2 * (n1 + f0)) >> 3) + n0) >> 1;
    let right = (((avg + 2 * (n0 + f1)) >> 3) + n1) >> 1;
    (left as u8, right as u8)
}

/// One output row through libwebp's "fancy" upsampler (`UpsampleRgbaLinePair`):
/// each chroma sample weighted 9:3:3:1 among the four nearest, `near` being
/// the chroma row on this row's side of the pair and `far` the other.
fn upsample_row(y: &[u8], near: [&[u8]; 2], far: [&[u8]; 2], out: &mut [u32]) {
    let len = y.len();
    let edge = |k: usize, x: usize| -> u8 { fancy_edge(near[k][x], far[k][x]) };
    out[0] = yuv_to_rgb(y[0], edge(0, 0), edge(1, 0));
    let last_pair = (len - 1) >> 1;
    for x in 1..=last_pair {
        let chroma = |k: usize| -> (u8, u8) {
            fancy_pair(near[k][x - 1], near[k][x], far[k][x - 1], far[k][x])
        };
        let ((u0, u1), (v0, v1)) = (chroma(0), chroma(1));
        out[2 * x - 1] = yuv_to_rgb(y[2 * x - 1], u0, v0);
        out[2 * x] = yuv_to_rgb(y[2 * x], u1, v1);
    }
    if len % 2 == 0 {
        out[len - 1] = yuv_to_rgb(y[len - 1], edge(0, last_pair), edge(1, last_pair));
    }
}

/// The picture as ARGB words, with `alpha` when there is one (opaque
/// otherwise): libwebp's `EmitFancyRGB` over the whole frame at once.
///
/// Row 0 and — for an even height — the last row each read one chroma row
/// alone; every other row is one of a pair that straddles two chroma rows.
pub(super) fn to_argb(p: &Picture, alpha: Option<&[u8]>) -> Vec<u32> {
    let (w, h) = (p.width, p.height);
    let uw = w.div_ceil(2);
    let chroma =
        |r: usize| -> [&[u8]; 2] { [&p.u[r * uw..r * uw + uw], &p.v[r * uw..r * uw + uw]] };
    let mut out = vec![0u32; w * h];
    for (j, line) in out.chunks_exact_mut(w).enumerate() {
        let (near, far) = if j == 0 {
            (0, 0)
        } else if j % 2 == 1 {
            // The top of a pair: nearer the chroma row above it.
            if j + 1 < h {
                (j / 2, j / 2 + 1)
            } else {
                (j / 2, j / 2)
            }
        } else {
            // The bottom of a pair: nearer the chroma row below.
            (j / 2, j / 2 - 1)
        };
        upsample_row(&p.y[j * w..j * w + w], chroma(near), chroma(far), line);
        match alpha.and_then(|a| a.get(j * w..j * w + w)) {
            Some(a) => {
                for (px, &a) in line.iter_mut().zip(a) {
                    *px |= u32::from(a) << 24;
                }
            }
            None => {
                for px in line.iter_mut() {
                    *px |= 0xff00_0000;
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests;
