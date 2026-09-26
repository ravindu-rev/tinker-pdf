//! Page operations: the page order, page boxes and content, and copying a
//! page in from another document.

use std::collections::HashMap;

use super::{without, DocumentEditor};
use crate::doc::CosDocument;
use crate::name::Name;
use crate::object::{Dict, ObjRef, Object};
use crate::pages::{self, Rect};
use crate::write::StreamData;

impl DocumentEditor {
    /// The page references, in order, including any reordering done here.
    #[must_use]
    pub fn page_refs(&self) -> Vec<ObjRef> {
        match &self.page_order {
            Some(order) => order.clone(),
            None => pages::collect(&self.doc)
                .into_iter()
                .map(|p| p.reference)
                .collect(),
        }
    }

    fn ensure_order(&mut self) -> &mut Vec<ObjRef> {
        if self.page_order.is_none() {
            self.page_order = Some(self.page_refs());
        }
        self.page_order.get_or_insert_with(Vec::new)
    }

    /// Removes a page. Returns false when the index does not exist.
    ///
    /// Bounds are checked **before** the page order is materialized, so a
    /// refused operation leaves the editor genuinely unchanged rather than
    /// merely unchanged in content — `is_dirty` must not become true because
    /// someone asked for a page that is not there.
    pub fn delete_page(&mut self, index: u32) -> bool {
        let index = index as usize;
        if index >= self.page_refs().len() {
            return false;
        }
        self.ensure_order().remove(index);
        true
    }

    /// Moves a page to a new position.
    pub fn move_page(&mut self, from: u32, to: u32) -> bool {
        let (from, to) = (from as usize, to as usize);
        let len = self.page_refs().len();
        if from >= len || to >= len {
            return false;
        }
        let order = self.ensure_order();
        let page = order.remove(from);
        order.insert(to, page);
        true
    }

    /// Rotates a page by a quarter-turn multiple, relative to its current
    /// rotation.
    pub fn rotate_page(&mut self, index: u32, degrees: i64) -> bool {
        let Some(reference) = self.page_refs().get(index as usize).copied() else {
            return false;
        };
        let Some(Object::Dict(mut dict)) = self.get(reference) else {
            return false;
        };

        let rotate = self.intern(b"Rotate");
        let current = dict.get_int(rotate).unwrap_or(0);
        let next = pages::normalize_rotation(current + degrees);
        dict.insert(rotate, Object::Int(i64::from(next)));
        self.put(reference, Object::Dict(dict));
        true
    }

    /// Sets a page's `/CropBox` (14.11.2), in the page's own user space.
    ///
    /// The rectangle is written as the caller gives it. It is **not** clipped
    /// to the media box here, because 14.11.2 lets the two disagree and a
    /// reader is the one that reconciles them — `Page::crop_box` already does,
    /// and doing it twice would mean an editor could not write the document
    /// its caller asked for.
    ///
    /// A degenerate rectangle is refused rather than written: a crop box of no
    /// area is a page a viewer cannot show, and 7.9.5 wants two distinct
    /// corners.
    pub fn set_crop_box(&mut self, index: u32, x0: f64, y0: f64, x1: f64, y1: f64) -> bool {
        if ![x0, y0, x1, y1].iter().all(|v| v.is_finite()) {
            return false;
        }
        let (left, right) = (x0.min(x1), x0.max(x1));
        let (bottom, top) = (y0.min(y1), y0.max(y1));
        if right - left <= 0.0 || top - bottom <= 0.0 {
            return false;
        }
        let Some(reference) = self.page_refs().get(index as usize).copied() else {
            return false;
        };
        let Some(Object::Dict(mut dict)) = self.get(reference) else {
            return false;
        };
        let key = self.intern(b"CropBox");
        dict.insert(
            key,
            Object::Array(vec![
                Object::Real(left),
                Object::Real(bottom),
                Object::Real(right),
                Object::Real(top),
            ]),
        );
        self.put(reference, Object::Dict(dict));
        true
    }

    /// Inserts a blank page of the given size at `index`.
    ///
    /// `index` may equal the page count, which appends. A larger one is
    /// refused rather than clamped: a caller who miscounted wants to know,
    /// not to have the page land somewhere plausible.
    pub fn insert_page(&mut self, index: u32, width: f64, height: f64) -> Option<ObjRef> {
        let at = index as usize;
        if at > self.page_refs().len() || !width.is_finite() || !height.is_finite() {
            return None;
        }
        if width <= 0.0 || height <= 0.0 {
            return None;
        }

        let reference = self.allocate();
        let mut dict = Dict::new();
        dict.insert(Name::TYPE, Object::Name(self.intern(b"Page")));
        dict.insert(
            Name::MEDIA_BOX,
            Object::Array(vec![
                Object::Int(0),
                Object::Int(0),
                Object::Real(width),
                Object::Real(height),
            ]),
        );
        dict.insert(Name::RESOURCES, Object::Dict(Dict::new()));
        // /Parent is written by `save`, which is the only place that knows
        // what the tree will look like once every operation has been applied.
        self.put(reference, Object::Dict(dict));

        self.ensure_order().insert(at, reference);
        Some(reference)
    }

    /// Copies a page from another document into this one, at `index`.
    ///
    /// Every object the page reaches is copied with it and renumbered, because
    /// the two documents number independently and the source's numbers mean
    /// nothing here. A shared resource copied twice is copied twice: dedup
    /// needs content hashing to be safe, and a wrong dedup silently merges two
    /// different fonts.
    ///
    /// Returns the new page's reference, or `None` when the source has no such
    /// page.
    pub fn import_page(&mut self, source: &CosDocument, page: u32, index: u32) -> Option<ObjRef> {
        let at = index as usize;
        if at > self.page_refs().len() {
            return None;
        }

        let pages = pages::collect(source);
        let from = pages.get(page as usize)?;

        // The page's own dictionary, minus the entries that describe where it
        // sat in a tree it is leaving.
        let object = source.get(from.reference).ok()?;
        let mut dict = object.as_dict()?.clone();
        dict = without(&dict, Name::PARENT);

        // Inheritable attributes are resolved rather than inherited, because
        // the ancestors that carried them are not coming along.
        let media = Object::Array(vec![
            Object::Real(from.media_box.x0),
            Object::Real(from.media_box.y0),
            Object::Real(from.media_box.x1),
            Object::Real(from.media_box.y1),
        ]);
        dict.insert(Name::MEDIA_BOX, media);
        if let Some(resources) = from.resources.clone() {
            dict.insert(Name::RESOURCES, Object::Dict(resources));
        }
        if from.rotation != 0 {
            dict.insert(
                source.intern(b"Rotate"),
                Object::Int(i64::from(from.rotation)),
            );
        }

        let mut mapping: HashMap<u32, ObjRef> = HashMap::new();
        let copied = self.copy_value(source, &Object::Dict(dict), &mut mapping, 0)?;

        let reference = self.allocate();
        self.put(reference, copied);
        self.ensure_order().insert(at, reference);
        Some(reference)
    }

    /// Copies one value from `source`, following every reference it holds.
    ///
    /// Depth-capped and cycle-guarded through `mapping`: a page whose
    /// resources refer back to it is unusual but legal, and following it
    /// blindly does not terminate.
    fn copy_value(
        &mut self,
        source: &CosDocument,
        value: &Object,
        mapping: &mut HashMap<u32, ObjRef>,
        depth: u32,
    ) -> Option<Object> {
        if depth > crate::limits::MAX_NEST_DEPTH {
            return Some(Object::Null);
        }

        match value {
            Object::Ref(r) => {
                if let Some(existing) = mapping.get(&r.num) {
                    return Some(Object::Ref(*existing));
                }
                let target = self.allocate();
                // Recorded *before* recursing, so a cycle finds the number
                // rather than allocating forever.
                mapping.insert(r.num, target);

                let loaded = source.get(*r).ok()?;
                match loaded.as_ref() {
                    Object::Stream(stream) => {
                        // The raw bytes, which are plaintext once the source
                        // has been authenticated, plus its dictionary with the
                        // references inside it remapped.
                        let data = source.stream_raw(*r).unwrap_or_default();
                        let dict = self.copy_dict(source, &stream.dict, mapping, depth + 1)?;
                        self.put_stream(target, StreamData { dict, data });
                    }
                    other => {
                        let copied = self.copy_value(source, other, mapping, depth + 1)?;
                        self.put(target, copied);
                    }
                }
                Some(Object::Ref(target))
            }
            Object::Array(items) => {
                let mut out = Vec::with_capacity(items.len());
                for item in items {
                    out.push(self.copy_value(source, item, mapping, depth + 1)?);
                }
                Some(Object::Array(out))
            }
            Object::Dict(dict) => Some(Object::Dict(self.copy_dict(
                source,
                dict,
                mapping,
                depth + 1,
            )?)),
            Object::Name(name) => {
                // Names are interned per document, so the *bytes* travel and
                // the number is re-derived. Copying the number would name
                // whatever this document happens to have at that index.
                let bytes = source.name_bytes(*name)?;
                Some(Object::Name(self.intern(&bytes)))
            }
            other => Some(other.clone()),
        }
    }

    fn copy_dict(
        &mut self,
        source: &CosDocument,
        dict: &Dict,
        mapping: &mut HashMap<u32, ObjRef>,
        depth: u32,
    ) -> Option<Dict> {
        let mut out = Dict::with_capacity(dict.len());
        for (key, value) in dict.iter() {
            let key_bytes = source.name_bytes(*key)?;
            let copied = self.copy_value(source, value, mapping, depth + 1)?;
            out.insert(self.intern(&key_bytes), copied);
        }
        Some(out)
    }

    /// Keeps only the pages in `keep`, in that order.
    ///
    /// The split half of split-and-merge: saving the result twice with
    /// different selections divides a document. Pages appear in the order
    /// given, so this reorders as well, and a repeated index appears twice —
    /// both are what a caller asking for an explicit order means.
    ///
    /// Returns false when any index is out of range, having changed nothing.
    pub fn keep_pages(&mut self, keep: &[u32]) -> bool {
        let order = self.page_refs();
        if keep.iter().any(|i| *i as usize >= order.len()) {
            return false;
        }
        let kept: Vec<ObjRef> = keep
            .iter()
            .filter_map(|i| order.get(*i as usize).copied())
            .collect();
        *self.ensure_order() = kept;
        true
    }

    /// Appends operators to a page's content, wrapped so they cannot disturb
    /// what is already there.
    ///
    /// 8.10.1: a content stream may leave the graphics state unbalanced, so
    /// the existing content is bracketed with `q`/`Q` before anything is
    /// added — without that, a page whose content ends inside a `q` would
    /// apply its transform to the addition.
    pub fn append_content(&mut self, page: u32, operators: &[u8]) -> bool {
        let Some(reference) = self.page_refs().get(page as usize).copied() else {
            return false;
        };
        let Some(Object::Dict(mut dict)) = self.get(reference) else {
            return false;
        };

        let existing = self.page_content(reference);
        let mut data = Vec::with_capacity(existing.len() + operators.len() + 8);
        data.extend_from_slice(b"q\n");
        data.extend_from_slice(&existing);
        data.extend_from_slice(b"\nQ\nq\n");
        data.extend_from_slice(operators);
        data.extend_from_slice(b"\nQ\n");

        let content_ref = self.allocate();
        self.put_stream(
            content_ref,
            StreamData {
                dict: Dict::new(),
                data,
            },
        );
        dict.insert(Name::CONTENTS, Object::Ref(content_ref));
        self.put(reference, Object::Dict(dict));
        true
    }

    /// A page's current content, joined.
    fn page_content(&self, reference: ObjRef) -> Vec<u8> {
        let Some(Object::Dict(dict)) = self.get(reference) else {
            return Vec::new();
        };

        let refs: Vec<ObjRef> = match dict.get(Name::CONTENTS) {
            Some(Object::Ref(r)) => vec![*r],
            Some(Object::Array(items)) => items.iter().filter_map(Object::as_objref).collect(),
            _ => Vec::new(),
        };

        let mut out = Vec::new();
        for r in refs {
            // An overlay stream is this editor's own work; otherwise the
            // document's decoded bytes. One reader for both, so that a caller
            // asking the same question through `stream_bytes` cannot get a
            // different answer from the one this uses.
            if let Some(bytes) = self.stream_bytes(r) {
                out.extend_from_slice(&bytes);
            }
            out.push(b'\n');
        }
        out
    }

    /// The media box of a page, for callers placing things on it.
    #[must_use]
    pub fn page_box(&self, index: u32) -> Option<Rect> {
        pages::collect(&self.doc)
            .get(index as usize)
            .map(|p| p.media_box)
    }

    /// The page tree and page dictionaries a reordering changes.
    ///
    /// Returned rather than applied, because the two save modes build
    /// different object sets and both need them.
    pub(super) fn page_tree_updates(&self) -> Vec<(u32, Object)> {
        let Some(order) = &self.page_order else {
            return Vec::new();
        };
        let Some(catalog) = self.doc.catalog() else {
            return Vec::new();
        };
        let Some(tree_ref) = catalog.get_ref(Name::PAGES) else {
            return Vec::new();
        };
        let Some(Object::Dict(mut tree)) = self.get(tree_ref) else {
            return Vec::new();
        };

        let mut out = Vec::with_capacity(order.len() + 1);
        tree.insert(
            Name::KIDS,
            Object::Array(order.iter().map(|r| Object::Ref(*r)).collect()),
        );
        tree.insert(Name::COUNT, Object::Int(order.len() as i64));
        out.push((tree_ref.num, Object::Dict(tree)));

        // 7.7.3.2: every page names its parent, and a reader walking upward
        // from a page — for an inherited attribute, or to find which document
        // it belongs to — needs it. An inserted or imported page has never had
        // one, and a moved page may have belonged to a node that is no longer
        // in the tree, so the whole flattened list is pointed at the root
        // rather than only the new arrivals.
        for reference in order {
            if let Some(Object::Dict(mut page)) = self.get(*reference) {
                page.insert(Name::PARENT, Object::Ref(tree_ref));
                out.push((reference.num, Object::Dict(page)));
            }
        }
        out
    }
}
