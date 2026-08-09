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

    /// Adds a cubic Bézier.
    pub fn curve_to(&mut self, x1: f64, y1: f64, x2: f64, y2: f64, x3: f64, y3: f64) {
        self.push(Verb::CurveTo(
            Point::new(x1, y1),
            Point::new(x2, y2),
            Point::new(x3, y3),
        ));
    }

    /// Closes the current subpath.
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

/// Appends a flattened cubic, excluding its start point.
fn subdivide(p0: Point, p1: Point, p2: Point, p3: Point, tolerance: f64, out: &mut Vec<Point>) {
    // The control polygon's length bounds the arc length, and the number of
    // segments needed grows with its square root.
    let polygon = distance(p0, p1) + distance(p1, p2) + distance(p2, p3);
    if !polygon.is_finite() || polygon <= tolerance {
        out.push(p3);
        return;
    }

    let steps = ((polygon / tolerance).sqrt() * 1.5).ceil();
    let steps = steps.clamp(1.0, 512.0) as u32;

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
