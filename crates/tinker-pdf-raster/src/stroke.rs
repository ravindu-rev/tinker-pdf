//! Stroking: turning a path and a pen into an outline to fill.
//!
//! Expanding a stroke into a fillable path rather than rasterizing it directly
//! means one rasterizer serves both, so a stroked edge and a filled one
//! anti-alias identically — which is what stops strokes looking subtly
//! different from fills at the same position.

use tinker_pdf_math as math;

use crate::geom::{flatten, Path, Point};

/// How a stroke's ends are finished (8.4.3.3, Table 54).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LineCap {
    /// 0: cut off square at the endpoint.
    #[default]
    Butt,
    /// 1: a semicircle beyond the endpoint.
    Round,
    /// 2: a square extending half the width beyond.
    Square,
}

/// How corners are finished (8.4.3.4, Table 55).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LineJoin {
    /// 0: extended to a point, unless the miter limit is exceeded.
    #[default]
    Miter,
    /// 1: rounded.
    Round,
    /// 2: cut off square.
    Bevel,
}

/// The pen.
#[derive(Clone, Debug)]
pub struct StrokeStyle {
    /// Line width in device units.
    pub width: f64,
    /// End treatment.
    pub cap: LineCap,
    /// Corner treatment.
    pub join: LineJoin,
    /// 8.4.3.5: beyond this ratio a miter becomes a bevel.
    pub miter_limit: f64,
    /// Dash array in device units; empty means solid.
    pub dashes: Vec<f64>,
    /// Distance into the dash pattern at which to start.
    pub dash_phase: f64,
}

impl Default for StrokeStyle {
    fn default() -> Self {
        StrokeStyle {
            width: 1.0,
            cap: LineCap::default(),
            join: LineJoin::default(),
            miter_limit: 10.0,
            dashes: Vec::new(),
            dash_phase: 0.0,
        }
    }
}

/// How finely round joins and caps are approximated.
const ARC_STEPS: usize = 16;

/// Dash steps between one cancellation check and the next.
///
/// A step is a handful of arithmetic operations and a push, and one segment
/// may take a hundred thousand of them, so a thousand steps is microseconds
/// between one answer and the next. Like [`crate::fill`]'s row constant this
/// number cannot change the outline: the predicate decides only whether the
/// expansion continues.
const STOP_EVERY: u32 = 1024;

/// Expands a stroke into a path to fill with the non-zero rule.
///
/// The outline is built per segment — a quad for the body, a shape for each
/// join and cap — rather than by offsetting the whole path. Overlapping quads
/// are exactly what the non-zero rule resolves, and the alternative (a true
/// offset curve) is where stroking implementations go to die on self-
/// intersection.
///
/// `stop` is asked once per subpath and once every [`STOP_EVERY`] dash steps,
/// and what comes back when it answers `true` is the partial outline. Dash
/// expansion is where this matters: a long path under a fine dash array runs
/// to completion *before* any fill starts, so a stroke with no hook here is
/// uninterruptible however promptly the fill checks. A solid stroke never
/// reaches the inner loop at all — 8.4.3.6's empty array returns the polyline
/// unchanged — so it pays only the per-subpath question. `None` is the whole
/// of the previous behaviour.
#[must_use]
pub fn stroke(
    path: &Path,
    style: &StrokeStyle,
    tolerance: f64,
    stop: Option<&dyn Fn() -> bool>,
) -> Path {
    let mut out = Path::new();

    let width = if style.width.is_finite() && style.width > 0.0 {
        style.width
    } else {
        // 8.4.3.2: a width of zero means the thinnest line the device can
        // render, which for a rasterizer is one pixel.
        1.0
    };
    let radius = width / 2.0;

    for poly in flatten(path, tolerance) {
        if stop.is_some_and(|stop| stop()) {
            return out;
        }
        for piece in apply_dashes(&poly, style, stop) {
            stroke_polyline(&piece, radius, style, &mut out);
        }
    }

    out
}

/// Splits a polyline into the pieces a dash pattern leaves visible.
fn apply_dashes(
    poly: &[Point],
    style: &StrokeStyle,
    stop: Option<&dyn Fn() -> bool>,
) -> Vec<Vec<Point>> {
    let pattern: Vec<f64> = style
        .dashes
        .iter()
        .copied()
        .filter(|d| d.is_finite() && *d >= 0.0)
        .collect();
    // 8.4.3.6: an empty array, or one summing to zero, is a solid line.
    if pattern.is_empty() || pattern.iter().sum::<f64>() <= 0.0 {
        return vec![poly.to_vec()];
    }

    let mut pieces = Vec::new();
    let mut current: Vec<Point> = Vec::new();

    // Where in the pattern the phase starts.
    let total: f64 = pattern.iter().sum();
    let mut remaining_phase = if style.dash_phase.is_finite() {
        style.dash_phase.rem_euclid(total)
    } else {
        0.0
    };
    let mut index = 0usize;
    while remaining_phase > 0.0 {
        let step = pattern.get(index % pattern.len()).copied().unwrap_or(0.0);
        if remaining_phase < step {
            break;
        }
        remaining_phase -= step;
        index += 1;
    }
    let mut left = pattern.get(index % pattern.len()).copied().unwrap_or(0.0) - remaining_phase;
    let mut on = index % 2 == 0;

    if on {
        if let Some(first) = poly.first() {
            current.push(*first);
        }
    }

    // Counted across the whole polyline rather than per segment, so a path of
    // ten thousand short segments is asked as often as one long segment that
    // takes the same number of steps.
    let mut steps = 0u32;

    for pair in poly.windows(2) {
        let (Some(&a), Some(&b)) = (pair.first(), pair.get(1)) else {
            continue;
        };
        let mut segment_left = distance(a, b);
        if segment_left <= 0.0 {
            continue;
        }
        let (dx, dy) = ((b.x - a.x) / segment_left, (b.y - a.y) / segment_left);
        let mut at = a;

        // Bound the number of dashes one segment can produce: a tiny pattern
        // over a long line would otherwise generate millions of pieces.
        let mut guard = 0u32;
        while segment_left > 0.0 && guard < 100_000 {
            if steps % STOP_EVERY == 0 && stop.is_some_and(|stop| stop()) {
                // The pieces already measured, and the one in hand, rather
                // than nothing: the same partial answer `fill` gives.
                if current.len() > 1 {
                    pieces.push(current);
                }
                return pieces;
            }
            steps = steps.wrapping_add(1);
            guard += 1;
            if left <= 0.0 {
                index += 1;
                left = pattern.get(index % pattern.len()).copied().unwrap_or(0.0);
                on = index % 2 == 0;
                if on {
                    current.push(at);
                } else if current.len() > 1 {
                    pieces.push(std::mem::take(&mut current));
                } else {
                    current.clear();
                }
                if left <= 0.0 {
                    // A zero-length entry would spin; treat it as consumed.
                    continue;
                }
            }

            let step = left.min(segment_left);
            let next = Point::new(at.x + dx * step, at.y + dy * step);
            if on {
                current.push(next);
            }
            at = next;
            left -= step;
            segment_left -= step;
        }
    }

    if current.len() > 1 {
        pieces.push(current);
    }
    pieces
}

fn stroke_polyline(poly: &[Point], radius: f64, style: &StrokeStyle, out: &mut Path) {
    if poly.len() < 2 {
        // 8.4.3.3: a degenerate subpath draws a dot under a round cap, and
        // nothing at all under a butt cap.
        if let (Some(&p), LineCap::Round) = (poly.first(), style.cap) {
            circle(p, radius, out);
        } else if let (Some(&p), LineCap::Square) = (poly.first(), style.cap) {
            emit(
                &[
                    Point::new(p.x - radius, p.y - radius),
                    Point::new(p.x + radius, p.y - radius),
                    Point::new(p.x + radius, p.y + radius),
                    Point::new(p.x - radius, p.y + radius),
                ],
                out,
            );
        }
        return;
    }

    let closed = poly.first() == poly.last() && poly.len() > 2;

    for pair in poly.windows(2) {
        let (Some(&a), Some(&b)) = (pair.first(), pair.get(1)) else {
            continue;
        };
        let len = distance(a, b);
        if len <= 0.0 {
            continue;
        }
        let (nx, ny) = (-(b.y - a.y) / len * radius, (b.x - a.x) / len * radius);

        emit(
            &[
                Point::new(a.x + nx, a.y + ny),
                Point::new(b.x + nx, b.y + ny),
                Point::new(b.x - nx, b.y - ny),
                Point::new(a.x - nx, a.y - ny),
            ],
            out,
        );
    }

    // Joins at every interior vertex.
    for window in poly.windows(3) {
        if let (Some(&a), Some(&b), Some(&c)) = (window.first(), window.get(1), window.get(2)) {
            join(a, b, c, radius, style, out);
        }
    }
    if closed {
        // The wrap-around corner, which windows(3) cannot see.
        if let (Some(&last_but_one), Some(&first), Some(&second)) = (
            poly.get(poly.len().wrapping_sub(2)),
            poly.first(),
            poly.get(1),
        ) {
            join(last_but_one, first, second, radius, style, out);
        }
    }

    if !closed {
        if let (Some(&first), Some(&second)) = (poly.first(), poly.get(1)) {
            cap(first, second, radius, style.cap, out);
        }
        if let (Some(&last), Some(&before)) = (poly.last(), poly.get(poly.len().wrapping_sub(2))) {
            cap(last, before, radius, style.cap, out);
        }
    }
}

fn join(a: Point, b: Point, c: Point, radius: f64, style: &StrokeStyle, out: &mut Path) {
    match style.join {
        LineJoin::Round => circle(b, radius, out),
        LineJoin::Bevel => bevel(a, b, c, radius, out),
        LineJoin::Miter => {
            let (Some(u), Some(v)) = (unit(a, b), unit(b, c)) else {
                return;
            };
            // 8.4.3.5: the miter length over the line width is
            // 1/sin(theta/2); compare against the limit before drawing.
            let cos_theta = (-u.0 * v.0 - u.1 * v.1).clamp(-1.0, 1.0);
            let half = ((1.0 - cos_theta) / 2.0).max(0.0).sqrt();
            if half <= f64::EPSILON || 1.0 / half > style.miter_limit {
                bevel(a, b, c, radius, out);
                return;
            }

            // The miter tip lies along the bisector of the outer corner.
            let bisector = (u.0 - v.0, u.1 - v.1);
            let length = (bisector.0 * bisector.0 + bisector.1 * bisector.1).sqrt();
            if length <= f64::EPSILON {
                bevel(a, b, c, radius, out);
                return;
            }
            let miter = radius / half;
            // Outward along the exterior bisector: `u - v` points away from
            // the corner, which is where a miter's tip belongs. Negating it
            // folds the tip back inside the join, where it adds nothing.
            let tip = Point::new(
                b.x + bisector.0 / length * miter,
                b.y + bisector.1 / length * miter,
            );

            let (n1x, n1y) = (-u.1 * radius, u.0 * radius);
            let (n2x, n2y) = (-v.1 * radius, v.0 * radius);
            // Both offsets of the corner plus the tip: filled non-zero, this
            // is the miter whichever side the turn is on.
            emit(
                &[
                    Point::new(b.x + n1x, b.y + n1y),
                    tip,
                    Point::new(b.x + n2x, b.y + n2y),
                    b,
                ],
                out,
            );
            emit(
                &[
                    Point::new(b.x - n1x, b.y - n1y),
                    tip,
                    Point::new(b.x - n2x, b.y - n2y),
                    b,
                ],
                out,
            );
        }
    }
}

fn bevel(a: Point, b: Point, c: Point, radius: f64, out: &mut Path) {
    let (Some(u), Some(v)) = (unit(a, b), unit(b, c)) else {
        return;
    };
    let (n1x, n1y) = (-u.1 * radius, u.0 * radius);
    let (n2x, n2y) = (-v.1 * radius, v.0 * radius);

    emit(
        &[
            Point::new(b.x + n1x, b.y + n1y),
            Point::new(b.x + n2x, b.y + n2y),
            b,
        ],
        out,
    );
    emit(
        &[
            Point::new(b.x - n1x, b.y - n1y),
            Point::new(b.x - n2x, b.y - n2y),
            b,
        ],
        out,
    );
}

fn cap(end: Point, toward: Point, radius: f64, cap: LineCap, out: &mut Path) {
    match cap {
        LineCap::Butt => {}
        LineCap::Round => circle(end, radius, out),
        LineCap::Square => {
            let Some((dx, dy)) = unit(toward, end) else {
                return;
            };
            let (nx, ny) = (-dy * radius, dx * radius);
            let (ex, ey) = (dx * radius, dy * radius);
            emit(
                &[
                    Point::new(end.x + nx, end.y + ny),
                    Point::new(end.x + nx + ex, end.y + ny + ey),
                    Point::new(end.x - nx + ex, end.y - ny + ey),
                    Point::new(end.x - nx, end.y - ny),
                ],
                out,
            );
        }
    }
}

/// Emits a polygon with a consistent orientation.
///
/// Every piece of a stroke outline — segment quads, joins, caps — is filled
/// together under the non-zero rule, and **two overlapping pieces wound in
/// opposite directions cancel to nothing**. Round caps vanishing entirely is
/// what that looks like, so orientation is normalized here rather than trusted
/// from each construction site.
fn emit(points: &[Point], out: &mut Path) {
    if points.len() < 3 {
        return;
    }
    if points.iter().any(|p| !p.is_finite()) {
        return;
    }

    // Twice the signed area; negative is the orientation the segment quads
    // produce, so match it.
    let mut area = 0.0;
    for i in 0..points.len() {
        let (Some(a), Some(b)) = (points.get(i), points.get((i + 1) % points.len())) else {
            return;
        };
        area += a.x * b.y - b.x * a.y;
    }

    let ordered: Vec<Point> = if area > 0.0 {
        points.iter().rev().copied().collect()
    } else {
        points.to_vec()
    };

    for (i, p) in ordered.iter().enumerate() {
        if i == 0 {
            out.move_to(p.x, p.y);
        } else {
            out.line_to(p.x, p.y);
        }
    }
    out.close();
}

fn circle(center: Point, radius: f64, out: &mut Path) {
    if !center.is_finite() || !radius.is_finite() || radius <= 0.0 {
        return;
    }
    let points: Vec<Point> = (0..ARC_STEPS)
        .map(|step| {
            let angle = (step as f64) / (ARC_STEPS as f64) * std::f64::consts::TAU;
            Point::new(
                center.x + radius * math::cos(angle),
                center.y + radius * math::sin(angle),
            )
        })
        .collect();
    emit(&points, out);
}

fn unit(from: Point, to: Point) -> Option<(f64, f64)> {
    let len = distance(from, to);
    (len > 0.0 && len.is_finite()).then(|| ((to.x - from.x) / len, (to.y - from.y) / len))
}

fn distance(a: Point, b: Point) -> f64 {
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    (dx * dx + dy * dy).sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fill::fill;
    use crate::geom::FillRule;

    fn coverage(path: &Path, w: u32, h: u32) -> f64 {
        let mask = fill(path, FillRule::NonZero, 0, 0, w, h, 0.05, None);
        mask.data.iter().map(|&v| f64::from(v)).sum::<f64>() / 255.0
    }

    #[test]
    fn a_horizontal_line_strokes_to_its_width_times_its_length() {
        let mut path = Path::new();
        path.move_to(2.0, 10.0);
        path.line_to(18.0, 10.0);

        let outline = stroke(
            &path,
            &StrokeStyle {
                width: 4.0,
                ..StrokeStyle::default()
            },
            0.05,
            None,
        );
        let area = coverage(&outline, 24, 24);
        // 16 long, 4 wide, butt caps: 64 square units.
        assert!(
            (60.0..68.0).contains(&area),
            "expected about 64, got {area}"
        );
    }

    #[test]
    fn square_caps_extend_the_line_by_half_a_width_at_each_end() {
        let mut path = Path::new();
        path.move_to(4.0, 10.0);
        path.line_to(16.0, 10.0);

        let butt = stroke(
            &path,
            &StrokeStyle {
                width: 4.0,
                cap: LineCap::Butt,
                ..StrokeStyle::default()
            },
            0.05,
            None,
        );
        let square = stroke(
            &path,
            &StrokeStyle {
                width: 4.0,
                cap: LineCap::Square,
                ..StrokeStyle::default()
            },
            0.05,
            None,
        );

        let extra = coverage(&square, 24, 24) - coverage(&butt, 24, 24);
        // Two square caps of 2 by 4.
        assert!(
            (13.0..19.0).contains(&extra),
            "square caps should add about 16, got {extra}"
        );
    }

    #[test]
    fn round_caps_add_a_disc_worth_of_area() {
        let mut path = Path::new();
        path.move_to(6.0, 12.0);
        path.line_to(18.0, 12.0);

        let butt = coverage(
            &stroke(
                &path,
                &StrokeStyle {
                    width: 6.0,
                    cap: LineCap::Butt,
                    ..StrokeStyle::default()
                },
                0.05,
                None,
            ),
            28,
            28,
        );
        let round = coverage(
            &stroke(
                &path,
                &StrokeStyle {
                    width: 6.0,
                    cap: LineCap::Round,
                    ..StrokeStyle::default()
                },
                0.05,
                None,
            ),
            28,
            28,
        );

        // Two half-discs of radius 3 make one circle: about 28.3.
        let extra = round - butt;
        assert!(
            (24.0..33.0).contains(&extra),
            "round caps should add about 28, got {extra}"
        );
    }

    #[test]
    fn a_dash_pattern_removes_area() {
        let mut path = Path::new();
        path.move_to(0.0, 10.0);
        path.line_to(40.0, 10.0);

        let solid = coverage(
            &stroke(
                &path,
                &StrokeStyle {
                    width: 2.0,
                    ..StrokeStyle::default()
                },
                0.05,
                None,
            ),
            44,
            20,
        );
        let dashed = coverage(
            &stroke(
                &path,
                &StrokeStyle {
                    width: 2.0,
                    dashes: vec![4.0, 4.0],
                    ..StrokeStyle::default()
                },
                0.05,
                None,
            ),
            44,
            20,
        );

        assert!(
            dashed < solid * 0.65,
            "an even on-off dash should roughly halve the ink: {dashed} vs {solid}"
        );
        assert!(dashed > solid * 0.35, "but not remove all of it");
    }

    #[test]
    fn the_miter_limit_turns_a_sharp_corner_into_a_bevel() {
        // A very sharp turn: the miter would extend far past the corner.
        let mut path = Path::new();
        path.move_to(2.0, 2.0);
        path.line_to(20.0, 3.0);
        path.line_to(2.0, 4.0);

        let generous = coverage(
            &stroke(
                &path,
                &StrokeStyle {
                    width: 2.0,
                    join: LineJoin::Miter,
                    miter_limit: 100.0,
                    ..StrokeStyle::default()
                },
                0.05,
                None,
            ),
            40,
            12,
        );
        let limited = coverage(
            &stroke(
                &path,
                &StrokeStyle {
                    width: 2.0,
                    join: LineJoin::Miter,
                    miter_limit: 1.5,
                    ..StrokeStyle::default()
                },
                0.05,
                None,
            ),
            40,
            12,
        );

        assert!(
            limited < generous,
            "a low limit clips the spike: {limited} vs {generous}"
        );
    }

    #[test]
    fn a_zero_width_line_still_draws() {
        let mut path = Path::new();
        path.move_to(0.0, 5.0);
        path.line_to(10.0, 5.0);
        // 8.4.3.2: zero means the thinnest renderable line, not nothing.
        let outline = stroke(
            &path,
            &StrokeStyle {
                width: 0.0,
                ..StrokeStyle::default()
            },
            0.05,
            None,
        );
        assert!(coverage(&outline, 12, 12) > 5.0);
    }

    #[test]
    fn degenerate_input_does_not_hang() {
        let mut dot = Path::new();
        dot.move_to(5.0, 5.0);

        // A lone point is a dot under a round cap and nothing under a butt.
        let round = stroke(
            &dot,
            &StrokeStyle {
                width: 4.0,
                cap: LineCap::Round,
                ..StrokeStyle::default()
            },
            0.05,
            None,
        );
        assert!(coverage(&round, 12, 12) > 8.0);

        // Pathological dash patterns must terminate.
        let mut line = Path::new();
        line.move_to(0.0, 0.0);
        line.line_to(1000.0, 0.0);
        for dashes in [vec![0.0, 0.0], vec![0.0], vec![f64::NAN], vec![1e-9, 1e-9]] {
            let _ = stroke(
                &line,
                &StrokeStyle {
                    width: 1.0,
                    dashes,
                    ..StrokeStyle::default()
                },
                0.05,
                None,
            );
        }
    }

    // -----------------------------------------------------------------------
    // Stopping the expansion (gap 15).
    // -----------------------------------------------------------------------

    /// A predicate that answers `false` until its `at`-th question and `true`
    /// from then on, counting how many times it was asked. Deterministic: it
    /// fires on the Nth question, never on a clock.
    struct StopAt {
        at: u32,
        calls: std::cell::Cell<u32>,
    }

    impl StopAt {
        fn new(at: u32) -> StopAt {
            StopAt {
                at,
                calls: std::cell::Cell::new(0),
            }
        }

        fn ask(&self) -> bool {
            self.calls.set(self.calls.get().saturating_add(1));
            self.calls.get() >= self.at
        }
    }

    /// A long line under a fine dash array: the case the gap plan names, where
    /// the whole expansion runs before a single row is filled.
    fn dashed_rule() -> (Path, StrokeStyle) {
        let mut path = Path::new();
        path.move_to(0.0, 0.0);
        path.line_to(200_000.0, 0.0);
        (
            path,
            StrokeStyle {
                width: 1.0,
                dashes: vec![1.0, 1.0],
                ..StrokeStyle::default()
            },
        )
    }

    /// Milestone 2: the dash loop stops when asked, and hands back the pieces
    /// it had already measured rather than nothing.
    ///
    /// The first question is the subpath's, asked before the expansion starts;
    /// the second is the expansion's own at step zero; the third comes
    /// `STOP_EVERY` steps later and is the one that fires here. So the outline
    /// carries about a thousand dashes out of the fifty thousand the whole
    /// expansion produces — enough to prove both that it stopped and that it
    /// did not throw away what it had.
    #[test]
    fn a_stopped_dash_expansion_keeps_the_pieces_it_had_measured() {
        let (path, style) = dashed_rule();
        let whole = stroke(&path, &style, 0.05, None);

        let stop = StopAt::new(3);
        let ask = || stop.ask();
        let partial = stroke(&path, &style, 0.05, Some(&ask));

        assert!(
            !partial.verbs().is_empty(),
            "the dashes already measured, not an empty path"
        );
        assert!(
            partial.verbs().len() * 10 < whole.verbs().len(),
            "and far short of the whole: {} against {}",
            partial.verbs().len(),
            whole.verbs().len()
        );
    }

    /// A solid stroke never enters the dash loop — 8.4.3.6's empty array
    /// returns the polyline unchanged — so the per-subpath question is the
    /// only one it can be stopped by. Four identical subpaths, stopped at the
    /// third question, leave exactly two.
    #[test]
    fn a_solid_stroke_stops_between_subpaths() {
        let mut path = Path::new();
        for i in 0..4 {
            let y = f64::from(i) * 10.0;
            path.move_to(0.0, y);
            path.line_to(40.0, y);
        }
        let style = StrokeStyle {
            width: 2.0,
            ..StrokeStyle::default()
        };

        let whole = stroke(&path, &style, 0.05, None);
        let stop = StopAt::new(3);
        let ask = || stop.ask();
        let partial = stroke(&path, &style, 0.05, Some(&ask));

        assert_eq!(
            partial.verbs().len() * 2,
            whole.verbs().len(),
            "two of the four subpaths"
        );
    }

    /// The hook is not an input to the outline: a predicate that never answers
    /// yes leaves the path exactly where `None` left it (ruling 4).
    #[test]
    fn a_predicate_that_never_answers_yes_changes_no_outline() {
        let (path, style) = dashed_rule();
        let stop = StopAt::new(u32::MAX);
        let ask = || stop.ask();

        let hooked = stroke(&path, &style, 0.05, Some(&ask));
        let bare = stroke(&path, &style, 0.05, None);

        assert_eq!(hooked.verbs(), bare.verbs(), "the same verbs");
        assert!(
            stop.calls.get() < 200,
            "one question per subpath and per band of {STOP_EVERY} steps, \
             not one per step: {}",
            stop.calls.get()
        );
    }
}
