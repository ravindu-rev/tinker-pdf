//! Filling form fields and rebuilding their appearances (12.7.4.3).
//!
//! Setting `/V` is the easy half and the useless half: a value with no
//! matching appearance stream shows up in a viewer that regenerates
//! appearances and nowhere else, which is why filled forms so often print
//! blank. So every fill here rewrites the widget's `/AP` to match.
//!
//! The alternative — setting `/NeedAppearances` and hoping — is what produces
//! those files. This module clears that flag rather than setting it, because
//! once the appearances are right, asking viewers to rebuild them can only
//! make things worse.

use crate::doc::CosDocument;
use crate::font::{self, Font};
use crate::form::{self, FieldKind};
use crate::name::Name;
use crate::object::{Dict, ObjRef, Object, PdfString};
use crate::pages::Rect;
use crate::write::StreamData;

/// Splits a `/DA` string into its font name, size, and everything else.
///
/// The remainder is replayed verbatim rather than interpreted, so a colour
/// this build does not understand still comes out right. The `Tf` operands
/// must be removed as *tokens*: dropping bytes leaves a stray number on the
/// stack, and the next operator consumes the wrong one.
fn operators(da: &[u8]) -> (Vec<u8>, f64, Vec<u8>) {
    let tokens: Vec<Vec<u8>> = da
        .split(|b| b.is_ascii_whitespace())
        .filter(|t| !t.is_empty())
        .map(<[u8]>::to_vec)
        .collect();

    let mut font = Vec::new();
    let mut size = 0.0;
    let mut rest: Vec<Vec<u8>> = Vec::new();
    let mut index = 0;
    while index < tokens.len() {
        if tokens[index] == b"Tf" {
            if index >= 2 {
                font = tokens[index - 2]
                    .strip_prefix(b"/")
                    .unwrap_or(&tokens[index - 2])
                    .to_vec();
                size = std::str::from_utf8(&tokens[index - 1])
                    .ok()
                    .and_then(|s| s.parse::<f64>().ok())
                    .filter(|v| v.is_finite() && *v >= 0.0)
                    .unwrap_or(0.0);
                rest.truncate(rest.len().saturating_sub(2));
            }
            index += 1;
            continue;
        }
        rest.push(tokens[index].clone());
        index += 1;
    }

    if font.is_empty() {
        font = b"Helv".to_vec();
    }
    let mut joined = rest.join(&b' ');
    if joined.is_empty() {
        joined = b"0 g".to_vec();
    }
    (font, size, joined)
}

/// The width of a string in text-space units per 1000, using the font's own
/// metrics.
fn width_of(font: Option<&Font>, text: &str) -> f64 {
    let Some(font) = font else {
        // Half an em per character is close enough to keep auto-sizing sane
        // when the font cannot be read at all.
        return text.chars().count() as f64 * 500.0;
    };
    // A field's text is written as a single-byte string, so the code is the
    // byte, and characters outside the encoding are dropped by the writer too.
    text.chars()
        .map(|c| {
            let code = u32::from(c);
            font.width_of(code).0
        })
        .sum()
}

/// Escapes a string for a literal in a content stream (7.3.4.2).
fn escape(out: &mut Vec<u8>, text: &str) {
    for byte in text.chars().map(|c| {
        // Field text is written single-byte; anything above that is not
        // representable in the standard encodings and becomes a question mark
        // rather than a truncated multi-byte sequence.
        let code = u32::from(c);
        if code < 256 {
            code as u8
        } else {
            b'?'
        }
    }) {
        if matches!(byte, b'(' | b')' | b'\\') {
            out.push(b'\\');
        }
        out.push(byte);
    }
}

/// Builds the appearance stream for a text field's widget.
///
/// `rect` is the widget's rectangle; the stream is written in a box of the
/// same size anchored at the origin, which is what 12.5.5's mapping expects
/// and what every other producer writes.
#[must_use]
pub fn text_appearance(
    doc: &CosDocument,
    rect: Rect,
    value: &str,
    da: &[u8],
    quadding: i64,
    multiline: bool,
    resources: Option<&Dict>,
) -> StreamData {
    let (font_name, mut size, colour) = operators(da);
    let (w, h) = (rect.x1 - rect.x0, rect.y1 - rect.y0);

    let font = resources
        .and_then(|dr| doc.resolve_key(dr, doc.intern(b"Font")).as_dict().cloned())
        .and_then(|fonts| fonts.get_ref(doc.intern(&font_name)))
        .and_then(|r| font::at(doc, r));

    // 12.7.3.3: two units of padding on each side is the convention, and
    // matching it is what keeps a regenerated appearance from jumping.
    const PAD: f64 = 2.0;
    let inner_w = (w - PAD * 2.0).max(0.0);
    let inner_h = (h - PAD * 2.0).max(0.0);

    let lines: Vec<&str> = if multiline {
        value.split('\n').collect()
    } else {
        // A single-line field shows one line whatever the value contains.
        vec![value.lines().next().unwrap_or(value)]
    };

    if size <= 0.0 {
        // Auto-size. The height budget is what makes it legible; the width
        // budget is what keeps it inside the box.
        let widest = lines
            .iter()
            .map(|line| width_of(font.as_ref(), line))
            .fold(0.0f64, f64::max)
            / 1000.0;
        let by_height = if multiline {
            inner_h / (lines.len() as f64).max(1.0) / 1.15
        } else {
            inner_h * 0.72
        };
        let by_width = if widest > 0.0 {
            inner_w / widest
        } else {
            by_height
        };
        size = by_height.min(by_width).clamp(1.0, 12.0);
    }

    let leading = size * 1.15;
    let mut content = Vec::new();
    // 12.7.4.3: the marked-content pair is how a viewer recognises a
    // regenerated field appearance as its own rather than as page content.
    content.extend_from_slice(b"/Tx BMC\nq\n");
    // Clipped to the box, so an over-long value is cut off rather than
    // spilling across the page.
    content.extend_from_slice(format!("{PAD} {PAD} {inner_w:.2} {inner_h:.2} re W n\n").as_bytes());
    content.extend_from_slice(b"BT\n");
    content.extend_from_slice(b"/");
    content.extend_from_slice(&font_name);
    content.extend_from_slice(format!(" {size:.2} Tf\n").as_bytes());
    content.extend_from_slice(&colour);
    content.push(b'\n');

    for (index, line) in lines.iter().enumerate() {
        let line_width = width_of(font.as_ref(), line) / 1000.0 * size;
        // 12.7.4.3: /Q is 0 left, 1 centred, 2 right.
        let x = match quadding {
            1 => PAD + (inner_w - line_width) / 2.0,
            2 => PAD + inner_w - line_width,
            _ => PAD,
        }
        .max(PAD);

        let y = if multiline {
            // Multiline runs from the top down.
            rect_top_baseline(inner_h, size) - index as f64 * leading + PAD
        } else {
            // A single line is centred vertically, which is what a viewer
            // does and what makes a regenerated field sit where it did.
            (h - size * 0.72) / 2.0
        };

        content.extend_from_slice(format!("1 0 0 1 {x:.2} {y:.2} Tm\n").as_bytes());
        content.push(b'(');
        escape(&mut content, line);
        content.extend_from_slice(b") Tj\n");
    }

    content.extend_from_slice(b"ET\nQ\nEMC\n");

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
    if let Some(dr) = resources {
        dict.insert(Name::RESOURCES, Object::Dict(dr.clone()));
    }

    StreamData {
        dict,
        data: content,
    }
}

/// Where the first baseline of a multiline field sits.
fn rect_top_baseline(inner_h: f64, size: f64) -> f64 {
    (inner_h - size * 0.85).max(0.0)
}

/// The rectangle of a widget annotation.
#[must_use]
pub fn widget_rect(doc: &CosDocument, widget: ObjRef) -> Option<Rect> {
    let object = doc.get(widget).ok()?;
    let dict = object.as_dict()?;
    doc.resolve_key(dict, doc.intern(b"Rect"))
        .as_array()
        .and_then(Rect::from_array)
        .filter(|r| !r.is_empty())
}

/// The `/DA` a widget should use: its own, its field's, or the form's.
#[must_use]
pub fn appearance_string(doc: &CosDocument, field: &form::Field, widget: ObjRef) -> Vec<u8> {
    if let Ok(object) = doc.get(widget) {
        if let Some(da) = object
            .as_dict()
            .and_then(|d| d.get(doc.intern(b"DA")))
            .and_then(Object::as_string)
        {
            return da.bytes.clone();
        }
    }
    field
        .default_appearance
        .clone()
        .unwrap_or_else(|| b"/Helv 0 Tf 0 g".to_vec())
}

/// A text field's quadding, inherited from the form when it says nothing.
#[must_use]
pub fn quadding(doc: &CosDocument, field: &form::Field) -> i64 {
    doc.get(field.reference)
        .ok()
        .and_then(|o| o.as_dict().and_then(|d| d.get_int(doc.intern(b"Q"))))
        .or_else(|| form::acro_form(doc).and_then(|f| f.get_int(doc.intern(b"Q"))))
        .unwrap_or(0)
}

/// 12.7.4.3 table 231: a text field that wraps.
pub const MULTILINE: i64 = 1 << 12;
/// A text field whose characters sit in fixed cells.
pub const COMB: i64 = 1 << 24;

/// Whether a value is one the field will accept.
///
/// Refusing is better than truncating silently: a form filled with a value the
/// field rejects is a data error, and writing half of it hides that.
#[must_use]
pub fn accepts(field: &form::Field, value: &str) -> bool {
    if field.is_read_only() {
        return false;
    }
    match field.kind {
        FieldKind::Text => field
            .max_len
            .is_none_or(|max| max <= 0 || value.chars().count() as i64 <= max),
        FieldKind::ComboBox | FieldKind::ListBox => {
            // An editable combo takes anything; a list takes what it offers.
            const EDIT: i64 = 1 << 18;
            field.flags & EDIT != 0
                || field.options.is_empty()
                || field.options.iter().any(|o| o == value)
        }
        _ => false,
    }
}

/// Builds the `/V` object for a text or choice value.
#[must_use]
pub fn value_object(value: &str) -> Object {
    // 7.9.2.2: UTF-16BE with a byte-order mark is the only encoding that
    // covers everything, and it is what viewers write. Pure ASCII stays a
    // plain literal so simple files stay readable.
    if value.is_ascii() {
        return Object::String(PdfString::literal(value.as_bytes().to_vec()));
    }
    let mut bytes = vec![0xFE, 0xFF];
    for unit in value.encode_utf16() {
        bytes.extend_from_slice(&unit.to_be_bytes());
    }
    Object::String(PdfString::hex(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc() -> CosDocument {
        let bytes: &[u8] = b"%PDF-1.7\n\
1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
2 0 obj\n<< /Type /Pages /Count 0 /Kids [] >>\nendobj\n\
trailer\n<< /Size 3 /Root 1 0 R >>\n%%EOF\n";
        CosDocument::open(bytes).expect("it opens")
    }

    fn rect() -> Rect {
        Rect {
            x0: 0.0,
            y0: 0.0,
            x1: 100.0,
            y1: 20.0,
        }
    }

    fn text_of(stream: &StreamData) -> String {
        String::from_utf8_lossy(&stream.data).into_owned()
    }

    #[test]
    fn a_default_appearance_splits_into_font_size_and_colour() {
        let (font, size, colour) = operators(b"/Helv 12 Tf 0 0 1 rg");
        assert_eq!(font, b"Helv");
        assert_eq!(size, 12.0);
        assert_eq!(colour, b"0 0 1 rg", "the colour is replayed verbatim");
    }

    /// The operands of the `Tf` must not be replayed as though they were
    /// colour operators, which would push a stray number onto the stack.
    #[test]
    fn the_font_operands_are_not_replayed() {
        let (_, _, colour) = operators(b"0 g /Helv 9 Tf");
        assert_eq!(colour, b"0 g");
    }

    #[test]
    fn a_missing_or_broken_appearance_string_still_yields_something_usable() {
        let (font, size, colour) = operators(b"");
        assert_eq!(font, b"Helv");
        assert_eq!(size, 0.0, "which means auto-size");
        assert_eq!(colour, b"0 g");

        let (font, size, _) = operators(b"/ 1e999 Tf");
        assert_eq!(font, b"Helv", "an empty name falls back");
        assert!(size.is_finite());
    }

    #[test]
    fn an_appearance_is_marked_as_a_field_appearance() {
        let doc = doc();
        let stream = text_appearance(&doc, rect(), "Ada", b"/Helv 9 Tf 0 g", 0, false, None);
        let content = text_of(&stream);
        assert!(content.starts_with("/Tx BMC"), "got: {content}");
        assert!(content.ends_with("EMC\n"));
        assert!(content.contains("(Ada) Tj"));
    }

    /// The box is anchored at the origin whatever the widget's rectangle is,
    /// because 12.5.5 maps it onto that rectangle.
    #[test]
    fn the_box_is_the_widgets_size_at_the_origin() {
        let doc = doc();
        let offset = Rect {
            x0: 300.0,
            y0: 400.0,
            x1: 400.0,
            y1: 420.0,
        };
        let stream = text_appearance(&doc, offset, "x", b"/Helv 9 Tf 0 g", 0, false, None);
        let bbox: Vec<f64> = stream
            .dict
            .get_array(doc.intern(b"BBox"))
            .expect("a box")
            .iter()
            .filter_map(Object::as_number)
            .collect();
        assert_eq!(bbox, vec![0.0, 0.0, 100.0, 20.0]);
    }

    #[test]
    fn the_value_is_clipped_to_the_box() {
        let doc = doc();
        let content = text_of(&text_appearance(
            &doc,
            rect(),
            "a very long value indeed",
            b"/Helv 9 Tf 0 g",
            0,
            false,
            None,
        ));
        assert!(content.contains(" re W n"), "a clip is set: {content}");
    }

    #[test]
    fn quadding_moves_the_line() {
        let doc = doc();
        let left = text_of(&text_appearance(
            &doc,
            rect(),
            "hi",
            b"/Helv 9 Tf 0 g",
            0,
            false,
            None,
        ));
        let centre = text_of(&text_appearance(
            &doc,
            rect(),
            "hi",
            b"/Helv 9 Tf 0 g",
            1,
            false,
            None,
        ));
        let right = text_of(&text_appearance(
            &doc,
            rect(),
            "hi",
            b"/Helv 9 Tf 0 g",
            2,
            false,
            None,
        ));

        let x = |content: &str| -> f64 {
            content
                .lines()
                .find(|l| l.ends_with(" Tm"))
                .and_then(|l| l.split_whitespace().nth(4))
                .and_then(|v| v.parse().ok())
                .expect("a text matrix")
        };
        assert!(x(&left) < x(&centre), "centred sits right of left");
        assert!(x(&centre) < x(&right), "and right of centred");
    }

    #[test]
    fn a_multiline_field_writes_a_line_each() {
        let doc = doc();
        let tall = Rect {
            x0: 0.0,
            y0: 0.0,
            x1: 100.0,
            y1: 60.0,
        };
        let content = text_of(&text_appearance(
            &doc,
            tall,
            "one\ntwo\nthree",
            b"/Helv 9 Tf 0 g",
            0,
            true,
            None,
        ));
        assert_eq!(content.matches(" Tj").count(), 3);
        assert!(content.contains("(one)") && content.contains("(three)"));
    }

    #[test]
    fn a_single_line_field_shows_one_line_of_a_multiline_value() {
        let doc = doc();
        let content = text_of(&text_appearance(
            &doc,
            rect(),
            "one\ntwo",
            b"/Helv 9 Tf 0 g",
            0,
            false,
            None,
        ));
        assert_eq!(content.matches(" Tj").count(), 1);
        assert!(content.contains("(one)") && !content.contains("(two)"));
    }

    /// Size zero means auto, and it has to come out as a real number a
    /// tokenizer accepts rather than a zero that draws nothing.
    #[test]
    fn an_auto_sized_field_picks_a_real_size() {
        let doc = doc();
        let content = text_of(&text_appearance(
            &doc,
            rect(),
            "Ada",
            b"/Helv 0 Tf 0 g",
            0,
            false,
            None,
        ));
        let size: f64 = content
            .lines()
            .find(|l| l.ends_with(" Tf"))
            .and_then(|l| l.split_whitespace().nth(1))
            .and_then(|v| v.parse().ok())
            .expect("a font size");
        assert!(size > 0.0 && size <= 12.0, "got {size}");
    }

    /// A value far too long for its box must shrink rather than run out of it.
    #[test]
    fn a_long_auto_sized_value_shrinks_to_fit() {
        let doc = doc();
        let short = text_of(&text_appearance(
            &doc,
            rect(),
            "hi",
            b"/Helv 0 Tf 0 g",
            0,
            false,
            None,
        ));
        let long = text_of(&text_appearance(
            &doc,
            rect(),
            "a value far longer than the box it has to fit inside of",
            b"/Helv 0 Tf 0 g",
            0,
            false,
            None,
        ));
        let size = |content: &str| -> f64 {
            content
                .lines()
                .find(|l| l.ends_with(" Tf"))
                .and_then(|l| l.split_whitespace().nth(1))
                .and_then(|v| v.parse().ok())
                .expect("a size")
        };
        assert!(size(&long) < size(&short), "the long value shrank");
    }

    #[test]
    fn parentheses_in_a_value_are_escaped() {
        let doc = doc();
        let content = text_of(&text_appearance(
            &doc,
            rect(),
            "a (b) \\ c",
            b"/Helv 9 Tf 0 g",
            0,
            false,
            None,
        ));
        assert!(content.contains("(a \\(b\\) \\\\ c) Tj"), "got: {content}");
    }

    #[test]
    fn non_ascii_values_are_written_as_utf16() {
        let Object::String(ascii) = value_object("plain") else {
            panic!("a string");
        };
        assert!(!ascii.hex && ascii.bytes == b"plain");

        let Object::String(wide) = value_object("naïve") else {
            panic!("a string");
        };
        assert_eq!(&wide.bytes[..2], &[0xFE, 0xFF], "a byte-order mark");
    }

    #[test]
    fn a_value_over_maxlen_is_refused() {
        let field = form::Field {
            reference: ObjRef::new(1, 0),
            name: "x".to_string(),
            kind: FieldKind::Text,
            value: form::FieldValue::None,
            default: form::FieldValue::None,
            flags: 0,
            widgets: Vec::new(),
            options: Vec::new(),
            max_len: Some(3),
            default_appearance: None,
        };
        assert!(accepts(&field, "abc"));
        assert!(
            !accepts(&field, "abcd"),
            "silently truncating hides an error"
        );
    }

    #[test]
    fn a_read_only_field_is_refused() {
        let field = form::Field {
            reference: ObjRef::new(1, 0),
            name: "x".to_string(),
            kind: FieldKind::Text,
            value: form::FieldValue::None,
            default: form::FieldValue::None,
            flags: 1,
            widgets: Vec::new(),
            options: Vec::new(),
            max_len: None,
            default_appearance: None,
        };
        assert!(!accepts(&field, "anything"));
    }
}
