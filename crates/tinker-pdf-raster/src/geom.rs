//! Paths and the flattening that turns curves into edges.
//!
//! Zero PDF knowledge lives here: points, curves and transforms, nothing else.

/// A point in device space.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Point {
    /// Horizontal.
    pub x: f64,
    /// Vertical.
    pub y: f64,
}

impl Point {
    /// A point.
    #[must_use]
    pub fn new(x: f64, y: f64) -> Point {
        Point { x, y }
    }

    /// Whether both coordinates are finite.
    #[must_use]
    pub fn is_finite(&self) -> bool {
        self.x.is_finite() && self.y.is_finite()
    }
}

/// One instruction of a path.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Verb {
    /// Begin a subpath.
    MoveTo(Point),
    /// A straight segment.
    LineTo(Point),
    /// A quadratic Bézier, with its single control point and its end.
    ///
    /// First class, not raised to a cubic: every TrueType glyph outline is
    /// quadratic, so the up-conversion would run on the hottest path in the
    /// engine and pay for a control point the curve does not have.
    QuadTo(Point, Point),
    /// A cubic Bézier, with its two control points and its end.
    CurveTo(Point, Point, Point),
    /// Close the current subpath.
    Close,
}

/// A path: a sequence of subpaths.
#[derive(Clone, Debug, Default)]
pub struct Path {
    verbs: Vec<Verb>,
}

impl Path {
    /// An empty path.
    #[must_use]
    pub fn new() -> Path {
        Path::default()
    }

    /// The verbs.
    #[must_use]
    pub fn verbs(&self) -> &[Verb] {
        &self.verbs
    }

    /// Whether the path has no verbs.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.verbs.is_empty()
    }

    /// Begins a subpath.
    pub fn move_to(&mut self, x: f64, y: f64) {
        self.push(Verb::MoveTo(Point::new(x, y)));
    }

    /// Adds a straight segment.
    pub fn line_to(&mut self, x: f64, y: f64) {
        self.push(Verb::LineTo(Point::new(x, y)));
    }

    /// Adds a quadratic Bézier.
    pub fn quad_to(&mut self, cx: f64, cy: f64, x: f64, y: f64) {
        self.push(Verb::QuadTo(Point::new(cx, cy), Point::new(x, y)));
    }

    /// Adds a cubic Bézier.
    pub fn curve_to(&mut self, x1: f64, y1: f64, x2: f64, y2: f64, x3: f64, y3: f64) {
        self.push(Verb::CurveTo(
            Point::new(x1, y1),
            Point::new(x2, y2),
            Point::new(x3, y3),
        ));
    }

    /// Closes the current subpath.
    /// Appends another path's verbs to this one.
    ///
    /// The two keep their own subpaths — nothing is joined — which is what
    /// accumulating many glyph outlines into a single clipping path needs.
    pub fn extend(&mut self, other: &Path) {
        self.verbs.extend_from_slice(other.verbs());
    }

    pub fn close(&mut self) {
        self.push(Verb::Close);
    }

    /// Adds a rectangle as its own subpath.
    pub fn rect(&mut self, x: f64, y: f64, w: f64, h: f64) {
        self.move_to(x, y);
        self.line_to(x + w, y);
        self.line_to(x + w, y + h);
        self.line_to(x, y + h);
        self.close();
    }

    fn push(&mut self, verb: Verb) {
        // A path with non-finite points cannot be rasterized; dropping the
        // verb keeps the rest of the path usable.
        let finite = match verb {
            Verb::MoveTo(p) | Verb::LineTo(p) => p.is_finite(),
            Verb::QuadTo(c, p) => c.is_finite() && p.is_finite(),
            Verb::CurveTo(a, b, c) => a.is_finite() && b.is_finite() && c.is_finite(),
            Verb::Close => true,
        };
        if finite && self.verbs.len() < MAX_VERBS {
            self.verbs.push(verb);
        }
    }

    /// The path's bounding box as `(x0, y0, x1, y1)`, control points included.
    #[must_use]
    pub fn bounds(&self) -> Option<(f64, f64, f64, f64)> {
        let mut bounds: Option<(f64, f64, f64, f64)> = None;
        let mut add = |p: Point| {
            bounds = Some(match bounds {
                None => (p.x, p.y, p.x, p.y),
                Some((x0, y0, x1, y1)) => (x0.min(p.x), y0.min(p.y), x1.max(p.x), y1.max(p.y)),
            });
        };
        for verb in &self.verbs {
            match *verb {
                Verb::MoveTo(p) | Verb::LineTo(p) => add(p),
                Verb::QuadTo(c, p) => {
                    add(c);
                    add(p);
                }
                Verb::CurveTo(a, b, c) => {
                    add(a);
                    add(b);
                    add(c);
                }
                Verb::Close => {}
            }
        }
        bounds
    }
}

/// A path may not describe more verbs than this. A content stream can ask for
/// unbounded geometry; a real page never does.
const MAX_VERBS: usize = 1 << 20;

/// Flattens a path into polylines, one per subpath.
///
/// Single-point subpaths are **kept**: filling ignores them (a point has no
/// edges) but stroking draws them as dots under a round or square cap, which
/// 8.4.3.3 requires. Dropping them here would lose that information for good.
///
/// `tolerance` is the greatest distance a chord may stray from the true curve,
/// in device units. Subdivision is by a fixed count derived from the curve's
/// control polygon, which is cheap, deterministic, and — unlike recursive
/// subdivision with a floating-point termination test — produces the same
/// output on every platform (ruling 4).
#[must_use]
pub fn flatten(path: &Path, tolerance: f64) -> Vec<Vec<Point>> {
    let tolerance = if tolerance.is_finite() && tolerance > 1e-6 {
        tolerance
    } else {
        0.1
    };

    let mut out: Vec<Vec<Point>> = Vec::new();
    let mut current: Vec<Point> = Vec::new();
    let mut start = Point::new(0.0, 0.0);
    let mut cursor = start;

    for verb in path.verbs() {
        match *verb {
            Verb::MoveTo(p) => {
                if !current.is_empty() {
                    out.push(std::mem::take(&mut current));
                }
                current.push(p);
                start = p;
                cursor = p;
            }
            Verb::LineTo(p) => {
                if current.is_empty() {
                    current.push(cursor);
                }
                current.push(p);
                cursor = p;
            }
            Verb::QuadTo(c, end) => {
                if current.is_empty() {
                    current.push(cursor);
                }
                subdivide_quadratic(cursor, c, end, tolerance, &mut current);
                cursor = end;
            }
            Verb::CurveTo(c1, c2, end) => {
                if current.is_empty() {
                    current.push(cursor);
                }
                subdivide(cursor, c1, c2, end, tolerance, &mut current);
                cursor = end;
            }
            Verb::Close => {
                if current.len() > 1 {
                    current.push(start);
                }
                if !current.is_empty() {
                    out.push(std::mem::take(&mut current));
                }
                cursor = start;
            }
        }
    }

    if !current.is_empty() {
        out.push(current);
    }
    out
}

/// How many segments a curve whose control polygon is `polygon` long needs.
///
/// The control polygon's length bounds the arc length, and the number of
/// segments needed grows with its square root. Shared by both subdividers, so
/// a curve cannot be flattened more finely because of which verb describes it.
fn steps_for(polygon: f64, tolerance: f64) -> u32 {
    let steps = ((polygon / tolerance).sqrt() * 1.5).ceil();
    steps.clamp(1.0, 512.0) as u32
}

/// Appends a flattened quadratic, excluding its start point.
fn subdivide_quadratic(p0: Point, p1: Point, p2: Point, tolerance: f64, out: &mut Vec<Point>) {
    // Measured through the cubic this quadratic is equal to — control points
    // two thirds of the way from each end toward `p1`. That cubic's polygon is
    // `2/3` of each leg plus a third of the chord, and using it rather than the
    // quadratic's own two legs is what makes the two arms agree: the same
    // curve gets the same step count whichever verb carries it, so a glyph
    // does not change smoothness where a font happens to switch from
    // quadratics to cubics.
    let polygon = 2.0 / 3.0 * (distance(p0, p1) + distance(p1, p2)) + distance(p0, p2) / 3.0;
    if !polygon.is_finite() || polygon <= tolerance {
        out.push(p2);
        return;
    }

    let steps = steps_for(polygon, tolerance);
    for i in 1..=steps {
        let t = f64::from(i) / f64::from(steps);
        let u = 1.0 - t;
        let x = u * u * p0.x + 2.0 * u * t * p1.x + t * t * p2.x;
        let y = u * u * p0.y + 2.0 * u * t * p1.y + t * t * p2.y;
        out.push(Point::new(x, y));
    }
}

/// Appends a flattened cubic, excluding its start point.
fn subdivide(p0: Point, p1: Point, p2: Point, p3: Point, tolerance: f64, out: &mut Vec<Point>) {
    let polygon = distance(p0, p1) + distance(p1, p2) + distance(p2, p3);
    if !polygon.is_finite() || polygon <= tolerance {
        out.push(p3);
        return;
    }

    let steps = steps_for(polygon, tolerance);
    for i in 1..=steps {
        let t = f64::from(i) / f64::from(steps);
        let u = 1.0 - t;
        let x =
            u * u * u * p0.x + 3.0 * u * u * t * p1.x + 3.0 * u * t * t * p2.x + t * t * t * p3.x;
        let y =
            u * u * u * p0.y + 3.0 * u * u * t * p1.y + 3.0 * u * t * t * p2.y + t * t * t * p3.y;
        out.push(Point::new(x, y));
    }
}

fn distance(a: Point, b: Point) -> f64 {
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    (dx * dx + dy * dy).sqrt()
}

/// Which points a fill considers inside (8.5.3.3).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FillRule {
    /// Non-zero winding: the default.
    #[default]
    NonZero,
    /// Even-odd.
    EvenOdd,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_rectangle_flattens_to_a_closed_polygon() {
        let mut path = Path::new();
        path.rect(0.0, 0.0, 10.0, 20.0);
        let polys = flatten(&path, 0.1);

        assert_eq!(polys.len(), 1);
        let poly = polys.first().expect("a polygon");
        assert_eq!(poly.len(), 5, "four corners and the closing point");
        assert_eq!(poly.first(), poly.last(), "closed");
        assert_eq!(path.bounds(), Some((0.0, 0.0, 10.0, 20.0)));
    }

    #[test]
    fn a_curve_subdivides_toward_its_tolerance() {
        let mut path = Path::new();
        path.move_to(0.0, 0.0);
        path.curve_to(0.0, 100.0, 100.0, 100.0, 100.0, 0.0);

        let coarse = flatten(&path, 10.0);
        let fine = flatten(&path, 0.01);
        let coarse_len = coarse.first().map_or(0, Vec::len);
        let fine_len = fine.first().map_or(0, Vec::len);
        assert!(
            fine_len > coarse_len,
            "a tighter tolerance means more segments: {fine_len} vs {coarse_len}"
        );

        // Both start and end at the curve's own endpoints.
        for polys in [&coarse, &fine] {
            let poly = polys.first().expect("a polyline");
            assert_eq!(poly.first().copied(), Some(Point::new(0.0, 0.0)));
            assert_eq!(poly.last().copied(), Some(Point::new(100.0, 0.0)));
        }
    }

    /// Every quadratic in these tests, as start, control and end.
    const QUADRATICS: &[[(f64, f64); 3]] = &[
        // A symmetric arch, which is what most glyph curves are.
        [(0.0, 0.0), (50.0, 100.0), (100.0, 0.0)],
        // Asymmetric, so a subdivider that folded the wrong way shows.
        [(3.5, 7.25), (90.0, 4.0), (12.0, 60.5)],
        // The control point on the chord: the curve is a straight line, and
        // the two arms must still agree about how many pieces it is in.
        [(0.0, 0.0), (25.0, 25.0), (100.0, 100.0)],
        // A cusp: the control point behind the start, so the curve doubles
        // back on itself.
        [(10.0, 10.0), (-40.0, 10.0), (30.0, 10.0)],
        // Smaller than any sane tolerance, which takes the early return.
        [(1.0, 1.0), (1.02, 1.01), (1.04, 1.0)],
        // Far from the origin, where the absolute float spacing is coarse.
        [(8000.0, -6000.0), (8300.5, -5800.25), (8600.0, -6000.0)],
        // Steep enough to need the step cap.
        [(0.0, 0.0), (4000.0, 9000.0), (8000.0, 0.0)],
    ];

    /// Milestone 1's exit criterion, and the only thing that stops the two
    /// subdividers drifting apart.
    ///
    /// The comparison is against the exact cubic equivalent, built by the very
    /// two-thirds rule this verb exists to delete: control points two thirds
    /// of the way from each end toward the quadratic's own control. The two
    /// are the same curve, so they must flatten to the same polyline, point
    /// for point.
    ///
    /// `1e-9` is not a fudge factor. The two evaluations are algebraically
    /// identical and differ only in float rounding, so the real distance is
    /// nearer 1e-12; the bound is still four orders below the 1/256 unit the
    /// rasteriser snaps to. A quadratic arm with its own tolerance, or one
    /// that interpolated at the wrong point, would miss by device-visible
    /// amounts rather than by this.
    #[test]
    fn a_quadratic_flattens_like_its_exact_cubic_equivalent() {
        for tolerance in [1.0, 0.25, 0.1, 0.01, 0.001] {
            for curve in QUADRATICS {
                let [(x0, y0), (cx, cy), (x1, y1)] = *curve;

                let mut quad = Path::new();
                quad.move_to(x0, y0);
                quad.quad_to(cx, cy, x1, y1);

                let mut cubic = Path::new();
                cubic.move_to(x0, y0);
                cubic.curve_to(
                    x0 + 2.0 / 3.0 * (cx - x0),
                    y0 + 2.0 / 3.0 * (cy - y0),
                    x1 + 2.0 / 3.0 * (cx - x1),
                    y1 + 2.0 / 3.0 * (cy - y1),
                    x1,
                    y1,
                );

                let flat_quad = flatten(&quad, tolerance);
                let flat_cubic = flatten(&cubic, tolerance);
                let from_quad = flat_quad.first().expect("a polyline");
                let from_cubic = flat_cubic.first().expect("a polyline");

                assert_eq!(
                    from_quad.len(),
                    from_cubic.len(),
                    "{curve:?} at tolerance {tolerance} broke into \
                     {} pieces as a quadratic and {} as the same cubic",
                    from_quad.len(),
                    from_cubic.len(),
                );
                for (a, b) in from_quad.iter().zip(from_cubic) {
                    let off = distance(*a, *b);
                    assert!(
                        off <= 1e-9,
                        "{curve:?} at tolerance {tolerance}: {a:?} against \
                         {b:?} is {off} apart"
                    );
                }
            }
        }
    }

    /// The bug the verb removes, at the level the flattener owns it.
    ///
    /// A close returns the pen to where the subpath began, so a curve that
    /// follows one starts there. The verb carries no start point, which is why
    /// it cannot get this wrong; the caller that had to look one up could, and
    /// did.
    #[test]
    fn a_quadratic_after_a_close_starts_where_the_subpath_did() {
        let mut path = Path::new();
        path.move_to(10.0, 10.0);
        path.line_to(20.0, 10.0);
        path.line_to(20.0, 14.0);
        path.close();
        path.quad_to(30.0, 60.0, 50.0, 10.0);

        let polys = flatten(&path, 0.1);
        assert_eq!(polys.len(), 2, "the close ended the first subpath");
        let arch = polys.get(1).expect("the second subpath");
        assert_eq!(
            arch.first().copied(),
            Some(Point::new(10.0, 10.0)),
            "the curve begins at the closed subpath's start, not at its own end"
        );
        assert_eq!(arch.last().copied(), Some(Point::new(50.0, 10.0)));
        // The apex of that quadratic is at t = 1/2, a quarter of each end plus
        // half the control: (30, 35). Raising this to a cubic from the curve's
        // own end instead — the defect this verb replaces — peaks at (35, 35).
        let apex = arch
            .iter()
            .copied()
            .max_by(|a, b| a.y.total_cmp(&b.y))
            .expect("a highest point");
        assert!(
            (apex.x - 30.0).abs() < 0.5 && (apex.y - 35.0).abs() < 0.5,
            "the curve should peak near (30, 35), not {apex:?}"
        );
    }

    #[test]
    fn a_quadratics_control_point_counts_toward_the_bounds() {
        let mut path = Path::new();
        path.move_to(0.0, 0.0);
        path.quad_to(50.0, 100.0, 100.0, 0.0);
        assert_eq!(
            path.bounds(),
            Some((0.0, 0.0, 100.0, 100.0)),
            "control points are included, so a bound is never too small"
        );
    }

    #[test]
    fn a_quadratic_with_a_non_finite_point_is_refused_too() {
        let mut path = Path::new();
        path.move_to(0.0, 0.0);
        path.quad_to(f64::INFINITY, 1.0, 2.0, 3.0);
        path.quad_to(1.0, f64::NAN, 2.0, 3.0);
        path.quad_to(1.0, 2.0, f64::NAN, 3.0);
        path.quad_to(5.0, 9.0, 10.0, 0.0);
        assert_eq!(path.verbs().len(), 2, "only the finite curve survived");

        for poly in flatten(&path, 0.1) {
            assert!(poly.iter().all(Point::is_finite));
        }
    }

    #[test]
    fn a_quadratic_obeys_the_same_per_curve_point_cap_as_a_cubic() {
        let mut quad = Path::new();
        quad.move_to(0.0, 0.0);
        quad.quad_to(1e6, 1e6, 2e6, 0.0);
        let points = flatten(&quad, 1e-6).first().map_or(0, Vec::len);
        assert_eq!(points, 513, "512 steps plus the start point");
    }

    #[test]
    fn flattening_is_deterministic() {
        let mut path = Path::new();
        path.move_to(3.5, 7.25);
        path.curve_to(10.0, 90.0, 80.0, 95.5, 100.0, 2.0);
        assert_eq!(flatten(&path, 0.1), flatten(&path, 0.1));
    }

    #[test]
    fn non_finite_points_are_refused_at_construction() {
        let mut path = Path::new();
        path.move_to(0.0, 0.0);
        path.line_to(f64::NAN, 5.0);
        path.line_to(10.0, 10.0);
        assert_eq!(path.verbs().len(), 2, "the NaN verb was dropped");

        for poly in flatten(&path, 0.1) {
            assert!(poly.iter().all(Point::is_finite));
        }
    }

    #[test]
    fn degenerate_input_produces_nothing_rather_than_panicking() {
        assert!(flatten(&Path::new(), 0.1).is_empty());

        // A single point is kept so stroking can draw it as a dot; filling
        // ignores it, since a point has no edges.
        let mut lone = Path::new();
        lone.move_to(1.0, 1.0);
        assert_eq!(flatten(&lone, 0.1), vec![vec![Point::new(1.0, 1.0)]]);

        let mut closed_nothing = Path::new();
        closed_nothing.close();
        assert!(flatten(&closed_nothing, 0.1).is_empty());

        let mut path = Path::new();
        path.move_to(0.0, 0.0);
        path.line_to(1.0, 1.0);
        // A nonsense tolerance falls back to the default rather than dividing
        // by zero or looping forever.
        let _ = flatten(&path, 0.0);
        let _ = flatten(&path, -1.0);
        let _ = flatten(&path, f64::NAN);
    }

    #[test]
    fn subpaths_separate_at_every_move() {
        let mut path = Path::new();
        path.move_to(0.0, 0.0);
        path.line_to(10.0, 0.0);
        path.move_to(0.0, 10.0);
        path.line_to(10.0, 10.0);
        assert_eq!(flatten(&path, 0.1).len(), 2);
    }
}
