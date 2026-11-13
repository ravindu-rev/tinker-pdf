//! 26.6 fixed-point arithmetic, as `docs/design/hinting.md`'s *Numbers*
//! states it.
//!
//! A coordinate, a distance and a control value are F26Dot6: an `i32`
//! counting sixty-fourths of a pixel. Every operation here is integer
//! arithmetic in 64 bits or wider, so it gives the same answer on every
//! target (ruling 4), and every one is total: none panics, whatever its
//! operands. A result that leaves the 32 bits wraps, the two's-complement
//! answer the instruction set's "no overflow or underflow protection" leaves
//! (*Arithmetic and math instructions*).
//!
//! The other hinters fit outlines to the same grid with these operations
//! rather than with their own.

/// One pixel, in 26.6.
pub const ONE: i32 = 64;

/// `n / d` rounded to the nearest integer, a half away from zero.
///
/// The rounding every inexact step of the interpreter takes, this engine's
/// choice where the specification states none. A zero `d` gives zero, which
/// no caller asks for: each checks its divisor first.
#[must_use]
pub fn round_div(n: i64, d: i64) -> i64 {
    if d == 0 {
        return 0;
    }
    // In 128 bits nothing below overflows, `i64::MIN / -1` included.
    let (n, d) = (i128::from(n), i128::from(d));
    let quotient = n / d;
    let remainder = n % d;
    let rounded = if 2 * remainder.abs() >= d.abs() {
        // The remainder carries `n`'s sign; the step away from zero carries
        // the quotient's, which is `n`'s and `d`'s together.
        quotient + if (n < 0) == (d < 0) { 1 } else { -1 }
    } else {
        quotient
    };
    // Only `i64::MIN / -1` leaves 64 bits, and it wraps as the rest do.
    rounded as i64
}

/// `MUL`: the 26.6 product `a · b / 64`, rounded half away from zero (this
/// engine's choice: the specification states no rounding), then wrapped.
#[must_use]
pub fn mul(a: i32, b: i32) -> i32 {
    round_div(i64::from(a) * i64::from(b), i64::from(ONE)) as i32
}

/// `DIV`: the 26.6 quotient `dividend · 64 / divisor`, truncated toward zero
/// as the specification states ("this truncates rather than rounds the
/// value"), then wrapped. `None` for a zero divisor, which is an error.
#[must_use]
pub fn div(dividend: i32, divisor: i32) -> Option<i32> {
    if divisor == 0 {
        return None;
    }
    // At most 2^37 / 1, so 64 bits hold every quotient; Rust's `/` truncates
    // toward zero.
    Some((i64::from(dividend) * i64::from(ONE) / i64::from(divisor)) as i32)
}

/// `FLOOR`: the greatest whole pixel not above `n`.
#[must_use]
pub fn floor(n: i32) -> i32 {
    n & !(ONE - 1)
}

/// `CEILING`: the least whole pixel not below `n`, wrapping past the top.
#[must_use]
pub fn ceil(n: i32) -> i32 {
    n.wrapping_add(ONE - 1) & !(ONE - 1)
}

/// `n` to the nearest whole pixel, a half away from zero: rounding to the
/// grid with no compensation, symmetric about zero as the instruction set's
/// rounding is (*Instructing TrueType Glyphs*, "Controlling rounding").
#[must_use]
pub fn round(n: i32) -> i32 {
    let magnitude = i64::from(n).abs();
    let rounded = (magnitude + i64::from(ONE / 2)) & !i64::from(ONE - 1);
    (if n < 0 { -rounded } else { rounded }) as i32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_div_rounds_each_half_away_from_zero() {
        for (n, d, expected) in [
            (5, 2, 3),
            (-5, 2, -3),
            (5, -2, -3),
            (-5, -2, 3),
            (4, 2, 2),
            (7, 3, 2),
            (-7, 3, -2),
            (8, 3, 3),
            (-8, 3, -3),
            (0, 7, 0),
            (1, 3, 0),
            (-1, 3, 0),
        ] {
            assert_eq!(round_div(n, d), expected, "{n} / {d}");
        }
        assert_eq!(round_div(9, 0), 0, "a zero divisor answers zero");
        assert_eq!(round_div(i64::MIN, -1), i64::MIN, "the one overflow wraps");
        assert_eq!(round_div(i64::MAX, 1), i64::MAX);
    }

    #[test]
    fn the_grid_operations_land_on_whole_pixels() {
        for n in [
            -200i32, -64, -63, -33, -32, -31, -1, 0, 1, 31, 32, 33, 63, 64, 200,
        ] {
            let below = n.div_euclid(64) * 64;
            assert_eq!(floor(n), below, "floor {n}");
            let above = if n % 64 == 0 { n } else { below + 64 };
            assert_eq!(ceil(n), above, "ceil {n}");
            let nearest = if n < 0 {
                -((-n + 32) / 64 * 64)
            } else {
                (n + 32) / 64 * 64
            };
            assert_eq!(round(n), nearest, "round {n}");
        }
        assert_eq!(round(i32::MIN), i32::MIN, "the magnitude past 2^31 wraps");
        assert_eq!(ceil(i32::MAX), i32::MIN, "CEILING wraps past the top");
        assert_eq!(floor(i32::MIN), i32::MIN);
    }
}
