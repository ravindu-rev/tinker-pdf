//! Destinations and actions (12.3.2, 12.6).
//!
//! The type here is the design answer to a specific defect. MuPDF's outline
//! writer took a `uri` field and, given `#page=2`, stored it as a *named*
//! destination, percent-encoded to `#nameddest=%23page%3D2`, which resolves to
//! no page when the file is read back. The three kinds are different things
//! and this engine never conflates them, in either direction (ruling 6).

use crate::doc::CosDocument;
use crate::name::{Name, NameTable};
use crate::object::{Dict, ObjRef, Object, PdfString};
use crate::pages::{Page, Rect};

/// How a destination positions the page it names (12.3.2.2, Table 151).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum DestKind {
    /// `/XYZ left top zoom`; any component may be null, meaning "unchanged".
    Xyz {
        /// Left edge, or `None` for the current value.
        left: Option<f64>,
        /// Top edge, or `None` for the current value.
        top: Option<f64>,
        /// Magnification, or `None` for the current value. Zero also means it.
        zoom: Option<f64>,
    },
    /// `/Fit`: fit the whole page.
    Fit,
    /// `/FitH top`: fit the width.
    FitH {
        /// Vertical position of the top edge.
        top: Option<f64>,
    },
    /// `/FitV left`: fit the height.
    FitV {
        /// Horizontal position of the left edge.
        left: Option<f64>,
    },
    /// `/FitR left bottom right top`: fit a rectangle.
    FitR {
        /// Left edge.
        left: f64,
        /// Bottom edge.
        bottom: f64,
        /// Right edge.
        right: f64,
        /// Top edge.
        top: f64,
    },
    /// `/FitB`: fit the bounding box of the page's contents.
    FitB,
    /// `/FitBH top`: fit the bounding box's width.
    FitBH {
        /// Vertical position of the top edge.
        top: Option<f64>,
    },
    /// `/FitBV left`: fit the bounding box's height.
    FitBV {
        /// Horizontal position of the left edge.
        left: Option<f64>,
    },
}

/// Where a link or outline entry goes.
///
/// Three genuinely different things, kept apart: an explicit page and view, a
/// name to be looked up in the document's own tables, and a URI that leaves
/// the document altogether.
#[derive(Clone, Debug, PartialEq)]
pub enum Destination {
    /// A page in this document, with a view.
    Explicit {
        /// Zero-based page index, when the page reference could be resolved.
        page_index: Option<u32>,
        /// The page's own reference, kept whether or not it resolved.
        page_ref: Option<ObjRef>,
        /// How to position it.
        kind: DestKind,
    },
    /// A name to look up in `/Names /Dests` or the legacy `/Dests` dictionary.
    Named(Vec<u8>),
    /// A URI. Never a named destination, however much it may look like one.
    Uri(Vec<u8>),
}

/// What an annotation or outline entry does (12.6.4).
#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    /// `/GoTo`: a destination in this document.
    GoTo(Destination),
    /// `/GoToR`: a destination in another file.
    GoToR {
        /// The target file, as the file specification gave it.
        file: Option<Vec<u8>>,
        /// The destination within that file.
        dest: Option<Destination>,
    },
    /// `/URI`.
    Uri(Vec<u8>),
    /// `/Named`: a viewer command such as `NextPage`.
    Named(Vec<u8>),
    /// `/Launch`: **reported, never executed.** Running a program because a
    /// document asked is not a service this engine provides.
    Launch {
        /// The file the document wanted opened.
        file: Option<Vec<u8>>,
    },
    /// Any other action type, preserved rather than discarded so a caller can
    /// decide what to do with it.
    Other {
        /// The action's `/S` subtype.
        subtype: Vec<u8>,
    },
}

/// Reads destinations and actions in the context of one document.
pub struct Resolver<'d> {
    doc: &'d CosDocument,
    pages: Vec<Page>,
    xyz: Name,
    fit: Name,
    fit_h: Name,
    fit_v: Name,
    fit_r: Name,
    fit_b: Name,
    fit_bh: Name,
    fit_bv: Name,
    d: Name,
    s: Name,
    a: Name,
    dest: Name,
    uri: Name,
    n: Name,
    f: Name,
    go_to: Name,
    go_to_r: Name,
    uri_action: Name,
    named_action: Name,
    launch: Name,
    names: Name,
    dests: Name,
}

impl<'d> Resolver<'d> {
    /// Builds a resolver, interning the names it needs once.
    #[must_use]
    pub fn new(doc: &'d CosDocument) -> Resolver<'d> {
        let i = |b: &[u8]| doc.intern(b);
        Resolver {
            pages: crate::pages::collect(doc),
            xyz: i(b"XYZ"),
            fit: i(b"Fit"),
            fit_h: i(b"FitH"),
            fit_v: i(b"FitV"),
            fit_r: i(b"FitR"),
            fit_b: i(b"FitB"),
            fit_bh: i(b"FitBH"),
            fit_bv: i(b"FitBV"),
            d: i(b"D"),
            s: i(b"S"),
            a: i(b"A"),
            dest: i(b"Dest"),
            uri: i(b"URI"),
            n: i(b"N"),
            f: i(b"F"),
            go_to: i(b"GoTo"),
            go_to_r: i(b"GoToR"),
            uri_action: i(b"URI"),
            named_action: i(b"Named"),
            launch: i(b"Launch"),
            names: i(b"Names"),
            dests: i(b"Dests"),
            doc,
        }
    }

    /// The pages this resolver indexed, for callers that want them anyway.
    #[must_use]
    pub fn pages(&self) -> &[Page] {
        &self.pages
    }

    fn kind_of(&self, name: Name, values: &[Object]) -> Option<DestKind> {
        let at = |i: usize| values.get(i).and_then(Object::as_number);
        let opt = |i: usize| match values.get(i) {
            Some(Object::Null) | None => None,
            other => other.and_then(Object::as_number),
        };

        if name == self.xyz {
            // 12.3.2.2: a zoom of 0 means "unchanged", exactly as null does.
            let zoom = opt(4).filter(|z| *z != 0.0);
            Some(DestKind::Xyz {
                left: opt(2),
                top: opt(3),
                zoom,
            })
        } else if name == self.fit {
            Some(DestKind::Fit)
        } else if name == self.fit_h {
            Some(DestKind::FitH { top: opt(2) })
        } else if name == self.fit_v {
            Some(DestKind::FitV { left: opt(2) })
        } else if name == self.fit_r {
            Some(DestKind::FitR {
                left: at(2)?,
                bottom: at(3)?,
                right: at(4)?,
                top: at(5)?,
            })
        } else if name == self.fit_b {
            Some(DestKind::FitB)
        } else if name == self.fit_bh {
            Some(DestKind::FitBH { top: opt(2) })
        } else if name == self.fit_bv {
            Some(DestKind::FitBV { left: opt(2) })
        } else {
            // An unknown kind still names a page, and fitting it is a better
            // answer than refusing to navigate.
            Some(DestKind::Fit)
        }
    }

    /// Reads a `/Dest` value: an array, a name, or a string naming one.
    #[must_use]
    pub fn destination(&self, value: &Object) -> Option<Destination> {
        let resolved = self.doc.resolve(value);
        match resolved.as_ref() {
            Object::Array(values) => {
                let target = values.first()?;
                let (page_ref, page_index) = match target {
                    Object::Ref(r) => (
                        Some(*r),
                        self.pages
                            .iter()
                            .find(|p| p.reference == *r)
                            .map(|p| p.index),
                    ),
                    Object::Int(n) if *n >= 0 => (None, u32::try_from(*n).ok()),
                    _ => (None, None),
                };
                let kind_name = match values.get(1) {
                    Some(Object::Name(n)) => *n,
                    _ => self.fit,
                };
                Some(Destination::Explicit {
                    page_index,
                    page_ref,
                    kind: self.kind_of(kind_name, values)?,
                })
            }
            // 12.3.2.3: a name or a byte string names an entry in the
            // document's destination tables. It is NOT a URI.
            Object::Name(n) => self
                .doc
                .name_bytes(*n)
                .map(|b| Destination::Named(b.to_vec())),
            Object::String(s) => Some(Destination::Named(s.bytes.clone())),
            _ => None,
        }
    }

    /// Follows a named destination to the explicit one it stands for.
    ///
    /// Both spellings are consulted: the `/Names /Dests` name tree of 12.3.2.3
    /// and the older `/Dests` dictionary in the catalog.
    #[must_use]
    pub fn resolve_named(&self, name: &[u8]) -> Option<Destination> {
        let catalog = self.doc.catalog()?;

        // The name tree first: it is where PDF 1.2 and later put them.
        if let Some(names) = catalog.get_ref(self.names) {
            if let Ok(dict) = self.doc.get(names) {
                if let Some(tree) = dict.as_dict().and_then(|d| d.get_ref(self.dests)) {
                    if let Some(found) = crate::trees::lookup_name(self.doc, tree, name) {
                        return self.dest_from_entry(&found);
                    }
                }
            }
        }

        // Then the legacy dictionary, whose keys are names rather than strings.
        let legacy = catalog.get_ref(self.dests)?;
        let dict = self.doc.get(legacy).ok()?;
        let key = self.doc.intern(name);
        let entry = dict.as_dict()?.get(key)?.clone();
        self.dest_from_entry(&entry)
    }

    /// A destination table entry is either the array itself or a dictionary
    /// with the array under `/D` (12.3.2.3).
    fn dest_from_entry(&self, entry: &Object) -> Option<Destination> {
        let resolved = self.doc.resolve(entry);
        if let Some(dict) = resolved.as_dict() {
            let inner = dict.get(self.d)?.clone();
            return self.destination(&inner);
        }
        self.destination(&resolved)
    }

    /// Reads an action dictionary (12.6.2).
    #[must_use]
    pub fn action(&self, dict: &Dict) -> Option<Action> {
        let subtype = match dict.get(self.s) {
            Some(Object::Name(n)) => *n,
            _ => return None,
        };

        if subtype == self.go_to {
            let value = dict.get(self.d)?.clone();
            Some(Action::GoTo(self.destination(&value)?))
        } else if subtype == self.go_to_r {
            Some(Action::GoToR {
                file: self.file_spec(dict),
                dest: dict.get(self.d).and_then(|v| self.destination(v)),
            })
        } else if subtype == self.uri_action {
            // 12.6.4.7: the URI is a byte string, and it stays one.
            let value = self.doc.resolve_key(dict, self.uri);
            value
                .as_string()
                .map(|s| Action::Uri(s.bytes.clone()))
                .or(Some(Action::Uri(Vec::new())))
        } else if subtype == self.named_action {
            match dict.get(self.n) {
                Some(Object::Name(n)) => self
                    .doc
                    .name_bytes(*n)
                    .map(|b| Action::Named(b.to_vec()))
                    .or(Some(Action::Named(Vec::new()))),
                _ => Some(Action::Named(Vec::new())),
            }
        } else if subtype == self.launch {
            Some(Action::Launch {
                file: self.file_spec(dict),
            })
        } else {
            self.doc
                .name_bytes(subtype)
                .map(|b| Action::Other {
                    subtype: b.to_vec(),
                })
                .or(Some(Action::Other {
                    subtype: Vec::new(),
                }))
        }
    }

    /// 7.11.3: a file specification is a string, or a dictionary with the
    /// string under `/F` (and platform-specific variants this ignores).
    fn file_spec(&self, dict: &Dict) -> Option<Vec<u8>> {
        let value = self.doc.resolve_key(dict, self.f);
        if let Some(s) = value.as_string() {
            return Some(s.bytes.clone());
        }
        let inner = value.as_dict()?;
        let f = self.doc.resolve_key(inner, self.f);
        f.as_string().map(|s| s.bytes.clone())
    }

    /// The destination of an entry that may carry either `/Dest` or `/A`
    /// (12.3.3 for outlines, 12.5.6.5 for links).
    ///
    /// `/Dest` wins where both exist, which is what viewers do.
    #[must_use]
    pub fn entry_target(&self, dict: &Dict) -> Option<Destination> {
        if let Some(value) = dict.get(self.dest) {
            return self.destination(value);
        }

        let action = self.doc.resolve_key(dict, self.a);
        let action_dict = action.as_dict()?;
        match self.action(action_dict)? {
            Action::GoTo(d) => Some(d),
            // A URI action is a URI destination — never rewritten into a name.
            Action::Uri(u) => Some(Destination::Uri(u)),
            _ => None,
        }
    }
}

/// One link annotation on a page (12.5.6.5).
#[derive(Clone, Debug, PartialEq)]
pub struct Link {
    /// The annotation object, when `/Annots` named it indirectly.
    pub reference: Option<ObjRef>,
    /// `/Rect`, with its corners ordered.
    pub rect: Rect,
    /// Where the link goes.
    ///
    /// `None` for a `/Link` that carries neither `/Dest` nor a usable `/A`,
    /// which is legal, useless, and worth reporting rather than hiding.
    pub target: Option<Action>,
}

/// The link annotations of a page, in `/Annots` order.
///
/// Only `/Subtype /Link` is returned. Every other annotation subtype belongs
/// to the annotation model in [`crate::edit`], and reaching for them from here
/// is how one gets written twice.
///
/// 12.5.6.5: `/Dest` and `/A` are alternatives, and a file that carries both
/// is malformed. `/Dest` wins, because a destination cannot launch anything
/// and an action can.
#[must_use]
pub fn links(doc: &CosDocument, page: ObjRef) -> Vec<Link> {
    let Ok(object) = doc.get(page) else {
        return Vec::new();
    };
    let Some(dict) = object.as_dict() else {
        return Vec::new();
    };

    let annots = doc.resolve_key(dict, doc.intern(b"Annots"));
    let Some(items) = annots.as_array() else {
        return Vec::new();
    };

    let resolver = Resolver::new(doc);
    let subtype_key = doc.intern(b"Subtype");
    let link_name = doc.intern(b"Link");
    let rect_key = doc.intern(b"Rect");
    let dest_key = doc.intern(b"Dest");

    let mut out = Vec::new();
    for item in items.iter().take(crate::limits::MAX_ARRAY_LEN) {
        let reference = item.as_objref();
        let resolved = doc.resolve(item);
        let Some(annot) = resolved.as_dict() else {
            continue;
        };
        if annot.get_name(subtype_key) != Some(link_name) {
            continue;
        }

        let Some(rect) = doc
            .resolve_key(annot, rect_key)
            .as_array()
            .and_then(Rect::from_array)
        else {
            // 12.5.2 requires /Rect; without one there is nothing to click.
            continue;
        };

        let target = annot
            .get(dest_key)
            .and_then(|value| resolver.destination(value))
            .map(Action::GoTo)
            .or_else(|| {
                // 12.5.3: /A holds the action dictionary itself, not the
                // annotation that carries it.
                let action = doc.resolve_key(annot, doc.intern(b"A"));
                resolver.action(action.as_dict()?)
            });

        out.push(Link {
            reference,
            rect,
            target,
        });
    }
    out
}

// ---------------------------------------------------------------------------
// The writing side (gap 31, milestone 5).
//
// Everything below turns a value into the bytes 12.3.2.2 and 12.6.4.7 spell,
// and it lives here rather than in `build.rs` because the *reading* side of
// each of these entries is the code above it. A destination array written in
// one file and parsed in another is how the two halves drift apart; keeping
// them in one file is what makes `kind_of` and [`destination_array`] readable
// against each other.
// ---------------------------------------------------------------------------

impl DestKind {
    /// Whether this view can be written to a file.
    ///
    /// Refused rather than repaired, which is [`crate::build::DocumentBuilder`]'s
    /// posture everywhere else: a `NaN` coordinate would reach the file as the
    /// token `NaN`, which is not a PDF number at all, and a `/FitR` whose
    /// rectangle encloses nothing asks a viewer for infinite magnification.
    ///
    /// A **negative** zoom is refused for the same reason and a zoom of zero
    /// is not: 12.3.2.2 gives zero a meaning — "leave the magnification
    /// unchanged", exactly what `null` means — and gives a negative number
    /// none.
    #[must_use]
    pub fn is_writable(&self) -> bool {
        let finite = |v: Option<f64>| v.is_none_or(|v| v.is_finite());
        match self {
            DestKind::Fit | DestKind::FitB => true,
            DestKind::Xyz { left, top, zoom } => {
                finite(*left) && finite(*top) && zoom.is_none_or(|z| z.is_finite() && z >= 0.0)
            }
            DestKind::FitH { top } | DestKind::FitBH { top } => finite(*top),
            DestKind::FitV { left } | DestKind::FitBV { left } => finite(*left),
            DestKind::FitR {
                left,
                bottom,
                right,
                top,
            } => {
                [*left, *bottom, *right, *top].iter().all(|v| v.is_finite())
                    && right > left
                    && top > bottom
            }
        }
    }
}

/// One optional coordinate of a destination, as Table 151 spells it.
///
/// `null` and not an omission: 12.3.2.2 positions each parameter by its index
/// in the array, so a `/XYZ` that left out its `left` would make the file's
/// `top` this reader's `left`.
fn coordinate(value: Option<f64>) -> Object {
    value.map_or(Object::Null, Object::Real)
}

/// The `/Dest` array for a page and a view (12.3.2.2, Table 151).
///
/// The page is an **indirect reference**, never the page number 12.3.2.2 also
/// permits: a number is only correct while nothing renumbers the pages, and
/// this crate's own editor does.
pub(crate) fn destination_array(names: &NameTable, page: ObjRef, view: &DestKind) -> Object {
    let mut out = vec![Object::Ref(page)];
    match view {
        DestKind::Xyz { left, top, zoom } => {
            out.push(Object::Name(names.intern(b"XYZ")));
            out.push(coordinate(*left));
            out.push(coordinate(*top));
            // 12.3.2.2: "a zoom value of 0 has the same meaning as a null
            // value". Two spellings of one value is a distinction with no
            // consequence, so the caller's zero is written as the null, and
            // the round trip through `kind_of` — which filters zero back out
            // — is an equality rather than an equivalence.
            out.push(coordinate(zoom.filter(|z| *z != 0.0)));
        }
        DestKind::Fit => out.push(Object::Name(names.intern(b"Fit"))),
        DestKind::FitH { top } => {
            out.push(Object::Name(names.intern(b"FitH")));
            out.push(coordinate(*top));
        }
        DestKind::FitV { left } => {
            out.push(Object::Name(names.intern(b"FitV")));
            out.push(coordinate(*left));
        }
        DestKind::FitR {
            left,
            bottom,
            right,
            top,
        } => {
            out.push(Object::Name(names.intern(b"FitR")));
            out.push(Object::Real(*left));
            out.push(Object::Real(*bottom));
            out.push(Object::Real(*right));
            out.push(Object::Real(*top));
        }
        DestKind::FitB => out.push(Object::Name(names.intern(b"FitB"))),
        DestKind::FitBH { top } => {
            out.push(Object::Name(names.intern(b"FitBH")));
            out.push(coordinate(*top));
        }
        DestKind::FitBV { left } => {
            out.push(Object::Name(names.intern(b"FitBV")));
            out.push(coordinate(*left));
        }
    }
    Object::Array(out)
}

/// Whether a URI can be written as a `/URI` action (12.6.4.7).
///
/// 12.6.4.7 makes the URI "encoded in 7-bit ASCII", so a byte with its top bit
/// set is refused rather than written: a caller with a non-ASCII URI owes it
/// RFC 3986's percent-encoding, and doing that here would mean guessing which
/// encoding the bytes were in. ASCII control characters go the same way, since
/// no URI contains one and a literal string would carry it into the file
/// intact.
///
/// An **empty** URI is refused too. `/URI ()` is a well-formed action that
/// resolves to nothing, which is the class of output this repository's plans
/// call worse than none.
pub(crate) fn is_writable_uri(uri: &str) -> bool {
    !uri.is_empty() && uri.bytes().all(|b| (0x20..0x7f).contains(&b))
}

/// The `/A` dictionary for a URI action (12.6.4.7).
pub(crate) fn uri_action(names: &NameTable, uri: &str) -> Dict {
    let mut dict = Dict::new();
    dict.insert(Name::TYPE, Object::Name(names.intern(b"Action")));
    dict.insert(names.intern(b"S"), Object::Name(names.intern(b"URI")));
    dict.insert(
        names.intern(b"URI"),
        Object::String(PdfString::literal(uri.as_bytes().to_vec())),
    );
    dict
}

/// `/Border [0 0 0]` — a link a viewer does not draw a box around.
///
/// 12.5.2 Table 164 defaults `/Border` to `[0 0 1]`, a one-unit border on
/// every link that omits it. That default is a viewer artefact rather than
/// content: a cross-reference inside a paragraph is styled by the document,
/// and a rectangle drawn around it by the reader is not something any producer
/// asked for. Written explicitly, because the absence is the box.
pub(crate) fn no_border() -> Object {
    Object::Array(vec![Object::Int(0), Object::Int(0), Object::Int(0)])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_three_kinds_are_distinct_types() {
        // The regression this file exists for: a URI must never be
        // representable as a named destination by accident.
        let uri = Destination::Uri(b"https://example.invalid/x".to_vec());
        let named = Destination::Named(b"https://example.invalid/x".to_vec());
        assert_ne!(uri, named);
    }
}

#[cfg(test)]
mod link_tests {
    use super::*;

    fn document() -> CosDocument {
        let bytes: &[u8] = b"%PDF-1.7\n\
1 0 obj\n<< /Type /Catalog /Pages 2 0 R /Names << /Dests 8 0 R >> >>\nendobj\n\
2 0 obj\n<< /Type /Pages /Count 2 /Kids [3 0 R 4 0 R] >>\nendobj\n\
3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200]\n\
   /Annots [5 0 R 6 0 R 7 0 R 9 0 R] >>\nendobj\n\
4 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] >>\nendobj\n\
5 0 obj\n<< /Type /Annot /Subtype /Link /Rect [10 20 90 40]\n\
   /Dest [4 0 R /Fit] >>\nendobj\n\
6 0 obj\n<< /Type /Annot /Subtype /Link /Rect [10 60 90 80]\n\
   /A << /S /URI /URI (https://example.invalid/) >> >>\nendobj\n\
7 0 obj\n<< /Type /Annot /Subtype /Square /Rect [0 0 10 10] >>\nendobj\n\
8 0 obj\n<< /Names [(chapter) [4 0 R /Fit]] >>\nendobj\n\
9 0 obj\n<< /Type /Annot /Subtype /Link /Rect [10 100 90 120] >>\nendobj\n\
trailer\n<< /Size 10 /Root 1 0 R >>\n%%EOF\n";
        CosDocument::open(bytes).expect("it opens")
    }

    #[test]
    fn only_link_annotations_are_returned() {
        let doc = document();
        let found = links(&doc, ObjRef::new(3, 0));
        assert_eq!(found.len(), 3, "the /Square is not a link");
        assert_eq!(found[0].rect.x0, 10.0);
        assert_eq!(found[0].rect.y1, 40.0);
    }

    /// Ruling 6: an explicit destination stays explicit, and a URI is never
    /// mistaken for one.
    #[test]
    fn a_destination_and_a_uri_stay_distinct() {
        let doc = document();
        let found = links(&doc, ObjRef::new(3, 0));

        match &found[0].target {
            Some(Action::GoTo(Destination::Explicit { page_index, .. })) => {
                assert_eq!(*page_index, Some(1), "it points at the second page");
            }
            other => panic!("expected an explicit destination, got {other:?}"),
        }

        match &found[1].target {
            Some(Action::Uri(uri)) => {
                assert_eq!(uri.as_slice(), b"https://example.invalid/");
            }
            other => panic!("expected a URI action, got {other:?}"),
        }
    }

    /// A link with neither `/Dest` nor `/A` is legal and useless. It is
    /// reported with no target rather than dropped, because a reader that
    /// draws link borders still has to draw this one.
    #[test]
    fn a_link_with_no_target_is_kept() {
        let doc = document();
        let found = links(&doc, ObjRef::new(3, 0));
        assert!(found[2].target.is_none());
        assert_eq!(found[2].reference, Some(ObjRef::new(9, 0)));
    }

    #[test]
    fn a_page_with_no_annotations_has_no_links() {
        let doc = document();
        assert!(links(&doc, ObjRef::new(4, 0)).is_empty());
    }
}
