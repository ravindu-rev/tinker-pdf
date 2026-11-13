//! Measuring and moving points along the vectors: *Reading and writing
//! data*'s `GC`, `SCFS` and `MD`, and the movers of *Managing outlines* —
//! `MDAP`, `MIAP`, `MDRP`, `MIRP` and `MSIRP` — with their rounding, the
//! control value cut-in, the single width, the minimum distance and auto
//! flip, each touching the point it moves and setting the reference points
//! its section names (`docs/design/hinting.md`, "Measuring and moving").
//!
//! A distance is measured along the projection vector — along the dual
//! projection vector where an instruction measures the original outline —
//! and a point moves along the freedom vector by what changes its
//! projection by the distance. A point in the twilight zone that `MIAP`,
//! `MIRP`, `MSIRP` or `SCFS` moves takes its new position as its original
//! too, and `MIAP` and `MIRP` take such a point's original measurement to be
//! the control value's, as it has no outline to measure.

use super::fixed;
use super::state::{GraphicsState, Point, Vector, TOUCHED_X, TOUCHED_Y};
use super::vm::{Error, Vm};

/// `(dx, dy)` projected on `v`, in 26.6: `(dx · vx + dy · vy) / 16 384` in 64
/// bits, rounded half away from zero, then wrapped.
pub(super) fn project(v: Vector, dx: i64, dy: i64) -> i32 {
    fixed::round_div(dx * i64::from(v.x) + dy * i64::from(v.y), 16_384) as i32
}

/// `b - a`, each axis in 64 bits.
fn difference(a: Point, b: Point) -> (i64, i64) {
    (
        i64::from(b.x) - i64::from(a.x),
        i64::from(b.y) - i64::from(a.y),
    )
}

/// The distance `d` keeps at least `minimum`, in its own direction, or the
/// direction of `original` for a distance of zero, this engine's choice: "the
/// distance can be kept greater than or equal to the value established by
/// the minimum_distance state variable".
fn keep_minimum(d: i32, original: i32, minimum: i32) -> i32 {
    let positive = if d == 0 { original >= 0 } else { d > 0 };
    if positive {
        d.max(minimum)
    } else {
        d.min(minimum.wrapping_neg())
    }
}

/// `d` as the single width, with `d`'s sign, where its magnitude is within
/// the single width cut-in of it ("used when the absolute difference between
/// the single_width_value and the original value is smaller than this
/// single_width_cut_in"); `d` otherwise.
fn single_width(d: i32, value: i32, cut_in: i32) -> i32 {
    let magnitude = i64::from(d).abs();
    if (magnitude - i64::from(value)).abs() < i64::from(cut_in) {
        if d >= 0 {
            value
        } else {
            value.wrapping_neg()
        }
    } else {
        d
    }
}

impl Vm {
    /// Touches point `index` of `zone` along the axes the freedom vector
    /// moves it on: "Whenever an instruction has the effect of moving a point,
    /// that point is marked as touched in the x-direction or y-direction or
    /// both" (*Instructing TrueType Glyphs*).
    pub(super) fn touch(&mut self, zone: usize, index: usize) {
        let freedom = self.gs.freedom_vector;
        if let Some(flags) = self.zones[zone].flags.get_mut(index) {
            if freedom.x != 0 {
                *flags |= TOUCHED_X;
            }
            if freedom.y != 0 {
                *flags |= TOUCHED_Y;
            }
        }
    }

    /// Moves point `index` of `zone` along the freedom vector by what changes
    /// its projection on the projection vector by `distance`: `f · distance ·
    /// 16 384 / (f · p)`, each component rounded half away from zero, the
    /// coordinates wrapping; and touches it. A freedom vector orthogonal to
    /// the projection vector is an error: "When moving points, it is not
    /// valid for the freedom_vector and the projection_vector to be
    /// orthogonal."
    pub(super) fn move_by(
        &mut self,
        zone: usize,
        index: usize,
        distance: i32,
    ) -> Result<(), Error> {
        let f = self.gs.freedom_vector;
        let p = self.gs.projection_vector;
        let dot = i64::from(f.x) * i64::from(p.x) + i64::from(f.y) * i64::from(p.y);
        if dot == 0 {
            return Err(Error::OrthogonalVectors);
        }
        let scaled = i64::from(distance) * 16_384;
        let dx = fixed::round_div(i64::from(f.x) * scaled, dot);
        let dy = fixed::round_div(i64::from(f.y) * scaled, dot);
        if let Some(point) = self.zones[zone].current.get_mut(index) {
            point.x = (i64::from(point.x) + dx) as i32;
            point.y = (i64::from(point.y) + dy) as i32;
        }
        self.touch(zone, index);
        Ok(())
    }

    /// A twilight point a move set takes its position as its original too.
    fn settle_twilight(&mut self, zone: usize, index: usize) {
        if zone == 0 {
            if let (Some(&current), Some(original)) = (
                self.zones[0].current.get(index),
                self.zones[0].original.get_mut(index),
            ) {
                *original = current;
            }
        }
    }

    /// The zone and index of point `p` in the zone zone pointer `pointer`
    /// points at.
    fn locate(&self, pointer: usize, p: i32) -> Result<(usize, usize), Error> {
        let zone = self.zone_at(pointer)?;
        Ok((zone, self.point_index(zone, p)?))
    }

    /// The control value table's entry `n`.
    fn control_value(&self, n: i32) -> Result<i32, Error> {
        usize::try_from(n)
            .ok()
            .and_then(|index| self.cvt.get(index).copied())
            .ok_or(Error::CvtIndex(n))
    }

    /// Executes the measuring or moving instruction `op`.
    pub(super) fn measure_or_move(&mut self, op: u8) -> Result<(), Error> {
        match op {
            // GC[a]: pops p, in zp2's zone, and pushes its coordinate on the
            // projection vector: a = 0 its current position, a = 1 its
            // original one, on the dual projection vector.
            0x46 | 0x47 => {
                let p = self.pop()?;
                let (current, original) = self.point_at(2, p)?;
                let c = if op == 0x46 {
                    project(
                        self.gs.projection_vector,
                        i64::from(current.x),
                        i64::from(current.y),
                    )
                } else {
                    project(
                        self.gs.dual_projection_vector,
                        i64::from(original.x),
                        i64::from(original.y),
                    )
                };
                self.push(c)?;
            }
            // SCFS[ ]: pops a value, then p, in zp2's zone, and "Moves point p
            // from its current position along the freedom_vector so that its
            // component along the projection_vector becomes the value".
            0x48 => {
                let value = self.pop()?;
                let p = self.pop()?;
                let (zone, index) = self.locate(2, p)?;
                let current = self.zones[zone].current[index];
                let c = project(
                    self.gs.projection_vector,
                    i64::from(current.x),
                    i64::from(current.y),
                );
                self.move_by(zone, index, value.wrapping_sub(c))?;
                self.settle_twilight(zone, index);
            }
            // MD[a]: pops p1, in zp1's zone, then p2, in zp0's, and pushes the
            // distance from p2 to p1: a = 0 in the grid-fitted outline, a = 1
            // in the original one on the dual projection vector — as both
            // specifications' flag tables give it.
            0x49 | 0x4A => {
                let p1 = self.pop()?;
                let p2 = self.pop()?;
                let (current1, original1) = self.point_at(1, p1)?;
                let (current2, original2) = self.point_at(0, p2)?;
                let d = if op == 0x49 {
                    let (dx, dy) = difference(current2, current1);
                    project(self.gs.projection_vector, dx, dy)
                } else {
                    let (dx, dy) = difference(original2, original1);
                    project(self.gs.dual_projection_vector, dx, dy)
                };
                self.push(d)?;
            }
            // MDAP[a]: pops p, in zp0's zone; a = 1 rounds its coordinate on
            // the projection vector and moves it the rounded distance, a = 0
            // "simply marks the point as touched"; rp0 = rp1 = p.
            0x2E | 0x2F => {
                let p = self.pop()?;
                let (zone, index) = self.locate(0, p)?;
                if op == 0x2F {
                    let current = self.zones[zone].current[index];
                    let c = project(
                        self.gs.projection_vector,
                        i64::from(current.x),
                        i64::from(current.y),
                    );
                    let rounded = self.gs.round_state.apply(c);
                    self.move_by(zone, index, rounded.wrapping_sub(c))?;
                } else {
                    self.touch(zone, index);
                }
                self.gs.rp[0] = p as u32;
                self.gs.rp[1] = p as u32;
            }
            // MIAP[a]: pops a CVT entry number, then p, in zp0's zone, and moves
            // p to the CVT value's coordinate on the projection vector; a = 1
            // rounds it, and uses the point's original coordinate instead
            // where the two differ by more than the control value cut-in.
            // rp0 = rp1 = p.
            0x3E | 0x3F => {
                let n = self.pop()?;
                let p = self.pop()?;
                let cvt = self.control_value(n)?;
                let (zone, index) = self.locate(0, p)?;
                let point = self.zones[zone].current[index];
                let original = self.zones[zone].original[index];
                let target = if op == 0x3F {
                    let measured = if zone == 0 {
                        cvt
                    } else {
                        project(
                            self.gs.projection_vector,
                            i64::from(original.x),
                            i64::from(original.y),
                        )
                    };
                    let chosen = if (i64::from(cvt) - i64::from(measured)).abs()
                        > i64::from(self.gs.control_value_cut_in)
                    {
                        measured
                    } else {
                        cvt
                    };
                    self.gs.round_state.apply(chosen)
                } else {
                    cvt
                };
                let c = project(
                    self.gs.projection_vector,
                    i64::from(point.x),
                    i64::from(point.y),
                );
                self.move_by(zone, index, target.wrapping_sub(c))?;
                self.settle_twilight(zone, index);
                self.gs.rp[0] = p as u32;
                self.gs.rp[1] = p as u32;
            }
            // MSIRP[a]: pops a distance, then p, in zp1's zone, and makes the
            // distance from rp0, in zp0's, to p that distance: "the cut_in
            // does not affect the results of a MSIRP. Additionally, MSIRP is
            // unaffected by the round_state."
            0x3A | 0x3B => {
                let d = self.pop()?;
                let p = self.pop()?;
                self.relative_move(p, op & 1 == 1, true, |_, _| d)?;
            }
            // MDRP[abcde]: pops p, in zp1's zone, and keeps its distance from
            // rp0 the original outline's, on the dual projection vector — the
            // single width where it is within the single width cut-in, rounded
            // where c is set, kept to the minimum distance where b is set.
            0xC0..=0xDF => {
                let p = self.pop()?;
                let round = op & 0x04 != 0;
                let minimum = op & 0x08 != 0;
                self.relative_move(p, op & 0x10 != 0, false, |gs, measured| {
                    let original = measured.unwrap_or(0);
                    let mut d =
                        single_width(original, gs.single_width_value, gs.single_width_cut_in);
                    if round {
                        d = gs.round_state.apply(d);
                    }
                    if minimum {
                        d = keep_minimum(d, original, gs.minimum_distance);
                    }
                    d
                })?;
            }
            // MIRP[abcde]: pops a CVT entry number, then p, in zp1's zone, and
            // makes its distance from rp0 the CVT value — the single width
            // where it is within the single width cut-in, its sign the
            // original distance's under auto flip, and where c is set the
            // original distance instead if the two differ by more than the
            // control value cut-in, then rounded; kept to the minimum distance
            // where b is set.
            _ => {
                let n = self.pop()?;
                let p = self.pop()?;
                let cvt = self.control_value(n)?;
                let round = op & 0x04 != 0;
                let minimum = op & 0x08 != 0;
                self.relative_move(p, op & 0x10 != 0, true, |gs, measured| {
                    let mut cvt = single_width(cvt, gs.single_width_value, gs.single_width_cut_in);
                    // A twilight point has no outline to measure: its original
                    // distance is taken to be the CVT's.
                    let original = measured.unwrap_or(cvt);
                    if gs.auto_flip && (original >= 0) != (cvt >= 0) {
                        cvt = cvt.wrapping_neg();
                    }
                    let mut d = cvt;
                    if round {
                        if (i64::from(cvt) - i64::from(original)).abs()
                            > i64::from(gs.control_value_cut_in)
                        {
                            d = original;
                        }
                        d = gs.round_state.apply(d);
                    }
                    if minimum {
                        d = keep_minimum(d, original, gs.minimum_distance);
                    }
                    d
                })?;
            }
        }
        Ok(())
    }

    /// What `MSIRP`, `MDRP` and `MIRP` share: point `p` in `zp1`'s zone moved
    /// so its distance from `rp0`'s current position, in `zp0`'s zone, is
    /// what `distance` answers from the graphics state and the two points'
    /// original distance on the dual projection vector — `None` for a point
    /// a creating move (`MSIRP`, `MIRP`) sets in the twilight zone, which has
    /// no outline. Then "rp1 = rp0, rp2 = point p, and if a=1, rp0 is set to
    /// point p".
    fn relative_move(
        &mut self,
        p: i32,
        set_rp0: bool,
        creates: bool,
        distance: impl FnOnce(&GraphicsState, Option<i32>) -> i32,
    ) -> Result<(), Error> {
        let rp0 = self.gs.rp[0];
        let (zone, index) = self.locate(1, p)?;
        let (reference_zone, reference) = self.locate(0, rp0 as i32)?;
        let point = self.zones[zone].current[index];
        let original = self.zones[zone].original[index];
        let reference_point = self.zones[reference_zone].current[reference];
        let reference_original = self.zones[reference_zone].original[reference];
        let measured = if creates && zone == 0 {
            None
        } else {
            let (dx, dy) = difference(reference_original, original);
            Some(project(self.gs.dual_projection_vector, dx, dy))
        };
        let d = distance(&self.gs, measured);
        let (dx, dy) = difference(reference_point, point);
        let c = project(self.gs.projection_vector, dx, dy);
        self.move_by(zone, index, d.wrapping_sub(c))?;
        if creates {
            self.settle_twilight(zone, index);
        }
        self.gs.rp[1] = rp0;
        self.gs.rp[2] = p as u32;
        if set_rp0 {
            self.gs.rp[0] = p as u32;
        }
        Ok(())
    }
}
