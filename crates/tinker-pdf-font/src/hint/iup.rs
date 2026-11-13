//! Interpolation and exceptions: *Managing outlines*' `IUP`, which moves each
//! point no instruction touched along one axis to where its contour's touched
//! points say, and *Managing exceptions*' `DELTAP1` to `DELTAP3`, which move a
//! point, and `DELTAC1` to `DELTAC3`, which change a control value, at one
//! size each (`docs/design/hinting.md`, "Interpolation and exceptions").
//!
//! `IUP` reads the glyph's zone alone: the twilight zone has no contours, so
//! `zp2` pointing at it is an error. Every point it considers, and every pair
//! a delta pops, is a step of the run's budget.

use super::fixed;
use super::state::{Point, Zone, TOUCHED_X, TOUCHED_Y};
use super::vm::{Error, Vm};

/// The axis an `IUP` interpolates along.
#[derive(Clone, Copy)]
enum Axis {
    X,
    Y,
}

impl Axis {
    /// The coordinate of `p` along the axis, in 64 bits.
    fn of(self, p: Point) -> i64 {
        i64::from(match self {
            Axis::X => p.x,
            Axis::Y => p.y,
        })
    }

    /// Sets the coordinate of `p` along the axis to `value`, wrapped to 32
    /// bits as every coordinate is.
    fn set(self, p: &mut Point, value: i64) {
        let value = value as i32;
        match self {
            Axis::X => p.x = value,
            Axis::Y => p.y = value,
        }
    }

    /// The touched flag of the axis.
    fn flag(self) -> u8 {
        match self {
            Axis::X => TOUCHED_X,
            Axis::Y => TOUCHED_Y,
        }
    }
}

/// Where an untouched point whose original coordinate is `o` goes between
/// the touched points `a` and `b`, each its original and current coordinate:
/// "moved such that its position relative to those touched points is the
/// same as it was in the original outline" where `o` lies between their
/// original coordinates — `c1 + (o − o1)(c2 − c1)/(o2 − o1)`, rounded half
/// away from zero, the lower of the two in original coordinate as `1` — and
/// shifted by the displacement of the nearer touched point otherwise, the
/// first of the two where both stand at one original coordinate.
fn interpolated(o: i64, a: (i64, i64), b: (i64, i64)) -> i64 {
    let (low, high) = if a.0 <= b.0 { (a, b) } else { (b, a) };
    if o <= low.0 {
        o + (low.1 - low.0)
    } else if o >= high.0 {
        o + (high.1 - high.0)
    } else {
        // `o` strictly between the two, so the span is above zero. Each factor
        // is a difference of 32-bit coordinates, under 2^33, so the product
        // takes 128 bits; the quotient is under the current span, which fits.
        let (n, d) = (
            i128::from(o - low.0) * i128::from(high.1 - low.1),
            i128::from(high.0 - low.0),
        );
        let quotient = n / d;
        let rounded = if 2 * (n % d).abs() >= d {
            quotient + n.signum()
        } else {
            quotient
        };
        low.1 + rounded as i64
    }
}

/// `IUP` over the contour `first..=last` of `zone` along `axis`: nothing
/// where no point of it is touched; every other point shifted by the one
/// touched point's displacement where there is one; and otherwise each run
/// of untouched points between two touched points that follow each other
/// round the contour placed by [`interpolated`] from those two, the run
/// after the last touched point taking the last and the first. A point it
/// moves stays untouched.
fn interpolate_contour(zone: &mut Zone, first: usize, last: usize, axis: Axis) {
    let flag = axis.flag();
    let touched: Vec<usize> = (first..=last)
        .filter(|&i| zone.flags[i] & flag != 0)
        .collect();
    let place = |zone: &Zone, i: usize| (axis.of(zone.original[i]), axis.of(zone.current[i]));
    match touched.as_slice() {
        [] => {}
        [only] => {
            let (original, current) = place(zone, *only);
            for i in (first..=last).filter(|i| i != only) {
                let moved = axis.of(zone.original[i]) + (current - original);
                axis.set(&mut zone.current[i], moved);
            }
        }
        _ => {
            let next = |i: usize| if i == last { first } else { i + 1 };
            for (k, &a) in touched.iter().enumerate() {
                let b = touched[(k + 1) % touched.len()];
                let (from, to) = (place(zone, a), place(zone, b));
                let mut i = next(a);
                while i != b {
                    let moved = interpolated(axis.of(zone.original[i]), from, to);
                    axis.set(&mut zone.current[i], moved);
                    i = next(i);
                }
            }
        }
    }
}

/// The distance a delta exception's argument moves by, in 26.6: its low four
/// bits select a step of −8 to −1 or 1 to 8 — 0 is −8, 7 is −1, 8 is 1 and
/// 15 is 8 — and a step is `1 / 2^delta_shift` pixels. A shift of 0 to 6
/// steps by a whole number of sixty-fourths; one above takes the step's
/// share of a sixty-fourth rounded half away from zero, and one below zero a
/// step of more than a pixel, each held to 32 bits — this engine's reading
/// of a shift the specifications give no range for.
fn delta_amount(arg: i32, shift: i32) -> i32 {
    let selector = arg & 0xF;
    let step = if selector < 8 {
        selector - 8
    } else {
        selector - 7
    };
    let sixty_fourths = i128::from(step) * i128::from(fixed::ONE);
    let amount = if shift >= 0 {
        // From a shift of 11 every step is under half a sixty-fourth, so one
        // past 16 rounds to nothing as 16 does.
        let divisor = 1i128 << shift.min(16);
        let quotient = sixty_fourths / divisor;
        let remainder = sixty_fourths % divisor;
        if 2 * remainder.abs() >= divisor {
            quotient + sixty_fourths.signum()
        } else {
            quotient
        }
    } else {
        // 2^40 times at most 512 is far inside 128 bits.
        sixty_fourths << shift.unsigned_abs().min(40)
    };
    amount.clamp(i128::from(i32::MIN), i128::from(i32::MAX)) as i32
}

impl Vm {
    /// Executes `IUP`, `DELTAP1` to `DELTAP3` or `DELTAC1` to `DELTAC3`.
    pub(super) fn interpolate_or_except(&mut self, op: u8) -> Result<(), Error> {
        match op {
            // IUP[a]: a = 0 the y axis, 1 the x axis, over the zone zp2
            // points at, which must be the glyph's: "Applying IUP to zone 0 is
            // an error" (Apple's manual).
            0x30 | 0x31 => {
                if self.zone_at(2)? == 0 {
                    return Err(Error::TwilightInterpolation);
                }
                let axis = if op == 0x31 { Axis::X } else { Axis::Y };
                let ends = self.zones[1].ends.clone();
                let mut first = 0usize;
                for end in ends {
                    self.spend(end + 1 - first)?;
                    interpolate_contour(&mut self.zones[1], first, end, axis);
                    first = end + 1;
                }
            }
            // DELTAP1[ ] to DELTAP3[ ], DELTAC1[ ] to DELTAC3[ ]: pops n, then
            // n pairs, each a point or a CVT entry and then its argument;
            // DELTAP1 and DELTAC1 apply at delta_base plus the argument's high
            // four bits, the second at sixteen more and the third at thirty-two
            // more.
            _ => self.exceptions(op)?,
        }
        Ok(())
    }

    /// The delta exceptions. `n` is a count of pairs, a uint32, so a negative
    /// one names more pairs than any stack holds. Each pair's target is
    /// checked whatever the size, so a program is the same program at every
    /// size; at the size its argument names, a point in `zp0`'s zone moves by
    /// the argument's amount along the freedom vector, its projection
    /// changing by it, and is touched, and a CVT entry gains the amount.
    fn exceptions(&mut self, op: u8) -> Result<(), Error> {
        let n = self.pop()?;
        let (offset, points) = match op {
            0x5D => (0, true),
            0x71 => (16, true),
            0x72 => (32, true),
            0x73 => (0, false),
            0x74 => (16, false),
            _ => (32, false),
        };
        let ppem = i64::from(self.size.pixels_per_em());
        for _ in 0..n as u32 {
            self.spend(1)?;
            let target = self.pop()?;
            let arg = self.pop()?;
            let at = i64::from(self.gs.delta_base) + offset + i64::from((arg >> 4) & 0xF);
            let amount = delta_amount(arg, self.gs.delta_shift);
            if points {
                let zone = self.zone_at(0)?;
                let index = self.point_index(zone, target)?;
                if at == ppem {
                    self.move_by(zone, index, amount)?;
                }
            } else {
                let entry = usize::try_from(target)
                    .ok()
                    .filter(|&entry| entry < self.cvt.len())
                    .ok_or(Error::CvtIndex(target))?;
                if at == ppem {
                    self.cvt[entry] = self.cvt[entry].wrapping_add(amount);
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A point between touched points at the ends of the 26.6 range: the
    /// product of the two spans is past 64 bits, and the answer is worked by
    /// hand — half way along a span of `2^32 − 1`, from the lowest coordinate
    /// to the highest, and from the highest down to the lowest.
    #[test]
    fn an_interpolation_across_the_whole_range_does_not_overflow() {
        let (least, most) = (i64::from(i32::MIN), i64::from(i32::MAX));
        assert_eq!(interpolated(0, (least, least), (most, most)), 0);
        // `(0 + 2^31)(−(2^32 − 1)) / (2^32 − 1)` is `−2^31`, from `2^31 − 1`.
        assert_eq!(interpolated(0, (least, most), (most, least)), -1);
        assert_eq!(interpolated(0, (most, least), (least, most)), -1);
    }

    /// Every selector's step, at the default shift and at the ends of the
    /// range the shift is read over: the step table of *DELTAP1*, scaled to
    /// sixty-fourths and rounded or held as [`delta_amount`] says.
    #[test]
    fn a_delta_argument_selects_its_step() {
        let steps = [-8, -7, -6, -5, -4, -3, -2, -1, 1, 2, 3, 4, 5, 6, 7, 8];
        for (selector, step) in steps.iter().enumerate() {
            let arg = selector as i32 | 0x50;
            assert_eq!(delta_amount(arg, 3), step * 8, "a step of 1/8 pixel");
            assert_eq!(delta_amount(arg, 0), step * 64, "a whole pixel");
            assert_eq!(delta_amount(arg, 6), *step, "a sixty-fourth");
            // At a shift of 7 a step is half a sixty-fourth: an odd step's
            // half rounds away from zero.
            let half = (step.abs() + 1) / 2 * step.signum();
            assert_eq!(
                delta_amount(arg, 7),
                half,
                "half a sixty-fourth, away from zero"
            );
            assert_eq!(delta_amount(arg, 32_767), 0, "nothing at a vast shift");
            assert_eq!(delta_amount(arg, -1), step * 128, "two pixels");
            assert_eq!(
                delta_amount(arg, i32::MIN),
                if *step > 0 { i32::MAX } else { i32::MIN },
                "held to 32 bits"
            );
        }
        assert_eq!(
            delta_amount(0x7FFF_FF0F, 3),
            64,
            "only the low four bits select"
        );
    }
}
