//! Transcendental functions that give the same bits on every target.
//!
//! Feature documentation: `docs/features/determinism.md`.
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
//! One rule the compiler cannot enforce inside these walls is **no
//! `mul_add`**: it may compile to a fused multiply-add on one target and a
//! multiply-then-add on another, and those differ. The extra precision would
//! be welcome; the divergence is not. `cargo xtask libm` bans the call across
//! every pixel-path crate, so the rule is checked rather than remembered.
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
/// (Cody and Waite's method. From 2^22 quadrants on the product is no longer
/// exact, so a larger argument is reduced by [`reduce_exactly`] instead.)
const PIO2_HI: f64 = f64::from_bits(0x3FF9_21FB_5440_0000);
const PIO2_LO: f64 = 6.077_100_506_506_192e-11;

/// The quadrant count from which `n · PIO2_HI` stops being exact.
const EXACT_QUADRANTS: u64 = 1 << 22;

/// 2/π's binary fraction `0.b₁b₂b₃…`, most significant bit first: bit `i` is
/// bit `63 − (i − 1) mod 64` of word `(i − 1) / 64`.
///
/// 1 216 bits, of which [`reduce_exactly`] reads at most the first 1 161:
/// the largest double is `m · 2^971`, and the bits of 2/π above position 970
/// weigh whole turns of it. They are fdlibm's `ipio2` digits, which are
/// 2/π's, recomputed in integers by Machin's formula — the same figures to
/// the last bit, so the table is checked by arithmetic rather than copied.
const TWO_OVER_PI: [u64; 19] = [
    0xA2F9_836E_4E44_1529,
    0xFC27_57D1_F534_DDC0,
    0xDB62_9599_3C43_9041,
    0xFE51_63AB_DEBB_C561,
    0xB724_6E3A_424D_D2E0,
    0x0649_2EEA_09D1_921C,
    0xFE1D_EB1C_B129_A73E,
    0xE882_35F5_2EBB_4484,
    0xE99C_7026_B45F_7E41,
    0x3991_D639_8353_39F4,
    0x9C84_5F8B_BDF9_283B,
    0x1FF8_97FF_DE05_980F,
    0xEF2F_118B_5A0A_6D1F,
    0x6D36_7ECF_27CB_09B7,
    0x4F46_3F66_9E5F_EA2D,
    0x7527_BAC7_EBE5_F17B,
    0x3D07_39F7_8A52_92EA,
    0x6BFB_5FB1_1F8D_5D08,
    0x5603_3046_FC7B_6BAB,
];

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
/// and how many came off — past [`EXACT_QUADRANTS`], how many modulo four,
/// which is all `sin` and `cos` read.
fn reduce(x: f64) -> (f64, i64) {
    // Small arguments need no reduction, and taking the general path for them
    // would only add rounding.
    if abs(x) < FRAC_PI_4 {
        return (x, 0);
    }

    let quadrant = round_to_int(x * FRAC_2_PI);
    if quadrant.unsigned_abs() >= EXACT_QUADRANTS {
        return reduce_exactly(x);
    }
    let n = quadrant as f64;

    // `n · PIO2_HI` is exact, so the first subtraction is too; the second
    // removes the rest of π/2 and is the only step that rounds at all.
    let r = (x - n * PIO2_HI) - n * PIO2_LO;
    (r, quadrant)
}

/// Payne and Hanek's reduction, for an argument too large for Cody and
/// Waite's: `x · 2/π` modulo four, worked in integers from as many bits of
/// 2/π as the product needs, so that every finite argument keeps a remainder
/// in about `[-π/4, π/4]` and no argument's sine is anything but a sine.
///
/// `|x|` is `m · 2^k` for a 53-bit integer `m`. A bit of 2/π at position
/// `i ≤ k − 2` adds `m · 2^(k−i)` quadrants, a multiple of four, which moves
/// no answer; so the product is taken of `m` and a 192-bit window of 2/π
/// starting at position `k − 1` (or at the first bit, for a small `k`).
/// What the window leaves out is below 2^-137 of a quadrant, and the closest
/// a double comes to a multiple of π/2 is about 2^-61 of one, so the
/// remainder keeps more significant bits than a double holds.
fn reduce_exactly(x: f64) -> (f64, i64) {
    let bits = x.to_bits();
    let m = (bits & ((1 << 52) - 1)) | (1 << 52);
    let k = ((bits >> 52) & 0x7FF) as i64 - 1075;
    let start = (k - 1).max(1);
    let product = times_window(m, two_over_pi_from(start as u32));
    // `product` is `|x| · 2/π` with this many bits after the point.
    let point = (start + 191 - k) as u32;

    let mut quadrant = bit(&product, point) | bit(&product, point + 1) << 1;
    let mut fraction = product;
    keep_below(&mut fraction, point);
    // A fraction of a half or more rounds to the next quadrant, and what is
    // left is the distance back from it.
    let past_half = bit(&fraction, point - 1) == 1;
    if past_half {
        quadrant += 1;
        negate(&mut fraction);
        keep_below(&mut fraction, point);
    }
    let r = match highest_bit(&fraction) {
        None => 0.0,
        Some(top) => {
            // Sixty-four bits from the leading one, rounded once to a double
            // and placed by its exponent: the fraction of a quadrant.
            let mut leading = 0u64;
            for step in 0..64 {
                let at = i64::from(top) - step;
                leading = leading << 1
                    | if at >= 0 {
                        bit(&fraction, at as u32)
                    } else {
                        0
                    };
            }
            let of_a_quadrant =
                scale_by_power_of_two(leading as f64, i64::from(top) - 63 - i64::from(point));
            of_a_quadrant * FRAC_PI_2
        }
    };
    let r = if past_half { -r } else { r };
    let quadrant = (quadrant & 3) as i64;
    if is_negative(x) {
        (-r, (4 - quadrant) & 3)
    } else {
        (r, quadrant)
    }
}

/// 192 bits of [`TWO_OVER_PI`] from 1-based position `start`, most
/// significant word first.
fn two_over_pi_from(start: u32) -> [u64; 3] {
    let offset = (start - 1) as usize;
    let (word, shift) = (offset / 64, offset % 64);
    let at = |i: usize| TWO_OVER_PI.get(i).copied().unwrap_or(0);
    let mut window = [0u64; 3];
    for (j, slot) in window.iter_mut().enumerate() {
        *slot = if shift == 0 {
            at(word + j)
        } else {
            at(word + j) << shift | at(word + j + 1) >> (64 - shift)
        };
    }
    window
}

/// `m` times a 192-bit window: 256 bits, most significant word first.
fn times_window(m: u64, window: [u64; 3]) -> [u64; 4] {
    let mut product = [0u64; 4];
    let mut carry = 0u128;
    for j in (0..3).rev() {
        let wide = u128::from(m) * u128::from(window[j]) + carry;
        product[j + 1] = wide as u64;
        carry = wide >> 64;
    }
    product[0] = carry as u64;
    product
}

/// Bit `at` of a 256-bit number, counted from the least significant.
fn bit(number: &[u64; 4], at: u32) -> u64 {
    3usize
        .checked_sub((at / 64) as usize)
        .and_then(|word| number.get(word))
        .map_or(0, |w| w >> (at % 64) & 1)
}

/// Clears every bit of a 256-bit number from position `from` up.
fn keep_below(number: &mut [u64; 4], from: u32) {
    for (index, word) in number.iter_mut().enumerate() {
        let low = (3 - index) as u32 * 64;
        if from <= low {
            *word = 0;
        } else if from < low + 64 {
            *word &= (1u64 << (from - low)) - 1;
        }
    }
}

/// A 256-bit number's two's complement, in place.
fn negate(number: &mut [u64; 4]) {
    let mut carry = true;
    for word in number.iter_mut().rev() {
        let (sum, overflowed) = (!*word).overflowing_add(u64::from(carry));
        *word = sum;
        carry = overflowed;
    }
}

/// The position of a 256-bit number's highest set bit, or `None` for zero.
fn highest_bit(number: &[u64; 4]) -> Option<u32> {
    number
        .iter()
        .enumerate()
        .find(|(_, word)| **word != 0)
        .map(|(index, word)| (3 - index) as u32 * 64 + 63 - word.leading_zeros())
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

    /// Past Cody and Waite's range an argument is reduced exactly, so the
    /// sine and cosine of any finite double are its own. The expected values
    /// are each argument reduced by π computed to 3 000 bits in integers
    /// (Machin's formula), not the platform's: `1e22` is the classic case,
    /// `6381956970095103 · 2^797` the double nearest a multiple of π/2, and
    /// `f64::MAX` the largest there is.
    #[test]
    fn a_huge_argument_keeps_its_own_sine_and_cosine() {
        #[allow(clippy::excessive_precision)]
        let cases: [(f64, f64, f64); 12] = [
            (1.0e7, 0.420_547_793_190_782_45, -0.907_270_386_181_739_6),
            (1.0e15, 0.858_272_793_170_235_9, -0.513_193_737_786_970_3),
            (
                7.0 * 9_007_199_254_740_992.0,
                -0.727_357_272_887_399_8,
                -0.686_258_987_247_383_5,
            ),
            (1.4e19, -0.995_925_160_358_515_2, 0.090_183_562_609_080_09),
            (1.0e22, -0.852_200_849_767_188_8, 0.523_214_785_395_139),
            (
                5.313_557_944_960_515e35,
                0.263_176_822_585_353_2,
                0.964_747_614_691_986_4,
            ),
            (1.0e100, -0.380_637_731_005_028_7, 0.924_724_238_751_933_8),
            (1.0e300, -0.817_881_912_115_908_5, -0.575_386_111_957_549_1),
            (
                f64::MAX,
                0.004_961_954_789_184_061,
                -0.999_987_689_426_559_9,
            ),
            (-1.0e22, 0.852_200_849_767_188_8, 0.523_214_785_395_139),
            (-1.0e300, 0.817_881_912_115_908_5, -0.575_386_111_957_549_1),
            (5.319_372_648_326_541e255, 1.0, -4.687_165_924_254_628e-19),
        ];
        for (x, want_sin, want_cos) in cases {
            within_ulps(sin(x), want_sin, 2, &format!("sin({x:e})"));
            within_ulps(cos(x), want_cos, 2, &format!("cos({x:e})"));
        }
    }

    /// Every argument past Cody and Waite's range, sampled across the
    /// exponents, has a sine and a cosine on the unit circle — none is a NaN,
    /// which is what a remainder left unreduced became.
    #[test]
    fn every_finite_argument_has_a_point_on_the_circle() {
        let mut x = 6.0e6f64;
        while x.is_finite() {
            for value in [x, -x, x * 1.7, -x * 1.3] {
                let (s, c) = (sin(value), cos(value));
                assert!(
                    s.is_finite() && c.is_finite(),
                    "sin and cos of {value:e}: {s}, {c}"
                );
                assert!(
                    abs(s * s + c * c - 1.0) < 1e-14,
                    "sin^2 + cos^2 at {value:e} is {}",
                    s * s + c * c
                );
            }
            x *= 3.1;
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
