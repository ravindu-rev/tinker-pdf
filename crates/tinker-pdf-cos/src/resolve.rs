//! Reading objects through a view of a document: the file as it was opened,
//! or an editor's overlay over it.
//!
//! Feature documentation: `docs/features/editing.md`.

use std::borrow::Cow;
use std::sync::Arc;

use crate::doc::{CosDocument, CosError};
use crate::limits;
use crate::name::Name;
use crate::object::{Dict, ObjRef, Object};
use crate::warn::WarningKind;

/// Where objects are read from.
///
/// [`CosDocument`] reads the file. [`crate::edit::DocumentEditor`] reads its
/// own changes first and the file underneath them, which is what lets one
/// edit see the edits made before it: a field the editor has `put` and listed
/// in `/Fields` is in its `fields()`, and a catalog it has changed is the
/// catalog the next change starts from.
///
/// The methods carry [`CosDocument`]'s own names and types, so a walk written
/// against this trait reads exactly as it did against a document. Four are
/// required; the rest follow from them, and a document overrides those with
/// its own cycle-tracking, warning-recording versions.
pub trait Resolve {
    /// The document underneath: the name table every [`Name`] here was
    /// interned in, and the sink warnings go to.
    fn document(&self) -> &CosDocument;

    /// One object, as this view has it. An object the view has deleted, or
    /// the file never had, is null (7.3.10).
    ///
    /// # Errors
    /// As [`CosDocument::get`].
    fn get(&self, r: ObjRef) -> Result<Arc<Object>, CosError>;

    /// The trailer (7.5.5), as this view has it.
    fn trailer(&self) -> Cow<'_, Dict>;

    /// A stream's bytes with its filters applied.
    ///
    /// # Errors
    /// As [`CosDocument::stream_decoded`].
    fn stream_decoded(&self, r: ObjRef) -> Result<Vec<u8>, CosError>;

    /// Follows indirect references until something else comes back, at most
    /// [`limits::MAX_RESOLVE_DEPTH`] of them.
    fn resolve(&self, object: &Object) -> Arc<Object> {
        let Some(mut at) = object.as_objref() else {
            return Arc::new(object.clone());
        };
        for _ in 0..limits::MAX_RESOLVE_DEPTH {
            let Ok(next) = self.get(at) else {
                return Arc::new(Object::Null);
            };
            match next.as_objref() {
                Some(further) => at = further,
                None => return next,
            }
        }
        Arc::new(Object::Null)
    }

    /// The value of `key` in `dict`, resolved; null when absent.
    fn resolve_key(&self, dict: &Dict, key: Name) -> Arc<Object> {
        match dict.get(key) {
            Some(object) => self.resolve(object),
            None => Arc::new(Object::Null),
        }
    }

    /// The catalog (7.7.2): the trailer's `/Root`, as this view has it.
    fn catalog(&self) -> Option<Arc<Dict>> {
        let root = self.trailer().get_ref(Name::ROOT)?;
        let object = self.get(root).ok()?;
        object.as_dict().map(|d| Arc::new(d.clone()))
    }

    /// The symbol for `bytes` in the document's name table.
    fn intern(&self, bytes: &[u8]) -> Name {
        self.document().intern(bytes)
    }

    /// The bytes behind a symbol.
    fn name_bytes(&self, name: Name) -> Option<Arc<[u8]>> {
        self.document().name_bytes(name)
    }

    /// Records a warning against the document (ruling 10).
    fn warn(&self, kind: WarningKind) {
        self.document().warn(kind);
    }
}

impl Resolve for CosDocument {
    fn document(&self) -> &CosDocument {
        self
    }

    fn get(&self, r: ObjRef) -> Result<Arc<Object>, CosError> {
        CosDocument::get(self, r)
    }

    fn trailer(&self) -> Cow<'_, Dict> {
        Cow::Borrowed(CosDocument::trailer(self))
    }

    fn stream_decoded(&self, r: ObjRef) -> Result<Vec<u8>, CosError> {
        CosDocument::stream_decoded(self, r)
    }

    fn resolve(&self, object: &Object) -> Arc<Object> {
        CosDocument::resolve(self, object)
    }

    fn resolve_key(&self, dict: &Dict, key: Name) -> Arc<Object> {
        CosDocument::resolve_key(self, dict, key)
    }

    fn catalog(&self) -> Option<Arc<Dict>> {
        CosDocument::catalog(self)
    }
}
