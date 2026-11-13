//! The zones and their points, and the graphics state: the instructions of
//! *Managing graphics state* that set the projection, freedom and dual
//! vectors, the reference points, the zone pointers, the loop, the round
//! states and what rounds by them (`ROUND`, `NROUND`, `ODD`, `EVEN`), the
//! distance and delta settings, and the scan converter's dropout state
//! (`docs/design/hinting.md`, "Zones, points and the graphics state").

use super::fixed;
use super::vm::{Error, Vm};

/// A point's position, in 26.6.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Point {
    /// Across, in 26.6.
    pub x: i32,
    /// Up, in 26.6.
    pub y: i32,
}

/// A point's flags: on the curve, and touched along each axis.
pub(super) const ON_CURVE: u8 = 1;
pub(super) const TOUCHED_X: u8 = 2;
pub(super) const TOUCHED_Y: u8 = 4;

/// One of the two zones a program's points live in: zone 1 the glyph, its
/// contour points and the phantom points after them; zone 0 the twilight
/// zone, `maxTwilightPoints` points that belong to no contour.
///
/// Each point has a current position, which instructions move, an original
/// position — the unhinted point, which only a twilight point's move changes
/// — its on-curve flag, and whether it has been touched across and up.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Zone {
    pub(super) current: Vec<Point>,
    pub(super) original: Vec<Point>,
    pub(super) flags: Vec<u8>,
    /// Each contour's last point, increasing.
    pub(super) ends: Vec<usize>,
}

impl Zone {
    /// The twilight zone: `points` points at the origin, current and
    /// original, untouched and off the curve, in no contour.
    #[must_use]
    pub fn twilight(points: usize) -> Zone {
        Zone {
            current: vec![Point::default(); points],
            original: vec![Point::default(); points],
            flags: vec![0; points],
            ends: Vec::new(),
        }
    }

    /// A glyph's zone: its points, each both current and original, their
    /// on-curve flags, and each contour's last point. `None` where the flags
    /// and the points differ in number, or the ends do not increase or pass
    /// the last point. Points after the last contour — the phantom points —
    /// belong to none.
    #[must_use]
    pub fn glyph(points: &[Point], on_curve: &[bool], ends: &[usize]) -> Option<Zone> {
        Zone::with_positions(points, points, on_curve, ends)
    }

    /// A glyph's zone whose points stand somewhere other than their original
    /// positions — a composite's components, hinted, over their unhinted
    /// points — untouched; `None` as for [`Zone::glyph`], or where the two
    /// lists of positions differ in number.
    #[must_use]
    pub fn with_positions(
        current: &[Point],
        original: &[Point],
        on_curve: &[bool],
        ends: &[usize],
    ) -> Option<Zone> {
        if current.len() != original.len() || current.len() != on_curve.len() {
            return None;
        }
        if ends.windows(2).any(|pair| pair[0] >= pair[1])
            || ends.last().is_some_and(|&last| last >= current.len())
        {
            return None;
        }
        Some(Zone {
            current: current.to_vec(),
            original: original.to_vec(),
            flags: on_curve
                .iter()
                .map(|&on| if on { ON_CURVE } else { 0 })
                .collect(),
            ends: ends.to_vec(),
        })
    }

    /// How many points the zone holds.
    #[must_use]
    pub fn len(&self) -> usize {
        self.current.len()
    }

    /// Whether the zone holds no point.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.current.is_empty()
    }

    /// The points' current positions.
    #[must_use]
    pub fn current(&self) -> &[Point] {
        &self.current
    }

    /// The points' original positions.
    #[must_use]
    pub fn original(&self) -> &[Point] {
        &self.original
    }

    /// Whether point `i` is on the curve; `false` past the zone.
    #[must_use]
    pub fn on_curve(&self, i: usize) -> bool {
        self.flags
            .get(i)
            .is_some_and(|&flags| flags & ON_CURVE != 0)
    }

    /// Whether point `i` has been touched across and up; untouched past the
    /// zone.
    #[must_use]
    pub fn touched(&self, i: usize) -> (bool, bool) {
        let flags = self.flags.get(i).copied().unwrap_or(0);
        (flags & TOUCHED_X != 0, flags & TOUCHED_Y != 0)
    }

    /// Each contour's last point.
    #[must_use]
    pub fn contour_ends(&self) -> &[usize] {
        &self.ends
    }
}

/// A unit vector, its components in 2.14: 16 384 is 1.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Vector {
    /// Across, in 2.14.
    pub x: i32,
    /// Up, in 2.14.
    pub y: i32,
}

impl Vector {
    /// The x axis.
    pub const X: Vector = Vector { x: 16_384, y: 0 };
    /// The y axis.
    pub const Y: Vector = Vector { x: 0, y: 16_384 };

    /// The unit vector along `(dx, dy)`: each component `round(16 384 · d /
    /// sqrt(dx² + dy²))`, the squares, their sum and the root in `f64`, each
    /// correctly rounded by IEEE 754 so the same on every target (ruling 4).
    /// A difference of zero gives the x axis, this engine's choice.
    #[must_use]
    pub fn along(dx: i64, dy: i64) -> Vector {
        if dx == 0 && dy == 0 {
            return Vector::X;
        }
        let (x, y) = (dx as f64, dy as f64);
        let length = (x * x + y * y).sqrt();
        // `as` saturates, and each quotient lies within ±16 384.
        Vector {
            x: (16_384.0 * x / length).round() as i32,
            y: (16_384.0 * y / length).round() as i32,
        }
    }

    /// The vector turned a quarter counter-clockwise, as `SPVTL[1]`,
    /// `SFVTL[1]` and `SDPVTL[1]` turn the vector along their line.
    #[must_use]
    pub fn perpendicular(self) -> Vector {
        Vector {
            x: -self.y,
            y: self.x,
        }
    }
}

/// How distances round (*Instructing TrueType Glyphs*, "Controlling
/// rounding"). Each state is a period, a phase and a threshold (Apple's
/// manual, chapter 2, table 19), and rounding is symmetric about zero.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Round {
    /// `RTHG`: to the half grid — period 1, phase 1/2, threshold 1/2.
    HalfGrid,
    /// `RTG`: to the grid — period 1, threshold 1/2.
    Grid,
    /// `RTDG`: to the grid or the half grid — period 1/2, threshold 1/4.
    DoubleGrid,
    /// `RDTG`: down to the grid — period 1, threshold 0.
    DownToGrid,
    /// `RUTG`: up to the grid — period 1, threshold just under 1.
    UpToGrid,
    /// `ROFF`: no rounding.
    Off,
    /// `SROUND` and `S45ROUND`'s states, in 26.6.
    Super {
        /// The distance between rounded values.
        period: i32,
        /// Their offset from the multiples of the period.
        phase: i32,
        /// The part of the period before a rounded value that maps to it.
        threshold: i32,
    },
}

/// SROUND's grid period: one pixel.
const GRID: i32 = 64;
/// S45ROUND's grid period: "SQRT(2)/2 pixels rather than 1 pixel", 45.25
/// sixty-fourths, to the nearest 45.
const GRID_45: i32 = 45;
/// S45ROUND's three periods, `√2/4`, `√2/2` and `√2` pixels each rounded to
/// the nearest sixty-fourth: 22.63, 45.25 and 90.51.
const PERIODS_45: [i32; 3] = [23, 45, 91];

impl Round {
    /// `n` rounded by this state, with no engine compensation: for `n` at
    /// or above zero, *SROUND*'s steps — subtract the phase, add the
    /// threshold, truncate to the next lowest periodic value, add the phase
    /// back — and a positive value that became negative set to the positive
    /// round value closest to zero, the phase; a negative `n` rounds as its
    /// magnitude does, negated, "rounding is symmetric about zero" (Apple's
    /// manual, `ROUND[ab]`). A period of zero or less rounds nothing.
    #[must_use]
    pub fn apply(self, n: i32) -> i32 {
        let (period, phase, threshold) = match self {
            Round::Off => return n,
            Round::HalfGrid => (64, 32, 32),
            Round::Grid => (64, 0, 32),
            Round::DoubleGrid => (32, 0, 16),
            Round::DownToGrid => (64, 0, 0),
            Round::UpToGrid => (64, 0, 63),
            Round::Super {
                period,
                phase,
                threshold,
            } => (period, phase, threshold),
        };
        if period <= 0 {
            return n;
        }
        let (period, phase, threshold) =
            (i64::from(period), i64::from(phase), i64::from(threshold));
        let magnitude = i64::from(n).abs();
        let mut rounded = (magnitude - phase + threshold).div_euclid(period) * period + phase;
        if rounded < 0 {
            rounded = phase;
        }
        (if n < 0 { -rounded } else { rounded }) as i32
    }

    /// The state `SROUND` (or, `diagonal`, `S45ROUND`) sets from `n`'s low
    /// byte: the period in bits 7 and 6 (gridPeriod/2, gridPeriod,
    /// gridPeriod·2, and 3 reserved, an error); the phase in bits 5 and 4
    /// (0, period/4, period/2, gridPeriod·3/4 — the OpenType table's fourth
    /// entry, where Apple's manual reads period·3/4; the two agree at a
    /// period of one grid); the threshold in bits 3 to 0 (0 the period less
    /// one, "largest-number-smaller-than-period", and t the period times
    /// (t − 4)/8). Each part is taken half away from zero to a sixty-fourth.
    pub fn decode(n: i32, diagonal: bool) -> Result<Round, Error> {
        let byte = n & 0xFF;
        let (grid, periods) = if diagonal {
            (GRID_45, PERIODS_45)
        } else {
            (GRID, [GRID / 2, GRID, GRID * 2])
        };
        let period = match byte >> 6 {
            0 => periods[0],
            1 => periods[1],
            2 => periods[2],
            _ => return Err(Error::ReservedPeriod(n)),
        };
        let part =
            |numerator: i64, denominator: i64| fixed::round_div(numerator, denominator) as i32;
        let phase = match (byte >> 4) & 3 {
            0 => 0,
            1 => part(i64::from(period), 4),
            2 => part(i64::from(period), 2),
            _ => part(3 * i64::from(grid), 4),
        };
        let threshold = match byte & 0xF {
            0 => period - 1,
            t => part(i64::from(t - 4) * i64::from(period), 8),
        };
        Ok(Round::Super {
            period,
            phase,
            threshold,
        })
    }
}

/// The dropout control the scan converter applies to a glyph drawn whole or
/// empty: *SCANTYPE*'s rule 3, 4, 5 or 6 beside rules 1 and 2.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Dropout {
    /// Rules 5 and 6, "Smart" dropout control: the pixel nearer the midpoint
    /// of the two crossings, rather than rule 3 and 4's left-most across or
    /// bottom-most up.
    pub smart: bool,
    /// Rules 3 and 5: stubs too; rules 4 and 6 leave a stub out.
    pub stubs: bool,
}

/// The graphics state, as the *Graphics State Summary* names its variables;
/// [`Default`] gives each the default the summary states.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GraphicsState {
    /// Whether `MIRP` makes a CVT entry's sign the original distance's.
    pub auto_flip: bool,
    /// How far a CVT value and an outline measurement may differ for the CVT
    /// value to be used, in 26.6.
    pub control_value_cut_in: i32,
    /// The size `DELTAP1` and `DELTAC1`'s first exception applies at.
    pub delta_base: i32,
    /// The power of two a delta's step divides a pixel by.
    pub delta_shift: i32,
    /// The vector `SDPVTL` sets from two points' original positions, and
    /// every other instruction that sets the projection vector sets to it.
    pub dual_projection_vector: Vector,
    /// The direction points move in.
    pub freedom_vector: Vector,
    /// The direction distances are measured along.
    pub projection_vector: Vector,
    /// The flags `INSTCTRL` sets: bit 0 inhibits grid-fitting, bit 1 ignores
    /// the control value program's settings, bit 2 asks for native ClearType.
    pub instruct_control: u8,
    /// How many times the next loop-counted instruction runs.
    pub loop_count: i32,
    /// The least distance `MDRP` and `MIRP` keep, in 26.6.
    pub minimum_distance: i32,
    /// How distances round.
    pub round_state: Round,
    /// The three reference points, `rp0` to `rp2`.
    pub rp: [u32; 3],
    /// The three zone pointers, `zp0` to `zp2`: 0 the twilight zone, 1 the
    /// glyph.
    pub zp: [u8; 3],
    /// `SCANCTRL`'s flags: a threshold in bits 0 to 7, conditions in bits 8
    /// to 13.
    pub scan_control: u16,
    /// `SCANTYPE`'s mode.
    pub scan_type: i32,
    /// How near a distance must be to the single width for it to be used,
    /// in 26.6.
    pub single_width_cut_in: i32,
    /// The single width, in 26.6 at the size.
    pub single_width_value: i32,
    /// `SANGW`'s weight, which no instruction this engine executes reads.
    pub angle_weight: i32,
}

impl Default for GraphicsState {
    fn default() -> GraphicsState {
        GraphicsState {
            auto_flip: true,
            control_value_cut_in: 68,
            delta_base: 9,
            delta_shift: 3,
            dual_projection_vector: Vector::X,
            freedom_vector: Vector::X,
            projection_vector: Vector::X,
            instruct_control: 0,
            loop_count: 1,
            minimum_distance: 64,
            round_state: Round::Grid,
            rp: [0; 3],
            zp: [1; 3],
            scan_control: 0,
            scan_type: 0,
            single_width_cut_in: 0,
            single_width_value: 0,
            angle_weight: 0,
        }
    }
}

impl GraphicsState {
    /// The dropout control a glyph of `ppem` whole pixels per em is filled
    /// with, by *SCANCTRL*'s flags and *SCANTYPE*'s mode; `None` for rules 1
    /// and 2 alone. The threshold in bits 0 to 7 holds at a size up to it,
    /// at every size for 0xFF and at none for 0; bit 8 turns dropout control
    /// on where the threshold holds, and bit 11 off where it does not; bits
    /// 9 and 10 turn it on for a rotated or stretched glyph, and bits 12 and
    /// 13 off for one that is not, and a hinted glyph is never either.
    /// Modes 0, 1, 4 and 5 add rules 3, 4, 5 and 6; every other mode is
    /// rules 1 and 2.
    #[must_use]
    pub fn dropout(&self, ppem: i32) -> Option<Dropout> {
        let flags = self.scan_control;
        let threshold = i32::from(flags & 0xFF);
        let holds = threshold == 0xFF || (threshold != 0 && ppem <= threshold);
        let set = |bit: u16| flags & (1 << bit) != 0;
        // Never rotated or stretched: bits 9 and 10 turn nothing on, and bits
        // 12 and 13 always turn it off.
        let on = set(8) && holds;
        let off = (set(11) && !holds) || set(12) || set(13);
        if !on || off {
            return None;
        }
        match self.scan_type {
            0 => Some(Dropout {
                smart: false,
                stubs: true,
            }),
            1 => Some(Dropout {
                smart: false,
                stubs: false,
            }),
            4 => Some(Dropout {
                smart: true,
                stubs: true,
            }),
            5 => Some(Dropout {
                smart: true,
                stubs: false,
            }),
            _ => None,
        }
    }
}

impl Vm {
    /// The zone zone pointer `pointer` (0 to 2) points at, as an index into
    /// the zones.
    pub(super) fn zone_at(&self, pointer: usize) -> Result<usize, Error> {
        let zone = self.gs.zp.get(pointer).copied().unwrap_or(u8::MAX);
        match zone {
            0 | 1 => Ok(usize::from(zone)),
            other => Err(Error::ZoneIndex(i32::from(other))),
        }
    }

    /// The index point `p` of `zone` stands at.
    pub(super) fn point_index(&self, zone: usize, p: i32) -> Result<usize, Error> {
        let len = self.zones.get(zone).map_or(0, Zone::len);
        usize::try_from(p)
            .ok()
            .filter(|&index| index < len)
            .ok_or(Error::PointIndex(p))
    }

    /// Point `p`'s current and original positions, in the zone zone pointer
    /// `pointer` points at.
    pub(super) fn point_at(&self, pointer: usize, p: i32) -> Result<(Point, Point), Error> {
        let zone = self.zone_at(pointer)?;
        let index = self.point_index(zone, p)?;
        let zone = &self.zones[zone];
        Ok((zone.current[index], zone.original[index]))
    }

    /// Executes the graphics state instruction `op`.
    pub(super) fn set_state(&mut self, op: u8) -> Result<(), Error> {
        match op {
            // SVTCA[a], SPVTCA[a], SFVTCA[a]: a = 0 the y axis, 1 the x axis.
            0x00..=0x05 => {
                let axis = if op & 1 == 1 { Vector::X } else { Vector::Y };
                if op <= 0x01 || op >= 0x04 {
                    self.gs.freedom_vector = axis;
                }
                if op <= 0x03 {
                    self.set_projection(axis);
                }
            }
            // SPVTL[a], SFVTL[a], SDPVTL[a]: pops p1, then p2, p1 in the zone
            // zp2 points at and p2 in zp1's; the vector points "from p1 toward
            // p2", and a = 1 turns it a quarter counter-clockwise.
            0x06..=0x09 | 0x86 | 0x87 => {
                let p1 = self.pop()?;
                let p2 = self.pop()?;
                let (current1, original1) = self.point_at(2, p1)?;
                let (current2, original2) = self.point_at(1, p2)?;
                let along = |from: Point, to: Point| {
                    let v = Vector::along(
                        i64::from(to.x) - i64::from(from.x),
                        i64::from(to.y) - i64::from(from.y),
                    );
                    if op & 1 == 1 {
                        v.perpendicular()
                    } else {
                        v
                    }
                };
                let current = along(current1, current2);
                match op {
                    0x06 | 0x07 => self.set_projection(current),
                    0x08 | 0x09 => self.gs.freedom_vector = current,
                    // SDPVTL: "uses coordinates from the scaled outline before
                    // any grid-fitting took place" for the dual vector, and
                    // the current positions for the projection vector.
                    _ => {
                        self.gs.projection_vector = current;
                        self.gs.dual_projection_vector = along(original1, original2);
                    }
                }
            }
            // SPVFS[ ], SFVFS[ ]: pops y, then x, each a 2.14 value in the
            // low 16 bits, taken as given.
            0x0A | 0x0B => {
                let y = self.pop()?;
                let x = self.pop()?;
                let v = Vector {
                    x: i32::from(x as i16),
                    y: i32::from(y as i16),
                };
                if op == 0x0A {
                    self.set_projection(v);
                } else {
                    self.gs.freedom_vector = v;
                }
            }
            // GPV[ ], GFV[ ]: pushes x, then y.
            0x0C | 0x0D => {
                let v = if op == 0x0C {
                    self.gs.projection_vector
                } else {
                    self.gs.freedom_vector
                };
                self.push(v.x)?;
                self.push(v.y)?;
            }
            // SFVTPV[ ].
            0x0E => self.gs.freedom_vector = self.gs.projection_vector,
            // SRP0[ ] to SRP2[ ]: pops a point number, checked where it is
            // used.
            0x10..=0x12 => {
                let p = self.pop()?;
                self.gs.rp[usize::from(op - 0x10)] = p as u32;
            }
            // SZP0[ ] to SZP2[ ], SZPS[ ]: pops a zone number, 0 or 1: "Any
            // other value for n is an error."
            0x13..=0x16 => {
                let n = self.pop()?;
                let zone = u8::try_from(n)
                    .ok()
                    .filter(|&zone| zone <= 1)
                    .ok_or(Error::ZoneIndex(n))?;
                if op == 0x16 {
                    self.gs.zp = [zone; 3];
                } else {
                    self.gs.zp[usize::from(op - 0x13)] = zone;
                }
            }
            // SLOOP[ ]: "Setting the loop variable to zero is an error"
            // (Apple's manual), as a count below it is.
            0x17 => {
                let n = self.pop()?;
                if n < 1 {
                    return Err(Error::LoopCount(n));
                }
                self.gs.loop_count = n;
            }
            0x18 => self.gs.round_state = Round::Grid,
            0x19 => self.gs.round_state = Round::HalfGrid,
            0x3D => self.gs.round_state = Round::DoubleGrid,
            0x7D => self.gs.round_state = Round::DownToGrid,
            0x7C => self.gs.round_state = Round::UpToGrid,
            0x7A => self.gs.round_state = Round::Off,
            // SROUND[ ], S45ROUND[ ]: pops n.
            0x76 | 0x77 => {
                let n = self.pop()?;
                self.gs.round_state = Round::decode(n, op == 0x77)?;
            }
            // ROUND[ab]: n1 compensated for its distance type, then rounded;
            // NROUND[ab] compensated only. This engine's compensation is zero
            // for every distance type, so NROUND gives n1 back.
            0x68..=0x6F => {
                let n = self.pop()?;
                let n = if op <= 0x6B {
                    self.gs.round_state.apply(n)
                } else {
                    n
                };
                self.push(n)?;
            }
            // ODD[ ], EVEN[ ]: e1 rounded by the round state, "shifted from a
            // fixed-point value to an integer value (any fractional values
            // are ignored)", then tested.
            0x56 | 0x57 => {
                let n = self.pop()?;
                let odd = (self.gs.round_state.apply(n) >> 6) & 1 == 1;
                self.push(i32::from(odd == (op == 0x56)))?;
            }
            // SMD[ ], SCVTCI[ ], SSWCI[ ]: pops a 26.6 value.
            0x1A => self.gs.minimum_distance = self.pop()?,
            0x1D => self.gs.control_value_cut_in = self.pop()?,
            0x1E => self.gs.single_width_cut_in = self.pop()?,
            // SSW[ ]: pops a value in font units, which "The interpreter will
            // convert ... to pixels (F26Dot6)".
            0x1F => {
                let n = self.pop()?;
                self.gs.single_width_value = self.size.scale(n);
            }
            0x4D => self.gs.auto_flip = true,
            0x4E => self.gs.auto_flip = false,
            // SANGW[ ]: "Except for popping a single stack element, it has no
            // effect"; the weight is kept.
            0x7E => self.gs.angle_weight = self.pop()?,
            0x5E => self.gs.delta_base = self.pop()?,
            0x5F => self.gs.delta_shift = self.pop()?,
            // SCANCTRL[ ]: a 16-bit word of flags.
            0x85 => self.gs.scan_control = self.pop()? as u16,
            // SCANTYPE[ ].
            _ => self.gs.scan_type = self.pop()?,
        }
        Ok(())
    }

    /// Sets the projection vector, and the dual projection vector with it:
    /// "The dual_projection_vector will disappear when any other instruction
    /// that sets the projection_vector is used."
    fn set_projection(&mut self, v: Vector) {
        self.gs.projection_vector = v;
        self.gs.dual_projection_vector = v;
    }
}
