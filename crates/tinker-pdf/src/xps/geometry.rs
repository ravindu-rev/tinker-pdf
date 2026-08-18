//! ECMA-388 11.2's path geometry, in both of the spellings a real package uses
//! (gap 30, milestone 6).
//!
//! # Two grammars for one value, and the corpus carries both
//!
//! 11.2.3's **abbreviated** syntax is a string: `M0,0L200,0 200,200 0,200Z`.
//! 11.2.1's **element** syntax is a `PathGeometry` holding `PathFigure`s
//! holding `PolyLineSegment`s. They are the same value and this module has one
//! type for it, so [`abbreviated`] and [`from_geometry_node`] produce the
//! **identical** [`Geometry`] for the same shape — which is an assertion in the
//! tests rather than a claim here, because two parsers for one grammar is
//! exactly the shape gap 29's `/Mask` finding came in: two paths, two pictures,
//! and nothing comparing them.
//!
//! Milestone 1's corpus carries two spellings of the abbreviated form as well,
//! from two Microsoft producers that do not agree with each other:
//! `M0,0L200,0 200,200 0,200Z` from WPF and `M 0,0 L 200,0 200,200 0,200 Z`
//! from the XPS object model. A reader that took either alone refuses half the
//! corpus.
//!
//! # Absolute and relative are two grammars, not one
//!
//! `L` and `l` differ in one bit of ASCII and in everything else: one names a
//! point and the other names a displacement from wherever the pen is. Three
//! consequences the tests hold apart, because each can be right while the
//! others are wrong:
//!
//! - a relative **first** `Move` displaces from `0,0`, which is the only
//!   position a figure can start from before one is stated;
//! - `Z` sets the current point to the **figure's own first point**, so a
//!   relative command after a close displaces from there and not from the last
//!   point drawn;
//! - the omitted-repeat form (`L 100,200 300,400`) re-applies the command, and
//!   under a relative command each repeat displaces from the point the
//!   previous repeat reached.
//!
//! # What is refused, and what it costs
//!
//! A `Data` that will not parse is [`GeometryError::Syntax`], and gap 30's
//! element-level rule makes that a shape **not painted** with a named warning:
//! a missing shape is visible as a missing shape, where a shape drawn from
//! half a parse is a picture that looks like the producer's fault.
//! [`GeometryError::Exhausted`] is a different answer entirely — one of the
//! document's work totals is spent — and it refuses the package.

use super::markup::{self, Budget, Node, Trouble};

/// One drawing step, in the four shapes PDF 8.5.2 has operators for.
///
/// Quadratic curves and elliptical arcs are converted here rather than
/// carried: PDF has `v`, `y` and `c` and no quadratic or arc operator at all,
/// so a representation that kept them would have to convert somewhere else,
/// and "somewhere else" is how two spellings of one shape appear.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Segment {
    /// `x y m`: start a figure.
    Move([f64; 2]),
    /// `x y l`.
    Line([f64; 2]),
    /// `x1 y1 x2 y2 x3 y3 c`.
    Curve([f64; 6]),
    /// `h`.
    Close,
}

impl Segment {
    /// Every point the segment names, control points included.
    fn points(&self) -> &[f64] {
        match self {
            Segment::Move(p) | Segment::Line(p) => p,
            Segment::Curve(p) => p,
            Segment::Close => &[],
        }
    }
}

/// 11.2.1's `FillRule`.
///
/// **`EvenOdd` is the default**, which is the opposite of PDF's `f` operator
/// and of most drawing APIs — so a reader that defaulted to the one it was
/// used to would fill every self-intersecting shape solid and every other
/// shape correctly.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FillRule {
    /// `F0`, `FillRule="EvenOdd"`: PDF's `f*`.
    #[default]
    EvenOdd,
    /// `F1`, `FillRule="Nonzero"`: PDF's `f`.
    NonZero,
}

impl FillRule {
    /// The PDF painting operator suffix: `*` for even-odd, nothing for
    /// non-zero.
    fn suffix(self) -> &'static str {
        match self {
            FillRule::EvenOdd => "*",
            FillRule::NonZero => "",
        }
    }

    fn from_attribute(text: &str) -> Option<FillRule> {
        // Both spellings appear in the wild; ECMA-388 writes `Nonzero` and
        // XAML writes `NonZero`, and neither is ambiguous.
        match text.trim() {
            "EvenOdd" => Some(FillRule::EvenOdd),
            "Nonzero" | "NonZero" => Some(FillRule::NonZero),
            _ => None,
        }
    }
}

/// One figure: a `PathFigure`, or everything an abbreviated `M` opened.
#[derive(Clone, Debug, PartialEq)]
pub struct Figure {
    /// 11.2.2's `IsFilled`, which defaults to true.
    ///
    /// A figure that is not filled still bounds nothing and still strokes, so
    /// it is kept in the segment list and skipped when the fill is emitted.
    /// The abbreviated syntax has no spelling for it, so every figure it
    /// produces is filled.
    pub filled: bool,
    /// The steps, beginning with a [`Segment::Move`].
    pub segments: Vec<Segment>,
}

/// A whole `PathGeometry`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Geometry {
    /// 11.2.1's `FillRule`.
    pub fill_rule: FillRule,
    /// The figures, in document order.
    pub figures: Vec<Figure>,
}

/// Why a geometry did not read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GeometryError {
    /// The markup is not 11.2's geometry. **The element is not painted**, and
    /// warns.
    Syntax,
    /// A document work total is spent. **The package is refused.**
    Exhausted,
}

impl From<markup::Exhausted> for GeometryError {
    fn from(_: markup::Exhausted) -> GeometryError {
        GeometryError::Exhausted
    }
}

impl From<Trouble> for GeometryError {
    fn from(trouble: Trouble) -> GeometryError {
        match trouble {
            Trouble::Markup => GeometryError::Syntax,
            Trouble::Exhausted => GeometryError::Exhausted,
        }
    }
}

impl Geometry {
    /// Every segment of every figure, in order.
    pub fn segments(&self) -> impl Iterator<Item = &Segment> {
        self.figures
            .iter()
            .flat_map(|figure| figure.segments.iter())
    }

    /// How many segments this geometry holds, which is what
    /// [`super::MAX_XPS_SEGMENTS`] counts.
    #[must_use]
    pub fn len(&self) -> usize {
        self.figures.iter().map(|f| f.segments.len()).sum()
    }

    /// Whether there is nothing to draw.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.figures.iter().all(|f| f.segments.is_empty())
    }

    /// The bounding box, as `[x0 y0 x1 y1]`, over every point **including
    /// control points**.
    ///
    /// Conservative on purpose. A curve lies inside its control hull, so a box
    /// computed this way contains the shape and may be larger than it; the two
    /// things it is used for — a form XObject's `/BBox`, and deciding whether
    /// two canvas children overlap — are each safe in that direction and
    /// unsafe in the other. A `/BBox` that is too small clips the drawing
    /// away, and an overlap test that says "no" when the answer is "yes"
    /// composites an opacity the wrong way round.
    #[must_use]
    pub fn bbox(&self) -> Option<[f64; 4]> {
        let mut box_: Option<[f64; 4]> = None;
        for segment in self.segments() {
            for point in segment.points().chunks_exact(2) {
                let (x, y) = (point[0], point[1]);
                box_ = Some(match box_ {
                    None => [x, y, x, y],
                    Some([x0, y0, x1, y1]) => [x0.min(x), y0.min(y), x1.max(x), y1.max(y)],
                });
            }
        }
        box_
    }

    /// The same geometry through a matrix, or `None` when a point leaves what
    /// [`markup::MAX_COORDINATE`] admits.
    #[must_use]
    pub fn transformed(&self, matrix: [f64; 6]) -> Option<Geometry> {
        let mut out = Geometry {
            fill_rule: self.fill_rule,
            figures: Vec::with_capacity(self.figures.len()),
        };
        for figure in &self.figures {
            let mut moved = Figure {
                filled: figure.filled,
                segments: Vec::with_capacity(figure.segments.len()),
            };
            for segment in &figure.segments {
                moved.segments.push(match segment {
                    Segment::Move(p) => Segment::Move(point_through(matrix, *p)?),
                    Segment::Line(p) => Segment::Line(point_through(matrix, *p)?),
                    Segment::Curve(p) => {
                        let a = point_through(matrix, [p[0], p[1]])?;
                        let b = point_through(matrix, [p[2], p[3]])?;
                        let c = point_through(matrix, [p[4], p[5]])?;
                        Segment::Curve([a[0], a[1], b[0], b[1], c[0], c[1]])
                    }
                    Segment::Close => Segment::Close,
                });
            }
            out.figures.push(moved);
        }
        Some(out)
    }

    /// Writes the path-construction operators.
    ///
    /// `filled_only` skips the figures 11.2.2's `IsFilled` excludes, which is
    /// what a fill and a clip want and what a stroke does not.
    pub fn emit(&self, out: &mut Vec<u8>, filled_only: bool) {
        for figure in &self.figures {
            if filled_only && !figure.filled {
                continue;
            }
            for segment in &figure.segments {
                match segment {
                    Segment::Move(p) => markup::op(out, p, "m"),
                    Segment::Line(p) => markup::op(out, p, "l"),
                    Segment::Curve(p) => markup::op(out, p, "c"),
                    Segment::Close => out.extend_from_slice(b"h\n"),
                }
            }
        }
    }

    /// The `f`/`f*` operator this geometry's fill rule asks for.
    #[must_use]
    pub fn fill_operator(&self) -> String {
        format!("f{}", self.fill_rule.suffix())
    }

    /// The `W n`/`W* n` pair this geometry's fill rule asks for, as a clip.
    #[must_use]
    pub fn clip_operator(&self) -> String {
        format!("W{} n", self.fill_rule.suffix())
    }
}

fn point_through(matrix: [f64; 6], point: [f64; 2]) -> Option<[f64; 2]> {
    let (x, y) = markup::apply(matrix, (point[0], point[1]));
    (markup::usable(x) && markup::usable(y)).then_some([x, y])
}

// ---- 11.2.3, the abbreviated syntax -----------------------------------------

/// Reads 11.2.3's abbreviated geometry.
///
/// # Errors
/// [`GeometryError::Syntax`] for anything that is not the grammar,
/// [`GeometryError::Exhausted`] when the document's segment total is spent.
pub fn abbreviated(text: &str, budget: &mut Budget) -> Result<Geometry, GeometryError> {
    let mut scan = Scan {
        bytes: text.as_bytes(),
        at: 0,
    };
    let mut out = Geometry::default();
    let mut pen = Pen::default();

    scan.wsp();
    // `F0` and `F1`, and only at the front — 11.2.3 puts the fill rule before
    // the first command and nowhere else.
    if matches!(scan.peek(), Some(b'F')) {
        scan.at += 1;
        scan.wsp();
        out.fill_rule = match scan.take() {
            Some(b'0') => FillRule::EvenOdd,
            Some(b'1') => FillRule::NonZero,
            _ => return Err(GeometryError::Syntax),
        };
    }

    loop {
        scan.wsp();
        let Some(command) = scan.take() else { break };
        let relative = command.is_ascii_lowercase();
        match command.to_ascii_uppercase() {
            b'M' => {
                let point = scan.point().ok_or(GeometryError::Syntax)?;
                pen.moveto(&mut out, relative, point, budget)?;
                // A repeated point after a `Move` is 11.2.3's omitted-repeat
                // form applied to the one command whose repeat means something
                // else: the second point is a line, not a second figure. The
                // grammar's `move_command` takes one point, so this is
                // leniency and is written down as such — there is no second
                // reading of the bytes, and refusing them would lose a shape
                // no consumer disagrees about.
                while let Some(point) = scan.point() {
                    pen.lineto(&mut out, relative, point, budget)?;
                }
            }
            b'L' => {
                let mut any = false;
                while let Some(point) = scan.point() {
                    pen.lineto(&mut out, relative, point, budget)?;
                    any = true;
                }
                if !any {
                    return Err(GeometryError::Syntax);
                }
            }
            b'H' | b'V' => {
                let horizontal = command.eq_ignore_ascii_case(&b'H');
                let mut any = false;
                while let Some(value) = scan.number() {
                    let point = match (horizontal, relative) {
                        (true, true) => (value, 0.0),
                        (true, false) => (value, pen.at.1),
                        (false, true) => (0.0, value),
                        (false, false) => (pen.at.0, value),
                    };
                    pen.lineto(&mut out, relative, point, budget)?;
                    any = true;
                }
                if !any {
                    return Err(GeometryError::Syntax);
                }
            }
            b'C' => {
                let mut any = false;
                while let Some(one) = scan.point() {
                    let two = scan.point().ok_or(GeometryError::Syntax)?;
                    let three = scan.point().ok_or(GeometryError::Syntax)?;
                    pen.cubic(&mut out, relative, one, two, three, budget)?;
                    any = true;
                }
                if !any {
                    return Err(GeometryError::Syntax);
                }
            }
            b'Q' => {
                let mut any = false;
                while let Some(control) = scan.point() {
                    let end = scan.point().ok_or(GeometryError::Syntax)?;
                    pen.quadratic(&mut out, relative, control, end, budget)?;
                    any = true;
                }
                if !any {
                    return Err(GeometryError::Syntax);
                }
            }
            b'S' => {
                let mut any = false;
                while let Some(control) = scan.point() {
                    let end = scan.point().ok_or(GeometryError::Syntax)?;
                    pen.smooth(&mut out, relative, control, end, budget)?;
                    any = true;
                }
                if !any {
                    return Err(GeometryError::Syntax);
                }
            }
            b'A' => {
                let mut any = false;
                while let Some(radii) = scan.point() {
                    let rotation = scan.number().ok_or(GeometryError::Syntax)?;
                    let large = scan.flag().ok_or(GeometryError::Syntax)?;
                    let sweep = scan.flag().ok_or(GeometryError::Syntax)?;
                    let end = scan.point().ok_or(GeometryError::Syntax)?;
                    pen.arc(
                        &mut out, relative, radii, rotation, large, sweep, end, budget,
                    )?;
                    any = true;
                }
                if !any {
                    return Err(GeometryError::Syntax);
                }
            }
            b'Z' => pen.close(&mut out, budget)?,
            _ => return Err(GeometryError::Syntax),
        }
    }

    scan.wsp();
    if scan.at != scan.bytes.len() {
        return Err(GeometryError::Syntax);
    }
    Ok(out)
}

/// Where the pen is, which is the whole of the relative grammar.
#[derive(Clone, Copy)]
struct Pen {
    /// The current point.
    at: (f64, f64),
    /// The current figure's first point, which is where `Z` returns to.
    start: (f64, f64),
    /// Whether a figure is open. A drawing command with none open opens one at
    /// the current point, which is `0,0` before any `Move` and the closed
    /// figure's own start after a `Z`.
    open: bool,
    /// The second control point of the last cubic, for `S`'s reflection.
    /// `None` when the last command was not a cubic, which 11.2.3 makes the
    /// current point.
    last_control: Option<(f64, f64)>,
}

impl Default for Pen {
    fn default() -> Pen {
        Pen {
            at: (0.0, 0.0),
            start: (0.0, 0.0),
            open: false,
            last_control: None,
        }
    }
}

impl Pen {
    /// Resolves a point against the pen, per the command's case.
    fn resolve(&self, relative: bool, point: (f64, f64)) -> Option<(f64, f64)> {
        let out = if relative {
            (self.at.0 + point.0, self.at.1 + point.1)
        } else {
            point
        };
        (markup::usable(out.0) && markup::usable(out.1)).then_some(out)
    }

    fn push(
        &mut self,
        out: &mut Geometry,
        segment: Segment,
        budget: &mut Budget,
    ) -> Result<(), GeometryError> {
        budget.segments(1)?;
        match out.figures.last_mut() {
            Some(figure) => figure.segments.push(segment),
            None => out.figures.push(Figure {
                filled: true,
                segments: vec![segment],
            }),
        }
        Ok(())
    }

    /// Opens a figure at the current point when a drawing command arrives with
    /// none open, which is 11.2.3's `move_command?` being optional and `Z`
    /// having ended the last one.
    fn ensure_open(
        &mut self,
        out: &mut Geometry,
        budget: &mut Budget,
    ) -> Result<(), GeometryError> {
        if self.open {
            return Ok(());
        }
        budget.segments(1)?;
        out.figures.push(Figure {
            filled: true,
            segments: vec![Segment::Move([self.at.0, self.at.1])],
        });
        self.start = self.at;
        self.open = true;
        Ok(())
    }

    fn moveto(
        &mut self,
        out: &mut Geometry,
        relative: bool,
        point: (f64, f64),
        budget: &mut Budget,
    ) -> Result<(), GeometryError> {
        let to = self.resolve(relative, point).ok_or(GeometryError::Syntax)?;
        budget.segments(1)?;
        out.figures.push(Figure {
            filled: true,
            segments: vec![Segment::Move([to.0, to.1])],
        });
        self.at = to;
        self.start = to;
        self.open = true;
        self.last_control = None;
        Ok(())
    }

    fn lineto(
        &mut self,
        out: &mut Geometry,
        relative: bool,
        point: (f64, f64),
        budget: &mut Budget,
    ) -> Result<(), GeometryError> {
        self.ensure_open(out, budget)?;
        let to = self.resolve(relative, point).ok_or(GeometryError::Syntax)?;
        self.push(out, Segment::Line([to.0, to.1]), budget)?;
        self.at = to;
        self.last_control = None;
        Ok(())
    }

    fn cubic(
        &mut self,
        out: &mut Geometry,
        relative: bool,
        one: (f64, f64),
        two: (f64, f64),
        three: (f64, f64),
        budget: &mut Budget,
    ) -> Result<(), GeometryError> {
        self.ensure_open(out, budget)?;
        let c1 = self.resolve(relative, one).ok_or(GeometryError::Syntax)?;
        let c2 = self.resolve(relative, two).ok_or(GeometryError::Syntax)?;
        let end = self.resolve(relative, three).ok_or(GeometryError::Syntax)?;
        self.push(
            out,
            Segment::Curve([c1.0, c1.1, c2.0, c2.1, end.0, end.1]),
            budget,
        )?;
        self.at = end;
        self.last_control = Some(c2);
        Ok(())
    }

    fn quadratic(
        &mut self,
        out: &mut Geometry,
        relative: bool,
        control: (f64, f64),
        end: (f64, f64),
        budget: &mut Budget,
    ) -> Result<(), GeometryError> {
        self.ensure_open(out, budget)?;
        let q = self
            .resolve(relative, control)
            .ok_or(GeometryError::Syntax)?;
        let to = self.resolve(relative, end).ok_or(GeometryError::Syntax)?;
        // 11.2.3 has a quadratic and PDF 8.5.2 has none, so it is raised to
        // the cubic that draws the identical curve: the two control points sit
        // two thirds of the way from each end point to the quadratic's own.
        let from = self.at;
        let c1 = (
            from.0 + 2.0 / 3.0 * (q.0 - from.0),
            from.1 + 2.0 / 3.0 * (q.1 - from.1),
        );
        let c2 = (
            to.0 + 2.0 / 3.0 * (q.0 - to.0),
            to.1 + 2.0 / 3.0 * (q.1 - to.1),
        );
        self.push(
            out,
            Segment::Curve([c1.0, c1.1, c2.0, c2.1, to.0, to.1]),
            budget,
        )?;
        self.at = to;
        // A quadratic is not a cubic for `S`'s purposes: 11.2.3 reflects the
        // last **cubic**'s control point and treats everything else as having
        // none.
        self.last_control = None;
        Ok(())
    }

    fn smooth(
        &mut self,
        out: &mut Geometry,
        relative: bool,
        control: (f64, f64),
        end: (f64, f64),
        budget: &mut Budget,
    ) -> Result<(), GeometryError> {
        self.ensure_open(out, budget)?;
        let from = self.at;
        let c1 = match self.last_control {
            Some((x, y)) => (2.0 * from.0 - x, 2.0 * from.1 - y),
            None => from,
        };
        let c2 = self
            .resolve(relative, control)
            .ok_or(GeometryError::Syntax)?;
        let to = self.resolve(relative, end).ok_or(GeometryError::Syntax)?;
        if !markup::usable(c1.0) || !markup::usable(c1.1) {
            return Err(GeometryError::Syntax);
        }
        self.push(
            out,
            Segment::Curve([c1.0, c1.1, c2.0, c2.1, to.0, to.1]),
            budget,
        )?;
        self.at = to;
        self.last_control = Some(c2);
        Ok(())
    }

    #[allow(clippy::too_many_arguments, reason = "11.2.3's arc takes seven")]
    fn arc(
        &mut self,
        out: &mut Geometry,
        relative: bool,
        radii: (f64, f64),
        rotation: f64,
        large: bool,
        sweep: bool,
        end: (f64, f64),
        budget: &mut Budget,
    ) -> Result<(), GeometryError> {
        self.ensure_open(out, budget)?;
        let to = self.resolve(relative, end).ok_or(GeometryError::Syntax)?;
        for segment in
            arc_to(self.at, radii, rotation, large, sweep, to).ok_or(GeometryError::Syntax)?
        {
            self.push(out, segment, budget)?;
        }
        self.at = to;
        self.last_control = None;
        Ok(())
    }

    fn close(&mut self, out: &mut Geometry, budget: &mut Budget) -> Result<(), GeometryError> {
        // A `Z` with nothing open closes nothing. The alternative — pushing a
        // lone `h` — is a PDF content stream with a close operator and no
        // subpath, which 8.5.2 does not define.
        if !self.open {
            return Ok(());
        }
        self.push(out, Segment::Close, budget)?;
        self.at = self.start;
        self.open = false;
        self.last_control = None;
        Ok(())
    }
}

/// 11.2.3's elliptical arc as cubic curves.
///
/// The endpoint parameterisation the markup uses — two radii, a rotation, two
/// flags and an end point — is turned into the centre parameterisation and
/// then into at most four cubics of ninety degrees each. `None` is a
/// degenerate arc that is not a line either.
///
/// The sweep flag is `1` for **clockwise**, and clockwise in XPS means
/// clockwise on the page: 18.1 puts the y axis pointing down, and this reader
/// keeps the markup's own numbers and flips once in the page's own `cm`, so
/// the direction never has to be negated here.
#[must_use]
fn arc_to(
    from: (f64, f64),
    radii: (f64, f64),
    rotation: f64,
    large: bool,
    sweep: bool,
    to: (f64, f64),
) -> Option<Vec<Segment>> {
    if !markup::usable(rotation) {
        return None;
    }
    // 11.2.3 and SVG F.6.2 agree: an arc to where the pen already is draws
    // nothing at all, rather than a full ellipse.
    if from == to {
        return Some(Vec::new());
    }
    let (mut rx, mut ry) = (radii.0.abs(), radii.1.abs());
    if !markup::usable(rx) || !markup::usable(ry) {
        return None;
    }
    // A zero radius makes the ellipse a line, which is what F.6.2 says to draw
    // rather than an error.
    if rx == 0.0 || ry == 0.0 {
        return Some(vec![Segment::Line([to.0, to.1])]);
    }

    let phi = rotation.to_radians();
    let (sin_p, cos_p) = phi.sin_cos();
    let dx = (from.0 - to.0) / 2.0;
    let dy = (from.1 - to.1) / 2.0;
    let x1 = cos_p * dx + sin_p * dy;
    let y1 = -sin_p * dx + cos_p * dy;

    // F.6.6: radii too small to reach both endpoints are scaled up rather than
    // refused, because the endpoints are what the file stated and the radii
    // are a hint about the curve between them.
    let lambda = (x1 * x1) / (rx * rx) + (y1 * y1) / (ry * ry);
    if lambda > 1.0 {
        let scale = lambda.sqrt();
        rx *= scale;
        ry *= scale;
    }

    let denominator = rx * rx * y1 * y1 + ry * ry * x1 * x1;
    if denominator == 0.0 {
        return None;
    }
    let numerator = (rx * rx * ry * ry - denominator).max(0.0);
    let mut factor = (numerator / denominator).sqrt();
    if large == sweep {
        factor = -factor;
    }
    let cx1 = factor * rx * y1 / ry;
    let cy1 = -factor * ry * x1 / rx;
    let cx = cos_p * cx1 - sin_p * cy1 + (from.0 + to.0) / 2.0;
    let cy = sin_p * cx1 + cos_p * cy1 + (from.1 + to.1) / 2.0;

    let start = ((x1 - cx1) / rx, (y1 - cy1) / ry);
    let finish = ((-x1 - cx1) / rx, (-y1 - cy1) / ry);
    let theta = angle((1.0, 0.0), start);
    let mut delta = angle(start, finish);
    if !sweep && delta > 0.0 {
        delta -= std::f64::consts::TAU;
    } else if sweep && delta < 0.0 {
        delta += std::f64::consts::TAU;
    }
    if !theta.is_finite() || !delta.is_finite() {
        return None;
    }

    let steps = (delta.abs() / std::f64::consts::FRAC_PI_2).ceil().max(1.0);
    let steps = steps.min(4.0) as usize;
    let step = delta / steps as f64;
    let alpha = 4.0 / 3.0 * (step / 4.0).tan();
    let mut out = Vec::with_capacity(steps);
    for index in 0..steps {
        let t1 = theta + step * index as f64;
        let t2 = t1 + step;
        let p1 = on_ellipse(cx, cy, rx, ry, sin_p, cos_p, t1);
        let p2 = on_ellipse(cx, cy, rx, ry, sin_p, cos_p, t2);
        let d1 = tangent(rx, ry, sin_p, cos_p, t1);
        let d2 = tangent(rx, ry, sin_p, cos_p, t2);
        let c1 = (p1.0 + alpha * d1.0, p1.1 + alpha * d1.1);
        let c2 = (p2.0 - alpha * d2.0, p2.1 - alpha * d2.1);
        for value in [c1.0, c1.1, c2.0, c2.1, p2.0, p2.1] {
            if !markup::usable(value) {
                return None;
            }
        }
        // The last step lands on the point the file stated rather than on the
        // one the trigonometry reached, so an arc followed by a `Z` closes
        // exactly.
        let end = if index + 1 == steps { to } else { p2 };
        out.push(Segment::Curve([c1.0, c1.1, c2.0, c2.1, end.0, end.1]));
    }
    Some(out)
}

fn on_ellipse(cx: f64, cy: f64, rx: f64, ry: f64, sin_p: f64, cos_p: f64, t: f64) -> (f64, f64) {
    let (sin_t, cos_t) = t.sin_cos();
    (
        cx + rx * cos_p * cos_t - ry * sin_p * sin_t,
        cy + rx * sin_p * cos_t + ry * cos_p * sin_t,
    )
}

fn tangent(rx: f64, ry: f64, sin_p: f64, cos_p: f64, t: f64) -> (f64, f64) {
    let (sin_t, cos_t) = t.sin_cos();
    (
        -rx * cos_p * sin_t - ry * sin_p * cos_t,
        -rx * sin_p * sin_t + ry * cos_p * cos_t,
    )
}

fn angle(u: (f64, f64), v: (f64, f64)) -> f64 {
    let dot = u.0 * v.0 + u.1 * v.1;
    let magnitude = (u.0 * u.0 + u.1 * u.1).sqrt() * (v.0 * v.0 + v.1 * v.1).sqrt();
    if magnitude == 0.0 {
        return 0.0;
    }
    let sign = if u.0 * v.1 - u.1 * v.0 < 0.0 {
        -1.0
    } else {
        1.0
    };
    sign * (dot / magnitude).clamp(-1.0, 1.0).acos()
}

/// A byte scanner over the abbreviated grammar.
///
/// Bytes rather than characters: every terminal in 11.2.3 is US-ASCII, and a
/// multi-byte character can only ever be a syntax error, which it becomes
/// because no arm matches it.
struct Scan<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl Scan<'_> {
    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.at).copied()
    }

    fn take(&mut self) -> Option<u8> {
        let byte = self.peek()?;
        self.at += 1;
        Some(byte)
    }

    /// 11.2.3's `wsp`: space, tab, carriage return, line feed.
    fn wsp(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\r' | b'\n')) {
            self.at += 1;
        }
    }

    /// `comma_wsp`: whitespace with at most one comma in it.
    fn comma_wsp(&mut self) {
        self.wsp();
        if self.peek() == Some(b',') {
            self.at += 1;
            self.wsp();
        }
    }

    fn digits(&mut self) -> usize {
        let start = self.at;
        while self.peek().is_some_and(|b| b.is_ascii_digit()) {
            self.at += 1;
        }
        self.at - start
    }

    /// One number, or nothing consumed.
    fn number(&mut self) -> Option<f64> {
        self.comma_wsp();
        let start = self.at;
        if matches!(self.peek(), Some(b'+' | b'-')) {
            self.at += 1;
        }
        let whole = self.digits();
        let mut fraction = 0;
        if self.peek() == Some(b'.') {
            self.at += 1;
            fraction = self.digits();
        }
        if whole == 0 && fraction == 0 {
            self.at = start;
            return None;
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            let mark = self.at;
            self.at += 1;
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.at += 1;
            }
            if self.digits() == 0 {
                self.at = mark;
            }
        }
        let text = std::str::from_utf8(self.bytes.get(start..self.at)?).ok()?;
        let value = text.parse::<f64>().ok()?;
        if !markup::usable(value) {
            self.at = start;
            return None;
        }
        Some(value)
    }

    fn point(&mut self) -> Option<(f64, f64)> {
        let mark = self.at;
        let Some(x) = self.number() else {
            self.at = mark;
            return None;
        };
        let Some(y) = self.number() else {
            self.at = mark;
            return None;
        };
        Some((x, y))
    }

    /// `0` or `1`, which is how 11.2.3 spells the arc's two flags.
    fn flag(&mut self) -> Option<bool> {
        self.comma_wsp();
        match self.peek() {
            Some(b'0') => {
                self.at += 1;
                Some(false)
            }
            Some(b'1') => {
                self.at += 1;
                Some(true)
            }
            _ => None,
        }
    }
}

// ---- 11.2.1 and 11.2.2, the element syntax ----------------------------------

/// Reads a `PathGeometry` element and everything under it.
///
/// # Errors
/// As [`abbreviated`].
pub fn from_geometry_node(node: &Node, budget: &mut Budget) -> Result<Geometry, GeometryError> {
    if !node.xps || node.local != "PathGeometry" {
        return Err(GeometryError::Syntax);
    }

    // `Figures` is the abbreviated syntax inside the element syntax — the same
    // string, in an attribute — so the two grammars meet here and produce the
    // same value by construction rather than by two parsers agreeing.
    let mut out = match node.attr("Figures") {
        Some(text) => abbreviated(text, budget)?,
        None => {
            let mut out = Geometry::default();
            for figure in figure_nodes(node) {
                out.figures.push(read_figure(figure, budget)?);
            }
            out
        }
    };

    // The attribute wins over whatever an abbreviated `F` said, because
    // 11.2.1's `FillRule` is a property of the geometry and 11.2.3's `F` is a
    // spelling of the same property inside a value the property contains.
    if let Some(text) = node.attr("FillRule") {
        out.fill_rule = FillRule::from_attribute(text).ok_or(GeometryError::Syntax)?;
    }

    if let Some(matrix) = geometry_transform(node)? {
        out = out.transformed(matrix).ok_or(GeometryError::Syntax)?;
    }
    Ok(out)
}

/// `PathFigure`s, whether written directly under the geometry or inside
/// `PathGeometry.Figures`.
///
/// Both spellings are XAML for the same thing: a content property may be
/// written out or left implicit, and a producer may choose either.
fn figure_nodes(node: &Node) -> Vec<&Node> {
    let mut out = Vec::new();
    for child in &node.children {
        if !child.xps {
            continue;
        }
        if child.local == "PathFigure" {
            out.push(child);
        } else if child.local == "PathGeometry.Figures" {
            out.extend(
                child
                    .children
                    .iter()
                    .filter(|f| f.xps && f.local == "PathFigure"),
            );
        }
    }
    out
}

/// 14.4's transform, in either of its two spellings, from an element that may
/// carry one.
///
/// `Transform="1,0,0,1,10,10"` and
/// `<Owner.Transform><MatrixTransform Matrix="…"/></Owner.Transform>` are the
/// same value, and 14.4.1 gives the six numbers in the order PDF's `cm` takes
/// them.
///
/// # Errors
/// [`GeometryError::Syntax`] when a transform is stated and is not one — which
/// gap 30's design section makes a **refusal of the element** rather than a
/// default to the identity, because the identity draws the right content in
/// the wrong place at the right size and reads as the producer's fault.
pub fn transform_of(node: &Node, owner: &str) -> Result<Option<[f64; 6]>, GeometryError> {
    if let Some(text) = node
        .attr("Transform")
        .or_else(|| node.attr("RenderTransform"))
    {
        return markup::matrix(text).map(Some).ok_or(GeometryError::Syntax);
    }
    for child in &node.children {
        if !child.xps {
            continue;
        }
        let is_owned = child.local == format!("{owner}.Transform")
            || child.local == format!("{owner}.RenderTransform");
        if !is_owned {
            continue;
        }
        let matrix = child
            .child("MatrixTransform")
            .ok_or(GeometryError::Syntax)?
            .attr("Matrix")
            .and_then(markup::matrix)
            .ok_or(GeometryError::Syntax)?;
        return Ok(Some(matrix));
    }
    Ok(None)
}

fn geometry_transform(node: &Node) -> Result<Option<[f64; 6]>, GeometryError> {
    transform_of(node, "PathGeometry")
}

fn read_figure(node: &Node, budget: &mut Budget) -> Result<Figure, GeometryError> {
    let start = node
        .attr("StartPoint")
        .and_then(markup::pair)
        .ok_or(GeometryError::Syntax)?;
    let filled = match node.attr("IsFilled") {
        Some(text) => markup::boolean(text).ok_or(GeometryError::Syntax)?,
        None => true,
    };
    let mut figure = Figure {
        filled,
        segments: Vec::new(),
    };
    budget.segments(1)?;
    figure.segments.push(Segment::Move([start.0, start.1]));
    let mut at = start;
    for child in &node.children {
        if !child.xps {
            continue;
        }
        read_segment(child, &mut figure, &mut at, budget)?;
    }
    let closed = match node.attr("IsClosed") {
        Some(text) => markup::boolean(text).ok_or(GeometryError::Syntax)?,
        None => false,
    };
    if closed {
        budget.segments(1)?;
        figure.segments.push(Segment::Close);
    }
    Ok(figure)
}

fn read_segment(
    node: &Node,
    figure: &mut Figure,
    at: &mut (f64, f64),
    budget: &mut Budget,
) -> Result<(), GeometryError> {
    // 11.2.2's `IsStroked` is read for its syntax and not honoured: this
    // milestone strokes a whole geometry or none of it, and a per-segment flag
    // would need a second segment list. Recorded here rather than in a
    // progress note nobody reads next to the code.
    match node.local.as_str() {
        "PolyLineSegment" => {
            let points = node
                .attr("Points")
                .and_then(markup::points)
                .ok_or(GeometryError::Syntax)?;
            budget.segments(points.len())?;
            for point in points {
                figure.segments.push(Segment::Line([point.0, point.1]));
                *at = point;
            }
        }
        "PolyBezierSegment" => {
            let points = node
                .attr("Points")
                .and_then(markup::points)
                .ok_or(GeometryError::Syntax)?;
            if points.len() % 3 != 0 {
                return Err(GeometryError::Syntax);
            }
            budget.segments(points.len() / 3)?;
            for step in points.chunks_exact(3) {
                figure.segments.push(Segment::Curve([
                    step[0].0, step[0].1, step[1].0, step[1].1, step[2].0, step[2].1,
                ]));
                *at = step[2];
            }
        }
        "PolyQuadraticBezierSegment" => {
            let points = node
                .attr("Points")
                .and_then(markup::points)
                .ok_or(GeometryError::Syntax)?;
            if points.len() % 2 != 0 {
                return Err(GeometryError::Syntax);
            }
            budget.segments(points.len() / 2)?;
            for step in points.chunks_exact(2) {
                let (q, to) = (step[0], step[1]);
                let from = *at;
                let c1 = (
                    from.0 + 2.0 / 3.0 * (q.0 - from.0),
                    from.1 + 2.0 / 3.0 * (q.1 - from.1),
                );
                let c2 = (
                    to.0 + 2.0 / 3.0 * (q.0 - to.0),
                    to.1 + 2.0 / 3.0 * (q.1 - to.1),
                );
                figure
                    .segments
                    .push(Segment::Curve([c1.0, c1.1, c2.0, c2.1, to.0, to.1]));
                *at = to;
            }
        }
        "ArcSegment" => {
            let to = node
                .attr("Point")
                .and_then(markup::pair)
                .ok_or(GeometryError::Syntax)?;
            let size = node
                .attr("Size")
                .and_then(markup::pair)
                .ok_or(GeometryError::Syntax)?;
            let rotation = match node.attr("RotationAngle") {
                Some(text) => markup::number(text).ok_or(GeometryError::Syntax)?,
                None => 0.0,
            };
            let large = match node.attr("IsLargeArc") {
                Some(text) => markup::boolean(text).ok_or(GeometryError::Syntax)?,
                None => false,
            };
            let sweep = match node.attr("SweepDirection") {
                Some(text) => match text.trim() {
                    "Clockwise" => true,
                    "Counterclockwise" => false,
                    _ => return Err(GeometryError::Syntax),
                },
                None => false,
            };
            let segments =
                arc_to(*at, size, rotation, large, sweep, to).ok_or(GeometryError::Syntax)?;
            budget.segments(segments.len())?;
            figure.segments.extend(segments);
            *at = to;
        }
        // A `PathFigure` child that is not a segment is markup this reader
        // does not know, and a geometry with an unread step in the middle of
        // it is the wrong shape rather than a shorter one.
        _ => return Err(GeometryError::Syntax),
    }
    Ok(())
}

/// A geometry from whichever spelling an element used, for `Data` and `Clip`
/// alike.
///
/// `Data="M0,0…"` and `<Path.Data><PathGeometry…/></Path.Data>` are the same
/// property, and `Clip` is the same pair again — which is why both go through
/// one function rather than two that could drift.
///
/// # Errors
/// As [`abbreviated`].
pub fn property(
    node: &Node,
    owner: &str,
    property: &str,
    budget: &mut Budget,
) -> Result<Option<Geometry>, GeometryError> {
    if let Some(text) = node.attr(property) {
        return abbreviated(text, budget).map(Some);
    }
    let wanted = format!("{owner}.{property}");
    for child in &node.children {
        if !child.xps || child.local != wanted {
            continue;
        }
        let geometry = child
            .children
            .iter()
            .find(|g| g.xps && g.local == "PathGeometry")
            .ok_or(GeometryError::Syntax)?;
        return from_geometry_node(geometry, budget).map(Some);
    }
    Ok(None)
}
