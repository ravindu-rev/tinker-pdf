//! SVG 1.1 §8.3's path data, and the basic shapes that become one.
//!
//! # Everything becomes a move, a line, a cubic or a close
//!
//! SVG has twenty commands; an [`Outline`] has four segments. Quadratics are
//! raised to cubics and elliptical arcs are approximated by them, here, once —
//! because **PDF has neither**. A consumer that received quadratics and arcs
//! would have to convert them itself, and there would then be two conversions
//! in the tree that had to agree: this crate's for its own tests and the
//! facade's for the page. The one that survives is the one a test can reach.
//!
//! The raising is exact. A quadratic's control point maps to two cubic ones at
//! two thirds of the way from each end (§F.6.6 gives the same identity for the
//! reverse direction), so nothing is approximated there and only the arc is.

use tinker_pdf_math as math;

use crate::transform;

/// One piece of an outline.
///
/// Absolute, in the space the outline is stated in — the relative commands are
/// resolved as they are read, because a relative segment is only meaningful
/// beside the one before it and a consumer should never have to track a pen.
#[derive(Clone, Copy, Debug, PartialEq)]
#[non_exhaustive]
pub enum Segment {
    /// Start a new subpath at a point.
    Move([f64; 2]),
    /// A straight line to a point.
    Line([f64; 2]),
    /// A cubic Bézier: two controls and an endpoint.
    Cubic([f64; 2], [f64; 2], [f64; 2]),
    /// Close the current subpath.
    ///
    /// §8.3.3 makes this a *line* back to the subpath's start as well as a
    /// closure, which matters for a stroke's join: an open path that happens to
    /// end where it began has two caps where a closed one has a join.
    Close,
}

/// A path, as segments.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Outline {
    /// The segments, in order.
    pub segments: Vec<Segment>,
}

impl Outline {
    /// The outline through a matrix.
    #[must_use]
    pub fn transformed(&self, matrix: [f64; 6]) -> Outline {
        let at = |p: [f64; 2]| transform::apply(matrix, p);
        Outline {
            segments: self
                .segments
                .iter()
                .map(|segment| match *segment {
                    Segment::Move(p) => Segment::Move(at(p)),
                    Segment::Line(p) => Segment::Line(at(p)),
                    Segment::Cubic(a, b, c) => Segment::Cubic(at(a), at(b), at(c)),
                    Segment::Close => Segment::Close,
                })
                .collect(),
        }
    }

    /// Whether the outline draws nothing.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        !self
            .segments
            .iter()
            .any(|s| !matches!(s, Segment::Move(_) | Segment::Close))
    }
}

/// Why path data produced no outline.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum PathError {
    /// Not §8.3.2's grammar.
    Syntax,
    /// More segments than the caller's budget allows.
    TooManySegments,
}

/// Reads §8.3.2's path data.
///
/// **§8.3.2's error rule is followed rather than improved on**: *"the path data
/// up to the point of the error is rendered"*. So a `d` that goes wrong halfway
/// yields the half that parsed, and the caller is told by [`PathError`] only
/// when *nothing* parsed. Refusing the whole path instead would lose drawing a
/// real file intended, and drawing past the error would invent one it did not.
///
/// `budget` is decremented by every segment produced, so one `d` cannot be the
/// whole document.
pub fn parse(data: &str, budget: &mut usize) -> Result<Outline, PathError> {
    let mut scan = Scan {
        bytes: data.as_bytes(),
        at: 0,
    };
    let mut out = Outline::default();
    // The current point, the start of the current subpath, and the reflection
    // a smooth command needs — which is `None` unless the previous command was
    // of the matching family, per §8.3.6.
    let mut pen = [0.0f64; 2];
    let mut start = [0.0f64; 2];
    let mut cubic_reflect: Option<[f64; 2]> = None;
    let mut quad_reflect: Option<[f64; 2]> = None;
    let mut previous: Option<u8> = None;

    scan.spaces();
    // §8.3.2: the first command must be a moveto. A `d` that begins anywhere
    // else is in error before it has drawn anything, so there is no prefix to
    // keep and it is a refusal rather than a short outline.
    if !matches!(scan.peek(), Some(b'M' | b'm')) {
        return if scan.at >= scan.bytes.len() {
            Ok(out)
        } else {
            Err(PathError::Syntax)
        };
    }

    while let Some(byte) = scan.peek() {
        let command = if byte.is_ascii_alphabetic() {
            scan.at += 1;
            previous = Some(byte);
            byte
        } else {
            // §8.3.2's repetition: numbers after a command repeat it, except
            // that a repeated moveto is an implicit lineto.
            match previous {
                Some(b'M') => b'L',
                Some(b'm') => b'l',
                Some(other) => other,
                None => return Err(PathError::Syntax),
            }
        };
        let relative = command.is_ascii_lowercase();
        let absolute = |pen: [f64; 2], x: f64, y: f64| {
            if relative {
                [pen[0] + x, pen[1] + y]
            } else {
                [x, y]
            }
        };

        let mut push = |out: &mut Outline, segment: Segment| -> Result<(), PathError> {
            if *budget == 0 {
                return Err(PathError::TooManySegments);
            }
            *budget -= 1;
            out.segments.push(segment);
            Ok(())
        };

        match command.to_ascii_uppercase() {
            b'Z' => {
                push(&mut out, Segment::Close)?;
                pen = start;
                cubic_reflect = None;
                quad_reflect = None;
            }
            b'M' => {
                let Some(n) = scan.numbers(2) else { break };
                pen = absolute(pen, n[0], n[1]);
                start = pen;
                push(&mut out, Segment::Move(pen))?;
                cubic_reflect = None;
                quad_reflect = None;
            }
            b'L' => {
                let Some(n) = scan.numbers(2) else { break };
                pen = absolute(pen, n[0], n[1]);
                push(&mut out, Segment::Line(pen))?;
                cubic_reflect = None;
                quad_reflect = None;
            }
            b'H' => {
                let Some(n) = scan.numbers(1) else { break };
                pen = if relative {
                    [pen[0] + n[0], pen[1]]
                } else {
                    [n[0], pen[1]]
                };
                push(&mut out, Segment::Line(pen))?;
                cubic_reflect = None;
                quad_reflect = None;
            }
            b'V' => {
                let Some(n) = scan.numbers(1) else { break };
                pen = if relative {
                    [pen[0], pen[1] + n[0]]
                } else {
                    [pen[0], n[0]]
                };
                push(&mut out, Segment::Line(pen))?;
                cubic_reflect = None;
                quad_reflect = None;
            }
            b'C' => {
                let Some(n) = scan.numbers(6) else { break };
                let a = absolute(pen, n[0], n[1]);
                let b = absolute(pen, n[2], n[3]);
                let end = absolute(pen, n[4], n[5]);
                push(&mut out, Segment::Cubic(a, b, end))?;
                cubic_reflect = Some(b);
                quad_reflect = None;
                pen = end;
            }
            b'S' => {
                let Some(n) = scan.numbers(4) else { break };
                // §8.3.6: the first control is the reflection of the previous
                // command's second, **or the current point** when the previous
                // command was not a cubic. Getting that fallback wrong makes a
                // smooth curve that follows a line bulge.
                let a = reflect(pen, cubic_reflect);
                let b = absolute(pen, n[0], n[1]);
                let end = absolute(pen, n[2], n[3]);
                push(&mut out, Segment::Cubic(a, b, end))?;
                cubic_reflect = Some(b);
                quad_reflect = None;
                pen = end;
            }
            b'Q' => {
                let Some(n) = scan.numbers(4) else { break };
                let control = absolute(pen, n[0], n[1]);
                let end = absolute(pen, n[2], n[3]);
                let (a, b) = raise(pen, control, end);
                push(&mut out, Segment::Cubic(a, b, end))?;
                quad_reflect = Some(control);
                cubic_reflect = None;
                pen = end;
            }
            b'T' => {
                let Some(n) = scan.numbers(2) else { break };
                let control = reflect(pen, quad_reflect);
                let end = absolute(pen, n[0], n[1]);
                let (a, b) = raise(pen, control, end);
                push(&mut out, Segment::Cubic(a, b, end))?;
                quad_reflect = Some(control);
                cubic_reflect = None;
                pen = end;
            }
            b'A' => {
                let Some(n) = scan.arc() else { break };
                let end = absolute(pen, n.5, n.6);
                for (a, b, c) in arc_to_curves(pen, [n.0, n.1], n.2, n.3, n.4, end) {
                    push(&mut out, Segment::Cubic(a, b, c))?;
                }
                cubic_reflect = None;
                quad_reflect = None;
                pen = end;
            }
            _ => break,
        }
        scan.separators();
    }
    Ok(out)
}

/// §8.3.6's reflection: the previous control mirrored through the current
/// point, or the current point itself when there is nothing to mirror.
fn reflect(pen: [f64; 2], previous: Option<[f64; 2]>) -> [f64; 2] {
    match previous {
        Some(p) => [2.0 * pen[0] - p[0], 2.0 * pen[1] - p[1]],
        None => pen,
    }
}

/// A quadratic's two cubic controls. Exact, not an approximation: a degree
/// elevation puts each control two thirds of the way from its own end.
fn raise(from: [f64; 2], control: [f64; 2], to: [f64; 2]) -> ([f64; 2], [f64; 2]) {
    const TWO_THIRDS: f64 = 2.0 / 3.0;
    (
        [
            from[0] + TWO_THIRDS * (control[0] - from[0]),
            from[1] + TWO_THIRDS * (control[1] - from[1]),
        ],
        [
            to[0] + TWO_THIRDS * (control[0] - to[0]),
            to[1] + TWO_THIRDS * (control[1] - to[1]),
        ],
    )
}

/// §F.6.5's endpoint-to-centre conversion, and §F.6.6's out-of-range
/// corrections, turned into cubic Béziers.
///
/// The arc is cut into segments of at most a quarter turn, because a cubic
/// approximates a circular arc well below ninety degrees and badly above it.
/// The magic constant is the standard one: for a sweep of `θ`, the control
/// handles are `4/3 · tan(θ/4)` of the radius along the tangents, which is
/// exact at the endpoints and at the midpoint.
///
/// **§F.6.6's corrections are not optional and each has a file behind it.** A
/// zero radius makes the arc a straight line (F.6.6.1); negative radii are
/// taken as their absolute values; and radii too small to span the endpoints
/// are scaled *up* until they exactly do (F.6.6.2), which is what stops a
/// rounding error in somebody's exporter from producing no arc at all.
///
/// **Public because [`crate::shape`] draws every curved basic shape with it.**
/// §9.2's rounded rectangle, §9.3's circle and §9.4's ellipse are each defined
/// by the specification as arcs, and a second quarter-turn approximation
/// written beside this one would be a second answer to how round a corner is —
/// visible only where the two meet.
pub fn arc_to_curves(
    from: [f64; 2],
    radii: [f64; 2],
    rotation_degrees: f64,
    large: bool,
    sweep: bool,
    to: [f64; 2],
) -> Vec<([f64; 2], [f64; 2], [f64; 2])> {
    // F.6.2: identical endpoints mean the arc is omitted entirely.
    if (from[0] - to[0]).abs() < f64::EPSILON && (from[1] - to[1]).abs() < f64::EPSILON {
        return Vec::new();
    }
    let (mut rx, mut ry) = (radii[0].abs(), radii[1].abs());
    // F.6.6.1: a zero radius is a straight line, which a single cubic with its
    // controls on the chord draws exactly.
    if rx == 0.0 || ry == 0.0 {
        return vec![(from, to, to)];
    }
    let phi = math::to_radians(rotation_degrees % 360.0);
    let (sin_phi, cos_phi) = (math::sin(phi), math::cos(phi));

    // F.6.5.1: the endpoints in the ellipse's own frame.
    let dx = (from[0] - to[0]) / 2.0;
    let dy = (from[1] - to[1]) / 2.0;
    let x1 = cos_phi * dx + sin_phi * dy;
    let y1 = -sin_phi * dx + cos_phi * dy;

    // F.6.6.2: grow the radii until they can span the chord.
    let lambda = (x1 * x1) / (rx * rx) + (y1 * y1) / (ry * ry);
    if lambda > 1.0 {
        let scale = lambda.sqrt();
        rx *= scale;
        ry *= scale;
    }

    // F.6.5.2: the centre, in the ellipse's frame.
    let numerator = (rx * rx * ry * ry) - (rx * rx * y1 * y1) - (ry * ry * x1 * x1);
    let denominator = (rx * rx * y1 * y1) + (ry * ry * x1 * x1);
    let mut factor = if denominator == 0.0 {
        0.0
    } else {
        (numerator / denominator).max(0.0).sqrt()
    };
    if large == sweep {
        factor = -factor;
    }
    let cx1 = factor * rx * y1 / ry;
    let cy1 = -factor * ry * x1 / rx;

    // F.6.5.3: back to user space.
    let cx = cos_phi * cx1 - sin_phi * cy1 + (from[0] + to[0]) / 2.0;
    let cy = sin_phi * cx1 + cos_phi * cy1 + (from[1] + to[1]) / 2.0;

    // F.6.5.5 and F.6.5.6: the start angle and the sweep.
    let start = math::atan2((y1 - cy1) / ry, (x1 - cx1) / rx);
    let end = math::atan2((-y1 - cy1) / ry, (-x1 - cx1) / rx);
    let mut delta = end - start;
    let tau = core::f64::consts::PI * 2.0;
    if !sweep && delta > 0.0 {
        delta -= tau;
    } else if sweep && delta < 0.0 {
        delta += tau;
    }

    // A quarter turn at most per curve; `ceil` so a sweep of exactly a quarter
    // stays one segment.
    let count = ((delta.abs() / (core::f64::consts::FRAC_PI_2)).ceil() as usize).max(1);
    let step = delta / count as f64;
    let handle = 4.0 / 3.0 * math::tan(step / 4.0);

    let point = |angle: f64| -> [f64; 2] {
        let (sin_a, cos_a) = (math::sin(angle), math::cos(angle));
        [
            cx + rx * cos_a * cos_phi - ry * sin_a * sin_phi,
            cy + rx * cos_a * sin_phi + ry * sin_a * cos_phi,
        ]
    };
    let tangent = |angle: f64| -> [f64; 2] {
        let (sin_a, cos_a) = (math::sin(angle), math::cos(angle));
        [
            -rx * sin_a * cos_phi - ry * cos_a * sin_phi,
            -rx * sin_a * sin_phi + ry * cos_a * cos_phi,
        ]
    };

    let mut out = Vec::with_capacity(count);
    for index in 0..count {
        let a0 = start + step * index as f64;
        let a1 = a0 + step;
        let p0 = point(a0);
        let p1 = point(a1);
        let t0 = tangent(a0);
        let t1 = tangent(a1);
        out.push((
            [p0[0] + handle * t0[0], p0[1] + handle * t0[1]],
            [p1[0] - handle * t1[0], p1[1] - handle * t1[1]],
            // The last endpoint is the command's own, exactly, rather than the
            // one the angles reproduce: a rounding error at the join of two
            // arcs is a visible gap, and the file already said where it ends.
            if index + 1 == count { to } else { p1 },
        ));
    }
    out
}

/// A cursor over path data.
struct Scan<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl Scan<'_> {
    fn peek(&mut self) -> Option<u8> {
        self.spaces();
        self.bytes.get(self.at).copied()
    }

    fn spaces(&mut self) {
        while matches!(
            self.bytes.get(self.at),
            Some(b' ' | b'\t' | b'\r' | b'\n' | b',')
        ) {
            self.at += 1;
        }
    }

    fn separators(&mut self) {
        self.spaces();
    }

    /// One number, in SVG's grammar rather than Rust's.
    fn number(&mut self) -> Option<f64> {
        self.spaces();
        let start = self.at;
        if matches!(self.bytes.get(self.at), Some(b'+' | b'-')) {
            self.at += 1;
        }
        let mut digits = 0usize;
        while matches!(self.bytes.get(self.at), Some(b) if b.is_ascii_digit()) {
            self.at += 1;
            digits += 1;
        }
        if self.bytes.get(self.at) == Some(&b'.') {
            self.at += 1;
            while matches!(self.bytes.get(self.at), Some(b) if b.is_ascii_digit()) {
                self.at += 1;
                digits += 1;
            }
        }
        if digits == 0 {
            self.at = start;
            return None;
        }
        if matches!(self.bytes.get(self.at), Some(b'e' | b'E')) {
            let mark = self.at;
            self.at += 1;
            if matches!(self.bytes.get(self.at), Some(b'+' | b'-')) {
                self.at += 1;
            }
            let mut exponent = 0usize;
            while matches!(self.bytes.get(self.at), Some(b) if b.is_ascii_digit()) {
                self.at += 1;
                exponent += 1;
            }
            if exponent == 0 {
                self.at = mark;
            }
        }
        let text = core::str::from_utf8(self.bytes.get(start..self.at)?).ok()?;
        let value: f64 = text.parse().ok()?;
        value.is_finite().then_some(value)
    }

    fn numbers(&mut self, count: usize) -> Option<Vec<f64>> {
        let mark = self.at;
        let mut out = Vec::with_capacity(count);
        for _ in 0..count {
            match self.number() {
                Some(value) => out.push(value),
                None => {
                    self.at = mark;
                    return None;
                }
            }
        }
        Some(out)
    }

    /// An arc's seven arguments.
    ///
    /// The two flags are **one character each** and are not numbers: §8.3.8's
    /// grammar makes `a1 1 0 1130` a valid arc with flags `1` and `1` and an
    /// x of `30`, where a number scan would read `1130`. That is the classic
    /// path-parser defect and it is why this is not `numbers(7)`.
    fn arc(&mut self) -> Option<(f64, f64, f64, bool, bool, f64, f64)> {
        let mark = self.at;
        let fail = |scan: &mut Self| {
            scan.at = mark;
            None::<(f64, f64, f64, bool, bool, f64, f64)>
        };
        let Some(rx) = self.number() else {
            return fail(self);
        };
        let Some(ry) = self.number() else {
            return fail(self);
        };
        let Some(rotation) = self.number() else {
            return fail(self);
        };
        let Some(large) = self.flag() else {
            return fail(self);
        };
        let Some(sweep) = self.flag() else {
            return fail(self);
        };
        let Some(x) = self.number() else {
            return fail(self);
        };
        let Some(y) = self.number() else {
            return fail(self);
        };
        Some((rx, ry, rotation, large, sweep, x, y))
    }

    /// One flag: exactly the character `0` or `1`.
    fn flag(&mut self) -> Option<bool> {
        self.spaces();
        match self.bytes.get(self.at) {
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
