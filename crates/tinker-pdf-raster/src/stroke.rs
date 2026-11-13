//! Stroking: turning a path and a pen into an outline to fill.
//!
//! Expanding a stroke into a fillable outline rather than rasterizing it
//! directly means one rasterizer serves both, so a stroked edge and a filled
//! one anti-alias identically — which is what stops strokes looking subtly
//! different from fills at the same position.
//!
//! The outline is written piece by piece through a sink: into a [`Path`] for
//! a caller that wants the outline ([`stroke`], [`stroke_mapped`]), or
//! straight into a [`FillScratch`]'s edges for one that wants its coverage
//! ([`stroke_with`], [`stroke_mapped_with`], then
//! [`crate::fill::fill_outline`]), where it is never a path and never
//! flattened a second time.

use tinker_pdf_math as math;

use crate::fill::{FillScratch, Outline};
use crate::geom::{flatten, flatten_mapped, Path, Point};
use crate::image::Transform;

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
    /// Line width, in the units of the path it strokes: device units for
    /// [`stroke`], the pen's own space for [`stroke_mapped`].
    pub width: f64,
    /// End treatment.
    pub cap: LineCap,
    /// Corner treatment.
    pub join: LineJoin,
    /// 8.4.3.5: beyond this ratio a miter becomes a bevel.
    pub miter_limit: f64,
    /// Dash array, in the same units as the width; empty means solid.
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
    outline(path, style, tolerance, stop, &mut out);
    out
}

/// [`stroke`], with the outline written into `scratch` as the edges of a fill
/// rather than returned as a path: [`crate::fill::fill_outline`] then fills
/// it, to the mask [`crate::fill::fill`] gives the path [`stroke`] returns,
/// byte for byte, and [`FillScratch::outline_bounds`] gives that path's
/// bounds.
///
/// The path is flattened once, here, and the outline not at all: each piece
/// becomes edges as it is made. The outline replaces the one the last stroke
/// wrote into `scratch`; `stop` is asked as [`stroke`] asks it, and a stop
/// leaves the partial outline.
pub fn stroke_with(
    scratch: &mut FillScratch,
    path: &Path,
    style: &StrokeStyle,
    tolerance: f64,
    stop: Option<&dyn Fn() -> bool>,
) {
    outline(path, style, tolerance, stop, scratch.begin_outline());
}

/// The walk [`stroke`] and [`stroke_with`] share, writing every piece into
/// `out`.
fn outline(
    path: &Path,
    style: &StrokeStyle,
    tolerance: f64,
    stop: Option<&dyn Fn() -> bool>,
    out: &mut dyn Pieces,
) {
    let width = if style.width.is_finite() && style.width > 0.0 {
        style.width
    } else {
        // 8.4.3.2: a width of zero means the thinnest line the device can
        // render, which for a rasterizer is one pixel.
        1.0
    };
    let radius = width / 2.0;

    let mut sink = Sink { out, map: None };
    for poly in flatten(path, tolerance) {
        if stop.is_some_and(|stop| stop()) {
            return;
        }
        for piece in apply_dashes(&poly, style, stop) {
            stroke_polyline(&piece, radius, style, &mut sink);
        }
    }
}

/// Expands a stroke whose pen lives in another space from the device's: the
/// path, the width and the dashes are all in the pen's space, and `map` takes
/// that space to the device.
///
/// This is 8.4.3.2 read literally — the line width, and with it every cap,
/// join and dash, is measured in user space — for a map that is not a
/// similarity, where no single device width can say it: under `scale(1, 3)` a
/// pen two units wide covers six device units where the path runs across the
/// stretch and two where it runs along it. The outline is built in the pen's
/// space, exactly as [`stroke`] builds one, and every piece is carried
/// through `map` on its way out. For a similarity the two are the same
/// outline, and [`stroke`] is the cheaper way to it.
///
/// **`floor` is in device units**, because 8.4.3.2's thinnest line is the
/// device's: wherever the mapped pen is thinner than `floor` — every
/// direction of a zero width, the squeezed directions of a thin one — the
/// outline also carries the same dashed pieces mapped first and stroked at
/// `floor`, so the union is at least that wide in every direction and exactly
/// the pen where the pen is wider. A width of zero is the floor alone.
///
/// `tolerance` is in device units too, read as [`stroke`] reads it; the path
/// is flattened in the pen's space at `tolerance` over the map's largest
/// stretch, so a chord is never further than `tolerance` from its curve once
/// mapped — however far below a millionth that quotient is, since it is a
/// length in the pen's space and not the device's. A map that is not finite,
/// or has no stretch at all, outlines nothing. `stop` is asked as [`stroke`]
/// asks it.
#[must_use]
pub fn stroke_mapped(
    path: &Path,
    style: &StrokeStyle,
    map: &Transform,
    floor: f64,
    tolerance: f64,
    stop: Option<&dyn Fn() -> bool>,
) -> Path {
    let mut out = Path::new();
    outline_mapped(path, style, map, floor, tolerance, stop, &mut out);
    out
}

/// [`stroke_mapped`], with the outline written into `scratch` as the edges of
/// a fill, as [`stroke_with`] writes [`stroke`]'s: the path is flattened once,
/// in the pen's space, and neither the pen's pieces nor the floor's are
/// flattened again.
pub fn stroke_mapped_with(
    scratch: &mut FillScratch,
    path: &Path,
    style: &StrokeStyle,
    map: &Transform,
    floor: f64,
    tolerance: f64,
    stop: Option<&dyn Fn() -> bool>,
) {
    outline_mapped(
        path,
        style,
        map,
        floor,
        tolerance,
        stop,
        scratch.begin_outline(),
    );
}

/// The walk [`stroke_mapped`] and [`stroke_mapped_with`] share, writing every
/// piece into `out`.
fn outline_mapped(
    path: &Path,
    style: &StrokeStyle,
    map: &Transform,
    floor: f64,
    tolerance: f64,
    stop: Option<&dyn Fn() -> bool>,
    out: &mut dyn Pieces,
) {
    let (largest, smallest) = stretches(map);
    if !(largest.is_finite() && largest > 0.0) {
        return;
    }
    let width = if style.width.is_finite() && style.width > 0.0 {
        style.width
    } else {
        0.0
    };
    let floor = if floor.is_finite() && floor > 0.0 {
        floor
    } else {
        0.0
    };
    // Whether some direction of the mapped pen is thinner than the floor:
    // the pen's narrowest image is its width times the smallest stretch.
    let floored = floor > 0.0 && width * smallest < floor;

    // The device tolerance read as `flatten` reads one, then carried into
    // the pen's space, where `flatten`'s floor for a device tolerance does
    // not apply: under a stretch of 400 000, 0.2 is half a millionth there.
    let tolerance = if tolerance.is_finite() && tolerance > 1e-6 {
        tolerance
    } else {
        0.1
    };
    for poly in flatten_mapped(path, tolerance / largest) {
        if stop.is_some_and(|stop| stop()) {
            return;
        }
        for piece in apply_dashes(&poly, style, stop) {
            if width > 0.0 {
                let mut sink = Sink {
                    out: &mut *out,
                    map: Some(map),
                };
                stroke_polyline(&piece, width / 2.0, style, &mut sink);
            }
            if floored {
                let mapped: Vec<Point> = piece
                    .iter()
                    .map(|p| {
                        let (x, y) = map.apply(p.x, p.y);
                        Point::new(x, y)
                    })
                    .collect();
                let mut sink = Sink {
                    out: &mut *out,
                    map: None,
                };
                stroke_polyline(&mapped, floor / 2.0, style, &mut sink);
            }
        }
    }
}

/// The pieces a dash pattern leaves of a path, as polylines in the path's
/// own space: 8.4.3.6's dashing without the pen, for a writer that has to
/// state a dashed line some other way than with a dash array.
///
/// `dashes` and `phase` are in the path's units; an empty pattern, or one
/// summing to zero, leaves each flattened subpath whole. The path is
/// flattened at `tolerance`, in the path's units too and honoured however
/// small, since a caller carrying the pieces through a large stretch divides
/// a device tolerance by it; and the same 100 000-step bound per segment
/// [`stroke`] keeps applies.
///
/// **Each piece is handed to `piece` as it is cut**, never collected: the
/// bound is per segment, so the pieces of a short path under a fine pattern
/// run to millions — forty nine-byte segments of `[0.01 0.01]` are two
/// million — and a caller writing them somewhere bounded stops the cutting
/// by answering `false`, having paid only for what it kept. Returns whether
/// every piece was handed over.
///
/// A pattern whose every dash has no length — `[0 0.01]` — leaves no piece,
/// a dash of no length being a single point, and that is answered without
/// walking the path: otherwise the walk spends its bound on every segment
/// with nothing handed over that a caller could stop it on. A dash of no
/// length among others is taken out before the walk, its gaps joined, so
/// every dash the walk steps through has length and a piece is handed over
/// every other step, besides the segments a dash or a gap crosses.
pub fn dash(
    path: &Path,
    dashes: &[f64],
    phase: f64,
    tolerance: f64,
    piece: &mut dyn FnMut(&[Point]) -> bool,
) -> bool {
    let style = StrokeStyle {
        dashes: dashes.to_vec(),
        dash_phase: phase,
        ..StrokeStyle::default()
    };
    for poly in flatten_mapped(path, tolerance) {
        if !each_dash(&poly, &style, None, &mut |cut| piece(&cut)) {
            return false;
        }
    }
    true
}

/// The largest and smallest factor by which `map` stretches a length — its
/// two singular values — or zeros for a map that is not finite.
///
/// With `E`, `F`, `G`, `H` the halves of the sums and differences of the
/// diagonal and off-diagonal entries, `Q = hypot(E, H)` and
/// `R = hypot(F, G)`, the two are `Q + R` and `|Q − R|`: `R` vanishes for a
/// rotation and scale, `Q` for a reflection and scale, which is why their
/// difference measures how far a map is from a similarity. Only `sqrt`, which
/// IEEE 754 rounds exactly, is used (ruling 4).
#[must_use]
pub fn stretches(map: &Transform) -> (f64, f64) {
    let (a, b, c, d) = (map.a, map.b, map.c, map.d);
    if ![a, b, c, d].iter().all(|v| v.is_finite()) {
        return (0.0, 0.0);
    }
    let (e, f) = ((a + d) / 2.0, (a - d) / 2.0);
    let (g, h) = ((b + c) / 2.0, (b - c) / 2.0);
    let q = (e * e + h * h).sqrt();
    let r = (f * f + g * g).sqrt();
    (q + r, (q - r).abs())
}

/// Splits a polyline into the pieces a dash pattern leaves visible.
fn apply_dashes(
    poly: &[Point],
    style: &StrokeStyle,
    stop: Option<&dyn Fn() -> bool>,
) -> Vec<Vec<Point>> {
    let mut pieces = Vec::new();
    each_dash(poly, style, stop, &mut |piece| {
        pieces.push(piece);
        true
    });
    pieces
}

/// [`apply_dashes`]' walk, handing each piece to `piece` as it is cut, in
/// the same order, and stopping as soon as `piece` answers `false` — which
/// is what this returns — or `stop` answers `true`, after the piece in hand.
fn each_dash(
    poly: &[Point],
    style: &StrokeStyle,
    stop: Option<&dyn Fn() -> bool>,
    piece: &mut dyn FnMut(Vec<Point>) -> bool,
) -> bool {
    let pattern: Vec<f64> = style
        .dashes
        .iter()
        .copied()
        .filter(|d| d.is_finite() && *d >= 0.0)
        .collect();
    // 8.4.3.6: an empty array, or one summing to zero, is a solid line.
    if pattern.is_empty() || pattern.iter().sum::<f64>() <= 0.0 {
        return piece(poly.to_vec());
    }
    // The entries the walk below ever treats as dashes are the even ones of
    // an even-length pattern, and every one of an odd-length pattern, whose
    // roles swap each time round. When none of them has length — `[0 0.01]`
    // — every dash is a single point, which the walk drops (a piece needs two
    // points), so it cuts nothing; but it took its 100 000 steps a segment to
    // find that out, and a writer bounding the work by the pieces it is
    // handed was never handed one to stop on. Nothing is cut either way.
    if pattern.len() % 2 == 0 && !pattern.iter().step_by(2).any(|d| *d > 0.0) {
        return true;
    }

    let mut current: Vec<Point> = Vec::new();

    // Where in the pattern the phase starts.
    let total: f64 = pattern.iter().sum();
    let phase = if style.dash_phase.is_finite() {
        style.dash_phase.rem_euclid(total)
    } else {
        0.0
    };
    // A dash of no length among dashes with length: cut nothing, cost
    // nothing. A pattern with none is walked exactly as it always was.
    let (pattern, mut remaining_phase) =
        without_empty_dashes(&pattern, phase).unwrap_or((pattern, phase));
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
                    piece(current);
                }
                return true;
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
                    if !piece(std::mem::take(&mut current)) {
                        return false;
                    }
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
        return piece(current);
    }
    true
}

/// `pattern` with its dashes of no length taken out and the gaps either
/// side of each joined, and `phase` moved to match — or `None` when every
/// dash has length, and the pattern is walked as it is.
///
/// A dash of no length is a single point, which [`each_dash`] drops (a
/// piece needs two), so taking it out leaves every piece where it was.
/// What it saves is time: each entry is a step of the walk, so 250 dashes
/// of no length padding one of 0.001 were 502 steps a piece, the
/// 100 000-step bound per segment ran out a fifth of the way along a line
/// a thousand long, and a caller bounding its work by the pieces it is
/// handed, with no cancel hook to stop it otherwise, paid hundreds of
/// steps for each. Without them a piece is cut every other step, besides
/// the segments a dash or a gap crosses.
///
/// The walk reads an odd pattern's entries as dashes and gaps by turns, so
/// it is written out twice, which the walk reads the same way; and the
/// result starts at its first dash with length, so that dashes of no length
/// at the start join the gap at the end. `phase` is where the walk's own
/// reduction puts it — in `[0, sum)` of the pattern as given, so an odd
/// pattern's second time round is reached by walking, as it always was —
/// and moves back by what the turn skipped. Every entry is finite and not
/// negative, and one dash has length: the caller has filtered and answered
/// every other pattern already.
fn without_empty_dashes(pattern: &[f64], phase: f64) -> Option<(Vec<f64>, f64)> {
    let twice: Vec<f64>;
    let even = if pattern.len() % 2 == 1 {
        twice = [pattern, pattern].concat();
        &twice[..]
    } else {
        pattern
    };
    let pairs: Vec<(f64, f64)> = even
        .chunks_exact(2)
        .filter_map(|pair| match *pair {
            [on, off] => Some((on, off)),
            _ => None,
        })
        .collect();
    if pairs.iter().all(|(on, _)| *on > 0.0) {
        return None;
    }
    let first = pairs.iter().position(|(on, _)| *on > 0.0)?;
    let skipped: f64 = pairs.iter().take(first).map(|(on, off)| on + off).sum();
    let mut kept: Vec<(f64, f64)> = Vec::with_capacity(pairs.len());
    for &(on, off) in pairs.iter().cycle().skip(first).take(pairs.len()) {
        match kept.last_mut() {
            Some(last) if on <= 0.0 => last.1 += off,
            _ => kept.push((on, off)),
        }
    }
    let total: f64 = kept.iter().map(|(on, off)| on + off).sum();
    let phase = (phase - skipped).rem_euclid(total);
    let phase = if phase.is_finite() { phase } else { 0.0 };
    Some((
        kept.into_iter().flat_map(|(on, off)| [on, off]).collect(),
        phase,
    ))
}

fn stroke_polyline(poly: &[Point], radius: f64, style: &StrokeStyle, out: &mut Sink<'_>) {
    // A point repeated has no direction between its copies, so a join or a
    // cap that reads one off them finds none and draws nothing. The commonest
    // repeat is the close: `flatten` ends a closed subpath on its start point
    // whether or not the last segment already returned there, so every circle
    // of four Béziers and an `h` — and every polygon that names its first
    // corner again before closing — lost the join at its start, a notch in
    // the outline 8.4.3.4 says is joined. Repeats are dropped while two
    // distinct points are left; a subpath of one point repeated is still the
    // degenerate one 8.5.3.2 describes, and is handled as it was.
    let distinct: Vec<Point>;
    let poly = if poly.windows(2).any(|pair| pair.first() == pair.get(1)) {
        distinct = poly
            .iter()
            .fold(Vec::with_capacity(poly.len()), |mut kept, &p| {
                if kept.last() != Some(&p) {
                    kept.push(p);
                }
                kept
            });
        if distinct.len() >= 2 {
            &distinct[..]
        } else {
            poly
        }
    } else {
        poly
    };
    if poly.len() < 2 {
        // 8.4.3.3: a degenerate subpath draws a dot under a round cap, and
        // nothing at all under a butt cap.
        if let (Some(&p), LineCap::Round) = (poly.first(), style.cap) {
            circle(p, radius, out);
        } else if let (Some(&p), LineCap::Square) = (poly.first(), style.cap) {
            emit(
                [
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
            [
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

fn join(a: Point, b: Point, c: Point, radius: f64, style: &StrokeStyle, out: &mut Sink<'_>) {
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
                [
                    Point::new(b.x + n1x, b.y + n1y),
                    tip,
                    Point::new(b.x + n2x, b.y + n2y),
                    b,
                ],
                out,
            );
            emit(
                [
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

fn bevel(a: Point, b: Point, c: Point, radius: f64, out: &mut Sink<'_>) {
    let (Some(u), Some(v)) = (unit(a, b), unit(b, c)) else {
        return;
    };
    let (n1x, n1y) = (-u.1 * radius, u.0 * radius);
    let (n2x, n2y) = (-v.1 * radius, v.0 * radius);

    emit(
        [
            Point::new(b.x + n1x, b.y + n1y),
            Point::new(b.x + n2x, b.y + n2y),
            b,
        ],
        out,
    );
    emit(
        [
            Point::new(b.x - n1x, b.y - n1y),
            Point::new(b.x - n2x, b.y - n2y),
            b,
        ],
        out,
    );
}

fn cap(end: Point, toward: Point, radius: f64, cap: LineCap, out: &mut Sink<'_>) {
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
                [
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

/// What the pieces of an outline are written into.
///
/// Every piece reaches it closed, finite and wound as every other piece is
/// ([`emit`]), so a target only records it: a [`Path`] as a move, lines and
/// a close, and a fill's [`Outline`] as the edges filling that path would
/// make of it.
trait Pieces {
    /// One piece: a polygon of at least three points.
    fn piece(&mut self, points: &[Point]);
}

impl Pieces for Path {
    fn piece(&mut self, points: &[Point]) {
        let mut points = points.iter();
        if let Some(first) = points.next() {
            self.move_to(first.x, first.y);
        }
        for p in points {
            self.line_to(p.x, p.y);
        }
        self.close();
    }
}

impl Pieces for Outline {
    fn piece(&mut self, points: &[Point]) {
        Outline::piece(self, points);
    }
}

/// Where the pieces of an outline go: a target, and the map they are carried
/// through on the way in, if the pen was not in the target's own space.
struct Sink<'a> {
    out: &'a mut dyn Pieces,
    map: Option<&'a Transform>,
}

/// Emits a polygon with a consistent orientation.
///
/// Every piece of a stroke outline — segment quads, joins, caps — is filled
/// together under the non-zero rule, and **two overlapping pieces wound in
/// opposite directions cancel to nothing**. Round caps vanishing entirely is
/// what that looks like, so orientation is normalized here rather than trusted
/// from each construction site, and in this one place whichever target the
/// sink writes to.
///
/// A piece built in the pen's space is mapped *before* its orientation is
/// measured: a map with a negative determinant turns every piece over, and
/// the floor's pieces ([`stroke_mapped`]), which are built in device space,
/// would otherwise be wound against them and punch holes where the two meet.
///
/// A piece is an array of its points — four for a quad, a miter or a square
/// cap, three for a bevel, [`ARC_STEPS`] for a circle — so mapping it and
/// turning it over happen on the stack, and no piece allocates.
fn emit<const N: usize>(points: [Point; N], sink: &mut Sink<'_>) {
    if N < 3 {
        return;
    }
    let mut points = match sink.map {
        Some(map) => points.map(|p| {
            let (x, y) = map.apply(p.x, p.y);
            Point::new(x, y)
        }),
        None => points,
    };
    if points.iter().any(|p| !p.is_finite()) {
        return;
    }

    // Twice the signed area; negative is the orientation the segment quads
    // produce, so match it. Each point with the next, the last with the
    // first.
    let mut area = 0.0;
    for (a, b) in points.iter().zip(points.iter().cycle().skip(1)) {
        area += a.x * b.y - b.x * a.y;
    }
    if area > 0.0 {
        points.reverse();
    }
    sink.out.piece(&points);
}

fn circle(center: Point, radius: f64, out: &mut Sink<'_>) {
    if !center.is_finite() || !radius.is_finite() || radius <= 0.0 {
        return;
    }
    let points: [Point; ARC_STEPS] = std::array::from_fn(|step| {
        let angle = (step as f64) / (ARC_STEPS as f64) * std::f64::consts::TAU;
        Point::new(
            center.x + radius * math::cos(angle),
            center.y + radius * math::sin(angle),
        )
    });
    emit(points, out);
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
    use crate::fill::{fill, fill_outline, fill_with, Mask};
    use crate::geom::{FillRule, Verb, MAX_VERBS};

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

    /// 8.4.3.4 joins every corner of a closed subpath, the one it starts at
    /// included, however the path got back there. A square from 4 to 14
    /// stroked two wide with miter joins is the ring between 3 and 15 and
    /// 5 and 13: `144 − 64 = 80`. Naming the first corner again before `h`
    /// — which is also what four Béziers and an `h` do to a circle — used to
    /// leave that corner unjoined and the ring a pixel short, 79, because the
    /// close repeated the point and a repeated point has no direction.
    #[test]
    fn a_closed_subpath_that_returns_to_its_start_keeps_the_join_there() {
        let style = StrokeStyle {
            width: 2.0,
            ..StrokeStyle::default()
        };
        let square = |again: bool| {
            let mut path = Path::new();
            path.move_to(4.0, 4.0);
            path.line_to(14.0, 4.0);
            path.line_to(14.0, 14.0);
            path.line_to(4.0, 14.0);
            if again {
                path.line_to(4.0, 4.0);
            }
            path.close();
            coverage(&stroke(&path, &style, 0.05, None), 20, 20)
        };
        for again in [false, true] {
            let area = square(again);
            assert!(
                (area - 80.0).abs() < 0.05,
                "first corner named again: {again}; the ring is {area}"
            );
        }
        // The degenerate subpath is still the degenerate one: a point named
        // twice under a round cap is one dot, not two on top of each other.
        let mut dot = Path::new();
        dot.move_to(10.0, 10.0);
        dot.line_to(10.0, 10.0);
        let round = StrokeStyle {
            width: 4.0,
            cap: LineCap::Round,
            ..StrokeStyle::default()
        };
        let area = coverage(&stroke(&dot, &round, 0.05, None), 20, 20);
        assert!((area - 12.2).abs() < 0.5, "a disc of radius 2: {area}");
    }

    // ---- a pen in another space ---------------------------------------------

    fn map(a: f64, b: f64, c: f64, d: f64, e: f64, f: f64) -> Transform {
        Transform { a, b, c, d, e, f }
    }

    fn segment(x0: f64, y0: f64, x1: f64, y1: f64) -> Path {
        let mut path = Path::new();
        path.move_to(x0, y0);
        path.line_to(x1, y1);
        path
    }

    /// The ink in row `y` of `path` filled over `w` columns, and its centre
    /// along the row.
    fn row(path: &Path, w: u32, h: u32, y: usize) -> (f64, f64) {
        let mask = fill(path, FillRule::NonZero, 0, 0, w, h, 0.05, None);
        let width = w as usize;
        let row = &mask.data[y * width..(y + 1) * width];
        let ink: f64 = row.iter().map(|&v| f64::from(v) / 255.0).sum();
        let moment: f64 = row
            .iter()
            .enumerate()
            .map(|(x, &v)| (x as f64 + 0.5) * f64::from(v) / 255.0)
            .sum();
        (ink, if ink > 0.0 { moment / ink } else { 0.0 })
    }

    /// The two stretches of a map are its singular values: a rotation's are
    /// one, `scale(1, 3)`'s are three and one, and the shear `[1 1; 0 1]`'s
    /// are the golden ratio and its reciprocal, `(√5 ± 1) / 2`.
    #[test]
    fn stretches_are_a_maps_singular_values() {
        let (big, small) = stretches(&map(0.6, 0.8, -0.8, 0.6, 7.0, 9.0));
        assert!((big - 1.0).abs() < 1e-15 && (small - 1.0).abs() < 1e-15);
        assert_eq!(stretches(&map(1.0, 0.0, 0.0, 3.0, 0.0, 0.0)), (3.0, 1.0));
        assert_eq!(stretches(&map(1.0, 0.0, 0.0, -3.0, 0.0, 0.0)), (3.0, 1.0));
        let (big, small) = stretches(&map(1.0, 0.0, 1.0, 1.0, 0.0, 0.0));
        let root5 = 5.0_f64.sqrt();
        assert!((big - (root5 + 1.0) / 2.0).abs() < 1e-15, "{big}");
        assert!((small - (root5 - 1.0) / 2.0).abs() < 1e-15, "{small}");
        assert_eq!(
            stretches(&map(f64::NAN, 0.0, 0.0, 1.0, 0.0, 0.0)),
            (0.0, 0.0)
        );
    }

    /// 8.4.3.2 in user space: under `scale(1, 3)` a pen two units wide is six
    /// device units across a line running along `x` and two across one
    /// running along `y`. The areas are the clause's — 20 × 6 and 24 × 2 —
    /// where one device width, `2√3`, would make them 69.3 and 83.1.
    #[test]
    fn a_pen_under_a_stretch_is_wide_across_it_and_narrow_along_it() {
        let stretch = map(1.0, 0.0, 0.0, 3.0, 4.0, 4.0);
        let style = StrokeStyle {
            width: 2.0,
            ..StrokeStyle::default()
        };
        let along = stroke_mapped(
            &segment(0.0, 4.0, 20.0, 4.0),
            &style,
            &stretch,
            0.0,
            0.05,
            None,
        );
        let area = coverage(&along, 32, 32);
        assert!((area - 120.0).abs() < 0.5, "20 long, 6 across: {area}");

        let across = stroke_mapped(
            &segment(4.0, 0.0, 4.0, 8.0),
            &style,
            &stretch,
            0.0,
            0.05,
            None,
        );
        let area = coverage(&across, 32, 32);
        assert!((area - 48.0).abs() < 0.5, "24 long, 2 across: {area}");
    }

    /// A dash is cut square in user space, so under a shear its ends are
    /// sheared on the device: with `x' = x + y`, the first dash of a line
    /// along `x` at `y = 10` has its ink in device row `r` (`y` from `r` to
    /// `r + 1`) from `x = 2 + r + 0.5` to `12 + r + 0.5` — ten long, centred
    /// at `7.5 + r`, one pixel further right each row down.
    #[test]
    fn a_dash_under_a_shear_is_cut_along_the_shear() {
        let shear = map(1.0, 0.0, 1.0, 1.0, 0.0, 0.0);
        let style = StrokeStyle {
            width: 4.0,
            dashes: vec![10.0, 10.0],
            ..StrokeStyle::default()
        };
        let outline = stroke_mapped(
            &segment(2.0, 10.0, 40.0, 10.0),
            &style,
            &shear,
            0.0,
            0.05,
            None,
        );
        // The second dash starts at user x = 22, past device x = 30 in every
        // row the pen covers, so thirty columns hold the first alone.
        for r in 8..12 {
            let (ink, centre) = row(&outline, 30, 16, r);
            assert!((ink - 10.0).abs() < 0.05, "row {r}: ten long, {ink}");
            let want = 7.5 + r as f64;
            assert!(
                (centre - want).abs() < 0.05,
                "row {r}: centred at {want}, sheared, not {centre}"
            );
        }
        for r in (0..8).chain(12..16) {
            assert_eq!(
                row(&outline, 30, 16, r).0,
                0.0,
                "row {r} is outside the pen"
            );
        }
    }

    /// The floor holds in device pixels in every direction, and it is wound
    /// as the pen is even under a map that turns the pen's pieces over: a
    /// reflecting map with a pen thinner than the floor is the floor's width
    /// across, where pieces wound against each other would cancel to the
    /// difference of the two.
    #[test]
    fn the_floor_is_the_device_s_and_is_wound_with_the_pen() {
        let flip = map(1.0, 0.0, 0.0, -3.0, 0.0, 40.0);
        let thin = StrokeStyle {
            width: 0.2,
            ..StrokeStyle::default()
        };
        // Along x, the pen is 0.6 across and the floor 1.
        let along = stroke_mapped(&segment(4.0, 4.0, 24.0, 4.0), &thin, &flip, 1.0, 0.05, None);
        let area = coverage(&along, 32, 48);
        assert!(
            (area - 20.0).abs() < 0.3,
            "20 long, the floor's 1 across: {area}"
        );

        // A zero width is the floor alone, in both directions.
        let zero = StrokeStyle {
            width: 0.0,
            ..StrokeStyle::default()
        };
        let along = stroke_mapped(&segment(4.0, 4.0, 24.0, 4.0), &zero, &flip, 1.0, 0.05, None);
        let area = coverage(&along, 32, 48);
        assert!((area - 20.0).abs() < 0.3, "{area}");
        let across = stroke_mapped(&segment(4.0, 2.0, 4.0, 10.0), &zero, &flip, 1.0, 0.05, None);
        let area = coverage(&across, 32, 48);
        assert!((area - 24.0).abs() < 0.3, "24 long, 1 across: {area}");

        // And a pen wider than the floor everywhere is the pen alone.
        let wide = StrokeStyle {
            width: 2.0,
            ..StrokeStyle::default()
        };
        let path = segment(4.0, 4.0, 24.0, 4.0);
        let floored = stroke_mapped(&path, &wide, &flip, 1.0, 0.05, None);
        let bare = stroke_mapped(&path, &wide, &flip, 0.0, 0.05, None);
        assert_eq!(floored.verbs(), bare.verbs(), "no floor pieces were needed");
    }

    /// A map that is not finite, or collapses everything, outlines nothing
    /// rather than a path of `NaN`s.
    #[test]
    fn a_map_with_no_stretch_outlines_nothing() {
        let style = StrokeStyle::default();
        let path = segment(0.0, 0.0, 10.0, 0.0);
        for bad in [
            map(f64::NAN, 0.0, 0.0, 1.0, 0.0, 0.0),
            map(0.0, 0.0, 0.0, 0.0, 1.0, 1.0),
            map(f64::INFINITY, 0.0, 0.0, 1.0, 0.0, 0.0),
        ] {
            assert!(stroke_mapped(&path, &style, &bad, 1.0, 0.05, None).is_empty());
        }
    }

    /// `dash` is the dasher alone: the pieces 8.4.3.6 leaves, in the path's
    /// own space.
    #[test]
    fn dash_hands_back_the_pieces_a_pattern_leaves() {
        let mut ends: Vec<(f64, f64)> = Vec::new();
        let whole = dash(
            &segment(0.0, 0.0, 30.0, 0.0),
            &[10.0, 5.0],
            0.0,
            0.05,
            &mut |piece| {
                ends.push((piece[0].x, piece[piece.len() - 1].x));
                true
            },
        );
        assert!(whole, "every piece was handed over");
        assert_eq!(ends, vec![(0.0, 10.0), (15.0, 25.0)]);
        let mut solid = 0;
        dash(&segment(0.0, 0.0, 30.0, 0.0), &[], 0.0, 0.05, &mut |_| {
            solid += 1;
            true
        });
        assert_eq!(solid, 1);
    }

    /// `dash` stops cutting when its caller says so: of the 1 500 pieces
    /// `[0.01 0.01]` leaves of a line thirty long, the third answer `false`
    /// is the last piece cut.
    #[test]
    fn dash_stops_cutting_when_the_caller_has_enough() {
        let mut handed = 0;
        let whole = dash(
            &segment(0.0, 0.0, 30.0, 0.0),
            &[0.01, 0.01],
            0.0,
            0.05,
            &mut |_| {
                handed += 1;
                handed < 3
            },
        );
        assert!(!whole, "the cutting was stopped");
        assert_eq!(handed, 3);
    }

    /// A pattern whose every dash has no length cuts nothing, and knows it
    /// without walking: `[0 0.01]` along a line a thousand long was 100 000
    /// steps of nothing, asking `stop` every [`STOP_EVERY`] of them and
    /// handing a caller no piece it could stop on. An odd-length pattern
    /// swaps its entries' roles each time round, so `[0 1 0]`, whose even
    /// entries have no length, still dashes: its `1` is a dash every other
    /// time round.
    #[test]
    fn dashes_of_no_length_cut_nothing_without_walking_the_line() {
        let line = [Point::new(0.0, 0.0), Point::new(1000.0, 0.0)];
        let style = |dashes: &[f64]| StrokeStyle {
            dashes: dashes.to_vec(),
            ..StrokeStyle::default()
        };
        let asked = std::cell::Cell::new(0u32);
        let ask = || {
            asked.set(asked.get() + 1);
            false
        };
        let pieces = apply_dashes(&line, &style(&[0.0, 0.01]), Some(&ask));
        assert!(pieces.is_empty(), "a dash of no length is no piece");
        assert_eq!(asked.get(), 0, "and the line was not walked to find that");

        let mut handed = 0;
        let whole = dash(
            &segment(0.0, 0.0, 1000.0, 0.0),
            &[0.0, 0.01, 0.0, 5.0],
            0.0,
            0.05,
            &mut |_| {
                handed += 1;
                true
            },
        );
        assert!(whole);
        assert_eq!(handed, 0, "every even entry of an even pattern is a dash");

        let odd = apply_dashes(&line, &style(&[0.0, 1.0, 0.0]), None);
        assert!(
            odd.first().is_some_and(|piece| piece.len() > 1),
            "an odd pattern's entries are dashes every other time round"
        );
    }

    /// The pieces two walks cut, side by side: as many, and each point
    /// within a nanometre of its twin.
    fn same_pieces(left: &[Vec<Point>], right: &[Vec<Point>], what: &str) {
        assert_eq!(left.len(), right.len(), "{what}: how many pieces");
        for (i, (a, b)) in left.iter().zip(right).enumerate() {
            assert_eq!(a.len(), b.len(), "{what}: piece {i}'s points");
            for (p, q) in a.iter().zip(b) {
                assert!(
                    (p.x - q.x).abs() < 1e-9 && (p.y - q.y).abs() < 1e-9,
                    "{what}: piece {i} at {p:?} against {q:?}"
                );
            }
        }
    }

    /// **A dash of no length costs no step of the walk**, among dashes with
    /// length as well as alone. It is a single point, which the walk drops,
    /// so it cuts nothing — but each entry was a step, and the third review
    /// of lane 8A padded one dash of 0.001 with 250 dashes of no length and
    /// their gaps (502 entries, under the interpreter's 512-operand stack):
    /// 502 steps a piece, so the 100 000-step bound per segment ran out a
    /// fifth of the way along a line a thousand long, and a caller of
    /// [`dash`] with no cancel hook, bounding its output by the pieces it
    /// is handed, spent about 21 steps per byte it wrote.
    ///
    /// Taken out before the walk, with the gaps either side joined, every
    /// piece is where it was and the whole line is cut in a step per entry
    /// left: the padded pattern cuts exactly what `[0.001 2.51]` cuts at the
    /// phase that puts its gap first, and asks `stop` once (the walk's
    /// first step) where it asked 98 times. An odd pattern is read twice
    /// round, so `[0 1 2]` is `[2 0 1 3]` turned to start one in, at every
    /// phase; and one in the middle, `[1 1 0 1]`, is `[1 2]`.
    #[test]
    fn a_dash_of_no_length_costs_no_step_of_the_walk() {
        let line = [Point::new(0.0, 0.0), Point::new(1000.0, 0.0)];
        let style = |dashes: &[f64], phase: f64| StrokeStyle {
            dashes: dashes.to_vec(),
            dash_phase: phase,
            ..StrokeStyle::default()
        };
        let mut padded = Vec::new();
        for _ in 0..250 {
            padded.extend_from_slice(&[0.0, 0.01]);
        }
        padded.extend_from_slice(&[0.001, 0.01]);
        let asked = std::cell::Cell::new(0u32);
        let ask = || {
            asked.set(asked.get() + 1);
            false
        };
        let cut = apply_dashes(&line, &style(&padded, 0.0), Some(&ask));
        let plain = apply_dashes(&line, &style(&[0.001, 2.51], 0.011), None);
        assert!(plain.len() > 390, "{} pieces", plain.len());
        same_pieces(&cut, &plain, "250 dashes of no length and one of 0.001");
        assert_eq!(
            asked.get(),
            1,
            "a step per dash and gap, not one per entry: {} asks",
            asked.get()
        );

        for phase in [0.0, 0.5, 1.0, 2.5, 2.999] {
            same_pieces(
                &apply_dashes(&line, &style(&[0.0, 1.0, 2.0], phase), None),
                &apply_dashes(
                    &line,
                    &style(&[2.0, 0.0, 1.0, 3.0], (phase - 1.0).rem_euclid(6.0)),
                    None,
                ),
                &format!("[0 1 2] at {phase}"),
            );
            same_pieces(
                &apply_dashes(&line, &style(&[1.0, 1.0, 0.0, 1.0], phase), None),
                &apply_dashes(&line, &style(&[1.0, 2.0], phase), None),
                &format!("[1 1 0 1] at {phase}"),
            );
        }
    }

    // ---- the outline written straight into a fill's edges -----------------

    /// A pen for the comparisons: a style, the map and floor it is carried
    /// out through where its space is not the device's, and the tolerance
    /// the path is flattened at.
    struct Pen {
        style: StrokeStyle,
        mapped: Option<(Transform, f64)>,
        tolerance: f64,
    }

    impl Pen {
        /// The outline as the path [`stroke`] or [`stroke_mapped`] returns.
        fn outline(&self, path: &Path, stop: Option<&dyn Fn() -> bool>) -> Path {
            match &self.mapped {
                Some((map, floor)) => {
                    stroke_mapped(path, &self.style, map, *floor, self.tolerance, stop)
                }
                None => stroke(path, &self.style, self.tolerance, stop),
            }
        }

        /// The outline written into `scratch`, by [`stroke_with`] or
        /// [`stroke_mapped_with`].
        fn write(&self, scratch: &mut FillScratch, path: &Path, stop: Option<&dyn Fn() -> bool>) {
            match &self.mapped {
                Some((map, floor)) => stroke_mapped_with(
                    scratch,
                    path,
                    &self.style,
                    map,
                    *floor,
                    self.tolerance,
                    stop,
                ),
                None => stroke_with(scratch, path, &self.style, self.tolerance, stop),
            }
        }
    }

    /// Pens with the cap and join given, between them every miter limit —
    /// the default, one every corner is past, one a right angle just passes,
    /// a vast one, one that is not a number and one below zero — every width
    /// — the thinnest line, a thin one, wide ones, one that is not a number —
    /// every kind of dash pattern — none, even, odd, every dash of no length,
    /// one of no length among others — at phases inside, below and past the
    /// pattern and one that is not a number, three tolerances, and pens in
    /// another space: a turn, a stretch, a shear, a reflection and a skew,
    /// with floors of none, under the pen, over it and not a number.
    fn pens(cap: LineCap, join: LineJoin) -> Vec<Pen> {
        let style = |width: f64, miter_limit: f64, dashes: &[f64], dash_phase: f64| StrokeStyle {
            width,
            cap,
            join,
            miter_limit,
            dashes: dashes.to_vec(),
            dash_phase,
        };
        let plain = |style: StrokeStyle, tolerance: f64| Pen {
            style,
            mapped: None,
            tolerance,
        };
        let carried = |style: StrokeStyle, to: Transform, floor: f64| Pen {
            style,
            mapped: Some((to, floor)),
            tolerance: 0.1,
        };
        vec![
            plain(style(2.0, 10.0, &[], 0.0), 0.05),
            plain(style(5.5, 1.0, &[], 0.0), 0.3),
            plain(style(3.0, 1.42, &[4.0, 2.0], 1.5), 0.1),
            plain(style(0.0, 4.0, &[3.0], -2.5), 0.1),
            plain(style(f64::NAN, f64::NAN, &[5.0, 1.0, 0.0, 1.0], 1e3), 0.2),
            plain(style(0.4, 1e6, &[0.0, 2.0], 0.0), 0.1),
            plain(style(2.5, -3.0, &[0.5, 0.25, 1.0], f64::NAN), 0.1),
            carried(
                style(2.0, 10.0, &[], 0.0),
                map(0.6, 0.8, -0.8, 0.6, 20.0, 2.0),
                0.0,
            ),
            carried(
                style(2.0, 10.0, &[4.0, 3.0], 1.0),
                map(1.0, 0.0, 0.0, 3.0, 4.0, 4.0),
                0.8,
            ),
            carried(
                style(0.2, 4.0, &[], 0.0),
                map(1.0, 0.0, 1.0, 1.0, 0.0, 0.0),
                1.5,
            ),
            carried(
                style(0.0, 10.0, &[2.0, 2.0], 0.5),
                map(1.0, 0.0, 0.0, -3.0, 0.0, 130.0),
                1.0,
            ),
            carried(
                style(1.0, 10.0, &[], 0.0),
                map(2.0, 0.5, 0.25, 0.5, 3.0, 9.0),
                f64::NAN,
            ),
        ]
    }

    /// Paths for the comparisons: open and closed, a sharp corner and a
    /// shallow one, both curve verbs, a circle closed on its start, a lone
    /// point, points named twice, one point named twice and nothing else,
    /// several subpaths, one a million pixels out and one reaching past any
    /// page.
    fn strokeable() -> Vec<(&'static str, Path)> {
        let mut out = Vec::new();

        let mut zigzag = Path::new();
        zigzag.move_to(3.0, 4.0);
        zigzag.line_to(28.0, 6.5);
        zigzag.line_to(5.5, 12.0);
        zigzag.line_to(30.0, 30.0);
        zigzag.line_to(31.0, 31.5);
        out.push(("an open zigzag", zigzag));

        let mut rectangle = Path::new();
        rectangle.rect(6.25, 5.5, 20.0, 17.75);
        out.push(("a closed rectangle", rectangle));

        let mut curves = Path::new();
        curves.move_to(4.0, 30.0);
        curves.curve_to(4.0, 2.0, 34.0, 2.0, 34.0, 30.0);
        curves.quad_to(19.0, 40.0, 10.0, 24.0);
        out.push(("a cubic and a quadratic", curves));

        let k = 0.552_284_75 * 12.0;
        let mut circle = Path::new();
        circle.move_to(32.0, 20.0);
        circle.curve_to(32.0, 20.0 + k, 20.0 + k, 32.0, 20.0, 32.0);
        circle.curve_to(20.0 - k, 32.0, 8.0, 20.0 + k, 8.0, 20.0);
        circle.curve_to(8.0, 20.0 - k, 20.0 - k, 8.0, 20.0, 8.0);
        circle.curve_to(20.0 + k, 8.0, 32.0, 20.0 - k, 32.0, 20.0);
        circle.close();
        out.push(("a circle of four curves and a close", circle));

        let mut dot = Path::new();
        dot.move_to(18.5, 18.5);
        out.push(("a lone point", dot));

        let mut repeats = Path::new();
        repeats.move_to(8.0, 8.0);
        repeats.line_to(8.0, 8.0);
        repeats.line_to(24.0, 14.0);
        repeats.line_to(24.0, 14.0);
        repeats.line_to(12.0, 28.0);
        out.push(("points named twice", repeats));

        let mut twice = Path::new();
        twice.move_to(14.0, 9.0);
        twice.line_to(14.0, 9.0);
        out.push(("one point named twice", twice));

        let mut several = Path::new();
        several.move_to(2.0, 2.0);
        several.line_to(14.0, 2.0);
        several.move_to(20.0, 6.0);
        several.line_to(20.0, 26.0);
        several.line_to(34.0, 26.0);
        several.move_to(6.0, 34.0);
        several.move_to(30.0, 4.0);
        several.line_to(36.0, 12.0);
        several.close();
        out.push(("several subpaths", several));

        let mut far = Path::new();
        far.move_to(1_000_003.25, 7.0);
        far.line_to(1_000_031.5, 29.75);
        far.line_to(1_000_009.0, 30.5);
        out.push(("a path a million pixels out", far));

        let mut huge = Path::new();
        huge.move_to(-1e300, 5.0);
        huge.line_to(1e300, 9.0);
        huge.line_to(20.0, 1e300);
        out.push(("a path reaching past any page", huge));

        out
    }

    /// The regions an outline is filled over: its rectangle with a pixel to
    /// spare, at most 160 pixels a side, one cutting through its middle, and
    /// the pixel at its centre — or a small square where it has no points.
    fn regions(bounds: Option<(f64, f64, f64, f64)>) -> [(i32, i32, u32, u32); 3] {
        let Some((x0, y0, x1, y1)) = bounds else {
            return [(0, 0, 16, 16), (4, 4, 3, 3), (8, 8, 1, 1)];
        };
        let at = |v: f64| (v as i64).clamp(-1_000_000_000, 1_000_000_000);
        let (left, top) = (at(x0.floor()) - 1, at(y0.floor()) - 1);
        let (right, bottom) = (at(x1.ceil()) + 1, at(y1.ceil()) + 1);
        let width = (right - left).clamp(1, 160);
        let height = (bottom - top).clamp(1, 160);
        let rect = |x: i64, y: i64, w: i64, h: i64| (x as i32, y as i32, w as u32, h as u32);
        [
            rect(left, top, width, height),
            rect(
                left + width / 3,
                top + height / 3,
                width / 2 + 1,
                height / 2 + 1,
            ),
            rect(left + width / 2, top + height / 2, 1, 1),
        ]
    }

    /// Two masks of one region, byte for byte, naming the first pixel apart.
    fn same_mask(got: &Mask, want: &Mask, what: &str) {
        assert_eq!(
            (got.x0, got.y0, got.width, got.height),
            (want.x0, want.y0, want.width, want.height),
            "{what}: the region"
        );
        assert_eq!(got.data.len(), want.data.len(), "{what}: the mask's size");
        if let Some(at) = got.data.iter().zip(&want.data).position(|(a, b)| a != b) {
            let across = want.width.max(1) as usize;
            panic!(
                "{what}: pixel ({}, {}) is {:?} where the path's fill has {:?}",
                i64::from(want.x0) + (at % across) as i64,
                i64::from(want.y0) + (at / across) as i64,
                got.data.get(at),
                want.data.get(at)
            );
        }
    }

    /// **The direct outline fills as the path outline did.** For every cap,
    /// join and pen above, over every path above, the outline a stroke writes
    /// into a scratch has the bounds of the path the same stroke returns, and
    /// fills — over its rectangle, one cutting through it and a single pixel —
    /// to the bytes `fill` gives that path; and so does every partial outline
    /// a stop leaves, the stop asked as often either way. One scratch takes
    /// every stroke, so no outline is left over to reach the next one's mask.
    #[test]
    fn the_direct_outline_fills_as_the_path_outline_did() {
        let mut scratch = FillScratch::new();
        let (mut cases, mut inked) = (0u64, 0u64);
        for (name, path) in strokeable() {
            for cap in [LineCap::Butt, LineCap::Round, LineCap::Square] {
                for join in [LineJoin::Miter, LineJoin::Round, LineJoin::Bevel] {
                    for (index, pen) in pens(cap, join).iter().enumerate() {
                        let what = format!("{name}, {cap:?} caps, {join:?} joins, pen {index}");
                        let outline = pen.outline(&path, None);
                        pen.write(&mut scratch, &path, None);
                        assert_eq!(
                            scratch.outline_bounds(),
                            outline.bounds(),
                            "{what}: the bounds"
                        );
                        for (x0, y0, width, height) in regions(outline.bounds()) {
                            let want = fill(
                                &outline,
                                FillRule::NonZero,
                                x0,
                                y0,
                                width,
                                height,
                                pen.tolerance,
                                None,
                            );
                            let got = fill_outline(&mut scratch, x0, y0, width, height, None);
                            same_mask(
                                &got,
                                &want,
                                &format!("{what} over {x0},{y0} {width}x{height}"),
                            );
                            cases += 1;
                            if want.data.iter().any(|v| *v != 0) {
                                inked += 1;
                            }
                        }

                        for at in [2, 3] {
                            let what = format!("{what}, stopped at question {at}");
                            let (asked_path, asked_edges) = (StopAt::new(at), StopAt::new(at));
                            let partial = pen.outline(&path, Some(&|| asked_path.ask()));
                            pen.write(&mut scratch, &path, Some(&|| asked_edges.ask()));
                            assert_eq!(
                                asked_edges.calls.get(),
                                asked_path.calls.get(),
                                "{what}: asked as often"
                            );
                            assert_eq!(
                                scratch.outline_bounds(),
                                partial.bounds(),
                                "{what}: the bounds"
                            );
                            let [(x0, y0, width, height), ..] = regions(partial.bounds());
                            same_mask(
                                &fill_outline(&mut scratch, x0, y0, width, height, None),
                                &fill(
                                    &partial,
                                    FillRule::NonZero,
                                    x0,
                                    y0,
                                    width,
                                    height,
                                    pen.tolerance,
                                    None,
                                ),
                                &what,
                            );
                        }
                    }
                }
            }
        }
        assert!(
            inked * 4 > cases,
            "{inked} of {cases} comparisons drew anything"
        );
    }

    /// **A stroke is flattened once**: by the stroker, never by the fill. A
    /// stroke written into a scratch and filled is one flattening pass —
    /// solid or dashed, with curves, several subpaths and a lone point, in
    /// the device's space or carried out of the pen's with a floor or none —
    /// where the same outline returned as a path and filled is two. A map
    /// with no stretch outlines nothing and flattens nothing to find that
    /// out.
    #[test]
    fn a_stroke_is_flattened_once() {
        let mut path = Path::new();
        path.move_to(3.0, 3.0);
        path.curve_to(3.0, 30.0, 30.0, 30.0, 30.0, 3.0);
        path.quad_to(16.0, -8.0, 3.0, 3.0);
        path.move_to(8.0, 20.0);
        path.line_to(24.0, 22.0);
        path.move_to(16.0, 16.0);
        let solid = StrokeStyle {
            width: 2.5,
            cap: LineCap::Round,
            join: LineJoin::Round,
            ..StrokeStyle::default()
        };
        let dashed = StrokeStyle {
            width: 1.5,
            dashes: vec![3.0, 1.5],
            dash_phase: 0.75,
            ..StrokeStyle::default()
        };
        let stretch = map(1.0, 0.0, 0.5, 3.0, 2.0, 1.0);
        let mut scratch = FillScratch::new();
        for (what, style) in [("solid", &solid), ("dashed", &dashed)] {
            for floor in [None, Some(0.0), Some(1.5)] {
                let _ = crate::geom::count::take();
                match floor {
                    None => stroke_with(&mut scratch, &path, style, 0.1, None),
                    Some(floor) => {
                        stroke_mapped_with(&mut scratch, &path, style, &stretch, floor, 0.1, None);
                    }
                }
                let _ = fill_outline(&mut scratch, -8, -8, 80, 120, None);
                assert_eq!(
                    crate::geom::count::take(),
                    1,
                    "{what}, floor {floor:?}: written and filled, one pass"
                );

                let outline = match floor {
                    None => stroke(&path, style, 0.1, None),
                    Some(floor) => stroke_mapped(&path, style, &stretch, floor, 0.1, None),
                };
                let _ = fill(&outline, FillRule::NonZero, -8, -8, 80, 120, 0.1, None);
                assert_eq!(
                    crate::geom::count::take(),
                    2,
                    "{what}, floor {floor:?}: returned as a path and filled, two"
                );
            }
        }

        let _ = crate::geom::count::take();
        let collapse = map(0.0, 0.0, 0.0, 0.0, 1.0, 1.0);
        stroke_mapped_with(&mut scratch, &path, &solid, &collapse, 1.0, 0.1, None);
        assert_eq!(crate::geom::count::take(), 0, "no stretch, no pass");
        assert_eq!(scratch.outline_bounds(), None, "and no outline");
    }

    /// The outline stays in the scratch until the next stroke writes another:
    /// a path filled through the scratch meanwhile leaves it, and so does
    /// filling it; the same stroke again grows none of the scratch's buffers;
    /// another stroke replaces it; and a stroke that outlines nothing leaves
    /// nothing to fill.
    #[test]
    fn an_outline_stays_in_the_scratch_until_the_next_stroke() {
        let style = StrokeStyle {
            width: 3.0,
            cap: LineCap::Round,
            dashes: vec![6.0, 2.0],
            ..StrokeStyle::default()
        };
        let first = segment(2.0, 4.0, 30.0, 20.0);
        let second = segment(4.0, 28.0, 28.0, 6.0);
        let path_fill = |path: &Path| {
            fill(
                &stroke(path, &style, 0.05, None),
                FillRule::NonZero,
                0,
                0,
                40,
                40,
                0.05,
                None,
            )
        };

        let mut scratch = FillScratch::new();
        stroke_with(&mut scratch, &first, &style, 0.05, None);
        let mut square = Path::new();
        square.rect(1.0, 1.0, 30.0, 30.0);
        let _ = fill_with(
            &mut scratch,
            &square,
            FillRule::EvenOdd,
            0,
            0,
            40,
            40,
            0.05,
            None,
        );
        let want = path_fill(&first);
        same_mask(
            &fill_outline(&mut scratch, 0, 0, 40, 40, None),
            &want,
            "after a path was filled through the scratch",
        );
        same_mask(
            &fill_outline(&mut scratch, 0, 0, 40, 40, None),
            &want,
            "filled a second time",
        );

        let held = scratch.capacities();
        stroke_with(&mut scratch, &first, &style, 0.05, None);
        same_mask(
            &fill_outline(&mut scratch, 0, 0, 40, 40, None),
            &want,
            "the same stroke again",
        );
        assert_eq!(
            scratch.grown_from(&held),
            0,
            "the same stroke again grows nothing"
        );

        stroke_with(&mut scratch, &second, &style, 0.05, None);
        assert_eq!(
            scratch.outline_bounds(),
            stroke(&second, &style, 0.05, None).bounds(),
            "the next stroke's bounds"
        );
        same_mask(
            &fill_outline(&mut scratch, 0, 0, 40, 40, None),
            &path_fill(&second),
            "the next stroke's outline",
        );

        let nowhere = map(f64::NAN, 0.0, 0.0, 1.0, 0.0, 0.0);
        stroke_mapped_with(&mut scratch, &first, &style, &nowhere, 1.0, 0.05, None);
        assert_eq!(scratch.outline_bounds(), None, "a map that is not finite");
        assert!(
            fill_outline(&mut scratch, 0, 0, 40, 40, None)
                .data
                .iter()
                .all(|v| *v == 0),
            "outlines nothing"
        );
    }

    /// A path holds at most `MAX_VERBS` verbs and drops the rest, and the
    /// outline written into a fill's edges stops where that path stops: the
    /// piece the bound cuts keeps the points whose verbs fit, closed back to
    /// its first as an unclosed subpath is, and nothing after it is kept.
    ///
    /// Round caps on dashes of `[1 1]` down a line sixty thousand long, with
    /// a corner in the first dash and in the third: a dash is a quad and two
    /// circles, 39 verbs, and a dash holding a corner 54, with its two quads
    /// and its miter, so the bound falls on the ninth point of a circle and
    /// its close is past it.
    #[test]
    fn an_outline_stops_where_its_path_reaches_the_verb_bound() {
        let mut path = Path::new();
        path.move_to(10.0, 0.0);
        path.line_to(10.0, 0.5);
        path.line_to(14.0, 0.5);
        path.line_to(14.0, 60_000.0);
        let style = StrokeStyle {
            width: 2.0,
            cap: LineCap::Round,
            dashes: vec![1.0, 1.0],
            ..StrokeStyle::default()
        };
        let outline = stroke(&path, &style, 0.1, None);
        assert_eq!(outline.verbs().len(), MAX_VERBS, "the path is at its bound");
        assert!(
            matches!(outline.verbs().last(), Some(Verb::LineTo(_))),
            "the bound falls inside a piece: {:?}",
            outline.verbs().last()
        );

        let mut scratch = FillScratch::new();
        stroke_with(&mut scratch, &path, &style, 0.1, None);
        assert_eq!(scratch.outline_bounds(), outline.bounds(), "the bounds");
        let Some((_, _, _, bottom)) = outline.bounds() else {
            panic!("an outline with points");
        };
        let cut = bottom.floor() as i32;
        for (y0, what) in [
            (-4, "the corners at the top"),
            (cut - 40, "where the bound cuts"),
        ] {
            let want = fill(&outline, FillRule::NonZero, 0, y0, 24, 64, 0.1, None);
            assert!(want.data.iter().any(|v| *v != 0), "{what}: ink");
            same_mask(
                &fill_outline(&mut scratch, 0, y0, 24, 64, None),
                &want,
                what,
            );
        }
    }
}
