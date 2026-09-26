//! Building the common annotation types (12.5.6).

use crate::doc::CosDocument;
use crate::name::Name;
use crate::object::{Dict, Object, PdfString};
use crate::pages::Rect;

/// A colour, as components from zero to one.
#[derive(Clone, Copy, Debug)]
pub struct Color {
    /// Red.
    pub r: f64,
    /// Green.
    pub g: f64,
    /// Blue.
    pub b: f64,
}

fn base(doc: &CosDocument, subtype: &[u8], rect: Rect) -> Dict {
    let mut dict = Dict::new();
    dict.insert(Name::TYPE, Object::Name(doc.intern(b"Annot")));
    dict.insert(doc.intern(b"Subtype"), Object::Name(doc.intern(subtype)));
    dict.insert(
        doc.intern(b"Rect"),
        Object::Array(vec![
            Object::Real(rect.x0),
            Object::Real(rect.y0),
            Object::Real(rect.x1),
            Object::Real(rect.y1),
        ]),
    );
    // 12.5.2: /F bit 3 is Print, which every annotation meant to appear
    // on paper must set. Viewers show unset ones on screen only, which is
    // a common and confusing omission.
    dict.insert(doc.intern(b"F"), Object::Int(4));
    dict
}

/// A text-highlight annotation over the given quads.
///
/// 12.5.6.10: `/QuadPoints` runs upper-left, upper-right, lower-left,
/// lower-right per quad — an order that is neither clockwise nor
/// counter-clockwise, and the usual source of highlights that appear
/// bow-tied.
#[must_use]
pub fn highlight(doc: &CosDocument, quads: &[[f64; 8]], color: Color) -> Dict {
    let bounds = quads.iter().fold(
        Rect {
            x0: f64::INFINITY,
            y0: f64::INFINITY,
            x1: f64::NEG_INFINITY,
            y1: f64::NEG_INFINITY,
        },
        |acc, q| Rect {
            x0: acc.x0.min(q[0]).min(q[4]),
            y0: acc.y0.min(q[5]).min(q[7]),
            x1: acc.x1.max(q[2]).max(q[6]),
            y1: acc.y1.max(q[1]).max(q[3]),
        },
    );

    let mut dict = base(doc, b"Highlight", bounds);
    let mut points = Vec::with_capacity(quads.len() * 8);
    for quad in quads {
        for value in quad {
            points.push(Object::Real(*value));
        }
    }
    dict.insert(doc.intern(b"QuadPoints"), Object::Array(points));
    dict.insert(
        doc.intern(b"C"),
        Object::Array(vec![
            Object::Real(color.r),
            Object::Real(color.g),
            Object::Real(color.b),
        ]),
    );
    dict
}

/// A square annotation.
#[must_use]
pub fn square(doc: &CosDocument, rect: Rect, color: Color, width: f64) -> Dict {
    let mut dict = base(doc, b"Square", rect);
    dict.insert(
        doc.intern(b"C"),
        Object::Array(vec![
            Object::Real(color.r),
            Object::Real(color.g),
            Object::Real(color.b),
        ]),
    );
    let mut border = Dict::new();
    border.insert(doc.intern(b"W"), Object::Real(width.max(0.0)));
    dict.insert(doc.intern(b"BS"), Object::Dict(border));
    dict
}

/// A sticky note.
#[must_use]
pub fn text_note(doc: &CosDocument, rect: Rect, contents: &str, open: bool) -> Dict {
    let mut dict = base(doc, b"Text", rect);
    dict.insert(
        doc.intern(b"Contents"),
        Object::String(PdfString::literal(contents.as_bytes().to_vec())),
    );
    dict.insert(doc.intern(b"Name"), Object::Name(doc.intern(b"Note")));
    dict.insert(doc.intern(b"Open"), Object::Bool(open));
    dict
}

/// A link to a page in the same document, fitted to the page.
///
/// The destination array and the border come from [`crate::dest`], which
/// is where [`crate::build::DocumentBuilder`] gets them too — one spelling
/// of Table 151 rather than two that can drift. This entry point states
/// `/Fit` because a caller editing an existing document has a page and no
/// layout; a caller *building* one has both, and says which view it wants
/// through [`crate::build::Target`].
#[must_use]
pub fn link(doc: &CosDocument, rect: Rect, page: crate::object::ObjRef) -> Dict {
    let mut dict = base(doc, b"Link", rect);
    // Ruling 6: an explicit destination, never a name that resembles one.
    dict.insert(
        doc.intern(b"Dest"),
        crate::dest::destination_array(doc.names_table(), page, &crate::dest::DestKind::Fit),
    );
    dict.insert(doc.intern(b"Border"), crate::dest::no_border());
    dict
}
