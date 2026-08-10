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

use crate::doc::CosDocument;
use crate::name::Name;
use crate::object::{Dict, Object};
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
        _ => return None,
    }

    if content.is_empty() {
        return None;
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
    if needs_multiply {
        let mut state = Dict::new();
        state.insert(Name::TYPE, Object::Name(doc.intern(b"ExtGState")));
        state.insert(doc.intern(b"BM"), Object::Name(doc.intern(b"Multiply")));
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
}
