//! The remaining point instructions of *Managing outlines*: the shifts `SHP`,
//! `SHC`, `SHZ` and `SHPIX`, the alignments `ALIGNRP` and `ALIGNPTS`, `IP`,
//! `ISECT` and `UTP`, the flips `FLIPPT`, `FLIPRGON` and `FLIPRGOFF`, and the
//! anachronistic `AA` (`docs/design/hinting.md`, "Measuring and moving").
//!
//! `SHP`, `SHPIX`, `IP`, `FLIPPT` and `ALIGNRP` repeat for the loop count,
//! popping a point each time, and reset it to one after; each point a
//! repeat, a shift of a contour or a zone, or a flip of a range acts on is a
//! step of the run's budget.

use super::moves::project;
use super::state::{ON_CURVE, TOUCHED_X, TOUCHED_Y};
use super::vm::{Error, Vm};

/// `n / d` to the nearest whole, a half away from zero, in 128 bits; `d` is
/// not zero.
fn round_div_wide(n: i128, d: i128) -> i128 {
    let quotient = n / d;
    let remainder = n % d;
    if 2 * remainder.abs() >= d.abs() {
        quotient + if (n < 0) == (d < 0) { 1 } else { -1 }
    } else {
        quotient
    }
}

impl Vm {
    /// Runs `act` on each of the loop count's points, popping one a time and
    /// spending a step for each, then resets the loop count: "After the
    /// instruction executes, the loop variable is reset to 1."
    fn each_point(
        &mut self,
        mut act: impl FnMut(&mut Vm, i32) -> Result<(), Error>,
    ) -> Result<(), Error> {
        let count = self.gs.loop_count.max(1);
        for _ in 0..count {
            self.spend(1)?;
            let p = self.pop()?;
            act(self, p)?;
        }
        self.gs.loop_count = 1;
        Ok(())
    }

    /// The reference point `SHP`, `SHC` and `SHZ` shift by — rp2 in `zp1`'s
    /// zone for `a = 0`, rp1 in `zp0`'s for `a = 1` — as its zone, its index,
    /// and how far it has moved from its original position on the projection
    /// vector.
    fn reference_shift(&self, rp1: bool) -> Result<(usize, usize, i32), Error> {
        let (pointer, rp) = if rp1 {
            (0, self.gs.rp[1])
        } else {
            (1, self.gs.rp[2])
        };
        let zone = self.zone_at(pointer)?;
        let index = self.point_index(zone, rp as i32)?;
        let current = self.zones[zone].current[index];
        let original = self.zones[zone].original[index];
        let shift = project(
            self.gs.projection_vector,
            i64::from(current.x) - i64::from(original.x),
            i64::from(current.y) - i64::from(original.y),
        );
        Ok((zone, index, shift))
    }

    /// Executes the shift, alignment, interpolation or flip instruction `op`.
    pub(super) fn shift_or_flip(&mut self, op: u8) -> Result<(), Error> {
        match op {
            // SHP[a]: pops the loop count's points, in zp2's zone, and shifts
            // each "by the same amount that the reference point has been
            // shifted", from its current position.
            0x32 | 0x33 => {
                let (_, _, shift) = self.reference_shift(op == 0x33)?;
                let zone = self.zone_at(2)?;
                self.each_point(|vm, p| {
                    let index = vm.point_index(zone, p)?;
                    vm.move_by(zone, index, shift)
                })?;
            }
            // SHC[a]: pops a contour, in zp2's zone, and shifts every point on
            // it; "If the reference point is one of the points defining the
            // contour, the reference point is not moved". The twilight zone
            // has no contours.
            0x34 | 0x35 => {
                let c = self.pop()?;
                let (reference_zone, reference, shift) = self.reference_shift(op == 0x35)?;
                let zone = self.zone_at(2)?;
                let ends = &self.zones[zone].ends;
                let contour = usize::try_from(c)
                    .ok()
                    .filter(|&contour| contour < ends.len())
                    .ok_or(Error::ContourIndex(c))?;
                let first = if contour == 0 {
                    0
                } else {
                    ends[contour - 1] + 1
                };
                let last = ends[contour];
                self.shift_points(zone, first..=last, (reference_zone, reference), shift)?;
            }
            // SHZ[a]: pops a zone, 0 or 1, and shifts every point in it, the
            // reference point excepted as SHC excepts it, "similar to SHC,
            // but all points in the zone are shifted".
            0x36 | 0x37 => {
                let e = self.pop()?;
                let zone = u8::try_from(e)
                    .ok()
                    .filter(|&zone| zone <= 1)
                    .map(usize::from)
                    .ok_or(Error::ZoneIndex(e))?;
                let (reference_zone, reference, shift) = self.reference_shift(op == 0x37)?;
                let len = self.zones[zone].len();
                if len > 0 {
                    self.shift_points(zone, 0..=len - 1, (reference_zone, reference), shift)?;
                }
            }
            // SHPIX[ ]: pops an amount, then the loop count's points, in zp2's
            // zone, and moves each by the amount along the freedom vector:
            // "It makes no use of the projection_vector."
            0x38 => {
                let amount = i64::from(self.pop()?);
                let zone = self.zone_at(2)?;
                let freedom = self.gs.freedom_vector;
                let dx = super::fixed::round_div(amount * i64::from(freedom.x), 16_384);
                let dy = super::fixed::round_div(amount * i64::from(freedom.y), 16_384);
                self.each_point(|vm, p| {
                    let index = vm.point_index(zone, p)?;
                    let point = &mut vm.zones[zone].current[index];
                    point.x = (i64::from(point.x) + dx) as i32;
                    point.y = (i64::from(point.y) + dy) as i32;
                    vm.touch(zone, index);
                    Ok(())
                })?;
            }
            // ALIGNRP[ ]: pops the loop count's points, in zp1's zone, and
            // "Reduces the distance between rp0 and point p to zero", rp0 in
            // zp0's zone.
            0x3C => {
                let reference_zone = self.zone_at(0)?;
                let reference = self.point_index(reference_zone, self.gs.rp[0] as i32)?;
                let zone = self.zone_at(1)?;
                self.each_point(|vm, p| {
                    let index = vm.point_index(zone, p)?;
                    let to = vm.zones[reference_zone].current[reference];
                    let from = vm.zones[zone].current[index];
                    let d = project(
                        vm.gs.projection_vector,
                        i64::from(from.x) - i64::from(to.x),
                        i64::from(from.y) - i64::from(to.y),
                    );
                    vm.move_by(zone, index, d.wrapping_neg())
                })?;
            }
            // ALIGNPTS[ ]: pops p1, in zp1's zone, then p2, in zp0's, and moves
            // both "along the freedom_vector to the average of both their
            // projections along the projection_vector", that average rounded
            // half away from zero.
            0x27 => {
                let p1 = self.pop()?;
                let p2 = self.pop()?;
                let zone1 = self.zone_at(1)?;
                let index1 = self.point_index(zone1, p1)?;
                let zone2 = self.zone_at(0)?;
                let index2 = self.point_index(zone2, p2)?;
                let on = |vm: &Vm, zone: usize, index: usize| {
                    let point = vm.zones[zone].current[index];
                    i64::from(project(
                        vm.gs.projection_vector,
                        i64::from(point.x),
                        i64::from(point.y),
                    ))
                };
                let (c1, c2) = (on(self, zone1, index1), on(self, zone2, index2));
                let average = super::fixed::round_div(c1 + c2, 2);
                self.move_by(zone1, index1, (average - c1) as i32)?;
                self.move_by(zone2, index2, (average - c2) as i32)?;
            }
            // IP[ ]: pops the loop count's points, in zp2's zone, and moves
            // each "so that its relationship to rp1 and rp2 is the same as it
            // was in the original uninstructed outline", rp1 in zp0's zone and
            // rp2 in zp1's, the original distances on the dual projection
            // vector. "This instruction is not valid if rp1 and rp2 have the
            // same position on the projection_vector" in the original outline,
            // where the relationship is a ratio over nothing.
            0x39 => self.interpolate()?,
            // ISECT[ ]: pops b1, b0, a1, a0, then p; p in zp2's zone, line A
            // in zp1's and line B in zp0's. "Puts point p at the intersection
            // of the lines A and B", ignoring the freedom vector, so it is
            // touched on both axes; for parallel lines, "in the middle".
            0x0F => self.intersect()?,
            // UTP[ ]: pops p, in zp0's zone: "If the vector is set to the
            // x-axis, the point will be untouched in the x-direction. If the
            // vector is set to the y-axis, the point will be untouched in the
            // y-direction. Otherwise the point will be untouched in both."
            0x29 => {
                let p = self.pop()?;
                let zone = self.zone_at(0)?;
                let index = self.point_index(zone, p)?;
                let freedom = self.gs.freedom_vector;
                let clear = if freedom.y == 0 {
                    TOUCHED_X
                } else if freedom.x == 0 {
                    TOUCHED_Y
                } else {
                    TOUCHED_X | TOUCHED_Y
                };
                self.zones[zone].flags[index] &= !clear;
            }
            // FLIPPT[ ]: pops the loop count's points, in zp0's zone, and flips
            // each on or off the curve; "The point is not marked as touched."
            0x80 => {
                let zone = self.zone_at(0)?;
                self.each_point(|vm, p| {
                    let index = vm.point_index(zone, p)?;
                    vm.zones[zone].flags[index] ^= ON_CURVE;
                    Ok(())
                })?;
            }
            // FLIPRGON[ ], FLIPRGOFF[ ]: pops the highest point, then the
            // lowest, in zp0's zone, and puts every point from the lowest to
            // the highest on the curve, or off it; untouched. A highest point
            // below the lowest names no point, this engine's reading.
            0x81 | 0x82 => {
                let high = self.pop()?;
                let low = self.pop()?;
                let zone = self.zone_at(0)?;
                let high = self.point_index(zone, high)?;
                let low = self.point_index(zone, low)?;
                for index in low..=high {
                    self.spend(1)?;
                    let flags = &mut self.zones[zone].flags[index];
                    if op == 0x81 {
                        *flags |= ON_CURVE;
                    } else {
                        *flags &= !ON_CURVE;
                    }
                }
            }
            // AA[ ]: "Pops one argument from the stack. This instruction is
            // anachronistic and has no other effect."
            _ => {
                self.pop()?;
            }
        }
        Ok(())
    }

    /// Shifts points `range` of `zone` by `shift`, as `SHC` and `SHZ` do: the
    /// reference point, where it is one of them, stays; a step each.
    fn shift_points(
        &mut self,
        zone: usize,
        range: std::ops::RangeInclusive<usize>,
        reference: (usize, usize),
        shift: i32,
    ) -> Result<(), Error> {
        for index in range {
            if (zone, index) == reference {
                continue;
            }
            self.spend(1)?;
            self.move_by(zone, index, shift)?;
        }
        Ok(())
    }

    /// IP[ ].
    fn interpolate(&mut self) -> Result<(), Error> {
        let zone1 = self.zone_at(0)?;
        let rp1 = self.point_index(zone1, self.gs.rp[1] as i32)?;
        let zone2 = self.zone_at(1)?;
        let rp2 = self.point_index(zone2, self.gs.rp[2] as i32)?;
        let zone = self.zone_at(2)?;
        let (current1, original1) = (
            self.zones[zone1].current[rp1],
            self.zones[zone1].original[rp1],
        );
        let (current2, original2) = (
            self.zones[zone2].current[rp2],
            self.zones[zone2].original[rp2],
        );
        let dual = self.gs.dual_projection_vector;
        let projection = self.gs.projection_vector;
        let span = |a: i32, b: i32| i64::from(b) - i64::from(a);
        let original_span = project(
            dual,
            span(original1.x, original2.x),
            span(original1.y, original2.y),
        );
        if original_span == 0 {
            return Err(Error::CoincidentReferences);
        }
        let current_span = project(
            projection,
            span(current1.x, current2.x),
            span(current1.y, current2.y),
        );
        self.each_point(|vm, p| {
            let index = vm.point_index(zone, p)?;
            let point = vm.zones[zone].current[index];
            let original = vm.zones[zone].original[index];
            let from_rp1 = project(
                dual,
                span(original1.x, original.x),
                span(original1.y, original.y),
            );
            let now = project(
                projection,
                span(current1.x, point.x),
                span(current1.y, point.y),
            );
            // d3 : d4 = d1 : d2, the new distance from rp1 the original one
            // scaled by the references' spans.
            let wanted = super::fixed::round_div(
                i64::from(from_rp1) * i64::from(current_span),
                i64::from(original_span),
            );
            vm.move_by(zone, index, (wanted - i64::from(now)) as i32)
        })
    }

    /// ISECT[ ].
    fn intersect(&mut self) -> Result<(), Error> {
        let b1 = self.pop()?;
        let b0 = self.pop()?;
        let a1 = self.pop()?;
        let a0 = self.pop()?;
        let p = self.pop()?;
        let (a0, _) = self.point_at(1, a0)?;
        let (a1, _) = self.point_at(1, a1)?;
        let (b0, _) = self.point_at(0, b0)?;
        let (b1, _) = self.point_at(0, b1)?;
        let zone = self.zone_at(2)?;
        let index = self.point_index(zone, p)?;
        let wide = |v: i32| i128::from(v);
        let (ax, ay) = (wide(a1.x) - wide(a0.x), wide(a1.y) - wide(a0.y));
        let (bx, by) = (wide(b1.x) - wide(b0.x), wide(b1.y) - wide(b0.y));
        let denominator = ax * by - ay * bx;
        let (x, y) = if denominator == 0 {
            // "px = (a0x + a1x)/4 + (b0x + b1x)/4", the four taken together
            // and rounded half away from zero.
            let middle = |a: i32, b: i32, c: i32, d: i32| {
                round_div_wide(wide(a) + wide(b) + wide(c) + wide(d), 4)
            };
            (
                middle(a0.x, a1.x, b0.x, b1.x),
                middle(a0.y, a1.y, b0.y, b1.y),
            )
        } else {
            // a0 + t (a1 - a0), t = (b0 - a0) × (b1 - b0) / (a1 - a0) × (b1 - b0).
            let (cx, cy) = (wide(b0.x) - wide(a0.x), wide(b0.y) - wide(a0.y));
            let numerator = cx * by - cy * bx;
            (
                wide(a0.x) + round_div_wide(ax * numerator, denominator),
                wide(a0.y) + round_div_wide(ay * numerator, denominator),
            )
        };
        let zone = &mut self.zones[zone];
        zone.current[index].x = x as i32;
        zone.current[index].y = y as i32;
        zone.flags[index] |= TOUCHED_X | TOUCHED_Y;
        Ok(())
    }
}
