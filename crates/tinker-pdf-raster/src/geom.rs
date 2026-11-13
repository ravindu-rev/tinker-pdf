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

    /// Appends another path's verbs to this one.
    ///
    /// The two keep their own subpaths — nothing is joined — which is what
    /// accumulating many glyph outlines into a single clipping path needs.
    pub fn extend(&mut self, other: &Path) {
        self.verbs.extend_from_slice(other.verbs());
    }

    /// Closes the current subpath.
    pub fn close(&mut self) {
        self.push(Verb::Close);
    }

    /// The same path with every point, control points included, carried
    /// through `map` — which is exact for a Bézier, since an affine map of
    /// the control polygon is the map of the curve. A point the map takes
    /// past the finite is dropped as [`Path::line_to`] drops one.
    #[must_use]
    pub fn mapped(&self, map: &crate::image::Transform) -> Path {
        let at = |p: Point| {
            let (x, y) = map.apply(p.x, p.y);
            Point::new(x, y)
        };
        let mut out = Path::new();
        for verb in &self.verbs {
            out.push(match *verb {
                Verb::MoveTo(p) => Verb::MoveTo(at(p)),
                Verb::LineTo(p) => Verb::LineTo(at(p)),
                Verb::QuadTo(c, p) => Verb::QuadTo(at(c), at(p)),
                Verb::CurveTo(a, b, c) => Verb::CurveTo(at(a), at(b), at(c)),
                Verb::Close => Verb::Close,
            });
        }
        out
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

    /// The path with every contour moved outward by `horizontal` device units
    /// across and `vertical` down: stem darkening's outline offset
    /// (`docs/design/hinting.md`, *Stem darkening*).
    ///
    /// Every point of each contour, on and off the curve alike, moves along
    /// the bisector of its two edges' outward normals by the miter vector
    /// `(n_in + n_out) / (1 + n_in · n_out)`, its `x` scaled by `horizontal`
    /// and its `y` by `vertical`, so each edge of the control polygon moves
    /// out in proportion to its normal and an axis-aligned rectangle grows by
    /// exactly twice each amount. Outward is the side that grows the path's
    /// total signed area: a contour wound with the total grows and one wound
    /// against it — a counter — shrinks, whichever winding the glyph uses.
    ///
    /// A point's edges run to the nearest points of its contour that differ
    /// from it, so a zero-length edge keeps its point with its neighbours',
    /// and a contour of one point keeps it. A miter longer than
    /// [`OFFSET_MITER_LIMIT`] is held there along the bisector, and a turn
    /// straight back, where the bisector vanishes, keeps its point; a move
    /// that is not finite keeps its point too. A subpath the path leaves open
    /// is offset as the closed contour a fill makes of it (8.5.3.1), and one
    /// that begins after a `Close` without a `MoveTo` gains one at its first
    /// point. Amounts that are not finite, both amounts zero, and a path whose
    /// total area is zero or not finite give the path back as it came.
    ///
    /// Only `sqrt`, which IEEE 754 rounds correctly, and the four operations
    /// are used, so the offset is the same on every target (ruling 4).
    #[must_use]
    pub fn offset(&self, horizontal: f64, vertical: f64) -> Path {
        if !horizontal.is_finite()
            || !vertical.is_finite()
            || (horizontal == 0.0 && vertical == 0.0)
        {
            return self.clone();
        }
        let contours = Contour::all(self);
        let total: f64 = contours.iter().map(Contour::twice_area).sum();
        if !total.is_finite() || total == 0.0 {
            return self.clone();
        }
        let hand = if total > 0.0 { 1.0 } else { -1.0 };
        let mut out = Path::new();
        for contour in &contours {
            contour.offset_into(&mut out, hand, horizontal, vertical);
        }
        out
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

/// The longest a point's move may be, in units of the offset's amount: past
/// it the miter is held at this length along the bisector, so a spike — a turn
/// sharper than about 151° — grows by four times the amount and no more.
pub const OFFSET_MITER_LIMIT: f64 = 4.0;

/// Which verb a run of a contour's control points came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Drawn {
    Line,
    Quad,
    Cubic,
}

/// One subpath as [`Path::offset`] reads it: its start, then each drawing
/// verb's points in order, and whether a `Close` ended it.
#[derive(Debug, Default)]
struct Contour {
    points: Vec<Point>,
    drawn: Vec<Drawn>,
    closed: bool,
}

impl Contour {
    /// The path's subpaths, begun where [`flatten`] begins them: at a
    /// `MoveTo`, and after a `Close` or at the very start of the path, at the
    /// point the pen stands on.
    fn all(path: &Path) -> Vec<Contour> {
        let mut out = Vec::new();
        let mut current: Option<Contour> = None;
        let mut start = Point::new(0.0, 0.0);
        let mut cursor = start;
        let open = |current: &mut Option<Contour>, cursor: Point| {
            current.get_or_insert_with(|| Contour {
                points: vec![cursor],
                ..Contour::default()
            });
        };
        for verb in path.verbs() {
            match *verb {
                Verb::MoveTo(p) => {
                    out.extend(current.take());
                    current = Some(Contour {
                        points: vec![p],
                        ..Contour::default()
                    });
                    start = p;
                    cursor = p;
                }
                Verb::LineTo(p) => {
                    open(&mut current, cursor);
                    if let Some(contour) = current.as_mut() {
                        contour.points.push(p);
                        contour.drawn.push(Drawn::Line);
                    }
                    cursor = p;
                }
                Verb::QuadTo(c, p) => {
                    open(&mut current, cursor);
                    if let Some(contour) = current.as_mut() {
                        contour.points.extend([c, p]);
                        contour.drawn.push(Drawn::Quad);
                    }
                    cursor = p;
                }
                Verb::CurveTo(c1, c2, p) => {
                    open(&mut current, cursor);
                    if let Some(contour) = current.as_mut() {
                        contour.points.extend([c1, c2, p]);
                        contour.drawn.push(Drawn::Cubic);
                    }
                    cursor = p;
                }
                Verb::Close => {
                    if let Some(mut contour) = current.take() {
                        contour.closed = true;
                        out.push(contour);
                    }
                    cursor = start;
                }
            }
        }
        out.extend(current);
        out
    }

    /// Twice the signed area of the contour's closed control polygon, taken
    /// about its first point so that a contour far from the origin keeps its
    /// precision.
    fn twice_area(&self) -> f64 {
        let Some(&first) = self.points.first() else {
            return 0.0;
        };
        let ahead = self.points.iter().cycle().skip(1);
        self.points
            .iter()
            .zip(ahead)
            .map(|(a, b)| (a.x - first.x) * (b.y - first.y) - (b.x - first.x) * (a.y - first.y))
            .sum()
    }

    /// Writes the contour, offset, into `out`: a `MoveTo` at its first point,
    /// its verbs with their points moved, and its `Close` if it had one.
    fn offset_into(&self, out: &mut Path, hand: f64, horizontal: f64, vertical: f64) {
        let moved = self.moved(hand, horizontal, vertical);
        let mut at = moved.iter().copied();
        let Some(first) = at.next() else {
            return;
        };
        out.move_to(first.x, first.y);
        for drawn in &self.drawn {
            match drawn {
                Drawn::Line => {
                    if let Some(p) = at.next() {
                        out.line_to(p.x, p.y);
                    }
                }
                Drawn::Quad => {
                    if let (Some(c), Some(p)) = (at.next(), at.next()) {
                        out.quad_to(c.x, c.y, p.x, p.y);
                    }
                }
                Drawn::Cubic => {
                    if let (Some(c1), Some(c2), Some(p)) = (at.next(), at.next(), at.next()) {
                        out.curve_to(c1.x, c1.y, c2.x, c2.y, p.x, p.y);
                    }
                }
            }
        }
        if self.closed {
            out.close();
        }
    }

    /// Every point of the contour, moved. Consecutive equal points — around
    /// the contour, the last beside the first — are one run and one vertex:
    /// each run's neighbours are the runs either side, read once around, so
    /// the work is linear however many points repeat.
    fn moved(&self, hand: f64, horizontal: f64, vertical: f64) -> Vec<Point> {
        let points = &self.points;
        let n = points.len();
        let mut out = points.clone();
        // Whether a point differs from the one before it around the contour,
        // which starts a run.
        let differs = |i: usize| match (points.get(i), points.get((i + n - 1) % n)) {
            (Some(here), Some(before)) => here != before,
            _ => false,
        };
        // A contour with no such point is a single point, however often it is
        // written, and keeps it.
        let Some(begin) = (0..n).find(|&i| differs(i)) else {
            return out;
        };
        let mut runs: Vec<(usize, usize)> = Vec::new();
        for step in 0..n {
            let i = (begin + step) % n;
            match runs.last_mut() {
                Some((_, len)) if !differs(i) => *len += 1,
                _ => runs.push((i, 1)),
            }
        }
        // Two runs at least, since `begin` differs from the point before it.
        let count = runs.len();
        let vertex = |r: usize| {
            runs.get(r % count)
                .and_then(|(first, _)| points.get(*first))
                .copied()
        };
        for (r, &(first, len)) in runs.iter().enumerate() {
            let (Some(previous), Some(here), Some(next)) =
                (vertex(r + count - 1), vertex(r), vertex(r + 1))
            else {
                continue;
            };
            let Some(step) = miter(previous, here, next, hand) else {
                continue;
            };
            let to = Point::new(here.x + horizontal * step.x, here.y + vertical * step.y);
            if !to.is_finite() {
                continue;
            }
            for k in 0..len {
                if let Some(point) = out.get_mut((first + k) % n) {
                    *point = to;
                }
            }
        }
        out
    }
}

/// The unit vector along `(x, y)`, or `None` for a zero or non-finite one.
/// Scaled by its larger component before the root, so no square overflows.
fn unit(x: f64, y: f64) -> Option<Point> {
    let largest = x.abs().max(y.abs());
    if largest == 0.0 || !largest.is_finite() {
        return None;
    }
    let (sx, sy) = (x / largest, y / largest);
    let length = (sx * sx + sy * sy).sqrt();
    Some(Point::new(sx / length, sy / length))
}

/// The move, per unit of offset, of the vertex `here` between the edges from
/// `previous` and to `next`: the miter vector of their outward normals, held
/// at [`OFFSET_MITER_LIMIT`]. `None` keeps the vertex: an edge with no
/// direction, or a turn straight back.
fn miter(previous: Point, here: Point, next: Point, hand: f64) -> Option<Point> {
    let incoming = unit(here.x - previous.x, here.y - previous.y)?;
    let outgoing = unit(next.x - here.x, next.y - here.y)?;
    // The normal on the right of each edge's direction, for a path of positive
    // total area; on the left for one of negative.
    let n_in = Point::new(hand * incoming.y, -hand * incoming.x);
    let n_out = Point::new(hand * outgoing.y, -hand * outgoing.x);
    let sum = Point::new(n_in.x + n_out.x, n_in.y + n_out.y);
    let length = (sum.x * sum.x + sum.y * sum.y).sqrt();
    if length <= 1e-9 {
        return None;
    }
    let cosine = 1.0 + (n_in.x * n_out.x + n_in.y * n_out.y);
    // `|m|` is `sqrt(2 / cosine)`, which reaches the limit at `2 / limit²`.
    if cosine < 2.0 / (OFFSET_MITER_LIMIT * OFFSET_MITER_LIMIT) {
        let scale = OFFSET_MITER_LIMIT / length;
        return Some(Point::new(sum.x * scale, sum.y * scale));
    }
    Some(Point::new(sum.x / cosine, sum.y / cosine))
}

/// A path may not describe more verbs than this. A content stream can ask for
/// unbounded geometry; a real page never does.
///
/// A stroke's outline written straight into a fill's edges holds the same
/// bound, counted in the verbs its pieces would have been
/// (`crate::fill::Outline`), so it stops where its path would have.
pub(crate) const MAX_VERBS: usize = 1 << 20;

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
///
/// A tolerance of a millionth of a device unit or less, or one that is not a
/// finite number, is read as a mistake and replaced with 0.1.
#[must_use]
pub fn flatten(path: &Path, tolerance: f64) -> Vec<Vec<Point>> {
    let tolerance = if tolerance.is_finite() && tolerance > 1e-6 {
        tolerance
    } else {
        0.1
    };
    flatten_at(path, tolerance)
}

/// [`flatten`] for a path in a space some map will carry far, at a tolerance
/// in **that** space: any finite positive tolerance is honoured, however
/// small, and only one that is not is replaced with 0.1.
///
/// `flatten`'s floor guards a tolerance stated in device units, where a
/// millionth is nonsense. A path in a pen's own space under a map that
/// stretches it 400 000 times is flattened at the device tolerance over that
/// stretch — half a millionth for 0.2 — which is under the floor and exactly
/// right; read as 0.1 of the path's own units it was 20 000 device units, and
/// a circle of four Béziers came out a diamond. The work stays bounded
/// however small the tolerance: a curve takes at most 512 steps.
#[must_use]
pub(crate) fn flatten_mapped(path: &Path, tolerance: f64) -> Vec<Vec<Point>> {
    let tolerance = if tolerance.is_finite() && tolerance > 0.0 {
        tolerance
    } else {
        0.1
    };
    flatten_at(path, tolerance)
}

/// [`flatten`] into two buffers the caller owns and reuses: every subpath's
/// points one after another in `points`, and in `ends` the index in `points`
/// each subpath ends at. Both are emptied first. The same points as
/// [`flatten`], from the same walk, without a vector a subpath.
pub(crate) fn flatten_flat(
    path: &Path,
    tolerance: f64,
    points: &mut Vec<Point>,
    ends: &mut Vec<usize>,
) {
    let tolerance = if tolerance.is_finite() && tolerance > 1e-6 {
        tolerance
    } else {
        0.1
    };
    points.clear();
    ends.clear();
    flatten_walk(path, tolerance, points, |points, _| ends.push(points.len()));
}

/// The flattening itself, at a tolerance already known to be finite and
/// positive.
fn flatten_at(path: &Path, tolerance: f64) -> Vec<Vec<Point>> {
    let mut out: Vec<Vec<Point>> = Vec::new();
    let mut points: Vec<Point> = Vec::new();
    flatten_walk(path, tolerance, &mut points, |points, begin| {
        out.push(points.split_off(begin));
    });
    out
}

/// The walk both forms share: each subpath's points are pushed onto the end
/// of `points`, and `close(points, begin)` is called as each one ends, with
/// the subpath being `points[begin..]`. A `close` may take those points away
/// or leave them; the next subpath begins wherever `points` then ends.
fn flatten_walk(
    path: &Path,
    tolerance: f64,
    points: &mut Vec<Point>,
    mut close: impl FnMut(&mut Vec<Point>, usize),
) {
    count::flattened();
    let mut begin = points.len();
    let mut start = Point::new(0.0, 0.0);
    let mut cursor = start;

    for verb in path.verbs() {
        match *verb {
            Verb::MoveTo(p) => {
                if points.len() > begin {
                    close(points, begin);
                    begin = points.len();
                }
                points.push(p);
                start = p;
                cursor = p;
            }
            Verb::LineTo(p) => {
                if points.len() == begin {
                    points.push(cursor);
                }
                points.push(p);
                cursor = p;
            }
            Verb::QuadTo(c, end) => {
                if points.len() == begin {
                    points.push(cursor);
                }
                subdivide_quadratic(cursor, c, end, tolerance, points);
                cursor = end;
            }
            Verb::CurveTo(c1, c2, end) => {
                if points.len() == begin {
                    points.push(cursor);
                }
                subdivide(cursor, c1, c2, end, tolerance, points);
                cursor = end;
            }
            Verb::Close => {
                if points.len() - begin > 1 {
                    points.push(start);
                }
                if points.len() > begin {
                    close(points, begin);
                    begin = points.len();
                }
                cursor = start;
            }
        }
    }

    if points.len() > begin {
        close(points, begin);
    }
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

/// Flattening passes, counted for the stroker's guard: compiled only under
/// `cfg(test)` and kept per thread, as the fill's counters are.
#[cfg(test)]
pub(crate) mod count {
    use std::cell::Cell;

    std::thread_local! {
        static FLATTENED: Cell<u64> = const { Cell::new(0) };
    }

    /// One walk of a path's verbs into polylines.
    pub(crate) fn flattened() {
        FLATTENED.with(|count| count.set(count.get() + 1));
    }

    /// The walks made on this thread since the last call, which starts the
    /// count again from zero.
    pub(crate) fn take() -> u64 {
        FLATTENED.with(|count| count.replace(0))
    }
}

/// The counter outside tests: nothing.
#[cfg(not(test))]
mod count {
    pub(crate) fn flattened() {}
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

    /// A tolerance in a pen's space is honoured below a millionth: a quarter
    /// circle of radius 1e-4 at 5e-7 is the same number of steps as one of
    /// radius 100 at 0.5 — the ratio is what counts — where `flatten`'s
    /// floor for a device tolerance makes it one chord.
    #[test]
    fn a_mapped_tolerance_is_honoured_below_a_millionth() {
        let quarter = |r: f64| {
            let k = 0.552_284_75 * r;
            let mut path = Path::new();
            path.move_to(r, 0.0);
            path.curve_to(r, k, k, r, 0.0, r);
            path
        };
        let count = |polys: Vec<Vec<Point>>| polys.first().map_or(0, Vec::len);
        let large = count(flatten(&quarter(100.0), 0.5));
        assert!(large > 2, "a curve, not a chord: {large}");
        assert_eq!(count(flatten_mapped(&quarter(1e-4), 5e-7)), large);
        assert_eq!(count(flatten(&quarter(1e-4), 5e-7)), 2, "the floor's chord");
        // Only a tolerance that is not a positive number is replaced.
        for bad in [0.0, -1.0, f64::NAN, f64::INFINITY] {
            assert_eq!(
                flatten_mapped(&quarter(1.0), bad),
                flatten(&quarter(1.0), 0.1)
            );
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

    /// The flat form the filler reuses is the nested form, point for point
    /// and subpath for subpath — over moves without lines, lines without a
    /// move, closes twice over, both curve verbs, and the tolerances `flatten`
    /// replaces — and it empties its buffers first.
    #[test]
    fn the_flat_form_is_the_nested_one() {
        let mut path = Path::new();
        path.line_to(3.0, 4.0);
        path.quad_to(9.0, 1.0, 12.0, 7.5);
        path.move_to(1.0, 1.0);
        path.move_to(2.0, 2.0);
        path.curve_to(30.0, 2.0, -10.0, 20.0, 15.0, 18.0);
        path.close();
        path.close();
        path.line_to(40.0, 3.0);
        path.quad_to(44.0, 9.0, 41.0, 12.0);
        path.close();
        path.move_to(5.0, 5.0);
        let (mut points, mut ends) = (vec![Point::new(9.0, 9.0)], vec![7]);
        for tolerance in [0.01, 0.2, 1.0, 0.0, f64::NAN] {
            flatten_flat(&path, tolerance, &mut points, &mut ends);
            let mut flat = Vec::new();
            let mut begin = 0;
            for end in &ends {
                flat.push(points.get(begin..*end).map(<[Point]>::to_vec));
                begin = *end;
            }
            assert_eq!(begin, points.len(), "every point is in a subpath");
            let nested: Vec<_> = flatten(&path, tolerance).into_iter().map(Some).collect();
            assert_eq!(flat, nested, "at tolerance {tolerance}");
        }
    }

    /// Every point a path's verbs carry, in order.
    fn points_of(path: &Path) -> Vec<Point> {
        let mut out = Vec::new();
        for verb in path.verbs() {
            match *verb {
                Verb::MoveTo(p) | Verb::LineTo(p) => out.push(p),
                Verb::QuadTo(c, p) => out.extend([c, p]),
                Verb::CurveTo(a, b, c) => out.extend([a, b, c]),
                Verb::Close => {}
            }
        }
        out
    }

    /// Whether two paths have the same verbs, each the same kind.
    fn same_shape(a: &Path, b: &Path) -> bool {
        a.verbs().len() == b.verbs().len()
            && a.verbs()
                .iter()
                .zip(b.verbs())
                .all(|(x, y)| std::mem::discriminant(x) == std::mem::discriminant(y))
    }

    /// The square from (x, y) to (x + side, y + side), its corners in the
    /// order given: `true` counter-clockwise in a y-up frame, positive area.
    fn square(path: &mut Path, x: f64, y: f64, side: f64, positive: bool) {
        let corners = [(x, y), (x + side, y), (x + side, y + side), (x, y + side)];
        let order: [usize; 4] = if positive { [0, 1, 2, 3] } else { [0, 3, 2, 1] };
        let (sx, sy) = corners[order[0]];
        path.move_to(sx, sy);
        for &i in &order[1..] {
            path.line_to(corners[i].0, corners[i].1);
        }
        path.close();
    }

    /// FT-04a.2: a rectangle's every corner moves out by each amount on its
    /// axis, so it grows by exactly twice each, at every whole size of the
    /// design's table from 8 to 16 pixels an em.
    #[test]
    fn an_axis_aligned_square_grows_by_twice_the_amount_per_axis() {
        for ppem in 8..=16 {
            let across = f64::from(24 - ppem) / 64.0;
            let down = f64::from(24 - ppem) / 128.0;
            let mut path = Path::new();
            path.rect(10.0, 20.0, 30.0, 40.0);
            let grown = path.offset(across, down);
            assert!(same_shape(&path, &grown), "the same verbs at {ppem}");
            assert_eq!(
                grown.bounds(),
                Some((10.0 - across, 20.0 - down, 40.0 + across, 60.0 + down)),
                "{ppem} pixels an em"
            );
            let expected = [
                (10.0 - across, 20.0 - down),
                (40.0 + across, 20.0 - down),
                (40.0 + across, 60.0 + down),
                (10.0 - across, 60.0 + down),
            ];
            let got: Vec<(f64, f64)> = points_of(&grown).iter().map(|p| (p.x, p.y)).collect();
            assert_eq!(got, expected, "each corner, exactly");
        }
        // The amounts are independent: across alone moves no y.
        let mut path = Path::new();
        path.rect(0.0, 0.0, 4.0, 2.0);
        assert_eq!(path.offset(0.5, 0.0).bounds(), Some((-0.5, 0.0, 4.5, 2.0)));
        assert_eq!(
            path.offset(0.0, 0.25).bounds(),
            Some((0.0, -0.25, 4.0, 2.25))
        );
        // A negative amount shrinks.
        assert_eq!(
            path.offset(-0.5, -0.25).bounds(),
            Some((0.5, 0.25, 3.5, 1.75))
        );
    }

    /// FT-04a.2: a counter, wound against its outer contour, shrinks by the
    /// amount its outer contour grows by, so a stem between them gains the
    /// amount on each side.
    #[test]
    fn a_counter_shrinks_as_its_outer_contour_grows() {
        let mut path = Path::new();
        square(&mut path, 0.0, 0.0, 10.0, true);
        square(&mut path, 3.0, 3.0, 4.0, false);
        let grown = path.offset(0.5, 0.25);
        assert!(same_shape(&path, &grown));
        let points = points_of(&grown);
        let (outer, counter) = points.split_at(4);
        for p in outer {
            assert!(p.x == -0.5 || p.x == 10.5, "{p:?}");
            assert!(p.y == -0.25 || p.y == 10.25, "{p:?}");
        }
        for p in counter {
            assert!(
                p.x == 3.5 || p.x == 6.5,
                "the counter's sides move in: {p:?}"
            );
            assert!(p.y == 3.25 || p.y == 6.75, "{p:?}");
        }
    }

    /// FT-04a.2: outward is the side that grows the path's total area, so a
    /// glyph drawn in either winding grows, and its counters shrink in
    /// either.
    #[test]
    fn either_winding_grows_outward() {
        for positive in [true, false] {
            let mut path = Path::new();
            square(&mut path, 0.0, 0.0, 10.0, positive);
            assert_eq!(
                path.offset(1.0, 0.5).bounds(),
                Some((-1.0, -0.5, 11.0, 10.5)),
                "wound {}",
                if positive { "positively" } else { "negatively" }
            );

            let mut glyph = Path::new();
            square(&mut glyph, 0.0, 0.0, 10.0, positive);
            square(&mut glyph, 3.0, 3.0, 4.0, !positive);
            let points = points_of(&glyph.offset(1.0, 0.5));
            for p in &points[4..] {
                assert!(p.x == 4.0 || p.x == 6.0, "{positive}: {p:?}");
                assert!(p.y == 3.5 || p.y == 6.5, "{positive}: {p:?}");
            }
        }
    }

    /// FT-04a.2: a zero-length edge keeps its point with its neighbours', a
    /// collinear point moves along its edge's normal, a turn straight back
    /// keeps its point, and a contour of one or two distinct points keeps
    /// them; nothing is a NaN.
    #[test]
    fn a_zero_length_edge_or_collinear_point_gives_no_nan() {
        let mut path = Path::new();
        path.move_to(0.0, 0.0);
        path.line_to(0.0, 0.0); // a zero-length edge
        path.line_to(5.0, 0.0); // collinear
        path.line_to(10.0, 0.0);
        path.line_to(10.0, 10.0);
        path.line_to(5.0, 10.0);
        path.line_to(5.0, 15.0); // up a spike
        path.line_to(5.0, 10.0); // and straight back
        path.line_to(0.0, 10.0);
        path.line_to(0.0, 0.0); // back to the start, written out
        path.close();
        path.move_to(20.0, 20.0); // a contour of one point
        path.move_to(30.0, 30.0); // and one of two
        path.line_to(40.0, 30.0);
        path.close();
        let grown = path.offset(0.5, 0.25);
        assert!(same_shape(&path, &grown));
        let points = points_of(&grown);
        assert!(points.iter().all(Point::is_finite), "{points:?}");
        let at = |i: usize| (points[i].x, points[i].y);
        assert_eq!(at(0), (-0.5, -0.25), "the corner");
        assert_eq!(at(1), (-0.5, -0.25), "its zero-length twin moves with it");
        assert_eq!(
            at(2),
            (5.0, -0.25),
            "a collinear point moves along the normal"
        );
        assert_eq!(at(3), (10.5, -0.25));
        assert_eq!(at(4), (10.5, 10.25));
        assert_eq!(at(5), (5.5, 10.25), "the spike's foot, on its right");
        assert_eq!(at(6), (5.0, 15.0), "a turn straight back keeps its point");
        assert_eq!(at(7), (4.5, 10.25), "the spike's foot, on its left");
        assert_eq!(at(8), (-0.5, 10.25));
        assert_eq!(at(9), (-0.5, -0.25), "the written-out return");
        assert_eq!(at(10), (20.0, 20.0), "one point keeps it");
        assert_eq!(at(11), (30.0, 30.0), "two points keep theirs");
        assert_eq!(at(12), (40.0, 30.0));
    }

    /// A subpath left open is offset as the closed contour a fill makes of
    /// it, and one begun after a `Close` with no `MoveTo` gains one at the
    /// point the fill begins it at.
    #[test]
    fn open_and_implicit_subpaths_are_offset_as_the_fill_reads_them() {
        let mut open = Path::new();
        open.move_to(0.0, 0.0);
        open.line_to(10.0, 0.0);
        open.line_to(10.0, 10.0);
        open.line_to(0.0, 10.0);
        let grown = open.offset(1.0, 1.0);
        assert!(same_shape(&open, &grown), "no Close is added");
        assert_eq!(grown.bounds(), Some((-1.0, -1.0, 11.0, 11.0)));

        let mut path = Path::new();
        path.rect(0.0, 0.0, 10.0, 10.0);
        path.line_to(20.0, 0.0);
        path.line_to(20.0, 10.0);
        path.line_to(15.0, 10.0);
        let grown = path.offset(1.0, 1.0);
        let kinds: Vec<&str> = grown
            .verbs()
            .iter()
            .map(|verb| match verb {
                Verb::MoveTo(_) => "move",
                Verb::LineTo(_) => "line",
                Verb::QuadTo(..) => "quad",
                Verb::CurveTo(..) => "curve",
                Verb::Close => "close",
            })
            .collect();
        assert_eq!(
            kinds,
            ["move", "line", "line", "line", "close", "move", "line", "line", "line"]
        );
        // Both flatten to polygons of the same points, so the fill reads one
        // shape: the second subpath begins where the first did.
        let flat = flatten(&grown, 0.1);
        assert_eq!(flat.len(), 2);
        assert!(flat.iter().flatten().all(Point::is_finite));
    }

    /// Amounts that are not finite or both zero, and a path with no area,
    /// come back as they went in.
    #[test]
    fn no_amount_or_no_area_gives_the_path_back() {
        let mut path = Path::new();
        path.rect(1.0, 2.0, 3.0, 4.0);
        for (across, down) in [
            (0.0, 0.0),
            (f64::NAN, 1.0),
            (1.0, f64::INFINITY),
            (f64::NEG_INFINITY, 0.0),
        ] {
            assert_eq!(
                path.offset(across, down).verbs(),
                path.verbs(),
                "{across} {down}"
            );
        }
        let mut line = Path::new();
        line.move_to(0.0, 0.0);
        line.line_to(10.0, 10.0);
        assert_eq!(line.offset(1.0, 1.0).verbs(), line.verbs(), "no area");
        assert!(Path::new().offset(1.0, 1.0).is_empty());
        let mut far = Path::new();
        far.rect(-1e308, -1e308, 1e308, 1e308);
        far.rect(1e308, 1e308, -1e308, -1e308);
        let back = far.offset(1.0, 1.0);
        assert!(points_of(&back).iter().all(Point::is_finite));
    }

    /// A curve's control points move with their contour: a circle of four
    /// cubics and one of four quadratics each grow, the on-curve points on the
    /// axes by exactly the amount.
    #[test]
    fn curves_move_their_control_points_with_their_contour() {
        let k = 0.552_284_75 * 10.0;
        let mut cubic = Path::new();
        cubic.move_to(10.0, 0.0);
        cubic.curve_to(10.0, k, k, 10.0, 0.0, 10.0);
        cubic.curve_to(-k, 10.0, -10.0, k, -10.0, 0.0);
        cubic.curve_to(-10.0, -k, -k, -10.0, 0.0, -10.0);
        cubic.curve_to(k, -10.0, 10.0, -k, 10.0, 0.0);
        cubic.close();
        let mut quad = Path::new();
        quad.move_to(10.0, 0.0);
        quad.quad_to(10.0, 10.0, 0.0, 10.0);
        quad.quad_to(-10.0, 10.0, -10.0, 0.0);
        quad.quad_to(-10.0, -10.0, 0.0, -10.0);
        quad.quad_to(10.0, -10.0, 10.0, 0.0);
        quad.close();
        for path in [cubic, quad] {
            let grown = path.offset(1.0, 1.0);
            assert!(same_shape(&path, &grown));
            let points = points_of(&grown);
            assert!(points.iter().all(Point::is_finite));
            assert_eq!(
                (points[0].x, points[0].y),
                (11.0, 0.0),
                "an on-curve point on the axis moves by the amount"
            );
            for (before, after) in points_of(&path).iter().zip(&points) {
                let d0 = before.x * before.x + before.y * before.y;
                let d1 = after.x * after.x + after.y * after.y;
                assert!(d1 > d0, "{before:?} moved out to {after:?}");
            }
        }
    }

    /// Ruling 1: generated paths of every verb, at every scale from the
    /// subnormal to the largest finite, with points repeated and amounts from
    /// zero to the largest finite, offset to paths whose every point is
    /// finite and whose verbs are at most one more a subpath.
    #[test]
    fn every_point_of_an_offset_stays_finite() {
        let mut state = 0x9E37_79B9_7F4A_7C15u64;
        let mut next = move || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state
        };
        let scales = [
            0.0,
            5e-324,
            1e-300,
            1e-6,
            1.0,
            37.5,
            1e3,
            1e150,
            1e300,
            f64::MAX,
        ];
        let amounts = [
            0.0,
            0.25,
            -0.25,
            0.125,
            1e-300,
            1e10,
            1e300,
            -f64::MAX,
            f64::MAX,
        ];
        for _ in 0..2_000 {
            let mut path = Path::new();
            let mut last = Point::new(0.0, 0.0);
            let verbs = (next() % 24) as usize;
            let point = |next: &mut dyn FnMut() -> u64, last: &mut Point| {
                if next() % 4 == 0 {
                    return *last;
                }
                let scale = scales[(next() % scales.len() as u64) as usize];
                let x = (next() % 2001) as f64 / 1000.0 - 1.0;
                let y = (next() % 2001) as f64 / 1000.0 - 1.0;
                let p = Point::new(x * scale, y * scale);
                *last = p;
                p
            };
            for _ in 0..verbs {
                match next() % 6 {
                    0 => {
                        let p = point(&mut next, &mut last);
                        path.move_to(p.x, p.y);
                    }
                    1 | 2 => {
                        let p = point(&mut next, &mut last);
                        path.line_to(p.x, p.y);
                    }
                    3 => {
                        let c = point(&mut next, &mut last);
                        let p = point(&mut next, &mut last);
                        path.quad_to(c.x, c.y, p.x, p.y);
                    }
                    4 => {
                        let a = point(&mut next, &mut last);
                        let b = point(&mut next, &mut last);
                        let p = point(&mut next, &mut last);
                        path.curve_to(a.x, a.y, b.x, b.y, p.x, p.y);
                    }
                    _ => path.close(),
                }
            }
            let across = amounts[(next() % amounts.len() as u64) as usize];
            let down = amounts[(next() % amounts.len() as u64) as usize];
            let grown = path.offset(across, down);
            let points = points_of(&grown);
            assert!(
                points.iter().all(Point::is_finite),
                "{path:?} by ({across}, {down}) gave {grown:?}"
            );
            let subpaths = path
                .verbs()
                .iter()
                .filter(|verb| matches!(verb, Verb::MoveTo(_) | Verb::Close))
                .count();
            assert!(grown.verbs().len() <= path.verbs().len() + subpaths + 1);
        }
    }
}
