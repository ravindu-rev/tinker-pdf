//! The editor as a [`Resolve`] view, and the catalog as it sees it.
//!
//! Every read the editor makes of its own state goes through here: the
//! overlay first, the file underneath. A read that goes to the document
//! directly sees the file as it was opened, and the second edit that starts
//! from it discards the first.

use std::borrow::Cow;
use std::sync::Arc;

use super::DocumentEditor;
use crate::doc::{CosDocument, CosError};
use crate::name::Name;
use crate::object::{Dict, ObjRef, Object};
use crate::resolve::Resolve;
use crate::write::Written;

impl Resolve for DocumentEditor {
    fn document(&self) -> &CosDocument {
        &self.doc
    }

    /// The overlay's object, the file's, or null for one this editor deleted.
    ///
    /// A stream this editor wrote comes back as its dictionary, as
    /// [`DocumentEditor::get`] returns it: its data is not in the file, so
    /// there is no [`crate::object::StreamObj`] extent to describe it with.
    /// [`Resolve::stream_decoded`] is how its bytes are read.
    fn get(&self, r: ObjRef) -> Result<Arc<Object>, CosError> {
        if self.deleted.contains(&r.num) {
            return Ok(Arc::new(Object::Null));
        }
        match self.overlay.get(&r.num) {
            Some(Written::Object(object)) => Ok(Arc::new(object.clone())),
            Some(Written::Stream(stream)) => Ok(Arc::new(Object::Dict(stream.dict.clone()))),
            None => self.doc.get(r),
        }
    }

    fn trailer(&self) -> Cow<'_, Dict> {
        Cow::Borrowed(self.doc.trailer())
    }

    fn stream_decoded(&self, r: ObjRef) -> Result<Vec<u8>, CosError> {
        self.stream_bytes(r).ok_or(CosError::NotAStream(r))
    }
}

impl DocumentEditor {
    /// The catalog's reference: the trailer's `/Root` (7.5.5).
    fn root(&self) -> Option<ObjRef> {
        Resolve::trailer(self).get_ref(Name::ROOT)
    }

    /// The catalog (7.7.2) **as this editor has it**: every change made to it
    /// so far included.
    ///
    /// `None` when the trailer has no `/Root` or it is not a dictionary.
    #[must_use]
    pub fn catalog(&self) -> Option<Dict> {
        match DocumentEditor::get(self, self.root()?)? {
            Object::Dict(dict) => Some(dict),
            _ => None,
        }
    }

    /// Changes the catalog in place: `change` is handed the catalog as this
    /// editor has it, and what it leaves is written back.
    ///
    /// The one door every catalog edit in this crate goes through, so two
    /// edits compose. They did not before: `/Perms`, `/AcroForm` and the
    /// `/NeedAppearances` clean-up each re-read the catalog their own way, and
    /// the last of them read it from the *file*, so clearing the flag after
    /// another edit had changed the catalog wrote the original catalog back
    /// over it.
    ///
    /// Returns false, changing nothing, when there is no catalog to change.
    pub fn update_catalog(&mut self, change: impl FnOnce(&mut Dict)) -> bool {
        let (Some(root), Some(mut catalog)) = (self.root(), self.catalog()) else {
            return false;
        };
        change(&mut catalog);
        self.put(root, Object::Dict(catalog));
        true
    }

    /// The version text strings this editor writes are encoded for
    /// (7.9.2.2): the one the document declares — the header's, or this
    /// editor's catalog `/Version` when that is later (7.7.2) — and 1.7 when
    /// it declares none.
    pub(super) fn text_version(&self) -> (u8, u8) {
        let catalog = self
            .catalog()
            .and_then(|c| c.get_name(self.intern(b"Version")))
            .and_then(|name| self.doc.name_bytes(name))
            .and_then(|bytes| String::from_utf8(bytes.to_vec()).ok());
        crate::outline::text_version_of(self.doc.header_version(), catalog)
    }
}
