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
//! - `Square` and `Circle` (12.5.6.8) read `/RD` too, and draw their border
//!   dashed when `/BS` says so.
//! - `Polygon` and `PolyLine` (12.5.6.9): `/Vertices`, closed and filled for
//!   a polygon, open with a line's endings for a polyline.
//! - `Squiggly` (12.5.6.10), the fourth text markup: a zigzag in each quad's
//!   own frame, at a cost per quad that does not grow with its length.
//! - `Caret` (12.5.6.11): the typographic caret, filled, inside `/RD`.
//! - `Ink` (12.5.6.13): each of `/InkList`'s paths, stroked with round caps
//!   and joins.
//! - `FreeText` (12.5.6.6), when its `/DA` names a simple font the form's
//!   `/DR` holds: the box, its border, `/Contents` laid out in it, and the
//!   callout.
//!
//! Each carries 12.5.6.2's `/CA` in the graphics state it selects, and each
//! stroked border or line its `/BS` (or `/Border`) dash.
//!
//! Every other subtype is declined: by name when 12.5.6 gives it an
//! appearance its dictionary does not determine ([`UNDETERMINED_SUBTYPES`]),
//! and as unknown otherwise.

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

/// The rectangle a shape is drawn in: `/Rect` less `/RD` (12.5.6.8
/// Table 177, and the caret's and free text's tables), whose four numbers
/// are the differences at the left, top, right and bottom.
///
/// Table 177 asks each to be at least zero and each pair to leave the shape
/// some width and height. A `/RD` that breaks either — a negative or
/// non-finite difference, or two that meet across the rectangle — is not
/// read, and the shape is drawn in the whole of `/Rect`: a border effect is
/// what makes a producer write `/RD`, and a shape drawn a little large is a
/// better guess than one drawn inside out.
fn drawn_rect(doc: &CosDocument, dict: &Dict, rect: Rect) -> Rect {
    let rd = numbers_of(doc, dict, b"RD");
    let [left, top, right, bottom] = match rd.get(..4) {
        Some(&[l, t, r, b]) => [l, t, r, b],
        _ => return rect,
    };
    let valid = [left, top, right, bottom].iter().all(|v| *v >= 0.0)
        && left + right < rect.x1 - rect.x0
        && top + bottom < rect.y1 - rect.y0;
    if !valid {
        return rect;
    }
    Rect {
        x0: rect.x0 + left,
        y0: rect.y0 + bottom,
        x1: rect.x1 - right,
        y1: rect.y1 - top,
    }
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

    let dashed = set_up(doc, annotation, &paint, out);
    if paint.strokes() {
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
    }
    // The endings are drawn solid whatever the line is: a dashed arrowhead
    // is a broken one.
    if dashed {
        out.extend_from_slice(b"[] 0 d\n");
    }
    ending(out, first, p1, (-along.0, -along.1), along, &paint);
    ending(out, last, p2, along, along, &paint);
    Some(())
}

/// Sets up a line-like annotation's paint: the stroke colour, the fill
/// colour, and, when it strokes at all, the width and the dash. Returns
/// whether a dash was set, so that what is drawn after the path — an
/// ending — can be drawn solid.
fn set_up(doc: &CosDocument, annotation: &Dict, paint: &Paint, out: &mut Vec<u8>) -> bool {
    if let Some(stroke) = paint.stroke {
        op(out, &stroke, b"RG");
    }
    if let Some(fill) = paint.fill {
        op(out, &fill, b"rg");
    }
    if !paint.strokes() {
        return false;
    }
    op(out, &[paint.width], b"w");
    match dash_of(doc, annotation) {
        Some(pattern) => {
            dash(out, &pattern, 0.0);
            true
        }
        None => false,
    }
}

/// An array of alternating x and y coordinates, as points (12.5.6.9's
/// `/Vertices`, and each path of 12.5.6.13's `/InkList`). A last number
/// with no partner names no point and is dropped.
fn points_of(numbers: &[f64]) -> Vec<Point> {
    numbers.chunks_exact(2).map(|c| (c[0], c[1])).collect()
}

/// The direction a path leaves `from` by: towards the first of `rest` that
/// is not `from` itself, so a vertex written twice does not turn an ending
/// to face nowhere. `None` when every point is `from`.
fn leaving(from: Point, rest: impl Iterator<Item = Point>) -> Option<Point> {
    rest.filter_map(|(x, y)| unit(x - from.0, y - from.1))
        .next()
}

/// A polygon or a polyline (12.5.6.9): `/Vertices` joined by straight
/// lines in `/C` at the `/BS` width and dash — closed and filled with
/// `/IC` for a polygon; open for a polyline, with `/LE`'s endings at its
/// first and last vertex and `/IC` filling only the closed ones (Table 178:
/// "the polygon's (or polyline's line endings)").
///
/// An absent `/C` strokes black, as a line's does — the outline is what a
/// polygon is, where a square's border is optional. Fewer than two
/// distinct vertices join nothing, and draw nothing.
fn polygon(doc: &CosDocument, annotation: &Dict, out: &mut Vec<u8>, closed: bool) -> Option<()> {
    let points = points_of(&numbers_of(doc, annotation, b"Vertices"));
    let (&first, &last) = (points.first()?, points.last()?);
    let start = leaving(first, points.iter().copied())?;
    let paint = Paint {
        stroke: stroke_color_of(doc, annotation),
        fill: color_of(doc, annotation, b"IC"),
        width: border_width(doc, annotation),
    };
    let (first_end, last_end) = if closed {
        (Ending::None, Ending::None)
    } else {
        endings_of(doc, annotation)
    };
    let painter: &[u8] = if closed {
        paint.closed()?
    } else {
        let fills_an_end = paint.fill.is_some() && (first_end.is_closed() || last_end.is_closed());
        if !paint.strokes() && !fills_an_end {
            return None;
        }
        b"S\n"
    };

    let dashed = set_up(doc, annotation, &paint, out);
    if closed || paint.strokes() {
        op(out, &[first.0, first.1], b"m");
        for (x, y) in points.iter().skip(1) {
            op(out, &[*x, *y], b"l");
        }
        if closed {
            out.extend_from_slice(b"h\n");
        }
        out.extend_from_slice(painter);
    }
    if closed {
        return Some(());
    }
    if dashed {
        out.extend_from_slice(b"[] 0 d\n");
    }
    // The last segment's direction, from the last distinct vertex before
    // the end to the end.
    let (bx, by) = leaving(last, points.iter().rev().copied())?;
    let end = (-bx, -by);
    ending(out, first_end, first, (-start.0, -start.1), start, &paint);
    ending(out, last_end, last, end, end, &paint);
    Some(())
}

/// A quad's own frame (12.5.6.10): its lower-left corner, the unit vector
/// along its baseline to the lower-right corner, the unit vector up its
/// side towards the upper-left corner, its length and its height. `None`
/// for a quad with no baseline or no height.
struct QuadFrame {
    origin: Point,
    along: Point,
    up: Point,
    length: f64,
    height: f64,
}

fn quad_frame(quad: &[f64; 8]) -> Option<QuadFrame> {
    let origin = (quad[4], quad[5]);
    let (dx, dy) = (quad[6] - quad[4], quad[7] - quad[5]);
    let along = unit(dx, dy)?;
    let length = (dx * dx + dy * dy).sqrt();
    // A quarter turn counter-clockwise of the baseline, turned over when the
    // upper-left corner is on the other side of it, so that what is drawn
    // in the frame is always on the quad's side of its baseline.
    let mut up = (-along.1, along.0);
    let mut height = (quad[0] - origin.0) * up.0 + (quad[1] - origin.1) * up.1;
    if height < 0.0 {
        up = (-up.0, -up.1);
        height = -height;
    }
    // Adding zero turns a negative zero, which the frame's `cm` would write
    // as `-0`, into a zero.
    let (along, up) = ((along.0 + 0.0, along.1 + 0.0), (up.0 + 0.0, up.1 + 0.0));
    (height.is_finite() && height > 1e-9).then_some(QuadFrame {
        origin,
        along,
        up,
        length,
        height,
    })
}

/// A squiggly underline (12.5.6.10): a zigzag under each quad's text, in
/// `/C` — black when `/C` is absent or empty, the rule `Underline` follows
/// here.
///
/// Drawn in each quad's own frame, so a quad on rotated text gets a zigzag
/// along its own baseline. The zigzag's band runs from 3% to 3% + ⅙ of the
/// quad's height above the baseline, its strokes rise and fall at forty-five
/// degrees — so a tooth is twice as wide as the band is high — and they are
/// as thick as an underline's line.
///
/// It is drawn **without a vertex per tooth**. A quad is eight numbers, and
/// a loop over its teeth would let eight numbers ask for as much content as
/// their length over their height: a quad a kilometre long on a point of
/// text is a million teeth. Instead the band is clipped, and each set of
/// parallel strokes is the dashes of one line drawn across them — a line
/// running up and to the right, as wide as the band is long, whose dashes,
/// cut square across it, are the falling strokes; and one running down and
/// to the right for the rising ones. That is a constant number of operators
/// per quad whatever its length; how many dashes a line that long becomes
/// is the renderer's to bound, as it is for any dashed line.
///
/// The lines are diagonal in the quad's frame rather than straight under a
/// shear — which would make the same dashes with less arithmetic — because
/// a shear is not a similarity, and a renderer that strokes in device space
/// with one scale for the width, as this engine's does, would square every
/// sheared dash back up into a vertical bar.
fn squiggly(doc: &CosDocument, annotation: &Dict, out: &mut Vec<u8>) -> Option<()> {
    const K: f64 = std::f64::consts::FRAC_1_SQRT_2;
    let color = color_of(doc, annotation, b"C").unwrap_or([0.0, 0.0, 0.0]);
    let frames: Vec<QuadFrame> = quads_of(doc, annotation)
        .iter()
        .filter_map(quad_frame)
        .collect();
    if frames.is_empty() {
        return None;
    }
    op(out, &color, b"RG");
    for frame in frames {
        let (length, h) = (frame.length, frame.height);
        let band = h / 6.0;
        let (low, high) = (h * 0.03, h * 0.03 + band);
        // Strokes at forty-five degrees a tooth's half-width apart, measured
        // along the band, are `band * sqrt 2` apart measured across them.
        let period = band * std::f64::consts::SQRT_2;
        // Never so thick that the gap between two strokes closes.
        let thickness = (h * 0.07).max(0.5).min(period / 2.0);
        // Wide enough that every dash crosses the whole band at any point
        // along it.
        let width = std::f64::consts::SQRT_2 * (length + band);
        let (cx, cy) = (length / 2.0, (low + high) / 2.0);

        out.extend_from_slice(b"q\n");
        let QuadFrame {
            origin, along, up, ..
        } = frame;
        op(
            out,
            &[along.0, along.1, up.0, up.1, origin.0, origin.1],
            b"cm",
        );
        op(out, &[0.0, low, length, band], b"re");
        out.extend_from_slice(b"W n\n");
        op(out, &[width], b"w");
        dash(out, &[thickness, period - thickness], 0.0);

        // The falling strokes lie along x + y = low + 2k * band, so the
        // line runs along (1, 1), measured by s = (x + y) / sqrt 2, through
        // the band's centre; its first dash is centred on the stroke that
        // reaches the baseline at x = 0.
        let across = (cy - cx) * K;
        let (s0, s1) = (low * K - thickness / 2.0, (length + high) * K + period);
        op(out, &[(s0 - across) * K, (s0 + across) * K], b"m");
        op(out, &[(s1 - across) * K, (s1 + across) * K], b"l");
        out.extend_from_slice(b"S\n");
        // The rising strokes lie along x - y = 2k * band - low: the line
        // runs along (1, -1), measured by u = (x - y) / sqrt 2, its first
        // dash centred on the stroke that rises from the baseline at x = 0.
        let across = (cx + cy) * K;
        let (u0, u1) = (-low * K - thickness / 2.0, (length - low) * K + period);
        op(out, &[(u0 + across) * K, (across - u0) * K], b"m");
        op(out, &[(u1 + across) * K, (across - u1) * K], b"l");
        out.extend_from_slice(b"S\nQ\n");
    }
    Some(())
}

/// A caret (12.5.6.11), the mark where text is to be inserted: filled in
/// `/C` inside `/Rect` less Table 180's `/RD`, "the actual boundaries of the
/// underlying caret".
///
/// 12.5.6.11 names the symbol and not its outline, so this draws the
/// typographic caret: a spike rising from the middle of the bottom edge to
/// the top, its two sides cubics bowed inward from the bottom corners, the
/// bottom edge straight. An absent `/C` fills black, as a line strokes;
/// an empty one is 12.5.2's transparent, and there is nothing to draw.
///
/// `/Sy /P` asks for a paragraph symbol "associated with the caret", and
/// 12.5.6.11 places it nowhere; the caret is drawn and the symbol is not,
/// and the feature doc's refusal table names it.
fn caret(doc: &CosDocument, annotation: &Dict, rect: Rect, out: &mut Vec<u8>) -> Option<()> {
    let color = stroke_color_of(doc, annotation)?;
    let caret = drawn_rect(doc, annotation, rect);
    let middle = (caret.x0 + caret.x1) / 2.0;
    let half = (caret.y0 + caret.y1) / 2.0;
    op(out, &color, b"rg");
    op(out, &[caret.x0, caret.y0], b"m");
    op(
        out,
        &[middle, caret.y0, middle, half, middle, caret.y1],
        b"c",
    );
    op(
        out,
        &[middle, half, middle, caret.y0, caret.x1, caret.y0],
        b"c",
    );
    out.extend_from_slice(b"h\nf\n");
    Some(())
}

/// `/InkList` (12.5.6.13 Table 182): one path per element, each an array of
/// alternating x and y coordinates.
///
/// A path written as a reference is followed once: a second reference to
/// the same array is the same path again, and skipping it keeps what a
/// small file can ask for to what it holds — `[5 0 R 5 0 R ...]` would
/// otherwise draw one large array as many times as the list names it, and
/// under `/CA` a stroke drawn twice over itself is darker than one.
fn ink_paths(doc: &CosDocument, dict: &Dict) -> Vec<Vec<Point>> {
    let list = doc.resolve_key(dict, doc.intern(b"InkList"));
    let Some(items) = list.as_array() else {
        return Vec::new();
    };
    let mut seen = std::collections::HashSet::new();
    items
        .iter()
        .filter(|item| item.as_objref().is_none_or(|r| seen.insert(r)))
        .filter_map(|item| {
            let path = doc.resolve(item);
            let numbers: Vec<f64> = path
                .as_array()?
                .iter()
                .filter_map(Object::as_number)
                .filter(|v| v.is_finite())
                .collect();
            let points = points_of(&numbers);
            (!points.is_empty()).then_some(points)
        })
        .collect()
}

/// An ink annotation (12.5.6.13): each of `/InkList`'s paths stroked in
/// `/C` at the `/BS` width and dash.
///
/// Table 182 leaves how the points are joined to the implementation —
/// "straight lines or curves" — and this joins them with straight lines,
/// which go through every point the pen recorded, with round caps and
/// joins, the shape a pen leaves; a path of one point is a dot. An absent
/// `/C` strokes black, as a line's does.
fn ink(doc: &CosDocument, annotation: &Dict, out: &mut Vec<u8>) -> Option<()> {
    let paths = ink_paths(doc, annotation);
    let paint = Paint {
        stroke: stroke_color_of(doc, annotation),
        fill: None,
        width: border_width(doc, annotation),
    };
    if paths.is_empty() || !paint.strokes() {
        return None;
    }
    set_up(doc, annotation, &paint, out);
    out.extend_from_slice(b"1 J\n1 j\n");
    for path in paths {
        let mut points = path.iter();
        let Some(&(x, y)) = points.next() else {
            continue;
        };
        op(out, &[x, y], b"m");
        if path.len() == 1 {
            // 8.5.3.2: a subpath of no length is a dot under round caps.
            op(out, &[x, y], b"l");
        }
        for (x, y) in points {
            op(out, &[*x, *y], b"l");
        }
    }
    out.extend_from_slice(b"S\n");
    Some(())
}

/// What a `/DA` string says (12.7.3.3): the font resource's name, its size
/// (zero asks for one to be chosen), and the colour its `g`, `rg` or `k`
/// sets, black when it sets none this reads.
///
/// Only those three are taken. The rest of the string is not replayed into
/// the appearance: it is the producer's text, and an operator in it — a `Q`,
/// an `ET` — would unbalance the stream it was copied into.
struct DefaultAppearance {
    font: Vec<u8>,
    size: f64,
    color: [f64; 3],
}

fn default_appearance(da: &[u8]) -> Option<DefaultAppearance> {
    let tokens: Vec<&[u8]> = da
        .split(|b| b.is_ascii_whitespace())
        .filter(|t| !t.is_empty())
        .collect();
    let number = |token: &[u8]| -> Option<f64> {
        std::str::from_utf8(token)
            .ok()?
            .parse::<f64>()
            .ok()
            .filter(|v| v.is_finite())
    };
    let mut font = None;
    let mut color = [0.0, 0.0, 0.0];
    for (index, token) in tokens.iter().enumerate() {
        // The operands of the operator at `index`, the `n` tokens before it.
        let operands = |n: usize| -> Option<Vec<f64>> {
            let start = index.checked_sub(n)?;
            tokens
                .get(start..index)?
                .iter()
                .map(|t| number(t))
                .collect()
        };
        match *token {
            b"Tf" => {
                let name = index.checked_sub(2).and_then(|i| tokens.get(i));
                let size = index.checked_sub(1).and_then(|i| tokens.get(i));
                if let (Some(name), Some(size)) = (name, size) {
                    if let (Some(name), Some(size)) = (name.strip_prefix(b"/"), number(size)) {
                        font = Some((name.to_vec(), size.max(0.0)));
                    }
                }
            }
            b"g" => {
                if let Some(&[g]) = operands(1).as_deref() {
                    color = [g, g, g];
                }
            }
            b"rg" => {
                if let Some(&[r, g, b]) = operands(3).as_deref() {
                    color = [r, g, b];
                }
            }
            b"k" => {
                if let Some(&[c, m, y, k]) = operands(4).as_deref() {
                    color = [
                        (1.0 - c) * (1.0 - k),
                        (1.0 - m) * (1.0 - k),
                        (1.0 - y) * (1.0 - k),
                    ];
                }
            }
            _ => {}
        }
    }
    let (font, size) = font?;
    Some(DefaultAppearance {
        font,
        size,
        color: color.map(|c| c.clamp(0.0, 1.0)),
    })
}

/// The font a `/DA` names, from the interactive form's `/DR` (12.7.3.3):
/// the entry as `/DR` holds it — a reference, or a dictionary written
/// inline — for the appearance's own `/Resources`, and the font as read.
///
/// Only a simple font with a byte per glyph — Type 1 or TrueType — is taken,
/// because a character is written here as one byte, the way a field's value
/// is (`fill::escape`); a composite or Type 3 font is refused.
fn form_font(doc: &CosDocument, name: &[u8]) -> Option<(Object, crate::font::Font)> {
    let form = crate::form::acro_form(doc)?;
    let resources = doc.resolve_key(&form, doc.intern(b"DR"));
    let fonts = doc.resolve_key(resources.as_dict()?, doc.intern(b"Font"));
    let entry = fonts.as_dict()?.get(doc.intern(name))?.clone();
    let dict = doc.resolve(&entry).as_dict()?.clone();
    let font = crate::font::read(doc, &dict);
    let simple = matches!(
        font.kind(),
        crate::font::FontKind::Type1 | crate::font::FontKind::TrueType
    );
    simple.then_some((entry, font))
}

/// Breaks text into the lines a box `width` wide holds at `size` (12.7.3.3's
/// multiline layout): at each line break in the text, and otherwise at the
/// last space that fits, or — for a word longer than the box — between two
/// characters.
fn wrap(text: &str, font: &crate::font::Font, size: f64, width: f64) -> Vec<String> {
    let advance = |c: char| font.width_of(u32::from(c)).0 * size / 1000.0;
    let mut lines = Vec::new();
    for paragraph in text.split("\r\n").flat_map(|p| p.split(['\r', '\n'])) {
        let mut line = String::new();
        let mut line_width = 0.0;
        for word in paragraph.split(' ') {
            let word_width: f64 = word.chars().map(advance).sum();
            let space = if line.is_empty() { 0.0 } else { advance(' ') };
            if !line.is_empty() && line_width + space + word_width > width {
                lines.push(std::mem::take(&mut line));
                line_width = 0.0;
            } else if !line.is_empty() {
                line.push(' ');
                line_width += space;
            }
            for c in word.chars() {
                let w = advance(c);
                if !line.is_empty() && line_width + w > width && line_width > 0.0 {
                    lines.push(std::mem::take(&mut line));
                    line_width = 0.0;
                }
                line.push(c);
                line_width += w;
            }
        }
        lines.push(line);
    }
    lines
}

/// A free text annotation (12.5.6.6): its box — `/Rect` less `/RD` —
/// filled with `/C`, bordered at the `/BS` width and dash, `/Contents`
/// written inside it in the `/DA` font, size and colour, aligned by `/Q`,
/// and, for `/IT /FreeTextCallout`, the `/CL` callout line with its `/LE`
/// ending at the point it calls out.
///
/// ISO 32000-1 names no colour for the border; it and the callout are
/// stroked in the text's colour, `/C` being 12.5.2's "background of the
/// annotation's icon" — which a free text annotation's box is — and an
/// absent `/C` leaves the box unfilled. The text is laid out as a multiline
/// field's is: two units in from the border, the first baseline 0.85 of a
/// line below the top, lines 1.15 apart, clipped to the box; a `/DA` size of
/// zero takes the largest whole size up to twelve at which every line fits.
///
/// Returns the font resource the text needs, under its `/DA` name. `None`
/// — no appearance at all — when the `/DA` names no font the form's `/DR`
/// holds as a simple font, or `/Contents` has a character such a font has
/// no byte for: a box with question marks in it is a wrong appearance, and
/// this module would rather draw none.
fn free_text(
    doc: &CosDocument,
    annotation: &Dict,
    rect: Rect,
    out: &mut Vec<u8>,
) -> Option<(Vec<u8>, Object)> {
    let da = doc.resolve_key(annotation, doc.intern(b"DA"));
    let da = default_appearance(&da.as_string()?.bytes)?;
    let (resource, font) = form_font(doc, &da.font)?;
    let contents = match doc
        .resolve_key(annotation, doc.intern(b"Contents"))
        .as_string()
    {
        Some(string) => crate::decode_text_string(&string.bytes),
        None => String::new(),
    };
    if contents.chars().any(|c| u32::from(c) > 0xFF) {
        return None;
    }

    let boxed = drawn_rect(doc, annotation, rect);
    let paint = Paint {
        stroke: Some(da.color),
        fill: color_of(doc, annotation, b"C"),
        width: border_width(doc, annotation),
    };
    if let Some(fill) = paint.fill {
        op(out, &fill, b"rg");
        op(
            out,
            &[boxed.x0, boxed.y0, boxed.x1 - boxed.x0, boxed.y1 - boxed.y0],
            b"re",
        );
        out.extend_from_slice(b"f\n");
    }
    let dashed = set_up(
        doc,
        annotation,
        &Paint {
            fill: None,
            ..paint
        },
        out,
    );
    if paint.strokes() {
        let border = inset(boxed, paint.width / 2.0);
        op(
            out,
            &[
                border.x0,
                border.y0,
                border.x1 - border.x0,
                border.y1 - border.y0,
            ],
            b"re",
        );
        out.extend_from_slice(b"S\n");
        callout(doc, annotation, &paint, dashed, out);
    }

    let inner = inset(boxed, paint.width + 2.0);
    let (inner_w, inner_h) = (inner.x1 - inner.x0, inner.y1 - inner.y0);
    // Lines fit when the last one's descender — a quarter of the size below
    // its baseline — is inside the box.
    let fits = |size: f64| {
        let lines = wrap(&contents, &font, size, inner_w);
        let height = size * (0.85 + 1.15 * (lines.len() as f64 - 1.0) + 0.25);
        (height <= inner_h).then_some(lines)
    };
    let (size, lines) = if da.size > 0.0 {
        (da.size, wrap(&contents, &font, da.size, inner_w))
    } else {
        (4..=12u8)
            .rev()
            .find_map(|s| fits(f64::from(s)).map(|lines| (f64::from(s), lines)))
            .unwrap_or_else(|| (4.0, wrap(&contents, &font, 4.0, inner_w)))
    };

    if lines.iter().any(|line| !line.is_empty()) && inner_w > 0.0 && inner_h > 0.0 {
        let quadding = doc
            .resolve_key(annotation, doc.intern(b"Q"))
            .as_int()
            .unwrap_or(0);
        out.extend_from_slice(b"q\n");
        op(out, &[inner.x0, inner.y0, inner_w, inner_h], b"re");
        out.extend_from_slice(b"W n\nBT\n/");
        out.extend_from_slice(&da.font);
        out.push(b' ');
        op(out, &[size], b"Tf");
        op(out, &da.color, b"rg");
        // The leading is said once, and each line after the first moves by
        // it (`T*`) and by how far its start is from the last one's — so a
        // line costs its text and at most one more number, rather than the
        // whole matrix again.
        let leading = size * 1.15;
        op(out, &[leading], b"TL");
        let mut unwritable = Vec::new();
        let mut previous: Option<f64> = None;
        for (index, line) in lines.iter().enumerate() {
            let y = inner.y1 - size * 0.85 - index as f64 * leading;
            // A line wholly below the box is clipped away, and so is every
            // one after it, so none of them is written; and an appearance
            // longer than any stream this crate decodes is not one its own
            // reader could draw (`MAX_DECODED_STREAM`).
            if y + size < inner.y0 || out.len() > crate::limits::MAX_DECODED_STREAM {
                break;
            }
            let line_width: f64 = line
                .chars()
                .map(|c| font.width_of(u32::from(c)).0 * size / 1000.0)
                .sum();
            let x = match quadding {
                1 => inner.x0 + (inner_w - line_width) / 2.0,
                2 => inner.x1 - line_width,
                _ => inner.x0,
            };
            match previous {
                None => op(out, &[1.0, 0.0, 0.0, 1.0, x, y], b"Tm"),
                Some(last) if (x - last).abs() < 1e-9 => out.extend_from_slice(b"T*\n"),
                Some(last) => {
                    op(out, &[x - last, 0.0], b"Td");
                    out.extend_from_slice(b"T*\n");
                }
            }
            previous = Some(x);
            out.push(b'(');
            crate::fill::escape(out, line, &mut unwritable);
            out.extend_from_slice(b") Tj\n");
        }
        out.extend_from_slice(b"ET\nQ\n");
    }
    Some((da.font, resource))
}

/// A free text callout (12.5.6.6 Table 174 `/CL`): two or three points from
/// the point called out to the box, stroked as the border is, with `/LE`'s
/// ending at the first point. Table 174 makes `/CL` meaningful only under
/// `/IT /FreeTextCallout`, and it is drawn only then.
fn callout(doc: &CosDocument, annotation: &Dict, paint: &Paint, dashed: bool, out: &mut Vec<u8>) {
    let intent = annotation
        .get_name(doc.intern(b"IT"))
        .and_then(|name| doc.name_bytes(name));
    if intent.as_deref() != Some(b"FreeTextCallout".as_slice()) {
        return;
    }
    let numbers = numbers_of(doc, annotation, b"CL");
    let points = match numbers.len() {
        4 | 6 => points_of(&numbers),
        _ => return,
    };
    let (Some(&first), Some(start)) = (
        points.first(),
        points
            .first()
            .and_then(|p| leaving(*p, points.iter().copied())),
    ) else {
        return;
    };
    op(out, &[first.0, first.1], b"m");
    for (x, y) in points.iter().skip(1) {
        op(out, &[*x, *y], b"l");
    }
    out.extend_from_slice(b"S\n");
    if dashed {
        out.extend_from_slice(b"[] 0 d\n");
    }
    let kind = annotation
        .get_name(doc.intern(b"LE"))
        .and_then(|name| doc.name_bytes(name))
        .map_or(Ending::None, |name| Ending::from_name(&name));
    let ending_paint = Paint {
        fill: color_of(doc, annotation, b"IC"),
        ..*paint
    };
    if let Some(fill) = ending_paint.fill.filter(|_| kind.is_closed()) {
        op(out, &fill, b"rg");
    }
    ending(out, kind, first, (-start.0, -start.1), start, &ending_paint);
}

/// The subtypes whose appearance their dictionary does not determine
/// (12.5.6, and ISO 32000-2's additions), which [`synthesize`] declines by
/// name: what each would show is a picture, a medium, a viewer's window or
/// a computation, and none of it is in the dictionary.
///
/// - `Stamp` (12.5.6.12), `FileAttachment` (12.5.6.15) and `Sound`
///   (12.5.6.16): `/Name` names an icon — `Approved`, `PushPin`, `Speaker` —
///   and 12.5.6 gives none of them an outline.
/// - `Movie` (12.5.6.17), `Screen` (12.5.6.18), `3D` (13.6.2) and
///   `RichMedia` (ISO 32000-2 13.7.2): the medium's own frame, poster or
///   view.
/// - `Popup` (12.5.6.14): the viewer's window for its parent's text.
/// - `Widget` (12.5.6.19): a field's, which the form filler builds from its
///   value (`fill.rs`), not from the annotation.
/// - `PrinterMark` (12.5.6.20), `TrapNet` (12.5.6.21) and `Watermark`
///   (12.5.6.22): a mark, a trapping result and a placement that exist only
///   as the `/AP` their producer wrote.
/// - `Redact` (12.5.6.23): its entries say what replaces the content once
///   the redaction is applied — `/IC`, `/RO`, `/OverlayText` — and not what
///   the mark looks like before.
/// - `Projection` (ISO 32000-2 12.5.6.24), which adds no entry at all.
pub const UNDETERMINED_SUBTYPES: &[&str] = &[
    "Stamp",
    "FileAttachment",
    "Sound",
    "Movie",
    "Screen",
    "3D",
    "RichMedia",
    "Popup",
    "Widget",
    "PrinterMark",
    "TrapNet",
    "Watermark",
    "Redact",
    "Projection",
];

/// Builds the appearance for an annotation, or `None` when its type needs
/// none — a link with no border draws nothing, and inventing something for it
/// would be worse than leaving it alone — or when its dictionary does not say
/// what it looks like: [`UNDETERMINED_SUBTYPES`], and a subtype 12.5.6 does
/// not name.
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
    // The font a free text annotation's text is shown in, under its `/DA`
    // name.
    let mut font = None;

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

            let box_ = inset(drawn_rect(doc, annotation, rect), width / 2.0);
            if let Some(fill) = fill {
                op(&mut content, &fill, b"rg");
            }
            if let Some(stroke) = stroke {
                op(&mut content, &stroke, b"RG");
                op(&mut content, &[width], b"w");
                if let Some(pattern) = dash_of(doc, annotation) {
                    dash(&mut content, &pattern, 0.0);
                }
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

            let box_ = inset(drawn_rect(doc, annotation, rect), width / 2.0);
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
                if let Some(pattern) = dash_of(doc, annotation) {
                    dash(&mut content, &pattern, 0.0);
                }
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
        b"Polygon" => polygon(doc, annotation, &mut content, true)?,
        b"PolyLine" => polygon(doc, annotation, &mut content, false)?,
        b"Squiggly" => squiggly(doc, annotation, &mut content)?,
        b"Caret" => caret(doc, annotation, rect, &mut content)?,
        b"Ink" => ink(doc, annotation, &mut content)?,
        b"FreeText" => font = Some(free_text(doc, annotation, rect, &mut content)?),
        // Declined by name: what these look like is not in their dictionary.
        named if UNDETERMINED_SUBTYPES.iter().any(|s| s.as_bytes() == named) => return None,
        // And a subtype 12.5.6 does not name, declined as unknown.
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
    if let Some((name, entry)) = font {
        let mut fonts = Dict::new();
        fonts.insert(doc.intern(&name), entry);
        resources.insert(doc.intern(b"Font"), Object::Dict(fonts));
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
        // This used to be `/Polygon`, until a polygon was drawn.
        dict.insert(
            doc.intern(b"Subtype"),
            Object::Name(doc.intern(b"Trapezium")),
        );
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

    /// 12.5.6.8's `/RD` insets the drawn shape within `/Rect` — left, top,
    /// right, bottom — before the border is inset by half its width; one
    /// Table 177 forbids is not read.
    #[test]
    fn a_shape_is_drawn_inside_its_rect_differences() {
        let doc = doc();
        let square = |rd: &str| {
            content_of(
                &doc,
                &format!(
                    "<< /Subtype /Square /Rect [10 20 110 60] /C [1 0 0] /BS << /W 2 >> {rd} >>"
                ),
            )
            .expect("a square")
        };
        assert_eq!(
            square("/RD [5 4 3 2]"),
            "1 0 0 RG\n2 w\n16 23 90 32 re\nS\n"
        );
        for ignored in [
            "/RD [60 0 50 0]",
            "/RD [0 20 0 20]",
            "/RD [-1 0 0 0]",
            "/RD [1 2 3]",
        ] {
            assert_eq!(
                square(ignored),
                "1 0 0 RG\n2 w\n11 21 98 38 re\nS\n",
                "{ignored} is not read"
            );
        }

        let circle = content_of(
            &doc,
            "<< /Subtype /Circle /Rect [10 20 110 60] /C [1 0 0] /BS << /W 2 >> \
             /RD [10 0 10 0] >>",
        )
        .expect("a circle");
        assert!(circle.contains("\n21 40 m\n"), "{circle}");
    }

    /// A square's and a circle's border is dashed as a line's is.
    #[test]
    fn a_shapes_border_is_dashed() {
        let doc = doc();
        let square = content_of(
            &doc,
            "<< /Subtype /Square /Rect [10 20 110 60] /C [1 0 0] \
             /BS << /W 2 /S /D /D [4 2] >> >>",
        )
        .expect("a square");
        assert_eq!(square, "1 0 0 RG\n2 w\n[4 2] 0 d\n11 21 98 38 re\nS\n");
        let circle = content_of(
            &doc,
            "<< /Subtype /Circle /Rect [10 20 110 60] /C [1 0 0] /BS << /W 2 /S /D >> >>",
        )
        .expect("a circle");
        assert!(circle.starts_with("1 0 0 RG\n2 w\n[3] 0 d\n"), "{circle}");
        let unstroked = content_of(
            &doc,
            "<< /Subtype /Square /Rect [10 20 110 60] /IC [0 0 1] /BS << /W 2 /S /D >> >>",
        )
        .expect("a filled square");
        assert!(
            !unstroked.contains(" d\n"),
            "nothing is stroked: {unstroked}"
        );
    }

    /// 12.5.6.9: a polygon is its vertices joined and closed, filled with
    /// `/IC`; a polyline is the same path left open, with no fill.
    #[test]
    fn a_polygon_is_closed_and_a_polyline_is_not() {
        let doc = doc();
        let drawn = |subtype: &str, rest: &str| {
            content_of(
                &doc,
                &format!(
                    "<< /Subtype /{subtype} /Rect [0 0 100 100] \
                     /Vertices [20 20 80 20 50 80] {rest} >>"
                ),
            )
        };
        assert_eq!(
            drawn("Polygon", "/C [1 0 0] /IC [0 0 1] /BS << /W 2 >>").as_deref(),
            Some("1 0 0 RG\n0 0 1 rg\n2 w\n20 20 m\n80 20 l\n50 80 l\nh\nB\n")
        );
        assert_eq!(
            drawn("PolyLine", "/C [1 0 0] /BS << /W 2 >>").as_deref(),
            Some("1 0 0 RG\n2 w\n20 20 m\n80 20 l\n50 80 l\nS\n")
        );
        assert_eq!(
            drawn("Polygon", "").as_deref(),
            Some("0 0 0 RG\n1 w\n20 20 m\n80 20 l\n50 80 l\nh\nS\n"),
            "an absent /C strokes black at the default width"
        );
        assert_eq!(
            drawn("Polygon", "/C [] /IC [0 0 1]").as_deref(),
            Some("0 0 1 rg\n20 20 m\n80 20 l\n50 80 l\nh\nf\n"),
            "a transparent /C fills only"
        );
        assert_eq!(drawn("Polygon", "/C []"), None, "nothing to paint");
        assert_eq!(
            drawn("PolyLine", "/C [] /IC [0 0 1]"),
            None,
            "a polyline's /IC is for its endings, and it has none"
        );
        let dashed = drawn("Polygon", "/BS << /W 2 /S /D >>").expect("a polygon");
        assert!(dashed.contains("2 w\n[3] 0 d\n20 20 m\n"), "{dashed}");
    }

    #[test]
    fn a_polygon_needs_two_distinct_vertices() {
        let doc = doc();
        for subtype in ["Polygon", "PolyLine"] {
            for vertices in [
                "",
                "/Vertices []",
                "/Vertices [10 10]",
                "/Vertices [10 10 10 10 10 10]",
                "/Vertices [10 10 20]",
                "/Vertices 4",
            ] {
                assert_eq!(
                    content_of(
                        &doc,
                        &format!("<< /Subtype /{subtype} /Rect [0 0 100 100] {vertices} >>")
                    ),
                    None,
                    "/{subtype} with {vertices:?} joins nothing"
                );
            }
        }
        assert_eq!(
            content_of(
                &doc,
                "<< /Subtype /PolyLine /Rect [0 0 100 100] /Vertices [10 10 20 20 30] >>"
            )
            .as_deref(),
            Some("0 0 0 RG\n1 w\n10 10 m\n20 20 l\nS\n"),
            "a last number with no partner is dropped"
        );
    }

    /// A polyline's endings face away along its first and last segments,
    /// past a vertex written twice: an open arrow pointing west from the
    /// first vertex, and a butt across the last segment, which runs north.
    #[test]
    fn a_polylines_endings_face_along_its_end_segments() {
        let doc = doc();
        let content = content_of(
            &doc,
            "<< /Subtype /PolyLine /Rect [0 0 100 100] /C [1 0 0] /LE [/OpenArrow /Butt] \
             /Vertices [20 20 20 20 80 20 80 70 80 70] >>",
        )
        .expect("a polyline");
        assert_eq!(
            content,
            "1 0 0 RG\n1 w\n20 20 m\n20 20 l\n80 20 l\n80 70 l\n80 70 l\nS\n\
             26 16.5359 m\n20 20 l\n26 23.4641 l\nS\n77 70 m\n83 70 l\nS\n"
        );
        let filled_only = content_of(
            &doc,
            "<< /Subtype /PolyLine /Rect [0 0 100 100] /C [] /IC [0 0 1] \
             /LE [/None /Diamond] /Vertices [20 20 80 20] >>",
        )
        .expect("the diamond is filled");
        assert_eq!(
            filled_only, "0 0 1 rg\n83 20 m\n80 23 l\n77 20 l\n80 17 l\nh\nf\n",
            "a polyline with no stroke draws only its closed endings"
        );
    }

    /// A squiggly underline, quad by quad: the quad's own frame, the band
    /// clipped, and the falling and rising strokes as the dashes of two
    /// diagonal lines. A quad sixty high has a band ten high, 1.8 above its
    /// baseline, strokes 4.2 thick and ten times the square root of two
    /// apart across them.
    #[test]
    fn a_squiggly_is_two_dashed_diagonals_a_quad() {
        let doc = doc();
        let squiggly = |quads: &str, rest: &str| {
            content_of(
                &doc,
                &format!(
                    "<< /Subtype /Squiggly /Rect [0 0 100 100] /QuadPoints [{quads}] {rest} >>"
                ),
            )
        };
        let content = squiggly("10 80 90 80 10 20 90 20", "/C [1 0 0]").expect("a squiggle");
        let head = "1 0 0 RG\nq\n1 0 0 1 10 20 cm\n0 1.8 80 10 re\nW n\n127.2792 w\n\
                    [4.2 9.9421] 0 d\n";
        assert!(content.starts_with(head), "{content}");
        assert!(content.ends_with("S\nQ\n"), "{content}");

        // The two lines, each `x y m`, `x y l`, `S`.
        let numbers: Vec<f64> = content[head.len()..]
            .split_whitespace()
            .filter_map(|word| word.parse().ok())
            .collect();
        let [fx0, fy0, fx1, fy1, rx0, ry0, rx1, ry1] = numbers[..] else {
            panic!("two lines of two points: {content}");
        };
        assert!(
            ((fy1 - fy0) / (fx1 - fx0) - 1.0).abs() < 1e-3,
            "the falling strokes' line runs up and to the right"
        );
        assert!(
            ((ry1 - ry0) / (rx1 - rx0) + 1.0).abs() < 1e-3,
            "the rising strokes' line runs down and to the right"
        );
        // Each line's first dash is centred on the stroke through (0, 1.8):
        // its start is half a stroke short of it, measured along the line.
        let k = std::f64::consts::FRAC_1_SQRT_2;
        assert!(((fx0 + fy0) * k - (1.8 * k - 2.1)).abs() < 1e-3);
        assert!(((rx0 - ry0) * k - (-1.8 * k - 2.1)).abs() < 1e-3);
        // And each passes through the band's middle, (40, 6.8).
        assert!(((fy0 - fx0) - (6.8 - 40.0)).abs() < 1e-3);
        assert!(((ry0 + rx0) - (6.8 + 40.0)).abs() < 1e-3);

        assert_eq!(
            squiggly("10 80 90 80 10 20 90 20", "")
                .as_deref()
                .map(|c| &c[..8]),
            Some("0 0 0 RG"),
            "black by default, as an underline is"
        );
        // Turned a quarter: the baseline runs up x = 80, the height leftward.
        let turned = squiggly("20 10 20 90 80 10 80 90", "").expect("a squiggle");
        assert!(
            turned.contains("q\n0 1 -1 0 80 10 cm\n0 1.8 80 10 re\n"),
            "{turned}"
        );
        // With the upper-left corner below the baseline, the frame turns
        // over rather than drawing the zigzag outside the quad.
        let flipped = squiggly("10 20 90 20 10 80 90 80", "").expect("a squiggle");
        assert!(
            flipped.contains("q\n1 0 0 -1 10 80 cm\n0 1.8 80 10 re\n"),
            "{flipped}"
        );

        for degenerate in [
            "",
            "10 80 90 80 10 20",
            "10 20 90 20 10 20 90 20",
            "10 80 10 80 10 20 10 20",
        ] {
            assert_eq!(
                squiggly(degenerate, ""),
                None,
                "{degenerate:?} has no frame"
            );
        }
        assert_eq!(
            squiggly(
                "10 20 90 20 10 20 90 20 10 80 90 80 10 20 90 20",
                "/C [1 0 0]"
            ),
            Some(content),
            "a quad with no height is skipped and the next drawn"
        );
    }

    /// A quad a million points long on text one point high draws the same
    /// operators as a short one: what a quad costs does not grow with it.
    #[test]
    fn a_squiggly_quad_costs_the_same_however_long_it_is() {
        let doc = doc();
        let ops = |quads: &str| {
            let content = content_of(
                &doc,
                &format!("<< /Subtype /Squiggly /Rect [0 0 100 100] /QuadPoints [{quads}] >>"),
            )
            .expect("a squiggle");
            (content.lines().count(), content.len())
        };
        let (short_lines, _) = ops("0 1 10 1 0 0 10 0");
        let (long_lines, long_bytes) = ops("0 1 1000000 1 0 0 1000000 0");
        assert_eq!(short_lines, long_lines);
        assert!(long_bytes < 400, "{long_bytes} bytes");
    }

    /// 12.5.6.11: the caret fills the rectangle `/RD` leaves inside `/Rect`,
    /// from its bottom corners to the middle of its top; `/Sy /P` draws the
    /// same caret, and its paragraph symbol nowhere.
    #[test]
    fn a_caret_is_drawn_inside_its_rect_differences() {
        let doc = doc();
        let caret = |rest: &str| {
            content_of(
                &doc,
                &format!("<< /Subtype /Caret /Rect [10 10 90 90] {rest} >>"),
            )
        };
        assert_eq!(
            caret("/RD [10 10 10 10] /C [0 0 1]").as_deref(),
            Some("0 0 1 rg\n20 20 m\n50 20 50 50 50 80 c\n50 50 50 20 80 20 c\nh\nf\n")
        );
        assert_eq!(
            caret("").as_deref(),
            Some("0 0 0 rg\n10 10 m\n50 10 50 50 50 90 c\n50 50 50 10 90 10 c\nh\nf\n"),
            "black without /C, in the whole /Rect without /RD"
        );
        assert_eq!(caret("/C []"), None, "a transparent caret draws nothing");
        assert_eq!(caret("/Sy /P"), caret("/Sy /None"));
    }

    /// 12.5.6.13: each path of `/InkList` is a subpath of one stroke, joined
    /// with straight lines and round caps and joins; a path of one point is
    /// a dot, and an empty one is nothing.
    #[test]
    fn an_ink_annotation_strokes_each_of_its_paths() {
        let doc = doc();
        let ink = |rest: &str| {
            content_of(
                &doc,
                &format!("<< /Subtype /Ink /Rect [0 0 100 100] {rest} >>"),
            )
        };
        assert_eq!(
            ink("/InkList [[10 20 50 60 90 20] [] [10 80 90 80 7] [40 40]] /C [0 0 1] /BS << /W 3 >>")
                .as_deref(),
            Some(
                "0 0 1 RG\n3 w\n1 J\n1 j\n10 20 m\n50 60 l\n90 20 l\n10 80 m\n90 80 l\n\
                 40 40 m\n40 40 l\nS\n"
            )
        );
        assert_eq!(
            ink("/InkList [[10 20 50 60]] /BS << /W 2 /S /D >>").as_deref(),
            Some("0 0 0 RG\n2 w\n[3] 0 d\n1 J\n1 j\n10 20 m\n50 60 l\nS\n"),
            "black without /C, and dashed"
        );
        for nothing in [
            "/InkList [[10 20 50 60]] /C []",
            "/InkList [[10 20 50 60]] /BS << /W 0 >>",
            "/InkList []",
            "/InkList [[] [7]]",
            "/InkList [10 20 50 60]",
            "",
        ] {
            assert_eq!(ink(nothing), None, "{nothing} draws nothing");
        }
    }

    /// A one-page file whose interactive form's `/DR` holds `fonts`, with
    /// `objects` from object 4 on, all written as PDF text.
    fn form_doc(fonts: &str, objects: &[&str]) -> CosDocument {
        let mut bodies = vec![
            format!(
                "<< /Type /Catalog /Pages 2 0 R \
                 /AcroForm << /Fields [] /DR << /Font << {fonts} >> >> >> >>"
            ),
            "<< /Type /Pages /Count 1 /Kids [3 0 R] >>".to_owned(),
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] >>".to_owned(),
        ];
        bodies.extend(objects.iter().map(|o| (*o).to_owned()));
        let mut out = b"%PDF-1.7\n".to_vec();
        let mut offsets = Vec::new();
        for (index, body) in bodies.iter().enumerate() {
            offsets.push(out.len());
            out.extend_from_slice(format!("{} 0 obj\n{body}\nendobj\n", index + 1).as_bytes());
        }
        let xref = out.len();
        out.extend_from_slice(
            format!("xref\n0 {}\n0000000000 65535 f \n", bodies.len() + 1).as_bytes(),
        );
        for offset in offsets {
            out.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
        }
        out.extend_from_slice(
            format!(
                "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
                bodies.len() + 1
            )
            .as_bytes(),
        );
        CosDocument::open(out).expect("the form fixture opens")
    }

    const HELVETICA: &str = "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica \
                             /Encoding /WinAnsiEncoding >>";

    fn helvetica_form() -> CosDocument {
        form_doc("/Helv 4 0 R", &[HELVETICA])
    }

    /// `/DA`'s font, size and colour, and nothing else from it.
    #[test]
    fn a_default_appearance_gives_a_font_a_size_and_a_colour() {
        let read = |da: &[u8]| default_appearance(da).map(|d| (d.font, d.size, d.color));
        assert_eq!(
            read(b"/Helv 9 Tf 0 0 1 rg"),
            Some((b"Helv".to_vec(), 9.0, [0.0, 0.0, 1.0]))
        );
        assert_eq!(
            read(b"0.5 g /F1 0 Tf"),
            Some((b"F1".to_vec(), 0.0, [0.5; 3]))
        );
        assert_eq!(
            read(b"/F1 12 Tf 0 1 0 0 k"),
            Some((b"F1".to_vec(), 12.0, [1.0, 0.0, 1.0]))
        );
        assert_eq!(
            read(b"/F1 12 Tf /CS0 cs 1 scn"),
            Some((b"F1".to_vec(), 12.0, [0.0; 3])),
            "a colour this does not read is black"
        );
        assert_eq!(read(b"/F1 -3 Tf"), Some((b"F1".to_vec(), 0.0, [0.0; 3])));
        for none in [&b"0 g"[..], b"Tf", b"12 Tf", b"/F1 x Tf", b""] {
            assert!(default_appearance(none).is_none(), "{none:?} names no font");
        }
    }

    /// 12.5.6.6: the box bordered in the text's colour, and `/Contents`
    /// in the `/DA` font, two units in from the border, its first baseline
    /// 0.85 of the size below the top; the font is the appearance's own
    /// resource, by the reference `/DR` holds.
    #[test]
    fn a_free_text_is_written_in_its_da_font_inside_its_box() {
        let doc = helvetica_form();
        let annotation = parsed(
            &doc,
            "<< /Subtype /FreeText /Rect [10 10 110 60] /DA (/Helv 10 Tf 0 0 1 rg) \
             /Contents (Hello world) /BS << /W 1 >> >>",
        );
        let stream = synthesize(&doc, &annotation).expect("an appearance");
        assert_eq!(
            text_of(&stream),
            "0 0 1 RG\n1 w\n10.5 10.5 99 49 re\nS\n\
             q\n13 13 94 44 re\nW n\nBT\n/Helv 10 Tf\n0 0 1 rg\n11.5 TL\n\
             1 0 0 1 13 48.5 Tm\n(Hello world) Tj\nET\nQ\n"
        );
        let font = stream
            .dict
            .get_dict(doc.intern(b"Resources"))
            .and_then(|r| r.get_dict(doc.intern(b"Font")))
            .and_then(|f| f.get_ref(doc.intern(b"Helv")));
        assert_eq!(font, Some(ObjRef { num: 4, gen: 0 }));
    }

    /// `/Q`, `/C`, `/RD`, wrapping and a `/DA` size of zero.
    #[test]
    fn a_free_text_is_aligned_filled_inset_wrapped_and_sized() {
        let doc = helvetica_form();
        let content = |rest: &str| {
            content_of(
                &doc,
                &format!(
                    "<< /Subtype /FreeText /DA (/Helv 10 Tf 0 g) /BS << /W 0 >> \
                     /Contents (Hello world) {rest} >>"
                ),
            )
            .expect("an appearance")
        };
        // "Hello world" is 4 945 thousandths of Helvetica, 49.45 at ten
        // points, in a box 96 wide two in from each side of /Rect.
        let wide = "/Rect [10 10 110 60]";
        assert!(content(&format!("{wide} /Q 1")).contains("1 0 0 1 35.275 49.5 Tm\n"));
        assert!(content(&format!("{wide} /Q 2")).contains("1 0 0 1 58.55 49.5 Tm\n"));
        // /C fills the box, inside /RD: left 10, top 5, right 20, bottom 0.
        let filled = content(&format!("{wide} /C [1 1 0] /RD [10 5 20 0]"));
        assert!(
            filled.starts_with("1 1 0 rg\n20 10 70 45 re\nf\n"),
            "{filled}"
        );
        assert!(filled.contains("1 0 0 1 22 44.5 Tm\n"), "{filled}");
        // Too narrow for both words: "world" on a line of its own, the
        // leading of 11.5 below the first; set right, it starts 1.11 left
        // of where "Hello" did, being 1.11 wider.
        let narrow = content("/Rect [10 10 50 60]");
        assert!(
            narrow.contains("11.5 TL\n1 0 0 1 12 49.5 Tm\n(Hello) Tj\nT*\n(world) Tj\n"),
            "{narrow}"
        );
        let right = content("/Rect [10 10 50 60] /Q 2");
        assert!(
            right.contains("1 0 0 1 25.22 49.5 Tm\n(Hello) Tj\n-1.11 0 Td\nT*\n(world) Tj\n"),
            "{right}"
        );
        // A line wholly below the box is not written, nor any after it:
        // in a box sixteen high the third baseline is at -3.5.
        let short = content_of(
            &doc,
            "<< /Subtype /FreeText /DA (/Helv 10 Tf 0 g) /BS << /W 0 >> \
             /Contents (Hello world Hello world) /Rect [10 10 50 30] >>",
        )
        .expect("an appearance");
        assert_eq!(short.matches(") Tj\n").count(), 2, "{short}");
        // A size of zero is the largest of twelve down to four that fits.
        let sized = |rect: &str| {
            content_of(
                &doc,
                &format!(
                    "<< /Subtype /FreeText /DA (/Helv 0 Tf 0 g) /BS << /W 0 >> \
                     /Contents (Hello) /Rect {rect} >>"
                ),
            )
            .expect("an appearance")
        };
        assert!(sized("[10 10 110 60]").contains("/Helv 12 Tf\n"));
        assert!(sized("[10 10 110 23]").contains("/Helv 8 Tf\n"));
    }

    /// The callout of Table 174, drawn only under `/IT /FreeTextCallout`,
    /// from the box out to the point it calls, with `/LE` there.
    #[test]
    fn a_free_text_callout_is_drawn_only_for_that_intent() {
        let doc = helvetica_form();
        let content = |rest: &str| {
            content_of(
                &doc,
                &format!(
                    "<< /Subtype /FreeText /Rect [0 0 100 100] /RD [50 0 0 50] \
                     /DA (/Helv 10 Tf 1 0 0 rg) /CL [10 10 10 40 50 70] /LE /Butt {rest} >>"
                ),
            )
            .expect("an appearance")
        };
        let callout = "S\n10 10 m\n10 40 l\n50 70 l\nS\n13 10 m\n7 10 l\nS\n";
        assert!(content("/IT /FreeTextCallout").contains(callout));
        assert!(!content("").contains("10 10 m"), "no intent, no callout");
        assert!(!content("/IT /FreeTextTypeWriter").contains("10 10 m"));
    }

    /// No appearance at all, rather than a wrong one: a font `/DR` does not
    /// hold, or holds as composite, a `/DA` with no font, and a character a
    /// byte cannot address.
    #[test]
    fn a_free_text_without_a_font_to_write_it_in_is_refused() {
        let form = form_doc(
            "/Helv 4 0 R /Cmp 5 0 R",
            &[
                HELVETICA,
                "<< /Type /Font /Subtype /Type0 /BaseFont /Cmp /Encoding /Identity-H \
                 /DescendantFonts [] >>",
            ],
        );
        let content = |rest: &str| {
            content_of(
                &form,
                &format!("<< /Subtype /FreeText /Rect [10 10 110 60] /C [1 1 0] {rest} >>"),
            )
        };
        assert!(content("/DA (/Helv 10 Tf 0 g) /Contents (fine)").is_some());
        for refused in [
            "/DA (/Times 10 Tf 0 g) /Contents (fine)",
            "/DA (/Cmp 10 Tf 0 g) /Contents (fine)",
            "/DA (0 g) /Contents (fine)",
            "/Contents (fine)",
            "/DA (/Helv 10 Tf 0 g) /Contents <FEFF0100>",
        ] {
            assert_eq!(content(refused), None, "{refused}");
        }
        assert!(
            content_of(
                &doc(),
                "<< /Subtype /FreeText /Rect [10 10 110 60] /DA (/Helv 10 Tf 0 g) >>"
            )
            .is_none(),
            "a file with no form has no /DR"
        );
    }

    /// One dictionary carrying every geometric entry 12.5.6 reads, for the
    /// subtype given: whatever a subtype could draw from, it has.
    fn everything(subtype: &str) -> String {
        format!(
            "<< /Subtype /{subtype} /Rect [10 10 90 90] /C [1 0 0] /IC [0 0 1] \
             /BS << /W 2 >> /QuadPoints [10 60 90 60 10 40 90 40] /L [10 50 90 50] \
             /Vertices [20 20 80 20 50 80] /InkList [[10 20 50 60]] /RD [1 1 1 1] \
             /DA (/Helv 10 Tf 0 g) /Contents (text) /Name /Draft >>"
        )
    }

    /// 12.5.6's subtypes whose appearance no dictionary determines are
    /// declined by name, even handed every entry the others draw from.
    #[test]
    fn the_subtypes_no_dictionary_determines_are_declined_by_name() {
        let doc = helvetica_form();
        for subtype in UNDETERMINED_SUBTYPES {
            assert_eq!(
                content_of(&doc, &everything(subtype)),
                None,
                "/{subtype} is declined"
            );
        }
    }

    /// Every subtype ISO 32000-1 12.5.6 and ISO 32000-2 name is either drawn
    /// from the dictionary above, declined by name, or `Link`, which draws
    /// no border by 12.5.6.5's convention — so a subtype added to neither
    /// list is a test failure rather than a silent `None`.
    #[test]
    fn every_subtype_of_12_5_6_is_drawn_or_declined_by_name() {
        let doc = helvetica_form();
        const ALL: &[&str] = &[
            "Text",
            "Link",
            "FreeText",
            "Line",
            "Square",
            "Circle",
            "Polygon",
            "PolyLine",
            "Highlight",
            "Underline",
            "Squiggly",
            "StrikeOut",
            "Caret",
            "Stamp",
            "Ink",
            "Popup",
            "FileAttachment",
            "Sound",
            "Movie",
            "Screen",
            "Widget",
            "PrinterMark",
            "TrapNet",
            "Watermark",
            "3D",
            "Redact",
            "Projection",
            "RichMedia",
        ];
        let mut drawn = 0;
        for subtype in ALL {
            let declined = UNDETERMINED_SUBTYPES.contains(subtype) || *subtype == "Link";
            let content = content_of(&doc, &everything(subtype));
            assert_eq!(
                content.is_none(),
                declined,
                "/{subtype} is {}",
                if declined { "declined" } else { "drawn" }
            );
            drawn += usize::from(!declined);
        }
        assert_eq!(
            drawn, 13,
            "thirteen subtypes are drawn, and Link draws none"
        );
        assert_eq!(ALL.len(), drawn + 1 + UNDETERMINED_SUBTYPES.len());
    }

    /// Ruling 1, over every subtype this draws and a few it does not: no
    /// numbers a dictionary can hold — none, too few, too many, negative,
    /// enormous, infinite, not a number — make synthesis panic, and what it
    /// writes holds no number a tokenizer would refuse and stays small when
    /// the dictionary is.
    #[test]
    fn synthesis_never_panics_whatever_the_numbers() {
        use proptest::prelude::*;
        use proptest::test_runner::{Config, TestRunner};

        let docs = [doc(), helvetica_form()];
        const SUBTYPES: &[&[u8]] = &[
            b"Highlight",
            b"Underline",
            b"StrikeOut",
            b"Squiggly",
            b"Square",
            b"Circle",
            b"Text",
            b"Link",
            b"Line",
            b"Polygon",
            b"PolyLine",
            b"Caret",
            b"Ink",
            b"FreeText",
            b"Stamp",
            b"Trapezium",
        ];
        let number = prop_oneof![
            4 => -200.0..200.0f64,
            1 => Just(0.0),
            1 => Just(-0.0),
            1 => Just(1e300),
            1 => Just(-1e300),
            1 => Just(1e-300),
            1 => Just(f64::NAN),
            1 => Just(f64::INFINITY),
            1 => Just(f64::NEG_INFINITY),
        ];
        let strategy = (
            0..SUBTYPES.len(),
            proptest::collection::vec(number, 0..24),
            0usize..10,
            any::<bool>(),
        );
        let mut runner = TestRunner::new(Config {
            cases: 768,
            failure_persistence: None,
            ..Config::default()
        });
        let result = runner.run(&strategy, |(which, values, take, form)| {
            let doc = &docs[usize::from(form)];
            let name = |n: &[u8]| Object::Name(doc.intern(n));
            let real = |i: usize| Object::Real(values.get(i).copied().unwrap_or(1.0));
            let array = |from: usize, len: usize| {
                Object::Array(
                    values
                        .iter()
                        .skip(from)
                        .take(len)
                        .map(|v| Object::Real(*v))
                        .collect(),
                )
            };
            let mut style = Dict::new();
            style.insert(doc.intern(b"W"), real(5));
            style.insert(doc.intern(b"S"), name(b"D"));
            style.insert(doc.intern(b"D"), array(6, take));
            let mut dict = Dict::new();
            dict.insert(doc.intern(b"Subtype"), name(SUBTYPES[which]));
            dict.insert(doc.intern(b"Rect"), array(0, 4));
            dict.insert(doc.intern(b"QuadPoints"), array(0, values.len()));
            dict.insert(doc.intern(b"Vertices"), array(1, values.len()));
            dict.insert(doc.intern(b"L"), array(2, 4));
            dict.insert(doc.intern(b"CL"), array(0, take));
            dict.insert(doc.intern(b"RD"), array(3, 4));
            dict.insert(doc.intern(b"C"), array(0, take.min(5)));
            dict.insert(doc.intern(b"IC"), array(1, take.min(5)));
            dict.insert(
                doc.intern(b"InkList"),
                Object::Array(vec![array(0, take), array(take, values.len())]),
            );
            dict.insert(doc.intern(b"BS"), Object::Dict(style));
            for (key, index) in [
                (&b"LL"[..], 7),
                (b"LLE", 8),
                (b"LLO", 9),
                (b"CA", 10),
                (b"ca", 11),
            ] {
                dict.insert(doc.intern(key), real(index));
            }
            dict.insert(
                doc.intern(b"LE"),
                Object::Array(vec![name(b"ClosedArrow"), name(b"Slash")]),
            );
            dict.insert(doc.intern(b"IT"), name(b"FreeTextCallout"));
            dict.insert(doc.intern(b"Q"), Object::Int(take as i64 - 3));
            let size = values.get(12).copied().unwrap_or(10.0);
            let da = format!("/Helv {size:?} Tf {size:?} {size:?} 0 rg");
            dict.insert(
                doc.intern(b"DA"),
                Object::String(crate::object::PdfString::literal(da.into_bytes())),
            );
            dict.insert(
                doc.intern(b"Contents"),
                Object::String(crate::object::PdfString::literal(
                    b"MMM MMM\nMM M MMMMMMMMMMMM".to_vec(),
                )),
            );

            if let Some(stream) = synthesize(doc, &dict) {
                let text = String::from_utf8_lossy(&stream.data);
                prop_assert!(!text.contains("NaN") && !text.contains("inf"), "{text}");
                prop_assert!(stream.data.len() < 64 << 10, "{} bytes", stream.data.len());
            }
            Ok(())
        });
        if let Err(failure) = result {
            panic!("{failure}");
        }
    }
}
