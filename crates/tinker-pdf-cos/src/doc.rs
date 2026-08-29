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
    pub(crate) buffer: Backing,
    pub(crate) names: DocNames,
    xref: XrefTable,
    /// Annex F's head-only open, when it engaged. `None` for every document
    /// opened from a buffer.
    linearized: Option<Linearized>,
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
    /// open defers, so the decision moves to the first read that finds an
    /// entry lying. The values a caller sees are the same either way, which is
    /// the property that matters; what differs is when the fetch happens.
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
        CosDocument::open_backing(Backing::whole_buffer(bytes.into()))
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
        CosDocument::open_backing(Backing::chunked(source))
    }

    fn open_backing(backing: Backing) -> Result<CosDocument, OpenError> {
        let names = DocNames::new();
        let mut sink = WarningSink::new();
        let streamed = backing.is_streamed();

        // 7.5.2: bytes before %PDF- shift every offset the file stores. One
        // head window covers the scan limit the clause sets, which on a
        // streamed source is exactly one chunk.
        let head = match backing.window(0..limits::MAX_HEADER_SCAN as u64) {
            Ok(head) => head,
            Err(miss) => return Err(OpenError::SourceUnavailable(miss)),
        };
        let shift = xref::header_shift(&head, &mut sink);
        drop(head);

        // The walk reads windows: the tail for `startxref` (7.5.5), then each
        // section of the `/Prev` chain (7.5.4, 7.5.6, 7.5.8) as its own. A
        // document already in hand hands the walker the whole buffer for every
        // window, so it takes the path it always took at the cost it always
        // had.
        let held = if streamed {
            None
        } else {
            backing.materialise().ok()
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
            linearized_open(&backing, &names, &mut sink)
        } else {
            None
        };
        let built = match fast {
            Some((built, params)) => {
                linearized = Some(params);
                built
            }
            None => match xref::startxref(&view, &mut sink) {
                Some(start) => xref::build(view, start, shift, &names, &mut sink),
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
        match (&held, usable) {
            (Some(buffer), true) => {
                let validation = validate(buffer, &mut table, shift);
                failures = validation.failures;
                offset_entries = validation.offsets;
            }
            // Deferred on a streamed source, and only there. The eager pass
            // probes every type-1 offset against its `N G obj` header, which
            // means reading a byte near every object in the file -- the one
            // open-time step that touches everywhere, and the one thing a
            // streaming open cannot afford. Nothing is lost from safety:
            // `parse_at` re-checks the header of every object at load, so an
            // entry that lies is still caught, at first use rather than at
            // open. What is lost is that `ladder_level` starts out provisional,
            // which `CosDocument::complete_validation` is how a caller gets
            // back the eager answer.
            (None, _) => {}
            (Some(_), false) => {}
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
            let Some(buffer) = fetch_whole(&backing, &mut sink) else {
                return Err(OpenError::NoObjects);
            };
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
                    byte_range: 0..backing.len(),
                    // Nothing readable to chain to: the tables were discarded.
                    xref_at: 0,
                    trailer: trailer.clone(),
                });
            }
            scan = Some(Arc::new(index));
        } else if !failures.is_empty() {
            ladder = LadderLevel::Patch;
            let Some(buffer) = fetch_whole(&backing, &mut sink) else {
                return Err(OpenError::NoObjects);
            };
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
            buffer: backing,
            names,
            xref: table,
            trailer,
            revisions,
            linearized,
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
    /// that touches everywhere. This fetches the whole source, runs exactly
    /// that pass, and returns the answer a buffered open would have given.
    /// Both are documented observables rather than moods.
    ///
    /// Reads nothing and returns [`CosDocument::ladder_level`] for a document
    /// opened from a buffer, where the eager pass already ran.
    ///
    /// The values a caller reads do not depend on whether this was called: an
    /// entry that lies is repaired at first use either way. What this decides
    /// is the *verdict*, which a caller checking "did it open cleanly" needs
    /// and a caller rendering a page does not.
    pub fn complete_validation(&self) -> LadderLevel {
        if !self.buffer.is_streamed() {
            return self.ladder;
        }
        let mut sink = WarningSink::new();
        let Some(buffer) = fetch_whole(&self.buffer, &mut sink) else {
            self.absorb(sink);
            return self.ladder;
        };
        self.absorb(sink);
        // The header scan warned at open if it had anything to say; a second
        // sink keeps it from saying it twice.
        let mut scratch = WarningSink::new();
        let shift = xref::header_shift(&buffer, &mut scratch);
        let mut table = self.xref.clone();
        let validation = validate(&buffer, &mut table, shift);
        if validation.failures.is_empty() {
            return self.ladder;
        }
        if validation.failures.len() >= limits::LADDER_RESCAN_MIN_FAILURES
            && validation.failures.len() * 2 > validation.offsets
        {
            LadderLevel::Rescan
        } else {
            self.ladder.max(LadderLevel::Patch)
        }
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

    fn install_security(&self, decryptor: Arc<dyn Decryptor>, key: Option<FileKey>) {
        {
            let mut security = self.security.write_lock();
            security.decryptor = decryptor;
            security.has_decryptor = true;
            security.key = key;
        }
        // Everything already loaded was read as plaintext out of ciphertext.
        // Both caches are dropped so the next read goes back to the buffer;
        // outstanding `Arc`s keep the values they were given.
        self.store.clear();
        self.objstm.clear();
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
    pub fn bytes(&self) -> &[u8] {
        self.buffer.whole()
    }

    /// The highest object number the cross-reference table knows.
    ///
    /// New objects start above it, so an edit cannot collide with something
    /// the document already uses.
    pub fn max_object_number(&self) -> u32 {
        self.xref.max_number()
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
        self.linearized.as_ref()?;
        self.merged_xref().get(num)
    }

    /// The first-page table merged with the main one, fetched once.
    ///
    /// `/T` names the byte before the main table's first *entry* (F.2.2 item
    /// 5) rather than the `xref` keyword, so the keyword is looked for just
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
                let built = xref::build(self.buffer.view(), start, 0, &self.names, &mut sink);
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
                    let built = xref::build(self.buffer.view(), start, 0, &self.names, &mut sink);
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
        let missed_before = ctx.missed();
        let object = ctx.enter(num, |ctx| self.load_uncached(num, ctx, sink));
        if ctx.missed() && !missed_before {
            // A miss never publishes. This load reads as null because the
            // bytes were absent, not because the object is; publishing it
            // would give every later read the miss's answer forever, and the
            // host that goes and fetches the range would never see it change.
            return Arc::new(object);
        }
        self.store.publish(num, Arc::new(object))
    }

    fn load_uncached(&self, num: u32, ctx: &mut ResolveCtx, sink: &mut WarningSink) -> Object {
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
        if let Some(hit) = self.repair_index(sink).and_then(|scan| scan.get(num)) {
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

    /// The repair scanner's index, built on first need for a streamed
    /// document.
    ///
    /// A document opened from a buffer decided at open whether it needed one,
    /// because the eager offset probe ran then. A streamed document deferred
    /// that probe, so the same decision is made here, at the first read that
    /// finds an entry lying about its object -- which is exactly the condition
    /// the eager pass was looking for. The values a caller reads are therefore
    /// the same on both paths; what differs is when the whole source is
    /// fetched, and that is warned about rather than hidden (ruling 10).
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
        let index = Arc::new(ScanIndex::build(&buffer, &self.names));
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
            let mut parsed =
                parse_indirect_at(window.bytes(), local_at, &self.names.table, &mut local)?;
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
    /// `num`. The header check is what makes level 1 of the ladder safe.
    fn parse_at(
        &self,
        num: u32,
        offset: u64,
        ctx: &mut ResolveCtx,
        sink: &mut WarningSink,
    ) -> Option<Object> {
        let (parsed, warnings) = self.parse_windowed(offset, ctx)?;
        if parsed.reference.num != num {
            return None;
        }
        sink.extend(warnings);
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
    /// F.2.2 item 6, `/E`: the last byte of the first page's objects.
    ///
    /// Everything past it is the tail, and page one is rendered without
    /// touching a byte of it.
    end_of_first_page: u64,
    /// Item 5, `/T`: where the main cross-reference table begins. Fetched only
    /// when a read leaves page one.
    main_table_at: u64,
    /// Item 3, `/O`: the object number of the first page's page object.
    ///
    /// Annex F names it so that a reader holding only the head can reach page
    /// one **without the page tree**, whose root a linearized file is free to
    /// leave in the tail -- and which qpdf's linearizer does leave there, so
    /// this is not a nicety.
    first_page_object: u32,
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
    sink: &mut WarningSink,
) -> Option<(XrefBuild, Linearized)> {
    let view = backing.view();
    let len = backing.len();
    let mut want = limits::MAX_HEADER_SCAN as u64;
    loop {
        let head = view.window(0, want)?;
        let reaches_end = head.end() >= len;
        let mut scratch = WarningSink::new();

        // F.2.2: the parameter dictionary is the first object in the file, and
        // a file whose first object is anything else is simply not linearized.
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
        let (Some(end_of_first_page), Some(main_table_at), Some(first_page_object)) = (
            number(b"E"),
            number(b"T"),
            number(b"O").and_then(|v| u32::try_from(v).ok()),
        ) else {
            sink.warn(head.abs(at), WarningKind::LinearizedParametersUnusable);
            return None;
        };

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
