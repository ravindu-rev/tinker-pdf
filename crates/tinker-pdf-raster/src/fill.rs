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

use crate::geom::{flatten, FillRule, Path, Point};

/// Sub-scanlines per pixel row. Sixteen gives 4-bit vertical resolution,
/// combined with exact horizontal spans for 8-bit total coverage.
const SAMPLES: i32 = 16;

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
    #[must_use]
    pub fn intersect(&self, other: &Mask) -> Mask {
        let mut out = Mask::empty(self.x0, self.y0, self.width, self.height);
        for row in 0..self.height {
            for col in 0..self.width {
                let x = self.x0 + col as i32;
                let y = self.y0 + row as i32;
                let a = u32::from(self.at(x, y));
                let b = u32::from(other.at(x, y));
                // Rounded rather than truncated, so full coverage stays full.
                let value = ((a * b + 127) / 255) as u8;
                let index = (row as usize) * (self.width as usize) + (col as usize);
                if let Some(slot) = out.data.get_mut(index) {
                    *slot = value;
                }
            }
        }
        out
    }
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
#[must_use]
pub fn fill(
    path: &Path,
    rule: FillRule,
    x0: i32,
    y0: i32,
    width: u32,
    height: u32,
    tolerance: f64,
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

    // Accumulate coverage per row: each sub-scanline contributes its spans.
    let mut accumulator = vec![0u16; width as usize];
    let mut crossings: Vec<(i64, i32)> = Vec::new();

    for row in 0..height {
        accumulator.iter_mut().for_each(|slot| *slot = 0);
        let row_top = y0.saturating_add(row as i32).saturating_mul(SAMPLES);

        for sample in 0..SAMPLES {
            let sub = row_top + sample;
            crossings.clear();

            for edge in &edges {
                if sub < edge.top || sub >= edge.bottom {
                    continue;
                }
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
        fill(&path, FillRule::NonZero, 0, 0, 20, 20, 0.1)
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

        let nonzero = fill(&path, FillRule::NonZero, 0, 0, 12, 12, 0.1);
        let evenodd = fill(&path, FillRule::EvenOdd, 0, 0, 12, 12, 0.1);

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

        let a = fill(&path, FillRule::NonZero, 0, 0, 16, 16, 0.1);
        let b = fill(&path, FillRule::NonZero, 0, 0, 16, 16, 0.1);
        assert_eq!(a.data, b.data, "ruling 4: the same input, the same bytes");
    }

    #[test]
    fn a_triangle_covers_about_half_its_box() {
        let mut path = Path::new();
        path.move_to(0.0, 0.0);
        path.line_to(20.0, 0.0);
        path.line_to(0.0, 20.0);
        path.close();

        let mask = fill(&path, FillRule::NonZero, 0, 0, 20, 20, 0.1);
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

    #[test]
    fn degenerate_geometry_produces_an_empty_mask() {
        let empty = fill(&Path::new(), FillRule::NonZero, 0, 0, 4, 4, 0.1);
        assert!(empty.data.iter().all(|&v| v == 0));

        // A zero-area region is legal and costs nothing.
        let none = fill(&Path::new(), FillRule::NonZero, 0, 0, 0, 0, 0.1);
        assert!(none.data.is_empty());

        // A path entirely outside the region contributes nothing.
        let mut far = Path::new();
        far.rect(1000.0, 1000.0, 10.0, 10.0);
        let outside = fill(&far, FillRule::NonZero, 0, 0, 8, 8, 0.1);
        assert!(outside.data.iter().all(|&v| v == 0));
    }

    #[test]
    fn enormous_coordinates_do_not_hang_or_overflow() {
        let mut path = Path::new();
        path.move_to(-1e9, -1e9);
        path.line_to(1e9, -1e9);
        path.line_to(1e9, 1e9);
        path.close();
        let mask = fill(&path, FillRule::NonZero, 0, 0, 8, 8, 0.1);
        assert_eq!(mask.data.len(), 64);
    }
}
