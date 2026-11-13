//! Mesh shadings: the packed vertex stream, and the geometry it describes
//! (8.7.4.5.5 to 8.7.4.5.8).
//!
//! Types 4 and 5 are triangles outright. Types 6 and 7 are bicubic surfaces —
//! Coons and tensor patches — which are subdivided into triangles here and
//! handed to the same rasteriser, so the two patch types are a front end on
//! the two triangle types rather than a second implementation.
//!
//! # What is in shading space and what is in device space
//!
//! Everything a stream carries is in *shading* space, because that is the only
//! space the file names. The subdivision of a patch is chosen in **device**
//! space, because how finely a curve must be broken up depends on how large it
//! is going to be drawn: a patch subdivided in shading space is either far too
//! coarse at 300 dpi or far too fine at a thumbnail. So [`Mesh`] holds patches
//! and [`Mesh::tessellate`] takes the matrix.
//!
//! # Smoothness
//!
//! A patch's colour is bilinear over its parameters (8.7.4.5.7) and a triangle
//! of the grid carries it linearly, so the grid misses the bilinear term.
//! `/SM` (10.7.3) bounds that error: under it a patch takes the larger of its
//! device-length count and the count whose colour error is within the
//! smoothness, `docs/design/device-dependent-state.md`'s rule. Triangles
//! approximate nothing it bounds — 8.7.4.5.5 defines their colour as the
//! linear interpolation they are drawn with — so it reaches patches alone.
//!
//! # Determinism
//!
//! Ruling 4. Two things here could have made a mesh render differently on a
//! 32-bit target, and both are the shape gap 13 found in the flattener — a
//! floating-point comparison deciding a *count* rather than a position:
//!
//! - the subdivision step count. It is chosen by [`patch_steps`], which builds
//!   a length in `f64` from nothing but subtraction, addition and `abs`, casts
//!   it **once** into 16.16 fixed point, and then compares integers. The loop
//!   doubles its threshold rather than halving the length, because shifting
//!   truncates and a length a hair over the threshold would stop one step
//!   short. The count smoothness asks for, [`colour_steps`], is the same kind
//!   of decision: a colour measure built from the operations IEEE 754 pins
//!   exactly, cast once, and compared as integers;
//! - the count of vertices a stream yields. That is decided by bit arithmetic
//!   on `u64` and by `usize` comparisons against caps that are constants, so
//!   it does not depend on float rounding at all.
//!
//! No transcendental is called from any of it, which `cargo xtask libm`
//! enforces.

use tinker_pdf_color::{ColorSpace, Function};
use tinker_pdf_content::Matrix;

use crate::shading::ShadingScratch;

/// The most vertices one packed stream may yield.
///
/// A vertex stream is attacker-sized: `/BitsPerCoordinate 1` with one
/// one-bit component packs a vertex into five bits, so a megabyte of stream is
/// 1.6 million of them. The ceiling is checked against the stream's *length*
/// before anything is allocated, and again as vertices are appended, because
/// the two bound different things — the first stops a hostile `/Length` from
/// reserving gigabytes, the second stops a stream that really is that long.
pub const MAX_MESH_VERTICES: usize = 1 << 18;

/// The most patches one type 6 or type 7 stream may yield.
///
/// Each patch is subdivided, so a patch costs far more than a vertex; this is
/// the ceiling that keeps [`Mesh::tessellate`] from being handed more work
/// than [`MAX_MESH_TRIANGLES`] could ever absorb.
pub const MAX_MESH_PATCHES: usize = 1 << 14;

/// The most triangles one mesh may be tessellated into.
///
/// This is a *total*, and it is deliberately not the same kind of number as
/// [`MAX_MESH_PATCHES`]. A per-item cap is not a work cap the moment the
/// structure branches — which is what `MAX_TILE_WORK` and `MAX_GROUP_BUFFERS`
/// both exist to say one layer up — and a patch branches: sixteen thousand
/// patches each subdividing to two thousand triangles is thirty-three million
/// triangles inside a per-patch limit that never fires.
pub const MAX_MESH_TRIANGLES: usize = 1 << 17;

/// The most subdivisions a single patch's edge is broken into.
///
/// A power of two, because [`patch_steps`] reaches it by doubling. Thirty-two
/// steps is 2 048 triangles for one patch, which is finer than any patch that
/// fits on a page needs. It caps the count smoothness asks for too
/// ([`colour_steps`]), whatever `/SM` asks.
const MAX_PATCH_STEPS: u32 = 32;

/// The finest colour tolerance smoothness subdivides a patch to: one level of
/// 255, below which an 8-bit component's error cannot change a byte
/// (10.7.3 lets a device hold "internal limits on the maximum and minimum
/// tolerances attainable").
const FINEST_SMOOTHNESS: f64 = 1.0 / 255.0;

/// How many intervals a mesh's function is sampled over to measure its slope
/// for smoothness ([`sampled_slope`]): as many as a transfer function's table
/// has entries less one.
const SLOPE_INTERVALS: u32 = 256;

/// The device-space length one subdivision step aims at, in pixels.
///
/// The same idea as the flattener's tolerance and deliberately coarser: a
/// patch is a coloured surface rather than an outline, so the error that
/// matters is the colour's, and colour varies far more slowly across a patch
/// than the silhouette does along a curve.
const PATCH_STEP_PIXELS: f64 = 3.0;

/// One Coons or tensor patch, in shading space (8.7.4.5.7, 8.7.4.5.8).
///
/// A Coons patch is stored as the tensor patch it is equal to: 8.7.4.5.7 gives
/// the four internal control points as a formula over the twelve boundary
/// ones, so converting on the way in means there is one surface evaluator
/// rather than two.
#[derive(Clone, Debug)]
pub struct Patch {
    /// The sixteen control points, `points[u][v]`, so `points[0][0]` is the
    /// corner at `(u, v) = (0, 0)`.
    pub points: [[(f64, f64); 4]; 4],
    /// The four corner colour inputs, `components` each, at `(0, 0)`,
    /// `(0, 1)`, `(1, 1)` and `(1, 0)` — the order 8.7.4.5.7 numbers them
    /// `c1` to `c4` in.
    pub corners: Vec<f64>,
}

/// A mesh shading: types 4 to 7, decoded (8.7.4.5.5 to 8.7.4.5.8).
#[derive(Clone, Debug)]
pub struct Mesh {
    /// The `/ShadingType`, kept so a mesh that cannot be painted can say which
    /// type it was.
    pub kind: i64,
    /// The colour space the vertex components are in.
    pub space: ColorSpace,
    /// `/Function`, when each vertex carries one parametric value instead of
    /// colour components (8.7.4.5.5).
    pub function: Option<Function>,
    /// How many numbers one vertex's colour is: one with a `/Function`, and
    /// the colour space's component count without.
    pub components: usize,
    /// Vertex positions in shading space, for types 4 and 5.
    pub positions: Vec<(f64, f64)>,
    /// Colour inputs, `components` per vertex, parallel to `positions`.
    pub values: Vec<f64>,
    /// Triangles as indices into `positions`, for types 4 and 5.
    pub triangles: Vec<[u32; 3]>,
    /// Patches, for types 6 and 7.
    pub patches: Vec<Patch>,
}

/// A mesh flattened to device-space triangles.
#[derive(Clone, Debug, Default)]
pub struct Tessellation {
    /// Vertex positions in device pixels.
    pub positions: Vec<(f64, f64)>,
    /// Colour inputs, one group per vertex.
    pub values: Vec<f64>,
    /// Triangles as indices into `positions`.
    pub triangles: Vec<[u32; 3]>,
}

impl Mesh {
    /// The colour one interpolated set of inputs paints.
    ///
    /// This is the whole of "interpolation happens in the shading's colour
    /// space": the caller interpolates `inputs` — components, or the one
    /// parametric value a `/Function` takes — and only then does anything
    /// become RGB. Converting at the vertices and interpolating the results
    /// would move the midpoint of every non-linear space, and of every
    /// non-linear `/Function`, which is a difference no test of the endpoints
    /// can see.
    ///
    /// `scratch` is the colour pass's, the same one for every pixel, so the
    /// pass allocates nothing per pixel.
    #[must_use]
    pub fn color(&self, inputs: &[f64], scratch: &mut ShadingScratch) -> (u8, u8, u8) {
        match (&self.function, inputs) {
            (Some(function), [t]) => scratch.tabled(&self.space, function, *t),
            (Some(function), _) => scratch.color(&self.space, function, inputs),
            (None, _) => scratch.convert(&self.space, inputs),
        }
    }

    /// [`Mesh::color`], beside the components in the mesh's space its light
    /// was converted from: `inputs` themselves without a `/Function`, and the
    /// function's outputs at them with one — what a mesh on plates reads its
    /// tints from (RC-04.14). A colour read through the scratch's table
    /// evaluates nothing, so a pass that reads the components keeps none.
    #[must_use]
    pub fn color_components<'s>(
        &self,
        inputs: &'s [f64],
        scratch: &'s mut ShadingScratch,
    ) -> ((u8, u8, u8), &'s [f64]) {
        let light = self.color(inputs, scratch);
        if self.function.is_some() {
            (light, scratch.outputs())
        } else {
            (light, inputs)
        }
    }

    /// Whether this mesh describes any geometry at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.triangles.is_empty() && self.patches.is_empty()
    }

    /// Flattens the mesh into device-space triangles, or refuses one that is
    /// larger than [`MAX_MESH_TRIANGLES`].
    ///
    /// Refusing outright rather than truncating, for the reason
    /// `fill_with_tiles` gives about a lattice: half a mesh reads as a
    /// rendering artefact, where nothing painted plus a warning naming the
    /// type reads as the gap it is.
    ///
    /// `smoothness` is the `/SM` in force (10.7.3), the colour error a patch's
    /// grid may leave as a fraction of each component's range: read in 0 to 1
    /// and no finer than one level of 255 ([`FINEST_SMOOTHNESS`]), and `None`,
    /// or a number that is not finite, is the device's own — each patch
    /// subdivided by its device length alone, as before. Under it a patch
    /// takes the larger of its device-length count and [`colour_steps`]'s,
    /// capped at [`MAX_PATCH_STEPS`]. It moves no triangle of a type 4 or 5
    /// mesh, which interpolates its vertices as 8.7.4.5.5 defines it.
    #[must_use]
    pub fn tessellate(&self, to_device: &Matrix, smoothness: Option<f64>) -> Option<Tessellation> {
        if !to_device.is_finite() {
            return None;
        }

        let mut out = Tessellation::default();
        for (index, position) in self.positions.iter().enumerate() {
            let (x, y) = to_device.apply(position.0, position.1);
            out.positions.push((x, y));
            let start = index.checked_mul(self.components)?;
            let end = start.checked_add(self.components)?;
            match self.values.get(start..end) {
                Some(slice) => out.values.extend_from_slice(slice),
                // A vertex whose colour did not survive the stream keeps its
                // slot so the triangle indices stay meaningful.
                None => out.values.extend(std::iter::repeat_n(0.0, self.components)),
            }
        }
        if self.triangles.len() > MAX_MESH_TRIANGLES {
            return None;
        }
        out.triangles.extend_from_slice(&self.triangles);

        let smooth = if self.patches.is_empty() {
            None
        } else {
            Smooth::of(self, smoothness)
        };
        for patch in &self.patches {
            self.tessellate_patch(patch, to_device, smooth.as_ref(), &mut out)?;
        }
        Some(out)
    }

    /// Subdivides one patch onto a square grid and appends its triangles:
    /// as finely as its device length asks, and as its colour asks where
    /// `smooth` holds a smoothness.
    fn tessellate_patch(
        &self,
        patch: &Patch,
        to_device: &Matrix,
        smooth: Option<&Smooth>,
        out: &mut Tessellation,
    ) -> Option<()> {
        // The control net in device space: the subdivision is chosen from how
        // large the patch is *drawn*, not from how large it is written.
        let mut net = [[(0.0, 0.0); 4]; 4];
        for (row, source) in net.iter_mut().zip(patch.points.iter()) {
            for (slot, point) in row.iter_mut().zip(source.iter()) {
                if !point.0.is_finite() || !point.1.is_finite() {
                    return Some(()); // A patch with nonsense corners paints nothing.
                }
                *slot = to_device.apply(point.0, point.1);
            }
        }

        // Both counts are powers of two, so the larger is one too.
        let steps = match smooth {
            Some(smooth) => patch_steps(&net).max(smooth.steps(&patch.corners, self.components)),
            None => patch_steps(&net),
        };
        let span = (steps as usize).checked_add(1)?;
        let vertices = span.checked_mul(span)?;
        let triangles = (steps as usize)
            .checked_mul(steps as usize)?
            .checked_mul(2)?;
        if out.triangles.len().checked_add(triangles)? > MAX_MESH_TRIANGLES {
            return None;
        }
        if out.positions.len().checked_add(vertices)? > MAX_MESH_TRIANGLES {
            return None;
        }

        let base = u32::try_from(out.positions.len()).ok()?;
        let corner = |index: usize| -> &[f64] {
            let start = index * self.components;
            patch
                .corners
                .get(start..start + self.components)
                .unwrap_or(&[])
        };
        let (c1, c2, c3, c4) = (corner(0), corner(1), corner(2), corner(3));

        for i in 0..=steps {
            // Exact for every `steps` this can produce, which are the powers of
            // two up to 32: both operands are small integers and the quotient
            // is correctly rounded, so every target agrees on it.
            let u = f64::from(i) / f64::from(steps);
            for j in 0..=steps {
                let v = f64::from(j) / f64::from(steps);
                out.positions.push(surface(&net, u, v));
                // 8.7.4.5.7: the corner colours are interpolated bilinearly
                // over the unit square, in the shading's own space.
                for k in 0..self.components {
                    let at = |c: &[f64]| c.get(k).copied().unwrap_or(0.0);
                    let value = (1.0 - u) * (1.0 - v) * at(c1)
                        + (1.0 - u) * v * at(c2)
                        + u * v * at(c3)
                        + u * (1.0 - v) * at(c4);
                    out.values.push(value);
                }
            }
        }

        let span = steps + 1;
        for i in 0..steps {
            for j in 0..steps {
                let at = |i: u32, j: u32| base + i * span + j;
                out.triangles.push([at(i, j), at(i + 1, j), at(i, j + 1)]);
                out.triangles
                    .push([at(i + 1, j), at(i + 1, j + 1), at(i, j + 1)]);
            }
        }
        Some(())
    }
}

/// The tensor surface at `(u, v)`: the Bernstein form of 8.7.4.5.8's sum.
///
/// Addition and multiplication only, so it is bit-identical on every target.
fn surface(net: &[[(f64, f64); 4]; 4], u: f64, v: f64) -> (f64, f64) {
    let bu = bernstein(u);
    let bv = bernstein(v);
    let mut x = 0.0;
    let mut y = 0.0;
    for i in 0..4 {
        for j in 0..4 {
            let weight = bu[i] * bv[j];
            x += net[i][j].0 * weight;
            y += net[i][j].1 * weight;
        }
    }
    (x, y)
}

/// The four cubic Bernstein polynomials at `t`.
fn bernstein(t: f64) -> [f64; 4] {
    let s = 1.0 - t;
    [s * s * s, 3.0 * s * s * t, 3.0 * s * t * t, t * t * t]
}

/// How many pieces each edge of a patch is broken into, from a fixed-point
/// measure of the device-space control net.
///
/// **This is the function ruling 4 is about.** The plan's risk table names
/// adaptive subdivision that terminates on a floating-point comparison, and
/// gap 13 found exactly that one dimension down: a recursive flattener whose
/// stopping test lands differently on a 32-bit target produces a different
/// *number* of segments, which is not a rounding difference in the output but a
/// different mesh.
///
/// So the count is not adaptive and there is no recursion. The measure is the
/// longest control-polygon leg-sum among the patch's four boundary curves —
/// the same quantity gap 13's flattener takes its step count from — built in
/// `f64` out of subtraction, `abs` and addition, all of which IEEE 754 pins
/// exactly. It is cast **once** into 16.16 fixed point, and every comparison
/// after that is between two `i64`. The loop doubles its threshold rather than
/// halving the measure, because shifting truncates and a measure a hair over
/// the threshold would come out one doubling short (gap 12's finding in the
/// image pyramid).
fn patch_steps(net: &[[(f64, f64); 4]; 4]) -> u32 {
    // The four boundary curves, as index paths through the net.
    let edges: [[(usize, usize); 4]; 4] = [
        [(0, 0), (0, 1), (0, 2), (0, 3)],
        [(0, 3), (1, 3), (2, 3), (3, 3)],
        [(3, 3), (3, 2), (3, 1), (3, 0)],
        [(3, 0), (2, 0), (1, 0), (0, 0)],
    ];

    let mut longest = 0.0f64;
    for edge in &edges {
        let mut length = 0.0f64;
        for pair in edge.windows(2) {
            let a = net[pair[0].0][pair[0].1];
            let b = net[pair[1].0][pair[1].1];
            // The Manhattan length, so no square root is needed at all. It
            // over-measures a diagonal by up to 41 per cent, which buys a
            // slightly finer grid and never a coarser one.
            length += (b.0 - a.0).abs() + (b.1 - a.1).abs();
        }
        if length > longest {
            longest = length;
        }
    }
    if !longest.is_finite() {
        return 1;
    }

    // One cast, and integers from here down.
    let measure = (longest * 65536.0).clamp(0.0, i64::MAX as f64) as i64;
    let step = (PATCH_STEP_PIXELS * 65536.0) as i64;

    let mut steps = 1u32;
    let mut threshold = step;
    while threshold < measure && steps < MAX_PATCH_STEPS {
        steps *= 2;
        threshold = threshold.saturating_mul(2);
    }
    steps
}

/// What `/SM` asks of a mesh's patches (10.7.3): the tolerance, and what one
/// unit of each colour input's bilinear term costs the colour, in fractions of
/// a component's range.
struct Smooth {
    /// The colour error a patch's grid may leave, as a fraction of each
    /// component's range.
    tolerance: f64,
    /// Per colour input: 1 over its component's range, or, for the one
    /// parametric value a `/Function` takes, the function's sampled slope over
    /// each output's range, the steepest ([`sampled_slope`]).
    weights: Vec<f64>,
}

impl Smooth {
    /// What `smoothness` asks of `mesh`, or `None` where it asks nothing:
    /// unset, or not a finite number.
    fn of(mesh: &Mesh, smoothness: Option<f64>) -> Option<Smooth> {
        let tolerance = smoothness_tolerance(smoothness)?;
        let weights = match &mesh.function {
            Some(function) => vec![sampled_slope(mesh, function)],
            None => (0..mesh.components)
                .map(|index| 1.0 / component_range(&mesh.space, index))
                .collect(),
        };
        Some(Smooth { tolerance, weights })
    }

    /// The count a patch whose corner inputs are `corners` (`components` a
    /// corner, in `c1` to `c4`'s order) takes for its colour to be within the
    /// tolerance.
    ///
    /// The error is the bilinear term's (8.7.4.5.7): over a cell `h` of the
    /// parameter on a side, a triangle carrying the corners linearly misses
    /// `|c₁ − c₂ + c₃ − c₄| × h² / 4` at most, at the cell's centre. So the
    /// measure is that coefficient for each input, weighted into a fraction of
    /// its colour's range, and the largest — 10.7.3's "the maximum independent
    /// error shall be used".
    fn steps(&self, corners: &[f64], components: usize) -> u32 {
        let mut measure = 0.0f64;
        for (input, weight) in self.weights.iter().enumerate() {
            let at = |corner: usize| {
                corner
                    .checked_mul(components)
                    .and_then(|start| start.checked_add(input))
                    .and_then(|index| corners.get(index))
                    .copied()
                    .unwrap_or(0.0)
            };
            let error = (at(0) - at(1) + at(2) - at(3)).abs() * weight;
            // A comparison NaN answers false to, so a nonsense corner adds
            // nothing rather than poisoning the measure.
            if error > measure {
                measure = error;
            }
        }
        colour_steps(measure, self.tolerance)
    }
}

/// `/SM` as a colour tolerance: 10.7.3's "fraction of the range of the colour
/// component, from 0.0 to 1.0", read in that range and no finer than
/// [`FINEST_SMOOTHNESS`]. `None`, and a number that is not finite, are the
/// device's own, which asks nothing of a patch's colour.
fn smoothness_tolerance(smoothness: Option<f64>) -> Option<f64> {
    let value = smoothness.filter(|value| value.is_finite())?;
    Some(value.clamp(0.0, 1.0).max(FINEST_SMOOTHNESS))
}

/// How many pieces each edge of a patch is broken into for its colour to be
/// within `tolerance`: the least power of two `n`, up to
/// [`MAX_PATCH_STEPS`], whose error `measure / (4 n²)` is within it — that is,
/// `measure ≤ 4 n² × tolerance`.
///
/// The same fixed-point decision as [`patch_steps`] (ruling 4): the measure
/// and the allowance at one step are each cast **once** into 16.16, and the
/// loop compares integers, the allowance multiplied by four at each doubling
/// rather than the measure divided. A measure that is not a number reads as
/// zero, one step; one past the fixed-point range reads as its largest value,
/// the cap.
fn colour_steps(measure: f64, tolerance: f64) -> u32 {
    let fixed = |value: f64| (value * 65536.0).clamp(0.0, i64::MAX as f64) as i64;
    let measure = fixed(measure);
    let mut allowance = fixed(4.0 * tolerance);
    let mut steps = 1u32;
    while allowance < measure && steps < MAX_PATCH_STEPS {
        steps *= 2;
        allowance = allowance.saturating_mul(4);
    }
    steps
}

/// The span of a colour component's values, which smoothness is a fraction
/// of (10.7.3): an `/Indexed` index's `0` to `hival` (8.6.6.3), Lab's L\* `0`
/// to `100` and its a\* and b\* their `/Range` (8.6.5.4), and every other
/// space's components `0` to `1` — the range each converts within. A span of
/// nothing, or one that is not a number, reads as 1.
fn component_range(space: &ColorSpace, index: usize) -> f64 {
    let span = match space {
        ColorSpace::Indexed { high, .. } => f64::from(*high),
        ColorSpace::Lab { range } => match index {
            0 => 100.0,
            1 => (range[1] - range[0]).abs(),
            _ => (range[3] - range[2]).abs(),
        },
        _ => 1.0,
    };
    if span.is_finite() && span > 0.0 {
        span
    } else {
        1.0
    }
}

/// The steepest `function` rises or falls over the parametric values `mesh`'s
/// patches' corners span, as a fraction of each output's component range per
/// unit of the parameter: the function evaluated at [`SLOPE_INTERVALS`] equal
/// intervals across the span, each output's change over an interval over the
/// interval and its range, the largest.
///
/// A patch's parametric value is bilinear like a colour, so its grid's error
/// in the parameter is the bilinear term's, and the colour's error is within
/// this slope times it as nearly as sampling can tell — 10.7.3's NOTE 1 says
/// as much of a function known only by its samples. Measured once a mesh,
/// over the whole span, so every patch is held to the steepest part. A span
/// of nothing, or one that is not finite, has no slope to measure.
fn sampled_slope(mesh: &Mesh, function: &Function) -> f64 {
    let (low, high) = mesh
        .patches
        .iter()
        .flat_map(|patch| patch.corners.iter())
        .fold((f64::INFINITY, f64::NEG_INFINITY), |(low, high), value| {
            (low.min(*value), high.max(*value))
        });
    if !(low.is_finite() && high.is_finite() && high > low) {
        return 0.0;
    }
    let span = high - low;
    let intervals = f64::from(SLOPE_INTERVALS);
    let mut previous = Vec::new();
    let mut current = Vec::new();
    function.eval_into(&[low], &mut previous);
    let mut steepest = 0.0f64;
    for step in 1..=SLOPE_INTERVALS {
        let t = low + span * f64::from(step) / intervals;
        function.eval_into(&[t], &mut current);
        for (output, (before, after)) in previous.iter().zip(current.iter()).enumerate() {
            let rise = (after - before).abs() / component_range(&mesh.space, output);
            if rise > steepest {
                steepest = rise;
            }
        }
        core::mem::swap(&mut previous, &mut current);
    }
    if steepest == 0.0 {
        return 0.0; // A flat function, however narrow its span.
    }
    // An interval so narrow the quotient leaves the finite is as steep as
    // anything can be: the cap.
    let slope = steepest / (span / intervals);
    if slope.is_finite() {
        slope
    } else {
        f64::MAX
    }
}

/// How a mesh shading's vertex stream is packed (Tables 79 to 82).
#[derive(Clone, Debug)]
pub struct MeshParams {
    /// `/ShadingType`, 4 to 7.
    pub kind: i64,
    /// `/BitsPerCoordinate`.
    pub bits_per_coordinate: u32,
    /// `/BitsPerComponent`.
    pub bits_per_component: u32,
    /// `/BitsPerFlag`, unused by type 5.
    pub bits_per_flag: u32,
    /// `/Decode`: `(min, max)` for x, for y, then one per colour input.
    pub decode: Vec<(f64, f64)>,
    /// `/VerticesPerRow`, type 5 only.
    pub vertices_per_row: u32,
    /// How many numbers one vertex's colour is.
    pub components: usize,
}

impl MeshParams {
    /// Whether the bit widths are ones the spec allows (Table 79).
    ///
    /// Refused rather than rounded to a nearby legal width: the widths decide
    /// how the whole stream is cut up, so a wrong one does not produce
    /// slightly wrong vertices, it produces a different number of them at
    /// different places.
    fn widths_are_legal(&self) -> bool {
        matches!(self.bits_per_coordinate, 1 | 2 | 4 | 8 | 12 | 16 | 24 | 32)
            && matches!(self.bits_per_component, 1 | 2 | 4 | 8 | 12 | 16)
            && (self.kind == 5 || matches!(self.bits_per_flag, 2 | 4 | 8))
    }

    /// The bits one vertex occupies, before any padding.
    fn vertex_bits(&self) -> u64 {
        let flag = if self.kind == 5 {
            0
        } else {
            u64::from(self.bits_per_flag)
        };
        flag + 2 * u64::from(self.bits_per_coordinate)
            + (self.components as u64) * u64::from(self.bits_per_component)
    }
}

/// A big-endian bit stream, which is how every packed value in 8.7.4.5.5 is
/// written.
struct BitReader<'a> {
    data: &'a [u8],
    /// The next bit to read, counted from the first bit of the first byte.
    bit: u64,
}

impl<'a> BitReader<'a> {
    fn new(data: &'a [u8]) -> BitReader<'a> {
        BitReader { data, bit: 0 }
    }

    /// How many bits are left.
    fn remaining(&self) -> u64 {
        let total = (self.data.len() as u64).saturating_mul(8);
        total.saturating_sub(self.bit)
    }

    /// Reads up to 32 bits, most significant first. `None` at end of stream.
    fn read(&mut self, bits: u32) -> Option<u64> {
        if bits == 0 || bits > 32 || self.remaining() < u64::from(bits) {
            return None;
        }
        let mut value = 0u64;
        for _ in 0..bits {
            let byte = self.data.get((self.bit / 8) as usize)?;
            let shift = 7 - (self.bit % 8);
            value = (value << 1) | u64::from((byte >> shift) & 1);
            self.bit += 1;
        }
        Some(value)
    }

    /// Skips to the next byte boundary. 8.7.4.5.5 pads each vertex of a type 4
    /// stream, and 8.7.4.5.7 each patch of a type 6 or 7 one; type 5 has no
    /// padding at all, which is why this is called by the readers rather than
    /// by [`BitReader::read`].
    fn align(&mut self) {
        let slack = self.bit % 8;
        if slack != 0 {
            self.bit += 8 - slack;
        }
    }
}

/// Maps one packed integer through its `/Decode` range (8.7.4.5.5).
///
/// The raw value spans `0..=2^bits - 1` whatever the width is, so the
/// interpolation divides by that rather than by a power of two — an off-by-one
/// here scales the whole mesh by a factor of `2^b / (2^b - 1)`, which at 8 bits
/// is four tenths of a per cent and invisible, and at 1 bit is a factor of two.
fn decode_value(raw: u64, bits: u32, range: (f64, f64)) -> f64 {
    let span = if bits >= 64 {
        u64::MAX
    } else {
        (1u64 << bits) - 1
    };
    if span == 0 {
        return range.0;
    }
    let (low, high) = range;
    low + (raw as f64) * (high - low) / (span as f64)
}

/// Reads a packed mesh stream into geometry, or refuses it.
///
/// `None` means the mesh cannot be painted at all — illegal bit widths, a
/// `/Decode` array too short to interpret the stream, or more geometry than
/// the caps above allow. The caller reports it as the shading type it is,
/// which is what a reader can act on.
#[must_use]
pub fn read_mesh(
    params: &MeshParams,
    data: &[u8],
    space: ColorSpace,
    function: Option<Function>,
) -> Option<Mesh> {
    if !params.widths_are_legal() || params.components == 0 {
        return None;
    }
    // x, y, and one range per colour input. A short array cannot be completed
    // by guessing: the missing range is exactly the information that says what
    // the numbers mean.
    if params.decode.len() < 2 + params.components {
        return None;
    }
    let vertex_bits = params.vertex_bits();
    if vertex_bits == 0 {
        return None;
    }

    let mut mesh = Mesh {
        kind: params.kind,
        space,
        function,
        components: params.components,
        positions: Vec::new(),
        values: Vec::new(),
        triangles: Vec::new(),
        patches: Vec::new(),
    };

    // Bounded *before* allocating, from the stream's own length: the ceiling
    // is what a hostile `/Length` cannot get past, and the reserve is what
    // stops an honest one from growing a vector a byte at a time.
    let ceiling = ((data.len() as u64).saturating_mul(8) / vertex_bits) as usize;
    let reserve = ceiling.min(MAX_MESH_VERTICES);
    mesh.positions.reserve(reserve);
    mesh.values
        .reserve(reserve.saturating_mul(params.components));

    let mut reader = BitReader::new(data);
    match params.kind {
        4 => read_free_form(&mut reader, params, &mut mesh)?,
        5 => read_lattice(&mut reader, params, &mut mesh)?,
        6 | 7 => read_patches(&mut reader, params, &mut mesh)?,
        _ => return None,
    }
    Some(mesh)
}

/// One vertex's flag, position and colour inputs.
///
/// `None` means the stream ran out, which is a truncation to tolerate rather
/// than a mesh to refuse — the callers stop and keep what they have. The
/// ceilings are checked by the callers for exactly that reason: a stream that
/// *ends* and a stream that is too big are different answers.
fn read_vertex(
    reader: &mut BitReader<'_>,
    params: &MeshParams,
    mesh: &mut Mesh,
    with_flag: bool,
) -> Option<(u64, u32)> {
    let flag = if with_flag {
        reader.read(params.bits_per_flag)?
    } else {
        0
    };
    let (x, y) = read_point(reader, params)?;
    let index = u32::try_from(mesh.positions.len()).ok()?;
    mesh.positions.push((x, y));
    for component in 0..params.components {
        let raw = reader.read(params.bits_per_component)?;
        let range = params.decode.get(2 + component).copied()?;
        mesh.values
            .push(decode_value(raw, params.bits_per_component, range));
    }
    Some((flag, index))
}

/// One packed coordinate pair through `/Decode`.
fn read_point(reader: &mut BitReader<'_>, params: &MeshParams) -> Option<(f64, f64)> {
    let raw_x = reader.read(params.bits_per_coordinate)?;
    let raw_y = reader.read(params.bits_per_coordinate)?;
    let x = decode_value(raw_x, params.bits_per_coordinate, params.decode[0]);
    let y = decode_value(raw_y, params.bits_per_coordinate, params.decode[1]);
    Some((x, y))
}

/// Type 4: free-form triangles, each vertex carrying an edge flag (8.7.4.5.5).
fn read_free_form(reader: &mut BitReader<'_>, params: &MeshParams, mesh: &mut Mesh) -> Option<()> {
    let mut previous: Option<[u32; 3]> = None;
    loop {
        if mesh.positions.len() >= MAX_MESH_VERTICES {
            return None;
        }
        let Some((flag, first)) = read_vertex(reader, params, mesh, true) else {
            return Some(());
        };
        reader.align();

        let triangle = match flag {
            0 => {
                // 8.7.4.5.5: a flag of 0 begins a new triangle, and the two
                // vertices after it complete it. Their own flags are read and
                // ignored, which is what the spec requires them to be.
                let Some((_, second)) = read_vertex(reader, params, mesh, true) else {
                    return Some(());
                };
                reader.align();
                let Some((_, third)) = read_vertex(reader, params, mesh, true) else {
                    return Some(());
                };
                reader.align();
                [first, second, third]
            }
            // 1 keeps the previous triangle's second and third vertices, 2 its
            // first and third. Ignoring the distinction is the classic mesh
            // bug: every strip still draws, and every other triangle in it is
            // the wrong one.
            1 => match previous {
                Some([_, b, c]) => [b, c, first],
                None => continue,
            },
            2 => match previous {
                Some([a, _, c]) => [a, c, first],
                None => continue,
            },
            _ => return Some(()),
        };

        if mesh.triangles.len() >= MAX_MESH_TRIANGLES {
            return None;
        }
        mesh.triangles.push(triangle);
        previous = Some(triangle);
    }
}

/// Type 5: a lattice, `/VerticesPerRow` wide, with no flags (8.7.4.5.6).
fn read_lattice(reader: &mut BitReader<'_>, params: &MeshParams, mesh: &mut Mesh) -> Option<()> {
    let per_row = params.vertices_per_row as usize;
    if per_row < 2 {
        return None;
    }

    loop {
        if mesh.positions.len() >= MAX_MESH_VERTICES {
            return None;
        }
        if read_vertex(reader, params, mesh, false).is_none() {
            break;
        }
    }

    let rows = mesh.positions.len() / per_row;
    if rows < 2 {
        return Some(());
    }
    let quads = (rows - 1).checked_mul(per_row - 1)?;
    if quads.checked_mul(2)? > MAX_MESH_TRIANGLES {
        return None;
    }
    for row in 0..rows - 1 {
        for column in 0..per_row - 1 {
            let at = |row: usize, column: usize| (row * per_row + column) as u32;
            mesh.triangles
                .push([at(row, column), at(row, column + 1), at(row + 1, column)]);
            mesh.triangles.push([
                at(row, column + 1),
                at(row + 1, column + 1),
                at(row + 1, column),
            ]);
        }
    }
    Some(())
}

/// One patch as the stream carries it: the sixteen control points in the
/// spiral order 8.7.4.5.7 numbers them, and the four corner colour inputs.
///
/// Kept in that order rather than converted immediately, because a continuing
/// patch names the previous patch's points *by that numbering* (Table 85).
type RawPatch = ([(f64, f64); 16], Vec<f64>);

/// Types 6 and 7: Coons and tensor patches (8.7.4.5.7, 8.7.4.5.8).
fn read_patches(reader: &mut BitReader<'_>, params: &MeshParams, mesh: &mut Mesh) -> Option<()> {
    let tensor = params.kind == 7;
    let mut previous: Option<RawPatch> = None;

    loop {
        let Some((points, colors)) = read_patch(reader, params, tensor, previous.as_ref()) else {
            return Some(());
        };
        if mesh.patches.len() >= MAX_MESH_PATCHES {
            return None;
        }
        mesh.patches.push(Patch {
            points: tensor_net(&points, tensor),
            corners: colors.clone(),
        });
        previous = Some((points, colors));
    }
}

/// One patch, in the spiral point order the stream writes.
///
/// `None` ends the stream: no more flag, a flag out of range, or data that ran
/// out mid-patch. All three are stopping conditions rather than refusals — a
/// mesh that ends early paints the patches it did carry.
fn read_patch(
    reader: &mut BitReader<'_>,
    params: &MeshParams,
    tensor: bool,
    previous: Option<&RawPatch>,
) -> Option<RawPatch> {
    let flag = reader.read(params.bits_per_flag)?;
    if flag > 3 {
        return None;
    }

    let mut points = [(0.0, 0.0); 16];
    let mut colors: Vec<f64> = Vec::with_capacity(4 * params.components);

    let (start, new_points) = if flag == 0 {
        (0, if tensor { 16 } else { 12 })
    } else {
        // 8.7.4.5.7 Table 85: a continuing patch shares one edge with the
        // last, so its first four points and first two colours are the
        // previous patch's rather than being written again.
        let (last_points, last_colors) = previous?;
        let (edge, first, second) = match flag {
            1 => ([3, 4, 5, 6], 1, 2),
            2 => ([6, 7, 8, 9], 2, 3),
            _ => ([9, 10, 11, 0], 3, 0),
        };
        for (slot, source) in edge.iter().enumerate() {
            points[slot] = last_points[*source];
        }
        for corner in [first, second] {
            let at = corner * params.components;
            colors.extend_from_slice(
                last_colors
                    .get(at..at + params.components)
                    .unwrap_or(&[0.0; 0]),
            );
        }
        (4, if tensor { 12 } else { 8 })
    };

    for slot in points.iter_mut().skip(start).take(new_points) {
        *slot = read_point(reader, params)?;
    }

    let new_colors = if flag == 0 { 4 } else { 2 };
    for _ in 0..new_colors {
        for component in 0..params.components {
            let raw = reader.read(params.bits_per_component)?;
            let range = params.decode.get(2 + component).copied()?;
            colors.push(decode_value(raw, params.bits_per_component, range));
        }
    }
    // 8.7.4.5.7: a patch's data is padded to a byte boundary, where a type 4
    // stream pads each vertex.
    reader.align();

    (colors.len() == 4 * params.components).then_some((points, colors))
}

/// The sixteen tensor control points of a patch, from the spiral order a
/// stream writes them in (8.7.4.5.7 Figure 46, 8.7.4.5.8 Figure 47).
///
/// For a Coons patch the four internal points are not in the stream at all;
/// 8.7.4.5.7 gives them as a fixed combination of the twelve boundary points,
/// which is what makes a Coons patch a tensor patch with a constrained
/// interior rather than a different kind of surface.
fn tensor_net(points: &[(f64, f64); 16], tensor: bool) -> [[(f64, f64); 4]; 4] {
    let mut p = [[(0.0, 0.0); 4]; 4];
    // Round the boundary: up the v edge at u = 0, across at v = 3, down at
    // u = 3, back along v = 0.
    p[0][0] = points[0];
    p[0][1] = points[1];
    p[0][2] = points[2];
    p[0][3] = points[3];
    p[1][3] = points[4];
    p[2][3] = points[5];
    p[3][3] = points[6];
    p[3][2] = points[7];
    p[3][1] = points[8];
    p[3][0] = points[9];
    p[2][0] = points[10];
    p[1][0] = points[11];

    if tensor {
        p[1][1] = points[12];
        p[1][2] = points[13];
        p[2][2] = points[14];
        p[2][1] = points[15];
        return p;
    }

    // 8.7.4.5.7's formula for the implied interior, one axis at a time.
    let mix = |a: (f64, f64), b: (f64, f64), c: (f64, f64), d: (f64, f64), e: (f64, f64)| {
        // -4a + 6b - 2c + 3d - e, all over 9, with b, c and d already summed
        // by the caller where the formula pairs them.
        (
            (-4.0 * a.0 + 6.0 * b.0 - 2.0 * c.0 + 3.0 * d.0 - e.0) / 9.0,
            (-4.0 * a.1 + 6.0 * b.1 - 2.0 * c.1 + 3.0 * d.1 - e.1) / 9.0,
        )
    };
    let sum = |a: (f64, f64), b: (f64, f64)| (a.0 + b.0, a.1 + b.1);

    p[1][1] = mix(
        p[0][0],
        sum(p[0][1], p[1][0]),
        sum(p[0][3], p[3][0]),
        sum(p[3][1], p[1][3]),
        p[3][3],
    );
    p[1][2] = mix(
        p[0][3],
        sum(p[0][2], p[1][3]),
        sum(p[0][0], p[3][3]),
        sum(p[3][2], p[1][0]),
        p[3][0],
    );
    p[2][1] = mix(
        p[3][0],
        sum(p[3][1], p[2][0]),
        sum(p[3][3], p[0][0]),
        sum(p[0][1], p[2][3]),
        p[0][3],
    );
    p[2][2] = mix(
        p[3][3],
        sum(p[3][2], p[2][3]),
        sum(p[3][0], p[0][3]),
        sum(p[0][2], p[2][0]),
        p[0][0],
    );
    p
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Packs values of a fixed bit width, most significant bit first.
    struct BitWriter {
        data: Vec<u8>,
        bit: u32,
    }

    impl BitWriter {
        fn new() -> BitWriter {
            BitWriter {
                data: Vec::new(),
                bit: 0,
            }
        }

        fn push(&mut self, value: u64, bits: u32) {
            for index in (0..bits).rev() {
                if self.bit == 0 {
                    self.data.push(0);
                }
                let one = (value >> index) & 1;
                if let Some(byte) = self.data.last_mut() {
                    *byte |= (one as u8) << (7 - self.bit);
                }
                self.bit = (self.bit + 1) % 8;
            }
        }

        fn align(&mut self) {
            self.bit = 0;
        }
    }

    fn params(kind: i64, coordinate: u32, component: u32) -> MeshParams {
        MeshParams {
            kind,
            bits_per_coordinate: coordinate,
            bits_per_component: component,
            bits_per_flag: 8,
            decode: vec![(0.0, 100.0), (0.0, 50.0), (0.0, 1.0)],
            vertices_per_row: 0,
            components: 1,
        }
    }

    /// The whole raw range maps onto the whole `/Decode` range, at every width
    /// the spec allows. Milestone 1's exit criterion: a hand-built stream reads
    /// back the vertices exactly.
    #[test]
    fn a_packed_vertex_reads_back_at_every_coordinate_width() {
        for bits in [8u32, 16, 32] {
            let top = (1u64 << bits) - 1;
            let mut writer = BitWriter::new();
            // Three vertices with flag 0: the ends of the ranges, and the
            // middle, which is the value an off-by-one in the span moves.
            for (x, y, c) in [
                (0u64, top, 0u64),
                (top, 0, top),
                (top / 2, top / 2, top / 2),
            ] {
                writer.push(0, 8);
                writer.push(x, bits);
                writer.push(y, bits);
                writer.push(c, 8);
                writer.align();
            }

            let p = params(4, bits, 8);
            let mesh = read_mesh(&p, &writer.data, ColorSpace::DeviceGray, None)
                .expect("the stream reads");

            assert_eq!(mesh.positions.len(), 3, "at {bits} bits per coordinate");
            assert_eq!(mesh.triangles, vec![[0, 1, 2]]);
            assert_eq!(mesh.positions[0], (0.0, 50.0), "the range's ends");
            assert_eq!(mesh.positions[1], (100.0, 0.0), "and the other way round");
            assert_eq!(mesh.values[0], 0.0);
            assert_eq!(mesh.values[1], 1.0);

            // The midpoint, to within one raw step of the width: this is the
            // value an off-by-one in the span moves, and at 8 bits it moves it
            // by 0.2 of a unit, so the bound has to be tighter than that at 32
            // bits and cannot be at 8.
            let step = 100.0 / top as f64;
            let (mx, my) = mesh.positions[2];
            assert!((mx - 50.0).abs() <= step, "{bits} bits: x midpoint {mx}");
            assert!((my - 25.0).abs() <= step, "{bits} bits: y midpoint {my}");
        }
    }

    /// Twelve and twenty-four bits are the widths that are not a whole number
    /// of bytes, so they are the ones a byte-oriented reader gets wrong.
    #[test]
    fn the_widths_that_are_not_whole_bytes_read_too() {
        let mut writer = BitWriter::new();
        writer.push(0, 8);
        writer.push(0xFFF, 12);
        writer.push(0x000, 12);
        writer.push(0x8, 4);
        writer.align();
        writer.push(0, 8);
        writer.push(0x000, 12);
        writer.push(0xFFF, 12);
        writer.push(0xF, 4);
        writer.align();
        writer.push(0, 8);
        writer.push(0x800, 12);
        writer.push(0x800, 12);
        writer.push(0x0, 4);
        writer.align();

        let mut p = params(4, 12, 4);
        p.decode = vec![(0.0, 100.0), (0.0, 50.0), (0.0, 1.0)];
        let mesh = read_mesh(&p, &writer.data, ColorSpace::DeviceGray, None).expect("it reads");
        assert_eq!(mesh.positions[0], (100.0, 0.0));
        assert_eq!(mesh.positions[1], (0.0, 50.0));
        assert!((mesh.values[0] - 8.0 / 15.0).abs() < 1e-12);
        assert_eq!(mesh.values[1], 1.0);
    }

    /// A `/Decode` range given the other way round runs the other way. It is
    /// the one entry a reader can ignore and still produce a plausible mesh —
    /// in the wrong place, which looks like a transform bug.
    #[test]
    fn a_reversed_decode_range_reverses_the_axis() {
        let mut writer = BitWriter::new();
        writer.push(0, 8);
        writer.push(0, 8);
        writer.push(0, 8);
        writer.push(0, 8);
        writer.align();

        let mut p = params(4, 8, 8);
        p.decode = vec![(100.0, 0.0), (50.0, 10.0), (1.0, 0.0)];
        let mesh = read_mesh(&p, &writer.data, ColorSpace::DeviceGray, None).expect("it reads");
        assert_eq!(
            mesh.positions[0],
            (100.0, 50.0),
            "zero is the range's start"
        );
        assert_eq!(mesh.values[0], 1.0);
    }

    /// The edge flag decides which two vertices a continuing triangle keeps.
    #[test]
    fn the_edge_flag_chooses_which_edge_is_shared() {
        let mut writer = BitWriter::new();
        let vertex = |writer: &mut BitWriter, flag: u64, x: u64, y: u64| {
            writer.push(flag, 8);
            writer.push(x, 8);
            writer.push(y, 8);
            writer.push(128, 8);
            writer.align();
        };
        vertex(&mut writer, 0, 0, 0);
        vertex(&mut writer, 0, 10, 0);
        vertex(&mut writer, 0, 0, 10);
        vertex(&mut writer, 1, 20, 20);
        vertex(&mut writer, 2, 30, 30);

        let p = params(4, 8, 8);
        let mesh = read_mesh(&p, &writer.data, ColorSpace::DeviceGray, None).expect("it reads");
        assert_eq!(
            mesh.triangles,
            vec![[0, 1, 2], [1, 2, 3], [1, 3, 4]],
            "flag 1 keeps vb and vc; flag 2 keeps va and vc of *that* triangle"
        );
    }

    /// A lattice needs no flags and takes its shape from `/VerticesPerRow`.
    #[test]
    fn a_lattice_makes_two_triangles_per_cell() {
        let mut writer = BitWriter::new();
        for index in 0..6u64 {
            writer.push(index * 10, 8);
            writer.push(index, 8);
            writer.push(100, 8);
        }

        let mut p = params(5, 8, 8);
        p.vertices_per_row = 3;
        let mesh = read_mesh(&p, &writer.data, ColorSpace::DeviceGray, None).expect("it reads");
        assert_eq!(mesh.positions.len(), 6, "no flag bits are consumed");
        assert_eq!(
            mesh.triangles,
            vec![[0, 1, 3], [1, 4, 3], [1, 2, 4], [2, 5, 4]]
        );
    }

    /// Illegal bit widths are refused rather than rounded to a legal one.
    #[test]
    fn an_illegal_bit_width_refuses_the_whole_mesh() {
        for (coordinate, component, flag) in [(7u32, 8u32, 8u32), (8, 7, 8), (8, 8, 3)] {
            let mut p = params(4, coordinate, component);
            p.bits_per_flag = flag;
            assert!(
                read_mesh(&p, &[0; 64], ColorSpace::DeviceGray, None).is_none(),
                "{coordinate}/{component}/{flag} is not a legal set of widths"
            );
        }
        // And a `/Decode` too short to say what the numbers mean.
        let mut p = params(4, 8, 8);
        p.decode = vec![(0.0, 1.0), (0.0, 1.0)];
        assert!(read_mesh(&p, &[0; 64], ColorSpace::DeviceGray, None).is_none());
    }

    /// A stream that ends mid-vertex contributes what it completed and stops.
    #[test]
    fn a_truncated_stream_stops_rather_than_panicking() {
        let mut writer = BitWriter::new();
        for flag in [0u64, 0, 0] {
            writer.push(flag, 8);
            writer.push(1, 8);
            writer.push(2, 8);
            writer.push(3, 8);
            writer.align();
        }
        writer.push(1, 8);
        writer.push(9, 8); // and then nothing

        let p = params(4, 8, 8);
        let mesh = read_mesh(&p, &writer.data, ColorSpace::DeviceGray, None).expect("it reads");
        assert_eq!(mesh.triangles.len(), 1, "the completed triangle survives");
    }

    fn flat_patch_stream(kind: i64) -> Vec<u8> {
        // A flat unit square, its twelve boundary points evenly spaced so that
        // the Coons interior lands on the plane.
        let corners = [(0.0f64, 0.0f64), (0.0, 30.0), (30.0, 30.0), (30.0, 0.0)];
        let mut points: Vec<(f64, f64)> = Vec::new();
        for index in 0..4 {
            let a = corners[index];
            let b = corners[(index + 1) % 4];
            for step in 0..3 {
                let t = f64::from(step) / 3.0;
                points.push((a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t));
            }
        }
        if kind == 7 {
            for (u, v) in [(1, 1), (1, 2), (2, 2), (2, 1)] {
                points.push((f64::from(u) * 10.0, f64::from(v) * 10.0));
            }
        }

        let mut writer = BitWriter::new();
        writer.push(0, 8);
        for (x, y) in &points {
            writer.push((x / 30.0 * 255.0) as u64, 8);
            writer.push((y / 30.0 * 255.0) as u64, 8);
        }
        for value in [0u64, 85, 170, 255] {
            writer.push(value, 8);
        }
        writer.align();
        writer.data
    }

    /// A patch stream reads into one patch whose corners are where the stream
    /// put them, for both patch types.
    #[test]
    fn a_patch_reads_its_corners_and_its_colours() {
        for kind in [6i64, 7] {
            let mut p = params(kind, 8, 8);
            p.decode = vec![(0.0, 30.0), (0.0, 30.0), (0.0, 1.0)];
            let mesh = read_mesh(&p, &flat_patch_stream(kind), ColorSpace::DeviceGray, None)
                .expect("it reads");
            assert_eq!(mesh.patches.len(), 1, "type {kind}");
            let patch = &mesh.patches[0];
            assert_eq!(patch.points[0][0], (0.0, 0.0), "type {kind} corner 1");
            assert_eq!(patch.points[0][3], (0.0, 30.0), "corner 2");
            assert_eq!(patch.points[3][3], (30.0, 30.0), "corner 3");
            assert_eq!(patch.points[3][0], (30.0, 0.0), "corner 4");
            assert_eq!(patch.corners.len(), 4);
            assert_eq!(patch.corners[0], 0.0);
            assert_eq!(patch.corners[3], 1.0);
        }
    }

    /// A flat Coons patch's implied interior lies on the same plane its
    /// boundary does — which is what makes 8.7.4.5.7's formula the right one
    /// rather than merely a plausible one.
    #[test]
    fn a_flat_coons_patch_has_a_flat_interior() {
        let mut p = params(6, 8, 8);
        p.decode = vec![(0.0, 30.0), (0.0, 30.0), (0.0, 1.0)];
        let mesh =
            read_mesh(&p, &flat_patch_stream(6), ColorSpace::DeviceGray, None).expect("it reads");
        let net = mesh.patches[0].points;
        for (u, v, want) in [
            (1usize, 1usize, (10.0, 10.0)),
            (1, 2, (10.0, 20.0)),
            (2, 1, (20.0, 10.0)),
            (2, 2, (20.0, 20.0)),
        ] {
            let got = net[u][v];
            assert!(
                (got.0 - want.0).abs() < 0.5 && (got.1 - want.1).abs() < 0.5,
                "p{u}{v} is {got:?}, not about {want:?}"
            );
        }
    }

    /// A Coons patch's implied interior follows 8.7.4.5.7's formula rather
    /// than any plausible interpolation of the corners.
    ///
    /// The flat case above cannot tell the two apart, and injection proved it:
    /// replacing the formula with a bilinear blend of the four corners moved
    /// nothing in the whole workspace except the determinism fingerprint,
    /// because a flat patch's interior is the flat grid under either rule.
    /// What separates them is a **bowed boundary**, whose curvature the
    /// formula carries into the interior and a corner blend cannot see.
    ///
    /// Here the `v = 0` edge is bowed nine units below its two corners. The
    /// four interior points then land at `y = 4` and `y = 17` where the flat
    /// grid would put them at 10 and 20 — the numbers are the formula's,
    /// worked through by hand, so this pins the coefficients rather than
    /// merely the fact that something moved.
    #[test]
    fn a_bowed_coons_edge_moves_the_implied_interior() {
        let points: [(f64, f64); 16] = [
            (0.0, 0.0),   // p00
            (0.0, 10.0),  // p01
            (0.0, 20.0),  // p02
            (0.0, 30.0),  // p03
            (10.0, 30.0), // p13
            (20.0, 30.0), // p23
            (30.0, 30.0), // p33
            (30.0, 20.0), // p32
            (30.0, 10.0), // p31
            (30.0, 0.0),  // p30
            (20.0, -9.0), // p20, bowed
            (10.0, -9.0), // p10, bowed
            (0.0, 0.0),
            (0.0, 0.0),
            (0.0, 0.0),
            (0.0, 0.0),
        ];
        let net = tensor_net(&points, false);
        for (u, v, want) in [
            (1usize, 1usize, (10.0, 4.0)),
            (1, 2, (10.0, 17.0)),
            (2, 1, (20.0, 4.0)),
            (2, 2, (20.0, 17.0)),
        ] {
            let got = net[u][v];
            assert!(
                (got.0 - want.0).abs() < 1e-9 && (got.1 - want.1).abs() < 1e-9,
                "p{u}{v} is {got:?}, not 8.7.4.5.7's {want:?}"
            );
        }

        // And a tensor patch of the same boundary takes the four points the
        // stream gave instead, untouched.
        let mut tensor_points = points;
        tensor_points[12] = (7.0, 3.0);
        tensor_points[13] = (8.0, 21.0);
        tensor_points[14] = (23.0, 22.0);
        tensor_points[15] = (24.0, 5.0);
        let net = tensor_net(&tensor_points, true);
        assert_eq!(net[1][1], (7.0, 3.0));
        assert_eq!(net[1][2], (8.0, 21.0));
        assert_eq!(net[2][2], (23.0, 22.0));
        assert_eq!(net[2][1], (24.0, 5.0));
    }

    /// The subdivision count is a step function of a fixed-point measure, and
    /// it is the same number however the measure was arrived at.
    #[test]
    fn the_subdivision_count_is_a_power_of_two_from_a_fixed_point_measure() {
        let flat = |size: f64| {
            let mut net = [[(0.0, 0.0); 4]; 4];
            for (u, row) in net.iter_mut().enumerate() {
                for (v, slot) in row.iter_mut().enumerate() {
                    *slot = (
                        size * f64::from(u as u32) / 3.0,
                        size * f64::from(v as u32) / 3.0,
                    );
                }
            }
            net
        };

        assert_eq!(patch_steps(&flat(1.0)), 1, "a patch smaller than one step");
        assert_eq!(patch_steps(&flat(3.0)), 1, "exactly one step");
        assert_eq!(patch_steps(&flat(4.0)), 2);
        assert_eq!(patch_steps(&flat(12.0)), 4);
        assert_eq!(patch_steps(&flat(100.0)), 32, "capped");
        assert_eq!(patch_steps(&flat(1e12)), 32, "and still capped");

        // Every count is a power of two, and the sequence never goes backwards.
        let mut previous = 0;
        for size in 1..200 {
            let steps = patch_steps(&flat(f64::from(size)));
            assert!(steps.is_power_of_two(), "{size} gave {steps}");
            assert!(steps >= previous, "{size} went backwards");
            previous = steps;
        }
    }

    /// A patch subdivides into a square grid of triangles, in device space.
    #[test]
    fn a_patch_tessellates_into_a_grid() {
        let mut p = params(6, 8, 8);
        p.decode = vec![(0.0, 30.0), (0.0, 30.0), (0.0, 1.0)];
        let mesh =
            read_mesh(&p, &flat_patch_stream(6), ColorSpace::DeviceGray, None).expect("it reads");

        let tessellation = mesh.tessellate(&Matrix::IDENTITY, None).expect("it fits");
        let steps = 16u32; // 30 units of Manhattan leg-sum per edge, at 3 per step
        assert_eq!(
            tessellation.triangles.len(),
            2 * (steps * steps) as usize,
            "two triangles per cell"
        );
        assert_eq!(
            tessellation.positions.len(),
            ((steps + 1) * (steps + 1)) as usize
        );

        // The corners land on the patch's corners, and the colour with them.
        assert_eq!(tessellation.positions[0], (0.0, 0.0));
        assert_eq!(tessellation.values[0], 0.0);
        let last = tessellation.positions.len() - 1;
        let (x, y) = tessellation.positions[last];
        assert!(
            (x - 30.0).abs() < 1e-9 && (y - 30.0).abs() < 1e-9,
            "{x} {y}"
        );
        assert!((tessellation.values[last] - 170.0 / 255.0).abs() < 1e-9);
    }

    /// The same mesh under a magnifying transform subdivides more finely,
    /// which is the whole reason the matrix reaches this far.
    #[test]
    fn subdivision_follows_the_device_transform() {
        let mut p = params(6, 8, 8);
        p.decode = vec![(0.0, 30.0), (0.0, 30.0), (0.0, 1.0)];
        let mesh =
            read_mesh(&p, &flat_patch_stream(6), ColorSpace::DeviceGray, None).expect("it reads");

        let small = mesh
            .tessellate(&Matrix::scale(0.05, 0.05), None)
            .expect("it fits")
            .triangles
            .len();
        let large = mesh
            .tessellate(&Matrix::scale(4.0, 4.0), None)
            .expect("it fits")
            .triangles
            .len();
        assert!(small < large, "{small} is not finer than {large}");
        assert_eq!(small, 2, "a patch a pixel across is one cell");
    }

    /// **The colour pass's one scratch converts what a fresh one would.** A
    /// mesh carrying a `/Function` into a two-ink space, whose tint transform
    /// writes the scratch too, and one carrying CMYK components outright, take
    /// turns on one scratch over a run of interpolated inputs.
    #[test]
    fn a_kept_scratch_converts_what_a_fresh_one_does() {
        let mesh = |space: ColorSpace, function: Option<Function>, components: usize| Mesh {
            kind: 4,
            space,
            function,
            components,
            positions: Vec::new(),
            values: Vec::new(),
            triangles: Vec::new(),
            patches: Vec::new(),
        };
        let inks = ColorSpace::Separation {
            components: 2,
            alternate: Box::new(ColorSpace::DeviceRgb),
            colorants: tinker_pdf_color::Colorants::default(),
            tint: Box::new(Function::PostScript {
                domain: vec![(0.0, 1.0); 2],
                range: vec![(0.0, 1.0); 3],
                program: tinker_pdf_color::function::parse_postscript(
                    b"{ 2 copy add 0.5 mul 3 1 roll }",
                ),
            }),
        };
        let carried = mesh(
            inks,
            Some(Function::Array(vec![
                Function::Exponential {
                    domain: (0.0, 1.0),
                    c0: vec![0.0],
                    c1: vec![1.0],
                    n: 2.2,
                },
                Function::Exponential {
                    domain: (0.0, 1.0),
                    c0: vec![1.0],
                    c1: vec![0.25],
                    n: 1.0,
                },
            ])),
            1,
        );
        let components = mesh(ColorSpace::DeviceCmyk, None, 4);

        let mut kept = ShadingScratch::default();
        for step in 0..=64 {
            let t = f64::from(step) / 64.0;
            assert_eq!(
                carried.color(&[t], &mut kept),
                carried.color(&[t], &mut ShadingScratch::default()),
                "t = {t}"
            );
            let cmyk = [t, 1.0 - t, t * t, 0.1];
            assert_eq!(
                components.color(&cmyk, &mut kept),
                components.color(&cmyk, &mut ShadingScratch::default()),
                "{cmyk:?}"
            );
        }
    }

    // ---- smoothness (10.7.3) ------------------------------------------------

    /// A flat patch thirty units on a side, its control net on the thirds, in
    /// `space` with `components` inputs a corner and `corners` in `c1` to
    /// `c4`'s order.
    fn flat_patch_mesh(
        space: ColorSpace,
        function: Option<Function>,
        components: usize,
        corners: Vec<f64>,
    ) -> Mesh {
        let mut points = [[(0.0, 0.0); 4]; 4];
        for (u, row) in points.iter_mut().enumerate() {
            for (v, slot) in row.iter_mut().enumerate() {
                *slot = (10.0 * f64::from(u as u32), 10.0 * f64::from(v as u32));
            }
        }
        Mesh {
            kind: 6,
            space,
            function,
            components,
            positions: Vec::new(),
            values: Vec::new(),
            triangles: Vec::new(),
            patches: vec![Patch { points, corners }],
        }
    }

    /// The cells a side squared that `mesh` is cut into at a twentieth of its
    /// size — a patch a pixel and a half across, whose device length asks for
    /// one — under `smoothness`.
    fn cells(mesh: &Mesh, smoothness: Option<f64>) -> usize {
        mesh.tessellate(&Matrix::scale(0.05, 0.05), smoothness)
            .expect("it fits")
            .triangles
            .len()
            / 2
    }

    /// The cells the design's rule gives a patch whose bilinear term is
    /// `measure` of its range (`docs/design/device-dependent-state.md`): the
    /// least power of two `n`, at most 32, whose error `measure / (4 n²)` is
    /// within the smoothness read in 0 to 1 and no finer than 1/255; `n²`.
    fn rule(measure: f64, smoothness: f64) -> usize {
        let tolerance = smoothness.clamp(0.0, 1.0).max(1.0 / 255.0);
        let mut n = 1u32;
        while 4.0 * f64::from(n * n) * tolerance < measure && n < 32 {
            n *= 2;
        }
        (n * n) as usize
    }

    /// **Smoothness subdivides a patch until its colour is within it**: the
    /// grey corners 0, 1/3, 2/3 and 1 leave a bilinear term of 2/3, and at a
    /// size whose length asks for one cell each tolerance takes the least
    /// power of two the rule gives, computed here. The device's own, and 1,
    /// ask nothing more; past 1 reads as 1, 0 and under it as the finest, a
    /// tolerance finer than one level of 255 as that level, and a number that
    /// is not finite as none.
    #[test]
    fn smoothness_subdivides_a_patch_to_its_colour_tolerance() {
        let mesh = flat_patch_mesh(
            ColorSpace::DeviceGray,
            None,
            1,
            vec![0.0, 1.0 / 3.0, 2.0 / 3.0, 1.0],
        );
        assert_eq!(cells(&mesh, None), 1, "the device's own asks nothing");
        for smoothness in [1.0, 0.2, 0.05, 0.01, 1.0 / 255.0] {
            assert_eq!(
                cells(&mesh, Some(smoothness)),
                rule(2.0 / 3.0, smoothness),
                "/SM {smoothness}"
            );
        }
        assert_eq!(cells(&mesh, Some(1.0)), 1);
        assert_eq!(cells(&mesh, Some(0.05)), 4, "two a side");
        assert_eq!(cells(&mesh, Some(0.01)), 64, "eight a side");
        let finest = cells(&mesh, Some(1.0 / 255.0));
        assert_eq!(cells(&mesh, Some(2.0)), 1, "past 1 is 1");
        assert_eq!(cells(&mesh, Some(0.0)), finest, "0 is the finest");
        assert_eq!(cells(&mesh, Some(-3.0)), finest, "under 0 is the finest");
        assert_eq!(cells(&mesh, Some(1e-6)), finest, "finer than a level");
        assert_eq!(cells(&mesh, Some(f64::NAN)), 1, "not a number");
        assert_eq!(cells(&mesh, Some(f64::INFINITY)), 1, "nor infinity");

        // The same patch read from a stream takes the same counts.
        let mut p = params(6, 8, 8);
        p.decode = vec![(0.0, 30.0), (0.0, 30.0), (0.0, 1.0)];
        let read =
            read_mesh(&p, &flat_patch_stream(6), ColorSpace::DeviceGray, None).expect("it reads");
        for smoothness in [0.05, 0.01] {
            assert_eq!(
                cells(&read, Some(smoothness)),
                cells(&mesh, Some(smoothness))
            );
        }
    }

    /// **Smoothness only adds steps, and stops at the cap**: at full size the
    /// device length's sixteen a side stand under a tolerance that asks for
    /// eight, and a grey stretched by its `/Decode` to a thousand times its
    /// range takes 32 a side at the finest, however few its length asks for.
    #[test]
    fn smoothness_adds_steps_up_to_the_cap() {
        let mesh = flat_patch_mesh(
            ColorSpace::DeviceGray,
            None,
            1,
            vec![0.0, 1.0 / 3.0, 2.0 / 3.0, 1.0],
        );
        let full = |smoothness| {
            mesh.tessellate(&Matrix::IDENTITY, smoothness)
                .expect("it fits")
                .triangles
                .len()
                / 2
        };
        assert_eq!(full(None), 256, "sixteen a side from the length");
        assert_eq!(
            full(Some(1.0 / 255.0)),
            256,
            "the colour's eight do not lower it"
        );

        let stretched = flat_patch_mesh(
            ColorSpace::DeviceGray,
            None,
            1,
            vec![0.0, 1000.0 / 3.0, 2000.0 / 3.0, 1000.0],
        );
        assert_eq!(cells(&stretched, Some(1.0 / 255.0)), 32 * 32, "capped");
        assert_eq!(rule(2000.0 / 3.0, 1.0 / 255.0), 32 * 32);
    }

    /// **A patch with no bilinear term takes its device count**: corners that
    /// change along one parameter alone are carried exactly by the triangles,
    /// so no tolerance asks for a step more.
    #[test]
    fn a_patch_whose_colour_is_linear_takes_its_device_count() {
        let ramp = flat_patch_mesh(ColorSpace::DeviceGray, None, 1, vec![0.0, 0.0, 1.0, 1.0]);
        for smoothness in [None, Some(1.0), Some(0.01), Some(0.0)] {
            assert_eq!(cells(&ramp, smoothness), 1, "{smoothness:?}");
        }
    }

    /// **A triangle mesh takes nothing from smoothness**: 8.7.4.5.5 defines its
    /// colour as the linear interpolation it is drawn with, so the finest
    /// tolerance leaves its triangles, positions and values as they were.
    #[test]
    fn a_triangle_mesh_takes_nothing_from_smoothness() {
        let mut writer = BitWriter::new();
        for (x, y, grey) in [(0u64, 0u64, 0u64), (255, 0, 255), (0, 255, 128)] {
            writer.push(0, 8);
            writer.push(x, 8);
            writer.push(y, 8);
            writer.push(grey, 8);
            writer.align();
        }
        let mesh =
            read_mesh(&params(4, 8, 8), &writer.data, ColorSpace::DeviceGray, None).expect("reads");
        let plain = mesh.tessellate(&Matrix::IDENTITY, None).expect("it fits");
        let smooth = mesh
            .tessellate(&Matrix::IDENTITY, Some(1.0 / 255.0))
            .expect("it fits");
        assert_eq!(plain.triangles, smooth.triangles);
        assert_eq!(plain.positions, smooth.positions);
        assert_eq!(plain.values, smooth.values);
    }

    /// **A component's range is what its tolerance is a fraction of**: Lab's
    /// L\* over 0 to 100 and its a\* over its `/Range`, and an `/Indexed`
    /// index over 0 to `hival`, each with a bilinear term two thirds of its
    /// range, take the grey patch's counts; L\* moving a hundredth as far
    /// takes far fewer.
    #[test]
    fn a_component_s_range_scales_its_tolerance() {
        let grey = flat_patch_mesh(
            ColorSpace::DeviceGray,
            None,
            1,
            vec![0.0, 1.0 / 3.0, 2.0 / 3.0, 1.0],
        );
        let lab = |corners: [[f64; 3]; 4]| {
            flat_patch_mesh(
                ColorSpace::Lab {
                    range: [-128.0, 127.0, -128.0, 127.0],
                },
                None,
                3,
                corners.iter().flatten().copied().collect(),
            )
        };
        let lightness = lab([
            [0.0, 0.0, 0.0],
            [100.0 / 3.0, 0.0, 0.0],
            [200.0 / 3.0, 0.0, 0.0],
            [100.0, 0.0, 0.0],
        ]);
        let red_green = lab([
            [50.0, -128.0, 0.0],
            [50.0, -43.0, 0.0],
            [50.0, 42.0, 0.0],
            [50.0, 127.0, 0.0],
        ]);
        let indexed = flat_patch_mesh(
            ColorSpace::Indexed {
                base: Box::new(ColorSpace::DeviceGray),
                lookup: (0..=255u8).collect(),
                high: 255,
            },
            None,
            1,
            vec![0.0, 85.0, 170.0, 255.0],
        );
        for smoothness in [0.05, 0.01, 1.0 / 255.0] {
            let want = cells(&grey, Some(smoothness));
            assert_eq!(
                cells(&lightness, Some(smoothness)),
                want,
                "L* at {smoothness}"
            );
            assert_eq!(
                cells(&red_green, Some(smoothness)),
                want,
                "a* at {smoothness}"
            );
            assert_eq!(
                cells(&indexed, Some(smoothness)),
                want,
                "index at {smoothness}"
            );
        }
        let faint = lab([
            [0.0, 0.0, 0.0],
            [1.0 / 3.0, 0.0, 0.0],
            [2.0 / 3.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
        ]);
        assert_eq!(
            cells(&faint, Some(0.01)),
            1,
            "a hundredth of L* asks nothing"
        );
        assert_eq!(cells(&grey, Some(0.01)), 64);
    }

    /// **A function's slope weighs its parametric value**: a patch whose
    /// corners carry `t` through `/Function` takes the counts its outputs'
    /// error asks for — an identity's the grey patch's, a quarter's fewer, a
    /// square's (slope near 2 at the top of the span) more — and a flat
    /// function, or corners that are all one value, take none.
    #[test]
    fn a_function_s_slope_weighs_its_parametric_value() {
        let through = |c1: f64, n: f64| {
            flat_patch_mesh(
                ColorSpace::DeviceGray,
                Some(Function::Exponential {
                    domain: (0.0, 1.0),
                    c0: vec![0.0],
                    c1: vec![c1],
                    n,
                }),
                1,
                vec![0.0, 1.0 / 3.0, 2.0 / 3.0, 1.0],
            )
        };
        let grey = flat_patch_mesh(
            ColorSpace::DeviceGray,
            None,
            1,
            vec![0.0, 1.0 / 3.0, 2.0 / 3.0, 1.0],
        );
        for smoothness in [0.05, 0.01] {
            assert_eq!(
                cells(&through(1.0, 1.0), Some(smoothness)),
                cells(&grey, Some(smoothness)),
                "the identity at {smoothness}"
            );
        }
        let quarter = through(0.25, 1.0);
        assert_eq!(cells(&quarter, Some(0.05)), rule(2.0 / 3.0 * 0.25, 0.05));
        assert_eq!(cells(&quarter, Some(0.05)), 1);
        assert_eq!(cells(&quarter, Some(0.01)), 16);
        let square = through(1.0, 2.0);
        assert_eq!(
            cells(&square, Some(0.05)),
            16,
            "four a side, where t takes two"
        );
        assert_eq!(cells(&grey, Some(0.05)), 4);

        let flat = through(0.0, 1.0);
        assert_eq!(cells(&flat, Some(1.0 / 255.0)), 1, "a flat function");
        let level = flat_patch_mesh(
            ColorSpace::DeviceGray,
            Some(Function::Exponential {
                domain: (0.0, 1.0),
                c0: vec![0.0],
                c1: vec![1.0],
                n: 1.0,
            }),
            1,
            vec![0.5; 4],
        );
        assert_eq!(
            cells(&level, Some(1.0 / 255.0)),
            1,
            "one value at every corner"
        );
    }

    /// **The colour count is a fixed-point power of two**: nothing measured is
    /// one step, a measure exactly at an allowance stops there, a measure that
    /// is not a number is one step, and one past every allowance is the cap.
    #[test]
    fn the_colour_count_is_a_fixed_point_power_of_two() {
        assert_eq!(colour_steps(0.0, 0.01), 1);
        assert_eq!(colour_steps(1.0, 0.25), 1, "exactly 4 × 1² × 0.25");
        assert_eq!(colour_steps(4.0, 0.25), 2, "exactly 4 × 2² × 0.25");
        assert_eq!(colour_steps(4.5, 0.25), 4);
        assert_eq!(colour_steps(f64::NAN, 0.01), 1);
        assert_eq!(colour_steps(f64::INFINITY, 0.01), 32);
        assert_eq!(colour_steps(1e300, 1.0), 32);
        let mut previous = 0;
        for tenth in 0..400 {
            let steps = colour_steps(f64::from(tenth) / 10.0, 0.01);
            assert!(steps.is_power_of_two() && steps >= previous, "{tenth}");
            previous = steps;
        }
    }
}
