//! The photo core transform's properties.
//!
//! # Why this stage has evidence the rest of the decoder cannot have yet
//!
//! Everything else here is checked end to end, against a raster this
//! repository authored and Windows' encoder merely carried. The transform is
//! different: T.832 specifies it as a **reversible integer lifting
//! structure**, which is a claim about the operator itself, and a claim can be
//! checked without any picture at all.
//!
//! Three properties:
//!
//! 1. **Round trip.** [`fwd_ict4x4`] after [`ict4x4`] returns the original
//!    bits. The forward direction is *not* a mirror of the inverse — it is
//!    derived by reversing each of 9.9.7's lifting steps independently from
//!    the clause's own text — so this is a genuine cross-check and not a
//!    tautology. It is the property that catches the most.
//! 2. **`T2x2h( )` is an involution**, which settles a reading of 9.9.7.2's
//!    NOTE that would otherwise silently break the forward operator.
//! 3. **The DC basis function is flat.** Inverse-transforming a block whose
//!    only non-zero coefficient is the DC one must give a *constant* block —
//!    that is what makes it the DC coefficient. Narrow, but derived from what
//!    the transform *means* rather than from any code in this repository.
//!
//! # Counted injections
//!
//! Each injection is applied to the **inverse only**, and measured against the
//! real forward and the real flatness claim.
//!
//! | Injection | Flatness (of 6 probes) | Round trip (of 256 vectors) |
//! | --- | ---: | ---: |
//! | `InvTodd`'s `(3 * c + 4) >> 3` written `>> 2` | **6** | **256** |
//! | `T2x2h`'s `valRound` forced to 0 in the first stage | **0** | **130** |
//! | `InvPermArr` read as a source index, not a destination | **0** | **256** |
//! | `InvToddodd`'s `>> 2` written `>> 3` | **0** | **256** |
//! | A slip *mirrored* into the forward operator too | not probed | **0** |
//!
//! **The numbers to read twice are the zeros and the 130.**
//!
//! Flatness catches exactly one of the four. A DC-only block is a degenerate
//! input — most of the sixteen lifting positions carry zero through it — so
//! three of the four slips never touch a non-zero operand. It is kept because
//! the one it does catch is the likeliest slip in the clause (3/8 misread as
//! 3/4), and because it is the only property here derived from the
//! transform's meaning rather than from its structure. It is **not** a
//! general-purpose check and this file does not treat it as one.
//!
//! `valRound` fires on 130 of 256 rather than all of them: it shifts a result
//! by one only when a parity works out, so roughly half the random vectors
//! are unaffected. Half is plenty for a test and would be nowhere near enough
//! for a reviewer's eye on a picture.
//!
//! The last row is the blind spot, stated as a zero: a bijection composed
//! with its own inverse is the identity *whatever the bijection is*, so a
//! slip applied to both directions passes everything here. That is why the
//! lossless identity downstream — which compares pixels against a raster this
//! repository authored — is the check that actually closes this stage, and
//! why `docs/design/jpeg-xr.md` records the transform's own evidence as
//! necessary and not sufficient.

use super::*;

/// A small deterministic generator. `rand` is a dev-dependency this
/// repository does not take, and a fixed sequence is what ruling 4 wants
/// anyway: a property that fails must fail the same way on every machine.
struct Rng(u64);

impl Rng {
    /// SplitMix64's mixing function, integer-only.
    fn next(&mut self) -> i32 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        // The high half is the well-mixed one; the cast reinterprets it.
        ((z ^ (z >> 31)) >> 32) as i32
    }

    /// A coefficient of the magnitude a conformant codestream produces.
    /// Annex B's profile and level constraints bound the real dynamic range
    /// well inside this; the bound is here so the round trip runs on values a
    /// real file contains, while `transform.rs`'s wrapping arithmetic keeps a
    /// fuzzer's values from panicking.
    fn coeff(&mut self) -> i32 {
        self.next() % 100_000
    }
}

/// The DC probes, shared so the property and its injections cannot drift.
const DC_PROBES: [i32; 6] = [0, 1, -1, 7, -7, 1000];

/// The number of vectors the round trip runs on, shared for the same reason.
const VECTORS: usize = 256;

/// [`ict4x4`] with one part replaced, so an injection differs from the real
/// transform in exactly one place and nowhere else.
fn ict4x4_with(
    c: &mut [i32; 16],
    permute: impl Fn(&mut [i32; 16]),
    round1: i32,
    todd: impl Fn(&mut [i32; 4]) + Copy,
    toddodd: impl Fn(&mut [i32; 4]),
) {
    permute(c);
    butterfly(c, [0, 1, 4, 5], |a| t2x2h(a, round1));
    butterfly(c, [2, 3, 6, 7], todd);
    butterfly(c, [8, 12, 9, 13], todd);
    butterfly(c, [10, 11, 14, 15], toddodd);
    butterfly(c, [0, 3, 12, 15], |a| t2x2h(a, 0));
    butterfly(c, [5, 6, 9, 10], |a| t2x2h(a, 0));
    butterfly(c, [1, 2, 13, 14], |a| t2x2h(a, 0));
    butterfly(c, [4, 7, 8, 11], |a| t2x2h(a, 0));
}

/// How many of [`DC_PROBES`] `inverse` fails to reconstruct flat.
fn flatness_failures(inverse: impl Fn(&mut [i32; 16])) -> usize {
    DC_PROBES
        .into_iter()
        .filter(|&k| {
            let mut block = [0i32; 16];
            block[0] = 4 * k;
            inverse(&mut block);
            block != [k; 16]
        })
        .count()
}

/// How many of [`VECTORS`] random vectors fail to survive `inverse` followed
/// by the **real** forward transform.
fn round_trip_failures(inverse: impl Fn(&mut [i32; 16])) -> usize {
    let mut rng = Rng(0xABCD_EF01);
    let mut bad = 0;
    for _ in 0..VECTORS {
        let mut original = [0i32; 16];
        for v in &mut original {
            *v = rng.coeff();
        }
        let mut c = original;
        inverse(&mut c);
        fwd_ict4x4(&mut c);
        if c != original {
            bad += 1;
        }
    }
    bad
}

// --- the injected operators ----------------------------------------------

/// 9.9.7.3's `InvTodd( )` with `(3 * c + 4) >> 3` written `>> 2`.
///
/// The likeliest transcription slip in the clause: the three lifting
/// coefficients are 3/8, and 3/4 is what a reader gets by taking the 4 in the
/// numerator for the denominator.
fn todd_with_wrong_shift(c: &mut [i32; 4]) {
    c[1] = c[1].wrapping_add(c[3]);
    c[0] = c[0].wrapping_sub(c[2]);
    c[3] = c[3].wrapping_sub(c[1] >> 1);
    c[2] = c[2].wrapping_add((c[0].wrapping_add(1)) >> 1);
    c[0] = c[0].wrapping_sub((c[1].wrapping_mul(3).wrapping_add(4)) >> 2);
    c[1] = c[1].wrapping_add((c[0].wrapping_mul(3).wrapping_add(4)) >> 2);
    c[2] = c[2].wrapping_sub((c[3].wrapping_mul(3).wrapping_add(4)) >> 2);
    c[3] = c[3].wrapping_add((c[2].wrapping_mul(3).wrapping_add(4)) >> 2);
    c[2] = c[2].wrapping_sub((c[1].wrapping_add(1)) >> 1);
    c[3] = ((c[0].wrapping_add(1)) >> 1).wrapping_sub(c[3]);
    c[1] = c[1].wrapping_add(c[2]);
    c[0] = c[0].wrapping_sub(c[3]);
}

/// 9.9.7.4's `InvToddodd( )` with its middle `>> 2` written `>> 3`.
///
/// That step's shift genuinely differs from its two neighbours', which is
/// exactly the shape of asymmetry a transcriber tidies away.
fn toddodd_with_wrong_shift(c: &mut [i32; 4]) {
    c[3] = c[3].wrapping_add(c[0]);
    c[2] = c[2].wrapping_sub(c[1]);
    let t1 = c[3] >> 1;
    let t2 = c[2] >> 1;
    c[0] = c[0].wrapping_sub(t1);
    c[1] = c[1].wrapping_add(t2);
    c[0] = c[0].wrapping_sub((c[1].wrapping_mul(3).wrapping_add(3)) >> 3);
    c[1] = c[1].wrapping_add((c[0].wrapping_mul(3).wrapping_add(3)) >> 3);
    c[0] = c[0].wrapping_sub((c[1].wrapping_mul(3).wrapping_add(4)) >> 3);
    c[1] = c[1].wrapping_sub(t2);
    c[0] = c[0].wrapping_add(t1);
    c[2] = c[2].wrapping_add(c[1]);
    c[3] = c[3].wrapping_sub(c[0]);
    c[1] = c[1].wrapping_neg();
    c[2] = c[2].wrapping_neg();
}

// --- property 1: reversibility -------------------------------------------

#[test]
fn the_core_transform_round_trips_bit_exactly() {
    // 9.9.7's transform is a lifting structure: every step writes one
    // variable from a function of variables it does not write, so every step
    // is invertible and so is their composition. "Bit-exactly" is therefore
    // the *only* acceptable result — not "within one least significant bit" —
    // and it is what the decoder's determinism rests on.
    assert_eq!(round_trip_failures(ict4x4), 0);
    // And the other way round, which is a different composition.
    let mut rng = Rng(0x0BAD_C0DE);
    for _ in 0..VECTORS {
        let mut original = [0i32; 16];
        for v in &mut original {
            *v = rng.coeff();
        }
        let mut c = original;
        fwd_ict4x4(&mut c);
        ict4x4(&mut c);
        assert_eq!(c, original, "forward then inverse lost bits");
    }
}

#[test]
fn every_lifting_operator_round_trips_on_its_own() {
    // Narrower than the whole transform and worth having separately: a
    // failure here names the operator, where a failure above only says the
    // transform is wrong somewhere.
    let mut rng = Rng(0x1357_9BDF);
    for _ in 0..512 {
        let original = [rng.coeff(), rng.coeff(), rng.coeff(), rng.coeff()];
        let mut c = original;
        inv_todd(&mut c);
        fwd_todd(&mut c);
        assert_eq!(c, original, "InvTodd");
        let mut c = original;
        inv_toddodd(&mut c);
        fwd_toddodd(&mut c);
        assert_eq!(c, original, "InvToddodd");
    }
}

#[test]
fn the_two_point_transform_round_trips() {
    // 9.9.7.7's `T2pt( )` has no caller — every internal format that uses it
    // is refused by name — so this is the only thing holding it to the
    // clause. An operator with no caller and no test is the one that is wrong
    // when a caller arrives.
    let mut rng = Rng(0x2468_ACE0);
    for _ in 0..512 {
        let original = [rng.coeff(), rng.coeff()];
        let mut c = original;
        t2pt(&mut c);
        t2pt_inverse(&mut c);
        assert_eq!(c, original);
    }
}

// --- property 2: 9.9.7.2's operator --------------------------------------

#[test]
fn t2x2h_is_an_involution() {
    // 9.9.7.2's NOTE reads "the inverse of T2x2Th( ) is two successive
    // applications of T2x2Th ... with the same value of valRound". Taken
    // literally that makes the operator order three. It is not: two
    // applications are the *identity*, so the inverse is one application.
    //
    // The distinction is not academic. Building the forward transform on the
    // literal reading gives an operator that is wrong everywhere and still
    // looks principled, and the round trip above would fail with no clue why.
    let mut rng = Rng(0x5EED_1234);
    for round in [0i32, 1] {
        for _ in 0..512 {
            let original = [rng.coeff(), rng.coeff(), rng.coeff(), rng.coeff()];
            let mut twice = original;
            t2x2h(&mut twice, round);
            t2x2h(&mut twice, round);
            assert_eq!(twice, original, "T2x2h is not an involution at {round}");
            // And `t2x2h_inverse` is that fact, spelled out.
            let mut c = original;
            t2x2h(&mut c, round);
            t2x2h_inverse(&mut c, round);
            assert_eq!(c, original, "t2x2h_inverse at valRound {round}");
        }
    }
}

#[test]
fn the_literal_reading_of_9_9_7_2s_note_is_not_the_identity() {
    // The counterweight: if three applications *were* the identity, the two
    // readings would agree and none of this would matter. They do not agree,
    // which is what makes the measurement above load-bearing.
    let mut rng = Rng(0x5EED_1234);
    let mut differs = 0;
    for _ in 0..512 {
        let original = [rng.coeff(), rng.coeff(), rng.coeff(), rng.coeff()];
        let mut thrice = original;
        for _ in 0..3 {
            t2x2h(&mut thrice, 0);
        }
        if thrice != original {
            differs += 1;
        }
    }
    // Every vector but the operator's fixed points, of which a random draw
    // over this range finds none.
    assert_eq!(differs, 512);
}

// --- property 3: the DC basis function -----------------------------------

#[test]
fn a_dc_only_block_inverse_transforms_to_a_flat_block() {
    // The defining property of a DC coefficient: it carries the block's
    // constant term and nothing else, so a block with only a DC coefficient
    // reconstructs to one value repeated sixteen times.
    //
    // The gain is exactly four — two levels of a 2x2 butterfly, each halving
    // — so a DC of `4k` gives sixteen samples of `k`. Multiples of four are
    // used so the claim is exact rather than "flat to within rounding": an
    // off-by-one from a wrong shift would otherwise hide inside a tolerance.
    for k in DC_PROBES {
        let mut block = [0i32; 16];
        block[0] = 4 * k;
        ict4x4(&mut block);
        assert_eq!(block, [k; 16], "a DC of {} did not reconstruct flat", 4 * k);
    }
    assert_eq!(flatness_failures(ict4x4), 0);
}

// --- the counted injections ----------------------------------------------

#[test]
fn a_wrong_shift_in_inv_todd_is_caught_by_both_properties() {
    let broken =
        |c: &mut [i32; 16]| ict4x4_with(c, inv_permute, 1, todd_with_wrong_shift, inv_toddodd);
    assert_eq!(flatness_failures(broken), 6, "flatness");
    assert_eq!(round_trip_failures(broken), VECTORS, "round trip");
}

#[test]
fn dropping_the_first_stages_round_control_is_caught_by_half_the_vectors() {
    // 9.9.7.1 passes `valRound = 1` to exactly one of its eight butterflies
    // and 0 to the other seven. Reading `valRound` as a rounding *mode*
    // rather than as a selector between two operators makes that asymmetry
    // look like a typo to tidy away.
    //
    // Flatness sees none of it: the probes are multiples of four, and
    // `(4k + 1) >> 1` and `4k >> 1` agree. The round trip sees it on 130 of
    // 256 vectors, because the difference is a parity that lands about half
    // the time.
    let broken = |c: &mut [i32; 16]| ict4x4_with(c, inv_permute, 0, inv_todd, inv_toddodd);
    assert_eq!(flatness_failures(broken), 0, "flatness is blind to this");
    assert_eq!(round_trip_failures(broken), 130, "round trip");
}

#[test]
fn reading_the_permutation_backwards_is_caught_only_by_the_round_trip() {
    // Table 164 maps `arrayTemp[InvPermArr[i]] = arrayInput[i]`; reading it as
    // `arrayTemp[i] = arrayInput[InvPermArr[i]]` transposes the whole
    // transform. `InvPermArr[0]` is 0, so a DC-only block is unmoved by
    // either reading and flatness cannot see it at all.
    let broken = |c: &mut [i32; 16]| ict4x4_with(c, fwd_permute, 1, inv_todd, inv_toddodd);
    assert_eq!(flatness_failures(broken), 0, "flatness is blind to this");
    assert_eq!(round_trip_failures(broken), VECTORS, "round trip");
}

#[test]
fn a_wrong_shift_in_inv_toddodd_is_caught_only_by_the_round_trip() {
    let broken =
        |c: &mut [i32; 16]| ict4x4_with(c, inv_permute, 1, inv_todd, toddodd_with_wrong_shift);
    assert_eq!(flatness_failures(broken), 0, "flatness is blind to this");
    assert_eq!(round_trip_failures(broken), VECTORS, "round trip");
}

#[test]
fn a_mirrored_injection_survives_everything_here() {
    // The blind spot, stated as a zero. A bijection composed with its own
    // inverse is the identity whatever the bijection is, so a slip applied to
    // both directions passes. Nothing in this file can see it; the lossless
    // identity downstream can, because it compares pixels against a raster
    // this repository authored rather than against this file's own algebra.
    fn broken_inverse(c: &mut [i32; 4]) {
        c[0] = c[0].wrapping_add(c[3]);
        c[1] = c[1].wrapping_sub(c[2]);
    }
    fn broken_forward(c: &mut [i32; 4]) {
        c[1] = c[1].wrapping_add(c[2]);
        c[0] = c[0].wrapping_sub(c[3]);
    }
    let mut rng = Rng(0xFEED_FACE);
    let mut caught = 0;
    for _ in 0..VECTORS {
        let original = [rng.coeff(), rng.coeff(), rng.coeff(), rng.coeff()];
        let mut c = original;
        broken_inverse(&mut c);
        broken_forward(&mut c);
        if c != original {
            caught += 1;
        }
    }
    assert_eq!(
        caught, 0,
        "a mirrored pair must round-trip; if this fires the module header is stale"
    );
}

// --- the two levels ------------------------------------------------------

#[test]
fn the_second_level_transforms_every_block_and_leaves_no_gap() {
    // 8.3.21 and 8.3.22 make the extended dimensions multiples of 16, so the
    // 4x4 grid divides them exactly. This pins that the loop covers the whole
    // plane: a plane of DC-only blocks must come back flat everywhere,
    // including the last row and column, which an off-by-one bound would
    // leave untouched — and untouched means zero, a black edge.
    let (w, h) = (32usize, 16usize);
    let mut plane = vec![0i32; w * h];
    for by in (0..h).step_by(4) {
        for bx in (0..w).step_by(4) {
            plane[by * w + bx] = 4 * 9;
        }
    }
    second_level(&mut plane, w, h);
    assert!(
        plane.iter().all(|&v| v == 9),
        "a plane of DC-only blocks did not reconstruct flat everywhere"
    );
}

#[test]
fn the_first_level_doubles_chroma_only_when_scaled() {
    // 9.9.2's NOTE: the factor of two puts back the bit an RGB-to-YUV
    // conversion can add to U and V, whose quantizer was halved to pay for
    // it. It applies to chroma and never to luma, and only when SCALED_FLAG
    // is set — three conditions, so three ways to get it wrong.
    let mut with_scale = vec![0i32; 3 * 16];
    let mut without = vec![0i32; 3 * 16];
    for i in 0..3 {
        with_scale[i * 16] = 4 * 5;
        without[i * 16] = 4 * 5;
    }
    first_level(&mut with_scale, 3, true, 1);
    first_level(&mut without, 3, false, 1);
    assert_eq!(&with_scale[0..16], &[5i32; 16], "luma is never doubled");
    assert_eq!(&with_scale[16..32], &[10i32; 16], "chroma is doubled");
    assert_eq!(&with_scale[32..48], &[10i32; 16], "and so is the second");
    assert_eq!(&without[16..32], &[5i32; 16], "not without SCALED_FLAG");
}
