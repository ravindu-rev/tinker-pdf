//! Gouraud-shaded triangle meshes, rasterized into one buffer.
//!
//! `07-rasterizer.md`'s non-goals name this as the seam: shadings are
//! decomposed by phase 08 into filled geometry, "if that proves to band, a
//! vertex-interpolated triangle primitive is added *here* but scheduled
//! *there*". It banded, and worse — this is that primitive.
//!
//! # Why one buffer rather than one fill per triangle
//!
//! A mesh is thousands of adjacent triangles. Filling each one through the
//! ordinary path — an anti-aliased coverage mask, composited — anti-aliases
//! **every shared edge against whatever is underneath**, so two triangles
//! meeting along an edge each contribute about half coverage there and the
//! result is about three quarters rather than whole. Repeated over a mesh that
//! is a visible lattice of pale seams, and it reads as a rendering bug in the
//! blend rather than as one in the shading, which is very hard to attribute.
//!
//! So the whole mesh is rasterized once, into two things:
//!
//! - **coverage**, from a single [`fill`] over one path holding every
//!   triangle, under the non-zero rule. The fill accumulates *signed* area, so
//!   two triangles meeting along an edge contribute half a pixel each and the
//!   pixel comes out whole. That is what removes the seam. [`draw_mesh`] also
//!   normalizes each triangle's winding, which is a narrower guarantee than it
//!   looks: a shared edge is safe either way, because the two crossings land
//!   on the same `x` and cancel out of the sweep, but an *overlap* between
//!   triangles wound opposite ways sums to a winding of zero and punches a
//!   hole the shape of the smaller one;
//! - **colour**, from a scanline walk per triangle that interpolates the
//!   vertex values barycentrically and asks the caller what colour they are.
//!   Overlapping writes along a shared edge are harmless because the two
//!   triangles agree there: linear interpolation along an edge depends only on
//!   its two endpoints, which both triangles share.
//!
//! # Ruling 8
//!
//! No PDF vocabulary crosses this boundary. The mesh arrives as positions,
//! opaque colour *inputs* and triangle indices, and the conversion from an
//! interpolated input vector to a device colour is a `&dyn Fn` the caller
//! supplies — the same shape `ImageDraw::stop` has used since gap 12. That is
//! also what keeps colour interpolation in the shading's own space: this crate
//! interpolates numbers it cannot interpret and converts nothing.

use crate::canvas::Color;
use crate::fill::{fill_with, FillScratch, Mask};
use crate::geom::{FillRule, Path};

/// The most work one mesh may cost, summed over its triangles.
///
/// Both passes are charged to it, because they run away for different reasons
/// and a budget that only counted one of them would be no budget at all:
///
/// - the colour pass is charged a triangle's clamped bounding box, the most
///   it can visit, so a mesh of large overlapping triangles costs the page
///   many times over;
/// - the coverage pass costs each triangle's three edges once per
///   sub-scanline it crosses, so a mesh of tall thin slivers costs almost
///   nothing in bounding boxes and a great deal in crossings. That is the
///   [`SLIVER_COST`] term, and without it a million degenerate triangles
///   stacked on ten pixels would pass a bounding-box budget untouched.
///
/// Counted in units of the space the mesh is drawn from — rows and columns
/// divided by [`MeshDraw::scale`], pixels at 72 dpi — and not in device
/// pixels, because the pixels a mesh covers grow with the render's scale
/// while the mesh stays the same mesh: counted in pixels, a page drew its
/// mesh at 300 dpi and left it out at 600, so whether a document showed it
/// was decided by the resolution it was asked for.
///
/// Sixty-seven million is well above a full-page mesh, which is about 1.2
/// million at 72 dpi. Past it the mesh paints nothing and the caller reports
/// it, which is the answer `fill_with_tiles` already gives for a lattice that
/// will not fit: half a mesh reads as an artefact where nothing reads as the
/// gap it is.
pub const MAX_MESH_WORK: u64 = 1 << 26;

/// What one row of a triangle costs the coverage sweep: three edges, at the
/// fill's sixteen sub-scanlines per row.
const SLIVER_COST: u64 = 48;

/// How far the colour buffer is spread into pixels the coverage mask reaches
/// and no triangle's interior does.
///
/// The silhouette is anti-aliased, so a pixel just outside the last one whose
/// *centre* is inside a triangle still has partial coverage and would have no
/// colour to composite. Anti-aliasing cannot reach further than one pixel from
/// an edge, so two rounds of four-neighbour spread cover it; each round works
/// from a snapshot of the last, so the result does not depend on which way the
/// scan runs.
const FRINGE_ROUNDS: usize = 2;

/// A mesh of Gouraud-shaded triangles, in device pixels.
pub struct MeshDraw<'a> {
    /// Vertex positions.
    pub positions: &'a [(f64, f64)],
    /// Colour inputs, [`MeshDraw::components`] per vertex, parallel to
    /// `positions`.
    pub values: &'a [f64],
    /// How many numbers one vertex's colour input is.
    pub components: usize,
    /// Triangles, as indices into `positions`.
    pub triangles: &'a [[u32; 3]],
    /// What one interpolated input vector paints.
    ///
    /// Called per pixel, after interpolation — never before it. Converting at
    /// the vertices and interpolating the results is a different picture
    /// wherever the conversion is not linear, which is every colour space and
    /// every function worth having.
    pub color: &'a dyn Fn(&[f64]) -> Color,
    /// Asked at each triangle and every sixteenth of its rows, which is the
    /// same scanline-band granularity [`fill`] promises.
    pub stop: Option<&'a dyn Fn() -> bool>,
    /// Device pixels to one unit of the space the mesh is drawn from, along
    /// each axis: the render's scale. [`MAX_MESH_WORK`] is counted in that
    /// space, so the same mesh is drawn or refused alike at every scale.
    pub scale: f64,
}

/// One mesh, rasterized.
pub struct MeshBuffer {
    /// The anti-aliased union of every triangle.
    pub coverage: Mask,
    /// `[r, g, b, painted]` per pixel of `coverage`'s rectangle, row-major.
    ///
    /// `painted` is 0 where no triangle reached the pixel, which is not the
    /// same question as whether it has coverage: a pixel on the silhouette can
    /// have coverage and no colour, which is what the fringe spread is for,
    /// and a pixel can have colour and no coverage, which composites nothing.
    pub color: Vec<[u8; 4]>,
}

impl MeshBuffer {
    /// The colour at a device pixel, or `None` where nothing was painted.
    #[must_use]
    pub fn at(&self, x: i32, y: i32) -> Option<Color> {
        let (Some(column), Some(row)) = (
            x.checked_sub(self.coverage.x0),
            y.checked_sub(self.coverage.y0),
        ) else {
            return None;
        };
        if column < 0
            || row < 0
            || column as u32 >= self.coverage.width
            || row as u32 >= self.coverage.height
        {
            return None;
        }
        let index = (row as usize) * (self.coverage.width as usize) + column as usize;
        let pixel = self.color.get(index)?;
        (pixel[3] != 0).then_some(Color {
            r: pixel[0],
            g: pixel[1],
            b: pixel[2],
            a: 0xFF,
        })
    }
}

/// Rasterizes a whole mesh into one buffer over the given region.
///
/// `None` refuses a mesh that would cost more than [`MAX_MESH_WORK`]. The
/// region is the caller's: a mesh is bounded by the clip in force exactly as
/// every other paint is, and clamping the triangles to it before the budget is
/// measured is what makes a mesh far off the page cost nothing. The mesh's
/// coverage is filled in `scratch` ([`fill_with`]).
#[must_use]
pub fn draw_mesh(
    draw: &MeshDraw<'_>,
    x0: i32,
    y0: i32,
    width: u32,
    height: u32,
    scratch: &mut FillScratch,
) -> Option<MeshBuffer> {
    let region = (x0, y0, width, height);
    draw_mesh_over(draw, region, region, scratch)
}

/// [`draw_mesh`] for a canvas that is a part of the picture the mesh is in.
///
/// `whole` is the region a render of the **whole** picture draws the mesh
/// over, and `wanted` is the part of it this caller will composite, both as
/// `(x0, y0, width, height)` in device pixels. The two are what make a tile's
/// mesh the page's mesh (ruling 5):
///
/// - **the budget is measured over `whole`**, so a tile refuses exactly the
///   meshes the page refuses, rather than accepting one whose part in the tile
///   happens to be cheap;
/// - **the mesh is drawn over `wanted` widened by the fringe's reach** and kept
///   inside `whole`, because the spread that colours an anti-aliased edge
///   reads [`FRINGE_ROUNDS`] pixels of neighbours. Drawn over `wanted` alone,
///   a silhouette pixel on the tile's edge had fewer neighbours than the same
///   pixel on the page and took its colour from a different one — measured
///   on `render_regions.rs`'s mesh fixture, 10 levels at 1x and 30 at 4x.
///
/// The buffer comes back over the widened region; the caller composites the
/// part it wants. With `wanted` equal to `whole` this is [`draw_mesh`].
#[must_use]
pub fn draw_mesh_over(
    draw: &MeshDraw<'_>,
    wanted: (i32, i32, u32, u32),
    whole: (i32, i32, u32, u32),
    scratch: &mut FillScratch,
) -> Option<MeshBuffer> {
    let empty = |(x0, y0, width, height): (i32, i32, u32, u32)| {
        let pixels = (width as usize).checked_mul(height as usize)?;
        Some(MeshBuffer {
            coverage: Mask::empty(x0, y0, width, height),
            color: vec![[0; 4]; pixels],
        })
    };
    if whole.2 == 0 || whole.3 == 0 || draw.components == 0 {
        return empty(whole);
    }
    let whole = Region::of(whole);

    // The budget first, over the whole mesh, before a pixel is touched: a
    // refusal must not depend on how far the render got — nor on how much of
    // the picture this caller asked for, nor on the scale it asked for it at.
    let scale = if draw.scale.is_finite() && draw.scale > 0.0 {
        draw.scale
    } else {
        1.0
    };
    let mut work = 0.0f64;
    for triangle in draw.triangles {
        let Some(corners) = corners(draw, triangle) else {
            continue;
        };
        let Some(box_) = clamped_box(&corners, &whole) else {
            continue;
        };
        let rows = (box_.y1 - box_.y0) as f64 / scale;
        let columns = (box_.x1 - box_.x0) as f64 / scale;
        work += rows * columns + rows * SLIVER_COST as f64;
        if work > MAX_MESH_WORK as f64 {
            return None;
        }
    }

    // `wanted`, widened by the fringe's reach and kept inside `whole`.
    let reach = FRINGE_ROUNDS as i64;
    let wanted = Region::of(wanted);
    let left = (i64::from(wanted.x0) - reach).max(i64::from(whole.x0));
    let top = (i64::from(wanted.y0) - reach).max(i64::from(whole.y0));
    let right = (wanted.x1 + reach).min(whole.x1).max(left);
    let bottom = (wanted.y1 + reach).min(whole.y1).max(top);
    // Both corners are inside `whole`, whose corner is an `i32` and whose
    // extent is a `u32`, so these conversions are exact.
    let (x0, y0) = (left as i32, top as i32);
    let (width, height) = ((right - left) as u32, (bottom - top) as u32);

    let pixels = (width as usize).checked_mul(height as usize)?;
    if pixels == 0 {
        return empty((x0, y0, width, height));
    }

    let region = Region {
        x0,
        y0,
        x1: right,
        y1: bottom,
    };

    // One path, one fill, one mask.
    let mut path = Path::new();
    for triangle in draw.triangles {
        let Some(corners) = corners(draw, triangle) else {
            continue;
        };
        let area = signed_area(&corners);
        if !area.is_finite() || area == 0.0 {
            continue; // A degenerate triangle covers no pixel and no span.
        }
        // Wound the same way as every other, so the non-zero accumulation adds
        // at a shared edge instead of cancelling.
        let order = if area > 0.0 { [0, 1, 2] } else { [0, 2, 1] };
        path.move_to(corners[order[0]].0, corners[order[0]].1);
        path.line_to(corners[order[1]].0, corners[order[1]].1);
        path.line_to(corners[order[2]].0, corners[order[2]].1);
        path.close();
    }
    // A mesh path holds nothing but straight lines, so the flattening
    // tolerance is never consulted; it is passed because `fill` takes one.
    let coverage = fill_with(
        scratch,
        &path,
        FillRule::NonZero,
        x0,
        y0,
        width,
        height,
        1.0,
        draw.stop,
    );

    let mut color = vec![[0u8; 4]; pixels];
    paint_triangles(draw, &region, width, &mut color);
    spread_fringe(&coverage, width, height, &mut color);

    Some(MeshBuffer { coverage, color })
}

/// The region a mesh is being drawn into, in device pixels.
struct Region {
    x0: i32,
    y0: i32,
    x1: i64,
    y1: i64,
}

impl Region {
    /// A rectangle given as `(x0, y0, width, height)`.
    fn of(rect: (i32, i32, u32, u32)) -> Region {
        let (x0, y0, width, height) = rect;
        Region {
            x0,
            y0,
            x1: i64::from(x0) + i64::from(width),
            y1: i64::from(y0) + i64::from(height),
        }
    }
}

/// A triangle's clamped pixel bounds, half-open.
struct Bounds {
    x0: i64,
    y0: i64,
    x1: i64,
    y1: i64,
}

/// One triangle's three vertex positions.
fn corners(draw: &MeshDraw<'_>, triangle: &[u32; 3]) -> Option<[(f64, f64); 3]> {
    let mut out = [(0.0, 0.0); 3];
    for (slot, index) in out.iter_mut().zip(triangle.iter()) {
        let point = draw.positions.get(*index as usize)?;
        if !point.0.is_finite() || !point.1.is_finite() {
            return None;
        }
        *slot = *point;
    }
    Some(out)
}

/// Twice the signed area, which is also the barycentric denominator.
fn signed_area(corners: &[(f64, f64); 3]) -> f64 {
    let [a, b, c] = *corners;
    (b.0 - a.0) * (c.1 - a.1) - (c.0 - a.0) * (b.1 - a.1)
}

/// The pixels a triangle can reach inside the region, or `None` for none.
fn clamped_box(corners: &[(f64, f64); 3], region: &Region) -> Option<Bounds> {
    let min = |f: fn(&(f64, f64)) -> f64| corners.iter().map(f).fold(f64::INFINITY, f64::min);
    let max = |f: fn(&(f64, f64)) -> f64| corners.iter().map(f).fold(f64::NEG_INFINITY, f64::max);
    // `floor` and `ceil` are correctly rounded, so this bound is the same on
    // every target (ruling 4's amendment names both as permitted).
    let low = |v: f64| v.floor().clamp(-1.0e18, 1.0e18) as i64;
    let high = |v: f64| v.ceil().clamp(-1.0e18, 1.0e18) as i64 + 1;

    let x0 = low(min(|p| p.0)).max(i64::from(region.x0));
    let y0 = low(min(|p| p.1)).max(i64::from(region.y0));
    let x1 = high(max(|p| p.0)).min(region.x1);
    let y1 = high(max(|p| p.1)).min(region.y1);
    (x1 > x0 && y1 > y0).then_some(Bounds { x0, y0, x1, y1 })
}

/// How far from the origin a triangle's coordinates may lie, in pixels, for
/// its rows to be walked by their spans ([`Spans`]).
const SPAN_REACH: f64 = 65_536.0;

/// How near a row's centre may come to a vertex, in pixels, and still be
/// walked by its span ([`Spans`]).
const SPAN_CLEARANCE: f64 = 1.0 / 1024.0;

/// How narrow a triangle may be, as twice its area over its height, and still
/// have its rows walked by their spans ([`Spans`]).
const SPAN_WIDTH: f64 = 0.25;

/// One edge of a triangle, for its crossings of a row.
#[derive(Clone, Copy)]
struct Crossing {
    /// An end of the edge.
    from: (f64, f64),
    /// The edge's extent down the page: it crosses a row whose centre lies
    /// strictly between the two.
    top: f64,
    bottom: f64,
    /// Its run over its rise, `dx / dy`, where it rises at all.
    slope: f64,
}

/// The columns of a triangle's rows that can hold a pixel its inside test
/// accepts.
///
/// In real arithmetic a row meets a triangle between two crossings of its
/// edges, and every pixel centre the triangle holds lies between them. The
/// inside test is rounded, so a centre within rounding of an edge may come out
/// on either side of it; the columns walked are the crossings widened by a
/// pixel each side, which holds every such centre wherever rounding moves the
/// test's answer, and the crossings themselves, by less than a pixel along the
/// row. That is so for a triangle whose coordinates lie within [`SPAN_REACH`]
/// of the origin and which is at least [`SPAN_WIDTH`] of a pixel wide, on a row
/// at least [`SPAN_CLEARANCE`] from every vertex. There an edge the row crosses
/// rises at least twice that clearance, a row the triangle misses passes at
/// least that far from it, and the test's rounding on coordinates that size,
/// under 10^-4 of a square pixel, moves its answer by well under a tenth of a
/// pixel along the row and cannot accept a centre on a row it misses.
/// Anywhere else — a row through a vertex, a sliver, a vertex far off the
/// page — the row is walked across the whole clamped box, so the pixels
/// painted are the box walk's in every case
/// (`a_span_walk_paints_what_the_box_walk_paints`).
struct Spans {
    edges: [Crossing; 3],
    /// The vertices' rows.
    rows: [f64; 3],
    /// Whether the triangle is one whose rows the crossings bound.
    bounded: bool,
}

impl Spans {
    fn new(corners: &[(f64, f64); 3], area: f64) -> Spans {
        let [a, b, c] = *corners;
        let edge = |from: (f64, f64), to: (f64, f64)| Crossing {
            from,
            top: from.1.min(to.1),
            bottom: from.1.max(to.1),
            slope: (to.0 - from.0) / (to.1 - from.1),
        };
        let near = corners
            .iter()
            .all(|&(x, y)| x.abs() <= SPAN_REACH && y.abs() <= SPAN_REACH);
        let height = a.1.max(b.1).max(c.1) - a.1.min(b.1).min(c.1);
        Spans {
            edges: [edge(a, b), edge(b, c), edge(c, a)],
            rows: [a.1, b.1, c.1],
            bounded: near && area.abs() >= height * SPAN_WIDTH,
        }
    }

    /// The columns of the row whose centre is `y` that can hold a pixel the
    /// inside test accepts, half-open and not yet clamped to the region:
    /// `None` where the row is walked across the whole clamped box.
    fn columns(&self, y: f64) -> Option<(i64, i64)> {
        if !self.bounded || self.rows.iter().any(|row| (y - row).abs() < SPAN_CLEARANCE) {
            return None;
        }
        let mut low = f64::INFINITY;
        let mut high = f64::NEG_INFINITY;
        for edge in &self.edges {
            // Strictly between, since the row clears both ends: an edge along
            // the row has its ends on it and has already sent the row whole.
            if y <= edge.top || y >= edge.bottom {
                continue;
            }
            let x = edge.from.0 + (y - edge.from.1) * edge.slope;
            low = low.min(x);
            high = high.max(x);
        }
        if low > high {
            return Some((0, 0)); // The row misses the triangle.
        }
        // The columns whose centres lie between the crossings, and a column
        // more each side. Both crossings lie within the triangle's own
        // coordinates, so the conversions are exact.
        let from = (low - 0.5).ceil() as i64 - 1;
        let to = (high - 0.5).floor() as i64 + 2;
        Some((from, to))
    }
}

/// The colour pass: one scanline walk per triangle, interpolating the vertex
/// inputs and asking the caller what they paint.
///
/// Each row walks its [`Spans::columns`] rather than the whole clamped box,
/// with the box walk's inside test and weights, so the pixels painted and the
/// colours they take are the box walk's.
fn paint_triangles(draw: &MeshDraw<'_>, region: &Region, width: u32, color: &mut [[u8; 4]]) {
    let mut mixed = vec![0.0f64; draw.components];
    for triangle in draw.triangles {
        if draw.stop.is_some_and(|stop| stop()) {
            return;
        }
        let Some(corners) = corners(draw, triangle) else {
            continue;
        };
        let area = signed_area(&corners);
        if !area.is_finite() || area == 0.0 {
            continue;
        }
        let Some(bounds) = clamped_box(&corners, region) else {
            continue;
        };
        let inputs: [&[f64]; 3] = [
            values_of(draw, triangle[0]),
            values_of(draw, triangle[1]),
            values_of(draw, triangle[2]),
        ];

        let [a, b, c] = corners;
        let spans = Spans::new(&corners, area);
        let mut painted = false;
        for py in bounds.y0..bounds.y1 {
            // Band granularity rather than row, for the reason `fill` gives:
            // a branch per row is a branch in the hottest loop, and sixteen
            // rows is tens of microseconds between one answer and the next.
            if (py - bounds.y0) % 16 == 0 && draw.stop.is_some_and(|stop| stop()) {
                return;
            }
            count::row();
            let y = py as f64 + 0.5;
            let (from, to) = match spans.columns(y) {
                Some((from, to)) => (from.max(bounds.x0), to.min(bounds.x1)),
                None => (bounds.x0, bounds.x1),
            };
            for px in from..to {
                count::visited();
                let x = px as f64 + 0.5;
                // The three barycentric weights, each an edge function over
                // the same denominator, so their signs agree whichever way the
                // triangle is wound and the test is one comparison each.
                let w0 = ((b.0 - x) * (c.1 - y) - (c.0 - x) * (b.1 - y)) / area;
                let w1 = ((c.0 - x) * (a.1 - y) - (a.0 - x) * (c.1 - y)) / area;
                let w2 = 1.0 - w0 - w1;
                if w0 < 0.0 || w1 < 0.0 || w2 < 0.0 {
                    continue;
                }
                count::inside();
                for (component, slot) in mixed.iter_mut().enumerate() {
                    let at = |v: &[f64]| v.get(component).copied().unwrap_or(0.0);
                    *slot = w0 * at(inputs[0]) + w1 * at(inputs[1]) + w2 * at(inputs[2]);
                }
                write(color, region, width, px, py, (draw.color)(&mixed));
                painted = true;
            }
        }

        if !painted {
            // A triangle thinner than a pixel contains no pixel centre at all,
            // so the loop above wrote nothing while the coverage sweep still
            // found spans for it. Its centroid colour goes in the one pixel
            // its centroid falls in, which gives the fringe spread something
            // to work from; the coverage mask still decides what composites.
            let centroid = ((a.0 + b.0 + c.0) / 3.0, (a.1 + b.1 + c.1) / 3.0);
            for (component, slot) in mixed.iter_mut().enumerate() {
                let at = |v: &[f64]| v.get(component).copied().unwrap_or(0.0);
                *slot = (at(inputs[0]) + at(inputs[1]) + at(inputs[2])) / 3.0;
            }
            let px = centroid.0.floor().clamp(-1.0e18, 1.0e18) as i64;
            let py = centroid.1.floor().clamp(-1.0e18, 1.0e18) as i64;
            if px >= bounds.x0 && px < bounds.x1 && py >= bounds.y0 && py < bounds.y1 {
                write(color, region, width, px, py, (draw.color)(&mixed));
            }
        }
    }
}

/// Work the colour pass does, counted for its tests: compiled only under
/// `cfg(test)` and kept per thread, so tests running side by side cannot add
/// into each other's.
#[cfg(test)]
mod count {
    use std::cell::Cell;

    std::thread_local! {
        static ROWS: Cell<u64> = const { Cell::new(0) };
        static VISITED: Cell<u64> = const { Cell::new(0) };
        static INSIDE: Cell<u64> = const { Cell::new(0) };
    }

    /// One row of a triangle's clamped box, walked or not.
    pub(super) fn row() {
        ROWS.with(|count| count.set(count.get() + 1));
    }

    /// One pixel whose inside test was taken.
    pub(super) fn visited() {
        VISITED.with(|count| count.set(count.get() + 1));
    }

    /// One pixel the inside test accepted.
    pub(super) fn inside() {
        INSIDE.with(|count| count.set(count.get() + 1));
    }

    /// The counts on this thread since the last call: rows, pixels visited and
    /// pixels inside.
    pub(super) fn take() -> (u64, u64, u64) {
        (
            ROWS.with(|count| count.replace(0)),
            VISITED.with(|count| count.replace(0)),
            INSIDE.with(|count| count.replace(0)),
        )
    }
}

/// The counters outside tests: nothing.
#[cfg(not(test))]
mod count {
    pub(super) fn row() {}

    pub(super) fn visited() {}

    pub(super) fn inside() {}
}

/// One vertex's colour inputs.
fn values_of<'a>(draw: &MeshDraw<'a>, index: u32) -> &'a [f64] {
    let start = (index as usize).saturating_mul(draw.components);
    draw.values
        .get(start..start + draw.components)
        .unwrap_or(&[])
}

/// Stores a colour, marking the pixel painted.
fn write(color: &mut [[u8; 4]], region: &Region, width: u32, px: i64, py: i64, value: Color) {
    let column = px - i64::from(region.x0);
    let row = py - i64::from(region.y0);
    if column < 0 || row < 0 {
        return;
    }
    let index = (row as usize) * (width as usize) + column as usize;
    if let Some(slot) = color.get_mut(index) {
        *slot = [value.r, value.g, value.b, 0xFF];
    }
}

/// Spreads colour into the anti-aliased fringe: pixels the coverage mask
/// reaches that no triangle's interior did.
fn spread_fringe(coverage: &Mask, width: u32, height: u32, color: &mut [[u8; 4]]) {
    if width == 0 || height == 0 {
        return;
    }
    for _ in 0..FRINGE_ROUNDS {
        // From a snapshot, so a pixel filled this round cannot feed the next
        // pixel in scan order and make the result depend on the direction.
        let previous = color.to_vec();
        let mut changed = false;
        for row in 0..height {
            for column in 0..width {
                let index = (row as usize) * (width as usize) + column as usize;
                if previous[index][3] != 0 {
                    continue;
                }
                let x = coverage.x0.saturating_add(column as i32);
                let y = coverage.y0.saturating_add(row as i32);
                if coverage.at(x, y) == 0 {
                    continue;
                }
                // The first neighbour in a fixed order, so two neighbours with
                // different colours resolve the same way every time.
                let neighbours = [
                    (column.checked_sub(1), Some(row)),
                    (Some(column + 1), Some(row)),
                    (Some(column), row.checked_sub(1)),
                    (Some(column), Some(row + 1)),
                ];
                for (nx, ny) in neighbours {
                    let (Some(nx), Some(ny)) = (nx, ny) else {
                        continue;
                    };
                    if nx >= width || ny >= height {
                        continue;
                    }
                    let source = (ny as usize) * (width as usize) + nx as usize;
                    if previous[source][3] != 0 {
                        color[index] = previous[source];
                        changed = true;
                        break;
                    }
                }
            }
        }
        if !changed {
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Grey from one input, so a colour is a number and an assertion can be
    /// read.
    fn grey(inputs: &[f64]) -> Color {
        let level = (inputs.first().copied().unwrap_or(0.0) * 255.0).clamp(0.0, 255.0) as u8;
        Color {
            r: level,
            g: level,
            b: level,
            a: 0xFF,
        }
    }

    fn draw(
        positions: &[(f64, f64)],
        values: &[f64],
        triangles: &[[u32; 3]],
        size: u32,
    ) -> MeshBuffer {
        let color = grey;
        let draw = MeshDraw {
            positions,
            values,
            components: 1,
            triangles,
            color: &color,
            stop: None,
            scale: 1.0,
        };
        draw_mesh(&draw, 0, 0, size, size, &mut FillScratch::new()).expect("it fits the budget")
    }

    /// **Milestone 2's exit criterion.** Two triangles sharing an edge
    /// interpolate colour across it with no seam.
    ///
    /// Two things are asserted, because only one of them is about the seam.
    /// The colour has to be continuous across the shared edge — which it is
    /// almost for free, since linear interpolation along an edge depends only
    /// on the two vertices both triangles share. The *coverage* is the part
    /// that goes wrong: filling each triangle separately gives about half
    /// coverage from each and about three quarters in total, and it is a
    /// visible pale line down the middle of the square.
    #[test]
    fn two_triangles_sharing_an_edge_have_no_seam_between_them() {
        // A 20 x 20 square split along its leading diagonal. The colour rises
        // left to right, so both triangles agree along the diagonal by
        // construction and any discontinuity there is the rasterizer's.
        let positions = [(0.0, 0.0), (20.0, 0.0), (20.0, 20.0), (0.0, 20.0)];
        let values = [0.0, 1.0, 1.0, 0.0];
        let buffer = draw(&positions, &values, &[[0, 1, 2], [0, 2, 3]], 20);

        for y in 0..20 {
            for x in 0..20 {
                assert_eq!(
                    buffer.coverage.at(x, y),
                    255,
                    "the square is solid: no seam at ({x}, {y})"
                );
                let pixel = buffer.at(x, y).expect("every pixel has a colour");
                // The ramp is linear in x, so the colour is decided by the
                // column alone. Two triangles that disagreed about the
                // diagonal would show as a row that is not flat.
                let want = ((x as f64 + 0.5) / 20.0 * 255.0).round() as i32;
                assert!(
                    (i32::from(pixel.r) - want).abs() <= 2,
                    "at ({x}, {y}) the colour is {} where the ramp says {want}",
                    pixel.r
                );
            }
        }
    }

    /// Triangles wound the other way still add rather than cancel.
    ///
    /// The shape of this test was decided by injection, and its first version
    /// was worthless. Deleting the winding normalization moved **no pixel
    /// anywhere in the workspace**, and the reason is worth writing down: two
    /// triangles that merely *share an edge* are disjoint, and their two
    /// crossings at that edge land on exactly the same `x` — the edge is built
    /// from the same two points, so the same slope and the same intercept come
    /// out whichever order the triangle names them. Opposite signs there
    /// cancel each other out of the sweep and leave the spans contiguous
    /// either way. The shared edge was never the case that needed normalizing.
    ///
    /// The case that does is an **overlap**. A clockwise triangle lying inside
    /// an anticlockwise one contributes -1 where the other contributes +1, the
    /// non-zero rule sees a winding of zero, and a hole is punched exactly the
    /// shape of the smaller triangle. A mesh stream is perfectly free to
    /// produce that: nothing in 8.7.4.5.5 requires a consistent orientation,
    /// and a folded gradient mesh overlaps itself by design.
    #[test]
    fn triangles_wound_the_other_way_do_not_punch_holes() {
        let positions = [
            (0.0, 0.0),
            (20.0, 0.0),
            (20.0, 20.0),
            (0.0, 20.0),
            // A smaller triangle well inside the square.
            (5.0, 5.0),
            (15.0, 6.0),
            (9.0, 15.0),
        ];
        let values = [0.5; 7];
        // The square anticlockwise, and the inner triangle the other way.
        let buffer = draw(&positions, &values, &[[0, 1, 2], [0, 2, 3], [4, 6, 5]], 20);
        for y in 0..20 {
            for x in 0..20 {
                assert_eq!(
                    buffer.coverage.at(x, y),
                    255,
                    "a hole at ({x}, {y}): an overlapping triangle wound the \
                     other way cancelled the winding underneath it"
                );
            }
        }
    }

    /// Colour is interpolated, not flat: a triangle whose corners differ is a
    /// gradient across its interior.
    #[test]
    fn colour_runs_across_a_triangle_rather_than_filling_it() {
        let positions = [(0.0, 0.0), (30.0, 0.0), (0.0, 30.0)];
        let values = [0.0, 1.0, 0.5];
        let buffer = draw(&positions, &values, &[[0, 1, 2]], 30);

        let corner = buffer.at(0, 0).expect("the dark corner").r;
        let far = buffer.at(28, 0).expect("the light corner").r;
        let low = buffer.at(0, 28).expect("the middle corner").r;
        assert!(corner < 20, "corner is {corner}");
        assert!(far > 230, "far is {far}");
        assert!((110..=145).contains(&low), "low is {low}");

        // And it is monotone along the top row, which a flat fill is not.
        let mut previous = 0;
        for x in 0..25 {
            let value = buffer.at(x, 0).expect("a colour").r;
            assert!(value >= previous, "the ramp went backwards at {x}");
            previous = value;
        }
    }

    /// The silhouette keeps its anti-aliasing, and the fringe has a colour to
    /// composite.
    #[test]
    fn the_outer_edge_is_soft_and_still_coloured() {
        // Half-pixel edges on the right and bottom.
        let positions = [(2.0, 2.0), (10.5, 2.0), (10.5, 10.5), (2.0, 10.5)];
        let values = [0.4, 0.4, 0.4, 0.4];
        let buffer = draw(&positions, &values, &[[0, 1, 2], [0, 2, 3]], 16);

        let edge = buffer.coverage.at(10, 5);
        assert!(
            (110..=145).contains(&edge),
            "a half-covered pixel should be near 128, got {edge}"
        );
        let colour = buffer.at(10, 5).expect("the fringe is coloured");
        assert_eq!(colour.r, 102, "and it is the mesh's colour, not black");
    }

    /// A mesh far larger than the region costs the region, not the mesh.
    #[test]
    fn a_mesh_is_bounded_by_the_region_it_is_drawn_into() {
        let positions = [(-1.0e6, -1.0e6), (1.0e6, -1.0e6), (0.0, 1.0e6)];
        let values = [0.0, 0.5, 1.0];
        let buffer = draw(&positions, &values, &[[0, 1, 2]], 8);
        assert_eq!(buffer.color.len(), 64);
        assert_eq!(buffer.coverage.at(4, 4), 255);
    }

    /// The budget refuses rather than running: a mesh of tall slivers costs
    /// almost no bounding box and a great deal of sweep, which is why the
    /// budget charges both.
    #[test]
    fn a_mesh_past_its_work_budget_is_refused_whole() {
        let mut positions = Vec::new();
        let mut values = Vec::new();
        let mut triangles = Vec::new();
        // Slivers one pixel wide and two thousand tall, stacked. Their
        // bounding boxes alone are 2e3 pixels each; the sweep term is what
        // makes them expensive.
        for index in 0..40_000u32 {
            let x = f64::from(index % 3);
            positions.push((x, 0.0));
            positions.push((x + 0.5, 0.0));
            positions.push((x, 2000.0));
            values.extend_from_slice(&[0.0, 0.5, 1.0]);
            let base = index * 3;
            triangles.push([base, base + 1, base + 2]);
        }

        let color = grey;
        let draw = MeshDraw {
            positions: &positions,
            values: &values,
            components: 1,
            triangles: &triangles,
            color: &color,
            stop: None,
            scale: 1.0,
        };
        assert!(
            draw_mesh(&draw, 0, 0, 2000, 2000, &mut FillScratch::new()).is_none(),
            "40 000 page-tall slivers is past any sane budget"
        );

        // And the same mesh into a region only a few rows tall fits, because
        // the budget is charged on the clamped boxes.
        assert!(draw_mesh(&draw, 0, 0, 8, 4, &mut FillScratch::new()).is_some());
    }

    /// Whether a mesh is affordable is a question about the mesh, so every
    /// scale answers it alike. Two thousand slivers a hundred units tall fit
    /// the budget at the space's own scale, and drawn eight times as large —
    /// eight times the rows their edges cross, more than sixty times the
    /// pixels in their boxes — they are drawn too, where a budget counted in
    /// device pixels refused them; forty thousand two thousand units tall are
    /// refused at both.
    #[test]
    fn a_mesh_is_drawn_or_refused_alike_at_every_scale() {
        for scale in [1.0, 8.0] {
            for (count, tall, drawn) in [(2_000u32, 100.0, true), (40_000, 2000.0, false)] {
                let mut positions = Vec::new();
                let mut values = Vec::new();
                let mut triangles = Vec::new();
                for index in 0..count {
                    let x = f64::from(index % 3);
                    positions.push((x * scale, 0.0));
                    positions.push(((x + 0.5) * scale, 0.0));
                    positions.push((x * scale, tall * scale));
                    values.extend_from_slice(&[0.0, 0.5, 1.0]);
                    let base = index * 3;
                    triangles.push([base, base + 1, base + 2]);
                }
                let color = grey;
                let draw = MeshDraw {
                    positions: &positions,
                    values: &values,
                    components: 1,
                    triangles: &triangles,
                    color: &color,
                    stop: None,
                    scale,
                };
                let width = (4.0 * scale) as u32;
                let height = (tall * scale) as u32;
                assert_eq!(
                    draw_mesh(&draw, 0, 0, width, height, &mut FillScratch::new()).is_some(),
                    drawn,
                    "{count} slivers {tall} tall at {scale}x"
                );
            }
        }
    }

    /// Ruling 4's weaker half, checked here because a mesh has two passes and
    /// a spread between them: the same input twice is the same bytes.
    #[test]
    fn a_mesh_rasterizes_the_same_way_twice() {
        let positions = [(1.3, 2.7), (18.1, 4.4), (9.9, 17.5), (2.2, 15.1)];
        let values = [0.1, 0.9, 0.4, 0.7];
        let one = draw(&positions, &values, &[[0, 1, 2], [0, 2, 3]], 20);
        let two = draw(&positions, &values, &[[0, 1, 2], [0, 2, 3]], 20);
        assert_eq!(one.coverage.data, two.coverage.data);
        assert_eq!(one.color, two.color);
    }

    /// Degenerate input paints nothing rather than panicking (ruling 1).
    #[test]
    fn degenerate_triangles_are_dropped() {
        let positions = [
            (0.0, 0.0),
            (0.0, 0.0),
            (0.0, 0.0),
            (f64::NAN, 1.0),
            (2.0, f64::INFINITY),
        ];
        let values = [0.0; 5];
        let buffer = draw(&positions, &values, &[[0, 1, 2], [0, 3, 4], [9, 9, 9]], 8);
        assert!(buffer.coverage.data.iter().all(|&v| v == 0));
        assert!(buffer.color.iter().all(|p| p[3] == 0));
    }

    /// The colour pass as it was before a row walked its span: every column of
    /// the clamped box, through the same inside test and weights. Kept, word
    /// for word, as the reference `a_span_walk_paints_what_the_box_walk_paints`
    /// holds the span walk to.
    fn paint_triangles_by_box(
        draw: &MeshDraw<'_>,
        region: &Region,
        width: u32,
        color: &mut [[u8; 4]],
    ) {
        let mut mixed = vec![0.0f64; draw.components];
        for triangle in draw.triangles {
            if draw.stop.is_some_and(|stop| stop()) {
                return;
            }
            let Some(corners) = corners(draw, triangle) else {
                continue;
            };
            let area = signed_area(&corners);
            if !area.is_finite() || area == 0.0 {
                continue;
            }
            let Some(bounds) = clamped_box(&corners, region) else {
                continue;
            };
            let inputs: [&[f64]; 3] = [
                values_of(draw, triangle[0]),
                values_of(draw, triangle[1]),
                values_of(draw, triangle[2]),
            ];

            let [a, b, c] = corners;
            let mut painted = false;
            for py in bounds.y0..bounds.y1 {
                // Band granularity rather than row, for the reason `fill` gives:
                // a branch per row is a branch in the hottest loop, and sixteen
                // rows is tens of microseconds between one answer and the next.
                if (py - bounds.y0) % 16 == 0 && draw.stop.is_some_and(|stop| stop()) {
                    return;
                }
                let y = py as f64 + 0.5;
                for px in bounds.x0..bounds.x1 {
                    let x = px as f64 + 0.5;
                    // The three barycentric weights, each an edge function over
                    // the same denominator, so their signs agree whichever way the
                    // triangle is wound and the test is one comparison each.
                    let w0 = ((b.0 - x) * (c.1 - y) - (c.0 - x) * (b.1 - y)) / area;
                    let w1 = ((c.0 - x) * (a.1 - y) - (a.0 - x) * (c.1 - y)) / area;
                    let w2 = 1.0 - w0 - w1;
                    if w0 < 0.0 || w1 < 0.0 || w2 < 0.0 {
                        continue;
                    }
                    for (component, slot) in mixed.iter_mut().enumerate() {
                        let at = |v: &[f64]| v.get(component).copied().unwrap_or(0.0);
                        *slot = w0 * at(inputs[0]) + w1 * at(inputs[1]) + w2 * at(inputs[2]);
                    }
                    write(color, region, width, px, py, (draw.color)(&mixed));
                    painted = true;
                }
            }

            if !painted {
                // A triangle thinner than a pixel contains no pixel centre at all,
                // so the loop above wrote nothing while the coverage sweep still
                // found spans for it. Its centroid colour goes in the one pixel
                // its centroid falls in, which gives the fringe spread something
                // to work from; the coverage mask still decides what composites.
                let centroid = ((a.0 + b.0 + c.0) / 3.0, (a.1 + b.1 + c.1) / 3.0);
                for (component, slot) in mixed.iter_mut().enumerate() {
                    let at = |v: &[f64]| v.get(component).copied().unwrap_or(0.0);
                    *slot = (at(inputs[0]) + at(inputs[1]) + at(inputs[2])) / 3.0;
                }
                let px = centroid.0.floor().clamp(-1.0e18, 1.0e18) as i64;
                let py = centroid.1.floor().clamp(-1.0e18, 1.0e18) as i64;
                if px >= bounds.x0 && px < bounds.x1 && py >= bounds.y0 && py < bounds.y1 {
                    write(color, region, width, px, py, (draw.color)(&mixed));
                }
            }
        }
    }

    /// A colour every input reaches, so two walks that painted different
    /// pixels, or the same pixel from different weights, paint differently.
    fn spread(inputs: &[f64]) -> Color {
        let level =
            |at: usize| (inputs.get(at).copied().unwrap_or(0.5) * 251.0).clamp(0.0, 255.0) as u8;
        Color {
            r: level(0),
            g: level(1),
            b: level(0) ^ level(1).rotate_left(3),
            a: 0xFF,
        }
    }

    /// A small deterministic generator for the cases below.
    struct Xorshift(u64);

    impl Xorshift {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0
        }

        /// A number in `low..high`.
        fn within(&mut self, low: f64, high: f64) -> f64 {
            low + (high - low) * ((self.next() >> 11) as f64 / (1u64 << 53) as f64)
        }

        /// One of `choices`.
        fn pick<T: Copy>(&mut self, choices: &[T]) -> T {
            choices[(self.next() % choices.len() as u64) as usize]
        }
    }

    /// Runs both walks over one mesh in one region and holds them equal.
    fn both_walks_agree(
        positions: &[(f64, f64)],
        values: &[f64],
        triangles: &[[u32; 3]],
        rect: (i32, i32, u32, u32),
        case: &str,
    ) {
        let color = spread;
        let draw = MeshDraw {
            positions,
            values,
            components: 2,
            triangles,
            color: &color,
            stop: None,
            scale: 1.0,
        };
        let region = Region::of(rect);
        let pixels = (rect.2 as usize) * (rect.3 as usize);
        let mut spans = vec![[0u8; 4]; pixels];
        let mut boxes = vec![[0u8; 4]; pixels];
        paint_triangles(&draw, &region, rect.2, &mut spans);
        paint_triangles_by_box(&draw, &region, rect.2, &mut boxes);
        let _ = count::take();
        if let Some(first) = (0..pixels).find(|&at| spans[at] != boxes[at]) {
            panic!(
                "{case}: the span walk painted {:?} and the box walk {:?} at pixel {first} of \
                 {rect:?}, over {positions:?} as {triangles:?}",
                spans[first], boxes[first]
            );
        }
    }

    /// **The pixels a span walk paints are the box walk's, and so are their
    /// colours,** wherever rounding decides the inside test: over triangles
    /// at random, vertices on and beside pixel centres and pixel edges, edges
    /// a hair from level, slivers, specks, vertices far off the region and
    /// past the span reach, and regions far from the origin.
    #[test]
    fn a_span_walk_paints_what_the_box_walk_paints() {
        let mut rng = Xorshift(0x9e37_79b9_7f4a_7c15);
        let hair = [
            1e-14,
            1e-12,
            1e-9,
            1e-6,
            1.0 / 2048.0,
            1.0 / 1024.0,
            1.0 / 512.0,
        ];
        let far = [-1e300, -1e9, -70_000.0, 70_000.0, 1e6, 1e300];
        for round in 0..400 {
            // The region: at the origin, or far out where coordinates are
            // large but within reach, or past it.
            let (x0, y0) = rng.pick(&[(0, 0), (0, 0), (-40, 17), (30_000, -30_000), (65_500, 20)]);
            let rect = (x0, y0, 48, 40);
            let at = |along: f64, down: f64| (f64::from(x0) + along, f64::from(y0) + down);
            let mut positions = Vec::new();
            let kind = round % 8;
            for _ in 0..9 {
                let point = match kind {
                    // At random, some outside the region.
                    0 | 1 => {
                        let (along, down) = (rng.within(-12.0, 60.0), rng.within(-12.0, 52.0));
                        at(along, down)
                    }
                    // On a pixel centre or a pixel edge, either axis.
                    2 => {
                        let along = f64::from((rng.next() % 50) as u32) - 1.0;
                        let down = f64::from((rng.next() % 42) as u32) - 1.0;
                        let half = |rng: &mut Xorshift| rng.pick(&[0.0, 0.5]);
                        let (a, b) = (half(&mut rng), half(&mut rng));
                        at(along + a, down + b)
                    }
                    // A hair from a pixel centre's row.
                    3 => {
                        let along = rng.within(-4.0, 52.0);
                        let row = f64::from((rng.next() % 40) as u32) + 0.5;
                        let sign = rng.pick(&[-1.0, 1.0]);
                        let offset = sign * rng.pick(&hair);
                        at(along, row + offset)
                    }
                    // Specks: three points within a pixel.
                    4 => {
                        let (along, down) = (rng.within(0.0, 46.0), rng.within(0.0, 38.0));
                        let (dx, dy) = (rng.within(0.0, 1.0), rng.within(0.0, 1.0));
                        at(along + dx, down + dy)
                    }
                    // Vertices far off the region, past the span reach.
                    5 => {
                        if rng.next() % 3 == 0 {
                            (rng.pick(&far), rng.within(-4.0, 44.0) + f64::from(y0))
                        } else {
                            let (along, down) = (rng.within(-4.0, 52.0), rng.within(-4.0, 44.0));
                            at(along, down)
                        }
                    }
                    _ => (0.0, 0.0), // Laid out below.
                };
                positions.push(point);
            }
            let mut triangles: Vec<[u32; 3]> =
                (0..3).map(|t| [3 * t, 3 * t + 1, 3 * t + 2]).collect();
            match kind {
                // Edges a hair from level: two vertices far apart across the
                // region, their rows a hair apart, one of them on or beside a
                // pixel centre's row.
                6 => {
                    for t in 0..3 {
                        let row = f64::from((rng.next() % 40) as u32) + rng.pick(&[0.5, 0.25]);
                        let rise = rng.pick(&[0.0, 1e-15, 1e-13, 1e-10, 1e-7, 1.0 / 2048.0]);
                        let apex = rng.within(-30.0, 30.0);
                        let (left, right, top) = (
                            rng.within(-8.0, 10.0),
                            rng.within(38.0, 56.0),
                            rng.within(0.0, 48.0),
                        );
                        positions[3 * t] = at(left, row);
                        positions[3 * t + 1] = at(right, row + rise);
                        positions[3 * t + 2] = at(top, row + apex);
                    }
                }
                // Slivers: a third vertex a hair off the line through two.
                7 => {
                    for t in 0..3 {
                        let (ax, ay) = (rng.within(-6.0, 54.0), rng.within(-6.0, 46.0));
                        let (bx, by) = (rng.within(-6.0, 54.0), rng.within(-6.0, 46.0));
                        let share = rng.within(0.0, 1.0);
                        let (nx, ny) = (ay - by, bx - ax);
                        let off = rng.pick(&[0.0, 1e-12, 1e-6, 1e-3, 0.05, 0.3]);
                        positions[3 * t] = at(ax, ay);
                        positions[3 * t + 1] = at(bx, by);
                        positions[3 * t + 2] = at(
                            ax + share * (bx - ax) + off * nx,
                            ay + share * (by - ay) + off * ny,
                        );
                    }
                }
                _ => {}
            }
            // Shared vertices and overlaps as a mesh has them.
            triangles.push([0, 4, 8]);
            triangles.push([2, 3, 7]);
            let values: Vec<f64> = (0..positions.len() * 2)
                .map(|_| rng.within(0.0, 1.0))
                .collect();
            both_walks_agree(
                &positions,
                &values,
                &triangles,
                rect,
                &format!("round {round}"),
            );
        }
    }

    /// **P-14.2's exit.** A row of a triangle visits the pixel centres inside
    /// it and two columns more, not its clamped box: over three triangles
    /// whose vertices stand clear of every row's centre, the pixels visited
    /// are at most those inside plus two a row, and fewer than the box holds.
    #[test]
    fn a_triangle_visits_its_span_not_its_box() {
        let color = grey;
        let region = Region::of((0, 0, 64, 64));
        for corners in [
            [(2.3, 1.2), (61.7, 9.9), (30.1, 58.3)],
            [(60.6, 3.7), (4.2, 33.4), (59.1, 62.8)],
            [(10.2, 2.9), (13.8, 61.1), (52.4, 40.3)],
        ] {
            let values = [0.0, 0.5, 1.0];
            let draw = MeshDraw {
                positions: &corners,
                values: &values,
                components: 1,
                triangles: &[[0, 1, 2]],
                color: &color,
                stop: None,
                scale: 1.0,
            };
            let mut buffer = vec![[0u8; 4]; 64 * 64];
            let _ = count::take();
            paint_triangles(&draw, &region, 64, &mut buffer);
            let (rows, visited, inside) = count::take();
            let boxed = clamped_box(&corners, &region).expect("the triangle is in the region");
            let box_pixels = ((boxed.x1 - boxed.x0) * (boxed.y1 - boxed.y0)) as u64;
            assert_eq!(rows, (boxed.y1 - boxed.y0) as u64, "every row of the box");
            assert!(inside > 0, "{corners:?} holds pixel centres");
            assert!(
                visited <= inside + 2 * rows,
                "{corners:?}: {visited} visits for {inside} pixels inside over {rows} rows"
            );
            assert!(
                visited < box_pixels,
                "{corners:?}: {visited} visits, no fewer than the box's {box_pixels}"
            );
        }
    }

    /// The span of a row, and the rows walked whole: a row the crossings
    /// bound walks the columns between them and one more each side; a row
    /// the triangle misses walks none; a row within the clearance of a vertex,
    /// any row of a sliver, and any row of a triangle reaching past the span
    /// reach are walked across the whole box.
    #[test]
    fn a_row_walks_its_crossings_widened_or_the_whole_box() {
        let spans = |corners: [(f64, f64); 3]| Spans::new(&corners, signed_area(&corners));

        // Crossings at 10.25 and 20.371875 on the row at 20.5: centres from
        // column 10 to 19, and the columns 9 and 20 beside them.
        let right = spans([(10.25, 0.25), (30.75, 0.25), (10.25, 40.25)]);
        assert_eq!(right.columns(20.5), Some((9, 21)));
        assert_eq!(right.columns(41.5), Some((0, 0)), "below the triangle");

        let ordinary = spans([(2.3, 1.2), (61.7, 9.9), (30.1, 58.3)]);
        assert!(ordinary.columns(30.5).is_some());
        assert_eq!(ordinary.columns(0.5), Some((0, 0)), "above it");

        // A vertex on a row's centre, within the clearance of one, and just
        // outside it.
        let on = spans([(2.3, 10.5), (61.7, 1.9), (30.1, 58.3)]);
        assert_eq!(on.columns(10.5), None);
        assert!(on.columns(11.5).is_some());
        let within = spans([(2.3, 20.5 + 1.0 / 2048.0), (61.7, 1.9), (30.1, 58.3)]);
        assert_eq!(within.columns(20.5), None);
        let clear = spans([(2.3, 20.5 + 1.0 / 512.0), (61.7, 1.9), (30.1, 58.3)]);
        assert!(clear.columns(20.5).is_some());

        // A sliver, a third of a pixel wide over sixty rows: twice its area
        // is 2.95 against a height of 59.7.
        let sliver = spans([(0.3, 0.2), (60.7, 59.9), (30.6, 30.1)]);
        assert_eq!(sliver.columns(15.5), None);

        // A vertex past the span reach, either axis.
        let far = spans([(2.3, 1.2), (70_000.7, 9.9), (30.1, 58.3)]);
        assert_eq!(far.columns(5.5), None);
        let deep = spans([(2.3, -70_000.2), (61.7, 9.9), (30.1, 58.3)]);
        assert_eq!(deep.columns(5.5), None);
    }

    /// A triangle with no pixel centre inside still puts its centroid's
    /// colour in its centroid's pixel, as the box walk did: the span walk
    /// paints nothing more and nothing less of it.
    #[test]
    fn a_triangle_holding_no_centre_still_paints_its_centroid() {
        // Lying between two rows' centres.
        let positions = [(3.1, 4.6), (19.7, 4.8), (11.2, 4.9)];
        let values = [0.2, 0.2, 0.8, 0.8, 0.5, 0.5];
        both_walks_agree(
            &positions,
            &values,
            &[[0, 1, 2]],
            (0, 0, 24, 12),
            "a sliver",
        );
        let color = spread;
        let draw = MeshDraw {
            positions: &positions,
            values: &values,
            components: 2,
            triangles: &[[0, 1, 2]],
            color: &color,
            stop: None,
            scale: 1.0,
        };
        let mut buffer = vec![[0u8; 4]; 24 * 12];
        paint_triangles(&draw, &Region::of((0, 0, 24, 12)), 24, &mut buffer);
        let painted: Vec<usize> = (0..buffer.len()).filter(|&at| buffer[at][3] != 0).collect();
        // The centroid is (11.33..., 4.766...): column 11 of row 4.
        assert_eq!(painted, vec![4 * 24 + 11]);
        let _ = count::take();
    }
}
