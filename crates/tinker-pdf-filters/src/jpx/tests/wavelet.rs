//! Gate 2: the fixed-point 9/7 against an `f64` reference of F.3.8.2's own
//! lifting steps, and Table F.4's constants recomputed rather than restated.
//!
//! # Why an `f64` reference and not just the oracle
//!
//! Gap 18 pre-argued the obvious oracle away: a fixed-point 9/7 differs from
//! every float-based reference decoder, OpenJPEG included, so a disagreement
//! with `opj_decompress` cannot by itself tell a defect from the arithmetic
//! doing what it was designed to do. The reference below runs F.3.8.2's six
//! steps in `f64` over **the same dequantised coefficients**, walking the same
//! ladder through [`super::super::wavelet::ladder`], so the *only* difference
//! between the two sides is the arithmetic — which is what makes a tolerance
//! of one level of 255 a real gate rather than a negotiation.
//!
//! It also catches what an oracle cannot cheaply catch: OpenJPEG's own
//! irreversible path is `f32` and it rounds the final sample half-to-even,
//! where this build rounds half-up (the plan's choice, because half-up on an
//! arithmetic shift is identical on every target and `lrintf` is not). So the
//! oracle comparison carries a systematic one-level residual wherever a
//! reconstruction lands exactly on a tie, and a gate at *zero* against
//! OpenJPEG would be a gate against its rounding rather than against this
//! decoder.
//!
//! The reference is test-only and must stay so: shipping it would put a float
//! on a pixel path, which is what `cargo run -p xtask -- libm` exists to
//! stop.

use crate::jpx::codestream::Roi;
use crate::jpx::wavelet::{
    into_samples, ladder, level_shift, maxshift, mul_q24, synthesise_97, Arith, Fixed, Realigned,
    ALPHA, BETA, DELTA, GAMMA, K, MAX_PRODUCT, PLANE_BOUND, Q, QC, TWO_OVER_K,
};
use crate::jpx::{boxes, codestream, tier1, tier2};

// --- T.800 Table F.4, as decimals -----------------------------------------
//
// Transcribed from the standard, and used two ways: the reference arithmetic
// lifts with them, and `table_f4_constants_are_recomputed_not_restated`
// derives the shipped integers from them. Restating the integers here would
// let a transcription slip in the constant agree with a transcription slip in
// the test, which is the one thing this test exists to prevent.
//
// The signs are Table F.4's: alpha and beta negative, gamma and delta
// positive. The shipped constants carry magnitudes and write the sign into
// the lifting step, so the recomputation below takes absolute values.
const F4_ALPHA: f64 = -1.586134342059924;
const F4_BETA: f64 = -0.052980118572961;
const F4_GAMMA: f64 = 0.882911075530934;
const F4_DELTA: f64 = 0.443506852043971;
const F4_K: f64 = 1.230174104914001;

/// The `f64` reference arithmetic: F.3.8.2 as the standard writes it.
///
/// `pub(super)` because `tests::colour` instantiates the same ladder through
/// it to put the fixed-point **ICT** against the same reference — G.2.2 is a
/// float transform in the standard exactly as F.3.8.2 is, so it gets the same
/// gate rather than a weaker one of its own.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(super) struct Reference(pub(super) f64);

impl Arith for Reference {
    fn dyadic(m: i64, e: i32) -> Self {
        // `m` is at most 2^44 and so is exact in an `f64`; the scale is a
        // power of two. The reference therefore starts from the *exact*
        // dequantised coefficient, and every difference below is the
        // fixed-point path's.
        Reference(m as f64 * 2.0f64.powi(e))
    }

    fn synthesise(line: &mut [Self], i0: u32) {
        let mut y: Vec<f64> = line.iter().map(|v| v.0).collect();
        reference_97(&mut y, i0);
        for (dst, src) in line.iter_mut().zip(y) {
            *dst = Reference(src);
        }
    }

    fn sample(self) -> i32 {
        // Round-half-up, matching `Fixed::sample`'s `(v + (1 << 11)) >> 12`.
        // `f64::round` is half-away-from-zero and would disagree on negative
        // ties, which is precisely the kind of difference this gate must not
        // be measuring.
        (self.0 + 0.5).floor() as i32
    }

    fn inverse_mct(y0: Self, y1: Self, y2: Self) -> (Self, Self, Self) {
        // G.2.2 in `f64`, from the standard's decimals rather than from the
        // shipped Q24 integers. The shipped path rounds four products to Q12;
        // this one does not round at all, so the difference between them is
        // the fixed point's and nothing else.
        use super::colour::{G2_B_CB, G2_G_CB, G2_G_CR, G2_R_CR};
        (
            Reference(y0.0 + G2_R_CR * y2.0),
            Reference(y0.0 - G2_G_CB * y1.0 - G2_G_CR * y2.0),
            Reference(y0.0 + G2_B_CB * y1.0),
        )
    }
}

/// F.3.8.2's six steps, in `f64`, written from the equations rather than
/// from the shipped code.
fn reference_97(y: &mut [f64], i0: u32) {
    let n = y.len();
    if n == 0 {
        return;
    }
    if n == 1 {
        if i0 % 2 == 1 {
            y[0] /= 2.0;
        }
        return;
    }
    let odd_first = i0 % 2 == 1;
    let is_even = |k: usize| (k % 2 == 0) != odd_first;

    // F-8 and F-9: the low band by K, the high band by 2/K. Derived from K
    // rather than transcribed, so the reference states the relationship the
    // shipped `TWO_OVER_K` only stores the answer to.
    for (k, v) in y.iter_mut().enumerate() {
        *v *= if is_even(k) { F4_K } else { 2.0 / F4_K };
    }
    // F-10 to F-13, all four subtracting, with Table F.4's signed values.
    reference_lift(y, odd_first, true, -F4_DELTA);
    reference_lift(y, odd_first, false, -F4_GAMMA);
    reference_lift(y, odd_first, true, -F4_BETA);
    reference_lift(y, odd_first, false, -F4_ALPHA);
}

fn reference_lift(y: &mut [f64], odd_first: bool, target_even: bool, c: f64) {
    let n = y.len() as i64;
    let mirror = |mut i: i64| -> usize {
        while i < 0 || i >= n {
            if i < 0 {
                i = -i;
            }
            if i >= n {
                i = 2 * (n - 1) - i;
            }
        }
        i as usize
    };
    for k in 0..y.len() {
        if ((k % 2 == 0) != odd_first) != target_even {
            continue;
        }
        let i = k as i64;
        y[k] += c * (y[mirror(i - 1)] + y[mirror(i + 1)]);
    }
}

// --- the constants --------------------------------------------------------

/// Every shipped constant equals `round(c * 2^24)` recomputed from Table
/// F.4's decimals, with a residual under 2^-25.
///
/// The residual bound is not decoration. The plan's error analysis assumes it
/// when it concludes that constant rounding costs 0.03 sample units for an
/// 8-bit image against the 1.9 it would cost at Q16, and a constant that
/// missed it would break the argument for Q24 rather than merely be slightly
/// off.
#[test]
fn table_f4_constants_are_recomputed_not_restated() {
    let scale = 2.0f64.powi(QC as i32);
    let cases: &[(&str, f64, i64)] = &[
        ("alpha", F4_ALPHA.abs(), ALPHA),
        ("beta", F4_BETA.abs(), BETA),
        ("gamma", F4_GAMMA.abs(), GAMMA),
        ("delta", F4_DELTA.abs(), DELTA),
        ("K", F4_K, K),
        ("2/K", 2.0 / F4_K, TWO_OVER_K),
    ];
    for &(name, decimal, shipped) in cases {
        let recomputed = (decimal * scale).round() as i64;
        assert_eq!(
            recomputed, shipped,
            "{name}: Table F.4's decimal rounds to {recomputed} at Q{QC}, not {shipped}"
        );
        let residual = (shipped as f64 / scale - decimal).abs();
        assert!(
            residual < 2.0f64.powi(-25),
            "{name}: residual {residual:e} is past the 2^-25 the plan's error \
             analysis assumes, so Q{QC} no longer buys what it was chosen for"
        );
    }
    assert_eq!(
        Q, 12,
        "the plane format is Q12 by decision, not by accident"
    );
}

/// The Q36-to-Q12 rounding is round-half-up, not a truncating shift.
///
/// This test exists because of what an injection found: replacing
/// `(p + (1 << 23)) >> 24` with a bare `p >> 24` was caught by **nothing in
/// the repository** — not gate 2 against the `f64` reference, not gate 3
/// against `opj_decompress`, not one of the two hundred unit tests. That is
/// the two gates behaving correctly rather than failing: the error is under
/// 2^-12 of a sample unit per multiply, which is inside what the plan's own
/// error analysis bounds and far inside a level of 255.
///
/// It is still a defect, and the reason is that it is a **bias** rather than
/// noise. An arithmetic right shift floors, so every one of the sixty
/// multiplies on the longest path moves the same way, and a systematic
/// downward drift is exactly the kind of thing that shows up as a black point
/// creeping on a 16-bit medical image and nowhere on an 8-bit test fixture.
/// "Round to nearest" has three meanings and only one of them is a
/// specification, so the specification is pinned here directly rather than
/// hoped for through an image.
#[test]
fn the_q24_product_rounds_half_up_rather_than_truncating() {
    let half = 1i64 << (QC - 1);
    // Exactly +0.5 of a Q12 unit goes up to 1; a truncating shift gives 0.
    assert_eq!(mul_q24(1, half), 1, "+0.5 must round up");
    // Exactly -0.5 goes up to 0; a truncating shift gives -1. This asymmetry
    // is what makes it half-*up* rather than half-away-from-zero, and it is
    // what an arithmetic shift gives for free on every target — where
    // `lrintf` and `f64::round` each give something else.
    assert_eq!(
        mul_q24(-1, half),
        0,
        "-0.5 must round up, not away from zero"
    );
    assert_eq!(mul_q24(1, half - 1), 0, "just under a half stays down");
    assert_eq!(mul_q24(1, half + 1), 1, "just over a half goes up");
    // A whole Q12 unit times K is K, which says the scale is Q24 and not
    // something a factor of two away from it.
    assert_eq!(mul_q24(1 << QC, K), K);
}

/// The `i64` product bound, exercised at the clamped worst case rather than
/// asserted about it.
///
/// A plane entry is bounded by `PLANE_BOUND` because the dequantiser clamps
/// it there, and every figure in the overflow proof follows from that one
/// number. This drives a line of alternating extremes — the shape that
/// maximises every lifting step at once — through the shipped synthesis, so
/// the `debug_assert` inside `mul_q24` is the thing under test.
#[test]
fn the_lifting_stays_inside_the_i64_product_bound() {
    for i0 in 0..2u32 {
        for n in [2usize, 3, 5, 8, 33, 64] {
            let mut line: Vec<i64> = (0..n)
                .map(|k| {
                    if k % 2 == 0 {
                        PLANE_BOUND
                    } else {
                        -PLANE_BOUND
                    }
                })
                .collect();
            synthesise_97(&mut line, i0);
            // The plan's growth figure: 24.08 across the four lifting steps
            // on top of 1.6258 from the scaling, which is 2^35.30 from 2^30.
            for v in line {
                assert!(
                    v.unsigned_abs() < 1 << 36,
                    "a clamped line grew to {v}, past the 2^35.30 the overflow \
                     proof bounds it by"
                );
            }
        }
    }
    assert_eq!(MAX_PRODUCT, 1 << 60, "the asserted product bound moved");
}

// --- gate 2 ---------------------------------------------------------------

fn fixture(name: &str) -> Vec<u8> {
    let path =
        std::path::PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/jpx")).join(name);
    std::fs::read(&path).unwrap_or_else(|e| panic!("{path:?}: {e}"))
}

/// Gate 2: no sample differs from the `f64` reference by more than one level
/// of 255, and at most 1 per cent differ at all.
///
/// Both budgets are an order of magnitude inside what the fixed-point
/// analysis bounds — 0.09 sample units for an 8-bit image, an eleventh of a
/// level — so a failure here is evidence of a defect and not of a tolerance
/// wanting renegotiation.
#[test]
fn the_fixed_point_97_tracks_an_f64_reference_of_f382() {
    let cases: &[&str] = &[
        "r1-i2", "r1-i3", "r2-i2", "r2-i3", "r3-i2", "r3-i3", "r4-i1", "r5-i1", "r1-q20", "r2-q20",
    ];

    let mut compared = 0;
    for &name in cases {
        let bytes = fixture(&format!("{name}.jp2"));
        let container = boxes::parse(&bytes).expect("the fixture is a JP2");
        let stream = codestream::parse(container.codestream).expect("the codestream parses");
        assert!(
            !stream.cod_for(0).style.reversible,
            "{name} is supposed to be an irreversible 9/7 fixture"
        );
        let mut tiles = tier2::decode_tiles(&stream).expect("tier-2 runs");
        tier1::decode_tiles(&stream, &mut tiles).expect("tier-1 runs");
        let precision = stream.siz.components[0].precision;
        let signed = stream.siz.components[0].signed;

        let (mut total, mut differing, mut worst) = (0usize, 0usize, 0i32);
        for tile in &tiles {
            let mut clamped = false;
            let mut fixed =
                into_samples(ladder::<Fixed>(&stream, tile, 0, &mut clamped).expect("fixed"));
            let mut reference = into_samples(
                ladder::<Reference>(&stream, tile, 0, &mut clamped).expect("reference"),
            );
            assert!(!clamped, "{name} should not reach E.1's clamp");
            level_shift(&mut fixed, precision, signed);
            level_shift(&mut reference, precision, signed);

            for (&got, &want) in fixed.samples.iter().zip(&reference.samples) {
                total += 1;
                let moved = (got - want).abs();
                worst = worst.max(moved);
                if moved != 0 {
                    differing += 1;
                }
            }
        }

        assert!(
            worst <= 1,
            "{name}: a sample moved {worst} levels from the f64 reference, \
             where the fixed-point analysis bounds the whole path at an \
             eleventh of one"
        );
        let fraction = differing as f64 / total as f64;
        assert!(
            fraction <= 0.01,
            "{name}: {differing} of {total} samples differ from the f64 \
             reference ({fraction:.4}), past the 1 per cent budget"
        );
        compared += 1;
    }

    assert!(
        compared >= 10,
        "only {compared} fixtures were compared; a gate that silently stops \
         comparing is worse than no gate"
    );
}

/// The two scaling constants are not interchangeable, and the failure they
/// produce is the one the plan warns reads as a colour bug.
///
/// F.3.8.2 scales the low band by `K` and the high band by `2/K`. Swapping
/// them leaves the structure of the picture intact and moves its contrast,
/// which is why this is pinned by arithmetic here rather than left to
/// inspection: `2/K` over `K` is 1.3216, so a swap is a 32 per cent error on
/// every low-band sample and the image still looks like an image.
#[test]
fn the_scaling_constants_are_k_and_two_over_k() {
    // 2/K is the larger of the two: JPEG 2000 normalises the analysis
    // highpass to Nyquist gain 2 and the factor of two rides on the high
    // band's scaling.
    assert_eq!(
        TWO_OVER_K.max(K),
        TWO_OVER_K,
        "the high band's scaling constant must be the larger one"
    );
    // K * (2/K) is 2 exactly, which 1/K and K would make 1.
    let product =
        (K as f64 / 2.0f64.powi(QC as i32)) * (TWO_OVER_K as f64 / 2.0f64.powi(QC as i32));
    assert!(
        (product - 2.0).abs() < 1e-7,
        "K * 2/K is {product}, not the 2 that says the factor of two is on \
         the high band"
    );
}

// --- T.800 H.1's three branches, worked by hand ---------------------------

/// **H.1 step by step, on cases derived from the clause rather than from a
/// decode.**
///
/// This is a *transcription pin*, not an adjudication, and the difference is
/// worth stating: T.800 publishes no ROI test data at all — Annex H is prose
/// and seven equations, `0xFF5E` appears only in Tables A.2 and A.24, and no
/// codestream the standard prints carries an RGN. What adjudicates the
/// Maxshift decode against the standard is `tests/jpx_annex_h.rs`, which runs
/// H.1 over the coefficients T.800 J.10.4 publishes and demands the samples
/// J.10.5 publishes. What this test adds is reach: the branch boundaries, and
/// the one case — (H-1)'s mask, which needs `s > Mb` — that no codestream in
/// this repository produces and no fixture therefore covers.
///
/// Every case below is `(magnitude, half, align, s, Mb)` with the branch and
/// the arithmetic named.
#[test]
fn h1s_three_branches_land_where_the_clause_puts_them() {
    let at =
        |magnitude, half, align, shift, mb| maxshift(magnitude, half, align, Roi { shift }, mb);

    // **Step 2**, `Nb(u, v) < Mb`: "no modification takes place". That is
    // `align + half > 0` — every decoded bit is of weight 2^1 or more, so
    // there is no Maxshift headroom under the coefficient. A truncated
    // stream, nothing to do with an ROI, and the shift must not touch it.
    assert_eq!(
        at(5, 0, 2, 7, 9),
        Realigned {
            magnitude: 5,
            half: 0,
            exponent: 2
        },
        "step 2 leaves the coefficient, its lowest plane and its alignment"
    );
    // The boundary is `align + half`, not `align`: a coefficient whose own
    // passes stopped one plane early is still step 2's.
    assert_eq!(
        at(5, 1, 0, 7, 9),
        Realigned {
            magnitude: 5,
            half: 1,
            exponent: 0
        },
        "half is part of Nb(u, v) and so part of step 2's test"
    );

    // **Step 3**, the ROI branch: `Nb(u, v) >= Mb` and at least one of the
    // first Mb MSBs is non-zero, so `Nb(u, v) = Mb`. The first Mb MSBs are
    // the bits of weight 2^0 and above, so the test is `floor(|q|) != 0` and
    // the action is that floor. With `align = -3` the value is `m / 8`:
    // 26/8 -> 3, landing on 2^0 with its interval one whole unit wide.
    assert_eq!(
        at(26, 0, -3, 3, 6),
        Realigned {
            magnitude: 3,
            half: 0,
            exponent: 0
        },
        "step 3 truncates to the integer part and sets Nb(u, v) = Mb"
    );
    // The boundary: 8/8 is exactly 1, which has a bit among the first Mb
    // MSBs, and 7/8 does not.
    assert_eq!(at(8, 0, -3, 3, 6).magnitude, 1, "8/8 = 1 is step 3's");
    assert_eq!(
        at(7, 0, -3, 3, 6),
        Realigned {
            magnitude: 7,
            half: 0,
            exponent: 0
        },
        "7/8 < 1 is step 4's, and x 2^3 puts it back on 2^0"
    );
    // Step 3 is a truncation and **not** a shift by s: the two coincide only
    // when `s` happens to equal the headroom. Here the headroom is 3 and the
    // shift is 5, and the answer is the headroom's.
    assert_eq!(at(26, 0, -3, 5, 6).magnitude, 3, "step 3 never reads s");

    // **Step 4**, the background branch: all of the first Mb MSBs are zero,
    // so H-1 shifts the rest s places and H-2 sets
    // `Nb(u, v) = max(0, Nb(u, v) - s)`. That is a multiplication by 2^s,
    // and the lowest decoded plane rides with it.
    assert_eq!(
        at(5, 0, -3, 3, 7),
        Realigned {
            magnitude: 5,
            half: 0,
            exponent: 0
        },
        "s equal to the headroom puts a background coefficient back on 2^0"
    );
    assert_eq!(
        at(5, 0, -3, 4, 7),
        Realigned {
            magnitude: 5,
            half: 0,
            exponent: 1
        },
        "s and the headroom are independent: a truncated Maxshift stream has \
         fewer planes than the shift it declares, and H-1 shifts by s anyway"
    );
    assert_eq!(
        at(5, 2, -3, 3, 7).half,
        2,
        "step 4 keeps the coefficient's own lowest plane, which E.1.1.2's \
         reconstruction offset is half of"
    );

    // **(H-1) discards, and only a malformed s makes it bite.** H-1 is
    // `MSBi <- MSB(i+s)`, so the s most significant positions leave the
    // coefficient. In step 4's branch the first Mb of them are known to be
    // zero, so while `s <= Mb` nothing is lost. With `Mb = 3`, `align = -6`
    // and `s = 6`, positions 4, 5 and 6 are discarded although they are set:
    // bit b of the magnitude carries E-1 index `Mb - align - b`, so the bits
    // that survive are `b < Mb - align - s = 3`.
    //
    // 26 is `0b11010`, whose bits 1, 3 and 4 are set; masking to `b < 3`
    // leaves bit 1 alone, which is 2.
    assert_eq!(
        at(26, 0, -6, 6, 3),
        Realigned {
            magnitude: 2,
            half: 0,
            exponent: 0
        },
        "H-1 keeps the MSB positions above s and discards the rest"
    );
    // And when the mask takes everything, the coefficient is zero rather
    // than an arbitrary remainder.
    assert_eq!(at(26, 0, -6, 9, 0).magnitude, 0, "every position discarded");

    // Ruling 1: `SPrgn` is one attacker-controlled byte and Table A.26 lets
    // it be 255. Nothing here may shift past what an `i64` holds, index a
    // shift by a negative amount, or return a negative magnitude.
    for shift in [0u8, 1, 31, 32, 63, 64, 200, 255] {
        for align in [-80i32, -37, -1, 0, 1, 37] {
            let r = maxshift(u32::MAX, 31, align, Roi { shift }, 37);
            assert!(r.magnitude >= 0, "shift {shift}, align {align}");
        }
    }
}
