//! Appearance-stream synthesis for annotations (12.5.5).
//!
//! An annotation without an `/AP` entry is at the mercy of whatever the viewer
//! decides to draw, and viewers disagree — some invent a plausible appearance,
//! some draw nothing at all, and printing usually gets the worst of it. An
//! annotation with one looks the same everywhere, which is the whole point of
//! writing it.
//!
//! Every appearance here is a Form XObject whose `/BBox` equals the
//! annotation's `/Rect` and whose `/Matrix` is the identity. 12.5.5's
//! algorithm maps the transformed bounding box onto the rectangle, so those
//! two choices together make the mapping the identity and let the content be
//! written in ordinary page coordinates. Any other choice means computing that
//! map, getting it slightly wrong, and shipping annotations that drift.
//!
//! # What is drawn
//!
//! A subtype is drawn when its dictionary determines its appearance — when
//! the geometry is in its own entries and 12.5.6 says what to do with it:
//!
//! - `Highlight`, `Underline`, `StrikeOut`, `Square`, `Circle` and `Text`, the
//!   first six, exactly as they were drawn before anything else was added
//!   (`the_seven_first_subtypes_are_drawn_exactly_as_they_were`); `Link`, the
//!   seventh, draws nothing.
//! - `Line` (12.5.6.7): `/L`, Table 176's endings, Figure 60's leader lines.
//!
//! Each carries 12.5.6.2's `/CA` in the graphics state it selects, and a line
//! its `/BS` (or `/Border`) dash.

use crate::doc::CosDocument;
use crate::name::Name;
use crate::object::{Dict, ObjRef, Object};
use crate::pages::Rect;
use crate::write::StreamData;

/// The number of user-space units by which a border straddles its path.
///
/// A stroke is centred on the path, so half of it falls outside the rectangle
/// and would be clipped away by the `/BBox`. Insetting by half the width keeps
/// the whole border visible.
fn inset(rect: Rect, by: f64) -> Rect {
    let by = by.max(0.0);
    // Never inset past the point where the rectangle turns inside out.
    let x = by.min((rect.x1 - rect.x0) / 2.0).max(0.0);
    let y = by.min((rect.y1 - rect.y0) / 2.0).max(0.0);
    Rect {
        x0: rect.x0 + x,
        y0: rect.y0 + y,
        x1: rect.x1 - x,
        y1: rect.y1 - y,
    }
}

fn rect_of(doc: &CosDocument, dict: &Dict) -> Option<Rect> {
    let value = doc.resolve_key(dict, doc.intern(b"Rect"));
    let rect = value.as_array().and_then(Rect::from_array)?;
    (!rect.is_empty()).then_some(rect)
}

/// An annotation colour entry, as `[]`, grey, RGB or CMYK (12.5.2 `/C`).
fn color_of(doc: &CosDocument, dict: &Dict, key: &[u8]) -> Option<[f64; 3]> {
    let value = doc.resolve_key(dict, doc.intern(key));
    let components = value.as_array()?;
    let n: Vec<f64> = components
        .iter()
        .filter_map(Object::as_number)
        .map(|v| v.clamp(0.0, 1.0))
        .collect();
    match n.as_slice() {
        // An empty array means transparent, which is not a colour.
        [] => None,
        [g] => Some([*g, *g, *g]),
        [r, g, b] => Some([*r, *g, *b]),
        [c, m, y, k] => Some([
            (1.0 - c) * (1.0 - k),
            (1.0 - m) * (1.0 - k),
            (1.0 - y) * (1.0 - k),
        ]),
        _ => None,
    }
}

/// The border width, from `/BS /W` or the legacy `/Border` array (12.5.4).
fn border_width(doc: &CosDocument, dict: &Dict) -> f64 {
    if let Some(style) = doc.resolve_key(dict, doc.intern(b"BS")).as_dict() {
        if let Some(width) = style.get_number(doc.intern(b"W")) {
            return width.max(0.0);
        }
    }
    // /Border is [hradius vradius width], so the width is the third entry.
    doc.resolve_key(dict, doc.intern(b"Border"))
        .as_array()
        .and_then(|border| border.get(2))
        .and_then(Object::as_number)
        .map_or(1.0, |w| w.max(0.0))
}

/// The `/QuadPoints` of a markup annotation, as `[x0 y0 .. x3 y3]` quads.
///
/// 12.5.6.10 orders the corners upper-left, upper-right, lower-left,
/// lower-right — neither clockwise nor counter-clockwise, and the reason
/// naively drawn highlights come out bow-tied.
fn quads_of(doc: &CosDocument, dict: &Dict) -> Vec<[f64; 8]> {
    let value = doc.resolve_key(dict, doc.intern(b"QuadPoints"));
    let Some(points) = value.as_array() else {
        return Vec::new();
    };
    let numbers: Vec<f64> = points.iter().filter_map(Object::as_number).collect();
    numbers
        .chunks_exact(8)
        .map(|c| [c[0], c[1], c[2], c[3], c[4], c[5], c[6], c[7]])
        .collect()
}

/// Writes a number the way a content stream wants it: short, and never in
/// exponential notation, which no PDF tokenizer accepts.
fn number(out: &mut Vec<u8>, value: f64) {
    let value = if value.is_finite() { value } else { 0.0 };
    let text = format!("{value:.4}");
    let trimmed = text.trim_end_matches('0').trim_end_matches('.');
    out.extend_from_slice(if trimmed.is_empty() { "0" } else { trimmed }.as_bytes());
}

fn op(out: &mut Vec<u8>, values: &[f64], operator: &[u8]) {
    for value in values {
        number(out, *value);
        out.push(b' ');
    }
    out.extend_from_slice(operator);
    out.push(b'\n');
}

/// A point in default user space.
type Point = (f64, f64);

/// The finite numbers of an array entry, in order; empty when the entry is
/// absent or not an array. A non-number element is dropped rather than read
/// as zero, so a malformed array shortens instead of growing a point at the
/// origin.
fn numbers_of(doc: &CosDocument, dict: &Dict, key: &[u8]) -> Vec<f64> {
    doc.resolve_key(dict, doc.intern(key))
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(Object::as_number)
                .filter(|v| v.is_finite())
                .collect()
        })
        .unwrap_or_default()
}

/// A finite number entry.
fn number_of(doc: &CosDocument, dict: &Dict, key: &[u8]) -> Option<f64> {
    doc.resolve_key(dict, doc.intern(key))
        .as_number()
        .filter(|v| v.is_finite())
}

/// The colour a line-like annotation strokes with: `/C`, or black when the
/// entry is absent — the convention `Underline` and `StrikeOut` already
/// follow here. An empty `/C` is 12.5.2's "transparent", and strokes nothing.
fn stroke_color_of(doc: &CosDocument, dict: &Dict) -> Option<[f64; 3]> {
    match *doc.resolve_key(dict, doc.intern(b"C")) {
        Object::Null => Some([0.0, 0.0, 0.0]),
        _ => color_of(doc, dict, b"C"),
    }
}

/// The dash pattern a border is drawn with (12.5.4 Table 166): `/BS /D` when
/// `/BS /S` is `/D`, defaulting to `[3]`; otherwise the legacy `/Border`'s
/// optional fourth element. `None` is a solid line.
///
/// A pattern 8.4.3.6 would refuse — a negative or non-finite element, or
/// every element zero — is drawn solid: a viewer handed `[0 0] 0 d` draws
/// nothing at all, which is the one thing a border must not become.
fn dash_of(doc: &CosDocument, dict: &Dict) -> Option<Vec<f64>> {
    let pattern = if let Some(style) = doc.resolve_key(dict, doc.intern(b"BS")).as_dict() {
        let dashed = style
            .get_name(doc.intern(b"S"))
            .and_then(|name| doc.name_bytes(name))
            .is_some_and(|name| name.as_ref() == b"D");
        if !dashed {
            return None;
        }
        match doc.resolve_key(style, doc.intern(b"D")).as_array() {
            Some(items) => items
                .iter()
                .map(Object::as_number)
                .collect::<Option<Vec<f64>>>()?,
            None => vec![3.0],
        }
    } else {
        let border = doc.resolve_key(dict, doc.intern(b"Border"));
        let items = border.as_array()?.get(3)?;
        let items = match items {
            Object::Ref(r) => doc.get(*r).ok()?.as_array()?.to_vec(),
            other => other.as_array()?.to_vec(),
        };
        items
            .iter()
            .map(Object::as_number)
            .collect::<Option<Vec<f64>>>()?
    };
    let valid = !pattern.is_empty()
        && pattern.iter().all(|v| v.is_finite() && *v >= 0.0)
        && pattern.iter().any(|v| *v > 0.0);
    valid.then_some(pattern)
}

/// Writes `[a b ...] phase d`.
fn dash(out: &mut Vec<u8>, pattern: &[f64], phase: f64) {
    out.push(b'[');
    for (index, value) in pattern.iter().enumerate() {
        if index > 0 {
            out.push(b' ');
        }
        number(out, *value);
    }
    out.extend_from_slice(b"] ");
    op(out, &[phase], b"d");
}

/// The constant opacity a markup annotation is painted with (12.5.6.2
/// Table 170 `/CA`, and ISO 32000-2's `/ca` for what is filled), as
/// `(stroking, non-stroking)`, when either is below one.
///
/// The appearance carries it as an `ExtGState`, because the renderer — this
/// one and every other — reads opacity from the content, not from the
/// annotation. It is applied per operator, so where a fill and a stroke
/// overlap (the inner half of a border) the two compose; that is the same
/// appearance every producer that writes `/CA` into a `gs` makes.
fn opacity_of(doc: &CosDocument, dict: &Dict) -> Option<(f64, f64)> {
    let stroking = number_of(doc, dict, b"CA").map_or(1.0, |v| v.clamp(0.0, 1.0));
    let filling = number_of(doc, dict, b"ca").map_or(stroking, |v| v.clamp(0.0, 1.0));
    (stroking < 1.0 || filling < 1.0).then_some((stroking, filling))
}

/// A unit vector along `(dx, dy)`, or `None` for one too short to have a
/// direction.
fn unit(dx: f64, dy: f64) -> Option<Point> {
    let length = (dx * dx + dy * dy).sqrt();
    (length.is_finite() && length > 1e-9).then(|| (dx / length, dy / length))
}

/// `tan 30°`, the half-angle of an arrowhead, written out so that drawing one
/// calls no transcendental function (ruling 4).
const TAN_30: f64 = 0.577_350_269_189_625_8;
/// `cos 60°` and `sin 60°`, the slash's angle from the line.
const COS_60: f64 = 0.5;
const SIN_60: f64 = 0.866_025_403_784_438_6;

/// One end of a line, polyline or callout (12.5.6.7 Table 176).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Ending {
    None,
    Square,
    Circle,
    Diamond,
    OpenArrow,
    ClosedArrow,
    Butt,
    ROpenArrow,
    RClosedArrow,
    Slash,
}

impl Ending {
    /// A name Table 176 does not list draws nothing at that end: an ending
    /// is a decoration, and inventing one is worse than leaving it off.
    fn from_name(name: &[u8]) -> Ending {
        match name {
            b"Square" => Ending::Square,
            b"Circle" => Ending::Circle,
            b"Diamond" => Ending::Diamond,
            b"OpenArrow" => Ending::OpenArrow,
            b"ClosedArrow" => Ending::ClosedArrow,
            b"Butt" => Ending::Butt,
            b"ROpenArrow" => Ending::ROpenArrow,
            b"RClosedArrow" => Ending::RClosedArrow,
            b"Slash" => Ending::Slash,
            _ => Ending::None,
        }
    }

    /// The endings Table 176 fills with the interior colour.
    fn is_closed(self) -> bool {
        matches!(
            self,
            Ending::Square
                | Ending::Circle
                | Ending::Diamond
                | Ending::ClosedArrow
                | Ending::RClosedArrow
        )
    }
}

/// `/LE`: the endings at the first and the last point, `None` by default.
fn endings_of(doc: &CosDocument, dict: &Dict) -> (Ending, Ending) {
    let value = doc.resolve_key(dict, doc.intern(b"LE"));
    let Some(items) = value.as_array() else {
        return (Ending::None, Ending::None);
    };
    let at = |index: usize| {
        items
            .get(index)
            .and_then(Object::as_name)
            .and_then(|name| doc.name_bytes(name))
            .map_or(Ending::None, |name| Ending::from_name(&name))
    };
    (at(0), at(1))
}

/// How a line-like annotation is painted: its stroke, its interior and its
/// width, already resolved.
#[derive(Clone, Copy)]
struct Paint {
    stroke: Option<[f64; 3]>,
    fill: Option<[f64; 3]>,
    width: f64,
}

impl Paint {
    fn strokes(&self) -> bool {
        self.stroke.is_some() && self.width > 0.0
    }

    /// The operator that paints a closed path: filled and stroked, filled,
    /// or stroked. `None` when it would paint nothing.
    fn closed(&self) -> Option<&'static [u8]> {
        match (self.fill.is_some(), self.strokes()) {
            (true, true) => Some(b"B\n"),
            (true, false) => Some(b"f\n"),
            (false, true) => Some(b"S\n"),
            (false, false) => None,
        }
    }
}

/// Draws one line ending at `at`, where `out_dir` is the unit vector pointing
/// away from the line — along it at the last point, back along it at the
/// first.
///
/// Table 176 says what each ending is and nothing about its size. Here every
/// one is sized from the line's width, three times it (and never less than
/// three units) from the point to the shape's edge, so a heavier line has a
/// heavier arrowhead: a square or circle six widths across, an arrowhead six
/// widths long, a butt or slash six widths from end to end. `line` is the
/// line's own direction, from its first point to its last, which a slash is
/// measured from at both ends.
fn ending(out: &mut Vec<u8>, kind: Ending, at: Point, out_dir: Point, line: Point, paint: &Paint) {
    // What paints this ending: a closed one is filled, stroked or both, an
    // open one only stroked. One with nothing to paint writes nothing, since
    // an unpainted path would be painted by whatever operator came next.
    let paint_with: &[u8] = match (kind, kind.is_closed()) {
        (Ending::None, _) => return,
        (_, true) => match paint.closed() {
            Some(operator) => operator,
            None => return,
        },
        (_, false) if paint.strokes() => b"S\n",
        (_, false) => return,
    };
    let half = 3.0 * paint.width.max(1.0);
    let (ux, uy) = out_dir;
    // The normal, a quarter turn counter-clockwise of the outward direction.
    let (nx, ny) = (-uy, ux);
    let (x, y) = at;
    match kind {
        Ending::None => {}
        Ending::Square => {
            op(out, &[x + half * (ux + nx), y + half * (uy + ny)], b"m");
            op(out, &[x + half * (-ux + nx), y + half * (-uy + ny)], b"l");
            op(out, &[x + half * (-ux - nx), y + half * (-uy - ny)], b"l");
            op(out, &[x + half * (ux - nx), y + half * (uy - ny)], b"l");
            out.extend_from_slice(b"h\n");
        }
        Ending::Circle => circle(out, x, y, half),
        Ending::Diamond => {
            op(out, &[x + half * ux, y + half * uy], b"m");
            op(out, &[x + half * nx, y + half * ny], b"l");
            op(out, &[x - half * ux, y - half * uy], b"l");
            op(out, &[x - half * nx, y - half * ny], b"l");
            out.extend_from_slice(b"h\n");
        }
        Ending::OpenArrow | Ending::ClosedArrow | Ending::ROpenArrow | Ending::RClosedArrow => {
            // The tip is the point; the two back corners are an arrowhead's
            // length behind it — back along the line for an arrow, outward
            // past the point for a reversed one — and spread at thirty
            // degrees either side.
            let back = if matches!(kind, Ending::OpenArrow | Ending::ClosedArrow) {
                -2.0 * half
            } else {
                2.0 * half
            };
            let spread = 2.0 * half * TAN_30;
            op(
                out,
                &[x + back * ux + spread * nx, y + back * uy + spread * ny],
                b"m",
            );
            op(out, &[x, y], b"l");
            op(
                out,
                &[x + back * ux - spread * nx, y + back * uy - spread * ny],
                b"l",
            );
            if kind.is_closed() {
                out.extend_from_slice(b"h\n");
            }
        }
        Ending::Butt => {
            op(out, &[x + half * nx, y + half * ny], b"m");
            op(out, &[x - half * nx, y - half * ny], b"l");
        }
        Ending::Slash => {
            // Thirty degrees clockwise of the perpendicular is sixty degrees
            // counter-clockwise of the line itself.
            let (lx, ly) = line;
            let (sx, sy) = (lx * COS_60 - ly * SIN_60, lx * SIN_60 + ly * COS_60);
            op(out, &[x + half * sx, y + half * sy], b"m");
            op(out, &[x - half * sx, y - half * sy], b"l");
        }
    }
    out.extend_from_slice(paint_with);
}

/// A line annotation (12.5.6.7): `/L` from its first point to its last, in
/// `/C` at the `/BS` width and dash, with `/LE`'s endings filled with `/IC`,
/// and, when `/LL` is not zero, the leader lines of Figure 60.
///
/// `/LL` is the leader lines' length, measured from `/LLO` past each point —
/// clockwise of the line's direction when positive — and the line proper is
/// drawn between their far ends, `/LLE` short of where they stop. `/Cap`'s
/// caption is not drawn: text needs a font the dictionary does not name.
fn line(doc: &CosDocument, annotation: &Dict, out: &mut Vec<u8>) -> Option<()> {
    let points = numbers_of(doc, annotation, b"L");
    let [x1, y1, x2, y2] = *points.get(..4)? else {
        return None;
    };
    let along = unit(x2 - x1, y2 - y1)?;
    let paint = Paint {
        stroke: stroke_color_of(doc, annotation),
        fill: color_of(doc, annotation, b"IC"),
        width: border_width(doc, annotation),
    };
    let (first, last) = endings_of(doc, annotation);
    let fills_an_end = paint.fill.is_some() && (first.is_closed() || last.is_closed());
    if !paint.strokes() && !fills_an_end {
        return None;
    }

    let leader = number_of(doc, annotation, b"LL").unwrap_or(0.0);
    let extension = number_of(doc, annotation, b"LLE").unwrap_or(0.0).max(0.0);
    let offset = number_of(doc, annotation, b"LLO").unwrap_or(0.0).max(0.0);
    // Clockwise of the direction of travel, for a positive /LL.
    let side = if leader < 0.0 { -1.0 } else { 1.0 };
    let (cx, cy) = (along.1 * side, -along.0 * side);
    let reach = offset + leader.abs();
    let (p1, p2) = (
        (x1 + cx * reach, y1 + cy * reach),
        (x2 + cx * reach, y2 + cy * reach),
    );

    if let Some(stroke) = paint.stroke {
        op(out, &stroke, b"RG");
    }
    if let Some(fill) = paint.fill {
        op(out, &fill, b"rg");
    }
    if paint.strokes() {
        op(out, &[paint.width], b"w");
        let pattern = dash_of(doc, annotation);
        if let Some(pattern) = &pattern {
            dash(out, pattern, 0.0);
        }
        if leader != 0.0 {
            let far = reach + extension;
            for (x, y) in [(x1, y1), (x2, y2)] {
                op(out, &[x + cx * offset, y + cy * offset], b"m");
                op(out, &[x + cx * far, y + cy * far], b"l");
            }
        }
        op(out, &[p1.0, p1.1], b"m");
        op(out, &[p2.0, p2.1], b"l");
        out.extend_from_slice(b"S\n");
        // The endings are drawn solid whatever the line is: a dashed
        // arrowhead is a broken one.
        if pattern.is_some() {
            out.extend_from_slice(b"[] 0 d\n");
        }
    }
    ending(out, first, p1, (-along.0, -along.1), along, &paint);
    ending(out, last, p2, along, along, &paint);
    Some(())
}

/// Builds the appearance for an annotation, or `None` when its type needs
/// none — a link with no border draws nothing, and inventing something for it
/// would be worse than leaving it alone.
///
/// The returned stream is a complete Form XObject, ready to be written as the
/// annotation's `/AP` `/N`.
#[must_use]
pub fn synthesize(doc: &CosDocument, annotation: &Dict) -> Option<StreamData> {
    let rect = rect_of(doc, annotation)?;
    let subtype = annotation
        .get_name(doc.intern(b"Subtype"))
        .and_then(|name| doc.name_bytes(name))?;

    let mut content = Vec::new();
    let mut needs_multiply = false;

    match subtype.as_ref() {
        b"Highlight" => {
            let color = color_of(doc, annotation, b"C").unwrap_or([1.0, 1.0, 0.0]);
            let quads = quads_of(doc, annotation);
            if quads.is_empty() {
                return None;
            }
            // A highlight that painted over the text would hide it. Multiply
            // is what viewers use, and it is the only blend mode this needs.
            needs_multiply = true;
            content.extend_from_slice(b"/GS0 gs\n");
            op(&mut content, &color, b"rg");
            for quad in quads {
                // Redrawn in path order — upper-left, upper-right,
                // lower-right, lower-left — from the spec's corner order.
                op(&mut content, &[quad[0], quad[1]], b"m");
                op(&mut content, &[quad[2], quad[3]], b"l");
                op(&mut content, &[quad[6], quad[7]], b"l");
                op(&mut content, &[quad[4], quad[5]], b"l");
                content.extend_from_slice(b"h\n");
            }
            content.extend_from_slice(b"f\n");
        }
        b"Underline" | b"StrikeOut" => {
            let color = color_of(doc, annotation, b"C").unwrap_or([0.0, 0.0, 0.0]);
            let quads = quads_of(doc, annotation);
            if quads.is_empty() {
                return None;
            }
            op(&mut content, &color, b"RG");
            for quad in quads {
                let top = quad[1].max(quad[3]);
                let bottom = quad[5].min(quad[7]);
                let height = (top - bottom).abs();
                // 12.5.6.11: the line sits under the text for an underline and
                // through it for a strike-out. Both are a fraction of the
                // quad's height, so they scale with the text they mark.
                let y = if subtype.as_ref() == b"Underline" {
                    bottom + height * 0.06
                } else {
                    bottom + height * 0.42
                };
                op(&mut content, &[(height * 0.07).max(0.5)], b"w");
                op(&mut content, &[quad[4].min(quad[0]), y], b"m");
                op(&mut content, &[quad[6].max(quad[2]), y], b"l");
                content.extend_from_slice(b"S\n");
            }
        }
        b"Square" => {
            let width = border_width(doc, annotation);
            let stroke = color_of(doc, annotation, b"C");
            let fill = color_of(doc, annotation, b"IC");
            if stroke.is_none() && fill.is_none() {
                return None;
            }

            let box_ = inset(rect, width / 2.0);
            if let Some(fill) = fill {
                op(&mut content, &fill, b"rg");
            }
            if let Some(stroke) = stroke {
                op(&mut content, &stroke, b"RG");
                op(&mut content, &[width], b"w");
            }
            op(
                &mut content,
                &[box_.x0, box_.y0, box_.x1 - box_.x0, box_.y1 - box_.y0],
                b"re",
            );
            content.extend_from_slice(match (fill.is_some(), stroke.is_some() && width > 0.0) {
                (true, true) => b"B\n".as_slice(),
                (true, false) => b"f\n",
                _ => b"S\n",
            });
        }
        b"Circle" => {
            let width = border_width(doc, annotation);
            let stroke = color_of(doc, annotation, b"C");
            let fill = color_of(doc, annotation, b"IC");
            if stroke.is_none() && fill.is_none() {
                return None;
            }

            let box_ = inset(rect, width / 2.0);
            let (cx, cy) = ((box_.x0 + box_.x1) / 2.0, (box_.y0 + box_.y1) / 2.0);
            let (rx, ry) = ((box_.x1 - box_.x0) / 2.0, (box_.y1 - box_.y0) / 2.0);
            // The constant that makes four cubics approximate an ellipse to
            // within about one part in a thousand.
            const K: f64 = 0.552_284_749_83;
            let (ox, oy) = (rx * K, ry * K);

            if let Some(fill) = fill {
                op(&mut content, &fill, b"rg");
            }
            if let Some(stroke) = stroke {
                op(&mut content, &stroke, b"RG");
                op(&mut content, &[width], b"w");
            }
            op(&mut content, &[cx - rx, cy], b"m");
            op(
                &mut content,
                &[cx - rx, cy + oy, cx - ox, cy + ry, cx, cy + ry],
                b"c",
            );
            op(
                &mut content,
                &[cx + ox, cy + ry, cx + rx, cy + oy, cx + rx, cy],
                b"c",
            );
            op(
                &mut content,
                &[cx + rx, cy - oy, cx + ox, cy - ry, cx, cy - ry],
                b"c",
            );
            op(
                &mut content,
                &[cx - ox, cy - ry, cx - rx, cy - oy, cx - rx, cy],
                b"c",
            );
            content.extend_from_slice(b"h\n");
            content.extend_from_slice(match (fill.is_some(), stroke.is_some() && width > 0.0) {
                (true, true) => b"B\n".as_slice(),
                (true, false) => b"f\n",
                _ => b"S\n",
            });
        }
        b"Text" => {
            // A sticky note: a rounded page with two rule lines. Drawn at
            // whatever size the rectangle gives, because viewers vary on
            // whether they force the conventional 20x20 and a note that
            // ignores its own rectangle looks like a bug.
            let color = color_of(doc, annotation, b"C").unwrap_or([1.0, 0.82, 0.0]);
            let box_ = inset(rect, 1.0);
            let (w, h) = (box_.x1 - box_.x0, box_.y1 - box_.y0);
            if w <= 0.0 || h <= 0.0 {
                return None;
            }

            op(&mut content, &color, b"rg");
            op(&mut content, &[0.0, 0.0, 0.0], b"RG");
            op(&mut content, &[(h * 0.05).clamp(0.4, 1.5)], b"w");
            op(&mut content, &[box_.x0, box_.y0, w, h], b"re");
            content.extend_from_slice(b"B\n");

            for fraction in [0.35, 0.6] {
                let y = box_.y0 + h * fraction;
                op(&mut content, &[box_.x0 + w * 0.2, y], b"m");
                op(&mut content, &[box_.x1 - w * 0.2, y], b"l");
            }
            content.extend_from_slice(b"S\n");
        }
        b"Link" => {
            // 12.5.6.5: a link's appearance is its border, and the convention
            // is not to have one. Drawing a box nobody asked for is worse than
            // drawing nothing.
            return None;
        }
        b"Line" => line(doc, annotation, &mut content)?,
        _ => return None,
    }

    if content.is_empty() {
        return None;
    }

    // 12.5.6.2's constant opacity, in the one graphics state this appearance
    // selects. A highlight already selects it for its blend mode, so the
    // opacity joins that state rather than adding a second.
    let opacity = opacity_of(doc, annotation);
    if opacity.is_some() && !needs_multiply {
        let mut selected = b"/GS0 gs\n".to_vec();
        selected.extend_from_slice(&content);
        content = selected;
    }

    let mut dict = Dict::new();
    dict.insert(Name::TYPE, Object::Name(doc.intern(b"XObject")));
    dict.insert(doc.intern(b"Subtype"), Object::Name(doc.intern(b"Form")));
    dict.insert(
        doc.intern(b"BBox"),
        Object::Array(vec![
            Object::Real(rect.x0),
            Object::Real(rect.y0),
            Object::Real(rect.x1),
            Object::Real(rect.y1),
        ]),
    );
    // Stated rather than left to default, because a viewer that reads /Matrix
    // from a previous object's leftovers is a bug that only shows up in files
    // that omit it.
    dict.insert(
        doc.intern(b"Matrix"),
        Object::Array(vec![
            Object::Int(1),
            Object::Int(0),
            Object::Int(0),
            Object::Int(1),
            Object::Int(0),
            Object::Int(0),
        ]),
    );

    let mut resources = Dict::new();
    if needs_multiply || opacity.is_some() {
        let mut state = Dict::new();
        state.insert(Name::TYPE, Object::Name(doc.intern(b"ExtGState")));
        if needs_multiply {
            state.insert(doc.intern(b"BM"), Object::Name(doc.intern(b"Multiply")));
        }
        if let Some((stroking, filling)) = opacity {
            state.insert(doc.intern(b"CA"), Object::Real(stroking));
            state.insert(doc.intern(b"ca"), Object::Real(filling));
        }
        let mut states = Dict::new();
        states.insert(doc.intern(b"GS0"), Object::Dict(state));
        resources.insert(doc.intern(b"ExtGState"), Object::Dict(states));
    }
    dict.insert(Name::RESOURCES, Object::Dict(resources));

    Some(StreamData {
        dict,
        data: content,
    })
}

/// Which mark a button's on appearance draws (12.7.4.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ButtonStyle {
    /// A square box, ticked when on — a check box (12.7.4.2.3).
    Check,
    /// A round box, dotted when on — one button of a radio group
    /// (12.7.4.2.4).
    Radio,
}

/// The normal appearance of one state of a check box or radio button, in a
/// `width` by `height` box at the origin.
///
/// A widget's appearance is mapped onto its `/Rect` by 12.5.5, so the box is
/// the widget's own size at the origin — the convention
/// [`crate::fill::text_appearance`] uses for a text field, and the one every
/// producer writes for a widget. Both states draw the frame, so an unticked
/// box still shows where it is; only the on state draws the mark.
///
/// Drawn with paths rather than a ZapfDingbats glyph, which is the other
/// convention: a glyph needs a font in the form's `/DR` and a reader that can
/// draw it, and a path needs neither.
pub(crate) fn button(
    doc: &CosDocument,
    width: f64,
    height: f64,
    style: ButtonStyle,
    on: bool,
) -> StreamData {
    let (w, h) = (width.max(0.0), height.max(0.0));
    let side = w.min(h);
    let border = (side * 0.06).clamp(0.5, 1.5);
    let mut content = Vec::new();
    op(&mut content, &[0.0], b"G");
    op(&mut content, &[border], b"w");
    match style {
        ButtonStyle::Check => {
            let whole = Rect {
                x0: 0.0,
                y0: 0.0,
                x1: w,
                y1: h,
            };
            let frame = inset(whole, border / 2.0);
            op(
                &mut content,
                &[frame.x0, frame.y0, frame.x1 - frame.x0, frame.y1 - frame.y0],
                b"re",
            );
            content.extend_from_slice(b"S\n");
            if on {
                // A tick: down to a foot a third of the way along, then up to
                // the top right.
                op(&mut content, &[(side * 0.12).max(0.5)], b"w");
                content.extend_from_slice(b"1 J\n1 j\n");
                op(&mut content, &[w * 0.22, h * 0.52], b"m");
                op(&mut content, &[w * 0.42, h * 0.26], b"l");
                op(&mut content, &[w * 0.78, h * 0.76], b"l");
                content.extend_from_slice(b"S\n");
            }
        }
        ButtonStyle::Radio => {
            let (cx, cy) = (w / 2.0, h / 2.0);
            let r = (side / 2.0 - border / 2.0).max(0.0);
            circle(&mut content, cx, cy, r);
            content.extend_from_slice(b"S\n");
            if on {
                op(&mut content, &[0.0], b"g");
                circle(&mut content, cx, cy, r * 0.5);
                content.extend_from_slice(b"f\n");
            }
        }
    }

    let mut dict = Dict::new();
    dict.insert(Name::TYPE, Object::Name(doc.intern(b"XObject")));
    dict.insert(doc.intern(b"Subtype"), Object::Name(doc.intern(b"Form")));
    dict.insert(
        doc.intern(b"BBox"),
        Object::Array(vec![
            Object::Int(0),
            Object::Int(0),
            Object::Real(w),
            Object::Real(h),
        ]),
    );
    dict.insert(Name::RESOURCES, Object::Dict(Dict::new()));
    StreamData {
        dict,
        data: content,
    }
}

/// A visible signature's normal appearance (12.7.4.5, 12.5.5), in a `width`
/// by `height` box at the origin.
///
/// `lines` of text in Helvetica, sized so the widest fits across and all of
/// them fit down, at most twelve points; and an image in the left two-fifths
/// of the box when there is one — the whole box when there is no text —
/// scaled to fit with its aspect ratio kept and centred in its part.
/// `image` is the image XObject's reference and its pixel size.
///
/// The font is Helvetica in `WinAnsiEncoding`, carried directly in the
/// appearance's own `/Resources` rather than in a form's `/DR`, because a
/// signature field's appearance is not regenerated from a `/DA` by anybody.
/// A character above the single-byte range is drawn as `?` and pushed onto
/// `unwritable`, for the caller to name against the widget (ruling 10) —
/// the same rule and the same writer a filled field's value goes through.
pub(crate) fn signature(
    doc: &CosDocument,
    width: f64,
    height: f64,
    lines: &[String],
    image: Option<(ObjRef, u32, u32)>,
    unwritable: &mut Vec<char>,
) -> StreamData {
    let (w, h) = (width.max(0.0), height.max(0.0));
    let pad = (w.min(h) * 0.05).clamp(1.0, 4.0);
    let mut content = Vec::new();

    // The image's part of the box, and where the text starts.
    let image_part = match (image, lines.is_empty()) {
        (Some(_), true) => w,
        (Some(_), false) => w * 0.4,
        (None, _) => 0.0,
    };
    if let Some((_, iw, ih)) = image {
        let (box_w, box_h) = ((image_part - 2.0 * pad).max(0.0), (h - 2.0 * pad).max(0.0));
        let (iw, ih) = (f64::from(iw.max(1)), f64::from(ih.max(1)));
        let scale = (box_w / iw).min(box_h / ih);
        let (dw, dh) = (iw * scale, ih * scale);
        let (x, y) = (pad + (box_w - dw) / 2.0, pad + (box_h - dh) / 2.0);
        content.extend_from_slice(b"q\n");
        op(&mut content, &[dw, 0.0, 0.0, dh, x, y], b"cm");
        content.extend_from_slice(b"/Img0 Do\nQ\n");
    }

    let mut font = Dict::new();
    font.insert(Name::TYPE, Object::Name(doc.intern(b"Font")));
    font.insert(doc.intern(b"Subtype"), Object::Name(doc.intern(b"Type1")));
    font.insert(
        doc.intern(b"BaseFont"),
        Object::Name(doc.intern(b"Helvetica")),
    );
    font.insert(
        doc.intern(b"Encoding"),
        Object::Name(doc.intern(b"WinAnsiEncoding")),
    );

    if !lines.is_empty() {
        let metrics = crate::font::read(doc, &font);
        // Thousandths of an em, as the writer will encode each line: a code
        // above the single-byte range is drawn as `?`.
        let advance = |line: &str| -> f64 {
            line.chars()
                .map(|c| {
                    let code = if u32::from(c) < 256 {
                        u32::from(c)
                    } else {
                        u32::from(b'?')
                    };
                    metrics.width_of(code).0
                })
                .sum()
        };
        let text_x = image_part + pad;
        let text_w = (w - text_x - pad).max(0.0);
        let text_h = (h - 2.0 * pad).max(0.0);
        let widest = lines.iter().map(|l| advance(l)).fold(0.0f64, f64::max) / 1000.0;
        let by_height = text_h / (lines.len() as f64 * 1.2);
        let by_width = if widest > 0.0 {
            text_w / widest
        } else {
            by_height
        };
        let size = by_height.min(by_width).clamp(1.0, 12.0);
        let leading = size * 1.2;
        // Clipped to the text's part, so a box too small for a line cuts it
        // off rather than drawing it over the image.
        content.extend_from_slice(b"q\n");
        op(&mut content, &[text_x, pad, text_w, text_h], b"re");
        content.extend_from_slice(b"W n\nBT\n/Helv ");
        op(&mut content, &[size], b"Tf");
        content.extend_from_slice(b"0 g\n");
        for (index, line) in lines.iter().enumerate() {
            let y = h - pad - size * 0.9 - index as f64 * leading;
            op(&mut content, &[1.0, 0.0, 0.0, 1.0, text_x, y], b"Tm");
            content.push(b'(');
            crate::fill::escape(&mut content, line, unwritable);
            content.extend_from_slice(b") Tj\n");
        }
        content.extend_from_slice(b"ET\nQ\n");
    }

    let mut resources = Dict::new();
    let mut fonts = Dict::new();
    fonts.insert(doc.intern(b"Helv"), Object::Dict(font));
    resources.insert(doc.intern(b"Font"), Object::Dict(fonts));
    if let Some((image, _, _)) = image {
        let mut xobjects = Dict::new();
        xobjects.insert(doc.intern(b"Img0"), Object::Ref(image));
        resources.insert(doc.intern(b"XObject"), Object::Dict(xobjects));
    }

    let mut dict = Dict::new();
    dict.insert(Name::TYPE, Object::Name(doc.intern(b"XObject")));
    dict.insert(doc.intern(b"Subtype"), Object::Name(doc.intern(b"Form")));
    dict.insert(
        doc.intern(b"BBox"),
        Object::Array(vec![
            Object::Int(0),
            Object::Int(0),
            Object::Real(w),
            Object::Real(h),
        ]),
    );
    dict.insert(Name::RESOURCES, Object::Dict(resources));
    StreamData {
        dict,
        data: content,
    }
}

/// A circle as four cubics — the path [`synthesize`] draws a `/Circle` with,
/// for the round case.
fn circle(out: &mut Vec<u8>, cx: f64, cy: f64, r: f64) {
    // The constant that makes four cubics approximate a circle to within
    // about one part in a thousand.
    const K: f64 = 0.552_284_749_83;
    let o = r * K;
    op(out, &[cx - r, cy], b"m");
    op(out, &[cx - r, cy + o, cx - o, cy + r, cx, cy + r], b"c");
    op(out, &[cx + o, cy + r, cx + r, cy + o, cx + r, cy], b"c");
    op(out, &[cx + r, cy - o, cx + o, cy - r, cx, cy - r], b"c");
    op(out, &[cx - o, cy - r, cx - r, cy - o, cx - r, cy], b"c");
    out.extend_from_slice(b"h\n");
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::build::DocumentBuilder;
    use crate::edit::annot::{self, Color};

    fn doc() -> CosDocument {
        let mut builder = DocumentBuilder::new();
        builder.add_page(200.0, 200.0, |_| {});
        CosDocument::open(builder.finish()).expect("it opens")
    }

    fn text_of(stream: &StreamData) -> String {
        String::from_utf8_lossy(&stream.data).into_owned()
    }

    const RED: Color = Color {
        r: 1.0,
        g: 0.0,
        b: 0.0,
    };

    fn rect() -> Rect {
        Rect {
            x0: 10.0,
            y0: 20.0,
            x1: 110.0,
            y1: 60.0,
        }
    }

    #[test]
    fn a_highlight_multiplies_so_the_text_shows_through() {
        let doc = doc();
        let quad = [10.0, 60.0, 110.0, 60.0, 10.0, 20.0, 110.0, 20.0];
        let dict = annot::highlight(&doc, &[quad], RED);
        let stream = synthesize(&doc, &dict).expect("a highlight has an appearance");

        let content = text_of(&stream);
        assert!(content.contains("/GS0 gs"), "the blend state is selected");
        assert!(content.contains("1 0 0 rg"), "in the annotation's colour");
        assert!(content.ends_with("f\n"), "and the quads are filled");

        let resources = stream
            .dict
            .get_dict(doc.intern(b"Resources"))
            .expect("resources");
        let states = resources
            .get_dict(doc.intern(b"ExtGState"))
            .and_then(|d| d.get_dict(doc.intern(b"GS0")))
            .expect("the state is defined, not merely named");
        assert_eq!(
            states.get_name(doc.intern(b"BM")),
            Some(doc.intern(b"Multiply"))
        );
    }

    /// The corner order of `/QuadPoints` is the classic trap: taken in
    /// sequence it draws a bow tie rather than a rectangle.
    #[test]
    fn a_highlight_quad_is_not_drawn_bow_tied() {
        let doc = doc();
        let quad = [10.0, 60.0, 110.0, 60.0, 10.0, 20.0, 110.0, 20.0];
        let dict = annot::highlight(&doc, &[quad], RED);
        let content = text_of(&synthesize(&doc, &dict).expect("an appearance"));

        // Upper-left, upper-right, lower-right, lower-left: the second point
        // shares the first's y, and the third shares the fourth's.
        let path: Vec<&str> = content
            .lines()
            .filter(|l| l.ends_with(" m") || l.ends_with(" l"))
            .collect();
        assert_eq!(
            path,
            vec!["10 60 m", "110 60 l", "110 20 l", "10 20 l"],
            "the corners are reordered into a path"
        );
    }

    #[test]
    fn a_form_maps_onto_its_rectangle_without_a_transform() {
        let doc = doc();
        let dict = annot::square(&doc, rect(), RED, 2.0);
        let stream = synthesize(&doc, &dict).expect("an appearance");

        let bbox = stream
            .dict
            .get_array(doc.intern(b"BBox"))
            .expect("a bounding box");
        let values: Vec<f64> = bbox.iter().filter_map(Object::as_number).collect();
        assert_eq!(
            values,
            vec![10.0, 20.0, 110.0, 60.0],
            "the box equals /Rect, so 12.5.5's mapping is the identity"
        );

        let matrix = stream
            .dict
            .get_array(doc.intern(b"Matrix"))
            .expect("a matrix");
        let values: Vec<f64> = matrix.iter().filter_map(Object::as_number).collect();
        assert_eq!(values, vec![1.0, 0.0, 0.0, 1.0, 0.0, 0.0]);
    }

    /// A stroke is centred on its path, so a border drawn on the rectangle
    /// itself loses its outer half to the bounding box.
    #[test]
    fn a_border_is_inset_by_half_its_width() {
        let doc = doc();
        let dict = annot::square(&doc, rect(), RED, 4.0);
        let content = text_of(&synthesize(&doc, &dict).expect("an appearance"));
        assert!(
            content.contains("12 22 96 36 re"),
            "inset by two on every side, got: {content}"
        );
    }

    #[test]
    fn a_square_with_no_colour_at_all_has_no_appearance() {
        let doc = doc();
        let mut dict = annot::square(&doc, rect(), RED, 1.0);
        dict.insert(doc.intern(b"C"), Object::Array(Vec::new()));
        assert!(
            synthesize(&doc, &dict).is_none(),
            "an empty /C means transparent, and nothing to draw"
        );
    }

    #[test]
    fn a_link_gets_no_appearance() {
        let doc = doc();
        let page = crate::pages::collect(&doc)[0].reference;
        let dict = annot::link(&doc, rect(), page);
        assert!(synthesize(&doc, &dict).is_none());
    }

    #[test]
    fn a_sticky_note_draws_something() {
        let doc = doc();
        let dict = annot::text_note(&doc, rect(), "hello", true);
        let content = text_of(&synthesize(&doc, &dict).expect("an appearance"));
        assert!(content.contains(" re"), "a note body");
        assert!(content.contains("B\n"), "filled and stroked");
    }

    #[test]
    fn a_degenerate_rectangle_has_no_appearance() {
        let doc = doc();
        let flat = Rect {
            x0: 10.0,
            y0: 20.0,
            x1: 10.0,
            y1: 20.0,
        };
        let dict = annot::square(&doc, flat, RED, 1.0);
        assert!(synthesize(&doc, &dict).is_none());
    }

    #[test]
    fn an_unknown_subtype_is_left_alone() {
        let doc = doc();
        let mut dict = annot::square(&doc, rect(), RED, 1.0);
        dict.insert(doc.intern(b"Subtype"), Object::Name(doc.intern(b"Polygon")));
        assert!(
            synthesize(&doc, &dict).is_none(),
            "better no appearance than a wrong one"
        );
    }

    #[test]
    fn cmyk_and_grey_colours_are_converted() {
        let doc = doc();
        let mut dict = annot::square(&doc, rect(), RED, 1.0);

        dict.insert(doc.intern(b"C"), Object::Array(vec![Object::Real(0.5)]));
        let grey = text_of(&synthesize(&doc, &dict).expect("an appearance"));
        assert!(grey.contains("0.5 0.5 0.5 RG"), "grey replicates");

        dict.insert(
            doc.intern(b"C"),
            Object::Array(vec![
                Object::Real(0.0),
                Object::Real(1.0),
                Object::Real(1.0),
                Object::Real(0.0),
            ]),
        );
        let cmyk = text_of(&synthesize(&doc, &dict).expect("an appearance"));
        assert!(cmyk.contains("1 0 0 RG"), "cmyk red, got: {cmyk}");
    }

    #[test]
    fn numbers_never_come_out_in_exponential_notation() {
        let mut out = Vec::new();
        number(&mut out, 0.000_012_5);
        number(&mut out, f64::INFINITY);
        number(&mut out, f64::NAN);
        let text = String::from_utf8(out).expect("ascii");
        assert!(
            !text.contains('e') && !text.contains("inf") && !text.contains("NaN"),
            "no tokenizer accepts those, got: {text}"
        );
    }

    /// A stream as the writer would emit it: the dictionary, then the
    /// content, so a change to either is a change to the pinned text.
    fn written(doc: &CosDocument, stream: Option<StreamData>) -> String {
        let Some(stream) = stream else {
            return "none".to_owned();
        };
        let mut out = Vec::new();
        crate::write::write_object(&mut out, &Object::Dict(stream.dict), doc.names_table());
        out.extend_from_slice(b"\n--\n");
        out.extend_from_slice(&stream.data);
        String::from_utf8_lossy(&out).into_owned()
    }

    fn with_subtype(doc: &CosDocument, mut dict: Dict, subtype: &[u8]) -> Dict {
        dict.insert(doc.intern(b"Subtype"), Object::Name(doc.intern(subtype)));
        dict
    }

    /// The seven subtypes this module drew before it drew any other, byte
    /// for byte, as the editor's own constructors (and those constructors
    /// with the subtype changed, for the three that have none) ask for them.
    ///
    /// Every subtype added since is added beside these, and none of them may
    /// move a byte here: an annotation a caller wrote yesterday must get the
    /// same appearance today. A key one of these seven did not read — `/CA`,
    /// a dash, `/RD` — may change what it draws *when it is present*, and
    /// none of these dictionaries carries one.
    #[test]
    fn the_seven_first_subtypes_are_drawn_exactly_as_they_were() {
        let doc = doc();
        let quad = [10.0, 60.0, 110.0, 60.0, 10.0, 20.0, 110.0, 20.0];
        let page = crate::pages::collect(&doc)[0].reference;
        let mut circle = with_subtype(&doc, annot::square(&doc, rect(), RED, 2.0), b"Circle");
        circle.insert(
            doc.intern(b"IC"),
            Object::Array(vec![
                Object::Real(0.0),
                Object::Real(0.0),
                Object::Real(1.0),
            ]),
        );
        let no_resources = "<</Type /XObject/Subtype /Form/BBox [10 20 110 60]\
/Matrix [1 0 0 1 0 0]/Resources <<>>>>\n--\n";
        let cases: [(&str, Dict, String); 7] = [
            (
                "Highlight",
                annot::highlight(&doc, &[quad], RED),
                "<</Type /XObject/Subtype /Form/BBox [10 20 110 60]/Matrix [1 0 0 1 0 0]\
/Resources <</ExtGState <</GS0 <</Type /ExtGState/BM /Multiply>>>>>>>>\n--\n\
/GS0 gs\n1 0 0 rg\n10 60 m\n110 60 l\n110 20 l\n10 20 l\nh\nf\n"
                    .to_owned(),
            ),
            (
                "Underline",
                with_subtype(&doc, annot::highlight(&doc, &[quad], RED), b"Underline"),
                format!("{no_resources}1 0 0 RG\n2.8 w\n10 22.4 m\n110 22.4 l\nS\n"),
            ),
            (
                "StrikeOut",
                with_subtype(&doc, annot::highlight(&doc, &[quad], RED), b"StrikeOut"),
                format!("{no_resources}1 0 0 RG\n2.8 w\n10 36.8 m\n110 36.8 l\nS\n"),
            ),
            (
                "Square",
                annot::square(&doc, rect(), RED, 2.0),
                format!("{no_resources}1 0 0 RG\n2 w\n11 21 98 38 re\nS\n"),
            ),
            (
                "Circle",
                circle,
                format!(
                    "{no_resources}0 0 1 rg\n1 0 0 RG\n2 w\n11 40 m\n\
11 50.4934 32.938 59 60 59 c\n87.062 59 109 50.4934 109 40 c\n\
109 29.5066 87.062 21 60 21 c\n32.938 21 11 29.5066 11 40 c\nh\nB\n"
                ),
            ),
            (
                "Text",
                annot::text_note(&doc, rect(), "hello", true),
                format!(
                    "{no_resources}1 0.82 0 rg\n0 0 0 RG\n1.5 w\n11 21 98 38 re\nB\n\
30.6 34.3 m\n89.4 34.3 l\n30.6 43.8 m\n89.4 43.8 l\nS\n"
                ),
            ),
            ("Link", annot::link(&doc, rect(), page), "none".to_owned()),
        ];
        for (subtype, dict, expected) in cases {
            assert_eq!(
                written(&doc, synthesize(&doc, &dict)),
                expected,
                "/{subtype}'s appearance moved"
            );
        }
    }

    /// An annotation dictionary written as PDF text, its names interned in
    /// `doc`'s table.
    fn parsed(doc: &CosDocument, text: &str) -> Dict {
        let mut sink = crate::warn::WarningSink::new();
        let parsed =
            crate::parse::parse_object_at(text.as_bytes(), 0, doc.names_table(), &mut sink);
        match parsed.object {
            Object::Dict(dict) => dict,
            other => panic!("the test's dictionary did not parse: {other:?}"),
        }
    }

    fn content_of(doc: &CosDocument, text: &str) -> Option<String> {
        synthesize(doc, &parsed(doc, text)).map(|stream| text_of(&stream))
    }

    #[test]
    fn a_line_needs_two_distinct_points() {
        let doc = doc();
        for l in ["", "/L [10 10 10]", "/L [10 10 10 10]", "/L [10 10 (x) 20]"] {
            assert_eq!(
                content_of(
                    &doc,
                    &format!("<< /Subtype /Line /Rect [0 0 100 100] {l} /C [1 0 0] >>")
                ),
                None,
                "a line with {l:?} has no direction to draw"
            );
        }
    }

    /// A line with no stroke colour still fills a closed ending with `/IC`,
    /// and an open ending — which only a stroke can draw — writes nothing at
    /// all. A path left unpainted would be painted by the next operator,
    /// which is the square's fill.
    #[test]
    fn an_ending_with_nothing_to_paint_writes_no_path() {
        let doc = doc();
        let content = content_of(
            &doc,
            "<< /Subtype /Line /Rect [0 0 100 100] /L [10 50 90 50] /C [] /IC [0 0 1] \
             /LE [/OpenArrow /Square] /BS << /W 1 >> >>",
        )
        .expect("the square is still filled");
        assert_eq!(
            content, "0 0 1 rg\n93 53 m\n87 53 l\n87 47 l\n93 47 l\nh\nf\n",
            "only the square, filled"
        );
        assert_eq!(
            content_of(
                &doc,
                "<< /Subtype /Line /Rect [0 0 100 100] /L [10 50 90 50] /C [] \
                 /LE [/OpenArrow /Square] >>",
            ),
            None,
            "and with no interior either, nothing is left to draw"
        );
    }

    /// Every ending, at the last point of a line travelling east, at width 1:
    /// each reaches three units from its point.
    #[test]
    fn each_ending_is_drawn_where_table_176_puts_it() {
        let doc = doc();
        let ending = |name: &str| -> String {
            let content = content_of(
                &doc,
                &format!(
                    "<< /Subtype /Line /Rect [0 0 100 100] /L [10 50 90 50] /C [1 0 0] \
                     /IC [0 0 1] /LE [/None /{name}] >>"
                ),
            )
            .expect("a line");
            let after = content
                .split_once("90 50 l\nS\n")
                .map(|(_, rest)| rest.to_owned())
                .expect("the line comes first");
            after
        };
        assert_eq!(ending("None"), "");
        assert_eq!(ending("Bogus"), "", "an unlisted name draws nothing");
        assert_eq!(ending("Butt"), "90 53 m\n90 47 l\nS\n");
        assert_eq!(
            ending("Diamond"),
            "93 50 m\n90 53 l\n87 50 l\n90 47 l\nh\nB\n"
        );
        // Tip at the point, back corners six behind it and 6 tan 30 either
        // side.
        assert_eq!(
            ending("OpenArrow"),
            "84 53.4641 m\n90 50 l\n84 46.5359 l\nS\n"
        );
        assert_eq!(
            ending("ClosedArrow"),
            "84 53.4641 m\n90 50 l\n84 46.5359 l\nh\nB\n"
        );
        assert_eq!(
            ending("ROpenArrow"),
            "96 53.4641 m\n90 50 l\n96 46.5359 l\nS\n"
        );
        // Sixty degrees counter-clockwise of east, three either side.
        assert_eq!(ending("Slash"), "91.5 52.5981 m\n88.5 47.4019 l\nS\n");
        assert!(ending("Circle").ends_with("h\nB\n"));
    }

    /// The first point's ending points the other way: back along the line.
    #[test]
    fn the_first_ending_points_away_from_the_line() {
        let doc = doc();
        let content = content_of(
            &doc,
            "<< /Subtype /Line /Rect [0 0 100 100] /L [10 50 90 50] /C [1 0 0] \
             /LE [/OpenArrow /None] >>",
        )
        .expect("a line");
        assert!(
            content.ends_with("16 46.5359 m\n10 50 l\n16 53.4641 l\nS\n"),
            "{content}"
        );
    }

    #[test]
    fn a_dash_is_drawn_and_one_that_paints_nothing_is_not() {
        let doc = doc();
        let line = |bs: &str| {
            content_of(
                &doc,
                &format!(
                    "<< /Subtype /Line /Rect [0 0 100 100] /L [10 50 90 50] /C [1 0 0] \
                     {bs} /LE [/None /Butt] >>"
                ),
            )
            .expect("a line")
        };
        let dashed = line("/BS << /W 2 /S /D /D [6 4] >>");
        assert!(dashed.contains("2 w\n[6 4] 0 d\n"), "{dashed}");
        assert!(
            dashed.contains("S\n[] 0 d\n"),
            "the ending is drawn solid: {dashed}"
        );
        assert!(
            line("/BS << /W 2 /S /D >>").contains("[3] 0 d\n"),
            "Table 166's default dash"
        );
        assert!(
            line("/Border [0 0 2 [5 1]]").contains("[5 1] 0 d\n"),
            "the legacy /Border's dash"
        );
        for solid in [
            "/BS << /W 2 /S /D /D [0 0] >>",
            "/BS << /W 2 /S /D /D [4 -1] >>",
            "/BS << /W 2 /S /D /D [] >>",
            "/BS << /W 2 /S /S /D [6 4] >>",
            "/BS << /W 2 >> /Border [0 0 2 [5 1]]",
        ] {
            assert!(!line(solid).contains(" d\n"), "{solid} is drawn solid");
        }
    }

    /// `/CA` selects a graphics state with both opacities, and `/ca` the
    /// non-stroking one on its own; a highlight's state carries its blend
    /// mode and the opacity together.
    #[test]
    fn opacity_is_one_graphics_state() {
        let doc = doc();
        let state = |text: &str| -> (String, Dict) {
            let stream = synthesize(&doc, &parsed(&doc, text)).expect("an appearance");
            let resources = stream
                .dict
                .get_dict(doc.intern(b"Resources"))
                .and_then(|r| r.get_dict(doc.intern(b"ExtGState")))
                .and_then(|s| s.get_dict(doc.intern(b"GS0")))
                .cloned()
                .expect("a state");
            (text_of(&stream), resources)
        };
        let number = |dict: &Dict, key: &[u8]| dict.get_number(doc.intern(key));

        let (content, gs) =
            state("<< /Subtype /Line /Rect [0 0 100 100] /L [10 50 90 50] /CA 0.25 >>");
        assert!(content.starts_with("/GS0 gs\n"));
        assert_eq!(number(&gs, b"CA"), Some(0.25));
        assert_eq!(number(&gs, b"ca"), Some(0.25));

        let (_, gs) =
            state("<< /Subtype /Line /Rect [0 0 100 100] /L [10 50 90 50] /CA 2 /ca 0.5 >>");
        assert_eq!(number(&gs, b"CA"), Some(1.0), "clamped to one");
        assert_eq!(number(&gs, b"ca"), Some(0.5));

        let (content, gs) = state(
            "<< /Subtype /Highlight /Rect [0 0 100 100] /CA 0.5 \
             /QuadPoints [10 60 90 60 10 40 90 40] >>",
        );
        assert_eq!(content.matches(" gs\n").count(), 1, "one state: {content}");
        assert_eq!(
            gs.get_name(doc.intern(b"BM")),
            Some(doc.intern(b"Multiply"))
        );
        assert_eq!(number(&gs, b"CA"), Some(0.5));

        let opaque = synthesize(
            &doc,
            &parsed(
                &doc,
                "<< /Subtype /Line /Rect [0 0 100 100] /L [10 50 90 50] /CA 1 >>",
            ),
        )
        .expect("an appearance");
        assert!(!text_of(&opaque).contains(" gs"), "opaque needs no state");
    }
}
