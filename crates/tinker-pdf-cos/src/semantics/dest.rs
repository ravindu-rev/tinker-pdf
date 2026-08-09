//! Destinations (12.3.2).
//!
//! Ruling 6, stated in code: `Explicit | Named | Uri` are three things and
//! reading never collapses them. This is the design answer to MuPDF limitation
//! #6, where writing a URI outline entry silently produced a percent-encoded
//! *named* destination that resolved to nothing on read-back. A URI stays a
//! URI here, forever, on both sides of the API.
//!
//! `Option` on the `/XYZ` components is load-bearing for the same reason:
//! `null` means "keep the current value", and collapsing it to `0.0` is a
//! write-side corruption this model refuses to set up.

use std::sync::Arc;

use crate::doc::CosDocument;
use crate::limits;
use crate::object::{Dict, ObjRef, Object};
use crate::semantics::geom::Rect;
use crate::semantics::trees::NameTree;
use crate::warn::{WarningKind, WarningSink};

/// The page a destination points at.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PageTarget {
    /// A page object in this document.
    Ref(ObjRef),
    /// A zero-based page number, which is how remote destinations (12.6.4.3
    /// `/GoToR`) address pages in a file they cannot resolve references into.
    Index(u32),
}

/// The view a destination asks for (Table 151).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum DestKind {
    /// `/XYZ left top zoom`. `None` means "leave this one as it is".
    Xyz {
        /// Left edge of the window.
        left: Option<f64>,
        /// Top edge of the window.
        top: Option<f64>,
        /// Zoom factor; 0 counts as `None` per Table 151.
        zoom: Option<f64>,
    },
    /// `/Fit`: fit the whole page.
    Fit,
    /// `/FitH top`: fit the width, positioned vertically at `top`.
    FitH {
        /// Top edge of the window.
        top: Option<f64>,
    },
    /// `/FitV left`: fit the height, positioned horizontally at `left`.
    FitV {
        /// Left edge of the window.
        left: Option<f64>,
    },
    /// `/FitR left bottom right top`: fit a rectangle.
    FitR {
        /// The rectangle to fit, normalized.
        rect: Rect,
    },
    /// `/FitB`: fit the bounding box of the page's contents.
    FitB,
    /// `/FitBH top`: fit the bounding box's width.
    FitBH {
        /// Top edge of the window.
        top: Option<f64>,
    },
    /// `/FitBV left`: fit the bounding box's height.
    FitBV {
        /// Left edge of the window.
        left: Option<f64>,
    },
}

/// A destination (12.3.2), in the three forms that are never conflated.
#[derive(Clone, Debug, PartialEq)]
pub enum Destination {
    /// An explicit destination array: a page plus a view.
    Explicit {
        /// Which page.
        page: PageTarget,
        /// Which view of it.
        kind: DestKind,
    },
    /// A name to be looked up in the document's destination trees. Bytes, not
    /// text: 7.9.6 keys are byte strings and a lossy `String` here would be a
    /// lookup that cannot find its own key.
    Named(Vec<u8>),
    /// A uniform resource identifier (12.6.4.7). Never a name, ever.
    Uri(Vec<u8>),
}

/// A destination resolved against this document's page tree.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ResolvedDest {
    /// Zero-based page index.
    pub page_index: u32,
    /// The view the destination asked for.
    pub kind: DestKind,
}

impl CosDocument {
    /// Reads a destination out of whatever object carried it.
    ///
    /// An array is explicit; a string or a name is a named destination; a
    /// dictionary carries its destination under `/D` (12.3.2.3).
    pub fn destination(&self, object: &Object) -> Option<Destination> {
        let mut sink = WarningSink::new();
        let dest = self.destination_in(object, 0, &mut sink);
        self.absorb(sink);
        dest
    }

    pub(crate) fn destination_in(
        &self,
        object: &Object,
        depth: u32,
        sink: &mut WarningSink,
    ) -> Option<Destination> {
        if depth >= limits::MAX_DEST_CHAIN {
            return None;
        }
        let resolved = self.resolve(object);
        match &*resolved {
            Object::Array(items) => self.explicit(items, sink),
            Object::String(s) => Some(Destination::Named(s.bytes.clone())),
            // 12.3.2.3 assigns names to the old-style /Dests dictionary and
            // strings to the name tree; files mix them, so both read as named.
            Object::Name(n) => Some(Destination::Named(self.name_bytes(*n)?.to_vec())),
            Object::Dict(dict) => {
                let inner = dict.get(self.names.sem.d)?;
                self.destination_in(inner, depth + 1, sink)
            }
            _ => None,
        }
    }

    fn explicit(&self, items: &[Object], sink: &mut WarningSink) -> Option<Destination> {
        let page = match items.first()? {
            Object::Ref(r) => PageTarget::Ref(*r),
            Object::Int(n) => PageTarget::Index(u32::try_from(*n).ok()?),
            _ => return None,
        };
        let number = |at: usize| -> Option<f64> {
            let item = items.get(at)?;
            self.resolve(item).as_number()
        };
        let sem = &self.names.sem;
        let kind_name = items.get(1).and_then(|o| self.resolve(o).as_name());
        let kind = match kind_name {
            Some(n) if n == sem.fit => DestKind::Fit,
            Some(n) if n == sem.fit_b => DestKind::FitB,
            Some(n) if n == sem.fit_h => DestKind::FitH { top: number(2) },
            Some(n) if n == sem.fit_bh => DestKind::FitBH { top: number(2) },
            Some(n) if n == sem.fit_v => DestKind::FitV { left: number(2) },
            Some(n) if n == sem.fit_bv => DestKind::FitBV { left: number(2) },
            Some(n) if n == sem.fit_r => DestKind::FitR {
                rect: Rect::new(
                    number(2).unwrap_or(0.0),
                    number(3).unwrap_or(0.0),
                    number(4).unwrap_or(0.0),
                    number(5).unwrap_or(0.0),
                ),
            },
            Some(n) if n == sem.xyz => DestKind::Xyz {
                left: number(2),
                top: number(3),
                // Table 151: a zoom of 0 has the same meaning as null.
                zoom: number(4).filter(|z| *z != 0.0),
            },
            // An array with no recognizable fit still names a page, and the
            // least destructive reading of "no view given" is "keep this one".
            _ => {
                sink.warn(0, WarningKind::DestinationKindUnknown);
                DestKind::Xyz {
                    left: None,
                    top: None,
                    zoom: None,
                }
            }
        };
        Some(Destination::Explicit { page, kind })
    }

    /// The destination a name stands for, through the `/Names` → `/Dests` name
    /// tree (7.9.6) and then the old-style catalog `/Dests` dictionary (12.3.2.3
    /// as of PDF 1.1).
    pub fn named_destination(&self, key: &[u8]) -> Option<Destination> {
        let mut sink = WarningSink::new();
        let value = self.named_destination_object(key);
        let dest = value.and_then(|value| self.destination_in(&value, 1, &mut sink));
        self.absorb(sink);
        dest
    }

    fn named_destination_object(&self, key: &[u8]) -> Option<Arc<Object>> {
        if let Some(root) = self.name_tree_root(self.names.sem.dests) {
            if let Some(value) = NameTree::new(self, root).get(key) {
                return Some(value);
            }
        }
        let catalog = self.catalog();
        let legacy = self.resolve_key(&catalog, self.names.sem.dests);
        let dict = legacy.as_dict()?;
        // The name table only holds names the document actually used, so a key
        // that was never a name in this file cannot be in the legacy
        // dictionary either.
        let name = self.names.table.lookup(key)?;
        Some(self.resolve(dict.get(name)?))
    }

    /// Every named destination in the document, name tree first, then the
    /// old-style dictionary.
    pub fn named_destinations(&self) -> Vec<(Vec<u8>, Destination)> {
        let mut sink = WarningSink::new();
        let mut out = Vec::new();
        if let Some(root) = self.name_tree_root(self.names.sem.dests) {
            for (key, value) in NameTree::new(self, root).entries() {
                if let Some(dest) = self.destination_in(&value, 1, &mut sink) {
                    out.push((key, dest));
                }
            }
        }
        let catalog = self.catalog();
        let legacy = self.resolve_key(&catalog, self.names.sem.dests);
        if let Some(dict) = legacy.as_dict() {
            for (name, value) in dict.iter() {
                let Some(key) = self.name_bytes(*name) else {
                    continue;
                };
                if out.iter().any(|(existing, _)| existing.as_slice() == &key[..]) {
                    continue;
                }
                if let Some(dest) = self.destination_in(value, 1, &mut sink) {
                    out.push((key.to_vec(), dest));
                }
            }
        }
        self.absorb(sink);
        out
    }

    /// The subtree of `/Root` → `/Names` under `category` (7.7.4).
    pub(crate) fn name_tree_root(&self, category: crate::name::Name) -> Option<Dict> {
        let catalog = self.catalog();
        let names = self.resolve_key(&catalog, self.names.sem.names);
        let dict = names.as_dict()?;
        self.resolve_key(dict, category).as_dict().cloned()
    }

    /// Turns a destination into a zero-based page index plus a view.
    ///
    /// `None` for a URI, which addresses no page, and for a dangling target,
    /// which warns — the caller keeps the outline item or the link, because a
    /// bookmark with a dead target still belongs in the sidebar.
    pub fn resolve_destination(&self, dest: &Destination) -> Option<ResolvedDest> {
        let mut sink = WarningSink::new();
        let resolved = self.resolve_destination_in(dest, &mut sink);
        self.absorb(sink);
        resolved
    }

    fn resolve_destination_in(
        &self,
        dest: &Destination,
        sink: &mut WarningSink,
    ) -> Option<ResolvedDest> {
        let mut current = dest.clone();
        for _ in 0..limits::MAX_DEST_CHAIN {
            match current {
                Destination::Explicit { page, kind } => {
                    let page_index = match page {
                        PageTarget::Index(index) => index,
                        PageTarget::Ref(r) => match self.page_index_of(r) {
                            Some(index) => index,
                            None => {
                                sink.warn_at(0, Some(r), WarningKind::DestinationDangling);
                                return None;
                            }
                        },
                    };
                    return Some(ResolvedDest { page_index, kind });
                }
                Destination::Named(key) => match self.named_destination(&key) {
                    Some(next) => current = next,
                    None => {
                        sink.warn(0, WarningKind::DestinationUnresolved);
                        return None;
                    }
                },
                Destination::Uri(_) => return None,
            }
        }
        sink.warn(0, WarningKind::DestinationUnresolved);
        None
    }
}
