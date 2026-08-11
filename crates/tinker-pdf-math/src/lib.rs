//! Transcendental functions that give the same bits on every target.
//!
//! Ruling 4 says rendering is deterministic: the same document at the same
//! scale produces byte-identical output on Linux, Windows, macOS and wasm.
//! The rasteriser's fixed-point coverage accumulation was built for that, and
//! then the platform's `libm` undid it — `sin`, `cos`, `atan2`, `ln`, `log10`
//! and `powf` are *not* correctly rounded, and glibc, musl, the MSVC runtime,
//! Apple's libm and the wasm shim disagree in the last unit in the last place.
//! One such disagreement in a round join or a tint transform moves a coverage
//! sample, which moves a pixel, which breaks the golden.
//!
//! What is safe to keep from the platform is anything IEEE 754 requires to be
//! correctly rounded: the four arithmetic operations, `sqrt`, and the
//! rounding family. Those are identical everywhere by specification, so this
//! crate does not reimplement them and code on a pixel path may use them
//! freely. Everything here is built out of nothing else. That is what makes
//! it deterministic — not the algorithms, which are ordinary range reduction
//! and polynomial evaluation, but the fact that every step is an operation
//! the standard pins exactly.
//!
//! The crate is `no_std` for that reason rather than for portability: without
//! `std` linked, `x.sin()` does not compile inside these walls, so the rule
//! is enforced by the compiler instead of by remembering it.
//!
//! One habit still has to be kept by hand: **no `mul_add`**. It may compile to
//! a fused multiply-add on one target and a multiply-then-add on another, and
//! those differ. The extra precision would be welcome; the divergence is not.
//!
//! Accuracy, measured against the platform across the ranges the tests
//! sweep: two units in the last place for `sin`, `cos`, `ln`, `atan` and
//! `cbrt`, three for `atan2`, four for `exp`, and eight for `pow` — which
//! compounds `exp` and `ln`, as its definition requires. That is worse than a
//! good libm and far below anything that survives quantisation to eight bits
//! per channel. Being identical everywhere is the requirement; being the
//! closest possible double is not.

#![forbid(unsafe_code)]
#![warn(missing_docs)]
#![no_std]

#[cfg(test)]
extern crate std;

/// π/2, split so that the product with the quadrant count is exact.
///
/// Argument reduction subtracts a multiple of π/2, and the error in that
/// subtraction is the error in the result. The trick is that `PIO2_HI` ends
/// in twenty-two zero bits: a quadrant count below 2^22 multiplied by it
/// still fits in a double's fifty-three bits, so `n · PIO2_HI` is *exact* and
/// the subtraction from `x` loses nothing. Taking the nearest double to π/2
/// instead — the obvious thing, and what this did first — leaves half an ulp
/// of the product as error, which at twenty radians is seven ulps in the
/// answer.
///
/// Written from its bits rather than as a decimal because the trailing zeros
/// are the entire point and a literal cannot promise them.
///
/// (Cody and Waite's method. Beyond about 2^22 radians the product stops
/// being exact and accuracy degrades — still deterministically, since it is
/// all still plain arithmetic — and nothing rotates a page by four million
/// radians.)
const PIO2_HI: f64 = f64::from_bits(0x3FF9_21FB_5440_0000);
const PIO2_LO: f64 = 6.077_100_506_506_192e-11;

/// ln 2, split the same way so `exp`'s reduction stays exact.
const LN2_HI: f64 = 0.693_147_180_369_123_8;
const LN2_LO: f64 = 1.908_214_929_270_587_7e-10;

/// The constants `core` already has exactly. Taking them from there rather
/// than writing decimals removes a class of mistake that would be invisible:
/// a mistyped π/4 changes only which branch a few arguments take.
use core::f64::consts::{
    FRAC_1_SQRT_2, FRAC_2_PI, FRAC_PI_2, FRAC_PI_4, FRAC_PI_6, LOG10_E, LOG2_E, PI,
};

/// tan(π/12) and √3, for the arctangent's second reduction. `core` has
/// neither.
const TAN_PI_12: f64 = 0.267_949_192_431_122_7;
const SQRT_3: f64 = 1.732_050_807_568_877_2;
/// 2^52, the magnitude above which a double has no fractional bits.
const TWO_52: f64 = 4_503_599_627_370_496.0;

/// The sine of `x` radians.
#[must_use]
pub fn sin(x: f64) -> f64 {
    if !x.is_finite() {
        return f64::NAN;
    }
    let (r, quadrant) = reduce(x);
    match quadrant & 3 {
        0 => sin_kernel(r),
        1 => cos_kernel(r),
        2 => -sin_kernel(r),
        _ => -cos_kernel(r),
    }
}

/// The cosine of `x` radians.
#[must_use]
pub fn cos(x: f64) -> f64 {
    if !x.is_finite() {
        return f64::NAN;
    }
    let (r, quadrant) = reduce(x);
    match quadrant & 3 {
        0 => cos_kernel(r),
        1 => -sin_kernel(r),
        2 => -cos_kernel(r),
        _ => sin_kernel(r),
    }
}

/// The tangent of `x` radians.
#[must_use]
pub fn tan(x: f64) -> f64 {
    let s = sin(x);
    let c = cos(x);
    if c == 0.0 {
        return if s < 0.0 {
            f64::NEG_INFINITY
        } else {
            f64::INFINITY
        };
    }
    s / c
}

/// Removes whole quadrants, returning the remainder in about `[-π/4, π/4]`
/// and how many came off.
fn reduce(x: f64) -> (f64, i64) {
    // Small arguments need no reduction, and taking the general path for them
    // would only add rounding.
    if abs(x) < FRAC_PI_4 {
        return (x, 0);
    }

    let quadrant = round_to_int(x * FRAC_2_PI);
    let n = quadrant as f64;

    // `n · PIO2_HI` is exact, so the first subtraction is too; the second
    // removes the rest of π/2 and is the only step that rounds at all.
    let r = (x - n * PIO2_HI) - n * PIO2_LO;
    (r, quadrant)
}

/// sin on `[-π/4, π/4]`. The coefficients are fdlibm's minimax fit, which is
/// where every correct implementation of this gets them.
///
/// Written to fdlibm's full precision rather than to the shortest decimal
/// that round-trips: the digits are how anyone checks these against the
/// source, and a trimmed constant is one nobody can verify by eye.
#[allow(clippy::excessive_precision)]
fn sin_kernel(r: f64) -> f64 {
    let z = r * r;
    let p = -1.666_666_666_666_663_2e-1
        + z * (8.333_333_333_322_489_5e-3
            + z * (-1.984_126_982_985_795e-4
                + z * (2.755_731_370_707_006_8e-6
                    + z * (-2.505_076_025_340_686_3e-8 + z * 1.589_690_995_211_55e-10))));
    // r + r³·P(z), so the leading term keeps all of its precision.
    r + r * z * p
}

/// cos on `[-π/4, π/4]`. fdlibm's coefficients, at its precision, for the
/// reason given on [`sin_kernel`].
#[allow(clippy::excessive_precision)]
fn cos_kernel(r: f64) -> f64 {
    let z = r * r;
    let p = 4.166_666_666_666_660_2e-2
        + z * (-1.388_888_888_887_411e-3
            + z * (2.480_158_728_947_673e-5
                + z * (-2.755_731_435_139_066_4e-7
                    + z * (2.087_572_321_298_175e-9 - z * 1.135_964_755_778_819_5e-11))));
    // 1 − z/2 + z²·P(z), grouped so the small correction is rounded once
    // before it meets the one.
    let half = 0.5 * z;
    1.0 - (half - z * z * p)
}

/// The arctangent of `x`, in radians, in `[-π/2, π/2]`.
#[must_use]
pub fn atan(x: f64) -> f64 {
    if x.is_nan() {
        return f64::NAN;
    }
    if x.is_infinite() {
        return if x > 0.0 { FRAC_PI_2 } else { -FRAC_PI_2 };
    }

    let negative = x < 0.0;
    let magnitude = abs(x);

    // Two reductions bring any magnitude down to tan(π/12), where the series
    // converges in a handful of terms: reciprocate above 1, then rotate by
    // π/6 using atan t = π/6 + atan((t√3 − 1)/(t + √3)).
    let (mut t, reciprocated) = if magnitude > 1.0 {
        (1.0 / magnitude, true)
    } else {
        (magnitude, false)
    };

    let rotated = t > TAN_PI_12;
    if rotated {
        t = (t * SQRT_3 - 1.0) / (t + SQRT_3);
    }

    let mut result = atan_series(t);
    if rotated {
        result += FRAC_PI_6;
    }
    if reciprocated {
        result = FRAC_PI_2 - result;
    }
    if negative {
        -result
    } else {
        result
    }
}

/// The Taylor series for arctangent, valid where `|t| ≤ tan(π/12)`.
///
/// Plain reciprocals rather than a minimax fit: after two reductions `t²` is
/// at most 0.072, so exact coefficients reach the last bit on their own and
/// cannot be mistyped into something subtly wrong.
///
/// The term count is not decoration. Each term multiplies by `z ≤ 0.072`, so
/// stopping at `z⁸` leaves about 5 × 10⁻¹⁴ — which is exactly the error this
/// first shipped with, eleven ulps at the top of the range. Going to `z¹⁶`
/// puts the first omitted term below the last bit of the result.
/// `−1/3, 1/5, −1/7 …`, the coefficients of `atan(t)/t` in `z = t²` after the
/// leading one.
///
/// A table and a loop rather than sixteen nested parentheses: written out, the
/// expression is deep enough that rustfmt does not terminate on it. Horner
/// from the last coefficient backwards performs exactly the multiplications
/// and additions the nested form would, in the same order, so the two agree
/// bit for bit.
const ATAN_COEFFICIENTS: [f64; 16] = [
    -1.0 / 3.0,
    1.0 / 5.0,
    -1.0 / 7.0,
    1.0 / 9.0,
    -1.0 / 11.0,
    1.0 / 13.0,
    -1.0 / 15.0,
    1.0 / 17.0,
    -1.0 / 19.0,
    1.0 / 21.0,
    -1.0 / 23.0,
    1.0 / 25.0,
    -1.0 / 27.0,
    1.0 / 29.0,
    -1.0 / 31.0,
    1.0 / 33.0,
];

fn atan_series(t: f64) -> f64 {
    let z = t * t;
    let mut p = 0.0;
    for &coefficient in ATAN_COEFFICIENTS.iter().rev() {
        p = coefficient + z * p;
    }
    t + t * z * p
}

/// The angle of the point `(x, y)` from the positive x axis, in `[-π, π]`.
#[must_use]
pub fn atan2(y: f64, x: f64) -> f64 {
    if x.is_nan() || y.is_nan() {
        return f64::NAN;
    }

    if x == 0.0 && y == 0.0 {
        // The signs of the zeros decide the answer, which is what a caller
        // arriving here with a degenerate vector is relying on.
        return match (is_negative(x), is_negative(y)) {
            (true, true) => -PI,
            (true, false) => PI,
            (false, true) => -0.0,
            (false, false) => 0.0,
        };
    }
    if x == 0.0 {
        return if y > 0.0 { FRAC_PI_2 } else { -FRAC_PI_2 };
    }
    if y == 0.0 {
        return if x > 0.0 {
            if is_negative(y) {
                -0.0
            } else {
                0.0
            }
        } else if is_negative(y) {
            -PI
        } else {
            PI
        };
    }

    let base = atan(y / x);
    if x > 0.0 {
        base
    } else if is_negative(y) {
        base - PI
    } else {
        base + PI
    }
}

/// The natural logarithm of `x`.
#[must_use]
#[allow(clippy::excessive_precision)]
pub fn ln(x: f64) -> f64 {
    if x.is_nan() || x < 0.0 {
        return f64::NAN;
    }
    if x == 0.0 {
        return f64::NEG_INFINITY;
    }
    if x.is_infinite() {
        return f64::INFINITY;
    }

    // Split off the exponent, then shift the mantissa from [0.5, 1) into
    // [1/√2, √2) so the series argument stays small on both sides of one.
    let (mut mantissa, mut exponent) = frexp(x);
    if mantissa < FRAC_1_SQRT_2 {
        mantissa *= 2.0;
        exponent -= 1;
    }

    // ln m = 2·atanh(s) with s = (m−1)/(m+1), which for m near one is a
    // fast series in s². The coefficients are fdlibm's, and are 2/3, 2/5,
    // 2/7 … with the tail adjusted.
    let s = (mantissa - 1.0) / (mantissa + 1.0);
    let z = s * s;
    let p = 6.666_666_666_666_735_1e-1
        + z * (3.999_999_999_940_942e-1
            + z * (2.857_142_874_366_239e-1
                + z * (2.222_219_843_214_978_4e-1
                    + z * (1.818_357_216_161_805e-1
                        + z * (1.531_383_769_920_937_3e-1 + z * 1.479_819_860_511_658_6e-1)))));
    // 2s + s·(z·P(z)): the polynomial already carries the factor of two that
    // the doubled series needs, so it is applied to the correction only.
    let ln_mantissa = 2.0 * s + s * (z * p);

    // The exponent's contribution goes in through the same split of ln 2 the
    // reduction uses, so the two roundings do not compound.
    let e = exponent as f64;
    e * LN2_HI + (ln_mantissa + e * LN2_LO)
}

/// The base-10 logarithm of `x`.
#[must_use]
pub fn log10(x: f64) -> f64 {
    ln(x) * LOG10_E
}

/// The base-2 logarithm of `x`.
#[must_use]
pub fn log2(x: f64) -> f64 {
    ln(x) * LOG2_E
}

/// `e` raised to `x`.
#[must_use]
pub fn exp(x: f64) -> f64 {
    if x.is_nan() {
        return f64::NAN;
    }
    // Past these the answer is infinite or zero, and the reduction below
    // would overflow its exponent trying to say so.
    if x > 709.782_712_893_384 {
        return f64::INFINITY;
    }
    if x < -745.133_219_101_941_2 {
        return 0.0;
    }

    // x = k·ln2 + r with |r| ≤ ln2/2, so e^x = 2^k·e^r.
    let k = round_to_int(x * LOG2_E);
    let kf = k as f64;
    let r = (x - kf * LN2_HI) - kf * LN2_LO;

    // e^r by its Taylor series. |r| ≤ 0.347, so the seventeenth term is
    // already below the last bit of the sum; the loop is written out rather
    // than unrolled into constants because a mistyped factorial would be
    // invisible and this cannot be.
    let mut term = 1.0;
    let mut sum = 1.0;
    let mut n = 1.0;
    while n <= 17.0 {
        term = term * r / n;
        sum += term;
        n += 1.0;
    }

    scale_by_power_of_two(sum, k)
}

/// `x` raised to `y`.
///
/// The special cases are the whole difficulty. A colour transform arriving
/// with a negative base and an integer exponent expects a real answer; one
/// arriving with a negative base and a fractional exponent expects to be told
/// there isn't one, rather than to receive a silently wrong number.
#[must_use]
pub fn pow(x: f64, y: f64) -> f64 {
    if y == 0.0 || x == 1.0 {
        return 1.0;
    }
    if x.is_nan() || y.is_nan() {
        return f64::NAN;
    }
    if y == 1.0 {
        return x;
    }

    if x == 0.0 {
        return if y > 0.0 { 0.0 } else { f64::INFINITY };
    }

    if x < 0.0 {
        if trunc(y) != y {
            return f64::NAN; // no real root
        }
        // An odd exponent keeps the sign; an even one does not.
        let odd = trunc(y * 0.5) * 2.0 != y;
        let magnitude = pow(-x, y);
        return if odd { -magnitude } else { magnitude };
    }

    // Small integer exponents come up constantly — squares in distances,
    // cubes in Lab — and repeated multiplication is both faster and exactly
    // what a reader expects `x³` to mean.
    if trunc(y) == y && abs(y) <= 64.0 {
        let n = y as i64;
        let mut result = 1.0;
        let mut base = x;
        let mut remaining = n.unsigned_abs();
        while remaining > 0 {
            if remaining & 1 == 1 {
                result *= base;
            }
            base *= base;
            remaining >>= 1;
        }
        return if n < 0 { 1.0 / result } else { result };
    }

    exp(y * ln(x))
}

/// The cube root of `x`, which Lab colour needs and IEEE does not pin.
#[must_use]
pub fn cbrt(x: f64) -> f64 {
    if x == 0.0 || !x.is_finite() {
        return x;
    }
    let negative = x < 0.0;
    let a = abs(x);

    // a = m·2^e with m in [1/2, 1), so cbrt(a) = cbrt(m)·2^(e/3). Both halves
    // of that matter: the exponent splits into a whole part and a remainder
    // of 0, 1 or 2, and the mantissa needs its *cube root* — seeding with the
    // mantissa itself, which is what this did first, starts 37% out and
    // Newton does not close that in four passes.
    let (mantissa, exponent) = frexp(a);
    let third = exponent.div_euclid(3);
    let remainder = exponent - third * 3;

    // A straight line through cbrt(1/2) and cbrt(1) is within 1.3% across the
    // interval, which Newton — squaring its error each pass — takes to the
    // last bit in four.
    let seed = 0.587_401_051_968_199_4 + 0.412_598_948_031_800_6 * mantissa;
    let mut guess = scale_by_power_of_two(seed * cbrt_seed(remainder), i64::from(third));

    for _ in 0..4 {
        guess = (2.0 * guess + a / (guess * guess)) / 3.0;
    }
    if negative {
        -guess
    } else {
        guess
    }
}

/// 2^(r/3) for the remainder the exponent split leaves behind, so the seed
/// lands in the right octave whatever that remainder is.
fn cbrt_seed(remainder: i32) -> f64 {
    match remainder {
        1 => 1.259_921_049_894_873_2,
        2 => 1.587_401_051_968_199_4,
        _ => 1.0,
    }
}

/// `x` in degrees, from radians.
#[must_use]
pub fn to_degrees(x: f64) -> f64 {
    x * 57.295_779_513_082_32
}

/// `x` in radians, from degrees.
#[must_use]
pub fn to_radians(x: f64) -> f64 {
    x * 0.017_453_292_519_943_295
}

// ---- The exactly-pinned pieces, spelled out because there is no `std` ----

fn abs(x: f64) -> f64 {
    f64::from_bits(x.to_bits() & !(1u64 << 63))
}

fn is_negative(x: f64) -> bool {
    x.to_bits() >> 63 == 1
}

/// Towards zero, to the nearest integer.
fn trunc(x: f64) -> f64 {
    let magnitude = abs(x);
    if magnitude >= TWO_52 || x == 0.0 {
        return x;
    }
    // Adding and removing 2^52 rounds to nearest; stepping back by one where
    // that rounded upward turns it into truncation.
    let rounded = magnitude + TWO_52 - TWO_52;
    let truncated = if rounded > magnitude {
        rounded - 1.0
    } else {
        rounded
    };
    if is_negative(x) {
        -truncated
    } else {
        truncated
    }
}

/// Nearest integer, halves away from zero, as an `i64`.
fn round_to_int(x: f64) -> i64 {
    if !x.is_finite() {
        return 0;
    }
    let shifted = if x >= 0.0 { x + 0.5 } else { x - 0.5 };
    // Short of i64::MAX, so the cast below cannot saturate on a value the
    // caller could not have meant anyway.
    let clamped = shifted.clamp(-9.0e18, 9.0e18);
    trunc(clamped) as i64
}

/// Splits `x` into a mantissa in `[0.5, 1)` and an exponent.
fn frexp(x: f64) -> (f64, i32) {
    let bits = x.to_bits();
    let raw = ((bits >> 52) & 0x7FF) as i32;

    if raw == 0 {
        // Subnormal: scale into the normal range and account for it.
        let (mantissa, exponent) = frexp(x * TWO_52);
        return (mantissa, exponent - 52);
    }

    let mantissa = f64::from_bits((bits & !(0x7FFu64 << 52)) | (1022u64 << 52));
    (mantissa, raw - 1022)
}

/// `x · 2^n`, through the exponent rather than by repeated multiplication.
fn scale_by_power_of_two(x: f64, n: i64) -> f64 {
    // In steps no one of which can overflow the exponent field, so a large
    // `n` saturates to infinity or zero the way it should rather than
    // wrapping into a finite number.
    let mut result = x;
    let mut remaining = n;
    while remaining != 0 {
        let step = remaining.clamp(-1000, 1000);
        result *= f64::from_bits(((step + 1023) as u64) << 52);
        remaining -= step;
        if result == 0.0 || !result.is_finite() {
            break;
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::format;

    /// How far a result may sit from the platform's own, in units of the last
    /// place. This is not a correctness bound — it says the algorithm is the
    /// right algorithm. Determinism is the property that matters, and that
    /// comes from construction rather than from any assertion here.
    #[track_caller]
    fn within_ulps(actual: f64, expected: f64, ulps: i64, what: &str) {
        if actual == expected {
            return;
        }
        assert!(
            actual.is_finite() && expected.is_finite(),
            "{what}: got {actual}, want {expected}"
        );

        let a = actual.to_bits() as i64;
        let b = expected.to_bits() as i64;
        let distance = if (a < 0) != (b < 0) {
            // Either side of zero: measure through it.
            a.abs() + b.abs()
        } else {
            (a - b).abs()
        };
        assert!(
            distance <= ulps,
            "{what}: got {actual}, want {expected}, {distance} ulps apart"
        );
    }

    #[test]
    fn sine_and_cosine_match_the_platform_across_the_circle() {
        let mut x = -20.0f64;
        while x <= 20.0 {
            within_ulps(sin(x), x.sin(), 2, &format!("sin({x})"));
            within_ulps(cos(x), x.cos(), 2, &format!("cos({x})"));
            x += 0.013;
        }
    }

    #[test]
    fn the_exact_angles_are_exact() {
        assert_eq!(sin(0.0), 0.0);
        assert_eq!(cos(0.0), 1.0);
        within_ulps(sin(FRAC_PI_2), 1.0, 2, "sin(pi/2)");
        within_ulps(cos(PI), -1.0, 2, "cos(pi)");
        assert!(
            abs(sin(PI)) < 1e-15,
            "sin(pi) is zero to within the error in pi itself, got {}",
            sin(PI)
        );
    }

    #[test]
    fn the_pythagorean_identity_holds_where_cancellation_would_show() {
        let mut x = -8.0f64;
        while x <= 8.0 {
            let (s, c) = (sin(x), cos(x));
            assert!(
                abs(s * s + c * c - 1.0) < 1e-14,
                "sin^2 + cos^2 at {x} is {}",
                s * s + c * c
            );
            x += 0.031;
        }
    }

    #[test]
    fn logarithms_match_the_platform() {
        for &x in &[
            1e-300, 1e-10, 0.001, 0.5, 0.999, 1.0, 1.001, 2.0, 10.0, 1e5, 1e100, 1e300,
        ] {
            within_ulps(ln(x), x.ln(), 2, &format!("ln({x})"));
            within_ulps(log10(x), x.log10(), 4, &format!("log10({x})"));
            within_ulps(log2(x), x.log2(), 4, &format!("log2({x})"));
        }
        assert_eq!(ln(1.0), 0.0);
        assert_eq!(ln(0.0), f64::NEG_INFINITY);
        assert!(ln(-1.0).is_nan());
    }

    #[test]
    fn exponentials_match_the_platform() {
        let mut x = -40.0f64;
        while x <= 40.0 {
            within_ulps(exp(x), x.exp(), 4, &format!("exp({x})"));
            x += 0.37;
        }
        assert_eq!(exp(0.0), 1.0);
        assert_eq!(exp(1000.0), f64::INFINITY);
        assert_eq!(exp(-1000.0), 0.0);
    }

    #[test]
    fn powers_match_the_platform() {
        for &base in &[0.001, 0.25, 0.5, 1.5, 2.0, 7.3, 100.0] {
            for &exponent in &[-3.0, -0.5, 0.0, 1.0 / 2.4, 0.5, 1.0, 2.4, 3.0, 7.0] {
                within_ulps(
                    pow(base, exponent),
                    base.powf(exponent),
                    8,
                    &format!("pow({base}, {exponent})"),
                );
            }
        }
    }

    /// The sRGB transfer function is why `pow` is on a pixel path at all, so
    /// it gets its own check across the whole channel range — and at the only
    /// precision that matters, which is after quantisation.
    #[test]
    fn the_srgb_transfer_function_agrees_to_the_last_byte() {
        for step in 0..=2000 {
            let v = f64::from(step) / 2000.0;
            let ours = 1.055 * pow(v, 1.0 / 2.4) - 0.055;
            let theirs = 1.055 * v.powf(1.0 / 2.4) - 0.055;
            let a = (ours.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
            let b = (theirs.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
            assert_eq!(a, b, "channel {v} quantises differently");
        }
    }

    #[test]
    fn powers_of_a_negative_base_follow_the_exponent() {
        assert_eq!(pow(-2.0, 2.0), 4.0);
        assert_eq!(pow(-2.0, 3.0), -8.0);
        assert!(pow(-2.0, 0.5).is_nan(), "no real root");
        assert_eq!(pow(-2.0, 0.0), 1.0);
        assert_eq!(pow(0.0, 2.0), 0.0);
        assert_eq!(pow(0.0, -1.0), f64::INFINITY);
        assert_eq!(pow(2.0, 10.0), 1024.0, "integer powers stay exact");
    }

    #[test]
    fn arctangents_match_the_platform() {
        let mut x = -50.0f64;
        while x <= 50.0 {
            within_ulps(atan(x), x.atan(), 2, &format!("atan({x})"));
            x += 0.17;
        }
        // Around the two reduction boundaries, where an off-by-one branch
        // would show as a jump rather than as a rounding difference.
        for &x in &[
            0.0,
            TAN_PI_12 - 1e-12,
            TAN_PI_12,
            TAN_PI_12 + 1e-12,
            1.0 - 1e-12,
            1.0,
            1.0 + 1e-12,
        ] {
            within_ulps(atan(x), x.atan(), 2, &format!("atan({x})"));
            within_ulps(atan(-x), (-x).atan(), 2, &format!("atan({})", -x));
        }
    }

    #[test]
    fn the_four_quadrants_of_atan2_match_the_platform() {
        for &(y, x) in &[
            (1.0, 1.0),
            (1.0, -1.0),
            (-1.0, 1.0),
            (-1.0, -1.0),
            (0.0, 1.0),
            (0.0, -1.0),
            (1.0, 0.0),
            (-1.0, 0.0),
            (3.0, 4.0),
            (-7.5, 0.25),
            (1e-300, 1e300),
        ] {
            within_ulps(atan2(y, x), y.atan2(x), 3, &format!("atan2({y}, {x})"));
        }
        assert_eq!(atan2(0.0, 0.0), 0.0);
        assert_eq!(atan2(-0.0, -0.0).to_bits(), (-PI).to_bits());
    }

    #[test]
    fn cube_roots_match_the_platform() {
        for &x in &[
            -1000.0, -8.0, -1.0, -1e-9, 0.0, 1e-9, 0.5, 1.0, 8.0, 27.0, 1e30, 1e-30,
        ] {
            within_ulps(cbrt(x), x.cbrt(), 2, &format!("cbrt({x})"));
        }
        assert_eq!(cbrt(8.0), 2.0);
    }

    /// The point of the crate: the same input gives the same bits every time,
    /// with no dependence on anything outside these functions.
    #[test]
    fn results_are_reproducible_bit_for_bit() {
        for &x in &[0.1, 1.0, 2.5, -3.75, 12.0, 100.25] {
            let once = (sin(x), cos(x), exp(x), atan(x));
            for _ in 0..4 {
                assert_eq!(sin(x).to_bits(), once.0.to_bits());
                assert_eq!(cos(x).to_bits(), once.1.to_bits());
                assert_eq!(exp(x).to_bits(), once.2.to_bits());
                assert_eq!(atan(x).to_bits(), once.3.to_bits());
                if x > 0.0 {
                    assert_eq!(ln(x).to_bits(), ln(x).to_bits());
                    assert_eq!(pow(x, 2.4).to_bits(), pow(x, 2.4).to_bits());
                }
            }
        }
    }

    /// The split constants carry the property the whole reduction rests on,
    /// and nothing else in the crate would notice if they lost it: a `PIO2_HI`
    /// with dirty low bits still reduces, just less exactly, and the failure
    /// shows up as a few ulps at large arguments rather than as anything
    /// obviously wrong.
    #[test]
    fn the_split_constants_are_split_where_they_claim() {
        assert!(
            PIO2_HI.to_bits().trailing_zeros() >= 22,
            "PIO2_HI must end in twenty-two zero bits or its product with the \
             quadrant count stops being exact; it ends in {}",
            PIO2_HI.to_bits().trailing_zeros()
        );
        assert_eq!(
            PIO2_HI + PIO2_LO,
            FRAC_PI_2,
            "and the two together must still be pi/2"
        );
        const {
            assert!(
                PIO2_LO > 0.0,
                "the remainder must be positive, so the high part is the low \
                 estimate and the reduction subtracts too little rather than \
                 too much"
            );
        }

        assert!(
            LN2_HI.to_bits().trailing_zeros() >= 21,
            "the same split for ln 2, which `exp` relies on the same way; it ends in {}",
            LN2_HI.to_bits().trailing_zeros()
        );
        assert_eq!(LN2_HI + LN2_LO, core::f64::consts::LN_2);

        // A quadrant count at the top of the documented range still gives an
        // exact product, which is what the doc comment claims.
        let n = 4_194_304.0f64; // 2^22
        assert_eq!(
            (n * PIO2_HI) / n,
            PIO2_HI,
            "exact, so dividing back returns it unchanged"
        );
    }

    #[test]
    fn the_helpers_that_stand_in_for_std_behave() {
        assert_eq!(trunc(3.7), 3.0);
        assert_eq!(trunc(-3.7), -3.0);
        assert_eq!(trunc(2.5), 2.0);
        assert_eq!(trunc(-2.5), -2.0);
        assert_eq!(trunc(0.0), 0.0);
        assert_eq!(trunc(1e300), 1e300);

        assert_eq!(round_to_int(2.5), 3);
        assert_eq!(round_to_int(-2.5), -3);
        assert_eq!(round_to_int(2.4), 2);
        assert_eq!(round_to_int(f64::NAN), 0);

        assert_eq!(frexp(8.0), (0.5, 4));
        assert_eq!(frexp(1.0), (0.5, 1));
        assert_eq!(scale_by_power_of_two(0.5, 4), 8.0);
        assert_eq!(scale_by_power_of_two(1.0, 5000), f64::INFINITY);
        assert_eq!(scale_by_power_of_two(1.0, -5000), 0.0);
    }

    #[test]
    fn degenerate_inputs_do_not_panic() {
        for &x in &[f64::NAN, f64::INFINITY, f64::NEG_INFINITY, 0.0, -0.0] {
            let _ = sin(x);
            let _ = cos(x);
            let _ = tan(x);
            let _ = atan(x);
            let _ = ln(x);
            let _ = log10(x);
            let _ = exp(x);
            let _ = cbrt(x);
            let _ = pow(x, 2.0);
            let _ = pow(2.0, x);
            let _ = pow(x, x);
            let _ = atan2(x, x);
            let _ = to_degrees(x);
            let _ = to_radians(x);
        }
    }
}
