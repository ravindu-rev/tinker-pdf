//! Scanline filling with anti-aliasing.
//!
//! Coverage is computed by sampling each pixel row at a fixed number of
//! sub-scanlines and accumulating in **integer** arithmetic. That is a
//! deliberate choice over analytic exact-area coverage: the sample count is
//! fixed, the accumulator is integral, and no platform's floating-point
//! rounding or fused multiply-add can change the result. Ruling 4 makes
//! bit-identical output across linux, windows, macOS and wasm a contract, and
//! this is what pays for it.
//!
//! The cost is that a nearly-horizontal edge quantizes to sixteen levels
//! vertically. At the sample count used the difference from exact-area
//! coverage is below one 8-bit level for every angle, so it is invisible and
//! reproducible, which is the trade worth making.

use core::ops::Range;

use crate::geom::{flatten, FillRule, Path, Point};

/// Sub-scanlines per pixel row. Sixteen gives 4-bit vertical resolution,
/// combined with exact horizontal spans for 8-bit total coverage.
const SAMPLES: i32 = 16;

/// Rows of the sweep between one cancellation check and the next.
///
/// Asking every row would put a branch in the hottest loop in the engine.
/// Asking every sixteenth amortises it to a sixteenth of that while keeping
/// the promise the render layer's documentation makes, which is scanline-band
/// granularity rather than per-pixel: sixteen rows of a 300 dpi US Letter page
/// is 40 800 pixels, tens of microseconds between one answer and the next.
///
/// A constant, deliberately. **The number cannot change a pixel** — the
/// predicate decides only *whether the sweep continues*, never what a
/// continued row computes — so it is free to be tuned without re-baselining
/// anything, and the seven determinism fingerprints are indifferent to it.
/// The count is of *iterations*, not of row indices, so the first row of every
/// fill is checked whatever part of the region the shape starts in.
const STOP_EVERY: usize = 16;

/// A coverage mask over a rectangular region.
#[derive(Clone, Debug)]
pub struct Mask {
    /// Left edge in device pixels.
    pub x0: i32,
    /// Top edge in device pixels.
    pub y0: i32,
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// Coverage, 0 to 255, row-major.
    pub data: Vec<u8>,
}

impl Mask {
    /// An all-zero mask.
    #[must_use]
    pub fn empty(x0: i32, y0: i32, width: u32, height: u32) -> Mask {
        let len = (width as usize).saturating_mul(height as usize);
        Mask {
            x0,
            y0,
            width,
            height,
            data: vec![0; len],
        }
    }

    /// The canvas pixels this mask can put coverage on: its own rectangle,
    /// clamped to a canvas of `width` by `height`, as `(x0, y0, x1, y1)` with
    /// the far edges exclusive.
    ///
    /// A mask reports zero outside itself, so a consumer that walks the whole
    /// canvas computes the same answer as one that walks this — it just pays
    /// the page for a comma. Every consumer walks this.
    #[must_use]
    pub fn overlap(&self, width: u32, height: u32) -> (u32, u32, u32, u32) {
        let clamp = |value: i64, limit: u32| value.clamp(0, i64::from(limit)) as u32;
        let x0 = clamp(i64::from(self.x0), width);
        let y0 = clamp(i64::from(self.y0), height);
        let x1 = clamp(i64::from(self.x0) + i64::from(self.width), width);
        let y1 = clamp(i64::from(self.y0) + i64::from(self.height), height);
        (x0, y0, x1.max(x0), y1.max(y0))
    }

    /// Coverage at a device pixel, zero outside the mask.
    #[must_use]
    pub fn at(&self, x: i32, y: i32) -> u8 {
        let (Some(col), Some(row)) = (x.checked_sub(self.x0), y.checked_sub(self.y0)) else {
            return 0;
        };
        if col < 0 || row < 0 || col as u32 >= self.width || row as u32 >= self.height {
            return 0;
        }
        let index = (row as usize) * (self.width as usize) + (col as usize);
        self.data.get(index).copied().unwrap_or(0)
    }

    /// Intersects with another mask, multiplying coverages.
    ///
    /// How clipping composes: a clip stack is the product of its masks, and
    /// multiplying is what makes a clipped anti-aliased edge look right rather
    /// than doubly-hard.
    ///
    /// The result covers only the rectangle the two have in common, because
    /// the product is zero everywhere else by construction — one of the two
    /// factors is outside itself there and reports zero. A larger mask would
    /// carry the same answer at a cost proportional to the page.
    #[must_use]
    pub fn intersect(&self, other: &Mask) -> Mask {
        let (x0, y0, width, height) = common_rect(self, other);
        let mut out = Mask::empty(x0, y0, width, height);
        for row in 0..height {
            let y = y0.saturating_add(row as i32);
            for col in 0..width {
                let x = x0.saturating_add(col as i32);
                let value = product(self.at(x, y), other.at(x, y));
                let index = (row as usize) * (width as usize) + (col as usize);
                if let Some(slot) = out.data.get_mut(index) {
                    *slot = value;
                }
            }
        }
        out
    }

    /// Multiplies `other`'s coverage into this mask, keeping this one's
    /// rectangle.
    ///
    /// The same arithmetic as [`Mask::intersect`] without the second
    /// allocation, for the caller that already knows its mask is no larger
    /// than the clip it is about to be multiplied by — which is every paint,
    /// since a paint asks for the region its path and the clip have in common
    /// before it rasterizes anything.
    pub fn intersect_in_place(&mut self, other: &Mask) {
        for row in 0..self.height {
            let y = self.y0.saturating_add(row as i32);
            let base = (row as usize) * (self.width as usize);
            for col in 0..self.width {
                let x = self.x0.saturating_add(col as i32);
                let Some(slot) = self.data.get_mut(base + col as usize) else {
                    continue;
                };
                *slot = product(*slot, other.at(x, y));
            }
        }
    }
}

/// Two coverages multiplied, rounded rather than truncated so that full
/// coverage stays full.
fn product(a: u8, b: u8) -> u8 {
    ((u32::from(a) * u32::from(b) + 127) / 255) as u8
}

/// The rectangle two masks have in common, as `(x0, y0, width, height)`.
fn common_rect(a: &Mask, b: &Mask) -> (i32, i32, u32, u32) {
    let span = |x0: i32, width: u32| (i64::from(x0), i64::from(x0) + i64::from(width));
    let (ax0, ax1) = span(a.x0, a.width);
    let (bx0, bx1) = span(b.x0, b.width);
    let (ay0, ay1) = span(a.y0, a.height);
    let (by0, by1) = span(b.y0, b.height);

    let x0 = ax0.max(bx0);
    let y0 = ay0.max(by0);
    let width = (ax1.min(bx1) - x0).max(0);
    let height = (ay1.min(by1) - y0).max(0);
    // Both origins came from an `i32`, so the maximum of the two still fits.
    (x0 as i32, y0 as i32, width as u32, height as u32)
}

/// One edge, in sub-scanline space.
struct Edge {
    /// Sub-scanline of the upper end, inclusive.
    top: i32,
    /// Sub-scanline of the lower end, exclusive.
    bottom: i32,
    /// x at `top`, in 1/256 pixel.
    x: i64,
    /// Change in x per sub-scanline, in 1/256 pixel.
    dxdy: i64,
    /// +1 or -1, for the non-zero winding rule.
    winding: i32,
}

/// Rasterizes a path into a coverage mask clipped to the given region.
///
/// The region bounds the work: a path far larger than the page costs only the
/// pixels that could be seen.
///
/// `stop` is asked once every [`STOP_EVERY`] rows, starting with the first,
/// and the sweep ends as soon as it answers `true`. What comes back then is
/// the **partial** mask — the rows already swept, and zero below them — rather
/// than an empty one, because a half-drawn shape is a better progressive frame
/// than a missing one and the caller is discarding the canvas anyway if the
/// stop was real. `None` is the whole of the previous behaviour: no predicate
/// is called and no row is skipped.
#[allow(clippy::too_many_arguments)]
#[must_use]
pub fn fill(
    path: &Path,
    rule: FillRule,
    x0: i32,
    y0: i32,
    width: u32,
    height: u32,
    tolerance: f64,
    stop: Option<&dyn Fn() -> bool>,
) -> Mask {
    let mut mask = Mask::empty(x0, y0, width, height);
    if width == 0 || height == 0 {
        return mask;
    }

    let polys = flatten(path, tolerance);
    let mut edges = Vec::new();
    for poly in &polys {
        build_edges(poly, &mut edges);
    }
    if edges.is_empty() {
        return mask;
    }

    // The active-edge list wants the edges in the order they begin. The sort
    // is stable, so edges starting on the same sub-scanline keep the order the
    // path gave them — which changes nothing, because the accumulation below
    // is order-independent, but it costs nothing to keep.
    edges.sort_by_key(|edge| edge.top);

    // Accumulate coverage per row: each sub-scanline contributes its spans.
    let mut accumulator = vec![0u16; width as usize];
    let mut crossings: Vec<(i64, i32)> = Vec::new();
    // Indices into `edges` of those crossing the current sub-scanline, and how
    // far into `edges` the sweep has reached.
    let mut active: Vec<usize> = Vec::new();
    let mut pending = 0usize;

    for (step, row) in active_rows(&edges, y0, height).enumerate() {
        // Row granularity, not pixel granularity: a row is bounded by the
        // canvas width, and stopping inside one would leave a half-computed
        // accumulator to reason about for no promptness anyone can perceive.
        if step % STOP_EVERY == 0 && stop.is_some_and(|stop| stop()) {
            return mask;
        }
        accumulator.iter_mut().for_each(|slot| *slot = 0);
        let row_top = y0.saturating_add(row as i32).saturating_mul(SAMPLES);

        for sample in 0..SAMPLES {
            let sub = row_top.saturating_add(sample);
            crossings.clear();

            // Sub-scanlines only ever increase, so an edge joins the list once
            // and leaves it once and the whole fill is a single pass over the
            // sorted edges. Testing every edge at every sample cost
            // `height x 16 x edges`, which is why a page-tall region made a
            // hundred-edge path expensive whatever it covered.
            while let Some(edge) = edges.get(pending) {
                if edge.top > sub {
                    break;
                }
                active.push(pending);
                pending += 1;
            }
            active.retain(|index| edges.get(*index).is_some_and(|edge| edge.bottom > sub));

            for index in &active {
                let Some(edge) = edges.get(*index) else {
                    continue;
                };
                // Saturating throughout: a path may reach ±1e9, whose
                // sub-scanline index alone exceeds i32, and the slope times
                // the height then exceeds i64. Clamping puts the crossing far
                // outside the region, which is where it belongs.
                let steps = i64::from(sub).saturating_sub(i64::from(edge.top));
                let x = edge.dxdy.saturating_mul(steps).saturating_add(edge.x);
                crossings.push((x, edge.winding));
            }
            if crossings.len() < 2 {
                continue;
            }
            crossings.sort_by_key(|(x, _)| *x);

            let mut winding = 0i32;
            let mut span_start = 0i64;
            for (x, w) in &crossings {
                let was_inside = inside(winding, rule);
                winding += w;
                let is_inside = inside(winding, rule);

                if !was_inside && is_inside {
                    span_start = *x;
                } else if was_inside && !is_inside {
                    add_span(&mut accumulator, x0, width, span_start, *x);
                }
            }
        }

        // Each sub-scanline contributes at most 256 units of a pixel's width,
        // over SAMPLES rows: scale to 0..=255.
        let base = (row as usize) * (width as usize);
        for (col, total) in accumulator.iter().enumerate() {
            let coverage = (u32::from(*total) / SAMPLES as u32).min(255) as u8;
            if let Some(slot) = mask.data.get_mut(base + col) {
                *slot = coverage;
            }
        }
    }

    mask
}

/// The rows of the region any edge can reach, as a half-open range of row
/// indices into the mask.
///
/// Rows outside it hold no crossing on any of their sixteen sub-scanlines, so
/// their coverage is zero and the mask already says zero there. A page-tall
/// region and a glyph-tall one therefore do the same work, which is what stops
/// the cost of a fill scaling with the paper rather than with the shape.
fn active_rows(edges: &[Edge], y0: i32, height: u32) -> Range<u32> {
    let (Some(first_sub), Some(last_sub)) = (
        edges.iter().map(|edge| edge.top).min(),
        edges.iter().map(|edge| edge.bottom).max(),
    ) else {
        return 0..0;
    };

    let samples = i64::from(SAMPLES);
    // An edge is exclusive at its lower end, so the last sub-scanline it
    // covers is the one before `bottom`. Rounding that the other way drops the
    // final row of every shape whose lowest edge ends exactly on a row
    // boundary — which for an axis-aligned rectangle is all of them.
    let first = i64::from(first_sub).div_euclid(samples);
    let last = i64::from(last_sub).saturating_sub(1).div_euclid(samples);

    let origin = i64::from(y0);
    let limit = i64::from(height);
    let start = (first - origin).clamp(0, limit) as u32;
    let end = (last - origin + 1).clamp(0, limit) as u32;
    start..end.max(start)
}

fn inside(winding: i32, rule: FillRule) -> bool {
    match rule {
        FillRule::NonZero => winding != 0,
        FillRule::EvenOdd => winding % 2 != 0,
    }
}

/// Adds a horizontal span's contribution, with exact partial coverage at both
/// ends. `from` and `to` are in 1/256 pixel, absolute.
fn add_span(accumulator: &mut [u16], x0: i32, width: u32, from: i64, to: i64) {
    if to <= from {
        return;
    }
    let origin = i64::from(x0).saturating_mul(256);
    let left = from.saturating_sub(origin).max(0);
    let right = to
        .saturating_sub(origin)
        .min(i64::from(width).saturating_mul(256));
    if right <= left {
        return;
    }

    let first = (left / 256) as usize;
    let last = ((right - 1) / 256) as usize;

    if first == last {
        // A span inside one pixel.
        if let Some(slot) = accumulator.get_mut(first) {
            *slot = slot.saturating_add((right - left) as u16);
        }
        return;
    }

    if let Some(slot) = accumulator.get_mut(first) {
        let covered = 256 - (left % 256);
        *slot = slot.saturating_add(covered as u16);
    }
    for col in (first + 1)..last {
        if let Some(slot) = accumulator.get_mut(col) {
            *slot = slot.saturating_add(256);
        }
    }
    if let Some(slot) = accumulator.get_mut(last) {
        let covered = right - (last as i64) * 256;
        *slot = slot.saturating_add(covered as u16);
    }
}

/// Turns a polyline into edges, dropping horizontal ones (they contribute no
/// crossings).
fn build_edges(poly: &[Point], edges: &mut Vec<Edge>) {
    for pair in poly.windows(2) {
        let (Some(a), Some(b)) = (pair.first(), pair.get(1)) else {
            continue;
        };
        if !a.is_finite() || !b.is_finite() {
            continue;
        }

        let (top, bottom, winding) = if a.y < b.y { (a, b, 1) } else { (b, a, -1) };

        let y_top = (top.y * f64::from(SAMPLES)).ceil();
        let y_bottom = (bottom.y * f64::from(SAMPLES)).ceil();
        if !y_top.is_finite() || !y_bottom.is_finite() {
            continue;
        }
        let (y_top, y_bottom) = (y_top as i32, y_bottom as i32);
        if y_top >= y_bottom {
            continue;
        }

        let dy = bottom.y - top.y;
        if dy.abs() < f64::EPSILON {
            continue;
        }
        let slope = (bottom.x - top.x) / dy;

        // x where the edge meets the first sub-scanline it covers.
        let y_first = f64::from(y_top) / f64::from(SAMPLES);
        let x_first = top.x + (y_first - top.y) * slope;
        let dxdy = slope / f64::from(SAMPLES);

        if !x_first.is_finite() || !dxdy.is_finite() {
            continue;
        }

        edges.push(Edge {
            top: y_top,
            bottom: y_bottom,
            x: (x_first * 256.0) as i64,
            dxdy: (dxdy * 256.0) as i64,
            winding,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect_mask(x: f64, y: f64, w: f64, h: f64) -> Mask {
        let mut path = Path::new();
        path.rect(x, y, w, h);
        fill(&path, FillRule::NonZero, 0, 0, 20, 20, 0.1, None)
    }

    #[test]
    fn a_pixel_aligned_rectangle_is_fully_covered() {
        let mask = rect_mask(2.0, 3.0, 5.0, 4.0);

        for y in 3..7 {
            for x in 2..7 {
                assert_eq!(mask.at(x, y), 255, "inside at ({x}, {y})");
            }
        }
        assert_eq!(mask.at(1, 3), 0, "left of the rectangle");
        assert_eq!(mask.at(7, 3), 0, "right of it");
        assert_eq!(mask.at(2, 2), 0, "above it");
        assert_eq!(mask.at(2, 7), 0, "below it");
    }

    #[test]
    fn a_half_pixel_edge_is_half_covered() {
        let mask = rect_mask(0.0, 0.0, 4.5, 4.0);
        let edge = mask.at(4, 1);
        assert!(
            (120..=136).contains(&edge),
            "a half-covered pixel should be near 128, got {edge}"
        );
    }

    #[test]
    fn the_even_odd_rule_leaves_a_hole() {
        // Two nested squares wound the same way: non-zero fills the middle,
        // even-odd punches it out.
        let mut path = Path::new();
        path.rect(0.0, 0.0, 10.0, 10.0);
        path.rect(3.0, 3.0, 4.0, 4.0);

        let nonzero = fill(&path, FillRule::NonZero, 0, 0, 12, 12, 0.1, None);
        let evenodd = fill(&path, FillRule::EvenOdd, 0, 0, 12, 12, 0.1, None);

        assert_eq!(nonzero.at(5, 5), 255, "non-zero fills the inner square");
        assert_eq!(evenodd.at(5, 5), 0, "even-odd leaves it empty");
        assert_eq!(evenodd.at(1, 1), 255, "the outer ring stays filled");
    }

    #[test]
    fn filling_is_bit_identical_across_runs() {
        let mut path = Path::new();
        path.move_to(1.3, 2.7);
        path.curve_to(8.1, 0.4, 9.9, 12.5, 2.2, 11.1);
        path.close();

        let a = fill(&path, FillRule::NonZero, 0, 0, 16, 16, 0.1, None);
        let b = fill(&path, FillRule::NonZero, 0, 0, 16, 16, 0.1, None);
        assert_eq!(a.data, b.data, "ruling 4: the same input, the same bytes");
    }

    #[test]
    fn a_triangle_covers_about_half_its_box() {
        let mut path = Path::new();
        path.move_to(0.0, 0.0);
        path.line_to(20.0, 0.0);
        path.line_to(0.0, 20.0);
        path.close();

        let mask = fill(&path, FillRule::NonZero, 0, 0, 20, 20, 0.1, None);
        let total: u64 = mask.data.iter().map(|&v| u64::from(v)).sum();
        let area = total as f64 / 255.0;
        assert!(
            (180.0..220.0).contains(&area),
            "half of 400 is 200, got {area}"
        );
    }

    #[test]
    fn masks_multiply_when_they_intersect() {
        let a = rect_mask(0.0, 0.0, 10.0, 10.0);
        let b = rect_mask(5.0, 0.0, 10.0, 10.0);
        let clipped = a.intersect(&b);

        assert_eq!(clipped.at(7, 5), 255, "both cover this");
        assert_eq!(clipped.at(2, 5), 0, "only the first covers this");
        assert_eq!(clipped.at(12, 5), 0, "only the second, and outside a");
    }

    /// Bounding the result must not change what it reads: a mask that does
    /// not cover a pixel and one that covers it with zero are the same mask.
    #[test]
    fn an_intersection_costs_the_overlap_and_reads_the_same_outside_it() {
        let mut path = Path::new();
        path.rect(1.0, 1.0, 14.0, 14.0);
        let big = fill(&path, FillRule::NonZero, 0, 0, 16, 16, 0.1, None);
        let small = fill(&path, FillRule::NonZero, 4, 6, 5, 3, 0.1, None);

        let clipped = big.intersect(&small);
        assert_eq!(
            (clipped.x0, clipped.y0, clipped.width, clipped.height),
            (4, 6, 5, 3),
            "the result is the overlap, not the larger of the two"
        );
        assert_eq!(clipped.data.len(), 15, "and it allocated only that");

        // Every pixel of the canvas agrees with the unbounded product.
        for y in -2..18 {
            for x in -2..18 {
                let want = u32::from(big.at(x, y)) * u32::from(small.at(x, y));
                let want = ((want + 127) / 255) as u8;
                assert_eq!(clipped.at(x, y), want, "at ({x}, {y})");
            }
        }

        // And in place, which keeps the destination's rectangle rather than
        // shrinking to the overlap, agrees with it pixel for pixel.
        let mut in_place = big.clone();
        in_place.intersect_in_place(&small);
        for y in -2..18 {
            for x in -2..18 {
                assert_eq!(in_place.at(x, y), clipped.at(x, y), "at ({x}, {y})");
            }
        }
    }

    /// The last row and the last column of an in-place intersection.
    ///
    /// Nothing in the workspace caught an `intersect_in_place` that stopped a
    /// row short — injected and re-run, zero assertions failed. It is invisible
    /// almost everywhere because a paint's rectangle is already the clip's
    /// rectangle, and a rectangular clip is fully covered in the middle of
    /// itself. It is *not* invisible at the rectangle's own edge, which is
    /// where the clip's coverage falls away: a row skipped there is ink
    /// outside the clip.
    ///
    /// The shape here is larger than the region, so its coverage is 255
    /// everywhere and the product must equal the clip exactly — at every
    /// pixel, including the ones a loop bound gets wrong.
    #[test]
    fn intersecting_in_place_reaches_the_last_row_and_column() {
        let mut clip_path = Path::new();
        clip_path.rect(2.0, 2.0, 6.5, 6.5);
        let clip = fill(&clip_path, FillRule::NonZero, 1, 1, 10, 10, 0.1, None);

        let mut path = Path::new();
        path.rect(-5.0, -5.0, 30.0, 30.0);
        let mut mask = fill(&path, FillRule::NonZero, 1, 1, 10, 10, 0.1, None);
        assert_eq!(mask.at(10, 10), 255, "the shape covers the whole region");

        mask.intersect_in_place(&clip);
        for y in 0..12 {
            for x in 0..12 {
                assert_eq!(
                    mask.at(x, y),
                    clip.at(x, y),
                    "an opaque shape clipped is the clip, at ({x}, {y})"
                );
            }
        }
        assert_eq!(mask.at(10, 10), 0, "and the last row is outside the clip");
    }

    /// The rectangle a consumer walks. Every edge is checked independently,
    /// because a mask hanging off one side of the canvas is exactly what a
    /// glyph at the margin is.
    #[test]
    fn a_mask_reports_the_canvas_pixels_it_can_reach() {
        let inside = Mask::empty(3, 4, 5, 6);
        assert_eq!(inside.overlap(100, 100), (3, 4, 8, 10));

        // Hanging off each edge in turn, clamped to the canvas and never past
        // it.
        assert_eq!(Mask::empty(-3, 4, 5, 6).overlap(100, 100), (0, 4, 2, 10));
        assert_eq!(Mask::empty(3, -4, 5, 6).overlap(100, 100), (3, 0, 8, 2));
        assert_eq!(Mask::empty(97, 4, 5, 6).overlap(100, 100), (97, 4, 100, 10));
        assert_eq!(Mask::empty(3, 96, 5, 6).overlap(100, 100), (3, 96, 8, 100));

        // Entirely outside, in both directions: an empty range rather than a
        // reversed one, which would panic a `for` loop or wrap a subtraction.
        assert_eq!(Mask::empty(-50, -50, 5, 6).overlap(100, 100), (0, 0, 0, 0));
        assert_eq!(
            Mask::empty(500, 500, 5, 6).overlap(100, 100),
            (100, 100, 100, 100)
        );
        // And nothing overflows at the extremes. Built by hand rather than
        // through `empty`, which would try to allocate the square of a `u32`.
        let huge = Mask {
            x0: i32::MAX - 1,
            y0: i32::MIN,
            width: u32::MAX,
            height: u32::MAX,
            data: Vec::new(),
        };
        // Its columns are past the right edge and its rows cover everything,
        // so the x range is empty and the y range is the canvas.
        assert_eq!(huge.overlap(10, 10), (10, 0, 10, 10));
    }

    /// Ruling 4's claim under the active-edge list, checked rather than
    /// assumed: the order edges arrive in cannot change a pixel.
    ///
    /// The plan's risk table says integer accumulation is order-independent
    /// "by construction". It is, but the construction is worth naming, because
    /// if any of it were floating-point the mitigation would be false. Within
    /// one sub-scanline the crossings are sorted by x and swept left to right;
    /// crossings at the *same* x can be swept in either order, and the two
    /// sweeps can decompose the same interval differently — `[a, x] + [x, b]`
    /// against `[a, b]`. `add_span` measures each pixel's overlap in exact
    /// 1/256 units of `i64` and adds them into a `u16` that cannot reach its
    /// ceiling (sixteen sub-scanlines of at most 256 each), so the two
    /// decompositions add to the same integer. No float, no saturation, no
    /// reordering hazard.
    ///
    /// These two paths hold the same subpaths in the opposite order, so the
    /// edge list is a permutation, and the vertical edges are deliberately
    /// shared so equal-x crossings actually occur.
    #[test]
    fn the_order_edges_arrive_in_does_not_change_a_pixel() {
        let (a, b) = ((2.0, 3.0, 6.0, 9.0), (2.0, 7.0, 6.0, 9.0));
        let mut first = Path::new();
        first.rect(a.0, a.1, a.2, a.3);
        first.rect(b.0, b.1, b.2, b.3);
        let mut second = Path::new();
        second.rect(b.0, b.1, b.2, b.3);
        second.rect(a.0, a.1, a.2, a.3);

        for rule in [FillRule::NonZero, FillRule::EvenOdd] {
            let one = fill(&first, rule, 0, 0, 16, 16, 0.1, None);
            let two = fill(&second, rule, 0, 0, 16, 16, 0.1, None);
            assert_eq!(one.data, two.data, "ruling 4, under {rule:?}");
        }

        // And with the overlap at a fractional x, where the spans really do
        // land inside a pixel rather than on its boundary.
        let mut first = Path::new();
        first.rect(1.25, 2.5, 7.75, 6.5);
        first.rect(5.125, 4.75, 4.5, 8.25);
        let mut second = Path::new();
        second.rect(5.125, 4.75, 4.5, 8.25);
        second.rect(1.25, 2.5, 7.75, 6.5);
        for rule in [FillRule::NonZero, FillRule::EvenOdd] {
            assert_eq!(
                fill(&first, rule, 0, 0, 16, 16, 0.1, None).data,
                fill(&second, rule, 0, 0, 16, 16, 0.1, None).data,
                "ruling 4 at a fractional overlap, under {rule:?}"
            );
        }
    }

    /// The row range the sweep runs over, including the boundary case the
    /// active-edge list is most likely to get wrong: a shape whose last edge
    /// ends exactly on a row boundary.
    #[test]
    fn the_rows_a_shape_reaches_include_the_one_it_ends_on() {
        let rows = |y: f64, h: f64, y0: i32, height: u32| {
            let mut path = Path::new();
            path.rect(2.0, y, 4.0, h);
            let mut edges = Vec::new();
            for poly in &flatten(&path, 0.1) {
                build_edges(poly, &mut edges);
            }
            edges.sort_by_key(|edge| edge.top);
            active_rows(&edges, y0, height)
        };

        // Exactly on both boundaries: rows 3 through 6 inclusive.
        assert_eq!(rows(3.0, 4.0, 0, 20), 3..7);
        // Past them by more than a sub-scanline, so the shape reaches one more
        // row at each end.
        assert_eq!(rows(2.9, 4.15, 0, 20), 2..8);
        // Past them by *less* than a sub-scanline: an edge's top snaps up to
        // the next sample, so a sliver thinner than 1/16 px contributes to no
        // row at all — which is what the row loop already did, and the point
        // of computing the range from the edges rather than from the geometry.
        assert_eq!(rows(2.99, 4.02, 0, 20), 3..8);
        // Fractional, and inside a single row.
        assert_eq!(rows(3.25, 0.5, 0, 20), 3..4);
        // Clamped to the region, from both directions.
        assert_eq!(rows(3.0, 4.0, 5, 20), 0..2);
        assert_eq!(rows(3.0, 4.0, -10, 20), 13..17);
        assert_eq!(rows(3.0, 4.0, 100, 20), 0..0, "entirely above the region");
        assert_eq!(rows(300.0, 4.0, 0, 20), 20..20, "entirely below it");
        assert!(active_rows(&[], 0, 20).is_empty());
    }

    #[test]
    fn degenerate_geometry_produces_an_empty_mask() {
        let empty = fill(&Path::new(), FillRule::NonZero, 0, 0, 4, 4, 0.1, None);
        assert!(empty.data.iter().all(|&v| v == 0));

        // A zero-area region is legal and costs nothing.
        let none = fill(&Path::new(), FillRule::NonZero, 0, 0, 0, 0, 0.1, None);
        assert!(none.data.is_empty());

        // A path entirely outside the region contributes nothing.
        let mut far = Path::new();
        far.rect(1000.0, 1000.0, 10.0, 10.0);
        let outside = fill(&far, FillRule::NonZero, 0, 0, 8, 8, 0.1, None);
        assert!(outside.data.iter().all(|&v| v == 0));
    }

    #[test]
    fn enormous_coordinates_do_not_hang_or_overflow() {
        let mut path = Path::new();
        path.move_to(-1e9, -1e9);
        path.line_to(1e9, -1e9);
        path.line_to(1e9, 1e9);
        path.close();
        let mask = fill(&path, FillRule::NonZero, 0, 0, 8, 8, 0.1, None);
        assert_eq!(mask.data.len(), 64);
    }

    // -----------------------------------------------------------------------
    // Stopping the sweep (gap 15).
    // -----------------------------------------------------------------------

    /// A predicate that answers `false` until its `at`-th question and `true`
    /// from then on, counting how many times it was asked.
    ///
    /// Deterministic by construction: it fires on the Nth *question*, never on
    /// a clock. A stop test written against elapsed time is flaky by
    /// construction — the sweep either reached the row before the deadline or
    /// it did not — and one in CI fails for reasons nobody can reproduce.
    struct StopAt {
        at: u32,
        calls: std::cell::Cell<u32>,
    }

    impl StopAt {
        fn new(at: u32) -> StopAt {
            StopAt {
                at,
                calls: std::cell::Cell::new(0),
            }
        }

        fn ask(&self) -> bool {
            self.calls.set(self.calls.get().saturating_add(1));
            self.calls.get() >= self.at
        }
    }

    /// A tall bar filling every row of a tall region, so the sweep's step
    /// count and the row index are the same number.
    fn tall_bar() -> Path {
        let mut path = Path::new();
        path.rect(5.0, 0.0, 30.0, 400.0);
        path
    }

    /// Milestone 2: the row loop stops when asked, and what comes back is the
    /// part of the shape it had already drawn.
    ///
    /// The partial mask is the point. Returning an empty one would be simpler
    /// and would pass any test that only asked whether the sweep stopped —
    /// which is why both halves are asserted here: ink above the cut, none at
    /// it or below.
    #[test]
    fn a_stopped_fill_keeps_the_rows_it_had_already_swept() {
        let path = tall_bar();
        let whole = fill(&path, FillRule::NonZero, 0, 0, 40, 400, 0.1, None);
        assert_eq!(whole.at(20, 399), 255, "the shape reaches the last row");

        // The fourth question is asked at row 3 x STOP_EVERY, and stops there.
        let stop = StopAt::new(4);
        let ask = || stop.ask();
        let partial = fill(&path, FillRule::NonZero, 0, 0, 40, 400, 0.1, Some(&ask));

        let cut = 3 * STOP_EVERY as i32;
        assert_eq!(partial.at(20, cut - 1), 255, "the rows before the cut drew");
        assert_eq!(partial.at(20, cut), 0, "the row it stopped on did not");
        assert_eq!(partial.at(20, 399), 0, "nor did anything below it");
        assert_eq!(
            partial.data.iter().filter(|&&v| v != 0).count(),
            cut as usize * 30,
            "exactly the rows above the cut, at the bar's own width"
        );
    }

    /// The hook is not an input to the coverage: a predicate that never
    /// answers yes leaves every byte where `None` left it (ruling 4).
    ///
    /// And the period is a band rather than a row — the cost the risk row in
    /// the gap plan is about. Derived from the constant rather than written
    /// out, so tuning `STOP_EVERY` retunes the assertion with it.
    #[test]
    fn a_predicate_that_never_answers_yes_changes_no_pixel() {
        let path = tall_bar();
        let stop = StopAt::new(u32::MAX);
        let ask = || stop.ask();

        let hooked = fill(&path, FillRule::NonZero, 0, 0, 40, 400, 0.1, Some(&ask));
        let bare = fill(&path, FillRule::NonZero, 0, 0, 40, 400, 0.1, None);

        assert_eq!(hooked.data, bare.data, "the same input, the same bytes");
        assert_eq!(
            stop.calls.get() as usize,
            400usize.div_ceil(STOP_EVERY),
            "one question per band of {STOP_EVERY} rows, not one per row"
        );
    }

    /// A shape that reaches no row is never asked at all, so a page full of
    /// off-canvas geometry costs no questions either.
    #[test]
    fn a_fill_with_nothing_to_sweep_asks_nothing() {
        let mut far = Path::new();
        far.rect(1000.0, 1000.0, 10.0, 10.0);
        let stop = StopAt::new(1);
        let ask = || stop.ask();

        let mask = fill(&far, FillRule::NonZero, 0, 0, 8, 8, 0.1, Some(&ask));
        assert!(mask.data.iter().all(|&v| v == 0));
        assert_eq!(stop.calls.get(), 0, "no rows, no questions");
    }
}
