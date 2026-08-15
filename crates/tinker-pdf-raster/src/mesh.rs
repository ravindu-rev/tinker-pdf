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
//!   two triangles wound the same way contribute +0.5 and +0.5 at a shared
//!   edge and the pixel comes out whole. That is what removes the seam, and it
//!   is why [`draw_mesh`] normalizes each triangle's winding before emitting
//!   it: two triangles wound opposite ways would cancel to zero there, which is
//!   the same seam with the sign flipped;
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
use crate::fill::{fill, Mask};
use crate::geom::{FillRule, Path};

/// The most work one mesh may cost, summed over its triangles.
///
/// Both passes are charged to it, because they run away for different reasons
/// and a budget that only counted one of them would be no budget at all:
///
/// - the colour pass costs a triangle's clamped bounding box, so a mesh of
///   large overlapping triangles costs the page many times over;
/// - the coverage pass costs each triangle's three edges once per
///   sub-scanline it crosses, so a mesh of tall thin slivers costs almost
///   nothing in bounding boxes and a great deal in crossings. That is the
///   [`SLIVER_COST`] term, and without it a million degenerate triangles
///   stacked on ten pixels would pass a bounding-box budget untouched.
///
/// Sixty-seven million is well above a full-page mesh at 300 dpi, which is
/// about twenty million. Past it the mesh paints nothing and the caller
/// reports it, which is the answer `fill_with_tiles` already gives for a
/// lattice that will not fit: half a mesh reads as an artefact where nothing
/// reads as the gap it is.
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
/// measured is what makes a mesh far off the page cost nothing.
#[must_use]
pub fn draw_mesh(
    draw: &MeshDraw<'_>,
    x0: i32,
    y0: i32,
    width: u32,
    height: u32,
) -> Option<MeshBuffer> {
    let pixels = (width as usize).checked_mul(height as usize)?;
    if pixels == 0 || draw.components == 0 {
        return Some(MeshBuffer {
            coverage: Mask::empty(x0, y0, width, height),
            color: vec![[0; 4]; pixels],
        });
    }

    let region = Region {
        x0,
        y0,
        x1: i64::from(x0) + i64::from(width),
        y1: i64::from(y0) + i64::from(height),
    };

    // The budget first, over the whole mesh, before a pixel is touched: a
    // refusal must not depend on how far the render got.
    let mut work = 0u64;
    for triangle in draw.triangles {
        let Some(corners) = corners(draw, triangle) else {
            continue;
        };
        let Some(box_) = clamped_box(&corners, &region) else {
            continue;
        };
        let rows = (box_.y1 - box_.y0) as u64;
        let columns = (box_.x1 - box_.x0) as u64;
        work = work.saturating_add(rows.saturating_mul(columns));
        work = work.saturating_add(rows.saturating_mul(SLIVER_COST));
        if work > MAX_MESH_WORK {
            return None;
        }
    }

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
    let coverage = fill(
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

/// The colour pass: one scanline walk per triangle, interpolating the vertex
/// inputs and asking the caller what they paint.
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
        };
        draw_mesh(&draw, 0, 0, size, size).expect("it fits the budget")
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

    /// The same square as two triangles wound in *opposite* directions.
    ///
    /// This is the seam with the sign flipped: under the non-zero rule a
    /// clockwise triangle contributes -1 where its neighbour contributes +1,
    /// so the shared edge cancels to nothing and a black line runs down the
    /// diagonal. Normalizing the winding is what removes it, and this is the
    /// test that would catch its removal.
    #[test]
    fn a_triangle_wound_the_other_way_is_still_seamless() {
        let positions = [(0.0, 0.0), (20.0, 0.0), (20.0, 20.0), (0.0, 20.0)];
        let values = [0.0, 1.0, 1.0, 0.0];
        // The second triangle is [0, 3, 2] rather than [0, 2, 3]: the same
        // three corners, the other way round.
        let buffer = draw(&positions, &values, &[[0, 1, 2], [0, 3, 2]], 20);
        for y in 0..20 {
            for x in 0..20 {
                assert_eq!(buffer.coverage.at(x, y), 255, "at ({x}, {y})");
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
        };
        assert!(
            draw_mesh(&draw, 0, 0, 2000, 2000).is_none(),
            "40 000 page-tall slivers is past any sane budget"
        );

        // And the same mesh into a region only a few rows tall fits, because
        // the budget is charged on the clamped boxes.
        assert!(draw_mesh(&draw, 0, 0, 8, 4).is_some());
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
}
