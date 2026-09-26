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
//!
//! The editor's state and the primitives every edit is made of live here; the
//! operations are grouped by concern in child modules, each adding its own
//! `impl DocumentEditor` block. A child module sees the private fields, so an
//! operation added later is a new file rather than a longer one.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use crate::doc::CosDocument;
use crate::name::Name;
use crate::object::{Dict, ObjRef, Object};
use crate::write::{StreamData, Written};

pub mod annot;
mod annotations;
mod forms;
mod overlay;
mod page_ops;
mod save;
mod signing;
#[cfg(test)]
mod tests;
mod trees;

pub use forms::{FillError, FillRejection, SkippedWidget, WidgetDefect};

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
    /// The next unused object number.
    ///
    /// **Restored by a rollback**, so an abandoned edit hands its numbers
    /// back. The alternative was considered and rejected: numbers allocated
    /// inside a transaction that rolls back are not in the overlay afterwards,
    /// so nothing is ever written at them, and leaving `next` advanced burns a
    /// number per allocation for the life of the editor. A form whose
    /// calculation is attempted and abandoned on every keystroke would climb
    /// through the number space, and each gap splits the appended
    /// cross-reference table into another subsection (7.5.4) and lifts the
    /// trailer's `/Size`, so the file grows on every *failed* edit -- which is
    /// the one edit that is supposed to cost nothing. It also contradicts the
    /// rule the page operations already keep: a refused edit leaves the editor
    /// genuinely unchanged, not merely unchanged in content.
    ///
    /// The cost is that a number **is reused**. An [`ObjRef`] handed out
    /// inside a rolled-back transaction -- by [`DocumentEditor::allocate`],
    /// [`DocumentEditor::insert_page`], [`DocumentEditor::import_page`] or
    /// [`DocumentEditor::add_annotation`] -- is void the moment the rollback
    /// happens, and a later allocation may give the same number to something
    /// else. Holding one across the boundary therefore addresses a different
    /// object rather than nothing. That is the same contract a database gives
    /// for a row id from a rolled-back transaction, and it is stated here
    /// because the alternative failure -- a reference that silently resolves
    /// to nothing -- is not obviously better and costs a file that grows.
    next: u32,
    /// The page order, as references, once it has been disturbed.
    page_order: Option<Vec<ObjRef>>,
}

/// Everything a rollback restores: an editor's state, taken as a value.
///
/// Exhaustive by construction: [`DocumentEditor`] holds these four fields and
/// one more -- the `Arc<CosDocument>` it overlays, which is immutable and
/// therefore has nothing to restore. A field added to the editor without being
/// added here is a silent hole in every transaction, which is why the two
/// declarations sit next to each other.
///
/// **A value, not an open transaction.** This is what
/// [`DocumentEditor::transaction`] uses internally, made public so the same
/// semantics can be had without a closure -- which is what a foreign-function
/// boundary needs, because closures do not cross one (ruling 11:
/// `docs/design/bindings-write.md`). The distinction that keeps that safe is
/// that there is no *state* here to misuse. Taking one changes nothing;
/// dropping one commits nothing, because nothing was pending;
/// [`DocumentEditor::restore`] is idempotent, so restoring twice is restoring
/// once. The failure a `begin`/`commit`/`rollback` triple has -- an editor
/// left in a condition a later reader cannot classify -- has no spelling here.
///
/// The fields stay private. A checkpoint is meaningful only to the editor it
/// came from, and this crate does not promise which four things an editor
/// keeps.
///
/// Restoring a checkpoint into a *different* editor is not checked and not
/// meaningful: object numbers are relative to the document, so the result is
/// an overlay addressing objects of another file. Nothing panics -- ruling 1
/// binds this crate -- and nothing else is promised.
#[derive(Clone)]
pub struct EditCheckpoint {
    overlay: HashMap<u32, Written>,
    deleted: HashSet<u32>,
    next: u32,
    page_order: Option<Vec<ObjRef>>,
}

impl core::fmt::Debug for EditCheckpoint {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        // The overlay's contents are a document's objects, which is not what
        // a caller printing a checkpoint wants to see; the sizes are.
        f.debug_struct("EditCheckpoint")
            .field("written", &self.overlay.len())
            .field("deleted", &self.deleted.len())
            .field("next", &self.next)
            .field("page_order_disturbed", &self.page_order.is_some())
            .finish()
    }
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

    /// The document being edited, as the shared handle it was opened with.
    ///
    /// For a caller that must build something *over* the document while
    /// holding the editor — a page's resources, an interpretation of a content
    /// stream — where a borrow of [`DocumentEditor::document`] would pin the
    /// editor for as long as that thing lived.
    #[must_use]
    pub fn shared_document(&self) -> Arc<CosDocument> {
        Arc::clone(&self.doc)
    }

    /// The decoded bytes of a stream **as this editor now has it**.
    ///
    /// The editor's own overlay first, the document underneath. That order is
    /// the whole point: a caller reading a content stream it has already
    /// rewritten must see the rewrite, and [`CosDocument::stream_decoded`]
    /// cannot show it one — it reads the file, which still says what it said
    /// before the edit. A pass that rewrites content and then walks it (the
    /// glyph-usage walk font subsetting does after a redaction) reads the
    /// *original* text through the document and would put back exactly what
    /// the redaction removed.
    ///
    /// `None` when the object is neither an overlay stream nor a decodable
    /// stream in the file — including one this editor has deleted.
    #[must_use]
    pub fn stream_bytes(&self, r: ObjRef) -> Option<Vec<u8>> {
        if self.deleted.contains(&r.num) {
            return None;
        }
        match self.overlay.get(&r.num) {
            // A stream written here is normally plain; one copied in from a
            // file (`import_page`) keeps the bytes and the `/Filter` the file
            // stored, and is decoded as the file's own would be. Handing back
            // the stored bytes spliced deflate output into a page as
            // operators the first time `append_content` met an imported page.
            Some(Written::Stream(stream)) if stream.dict.get(Name::FILTER).is_some() => {
                let mut sink = crate::warn::WarningSink::new();
                sink.set_context(Some(r));
                let decoded = self
                    .doc
                    .decode_with(&stream.data, &stream.dict, r.num, &mut sink);
                self.doc.absorb(sink);
                decoded.ok()
            }
            Some(Written::Stream(stream)) => Some(stream.data.clone()),
            // An overlay entry that is not a stream replaced the stream with
            // something else, and the file's bytes are no longer what this
            // object is.
            Some(Written::Object(_)) => None,
            None => self.doc.stream_decoded(r).ok(),
        }
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

    /// Runs `body` as one edit: everything it changes lands together, or
    /// nothing does.
    ///
    /// `Err` out of `body` rolls the editor back to exactly what it was before
    /// the call -- objects written, objects deleted, the page order, and the
    /// object-number counter (see [`DocumentEditor`]'s `next` for why that
    /// last one is restored and what it costs). `Ok` keeps everything, and the
    /// error type is the caller's, so a transaction composes with whatever
    /// result the work already produces.
    ///
    /// A closure rather than a `begin`/`commit`/`rollback` triple, because
    /// this API's whole problem is silent misuse. There is no way to leave
    /// this scope without either committing or rolling back: no forgotten
    /// `commit`, no `?` that returns past a `rollback`, and no editor left in
    /// a state where a later reader cannot tell whether a transaction is open.
    /// The one thing a manual triple buys -- a transaction whose lifetime
    /// crosses a function boundary -- is not something a document edit needs,
    /// and is exactly the shape that leaves an edit half applied.
    ///
    /// **This is sugar over [`DocumentEditor::checkpoint`] and
    /// [`DocumentEditor::restore`]**, and the two forms run the same two
    /// functions -- so the contract in the paragraph above is inherited by
    /// the pair rather than re-proven for it. The pair exists because a
    /// closure does not cross a foreign-function boundary and this is the API
    /// every binding must project (ruling 11). It is *not* the triple this
    /// paragraph rejects: a checkpoint is a value, so there is no open state
    /// to forget to close. What a caller of the pair gives up is the
    /// guarantee that it is called at all, which is why the managed bindings
    /// ship the closure sugar in their own languages and this stays the Rust
    /// one.
    ///
    /// Nesting works and means what it says: an inner rollback restores the
    /// inner start, an outer one restores the outer start.
    ///
    /// The snapshot copies this editor's changes, not the document -- the
    /// document is immutable and shared behind an `Arc`. So a transaction
    /// costs what has been edited so far, never what is being edited.
    ///
    /// A **panic** out of `body` is not a rollback. Nothing in this crate
    /// panics on document bytes (ruling 1), so this only arises from a
    /// caller's own closure; if one catches an unwind here it must drop the
    /// editor rather than keep using it.
    pub fn transaction<T, E>(
        &mut self,
        body: impl FnOnce(&mut DocumentEditor) -> Result<T, E>,
    ) -> Result<T, E> {
        let saved = self.checkpoint();
        match body(self) {
            Ok(value) => Ok(value),
            Err(error) => {
                self.restore(&saved);
                Err(error)
            }
        }
    }

    /// Takes this editor's state as a value, for
    /// [`DocumentEditor::restore`] to put back.
    ///
    /// The closure-free half of [`DocumentEditor::transaction`], which is
    /// nothing but `checkpoint` -> body -> `restore` on `Err`. Taking one
    /// changes nothing about the editor, and holding one across any number of
    /// further edits is fine -- it is a copy, not a borrow.
    ///
    /// The copy is of *this editor's changes*, never of the document: the
    /// document is immutable and shared behind an `Arc`. So a checkpoint
    /// costs what has been edited so far, not what is being edited -- which
    /// is why taking one per keystroke in a form is affordable and taking one
    /// per page of a hundred-megabyte file is too.
    ///
    /// See [`EditCheckpoint`] for why this is a value rather than an open
    /// transaction.
    #[must_use]
    pub fn checkpoint(&self) -> EditCheckpoint {
        EditCheckpoint {
            overlay: self.overlay.clone(),
            deleted: self.deleted.clone(),
            next: self.next,
            page_order: self.page_order.clone(),
        }
    }

    /// Puts this editor back to what a checkpoint recorded.
    ///
    /// Restores exactly what a rolled-back [`DocumentEditor::transaction`]
    /// restores, because it is the same function: objects written, objects
    /// deleted, the page order, and the object-number counter (see
    /// [`DocumentEditor`]'s `next` for why that last one, and what it costs).
    ///
    /// **Idempotent.** Restoring the same checkpoint twice leaves the editor
    /// where restoring it once did, so a caller that cannot tell whether it
    /// already restored may simply restore. That is what makes this safe to
    /// project across a foreign-function boundary where the caller's own
    /// error handling may run twice.
    ///
    /// The checkpoint is borrowed rather than consumed, so one checkpoint can
    /// undo several attempts -- the retry loop a closure cannot express.
    pub fn restore(&mut self, saved: &EditCheckpoint) {
        self.overlay = saved.overlay.clone();
        self.deleted = saved.deleted.clone();
        self.next = saved.next;
        self.page_order = saved.page_order.clone();
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
}
