//! ITU-T T.832 9.9.2, 9.9.5 and 9.9.7: the photo core transform (PCT).
//!
//! # Why this stage can be proved rather than only asserted
//!
//! T.832's transform is not a rounded approximation of a real-valued one. It
//! is a **lifting structure over `i32`** — every step of 9.9.7's `InvTodd( )`,
//! `InvToddodd( )` and `T2x2h( )` has the shape `a += f(b)` where `f` depends
//! only on operands the step does not write, so each step is invertible on
//! its own, and therefore so is their composition. That makes the transform a
//! **bijection on `i32^16`**, and bijectivity is checkable without an oracle:
//! [`tests::the_core_transform_round_trips_bit_exactly`] runs random vectors
//! through the inverse and back and requires the original bits.
//!
//! That property alone would not catch a transcription slip *mirrored* into
//! both directions, so it is not the only one. Two more come from the
//! Recommendation's own statements rather than from this file's code:
//!
//! - **`T2x2h( )` is an involution**, so applying it twice is the identity.
//!   9.9.7.2's NOTE states this as "the inverse ... is two successive
//!   applications", whose literal reading would make the operator order
//!   three; it is not, and [`tests::t2x2h_is_an_involution`] measures which
//!   of the two readings holds.
//! - **The DC basis function is flat.** An inverse transform of a block whose
//!   only non-zero coefficient is the DC one must produce a *constant* block;
//!   that is what makes it the DC coefficient. A wrong shift or a wrong sign
//!   anywhere in the lifting chain tilts it, and
//!   [`tests::a_dc_only_block_inverse_transforms_to_a_flat_block`] measures
//!   that it does not.
//!
//! # Determinism (ruling 4)
//!
//! Nothing here is a float, and there is nothing to round. `cargo xtask libm`
//! covers the other half of ruling 4; `#![deny(clippy::float_arithmetic)]`
//! below makes a float on this path a build failure rather than a convention.
//!
//! # Overflow
//!
//! Every arithmetic operation here wraps. A conformant codestream cannot
//! reach the edges of `i32` — 9.8's dequantization is bounded by the profile
//! and level constraints of Annex B — but a fuzzer's bytes can, and a debug
//! panic on a pixel path would be ruling 1's exact failure. Wrapping is also
//! deterministic, which a saturating alternative would be too but at the cost
//! of destroying the bijection this module's evidence rests on.

#![deny(clippy::float_arithmetic)]

use super::tables;

// --- 9.9.7: the basic operations ----------------------------------------

/// 9.9.7.2's `T2x2h( )`.
///
/// `round` is the clause's `valRound`, set to 0 or 1 by the caller. It is not
/// a rounding *mode*: it selects between two distinct operators, and 9.9.7.1
/// uses 1 for the first of its stage-one butterflies and 0 for every other.
pub(crate) fn t2x2h(c: &mut [i32; 4], round: i32) {
    c[0] = c[0].wrapping_add(c[3]);
    c[1] = c[1].wrapping_sub(c[2]);
    let t1 = c[0].wrapping_sub(c[1]).wrapping_add(round) >> 1;
    let t2 = c[2];
    c[2] = t1.wrapping_sub(c[3]);
    c[3] = t1.wrapping_sub(t2);
    c[0] = c[0].wrapping_sub(c[3]);
    c[1] = c[1].wrapping_add(c[2]);
}

/// The inverse of [`t2x2h`], which is [`t2x2h`]: **the operator is an
/// involution.**
///
/// 9.9.7.2's NOTE says "the inverse of `T2x2Th( )` is two successive
/// applications of `T2x2Th`, operating on variables of the array `iCoeff[ ]`
/// with the same value of `valRound`". Read literally that makes the operator
/// order three, and it is not: applying it twice returns the array to its
/// original values, so the inverse is one application and two are the
/// identity. That is a measurement rather than an opinion —
/// [`tests::t2x2h_is_an_involution`] checks it over the coefficient range a
/// conformant codestream produces, at both values of `valRound` — and it is
/// recorded here because the literal reading is the one a transcriber
/// reaches for first, and it silently breaks the forward operator that the
/// round-trip evidence is built on.
fn t2x2h_inverse(c: &mut [i32; 4], round: i32) {
    t2x2h(c, round);
}

/// 9.9.7.3's `InvTodd( )`.
fn inv_todd(c: &mut [i32; 4]) {
    c[1] = c[1].wrapping_add(c[3]);
    c[0] = c[0].wrapping_sub(c[2]);
    c[3] = c[3].wrapping_sub(c[1] >> 1);
    c[2] = c[2].wrapping_add((c[0].wrapping_add(1)) >> 1);
    c[0] = c[0].wrapping_sub((c[1].wrapping_mul(3).wrapping_add(4)) >> 3);
    c[1] = c[1].wrapping_add((c[0].wrapping_mul(3).wrapping_add(4)) >> 3);
    c[2] = c[2].wrapping_sub((c[3].wrapping_mul(3).wrapping_add(4)) >> 3);
    c[3] = c[3].wrapping_add((c[2].wrapping_mul(3).wrapping_add(4)) >> 3);
    c[2] = c[2].wrapping_sub((c[1].wrapping_add(1)) >> 1);
    c[3] = ((c[0].wrapping_add(1)) >> 1).wrapping_sub(c[3]);
    c[1] = c[1].wrapping_add(c[2]);
    c[0] = c[0].wrapping_sub(c[3]);
}

/// The forward counterpart of [`inv_todd`]: each lifting step undone in
/// reverse order.
///
/// `iCoeff[3] = ((iCoeff[0] + 1) >> 1) - iCoeff[3]` is an involution in
/// `iCoeff[3]` for a fixed `iCoeff[0]`, so it undoes itself and appears
/// unchanged below rather than negated.
fn fwd_todd(c: &mut [i32; 4]) {
    c[0] = c[0].wrapping_add(c[3]);
    c[1] = c[1].wrapping_sub(c[2]);
    c[3] = ((c[0].wrapping_add(1)) >> 1).wrapping_sub(c[3]);
    c[2] = c[2].wrapping_add((c[1].wrapping_add(1)) >> 1);
    c[3] = c[3].wrapping_sub((c[2].wrapping_mul(3).wrapping_add(4)) >> 3);
    c[2] = c[2].wrapping_add((c[3].wrapping_mul(3).wrapping_add(4)) >> 3);
    c[1] = c[1].wrapping_sub((c[0].wrapping_mul(3).wrapping_add(4)) >> 3);
    c[0] = c[0].wrapping_add((c[1].wrapping_mul(3).wrapping_add(4)) >> 3);
    c[2] = c[2].wrapping_sub((c[0].wrapping_add(1)) >> 1);
    c[3] = c[3].wrapping_add(c[1] >> 1);
    c[0] = c[0].wrapping_add(c[2]);
    c[1] = c[1].wrapping_sub(c[3]);
}

/// 9.9.7.4's `InvToddodd( )`.
fn inv_toddodd(c: &mut [i32; 4]) {
    c[3] = c[3].wrapping_add(c[0]);
    c[2] = c[2].wrapping_sub(c[1]);
    let t1 = c[3] >> 1;
    let t2 = c[2] >> 1;
    c[0] = c[0].wrapping_sub(t1);
    c[1] = c[1].wrapping_add(t2);
    c[0] = c[0].wrapping_sub((c[1].wrapping_mul(3).wrapping_add(3)) >> 3);
    c[1] = c[1].wrapping_add((c[0].wrapping_mul(3).wrapping_add(3)) >> 2);
    c[0] = c[0].wrapping_sub((c[1].wrapping_mul(3).wrapping_add(4)) >> 3);
    c[1] = c[1].wrapping_sub(t2);
    c[0] = c[0].wrapping_add(t1);
    c[2] = c[2].wrapping_add(c[1]);
    c[3] = c[3].wrapping_sub(c[0]);
    c[1] = c[1].wrapping_neg();
    c[2] = c[2].wrapping_neg();
}

/// The forward counterpart of [`inv_toddodd`].
///
/// `valT1` and `valT2` are captured from `iCoeff[3]` and `iCoeff[2]` after the
/// first two steps and used again after the middle three, during which
/// neither is written — so the forward direction recomputes them at the
/// matching point rather than carrying them across the reversal.
fn fwd_toddodd(c: &mut [i32; 4]) {
    c[2] = c[2].wrapping_neg();
    c[1] = c[1].wrapping_neg();
    c[3] = c[3].wrapping_add(c[0]);
    c[2] = c[2].wrapping_sub(c[1]);
    let t1 = c[3] >> 1;
    let t2 = c[2] >> 1;
    c[0] = c[0].wrapping_sub(t1);
    c[1] = c[1].wrapping_add(t2);
    c[0] = c[0].wrapping_add((c[1].wrapping_mul(3).wrapping_add(4)) >> 3);
    c[1] = c[1].wrapping_sub((c[0].wrapping_mul(3).wrapping_add(3)) >> 2);
    c[0] = c[0].wrapping_add((c[1].wrapping_mul(3).wrapping_add(3)) >> 3);
    c[1] = c[1].wrapping_sub(t2);
    c[0] = c[0].wrapping_add(t1);
    c[2] = c[2].wrapping_add(c[1]);
    c[3] = c[3].wrapping_sub(c[0]);
}

/// 9.9.7.5's `InvPermute( )`, using Table 164's `InvPermArr[i]`.
fn inv_permute(c: &mut [i32; 16]) {
    let mut temp = [0i32; 16];
    for (i, &to) in tables::INV_PERM.iter().enumerate() {
        temp[to] = c[i];
    }
    *c = temp;
}

/// The inverse of [`inv_permute`]: the same table read the other way.
fn fwd_permute(c: &mut [i32; 16]) {
    let mut temp = [0i32; 16];
    for (i, &from) in tables::INV_PERM.iter().enumerate() {
        temp[i] = c[from];
    }
    *c = temp;
}

/// 9.9.7.7's `T2pt( )`, the two-point transform.
///
/// Only 9.9.2's YUV422 branch calls it, and that internal format is refused
/// by name in [`super`] — so nothing in the decode path reaches it. It is
/// here because it is one of 9.9.7's basic operations and because
/// [`tests::the_two_point_transform_round_trips`] holds it to the same
/// reversibility the rest of the clause has; an operator with no caller and
/// no test is the one that is wrong when a caller arrives.
#[allow(dead_code)] // No caller until a subsampled internal format is built.
pub(crate) fn t2pt(c: &mut [i32; 2]) {
    c[0] = c[0].wrapping_sub((c[1].wrapping_add(1)) >> 1);
    c[1] = c[1].wrapping_add(c[0]);
}

/// The inverse of [`t2pt`].
#[allow(dead_code)] // Exists for the round-trip property; see `t2pt`.
pub(crate) fn t2pt_inverse(c: &mut [i32; 2]) {
    c[1] = c[1].wrapping_sub(c[0]);
    c[0] = c[0].wrapping_add((c[1].wrapping_add(1)) >> 1);
}

// --- 9.9.7.1: the 4x4 inverse core transform ----------------------------

/// 9.9.7.1's `ICT4x4( )`.
///
/// The sixteen coefficients are in **raster order**: index `j` is row `j / 4`,
/// column `j % 4`. That is 9.9.4's convention and 9.9.5's, and it is stated
/// here because the index quadruples below look arbitrary otherwise — they
/// are the clause's, and every one is a 2x2 butterfly over positions that are
/// *not* adjacent in raster order.
pub(crate) fn ict4x4(c: &mut [i32; 16]) {
    inv_permute(c);
    // First stage: four 2x2 transforms over all sixteen values.
    butterfly(c, [0, 1, 4, 5], |a| t2x2h(a, 1));
    butterfly(c, [2, 3, 6, 7], inv_todd);
    butterfly(c, [8, 12, 9, 13], inv_todd);
    butterfly(c, [10, 11, 14, 15], inv_toddodd);
    // Second stage. 9.9.7.1's NOTE 2: the first stage must complete before
    // any of these begins, which is why they are separate statements rather
    // than one fused loop.
    butterfly(c, [0, 3, 12, 15], |a| t2x2h(a, 0));
    butterfly(c, [5, 6, 9, 10], |a| t2x2h(a, 0));
    butterfly(c, [1, 2, 13, 14], |a| t2x2h(a, 0));
    butterfly(c, [4, 7, 8, 11], |a| t2x2h(a, 0));
}

/// The forward core transform: [`ict4x4`]'s stages undone in reverse order.
///
/// Nothing in the decoder calls this. It exists so that
/// [`tests::the_core_transform_round_trips_bit_exactly`] can hold the claim
/// this module is built on — that 9.9.7's transform is *reversible*, not
/// approximately invertible — against something. `docs/design/jpeg-xr.md`
/// records why that is the strongest evidence available at this stage and
/// what it does not reach.
#[allow(dead_code)] // Evidence, not decode path — see the doc comment.
pub(crate) fn fwd_ict4x4(c: &mut [i32; 16]) {
    butterfly(c, [4, 7, 8, 11], |a| t2x2h_inverse(a, 0));
    butterfly(c, [1, 2, 13, 14], |a| t2x2h_inverse(a, 0));
    butterfly(c, [5, 6, 9, 10], |a| t2x2h_inverse(a, 0));
    butterfly(c, [0, 3, 12, 15], |a| t2x2h_inverse(a, 0));
    butterfly(c, [10, 11, 14, 15], fwd_toddodd);
    butterfly(c, [8, 12, 9, 13], fwd_todd);
    butterfly(c, [2, 3, 6, 7], fwd_todd);
    butterfly(c, [0, 1, 4, 5], |a| t2x2h_inverse(a, 1));
    fwd_permute(c);
}

/// Gathers four of the sixteen coefficients, applies `op`, and scatters them
/// back — the "copy into `arrayLocal[ ]`, transform, copy out" that 9.9.7.1
/// writes out longhand sixteen times.
fn butterfly(c: &mut [i32; 16], at: [usize; 4], op: impl FnOnce(&mut [i32; 4])) {
    let mut local = [c[at[0]], c[at[1]], c[at[2]], c[at[3]]];
    op(&mut local);
    for (slot, value) in at.into_iter().zip(local) {
        c[slot] = value;
    }
}

// --- 9.9.2 and 9.9.5: the two levels ------------------------------------

/// 9.9.2's `FirstLevelInverseTransform( )` for the 4:4:4 and single-component
/// internal formats.
///
/// The `2 *` for chroma when `SCALED_FLAG` is set is 9.9.2's own NOTE: an
/// RGB-to-YUV conversion on the encoder's side can widen U and V by one bit,
/// so their quantizer is halved and the factor is put back here rather than
/// carried through the transform.
pub(crate) fn first_level(dclp: &mut [i32], components: usize, scaled: bool, mb_count: usize) {
    for mb in 0..mb_count {
        for i in 0..components {
            let base = (mb * components + i) * 16;
            let Some(block) = dclp.get_mut(base..base + 16) else {
                return;
            };
            let mut local = [0i32; 16];
            local.copy_from_slice(block);
            ict4x4(&mut local);
            if i > 0 && scaled {
                for v in &mut local {
                    *v = v.wrapping_mul(2);
                }
            }
            block.copy_from_slice(&local);
        }
    }
}

/// 9.9.5's `SecondLevelInverseTransform( )`.
///
/// `ExtendedWidth` and `ExtendedHeight` are multiples of 16 by 8.3.21 and
/// 8.3.22, so the 4x4 grid divides both exactly and there is no partial block
/// at the right or bottom edge — the reason this loop has no remainder case.
pub(crate) fn second_level(plane: &mut [i32], width: usize, height: usize) {
    if width == 0 || height == 0 {
        return;
    }
    let mut y = 0;
    while y + 4 <= height {
        let mut x = 0;
        while x + 4 <= width {
            let mut local = [0i32; 16];
            for row in 0..4 {
                for col in 0..4 {
                    local[row * 4 + col] = plane[(y + row) * width + x + col];
                }
            }
            ict4x4(&mut local);
            for row in 0..4 {
                for col in 0..4 {
                    plane[(y + row) * width + x + col] = local[row * 4 + col];
                }
            }
            x += 4;
        }
        y += 4;
    }
}

#[cfg(test)]
#[path = "tests/transform.rs"]
mod tests;
