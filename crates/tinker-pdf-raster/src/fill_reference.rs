//! The scanline sweep, kept as the reference `fill` is held to (P-07).
//!
//! [`fill_reference`] sorts a path's edges by their tops, keeps an active-edge
//! list per sub-scanline, sorts each sub-scanline's crossings by `x` and walks
//! them into a `u16` accumulator one span at a time, a span added where the
//! fill rule goes from inside back to outside. Its every byte is the contract
//! of `docs/design/rasterizer-accumulation.md`, and every differential test of
//! the filler compares against it. This file is compiled for tests only: it is
//! the reference, the paths the harness generates, and nothing else.

use core::ops::Range;

use crate::fill::Mask;
use crate::geom::{flatten, FillRule, Path, Point};
use crate::image::Transform;
use crate::stroke::{stroke, stroke_mapped, LineCap, LineJoin, StrokeStyle};

/// Sub-scanlines per pixel row.
const SAMPLES: i32 = 16;

/// Rows of the sweep between one cancellation check and the next.
pub(crate) const STOP_EVERY: usize = 16;

/// Sub-steps an edge's x carries below the 1/256 the spans are measured in,
/// as a bit count.
const SUBSTEP_BITS: u32 = 16;

/// [`SUBSTEP_BITS`] as a multiplier.
const SUBSTEP: i64 = 1 << SUBSTEP_BITS;

/// One edge, in sub-scanline space.
struct Edge {
    /// Sub-scanline of the upper end, inclusive.
    top: i32,
    /// Sub-scanline of the lower end, exclusive.
    bottom: i32,
    /// x at `top`, in 1/256 pixel scaled by [`SUBSTEP`].
    x: i64,
    /// Change in x per sub-scanline, in the same units as [`Edge::x`].
    dxdy: i64,
    /// +1 or -1, for the non-zero winding rule.
    winding: i32,
}

/// Rasterizes a path into a coverage mask clipped to the given region, by
/// the sorted sweep.
///
/// `stop` is asked once every [`STOP_EVERY`] rows, starting with the first,
/// and the sweep ends as soon as it answers `true`, returning the rows already
/// swept and zero below them.
#[allow(clippy::too_many_arguments)]
#[must_use]
pub(crate) fn fill_reference(
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

    let mut edges = edges_of(path, tolerance);
    if edges.is_empty() {
        return mask;
    }

    // The active-edge list wants the edges in the order they begin; the sort
    // is stable, so edges starting on one sub-scanline keep the path's order.
    edges.sort_by_key(|edge| edge.top);

    let mut accumulator = vec![0u16; width as usize];
    let mut crossings: Vec<(i64, i32)> = Vec::new();
    let mut active: Vec<usize> = Vec::new();
    let mut pending = 0usize;

    for (step, row) in active_rows(&edges, y0, height).enumerate() {
        if step % STOP_EVERY == 0 && stop.is_some_and(|stop| stop()) {
            return mask;
        }
        accumulator.iter_mut().for_each(|slot| *slot = 0);
        let row_top = y0.saturating_add(row as i32).saturating_mul(SAMPLES);

        for sample in 0..SAMPLES {
            let sub = row_top.saturating_add(sample);
            crossings.clear();

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
                crossings.push((crossing(edge, sub), edge.winding));
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

        let base = (row as usize) * (width as usize);
        if let Some(out) = mask.data.get_mut(base..base + accumulator.len()) {
            for (slot, total) in out.iter_mut().zip(accumulator.iter()) {
                *slot = (*total / SAMPLES as u16).min(255) as u8;
            }
        }
    }

    mask
}

/// The path's edges, in the order the path gives them.
fn edges_of(path: &Path, tolerance: f64) -> Vec<Edge> {
    let mut edges = Vec::new();
    for poly in &flatten(path, tolerance) {
        build_edges(poly, &mut edges);
    }
    edges
}

/// Where `edge` meets sub-scanline `sub`, in 1/256 pixel: saturating
/// throughout, and reduced to the nearest unit by `+ half, >> bits`.
fn crossing(edge: &Edge, sub: i32) -> i64 {
    let steps = i64::from(sub).saturating_sub(i64::from(edge.top));
    edge.dxdy
        .saturating_mul(steps)
        .saturating_add(edge.x)
        .saturating_add(SUBSTEP / 2)
        >> SUBSTEP_BITS
}

/// The rows of the region any edge can reach, as a half-open range of row
/// indices into the mask.
fn active_rows(edges: &[Edge], y0: i32, height: u32) -> Range<u32> {
    let (Some(first_sub), Some(last_sub)) = (
        edges.iter().map(|edge| edge.top).min(),
        edges.iter().map(|edge| edge.bottom).max(),
    ) else {
        return 0..0;
    };

    let samples = i64::from(SAMPLES);
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
        if let Some(slot) = accumulator.get_mut(first) {
            *slot = slot.saturating_add((right - left) as u16);
        }
        return;
    }

    if let Some(slot) = accumulator.get_mut(first) {
        let covered = 256 - (left % 256);
        *slot = slot.saturating_add(covered as u16);
    }
    let to = last.min(accumulator.len());
    if let Some(run) = accumulator.get_mut((first + 1).min(to)..to) {
        for slot in run {
            *slot = slot.saturating_add(256);
        }
    }
    if let Some(slot) = accumulator.get_mut(last) {
        let covered = right - (last as i64) * 256;
        *slot = slot.saturating_add(covered as u16);
    }
}

/// Turns a polyline into edges, dropping horizontal ones, and closes it
/// (8.5.3.1, 8.5.4).
fn build_edges(poly: &[Point], edges: &mut Vec<Edge>) {
    let closing = match (poly.first(), poly.last()) {
        (Some(first), Some(last)) if first != last => Some([*last, *first]),
        _ => None,
    };
    for pair in poly
        .windows(2)
        .chain(closing.as_ref().map(|pair| &pair[..]))
    {
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

        let y_first = f64::from(y_top) / f64::from(SAMPLES);
        let x_first = top.x + (y_first - top.y) * slope;
        let dxdy = slope / f64::from(SAMPLES);

        if !x_first.is_finite() || !dxdy.is_finite() {
            continue;
        }

        let sub = |v: f64| (v * 256.0 * SUBSTEP as f64).round() as i64;

        edges.push(Edge {
            top: y_top,
            bottom: y_bottom,
            x: sub(x_first),
            dxdy: sub(dxdy),
            winding,
        });
    }
}

/// What the sweep meets on a fill, counted by the sweep's own arithmetic.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Census {
    /// Crossings that land in a pixel of the region holding another crossing
    /// of their sub-scanline.
    pub(crate) shared: u64,
    /// Sub-scanlines of the region's rows whose crossings do not close: their
    /// windings sum to a value the rule calls inside.
    pub(crate) unclosed: u64,
    /// Pixels of the region's rows holding a crossing on any of their
    /// sub-scanlines.
    pub(crate) cells: u64,
    /// Runs of pixels holding none, between those and at either end of each
    /// row.
    pub(crate) runs: u64,
}

/// The [`Census`] of a fill over the given region.
pub(crate) fn census(
    path: &Path,
    rule: FillRule,
    x0: i32,
    y0: i32,
    width: u32,
    height: u32,
    tolerance: f64,
) -> Census {
    let mut census = Census::default();
    if width == 0 || height == 0 {
        return census;
    }
    let mut edges = edges_of(path, tolerance);
    edges.sort_by_key(|edge| edge.top);
    let origin = i64::from(x0) * 256;
    let right = origin + i64::from(width) * 256;
    let (mut active, mut pending) = (Vec::new(), 0usize);
    let mut pixels: Vec<i64> = Vec::new();
    let mut row_pixels: Vec<i64> = Vec::new();
    for row in active_rows(&edges, y0, height) {
        row_pixels.clear();
        let row_top = y0.saturating_add(row as i32).saturating_mul(SAMPLES);
        for sample in 0..SAMPLES {
            let sub = row_top.saturating_add(sample);
            while let Some(edge) = edges.get(pending) {
                if edge.top > sub {
                    break;
                }
                active.push(pending);
                pending += 1;
            }
            active.retain(|index| edges.get(*index).is_some_and(|edge| edge.bottom > sub));
            let mut winding = 0i32;
            pixels.clear();
            for edge in active.iter().filter_map(|index| edges.get(*index)) {
                winding = winding.wrapping_add(edge.winding);
                let x = crossing(edge, sub);
                if (origin..right).contains(&x) {
                    pixels.push((x - origin) >> 8);
                }
            }
            if inside(winding, rule) {
                census.unclosed += 1;
            }
            pixels.sort_unstable();
            row_pixels.extend_from_slice(&pixels);
            let mut at = 0;
            while let Some(&pixel) = pixels.get(at) {
                let run = pixels
                    .get(at..)
                    .map_or(0, |rest| rest.iter().take_while(|p| **p == pixel).count());
                if run > 1 {
                    census.shared += run as u64;
                }
                at += run.max(1);
            }
        }
        row_pixels.sort_unstable();
        row_pixels.dedup();
        census.cells += row_pixels.len() as u64;
        let mut at = 0;
        for pixel in &row_pixels {
            if *pixel > at {
                census.runs += 1;
            }
            at = pixel + 1;
        }
        if at < i64::from(width) {
            census.runs += 1;
        }
    }
    census
}

/// SplitMix64: the same stream on every target, so a case is the same case
/// wherever it runs.
pub(crate) struct Stream(u64);

impl Stream {
    pub(crate) fn new(seed: u64) -> Stream {
        Stream(seed)
    }

    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// A number in `[lo, hi)`.
    pub(crate) fn range(&mut self, lo: f64, hi: f64) -> f64 {
        let unit = (self.next() >> 11) as f64 / (1u64 << 53) as f64;
        lo + (hi - lo) * unit
    }

    /// A number below `bound`, which is at least one.
    pub(crate) fn below(&mut self, bound: u64) -> u64 {
        self.next() % bound
    }

    pub(crate) fn coin(&mut self) -> bool {
        self.next() & 1 == 1
    }

    /// A coordinate in `[lo, hi)`, now and then on the 1/16 or the 1/2 grid,
    /// where an edge lands exactly on a sub-scanline or a pixel boundary.
    pub(crate) fn coordinate(&mut self, lo: f64, hi: f64) -> f64 {
        let value = self.range(lo, hi);
        match self.below(8) {
            0 => (value * 16.0).round() / 16.0,
            1 => (value * 2.0).round() / 2.0,
            _ => value,
        }
    }
}

/// The kinds of path the harness generates, each a way a filler could part
/// from the sweep.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Class {
    /// Subpaths over one another, each wound either way.
    Overlap,
    /// Stars, bowties and polygons whose edges cross.
    SelfIntersecting,
    /// Several crossings in one pixel on one sub-scanline: fans from one
    /// apex, slivers side by side, triangles inside one pixel.
    Shared,
    /// Subpaths thinner than a sub-scanline.
    Sliver,
    /// Vertices near 1e9, where the arithmetic saturates.
    Far,
    /// The stroker's outlines: every cap, join, dash, mapped pen and floor.
    Stroked,
    /// Subpaths with an edge `build_edges` drops, so that a sub-scanline does
    /// not close.
    Dropped,
}

/// Every class, in the order the harness runs them.
pub(crate) const CLASSES: [Class; 7] = [
    Class::Overlap,
    Class::SelfIntersecting,
    Class::Shared,
    Class::Sliver,
    Class::Far,
    Class::Stroked,
    Class::Dropped,
];

/// A closed polygon, its points in order or reversed.
fn polygon(path: &mut Path, points: &[(f64, f64)], reversed: bool) {
    let mut order: Vec<(f64, f64)> = points.to_vec();
    if reversed {
        order.reverse();
    }
    for (i, (x, y)) in order.iter().enumerate() {
        if i == 0 {
            path.move_to(*x, *y);
        } else {
            path.line_to(*x, *y);
        }
    }
    path.close();
}

/// An ellipse of four cubics, wound either way.
fn ellipse(path: &mut Path, (cx, cy): (f64, f64), (rx, ry): (f64, f64), reversed: bool) {
    const K: f64 = 0.552_284_75;
    let ry = if reversed { -ry } else { ry };
    path.move_to(cx + rx, cy);
    path.curve_to(cx + rx, cy + K * ry, cx + K * rx, cy + ry, cx, cy + ry);
    path.curve_to(cx - K * rx, cy + ry, cx - rx, cy + K * ry, cx - rx, cy);
    path.curve_to(cx - rx, cy - K * ry, cx - K * rx, cy - ry, cx, cy - ry);
    path.curve_to(cx + K * rx, cy - ry, cx + rx, cy - K * ry, cx + rx, cy);
    path.close();
}

/// One shape of a few kinds near `(cx, cy)`, about `size` across.
fn shape(path: &mut Path, s: &mut Stream, (cx, cy): (f64, f64), size: f64) {
    let reversed = s.coin();
    match s.below(5) {
        0 => {
            let (w, h) = (s.range(0.5, size), s.range(0.5, size));
            let (x, y) = (cx - w / 2.0, cy - h / 2.0);
            polygon(
                path,
                &[(x, y), (x + w, y), (x + w, y + h), (x, y + h)],
                reversed,
            );
        }
        1 => {
            let points: Vec<(f64, f64)> = (0..3)
                .map(|_| {
                    (
                        cx + s.coordinate(-size, size),
                        cy + s.coordinate(-size, size),
                    )
                })
                .collect();
            polygon(path, &points, reversed);
        }
        2 => {
            let radii = (s.range(0.5, size), s.range(0.5, size));
            ellipse(path, (cx, cy), radii, reversed);
        }
        3 => {
            // A quadratic lens: two arcs between the same two points.
            let (ax, ay) = (cx - s.range(0.5, size), cy + s.coordinate(-size, size));
            let (bx, by) = (cx + s.range(0.5, size), cy + s.coordinate(-size, size));
            let (up, down) = (s.range(1.0, 2.0 * size), s.range(1.0, 2.0 * size));
            path.move_to(ax, ay);
            path.quad_to(cx, cy + up, bx, by);
            path.quad_to(cx, cy - down, ax, ay);
            path.close();
        }
        _ => {
            // An open subpath, which the fill closes.
            path.move_to(
                cx + s.coordinate(-size, size),
                cy + s.coordinate(-size, size),
            );
            for _ in 0..s.below(3) + 2 {
                path.line_to(
                    cx + s.coordinate(-size, size),
                    cy + s.coordinate(-size, size),
                );
            }
        }
    }
}

/// The next float above `y`.
fn above(y: f64) -> f64 {
    let bits = y.to_bits();
    if y > 0.0 {
        f64::from_bits(bits + 1)
    } else if y < 0.0 {
        f64::from_bits(bits - 1)
    } else {
        f64::from_bits(1)
    }
}

/// The `index`th path of `class`, from the stream.
pub(crate) fn generate(class: Class, index: u64, s: &mut Stream) -> Path {
    let mut path = Path::new();
    match class {
        Class::Overlap => {
            let (cx, cy) = (s.range(10.0, 40.0), s.range(10.0, 40.0));
            for _ in 0..s.below(4) + 2 {
                let at = (cx + s.range(-9.0, 9.0), cy + s.range(-9.0, 9.0));
                let size = s.range(3.0, 18.0);
                shape(&mut path, s, at, size);
            }
        }
        Class::SelfIntersecting => self_intersecting(&mut path, s),
        Class::Shared => shared(&mut path, s),
        Class::Sliver => {
            for _ in 0..s.below(4) + 1 {
                let x = s.range(-3.0, 30.0);
                let w = s.range(0.5, 30.0);
                // On a sub-scanline boundary half the time, between two
                // otherwise.
                let y = if s.coin() {
                    (s.range(0.0, 30.0) * 16.0).round() / 16.0 - s.range(0.0, 0.03)
                } else {
                    s.range(0.0, 30.0)
                };
                let h = s.range(0.0005, 0.0624);
                if s.coin() {
                    polygon(
                        &mut path,
                        &[(x, y), (x + w, y), (x + w, y + h), (x, y + h)],
                        s.coin(),
                    );
                } else {
                    // Nearly horizontal and spanning many pixels.
                    let drop = s.range(-0.06, 0.06);
                    polygon(
                        &mut path,
                        &[(x, y), (x + w, y + drop), (x + w, y + drop + h), (x, y + h)],
                        s.coin(),
                    );
                }
            }
            if s.coin() {
                let at = (s.range(5.0, 30.0), s.range(5.0, 30.0));
                let size = s.range(2.0, 12.0);
                shape(&mut path, s, at, size);
            }
        }
        Class::Far => far(&mut path, s),
        Class::Stroked => return stroked(index, s),
        Class::Dropped => dropped(&mut path, s),
    }
    path
}

fn self_intersecting(path: &mut Path, s: &mut Stream) {
    match s.below(3) {
        0 => {
            // A star polygon {n/k}: every edge crosses others.
            let stars = [(5, 2), (7, 2), (7, 3), (8, 3), (9, 2), (9, 4)];
            let (n, k) = stars
                .get(s.below(stars.len() as u64) as usize)
                .copied()
                .unwrap_or((5, 2));
            let (cx, cy) = (s.range(18.0, 30.0), s.range(18.0, 30.0));
            let r = s.range(6.0, 20.0);
            let turn = s.range(0.0, std::f64::consts::TAU);
            let points: Vec<(f64, f64)> = (0..n)
                .map(|i: i32| {
                    let angle = turn + std::f64::consts::TAU * f64::from(i * k) / f64::from(n);
                    (
                        cx + r * tinker_pdf_math::cos(angle),
                        cy + r * tinker_pdf_math::sin(angle),
                    )
                })
                .collect();
            polygon(path, &points, s.coin());
        }
        1 => {
            // A bowtie, its two halves wound against each other.
            let (x, y) = (s.range(4.0, 20.0), s.range(4.0, 20.0));
            let (w, h) = (s.range(4.0, 30.0), s.range(4.0, 30.0));
            polygon(
                path,
                &[(x, y), (x + w, y + h), (x + w, y), (x, y + h)],
                s.coin(),
            );
        }
        _ => {
            // A random polygon, crossing itself as it pleases.
            let points: Vec<(f64, f64)> = (0..s.below(9) + 6)
                .map(|_| (s.coordinate(2.0, 44.0), s.coordinate(2.0, 44.0)))
                .collect();
            polygon(path, &points, false);
        }
    }
}

fn shared(path: &mut Path, s: &mut Stream) {
    match s.below(3) {
        0 => {
            // A fan of thin wedges from one apex: near it, every edge is in
            // one pixel on every sub-scanline.
            let (ax, ay) = (s.range(10.0, 30.0), s.range(4.0, 12.0));
            for _ in 0..s.below(6) + 3 {
                let x = ax + s.range(-3.0, 3.0);
                let spread = s.range(0.05, 0.9);
                let low = ay + s.range(8.0, 24.0);
                polygon(
                    path,
                    &[(ax, ay), (x + spread, low), (x - spread, low)],
                    s.coin(),
                );
            }
        }
        1 => {
            // Slivers under a pixel wide, side by side in one column.
            let column = s.range(8.0, 30.0);
            for _ in 0..s.below(5) + 2 {
                let x = column + s.range(0.0, 1.5);
                let w = s.range(0.04, 0.9);
                let lean = s.range(-1.5, 1.5);
                let (top, bottom) = (s.range(2.0, 10.0), s.range(14.0, 30.0));
                polygon(
                    path,
                    &[
                        (x, top),
                        (x + w, top),
                        (x + w + lean, bottom),
                        (x + lean, bottom),
                    ],
                    s.coin(),
                );
            }
        }
        _ => {
            // Triangles inside one pixel or two.
            let (px, py) = (s.range(6.0, 30.0), s.range(6.0, 30.0));
            for _ in 0..s.below(10) + 3 {
                let points: Vec<(f64, f64)> = (0..3)
                    .map(|_| (px + s.range(0.0, 1.6), py + s.range(0.0, 1.6)))
                    .collect();
                polygon(path, &points, s.coin());
            }
        }
    }
}

fn far(path: &mut Path, s: &mut Stream) {
    const FAR: f64 = 1e9;
    let (x, y) = (s.coordinate(-4.0, 40.0), s.coordinate(-4.0, 40.0));
    match s.below(4) {
        0 => {
            let reach = FAR * s.range(0.5, 1.0);
            let dy = s.range(-3.0, 3.0);
            polygon(path, &[(x, y), (reach, y + dy), (x, FAR)], s.coin());
        }
        1 => {
            let (a, b) = (FAR * s.range(0.5, 1.0), FAR * s.range(0.5, 1.0));
            polygon(path, &[(-FAR, -FAR), (FAR, -a), (b, FAR)], s.coin());
        }
        2 => {
            // A long thin wedge out to 1e9 at a shallow angle.
            let rise = FAR * s.range(-1e-8, 1e-8);
            let thick = s.range(0.5, 9.0);
            polygon(
                path,
                &[(x, y), (FAR, y + rise), (FAR, y + rise + thick)],
                s.coin(),
            );
        }
        _ => {
            let (up, across) = (s.range(1.0, 20.0), s.range(-5.0, 5.0));
            polygon(
                path,
                &[(-FAR, y), (x, y - up), (FAR, y + across), (x, FAR)],
                s.coin(),
            );
        }
    }
    if s.coin() {
        let at = (s.range(5.0, 30.0), s.range(5.0, 30.0));
        let size = s.range(2.0, 12.0);
        shape(path, s, at, size);
    }
}

fn dropped(path: &mut Path, s: &mut Stream) {
    if s.coin() {
        // An edge whose two ends are an ulp apart across a sub-scanline
        // boundary: its `dy` is under `f64::EPSILON`, so it is dropped, and
        // the sub-scanline it covered holds one crossing too few.
        let y = (s.below(15) as f64 - 7.0) / 16.0;
        let (a, b) = (s.range(-2.0, 10.0), s.range(12.0, 30.0));
        let low = y + s.range(2.0, 12.0);
        polygon(path, &[(a, y), (b, above(y)), (b, low), (a, low)], s.coin());
    } else {
        // A vertex past what sixteen times it can hold, or a slope past the
        // finite.
        let (x, y) = (s.coordinate(-2.0, 20.0), s.coordinate(-2.0, 20.0));
        let far = match s.below(3) {
            0 => (x + s.range(1.0, 9.0), 1.5e308),
            1 => (-1.7e308, y + s.range(0.5, 3.0)),
            _ => (1.7e308, y - s.range(0.5, 3.0)),
        };
        let last = (x + s.range(2.0, 9.0), y + s.range(2.0, 9.0));
        polygon(path, &[(x, y), far, last], s.coin());
    }
    if s.coin() {
        let at = (s.range(0.0, 20.0), s.range(-2.0, 12.0));
        let size = s.range(2.0, 10.0);
        shape(path, s, at, size);
    }
}

/// A stroke outline from the stream: any width and miter limit, sometimes
/// under a map that is not a similarity, with a floor. The cap, the join and
/// whether it is dashed come from `index`, so every eighteen paths hold every
/// cap under every join, dashed and solid.
fn stroked(index: u64, s: &mut Stream) -> Path {
    let mut line = Path::new();
    line.move_to(s.range(4.0, 30.0), s.range(4.0, 30.0));
    match s.below(3) {
        0 => {
            for _ in 0..s.below(5) + 1 {
                line.line_to(s.coordinate(2.0, 44.0), s.coordinate(2.0, 44.0));
            }
        }
        1 => {
            let c: Vec<f64> = (0..6).map(|_| s.range(0.0, 44.0)).collect();
            if let [x1, y1, x2, y2, x3, y3] = c[..] {
                line.curve_to(x1, y1, x2, y2, x3, y3);
            }
        }
        _ => {
            let c: Vec<f64> = (0..6).map(|_| s.range(0.0, 44.0)).collect();
            if let [cx, cy, x, y, lx, ly] = c[..] {
                line.quad_to(cx, cy, x, y);
                line.line_to(lx, ly);
            }
            if s.coin() {
                line.close();
            }
        }
    }
    let caps = [LineCap::Butt, LineCap::Round, LineCap::Square];
    let joins = [LineJoin::Miter, LineJoin::Round, LineJoin::Bevel];
    let dashes = if index / 9 % 2 == 1 {
        (0..s.below(4) + 1).map(|_| s.range(0.4, 6.0)).collect()
    } else {
        Vec::new()
    };
    let style = StrokeStyle {
        width: s.range(0.2, 6.0),
        cap: caps.get((index % 3) as usize).copied().unwrap_or_default(),
        join: joins
            .get((index / 3 % 3) as usize)
            .copied()
            .unwrap_or_default(),
        miter_limit: s.range(1.0, 12.0),
        dashes,
        dash_phase: s.range(0.0, 8.0),
    };
    let tolerance = s.range(0.05, 0.4);
    if s.below(4) == 0 {
        let map = Transform {
            a: s.range(0.3, 2.5),
            b: s.range(-0.6, 0.6),
            c: s.range(-0.6, 0.6),
            d: s.range(0.3, 2.5),
            e: s.range(-4.0, 4.0),
            f: s.range(-4.0, 4.0),
        };
        let floor = s.range(0.0, 1.2);
        stroke_mapped(&line, &style, &map, floor, tolerance, None)
    } else {
        stroke(&line, &style, tolerance, None)
    }
}

/// A region to fill over: around the shapes, partly over them, or one pixel,
/// and now and then empty or where nothing reaches.
pub(crate) fn region(s: &mut Stream) -> (i32, i32, u32, u32) {
    let small = |s: &mut Stream, n: u64| s.below(n) as i32;
    match s.below(10) {
        0 => (small(s, 40) - 8, small(s, 40) - 8, 1, 1),
        1 => (small(s, 40), small(s, 40), 0, s.below(8) as u32),
        2 => (200 + small(s, 40), 200, 9, 9),
        _ => (
            small(s, 24) - 12,
            small(s, 24) - 12,
            s.below(64) as u32 + 1,
            s.below(64) as u32 + 1,
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fill::{fill, fill_with, FillScratch};

    /// Paths generated of each class.
    const PATHS: u64 = 48;
    /// Regions each path is filled over, under each rule.
    const REGIONS: u64 = 3;

    /// **The safety net**: `fill` is the sweep, byte for byte, on every path
    /// the classes generate, at every region, under both rules — and so is
    /// every fill made back to back through one [`FillScratch`], over shapes
    /// and regions of every size, so nothing a fill leaves in its scratch
    /// reaches the next one's pixels.
    ///
    /// The harness counts what it ran, so it cannot pass by generating
    /// nothing: every case of every class is compared, a quarter of them at
    /// least find ink, the sharing and stroked classes put crossings of one
    /// sub-scanline into one pixel — counted by the sweep's own arithmetic —
    /// and the dropped class leaves sub-scanlines unclosed where no other
    /// class leaves one.
    #[test]
    fn fill_is_the_reference_sweep_on_generated_paths() {
        let mut s = Stream::new(0x7072_6573_7465);
        let mut scratch = FillScratch::new();
        for class in CLASSES {
            let (mut cases, mut inked) = (0u64, 0u64);
            let mut met = Census::default();
            for index in 0..PATHS {
                let path = generate(class, index, &mut s);
                let tolerance = s.range(0.05, 0.5);
                for _ in 0..REGIONS {
                    let (x0, y0, width, height) = region(&mut s);
                    for rule in [FillRule::NonZero, FillRule::EvenOdd] {
                        let want =
                            fill_reference(&path, rule, x0, y0, width, height, tolerance, None);
                        let got = fill(&path, rule, x0, y0, width, height, tolerance, None);
                        let reused = fill_with(
                            &mut scratch,
                            &path,
                            rule,
                            x0,
                            y0,
                            width,
                            height,
                            tolerance,
                            None,
                        );
                        assert_eq!(
                            reused.data, got.data,
                            "{class:?} path {index} under {rule:?}: through the one scratch"
                        );
                        assert_eq!(
                            (got.x0, got.y0, got.width, got.height),
                            (want.x0, want.y0, want.width, want.height),
                            "{class:?} path {index} under {rule:?}: the mask's rectangle"
                        );
                        if let Some(at) = got.data.iter().zip(&want.data).position(|(a, b)| a != b)
                        {
                            let across = i64::from(width.max(1));
                            panic!(
                                "{class:?} path {index} under {rule:?} over {x0},{y0} \
                                 {width}x{height} at tolerance {tolerance}: pixel ({}, {}) is \
                                 {:?} where the sweep has {:?}\n{:?}",
                                i64::from(x0) + at as i64 % across,
                                i64::from(y0) + at as i64 / across,
                                got.data.get(at),
                                want.data.get(at),
                                path.verbs()
                            );
                        }
                        cases += 1;
                        if want.data.iter().any(|v| *v != 0) {
                            inked += 1;
                        }
                        let found = census(&path, rule, x0, y0, width, height, tolerance);
                        met.shared += found.shared;
                        met.unclosed += found.unclosed;
                    }
                }
            }
            assert_eq!(cases, PATHS * REGIONS * 2, "{class:?}: every case ran");
            assert!(
                inked * 4 >= cases,
                "{class:?}: {inked} of {cases} cases drew anything"
            );
            match class {
                Class::Shared | Class::Stroked => assert!(
                    met.shared > 20_000,
                    "{class:?}: {} crossings shared a pixel on a sub-scanline",
                    met.shared
                ),
                Class::Dropped => assert!(
                    met.unclosed > 1_000,
                    "{class:?}: {} sub-scanlines left unclosed",
                    met.unclosed
                ),
                _ => {}
            }
            if class != Class::Dropped {
                assert_eq!(
                    met.unclosed, 0,
                    "{class:?}: a path that closes left a sub-scanline open"
                );
            }
        }
    }

    /// The reference stops as the filler does, and keeps what it had swept:
    /// the fourth question is asked at row `3 × STOP_EVERY`, and fires there.
    #[test]
    fn the_reference_stops_at_the_question_and_keeps_its_rows() {
        let mut path = Path::new();
        path.rect(5.0, 0.0, 30.0, 400.0);
        let asked = std::cell::Cell::new(0u32);
        let stop = || {
            asked.set(asked.get() + 1);
            asked.get() >= 4
        };
        let partial = fill_reference(&path, FillRule::NonZero, 0, 0, 40, 400, 0.1, Some(&stop));
        let cut = 3 * STOP_EVERY as i32;
        assert_eq!(partial.at(20, cut - 1), 255, "the rows before the cut drew");
        assert_eq!(partial.at(20, cut), 0, "the row it stopped on did not");
        asked.set(0);
        let filled = fill(&path, FillRule::NonZero, 0, 0, 40, 400, 0.1, Some(&stop));
        assert_eq!(
            partial.data, filled.data,
            "the filler stops at the same row"
        );
    }
}
