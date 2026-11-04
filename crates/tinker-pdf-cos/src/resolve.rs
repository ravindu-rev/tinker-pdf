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
use crate::work::Work;

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

/// A view that counts: each stream it decodes counts into an operation's
/// tally, and everything it reads is the view underneath's, read the way
/// that view reads it.
///
/// Code written against [`Resolve`] counts its decodes when it is handed
/// this view instead of the one under it — a font's CMaps and its
/// `/CIDToGIDMap`, read while a scope is built, are counted that way. Over a
/// [`crate::DocumentEditor`] that counts its own decodes, wrap it in none:
/// each decode would count twice.
pub struct Counting<'a, R: Resolve + ?Sized> {
    inner: &'a R,
    work: &'a Work,
}

impl<'a, R: Resolve + ?Sized> Counting<'a, R> {
    /// `inner`, counting each stream it decodes into `work`.
    #[must_use]
    pub fn new(inner: &'a R, work: &'a Work) -> Counting<'a, R> {
        Counting { inner, work }
    }
}

impl<R: Resolve + ?Sized> Resolve for Counting<'_, R> {
    fn document(&self) -> &CosDocument {
        self.inner.document()
    }

    fn get(&self, r: ObjRef) -> Result<Arc<Object>, CosError> {
        self.inner.get(r)
    }

    fn trailer(&self) -> Cow<'_, Dict> {
        self.inner.trailer()
    }

    /// The view underneath's decode, counted when a chain ran: on bytes
    /// handed back, or on the chain's own refusal.
    fn stream_decoded(&self, r: ObjRef) -> Result<Vec<u8>, CosError> {
        let out = self.inner.stream_decoded(r);
        if matches!(out, Ok(_) | Err(CosError::Filter(_))) {
            crate::streams::count_run(Some(self.work), r, &out);
        }
        out
    }

    fn resolve(&self, object: &Object) -> Arc<Object> {
        self.inner.resolve(object)
    }

    fn resolve_key(&self, dict: &Dict, key: Name) -> Arc<Object> {
        self.inner.resolve_key(dict, key)
    }

    fn catalog(&self) -> Option<Arc<Dict>> {
        self.inner.catalog()
    }

    fn intern(&self, bytes: &[u8]) -> Name {
        self.inner.intern(bytes)
    }

    fn name_bytes(&self, name: Name) -> Option<Arc<[u8]>> {
        self.inner.name_bytes(name)
    }

    fn warn(&self, kind: WarningKind) {
        self.inner.warn(kind);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 1 the catalog, 2 a flate stream of `PLAIN`, 3 the same stream under
    /// `/DecodeParms` that describe no stream — a predictor row of no
    /// columns — 4 a number.
    fn document() -> CosDocument {
        let deflated = tinker_pdf_filters::zlib_compress(PLAIN);
        let flate = |parms: &str| {
            let mut out = format!(
                "<< /Filter /FlateDecode{parms} /Length {} >>\nstream\n",
                deflated.len()
            )
            .into_bytes();
            out.extend_from_slice(&deflated);
            out.extend_from_slice(b"\nendstream");
            out
        };
        let objects: Vec<Vec<u8>> = vec![
            b"<< /Type /Catalog >>".to_vec(),
            flate(""),
            flate(" /DecodeParms << /Predictor 12 /Columns 0 >>"),
            b"42".to_vec(),
        ];
        let mut out = b"%PDF-1.7\n".to_vec();
        let mut offsets = Vec::new();
        for (index, body) in objects.iter().enumerate() {
            offsets.push(out.len());
            out.extend_from_slice(format!("{} 0 obj\n", index + 1).as_bytes());
            out.extend_from_slice(body);
            out.extend_from_slice(b"\nendobj\n");
        }
        let xref = out.len();
        out.extend_from_slice(
            format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes(),
        );
        for offset in offsets {
            out.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
        }
        out.extend_from_slice(
            format!(
                "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
                objects.len() + 1
            )
            .as_bytes(),
        );
        CosDocument::open(out).expect("the fixture opens")
    }

    const PLAIN: &[u8] = b"0 0 m 10 10 l S";

    #[test]
    fn a_counting_view_counts_each_decode_and_reads_what_its_inner_view_reads() {
        let doc = document();
        let work = Work::new();
        let view = Counting::new(&doc, &work);
        let stream = ObjRef::new(2, 0);
        for _ in 0..2 {
            assert_eq!(view.stream_decoded(stream).expect("it decodes"), PLAIN);
        }
        assert_eq!(view.get(stream).ok(), doc.get(stream).ok());
        assert_eq!(*view.trailer(), *Resolve::trailer(&doc));
        let stats = work.stats();
        assert_eq!(stats.streams_decoded, 2);
        assert_eq!(stats.most_decodes_of_one_stream, 2);
        assert_eq!(stats.bytes_decoded, 2 * PLAIN.len() as u64);
    }

    #[test]
    fn a_refusal_is_a_run_and_a_reference_to_no_stream_is_not() {
        let doc = document();
        let work = Work::new();
        let view = Counting::new(&doc, &work);
        let refused = view.stream_decoded(ObjRef::new(3, 0));
        let not_a_stream = view.stream_decoded(ObjRef::new(4, 0));
        let missing = view.stream_decoded(ObjRef::new(9, 0));
        assert!(
            matches!(refused, Err(CosError::Filter(_))),
            "{refused:?}: the chain refuses its parameters"
        );
        assert!(not_a_stream.is_err() && missing.is_err());
        let stats = work.stats();
        assert_eq!(stats.streams_decoded, 1, "the refusal alone ran a chain");
        assert_eq!(stats.bytes_decoded, 0);
    }
}
