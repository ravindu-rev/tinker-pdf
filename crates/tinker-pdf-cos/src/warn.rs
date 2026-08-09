//! Typed, object-addressed warnings (ruling 10).
//!
//! Every leniency this crate performs emits one of these instead of a log
//! line, so "it opened" and "it opened cleanly" stay distinguishable and the
//! test suite can assert on the exact repair that happened. A file that lexes
//! and parses with an empty sink is a file that obeyed 7.2 and 7.3.

use core::fmt;

use crate::limits;
use crate::object::ObjRef;

/// One leniency action, addressed to the byte that caused it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Warning {
    /// Offset into the document buffer of the byte that triggered the repair.
    pub offset: u64,
    /// The indirect object being parsed when this happened, when known.
    pub object: Option<ObjRef>,
    /// What was repaired.
    pub kind: WarningKind,
}

impl fmt::Display for Warning {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.object {
            Some(r) => write!(f, "{}: {} {} R: {}", self.offset, r.num, r.gen, self.kind),
            None => write!(f, "{}: {}", self.offset, self.kind),
        }
    }
}

/// The closed set of repairs the lexer and object parser can perform.
///
/// Closed on purpose: a new leniency rule is a deliberate change to documented
/// behaviour, and callers that match exhaustively should be forced to notice.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum WarningKind {
    /// 7.3.4.2: a literal string ran to end of input; it was closed there.
    UnterminatedString,
    /// 7.3.4.3: a hex string ran to end of input; it was closed there.
    UnterminatedHexString,
    /// 7.3.4.3: a byte that is neither a hex digit nor whitespace was skipped.
    NonHexInHexString,
    /// 7.3.5: a `#` not followed by two hex digits was taken literally.
    BadNameEscape,
    /// 7.3.5: a name longer than [`limits::MAX_NAME_LEN`] was truncated.
    NameTruncated,
    /// 7.3.3: an integer outside `i64` was clamped to the nearest bound.
    IntOverflowClamped,
    /// 7.3.3: a real outside `f64`'s finite range was clamped to it.
    RealOverflowClamped,
    /// 7.3.3: more than one sign character; any `-` present makes it negative.
    DoubledSign,
    /// 7.3.3: exponent notation, which PDF does not permit, was evaluated.
    ExponentNumber,
    /// 7.3.3: a malformed numeric read as its longest valid prefix, or as 0.
    MalformedNumberPrefix,
    /// 7.3.8.1: `stream` was not followed by CRLF or LF.
    StreamKeywordBadEol,
    /// 7.3.8: a `stream` keyword followed something that was not a dictionary.
    StreamWithoutDict,
    /// 7.3.8.2: `/Length` was absent, negative, or of the wrong type. An
    /// indirect `/Length` is legal and common, and is not this.
    StreamLengthMissing,
    /// 7.3.6/7.3.7: nesting exceeded [`limits::MAX_NEST_DEPTH`]; the container
    /// was skipped and read as null.
    DepthCapHit,
    /// An array exceeded [`limits::MAX_ARRAY_LEN`]; further elements dropped.
    ArrayCapHit,
    /// A dictionary exceeded [`limits::MAX_DICT_ENTRIES`]; further entries
    /// dropped.
    DictCapHit,
    /// 7.3.6: an array ended at something other than `]`.
    UnterminatedArray,
    /// 7.3.7: a dictionary ended at something other than `>>`.
    UnterminatedDict,
    /// A closing delimiter did not match the container it appeared in.
    MismatchedClose,
    /// 7.3.7: a dictionary key position held something other than a name.
    DictKeyNotName,
    /// 7.3.7: a dictionary key had no value; it was stored as null.
    MissingDictValue,
    /// 7.3.7: a key appeared twice; the later value replaced the earlier one.
    DuplicateDictKey,
    /// A keyword appeared where an object was expected; read as null.
    UnexpectedKeyword,
    /// A delimiter or unrecognizable token appeared where an object was
    /// expected; read as null.
    UnexpectedToken,
    /// 7.3.10: an indirect object was not closed by `endobj`.
    MissingEndobj,
    /// [`limits::MAX_WARNINGS`] was reached; later warnings were counted but
    /// not retained. Emitted exactly once per sink.
    WarningCapReached,

    // ---- file structure (7.5) -------------------------------------------
    /// 7.5.2: `%PDF-` was not at byte 0. Every stored offset is short by the
    /// header's position, so that shift is tried first at every offset.
    HeaderNotAtStart,
    /// 7.5.2: no `%PDF-` header anywhere in the first
    /// [`limits::MAX_HEADER_SCAN`] bytes.
    HeaderMissing,
    /// 7.5.5: no `startxref` in the last [`limits::STARTXREF_SCAN_MAX`] bytes.
    StartxrefMissing,
    /// 7.5.5: `startxref` named an offset that is not a cross-reference
    /// section.
    StartxrefUnusable,
    /// 7.5.4/7.5.8: the section at this offset is neither a table nor a
    /// cross-reference stream; the chain stopped here.
    XrefUnreadable,
    /// 7.5.4: a subsection header was not `first count`.
    XrefSubsectionMalformed,
    /// 7.5.4: an entry did not read as `offset generation n|f`. Entries are
    /// resynchronized on the grammar, so 19- and 21-byte entries are fine and
    /// this means something worse.
    XrefEntryMalformed,
    /// 7.5.4: the table was not followed by `trailer`.
    XrefTrailerMissing,
    /// 7.5.8.2: `/W` was absent, not an array of non-negative integers, or
    /// described a zero-width entry.
    XrefStreamWidthsBad,
    /// 7.5.8.2: `/Index` was not an array of integer pairs; the default
    /// `[0 /Size]` was used.
    XrefStreamIndexBad,
    /// 7.5.8: the decoded stream ended mid-entry; the entries read so far were
    /// kept.
    XrefStreamTruncated,
    /// 7.5.8.3: an entry type other than 0, 1 or 2 was read as a reference to
    /// the null object.
    XrefStreamUnknownType,
    /// 7.5.6: a `/Prev` link pointed at a section already visited.
    XrefPrevCycle,
    /// 7.5.6: a `/Prev` value was not a usable offset.
    XrefPrevBad,
    /// [`limits::MAX_XREF_CHAIN`] `/Prev` links were followed; the rest of the
    /// chain was ignored.
    XrefChainCapHit,
    /// 7.5.8.4: the `/XRefStm` of a hybrid file was unreadable; the classic
    /// entries of that revision still apply.
    HybridXrefStmUnreadable,

    // ---- object loading --------------------------------------------------
    /// The bytes at a cross-reference offset are not this object's `N G obj`
    /// header.
    ObjectHeaderMismatch,
    /// An object was located through the repair scanner rather than the
    /// cross-reference table (ladder level 2).
    ObjectRepaired,
    /// An object could be found neither in the table nor by the scanner; it
    /// reads as null.
    ObjectMissing,
    /// An object was re-entered while it was still loading — a `/Length` into
    /// its own stream, a self-referential dictionary. It reads as null.
    ObjectCycle,
    /// [`limits::MAX_LOAD_DEPTH`] nested loads; this one reads as null.
    LoadDepthCapHit,
    /// [`limits::MAX_RESOLVE_DEPTH`] `Ref → Ref` hops; the chain reads as null.
    ResolveDepthCapHit,

    // ---- object streams (7.5.7) -----------------------------------------
    /// The container named by a type-2 entry is not a stream.
    ObjStmNotAStream,
    /// 7.5.7: `/N` or `/First` was absent or unusable; the pairs were
    /// recovered by lexing until they stopped making sense.
    ObjStmHeaderBad,
    /// 7.5.7: fewer pairs were readable than `/N` claimed; what parsed was
    /// kept.
    ObjStmPairsTruncated,
    /// 7.5.7: a pair's offset falls outside the decompressed stream.
    ObjStmEntryOutOfRange,
    /// 7.5.7: the pair at the index a type-2 entry named holds a different
    /// object number; the stream was searched by number instead.
    ObjStmIndexMismatch,
    /// 7.5.7: a contained object is itself a stream, which 7.5.7 forbids; the
    /// entry was skipped.
    ObjStmEntryIsStream,

    // ---- stream data (7.3.8.2) ------------------------------------------
    /// `/Length` did not end at `endstream`; the extent was recovered by
    /// scanning forward. Both lengths are carried so the damage is visible.
    StreamLengthRecovered {
        /// What `/Length` claimed, once resolved. `None` when it resolved to
        /// nothing usable.
        declared: Option<u64>,
        /// What the `endstream` keyword said.
        actual: u64,
    },
    /// No `endstream` follows the data at all; it was truncated at the next
    /// `N G obj` header or at end of buffer.
    StreamEndstreamMissing,
    /// An indirect `/Length` resolved to something that is not a non-negative
    /// integer.
    StreamLengthNotAnInteger,

    // ---- filters (7.4) ---------------------------------------------------
    /// `/Filter` named something this crate has no decoder for; the chain
    /// stops there and the bytes decoded so far come back.
    FilterUnknown,
    /// `/DecodeParms` could not describe any stream; the chain stops there.
    FilterParamsBad,
    /// An image codec terminated the chain: the bytes come back still encoded.
    ImageCodecNotDecoded,
    /// One leniency performed by [`tinker_pdf_filters`] while decoding.
    Filter(tinker_pdf_filters::Warning),

    // ---- the leniency ladder ---------------------------------------------
    /// No trailer dictionary was found; one was assembled from what the
    /// scanner recovered.
    TrailerSynthesized,
    /// No `/Root` anywhere; the `/Type /Catalog` object at the highest offset
    /// was taken as the catalog.
    RootSynthesized,
    /// Neither a `/Root` nor a catalog could be found. The document opens, but
    /// nothing above this layer can walk it.
    RootMissing,
    /// Ladder level 3: the cross-reference tables were discarded and the whole
    /// buffer was scanned for objects.
    DocumentRescanned,
}

impl WarningKind {
    /// A short stable identifier, suitable for a debug dump or a test name.
    pub fn as_str(self) -> &'static str {
        match self {
            WarningKind::UnterminatedString => "unterminated-string",
            WarningKind::UnterminatedHexString => "unterminated-hex-string",
            WarningKind::NonHexInHexString => "non-hex-in-hex-string",
            WarningKind::BadNameEscape => "bad-name-escape",
            WarningKind::NameTruncated => "name-truncated",
            WarningKind::IntOverflowClamped => "int-overflow-clamped",
            WarningKind::RealOverflowClamped => "real-overflow-clamped",
            WarningKind::DoubledSign => "doubled-sign",
            WarningKind::ExponentNumber => "exponent-number",
            WarningKind::MalformedNumberPrefix => "malformed-number-prefix",
            WarningKind::StreamKeywordBadEol => "stream-keyword-bad-eol",
            WarningKind::StreamWithoutDict => "stream-without-dict",
            WarningKind::StreamLengthMissing => "stream-length-missing",
            WarningKind::DepthCapHit => "depth-cap-hit",
            WarningKind::ArrayCapHit => "array-cap-hit",
            WarningKind::DictCapHit => "dict-cap-hit",
            WarningKind::UnterminatedArray => "unterminated-array",
            WarningKind::UnterminatedDict => "unterminated-dict",
            WarningKind::MismatchedClose => "mismatched-close",
            WarningKind::DictKeyNotName => "dict-key-not-name",
            WarningKind::MissingDictValue => "missing-dict-value",
            WarningKind::DuplicateDictKey => "duplicate-dict-key",
            WarningKind::UnexpectedKeyword => "unexpected-keyword",
            WarningKind::UnexpectedToken => "unexpected-token",
            WarningKind::MissingEndobj => "missing-endobj",
            WarningKind::WarningCapReached => "warning-cap-reached",
            WarningKind::HeaderNotAtStart => "header-not-at-start",
            WarningKind::HeaderMissing => "header-missing",
            WarningKind::StartxrefMissing => "startxref-missing",
            WarningKind::StartxrefUnusable => "startxref-unusable",
            WarningKind::XrefUnreadable => "xref-unreadable",
            WarningKind::XrefSubsectionMalformed => "xref-subsection-malformed",
            WarningKind::XrefEntryMalformed => "xref-entry-malformed",
            WarningKind::XrefTrailerMissing => "xref-trailer-missing",
            WarningKind::XrefStreamWidthsBad => "xref-stream-widths-bad",
            WarningKind::XrefStreamIndexBad => "xref-stream-index-bad",
            WarningKind::XrefStreamTruncated => "xref-stream-truncated",
            WarningKind::XrefStreamUnknownType => "xref-stream-unknown-type",
            WarningKind::XrefPrevCycle => "xref-prev-cycle",
            WarningKind::XrefPrevBad => "xref-prev-bad",
            WarningKind::XrefChainCapHit => "xref-chain-cap-hit",
            WarningKind::HybridXrefStmUnreadable => "hybrid-xrefstm-unreadable",
            WarningKind::ObjectHeaderMismatch => "object-header-mismatch",
            WarningKind::ObjectRepaired => "object-repaired",
            WarningKind::ObjectMissing => "object-missing",
            WarningKind::ObjectCycle => "object-cycle",
            WarningKind::LoadDepthCapHit => "load-depth-cap-hit",
            WarningKind::ResolveDepthCapHit => "resolve-depth-cap-hit",
            WarningKind::ObjStmNotAStream => "objstm-not-a-stream",
            WarningKind::ObjStmHeaderBad => "objstm-header-bad",
            WarningKind::ObjStmPairsTruncated => "objstm-pairs-truncated",
            WarningKind::ObjStmEntryOutOfRange => "objstm-entry-out-of-range",
            WarningKind::ObjStmIndexMismatch => "objstm-index-mismatch",
            WarningKind::ObjStmEntryIsStream => "objstm-entry-is-stream",
            WarningKind::StreamLengthRecovered { .. } => "stream-length-recovered",
            WarningKind::StreamEndstreamMissing => "stream-endstream-missing",
            WarningKind::StreamLengthNotAnInteger => "stream-length-not-an-integer",
            WarningKind::FilterUnknown => "filter-unknown",
            WarningKind::FilterParamsBad => "filter-params-bad",
            WarningKind::ImageCodecNotDecoded => "image-codec-not-decoded",
            WarningKind::Filter(_) => "filter",
            WarningKind::TrailerSynthesized => "trailer-synthesized",
            WarningKind::RootSynthesized => "root-synthesized",
            WarningKind::RootMissing => "root-missing",
            WarningKind::DocumentRescanned => "document-rescanned",
        }
    }
}

impl fmt::Display for WarningKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            WarningKind::StreamLengthRecovered {
                declared: Some(declared),
                actual,
            } => write!(
                f,
                "stream-length-recovered (declared {declared}, actual {actual})"
            ),
            WarningKind::StreamLengthRecovered {
                declared: None,
                actual,
            } => write!(f, "stream-length-recovered (undeclared, actual {actual})"),
            WarningKind::Filter(w) => write!(f, "filter: {w}"),
            other => f.write_str(other.as_str()),
        }
    }
}

/// A bounded collector of [`Warning`]s with an optional current object.
///
/// The sink retains at most [`limits::MAX_WARNINGS`] entries. When it fills,
/// the final retained entry is [`WarningKind::WarningCapReached`] and every
/// later warning is counted in [`WarningSink::dropped`] and discarded.
#[derive(Clone, Debug, Default)]
pub struct WarningSink {
    warnings: Vec<Warning>,
    context: Option<ObjRef>,
    dropped: u64,
    capped: bool,
}

impl WarningSink {
    /// An empty sink with no current object.
    pub fn new() -> WarningSink {
        WarningSink::default()
    }

    /// Records `kind` at `offset`, attributed to the current object.
    pub fn warn(&mut self, offset: u64, kind: WarningKind) {
        let object = self.context;
        self.push(Warning {
            offset,
            object,
            kind,
        });
    }

    /// Records `kind` at `offset`, attributed to `object` regardless of the
    /// current object.
    pub fn warn_at(&mut self, offset: u64, object: Option<ObjRef>, kind: WarningKind) {
        self.push(Warning {
            offset,
            object,
            kind,
        });
    }

    /// Records an already-built warning, applying the cap.
    pub fn push(&mut self, warning: Warning) {
        if self.warnings.len() + 1 < limits::MAX_WARNINGS {
            self.warnings.push(warning);
        } else if !self.capped {
            self.capped = true;
            self.dropped += 1;
            self.warnings.push(Warning {
                kind: WarningKind::WarningCapReached,
                ..warning
            });
        } else {
            self.dropped += 1;
        }
    }

    /// Appends warnings produced elsewhere, applying the cap to each.
    pub fn extend<I: IntoIterator<Item = Warning>>(&mut self, warnings: I) {
        for w in warnings {
            self.push(w);
        }
    }

    /// The object later warnings are attributed to.
    pub fn context(&self) -> Option<ObjRef> {
        self.context
    }

    /// Sets the object later warnings are attributed to, returning the
    /// previous one so callers can restore it.
    pub fn set_context(&mut self, object: Option<ObjRef>) -> Option<ObjRef> {
        core::mem::replace(&mut self.context, object)
    }

    /// The retained warnings, oldest first.
    pub fn warnings(&self) -> &[Warning] {
        &self.warnings
    }

    /// Removes and returns the retained warnings. The cap state is unaffected:
    /// the cap bounds total production, not what any one caller drained.
    pub fn take(&mut self) -> Vec<Warning> {
        core::mem::take(&mut self.warnings)
    }

    /// Number of retained warnings.
    pub fn len(&self) -> usize {
        self.warnings.len()
    }

    /// Whether nothing has been retained.
    pub fn is_empty(&self) -> bool {
        self.warnings.is_empty()
    }

    /// How many warnings were discarded because the cap had been reached.
    pub fn dropped(&self) -> u64 {
        self.dropped
    }

    /// Whether the cap has been reached.
    pub fn is_capped(&self) -> bool {
        self.capped
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn warning(kind: WarningKind) -> Warning {
        Warning {
            offset: 7,
            object: None,
            kind,
        }
    }

    #[test]
    fn sink_retains_in_order() {
        let mut sink = WarningSink::new();
        sink.warn(1, WarningKind::DoubledSign);
        sink.warn(2, WarningKind::ExponentNumber);
        let kinds: Vec<_> = sink.warnings().iter().map(|w| w.kind).collect();
        assert_eq!(
            kinds,
            [WarningKind::DoubledSign, WarningKind::ExponentNumber]
        );
        assert_eq!(sink.warnings()[0].offset, 1);
    }

    #[test]
    fn sink_attributes_to_context() {
        let mut sink = WarningSink::new();
        let r = ObjRef { num: 12, gen: 0 };
        assert_eq!(sink.set_context(Some(r)), None);
        sink.warn(0, WarningKind::MissingEndobj);
        sink.set_context(None);
        sink.warn(1, WarningKind::MissingEndobj);
        assert_eq!(sink.warnings()[0].object, Some(r));
        assert_eq!(sink.warnings()[1].object, None);
    }

    #[test]
    fn sink_caps_and_warns_once() {
        let mut sink = WarningSink::new();
        for _ in 0..limits::MAX_WARNINGS + 500 {
            sink.push(warning(WarningKind::DoubledSign));
        }
        assert_eq!(sink.len(), limits::MAX_WARNINGS);
        assert_eq!(sink.dropped(), 501);
        assert!(sink.is_capped());
        let last = sink.warnings().last().copied().unwrap();
        assert_eq!(last.kind, WarningKind::WarningCapReached);
        let cap_markers = sink
            .warnings()
            .iter()
            .filter(|w| w.kind == WarningKind::WarningCapReached)
            .count();
        assert_eq!(cap_markers, 1);
    }

    #[test]
    fn display_names_the_object() {
        let w = Warning {
            offset: 42,
            object: Some(ObjRef { num: 3, gen: 1 }),
            kind: WarningKind::UnterminatedString,
        };
        assert_eq!(w.to_string(), "42: 3 1 R: unterminated-string");
    }
}
