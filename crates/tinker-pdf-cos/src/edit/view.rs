//! The editor's state as a document of its own.

use std::sync::Arc;

use super::DocumentEditor;
use crate::doc::{CosDocument, OpenError};
use crate::write::{WriteMode, WriteOptions};

impl DocumentEditor {
    /// A [`CosDocument`] in which everything this editor has done resolves:
    /// every object it has put or allocated, every stream it has written,
    /// every deletion (as null), the page order and the trailer entries.
    ///
    /// For a caller that must hand an `Arc<CosDocument>` to something built
    /// over a document — a page's resources, an interpreter — and needs that
    /// thing to see an object this editor has only just allocated.
    /// [`DocumentEditor::shared_document`] is the file as it was opened, and
    /// an editor's new objects are invisible through it; [`crate::Resolve`]
    /// sees them but is not a `CosDocument`.
    ///
    /// # How
    ///
    /// The incremental update [`DocumentEditor::save`] would write is built in
    /// memory and opened. So object numbers are the editor's own — a
    /// reference the editor handed out means the same object here — and
    /// everything the view reads, it reads through the ordinary reader,
    /// filters and all. An untouched editor answers with the document it was
    /// opened with, and no copy.
    ///
    /// **An encrypted document** is viewed decrypted: the update is sealed
    /// with the file's own key, as a save seals it (7.6.2), and the view is
    /// given the security handler the document was authenticated with, so
    /// the original objects and the new ones decrypt alike. A document that
    /// was never authenticated gives a view that was not either — the same
    /// state, not a better one.
    ///
    /// # Cost
    ///
    /// A copy of the file and a parse of its cross-reference tables, every
    /// call. A view is a snapshot: edits made after it are not in it, so a
    /// caller that edits and then reads again takes another.
    ///
    /// # Errors
    ///
    /// [`OpenError`] when the bytes do not reopen — which for a document
    /// this reader opened, with an update this writer wrote, is a defect in
    /// one of the two rather than a property of the input.
    pub fn view(&self) -> Result<Arc<CosDocument>, OpenError> {
        if !self.is_dirty() {
            return Ok(Arc::clone(&self.doc));
        }
        let bytes = self.save(&WriteOptions {
            mode: WriteMode::Incremental,
            ..WriteOptions::default()
        });
        let view = CosDocument::open(bytes)?;
        view.inherit_security(&self.doc);
        Ok(Arc::new(view))
    }
}
