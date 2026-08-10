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

    /// Adds an annotation to a page (12.5).
    ///
    /// The dictionary is written as given; [`annot`] builds the common kinds
    /// with their appearance streams.
    pub fn add_annotation(&mut self, page: u32, annotation: Dict) -> Option<ObjRef> {
        let reference = self.page_refs().get(page as usize).copied()?;
        let Some(Object::Dict(mut dict)) = self.get(reference) else {
            return None;
        };

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
        if let Some(order) = &self.page_order {
            if let Some(catalog) = self.doc.catalog() {
                if let Some(tree_ref) = catalog.get_ref(Name::PAGES) {
                    if let Some(Object::Dict(mut tree)) = self.get(tree_ref) {
                        tree.insert(
                            Name::KIDS,
                            Object::Array(order.iter().map(|r| Object::Ref(*r)).collect()),
                        );
                        tree.insert(Name::COUNT, Object::Int(order.len() as i64));
                        set.insert(tree_ref.num, Object::Dict(tree));
                    }
                }
            }
        }

        let trailer = self.doc.trailer().clone();
        match options.mode {
            WriteMode::Incremental => write::incremental_update(
                self.doc.bytes(),
                &set,
                &trailer,
                self.doc.last_startxref(),
                self.doc.names_table(),
            ),
            WriteMode::Rewrite => {
                // A rewrite must carry everything, not only the changes.
                let mut all = ObjectSet::new();
                for num in 1..=self.doc.max_object_number() {
                    let r = ObjRef::new(num, 0);
                    if self.deleted.contains(&num) {
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
}
