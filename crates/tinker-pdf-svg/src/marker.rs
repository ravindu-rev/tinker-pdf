//! §11.6's markers: where a path's vertices are, and which way it points there.
//!
//! # A vertex is where a command ends
//!
//! §11.6.2 draws `marker-start` at a path's first vertex, `marker-end` at its
//! last, and `marker-mid` at every other — and a vertex is a point the *path
//! data* names, not one this crate made. An elliptical arc is up to four
//! cubics here (path.rs cuts it into quarter turns, because PDF has no arc), so
//! a list of segments over-counts vertices inside every arc; [`vertices`]
//! takes the command boundaries [`crate::path::parse_commands`] reports and
//! treats each command's run of segments as one piece of the path.
//!
//! # Direction
//!
//! `orient="auto"` turns a marker to the path's direction at the vertex, which
//! where two segments meet at an angle is the **bisector** of the direction
//! coming in and the direction going out. The direction of a segment at an end
//! is its tangent there: a line's own direction, a cubic's first control
//! minus its start (and the next point along if that control sits on the
//! start, which a quadratic raised to a cubic never does but a hand-written
//! `C` may). A closed subpath's first vertex has an incoming direction too —
//! the closing segment's — so a marker on the corner of a closed square is
//! turned to the corner's bisector, not to the first side. That is SVG 2's
//! rule (§13.7.3's "path directionality") and the reading every renderer
//! shares, where SVG 1.1 is silent.

use tinker_pdf_math as math;

use crate::path::{Outline, Segment};

/// One vertex of a path, in the path's own user space.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Vertex {
    /// Where it is.
    pub at: [f64; 2],
    /// The direction the path arrives in, or `None` where nothing arrives.
    pub inward: Option<[f64; 2]>,
    /// The direction the path leaves in, or `None` where nothing leaves.
    pub outward: Option<[f64; 2]>,
}

/// §11.6.2's `orient`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Orient {
    /// `auto`: the path's direction.
    Auto,
    /// SVG 2's `auto-start-reverse`: the path's direction, turned half a turn
    /// at the start vertex — so one arrowhead marker points outward at both
    /// ends of a line.
    AutoStartReverse,
    /// A fixed angle, in radians.
    Angle(f64),
}

/// Reads `orient`; `None` for a value that is not its grammar.
///
/// §11.6.2's initial value is the angle zero. An angle is a number with an
/// optional `deg`, `grad`, `rad` or (CSS's) `turn`; a bare number is degrees.
#[must_use]
pub fn orient(text: Option<&str>) -> Option<Orient> {
    let Some(text) = text.map(str::trim) else {
        return Some(Orient::Angle(0.0));
    };
    match text {
        "auto" => return Some(Orient::Auto),
        "auto-start-reverse" => return Some(Orient::AutoStartReverse),
        _ => {}
    }
    let (number, scale) = if let Some(n) = text.strip_suffix("deg") {
        (n, core::f64::consts::PI / 180.0)
    } else if let Some(n) = text.strip_suffix("grad") {
        (n, core::f64::consts::PI / 200.0)
    } else if let Some(n) = text.strip_suffix("rad") {
        (n, 1.0)
    } else if let Some(n) = text.strip_suffix("turn") {
        (n, 2.0 * core::f64::consts::PI)
    } else {
        (text, core::f64::consts::PI / 180.0)
    };
    let value: f64 = number.trim().parse().ok()?;
    let radians = value * scale;
    radians.is_finite().then_some(Orient::Angle(radians))
}

/// The angle a marker at `vertex` is turned by, in radians.
///
/// `first` says whether this is the path's first vertex, which is the one
/// `auto-start-reverse` turns around.
#[must_use]
pub fn angle(vertex: &Vertex, orient: Orient, first: bool) -> f64 {
    let auto = || -> f64 {
        let direction = |d: [f64; 2]| math::atan2(d[1], d[0]);
        match (vertex.inward, vertex.outward) {
            (Some(inward), Some(outward)) => {
                let a = direction(inward);
                let b = direction(outward);
                // Half the turn from one to the other, taken the short way
                // round: a path that doubles back by 350 degrees has turned
                // by -10, and its bisector is five degrees off the incoming
                // side rather than 175.
                let mut turn = b - a;
                let pi = core::f64::consts::PI;
                while turn > pi {
                    turn -= 2.0 * pi;
                }
                while turn <= -pi {
                    turn += 2.0 * pi;
                }
                a + turn / 2.0
            }
            (Some(only), None) | (None, Some(only)) => direction(only),
            (None, None) => 0.0,
        }
    };
    match orient {
        Orient::Angle(radians) => radians,
        Orient::Auto => auto(),
        Orient::AutoStartReverse => {
            if first {
                auto() + core::f64::consts::PI
            } else {
                auto()
            }
        }
    }
}

/// A direction, or `None` for one of no length.
fn direction(from: [f64; 2], to: [f64; 2]) -> Option<[f64; 2]> {
    let d = [to[0] - from[0], to[1] - from[1]];
    (d[0] != 0.0 || d[1] != 0.0).then_some(d)
}

/// Every vertex of an outline, in order.
///
/// `ends` is [`crate::path::parse_commands`]'s list of command boundaries, or
/// `None` where every segment is its own command — which is true of
/// `<line>`, `<polyline>` and `<polygon>`, whose outlines are lines and
/// nothing else.
#[must_use]
pub fn vertices(outline: &Outline, ends: Option<&[usize]>) -> Vec<Vertex> {
    let ends_here = |index: usize| ends.is_none_or(|ends| ends.binary_search(&index).is_ok());
    let segments = &outline.segments;
    let mut out: Vec<Vertex> = Vec::new();
    let mut current = [0.0f64; 2];
    let mut start = [0.0f64; 2];
    let mut opening = 0usize;
    let mut index = 0usize;
    while let Some(segment) = segments.get(index) {
        match *segment {
            Segment::Move(at) => {
                out.push(Vertex {
                    at,
                    inward: None,
                    outward: None,
                });
                opening = out.len() - 1;
                current = at;
                start = at;
                index += 1;
            }
            Segment::Close => {
                // §8.3.3: a close is a line back to the subpath's start. One of
                // no length has no direction of its own, and arrives the way
                // the segment before it did.
                let closing = direction(current, start);
                let inward = closing.or_else(|| out.last().and_then(|last| last.inward));
                if closing.is_some() {
                    if let Some(last) = out.last_mut() {
                        last.outward = last.outward.or(closing);
                    }
                }
                let leaves = out.get(opening).and_then(|first| first.outward);
                if let Some(first) = out.get_mut(opening) {
                    first.inward = inward;
                }
                out.push(Vertex {
                    at: start,
                    inward,
                    outward: leaves,
                });
                current = start;
                index += 1;
            }
            Segment::Line(_) | Segment::Cubic(..) => {
                // One command's run: from here to the segment that ends it.
                let mut last = index;
                while !ends_here(last)
                    && matches!(
                        segments.get(last + 1),
                        Some(Segment::Line(_) | Segment::Cubic(..))
                    )
                {
                    last += 1;
                }
                // `index` is in range because `segments.get(index)` answered,
                // and `last` only moved on while `segments.get(last + 1)` did,
                // so both reads below would succeed indexed — they are made
                // with `get` anyway, falling back to the run's first segment.
                let leaves = start_direction(*segment, current);
                let mut before = current;
                for piece in segments.get(index..last).unwrap_or_default() {
                    before = end_point(*piece).unwrap_or(before);
                }
                let closing = segments.get(last).copied().unwrap_or(*segment);
                let end = end_point(closing).unwrap_or(before);
                let arrives = end_direction(closing, before);
                if let Some(previous) = out.last_mut() {
                    previous.outward = previous.outward.or(leaves);
                } else {
                    // A path that does not begin with a move is not §8.3.2's
                    // grammar and `path::parse` refuses it; an outline built
                    // some other way still gets a vertex where it starts.
                    out.push(Vertex {
                        at: current,
                        inward: None,
                        outward: leaves,
                    });
                }
                out.push(Vertex {
                    at: end,
                    inward: arrives,
                    outward: None,
                });
                current = end;
                index = last + 1;
            }
        }
    }
    out
}

fn end_point(segment: Segment) -> Option<[f64; 2]> {
    match segment {
        Segment::Move(p) | Segment::Line(p) | Segment::Cubic(_, _, p) => Some(p),
        Segment::Close => None,
    }
}

/// A segment's direction where it starts, from `from`.
fn start_direction(segment: Segment, from: [f64; 2]) -> Option<[f64; 2]> {
    match segment {
        Segment::Line(to) => direction(from, to),
        Segment::Cubic(a, b, c) => direction(from, a)
            .or_else(|| direction(from, b))
            .or_else(|| direction(from, c)),
        _ => None,
    }
}

/// A segment's direction where it ends, having started at `from`.
fn end_direction(segment: Segment, from: [f64; 2]) -> Option<[f64; 2]> {
    match segment {
        Segment::Line(to) => direction(from, to),
        Segment::Cubic(a, b, c) => direction(b, c)
            .or_else(|| direction(a, c))
            .or_else(|| direction(from, c)),
        _ => None,
    }
}
