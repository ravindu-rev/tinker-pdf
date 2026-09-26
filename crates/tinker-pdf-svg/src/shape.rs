//! SVG 1.1 §9's basic shapes, every one of them an [`Outline`].
//!
//! # There is one geometry representation and this is not a second one
//!
//! A rectangle is four lines, a circle is four arcs, and both are already
//! spelled in [`crate::path::Segment`]. Keeping a `Rect { x, y, w, h }` variant
//! beside them would buy nothing a consumer wants — it would still have to be
//! turned into a path to be filled, stroked, clipped or transformed — and it
//! would cost the thing that matters: a `transform` with a skew in it turns a
//! rectangle into a parallelogram, so the variant would be a lie the moment a
//! matrix touched it.
//!
//! The curved shapes go through [`crate::path::arc_to_curves`] rather than
//! through a quarter-turn constant written out here, for the same reason. The
//! specification defines a rounded corner and a circle as *arcs*; two
//! approximations of an arc in one crate would differ in the fourth decimal,
//! and the difference would be visible only where a rounded rectangle meets a
//! circle drawn to the same radius.

use crate::document::{self, Node};
use crate::path::{arc_to_curves, Outline, Segment};

/// What a shape element produced.
///
/// Three answers rather than two, because §9's error rules distinguish them.
/// A rectangle of zero width *"disables rendering"* — it is not an error and
/// there is nothing to say about it — while a `cx` that is not a number is a
/// value the author got wrong, which is [`crate::Warning::ValueUnreadable`] at
/// the call site.
pub enum Shape {
    /// Geometry to draw.
    Outline(Outline),
    /// The element is legal and draws nothing.
    Nothing,
    /// An attribute is not the grammar its property states; the name is the
    /// one to warn about.
    Unreadable(&'static str),
}

/// A shape's geometry, in its own user space.
///
/// `viewport` is what a percentage resolves against, as §7.10 requires: the
/// width for an `x`-ish length, the height for a `y`-ish one, and the
/// normalised diagonal for a radius.
///
/// `degraded` collects the attributes that were **read, found wrong, and
/// worked around** — §9.2's negative radius is the one that exists. It is an
/// out-parameter rather than a [`Shape`] variant because degrading is not an
/// alternative to producing geometry: the whole point is that the element still
/// draws, and the caller still gets to say what was ignored (ruling 10).
#[must_use]
pub fn outline(
    node: &Node,
    viewport: (f64, f64),
    degraded: &mut Vec<&'static str>,
) -> Option<Shape> {
    match node.name.as_str() {
        "path" => Some(path(node)),
        "rect" => Some(rect(node, viewport, degraded)),
        "circle" => Some(circle(node, viewport)),
        "ellipse" => Some(ellipse_element(node, viewport)),
        "line" => Some(line(node, viewport)),
        "polyline" => Some(polygon(node, false)),
        "polygon" => Some(polygon(node, true)),
        _ => None,
    }
}

/// One length attribute, or a default when it is absent.
fn length(node: &Node, name: &'static str, basis: f64, default: f64) -> Result<f64, &'static str> {
    match node.attr(name) {
        None => Ok(default),
        Some(text) => document::length(text, Some(basis)).ok_or(name),
    }
}

/// §8.3's `<path>`.
///
/// A `d` this crate cannot read at all is a *syntax* error rather than an
/// unreadable attribute, and §8.3.2's own rule is followed: the data up to the
/// point of the error is rendered, so only a `d` from which **nothing** parsed
/// reaches [`Shape::Unreadable`].
fn path(node: &Node) -> Shape {
    let Some(data) = node.attr("d") else {
        // §8.3.1 makes `d` required; an element without one is not an error a
        // reader can do anything about and it draws nothing.
        return Shape::Nothing;
    };
    // A budget of its own, checked by the caller against the document's: this
    // function answers what the data says and the walk answers what it costs.
    let mut budget = usize::MAX;
    match crate::path::parse(data, &mut budget) {
        Ok(outline) if outline.segments.is_empty() => Shape::Nothing,
        Ok(outline) => Shape::Outline(outline),
        Err(_) => Shape::Unreadable("d"),
    }
}

/// §9.2's `<rect>`, with §9.2's own `rx`/`ry` rules.
///
/// The three of them, and each is a real file:
///
/// - **One radius given, the other absent**: the absent one takes the given
///   one's value. A build that defaulted it to zero would draw a shape with two
///   square corners and two round ones.
/// - **A radius past half the side**: clamped to half, so the corners meet
///   rather than crossing. Illustrator writes `rx` larger than the rectangle
///   when a shape is scaled down.
/// - **A negative radius**: §9.2 calls it an error. Ruling 2 says degrade, so it
///   is treated as absent and the attribute is named, which draws the rectangle
///   the file is about instead of losing it.
fn rect(node: &Node, viewport: (f64, f64), degraded: &mut Vec<&'static str>) -> Shape {
    let (x, y, width, height) = match (
        length(node, "x", viewport.0, 0.0),
        length(node, "y", viewport.1, 0.0),
        length(node, "width", viewport.0, 0.0),
        length(node, "height", viewport.1, 0.0),
    ) {
        (Ok(x), Ok(y), Ok(width), Ok(height)) => (x, y, width, height),
        (Err(name), ..) | (_, Err(name), ..) | (_, _, Err(name), _) | (_, _, _, Err(name)) => {
            return Shape::Unreadable(name)
        }
    };
    // §9.2: a zero or negative width or height disables rendering of the
    // element. Not an error — a rectangle of no area.
    if !(width > 0.0 && height > 0.0) {
        return Shape::Nothing;
    }

    // §9.2's negative radius is *"an error"*, and ruling 2 answers it by
    // drawing the rectangle the file is about rather than losing it: the radius
    // is treated as absent and the attribute is named.
    let mut radius = |name: &'static str, basis: f64| -> Option<f64> {
        match node.attr(name) {
            None => None,
            // SVG 2's spelling of "no radius stated", which means here what an
            // absent attribute means.
            Some(text) if text.trim() == "auto" => None,
            Some(text) => match document::length(text, Some(basis)) {
                Some(value) if value >= 0.0 => Some(value),
                _ => {
                    if !degraded.contains(&name) {
                        degraded.push(name);
                    }
                    None
                }
            },
        }
    };
    let given_x = radius("rx", viewport.0);
    let given_y = radius("ry", viewport.1);
    let mut rx = given_x.or(given_y).unwrap_or(0.0);
    let mut ry = given_y.or(given_x).unwrap_or(0.0);
    rx = rx.min(width / 2.0);
    ry = ry.min(height / 2.0);

    let mut out = Vec::new();
    if rx <= 0.0 || ry <= 0.0 {
        out.push(Segment::Move([x, y]));
        out.push(Segment::Line([x + width, y]));
        out.push(Segment::Line([x + width, y + height]));
        out.push(Segment::Line([x, y + height]));
        out.push(Segment::Close);
        return Shape::Outline(Outline { segments: out });
    }
    // §9.2's construction, verbatim: start after the top-left corner, run each
    // side, and turn each corner with a quarter arc in the sweep direction.
    let corner = |out: &mut Vec<Segment>, from: [f64; 2], to: [f64; 2]| {
        for (a, b, c) in arc_to_curves(from, [rx, ry], 0.0, false, true, to) {
            out.push(Segment::Cubic(a, b, c));
        }
    };
    out.push(Segment::Move([x + rx, y]));
    out.push(Segment::Line([x + width - rx, y]));
    corner(&mut out, [x + width - rx, y], [x + width, y + ry]);
    out.push(Segment::Line([x + width, y + height - ry]));
    corner(
        &mut out,
        [x + width, y + height - ry],
        [x + width - rx, y + height],
    );
    out.push(Segment::Line([x + rx, y + height]));
    corner(&mut out, [x + rx, y + height], [x, y + height - ry]);
    out.push(Segment::Line([x, y + ry]));
    corner(&mut out, [x, y + ry], [x + rx, y]);
    out.push(Segment::Close);
    Shape::Outline(Outline { segments: out })
}

/// §9.3's `<circle>`.
fn circle(node: &Node, viewport: (f64, f64)) -> Shape {
    let diagonal = document::diagonal(viewport.0, viewport.1);
    let (cx, cy, r) = match (
        length(node, "cx", viewport.0, 0.0),
        length(node, "cy", viewport.1, 0.0),
        length(node, "r", diagonal, 0.0),
    ) {
        (Ok(cx), Ok(cy), Ok(r)) => (cx, cy, r),
        (Err(name), ..) | (_, Err(name), _) | (_, _, Err(name)) => return Shape::Unreadable(name),
    };
    // §9.3: `r` of zero disables rendering; a negative one is an error, and
    // ruling 2 draws nothing rather than losing the document.
    if r <= 0.0 {
        return Shape::Nothing;
    }
    Shape::Outline(Outline {
        segments: ellipse(cx, cy, r, r),
    })
}

/// §9.4's `<ellipse>`.
fn ellipse_element(node: &Node, viewport: (f64, f64)) -> Shape {
    let (cx, cy, rx, ry) = match (
        length(node, "cx", viewport.0, 0.0),
        length(node, "cy", viewport.1, 0.0),
        length(node, "rx", viewport.0, 0.0),
        length(node, "ry", viewport.1, 0.0),
    ) {
        (Ok(cx), Ok(cy), Ok(rx), Ok(ry)) => (cx, cy, rx, ry),
        (Err(name), ..) | (_, Err(name), ..) | (_, _, Err(name), _) | (_, _, _, Err(name)) => {
            return Shape::Unreadable(name)
        }
    };
    if rx <= 0.0 || ry <= 0.0 {
        return Shape::Nothing;
    }
    Shape::Outline(Outline {
        segments: ellipse(cx, cy, rx, ry),
    })
}

/// A whole ellipse, as two half-turn arcs.
///
/// Two rather than four, and [`arc_to_curves`] cuts each into quarters itself —
/// so the curve is the same one an `A` command produces for the same geometry,
/// which is what stops a `<circle>` and a `<path>` drawn to the same radius
/// from disagreeing in the fourth decimal.
fn ellipse(cx: f64, cy: f64, rx: f64, ry: f64) -> Vec<Segment> {
    let right = [cx + rx, cy];
    let left = [cx - rx, cy];
    let mut out = vec![Segment::Move(right)];
    for to in [left, right] {
        let from = match out.last() {
            Some(Segment::Move(p)) => *p,
            Some(Segment::Cubic(_, _, p)) => *p,
            _ => right,
        };
        for (a, b, c) in arc_to_curves(from, [rx, ry], 0.0, false, true, to) {
            out.push(Segment::Cubic(a, b, c));
        }
    }
    out.push(Segment::Close);
    out
}

/// §9.5's `<line>`.
fn line(node: &Node, viewport: (f64, f64)) -> Shape {
    let (x1, y1, x2, y2) = match (
        length(node, "x1", viewport.0, 0.0),
        length(node, "y1", viewport.1, 0.0),
        length(node, "x2", viewport.0, 0.0),
        length(node, "y2", viewport.1, 0.0),
    ) {
        (Ok(a), Ok(b), Ok(c), Ok(d)) => (a, b, c, d),
        (Err(name), ..) | (_, Err(name), ..) | (_, _, Err(name), _) | (_, _, _, Err(name)) => {
            return Shape::Unreadable(name)
        }
    };
    Shape::Outline(Outline {
        segments: vec![Segment::Move([x1, y1]), Segment::Line([x2, y2])],
    })
}

/// §9.6's `<polyline>` and §9.7's `<polygon>`, which differ by one `Z`.
///
/// §9.7's error rule is §8.3.2's: *"the points specified up to the point of the
/// error are rendered"*. So an odd number of coordinates draws the pairs that
/// were complete and the dangling one is dropped, rather than the shape being
/// lost.
fn polygon(node: &Node, close: bool) -> Shape {
    let Some(text) = node.attr("points") else {
        return Shape::Nothing;
    };
    let Some(numbers) = crate::transform::numbers(text) else {
        return Shape::Unreadable("points");
    };
    if numbers.len() < 2 {
        return Shape::Nothing;
    }
    let mut segments = vec![Segment::Move([numbers[0], numbers[1]])];
    for pair in numbers[2..].chunks_exact(2) {
        segments.push(Segment::Line([pair[0], pair[1]]));
    }
    if close {
        segments.push(Segment::Close);
    }
    Shape::Outline(Outline { segments })
}
