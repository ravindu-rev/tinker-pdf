//! Filled outline glyphs drawn from position-independent masks
//! (`docs/design/performance.md`, P-06).
//!
//! A glyph's device transform is split into what its shape depends on and
//! where it stands. The shape is the em-to-device 2x2, quantised to
//! [`MATRIX_STEPS`] steps a pixel an em, and the origin's sub-pixel phase,
//! quantised to [`PHASES`] steps a pixel on each axis; the outline is
//! rasterised under those alone, in a frame whose pixel (0, 0) is the one the
//! origin falls in. Where it stands is that pixel, a whole number in the
//! page's frame, and placing the mask there is an integer offset that cannot
//! round. So the same glyph at the same phase draws the same mask wherever it
//! is on the page, which is what lets P-06.3 keep one.
//!
//! **Ruling 5.** The phase is taken from the origin in the page's frame —
//! `Renderer::base` is never translated, so a tile, a group's buffer and the
//! page all compute the same number — and a canvas subtracts its corner only
//! when it indexes the placed mask. A glyph on a phase boundary therefore
//! rounds the same way in a tile and on the page under it.

use tinker_pdf_content::Matrix;
use tinker_pdf_font::{Outline, Segment};
use tinker_pdf_raster::{
    fill::{fill, Mask},
    geom::{FillRule, Path},
};

/// Sub-pixel phases a pixel holds, on each axis: an origin's fraction of a
/// pixel is rounded to the nearest sixteenth, so a glyph moves at most a
/// thirty-second of a pixel along each axis, and one shape has up to
/// 16 × 16 = 256 phases.
pub(crate) const PHASES: u8 = 16;

/// Steps a device pixel an em is quantised to: each entry of the 2x2 is
/// rounded to the nearest 1/256 of a pixel an em, so a point one em from the
/// origin moves at most 1/256 of a pixel, one unit of the scanline's own
/// horizontal resolution.
pub(crate) const MATRIX_STEPS: f64 = 256.0;

/// The most pixels a glyph's mask may hold. A glyph whose mask would hold
/// more keeps the path route, which rasterises only the pixels its clip and
/// its canvas can show: a heading's glyph is worth no mask, and a glyph far
/// larger than its page must not be rasterised whole.
pub(crate) const MAX_GLYPH_MASK_PIXELS: u64 = 1 << 16;

/// The pixels a mask reaches past its outline's bounds on every side, as the
/// path route's paint region does (`SLACK` in `lib.rs`): the flattener may
/// round an ulp past an extreme control point, and an extreme on a pixel
/// boundary is the common case for a glyph.
const SLACK: i64 = 1;

/// A glyph's shape and where it stands, quantised.
///
/// The 2x2 and the phase are everything a mask depends on besides its outline;
/// the origin is where it is placed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) struct Placement {
    /// The em-to-device 2x2, `a`, `b`, `c` and `d`, in steps of
    /// 1 / [`MATRIX_STEPS`] of a pixel an em.
    pub(crate) matrix: [i32; 4],
    /// The origin's phase on each axis, in steps of 1 / [`PHASES`] of a pixel:
    /// `0` to `PHASES - 1`.
    pub(crate) phase: [u8; 2],
    /// The page-frame pixel the phase is measured from.
    pub(crate) origin: [i32; 2],
}

impl Placement {
    /// The placement of a glyph whose em square `combined` maps to the page's
    /// device pixels.
    ///
    /// `None` when an entry is not finite or does not fit once quantised —
    /// a 2x2 past `i32::MAX` steps, an origin outside `i32` — which leaves the
    /// glyph to the path route. `floor` and `round` are correctly rounded by
    /// IEEE 754, and an origin's fraction `e - floor(e)` is exact, so the
    /// placement is the same on every target (ruling 4).
    pub(crate) fn of(combined: &Matrix) -> Option<Placement> {
        let step = |value: f64| {
            let steps = (value * MATRIX_STEPS).round();
            (steps.is_finite() && steps.abs() <= f64::from(i32::MAX)).then_some(steps as i32)
        };
        let phase = |value: f64| {
            if !value.is_finite() {
                return None;
            }
            let whole = value.floor();
            let steps = ((value - whole) * f64::from(PHASES)).round();
            // A fraction that rounds up to a whole pixel is the next pixel's
            // phase 0, so the sixteen phases are the only sixteen.
            let (whole, steps) = if steps >= f64::from(PHASES) {
                (whole + 1.0, 0.0)
            } else {
                (whole, steps)
            };
            (whole.abs() <= f64::from(i32::MAX)).then_some((whole as i32, steps as u8))
        };
        let (left, x) = phase(combined.e)?;
        let (top, y) = phase(combined.f)?;
        Some(Placement {
            matrix: [
                step(combined.a)?,
                step(combined.b)?,
                step(combined.c)?,
                step(combined.d)?,
            ],
            phase: [x, y],
            origin: [left, top],
        })
    }

    /// The transform a mask is rasterised under: the quantised 2x2 and the
    /// phase, with the origin's whole pixel taken off. Every entry is a
    /// dyadic fraction a few bits long, so each is the number it names.
    fn local(&self) -> Matrix {
        let entry = |steps: i32| f64::from(steps) / MATRIX_STEPS;
        let phase = |steps: u8| f64::from(steps) / f64::from(PHASES);
        Matrix {
            a: entry(self.matrix[0]),
            b: entry(self.matrix[1]),
            c: entry(self.matrix[2]),
            d: entry(self.matrix[3]),
            e: phase(self.phase[0]),
            f: phase(self.phase[1]),
        }
    }
}

/// A glyph's coverage, rasterised in its own frame: the mask's pixel (0, 0)
/// is the page-frame pixel at [`Placement::origin`].
///
/// Filled non-zero, as 9.3.6 fills a glyph. `antialias` off hardens the mask
/// as the path route's `coverage` does, before any clip multiplies into it.
/// `None` when the mask would hold more than [`MAX_GLYPH_MASK_PIXELS`] or
/// stand outside `i32`, which leaves the glyph to the path route; a function
/// of the outline and the placement alone, so a tile and the page under it
/// take the same route.
pub(crate) fn rasterise(
    outline: &Outline,
    placement: &Placement,
    tolerance: f64,
    antialias: bool,
    stop: Option<&dyn Fn() -> bool>,
) -> Option<Mask> {
    let local = placement.local();
    let mut path = Path::new();
    for segment in &outline.segments {
        match *segment {
            Segment::MoveTo { x, y } => {
                let (x, y) = local.apply(x, y);
                path.move_to(x, y);
            }
            Segment::LineTo { x, y } => {
                let (x, y) = local.apply(x, y);
                path.line_to(x, y);
            }
            Segment::QuadTo { cx, cy, x, y } => {
                let (cx, cy) = local.apply(cx, cy);
                let (x, y) = local.apply(x, y);
                path.quad_to(cx, cy, x, y);
            }
            Segment::CurveTo {
                c1x,
                c1y,
                c2x,
                c2y,
                x,
                y,
            } => {
                let (c1x, c1y) = local.apply(c1x, c1y);
                let (c2x, c2y) = local.apply(c2x, c2y);
                let (x, y) = local.apply(x, y);
                path.curve_to(c1x, c1y, c2x, c2y, x, y);
            }
            Segment::Close => path.close(),
        }
    }
    let Some((x0, y0, x1, y1)) = path.bounds() else {
        return Some(Mask::empty(0, 0, 0, 0));
    };
    // Saturating, because a point a hostile font puts at 1e300 saturates to
    // `i64::MAX` and the slack has to be added past it without wrapping.
    let left = (x0.floor() as i64).saturating_sub(SLACK);
    let top = (y0.floor() as i64).saturating_sub(SLACK);
    let right = (x1.ceil() as i64).saturating_add(SLACK);
    let bottom = (y1.ceil() as i64).saturating_add(SLACK);
    let width = right.saturating_sub(left);
    let height = bottom.saturating_sub(top);
    let pixels = u64::try_from(width)
        .ok()?
        .checked_mul(u64::try_from(height).ok()?)?;
    if pixels > MAX_GLYPH_MASK_PIXELS {
        return None;
    }
    let left = i32::try_from(left).ok()?;
    let top = i32::try_from(top).ok()?;
    // Both are at most `MAX_GLYPH_MASK_PIXELS`, so both fit.
    let width = u32::try_from(width).ok()?;
    let height = u32::try_from(height).ok()?;
    let mut mask = fill(
        &path,
        FillRule::NonZero,
        left,
        top,
        width,
        height,
        tolerance,
        stop,
    );
    if !antialias {
        mask.harden();
    }
    Some(mask)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(e: f64, f: f64) -> Matrix {
        Matrix {
            a: 12.5,
            b: 0.0,
            c: 0.0,
            d: -12.5,
            e,
            f,
        }
    }

    #[test]
    fn an_origin_is_its_whole_pixel_and_the_nearest_sixteenth() {
        let placement = Placement::of(&at(10.3, 20.6)).expect("a placement");
        assert_eq!(placement.origin, [10, 20]);
        assert_eq!(
            placement.phase,
            [5, 10],
            "0.3 is 4.8 sixteenths and 0.6 is 9.6, to the nearest"
        );
        assert_eq!(placement.matrix, [3200, 0, 0, -3200]);
    }

    #[test]
    fn each_sixteenth_of_a_pixel_is_its_own_phase() {
        for k in 0..16u8 {
            let fraction = f64::from(k) / 16.0;
            let placement = Placement::of(&at(5.0 + fraction, -3.0 + fraction)).expect("placed");
            assert_eq!(placement.origin, [5, -3], "sixteenth {k}");
            assert_eq!(placement.phase, [k, k], "sixteenth {k}");
            // Just short of the midpoint to the next sixteenth stays here.
            let near = fraction + 1.0 / 32.0 - 1.0 / 1024.0;
            let placement = Placement::of(&at(5.0 + near, 0.0)).expect("placed");
            assert_eq!(placement.phase[0], k, "below the midpoint after {k}");
        }
    }

    #[test]
    fn a_fraction_that_rounds_up_is_the_next_pixels_first_phase() {
        // 10.98 is 10 and 0.98, -0.02 is -1 and 0.98: both fractions are
        // 15.68 sixteenths, which round to sixteen, the next pixel.
        let placement = Placement::of(&at(10.98, -0.02)).expect("a placement");
        assert_eq!(placement.origin, [11, 0]);
        assert_eq!(placement.phase, [0, 0]);
        // And -0.98 is -1 and 0.02, 0.32 of a sixteenth, which rounds down
        // to phase 0.
        let placement = Placement::of(&at(3.0, -0.98)).expect("a placement");
        assert_eq!(placement.origin, [3, -1]);
        assert_eq!(placement.phase, [0, 0]);
    }

    #[test]
    fn a_whole_pixel_origin_has_phase_zero_on_both_axes() {
        let placement = Placement::of(&at(-7.0, 33.0)).expect("a placement");
        assert_eq!(placement.origin, [-7, 33]);
        assert_eq!(placement.phase, [0, 0]);
    }

    #[test]
    fn a_transform_that_is_not_finite_or_does_not_fit_has_no_placement() {
        assert_eq!(Placement::of(&at(f64::NAN, 0.0)), None);
        assert_eq!(Placement::of(&at(1e12, 0.0)), None);
        let mut huge = at(0.0, 0.0);
        huge.a = 1e10;
        assert_eq!(Placement::of(&huge), None);
    }

    #[test]
    fn two_matrices_an_ulp_apart_quantise_alike() {
        let mut nudged = at(4.0, 4.0);
        nudged.a = f64::from_bits(12.5f64.to_bits() + 1);
        assert_eq!(Placement::of(&nudged), Placement::of(&at(4.0, 4.0)));
    }

    /// A square of one em, whose mask at a scale of 12.5 is 12.5 pixels a
    /// side plus the slack.
    fn square() -> Outline {
        Outline {
            segments: vec![
                Segment::MoveTo { x: 0.0, y: 0.0 },
                Segment::LineTo { x: 1.0, y: 0.0 },
                Segment::LineTo { x: 1.0, y: 1.0 },
                Segment::LineTo { x: 0.0, y: 1.0 },
                Segment::Close,
            ],
        }
    }

    #[test]
    fn a_mask_too_large_for_the_route_is_declined() {
        let mut big = at(0.0, 0.0);
        big.a = 400.0;
        big.d = -400.0;
        let placement = Placement::of(&big).expect("a placement");
        assert!(
            rasterise(&square(), &placement, 0.2, true, None).is_none(),
            "a 400-pixel square is past the bound and keeps the path route"
        );
        let placement = Placement::of(&at(0.0, 0.0)).expect("a placement");
        let mask = rasterise(&square(), &placement, 0.2, true, None).expect("a mask");
        assert_eq!((mask.width, mask.height), (15, 15));
    }

    #[test]
    fn hardening_leaves_every_pixel_whole_or_empty() {
        let placement = Placement::of(&at(0.25, 0.5)).expect("a placement");
        let mask = rasterise(&square(), &placement, 0.2, false, None).expect("a mask");
        assert!(mask.data.iter().all(|v| *v == 0 || *v == 255));
        assert!(mask.data.contains(&255));
    }
}
