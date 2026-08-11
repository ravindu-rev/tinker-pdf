//! Editing an existing document (phase 10).
//!
//! An editor is an **overlay**, not a mutation: the document it was opened
//! from is never touched, changed objects accumulate in a map, and saving
//! writes either the overlay alone (an incremental update, which keeps the
//! original bytes intact for any signature over them) or the whole graph.
//!
//! That shape is what makes concurrent readers safe — they keep reading the
//! document they already have — and what makes an edit undoable by simply
//! dropping the editor.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use crate::doc::CosDocument;
use crate::name::Name;
use crate::object::{Dict, ObjRef, Object, PdfString};
use crate::pages::{self, Rect};
use crate::write::{self, ObjectSet, StreamData, WriteMode, WriteOptions, Written};
use crate::{fill, form};

/// Every indirect reference inside an object, pushed onto `queue`.
fn references_of(object: &Object, queue: &mut Vec<ObjRef>) {
    match object {
        Object::Ref(r) => queue.push(*r),
        Object::Array(items) => {
            for item in items {
                references_of(item, queue);
            }
        }
        Object::Dict(dict) => {
            for (_, value) in dict.iter() {
                references_of(value, queue);
            }
        }
        Object::Stream(stream) => {
            for (_, value) in stream.dict.iter() {
                references_of(value, queue);
            }
        }
        _ => {}
    }
}

/// 12.5.5's algorithm, reduced to the scale and offset a `cm` needs.
///
/// The transformed bounding box is fitted to the rectangle. A form with no box
/// is drawn where its own matrix puts it, which is what a viewer does with one.
fn fit(bbox: Option<Rect>, matrix: [f64; 6], rect: Rect) -> (f64, f64, f64, f64) {
    let Some(bbox) = bbox.filter(|b| !b.is_empty()) else {
        return (1.0, 1.0, 0.0, 0.0);
    };

    let corners = [
        (bbox.x0, bbox.y0),
        (bbox.x1, bbox.y0),
        (bbox.x1, bbox.y1),
        (bbox.x0, bbox.y1),
    ];
    let mapped: Vec<(f64, f64)> = corners
        .iter()
        .map(|(x, y)| {
            (
                matrix[0] * x + matrix[2] * y + matrix[4],
                matrix[1] * x + matrix[3] * y + matrix[5],
            )
        })
        .collect();

    let (mut x0, mut y0) = mapped[0];
    let (mut x1, mut y1) = mapped[0];
    for (x, y) in &mapped {
        x0 = x0.min(*x);
        y0 = y0.min(*y);
        x1 = x1.max(*x);
        y1 = y1.max(*y);
    }

    let (dx, dy) = (x1 - x0, y1 - y0);
    if dx <= f64::EPSILON || dy <= f64::EPSILON {
        return (1.0, 1.0, 0.0, 0.0);
    }

    let sx = (rect.x1 - rect.x0) / dx;
    let sy = (rect.y1 - rect.y0) / dy;
    (sx, sy, rect.x0 - x0 * sx, rect.y0 - y0 * sy)
}

/// A copy of `dict` with one key gone.
///
/// Removing an entry is rare enough that [`Dict`] has no method for it, and
/// rare enough that rebuilding is cheaper than carrying one.
pub(crate) fn without(dict: &Dict, key: Name) -> Dict {
    let mut out = Dict::with_capacity(dict.len());
    for (name, value) in dict.iter() {
        if *name != key {
            out.insert(*name, value.clone());
        }
    }
    out
}

/// Edits layered over an open document.
pub struct DocumentEditor {
    doc: Arc<CosDocument>,
    /// Objects this editor has replaced or added.
    overlay: HashMap<u32, Written>,
    /// Objects deleted, which are written as null so references to them
    /// resolve to nothing rather than to stale data.
    deleted: HashSet<u32>,
    next: u32,
    /// The page order, as references, once it has been disturbed.
    page_order: Option<Vec<ObjRef>>,
}

impl DocumentEditor {
    /// Begins editing.
    #[must_use]
    pub fn new(doc: Arc<CosDocument>) -> DocumentEditor {
        // New objects start past everything the document already uses.
        let next = doc.max_object_number().saturating_add(1);
        DocumentEditor {
            doc,
            overlay: HashMap::new(),
            deleted: HashSet::new(),
            next,
            page_order: None,
        }
    }

    /// The document being edited.
    #[must_use]
    pub fn document(&self) -> &CosDocument {
        &self.doc
    }

    /// Whether anything has been changed.
    #[must_use]
    pub fn is_dirty(&self) -> bool {
        !self.overlay.is_empty() || !self.deleted.is_empty() || self.page_order.is_some()
    }

    /// Allocates an unused object number.
    pub fn allocate(&mut self) -> ObjRef {
        let r = ObjRef::new(self.next, 0);
        self.next = self.next.saturating_add(1);
        r
    }

    /// Reads an object, seeing this editor's changes.
    #[must_use]
    pub fn get(&self, r: ObjRef) -> Option<Object> {
        if self.deleted.contains(&r.num) {
            return Some(Object::Null);
        }
        match self.overlay.get(&r.num) {
            Some(Written::Object(object)) => Some(object.clone()),
            Some(Written::Stream(stream)) => Some(Object::Dict(stream.dict.clone())),
            None => self.doc.get(r).ok().map(|o| (*o).clone()),
        }
    }

    /// Replaces an object.
    pub fn put(&mut self, r: ObjRef, object: Object) {
        self.deleted.remove(&r.num);
        self.overlay.insert(r.num, Written::Object(object));
    }

    /// Replaces or adds a stream.
    pub fn put_stream(&mut self, r: ObjRef, stream: StreamData) {
        self.deleted.remove(&r.num);
        self.overlay.insert(r.num, Written::Stream(stream));
    }

    /// Deletes an object.
    ///
    /// Written as null rather than omitted: a reference to a removed object
    /// must resolve to nothing, and leaving the old bytes reachable through an
    /// earlier revision is exactly the mistake that makes "deleted" content
    /// recoverable.
    pub fn delete(&mut self, r: ObjRef) {
        self.overlay.remove(&r.num);
        self.deleted.insert(r.num);
    }

    /// Interns a name in the document's table.
    pub fn intern(&self, bytes: &[u8]) -> Name {
        self.doc.intern(bytes)
    }

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

    /// Draws a page's annotations into its content and removes them (12.5.5).
    ///
    /// Flattening is what makes an annotation permanent: a filled form or a
    /// highlight stops being a separate object a viewer may choose not to
    /// draw, or a user may delete. Only annotations with a normal appearance
    /// are flattened — one without has nothing to draw, and inventing
    /// something for it is the appearance synthesizer's job, not this.
    ///
    /// Returns how many were flattened.
    pub fn flatten_annotations(&mut self, page: u32) -> Option<usize> {
        let reference = self.page_refs().get(page as usize).copied()?;
        let Some(Object::Dict(dict)) = self.get(reference) else {
            return None;
        };

        let annots_key = self.intern(b"Annots");
        let list: Vec<Object> = match dict.get(annots_key) {
            Some(Object::Array(items)) => items.clone(),
            Some(Object::Ref(r)) => self
                .get(*r)
                .and_then(|o| o.as_array().map(<[Object]>::to_vec))
                .unwrap_or_default(),
            _ => return Some(0),
        };

        let mut painted = Vec::new();
        let mut names = Vec::new();
        let mut count = 0usize;

        for entry in &list {
            let Some(annot) = entry
                .as_objref()
                .and_then(|r| self.get(r))
                .and_then(|o| o.as_dict().cloned())
            else {
                continue;
            };

            // 12.5.3: hidden and no-view annotations are not on the page, so
            // flattening must not put them there.
            let flags = annot.get_int(self.intern(b"F")).unwrap_or(0);
            if flags & 2 != 0 || flags & 32 != 0 {
                continue;
            }

            let Some(rect) = self
                .doc
                .resolve_key(&annot, self.intern(b"Rect"))
                .as_array()
                .and_then(Rect::from_array)
            else {
                continue;
            };
            let Some(form) = self.normal_appearance(&annot) else {
                continue;
            };

            // 12.5.5 maps the form's bounding box onto the rectangle. The
            // synthesizer writes them equal, and a form from elsewhere may
            // not, so the scale is computed rather than assumed.
            let (bbox, matrix) = self.appearance_box(form);
            let (sx, sy, tx, ty) = fit(bbox, matrix, rect);

            let name = format!("TpdfFlat{count}");
            names.push((name.clone(), form));
            painted.push(format!("q {sx} 0 0 {sy} {tx} {ty} cm /{name} Do Q\n"));
            count += 1;
        }

        if count == 0 {
            return Some(0);
        }

        // The appearances become ordinary XObject resources of the page.
        let Some(Object::Dict(mut dict)) = self.get(reference) else {
            return None;
        };
        let resources_key = Name::RESOURCES;
        let mut resources = self
            .doc
            .resolve_key(&dict, resources_key)
            .as_dict()
            .cloned()
            .unwrap_or_default();
        let xobject_key = self.intern(b"XObject");
        let mut xobjects = resources
            .get_dict(xobject_key)
            .cloned()
            .unwrap_or_else(Dict::new);
        for (name, form) in names {
            xobjects.insert(self.intern(name.as_bytes()), Object::Ref(form));
        }
        resources.insert(xobject_key, Object::Dict(xobjects));
        dict.insert(resources_key, Object::Dict(resources));

        // The annotations go: a flattened one that stayed would be drawn
        // twice, once as content and once as itself.
        dict = without(&dict, annots_key);
        self.put(reference, Object::Dict(dict));

        self.append_content(page, painted.concat().as_bytes());
        Some(count)
    }

    /// The `/AP` `/N` stream of an annotation, when it has one.
    fn normal_appearance(&self, annot: &Dict) -> Option<ObjRef> {
        let ap = self.doc.resolve_key(annot, self.intern(b"AP"));
        let normal = ap.as_dict()?.get(self.intern(b"N"))?.clone();
        match normal {
            Object::Ref(r) => {
                let object = self.get(r)?;
                let dict = object.as_dict()?;
                if dict.get(self.intern(b"BBox")).is_some() {
                    return Some(r);
                }
                // A dictionary of states: the one `/AS` names.
                let state = annot.get_name(self.intern(b"AS"))?;
                dict.get_ref(state)
            }
            Object::Dict(states) => {
                let state = annot.get_name(self.intern(b"AS"))?;
                states.get_ref(state)
            }
            _ => None,
        }
    }

    /// An appearance stream's `/BBox` and `/Matrix`.
    fn appearance_box(&self, form: ObjRef) -> (Option<Rect>, [f64; 6]) {
        let Some(object) = self.get(form) else {
            return (None, [1.0, 0.0, 0.0, 1.0, 0.0, 0.0]);
        };
        let Some(dict) = object.as_dict() else {
            return (None, [1.0, 0.0, 0.0, 1.0, 0.0, 0.0]);
        };

        let bbox = self
            .doc
            .resolve_key(dict, self.intern(b"BBox"))
            .as_array()
            .and_then(Rect::from_array);

        let matrix = self
            .doc
            .resolve_key(dict, self.intern(b"Matrix"))
            .as_array()
            .map(|a| a.iter().filter_map(Object::as_number).collect::<Vec<f64>>())
            .filter(|v| v.len() >= 6 && v.iter().all(|x| x.is_finite()))
            .map(|v| [v[0], v[1], v[2], v[3], v[4], v[5]])
            .unwrap_or([1.0, 0.0, 0.0, 1.0, 0.0, 0.0]);

        (bbox, matrix)
    }

    /// Adds an annotation to a page (12.5).
    ///
    /// An appearance stream is synthesized and attached unless the dictionary
    /// already carries an `/AP`, because an annotation without one looks like
    /// whatever the viewer decides and prints like nothing at all. Passing an
    /// `/AP` in keeps it untouched; [`crate::appearance::synthesize`] is what
    /// gets called otherwise, and the subtypes it declines are left bare.
    pub fn add_annotation(&mut self, page: u32, annotation: Dict) -> Option<ObjRef> {
        let reference = self.page_refs().get(page as usize).copied()?;
        let Some(Object::Dict(mut dict)) = self.get(reference) else {
            return None;
        };

        let mut annotation = annotation;
        let ap = self.intern(b"AP");
        if !annotation.contains_key(ap) {
            if let Some(stream) = crate::appearance::synthesize(&self.doc, &annotation) {
                let form = self.allocate();
                self.put_stream(form, stream);
                let mut states = Dict::new();
                states.insert(self.intern(b"N"), Object::Ref(form));
                annotation.insert(ap, Object::Dict(states));
            }
        }

        let annot_ref = self.allocate();
        self.put(annot_ref, Object::Dict(annotation));

        let annots = self.intern(b"Annots");
        // /Annots may be direct or indirect; both are read, and the result is
        // written back directly, which is always legal.
        let mut list: Vec<Object> = match dict.get(annots) {
            Some(Object::Array(items)) => items.clone(),
            Some(Object::Ref(r)) => self
                .get(*r)
                .and_then(|o| o.as_array().map(<[Object]>::to_vec))
                .unwrap_or_default(),
            _ => Vec::new(),
        };
        list.push(Object::Ref(annot_ref));
        dict.insert(annots, Object::Array(list));
        self.put(reference, Object::Dict(dict));

        Some(annot_ref)
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
            // document's decoded bytes.
            if let Some(Written::Stream(stream)) = self.overlay.get(&r.num) {
                out.extend_from_slice(&stream.data);
            } else if let Ok(bytes) = self.doc.stream_decoded(r) {
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

    /// The document's form fields (12.7).
    #[must_use]
    pub fn fields(&self) -> Vec<form::Field> {
        form::fields(&self.doc)
    }

    /// Fills a text or choice field, rebuilding its appearance.
    ///
    /// Returns false when there is no such field, when it is read-only, or
    /// when the value is one the field does not accept — a value over
    /// `/MaxLen`, or one a non-editable list does not offer. Refusing is the
    /// point: writing a truncated value would hide a data error inside a file
    /// that then looks correctly filled.
    pub fn set_field_value(&mut self, name: &str, value: &str) -> bool {
        let Some(field) = self.fields().into_iter().find(|f| f.name == name) else {
            return false;
        };
        if !fill::accepts(&field, value) {
            return false;
        }

        let Some(Object::Dict(mut dict)) = self.get(field.reference) else {
            return false;
        };
        dict.insert(self.intern(b"V"), fill::value_object(value));
        self.put(field.reference, Object::Dict(dict));

        let resources = form::default_resources(&self.doc);
        let quadding = fill::quadding(&self.doc, &field);
        let multiline = field.flags & fill::MULTILINE != 0;

        for widget in &field.widgets {
            let Some(rect) = fill::widget_rect(&self.doc, *widget) else {
                continue;
            };
            let da = fill::appearance_string(&self.doc, &field, *widget);
            let stream = fill::text_appearance(
                &self.doc,
                rect,
                value,
                &da,
                quadding,
                multiline,
                resources.as_ref(),
            );

            let form_ref = self.allocate();
            self.put_stream(form_ref, stream);
            self.set_normal_appearance(*widget, Object::Ref(form_ref));
        }

        self.clear_need_appearances();
        true
    }

    /// Turns a checkbox on or off.
    ///
    /// The on state is whatever the widget's appearance dictionary calls it,
    /// which is `/Yes` by convention and something else often enough that
    /// assuming `/Yes` ticks a box the file cannot draw.
    pub fn set_checkbox(&mut self, name: &str, on: bool) -> bool {
        let Some(field) = self
            .fields()
            .into_iter()
            .find(|f| f.name == name && f.kind == form::FieldKind::Checkbox)
        else {
            return false;
        };
        if field.is_read_only() {
            return false;
        }

        let off = self.intern(b"Off");
        let state = if on {
            let Some(widget) = field.widgets.first() else {
                return false;
            };
            match form::on_state(&self.doc, *widget) {
                Some(state) => state,
                // Without an appearance for the on state there is nothing to
                // draw, and setting /V alone would leave a box that reads as
                // ticked and displays as empty.
                None => return false,
            }
        } else {
            off
        };

        let Some(Object::Dict(mut dict)) = self.get(field.reference) else {
            return false;
        };
        dict.insert(self.intern(b"V"), Object::Name(state));
        self.put(field.reference, Object::Dict(dict));

        for widget in &field.widgets {
            self.set_appearance_state(*widget, state);
        }
        self.clear_need_appearances();
        true
    }

    /// Selects one button of a radio group.
    ///
    /// 12.7.4.2: the group holds one value, and every widget's `/AS` follows
    /// it — the one whose appearance offers that state shows on, the rest show
    /// off. Setting only the chosen widget leaves the previous one still
    /// drawn, which is how two options end up looking selected at once.
    pub fn select_radio(&mut self, name: &str, option: &str) -> bool {
        let Some(field) = self
            .fields()
            .into_iter()
            .find(|f| f.name == name && f.kind == form::FieldKind::Radio)
        else {
            return false;
        };
        if field.is_read_only() {
            return false;
        }

        let wanted = self.intern(option.as_bytes());
        let off = self.intern(b"Off");
        let offers = |editor: &DocumentEditor, widget: ObjRef| -> bool {
            editor
                .get(widget)
                .and_then(|o| o.as_dict().cloned())
                .and_then(|d| d.get_dict(editor.intern(b"AP")).cloned())
                .and_then(|ap| ap.get_dict(editor.intern(b"N")).cloned())
                .is_some_and(|states| states.get(wanted).is_some())
        };

        if !field.widgets.iter().any(|w| offers(self, *w)) {
            return false;
        }

        let Some(Object::Dict(mut dict)) = self.get(field.reference) else {
            return false;
        };
        dict.insert(self.intern(b"V"), Object::Name(wanted));
        self.put(field.reference, Object::Dict(dict));

        for widget in &field.widgets {
            let state = if offers(self, *widget) { wanted } else { off };
            self.set_appearance_state(*widget, state);
        }
        self.clear_need_appearances();
        true
    }

    /// Restores every field to its default value (12.7.5.3).
    ///
    /// A field with no `/DV` loses its `/V` entirely rather than gaining an
    /// empty one, because "never filled" and "filled with nothing" are
    /// different states and a submitted form distinguishes them.
    pub fn reset_form(&mut self) {
        for field in self.fields() {
            let Some(Object::Dict(mut dict)) = self.get(field.reference) else {
                continue;
            };
            let v = self.intern(b"V");
            let dv = self.intern(b"DV");

            match dict.get(dv).cloned() {
                Some(default) => {
                    dict.insert(v, default);
                }
                None => {
                    dict = without(&dict, v);
                }
            }
            self.put(field.reference, Object::Dict(dict));

            match field.kind {
                form::FieldKind::Checkbox | form::FieldKind::Radio => {
                    let off = self.intern(b"Off");
                    let state = self
                        .get(field.reference)
                        .and_then(|o| o.as_dict().and_then(|d| d.get_name(self.intern(b"V"))))
                        .unwrap_or(off);
                    for widget in &field.widgets {
                        self.set_appearance_state(*widget, state);
                    }
                }
                _ => {
                    let value = self
                        .get(field.reference)
                        .map(|o| {
                            o.as_dict()
                                .and_then(|d| d.get(self.intern(b"V")).cloned())
                                .map_or(String::new(), |v| {
                                    v.as_string()
                                        .map(|s| crate::text_string::decode_text_string(&s.bytes))
                                        .unwrap_or_default()
                                })
                        })
                        .unwrap_or_default();
                    self.regenerate_text(&field, &value);
                }
            }
        }
        self.clear_need_appearances();
    }

    /// Rewrites a text field's appearance for a value already stored.
    fn regenerate_text(&mut self, field: &form::Field, value: &str) {
        let resources = form::default_resources(&self.doc);
        let quadding = fill::quadding(&self.doc, field);
        let multiline = field.flags & fill::MULTILINE != 0;

        for widget in &field.widgets {
            let Some(rect) = fill::widget_rect(&self.doc, *widget) else {
                continue;
            };
            let da = fill::appearance_string(&self.doc, field, *widget);
            let stream = fill::text_appearance(
                &self.doc,
                rect,
                value,
                &da,
                quadding,
                multiline,
                resources.as_ref(),
            );
            let form_ref = self.allocate();
            self.put_stream(form_ref, stream);
            self.set_normal_appearance(*widget, Object::Ref(form_ref));
        }
    }

    /// Points a widget's `/AP` `/N` at something.
    fn set_normal_appearance(&mut self, widget: ObjRef, normal: Object) {
        let Some(Object::Dict(mut dict)) = self.get(widget) else {
            return;
        };
        let ap = self.intern(b"AP");
        let n = self.intern(b"N");
        let mut states = dict.get_dict(ap).cloned().unwrap_or_else(Dict::new);
        states.insert(n, normal);
        dict.insert(ap, Object::Dict(states));
        // A widget showing a single appearance has no state to select, and a
        // leftover /AS naming one would suppress it entirely.
        let dict = without(&dict, self.intern(b"AS"));
        self.put(widget, Object::Dict(dict));
    }

    /// Selects which of a widget's appearance states is shown.
    fn set_appearance_state(&mut self, widget: ObjRef, state: Name) {
        let Some(Object::Dict(mut dict)) = self.get(widget) else {
            return;
        };
        dict.insert(self.intern(b"AS"), Object::Name(state));
        self.put(widget, Object::Dict(dict));
    }

    /// Drops `/NeedAppearances` now that the appearances are right.
    ///
    /// Leaving it set asks every viewer to throw away what was just written
    /// and rebuild it from its own idea of the field, which is how a correctly
    /// filled form comes out looking different in each one.
    fn clear_need_appearances(&mut self) {
        let Some(catalog) = self.doc.catalog() else {
            return;
        };
        let Some(form_ref) = catalog.get_ref(self.intern(b"AcroForm")) else {
            // A direct /AcroForm cannot be replaced without rewriting the
            // catalog, which is done here rather than skipped.
            let key = self.intern(b"AcroForm");
            let Some(form) = catalog.get_dict(key).cloned() else {
                return;
            };
            let cleaned = without(&form, self.intern(b"NeedAppearances"));
            let Some(root) = self.doc.trailer().get_ref(Name::ROOT) else {
                return;
            };
            let mut updated = (*catalog).clone();
            updated.insert(key, Object::Dict(cleaned));
            self.put(root, Object::Dict(updated));
            return;
        };

        let Some(Object::Dict(form)) = self.get(form_ref) else {
            return;
        };
        let cleaned = without(&form, self.intern(b"NeedAppearances"));
        self.put(form_ref, Object::Dict(cleaned));
    }

    /// Keeps only the objects something reaches from the trailer.
    ///
    /// A mark from the trailer's own references, then a sweep. Without it a
    /// rewrite is a serializer rather than a rewrite: an object detached from
    /// the page tree is still written, still numbered, and still readable by
    /// anyone who scans the file rather than following it — which is how
    /// "deleted" content stays recoverable.
    fn reachable(&self, all: &ObjectSet, trailer: &Dict) -> ObjectSet {
        let mut live: HashSet<u32> = HashSet::new();
        let mut queue: Vec<ObjRef> = Vec::new();

        for (_, value) in trailer.iter() {
            references_of(value, &mut queue);
        }

        while let Some(r) = queue.pop() {
            if !live.insert(r.num) {
                continue;
            }
            let Some(entry) = all.get(r.num) else {
                continue;
            };
            let dict = match entry {
                Written::Object(object) => {
                    references_of(object, &mut queue);
                    continue;
                }
                Written::Stream(stream) => &stream.dict,
            };
            for (_, value) in dict.iter() {
                references_of(value, &mut queue);
            }
        }

        let mut kept = ObjectSet::new();
        for (num, entry) in all.iter() {
            if !live.contains(num) {
                continue;
            }
            match entry {
                Written::Object(object) => kept.insert(*num, object.clone()),
                Written::Stream(stream) => kept.insert_stream(*num, stream.clone()),
            }
        }
        kept
    }

    /// The page tree and page dictionaries a reordering changes.
    ///
    /// Returned rather than applied, because the two save modes build
    /// different object sets and both need them.
    fn page_tree_updates(&self) -> Vec<(u32, Object)> {
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

    /// Saves the edits.
    ///
    /// An incremental save appends only what changed, leaving the original
    /// bytes untouched — the only way to modify a signed document without
    /// breaking the signature over it.
    #[must_use]
    pub fn save(&self, options: &WriteOptions) -> Vec<u8> {
        let mut set = ObjectSet::new();

        for (num, entry) in &self.overlay {
            match entry {
                Written::Object(object) => set.insert(*num, object.clone()),
                Written::Stream(stream) => set.insert_stream(*num, stream.clone()),
            }
        }
        for num in &self.deleted {
            set.insert(*num, Object::Null);
        }

        // A reordered page tree needs its /Kids and /Count rewritten.
        //
        // Computed once and applied to whichever object set the chosen mode
        // builds. Writing it only into the incremental set — which is what
        // this did — meant a *rewrite* silently dropped every page operation:
        // the tree kept its original /Kids, so an inserted page was written
        // into the file and never referenced, and a reordering did nothing at
        // all. Nothing caught it because the page-operation tests all saved
        // incrementally.
        let tree_updates = self.page_tree_updates();
        for (num, object) in &tree_updates {
            set.insert(*num, object.clone());
        }

        let trailer = self.doc.trailer().clone();
        match options.mode {
            WriteMode::Incremental => write::incremental_update(
                self.doc.bytes(),
                &set,
                &trailer,
                self.doc.last_startxref(),
                self.doc.names_table(),
                options.compress,
            ),
            WriteMode::Rewrite => {
                // 7.6.1: a rewrite decrypts on the way through — `stream_raw`
                // hands back plaintext once a decryptor is installed, and the
                // strings were decrypted when they were parsed. So the output
                // is a plaintext file, and carrying `/Encrypt` forward would
                // advertise encryption over it: every reader would then try to
                // decrypt bytes that are already clear and get garbage.
                //
                // Encrypt-on-save is a separate, unbuilt feature. Until it
                // exists, writing a readable file is the honest outcome, and
                // it is what `WriteOptions::encryption` will replace.
                let encrypt = self.intern(b"Encrypt");
                let trailer = without(&trailer, encrypt);
                let encrypt_num = self.doc.trailer().get_ref(encrypt).map(|r| r.num);

                // A rewrite must carry everything, not only the changes.
                let mut all = ObjectSet::new();
                for num in 1..=self.doc.max_object_number() {
                    let r = ObjRef::new(num, 0);
                    if self.deleted.contains(&num) {
                        continue;
                    }
                    // The encryption dictionary itself goes with the entry
                    // that named it; left behind it is an orphan describing a
                    // scheme the file no longer uses.
                    if encrypt_num == Some(num) {
                        continue;
                    }
                    match self.overlay.get(&num) {
                        Some(Written::Object(object)) => all.insert(num, object.clone()),
                        Some(Written::Stream(stream)) => all.insert_stream(num, stream.clone()),
                        None => {
                            if let Ok(object) = self.doc.get(r) {
                                if matches!(object.as_ref(), Object::Null) {
                                    continue;
                                }
                                // A stream must carry its data, which the
                                // parsed object does not hold.
                                if let Object::Stream(stream) = object.as_ref() {
                                    if let Ok(data) = self.doc.stream_raw(r) {
                                        all.insert_stream(
                                            num,
                                            StreamData {
                                                dict: stream.dict.clone(),
                                                data,
                                            },
                                        );
                                        continue;
                                    }
                                }
                                all.insert(num, (*object).clone());
                            }
                        }
                    }
                }
                for (num, entry) in &self.overlay {
                    match entry {
                        Written::Object(object) => all.insert(*num, object.clone()),
                        Written::Stream(stream) => all.insert_stream(*num, stream.clone()),
                    }
                }
                for (num, object) in &tree_updates {
                    all.insert(*num, object.clone());
                }
                if options.garbage_collect {
                    all = self.reachable(&all, &trailer);
                }
                write::rewrite(&all, &trailer, options, self.doc.names_table())
            }
        }
    }
}

/// Building the common annotation types (12.5.6).
pub mod annot {
    use super::{Dict, Name, Object, PdfString, Rect};
    use crate::doc::CosDocument;

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

    /// A link to a page in the same document.
    #[must_use]
    pub fn link(doc: &CosDocument, rect: Rect, page: crate::object::ObjRef) -> Dict {
        let mut dict = base(doc, b"Link", rect);
        // Ruling 6: an explicit destination, never a name that resembles one.
        dict.insert(
            doc.intern(b"Dest"),
            Object::Array(vec![Object::Ref(page), Object::Name(doc.intern(b"Fit"))]),
        );
        // A visible border on a link is rarely wanted and always ugly.
        dict.insert(
            doc.intern(b"Border"),
            Object::Array(vec![Object::Int(0), Object::Int(0), Object::Int(0)]),
        );
        dict
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::build::DocumentBuilder;

    fn document(pages: usize) -> Arc<CosDocument> {
        let mut builder = DocumentBuilder::new();
        builder.add_base_font(b"F0", b"Helvetica");
        for i in 0..pages {
            builder.add_page(200.0, 100.0, |page| {
                page.text(b"F0", 12.0, 10.0, 50.0, &format!("page {i}"));
            });
        }
        Arc::new(CosDocument::open(builder.finish()).expect("it opens"))
    }

    fn reopen(editor: &DocumentEditor, mode: WriteMode) -> CosDocument {
        let bytes = editor.save(&WriteOptions {
            mode,
            ..WriteOptions::default()
        });
        CosDocument::open(bytes).expect("the saved document opens")
    }

    #[test]
    fn an_untouched_editor_is_not_dirty() {
        let editor = DocumentEditor::new(document(2));
        assert!(!editor.is_dirty());
        assert_eq!(editor.page_refs().len(), 2);
    }

    #[test]
    fn deleting_a_page_removes_it() {
        let mut editor = DocumentEditor::new(document(3));
        assert!(editor.delete_page(1));
        assert!(editor.is_dirty());

        let saved = reopen(&editor, WriteMode::Incremental);
        assert_eq!(pages::count(&saved), 2);

        // The page that remains in the middle is the one that was third.
        let text = pages::content_bytes(&saved, &pages::collect(&saved)[1]);
        assert!(String::from_utf8_lossy(&text).contains("page 2"));
    }

    #[test]
    fn moving_a_page_reorders_it() {
        let mut editor = DocumentEditor::new(document(3));
        assert!(editor.move_page(2, 0));

        let saved = reopen(&editor, WriteMode::Incremental);
        let first = pages::content_bytes(&saved, &pages::collect(&saved)[0]);
        assert!(
            String::from_utf8_lossy(&first).contains("page 2"),
            "the last page is now first"
        );
        assert_eq!(pages::count(&saved), 3, "and none were lost");
    }

    #[test]
    fn rotating_a_page_accumulates_and_normalizes() {
        let mut editor = DocumentEditor::new(document(1));
        assert!(editor.rotate_page(0, 90));
        assert!(editor.rotate_page(0, 90));

        let saved = reopen(&editor, WriteMode::Incremental);
        assert_eq!(pages::collect(&saved)[0].rotation, 180);

        // And a further turn wraps rather than growing.
        let mut editor = DocumentEditor::new(Arc::new(saved));
        assert!(editor.rotate_page(0, 270));
        let saved = reopen(&editor, WriteMode::Incremental);
        assert_eq!(pages::collect(&saved)[0].rotation, 90);
    }

    #[test]
    fn out_of_range_page_operations_are_refused() {
        let mut editor = DocumentEditor::new(document(2));
        assert!(!editor.delete_page(9));
        assert!(!editor.move_page(0, 9));
        assert!(!editor.move_page(9, 0));
        assert!(!editor.rotate_page(9, 90));
        assert!(!editor.append_content(9, b"0 0 1 1 re f"));
        assert!(!editor.is_dirty(), "a refused edit changes nothing");
    }

    #[test]
    fn an_incremental_save_keeps_the_original_bytes() {
        let doc = document(2);
        let original = doc.bytes().to_vec();

        let mut editor = DocumentEditor::new(doc);
        editor.rotate_page(0, 90);
        let saved = editor.save(&WriteOptions {
            mode: WriteMode::Incremental,
            ..WriteOptions::default()
        });

        assert!(
            saved.starts_with(&original),
            "the signable prefix must survive an edit"
        );
    }

    #[test]
    fn appended_content_does_not_disturb_what_was_there() {
        let mut editor = DocumentEditor::new(document(1));
        assert!(editor.append_content(0, b"1 0 0 rg 10 10 50 20 re f"));

        let saved = reopen(&editor, WriteMode::Incremental);
        let content = pages::content_bytes(&saved, &pages::collect(&saved)[0]);
        let text = String::from_utf8_lossy(&content);

        assert!(text.contains("page 0"), "the original drawing survives");
        assert!(text.contains("1 0 0 rg"), "and the addition is present");
        assert!(
            text.matches('q').count() >= 2,
            "each part is bracketed so neither can disturb the other"
        );
    }

    #[test]
    fn annotations_reach_the_page() {
        let doc = document(1);
        let mut editor = DocumentEditor::new(doc.clone());

        let rect = Rect {
            x0: 10.0,
            y0: 10.0,
            x1: 60.0,
            y1: 30.0,
        };
        let note = annot::text_note(&doc, rect, "a remark", false);
        assert!(editor.add_annotation(0, note).is_some());

        let saved = reopen(&editor, WriteMode::Incremental);
        let page = pages::collect(&saved)[0].reference;
        let object = saved.get(page).expect("the page loads");
        let annots = object
            .as_dict()
            .and_then(|d| d.get_array(saved.intern(b"Annots")))
            .map(<[Object]>::to_vec)
            .unwrap_or_default();

        assert_eq!(annots.len(), 1, "one annotation");
        let annot_ref = annots[0].as_objref().expect("a reference");
        let annot = saved.get(annot_ref).expect("it loads");
        let dict = annot.as_dict().expect("a dictionary");

        let subtype = dict
            .get(saved.intern(b"Subtype"))
            .and_then(Object::as_name)
            .and_then(|n| saved.name_bytes(n));
        assert_eq!(subtype.as_deref(), Some(b"Text".as_slice()));
        assert_eq!(
            dict.get_int(saved.intern(b"F")),
            Some(4),
            "the print flag is set, or it will not appear on paper"
        );
    }

    /// An annotation without an appearance stream is drawn by guesswork, and
    /// viewers guess differently. One is attached on the way in, and it has to
    /// survive being written and read back as a real form.
    #[test]
    fn an_annotation_is_given_an_appearance_stream() {
        let doc = document(1);
        let mut editor = DocumentEditor::new(doc.clone());

        let rect = Rect {
            x0: 10.0,
            y0: 10.0,
            x1: 60.0,
            y1: 30.0,
        };
        let square = annot::square(
            &doc,
            rect,
            annot::Color {
                r: 1.0,
                g: 0.0,
                b: 0.0,
            },
            2.0,
        );
        editor.add_annotation(0, square).expect("it is added");

        let saved = reopen(&editor, WriteMode::Incremental);
        let page = pages::collect(&saved)[0].reference;
        let annot_ref = saved
            .get(page)
            .ok()
            .and_then(|o| o.as_dict().cloned())
            .and_then(|d| d.get_array(saved.intern(b"Annots")).map(<[Object]>::to_vec))
            .and_then(|a| a.first().and_then(Object::as_objref))
            .expect("the annotation is on the page");

        let annot = saved.get(annot_ref).expect("it loads");
        let form_ref = annot
            .as_dict()
            .and_then(|d| d.get_dict(saved.intern(b"AP")))
            .and_then(|ap| ap.get_ref(saved.intern(b"N")))
            .expect("with a normal appearance");

        let form = saved.get(form_ref).expect("the form loads");
        let form_dict = form.as_dict().expect("a stream dictionary");
        assert_eq!(
            form_dict
                .get_name(saved.intern(b"Subtype"))
                .and_then(|n| saved.name_bytes(n))
                .as_deref(),
            Some(b"Form".as_slice()),
            "the appearance is a form XObject"
        );

        let content = saved.stream_decoded(form_ref).expect("its content decodes");
        let text = String::from_utf8_lossy(&content);
        assert!(text.contains(" re"), "which draws the square: {text}");
    }

    /// An appearance supplied by the caller is theirs, not ours to replace.
    #[test]
    fn a_supplied_appearance_is_left_alone() {
        let doc = document(1);
        let mut editor = DocumentEditor::new(doc.clone());

        let rect = Rect {
            x0: 10.0,
            y0: 10.0,
            x1: 60.0,
            y1: 30.0,
        };
        let mut square = annot::square(
            &doc,
            rect,
            annot::Color {
                r: 1.0,
                g: 0.0,
                b: 0.0,
            },
            2.0,
        );
        let mine = ObjRef::new(4242, 0);
        let mut ap = Dict::new();
        ap.insert(doc.intern(b"N"), Object::Ref(mine));
        square.insert(doc.intern(b"AP"), Object::Dict(ap));

        let annot_ref = editor.add_annotation(0, square).expect("it is added");
        let kept = editor
            .get(annot_ref)
            .and_then(|o| o.as_dict().cloned())
            .and_then(|d| d.get_dict(doc.intern(b"AP")).cloned())
            .and_then(|ap| ap.get_ref(doc.intern(b"N")))
            .expect("the appearance survives");
        assert_eq!(kept, mine);
    }

    #[test]
    fn a_highlights_quadpoints_follow_the_specifications_order() {
        let doc = document(1);
        // Upper-left, upper-right, lower-left, lower-right.
        let quad = [10.0, 30.0, 60.0, 30.0, 10.0, 10.0, 60.0, 10.0];
        let dict = annot::highlight(
            &doc,
            &[quad],
            annot::Color {
                r: 1.0,
                g: 1.0,
                b: 0.0,
            },
        );

        let points = dict
            .get_array(doc.intern(b"QuadPoints"))
            .expect("quad points");
        assert_eq!(points.len(), 8);
        assert_eq!(points[0].as_number(), Some(10.0));
        assert_eq!(points[1].as_number(), Some(30.0), "upper edge first");

        // The rectangle encloses the quad.
        let rect = dict.get_array(doc.intern(b"Rect")).expect("a rect");
        assert_eq!(rect[0].as_number(), Some(10.0));
        assert_eq!(rect[3].as_number(), Some(30.0));
    }

    #[test]
    fn a_deleted_object_reads_as_null_afterwards() {
        let doc = document(1);
        let mut editor = DocumentEditor::new(doc);
        let victim = editor.allocate();
        editor.put(victim, Object::Int(5));
        assert_eq!(editor.get(victim), Some(Object::Int(5)));

        editor.delete(victim);
        assert_eq!(
            editor.get(victim),
            Some(Object::Null),
            "a deleted object must not read as its old value"
        );
    }

    #[test]
    fn a_rewrite_carries_everything_not_only_the_changes() {
        let mut editor = DocumentEditor::new(document(2));
        editor.rotate_page(0, 90);

        let saved = reopen(&editor, WriteMode::Rewrite);
        assert_eq!(pages::count(&saved), 2, "both pages survive a rewrite");
        assert_eq!(pages::collect(&saved)[0].rotation, 90);

        let content = pages::content_bytes(&saved, &pages::collect(&saved)[1]);
        assert!(
            String::from_utf8_lossy(&content).contains("page 1"),
            "including content streams, whose data lives outside the object"
        );
    }

    /// A form with a text field, a checkbox whose on state is not `/Yes`, and
    /// a radio pair — the shapes that trip a naive filler.
    fn form_document() -> Arc<CosDocument> {
        let bytes: &[u8] = b"%PDF-1.7
1 0 obj
<< /Type /Catalog /Pages 2 0 R /AcroForm << /Fields [10 0 R 20 0 R 30 0 R]
   /NeedAppearances true /DA (/Helv 0 Tf 0 g)
   /DR << /Font << /Helv 5 0 R >> >> >> >>
endobj
2 0 obj
<< /Type /Pages /Count 1 /Kids [3 0 R] >>
endobj
3 0 obj
<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200]
   /Annots [10 0 R 20 0 R 31 0 R 32 0 R] >>
endobj
5 0 obj
<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>
endobj
10 0 obj
<< /FT /Tx /T (name) /Rect [10 150 190 170] /Subtype /Widget /Type /Annot
   /MaxLen 10 >>
endobj
20 0 obj
<< /FT /Btn /T (agree) /Rect [10 120 30 140] /Subtype /Widget /Type /Annot
   /AP << /N << /On 21 0 R /Off 22 0 R >> >> >>
endobj
21 0 obj
<< /Type /XObject /Subtype /Form /BBox [0 0 20 20] /Length 0 >>
stream

endstream
endobj
22 0 obj
<< /Type /XObject /Subtype /Form /BBox [0 0 20 20] /Length 0 >>
stream

endstream
endobj
30 0 obj
<< /FT /Btn /Ff 32768 /T (colour) /Kids [31 0 R 32 0 R] >>
endobj
31 0 obj
<< /Parent 30 0 R /Subtype /Widget /Type /Annot /Rect [10 90 30 110] /AS /Off
   /AP << /N << /red 21 0 R /Off 22 0 R >> >> >>
endobj
32 0 obj
<< /Parent 30 0 R /Subtype /Widget /Type /Annot /Rect [40 90 60 110] /AS /Off
   /AP << /N << /blue 21 0 R /Off 22 0 R >> >> >>
endobj
trailer
<< /Size 33 /Root 1 0 R >>
%%EOF
";
        Arc::new(CosDocument::open(bytes).expect("the form opens"))
    }

    fn field_named(doc: &CosDocument, name: &str) -> form::Field {
        form::fields(doc)
            .into_iter()
            .find(|f| f.name == name)
            .expect("the field is there")
    }

    /// The half that usually gets skipped: the value must come with an
    /// appearance, or the field shows filled in some viewers and blank in the
    /// rest.
    #[test]
    fn filling_a_text_field_writes_a_value_and_an_appearance() {
        let doc = form_document();
        let mut editor = DocumentEditor::new(Arc::clone(&doc));
        assert!(editor.set_field_value("name", "Ada"));

        let saved = reopen(&editor, WriteMode::Incremental);
        let field = field_named(&saved, "name");
        assert_eq!(field.value, form::FieldValue::Text("Ada".to_string()));

        let widget = saved.get(field.widgets[0]).expect("the widget loads");
        let form_ref = widget
            .as_dict()
            .and_then(|d| d.get_dict(saved.intern(b"AP")))
            .and_then(|ap| ap.get_ref(saved.intern(b"N")))
            .expect("an appearance was written");
        let content = saved.stream_decoded(form_ref).expect("it decodes");
        let text = String::from_utf8_lossy(&content);
        assert!(text.contains("(Ada) Tj"), "which draws the value: {text}");
        assert!(text.starts_with("/Tx BMC"), "marked as a field appearance");
    }

    /// Leaving `/NeedAppearances` set asks every viewer to throw away what was
    /// just written and rebuild it from its own idea of the field.
    #[test]
    fn filling_clears_the_rebuild_request() {
        let doc = form_document();
        assert!(form::needs_appearances(&doc), "the fixture sets it");

        let mut editor = DocumentEditor::new(Arc::clone(&doc));
        editor.set_field_value("name", "Ada");
        let saved = reopen(&editor, WriteMode::Incremental);
        assert!(!form::needs_appearances(&saved));
    }

    #[test]
    fn a_value_the_field_refuses_changes_nothing() {
        let doc = form_document();
        let mut editor = DocumentEditor::new(Arc::clone(&doc));

        assert!(
            !editor.set_field_value("name", "far too long for ten"),
            "over /MaxLen"
        );
        assert!(!editor.set_field_value("nonesuch", "x"), "no such field");
        assert!(!editor.is_dirty(), "and nothing was written");
    }

    /// `/Yes` is a convention, not a rule, and assuming it ticks a box the
    /// file has no appearance for.
    #[test]
    fn a_checkbox_uses_the_on_state_its_appearance_declares() {
        let doc = form_document();
        let mut editor = DocumentEditor::new(Arc::clone(&doc));
        assert!(editor.set_checkbox("agree", true));

        let saved = reopen(&editor, WriteMode::Incremental);
        let field = field_named(&saved, "agree");
        assert_eq!(field.value, form::FieldValue::State("On".to_string()));

        let widget = saved.get(field.widgets[0]).expect("the widget");
        let state = widget
            .as_dict()
            .and_then(|d| d.get_name(saved.intern(b"AS")))
            .and_then(|n| saved.name_bytes(n));
        assert_eq!(
            state.as_deref(),
            Some(b"On".as_slice()),
            "the shown state follows the value"
        );
    }

    #[test]
    fn a_checkbox_turned_off_shows_off() {
        let doc = form_document();
        let mut editor = DocumentEditor::new(Arc::clone(&doc));
        editor.set_checkbox("agree", true);
        assert!(editor.set_checkbox("agree", false));

        let saved = reopen(&editor, WriteMode::Incremental);
        let field = field_named(&saved, "agree");
        assert!(!field.value.is_on());
    }

    /// Setting only the chosen widget leaves the previous one still drawn,
    /// which is how two radio options end up looking selected at once.
    #[test]
    fn selecting_a_radio_turns_its_siblings_off() {
        let doc = form_document();
        let mut editor = DocumentEditor::new(Arc::clone(&doc));
        assert!(editor.select_radio("colour", "blue"));

        let saved = reopen(&editor, WriteMode::Incremental);
        let field = field_named(&saved, "colour");
        assert_eq!(field.value, form::FieldValue::State("blue".to_string()));

        let state_of = |widget: ObjRef| -> String {
            saved
                .get(widget)
                .ok()
                .and_then(|o| o.as_dict().and_then(|d| d.get_name(saved.intern(b"AS"))))
                .and_then(|n| saved.name_bytes(n))
                .map(|b| String::from_utf8_lossy(&b).into_owned())
                .unwrap_or_default()
        };
        assert_eq!(state_of(field.widgets[0]), "Off", "the red one is off");
        assert_eq!(state_of(field.widgets[1]), "blue", "the blue one is on");
    }

    #[test]
    fn selecting_a_radio_option_that_does_not_exist_is_refused() {
        let doc = form_document();
        let mut editor = DocumentEditor::new(Arc::clone(&doc));
        assert!(!editor.select_radio("colour", "green"));
        assert!(!editor.is_dirty());
    }

    /// A field that was never filled and one filled with nothing are
    /// different states, and a submitted form distinguishes them.
    #[test]
    fn resetting_removes_a_value_that_has_no_default() {
        let doc = form_document();
        let mut editor = DocumentEditor::new(Arc::clone(&doc));
        editor.set_field_value("name", "Ada");
        editor.set_checkbox("agree", true);
        editor.reset_form();

        let saved = reopen(&editor, WriteMode::Incremental);
        assert_eq!(field_named(&saved, "name").value, form::FieldValue::None);
        assert!(!field_named(&saved, "agree").value.is_on());
    }
}

#[cfg(test)]
mod gc_tests {
    use super::*;
    use crate::build::DocumentBuilder;

    fn document_with_orphan() -> (Arc<CosDocument>, ObjRef) {
        let mut builder = DocumentBuilder::new();
        builder.add_page(100.0, 100.0, |page| {
            page.fill_rect(0.0, 0.0, 10.0, 10.0, 0.0);
        });
        let doc = Arc::new(CosDocument::open(builder.finish()).expect("it opens"));

        let mut editor = DocumentEditor::new(Arc::clone(&doc));
        let orphan = editor.allocate();
        editor.put_stream(
            orphan,
            StreamData {
                dict: Dict::new(),
                data: b"THIS-IS-UNREFERENCED".to_vec(),
            },
        );
        let saved = editor.save(&WriteOptions {
            mode: WriteMode::Rewrite,
            ..WriteOptions::default()
        });
        (
            Arc::new(CosDocument::open(saved).expect("it reopens")),
            orphan,
        )
    }

    /// The default is unchanged: a rewrite serializes everything, which is
    /// what callers that overwrite objects in place depend on.
    #[test]
    fn a_rewrite_keeps_orphans_by_default() {
        let (doc, orphan) = document_with_orphan();
        let editor = DocumentEditor::new(Arc::clone(&doc));
        let saved = editor.save(&WriteOptions {
            mode: WriteMode::Rewrite,
            ..WriteOptions::default()
        });
        let text = String::from_utf8_lossy(&saved).into_owned();
        assert!(
            text.contains("THIS-IS-UNREFERENCED"),
            "orphan {orphan:?} kept"
        );
    }

    /// With collection on, nothing the trailer cannot reach survives. An
    /// object detached from the page tree is still readable by anyone who
    /// scans the file rather than following it.
    #[test]
    fn collection_drops_what_nothing_references() {
        let (doc, _) = document_with_orphan();
        let editor = DocumentEditor::new(Arc::clone(&doc));
        let saved = editor.save(&WriteOptions {
            mode: WriteMode::Rewrite,
            garbage_collect: true,
            ..WriteOptions::default()
        });

        let text = String::from_utf8_lossy(&saved).into_owned();
        assert!(
            !text.contains("THIS-IS-UNREFERENCED"),
            "the orphan survived collection"
        );

        // And the document is still whole.
        let reopened = CosDocument::open(saved).expect("it reopens");
        let pages = pages::collect(&reopened);
        assert_eq!(pages.len(), 1);
        assert!(!pages::content_bytes(&reopened, &pages[0]).is_empty());
    }

    /// Collection must follow references through arrays and nested
    /// dictionaries, not only through the trailer's direct entries.
    #[test]
    fn collection_follows_references_through_nesting() {
        let mut builder = DocumentBuilder::new();
        builder.add_base_font(b"F0", b"Helvetica");
        builder.add_page(100.0, 100.0, |page| {
            page.text(b"F0", 12.0, 10.0, 50.0, "reachable");
        });
        let doc = Arc::new(CosDocument::open(builder.finish()).expect("it opens"));

        let editor = DocumentEditor::new(Arc::clone(&doc));
        let saved = editor.save(&WriteOptions {
            mode: WriteMode::Rewrite,
            garbage_collect: true,
            ..WriteOptions::default()
        });

        let reopened = CosDocument::open(saved).expect("it reopens");
        let pages = pages::collect(&reopened);
        assert_eq!(pages.len(), 1, "the page tree survived");
        // The font is reached only through the page's /Resources /Font.
        let resources = pages[0].resources.as_ref().expect("resources survived");
        let fonts = reopened.resolve_key(resources, reopened.intern(b"Font"));
        assert!(
            fonts.as_dict().is_some_and(|d| !d.is_empty()),
            "a font reached through two levels of nesting was collected"
        );
    }
}
