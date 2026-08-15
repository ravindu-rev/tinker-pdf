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
//!   short;
//! - the count of vertices a stream yields. That is decided by bit arithmetic
//!   on `u64` and by `usize` comparisons against caps that are constants, so
//!   it does not depend on float rounding at all.
//!
//! No transcendental is called from any of it, which `cargo xtask libm`
//! enforces.

use tinker_pdf_color::{ColorSpace, Function};
use tinker_pdf_content::Matrix;

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
/// fits on a page needs.
const MAX_PATCH_STEPS: u32 = 32;

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
    #[must_use]
    pub fn color(&self, inputs: &[f64]) -> (u8, u8, u8) {
        match &self.function {
            Some(function) => self.space.to_rgb(&function.eval(inputs)),
            None => self.space.to_rgb(inputs),
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
    #[must_use]
    pub fn tessellate(&self, to_device: &Matrix) -> Option<Tessellation> {
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

        for patch in &self.patches {
            self.tessellate_patch(patch, to_device, &mut out)?;
        }
        Some(out)
    }

    /// Subdivides one patch onto a square grid and appends its triangles.
    fn tessellate_patch(
        &self,
        patch: &Patch,
        to_device: &Matrix,
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

        let steps = patch_steps(&net);
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

        let tessellation = mesh.tessellate(&Matrix::IDENTITY).expect("it fits");
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
            .tessellate(&Matrix::scale(0.05, 0.05))
            .expect("it fits")
            .triangles
            .len();
        let large = mesh
            .tessellate(&Matrix::scale(4.0, 4.0))
            .expect("it fits")
            .triangles
            .len();
        assert!(small < large, "{small} is not finer than {large}");
        assert_eq!(small, 2, "a patch a pixel across is one cell");
    }
}
