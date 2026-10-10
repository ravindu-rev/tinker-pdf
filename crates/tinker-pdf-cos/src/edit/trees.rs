//! Writing name and number trees (7.9.6, 7.9.7) into the editor's overlay.

use std::sync::Arc;

use super::DocumentEditor;
use crate::object::{ObjRef, Object};
use crate::trees::{self, TreeWriteError};

impl DocumentEditor {
    /// Writes a name tree holding `entries` as new objects, and returns its
    /// root for the caller to hang where it belongs — `/Names /Dests`,
    /// `/Names /EmbeddedFiles`, anything 7.7.4 lists.
    ///
    /// Every node is allocated and put here; nothing already in the document
    /// is touched. See [`trees::write_name_tree`] for the shape.
    ///
    /// # Errors
    ///
    /// [`TreeWriteError`] for a key given twice or more entries than the
    /// reader walks, and then **nothing was written**: no number allocated,
    /// no node put, so the editor is exactly as it was.
    pub fn add_name_tree(
        &mut self,
        entries: Vec<(Vec<u8>, Object)>,
    ) -> Result<ObjRef, TreeWriteError> {
        // The document behind its own handle, so the name table it lends is
        // not a borrow of the editor the sink below needs mutably.
        let doc = Arc::clone(&self.doc);
        trees::write_name_tree(entries, doc.names_table(), |node| {
            let r = self.allocate();
            self.put(r, node);
            r
        })
    }

    /// Writes a number tree holding `entries` as new objects, and returns its
    /// root — for `/PageLabels`, a structure tree's `/ParentTree`, or any
    /// other 7.9.7 tree.
    ///
    /// # Errors
    ///
    /// As [`DocumentEditor::add_name_tree`], and with the same guarantee that
    /// a refusal writes nothing.
    pub fn add_number_tree(
        &mut self,
        entries: Vec<(i64, Object)>,
    ) -> Result<ObjRef, TreeWriteError> {
        let doc = Arc::clone(&self.doc);
        trees::write_number_tree(entries, doc.names_table(), |node| {
            let r = self.allocate();
            self.put(r, node);
            r
        })
    }
}
