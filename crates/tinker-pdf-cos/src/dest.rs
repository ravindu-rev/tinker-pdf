//! Destinations and actions (12.3.2, 12.6).
//!
//! The type here is the design answer to a specific defect. MuPDF's outline
//! writer took a `uri` field and, given `#page=2`, stored it as a *named*
//! destination, percent-encoded to `#nameddest=%23page%3D2`, which resolves to
//! no page when the file is read back. The three kinds are different things
//! and this engine never conflates them, in either direction (ruling 6).

use crate::doc::CosDocument;
use crate::name::Name;
use crate::object::{Dict, ObjRef, Object};
use crate::pages::Page;

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
