//! The page tree (7.7.3), its inheritable attributes (7.7.3.4), and per-page
//! geometry (7.7.3.3).
//!
//! The policy is: trust the fast path, verify on contact, repair once.
//! [`CosDocument::page_count`] reads the root `/Count` without touching a page,
//! and [`CosDocument::page`] descends by intermediate `/Count`s in O(depth).
//! The first inconsistency — a leaf where a count said there was a subtree, a
//! `/Count` that disagrees with its own kids — triggers exactly one full
//! enumeration, which builds the page map and its reverse, warns, and wins from
//! then on.
//!
//! Node classification ignores `/Type`: `/Kids` present means interior node,
//! otherwise leaf. A missing or wrong `/Type` is one of the most common
//! real-world defects and is not worth losing a document over.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use crate::doc::CosDocument;
use crate::limits;
use crate::name::Name;
use crate::object::{Dict, ObjRef, Object};
use crate::semantics::geom::{rect_from_object, PageGeometry, Rect, Rotation};
use crate::warn::{WarningKind, WarningSink};

/// The four attributes 7.7.3.4 makes inheritable, as the descent that reached
/// a node left them.
#[derive(Clone, Debug, Default)]
pub(crate) struct Inherited {
    resources: Option<Arc<Object>>,
    media_box: Option<Arc<Object>>,
    crop_box: Option<Arc<Object>>,
    rotate: Option<Arc<Object>>,
}

impl Inherited {
    /// Applies one node's own entries over what it inherited. A child's value
    /// always wins, which is what 7.7.3.4 means by inheritance.
    fn absorb(&mut self, doc: &CosDocument, dict: &Dict) {
        let sem = &doc.names.sem;
        if let Some(value) = dict.get(Name::RESOURCES) {
            self.resources = Some(doc.resolve(value));
        }
        if let Some(value) = dict.get(Name::MEDIA_BOX) {
            self.media_box = Some(doc.resolve(value));
        }
        if let Some(value) = dict.get(sem.crop_box) {
            self.crop_box = Some(doc.resolve(value));
        }
        if let Some(value) = dict.get(sem.rotate) {
            self.rotate = Some(doc.resolve(value));
        }
    }
}

/// One leaf of the page tree, with its inheritance already resolved.
#[derive(Clone, Debug)]
pub(crate) struct PageEntry {
    pub(crate) reference: Option<ObjRef>,
    pub(crate) dict: Dict,
    pub(crate) inherited: Inherited,
}

/// The full enumeration: pages in order, plus the reverse map destination
/// resolution needs.
#[derive(Debug, Default)]
pub(crate) struct PageMap {
    entries: Vec<PageEntry>,
    index: HashMap<u32, u32>,
}

impl PageMap {
    fn len(&self) -> u32 {
        u32::try_from(self.entries.len()).unwrap_or(u32::MAX)
    }

    fn entry(&self, index: u32) -> Option<&PageEntry> {
        self.entries.get(usize::try_from(index).ok()?)
    }
}

/// What one O(depth) descent found.
enum Descent {
    Found(Box<PageEntry>),
    /// The tree is self-consistent and simply has no such index.
    OutOfRange,
    /// Something disagreed with something else; the full walk decides.
    Inconsistent,
}

impl CosDocument {
    /// The document catalog (7.7.2), or an empty dictionary when `/Root` names
    /// nothing usable.
    pub fn catalog(&self) -> Dict {
        self.resolve_key(self.trailer(), Name::ROOT)
            .as_dict()
            .cloned()
            .unwrap_or_default()
    }

    /// How many pages the document has.
    ///
    /// The root `/Count` is trusted when it is a non-negative integer no larger
    /// than the number of cross-reference entries, since every page is an
    /// object and a `/Count` above that is provably a lie. Anything else costs
    /// one full walk, warns, and is remembered.
    pub fn page_count(&self) -> u32 {
        if let Some(map) = self.page_map_cached() {
            return map.len();
        }
        let root = self.page_tree_root();
        let ceiling = u32::try_from(self.xref().len()).unwrap_or(u32::MAX);
        match root.and_then(|dict| dict.get_int(Name::COUNT)) {
            Some(count) => match u32::try_from(count) {
                Ok(count) if count <= ceiling => count,
                _ => {
                    let mut sink = WarningSink::new();
                    sink.warn(0, WarningKind::PageCountMismatch);
                    self.absorb(sink);
                    self.page_map().len()
                }
            },
            None => self.page_map().len(),
        }
    }

    /// The page at a zero-based index.
    pub fn page(&self, index: u32) -> Option<Page<'_>> {
        let entry = match self.page_map_cached() {
            Some(map) => map.entry(index).cloned()?,
            None => match self.descend(index) {
                Descent::Found(entry) => *entry,
                Descent::OutOfRange => return None,
                Descent::Inconsistent => {
                    let mut sink = WarningSink::new();
                    sink.warn(0, WarningKind::PageCountMismatch);
                    self.absorb(sink);
                    self.page_map().entry(index).cloned()?
                }
            },
        };
        Some(self.build_page(index, entry))
    }

    /// The indirect reference of the page at `index`, without building a
    /// [`Page`].
    pub fn page_ref(&self, index: u32) -> Option<ObjRef> {
        self.page_map().entry(index)?.reference
    }

    /// The zero-based index of the page an indirect reference names.
    ///
    /// Generation is ignored, exactly as it is when loading an object: a table
    /// that disagrees with its own `N G obj` headers must not make a
    /// destination dangle.
    pub fn page_index_of(&self, reference: ObjRef) -> Option<u32> {
        self.page_map().index.get(&reference.num).copied()
    }

    // ---- internals -------------------------------------------------------

    fn page_tree_root(&self) -> Option<Dict> {
        let catalog = self.catalog();
        self.resolve_key(&catalog, Name::PAGES).as_dict().cloned()
    }

    fn build_page(&self, index: u32, entry: PageEntry) -> Page<'_> {
        let mut sink = WarningSink::new();
        let object = entry.reference;

        let media_box = match entry
            .inherited
            .media_box
            .as_deref()
            .and_then(|value| rect_from_object(self, value))
        {
            Some(rect) if !rect.is_degenerate() => rect,
            _ => {
                sink.warn_at(0, object, WarningKind::MediaBoxMissing);
                Rect::US_LETTER
            }
        };

        // 7.7.3.3: the crop box defaults to the media box, and a crop box that
        // does not overlap it describes nothing a reader could show.
        let crop_box = match entry
            .inherited
            .crop_box
            .as_deref()
            .and_then(|value| rect_from_object(self, value))
        {
            Some(rect) => {
                let clipped = rect.intersect(&media_box);
                if clipped.is_degenerate() {
                    sink.warn_at(0, object, WarningKind::CropBoxUnusable);
                    media_box
                } else {
                    clipped
                }
            }
            None => media_box,
        };

        let rotation = match entry.inherited.rotate.as_deref().and_then(Object::as_int) {
            Some(degrees) => {
                let (rotation, snapped) = Rotation::from_degrees(degrees);
                if snapped {
                    sink.warn_at(0, object, WarningKind::RotateNotAMultipleOfNinety);
                }
                rotation
            }
            None => Rotation::default(),
        };

        let resources = entry
            .inherited
            .resources
            .as_deref()
            .and_then(Object::as_dict)
            .cloned()
            .unwrap_or_default();

        self.absorb(sink);
        Page {
            doc: self,
            index,
            reference: entry.reference,
            dict: entry.dict,
            resources,
            media_box,
            crop_box,
            rotation,
        }
    }

    fn page_map_cached(&self) -> Option<&PageMap> {
        self.pages.get().map(|map| &**map)
    }

    pub(crate) fn page_map(&self) -> &PageMap {
        self.pages.get_or_init(|| Arc::new(self.build_page_map()))
    }

    /// O(depth) random access by intermediate `/Count` (7.7.3.2).
    fn descend(&self, index: u32) -> Descent {
        let Some(root) = self.page_tree_root() else {
            return Descent::Inconsistent;
        };
        let catalog = self.catalog();
        let mut node = root;
        let mut node_ref: Option<ObjRef> = catalog.get(Name::PAGES).and_then(Object::as_objref);
        let mut inherited = Inherited::default();
        let mut visited: HashSet<u32> = HashSet::new();
        let mut remaining = index;
        let mut depth = 0u32;

        loop {
            if depth >= limits::MAX_PAGE_TREE_DEPTH {
                return Descent::Inconsistent;
            }
            depth += 1;
            inherited.absorb(self, &node);

            let kids = match node.get(Name::KIDS) {
                Some(kids) => self.resolve(kids),
                None => {
                    // A leaf: the descent only ever arrives with remaining 0.
                    return if remaining == 0 {
                        Descent::Found(Box::new(PageEntry {
                            reference: node_ref,
                            dict: node,
                            inherited,
                        }))
                    } else {
                        Descent::Inconsistent
                    };
                }
            };
            let Some(items) = kids.as_array() else {
                return Descent::Inconsistent;
            };

            let mut next: Option<(Option<ObjRef>, Dict)> = None;
            for item in items {
                let kid_ref = item.as_objref();
                if let Some(r) = kid_ref {
                    if visited.contains(&r.num) {
                        return Descent::Inconsistent;
                    }
                }
                let resolved = self.resolve(item);
                let Some(kid) = resolved.as_dict() else {
                    return Descent::Inconsistent;
                };
                let count = match kid.get(Name::KIDS) {
                    Some(_) => match kid.get_int(Name::COUNT).and_then(|c| u32::try_from(c).ok()) {
                        Some(count) => count,
                        None => return Descent::Inconsistent,
                    },
                    None => 1,
                };
                if remaining < count {
                    if let Some(r) = kid_ref {
                        visited.insert(r.num);
                    }
                    next = Some((kid_ref, kid.clone()));
                    break;
                }
                remaining -= count;
            }

            match next {
                Some((reference, kid)) => {
                    node_ref = reference;
                    node = kid;
                }
                // Running out of kids at the root means the index is past the
                // last page; anywhere else it means a parent lied about its
                // own subtree.
                None => {
                    return if depth == 1 {
                        Descent::OutOfRange
                    } else {
                        Descent::Inconsistent
                    }
                }
            }
        }
    }

    /// The one full enumeration: iterative, with a visited set of object
    /// numbers, a depth cap and a node cap.
    fn build_page_map(&self) -> PageMap {
        let mut sink = WarningSink::new();
        let mut map = PageMap::default();
        let Some(root) = self.page_tree_root() else {
            sink.warn(0, WarningKind::PageTreeMissing);
            self.absorb(sink);
            return map;
        };

        let catalog = self.catalog();
        let root_ref = catalog.get(Name::PAGES).and_then(Object::as_objref);
        let mut visited: HashSet<u32> = HashSet::new();
        if let Some(r) = root_ref {
            visited.insert(r.num);
        }
        let mut nodes = 0usize;
        let mut stack: Vec<(Option<ObjRef>, Dict, Inherited, u32)> =
            vec![(root_ref, root, Inherited::default(), 0)];

        while let Some((reference, dict, mut inherited, depth)) = stack.pop() {
            nodes += 1;
            if nodes > limits::MAX_PAGE_TREE_NODES {
                sink.warn(0, WarningKind::PageTreeNodeCapHit);
                break;
            }
            inherited.absorb(self, &dict);

            let Some(kids) = dict.get(Name::KIDS) else {
                if let Some(r) = reference {
                    let next = map.len();
                    map.index.entry(r.num).or_insert(next);
                }
                map.entries.push(PageEntry {
                    reference,
                    dict,
                    inherited,
                });
                continue;
            };
            if depth + 1 >= limits::MAX_PAGE_TREE_DEPTH {
                sink.warn_at(0, reference, WarningKind::PageTreeDepthCapHit);
                continue;
            }
            let kids = self.resolve(kids);
            let Some(items) = kids.as_array() else {
                continue;
            };
            // Reversed, so popping visits the kids in file order.
            for item in items.iter().rev() {
                let kid_ref = item.as_objref();
                if let Some(r) = kid_ref {
                    if !visited.insert(r.num) {
                        sink.warn_at(0, Some(r), WarningKind::PageTreeCycle);
                        continue;
                    }
                }
                if let Some(kid) = self.resolve(item).as_dict() {
                    stack.push((kid_ref, kid.clone(), inherited.clone(), depth + 1));
                }
            }
        }

        // The declared count is a claim; the walk is the fact.
        if let Some(declared) = dict_count(self) {
            if declared != map.len() {
                sink.warn(0, WarningKind::PageCountMismatch);
            }
        }
        self.absorb(sink);
        map
    }
}

fn dict_count(doc: &CosDocument) -> Option<u32> {
    let catalog = doc.catalog();
    let pages = doc.resolve_key(&catalog, Name::PAGES);
    let count = pages.as_dict()?.get_int(Name::COUNT)?;
    u32::try_from(count).ok()
}

/// One page, with inheritance and geometry already resolved.
pub struct Page<'d> {
    doc: &'d CosDocument,
    index: u32,
    reference: Option<ObjRef>,
    dict: Dict,
    resources: Dict,
    media_box: Rect,
    crop_box: Rect,
    rotation: Rotation,
}

impl<'d> Page<'d> {
    /// The document this page belongs to.
    pub fn document(&self) -> &'d CosDocument {
        self.doc
    }

    /// The zero-based page index.
    pub fn index(&self) -> u32 {
        self.index
    }

    /// The page object's reference, absent only when `/Kids` held the page
    /// dictionary directly instead of a reference.
    pub fn reference(&self) -> Option<ObjRef> {
        self.reference
    }

    /// The page dictionary itself (7.7.3.3), exactly as the file wrote it.
    pub fn dict(&self) -> &Dict {
        &self.dict
    }

    /// The page's `/Resources`, inheritance already applied (7.7.3.4). Empty
    /// when the page and its ancestors all lack one.
    pub fn resources(&self) -> &Dict {
        &self.resources
    }

    /// The `/MediaBox`, normalized, falling back to US Letter.
    pub fn media_box(&self) -> Rect {
        self.media_box
    }

    /// The `/CropBox`, defaulted to the media box and already intersected with
    /// it.
    pub fn crop_box(&self) -> Rect {
        self.crop_box
    }

    /// `/Rotate`, normalized to a quarter turn.
    pub fn rotation(&self) -> Rotation {
        self.rotation
    }

    /// Boxes, rotation, and the displayed size with the 90/270 swap applied.
    pub fn geometry(&self) -> PageGeometry {
        PageGeometry::new(self.media_box, self.crop_box, self.rotation)
    }

    /// The `/Contents` chain (7.7.3.3), in order.
    ///
    /// A page may name one stream or an array of them, and the concatenation
    /// is what the interpreter reads. Direct (non-indirect) content streams
    /// cannot be named here and are the interpreter's problem, not this
    /// layer's.
    pub fn content_refs(&self) -> Vec<ObjRef> {
        let Some(contents) = self.dict.get(Name::CONTENTS) else {
            return Vec::new();
        };
        match contents.as_objref() {
            Some(r) if self.doc.resolve(contents).as_stream().is_some() => vec![r],
            _ => match self.doc.resolve(contents).as_array() {
                Some(items) => items.iter().filter_map(Object::as_objref).collect(),
                None => Vec::new(),
            },
        }
    }
}

impl core::fmt::Debug for Page<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Page")
            .field("index", &self.index)
            .field("reference", &self.reference)
            .field("media_box", &self.media_box)
            .field("crop_box", &self.crop_box)
            .field("rotation", &self.rotation)
            .finish()
    }
}
