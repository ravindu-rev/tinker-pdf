//! Serialising the edits: an incremental update or a whole rewrite.

use std::collections::HashSet;

use super::{without, DocumentEditor};
use crate::object::{Dict, ObjRef, Object};
use crate::write::{self, ObjectSet, StreamData, WriteMode, WriteOptions, Written};

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

impl DocumentEditor {
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

    /// Saves the edits.
    ///
    /// An incremental save appends only what changed, leaving the original
    /// bytes untouched — the only way to modify a signed document without
    /// breaking the signature over it.
    #[must_use]
    pub fn save(&self, options: &WriteOptions) -> Vec<u8> {
        let set = self.changed_set();

        // A reordered page tree needs its /Kids and /Count rewritten.
        //
        // Computed once and applied to whichever object set the chosen mode
        // builds. Writing it only into the incremental set — which is what
        // this did — meant a *rewrite* silently dropped every page operation:
        // the tree kept its original /Kids, so an inserted page was written
        // into the file and never referenced, and a reordering did nothing at
        // all. Nothing caught it because the page-operation tests all saved
        // incrementally.
        //
        // `changed_set` has already folded these into the incremental set;
        // the binding stays because the rewrite arm below needs them too.
        let tree_updates = self.page_tree_updates();

        // The document's trailer with this editor's entries over it, for both
        // modes: an `/Info` created here is named by it or by nothing.
        let trailer = self.merged_trailer();
        match options.mode {
            WriteMode::Incremental => {
                // 7.6.2: the update is sealed with the key the document was
                // opened with, because it appends into a file whose /Encrypt
                // still stands. An unencrypted document, or one never
                // authenticated, has no key and writes in the clear as before.
                let key = self.doc.file_key();
                let cipher = key.as_ref().map(|key| write::InheritedCipher { key });
                write::incremental_update(
                    self.doc.bytes(),
                    &set,
                    &trailer,
                    self.doc.last_startxref(),
                    self.doc.names_table(),
                    options.compress,
                    cipher.as_ref().map(|c| c as &dyn write::ObjectCipher),
                )
            }
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
                //
                // Over the object numbers the document **has**, not over the
                // range its numbering allows. This walked `1..=max_object_
                // number()`, which is the same thing for almost every file and
                // two thousand million lookups for
                // `pdfjs/test/pdfs/bug1980958.pdf` — 219 bytes, four objects,
                // the last of them numbered `i32::MAX`. A save over it had not
                // returned after three minutes. The reader was hardened
                // against exactly this shape years ago (`limits::MAX_XREF_
                // SLOTS`, dense below the cap and a map above it); the editor
                // never was, and nothing noticed because the corpus runner
                // reads a hang as a slow file.
                //
                // `XrefTable::iter` yields ascending object numbers across
                // both halves of that table, so the objects are visited in the
                // same order as before and the bytes a rewrite produces are
                // unchanged for every file whose numbering is dense.
                let mut all = ObjectSet::new();
                for (num, _) in self.doc.xref().iter() {
                    if num == 0 {
                        // Object zero is the head of the free list, never a
                        // real object; the old range started at one.
                        continue;
                    }
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

    /// The overlay, deletions and page-tree rewrites as one object set.
    ///
    /// Factored out of [`DocumentEditor::save`] so that signing writes exactly
    /// the same objects an ordinary incremental save would, rather than a
    /// second assembly of them that could drift.
    pub(super) fn changed_set(&self) -> ObjectSet {
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
        for (num, object) in &self.page_tree_updates() {
            set.insert(*num, object.clone());
        }
        set
    }
}

#[cfg(test)]
mod gc_tests {
    use std::sync::Arc;

    use super::*;
    use crate::build::DocumentBuilder;
    use crate::doc::CosDocument;
    use crate::pages;

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
