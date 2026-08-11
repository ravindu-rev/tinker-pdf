//! The page tree (7.7.3).
//!
//! One walk builds the index: page references in document order, each with its
//! inherited attributes already resolved. Doing it once means the geometry of
//! a thousand-page document costs one traversal, and a page's `/Resources`
//! never has to be chased up the tree again at render time.

use crate::doc::CosDocument;
use crate::limits;
use crate::name::Name;
use crate::object::{Dict, ObjRef, Object};
use crate::warn::WarningKind;
use std::collections::HashSet;

/// A rectangle in PDF units, normalized so the corners are ordered.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    /// Left edge.
    pub x0: f64,
    /// Bottom edge.
    pub y0: f64,
    /// Right edge.
    pub x1: f64,
    /// Top edge.
    pub y1: f64,
}

impl Rect {
    /// US Letter, the fallback when `/MediaBox` is absent or unusable.
    ///
    /// 7.7.3.3 makes `/MediaBox` required, so reaching this means the document
    /// is malformed; every surviving reader guesses rather than refuses.
    pub const US_LETTER: Rect = Rect {
        x0: 0.0,
        y0: 0.0,
        x1: 612.0,
        y1: 792.0,
    };

    /// Builds a rectangle from a `/MediaBox`-style array, ordering the corners.
    #[must_use]
    pub fn from_array(values: &[Object]) -> Option<Rect> {
        let n = |i: usize| values.get(i).and_then(Object::as_number);
        let (a, b, c, d) = (n(0)?, n(1)?, n(2)?, n(3)?);
        if ![a, b, c, d].iter().all(|v| v.is_finite()) {
            return None;
        }
        Some(Rect {
            x0: a.min(c),
            y0: b.min(d),
            x1: a.max(c),
            y1: b.max(d),
        })
    }

    /// Width in PDF units.
    #[must_use]
    pub fn width(&self) -> f64 {
        self.x1 - self.x0
    }

    /// Height in PDF units.
    #[must_use]
    pub fn height(&self) -> f64 {
        self.y1 - self.y0
    }

    /// Whether the rectangle encloses any area.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.width() <= 0.0 || self.height() <= 0.0
    }

    /// Whether every edge is a real number.
    ///
    /// A rectangle read from a file can hold an infinity or a NaN, and any of
    /// those written back into a content stream produces bytes no reader can
    /// lex — `NaN` is not a number to a PDF parser, it is a syntax error that
    /// throws away the rest of the stream.
    #[must_use]
    pub fn is_finite(&self) -> bool {
        self.x0.is_finite() && self.y0.is_finite() && self.x1.is_finite() && self.y1.is_finite()
    }

    /// The overlap of two rectangles, or `None` when they do not meet.
    #[must_use]
    pub fn intersect(&self, other: &Rect) -> Option<Rect> {
        let r = Rect {
            x0: self.x0.max(other.x0),
            y0: self.y0.max(other.y0),
            x1: self.x1.min(other.x1),
            y1: self.y1.min(other.y1),
        };
        (!r.is_empty()).then_some(r)
    }
}

/// One page, with its inherited attributes resolved.
#[derive(Clone, Debug)]
pub struct Page {
    /// The page's own indirect reference.
    pub reference: ObjRef,
    /// Zero-based position in the document.
    pub index: u32,
    /// `/MediaBox`, inherited if the page does not carry one.
    pub media_box: Rect,
    /// `/CropBox` clipped to `/MediaBox`; equal to it when absent (7.7.3.3).
    pub crop_box: Rect,
    /// `/Rotate` normalized to 0, 90, 180 or 270.
    pub rotation: u16,
    /// `/Resources`, inherited and already resolved.
    ///
    /// The dictionary itself rather than a reference to it, because 7.7.3.3
    /// allows it to be written inline and plenty of producers do — including
    /// this crate's own builder. Holding a reference here silently lost every
    /// direct one, which cost a page all of its fonts.
    pub resources: Option<Dict>,
}

impl Page {
    /// The page's size after rotation, which is what a viewer lays out.
    #[must_use]
    pub fn display_size(&self) -> (f64, f64) {
        let (w, h) = (self.crop_box.width(), self.crop_box.height());
        if self.rotation == 90 || self.rotation == 270 {
            (h, w)
        } else {
            (w, h)
        }
    }
}

/// Attributes that descend the tree (7.7.3.4).
#[derive(Clone, Debug, Default)]
struct Inherited {
    media_box: Option<Rect>,
    crop_box: Option<Rect>,
    rotation: Option<i64>,
    resources: Option<Dict>,
}

impl Inherited {
    /// Overlays whatever this node defines onto what it inherited.
    fn extend(self, dict: &Dict, doc: &CosDocument) -> Inherited {
        let rect = |key: Name| {
            let value = doc.resolve_key(dict, key);
            value.as_array().and_then(Rect::from_array)
        };
        // Resolved through the reference when there is one, taken as it
        // stands when the dictionary is written inline.
        let resources = doc
            .resolve_key(dict, Name::RESOURCES)
            .as_dict()
            .cloned()
            .or(self.resources);

        Inherited {
            media_box: rect(Name::MEDIA_BOX).or(self.media_box),
            crop_box: rect(doc.crop_box_name()).or(self.crop_box),
            rotation: dict.get_int(doc.rotate_name()).or(self.rotation),
            resources,
        }
    }
}

/// Every page in document order.
///
/// A cyclic or absurdly deep `/Kids` graph is truncated with a warning rather
/// than followed: the visited set makes a cycle terminate, and the depth cap
/// makes a very deep legitimate tree terminate too.
#[must_use]
pub fn collect(doc: &CosDocument) -> Vec<Page> {
    let mut pages = Vec::new();
    let mut visited = HashSet::new();

    let Some(root) = doc.catalog() else {
        return pages;
    };
    let Some(tree) = root.get_ref(Name::PAGES) else {
        return pages;
    };

    walk(doc, tree, Inherited::default(), 0, &mut visited, &mut pages);
    pages
}

fn walk(
    doc: &CosDocument,
    node: ObjRef,
    inherited: Inherited,
    depth: u32,
    visited: &mut HashSet<u32>,
    out: &mut Vec<Page>,
) {
    if depth > limits::MAX_NEST_DEPTH || out.len() >= limits::MAX_PAGES {
        doc.warn(WarningKind::PageTreeTruncated);
        return;
    }
    if !visited.insert(node.num) {
        // 7.7.3.2: the tree is a tree. A repeated node is a cycle or a
        // shared subtree, and following either duplicates pages forever.
        doc.warn(WarningKind::PageTreeCycle);
        return;
    }

    let Ok(object) = doc.get(node) else {
        return;
    };
    let Some(dict) = object.as_dict() else {
        return;
    };

    let inherited = inherited.extend(dict, doc);

    match dict.get_array(Name::KIDS) {
        Some(kids) => {
            let kids: Vec<ObjRef> = kids.iter().filter_map(Object::as_objref).collect();
            for kid in kids {
                walk(doc, kid, inherited.clone(), depth + 1, visited, out);
            }
        }
        // 7.7.3.3: a node without /Kids is a leaf, whatever its /Type says —
        // and plenty of files say nothing at all.
        None => out.push(leaf(node, out.len() as u32, inherited, doc)),
    }

    visited.remove(&node.num);
}

fn leaf(reference: ObjRef, index: u32, inherited: Inherited, doc: &CosDocument) -> Page {
    let media_box = match inherited.media_box {
        Some(r) if !r.is_empty() => r,
        _ => {
            doc.warn(WarningKind::MediaBoxMissing);
            Rect::US_LETTER
        }
    };

    // 7.7.3.3: /CropBox defaults to /MediaBox and is clipped to it. A crop box
    // that misses the page entirely is meaningless, so fall back.
    let crop_box = inherited
        .crop_box
        .and_then(|c| c.intersect(&media_box))
        .unwrap_or(media_box);

    Page {
        reference,
        index,
        media_box,
        crop_box,
        rotation: normalize_rotation(inherited.rotation.unwrap_or(0)),
        resources: inherited.resources,
    }
}

/// 7.7.3.3: `/Rotate` is a multiple of 90, which files routinely violate.
///
/// Values are taken modulo 360 in the positive direction, so -90 is 270 and
/// 450 is 90; anything not a multiple of 90 rounds to the nearest one.
#[must_use]
pub fn normalize_rotation(degrees: i64) -> u16 {
    let wrapped = degrees.rem_euclid(360);
    let quarter = ((wrapped as f64) / 90.0).round() as i64 % 4;
    (quarter * 90) as u16
}

/// The number of pages, from `/Count` when it is honest and by walking when it
/// is not.
///
/// `/Count` is a claim like any other in a PDF; a document that lies about it
/// gets counted the slow way rather than believed.
#[must_use]
pub fn count(doc: &CosDocument) -> u32 {
    let claimed = doc
        .catalog()
        .and_then(|root| root.get_ref(Name::PAGES))
        .and_then(|tree| doc.get(tree).ok())
        .and_then(|node| node.as_dict().and_then(|d| d.get_int(Name::COUNT)));

    let actual = collect(doc).len() as u32;
    match claimed {
        Some(c) if c >= 0 && c as u64 == u64::from(actual) => actual,
        Some(_) => {
            doc.warn(WarningKind::PageCountMismatch);
            actual
        }
        None => actual,
    }
}

/// Convenience: the page at `index`, if it exists.
#[must_use]
pub fn at(doc: &CosDocument, index: u32) -> Option<Page> {
    collect(doc).into_iter().nth(index as usize)
}

/// The `/Contents` streams of a page, in order.
///
/// `/Contents` is one stream or an array of them, and the array's parts are
/// concatenated with whitespace between (7.7.3.3) — so a caller wanting the
/// content must join them, not pick one.
#[must_use]
pub fn contents(doc: &CosDocument, page: &Page) -> Vec<ObjRef> {
    let Ok(object) = doc.get(page.reference) else {
        return Vec::new();
    };
    let Some(dict) = object.as_dict() else {
        return Vec::new();
    };

    if let Some(single) = dict.get_ref(Name::CONTENTS) {
        return vec![single];
    }
    let value = doc.resolve_key(dict, Name::CONTENTS);
    value
        .as_array()
        .map(|a| a.iter().filter_map(Object::as_objref).collect())
        .unwrap_or_default()
}

/// Every page's content streams decoded and joined, ready for the interpreter.
#[must_use]
pub fn content_bytes(doc: &CosDocument, page: &Page) -> Vec<u8> {
    let mut out = Vec::new();
    for part in contents(doc, page) {
        if let Ok(bytes) = doc.stream_decoded(part) {
            out.extend_from_slice(&bytes);
            // 7.7.3.3: the parts divide at lexical boundaries only if
            // separated, and a producer may end one mid-token.
            out.push(b'\n');
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rotation_normalizes_every_way_a_file_can_write_it() {
        assert_eq!(normalize_rotation(0), 0);
        assert_eq!(normalize_rotation(90), 90);
        assert_eq!(normalize_rotation(450), 90, "more than one turn");
        assert_eq!(normalize_rotation(-90), 270, "negative angles wrap");
        assert_eq!(normalize_rotation(-450), 270);
        assert_eq!(normalize_rotation(360), 0);
        assert_eq!(normalize_rotation(100), 90, "off-quarter angles round");
        assert_eq!(normalize_rotation(i64::MIN), 0, "no overflow");
        assert_eq!(normalize_rotation(i64::MAX), 0);
    }

    #[test]
    fn rectangles_order_their_corners() {
        let values = [
            Object::Int(100),
            Object::Int(200),
            Object::Int(0),
            Object::Int(0),
        ];
        let r = Rect::from_array(&values).expect("a rectangle");
        assert_eq!((r.x0, r.y0, r.x1, r.y1), (0.0, 0.0, 100.0, 200.0));
        assert_eq!((r.width(), r.height()), (100.0, 200.0));
    }

    #[test]
    fn a_rectangle_needs_four_finite_numbers() {
        assert!(Rect::from_array(&[Object::Int(1), Object::Int(2)]).is_none());
        assert!(Rect::from_array(&[
            Object::Real(f64::NAN),
            Object::Int(0),
            Object::Int(1),
            Object::Int(1)
        ])
        .is_none());
    }

    #[test]
    fn intersection_of_disjoint_rectangles_is_nothing() {
        let a = Rect {
            x0: 0.0,
            y0: 0.0,
            x1: 10.0,
            y1: 10.0,
        };
        let b = Rect {
            x0: 20.0,
            y0: 20.0,
            x1: 30.0,
            y1: 30.0,
        };
        assert!(a.intersect(&b).is_none());
        assert_eq!(a.intersect(&a), Some(a));
    }

    #[test]
    fn display_size_swaps_on_quarter_turns() {
        let page = Page {
            reference: ObjRef::new(1, 0),
            index: 0,
            media_box: Rect::US_LETTER,
            crop_box: Rect::US_LETTER,
            rotation: 0,
            resources: None,
        };
        assert_eq!(page.display_size(), (612.0, 792.0));
        let rotated = Page {
            rotation: 90,
            ..page.clone()
        };
        assert_eq!(rotated.display_size(), (792.0, 612.0));
        let upside_down = Page {
            rotation: 180,
            ..page
        };
        assert_eq!(upside_down.display_size(), (612.0, 792.0));
    }

    /// 7.7.3.3 allows `/Resources` to be an inline dictionary, and producers
    /// use it — this crate's own builder among them.
    ///
    /// This was once typed as a reference, so a direct dictionary resolved to
    /// nothing and the page lost every font it had. Nothing failed loudly:
    /// text extraction simply returned an empty string.
    #[test]
    fn a_direct_resource_dictionary_is_read() {
        let bytes = b"%PDF-1.7\n\
1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
2 0 obj\n<< /Type /Pages /Count 1 /Kids [3 0 R] >>\nendobj\n\
3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 100]\n\
   /Resources << /Font << /F0 4 0 R >> >> >>\nendobj\n\
4 0 obj\n<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>\nendobj\n\
trailer\n<< /Size 5 /Root 1 0 R >>\n%%EOF\n";

        let doc = CosDocument::open(&bytes[..]).expect("it opens");
        let pages = collect(&doc);
        assert_eq!(pages.len(), 1);

        let resources = pages[0]
            .resources
            .as_ref()
            .expect("the inline dictionary is read");
        assert!(
            resources.get(doc.intern(b"Font")).is_some(),
            "and its /Font survives"
        );
    }

    /// The indirect form still works, and still inherits down the tree.
    #[test]
    fn an_inherited_indirect_resource_dictionary_is_read() {
        let bytes = b"%PDF-1.7\n\
1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
2 0 obj\n<< /Type /Pages /Count 1 /Kids [3 0 R] /Resources 4 0 R >>\nendobj\n\
3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 100] >>\nendobj\n\
4 0 obj\n<< /Font << /F0 5 0 R >> >>\nendobj\n\
5 0 obj\n<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>\nendobj\n\
trailer\n<< /Size 6 /Root 1 0 R >>\n%%EOF\n";

        let doc = CosDocument::open(&bytes[..]).expect("it opens");
        let pages = collect(&doc);
        let resources = pages[0]
            .resources
            .as_ref()
            .expect("inherited from the parent node");
        assert!(resources.get(doc.intern(b"Font")).is_some());
    }
}
