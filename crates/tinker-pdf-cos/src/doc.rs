//! The document: one immutable buffer, a merged cross-reference table, and a
//! lazy store over it.
//!
//! Bytes in, values out. There are no file handles and no I/O traits here
//! because wasm32-unknown-unknown has neither files nor mmap and is a
//! first-class target; memory-mapping is a facade decision on native
//! platforms and everything below works on a slice.
//!
//! [`OpenError`] is reserved for total failure — not one object could be
//! located even after a full rescan. Anything less is a warning plus degraded
//! content, which is what the leniency ladder is:
//!
//! 1. **Trust.** Cross-reference offsets used as-is, every one validated
//!    against its `N G obj` header. Zero warnings means the file was honest.
//! 2. **Patch.** A validation failure repairs that entry from the scan index
//!    and warns, keeping the rest of the table. Bounded damage costs bounded
//!    work.
//! 3. **Rescan.** No usable `startxref`, unparseable tables, a `/Root` that
//!    cannot be located, or per-object failures past a threshold: the tables
//!    are discarded, the scan index becomes truth, and a trailer is
//!    synthesized if the file's own is gone.
//!
//! The ladder is deterministic: the same bytes always take the same path and
//! produce the same warnings, which is what reproducible fuzz triage and
//! cross-reader comparison both need.

use core::fmt;
use core::ops::Range;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};

use tinker_pdf_crypto::Permissions;
use tinker_pdf_filters::FilterError;

use crate::decrypt::{self, Decryptor, EncryptParams, IdentityDecryptor};
use crate::limits;
use crate::name::{Name, NameTable};
use crate::objstm::{self, ObjStm, ObjStmCache};
use crate::parse::{parse_indirect_at, parse_object_at};
use crate::repair::ScanIndex;
use crate::security::{AuthError, AuthLevel};
use crate::store::{MutexExt, ResolveCtx, SlotStore};
use crate::warn::{Warning, WarningKind, WarningSink};
use crate::xref::{self, Revision, XrefBuild, XrefEntry, XrefTable};
use crate::Dict;
use crate::ObjRef;
use crate::Object;

/// Which rung of the leniency ladder this document opened on.
///
/// Fixed when the document opens, so "it opened" and "it opened after a full
/// rescan" are different observable facts that no later read can blur.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum LadderLevel {
    /// The tables were used as written and every offset validated.
    Trust,
    /// Some offsets lied and were repaired from the repair scanner.
    Patch,
    /// The tables were discarded; the whole buffer was scanned for objects.
    Rescan,
}

/// The only way opening a document fails.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum OpenError {
    /// Not one indirect object could be located, even after a full rescan.
    /// This is not a PDF by any reader's definition.
    NoObjects,
}

impl fmt::Display for OpenError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            OpenError::NoObjects => f.write_str("no indirect objects found"),
        }
    }
}

impl std::error::Error for OpenError {}

/// A per-object failure. Never fatal to the document.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CosError {
    /// The object is not a stream (7.3.8) — including when it does not exist,
    /// since a missing object reads as null (7.3.10).
    NotAStream(ObjRef),
    /// `/DecodeParms` could not describe any stream, or the chain named a
    /// codec that is gated behind a capability.
    Filter(FilterError),
}

impl fmt::Display for CosError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CosError::NotAStream(r) => write!(f, "{r} is not a stream"),
            CosError::Filter(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for CosError {}

/// The names this layer needs beyond the pre-interned set, plus the table
/// itself, interned once per document.
pub(crate) struct DocNames {
    pub table: NameTable,
    pub xref: Name,
    pub obj_stm: Name,
    pub catalog: Name,
    pub dp: Name,
    pub predictor: Name,
    pub colors: Name,
    pub bits_per_component: Name,
    pub columns: Name,
    pub early_change: Name,
    pub crypt: Name,
    flate_decode: Name,
    fl: Name,
    lzw_decode: Name,
    lzw: Name,
    ascii_hex_decode: Name,
    ahx: Name,
    ascii85_decode: Name,
    a85: Name,
    run_length_decode: Name,
    rl: Name,
    dct_decode: Name,
    dct: Name,
    ccitt_fax_decode: Name,
    ccf: Name,
    jbig2_decode: Name,
    jpx_decode: Name,
    pub v: Name,
    pub r: Name,
    pub o: Name,
    pub u: Name,
    pub oe: Name,
    pub ue: Name,
    pub perms: Name,
    pub p: Name,
    pub cf: Name,
    pub cfm: Name,
    pub auth_event: Name,
    pub stm_f: Name,
    pub str_f: Name,
    pub eff: Name,
    pub encrypt_metadata: Name,
    pub sub_filter: Name,
    pub crop_box: Name,
    pub rotate: Name,
    info_keys: [Name; 6],
}

impl DocNames {
    pub(crate) fn new() -> DocNames {
        let table = NameTable::new();
        let n = |bytes: &[u8]| table.intern(bytes);
        DocNames {
            xref: n(b"XRef"),
            obj_stm: n(b"ObjStm"),
            catalog: n(b"Catalog"),
            dp: n(b"DP"),
            predictor: n(b"Predictor"),
            colors: n(b"Colors"),
            bits_per_component: n(b"BitsPerComponent"),
            columns: n(b"Columns"),
            early_change: n(b"EarlyChange"),
            crypt: n(b"Crypt"),
            flate_decode: n(b"FlateDecode"),
            fl: n(b"Fl"),
            lzw_decode: n(b"LZWDecode"),
            lzw: n(b"LZW"),
            ascii_hex_decode: n(b"ASCIIHexDecode"),
            ahx: n(b"AHx"),
            ascii85_decode: n(b"ASCII85Decode"),
            a85: n(b"A85"),
            run_length_decode: n(b"RunLengthDecode"),
            rl: n(b"RL"),
            dct_decode: n(b"DCTDecode"),
            dct: n(b"DCT"),
            ccitt_fax_decode: n(b"CCITTFaxDecode"),
            ccf: n(b"CCF"),
            jbig2_decode: n(b"JBIG2Decode"),
            jpx_decode: n(b"JPXDecode"),
            v: n(b"V"),
            r: n(b"R"),
            o: n(b"O"),
            u: n(b"U"),
            oe: n(b"OE"),
            ue: n(b"UE"),
            perms: n(b"Perms"),
            p: n(b"P"),
            cf: n(b"CF"),
            cfm: n(b"CFM"),
            auth_event: n(b"AuthEvent"),
            stm_f: n(b"StmF"),
            str_f: n(b"StrF"),
            eff: n(b"EFF"),
            encrypt_metadata: n(b"EncryptMetadata"),
            sub_filter: n(b"SubFilter"),
            crop_box: n(b"CropBox"),
            rotate: n(b"Rotate"),
            info_keys: [
                n(b"Producer"),
                n(b"Creator"),
                n(b"CreationDate"),
                n(b"ModDate"),
                n(b"Title"),
                n(b"Author"),
            ],
            table,
        }
    }

    /// The filter a `/Filter` name selects.
    ///
    /// The abbreviations of Table 6 are officially for inline images only, but
    /// real producers write them in stream dictionaries too, so both spellings
    /// are accepted.
    pub(crate) fn filter(&self, name: Name) -> Option<tinker_pdf_filters::Filter> {
        use tinker_pdf_filters::Filter;
        let filter = if name == self.flate_decode || name == self.fl {
            Filter::Flate
        } else if name == self.lzw_decode || name == self.lzw {
            Filter::Lzw
        } else if name == self.ascii_hex_decode || name == self.ahx {
            Filter::AsciiHex
        } else if name == self.ascii85_decode || name == self.a85 {
            Filter::Ascii85
        } else if name == self.run_length_decode || name == self.rl {
            Filter::RunLength
        } else if name == self.dct_decode || name == self.dct {
            Filter::Dct
        } else if name == self.ccitt_fax_decode || name == self.ccf {
            Filter::Ccitt
        } else if name == self.jbig2_decode {
            Filter::Jbig2
        } else if name == self.jpx_decode {
            Filter::Jpx
        } else {
            return None;
        };
        Some(filter)
    }

    /// Whether an untyped dictionary looks like a document information
    /// dictionary (14.3.3), for synthesizing `/Info` after a rescan.
    pub(crate) fn looks_like_info(&self, dict: &Dict) -> bool {
        self.info_keys.iter().any(|k| dict.contains_key(*k))
    }
}

/// A PDF file's object layer.
pub struct CosDocument {
    pub(crate) buffer: Arc<[u8]>,
    pub(crate) names: DocNames,
    xref: XrefTable,
    trailer: Dict,
    revisions: Vec<Revision>,
    store: SlotStore,
    objstm: ObjStmCache,
    scan: Option<Arc<ScanIndex>>,
    warnings: Mutex<WarningSink>,
    pub(crate) stream_ranges: RwLock<HashMap<u32, Range<u64>>>,
    pub(crate) decryptor: Arc<dyn Decryptor>,
    has_decryptor: bool,
    encrypt: Option<EncryptParams>,
    auth_level: AuthLevel,
    ladder: LadderLevel,
}

impl fmt::Debug for CosDocument {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CosDocument")
            .field("bytes", &self.buffer.len())
            .field("objects", &self.xref.len())
            .field("revisions", &self.revisions.len())
            .field("ladder", &self.ladder)
            .finish()
    }
}

impl CosDocument {
    /// Opens a document, running the leniency ladder.
    ///
    /// # Errors
    /// [`OpenError::NoObjects`] when not one indirect object could be located,
    /// even after a full rescan. Every lesser problem is a warning plus
    /// degraded content — check [`CosDocument::ladder_level`] and
    /// [`CosDocument::warnings`] to see what had to be repaired.
    pub fn open(bytes: impl Into<Arc<[u8]>>) -> Result<CosDocument, OpenError> {
        let buffer: Arc<[u8]> = bytes.into();
        let names = DocNames::new();
        let mut sink = WarningSink::new();

        // 7.5.2: bytes before %PDF- shift every offset the file stores.
        let shift = xref::header_shift(&buffer, &mut sink);
        let built = match xref::startxref(&buffer, &mut sink) {
            Some(start) => xref::build(&buffer, start, shift, &names, &mut sink),
            None => XrefBuild::default(),
        };

        let mut table = built.table;
        let mut trailer = built.trailer;
        let mut revisions = built.revisions;
        let mut scan: Option<Arc<ScanIndex>> = None;
        let mut ladder = LadderLevel::Trust;

        let usable = built.sections > 0 && !table.is_empty() && root_locatable(&table, &trailer);
        let mut failures = Vec::new();
        let mut offset_entries = 0usize;
        if usable {
            let validation = validate(&buffer, &mut table, shift);
            failures = validation.failures;
            offset_entries = validation.offsets;
        }

        // Level 3 when the tables never worked, or when so many entries lie
        // that patching them one at a time is worse than not believing any.
        let rescan = !usable
            || (failures.len() >= limits::LADDER_RESCAN_MIN_FAILURES
                && failures.len() * 2 > offset_entries);

        if rescan {
            ladder = LadderLevel::Rescan;
            sink.warn(0, WarningKind::DocumentRescanned);
            let index = ScanIndex::build(&buffer, &names);
            if index.is_empty() {
                return Err(OpenError::NoObjects);
            }
            let mut rebuilt = XrefTable::new();
            for (num, hit) in index.objects() {
                rebuilt.insert_new(
                    num,
                    XrefEntry::Offset {
                        offset: hit.offset,
                        gen: hit.gen,
                    },
                );
            }
            table = rebuilt;
            trailer = index.synthesized_trailer(trailer, &mut sink);
            if revisions.is_empty() {
                revisions.push(Revision {
                    byte_range: 0..buffer.len() as u64,
                    trailer: trailer.clone(),
                });
            }
            scan = Some(Arc::new(index));
        } else if !failures.is_empty() {
            ladder = LadderLevel::Patch;
            let index = Arc::new(ScanIndex::build(&buffer, &names));
            for num in failures {
                let offset = match table.get(num) {
                    Some(XrefEntry::Offset { offset, .. }) => offset,
                    _ => 0,
                };
                sink.warn(offset, WarningKind::ObjectHeaderMismatch);
                match index.get(num) {
                    Some(hit) => {
                        table.replace(
                            num,
                            XrefEntry::Offset {
                                offset: hit.offset,
                                gen: hit.gen,
                            },
                        );
                        sink.warn_at(
                            hit.offset,
                            Some(ObjRef::new(num, hit.gen)),
                            WarningKind::ObjectRepaired,
                        );
                    }
                    None => sink.warn_at(
                        offset,
                        Some(ObjRef::new(num, 0)),
                        WarningKind::ObjectMissing,
                    ),
                }
            }
            scan = Some(index);
        }

        let mut doc = CosDocument {
            buffer,
            names,
            xref: table,
            trailer,
            revisions,
            store: SlotStore::new(),
            objstm: ObjStmCache::new(),
            scan,
            warnings: Mutex::new(WarningSink::new()),
            stream_ranges: RwLock::new(HashMap::new()),
            decryptor: Arc::new(IdentityDecryptor),
            has_decryptor: false,
            encrypt: None,
            auth_level: AuthLevel::None,
            ladder,
        };
        doc.absorb(sink);
        if ladder == LadderLevel::Rescan {
            // The scanner sees `N G obj` headers, which objects inside an
            // object stream do not have. Their containers do, so the streams
            // found by the scan are expanded into type-2 entries (7.5.7).
            doc.expand_object_streams();
        }
        doc.extract_encrypt();
        Ok(doc)
    }

    /// The document trailer (7.5.5), merged across revisions newest-first so
    /// a key an incremental update dropped is still found where it was set.
    pub fn trailer(&self) -> &Dict {
        &self.trailer
    }

    /// The merged cross-reference table.
    pub fn xref(&self) -> &XrefTable {
        &self.xref
    }

    /// The incremental-update revisions (7.5.6), newest first.
    pub fn revisions(&self) -> &[Revision] {
        &self.revisions
    }

    /// The symbol for `bytes` in this document's name table.
    ///
    /// A [`Name`] is only meaningful against the table that issued it, and
    /// every document has its own, so a caller looking up a key outside the
    /// pre-interned set goes through here.
    pub fn intern(&self, bytes: &[u8]) -> Name {
        self.names.table.intern(bytes)
    }

    /// The bytes behind a symbol, or `None` if it came from another document.
    pub fn name_bytes(&self, name: Name) -> Option<Arc<[u8]>> {
        self.names.table.bytes(name)
    }

    /// Which rung of the ladder this document opened on.
    pub fn ladder_level(&self) -> LadderLevel {
        self.ladder
    }

    /// Everything this layer had to tolerate, in the order it happened.
    ///
    /// A copy rather than a slice: objects load lazily behind `&self`, so
    /// warnings keep arriving after `open` returns and no borrow of the
    /// document could stay valid across the next read.
    pub fn warnings(&self) -> Vec<Warning> {
        self.warnings.lock_safe().warnings().to_vec()
    }

    /// The `/Encrypt` scalars and the first `/ID` element, if the file is
    /// encrypted, as plain values for the security handler to consume.
    pub fn encrypt_params(&self) -> Option<&EncryptParams> {
        self.encrypt.as_ref()
    }

    /// Installs the security handler's decryptor.
    ///
    /// Every object loaded so far is forgotten, because strings decrypt when
    /// their containing object loads. `Arc`s already handed out keep the
    /// values they were given.
    pub fn set_decryptor(&mut self, decryptor: Arc<dyn Decryptor>) {
        self.decryptor = decryptor;
        self.has_decryptor = true;
        self.store.clear();
        self.objstm = ObjStmCache::new();
    }

    /// The document catalog (7.7.2), resolved from the trailer's `/Root`.
    ///
    /// `None` only when no catalog could be found at all — after a rescan the
    /// synthesized trailer points at the best candidate the scanner saw.
    pub fn catalog(&self) -> Option<Arc<Dict>> {
        let root = self.trailer().get_ref(Name::ROOT)?;
        let object = self.get(root).ok()?;
        object.as_dict().map(|d| Arc::new(d.clone()))
    }

    /// The version from the `%PDF-` header (7.5.2), as "1.7".
    ///
    /// Read from the buffer rather than stored at open, because a document
    /// with junk before its header has one at a shifted offset and the scan
    /// that found it is cheap.
    pub fn header_version(&self) -> Option<String> {
        let window = self
            .buffer
            .get(..limits::MAX_HEADER_SCAN.min(self.buffer.len()))?;
        let at = window
            .windows(5)
            .position(|w| w == b"%PDF-")
            .map(|p| p + 5)?;
        let digits: Vec<u8> = self
            .buffer
            .get(at..(at + 8).min(self.buffer.len()))?
            .iter()
            .copied()
            .take_while(|b| b.is_ascii_digit() || *b == b'.')
            .collect();
        String::from_utf8(digits).ok().filter(|s| !s.is_empty())
    }

    /// The document's own bytes.
    ///
    /// An incremental update must reproduce them exactly as its prefix, which
    /// is what keeps a signature over the original valid.
    pub fn bytes(&self) -> &[u8] {
        &self.buffer
    }

    /// The highest object number the cross-reference table knows.
    ///
    /// New objects start above it, so an edit cannot collide with something
    /// the document already uses.
    pub fn max_object_number(&self) -> u32 {
        self.xref.max_number()
    }

    /// The offset of the most recent cross-reference section, which an
    /// incremental update chains to through `/Prev`.
    pub fn last_startxref(&self) -> u64 {
        self.revisions
            .first()
            .map_or(0, |revision| revision.byte_range.end)
    }

    /// The name table, so a writer emits the same symbols this document read.
    pub fn names_table(&self) -> &NameTable {
        &self.names.table
    }

    /// Records a warning noticed by a layer built on this one.
    ///
    /// The structural readers — the page tree, outlines, name trees — sit
    /// above the parser but their leniency belongs in the same list as
    /// everything else the document tolerated (ruling 10).
    pub fn warn(&self, kind: WarningKind) {
        self.warnings.lock_safe().warn(0, kind);
    }

    /// `/CropBox`, interned once per document.
    pub(crate) fn crop_box_name(&self) -> Name {
        self.names.crop_box
    }

    /// `/Rotate`, interned once per document.
    pub(crate) fn rotate_name(&self) -> Name {
        self.names.rotate
    }

    /// Whether the document declares an `/Encrypt` dictionary.
    pub fn is_encrypted(&self) -> bool {
        self.encrypt.is_some()
    }

    /// How far the accepted password got. [`AuthLevel::None`] until one is.
    pub fn auth_level(&self) -> AuthLevel {
        self.auth_level
    }

    /// The permission flags, respecting the authentication level: an
    /// unencrypted document and one opened with the owner password both
    /// permit everything.
    pub fn permissions(&self) -> Permissions {
        crate::security::permissions(self.encrypt.as_ref(), self.auth_level)
    }

    /// Tries `password` and, if it matches, installs the decryptor.
    ///
    /// The owner password is tried first, so a document whose two passwords
    /// are equal authenticates as the owner. Whatever the handler had to
    /// tolerate reaching that answer becomes document warnings.
    pub fn authenticate(&mut self, password: &str) -> Result<AuthLevel, AuthError> {
        let params = self.encrypt.clone().ok_or(AuthError::NotEncrypted)?;
        let auth = crate::security::authenticate(&params, password)?;

        {
            let mut sink = self.warnings.lock_safe();
            for note in auth.notes {
                sink.warn(0, WarningKind::SecurityHandler(note));
            }
        }

        self.set_decryptor(auth.decryptor);
        self.auth_level = auth.level;
        Ok(auth.level)
    }

    /// How many object streams were actually decompressed.
    ///
    /// A diagnostic: fifty objects resolved out of one container must report
    /// one decode. Concurrent first reads of the same container may each
    /// decode it, since neither blocks the other, and only one result is kept.
    pub fn object_stream_decodes(&self) -> u64 {
        self.objstm.decodes()
    }

    /// The object `r` names, loading it if this is its first read.
    ///
    /// A reference to an object that does not exist reads as [`Object::Null`]
    /// (7.3.10), not as an error.
    ///
    /// The object number is the identity: `r.gen` is not part of the lookup,
    /// because a cross-reference table defines exactly one entry per object
    /// number and files whose tables disagree with their own `N G obj` headers
    /// about the generation are common. The header wins, and reading through a
    /// stale generation still finds the object every other reader finds.
    ///
    /// # Errors
    /// Never, today. The result type is the API's, because a later phase's
    /// per-object failures belong here rather than in a panic.
    pub fn get(&self, r: ObjRef) -> Result<Arc<Object>, CosError> {
        let mut sink = WarningSink::new();
        let mut ctx = ResolveCtx::new();
        let object = self.load(r.num, &mut ctx, &mut sink);
        self.absorb(sink);
        Ok(object)
    }

    /// Follows indirect references until something else comes back.
    ///
    /// Depth-capped independently of the load path, so a `Ref → Ref → Ref`
    /// chain terminates even when every link exists.
    pub fn resolve(&self, object: &Object) -> Arc<Object> {
        let mut sink = WarningSink::new();
        let mut ctx = ResolveCtx::new();
        let resolved = match object.as_objref() {
            Some(r) => self.resolve_ref(r, &mut ctx, &mut sink),
            None => Arc::new(object.clone()),
        };
        self.absorb(sink);
        resolved
    }

    /// The value of `key` in `dict`, resolved.
    pub fn resolve_key(&self, dict: &Dict, key: Name) -> Arc<Object> {
        match dict.get(key) {
            Some(object) => self.resolve(object),
            None => Arc::new(Object::Null),
        }
    }

    // ---- internals -------------------------------------------------------

    pub(crate) fn absorb(&self, mut sink: WarningSink) {
        if sink.is_empty() {
            return;
        }
        self.warnings.lock_safe().extend(sink.take());
    }

    pub(crate) fn encrypted(&self) -> bool {
        self.has_decryptor
    }

    /// The reference for an object number, taking the generation from the
    /// table, which validation has already reconciled with the file's own
    /// `N G obj` header.
    fn ref_of(&self, num: u32) -> ObjRef {
        let gen = match self.xref.get(num) {
            Some(XrefEntry::Offset { gen, .. }) | Some(XrefEntry::Free { gen, .. }) => gen,
            // 7.5.7: objects in an object stream always have generation 0.
            Some(XrefEntry::InStream { .. }) | None => 0,
        };
        ObjRef::new(num, gen)
    }

    fn entry_offset(&self, num: u32) -> u64 {
        match self.xref.get(num) {
            Some(XrefEntry::Offset { offset, .. }) => offset,
            _ => 0,
        }
    }

    pub(crate) fn resolve_ref(
        &self,
        r: ObjRef,
        ctx: &mut ResolveCtx,
        sink: &mut WarningSink,
    ) -> Arc<Object> {
        let mut current = r;
        for _ in 0..limits::MAX_RESOLVE_DEPTH {
            let object = self.load(current.num, ctx, sink);
            match object.as_objref() {
                Some(next) => current = next,
                None => return object,
            }
        }
        sink.warn(
            self.entry_offset(current.num),
            WarningKind::ResolveDepthCapHit,
        );
        Arc::new(Object::Null)
    }

    pub(crate) fn resolve_in(
        &self,
        object: &Object,
        ctx: &mut ResolveCtx,
        sink: &mut WarningSink,
    ) -> Object {
        match object.as_objref() {
            Some(r) => (*self.resolve_ref(r, ctx, sink)).clone(),
            None => object.clone(),
        }
    }

    /// Loads one object, publishing it with a compare-and-swap.
    fn load(&self, num: u32, ctx: &mut ResolveCtx, sink: &mut WarningSink) -> Arc<Object> {
        if let Some(object) = self.store.get(num) {
            return object;
        }
        if ctx.contains(num) {
            sink.warn_at(
                self.entry_offset(num),
                Some(self.ref_of(num)),
                WarningKind::ObjectCycle,
            );
            return Arc::new(Object::Null);
        }
        if ctx.depth() >= limits::MAX_LOAD_DEPTH {
            sink.warn_at(
                self.entry_offset(num),
                Some(self.ref_of(num)),
                WarningKind::LoadDepthCapHit,
            );
            return Arc::new(Object::Null);
        }
        // No lock is held here: two threads racing on the same object both
        // parse it and one wins the swap.
        let object = ctx.enter(num, |ctx| self.load_uncached(num, ctx, sink));
        self.store.publish(num, Arc::new(object))
    }

    fn load_uncached(&self, num: u32, ctx: &mut ResolveCtx, sink: &mut WarningSink) -> Object {
        let entry = self.xref.get(num);
        if let Some(XrefEntry::InStream { stream_num, idx }) = entry {
            return self.load_from_objstm(num, stream_num, idx, ctx, sink);
        }
        if let Some(XrefEntry::Offset { offset, .. }) = entry {
            if let Some(object) = self.parse_at(num, offset, ctx, sink) {
                return object;
            }
            sink.warn(offset, WarningKind::ObjectHeaderMismatch);
        }
        // Level 2 at read time: an entry that was good at open but is not the
        // object it claimed, or a type-2 entry whose container fell over.
        if let Some(hit) = self.scan.as_ref().and_then(|scan| scan.get(num)) {
            if let Some(object) = self.parse_at(num, hit.offset, ctx, sink) {
                sink.warn_at(
                    hit.offset,
                    Some(ObjRef::new(num, hit.gen)),
                    WarningKind::ObjectRepaired,
                );
                return object;
            }
        }
        if matches!(entry, Some(XrefEntry::Offset { .. })) {
            sink.warn(self.entry_offset(num), WarningKind::ObjectMissing);
        }
        // 7.3.10: a reference to a non-existent object is a reference to null.
        Object::Null
    }

    /// Parses the object whose header sits at `offset`, if that header names
    /// `num`. The header check is what makes level 1 of the ladder safe.
    fn parse_at(
        &self,
        num: u32,
        offset: u64,
        ctx: &mut ResolveCtx,
        sink: &mut WarningSink,
    ) -> Option<Object> {
        let mut local = WarningSink::new();
        let parsed = parse_indirect_at(&self.buffer, offset, &self.names.table, &mut local)?;
        if parsed.reference.num != num {
            return None;
        }
        sink.extend(local.take());
        let mut object = parsed.object;

        // 7.3.8.2: an indirect /Length is resolved on the load path, inside
        // this object's own context, so a /Length pointing into its own
        // stream is a cycle that reads as null rather than a hang.
        let length_ref = match &object {
            Object::Stream(stream) if stream.len_hint.is_none() => {
                stream.dict.get(Name::LENGTH).and_then(Object::as_objref)
            }
            _ => None,
        };
        if let Some(length_ref) = length_ref {
            let resolved = self.resolve_ref(length_ref, ctx, sink);
            let length = match &*resolved {
                Object::Int(n) if *n >= 0 => u64::try_from(*n).ok(),
                Object::Null => None,
                _ => {
                    sink.warn(offset, WarningKind::StreamLengthNotAnInteger);
                    None
                }
            };
            if let Object::Stream(stream) = &mut object {
                stream.len_hint = length;
            }
        }

        // 7.6.2: strings decrypt with the containing object's reference. The
        // /Encrypt dictionary is the one object exempt from it.
        if self.has_decryptor && self.encrypt_num() != Some(num) {
            decrypt::decrypt_strings(
                &mut object,
                ObjRef::new(num, parsed.reference.gen),
                self.decryptor.as_ref(),
            );
        }
        Some(object)
    }

    fn load_from_objstm(
        &self,
        num: u32,
        stream_num: u32,
        idx: u32,
        ctx: &mut ResolveCtx,
        sink: &mut WarningSink,
    ) -> Object {
        let Some(stm) = self.object_stream(stream_num, ctx, sink) else {
            sink.warn_at(
                self.entry_offset(stream_num),
                Some(ObjRef::new(num, 0)),
                WarningKind::ObjStmNotAStream,
            );
            return Object::Null;
        };
        let at = self.entry_offset(stream_num);
        let Some(offset) = stm.locate(num, idx, at, sink) else {
            return Object::Null;
        };

        let mut local = WarningSink::new();
        let parsed = parse_object_at(&stm.data, offset, &self.names.table, &mut local);
        // 7.5.7: a contained object is never itself a stream.
        if objstm::is_stream_at(&stm.data, parsed.end_offset) {
            sink.warn_at(
                at,
                Some(ObjRef::new(num, 0)),
                WarningKind::ObjStmEntryIsStream,
            );
            return Object::Null;
        }
        sink.extend(local.take());
        // 7.6.2: no per-string decryption here — the container was decrypted
        // as a whole, and decrypting its contents again would corrupt them.
        parsed.object
    }

    /// The decompressed object stream `num`, decoded at most once.
    pub(crate) fn object_stream(
        &self,
        num: u32,
        ctx: &mut ResolveCtx,
        sink: &mut WarningSink,
    ) -> Option<Arc<ObjStm>> {
        if let Some(stm) = self.objstm.get(num) {
            return Some(stm);
        }
        let container = self.load(num, ctx, sink);
        let stream = container.as_stream()?;
        let r = self.ref_of(num);
        let data = self.decrypted_bytes(r, stream);
        let decoded = self.decode_with(&data, &stream.dict, num, sink).ok()?;
        self.objstm.count_decode();

        let n = self
            .resolve_in(stream.dict.get(Name::N).unwrap_or(&Object::Null), ctx, sink)
            .as_int()
            .and_then(|v| u64::try_from(v).ok());
        let first = self
            .resolve_in(
                stream.dict.get(Name::FIRST).unwrap_or(&Object::Null),
                ctx,
                sink,
            )
            .as_int()
            .and_then(|v| u64::try_from(v).ok());
        let parsed = ObjStm::parse(decoded, n, first, sink, self.entry_offset(num));
        Some(self.objstm.publish(num, Arc::new(parsed)))
    }

    fn encrypt_num(&self) -> Option<u32> {
        self.encrypt
            .as_ref()
            .and_then(|e| e.encrypt_ref)
            .map(|r| r.num)
    }

    fn extract_encrypt(&mut self) {
        let Some(entry) = self.trailer.get(Name::ENCRYPT).cloned() else {
            return;
        };
        let (dict, reference) = match entry {
            Object::Dict(dict) => (Some(dict), None),
            Object::Ref(r) => {
                let object = self.get(r).unwrap_or_else(|_| Arc::new(Object::Null));
                (object.as_dict().cloned(), Some(r))
            }
            _ => (None, None),
        };
        if let Some(dict) = dict {
            self.encrypt = Some(decrypt::extract(
                &dict,
                reference,
                &self.trailer,
                &self.names,
            ));
        }
    }

    fn expand_object_streams(&mut self) {
        let Some(scan) = self.scan.clone() else {
            return;
        };
        let mut sink = WarningSink::new();
        let mut additions = Vec::new();
        for &stream_num in scan.object_streams() {
            let mut ctx = ResolveCtx::new();
            let Some(stm) = self.object_stream(stream_num, &mut ctx, &mut sink) else {
                continue;
            };
            for (idx, (num, _)) in stm.entries.iter().enumerate() {
                if let Ok(idx) = u32::try_from(idx) {
                    additions.push((*num, XrefEntry::InStream { stream_num, idx }));
                }
            }
        }
        for (num, entry) in additions {
            self.xref.insert_new(num, entry);
        }
        self.absorb(sink);
    }
}

struct Validation {
    failures: Vec<u32>,
    offsets: usize,
}

/// Checks every type-1 entry against its `N G obj` header, rewriting the ones
/// that are merely shifted and reporting the ones that are wrong.
///
/// Eager rather than lazy on purpose: the ladder must be decided by the bytes
/// alone, not by which objects a caller happened to read first, or "it opened
/// cleanly" would depend on the reader's access pattern.
fn validate(buffer: &[u8], table: &mut XrefTable, shift: u64) -> Validation {
    let mut failures = Vec::new();
    let mut fixes = Vec::new();
    let mut offsets = 0usize;
    for (num, entry) in table.iter() {
        let XrefEntry::Offset { offset, .. } = entry else {
            continue;
        };
        offsets += 1;
        let found = xref::offset_candidates(buffer.len() as u64, offset, shift)
            .into_iter()
            .find_map(|at| {
                xref::object_header_at(buffer, at)
                    .filter(|h| h.num == num)
                    .map(|h| (at, h.gen))
            });
        match found {
            // The header is the authority on the generation: a table that
            // disagrees with it is the thing being repaired.
            Some((offset, gen)) => fixes.push((num, XrefEntry::Offset { offset, gen })),
            None => failures.push(num),
        }
    }
    for (num, entry) in fixes {
        table.replace(num, entry);
    }
    Validation { failures, offsets }
}

fn root_locatable(table: &XrefTable, trailer: &Dict) -> bool {
    match trailer.get(Name::ROOT) {
        Some(Object::Dict(_)) => true,
        Some(Object::Ref(r)) => table
            .get(r.num)
            .is_some_and(|e| !matches!(e, XrefEntry::Free { .. })),
        _ => false,
    }
}
