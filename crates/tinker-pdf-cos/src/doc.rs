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
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock, RwLock};

use tinker_pdf_crypto::handler::FileKey;
use tinker_pdf_crypto::Permissions;
use tinker_pdf_filters::FilterError;

use crate::decrypt::{self, Decryptor, EncryptParams, IdentityDecryptor};
use crate::limits;
use crate::name::{Name, NameTable};
use crate::objstm::{self, ObjStm, ObjStmCache};
use crate::parse::{parse_indirect_at, parse_object_at, ParsedIndirect};
use crate::repair::{find_from, next_object_header, rfind_from, ScanIndex};
use crate::security::{AuthError, AuthLevel};
use crate::source::SourceMiss;
use crate::source::{Backing, ByteSource, Bytes};
use crate::store::{LockExt, MutexExt, ResolveCtx, SlotStore};
use crate::validate::hints;
use crate::warn::{Warning, WarningKind, WarningSink};
use crate::work::Work;
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
/// `Clone` but not `Copy`: [`OpenError::SourceUnavailable`] carries the range
/// that was wanted, and a range is not `Copy`. Naming the bytes is worth more
/// than the convenience — a host told only "a range was missing" has to guess
/// which one to fetch.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum OpenError {
    /// Not one indirect object could be located, even after a full rescan.
    /// This is not a PDF by any reader's definition.
    NoObjects,
    /// The source could not supply the bytes an open must have.
    ///
    /// Kept apart from [`OpenError::NoObjects`] because the right response is
    /// the opposite: a host told "not a PDF" stops, and a host told this
    /// fetches the range named and calls again. Collapsing them makes the
    /// whole [`crate::ByteSource`] seam unusable for the thing it exists for.
    ///
    /// Only the head window produces it. A miss further in has the rescan
    /// ladder underneath it and degrades (ruling 2) rather than failing; a
    /// miss at byte zero has nothing underneath it at all.
    SourceUnavailable(SourceMiss),
}

impl fmt::Display for OpenError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            OpenError::NoObjects => f.write_str("no indirect objects found"),
            OpenError::SourceUnavailable(miss) => write!(f, "{miss}"),
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
///
/// The table is shared, so a document reopened over its base (P-17.4) reads
/// the base's objects and its own with one set of symbols: an object the base
/// parsed and the reopened document hands out names what the reopened
/// document's own `intern` names.
#[derive(Clone)]
pub(crate) struct DocNames {
    pub table: Arc<NameTable>,
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
    /// `/Metadata`, whose stream `/EncryptMetadata false` leaves in the clear.
    pub metadata: Name,
    /// `/Identity`, the crypt filter that applies no encryption.
    pub identity: Name,
    /// `/Name`, the key inside a `/Crypt` filter parameters dictionary.
    pub name_key: Name,
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
    pub recipients: Name,
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
            metadata: n(b"Metadata"),
            identity: n(b"Identity"),
            name_key: n(b"Name"),
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
            recipients: n(b"Recipients"),
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
            table: Arc::new(table),
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
    pub(crate) buffer: Backing,
    pub(crate) names: DocNames,
    xref: XrefTable,
    /// Annex F's head-only open, when it engaged. `None` for every document
    /// opened from a buffer.
    linearized: Option<Linearized>,
    /// Annex F's hint tables, unpacked at the first request for a page other
    /// than the first.
    ///
    /// Behind a lock rather than a `OnceLock` because authentication can
    /// change the answer: the hint stream is an ordinary stream object and is
    /// encrypted like one (7.6.1 exempts only three things and this is not
    /// among them), so on an encrypted document it decodes to nothing until a
    /// password arrives. A refusal reached over ciphertext is dropped by
    /// `install_security` along with everything else read through it.
    page_hints: Mutex<HintState>,
    /// The page index (`crate::pages`): every page one walk found, kept
    /// beside the object store so every clone of a `Document` shares it, and
    /// dropped with the store by `install_security`.
    page_index: Mutex<crate::pages::IndexSlot>,
    /// How many loads have missed: read as null because their bytes were not
    /// available, and so not published. What a walk built over loads is kept
    /// only when this did not move under it (`crate::pages`).
    misses: AtomicU64,
    /// Where the hint tables said an object was.
    ///
    /// Consulted after the first-page table and before the main one, so a page
    /// the tables can place costs its own bytes and not the tail. Every entry
    /// here was read out of the file's own `N G obj` header inside a range the
    /// file's own tables named, and every one of them is checked again by
    /// [`CosDocument::parse_at`] when the object loads.
    hinted: RwLock<HashMap<u32, XrefEntry>>,
    /// The first-page table merged with the main one, once a read has left
    /// page one and paid for it.
    completed: OnceLock<XrefTable>,
    trailer: Dict,
    revisions: Vec<Revision>,
    store: SlotStore,
    objstm: ObjStmCache,
    /// The repair scanner's index, when the ladder needed one.
    ///
    /// Behind a lock because a streamed document builds it *lazily*: the
    /// eager validation that decides on one at open is the step a streaming
    /// open defers, so the decision moves to the first read the table cannot
    /// place, and is made from the verdict that validation reaches. The values
    /// a caller sees are the same either way, which is the property that
    /// matters; what differs is when the fetch happens.
    scan: RwLock<Option<Arc<ScanIndex>>>,
    warnings: Mutex<WarningSink>,
    pub(crate) stream_ranges: RwLock<HashMap<u32, Range<u64>>>,
    /// Everything authentication installs, behind a lock.
    ///
    /// Interior mutability rather than `&mut self` because a document is
    /// shared the moment a caller takes a page — every `Page` holds a clone of
    /// the same `Arc` — and an `Arc::get_mut` at that point fails. Requiring
    /// unique ownership to authenticate meant "look at a page, then supply the
    /// password" reported the document as unencrypted.
    security: RwLock<Security>,
    encrypt: Option<EncryptParams>,
    ladder: LadderLevel,
    /// 7.5.2's header shift, which every offset the file states is measured
    /// against. A buffered open rewrites each shifted entry when it validates;
    /// a streamed one keeps them as stated, and this is what its verdict and
    /// its reads are measured by instead.
    shift: u64,
    /// The ladder level a buffered open of the same bytes decides, once a
    /// streamed document has worked it out through windows. Kept because the
    /// bytes cannot change under it, and only once known: a verdict a missed
    /// read cut short is no verdict.
    verdict: OnceLock<LadderLevel>,
    /// The document this one was reopened over, when it was
    /// ([`CosDocument::reopen`]): `xref` is then the update's own entries,
    /// and every number they leave alone is read through it.
    base: Option<Reopen>,
    /// How many times security has been installed on this document. A
    /// reopened document reads through its base only while neither its own
    /// count nor its base's has moved since the reopen.
    security_epoch: AtomicU64,
}

/// What a reopened document keeps of its base (`docs/design/performance.md`,
/// P-17).
struct Reopen {
    /// The base.
    doc: Arc<CosDocument>,
    /// The base's security count at the reopen.
    base_epoch: u64,
    /// This document's, once the base's security was given it.
    own_epoch: u64,
    /// How many reopens deep this document is, counting its own.
    depth: u32,
}

impl Reopen {
    /// Whether `doc`, which was reopened over this base, still reads through
    /// it: neither document's security has been installed since.
    fn current(&self, doc: &CosDocument) -> bool {
        self.doc.security_epoch.load(Ordering::Acquire) == self.base_epoch
            && doc.security_epoch.load(Ordering::Acquire) == self.own_epoch
    }
}

/// Why [`CosDocument::reopen`] opened the whole document again rather than
/// reading only the update (`docs/design/performance.md`, P-17).
///
/// Each is a test of the base or of the update the reopen has just read, and
/// nothing else sends a reopen to a full open.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum FullOpen {
    /// The base opened on Annex F's head-only path: its table is the first
    /// page's until a read fetches the main one, and its one revision is the
    /// whole file.
    Linearized,
    /// The base's bytes come from a source: they are fetched whole first,
    /// which the fetch declares (ruling 10).
    Streamed,
    /// The base's ladder is `Patch` or `Rescan`: a full open of the joined
    /// bytes repairs from a scan of all of them.
    Repaired,
    /// The base's header is not at byte zero (7.5.2), so every offset the
    /// update states is read through the shift's candidates.
    Shifted,
    /// The base is itself [`limits::MAX_REOPEN_DEPTH`] reopens deep; the full
    /// open starts a chain of its own.
    Deep,
    /// The update does not chain to the base: its `startxref`,
    /// cross-reference data or trailer does not read cleanly, its `/Prev` is
    /// not the base's last section, its `/Encrypt` or an encrypted base's first
    /// `/ID` string is not the base's, it redefines the `/Encrypt` dictionary,
    /// an entry's header is not the object it names, the base's last
    /// revision has no `%%EOF` of its own to end at, or the base's chain is
    /// already as long as a full open walks.
    Unchained,
}

/// A document [`CosDocument::reopen`] opened over a base and an update
/// appended to it.
#[derive(Debug)]
pub struct Reopened {
    /// The document: the base's bytes followed by the update.
    pub document: CosDocument,
    /// Why the whole document was opened again, when it was; `None` when
    /// only the update was read.
    pub full_open: Option<FullOpen>,
}

/// The state a successful authentication installs.
struct Security {
    decryptor: Arc<dyn Decryptor>,
    has_decryptor: bool,
    auth_level: AuthLevel,
    /// The authenticated file key, for the one caller that has to *encrypt*:
    /// an incremental update appends into a file whose `/Encrypt` still
    /// stands, so it has to reproduce that file's encryption rather than
    /// invent its own.
    key: Option<FileKey>,
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
        CosDocument::open_backing(Backing::whole_buffer(bytes.into()), None)
    }

    /// [`CosDocument::open`], counting what the open read into `work`: the
    /// tokens lexed, the cross-reference entries read, the objects parsed and
    /// the bytes the repair scan looked through, the `/Encrypt` dictionary's
    /// load and the object streams a rescan expands included
    /// (`docs/design/resource-exhaustion.md`, "The counters").
    ///
    /// The document is the one [`CosDocument::open`] returns; nothing is kept
    /// on it, so a later read counts only through
    /// [`CosDocument::get_counted`].
    ///
    /// # Errors
    /// As [`CosDocument::open`]; a failed open's steps are counted too.
    pub fn open_counted(
        bytes: impl Into<Arc<[u8]>>,
        work: &Work,
    ) -> Result<CosDocument, OpenError> {
        CosDocument::open_backing(Backing::whole_buffer(bytes.into()), Some(work))
    }

    /// Opens a document whose bytes are fetched from `source` as they are
    /// needed.
    ///
    /// The same engine and the same answers: `source` is where bytes come
    /// from, never what they mean. A document opened here and the same
    /// document opened from a buffer produce the same objects, the same
    /// warnings, the same ladder level and the same pixels, whatever order or
    /// size the source answered in -- see `docs/design/streaming-open.md`, and
    /// `ShreddedSource` for the instrument that proves it.
    ///
    /// # Errors
    /// [`OpenError::NoObjects`] as [`CosDocument::open`], and for a source
    /// that could not supply the bytes the open path needed.
    pub fn open_source(source: Arc<dyn ByteSource>) -> Result<CosDocument, OpenError> {
        CosDocument::open_backing(Backing::chunked(source), None)
    }

    /// Opens `original` followed by `update`, an incremental section written
    /// after it, **without joining them**: the document [`CosDocument::open`]
    /// gives the concatenated bytes, object for object, read from the two
    /// where they lie.
    ///
    /// Both are in hand, so this opens as a buffer opens — the
    /// cross-reference chain walked, every entry probed against its header
    /// and the ladder decided at open, no fetch declared — and only the
    /// operations that need every byte as one slice ([`CosDocument::bytes`],
    /// a signature's byte range, the repair scanner) join them, once. A read
    /// inside either is that one's bytes, and only a read across the seam is
    /// copied, its own range alone (`docs/design/performance.md`, P-17).
    /// [`crate::DocumentEditor::save_update`] writes such a section.
    ///
    /// # Errors
    /// As [`CosDocument::open`].
    pub fn open_appended(
        original: impl Into<Arc<[u8]>>,
        update: impl Into<Arc<[u8]>>,
    ) -> Result<CosDocument, OpenError> {
        CosDocument::open_backing(
            Backing::segments(vec![original.into(), update.into()]),
            None,
        )
    }

    /// Reopens `base` with `update`, an incremental section written after its
    /// bytes, appended to them — reading only the update, and keeping the
    /// base's state for everything the update leaves alone
    /// (`docs/design/performance.md`, P-17).
    ///
    /// Of all the bytes it reads the update's `startxref`, its one
    /// cross-reference section and its trailer, and the header of each object
    /// the section places. Every number the section does not define is the
    /// base's: its entry, its parsed object and its decoded object stream,
    /// read through the base rather than again. A number the section defines
    /// is parsed afresh from the section, and so is an object whose parse in
    /// the base read a number the section redefines — a stream's indirect
    /// `/Length`, or an object stream's container and its `/Length`, `/N` and
    /// `/First`. The document reads as [`CosDocument::open`] reads the joined
    /// bytes holding the base's security, as [`crate::DocumentEditor::view`]
    /// gives a view its document's: its warnings are the base's, as they
    /// stand, and then its own, and its ladder level is the base's. Its page
    /// index is its own, since an update can change the page tree.
    ///
    /// A base the reopen cannot keep, or an update that does not chain to it,
    /// is opened whole instead, over segments as
    /// [`CosDocument::open_appended`] opens them and with the base's
    /// security, and [`Reopened::full_open`] says why ([`FullOpen`]).
    ///
    /// # Errors
    /// As [`CosDocument::open`], for a reopen that opens whole, and
    /// [`OpenError::SourceUnavailable`] for a streamed base whose bytes
    /// cannot all be fetched.
    pub fn reopen(
        base: &Arc<CosDocument>,
        update: impl Into<Arc<[u8]>>,
    ) -> Result<Reopened, OpenError> {
        CosDocument::reopen_with(base, update.into(), None)
    }

    /// [`CosDocument::reopen`], counting what it read into `work` as
    /// [`CosDocument::open_counted`] counts an open: the update's tokens and
    /// rows, and all a full open reads when the reopen opens whole.
    ///
    /// # Errors
    /// As [`CosDocument::reopen`].
    pub fn reopen_counted(
        base: &Arc<CosDocument>,
        update: impl Into<Arc<[u8]>>,
        work: &Work,
    ) -> Result<Reopened, OpenError> {
        CosDocument::reopen_with(base, update.into(), Some(work))
    }

    fn reopen_with(
        base: &Arc<CosDocument>,
        update: Arc<[u8]>,
        work: Option<&Work>,
    ) -> Result<Reopened, OpenError> {
        let refused = if base.linearized.is_some() {
            Some(FullOpen::Linearized)
        } else if base.buffer.is_streamed() {
            Some(FullOpen::Streamed)
        } else if base.ladder != LadderLevel::Trust {
            Some(FullOpen::Repaired)
        } else if base.shift > 0 {
            Some(FullOpen::Shifted)
        } else if base.reopen_depth() >= limits::MAX_REOPEN_DEPTH {
            Some(FullOpen::Deep)
        } else {
            None
        };
        if let Some(reason) = refused {
            return CosDocument::open_whole(base, update, work, reason);
        }
        let mut parts = base.buffer.parts().unwrap_or_default();
        parts.push(Arc::clone(&update));
        match CosDocument::read_update(base, Backing::segments(parts), work) {
            Some(document) => Ok(Reopened {
                document,
                full_open: None,
            }),
            None => CosDocument::open_whole(base, update, work, FullOpen::Unchained),
        }
    }

    /// `base`'s bytes and `update` opened whole over segments, with `base`'s
    /// security, for `reason`. A streamed base is fetched whole first and
    /// says so, as every whole-file fetch does (ruling 10).
    fn open_whole(
        base: &Arc<CosDocument>,
        update: Arc<[u8]>,
        work: Option<&Work>,
        reason: FullOpen,
    ) -> Result<Reopened, OpenError> {
        let mut parts = match base.buffer.parts() {
            Some(parts) => parts,
            None => {
                let declare = !base.buffer.whole_fetched();
                let bytes = base
                    .buffer
                    .materialise()
                    .map_err(OpenError::SourceUnavailable)?;
                if declare {
                    base.warn(WarningKind::WholeFileFetched);
                }
                vec![bytes]
            }
        };
        parts.push(update);
        let document = CosDocument::open_backing(Backing::segments(parts), work)?;
        document.inherit_security(base);
        Ok(Reopened {
            document,
            full_open: Some(reason),
        })
    }

    /// The reopen proper: `backing`, `base`'s segments and the update, read
    /// through the update alone — or `None` when the update does not chain to
    /// the base ([`FullOpen::Unchained`]).
    fn read_update(
        base: &Arc<CosDocument>,
        backing: Backing,
        work: Option<&Work>,
    ) -> Option<CosDocument> {
        let mut sink = WarningSink::new();
        let read = CosDocument::read_section(base, &backing, &mut sink);
        if let Some(work) = work {
            sink.steps().tally(work);
        }
        let (table, trailer, revision) = read?;
        let mut revisions = Vec::with_capacity(base.revisions.len().saturating_add(1));
        revisions.push(revision);
        revisions.extend(base.revisions.iter().cloned());
        let mut doc = CosDocument {
            buffer: backing,
            names: base.names.clone(),
            xref: table,
            trailer,
            revisions,
            linearized: None,
            page_hints: Mutex::new(HintState::Untried),
            page_index: Mutex::new(crate::pages::IndexSlot::default()),
            misses: AtomicU64::new(0),
            hinted: RwLock::new(HashMap::new()),
            completed: OnceLock::new(),
            store: SlotStore::new(),
            objstm: ObjStmCache::new(),
            scan: RwLock::new(None),
            warnings: Mutex::new(WarningSink::new()),
            stream_ranges: RwLock::new(HashMap::new()),
            security: RwLock::new(Security {
                decryptor: Arc::new(IdentityDecryptor),
                has_decryptor: false,
                auth_level: AuthLevel::None,
                key: None,
            }),
            encrypt: None,
            ladder: base.ladder,
            shift: 0,
            verdict: OnceLock::new(),
            base: Some(Reopen {
                doc: Arc::clone(base),
                base_epoch: base.security_epoch.load(Ordering::Acquire),
                own_epoch: 0,
                depth: base.reopen_depth().saturating_add(1),
            }),
            security_epoch: AtomicU64::new(0),
        };
        doc.inherit_security(base);
        let own = doc.security_epoch.load(Ordering::Acquire);
        if let Some(reopen) = doc.base.as_mut() {
            reopen.own_epoch = own;
        }
        doc.extract_encrypt(work);
        Some(doc)
    }

    /// The update's cross-reference section, read alone and checked to chain
    /// to `base`: its table, with every type-1 entry probed against its header
    /// and given the header's generation as a full open's validation gives
    /// it; the trailer, the section's entries over the base's; and the
    /// section's revision. `None` for anything [`FullOpen::Unchained`] names,
    /// a warning while reading the section among them.
    fn read_section(
        base: &CosDocument,
        backing: &Backing,
        sink: &mut WarningSink,
    ) -> Option<(XrefTable, Dict, Revision)> {
        let Backing::Segments(segments) = backing else {
            return None;
        };
        let view = backing.view();
        let start = xref::startxref(&view, sink)?;
        if start < base.buffer.len() {
            return None;
        }
        let built = xref::build_limited(view, start, 0, &base.names, sink, 1);
        if built.sections != 1 || !sink.is_empty() {
            return None;
        }
        let revision = built.revisions.into_iter().next()?;
        let prev = revision
            .trailer
            .get_int(Name::PREV)
            .and_then(|prev| u64::try_from(prev).ok());
        if base.last_startxref() == 0 || prev != Some(base.last_startxref()) {
            return None;
        }
        // A full open walks the update and every section of the base's chain
        // under one cap; a chain the cap would cut is read whole, as it cuts.
        if base.revisions.len() >= limits::MAX_XREF_CHAIN as usize {
            return None;
        }
        // A full open of the joined bytes looks for the base's last `%%EOF`
        // past its trailer and stops at the first; a base whose last revision
        // found none would find the update's there.
        if !base.ends_at_its_eof() {
            return None;
        }
        let mut trailer = revision.trailer.clone();
        for (key, value) in base.trailer().iter() {
            if !trailer.contains_key(*key) {
                trailer.insert(*key, value.clone());
            }
        }
        if trailer.get(Name::ENCRYPT) != base.trailer().get(Name::ENCRYPT)
            || (base.encrypt.is_some() && first_id(&trailer) != first_id(base.trailer()))
        {
            return None;
        }
        let mut table = built.table;
        if base
            .encrypt_num()
            .is_some_and(|num| table.get(num).is_some())
        {
            return None;
        }
        let locatable = match trailer.get(Name::ROOT) {
            Some(Object::Dict(_)) => true,
            Some(Object::Ref(r)) => table
                .get(r.num)
                .or_else(|| base.entry(r.num))
                .is_some_and(|entry| !matches!(entry, XrefEntry::Free { .. })),
            _ => false,
        };
        if !locatable {
            return None;
        }
        let validation = validate_with(backing.len(), &mut table, 0, |at| segments.header_at(at));
        if !validation.failures.is_empty() {
            return None;
        }
        Some((table, trailer, revision))
    }

    /// Whether this document's newest revision ends at an `%%EOF` of its own,
    /// rather than at the end of its bytes for want of one.
    fn ends_at_its_eof(&self) -> bool {
        let Some(end) = self
            .revisions
            .first()
            .map(|revision| revision.byte_range.end)
        else {
            return false;
        };
        let Some(from) = end.checked_sub(b"%%EOF".len() as u64) else {
            return false;
        };
        self.buffer
            .window(from..end)
            .is_ok_and(|bytes| &*bytes == b"%%EOF")
    }

    /// How many reopens deep this document is: none for one opened whole.
    fn reopen_depth(&self) -> u32 {
        self.base.as_ref().map_or(0, |reopen| reopen.depth)
    }

    fn open_backing(backing: Backing, work: Option<&Work>) -> Result<CosDocument, OpenError> {
        let mut sink = WarningSink::new();
        let opened = CosDocument::open_into(backing, &mut sink, work);
        // A failed open read what it read; a successful one has handed its
        // sink to the document already, and counted its steps on the way.
        if let (Err(_), Some(work)) = (&opened, work) {
            sink.steps().tally(work);
        }
        opened
    }

    fn open_into(
        backing: Backing,
        sink: &mut WarningSink,
        work: Option<&Work>,
    ) -> Result<CosDocument, OpenError> {
        let names = DocNames::new();
        let streamed = backing.is_streamed();

        // 7.5.2: bytes before %PDF- shift every offset the file stores. One
        // head window covers the scan limit the clause sets, which on a
        // streamed source is exactly one chunk.
        let head = match backing.window(0..limits::MAX_HEADER_SCAN as u64) {
            Ok(head) => head,
            Err(miss) => return Err(OpenError::SourceUnavailable(miss)),
        };
        let shift = xref::header_shift(&head, sink);
        drop(head);

        // The walk reads windows: the tail for `startxref` (7.5.5), then each
        // section of the `/Prev` chain (7.5.4, 7.5.6, 7.5.8) as its own. A
        // document already in hand hands the walker the whole buffer for every
        // window, so it takes the path it always took at the cost it always
        // had. Segments are in hand too, and are walked through windows that
        // borrow each one whole, so they are never joined for an open.
        let held = match &backing {
            Backing::Whole(bytes) => Some(Arc::clone(bytes)),
            Backing::Chunked(_) | Backing::Segments(_) => None,
        };
        let view = match &held {
            Some(buffer) => Bytes::Whole(buffer),
            None => backing.view(),
        };
        // Annex F's fast path first, and only for a streamed document: a
        // buffer already holds every byte, so there is nothing for a head-only
        // open to save and every reason not to take a second route through the
        // same clauses.
        let mut linearized = None;
        let fast = if streamed {
            linearized_open(&backing, &names, shift, sink)
        } else {
            None
        };
        let built = match fast {
            Some((built, params)) => {
                linearized = Some(params);
                built
            }
            None => match xref::startxref(&view, sink) {
                Some(start) => xref::build(view, start, shift, &names, sink),
                None => XrefBuild::default(),
            },
        };

        let mut table = built.table;
        let mut trailer = built.trailer;
        let mut revisions = built.revisions;
        let mut scan: Option<Arc<ScanIndex>> = None;
        let mut ladder = LadderLevel::Trust;

        let usable = built.sections > 0 && !table.is_empty() && root_locatable(&table, &trailer);
        let mut failures = Vec::new();
        let mut offset_entries = 0usize;
        // Deferred on a streamed source, and only there. The eager pass
        // probes every type-1 offset against its `N G obj` header, which
        // means reading a byte near every object in the file -- the one
        // open-time step that touches everywhere, and the one thing a
        // streaming open cannot afford. Nothing is lost from safety:
        // `parse_at` re-checks the header of every object at load, so an
        // entry that lies is still caught, at first use rather than at open.
        // What is lost is that `ladder_level` starts out provisional, which
        // `CosDocument::complete_validation` is how a caller gets back the
        // eager answer. Segments are probed where each header lies.
        if usable {
            let validation = match (&held, &backing) {
                (Some(buffer), _) => Some(validate(buffer, &mut table, shift)),
                (None, Backing::Segments(segments)) => {
                    Some(validate_with(backing.len(), &mut table, shift, |at| {
                        segments.header_at(at)
                    }))
                }
                (None, _) => None,
            };
            if let Some(validation) = validation {
                failures = validation.failures;
                offset_entries = validation.offsets;
            }
        }

        // Level 3 when the tables never worked, or when so many entries lie
        // that patching them one at a time is worse than not believing any.
        let rescan = !usable
            || (failures.len() >= limits::LADDER_RESCAN_MIN_FAILURES
                && failures.len() * 2 > offset_entries);

        if rescan {
            ladder = LadderLevel::Rescan;
            sink.warn(0, WarningKind::DocumentRescanned);
            // One forward pass over everything, which is the point of it. A
            // streamed document therefore fetches the whole source, and says
            // so first (ruling 10).
            let Some(buffer) = fetch_whole(&backing, sink) else {
                return Err(OpenError::NoObjects);
            };
            let index = ScanIndex::build(&buffer, &names);
            sink.add_steps(index.steps());
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
            trailer = index.synthesized_trailer(trailer, sink);
            if revisions.is_empty() {
                revisions.push(Revision {
                    byte_range: 0..backing.len(),
                    // Nothing readable to chain to: the tables were discarded.
                    xref_at: 0,
                    trailer: trailer.clone(),
                });
            }
            scan = Some(Arc::new(index));
        } else if !failures.is_empty() {
            ladder = LadderLevel::Patch;
            let Some(buffer) = fetch_whole(&backing, sink) else {
                return Err(OpenError::NoObjects);
            };
            let index = ScanIndex::build(&buffer, &names);
            sink.add_steps(index.steps());
            let index = Arc::new(index);
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
            buffer: backing,
            names,
            xref: table,
            trailer,
            revisions,
            linearized,
            page_hints: Mutex::new(HintState::Untried),
            page_index: Mutex::new(crate::pages::IndexSlot::default()),
            misses: AtomicU64::new(0),
            hinted: RwLock::new(HashMap::new()),
            completed: OnceLock::new(),
            store: SlotStore::new(),
            objstm: ObjStmCache::new(),
            scan: RwLock::new(scan),
            warnings: Mutex::new(WarningSink::new()),
            stream_ranges: RwLock::new(HashMap::new()),
            security: RwLock::new(Security {
                decryptor: Arc::new(IdentityDecryptor),
                has_decryptor: false,
                auth_level: AuthLevel::None,
                key: None,
            }),
            encrypt: None,
            ladder,
            shift,
            verdict: OnceLock::new(),
            base: None,
            security_epoch: AtomicU64::new(0),
        };
        if let Some(work) = work {
            sink.steps().tally(work);
        }
        doc.absorb(core::mem::take(sink));
        if ladder == LadderLevel::Rescan {
            // The scanner sees `N G obj` headers, which objects inside an
            // object stream do not have. Their containers do, so the streams
            // found by the scan are expanded into type-2 entries (7.5.7).
            doc.expand_object_streams(work);
        }
        doc.extract_encrypt(work);
        Ok(doc)
    }

    /// The document trailer (7.5.5), merged across revisions newest-first so
    /// a key an incremental update dropped is still found where it was set.
    pub fn trailer(&self) -> &Dict {
        &self.trailer
    }

    /// The merged cross-reference table.
    ///
    /// On Annex F's fast path this is the whole of it: asking for the table is
    /// asking about every object, so the main table at `/T` is fetched here if
    /// a read has not already paid for it. A caller that wants page one and
    /// nothing else never calls this, which is why a page-one render still
    /// touches no byte of the tail.
    pub fn xref(&self) -> &XrefTable {
        if self.linearized.is_some() {
            return self.merged_xref();
        }
        // A reopened document reads its base's entries where they lie, and
        // builds the merged table only for a caller that asks for all of it:
        // the update's entries over the base's, newest first.
        if let Some(reopen) = &self.base {
            return self.completed.get_or_init(|| {
                let mut merged = self.xref.clone();
                for (num, entry) in reopen.doc.xref().iter() {
                    merged.insert_new(num, entry);
                }
                merged
            });
        }
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

    /// Whether `/EncryptMetadata` permits the metadata stream to be encrypted.
    ///
    /// True when the document is not encrypted at all, which keeps the
    /// question answerable without a second one about whether it applies.
    pub(crate) fn encrypts_metadata(&self) -> bool {
        self.encrypt
            .as_ref()
            .is_none_or(|params| params.encrypt_metadata)
    }

    /// Whether this document's bytes are fetched from a [`ByteSource`] rather
    /// than held in one buffer.
    ///
    /// An observable rather than a mood: a caller deciding whether an
    /// operation is about to pull the whole file needs to be able to ask.
    pub fn is_streamed(&self) -> bool {
        self.buffer.is_streamed()
    }

    /// Where the first page's objects end (Annex F `/E`), when this document
    /// was opened on the linearized fast path.
    ///
    /// `None` for every other document, which is how a caller tells that the
    /// head-only open engaged rather than assuming it from the file. Every
    /// byte from here to the end is the tail, and rendering page one touches
    /// none of it.
    pub fn first_page_end(&self) -> Option<u64> {
        self.linearized.as_ref().map(|l| l.end_of_first_page)
    }

    /// The object number of the first page (Annex F `/O`), when this document
    /// was opened on the linearized fast path.
    ///
    /// `None` for every other document. Annex F names it so that a reader
    /// holding only the head can reach page one without walking the page
    /// tree, whose root the layout is free to leave in the tail.
    pub fn first_page_object(&self) -> Option<u32> {
        self.linearized.as_ref().map(|l| l.first_page_object)
    }

    /// Whether the main cross-reference table has been fetched.
    ///
    /// Always true for a document that did not open on Annex F's head-only
    /// path: an ordinary open reads the table it finds through `startxref`, so
    /// the question only distinguishes anything for a linearized file opened
    /// from its head. For one of those this is the observable that "page *N*
    /// without the main table" is measured by -- a page reached through the
    /// hint tables leaves it false, and the first read they cannot place
    /// fetches the table at `/T` and sets it.
    pub fn main_table_fetched(&self) -> bool {
        self.linearized.is_none() || self.completed.get().is_some()
    }

    /// Whether object `num` is placed without fetching the main
    /// cross-reference table: by the first-page table, by a hint table a page
    /// already read, or because the main table is in hand (or the document
    /// did not open on Annex F's head-only path, where every table is).
    ///
    /// What a head-only read asks before it follows a reference that the
    /// layout may have left in the tail, so that following it costs no tail
    /// read it did not decide to spend.
    pub(crate) fn placed_without_the_tail(&self, num: u32) -> bool {
        self.main_table_fetched()
            || self.xref.get(num).is_some()
            || self.hinted.read_lock().contains_key(&num)
    }

    /// Whether Annex F's hint tables have been read and are in use.
    ///
    /// False until something asks for a page other than the first, and false
    /// afterwards for a file whose tables would not describe it. Distinct from
    /// [`CosDocument::main_table_fetched`], which says what a read *cost*:
    /// this says whether the accelerator is live, which is what changes when a
    /// password arrives for a document whose hint stream was ciphertext.
    pub fn hint_tables_read(&self) -> bool {
        matches!(&*self.page_hints.lock_safe(), HintState::Read(_))
    }

    /// Whether every byte of the document has been fetched.
    ///
    /// Always true for a document opened from a buffer. For a streamed one it
    /// answers whether some whole-file operation -- a repair rescan, a save,
    /// a signature byte range -- has already pulled everything.
    pub fn whole_file_fetched(&self) -> bool {
        self.buffer.whole_fetched()
    }

    /// Which rung of the ladder this document opened on.
    pub fn ladder_level(&self) -> LadderLevel {
        self.ladder
    }

    /// Runs the eager offset probe that a streamed open deferred, and reports
    /// the ladder level it decides.
    ///
    /// [`CosDocument::ladder_level`] on a streamed document is **provisional**:
    /// it reflects the bytes read so far, because the pass that probes every
    /// type-1 entry against its `N G obj` header is the one open-time step
    /// that touches everywhere. This runs exactly that pass over the merged
    /// table and returns the answer a buffered open would have given. Both are
    /// documented observables rather than moods.
    ///
    /// The pass reads each header through a window straight from the source,
    /// one read for each run of offsets that fits in
    /// [`limits::VALIDATION_WINDOW`], and keeps none of them: the chunk cache
    /// holds afterwards what it held before, nothing is fetched whole, and the
    /// pass warns nothing. On Annex F's head-only path the main table is
    /// fetched first, into the cache, as [`CosDocument::xref`] fetches it,
    /// because the buffered open's verdict counts its entries too. The answer
    /// is kept once it is known; a source that could not supply a window
    /// leaves the provisional level, and the next call asks again.
    ///
    /// Reads nothing and returns [`CosDocument::ladder_level`] for a document
    /// opened from a buffer, where the eager pass already ran, and for one a
    /// rescan rebuilt at open, which no failure can move further down.
    ///
    /// The values a caller reads do not depend on whether this was called:
    /// the first read that needs the verdict, one the table cannot place,
    /// works it out itself. What this decides is the *verdict*, which a caller
    /// checking "did it open cleanly" needs and a caller rendering a page does
    /// not.
    pub fn complete_validation(&self) -> LadderLevel {
        self.verdict().unwrap_or(self.ladder)
    }

    /// Whether this document answers as a buffered open of the same bytes
    /// would.
    ///
    /// True for a document opened from a buffer. For a streamed one, true
    /// exactly when none of the three ways the two can part has happened
    /// (`docs/design/streaming-open.md`, *One rule for every path open*):
    ///
    /// - it did not open on Annex F's head-only path, which builds a page out
    ///   of the page's own dictionary where a buffered open walks the tree
    ///   ([`CosDocument::first_page_end`] is `None`);
    /// - it has warned nothing, because what a streamed open warns where a
    ///   buffered one does not is warned at open or by a whole-file fetch, and
    ///   a shifted header, which only a buffered open reconciles entry by
    ///   entry, is warned too;
    /// - its verdict, [`CosDocument::complete_validation`], is
    ///   [`LadderLevel::Trust`], because a buffered open that patched or
    ///   rescanned answers from the table and the warnings its repair made.
    ///
    /// A host that can read the whole file asks this straight after a
    /// streamed open, keeps the document when it is true and reads the file
    /// whole when it is not. Asking costs the verdict's windows and nothing
    /// else, and a source that could not supply them answers false.
    pub fn answers_as_buffered(&self) -> bool {
        if !self.buffer.is_streamed() {
            return true;
        }
        self.linearized.is_none()
            && self.warnings.lock_safe().is_empty()
            && self.verdict() == Ok(LadderLevel::Trust)
    }

    /// The ladder level a buffered open of these bytes decides: the one it
    /// opened on, for a buffered document and a rescanned one, and otherwise
    /// the eager probe's, reached through windows and kept once known.
    ///
    /// The eager pass fails an entry when no offset 7.5.2 allows it, the
    /// shifted one first, holds a header that names it, and this counts the
    /// same failures from the same candidates: each is paired with its entry,
    /// the pairs are probed in file order, and an entry passes at the first
    /// header that names it.
    fn verdict(&self) -> Result<LadderLevel, SourceMiss> {
        if !self.buffer.is_streamed() || self.ladder == LadderLevel::Rescan {
            return Ok(self.ladder);
        }
        if let Some(level) = self.verdict.get() {
            return Ok(*level);
        }
        let table = self.xref();
        let len = self.buffer.len();
        let mut nums = Vec::new();
        let mut probes: Vec<(u64, usize)> = Vec::new();
        for (num, entry) in table.iter() {
            let XrefEntry::Offset { offset, .. } = entry else {
                continue;
            };
            let index = nums.len();
            nums.push(num);
            probes.extend(
                xref::offset_candidates(len, offset, self.shift)
                    .into_iter()
                    .map(|at| (at, index)),
            );
        }
        probes.sort_unstable();
        let mut passed = vec![false; nums.len()];
        self.headers_through_windows(&probes, |index, header| {
            if let (Some(header), Some(&num), Some(pass)) =
                (header, nums.get(index), passed.get_mut(index))
            {
                *pass |= header.num == num;
            }
        })?;
        let failures = passed.iter().filter(|pass| !**pass).count();
        let level = if failures == 0 {
            self.ladder
        } else if failures >= limits::LADDER_RESCAN_MIN_FAILURES && failures * 2 > nums.len() {
            LadderLevel::Rescan
        } else {
            self.ladder.max(LadderLevel::Patch)
        };
        Ok(*self.verdict.get_or_init(|| level))
    }

    /// Hands `seen` the `N G obj` header at each probe's offset, as
    /// [`xref::object_header_at`] finds it in the whole file, with the probe's
    /// index. The probes are sorted by offset, and every offset lies inside
    /// the document.
    ///
    /// Each run of probes that fits in [`limits::VALIDATION_WINDOW`] is one
    /// read, from the first offset to [`limits::HEADER_PROBE_WINDOW`] past the
    /// last, so neighbours share a read and the bytes between distant ones are
    /// never asked for. A header whose lexing runs into the end of what is
    /// held, short of the document's end, doubles the window until the lexing
    /// stops inside it; what a grown window holds serves the runs it reaches,
    /// so no byte is read twice. Every read goes straight to the source and is
    /// dropped by the next run: the chunk cache never sees one.
    fn headers_through_windows(
        &self,
        probes: &[(u64, usize)],
        mut seen: impl FnMut(usize, Option<ObjRef>),
    ) -> Result<(), SourceMiss> {
        let len = self.buffer.len();
        let reach = |at: u64| at.saturating_add(limits::HEADER_PROBE_WINDOW).min(len);
        // The bytes `base..base + held.len()` of the document.
        let mut held: Vec<u8> = Vec::new();
        let mut base = 0u64;
        let mut next = 0usize;
        while let Some(&(start, _)) = probes.get(next) {
            let mut end = reach(start);
            let mut count = 1usize;
            while let Some(&(at, _)) = probes.get(next + count) {
                if reach(at).saturating_sub(start) > limits::VALIDATION_WINDOW {
                    break;
                }
                end = reach(at);
                count += 1;
            }
            let held_end = base.saturating_add(held.len() as u64);
            if (base..=held_end).contains(&start) {
                // A window an earlier probe grew reaches this run. What it
                // holds before the run is done with.
                let done = usize::try_from(start.saturating_sub(base)).unwrap_or(usize::MAX);
                held.drain(..done.min(held.len()));
                if end > held_end {
                    held.extend(self.buffer.read_transient(held_end..end)?);
                }
            } else {
                held = self.buffer.read_transient(start..end)?;
            }
            base = start;
            let mut last: Option<(u64, Option<ObjRef>)> = None;
            for &(at, index) in probes.get(next..next + count).unwrap_or_default() {
                let header = match last {
                    Some((before, header)) if before == at => header,
                    _ => loop {
                        let (header, stop) =
                            xref::object_header_within(&held, at.saturating_sub(base));
                        let held_end = base.saturating_add(held.len() as u64);
                        if stop < held.len() as u64 || held_end >= len {
                            break header;
                        }
                        let more = (held.len() as u64).max(limits::HEADER_PROBE_WINDOW);
                        let grown = self
                            .buffer
                            .read_transient(held_end..held_end.saturating_add(more).min(len))?;
                        if grown.is_empty() {
                            // Inside the document a read is whole or a miss,
                            // so this is the end of the bytes, not a stall.
                            break header;
                        }
                        held.extend(grown);
                    },
                };
                last = Some((at, header));
                seen(index, header);
            }
            next += count;
        }
        Ok(())
    }

    /// Everything this layer had to tolerate, in the order it happened.
    ///
    /// A copy rather than a slice: objects load lazily behind `&self`, so
    /// warnings keep arriving after `open` returns and no borrow of the
    /// document could stay valid across the next read.
    ///
    /// A document [`CosDocument::reopen`] read through its base carries the
    /// base's warnings, as they stand, and then its own: the base's open, and
    /// every read of a number the base parsed, besides what the update and
    /// this document's own parses raised.
    pub fn warnings(&self) -> Vec<Warning> {
        let own = self.warnings.lock_safe().warnings().to_vec();
        match &self.base {
            Some(reopen) => {
                let mut all = reopen.doc.warnings();
                all.extend(own);
                all
            }
            None => own,
        }
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
    pub fn set_decryptor(&self, decryptor: Arc<dyn Decryptor>) {
        self.install_security(decryptor, None);
    }

    /// [`CosDocument::set_decryptor`], also keeping the key it was built from.
    pub fn set_decryptor_with_key(&self, decryptor: Arc<dyn Decryptor>, key: FileKey) {
        self.install_security(decryptor, Some(key));
    }

    /// The authenticated file key, if this document has one.
    ///
    /// Cloned out rather than borrowed, because the lock is not the caller's
    /// to hold across a whole save.
    #[must_use]
    pub fn file_key(&self) -> Option<FileKey> {
        self.security.read_lock().key.clone()
    }

    /// Takes `from`'s security state — decryptor, file key and how far its
    /// password got — for a document that is `from` with an update appended.
    ///
    /// `pub(crate)` and nothing wider: this copies a handler from one document
    /// to another, and a handler is only right for the file whose `/Encrypt`
    /// and `/ID` it was derived from. The one caller
    /// is [`crate::edit::DocumentEditor::view`], whose bytes are `from`'s own
    /// followed by an update sealed with this same key (7.6.2), so the
    /// `/Encrypt` dictionary and the first `/ID` string are the ones the
    /// handler was built over. A `from` that was never authenticated passes
    /// nothing on, and the view is exactly as locked as the document.
    pub(crate) fn inherit_security(&self, from: &CosDocument) {
        let (decryptor, installed, key, level) = {
            let security = from.security.read_lock();
            (
                Arc::clone(&security.decryptor),
                security.has_decryptor,
                security.key.clone(),
                security.auth_level,
            )
        };
        if !installed {
            return;
        }
        self.install_security(decryptor, key);
        self.security.write_lock().auth_level = level;
    }

    fn install_security(&self, decryptor: Arc<dyn Decryptor>, key: Option<FileKey>) {
        {
            let mut security = self.security.write_lock();
            security.decryptor = decryptor;
            security.has_decryptor = true;
            security.key = key;
        }
        // Counted, so that a document reopened over this one, or this one
        // over its base, stops reading through the other once what was read
        // there was read under another security.
        self.security_epoch.fetch_add(1, Ordering::AcqRel);
        // Everything already loaded was read as plaintext out of ciphertext.
        // Both caches are dropped so the next read goes back to the buffer;
        // outstanding `Arc`s keep the values they were given.
        self.store.clear();
        self.objstm.clear();
        // Annex F's hint tables with them, and for the same reason: the hint
        // stream is encrypted like any other, so a refusal recorded before a
        // password arrived was a refusal to read ciphertext. Nothing in
        // `hinted` is dropped -- an `N G obj` header is plaintext wherever it
        // sits, and the offsets recorded from one are what they were.
        *self.page_hints.lock_safe() = HintState::Untried;
        // The page index with them: an index read over ciphertext holds
        // inherited `/Resources` whose strings were never decrypted.
        *self.page_index.lock_safe() = crate::pages::IndexSlot::default();
    }

    /// The page index's slot (`crate::pages`).
    pub(crate) fn page_index(&self) -> &Mutex<crate::pages::IndexSlot> {
        &self.page_index
    }

    /// How many loads have missed so far, on every thread.
    pub(crate) fn misses(&self) -> u64 {
        self.misses.load(Ordering::Relaxed)
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
        // One head window, which on a streamed document is one chunk and on
        // a buffer is a copy of its first page. Both the keyword and the
        // digits after it lie inside it by 7.5.2's own scan limit.
        let window = self.buffer.window(0..limits::MAX_HEADER_SCAN as u64).ok()?;
        let at = window
            .windows(5)
            .position(|w| w == b"%PDF-")
            .map(|p| p + 5)?;
        let digits: Vec<u8> = window
            .get(at..(at + 8).min(window.len()))?
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
    ///
    /// A streamed document fetches them whole the first time it is asked, and
    /// says so then, once, as every whole-file fetch does ([`fetch_whole`],
    /// ruling 10): asked again, it fetches nothing and says nothing.
    pub fn bytes(&self) -> &[u8] {
        if self.buffer.is_streamed() && !self.buffer.whole_fetched() {
            let mut sink = WarningSink::new();
            if fetch_whole(&self.buffer, &mut sink).is_some() {
                self.absorb(sink);
            }
        }
        self.buffer.whole()
    }

    /// What an update written after this document needs of it
    /// (`crate::write::update_section`): its length, and whether its last
    /// byte is a newline.
    ///
    /// Read from the last byte alone, so a streamed document asks its source
    /// for one byte where [`CosDocument::bytes`] would fetch every one; only a
    /// source that cannot supply that byte is fetched whole, and then the
    /// prefix is the one those bytes make — the bytes a whole save writes
    /// after, so the two cannot part.
    #[must_use]
    pub fn prefix(&self) -> crate::write::Prefix {
        let len = self.buffer.len();
        let Some(last) = len.checked_sub(1) else {
            return crate::write::Prefix {
                len,
                ends_in_newline: false,
            };
        };
        match self.buffer.window(last..len) {
            Ok(byte) => crate::write::Prefix {
                len,
                ends_in_newline: byte.first() == Some(&b'\n'),
            },
            Err(_) => crate::write::Prefix::of(self.bytes()),
        }
    }

    /// The highest object number the cross-reference table knows.
    ///
    /// New objects start above it, so an edit cannot collide with something
    /// the document already uses.
    pub fn max_object_number(&self) -> u32 {
        let own = self.xref.max_number();
        match &self.base {
            Some(reopen) => own.max(reopen.doc.max_object_number()),
            None => own,
        }
    }

    /// The offset of the most recent cross-reference section, which an
    /// incremental update chains to through `/Prev`. Zero when there is none
    /// to chain to — a document the repair scanner rebuilt.
    ///
    /// This returned `byte_range.end` — the byte just past the last `%%EOF` —
    /// which is not a cross-reference section and is never at the same offset
    /// as one. Every incremental update this engine wrote therefore published
    /// a `/Prev` that no reader could follow, so **every object the update did
    /// not itself carry became unreachable**: a filled form reopened with its
    /// widgets missing, a rotated page with no content. Nothing caught it
    /// because the reader recovers — it falls to the repair scanner and finds
    /// the objects by scanning — and only when the damage is bad enough to
    /// trigger the fall. A file whose catalog and pages happened to be in the
    /// update stayed at [`LadderLevel::Trust`] and quietly read the rest as
    /// null.
    pub fn last_startxref(&self) -> u64 {
        self.revisions
            .first()
            .map_or(0, |revision| revision.xref_at)
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

    /// Records a warning against the object it happened to (ruling 10).
    ///
    /// The offset comes from the table rather than from the caller, so a
    /// warning about an object always points at where this document believes
    /// that object to be -- including when the belief is what is wrong.
    pub fn warn_object(&self, r: ObjRef, kind: WarningKind) {
        let offset = self.entry_offset(r.num);
        self.warnings.lock_safe().warn_at(offset, Some(r), kind);
    }

    /// Records a warning about a byte position no object could be named for.
    pub(crate) fn warn_offset(&self, offset: u64, kind: WarningKind) {
        self.warnings.lock_safe().warn(offset, kind);
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
        self.security.read_lock().auth_level
    }

    /// The permission flags, respecting the authentication level: an
    /// unencrypted document and one opened with the owner password both
    /// permit everything.
    pub fn permissions(&self) -> Permissions {
        crate::security::permissions(self.encrypt.as_ref(), self.auth_level())
    }

    /// Tries `password` and, if it matches, installs the decryptor.
    ///
    /// The owner password is tried first, so a document whose two passwords
    /// are equal authenticates as the owner. Whatever the handler had to
    /// tolerate reaching that answer becomes document warnings.
    pub fn authenticate(&self, password: &str) -> Result<AuthLevel, AuthError> {
        let params = self.encrypt.clone().ok_or(AuthError::NotEncrypted)?;
        let auth = crate::security::authenticate(&params, password)?;

        {
            let mut sink = self.warnings.lock_safe();
            for note in auth.notes {
                sink.warn(0, WarningKind::SecurityHandler(note));
            }
        }

        self.set_decryptor_with_key(auth.decryptor, auth.key);
        self.security.write_lock().auth_level = auth.level;
        Ok(auth.level)
    }

    /// Opens a `/Adobe.PubSec` document with the caller's key (7.6.5).
    ///
    /// The public-key sibling of [`CosDocument::authenticate`]: same
    /// installation, different route to the file key. It does not check
    /// `/Filter` first, because a document whose `/Filter` says `Standard`
    /// simply has no `/Recipients` and is refused for that.
    ///
    /// # Errors
    /// [`crate::pubsec::PubSecError`].
    pub fn authenticate_with_recipient(
        &self,
        recipient: &dyn crate::pubsec::Recipient,
    ) -> Result<AuthLevel, crate::pubsec::PubSecError> {
        let params = self
            .encrypt
            .clone()
            .ok_or(crate::pubsec::PubSecError::NoRecipients)?;
        let auth = crate::pubsec::authenticate(&params, recipient)?;
        self.set_decryptor_with_key(auth.decryptor, auth.key);
        self.security.write_lock().auth_level = auth.level;
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
        self.get_with(r, None)
    }

    /// [`CosDocument::get`], counting what the read parsed into `work`: the
    /// tokens lexed and the objects parsed, an object stream's prologue and
    /// the member read out of it, and a repair scan the read had to build. A
    /// read the store answers parses nothing and counts nothing
    /// (`docs/design/resource-exhaustion.md`, "The counters").
    ///
    /// # Errors
    /// As [`CosDocument::get`].
    pub fn get_counted(&self, r: ObjRef, work: &Work) -> Result<Arc<Object>, CosError> {
        self.get_with(r, Some(work))
    }

    fn get_with(&self, r: ObjRef, work: Option<&Work>) -> Result<Arc<Object>, CosError> {
        let mut sink = WarningSink::new();
        let mut ctx = ResolveCtx::new();
        let object = self.load(r.num, &mut ctx, &mut sink);
        if let Some(work) = work {
            sink.steps().tally(work);
        }
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
        self.security.read_lock().has_decryptor
    }

    /// The installed decryptor, cloned so no lock is held while it runs.
    pub(crate) fn decryptor(&self) -> Arc<dyn Decryptor> {
        Arc::clone(&self.security.read_lock().decryptor)
    }

    /// The cross-reference entry for `num`.
    ///
    /// On Annex F's fast path the first-page table is consulted first and the
    /// main table at `/T` is fetched only when a read leaves page one -- which
    /// is what makes "not one read touches the tail" true of a page-one render
    /// and false of anything more.
    fn entry(&self, num: u32) -> Option<XrefEntry> {
        if let Some(entry) = self.xref.get(num) {
            return Some(entry);
        }
        // A reopened document's own table is its update's; every other
        // number's entry is its base's, where it lies.
        if let Some(reopen) = &self.base {
            return reopen.doc.entry(num);
        }
        self.linearized.as_ref()?;
        // Annex F's hint tables, when a page other than the first has already
        // placed this object. Between the two tables rather than instead of
        // either: the first-page table is the file's own cross-reference
        // section and outranks a hint, and the main table is still what
        // answers for everything the hints could not place.
        if let Some(entry) = self.hinted.read_lock().get(&num).copied() {
            return Some(entry);
        }
        self.merged_xref().get(num)
    }

    /// Annex F's hint tables, unpacked at most once.
    ///
    /// Not at open: page one is reached through `/O` and the first-page
    /// section, so a document nobody pages past costs nothing for these. The
    /// first request for another page pays for the hint stream and every
    /// request after it is free, which is F.4's own advice -- "read and decode
    /// the tables once and retain the information for as long as the document
    /// remains open".
    fn page_hints(&self) -> Option<Arc<PageHints>> {
        match &*self.page_hints.lock_safe() {
            HintState::Refused => return None,
            HintState::Read(hints) => return Some(Arc::clone(hints)),
            HintState::Untried => {}
        }
        // Decoded with no lock held: reading the hint stream loads an object,
        // which may load more, and a lock held across that is a lock held
        // across the whole store. Two threads arriving together both decode
        // the same bytes, so whichever publishes first wins and the other is
        // dropped -- the discipline the object store and the repair index
        // already resolve their races with.
        let decoded = self.decode_page_hints().map(Arc::new);
        let mut state = self.page_hints.lock_safe();
        match (&*state, decoded) {
            // Authentication cleared the slot while this was decoding, which
            // means these tables came out of bytes that are no longer what the
            // document says they are.
            (HintState::Read(hints), _) => Some(Arc::clone(hints)),
            (_, Some(hints)) => {
                *state = HintState::Read(Arc::clone(&hints));
                Some(hints)
            }
            (_, None) => {
                *state = HintState::Refused;
                None
            }
        }
    }

    /// Reads the primary hint stream and turns its two tables into byte
    /// ranges, or refuses the whole of it.
    ///
    /// Refusing is a leniency and says so (ruling 10): every `None` below
    /// leaves the page tree walk to find the page, which fetches the main
    /// table and costs the tail. That is slower and never wrong, which is the
    /// trade this whole path is allowed to make.
    fn decode_page_hints(&self) -> Option<PageHints> {
        let params = self.linearized.as_ref()?;
        let (Some(page_count), Some((hint_at, hint_len))) = (params.page_count, params.hint) else {
            self.warn_offset(0, WarningKind::LinearizedHintsUnusable);
            return None;
        };
        let len = self.buffer.len();

        // F.3.6: the hint stream is an indirect object that nothing in the
        // document references, so `/H` is the only route to it -- and the
        // first-page cross-reference table need not carry an entry for it.
        let mut ctx = ResolveCtx::new();
        let found = xref::offset_candidates(len, hint_at, params.shift)
            .into_iter()
            .find_map(|at| {
                self.parse_windowed(at, &mut ctx, &mut WarningSink::new())
                    .map(|(p, _)| (at, p))
            });
        let Some((at, parsed)) = found else {
            self.warn_offset(hint_at, WarningKind::LinearizedHintsUnusable);
            return None;
        };
        let reference = parsed.reference;
        let shared_at = parsed
            .object
            .as_stream()
            .and_then(|stream| stream.dict.get_int(self.names.table.intern(b"S")))
            .and_then(|v| usize::try_from(v).ok());
        let Some(shared_at) = shared_at else {
            self.warn_object(reference, WarningKind::LinearizedHintsUnusable);
            return None;
        };
        // The stream is read through the ordinary object path -- one decoder
        // for filters, one for decryption -- so where it is has to be recorded
        // before it can be read. Recorded and not imposed: `entry` consults
        // the first-page section first, so a file whose own section puts this
        // object number somewhere else is read *there*, and what is there is
        // not a hint stream. That refusal arrives below, by the ordinary
        // route, which is why there is no second check for it here.
        self.hinted
            .write_lock()
            .entry(reference.num)
            .or_insert(XrefEntry::Offset {
                offset: at,
                gen: reference.gen,
            });
        let Ok(data) = self.stream_decoded(reference) else {
            self.warn_object(reference, WarningKind::LinearizedHintsUnusable);
            return None;
        };
        let Some(tables) = hints::decode(&data, shared_at, page_count as usize) else {
            self.warn_object(reference, WarningKind::LinearizedHintsUnusable);
            return None;
        };

        // F.4: "the resulting positions shall be interpreted as if the primary
        // hint stream itself were not present. That is, a position greater
        // than the hint stream offset shall have the hint stream length added
        // to it". The clause says *greater*; a position exactly equal to the
        // offset is one the stream itself would occupy, so nothing real lands
        // there and equality can only mean the byte after the stream -- which
        // is where the first page begins in every linearized file in the
        // fetched qpdf corpus, all 32 of which come out at the offset their
        // own cross-reference table states only when equality is included.
        let real = |absent: u64| -> u64 {
            let placed = if absent >= hint_at {
                absent.saturating_add(hint_len)
            } else {
                absent
            };
            placed.saturating_add(params.shift).min(len)
        };

        // Table F.4 item 2 gives two routes to the first page's location: the
        // hint table's own header item, and the cross-reference entry for the
        // object `/O` names. Both are in hand here, which makes this the one
        // place the tables can be held to something the file states elsewhere
        // -- so they are, and tables that disagree with the table page one was
        // opened from buy nothing.
        let stated = real(u64::from(tables.first_page_offset));
        let agrees = match self.xref.get(params.first_page_object) {
            Some(XrefEntry::Offset { offset, .. }) => {
                xref::offset_candidates(len, offset, params.shift).contains(&stated)
            }
            _ => false,
        };
        if !agrees {
            self.warn_object(reference, WarningKind::LinearizedHintsUnusable);
            return None;
        }

        // The runs, and the one plausibility test the tables are held to.
        //
        // **A page's declared length decides which page a reader gets**, which
        // makes it the one hint here that can change an answer rather than a
        // cost. A table stating page one's length as zero puts page two's run
        // at page one's page object -- a page leaf carrying its own
        // `/MediaBox` and `/Resources`, so every other check passes it -- and
        // `page(1)` comes back holding page one's content under index 1.
        // Silently, cheaply, and wrong.
        //
        // A page occupies bytes, so each run begins strictly after the one
        // before it, and a table that says otherwise is not describing a
        // linearized layout -- none of it is used.
        //
        // Strictly increasing, and *not* "at or past `/E`", which was the
        // first form of this and refused eight corpus files that are perfectly
        // well linearized. Table F.3 item 4 measures a page's length from its
        // page object to the last byte of the last object it *uses*, while
        // `/E` ends part 6, and F.3.5 puts things in part 6 that the first
        // page does not use -- an outline hierarchy, when the catalogue asks
        // for one. The two numbers are not the same number, and a check built
        // on their being equal measured this reader's assumption rather than
        // the file.
        let mut runs: Vec<Range<u64>> = Vec::with_capacity(tables.pages.len());
        let mut absent = u64::from(tables.first_page_offset);
        for page in &tables.pages {
            // Accumulated in the hint tables' own coordinates and converted
            // per page, not converted once and then accumulated: F.3.6 lets
            // part 5 follow part 6 as well as precede it, and a reader that
            // added the lengths in file coordinates would walk straight
            // through the hint stream instead of over it.
            let start = real(absent);
            let end = start.saturating_add(u64::from(page.length)).min(len);
            let plausible = match runs.last() {
                Some(previous) => start > previous.start,
                None => true,
            };
            if !plausible {
                self.warn_object(reference, WarningKind::LinearizedHintsUnusable);
                return None;
            }
            runs.push(start..end);
            absent = absent.saturating_add(u64::from(page.length));
        }

        // Table F.6 item 1: the groups for the first page accumulate from the
        // first page's own location, and the rest from Table F.5 item 2. Only
        // the rest are kept -- a first-page group is a first-page object, and
        // the first-page cross-reference table already has it.
        let first_page_groups = tables.shared_first_page as usize;
        let mut groups = Vec::new();
        let mut absent = u64::from(tables.first_shared_offset);
        for length in tables.shared_lengths.iter().skip(first_page_groups) {
            let start = real(absent);
            let end = start.saturating_add(u64::from(*length)).min(len);
            groups.push(start..end);
            absent = absent.saturating_add(u64::from(*length));
        }

        Some(PageHints {
            runs,
            groups,
            first_page_groups,
            named: tables.pages.into_iter().map(|page| page.shared).collect(),
        })
    }

    /// The page object of page `index`, out of Annex F's hint tables, with
    /// everything the page needs from the same tables recorded on the way.
    ///
    /// `None` when there are no usable tables, when the file claims no such
    /// page, or when the range the tables name holds no object header at all.
    /// Each of those leaves the page tree walk to answer, which fetches the
    /// main cross-reference table.
    pub(crate) fn hinted_page_object(&self, index: u32) -> Option<ObjRef> {
        let hints = self.page_hints()?;
        let run = hints.runs.get(index as usize)?.clone();
        // The part 8 groups this page names (Table F.4 item 4) first, so a
        // font or a colour space several pages share is placed before the page
        // that needs it is parsed. An index below `first_page_groups` names a
        // first-page object and needs nothing: the table page one opened from
        // has it.
        if let Some(named) = hints.named.get(index as usize) {
            for entry in named {
                let Some(group) = (*entry as usize)
                    .checked_sub(hints.first_page_groups)
                    .and_then(|k| hints.groups.get(k))
                else {
                    continue;
                };
                self.record_hinted(group.clone());
            }
        }
        self.record_hinted(run)
    }

    /// Reads the `N G obj` headers inside one hint-named byte range, records
    /// where each object was found, and answers with the first.
    ///
    /// The range is the only thing taken from the tables. What is *in* it is
    /// read from the file's own object headers -- the same statement the
    /// repair scanner believes over a whole document at ladder level 3 -- and
    /// every offset recorded here is checked again by [`CosDocument::parse_at`]
    /// when the object loads. A table that names the wrong bytes therefore
    /// costs a window nobody uses, never a wrong object.
    ///
    /// Nothing recorded here ever displaces the first-page table: [`entry`]
    /// consults that first, and a number already present keeps the offset it
    /// already had.
    ///
    /// [`entry`]: CosDocument::entry
    fn record_hinted(&self, range: Range<u64>) -> Option<ObjRef> {
        if range.start >= range.end {
            return None;
        }
        let view = self.buffer.view();
        let want = range.end - range.start;
        let window = view.window(range.start, want)?;
        let from = window.local(range.start)?;
        let until = from.saturating_add(want).min(window.bytes().len() as u64);

        let mut found: Vec<(ObjRef, u64)> = Vec::new();
        let mut at = from;
        while let Some(header_at) = next_object_header(window.bytes(), at) {
            if header_at >= until {
                break;
            }
            let Some(reference) = xref::object_header_at(window.bytes(), header_at) else {
                break;
            };
            if found.len() >= limits::MAX_HINTED_PAGE_OBJECTS {
                // Ruling 1: the range is the file's choice, and so is how many
                // headers it packs into it. A run this long is not a page, so
                // the whole range is refused rather than half-recorded.
                self.warn_object(reference, WarningKind::LinearizedPageHintRejected);
                return None;
            }
            found.push((reference, window.abs(header_at)));
            at = header_at.saturating_add(1);
        }

        let mut hinted = self.hinted.write_lock();
        for (reference, offset) in &found {
            hinted.entry(reference.num).or_insert(XrefEntry::Offset {
                offset: *offset,
                gen: reference.gen,
            });
        }
        found.first().map(|(reference, _)| *reference)
    }

    /// The first-page table merged with the main one, fetched once.
    ///
    /// `/T` names the byte before the main table's first *entry* (Table F.1)
    /// rather than the `xref` keyword, so the keyword is looked for just
    /// behind it; a cross-reference stream has its object header there
    /// instead, and both are offered to the same walker. A file whose `/T`
    /// leads nowhere falls back to `startxref`, which by then costs nothing
    /// it has not already decided to spend.
    fn merged_xref(&self) -> &XrefTable {
        if let Some(table) = self.completed.get() {
            return table;
        }
        let mut sink = WarningSink::new();
        let mut merged = self.xref.clone();
        if let Some(params) = &self.linearized {
            let view = self.buffer.view();
            let back = params
                .main_table_at
                .saturating_sub(limits::XREF_SECTION_WINDOW);
            let keyword = view
                .window(back, params.main_table_at.saturating_sub(back) + 16)
                .and_then(|w| rfind_from(w.bytes(), b"xref", 0).map(|at| w.abs(at as u64)));
            let mut starts: Vec<u64> = keyword
                .into_iter()
                .chain(std::iter::once(params.main_table_at))
                .collect();
            let mut found = false;
            for start in starts.drain(..) {
                let built = xref::build(
                    self.buffer.view(),
                    start,
                    params.shift,
                    &self.names,
                    &mut sink,
                );
                if built.sections > 0 {
                    for (num, entry) in built.table.iter() {
                        merged.insert_new(num, entry);
                    }
                    found = true;
                    break;
                }
            }
            // `startxref` is the last resort and is asked for only when the
            // two candidates derived from `/T` came to nothing -- computing it
            // eagerly would read the tail even on the path that did not need
            // it, which is a byte budget telling a lie about itself.
            if !found {
                if let Some(start) = xref::startxref(&self.buffer.view(), &mut sink) {
                    let built = xref::build(
                        self.buffer.view(),
                        start,
                        params.shift,
                        &self.names,
                        &mut sink,
                    );
                    for (num, entry) in built.table.iter() {
                        merged.insert_new(num, entry);
                    }
                }
            }
        }
        self.absorb(sink);
        let _ = self.completed.set(merged);
        self.completed.get().unwrap_or(&self.xref)
    }

    /// The reference for an object number, taking the generation from the
    /// table, which validation has already reconciled with the file's own
    /// `N G obj` header.
    fn ref_of(&self, num: u32) -> ObjRef {
        let gen = match self.entry(num) {
            Some(XrefEntry::Offset { gen, .. }) | Some(XrefEntry::Free { gen, .. }) => gen,
            // 7.5.7: objects in an object stream always have generation 0.
            Some(XrefEntry::InStream { .. }) | None => 0,
        };
        ObjRef::new(num, gen)
    }

    fn entry_offset(&self, num: u32) -> u64 {
        match self.entry(num) {
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
    ///
    /// Under a budget the object is charged its estimate
    /// ([`CosDocument::charge_objects_to`]): the extent of its tokens in the
    /// bytes it was parsed from — for a stream, its dictionary up to its
    /// data, which a parsed stream does not hold — and
    /// [`crate::accountant::ENTRY_BYTES`] beside it.
    fn load(&self, num: u32, ctx: &mut ResolveCtx, sink: &mut WarningSink) -> Arc<Object> {
        if let Some(object) = self.store.get(num) {
            return object;
        }
        if let Some(object) = self.through_base(num, sink) {
            // The base's store holds it; a store under a budget leaves it
            // there rather than charging it a second time.
            if self.store.is_budgeted() {
                return object;
            }
            return self.store.publish(num, object, 0);
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
        let missed_before = ctx.missed();
        let (object, extent) = ctx.enter(num, |ctx| self.load_uncached(num, ctx, sink));
        if ctx.missed() && !missed_before {
            // A miss never publishes. This load reads as null because the
            // bytes were absent, not because the object is; publishing it
            // would give every later read the miss's answer forever, and the
            // host that goes and fetches the range would never see it change.
            self.misses.fetch_add(1, Ordering::Relaxed);
            return Arc::new(object);
        }
        self.store.publish(
            num,
            Arc::new(object),
            extent.saturating_add(crate::accountant::ENTRY_BYTES),
        )
    }

    /// Charges this document's parsed objects to `accountant` from now on
    /// (`docs/design/performance.md`, P-16, "Parsed objects under pressure"),
    /// and says whether it took.
    ///
    /// Each object a read parses is charged an estimate of its bytes — the
    /// extent of its tokens in the bytes it was parsed from, for a stream its
    /// dictionary up to its data, and
    /// [`ENTRY_BYTES`](crate::accountant::ENTRY_BYTES) beside it — and every
    /// read that reaches a kept object takes a number from the accountant's
    /// counter, as an entry of whatever cache shares the accountant does. Past
    /// the budget the objects least recently requested that no caller holds
    /// are dropped, and a later read parses the object again, from the same
    /// immutable bytes, to an equal value; a streamed document fetches its
    /// bytes again where its source no longer holds them. The cross-reference
    /// table, the decoded object streams and their indexes, the repair
    /// scanner's index, the page index and the hint tables are never charged
    /// and never dropped.
    ///
    /// A document's objects are charged to the first accountant handed it and
    /// to that one only: a later call changes nothing and answers `false`.
    /// The objects read before it took, the `/Encrypt` dictionary an open
    /// reads among them, are forgotten, so that none is held outside the
    /// budget. With no accountant handed it, nothing is ever dropped.
    pub fn charge_objects_to<E: crate::accountant::Entries>(
        &self,
        accountant: Arc<crate::accountant::Accountant<E>>,
    ) -> bool {
        self.store.charge_to(accountant)
    }

    /// Whether this document's parsed objects are charged to a budget
    /// ([`CosDocument::charge_objects_to`]).
    #[must_use]
    pub fn objects_charged(&self) -> bool {
        self.store.is_budgeted()
    }

    /// Object `num` as the base this document was reopened over has it, when
    /// the update leaves `num` alone, both documents keep the security the
    /// reopen found, and the base's parse of it read nothing the update
    /// redefines (P-17.4).
    ///
    /// The base parses it once, into its own store, counting its steps into
    /// `sink` and its warnings into its own list, which this document's
    /// warnings carry. Anything else — and every number once a document's
    /// security has moved — is this document's own to load.
    fn through_base(&self, num: u32, sink: &mut WarningSink) -> Option<Arc<Object>> {
        let reopen = self.base.as_ref()?;
        if self.xref.get(num).is_some() || !reopen.current(self) {
            return None;
        }
        let object = reopen.doc.load_alone(num, sink);
        let depends = self.reads_the_update(&reopen.doc, num, &object, 0);
        (!depends).then_some(object)
    }

    /// Object `num`, loaded as a read of it alone would load it: a resolve of
    /// its own, its steps added to `sink` and its warnings to this document's
    /// list.
    fn load_alone(&self, num: u32, sink: &mut WarningSink) -> Arc<Object> {
        let mut own = WarningSink::new();
        let mut ctx = ResolveCtx::new();
        let object = self.load(num, &mut ctx, &mut own);
        sink.add_steps(own.steps());
        self.absorb(own);
        object
    }

    /// Whether `base`'s parse of `num`, which gave `object`, read a number
    /// this document's update defines: a stream's indirect `/Length`, along
    /// its chain of references; for a member of an object stream, the
    /// container, what the container's own parse read, and the container's
    /// indirect `/N` and `/First`. Anything the base no longer holds to look
    /// at is taken as read, and the object is loaded afresh.
    fn reads_the_update(
        &self,
        base: &CosDocument,
        num: u32,
        object: &Object,
        depth: usize,
    ) -> bool {
        if depth > limits::MAX_LOAD_DEPTH {
            return true;
        }
        match base.entry(num) {
            Some(XrefEntry::InStream { stream_num, .. }) => {
                if self.xref.get(stream_num).is_some() {
                    return true;
                }
                let Some(container) = base.store.get(stream_num) else {
                    return true;
                };
                let Object::Stream(stream) = container.as_ref() else {
                    return true;
                };
                self.reads_the_update(base, stream_num, &container, depth + 1)
                    || [Name::N, Name::FIRST]
                        .iter()
                        .any(|key| self.chain_reads_the_update(base, stream.dict.get(*key)))
            }
            _ => match object {
                Object::Stream(stream) => {
                    self.chain_reads_the_update(base, stream.dict.get(Name::LENGTH))
                }
                _ => false,
            },
        }
    }

    /// Whether resolving `value` in `base` reads a number this document's
    /// update defines: each hop of its chain of references, as `base` holds
    /// it, until something that is not one.
    fn chain_reads_the_update(&self, base: &CosDocument, value: Option<&Object>) -> bool {
        let Some(mut at) = value.and_then(Object::as_objref) else {
            return false;
        };
        for _ in 0..limits::MAX_RESOLVE_DEPTH {
            if self.xref.get(at.num).is_some() {
                return true;
            }
            let Some(object) = base.store.get(at.num) else {
                return true;
            };
            match object.as_objref() {
                Some(next) => at = next,
                None => return false,
            }
        }
        true
    }

    /// Object `num` read from the bytes, and the extent of the tokens it was
    /// read from (`CosDocument::load`'s estimate); a null no bytes hold is of
    /// no extent.
    fn load_uncached(
        &self,
        num: u32,
        ctx: &mut ResolveCtx,
        sink: &mut WarningSink,
    ) -> (Object, u64) {
        let entry = self.entry(num);
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
        let scan = if self.scanner_may_answer(entry, ctx) {
            self.repair_index(sink)
        } else {
            None
        };
        if let Some(hit) = scan.and_then(|scan| scan.get(num)) {
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
        (Object::Null, 0)
    }

    /// How far a read that begins inside the first page may reach, on Annex
    /// F's head-only path.
    ///
    /// An object that ends at `/E` needs no byte past it -- but a window sized
    /// by a guess asks for more, and on a fixed granularity that guess pulls
    /// the first chunk of the tail for the sake of a few bytes it will not
    /// use. So the first attempt is clamped to `/E`, and only a parse that
    /// genuinely comes up short is allowed past it. Nothing is refused: the
    /// ceiling changes which bytes are fetched first, never which are
    /// readable.
    pub(crate) fn head_ceiling(&self, offset: u64) -> Option<u64> {
        let end = self.linearized.as_ref()?.end_of_first_page;
        (offset < end).then_some(end)
    }

    /// Whether the repair scanner may answer for an object the table could
    /// not place, as it would in a buffered open of the same bytes.
    ///
    /// A buffered open has a scanner exactly when its verdict is not `Trust`,
    /// and its `repair_index` answers from what the open built. So a streamed
    /// document asks only when its verdict is not `Trust` either, working the
    /// verdict out through windows if nothing has yet: a free entry whose old
    /// body is still in the file, or a number with no entry, then reads as the
    /// null 7.3.10 makes it, as it does buffered, rather than fetching the
    /// whole file to find a body no table names.
    ///
    /// One shape keeps the scanner whatever the verdict: a type-1 entry of a
    /// file whose header is not at byte 0. The walker keeps each entry as the
    /// file states it, from the header (7.5.2); a buffered open's validation
    /// moves it to where its object is, and a streamed one reads it where it
    /// is stated, misses its header, and finds the object through the scanner
    /// instead, as it always has. A shifted file decides `Trust`, so gating
    /// this on the verdict would read every object it has as null.
    ///
    /// A verdict the source could not supply the bytes for is a miss for this
    /// load, which is then not published, and the next read asks again.
    fn scanner_may_answer(&self, entry: Option<XrefEntry>, ctx: &mut ResolveCtx) -> bool {
        if !self.buffer.is_streamed() {
            return true;
        }
        if self.shift > 0 && matches!(entry, Some(XrefEntry::Offset { .. })) {
            return true;
        }
        match self.verdict() {
            Ok(level) => level != LadderLevel::Trust,
            Err(_) => {
                ctx.note_miss();
                false
            }
        }
    }

    /// The repair scanner's index, built on first need for a streamed
    /// document.
    ///
    /// A document opened from a buffer decided at open whether it needed one,
    /// because the eager offset probe ran then. A streamed document deferred
    /// that probe, so the same decision is made at the first read the table
    /// cannot place, by [`CosDocument::scanner_may_answer`] from the verdict
    /// the probe reaches -- which is exactly the condition the eager pass was
    /// looking for. The values a caller reads are therefore the same on both
    /// paths; what differs is when the whole source is fetched, and that is
    /// warned about rather than hidden (ruling 10).
    ///
    /// Never built for a buffered document: one that reached here with no
    /// index is one the eager pass found nothing wrong with, and inventing a
    /// scan for it would repair objects the buffered path reads as null.
    fn repair_index(&self, sink: &mut WarningSink) -> Option<Arc<ScanIndex>> {
        if let Some(scan) = self.scan.read_lock().clone() {
            return Some(scan);
        }
        if !self.buffer.is_streamed() {
            return None;
        }
        let buffer = fetch_whole(&self.buffer, sink)?;
        let index = ScanIndex::build(&buffer, &self.names);
        sink.add_steps(index.steps());
        let index = Arc::new(index);
        let mut slot = self.scan.write_lock();
        // Another thread may have built the same index meanwhile; it read the
        // same bytes, so whichever is there wins and this one is dropped.
        Some(Arc::clone(slot.get_or_insert(index)))
    }

    /// Parses the indirect object at `offset` out of a window, growing it
    /// until the object demonstrably ends inside it.
    ///
    /// A window that cut an object short would parse to a *different value*
    /// than the same bytes in one buffer, and ruling 4 does not allow the two
    /// to differ -- so the test is where the parse stopped, not whether it
    /// returned something. A stream is judged on where its data begins rather
    /// than on where its declared length ends: the dictionary and the `stream`
    /// keyword are all this parse has to contain, and the data extent is the
    /// document layer's own question (7.3.8.2).
    ///
    /// A document opened from a buffer gets the whole buffer as its window and
    /// takes one pass, exactly as it always did.
    fn parse_windowed(
        &self,
        offset: u64,
        ctx: &mut ResolveCtx,
        sink: &mut WarningSink,
    ) -> Option<(ParsedIndirect, Vec<Warning>)> {
        let view = self.buffer.view();
        let mut want = limits::OBJECT_WINDOW;
        let mut ceiling = self.head_ceiling(offset);
        loop {
            let asked = match ceiling {
                Some(end) => want.min(end.saturating_sub(offset)).max(1),
                None => want,
            };
            let Some(window) = view.window(offset, asked) else {
                // The bytes are not here yet. Recorded rather than swallowed,
                // so nothing publishes a null the host could still fix.
                ctx.note_miss();
                return None;
            };
            let reaches_end = window.end() >= self.buffer.len();
            let local_at = window.local(offset)?;
            let mut local = WarningSink::new();
            let parsed = parse_indirect_at(window.bytes(), local_at, &self.names.table, &mut local);
            // A window that turns out too short is parsed again, larger: both
            // parses are the load's steps, and only the last one's warnings
            // are the object's.
            sink.add_steps(local.steps());
            let mut parsed = parsed?;
            let consumed = match &parsed.object {
                Object::Stream(stream) => stream.data_start,
                _ => parsed.end_offset,
            };
            if !reaches_end && consumed >= window.bytes().len() as u64 {
                // The ceiling was a guess about where the head ends; a parse
                // that ran into it is the object saying otherwise, and the
                // object wins.
                ceiling = None;
                want = want.saturating_mul(2);
                continue;
            }
            // Window offsets become document offsets. A stream records where
            // its data starts and that number is read against the document
            // afterwards, so leaving it window-relative would point every
            // later read at the wrong bytes.
            if let Object::Stream(stream) = &mut parsed.object {
                stream.data_start = window.abs(stream.data_start);
            }
            parsed.end_offset = window.abs(parsed.end_offset);
            let warnings = local
                .take()
                .into_iter()
                .map(|mut warning| {
                    warning.offset = window.abs(warning.offset);
                    warning
                })
                .collect();
            return Some((parsed, warnings));
        }
    }

    /// Parses the object whose header sits at `offset`, if that header names
    /// `num`, and the extent of the tokens it was read from: to the end of
    /// `endobj`, or for a stream to the first byte of its data. The header
    /// check is what makes level 1 of the ladder safe.
    fn parse_at(
        &self,
        num: u32,
        offset: u64,
        ctx: &mut ResolveCtx,
        sink: &mut WarningSink,
    ) -> Option<(Object, u64)> {
        count::touched(num);
        let (parsed, warnings) = self.parse_windowed(offset, ctx, sink)?;
        if parsed.reference.num != num {
            return None;
        }
        count::parsed();
        sink.extend(warnings);
        let read_to = match &parsed.object {
            Object::Stream(stream) => stream.data_start,
            _ => parsed.end_offset,
        };
        let extent = read_to.saturating_sub(offset);
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
        if self.encrypted() && self.encrypt_num() != Some(num) {
            decrypt::decrypt_strings(
                &mut object,
                ObjRef::new(num, parsed.reference.gen),
                self.decryptor().as_ref(),
            );
        }
        Some((object, extent))
    }

    /// Member `num` of the object stream `stream_num`, read out of its
    /// decoded data, and the extent of its tokens there.
    fn load_from_objstm(
        &self,
        num: u32,
        stream_num: u32,
        idx: u32,
        ctx: &mut ResolveCtx,
        sink: &mut WarningSink,
    ) -> (Object, u64) {
        count::touched(num);
        let Some(stm) = self.object_stream(stream_num, ctx, sink) else {
            sink.warn_at(
                self.entry_offset(stream_num),
                Some(ObjRef::new(num, 0)),
                WarningKind::ObjStmNotAStream,
            );
            return (Object::Null, 0);
        };
        let at = self.entry_offset(stream_num);
        let Some(offset) = stm.locate(num, idx, at, sink) else {
            return (Object::Null, 0);
        };

        let mut local = WarningSink::new();
        let parsed = parse_object_at(&stm.data, offset, &self.names.table, &mut local);
        sink.add_steps(local.steps());
        sink.parsed_object();
        count::parsed();
        let extent = parsed.end_offset.saturating_sub(offset);
        // 7.5.7: a contained object is never itself a stream.
        if objstm::is_stream_at(&stm.data, parsed.end_offset) {
            sink.warn_at(
                at,
                Some(ObjRef::new(num, 0)),
                WarningKind::ObjStmEntryIsStream,
            );
            return (Object::Null, extent);
        }
        sink.extend(local.take());
        // 7.6.2: no per-string decryption here — the container was decrypted
        // as a whole, and decrypting its contents again would corrupt them.
        (parsed.object, extent)
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

    fn extract_encrypt(&mut self, work: Option<&Work>) {
        let Some(entry) = self.trailer.get(Name::ENCRYPT).cloned() else {
            return;
        };
        let (dict, reference) = match entry {
            Object::Dict(dict) => (Some(dict), None),
            Object::Ref(r) => {
                let object = self
                    .get_with(r, work)
                    .unwrap_or_else(|_| Arc::new(Object::Null));
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

    fn expand_object_streams(&mut self, work: Option<&Work>) {
        let Some(scan) = self.scan.read_lock().clone() else {
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
        if let Some(work) = work {
            sink.steps().tally(work);
        }
        self.absorb(sink);
    }
}

/// Every byte of a document, fetching them when it is a streamed one and
/// saying so first.
///
/// The declaration is the point (ruling 10). A streamed open that quietly
/// pulled the whole source would satisfy every functional test and destroy the
/// only thing streaming is for, and the byte budgets could not tell the two
/// apart. Warned once: a second whole-file operation on the same document
/// fetches nothing, so warning again would report an operation that did not
/// happen.
fn fetch_whole(backing: &Backing, sink: &mut WarningSink) -> Option<Arc<[u8]>> {
    let declare = backing.is_streamed() && !backing.whole_fetched();
    // The fetch is attempted before it is declared, because a declaration of
    // something that did not happen is worse than none: a host reading the
    // warnings would see a whole-file read it was never asked for.
    let all = backing.materialise().ok()?;
    if declare {
        sink.warn(0, WarningKind::WholeFileFetched);
    }
    Some(all)
}

/// What a linearized file (Annex F) promises about its own head.
///
/// Held only when the fast path engaged, which is only for a streamed
/// document: a buffer already has every byte, so there is nothing for a fast
/// path to save and every reason not to take a second route through the same
/// clauses.
pub(crate) struct Linearized {
    /// Table F.1's `/E`: the offset of the end of the first page.
    ///
    /// Everything past it is the tail, and page one is rendered without
    /// touching a byte of it.
    end_of_first_page: u64,
    /// Table F.1's `/T`: where the main cross-reference table begins, as the
    /// file states it -- 7.5.2's header shift is applied by the walker that
    /// reads it rather than here. Fetched only when a read leaves what the
    /// hint tables can place.
    main_table_at: u64,
    /// Table F.1's `/O`: the object number of the first page's page object.
    ///
    /// Annex F names it so that a reader holding only the head can reach page
    /// one **without the page tree**, whose root a linearized file is free to
    /// leave in the tail -- and which qpdf's linearizer does leave there, so
    /// this is not a nicety.
    first_page_object: u32,
    /// `/N`: how many pages the file says it has.
    ///
    /// The page offset hint table has one row per page and states its own row
    /// count nowhere, so the tables cannot be unpacked without this -- which
    /// makes it an attacker-chosen count that sizes a decode, and
    /// [`crate::validate::hints::decode`] caps it before anything is allocated
    /// for it.
    page_count: Option<u32>,
    /// `/H`: where the primary hint stream is, and how long.
    ///
    /// Both numbers are needed twice over. F.3.6 gives the hint streams the
    /// last object numbers in the file and says nothing that obliges the
    /// first-page table to carry an entry for the stream -- qpdf's does not --
    /// so `/H` is the only route to it. And F.4 writes every position a hint
    /// table states "as if the primary hint stream itself were not present",
    /// so the length is what turns one back into a file offset.
    hint: Option<(u64, u64)>,
    /// 7.5.2's header shift, which every offset the file states is measured
    /// against. Zero for a file whose `%PDF-` is at byte zero, which is nearly
    /// all of them and was every one this path was first measured on.
    shift: u64,
}

/// Whether Annex F's hint tables have been read, and what came of it.
enum HintState {
    /// Nothing has asked for a page other than the first.
    Untried,
    /// They were read and would not describe this file; the page tree walk
    /// answers instead, and says so once (ruling 10).
    Refused,
    /// They were read.
    Read(Arc<PageHints>),
}

/// Where Annex F's hint tables say each page's objects are (F.4.1, F.4.2).
///
/// Byte ranges and nothing else. The tables name *where to look*; what is
/// there is read out of the file's own `N G obj` headers, and every object
/// still passes [`CosDocument::parse_at`]'s header check when it loads. That
/// is the whole of "hints accelerate, never decide" in this module: a table
/// that lies costs a wasted window, never a wrong object.
struct PageHints {
    /// One byte range per page, in page order.
    ///
    /// Table F.4 item 2: a page's length runs from its page object to the last
    /// byte of the last object it uses, and the location of each page after
    /// the first is the accumulated lengths of the ones before it.
    runs: Vec<Range<u64>>,
    /// Part 8's shared object groups, in table order, from the first entry
    /// that is not a first-page one. Table index `k` is `k -
    /// first_page_groups` here.
    groups: Vec<Range<u64>>,
    /// Table F.5 item 3: how many leading entries of the shared object table
    /// describe first-page objects, which the first-page cross-reference table
    /// already carries.
    first_page_groups: usize,
    /// Table F.4 item 4: which shared object entries each page names.
    named: Vec<Vec<u32>>,
}

/// The head-only open of a linearized file (Annex F).
///
/// Hints accelerate, they never decide. Nothing here is trusted further than
/// it can be checked: `/L` is held to the source's own length, which is Annex
/// F's own rule for spotting a linearized file that was incrementally updated
/// and must be read as an ordinary one; the first-page section is parsed by
/// the same walker every other section goes through; and every object it
/// names still passes `parse_at`'s `N G obj` check at load. A file whose head
/// does not hold up falls back to the generic path with a typed warning
/// rather than failing (rulings 1 and 2).
fn linearized_open(
    backing: &Backing,
    names: &DocNames,
    shift: u64,
    sink: &mut WarningSink,
) -> Option<(XrefBuild, Linearized)> {
    let view = backing.view();
    let len = backing.len();
    let mut want = limits::MAX_HEADER_SCAN as u64;
    loop {
        let head = view.window(0, want)?;
        let reaches_end = head.end() >= len;
        let mut scratch = WarningSink::new();

        // F.3.3: the parameter dictionary is the first object in the body of
        // the file, and a file whose first object is anything else is simply
        // not linearized.
        let at = next_object_header(head.bytes(), 0)?;
        let Some(first) = parse_indirect_at(head.bytes(), at, &names.table, &mut scratch) else {
            if reaches_end {
                return None;
            }
            want = want.saturating_mul(2);
            continue;
        };
        let linearized = names.table.intern(b"Linearized");
        let dict = first
            .object
            .as_dict()
            .filter(|d| d.contains_key(linearized))?;
        let number = |key: &[u8]| {
            dict.get_int(names.table.intern(key))
                .and_then(|v| u64::try_from(v).ok())
        };

        // Annex F's own rule for a linearized file that was incrementally
        // updated: `/L` is the length of the whole file as it was linearized,
        // so a file that has grown since is read as an ordinary one.
        if number(b"L") != Some(len) {
            sink.warn(head.abs(at), WarningKind::LinearizedLengthMismatch);
            return None;
        }
        // 7.5.2 again: `/E` is an offset the file states, so it is measured
        // from `%PDF-` and not from byte zero -- and it is compared against
        // document offsets directly, by `head_ceiling` and by every caller of
        // `first_page_end`, so the shift is applied here. `/T` is not: it
        // reaches `xref::build`, whose walker already tries the shifted offset
        // first, and shifting it here as well would be the same correction
        // applied twice.
        let (Some(end_of_first_page), Some(main_table_at), Some(first_page_object)) = (
            number(b"E").map(|at| at.saturating_add(shift)),
            number(b"T"),
            number(b"O").and_then(|v| u32::try_from(v).ok()),
        ) else {
            sink.warn(head.abs(at), WarningKind::LinearizedParametersUnusable);
            return None;
        };
        // `/N` and `/H` are what the hint tables are reached and sized by, and
        // an absent or unusable one is not a reason to abandon the head-only
        // open: page one is found through `/O` and the first-page section,
        // neither of which needs a hint table. So they are carried as `None`
        // and only a page *other* than the first pays for them.
        let page_count = number(b"N").and_then(|v| u32::try_from(v).ok());
        let hint = dict.get_array(names.table.intern(b"H")).and_then(|array| {
            let at = array.first()?.as_int()?;
            let len = array.get(1)?.as_int()?;
            Some((u64::try_from(at).ok()?, u64::try_from(len).ok()?))
        });

        // The first-page cross-reference section follows part 2 immediately.
        // Both spellings are tried, because 7.5.8 lets it be a stream.
        let from = first.end_offset;
        let classic = find_from(
            head.bytes(),
            b"xref",
            usize::try_from(from).unwrap_or(usize::MAX),
        );
        let streamed = next_object_header(head.bytes(), from);
        let candidates: Vec<u64> = classic
            .map(|at| at as u64)
            .into_iter()
            .chain(streamed)
            .map(|local| head.abs(local))
            .collect();
        if candidates.is_empty() {
            if !reaches_end {
                want = want.saturating_mul(2);
                continue;
            }
            sink.warn(head.abs(at), WarningKind::LinearizedParametersUnusable);
            return None;
        }
        drop(head);

        for section in candidates {
            let mut scratch = WarningSink::new();
            // One section, never the `/Prev` it carries: that link is what
            // points at the main table at the end of the file, and following
            // it is the one thing that would put a tail read in a head-only
            // open.
            let built = xref::build_head(backing.view(), section, names, &mut scratch);
            if built.sections > 0
                && !built.table.is_empty()
                && root_locatable(&built.table, &built.trailer)
            {
                sink.extend(scratch.take());
                return Some((
                    built,
                    Linearized {
                        end_of_first_page,
                        main_table_at,
                        first_page_object,
                        page_count,
                        hint,
                        shift,
                    },
                ));
            }
        }
        sink.warn(0, WarningKind::LinearizedParametersUnusable);
        return None;
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
    validate_with(buffer.len() as u64, table, shift, |at| {
        xref::object_header_at(buffer, at)
    })
}

/// [`validate`] over a document of `len` bytes whose header at an offset
/// `header_at` reads: the same probes in the same order, wherever the bytes
/// lie.
fn validate_with(
    len: u64,
    table: &mut XrefTable,
    shift: u64,
    mut header_at: impl FnMut(u64) -> Option<ObjRef>,
) -> Validation {
    let mut failures = Vec::new();
    let mut fixes = Vec::new();
    let mut offsets = 0usize;
    for (num, entry) in table.iter() {
        let XrefEntry::Offset { offset, .. } = entry else {
            continue;
        };
        offsets += 1;
        count::touched(num);
        let found = xref::offset_candidates(len, offset, shift)
            .into_iter()
            .find_map(|at| header_at(at).filter(|h| h.num == num).map(|h| (at, h.gen)));
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

/// The first string of a trailer's `/ID` (14.4), from which an encrypted
/// document's key is computed (7.6.4.3).
fn first_id(trailer: &Dict) -> Option<&[u8]> {
    trailer
        .get_array(Name::ID)
        .and_then(<[Object]>::first)
        .and_then(Object::as_string)
        .map(|id| id.bytes.as_slice())
}

/// The object numbers an open touches — whose cross-reference row it reads,
/// whose header it probes, whose body it parses — counted for the reopen's
/// guard, and the objects parsed out of the bytes, counted for the budget's
/// (`store.rs`): compiled only under `cfg(test)` and kept per thread.
#[cfg(test)]
pub(crate) mod count {
    use std::cell::RefCell;
    use std::collections::BTreeSet;

    std::thread_local! {
        static TOUCHED: RefCell<BTreeSet<u32>> = const { RefCell::new(BTreeSet::new()) };
        static PARSES: core::cell::Cell<u64> = const { core::cell::Cell::new(0) };
    }

    /// Object `num` touched.
    pub(crate) fn touched(num: u32) {
        TOUCHED.with(|touched| {
            touched.borrow_mut().insert(num);
        });
    }

    /// The numbers touched on this thread since the last call, which starts
    /// the count again from nothing.
    pub(crate) fn take() -> BTreeSet<u32> {
        TOUCHED.with(|touched| core::mem::take(&mut *touched.borrow_mut()))
    }

    /// One object parsed out of the bytes: an indirect object whose header
    /// named it, or a member read out of its object stream.
    pub(crate) fn parsed() {
        PARSES.with(|parses| parses.set(parses.get().saturating_add(1)));
    }

    /// The objects parsed on this thread since the last call, which starts
    /// the count again from nothing.
    pub(crate) fn parses() -> u64 {
        PARSES.with(|parses| parses.replace(0))
    }
}

/// The counter outside tests: nothing.
#[cfg(not(test))]
pub(crate) mod count {
    pub(crate) fn touched(_num: u32) {}
    pub(crate) fn parsed() {}
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

#[cfg(test)]
mod tests {
    use super::*;

    /// A document whose body is `count` one-line objects, and where they sit.
    fn objects(count: usize) -> (Vec<u8>, Range<u64>) {
        let mut bytes = b"%PDF-1.7\n".to_vec();
        let start = bytes.len() as u64;
        for num in 1..=count {
            bytes.extend_from_slice(format!("{num} 0 obj\nnull\nendobj\n").as_bytes());
        }
        let end = bytes.len() as u64;
        bytes.extend_from_slice(b"trailer\n<< /Size 2 /Root 1 0 R >>\n%%EOF\n");
        (bytes, start..end)
    }

    /// Ruling 1: a hint-named byte range is the file's choice, and so is how
    /// many object headers it packs into it.
    ///
    /// Every header found becomes a map entry, so the walk is bounded by a
    /// count rather than by the range's length -- a megabyte of `1 0 obj`
    /// would otherwise buy a map of a hundred thousand entries for a page that
    /// has none. The range is refused whole rather than half-recorded, because
    /// half a page's objects placed and the rest not is a page drawn out of
    /// two different tables.
    #[test]
    fn a_hint_named_range_of_too_many_objects_is_refused_whole() {
        let (bytes, range) = objects(limits::MAX_HINTED_PAGE_OBJECTS + 1);
        let doc = CosDocument::open(bytes).expect("it opens");
        assert_eq!(doc.record_hinted(range), None, "one over the cap");
        assert!(
            doc.hinted.read_lock().is_empty(),
            "and nothing at all was recorded"
        );

        // The cap itself is recorded, so what refused the above is the count
        // being over it rather than anything else about these bytes.
        let (bytes, range) = objects(limits::MAX_HINTED_PAGE_OBJECTS);
        let doc = CosDocument::open(bytes).expect("it opens");
        assert_eq!(
            doc.record_hinted(range),
            Some(ObjRef::new(1, 0)),
            "the run is led by its first object"
        );
        assert_eq!(
            doc.hinted.read_lock().len(),
            limits::MAX_HINTED_PAGE_OBJECTS
        );
    }

    /// A range outside the document buys nothing and reads nothing.
    #[test]
    fn a_hint_named_range_outside_the_document_records_nothing() {
        let (bytes, range) = objects(4);
        let len = bytes.len() as u64;
        let doc = CosDocument::open(bytes).expect("it opens");
        assert_eq!(doc.record_hinted(len..len + 4096), None, "past the end");
        assert_eq!(doc.record_hinted(range.end..range.start), None, "inverted");
        assert_eq!(doc.record_hinted(0..0), None, "empty");
        assert!(doc.hinted.read_lock().is_empty());
    }

    /// Nothing a hint places ever displaces the first-page cross-reference
    /// section, which is a table the file wrote rather than a hint about one.
    #[test]
    fn a_recorded_offset_never_replaces_one_already_there() {
        let (bytes, range) = objects(4);
        let doc = CosDocument::open(bytes).expect("it opens");
        doc.hinted.write_lock().insert(
            2,
            XrefEntry::Offset {
                offset: 999,
                gen: 0,
            },
        );
        assert_eq!(doc.record_hinted(range), Some(ObjRef::new(1, 0)));
        assert_eq!(
            doc.hinted.read_lock().get(&2).copied(),
            Some(XrefEntry::Offset {
                offset: 999,
                gen: 0
            }),
            "the entry that was already there is the entry that stays"
        );
    }

    // ---- the windowed verdict, the scanner's gate, and the predicate -------

    /// A classic-table document written by hand, so a test can say what its
    /// table claims about each object and where the bytes really are.
    #[derive(Clone, Default)]
    struct Fixture {
        /// Bytes before `%PDF-` (7.5.2).
        prefix: &'static [u8],
        /// Object bodies, numbered from 1. Object 1 is the catalog.
        bodies: Vec<String>,
        /// A comment of this many bytes before every object, so that objects
        /// can sit chunks or windows apart.
        pad: usize,
        /// Objects whose entry states where the comment before them starts
        /// rather than where their header does.
        lead: Vec<u32>,
        /// Objects whose entry is free while their body stays in place.
        free: Vec<u32>,
        /// `(n, m)`: object `n`'s entry states where object `m` is.
        lies: Vec<(u32, u32)>,
        /// Objects whose entry states an offset past the end of the file.
        beyond: Vec<u32>,
        /// Bodies written after the tabled ones and named by no entry,
        /// numbered on from them.
        orphans: Vec<String>,
        /// `startxref` misspelt, so no reader finds the table.
        hide_startxref: bool,
    }

    impl Fixture {
        /// A catalog, an empty page tree and `extra` more objects, each
        /// tabled where it is.
        fn honest(extra: &[&str]) -> Fixture {
            let mut bodies = vec![
                "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
                "<< /Type /Pages /Count 0 /Kids [] >>".to_string(),
            ];
            bodies.extend(extra.iter().map(|body| (*body).to_string()));
            Fixture {
                bodies,
                ..Fixture::default()
            }
        }

        fn bytes(&self) -> Vec<u8> {
            let mut out = self.prefix.to_vec();
            let header = out.len();
            out.extend_from_slice(b"%PDF-1.7\n");
            let mut heads = Vec::new();
            let mut leads = Vec::new();
            for (index, body) in self.bodies.iter().chain(&self.orphans).enumerate() {
                leads.push((out.len() - header) as u64);
                if self.pad > 0 {
                    out.push(b'%');
                    out.extend(std::iter::repeat_n(b'.', self.pad));
                    out.push(b'\n');
                }
                heads.push((out.len() - header) as u64);
                out.extend_from_slice(format!("{} 0 obj\n{body}\nendobj\n", index + 1).as_bytes());
            }
            let xref_at = out.len() - header;
            let count = self.bodies.len();
            out.extend_from_slice(
                format!("xref\n0 {}\n0000000000 65535 f \n", count + 1).as_bytes(),
            );
            for index in 0..count {
                let num = u32::try_from(index + 1).expect("a small fixture");
                if self.free.contains(&num) {
                    out.extend_from_slice(b"0000000000 00001 f \n");
                    continue;
                }
                let lie = self.lies.iter().find(|(n, _)| *n == num);
                let at = match lie {
                    _ if self.beyond.contains(&num) => 9_999_999_999,
                    Some(&(_, other)) => heads[other as usize - 1],
                    None if self.lead.contains(&num) => leads[index],
                    None => heads[index],
                };
                out.extend_from_slice(format!("{at:010} 00000 n \n").as_bytes());
            }
            let keyword = if self.hide_startxref {
                "startxrEf"
            } else {
                "startxref"
            };
            out.extend_from_slice(
                format!(
                    "trailer\n<< /Size {} /Root 1 0 R >>\n{keyword}\n{xref_at}\n%%EOF\n",
                    count + 1
                )
                .as_bytes(),
            );
            out
        }
    }

    fn buffered(bytes: &[u8]) -> CosDocument {
        CosDocument::open(bytes.to_vec()).expect("it opens from a buffer")
    }

    fn streamed(bytes: &[u8]) -> CosDocument {
        CosDocument::open_source(Arc::new(crate::SliceSource::new(bytes.to_vec())))
            .expect("it opens from a source")
    }

    /// Over the most hostile source that conforms, which answers every read
    /// one byte at a time.
    fn shredded(bytes: &[u8]) -> CosDocument {
        CosDocument::open_source(Arc::new(crate::ShreddedSource::new(
            crate::SliceSource::new(bytes.to_vec()),
        )))
        .expect("it opens from a shredded source")
    }

    /// A linearized file of `pages` pages, as this crate's writer lays one out
    /// (Annex F).
    fn linearized(pages: usize) -> Vec<u8> {
        let mut builder = crate::DocumentBuilder::new();
        for _ in 0..pages {
            builder.add_page(200.0, 100.0, |page| page.raw(b"0 0 10 10 re f"));
        }
        let doc = Arc::new(crate::written_support::authored(
            "pages to linearize",
            builder.finish(),
        ));
        let bytes = crate::DocumentEditor::new(doc).save(&crate::WriteOptions {
            mode: crate::WriteMode::Rewrite,
            linearize: true,
            // A container would hold the first page's objects in the same blob
            // as everything else, and there would be no head to open from.
            object_streams: false,
            ..crate::WriteOptions::default()
        });
        crate::written_support::authored("those pages linearized", &bytes);
        bytes
    }

    /// [`linearized`], with one entry of its main table, the table at `/T`
    /// that a head-only open never reads, stating where another object is.
    fn linearized_with_a_lying_main_entry() -> Vec<u8> {
        let mut bytes = linearized(3);
        let whole = buffered(&bytes);
        let head = streamed(&bytes);
        assert!(
            head.first_page_end().is_some(),
            "the head-only path engages"
        );
        // An object only the main table places.
        let (num, offset) = whole
            .xref()
            .iter()
            .find_map(|(num, entry)| match entry {
                XrefEntry::Offset { offset, .. } if head.xref.get(num).is_none() => {
                    Some((num, offset))
                }
                _ => None,
            })
            .expect("the main table places objects the first-page section does not");
        let elsewhere = whole
            .xref()
            .iter()
            .find_map(|(other, entry)| match entry {
                XrefEntry::Offset { offset, .. } if other != num => Some(offset),
                _ => None,
            })
            .expect("another object");
        let table = rfind_from(&bytes, b"\nxref\n", 0).expect("a classic main table");
        let trailer = find_from(&bytes, b"trailer", table).expect("its trailer");
        let at = find_from(&bytes, format!("{offset:010} ").as_bytes(), table)
            .filter(|at| *at < trailer)
            .expect("the main table states it");
        bytes[at..at + 10].copy_from_slice(format!("{elsewhere:010}").as_bytes());
        bytes
    }

    /// A streamed document's verdict is the ladder level a buffered open of
    /// the same bytes reaches, however its tables are damaged and whichever
    /// table states an entry.
    ///
    /// Each row is a shape of the probe. A shifted header has two candidate
    /// offsets per entry; a lying entry, one past the end and four lying of six
    /// are the failures it counts; objects windows apart are read apart; and an
    /// entry stating the comment before its header makes a probe outgrow its
    /// first window, since the lexer skips the comment before it reads `N G
    /// obj`. The last row is the merged table: a linearized file opened from
    /// its head reads one cross-reference section at open, and the entry that
    /// lies is in the other.
    #[test]
    fn windowed_validation_decides_what_the_buffered_open_decided() {
        let honest = Fixture::honest(&["(three)", "(four)", "(five)", "(six)"]);
        let rows: Vec<(&str, Vec<u8>, LadderLevel)> = vec![
            ("clean", honest.bytes(), LadderLevel::Trust),
            (
                "shifted header",
                Fixture {
                    prefix: b"leading junk\n",
                    ..honest.clone()
                }
                .bytes(),
                LadderLevel::Trust,
            ),
            (
                "patched",
                Fixture {
                    lies: vec![(4, 3)],
                    ..honest.clone()
                }
                .bytes(),
                LadderLevel::Patch,
            ),
            (
                "rescanned",
                Fixture {
                    hide_startxref: true,
                    ..honest.clone()
                }
                .bytes(),
                LadderLevel::Rescan,
            ),
            (
                "rescanned for its failures",
                Fixture {
                    lies: vec![(3, 4), (4, 5), (5, 6), (6, 3)],
                    ..honest.clone()
                }
                .bytes(),
                LadderLevel::Rescan,
            ),
            (
                "an entry past the end of the file",
                Fixture {
                    beyond: vec![5],
                    ..honest.clone()
                }
                .bytes(),
                LadderLevel::Patch,
            ),
            (
                "objects windows apart",
                Fixture {
                    pad: limits::VALIDATION_WINDOW as usize,
                    ..honest.clone()
                }
                .bytes(),
                LadderLevel::Trust,
            ),
            (
                "objects windows apart, one lying",
                Fixture {
                    pad: limits::VALIDATION_WINDOW as usize,
                    lies: vec![(6, 3)],
                    ..honest.clone()
                }
                .bytes(),
                LadderLevel::Patch,
            ),
            (
                "headers behind comments longer than a probe",
                Fixture {
                    pad: 3 * limits::HEADER_PROBE_WINDOW as usize,
                    lead: vec![2, 3, 4, 5, 6],
                    ..honest.clone()
                }
                .bytes(),
                LadderLevel::Trust,
            ),
            (
                "headers behind comments, shifted",
                Fixture {
                    prefix: b"leading junk\n",
                    pad: 3 * limits::HEADER_PROBE_WINDOW as usize,
                    lead: vec![2, 3, 4, 5, 6],
                    ..honest.clone()
                }
                .bytes(),
                LadderLevel::Trust,
            ),
            (
                "a linearized file whose main table lies, opened from its head",
                linearized_with_a_lying_main_entry(),
                LadderLevel::Patch,
            ),
        ];
        for (name, bytes, level) in rows {
            let whole = buffered(&bytes);
            assert_eq!(
                whole.ladder_level(),
                level,
                "{name}: the fixture's own level"
            );
            for (how, doc) in [
                ("streamed", streamed(&bytes)),
                ("shredded", shredded(&bytes)),
            ] {
                assert_eq!(
                    doc.complete_validation(),
                    level,
                    "{name}, {how}: the verdict a buffered open reached"
                );
                assert_eq!(
                    doc.complete_validation(),
                    level,
                    "{name}, {how}: and the same verdict when asked again"
                );
            }
        }
    }

    /// The verdict is reached through windows nothing keeps.
    ///
    /// The chunk cache holds what the open read, the head and the tail, and
    /// the same after the call; the probes between them were read, which the
    /// count of source reads shows, and dropped.
    #[test]
    fn validation_keeps_no_byte_it_read() {
        let honest = Fixture {
            pad: 3 * crate::CHUNK_SIZE as usize,
            ..Fixture::honest(&["(three)", "(four)", "(five)", "(six)"])
        };
        let lying = Fixture {
            lies: vec![(4, 3)],
            ..honest.clone()
        };
        for (name, bytes, level) in [
            ("clean", honest.bytes(), LadderLevel::Trust),
            ("patched", lying.bytes(), LadderLevel::Patch),
        ] {
            let source = Arc::new(crate::CountingSource::new(crate::SliceSource::new(
                bytes.clone(),
            )));
            let doc = CosDocument::open_source(Arc::clone(&source) as Arc<dyn ByteSource>)
                .expect("it opens");
            let held = doc.buffer.retained();
            assert!(
                held < bytes.len() as u64,
                "{name}: the open read part of the file"
            );
            let reads = source.reads();
            assert_eq!(doc.complete_validation(), level, "{name}: the verdict");
            assert!(source.reads() > reads, "{name}: the probes read the source");
            assert_eq!(
                doc.buffer.retained(),
                held,
                "{name}: and the cache holds what it held before"
            );
            assert!(
                !doc.whole_file_fetched(),
                "{name}: nothing fetched the file"
            );
            assert!(
                !doc.warnings()
                    .iter()
                    .any(|w| w.kind == WarningKind::WholeFileFetched),
                "{name}: and nothing says it did"
            );
        }
    }

    /// An entry the table cannot place reads as a buffered open reads it.
    ///
    /// Over a clean file a buffered open has no repair scanner, so a free
    /// entry and a number with no entry read as null (7.3.10), even where the
    /// file still holds a body under that number. Over a patched file it has
    /// one, and both read what it finds. Each row opens its own streamed
    /// document, so no row's fetch is another's.
    #[test]
    fn a_free_or_absent_entry_reads_as_the_buffered_open_reads_it() {
        let clean = Fixture {
            free: vec![3],
            orphans: vec!["(orphan)".to_string()],
            ..Fixture::honest(&["(resurrected)", "(four)"])
        };
        let patched = Fixture {
            lies: vec![(4, 2)],
            ..clean.clone()
        };
        let resurrected = Object::String(crate::PdfString::literal(b"resurrected".to_vec()));
        let orphan = Object::String(crate::PdfString::literal(b"orphan".to_vec()));
        let rows = [
            (
                "clean",
                "a free entry, its old body in place",
                &clean,
                3,
                Object::Null,
            ),
            (
                "clean",
                "a number with no entry, a body under it",
                &clean,
                5,
                Object::Null,
            ),
            (
                "clean",
                "a number with no entry and no body",
                &clean,
                9,
                Object::Null,
            ),
            (
                "patched",
                "a free entry, its old body in place",
                &patched,
                3,
                resurrected,
            ),
            (
                "patched",
                "a number with no entry, a body under it",
                &patched,
                5,
                orphan,
            ),
            (
                "patched",
                "a number with no entry and no body",
                &patched,
                9,
                Object::Null,
            ),
        ];
        for (file, shape, fixture, num, want) in rows {
            let bytes = fixture.bytes();
            let whole = buffered(&bytes);
            let read = whole.get(ObjRef::new(num, 0)).expect("it reads");
            assert_eq!(*read, want, "{file}, {shape}: what a buffered open reads");
            for (how, doc) in [
                ("streamed", streamed(&bytes)),
                ("shredded", shredded(&bytes)),
            ] {
                let got = doc.get(ObjRef::new(num, 0)).expect("it reads");
                assert_eq!(*got, *read, "{file}, {shape}, {how}: the same value");
                if file == "clean" {
                    assert!(
                        !doc.whole_file_fetched(),
                        "{file}, {shape}, {how}: and no fetch of the file to find it"
                    );
                }
            }
        }
    }

    /// A streamed document answers as buffered only when none of the three
    /// ways the two can part has happened, and a buffered one always does.
    ///
    /// Each row that answers no is parted by exactly one of them, which the
    /// row states and the test reads back, so a predicate that lost any one
    /// condition answers yes for that row and fails here. The linearized
    /// file opened from its head warns nothing and decides `Trust`; one byte
    /// after it is an update to `/L`, warned at open, as a shifted header is;
    /// and the patched file warns nothing until a read reaches its entry.
    #[test]
    fn a_document_answers_as_buffered_only_when_nothing_could_part_them() {
        let honest = Fixture::honest(&["(three)", "(four)"]);
        let patched = Fixture {
            lies: vec![(4, 3)],
            ..honest.clone()
        }
        .bytes();
        let shifted = Fixture {
            prefix: b"leading junk\n",
            ..honest.clone()
        }
        .bytes();
        let mut updated = linearized(2);
        updated.push(b'\n');
        // (the row, the document, its answer, and what parts it: opened from
        // the head, warned at open, its verdict)
        let rows = [
            ("buffered", buffered(&honest.bytes()), true, None),
            ("buffered and patched", buffered(&patched), true, None),
            (
                "streamed",
                streamed(&honest.bytes()),
                true,
                Some((false, false, LadderLevel::Trust)),
            ),
            (
                "streamed from a linearized head",
                streamed(&linearized(2)),
                false,
                Some((true, false, LadderLevel::Trust)),
            ),
            (
                "streamed, a linearized file one byte longer than its /L",
                streamed(&updated),
                false,
                Some((false, true, LadderLevel::Trust)),
            ),
            (
                "streamed behind leading junk",
                streamed(&shifted),
                false,
                Some((false, true, LadderLevel::Trust)),
            ),
            (
                "streamed and patched",
                streamed(&patched),
                false,
                Some((false, false, LadderLevel::Patch)),
            ),
        ];
        for (name, doc, answer, parts) in rows {
            let held = doc.buffer.retained();
            assert_eq!(doc.answers_as_buffered(), answer, "{name}");
            assert_eq!(
                doc.answers_as_buffered(),
                answer,
                "{name}: and the same when asked again"
            );
            let Some((head, warned, verdict)) = parts else {
                assert!(!doc.is_streamed(), "{name}");
                continue;
            };
            assert!(doc.is_streamed(), "{name}");
            assert_eq!(
                doc.buffer.retained(),
                held,
                "{name}: asking kept nothing it read"
            );
            assert!(!doc.whole_file_fetched(), "{name}: nor fetched the file");
            assert_eq!(
                doc.first_page_end().is_some(),
                head,
                "{name}: the head path"
            );
            assert_eq!(!doc.warnings().is_empty(), warned, "{name}: warned at open");
            assert_eq!(doc.complete_validation(), verdict, "{name}: the verdict");
        }
    }

    /// What the windowed probe finds at each of `offsets`, sorted, each asked
    /// for `times` over so that equal offsets meet in one run.
    fn through_windows(doc: &CosDocument, offsets: &[u64], times: usize) -> Vec<Option<ObjRef>> {
        let probes: Vec<(u64, usize)> = offsets
            .iter()
            .enumerate()
            .flat_map(|(index, at)| std::iter::repeat_n((*at, index), times))
            .collect();
        let mut got = vec![None; offsets.len()];
        let mut asked = 0usize;
        doc.headers_through_windows(&probes, |index, header| {
            got[index] = header;
            asked += 1;
        })
        .expect("a slice source has every byte");
        assert_eq!(asked, probes.len(), "one answer for every probe");
        got
    }

    /// A window reads each header as the whole file does, wherever its edges
    /// fall.
    ///
    /// The verdict rests on this, so the fixture holds every shape that runs
    /// into an edge: a comment and a run of white space longer than a probe, a
    /// comment longer than a whole coalesced read, numbers and keywords a cut
    /// would shorten, `<<` and `>>`, names with escapes, strings with nested
    /// parentheses, headers that are not headers. It is several coalesced
    /// reads long. Every offset is probed together, so runs coalesce and a
    /// grown window is carried into the next run; every offset alone, so each
    /// probe starts from a window of [`limits::HEADER_PROBE_WINDOW`] bytes and
    /// grows from there; a sparse set, which starts runs inside the comment a
    /// grown window holds; and the last bytes, where a window reaches the end
    /// of the document.
    #[test]
    fn a_window_reads_each_header_as_the_whole_file_does() {
        let fragments = [
            "12 0 obj ",
            "12 0 objx ",
            "4294967296 0 obj ",
            "7 65536 obj ",
            "-3 0 obj ",
            "0000000012 00000 obj\n",
            "12\n0\nobj<<",
            "12 0 obj<</A 1>>",
            "% a comment that runs on for longer than one probe reads, up to its line end\n",
            "                                                                         ",
            "(a (nested) string 1 0 obj) ",
            "<< /Name#41#4 1 0 R >> ",
            "<4142> ",
            "endobj ",
            "3.5 0 obj ",
            "+4 0 obj ",
            "9 0 obj%\n",
            "\r\n\t\x0c",
        ];
        let mut body = String::new();
        for index in 0.. {
            if body.len() >= 3 * limits::VALIDATION_WINDOW as usize {
                break;
            }
            body.push_str(fragments[index % fragments.len()]);
            if index % 7 == 0 {
                body.push_str(&format!("{index} 0 obj "));
            }
        }
        let dense_end = body.len() as u64;
        // A comment longer than one coalesced read, which only growth sees
        // past, and a header behind it.
        body.push('%');
        body.push_str(&".".repeat(limits::VALIDATION_WINDOW as usize + 100));
        body.push_str("\n 5 0 obj ");
        let bytes = Fixture::honest(&[&body]).bytes();
        let len = bytes.len() as u64;
        let whole = |offsets: &[u64]| -> Vec<Option<ObjRef>> {
            offsets
                .iter()
                .map(|at| xref::object_header_at(&bytes, *at))
                .collect()
        };
        let dense: Vec<u64> = (0..dense_end.min(len)).collect();
        let sparse: Vec<u64> = (0..len).step_by(1009).collect();
        let tail: Vec<u64> = (len.saturating_sub(300)..len).collect();
        assert!(
            whole(&dense).iter().filter(|h| h.is_some()).count() > 1000,
            "the fixture is full of headers"
        );
        for (how, doc) in [
            ("streamed", streamed(&bytes)),
            ("shredded", shredded(&bytes)),
        ] {
            let held = doc.buffer.retained();
            for (set, offsets) in [
                ("every offset", &dense),
                ("sparse", &sparse),
                ("the tail", &tail),
            ] {
                for times in [1, 2] {
                    assert!(
                        through_windows(&doc, offsets, times) == whole(offsets),
                        "{how}, {set}, asked {times} times: a header read differently"
                    );
                }
            }
            for at in &dense {
                assert_eq!(
                    through_windows(&doc, &[*at], 1),
                    whole(&[*at]),
                    "{how}, alone at {at}: a header read differently"
                );
            }
            assert_eq!(doc.buffer.retained(), held, "{how}: and nothing kept");
        }
    }

    // ---- the reopen (P-17.4) ------------------------------------------------

    /// **A reopen parses only what changed** (`docs/design/performance.md`,
    /// P-17). One 100 by 100 pt square annotation added to page 501 of a
    /// generated 1 000-page file, saved as an update section and reopened
    /// over the document it was edited from: the reopen touches — reads the
    /// row of, probes the header of, or parses — only the numbers the
    /// section defines (the page, the annotation and its appearance) and the
    /// free head its table starts with, and parses no object's body, where a
    /// full open of the same bytes touches every number in the file. Reading
    /// the reopened document's whole page tree afterwards parses only what
    /// the update redefined, the page among it: every other node is the
    /// base's parse, kept.
    #[test]
    fn a_reopen_parses_only_what_changed() {
        let mut builder = crate::DocumentBuilder::new();
        builder.add_base_font(b"F0", b"Helvetica");
        for i in 0..1000 {
            builder.add_page(612.0, 792.0, |page| {
                page.text(b"F0", 12.0, 72.0, 720.0, &format!("page {i}"));
            });
        }
        let bytes = builder.finish();
        crate::written_support::authored("a thousand pages", &bytes);
        let base = Arc::new(CosDocument::open(bytes.clone()).expect("it opens"));
        let before = crate::pages::collect(&base);
        assert_eq!(before.len(), 1000, "the premise: a thousand pages");
        let highest = base.max_object_number();

        let mut editor = crate::DocumentEditor::new(Arc::clone(&base));
        let square = crate::annot::square(
            &base,
            crate::Rect {
                x0: 100.0,
                y0: 100.0,
                x1: 200.0,
                y1: 200.0,
            },
            crate::annot::Color {
                r: 1.0,
                g: 0.0,
                b: 0.0,
            },
            1.0,
        );
        let added = editor
            .add_annotation(500, square)
            .expect("page 501 is there");
        let update = editor.save_update(&crate::WriteOptions {
            mode: crate::WriteMode::Incremental,
            ..crate::WriteOptions::default()
        });
        // The save is the base and the update together, held before the
        // counting below begins, so the door's own open touches nothing it
        // counts.
        crate::written_support::authored(
            "a thousand pages, one annotated by an update",
            [&bytes[..], &update[..]].concat(),
        );
        let page = before[500].reference;
        // The page, and every number the edit allocated: the annotation, and
        // the appearance synthesized for it.
        let mut changed: std::collections::BTreeSet<u32> = (highest + 1..=added.num).collect();
        changed.insert(page.num);
        let mut allowed = changed.clone();
        allowed.insert(0);

        let _ = count::take();
        let work = Work::new();
        let reopened =
            CosDocument::reopen_counted(&base, update.clone(), &work).expect("it reopens");
        let touched = count::take();
        assert_eq!(reopened.full_open, None, "only the update was read");
        assert!(
            touched.is_subset(&allowed),
            "the reopen touched {touched:?}; the section defines {allowed:?}"
        );
        assert!(
            changed.is_subset(&touched),
            "every changed number's row is read: {touched:?}"
        );
        let stats = work.stats();
        assert_eq!(stats.objects_parsed, 0, "no object's body: {stats:?}");
        assert!(
            stats.xref_entries_read > 0 && stats.xref_entries_read <= allowed.len() as u64,
            "the section's rows and no others: {stats:?}"
        );

        // A full open of the same bytes touches every number in the file.
        let joined = [&bytes[..], &update[..]].concat();
        let whole = CosDocument::open(joined).expect("it opens");
        let all = count::take();
        assert!(
            all.len() > 1000 && all.is_superset(&allowed),
            "a full open reads every row: {} numbers",
            all.len()
        );

        // The page tree read again: what the update redefined is parsed, from
        // the section, and every other node is the base's.
        let document = reopened.document;
        let after = crate::pages::collect(&document);
        let parsed = count::take();
        assert!(
            parsed.contains(&page.num) && parsed.is_subset(&changed),
            "reading every page parsed {parsed:?}"
        );
        let fresh = crate::pages::collect(&whole);
        assert_eq!(after.len(), fresh.len());
        for (got, want) in after.iter().zip(&fresh) {
            assert_eq!(
                (got.reference, got.index, got.media_box, got.rotation),
                (want.reference, want.index, want.media_box, want.rotation),
                "page {} reads as a full open reads it",
                want.index
            );
        }
        let annots = document
            .get(page)
            .expect("the page loads")
            .as_dict()
            .and_then(|dict| dict.get_array(document.intern(b"Annots")))
            .map(<[Object]>::to_vec)
            .unwrap_or_default();
        assert_eq!(
            annots,
            vec![Object::Ref(added)],
            "the page names its annotation"
        );
        assert_eq!(document.ladder_level(), LadderLevel::Trust);
        assert_eq!(document.revisions().len(), 2, "the update, then the base");
    }
}
