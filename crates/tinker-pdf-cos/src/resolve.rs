//! Reading objects through a view of a document: the file as it was opened,
//! or an editor's overlay over it.
//!
//! Feature documentation: `docs/features/editing.md`.

use std::borrow::Cow;
use std::sync::Arc;

use crate::doc::{CosDocument, CosError};
use crate::flight::{Filled, Flights};
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
/// tally, and so does each key it is asked to look up by its bytes where a
/// constant stands for it; everything it reads is the view underneath's, read
/// the way that view reads it.
///
/// Code written against [`Resolve`] counts its decodes when it is handed
/// this view instead of the one under it — a font's CMaps and its
/// `/CIDToGIDMap`, read while a scope resolves it, are counted that way, and
/// so is every key reading the font spells out rather than takes as a
/// [`Name`] constant. Over a [`crate::DocumentEditor`] that counts its own
/// decodes, wrap it in none: each decode would count twice.
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

    /// The view underneath's symbol, counted as a key looked up by its bytes
    /// when it is one the table pre-interns ([`Name::is_preinterned`]): a
    /// lookup its constant would have answered.
    fn intern(&self, bytes: &[u8]) -> Name {
        let name = self.inner.intern(bytes);
        if name.is_preinterned() {
            self.work.add(crate::work::Counter::KeysLookedUp, 1);
        }
        name
    }

    fn name_bytes(&self, name: Name) -> Option<Arc<[u8]>> {
        self.inner.name_bytes(name)
    }

    fn warn(&self, kind: WarningKind) {
        self.inner.warn(kind);
    }
}

/// What one decode of a stream answered: its bytes, or its chain's refusal.
type Decode = Result<Arc<Vec<u8>>, CosError>;

/// The streams one operation has decoded, each by its reference in the
/// document the operation reads (`docs/design/performance.md`, P-03, "The
/// cache: who holds it, and its key"): what lets an operation decode each
/// stream once however many of its readers ask for it.
///
/// It keeps what a chain answered — the bytes, behind an `Arc` so that a later
/// read is handed them rather than a copy, or the chain's refusal — and
/// nothing else. A reference that names no stream ran no chain and is not
/// kept. A later read runs nothing, so it raises none of the warnings the
/// first raised, as a second read of a cached object raises none.
///
/// One an operation, made by the door the operation goes through and dropped
/// when it returns: it holds every stream the operation reads until then.
/// **Single-flight** ([`Flights`]): the threads of one pooled operation that
/// read one stream at once decode it once — the first runs the chain, outside
/// the lock, and the others wait for its answer — and a read made inside
/// another fill never waits, so no read can deadlock, a stream whose own
/// `/Length` points into it included.
#[derive(Debug, Default)]
pub struct DecodedStreams {
    streams: Flights<ObjRef, Decode>,
}

impl DecodedStreams {
    /// Streams of an operation that has decoded none yet.
    #[must_use]
    pub fn new() -> DecodedStreams {
        DecodedStreams::default()
    }

    /// `r`'s bytes with its filters applied, read once an operation: what
    /// the operation's first read of `r` answered, and on that first read
    /// what `decode` answers, kept when a chain ran — the bytes, or the
    /// chain's refusal ([`CosError::Filter`]).
    ///
    /// Two threads that read one stream at once run `decode` once: the
    /// second waits for the first's answer, unless it is itself decoding a
    /// stream, when it decodes this one for itself and keeps nothing.
    ///
    /// # Errors
    /// What `decode` answered, now or at the first read.
    pub fn read(
        &self,
        r: ObjRef,
        decode: impl FnOnce() -> Result<Vec<u8>, CosError>,
    ) -> Result<Arc<Vec<u8>>, CosError> {
        self.streams.read(r, || {
            let read = decode().map(Arc::new);
            // A reference that names no stream ran no chain: nothing is kept,
            // and the next read asks again.
            if matches!(read, Ok(_) | Err(CosError::Filter(_))) {
                Filled::Keep(read)
            } else {
                Filled::Once(read)
            }
        })
    }
}

/// A view that decodes each stream once an operation: its `stream_decoded`
/// answers from the operation's [`DecodedStreams`], and asks the view
/// underneath only for a stream the operation has not read yet. Everything
/// else is the view underneath's, read the way that view reads it — a
/// [`Counting`] view's count of the keys it looks up included.
///
/// Code written against [`Resolve`] decodes each stream once an operation when
/// it is handed this view: a font's CMaps and its `/CIDToGIDMap`, read again by
/// every scope of a render, a text extraction or a save that reads the font,
/// are decoded by the first. Over a [`Counting`] view, only the reads that run
/// a chain count.
pub struct Cached<'a, R: Resolve + ?Sized> {
    inner: &'a R,
    streams: &'a DecodedStreams,
}

impl<'a, R: Resolve + ?Sized> Cached<'a, R> {
    /// `inner`, decoding each stream through `streams`.
    #[must_use]
    pub fn new(inner: &'a R, streams: &'a DecodedStreams) -> Cached<'a, R> {
        Cached { inner, streams }
    }
}

impl<R: Resolve + ?Sized> Resolve for Cached<'_, R> {
    fn document(&self) -> &CosDocument {
        self.inner.document()
    }

    fn get(&self, r: ObjRef) -> Result<Arc<Object>, CosError> {
        self.inner.get(r)
    }

    fn trailer(&self) -> Cow<'_, Dict> {
        self.inner.trailer()
    }

    /// The operation's read of `r`, made through the view underneath the
    /// first time it is asked.
    fn stream_decoded(&self, r: ObjRef) -> Result<Vec<u8>, CosError> {
        self.streams
            .read(r, || self.inner.stream_decoded(r))
            .map(|bytes| bytes.as_ref().clone())
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

    /// A pre-interned key looked up by its bytes counts, the symbol being the
    /// constant's; any other name looked up does not.
    #[test]
    fn a_key_looked_up_by_its_bytes_counts_and_a_resource_name_does_not() {
        let doc = document();
        let work = Work::new();
        let view = Counting::new(&doc, &work);
        assert_eq!(view.intern(b"Font"), Name::FONT);
        assert_eq!(view.intern(b"Type"), Name::TYPE);
        assert_eq!(view.intern(b"F0"), doc.intern(b"F0"));
        assert_eq!(work.stats().keys_looked_up, 2);
        assert_eq!(work.stats().work(), 2);
    }

    /// A stream read three times through the cached view over a counting
    /// one runs its chain once, and every read hands back what that run
    /// decoded; another stream is another run.
    #[test]
    fn a_cached_view_decodes_each_stream_once_an_operation() {
        let doc = document();
        let work = Work::new();
        let counting = Counting::new(&doc, &work);
        let streams = DecodedStreams::new();
        let view = Cached::new(&counting, &streams);
        let stream = ObjRef::new(2, 0);
        for _ in 0..3 {
            assert_eq!(view.stream_decoded(stream).expect("it decodes"), PLAIN);
        }
        let stats = work.stats();
        assert_eq!(stats.streams_decoded, 1, "one run for three reads");
        assert_eq!(stats.most_decodes_of_one_stream, 1);
        assert_eq!(stats.bytes_decoded, PLAIN.len() as u64);

        // The same streams under a second view of the operation: kept already.
        let again = Cached::new(&counting, &streams);
        assert_eq!(again.stream_decoded(stream).expect("it decodes"), PLAIN);
        assert_eq!(work.stats().streams_decoded, 1, "the operation's read");
        // Everything else is the view underneath's, its count of keys too.
        assert_eq!(view.get(stream).ok(), doc.get(stream).ok());
        assert_eq!(view.intern(b"Font"), Name::FONT);
        assert_eq!(work.stats().keys_looked_up, 1);
    }

    /// A chain's refusal is kept as firmly as bytes, and its warning raised
    /// once; a reference that names no stream ran no chain, and is asked of
    /// the view underneath at every read.
    #[test]
    fn a_refusal_is_kept_and_a_reference_to_no_stream_is_not() {
        let doc = document();
        let streams = DecodedStreams::new();
        let refused = ObjRef::new(3, 0);
        let mut runs = 0;
        for _ in 0..2 {
            let read = streams.read(refused, || {
                runs += 1;
                doc.stream_decoded(refused)
            });
            assert!(matches!(read, Err(CosError::Filter(_))), "{read:?}");
        }
        assert_eq!(runs, 1, "the refusal is the operation's answer");
        let bad = doc
            .warnings()
            .iter()
            .filter(|warning| warning.kind == WarningKind::FilterParamsBad)
            .count();
        assert_eq!(bad, 1, "the refusal's warning, raised by its one run");

        let not_a_stream = ObjRef::new(4, 0);
        let mut asked = 0;
        for _ in 0..2 {
            let read = streams.read(not_a_stream, || {
                asked += 1;
                doc.stream_decoded(not_a_stream)
            });
            assert!(matches!(read, Err(CosError::NotAStream(_))), "{read:?}");
        }
        assert_eq!(asked, 2, "nothing ran, so nothing was kept");
    }

    /// Bytes kept are handed out, never copied: two reads of one stream are
    /// one allocation.
    #[test]
    fn a_kept_stream_is_handed_out_rather_than_copied() {
        let doc = document();
        let streams = DecodedStreams::new();
        let stream = ObjRef::new(2, 0);
        let first = streams
            .read(stream, || doc.stream_decoded(stream))
            .expect("it decodes");
        let second = streams
            .read(stream, || panic!("a kept stream is not decoded again"))
            .expect("it is kept");
        assert!(Arc::ptr_eq(&first, &second));
        assert_eq!(first.as_slice(), PLAIN);
    }

    /// The streams of one operation are shared between threads.
    #[test]
    fn decoded_streams_are_shared_between_threads() {
        fn shared<T: Send + Sync>() {}
        shared::<DecodedStreams>();
        let doc = document();
        let streams = DecodedStreams::new();
        let stream = ObjRef::new(2, 0);
        std::thread::scope(|scope| {
            for _ in 0..4 {
                scope.spawn(|| {
                    let read = streams
                        .read(stream, || doc.stream_decoded(stream))
                        .expect("it decodes");
                    assert_eq!(read.as_slice(), PLAIN);
                });
            }
        });
    }
}
