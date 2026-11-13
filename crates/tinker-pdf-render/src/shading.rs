//! Shadings: gradients and meshes (8.7.4.5).
//!
//! Types 1 to 3 paint a colour that varies with position, so they are
//! evaluated per pixel rather than composited from a mask, and
//! [`Shading::color_at`] is the whole of them. Types 4 to 7 are *geometry* —
//! triangles and patches — and cannot answer that question at a point, so they
//! carry a [`Mesh`] and are painted by the mesh rasteriser instead. That is why
//! `color_at` returns `None` for them rather than growing a branch that guesses.
//!
//! A shading is evaluated once a pixel, so what an evaluation writes — the
//! function's outputs, and a `/Separation` or `/DeviceN` space's tint — goes
//! into a [`ShadingScratch`] the sweep keeps from one pixel to the next, and
//! painting a shading allocates nothing per pixel.
//!
//! A sweep reads a device row at a time through [`Shading::row`]: an axial
//! shading's `t` and a radial shading's quadratic terms step from column to
//! column there instead of being evaluated afresh ([`ShadingRow`]).

use tinker_pdf_color::{ColorSpace, Function};
use tinker_pdf_content::Matrix;

use crate::mesh::Mesh;

/// A device row's inverse map, its row-invariant products taken once.
///
/// [`RowMap::at`] is `Matrix::apply` at `(x, y)` operation for operation —
/// `a·x + c·y + e` and `b·x + d·y + f`, added left to right, with `c·y` and
/// `d·y` the values this holds — so a pixel maps to the same bits whether its
/// row's products were taken once or at every column (ruling 4).
#[derive(Clone, Copy, Debug)]
pub(crate) struct RowMap {
    a: f64,
    b: f64,
    e: f64,
    f: f64,
    cy: f64,
    dy: f64,
}

impl RowMap {
    /// The map of the row at device `y`.
    pub(crate) fn new(m: &Matrix, y: f64) -> RowMap {
        RowMap {
            a: m.a,
            b: m.b,
            e: m.e,
            f: m.f,
            cy: m.c * y,
            dy: m.d * y,
        }
    }

    /// The point device `x` of this row maps to.
    pub(crate) fn at(&self, x: f64) -> (f64, f64) {
        (self.a * x + self.cy + self.e, self.b * x + self.dy + self.f)
    }
}

/// The page-frame columns at which a stepped parameter is evaluated afresh:
/// every multiple of this, whatever part of the page is being drawn.
pub const RESTART_COLUMNS: i64 = 64;

/// What up to [`RESTART_COLUMNS`] steps can leave in a stepped term, as a
/// fraction of the magnitudes it was stepped from: 2^-44, more than 64
/// roundings of four units in the last place each.
const STEP_ERROR: f64 = 1.0 / 17_592_186_044_416.0;

/// How many times its error bound a stepped value must lie from a threshold
/// 8.7.4.5.3 or 8.7.4.5.4 decides on — where a gradient starts or stops, a
/// circle's radius reaches zero, or no circle reaches the point — for the
/// stepped value to decide it; nearer, the pixel takes the clause's
/// expression evaluated at it.
const MARGIN: f64 = 1024.0;

/// The most a stepped parameter may be off and still be used: 2^-24, under
/// a ten-thousandth of a level of 255 in any colour it is mapped to.
const PRECISION: f64 = 1.0 / 16_777_216.0;

/// The largest step in `t` from one column to the next that is stepped: an
/// axis a pixel crosses more than eight thousand times over is evaluated at
/// each pixel, its steps too large to keep within [`PRECISION`].
const MAX_AXIAL_STEP: f64 = 8_192.0;

/// One device row of a shading, read left to right ([`Shading::row`]).
///
/// An axial shading's `t` is affine in the device column and a radial
/// shading's quadratic terms (8.7.4.5.4's `b` and `c`) are affine and
/// quadratic in it, so along a row they step by constant differences — `t`
/// by one addition a column, `b` by one and `c` by two — and a pixel's
/// parameter costs those additions, and for a radial shading its square
/// root, in place of the inverse map and the clause's expression. The steps
/// start afresh from the expression at every [`RESTART_COLUMNS`]th column of
/// the page frame and never at the edge of what is being drawn: a tile's
/// pixel is the page's pixel, stepped the same way from the same column
/// (ruling 5).
///
/// A stepped parameter differs from the expression evaluated at the pixel
/// by rounding alone, a few parts in 10^14 of the magnitudes it came from, so
/// a colour that lands on a rounding boundary may land a level of 255 from
/// the clause's — the bound ruling 22 sets. Where the step cannot be trusted
/// to decide whether a pixel is painted at all — a stepped value within
/// [`MARGIN`] times its error bound of where the gradient stops, of a zero
/// radius or of no circle at all, or an error bound past [`PRECISION`] — the
/// pixel takes the clause's expression, so the pixels painted are the ones
/// the expression paints. A function-based shading, a degenerate radial
/// one and an axis too short to step are evaluated at every pixel.
pub struct ShadingRow<'a> {
    shading: &'a Shading,
    map: RowMap,
    walk: Walk,
}

/// How a row's parameter is had.
enum Walk {
    /// From the clause's expression at every pixel.
    Direct,
    /// Type 2's `t`, stepped by `dt` a column.
    Axial {
        dt: f64,
        /// The column the state describes, and `t` there.
        at: Option<(i64, f64)>,
    },
    /// Type 3's `b` and `c`, stepped.
    Radial(Box<Radial>),
}

/// What a radial shading's row steps (8.7.4.5.4).
///
/// For a point `p` in shading space, relative to the first circle's centre,
/// the family's circles through it are the roots `s` of `a·s² − 2b·s + c`,
/// with `a` fixed by the shading and `b` and `c` by the point. Along a row
/// the point moves by the map's `(ma, mb)` a column, so `b` moves by `db` and
/// `c` by `dc`, which itself moves by `ddc`.
struct Radial {
    a: f64,
    inverse_a: f64,
    db: f64,
    ddc: f64,
    ma: f64,
    mb: f64,
    /// The column the state describes, if any.
    column: Option<i64>,
    b: f64,
    c: f64,
    dc: f64,
    /// Bounds on the magnitudes `b`'s and `c`'s terms reach over the block,
    /// which their errors are fractions of.
    b_scale: f64,
    c_scale: f64,
}

impl Shading {
    /// One device row of this shading, read through `inverse`, the map from
    /// device space to shading space, at the row's centre `y` in device
    /// space.
    #[must_use]
    pub fn row(&self, inverse: &Matrix, y: f64) -> ShadingRow<'_> {
        let map = RowMap::new(inverse, y);
        let walk = match self {
            Shading::Axial { coords, .. } => {
                let [ax, ay, bx, by] = *coords;
                let (dx, dy) = (bx - ax, by - ay);
                let denominator = dx * dx + dy * dy;
                let dt = (map.a * dx + map.b * dy) / denominator;
                if denominator > f64::EPSILON && dt.is_finite() && dt.abs() <= MAX_AXIAL_STEP {
                    Walk::Axial { dt, at: None }
                } else {
                    Walk::Direct
                }
            }
            Shading::Radial { coords, .. } => {
                let [x0, y0, r0, x1, y1, r1] = *coords;
                let (dx, dy, dr) = (x1 - x0, y1 - y0, r1 - r0);
                let a = dx * dx + dy * dy - dr * dr;
                let (ma, mb) = (map.a, map.b);
                let db = ma * dx + mb * dy;
                let ddc = 2.0 * (ma * ma + mb * mb);
                let finite = [a, db, ddc, r0, dr].iter().all(|v| v.is_finite());
                // `radial_parameter`'s own line between the quadratic and the
                // linear family: the linear one is not stepped.
                if finite && a.abs() >= 1e-9 {
                    Walk::Radial(Box::new(Radial {
                        a,
                        inverse_a: 1.0 / a,
                        db,
                        ddc,
                        ma,
                        mb,
                        column: None,
                        b: 0.0,
                        c: 0.0,
                        dc: 0.0,
                        b_scale: 0.0,
                        c_scale: 0.0,
                    }))
                } else {
                    Walk::Direct
                }
            }
            _ => Walk::Direct,
        };
        ShadingRow {
            shading: self,
            map,
            walk,
        }
    }
}

/// What evaluating a shading writes, kept by a sweep from one pixel to the
/// next: a function's outputs and a colour space's tint transform's, the
/// sweep's [`ColorTable`] where it reads its function through one, and how
/// many times the function was evaluated.
#[derive(Clone, Debug, Default)]
pub struct ShadingScratch {
    outputs: Vec<f64>,
    converted: Vec<f64>,
    table: Option<ColorTable>,
    evaluations: u64,
}

impl ShadingScratch {
    /// `function` at `inputs`, converted from `space`.
    pub(crate) fn color(
        &mut self,
        space: &ColorSpace,
        function: &Function,
        inputs: &[f64],
    ) -> (u8, u8, u8) {
        self.evaluations = self.evaluations.saturating_add(1);
        function.eval_into(inputs, &mut self.outputs);
        space.to_rgb_with(&self.outputs, &mut self.converted)
    }

    /// `function` at the one input `t`, converted from `space`: read from the
    /// sweep's table where it has one and it vouches for `t`, and evaluated
    /// otherwise.
    pub(crate) fn tabled(
        &mut self,
        space: &ColorSpace,
        function: &Function,
        t: f64,
    ) -> (u8, u8, u8) {
        if let Some(color) = self.table.as_ref().and_then(|table| table.color(t)) {
            return color;
        }
        self.color(space, function, &[t])
    }

    /// Reads `function`, converted from `space`, through a [`ColorTable`]
    /// over the inputs `low` to `high` for the rest of the sweep, where the
    /// function is one a table can stand in for (`Function::tabulable`) and
    /// the sweep, `asks` colours over the shape's rectangle on the whole page,
    /// would evaluate it more often than the table does. The decision is the
    /// page's, so a tile reads its colours as the page under it does
    /// (ruling 5).
    pub fn tabulate(
        &mut self,
        space: &ColorSpace,
        function: &Function,
        low: f64,
        high: f64,
        asks: u64,
    ) {
        self.table = None;
        if asks <= TABLE_ENTRIES as u64
            || !function.tabulable()
            || !low.is_finite()
            || !high.is_finite()
            || !(high > low)
        {
            return;
        }
        self.table = Some(ColorTable::new(self, space, function, low, high));
    }

    /// How many times a function was evaluated through this scratch: a table's
    /// entries, and each colour asked of a function no table stands in for.
    #[must_use]
    pub fn evaluations(&self) -> u64 {
        self.evaluations
    }

    /// `components` of `space`, converted, with no function between.
    pub(crate) fn convert(&mut self, space: &ColorSpace, components: &[f64]) -> (u8, u8, u8) {
        space.to_rgb_with(components, &mut self.converted)
    }
}

/// How many entries a [`ColorTable`] holds: the function's colours at 1 025
/// evenly spaced inputs, 1 024 intervals, which an input reaches in 16.16
/// fixed point.
pub const TABLE_ENTRIES: usize = 1025;

/// A one-input shading function's colours, sampled at [`TABLE_ENTRIES`] evenly
/// spaced inputs over the range the sweep can ask for, converted there, and
/// read per pixel through the input in 16.16 fixed point, the two entries
/// either side of it mixed in integers (P-14.5).
///
/// The table is built from the function itself, `tinker-pdf-math`'s `pow`
/// for an exponent other than one, so it is the same bits on every target
/// (ruling 4), and a lookup is a function of the input alone, so a tile and
/// the page under it read the same colour wherever they start. At an entry a
/// lookup is the function's colour exactly. Between entries it is within a
/// level of 255 of the function's colour wherever the function is linear
/// enough over two intervals that their middle entry lies within a level of
/// the mean of their ends; an interval pair where it does not — a stitch's
/// jump, a curve's steep foot — is not vouched for, and an input there is
/// evaluated as it is asked (ruling 22).
#[derive(Clone, Debug)]
pub struct ColorTable {
    /// The input of the first entry.
    low: f64,
    /// Entries a unit of input, in sixty-five-thousand-five-hundred-and-
    /// thirty-sixths: the input's fixed-point position is `(t − low) · scale`.
    scale: f64,
    entries: Vec<[u8; 3]>,
    /// For each pair of intervals, whether a lookup there is vouched for.
    vouched: Vec<bool>,
}

impl ColorTable {
    /// The table of `function` over `low` to `high`, evaluated through
    /// `scratch`, which counts the evaluations.
    fn new(
        scratch: &mut ShadingScratch,
        space: &ColorSpace,
        function: &Function,
        low: f64,
        high: f64,
    ) -> ColorTable {
        let last = (TABLE_ENTRIES - 1) as f64;
        let span = high - low;
        let entries: Vec<[u8; 3]> = (0..TABLE_ENTRIES)
            .map(|entry| {
                // The last entry is `high` itself, which a sum can miss.
                let t = if entry + 1 == TABLE_ENTRIES {
                    high
                } else {
                    low + span * (entry as f64 / last)
                };
                let (r, g, b) = scratch.color(space, function, &[t]);
                [r, g, b]
            })
            .collect();
        let vouched = entries
            .windows(3)
            .step_by(2)
            .map(|three| {
                (0..3).all(|channel| {
                    let (first, middle, last) = (
                        i32::from(three[0][channel]),
                        i32::from(three[1][channel]),
                        i32::from(three[2][channel]),
                    );
                    (2 * middle - first - last).abs() <= 1
                })
            })
            .collect();
        ColorTable {
            low,
            scale: last * 65_536.0 / span,
            entries,
            vouched,
        }
    }

    /// The colour at input `t`, or `None` where the table does not vouch for
    /// it. An input past either end reads that end's entry, which is the
    /// function's colour there, since a sweep's inputs lie between them.
    #[must_use]
    pub fn color(&self, t: f64) -> Option<(u8, u8, u8)> {
        let last = self.entries.len().checked_sub(1)?;
        let top = last as f64 * 65_536.0;
        let position = ((t - self.low) * self.scale).round();
        if position.is_nan() {
            return None;
        }
        // In `0..=top`, a whole number under 2^27: exact as an integer.
        let position = position.clamp(0.0, top) as u64;
        let index = (position >> 16) as usize;
        let fraction = (position & 0xFFFF) as u32;
        if index >= last {
            let [r, g, b] = *self.entries.get(last)?;
            return Some((r, g, b));
        }
        if !self.vouched.get(index / 2).copied().unwrap_or(false) {
            return None;
        }
        let (below, above) = (self.entries.get(index)?, self.entries.get(index + 1)?);
        let mix = |a: u8, b: u8| {
            ((u32::from(a) * (65_536 - fraction) + u32::from(b) * fraction + 32_768) >> 16) as u8
        };
        Some((
            mix(below[0], above[0]),
            mix(below[1], above[1]),
            mix(below[2], above[2]),
        ))
    }
}

/// A shading, reduced to what painting needs.
#[derive(Clone, Debug)]
pub enum Shading {
    /// Type 1: a function of `(x, y)` over a domain.
    FunctionBased {
        /// The colour space the function's outputs are in.
        space: ColorSpace,
        /// The function.
        function: Function,
        /// `[x0 x1 y0 y1]`, the domain in shading space.
        domain: [f64; 4],
    },
    /// Type 2: a gradient along the line between two points.
    Axial {
        /// The colour space.
        space: ColorSpace,
        /// The function from parameter to colour.
        function: Function,
        /// `[x0 y0 x1 y1]`, the axis in shading space.
        coords: [f64; 4],
        /// Whether the gradient continues past each end.
        extend: (bool, bool),
    },
    /// Type 3: a gradient between two circles.
    Radial {
        /// The colour space.
        space: ColorSpace,
        /// The function from parameter to colour.
        function: Function,
        /// `[x0 y0 r0 x1 y1 r1]`, the two circles.
        coords: [f64; 6],
        /// Whether the gradient continues past each end.
        extend: (bool, bool),
    },
    /// Types 4 to 7: a mesh of Gouraud triangles or of Coons or tensor
    /// patches (8.7.4.5.5 to 8.7.4.5.8).
    Mesh(Box<Mesh>),
}

impl Shading {
    /// An axial or radial shading's colour space and its function of the
    /// parameter, which a sweep may read through a [`ColorTable`] over `0` to
    /// `1`: the parameter it hands the function, `/Extend` applied.
    #[must_use]
    pub fn parametric(&self) -> Option<(&ColorSpace, &Function)> {
        match self {
            Shading::Axial {
                space, function, ..
            }
            | Shading::Radial {
                space, function, ..
            } => Some((space, function)),
            _ => None,
        }
    }

    /// The mesh a type 4 to 7 shading paints, if this is one.
    #[must_use]
    pub fn mesh(&self) -> Option<&Mesh> {
        match self {
            Shading::Mesh(mesh) => Some(mesh),
            _ => None,
        }
    }

    /// The colour at a point in shading space, or `None` where the shading
    /// paints nothing: the clause's expression evaluated at the point.
    /// `scratch` is the sweep's, the same one for every pixel.
    ///
    /// Always `None` for a mesh: a mesh is not a function of position, and
    /// answering "what colour is here" would mean locating the containing
    /// triangle per pixel, which is the search the rasteriser exists to avoid.
    #[must_use]
    pub fn color_at(&self, x: f64, y: f64, scratch: &mut ShadingScratch) -> Option<(u8, u8, u8)> {
        if !x.is_finite() || !y.is_finite() {
            return None;
        }

        match self {
            Shading::Mesh(_) => None,
            Shading::FunctionBased {
                space,
                function,
                domain,
            } => {
                let [x0, x1, y0, y1] = *domain;
                if x < x0.min(x1) || x > x0.max(x1) || y < y0.min(y1) || y > y0.max(y1) {
                    return None;
                }
                Some(scratch.color(space, function, &[x, y]))
            }

            Shading::Axial {
                space,
                function,
                coords,
                extend,
            } => {
                let t = axial_parameter(*coords, x, y)?;
                let t = clamp_extend(t, *extend)?;
                Some(scratch.tabled(space, function, t))
            }

            Shading::Radial {
                space,
                function,
                coords,
                extend,
            } => {
                let t = radial_parameter(*coords, x, y, *extend)?;
                Some(scratch.tabled(space, function, t))
            }
        }
    }
}

impl ShadingRow<'_> {
    /// The colour at page-frame column `px` of the row, or `None` where the
    /// shading paints nothing. Columns are asked for left to right, any of
    /// them skipped; a column asked for out of order costs a restart and
    /// gets the same colour. `scratch` is the sweep's.
    pub fn color_at(&mut self, px: i64, scratch: &mut ShadingScratch) -> Option<(u8, u8, u8)> {
        // The stepped parameter, `/Extend` applied, where the steps decide
        // the pixel: `None` sends it to the clause's expression.
        let stepped = match (self.shading, &mut self.walk) {
            (
                Shading::Axial {
                    space,
                    function,
                    coords,
                    extend,
                },
                Walk::Axial { dt, at },
            ) => {
                let t = axial_step(&self.map, *coords, *dt, at, px);
                let e = STEP_ERROR * (1.0 + t.abs() + 64.0 * dt.abs());
                // Negated, so an error no comparison can order is undecided.
                let undecided = !(e <= PRECISION)
                    || (!extend.0 && t.abs() <= MARGIN * e)
                    || (!extend.1 && (t - 1.0).abs() <= MARGIN * e);
                (!undecided).then(|| (space, function, clamp_extend(t, *extend)))
            }
            (
                Shading::Radial {
                    space,
                    function,
                    coords,
                    extend,
                },
                Walk::Radial(radial),
            ) => radial
                .parameter(&self.map, *coords, *extend, px)
                .ok()
                .map(|t| (space, function, t)),
            _ => None,
        };
        match stepped {
            Some((space, function, t)) => Some(scratch.tabled(space, function, t?)),
            None => {
                count::evaluated();
                let (x, y) = self.map.at(px as f64 + 0.5);
                self.shading.color_at(x, y, scratch)
            }
        }
    }
}

/// The column a page-frame column's steps start from: the last multiple of
/// [`RESTART_COLUMNS`] at or before it.
fn restart_of(px: i64) -> i64 {
    px - px.rem_euclid(RESTART_COLUMNS)
}

/// An axial shading's `t` at column `px`, before `/Extend` is applied:
/// `at`'s state stepped there, or the expression evaluated afresh at the
/// column the steps restart from and stepped from it.
fn axial_step(
    map: &RowMap,
    coords: [f64; 4],
    dt: f64,
    at: &mut Option<(i64, f64)>,
    px: i64,
) -> f64 {
    let restart = restart_of(px);
    let (mut column, mut t) = match *at {
        Some((column, t)) if column >= restart && column <= px => (column, t),
        _ => {
            count::evaluated();
            let (x, y) = map.at(restart as f64 + 0.5);
            // The axis has a length, or the row would not step.
            let [ax, ay, bx, by] = coords;
            let (dx, dy) = (bx - ax, by - ay);
            (
                restart,
                ((x - ax) * dx + (y - ay) * dy) / (dx * dx + dy * dy),
            )
        }
    };
    while column < px {
        t += dt;
        column += 1;
    }
    *at = Some((column, t));
    t
}

/// A stepped radial value too near a threshold, or too imprecise, to decide
/// the pixel by.
struct Undecided;

impl Radial {
    /// Brings the state to column `px`: afresh at the column its steps
    /// restart from when the state is in another block or behind it, and
    /// stepped from there.
    fn reach(&mut self, map: &RowMap, coords: [f64; 6], px: i64) {
        let restart = restart_of(px);
        let mut column = match self.column {
            Some(column) if column >= restart && column <= px => column,
            _ => {
                count::evaluated();
                let [x0, y0, r0, x1, y1, r1] = coords;
                let (dx, dy, dr) = (x1 - x0, y1 - y0, r1 - r0);
                let (x, y) = map.at(restart as f64 + 0.5);
                // `radial_parameter`'s terms, in its order, of the point
                // relative to the first circle's centre.
                let (ux, uy) = (x - x0, y - y0);
                self.b = ux * dx + uy * dy + r0 * dr;
                self.c = ux * ux + uy * uy - r0 * r0;
                self.dc =
                    2.0 * (ux * self.ma + uy * self.mb) + (self.ma * self.ma + self.mb * self.mb);
                let span = RESTART_COLUMNS as f64;
                self.b_scale = ux.abs() * dx.abs()
                    + uy.abs() * dy.abs()
                    + (r0 * dr).abs()
                    + span * ((self.ma * dx).abs() + (self.mb * dy).abs());
                let reach_x = ux.abs() + span * self.ma.abs();
                let reach_y = uy.abs() + span * self.mb.abs();
                self.c_scale = reach_x * reach_x + reach_y * reach_y + r0 * r0;
                restart
            }
        };
        while column < px {
            self.b += self.db;
            self.c += self.dc;
            self.dc += self.ddc;
            column += 1;
        }
        self.column = Some(column);
    }

    /// The parameter at column `px` (8.7.4.5.4), as `radial_parameter`
    /// decides it but from the stepped terms, or [`Undecided`] where the
    /// stepped terms cannot be trusted to decide it.
    fn parameter(
        &mut self,
        map: &RowMap,
        coords: [f64; 6],
        extend: (bool, bool),
        px: i64,
    ) -> Result<Option<f64>, Undecided> {
        self.reach(map, coords, px);
        let [_, _, r0, _, _, r1] = coords;
        let dr = r1 - r0;
        let (a, b, c) = (self.a, self.b, self.c);
        let e_b = STEP_ERROR * self.b_scale;
        let e_c = 4.0 * STEP_ERROR * self.c_scale;
        let discriminant = b * b - a * c;
        let e_discriminant = 2.0 * b.abs() * e_b + e_b * e_b + a.abs() * e_c;
        // Negated, so a value no comparison can order is undecided too.
        if !(discriminant.abs() > MARGIN * e_discriminant) {
            return Err(Undecided);
        }
        if discriminant < 0.0 {
            return Ok(None); // No circle in the family reaches this point.
        }
        let root = discriminant.sqrt();
        let e_root = e_discriminant / root;
        let mut best: Option<f64> = None;
        for candidate in [(b + root) * self.inverse_a, (b - root) * self.inverse_a] {
            let e = (e_b + e_root) * self.inverse_a.abs() + STEP_ERROR * candidate.abs();
            // A candidate past every number, or an error bound too wide to
            // keep a colour within a level, is the clause's to decide.
            if !candidate.is_finite() || !(e <= PRECISION) {
                return Err(Undecided);
            }
            let radius = r0 + candidate * dr;
            let e_radius = dr.abs() * e + STEP_ERROR * (r0.abs() + (candidate * dr).abs());
            if radius.abs() <= MARGIN * e_radius {
                return Err(Undecided);
            }
            if radius < 0.0 {
                continue; // A circle with a negative radius does not exist.
            }
            if (!extend.0 && candidate.abs() <= MARGIN * e)
                || (!extend.1 && (candidate - 1.0).abs() <= MARGIN * e)
            {
                return Err(Undecided);
            }
            let Some(clamped) = clamp_extend(candidate, extend) else {
                continue;
            };
            best = Some(match best {
                None => clamped,
                Some(previous) => previous.max(clamped),
            });
        }
        Ok(best)
    }
}

/// An axial shading's parameter at a point (8.7.4.5.3): the projection onto
/// the axis as a fraction of its length, before `/Extend`. `None` for an axis
/// of no length, which paints nothing.
fn axial_parameter(coords: [f64; 4], x: f64, y: f64) -> Option<f64> {
    let [ax, ay, bx, by] = coords;
    let (dx, dy) = (bx - ax, by - ay);
    let denominator = dx * dx + dy * dy;
    if denominator <= f64::EPSILON {
        return None; // A zero-length axis paints nothing.
    }
    // The parameter is the projection onto the axis.
    Some(((x - ax) * dx + (y - ay) * dy) / denominator)
}

/// Clamps a parameter to `0..=1`, or refuses it where the shading does not
/// extend (8.7.4.5.3).
fn clamp_extend(t: f64, extend: (bool, bool)) -> Option<f64> {
    if !t.is_finite() {
        return None;
    }
    if t < 0.0 {
        return extend.0.then_some(0.0);
    }
    if t > 1.0 {
        return extend.1.then_some(1.0);
    }
    Some(t)
}

/// The parameter of the largest circle in the family that contains a point
/// (8.7.4.5.4).
///
/// The family interpolates centre and radius linearly, so the condition is a
/// quadratic in `s`; the largest root with a non-negative radius wins, which
/// is what makes a radial gradient paint front-to-back correctly.
fn radial_parameter(coords: [f64; 6], x: f64, y: f64, extend: (bool, bool)) -> Option<f64> {
    let [x0, y0, r0, x1, y1, r1] = coords;

    let (dx, dy, dr) = (x1 - x0, y1 - y0, r1 - r0);
    let (px, py) = (x - x0, y - y0);

    let a = dx * dx + dy * dy - dr * dr;
    let b = px * dx + py * dy + r0 * dr;
    let c = px * px + py * py - r0 * r0;

    let candidates: [Option<f64>; 2] = if a.abs() < 1e-9 {
        // Degenerate: the circles have the same size, so it is linear.
        if b.abs() < 1e-12 {
            return None;
        }
        [Some(c / (2.0 * b)), None]
    } else {
        let discriminant = b * b - a * c;
        if discriminant < 0.0 {
            return None; // No circle in the family reaches this point.
        }
        let root = discriminant.sqrt();
        [Some((b + root) / a), Some((b - root) / a)]
    };

    // Prefer the larger parameter: the later circle is painted on top.
    let mut best: Option<f64> = None;
    for candidate in candidates.into_iter().flatten() {
        if !candidate.is_finite() {
            continue;
        }
        // A circle with a negative radius does not exist.
        if r0 + candidate * dr < 0.0 {
            continue;
        }
        let Some(clamped) = clamp_extend(candidate, extend) else {
            continue;
        };
        best = Some(match best {
            None => clamped,
            Some(previous) => previous.max(clamped),
        });
    }
    best
}

/// The parameter's evaluations from the clause's expression, counted for the
/// tests: compiled only under `cfg(test)` and kept per thread.
#[cfg(test)]
mod count {
    use std::cell::Cell;

    std::thread_local! {
        static EVALUATED: Cell<u64> = const { Cell::new(0) };
    }

    /// One parameter evaluated from the expression rather than stepped.
    pub(super) fn evaluated() {
        EVALUATED.with(|count| count.set(count.get() + 1));
    }

    /// The evaluations on this thread since the last call.
    pub(super) fn take() -> u64 {
        EVALUATED.with(|count| count.replace(0))
    }
}

/// The counter outside tests: nothing.
#[cfg(not(test))]
mod count {
    pub(super) fn evaluated() {}
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The colour at a point, from a scratch of its own.
    fn at(shading: &Shading, x: f64, y: f64) -> Option<(u8, u8, u8)> {
        shading.color_at(x, y, &mut ShadingScratch::default())
    }

    fn ramp() -> Function {
        // Black at zero, white at one.
        Function::Exponential {
            domain: (0.0, 1.0),
            c0: vec![0.0],
            c1: vec![1.0],
            n: 1.0,
        }
    }

    fn axial(extend: (bool, bool)) -> Shading {
        Shading::Axial {
            space: ColorSpace::DeviceGray,
            function: ramp(),
            coords: [0.0, 0.0, 10.0, 0.0],
            extend,
        }
    }

    #[test]
    fn an_axial_gradient_runs_along_its_axis() {
        let shading = axial((true, true));
        assert_eq!(at(&shading, 0.0, 0.0), Some((0, 0, 0)), "the start");
        assert_eq!(at(&shading, 10.0, 0.0), Some((255, 255, 255)), "the end");
        let middle = at(&shading, 5.0, 0.0).expect("a colour");
        assert!(
            (125..=130).contains(&middle.0),
            "halfway is mid grey, got {middle:?}"
        );
    }

    #[test]
    fn the_axis_is_a_projection_not_a_distance() {
        // A point off the axis takes the colour of its projection onto it.
        let shading = axial((true, true));
        assert_eq!(at(&shading, 5.0, 100.0), at(&shading, 5.0, 0.0));
    }

    #[test]
    fn extend_decides_what_happens_past_the_ends() {
        let extended = axial((true, true));
        assert_eq!(at(&extended, -50.0, 0.0), Some((0, 0, 0)));
        assert_eq!(at(&extended, 50.0, 0.0), Some((255, 255, 255)));

        let bounded = axial((false, false));
        assert_eq!(at(&bounded, -50.0, 0.0), None, "nothing before");
        assert_eq!(at(&bounded, 50.0, 0.0), None, "nothing after");
        assert!(at(&bounded, 5.0, 0.0).is_some(), "but inside is painted");
    }

    #[test]
    fn a_degenerate_axis_paints_nothing() {
        let shading = Shading::Axial {
            space: ColorSpace::DeviceGray,
            function: ramp(),
            coords: [3.0, 3.0, 3.0, 3.0],
            extend: (true, true),
        };
        assert_eq!(at(&shading, 3.0, 3.0), None);
    }

    #[test]
    fn a_radial_gradient_runs_between_its_circles() {
        // Concentric: a point at the inner radius is at zero, the outer at one.
        let shading = Shading::Radial {
            space: ColorSpace::DeviceGray,
            function: ramp(),
            coords: [0.0, 0.0, 0.0, 0.0, 0.0, 10.0],
            extend: (true, true),
        };

        assert_eq!(at(&shading, 0.0, 0.0), Some((0, 0, 0)), "the centre");
        assert_eq!(at(&shading, 10.0, 0.0), Some((255, 255, 255)), "the rim");
        let middle = at(&shading, 5.0, 0.0).expect("a colour");
        assert!(
            (125..=130).contains(&middle.0),
            "halfway out is mid grey, got {middle:?}"
        );
    }

    #[test]
    fn a_radial_gradient_that_does_not_extend_stops_at_its_rim() {
        let shading = Shading::Radial {
            space: ColorSpace::DeviceGray,
            function: ramp(),
            coords: [0.0, 0.0, 0.0, 0.0, 0.0, 10.0],
            extend: (false, false),
        };
        assert!(at(&shading, 5.0, 0.0).is_some(), "inside");
        assert_eq!(at(&shading, 50.0, 0.0), None, "well outside");
    }

    #[test]
    fn nonsense_coordinates_are_refused_rather_than_painted() {
        let shading = axial((true, true));
        assert_eq!(at(&shading, f64::NAN, 0.0), None);
        assert_eq!(at(&shading, 0.0, f64::INFINITY), None);

        // A radial family with no real solution anywhere.
        let impossible = Shading::Radial {
            space: ColorSpace::DeviceGray,
            function: ramp(),
            coords: [0.0, 0.0, 5.0, 0.0, 0.0, 5.0],
            extend: (false, false),
        };
        let _ = at(&impossible, 100.0, 100.0);
    }

    /// **A sweep's one scratch paints what a fresh one would at every
    /// pixel.** Nothing an evaluation leaves in the scratch may reach the
    /// next: not a longer function's outputs before a shorter one's, not a
    /// tint transform's, not a type 4 program's stack. Each shading is swept
    /// over a grid through one scratch, against a fresh scratch per point,
    /// and the shadings take turns on the one scratch so their outputs
    /// differ in length from one point to the next.
    #[test]
    fn a_kept_scratch_paints_what_a_fresh_one_does() {
        let gamma = |c0: Vec<f64>, c1: Vec<f64>, n: f64| Function::Exponential {
            domain: (0.0, 1.0),
            c0,
            c1,
            n,
        };
        let spot = ColorSpace::Separation {
            components: 1,
            alternate: Box::new(ColorSpace::DeviceCmyk),
            tint: Box::new(gamma(vec![0.0; 4], vec![0.1, 0.7, 0.2, 0.05], 1.8)),
            colorants: tinker_pdf_color::Colorants::default(),
        };
        let two_inks = ColorSpace::Separation {
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
        let shadings = [
            Shading::Axial {
                space: ColorSpace::DeviceRgb,
                function: gamma(vec![0.0, 0.1, 0.6], vec![1.0, 0.9, 0.2], 1.0),
                coords: [0.0, 0.0, 40.0, 25.0],
                extend: (true, false),
            },
            Shading::Radial {
                space: ColorSpace::DeviceRgb,
                function: Function::Stitching {
                    domain: (0.0, 1.0),
                    functions: vec![
                        gamma(vec![1.0, 1.0, 1.0], vec![0.9, 0.2, 0.1], 2.2),
                        gamma(vec![0.9, 0.2, 0.1], vec![0.0, 0.0, 0.3], 0.5),
                    ],
                    bounds: vec![0.4],
                    encode: vec![(0.0, 1.0), (0.0, 1.0)],
                },
                coords: [20.0, 20.0, 2.0, 24.0, 18.0, 30.0],
                extend: (false, true),
            },
            Shading::Axial {
                space: spot,
                function: ramp(),
                coords: [40.0, 0.0, 0.0, 40.0],
                extend: (true, true),
            },
            Shading::FunctionBased {
                space: two_inks,
                function: Function::PostScript {
                    domain: vec![(0.0, 40.0); 2],
                    range: vec![(0.0, 1.0); 2],
                    program: tinker_pdf_color::function::parse_postscript(
                        b"{ 40 div exch 40 div 2 copy gt { exch } if }",
                    ),
                },
                domain: [0.0, 40.0, 0.0, 40.0],
            },
            Shading::Axial {
                space: ColorSpace::DeviceGray,
                function: Function::Sampled {
                    domain: vec![(0.0, 1.0)],
                    range: vec![(0.0, 1.0)],
                    size: vec![5],
                    bits: 8,
                    encode: vec![],
                    decode: vec![],
                    samples: vec![0, 60, 130, 200, 255],
                },
                coords: [0.0, 40.0, 40.0, 0.0],
                extend: (true, true),
            },
        ];
        let mut kept = ShadingScratch::default();
        for row in 0..40 {
            for column in 0..40 {
                let (x, y) = (f64::from(column) + 0.5, f64::from(row) + 0.5);
                for (which, shading) in shadings.iter().enumerate() {
                    assert_eq!(
                        shading.color_at(x, y, &mut kept),
                        at(shading, x, y),
                        "shading {which} at ({x}, {y})"
                    );
                }
            }
        }
    }

    /// **A row's map is the matrix's map to the bit.** The shading sweeps take
    /// `c·y` and `d·y` once a row; every pixel must still map to the bits
    /// `Matrix::apply` gives it, or hoisting them would move pixels. Matrices
    /// whose products round, a rotation and a shear among them, at
    /// pixel-centre coordinates far enough out for the sums to round too.
    #[test]
    fn a_row_map_is_the_matrix_s_map_to_the_bit() {
        let matrices = [
            Matrix {
                a: 0.3,
                b: 0.7,
                c: -1.1,
                d: 0.9,
                e: 13.37,
                f: -2.71,
            },
            Matrix {
                a: 1.0 / 3.0,
                b: 0.0,
                c: 0.0,
                d: -1.0 / 3.0,
                e: 0.1,
                f: 280.6,
            },
            Matrix {
                a: 0.866_025_403_784_438_6,
                b: 0.5,
                c: -0.5,
                d: 0.866_025_403_784_438_6,
                e: -123.456,
                f: 789.012,
            },
        ];
        let mut seed = 0x2545_f491_4f6c_dd1du64;
        for m in &matrices {
            for _ in 0..2_000 {
                seed ^= seed << 13;
                seed ^= seed >> 7;
                seed ^= seed << 17;
                let x = (seed % 9_000) as f64 + 0.5;
                let y = ((seed >> 20) % 9_000) as f64 + 0.5;
                let (ax, ay) = m.apply(x, y);
                let (rx, ry) = RowMap::new(m, y).at(x);
                assert_eq!(
                    (ax.to_bits(), ay.to_bits()),
                    (rx.to_bits(), ry.to_bits()),
                    "({x}, {y}) under {m:?}"
                );
            }
        }
    }

    /// A map from device space to shading space with every coefficient in
    /// play, as a rotated and scaled page's inverse has.
    fn tilted() -> Matrix {
        Matrix {
            a: 0.37,
            b: 0.11,
            c: -0.21,
            d: 0.43,
            e: 3.1,
            f: -7.9,
        }
    }

    /// Three outputs that move apart, so a parameter a little off shows in
    /// every channel.
    fn spread_ramp() -> Function {
        Function::Exponential {
            domain: (0.0, 1.0),
            c0: vec![0.0, 0.2, 0.9],
            c1: vec![1.0, 0.7, 0.1],
            n: 1.0,
        }
    }

    /// An axial shading along a slanted axis and a radial one between two
    /// concentric circles, both extended: shadings whose every pixel the
    /// steps decide, with no pixel near where either stops.
    fn stepped_shadings() -> [Shading; 2] {
        [
            Shading::Axial {
                space: ColorSpace::DeviceRgb,
                function: spread_ramp(),
                coords: [-5.0, 2.0, 60.0, 41.0],
                extend: (true, true),
            },
            Shading::Radial {
                space: ColorSpace::DeviceRgb,
                function: spread_ramp(),
                coords: [20.3, 14.9, 3.0, 20.3, 14.9, 90.0],
                extend: (true, true),
            },
        ]
    }

    /// The clause's colour at page-frame column `px` of the row at `y`.
    fn direct(shading: &Shading, inverse: &Matrix, y: f64, px: i64) -> Option<(u8, u8, u8)> {
        let (x, y) = RowMap::new(inverse, y).at(px as f64 + 0.5);
        at(shading, x, y)
    }

    /// **P-14.3's exit.** A row evaluates its parameter from the clause's
    /// expression once per restart column it reaches and steps the rest:
    /// walked whole, walked with gaps as a clip leaves them, and walked from
    /// partway along as a tile is.
    #[test]
    fn a_row_evaluates_its_parameter_from_scratch_only_at_restart_columns() {
        let inverse = tilted();
        let mut scratch = ShadingScratch::default();
        for shading in &stepped_shadings() {
            for (first, last, every) in [
                (5_i64, 300_i64, 1_usize),
                (5, 300, 7),
                (130, 300, 1),
                (0, 63, 1),
            ] {
                let restarts = (restart_of(last) - restart_of(first)) / RESTART_COLUMNS + 1;
                let mut walk = shading.row(&inverse, 23.5);
                let _ = count::take();
                for px in (first..=last).step_by(every) {
                    let _ = walk.color_at(px, &mut scratch);
                }
                let evaluated = count::take();
                assert!(
                    evaluated >= 1 && evaluated <= restarts as u64,
                    "{shading:?}, columns {first} to {last} every {every}: {evaluated} \
                     evaluations for {restarts} restart columns"
                );
            }
        }
    }

    /// **Ruling 5.** A pixel is stepped from its own restart column whatever
    /// column the walk began at and whichever columns it skipped, so a tile
    /// starting anywhere paints the page's colours to the bit.
    #[test]
    fn a_tile_steps_as_the_page_does() {
        let inverse = tilted();
        let mut scratch = ShadingScratch::default();
        for shading in &stepped_shadings() {
            let mut page = shading.row(&inverse, 41.5);
            let whole: Vec<_> = (0..300).map(|px| page.color_at(px, &mut scratch)).collect();
            for (first, every) in [(37_i64, 1_usize), (64, 1), (100, 3), (299, 1), (1, 64)] {
                let mut tile = shading.row(&inverse, 41.5);
                for px in (first..300).step_by(every) {
                    assert_eq!(
                        tile.color_at(px, &mut scratch),
                        whole[px as usize],
                        "{shading:?} at column {px}, the tile starting at {first}"
                    );
                }
            }
            // Out of order costs a restart and changes nothing.
            let mut back = shading.row(&inverse, 41.5);
            for px in [250_i64, 70, 71, 3, 299] {
                assert_eq!(back.color_at(px, &mut scratch), whole[px as usize]);
            }
        }
    }

    /// **Ruling 22.** A stepped pixel is within one level of 255 of the
    /// clause's expression evaluated at it, in every channel, and is painted
    /// exactly where the expression paints: over axial and radial shadings
    /// that extend and that stop, a radial pair one circle of which is outside
    /// the other, so that some points no circle reaches, a cone whose smaller
    /// circle has its radius reach zero, and maps that rotate, shear and
    /// magnify.
    #[test]
    fn a_stepped_parameter_paints_within_a_level_of_the_clause() {
        let maps = [
            tilted(),
            Matrix {
                a: 1.0,
                b: 0.0,
                c: 0.0,
                d: -1.0,
                e: 0.0,
                f: 120.0,
            },
            Matrix {
                a: 1.0 / 3.0,
                b: 0.0,
                c: 0.0,
                d: 1.0 / 3.0,
                e: -0.25,
                f: 0.75,
            },
            Matrix {
                a: 0.866_025_403_784_438_6 / 7.0,
                b: 0.5 / 7.0,
                c: -0.5 / 7.0,
                d: 0.866_025_403_784_438_6 / 7.0,
                e: 11.0,
                f: -3.0,
            },
        ];
        let mut shadings = Vec::new();
        for extend in [(true, true), (false, false), (true, false), (false, true)] {
            shadings.push(Shading::Axial {
                space: ColorSpace::DeviceRgb,
                function: spread_ramp(),
                coords: [12.5, 30.0, 92.5, 30.0],
                extend,
            });
            shadings.push(Shading::Axial {
                space: ColorSpace::DeviceRgb,
                function: spread_ramp(),
                coords: [-5.0, 2.0, 60.0, 41.0],
                extend,
            });
            shadings.push(Shading::Radial {
                space: ColorSpace::DeviceRgb,
                function: spread_ramp(),
                coords: [40.0, 40.0, 5.0, 40.0, 40.0, 35.0],
                extend,
            });
            shadings.push(Shading::Radial {
                space: ColorSpace::DeviceRgb,
                function: spread_ramp(),
                coords: [20.0, 30.0, 6.0, 70.0, 45.0, 12.0],
                extend,
            });
            shadings.push(Shading::Radial {
                space: ColorSpace::DeviceRgb,
                function: spread_ramp(),
                coords: [30.0, 20.0, 18.0, 60.0, 50.0, 2.0],
                extend,
            });
        }
        let mut scratch = ShadingScratch::default();
        for inverse in &maps {
            for shading in &shadings {
                for row in (0..120).step_by(7) {
                    let y = f64::from(row) + 0.5;
                    let mut walk = shading.row(inverse, y);
                    for px in 0..200 {
                        let stepped = walk.color_at(px, &mut scratch);
                        let clause = direct(shading, inverse, y, px);
                        match (stepped, clause) {
                            (None, None) => {}
                            (Some(s), Some(c)) => assert!(
                                [(s.0, c.0), (s.1, c.1), (s.2, c.2)]
                                    .iter()
                                    .all(|(s, c)| s.abs_diff(*c) <= 1),
                                "{shading:?} under {inverse:?} at ({px}, {y}): {s:?} \
                                 against {c:?}"
                            ),
                            _ => panic!(
                                "{shading:?} under {inverse:?} at ({px}, {y}): stepped \
                                 {stepped:?}, the clause {clause:?}"
                            ),
                        }
                    }
                }
            }
        }
    }

    /// Where a gradient that does not extend starts and stops on a pixel
    /// centre, the clause paints that pixel and not its neighbour outside;
    /// the steps, which carry rounding there, leave the decision to it, and
    /// every pixel between is within a level of the clause's.
    #[test]
    fn where_a_gradient_stops_on_a_pixel_centre_the_clause_decides() {
        let identity = Matrix {
            a: 1.0,
            b: 0.0,
            c: 0.0,
            d: 1.0,
            e: 0.0,
            f: 0.0,
        };
        // An axis sixty long from 10.5: t is 0 at column 10 and 1 at 70,
        // and a sixtieth is not a binary fraction.
        let shading = Shading::Axial {
            space: ColorSpace::DeviceRgb,
            function: spread_ramp(),
            coords: [10.5, 0.0, 70.5, 0.0],
            extend: (false, false),
        };
        let mut scratch = ShadingScratch::default();
        let mut walk = shading.row(&identity, 3.5);
        let painted: Vec<i64> = (0..128)
            .filter(|&px| walk.color_at(px, &mut scratch).is_some())
            .collect();
        assert_eq!(painted, (10..=70).collect::<Vec<_>>());
        let mut walk = shading.row(&identity, 3.5);
        for px in 0..128 {
            let stepped = walk.color_at(px, &mut scratch);
            let clause = direct(&shading, &identity, 3.5, px);
            assert_eq!(stepped.is_some(), clause.is_some(), "column {px}");
            if let (Some(s), Some(c)) = (stepped, clause) {
                assert!(
                    s.0.abs_diff(c.0) <= 1 && s.1.abs_diff(c.1) <= 1 && s.2.abs_diff(c.2) <= 1,
                    "column {px}: {s:?} against {c:?}"
                );
            }
        }
    }

    /// The shadings that are not stepped — a function-based one, a radial
    /// family whose first circle touches the second from inside, which
    /// 8.7.4.5.4's quadratic leaves linear, an axis of no length, and a map
    /// no number can follow — are the clause's expression at every pixel,
    /// one evaluation a pixel.
    #[test]
    fn a_shading_that_does_not_step_is_the_clause_at_every_pixel() {
        // The centres five apart and the radii five apart: `a` is zero.
        let linear = Shading::Radial {
            space: ColorSpace::DeviceRgb,
            function: spread_ramp(),
            coords: [10.0, 10.0, 2.0, 13.0, 14.0, 7.0],
            extend: (true, true),
        };
        let based = Shading::FunctionBased {
            space: ColorSpace::DeviceGray,
            function: Function::PostScript {
                domain: vec![(0.0, 100.0); 2],
                range: vec![(0.0, 1.0)],
                program: tinker_pdf_color::function::parse_postscript(b"{ add 200 div }"),
            },
            domain: [0.0, 100.0, 0.0, 100.0],
        };
        let pointless = Shading::Axial {
            space: ColorSpace::DeviceRgb,
            function: spread_ramp(),
            coords: [5.0, 5.0, 5.0, 5.0],
            extend: (true, true),
        };
        let broken = Matrix {
            a: f64::NAN,
            b: 0.0,
            c: 0.0,
            d: f64::INFINITY,
            e: 0.0,
            f: 0.0,
        };
        let mut scratch = ShadingScratch::default();
        for (shading, inverse) in [
            (&linear, tilted()),
            (&based, tilted()),
            (&pointless, tilted()),
            (&stepped_shadings()[0], broken),
            (&stepped_shadings()[1], broken),
        ] {
            let mut walk = shading.row(&inverse, 9.5);
            for px in 0..140 {
                assert_eq!(
                    walk.color_at(px, &mut scratch),
                    direct(shading, &inverse, 9.5, px),
                    "{shading:?} at column {px}"
                );
            }
        }
        // One evaluation a pixel where nothing steps.
        let mut walk = based.row(&tilted(), 9.5);
        let _ = count::take();
        for px in 0..140 {
            let _ = walk.color_at(px, &mut scratch);
        }
        assert_eq!(count::take(), 140);
    }

    /// Columns far out in the page frame, and negative ones, restart where
    /// the frame says, and coordinates past any page step without panicking.
    #[test]
    fn columns_far_out_and_before_the_origin_step_as_the_frame_says() {
        assert_eq!(restart_of(0), 0);
        assert_eq!(restart_of(63), 0);
        assert_eq!(restart_of(64), 64);
        assert_eq!(restart_of(-1), -64);
        assert_eq!(restart_of(-64), -64);
        assert_eq!(restart_of(-65), -128);
        let inverse = tilted();
        let mut scratch = ShadingScratch::default();
        for shading in &stepped_shadings() {
            for first in [-200_i64, 1_000_000, i64::from(i32::MAX) - 300] {
                let mut walk = shading.row(&inverse, 7.5);
                for px in first..first + 200 {
                    let stepped = walk.color_at(px, &mut scratch);
                    let clause = direct(shading, &inverse, 7.5, px);
                    assert_eq!(stepped.is_some(), clause.is_some(), "column {px}");
                }
            }
        }
        let huge = Shading::Radial {
            space: ColorSpace::DeviceRgb,
            function: spread_ramp(),
            coords: [1e300, -1e300, 1e300, 0.0, 0.0, 1.0],
            extend: (true, true),
        };
        let mut walk = huge.row(&inverse, 7.5);
        for px in 0..130 {
            let _ = walk.color_at(px, &mut scratch);
        }
    }

    /// The functions a table stands in for, each with the space it converts
    /// in: exponentials straight, curved and steep at their foot, a stitch
    /// that joins and one that jumps, a one-input sampled table, an array of
    /// one per channel, and a tint transform after the function.
    fn tabled_functions() -> Vec<(ColorSpace, Function)> {
        let gamma = |c0: Vec<f64>, c1: Vec<f64>, n: f64| Function::Exponential {
            domain: (0.0, 1.0),
            c0,
            c1,
            n,
        };
        let stitch = |second_start: Vec<f64>| Function::Stitching {
            domain: (0.0, 1.0),
            functions: vec![
                gamma(vec![0.0, 0.1, 0.9], vec![0.6, 0.5, 0.3], 1.0),
                gamma(second_start, vec![1.0, 0.2, 0.0], 1.0),
            ],
            bounds: vec![0.37],
            encode: vec![(0.0, 1.0), (0.0, 1.0)],
        };
        vec![
            (ColorSpace::DeviceRgb, spread_ramp()),
            (
                ColorSpace::DeviceRgb,
                gamma(vec![0.0, 0.2, 0.9], vec![1.0, 0.7, 0.1], 2.2),
            ),
            (
                ColorSpace::DeviceRgb,
                gamma(vec![0.0, 0.0, 0.0], vec![1.0, 1.0, 1.0], 0.45),
            ),
            (ColorSpace::DeviceRgb, stitch(vec![0.6, 0.5, 0.3])),
            (ColorSpace::DeviceRgb, stitch(vec![0.0, 0.9, 1.0])),
            (
                ColorSpace::DeviceGray,
                Function::Sampled {
                    domain: vec![(0.0, 1.0)],
                    range: vec![(0.0, 1.0)],
                    size: vec![5],
                    bits: 8,
                    encode: vec![],
                    decode: vec![],
                    samples: vec![0, 200, 40, 255, 128],
                },
            ),
            (
                ColorSpace::DeviceRgb,
                Function::Array(vec![
                    gamma(vec![0.0], vec![1.0], 1.0),
                    gamma(vec![1.0], vec![0.0], 3.0),
                    gamma(vec![0.3], vec![0.8], 0.7),
                ]),
            ),
            (
                ColorSpace::Separation {
                    components: 1,
                    alternate: Box::new(ColorSpace::DeviceCmyk),
                    tint: Box::new(gamma(vec![0.0; 4], vec![0.1, 0.7, 0.2, 0.05], 1.6)),
                    colorants: tinker_pdf_color::Colorants::default(),
                },
                gamma(vec![0.0], vec![1.0], 1.0),
            ),
        ]
    }

    /// **The table agrees with the clause at and between its entries**
    /// (ruling 22): at an entry a lookup is the function's colour exactly,
    /// between entries it is within one level of 255 in every channel, and
    /// where it is not vouched for it says so and the colour is the
    /// function's, evaluated.
    #[test]
    fn a_table_agrees_with_the_clause_at_and_between_its_entries() {
        let last = (TABLE_ENTRIES - 1) as f64;
        for (space, function) in tabled_functions() {
            let mut scratch = ShadingScratch::default();
            scratch.tabulate(&space, &function, 0.0, 1.0, u64::MAX);
            let table = scratch.table.clone().expect("the function is tabulable");
            let mut fresh = ShadingScratch::default();
            for entry in 0..TABLE_ENTRIES {
                let t = entry as f64 / last;
                assert_eq!(
                    table
                        .color(t)
                        .unwrap_or_else(|| fresh.color(&space, &function, &[t])),
                    fresh.color(&space, &function, &[t]),
                    "{function:?} at entry {entry}"
                );
            }
            for step in 0..=20_000u32 {
                let t = f64::from(step) / 20_000.0;
                let clause = fresh.color(&space, &function, &[t]);
                let read = scratch.tabled(&space, &function, t);
                assert!(
                    clause.0.abs_diff(read.0) <= 1
                        && clause.1.abs_diff(read.1) <= 1
                        && clause.2.abs_diff(read.2) <= 1,
                    "{function:?} in {space:?} at {t}: the table read {read:?}, the clause \
                     {clause:?}"
                );
            }
        }
    }

    /// The table is the function evaluated once an entry: building it costs
    /// [`TABLE_ENTRIES`] evaluations, and reading any number of colours from
    /// a function it vouches for everywhere costs none more.
    #[test]
    fn a_table_evaluates_its_function_once_an_entry() {
        let mut scratch = ShadingScratch::default();
        scratch.tabulate(&ColorSpace::DeviceRgb, &spread_ramp(), 0.0, 1.0, 1_000_000);
        assert_eq!(scratch.evaluations(), TABLE_ENTRIES as u64);
        for step in 0..100_000u32 {
            let t = f64::from(step) / 99_999.0;
            let _ = scratch.tabled(&ColorSpace::DeviceRgb, &spread_ramp(), t);
        }
        assert_eq!(scratch.evaluations(), TABLE_ENTRIES as u64);
    }

    /// No table where none should stand in: a type 4 program, a sweep asking
    /// no more colours than a table holds, and a range with no width; and a
    /// lookup outside the range reads its end, and one of no number declines.
    #[test]
    fn a_table_stands_in_only_where_it_should() {
        let calculator = Function::PostScript {
            domain: vec![(0.0, 1.0)],
            range: vec![(0.0, 1.0); 3],
            program: tinker_pdf_color::function::parse_postscript(b"{ dup dup }"),
        };
        let mut scratch = ShadingScratch::default();
        scratch.tabulate(&ColorSpace::DeviceRgb, &calculator, 0.0, 1.0, u64::MAX);
        assert!(scratch.table.is_none(), "a type 4 program");
        scratch.tabulate(
            &ColorSpace::DeviceRgb,
            &spread_ramp(),
            0.0,
            1.0,
            TABLE_ENTRIES as u64,
        );
        assert!(scratch.table.is_none(), "a sweep no larger than the table");
        scratch.tabulate(&ColorSpace::DeviceRgb, &spread_ramp(), 0.5, 0.5, u64::MAX);
        assert!(scratch.table.is_none(), "a range of no width");
        scratch.tabulate(
            &ColorSpace::DeviceRgb,
            &spread_ramp(),
            f64::NAN,
            1.0,
            u64::MAX,
        );
        assert!(scratch.table.is_none(), "a range of no number");
        assert_eq!(
            scratch.evaluations(),
            0,
            "and nothing evaluated for any of them"
        );

        scratch.tabulate(&ColorSpace::DeviceRgb, &spread_ramp(), 0.25, 0.75, u64::MAX);
        let table = scratch.table.clone().expect("a table");
        let mut fresh = ShadingScratch::default();
        assert_eq!(
            table.color(0.0),
            Some(fresh.color(&ColorSpace::DeviceRgb, &spread_ramp(), &[0.25]))
        );
        assert_eq!(
            table.color(2.0),
            Some(fresh.color(&ColorSpace::DeviceRgb, &spread_ramp(), &[0.75]))
        );
        assert_eq!(table.color(f64::NAN), None);
        assert_eq!(table.color(f64::INFINITY), table.color(0.75));
    }

    /// A row read through a table paints within a level of the clause, as a
    /// stepped row does without one, and a tile reads the page's colours.
    #[test]
    fn a_row_read_through_a_table_paints_within_a_level_of_the_clause() {
        let inverse = tilted();
        for shading in &stepped_shadings() {
            let (space, function) = shading.parametric().expect("axial or radial");
            let mut page = ShadingScratch::default();
            page.tabulate(space, function, 0.0, 1.0, u64::MAX);
            let mut tile = page.clone();
            for row in [3.5, 40.5, 97.5] {
                let mut walk = shading.row(&inverse, row);
                let mut part = shading.row(&inverse, row);
                for px in 0..256 {
                    let read = walk.color_at(px, &mut page);
                    if px >= 77 {
                        assert_eq!(part.color_at(px, &mut tile), read, "column {px}");
                    }
                    let clause = direct(shading, &inverse, row, px);
                    assert_eq!(read.is_some(), clause.is_some(), "column {px}");
                    if let (Some(read), Some(clause)) = (read, clause) {
                        assert!(
                            read.0.abs_diff(clause.0) <= 1
                                && read.1.abs_diff(clause.1) <= 1
                                && read.2.abs_diff(clause.2) <= 1,
                            "{shading:?} at ({px}, {row}): {read:?} against {clause:?}"
                        );
                    }
                }
            }
        }
    }
}
