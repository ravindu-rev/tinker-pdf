//! The strict validator (ruling 13): a file re-read with the leniency ladder
//! **off**, plus the structures the tolerant reader never consults.
//!
//! [`crate::doc`]'s ladder exists so that a damaged document still opens. That
//! is the right posture for reading the world and the wrong one for judging
//! what this engine wrote: every default the reader supplies is a mistake the
//! writer is allowed to make. `read_shading` defaults a missing `/Extend`,
//! `parse_function` defaults a missing `/Domain`, the page walker invents US
//! Letter for a missing `/MediaBox` — a file built out of those omissions
//! round-trips through this crate perfectly and is not a PDF anybody else can
//! read.
//!
//! So nothing here reads through a typed reader, and nothing here reads the
//! *merged* cross-reference table either. [`CosDocument::xref`] is what the
//! ladder decided rather than what the file says: an entry whose generation is
//! wrong is normalised on the way in, and an offset pointing six bytes before
//! its object is accepted because the header lookup lexes forward. Both open
//! at [`LadderLevel::Trust`] with no warning. This module therefore walks the
//! cross-reference **sections** out of the bytes, one `/Prev` link at a time,
//! and holds every entry to the byte it names.
//!
//! # Two tiers, because a rewrite has two authors
//!
//! [`Tier::Structure`] is what the writer owns whatever it was handed: the
//! header, the sections, the offsets, the stream extents, the trailer. A
//! rewrite of *any* document — including one this project never authored —
//! must be clean here, which is what makes a corpus-wide pass mean something.
//!
//! [`Tier::Semantics`] is what the document says: page-tree back-links,
//! outline chains, resource dictionaries, font and shading structure. A defect
//! there in a rewrite of somebody else's file is usually that file's, so it is
//! reported and never ratcheted.
//!
//! # What this cannot do
//!
//! It cannot establish that anybody else accepts these files. It is this
//! project's own reader applied strictly, so a clause misread here and in the
//! writer agrees with itself. `docs/verification.md` states that in its own
//! voice; it is the property that left with the oracles.

mod hints;

use std::collections::{BTreeMap, BTreeSet};

use crate::doc::{CosDocument, LadderLevel};
use crate::limits;
use crate::name::Name;
use crate::object::{Dict, ObjRef, Object};
use crate::parse::{parse_indirect_at, parse_object_at};
use crate::warn::{WarningKind, WarningSink};

/// Which half of the verdict a rule belongs to.
///
/// The split is a claim about *whose* defect it is, not about severity.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Tier {
    /// File structure the writer owns whatever it was handed.
    Structure,
    /// Document semantics, which a rewrite inherits from its source.
    Semantics,
}

impl Tier {
    /// A short stable identifier for a report.
    pub fn as_str(self) -> &'static str {
        match self {
            Tier::Structure => "structure",
            Tier::Semantics => "semantics",
        }
    }
}

/// One finding, addressed to the object or the byte that caused it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Defect {
    /// The indirect object it was found in, when the rule has one.
    pub object: Option<ObjRef>,
    /// Offset into the document buffer, when the rule has one.
    pub offset: Option<u64>,
    /// What is wrong.
    pub kind: DefectKind,
}

impl core::fmt::Display for Defect {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match (self.object, self.offset) {
            (Some(r), Some(at)) => write!(f, "{at}: {} {} R: {}", r.num, r.gen, self.kind),
            (Some(r), None) => write!(f, "{} {} R: {}", r.num, r.gen, self.kind),
            (None, Some(at)) => write!(f, "{at}: {}", self.kind),
            (None, None) => write!(f, "{}", self.kind),
        }
    }
}

/// The closed set of rules this validator can fail.
///
/// Closed for the reason [`WarningKind`] is: a new rule is a deliberate change
/// to what "valid" means here, and every consumer that matches exhaustively —
/// the corpus report's per-kind counts among them — should be made to notice.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DefectKind {
    // ---- the ladder itself, which is the first rule -----------------------
    /// The document did not open on [`LadderLevel::Trust`]: an offset lied, or
    /// the tables were discarded entirely.
    OpenedLenient(LadderLevel),
    /// The reader performed a leniency while opening. One per distinct kind.
    Repaired(WarningKind),
    /// The reader performed a leniency while this validation read the file —
    /// a stream that only decodes with a repair, a CMap that needed one.
    /// Separate from [`DefectKind::Repaired`] because opening is lazy and
    /// these are the repairs no reader sees until it reads that far.
    RepairedWhileReading(WarningKind),

    // ---- the file frame (7.5.2, 7.5.5) ------------------------------------
    /// 7.5.2: no `%PDF-` at byte zero.
    HeaderMissing,
    /// 7.5.2: the header is there but its version is not `N.M`.
    HeaderVersionUnreadable,
    /// 7.5.2: no comment line of four bytes above 127 follows the header, so
    /// transfer software may treat the file as text.
    BinaryCommentMissing,
    /// 7.5.5: the file does not end with `%%EOF`.
    EofMissing,
    /// 7.5.5: bytes other than end-of-line follow the final `%%EOF`.
    BytesAfterEof {
        /// How many.
        count: u64,
    },
    /// 7.5.5: no `startxref` near the end of the file.
    StartxrefMissing,
    /// 7.5.5: `startxref` names an offset that is neither a cross-reference
    /// table nor a cross-reference stream.
    StartxrefNotASection,

    // ---- the cross-reference sections (7.5.4, 7.5.6, 7.5.8) ---------------
    /// 7.5.4/7.5.8: a `/Prev` or `/XRefStm` link names no section.
    SectionUnreadable,
    /// 7.5.6: a `/Prev` chain revisits a section it has already read.
    PrevCycle,
    /// 7.5.4: a subsection header is not `first count`.
    SubsectionMalformed,
    /// 7.5.4: an entry is not the twenty bytes the clause requires — ten
    /// digits, a space, five digits, a space, `n` or `f`, and a two-byte
    /// end-of-line.
    EntryNotTwentyBytes,
    /// 7.5.4: a classic table is not followed by `trailer`.
    TableTrailerMissing,
    /// 7.5.8.2: `/W` is absent or is not three or more non-negative integers.
    XrefStreamWidthsBad,
    /// 7.5.8.2: `/Index` is present and is not an even-length array of
    /// non-negative integers.
    XrefStreamIndexBad,
    /// 7.5.8: the decoded stream is not exactly the declared number of rows.
    XrefStreamRowsWrong {
        /// Rows `/Index` (or `/Size`) accounts for.
        declared: u64,
        /// Rows the data holds.
        actual: u64,
    },
    /// 7.5.8.3: an entry type other than 0, 1 or 2.
    XrefStreamTypeUnknown {
        /// The type field's value.
        kind: u64,
    },

    // ---- the trailer (7.5.5 Table 15) -------------------------------------
    /// Table 15: no `/Root`.
    TrailerRootMissing,
    /// Table 15: no `/Size`.
    TrailerSizeMissing,
    /// 7.7.2: `/Root` does not resolve to a `/Type /Catalog` dictionary.
    RootNotACatalog,
    /// Table 15: `/Encrypt` is present and `/ID` is not. The one defect an
    /// outside reader found in this engine's output and no test here did.
    EncryptWithoutId,
    /// Table 15: `/ID` is present but is not two non-empty strings.
    IdMalformed,
    /// Table 15: `/Size` is not greater than the highest object number.
    SizeTooSmall {
        /// What the trailer claims.
        declared: i64,
        /// The highest number any cross-reference entry defines.
        highest: u32,
    },

    // ---- the entries themselves (7.5.4) -----------------------------------
    /// 7.5.4: object zero is not present as the head of the free list.
    FreeHeadMissing,
    /// 7.5.4: object zero's generation is not 65535.
    FreeHeadGeneration {
        /// What the entry says.
        gen: u16,
    },
    /// 7.5.4: a free entry's `next` names an object that is in use.
    FreeNextNotFree {
        /// The object number it names.
        next: u32,
    },
    /// 7.5.5: an entry defines an object number at or above `/Size`, which a
    /// conforming reader must then ignore.
    EntryPastSize {
        /// What the trailer claims.
        declared: i64,
    },
    /// 7.5.4: the bytes at the entry's offset are not an `N G obj` header.
    ObjectHeaderAbsent,
    /// 7.5.4: the header at the entry's offset spells a different object.
    ObjectHeaderMismatch {
        /// What the header spells.
        found: ObjRef,
    },
    /// 7.3: re-parsing the object at its own offset needed a repair.
    ObjectRepaired(WarningKind),

    // ---- object streams (7.5.7) -------------------------------------------
    /// A type-2 entry names a container that is not a `/Type /ObjStm` stream.
    ObjStmNotAStream {
        /// The container it names.
        container: u32,
    },
    /// 7.5.7: the container has no direct `/N` and `/First`.
    ObjStmHeaderMissing {
        /// The container.
        container: u32,
    },
    /// 7.5.7: the entry's index is at or above the container's `/N`.
    ObjStmIndexOutOfRange {
        /// The container.
        container: u32,
        /// The index the entry names.
        index: u32,
    },
    /// 7.5.7: the pair at that index names a different object number.
    ObjStmNumberMismatch {
        /// The container.
        container: u32,
        /// The number the pair carries.
        found: u32,
    },
    /// 7.5.7: a pair's offset falls outside the decompressed stream.
    ObjStmOffsetOutOfRange {
        /// The container.
        container: u32,
    },

    // ---- the page tree (7.7.3) --------------------------------------------
    /// 7.7.3.2: a node reached from `/Kids` is neither `/Page` nor `/Pages`.
    PageNodeUntyped,
    /// 7.7.3.2: a node's `/Parent` does not name the node that reached it.
    /// The reader walks downwards only, so nothing else here ever looks.
    PageParentWrong {
        /// What the node names, when it names anything.
        found: Option<ObjRef>,
    },
    /// 7.7.3.2: `/Count` is not the number of leaves below the node.
    PageCountWrong {
        /// What the node claims.
        declared: i64,
        /// What its subtree holds.
        actual: u64,
    },
    /// 7.7.3.2: `/Kids` is absent, or is not an array of references.
    KidsMalformed,
    /// 7.7.3.2: the tree reaches a node it has already visited.
    PageTreeCycle,
    /// 7.7.3.3: no `/MediaBox` on the page or any ancestor. The reader assumes
    /// US Letter and warns; nobody else has to.
    MediaBoxAbsent,
    /// 7.7.3.3: `/MediaBox` is not four numbers, or encloses no area.
    MediaBoxDegenerate,

    // ---- the outline (12.3.3) ---------------------------------------------
    /// 12.3.3: an item's `/Parent` does not name the node that reached it.
    OutlineParentWrong,
    /// 12.3.3: the sibling chain runs both ways, and this item's `/Prev` does
    /// not name the item before it. A reader that walks forward only — which
    /// this one does — cannot tell.
    OutlinePrevWrong,
    /// 12.3.3: an item's `/Next` does not name the item after it.
    OutlineNextWrong,
    /// 12.3.3: `/First` or `/Last` does not name the chain's own ends.
    OutlineEndsWrong,
    /// 12.3.3 Table 152: `/Count` is not the number of items the node exposes,
    /// negated when the node is closed.
    OutlineCountWrong {
        /// What the node claims.
        declared: i64,
        /// What it exposes.
        actual: i64,
    },
    /// 12.3.3: an item has no `/Title`.
    OutlineTitleMissing,

    // ---- the resource graph (8.4, 8.7, 9.7) -------------------------------
    //
    // Every rule below names the entry at fault rather than getting a variant
    // of its own: they are one family — a dictionary this writer emits that
    // somebody else has to read — and a report wants to count the family and
    // read the entry.
    /// 7.8.3: a name in a resource dictionary resolves to nothing.
    ResourceUnresolved,
    /// 11.6.4.4/11.3.5: a graphics state parameter dictionary entry is not
    /// what its table says.
    ExtGStateMalformed {
        /// The entry at fault.
        entry: &'static str,
    },
    /// 11.6.6: a transparency group's own dictionary.
    GroupMalformed {
        /// The entry at fault.
        entry: &'static str,
    },
    /// 8.7.4.5: a shading dictionary.
    ShadingMalformed {
        /// The entry at fault.
        entry: &'static str,
    },
    /// 7.10: a function dictionary.
    FunctionMalformed {
        /// The entry at fault.
        entry: &'static str,
    },
    /// 8.7.3: a pattern dictionary.
    PatternMalformed {
        /// The entry at fault.
        entry: &'static str,
    },
    /// 9.5–9.7: a font dictionary.
    FontMalformed {
        /// The entry at fault.
        entry: &'static str,
    },
    /// 8.9/8.10: an image or form XObject.
    XObjectMalformed {
        /// The entry at fault.
        entry: &'static str,
    },

    // ---- annotations (12.5) -----------------------------------------------
    /// 12.5.2 Table 164: an annotation with no `/Subtype`.
    AnnotSubtypeMissing,
    /// 12.5.2: `/Rect` is not four numbers.
    AnnotRectMalformed,
    /// 12.5.2: `/Rect`'s corners are not in increasing order, so a viewer that
    /// takes them as given draws an inverted or empty hot spot.
    AnnotRectUnordered,
    /// 12.5.6.5: a link annotation names neither `/Dest` nor `/A`.
    LinkWithoutTarget,

    // ---- linearization (Annex F) ------------------------------------------
    /// F.2.2: a parameter dictionary that is not what it declares — `/L`
    /// against the file's own length, `/N` against the pages, `/O` against the
    /// first page's object, `/T` against the main table's first entry.
    LinearizedParameterWrong {
        /// The entry at fault.
        entry: &'static str,
    },
    /// F.3/F.4: the primary hint stream cannot be read at all — no stream at
    /// `/H`, no `/S`, or a table that runs out mid-field.
    HintStreamUnreadable,
    /// F.3/F.4: a hint table states a number the file's own object extents
    /// contradict. This is the one the writer got wrong in five separate ways
    /// while every test passed, because nothing had ever read it back.
    HintValueWrong {
        /// What the number describes.
        entry: &'static str,
    },

    // ---- stream extents and filters (7.3.8, 7.4) --------------------------
    /// 7.3.8.2: `/Length` is absent, or indirect and unresolvable.
    StreamLengthUnresolved,
    /// 7.3.8.1: the declared length does not reach `endstream`.
    StreamLengthNotExact {
        /// What `/Length` claims.
        declared: u64,
        /// Where `endstream` actually is, measured from the first data byte,
        /// or `None` when there is no `endstream` at all.
        actual: Option<u64>,
    },
    /// 7.4: the filter chain refused the data.
    StreamDoesNotDecode,
}

impl DefectKind {
    /// Which half of the verdict this rule belongs to.
    pub fn tier(self) -> Tier {
        match self {
            // Every rule so far is a property of the bytes the writer laid
            // down, so a rewrite of any document at all must be clean here.
            // The semantic tier arrives with the rules that read the
            // document's own dictionaries.
            // A repair is tiered by what it repaired: a rewrite re-serialises
            // every object and owns the result, but it copies stream *bytes*
            // and page dictionaries from its source, so a sloppy filter tail
            // or a page tree that loops is the source document's.
            DefectKind::Repaired(kind)
            | DefectKind::RepairedWhileReading(kind)
            | DefectKind::ObjectRepaired(kind) => repair_tier(kind),

            DefectKind::OpenedLenient(_)
            | DefectKind::HeaderMissing
            | DefectKind::HeaderVersionUnreadable
            | DefectKind::BinaryCommentMissing
            | DefectKind::EofMissing
            | DefectKind::BytesAfterEof { .. }
            | DefectKind::StartxrefMissing
            | DefectKind::StartxrefNotASection
            | DefectKind::SectionUnreadable
            | DefectKind::PrevCycle
            | DefectKind::SubsectionMalformed
            | DefectKind::EntryNotTwentyBytes
            | DefectKind::TableTrailerMissing
            | DefectKind::XrefStreamWidthsBad
            | DefectKind::XrefStreamIndexBad
            | DefectKind::XrefStreamRowsWrong { .. }
            | DefectKind::XrefStreamTypeUnknown { .. }
            | DefectKind::TrailerRootMissing
            | DefectKind::TrailerSizeMissing
            | DefectKind::RootNotACatalog
            | DefectKind::EncryptWithoutId
            | DefectKind::IdMalformed
            | DefectKind::SizeTooSmall { .. }
            | DefectKind::FreeHeadMissing
            | DefectKind::FreeHeadGeneration { .. }
            | DefectKind::FreeNextNotFree { .. }
            | DefectKind::EntryPastSize { .. }
            | DefectKind::ObjectHeaderAbsent
            | DefectKind::ObjectHeaderMismatch { .. }
            | DefectKind::ObjStmNotAStream { .. }
            | DefectKind::ObjStmHeaderMissing { .. }
            | DefectKind::ObjStmIndexOutOfRange { .. }
            | DefectKind::ObjStmNumberMismatch { .. }
            | DefectKind::ObjStmOffsetOutOfRange { .. }
            | DefectKind::StreamLengthUnresolved
            | DefectKind::StreamLengthNotExact { .. }
            | DefectKind::StreamDoesNotDecode
            | DefectKind::LinearizedParameterWrong { .. }
            | DefectKind::HintStreamUnreadable
            | DefectKind::HintValueWrong { .. } => Tier::Structure,

            // What the document says rather than how the file is laid out. A
            // rewrite inherits every one of these from its source, which is
            // why they are reported and never ratcheted.
            DefectKind::PageNodeUntyped
            | DefectKind::PageParentWrong { .. }
            | DefectKind::PageCountWrong { .. }
            | DefectKind::KidsMalformed
            | DefectKind::PageTreeCycle
            | DefectKind::MediaBoxAbsent
            | DefectKind::MediaBoxDegenerate
            | DefectKind::OutlineParentWrong
            | DefectKind::OutlinePrevWrong
            | DefectKind::OutlineNextWrong
            | DefectKind::OutlineEndsWrong
            | DefectKind::OutlineCountWrong { .. }
            | DefectKind::OutlineTitleMissing
            | DefectKind::AnnotSubtypeMissing
            | DefectKind::AnnotRectMalformed
            | DefectKind::AnnotRectUnordered
            | DefectKind::LinkWithoutTarget
            | DefectKind::ResourceUnresolved
            | DefectKind::ExtGStateMalformed { .. }
            | DefectKind::GroupMalformed { .. }
            | DefectKind::ShadingMalformed { .. }
            | DefectKind::FunctionMalformed { .. }
            | DefectKind::PatternMalformed { .. }
            | DefectKind::FontMalformed { .. }
            | DefectKind::XObjectMalformed { .. } => Tier::Semantics,
        }
    }

    /// A short stable identifier, for a report's per-kind counts.
    pub fn as_str(self) -> &'static str {
        match self {
            DefectKind::OpenedLenient(_) => "opened-lenient",
            DefectKind::Repaired(_) => "repaired",
            DefectKind::RepairedWhileReading(_) => "repaired-while-reading",
            DefectKind::HeaderMissing => "header-missing",
            DefectKind::HeaderVersionUnreadable => "header-version-unreadable",
            DefectKind::BinaryCommentMissing => "binary-comment-missing",
            DefectKind::EofMissing => "eof-missing",
            DefectKind::BytesAfterEof { .. } => "bytes-after-eof",
            DefectKind::StartxrefMissing => "startxref-missing",
            DefectKind::StartxrefNotASection => "startxref-not-a-section",
            DefectKind::SectionUnreadable => "section-unreadable",
            DefectKind::PrevCycle => "prev-cycle",
            DefectKind::SubsectionMalformed => "subsection-malformed",
            DefectKind::EntryNotTwentyBytes => "entry-not-twenty-bytes",
            DefectKind::TableTrailerMissing => "table-trailer-missing",
            DefectKind::XrefStreamWidthsBad => "xref-stream-widths-bad",
            DefectKind::XrefStreamIndexBad => "xref-stream-index-bad",
            DefectKind::XrefStreamRowsWrong { .. } => "xref-stream-rows-wrong",
            DefectKind::XrefStreamTypeUnknown { .. } => "xref-stream-type-unknown",
            DefectKind::TrailerRootMissing => "trailer-root-missing",
            DefectKind::TrailerSizeMissing => "trailer-size-missing",
            DefectKind::RootNotACatalog => "root-not-a-catalog",
            DefectKind::EncryptWithoutId => "encrypt-without-id",
            DefectKind::IdMalformed => "id-malformed",
            DefectKind::SizeTooSmall { .. } => "size-too-small",
            DefectKind::FreeHeadMissing => "free-head-missing",
            DefectKind::FreeHeadGeneration { .. } => "free-head-generation",
            DefectKind::FreeNextNotFree { .. } => "free-next-not-free",
            DefectKind::EntryPastSize { .. } => "entry-past-size",
            DefectKind::ObjectHeaderAbsent => "object-header-absent",
            DefectKind::ObjectHeaderMismatch { .. } => "object-header-mismatch",
            DefectKind::ObjectRepaired(_) => "object-repaired",
            DefectKind::ObjStmNotAStream { .. } => "objstm-not-a-stream",
            DefectKind::ObjStmHeaderMissing { .. } => "objstm-header-missing",
            DefectKind::ObjStmIndexOutOfRange { .. } => "objstm-index-out-of-range",
            DefectKind::ObjStmNumberMismatch { .. } => "objstm-number-mismatch",
            DefectKind::ObjStmOffsetOutOfRange { .. } => "objstm-offset-out-of-range",
            DefectKind::StreamLengthUnresolved => "stream-length-unresolved",
            DefectKind::StreamLengthNotExact { .. } => "stream-length-not-exact",
            DefectKind::StreamDoesNotDecode => "stream-does-not-decode",
            DefectKind::LinearizedParameterWrong { .. } => "linearized-parameter-wrong",
            DefectKind::HintStreamUnreadable => "hint-stream-unreadable",
            DefectKind::HintValueWrong { .. } => "hint-value-wrong",
            DefectKind::PageNodeUntyped => "page-node-untyped",
            DefectKind::PageParentWrong { .. } => "page-parent-wrong",
            DefectKind::PageCountWrong { .. } => "page-count-wrong",
            DefectKind::KidsMalformed => "kids-malformed",
            DefectKind::PageTreeCycle => "page-tree-cycle",
            DefectKind::MediaBoxAbsent => "media-box-absent",
            DefectKind::MediaBoxDegenerate => "media-box-degenerate",
            DefectKind::OutlineParentWrong => "outline-parent-wrong",
            DefectKind::OutlinePrevWrong => "outline-prev-wrong",
            DefectKind::OutlineNextWrong => "outline-next-wrong",
            DefectKind::OutlineEndsWrong => "outline-ends-wrong",
            DefectKind::OutlineCountWrong { .. } => "outline-count-wrong",
            DefectKind::OutlineTitleMissing => "outline-title-missing",
            DefectKind::AnnotSubtypeMissing => "annot-subtype-missing",
            DefectKind::AnnotRectMalformed => "annot-rect-malformed",
            DefectKind::AnnotRectUnordered => "annot-rect-unordered",
            DefectKind::LinkWithoutTarget => "link-without-target",
            DefectKind::ResourceUnresolved => "resource-unresolved",
            DefectKind::ExtGStateMalformed { .. } => "ext-gstate-malformed",
            DefectKind::GroupMalformed { .. } => "group-malformed",
            DefectKind::ShadingMalformed { .. } => "shading-malformed",
            DefectKind::FunctionMalformed { .. } => "function-malformed",
            DefectKind::PatternMalformed { .. } => "pattern-malformed",
            DefectKind::FontMalformed { .. } => "font-malformed",
            DefectKind::XObjectMalformed { .. } => "xobject-malformed",
        }
    }
}

impl core::fmt::Display for DefectKind {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            DefectKind::OpenedLenient(level) => {
                write!(f, "the document opened at {level:?} rather than Trust")
            }
            DefectKind::Repaired(kind) => write!(f, "the reader repaired: {kind}"),
            DefectKind::RepairedWhileReading(kind) => {
                write!(f, "reading the file needed a repair: {kind}")
            }
            DefectKind::HeaderMissing => f.write_str("no %PDF- header at byte zero (7.5.2)"),
            DefectKind::HeaderVersionUnreadable => f.write_str("the header states no version"),
            DefectKind::BinaryCommentMissing => {
                f.write_str("no binary comment follows the header (7.5.2)")
            }
            DefectKind::EofMissing => f.write_str("the file does not end with %%EOF (7.5.5)"),
            DefectKind::BytesAfterEof { count } => {
                write!(f, "{count} bytes follow the final %%EOF (7.5.5)")
            }
            DefectKind::StartxrefMissing => f.write_str("no startxref near the end (7.5.5)"),
            DefectKind::StartxrefNotASection => {
                f.write_str("startxref names no cross-reference section (7.5.5)")
            }
            DefectKind::SectionUnreadable => {
                f.write_str("no cross-reference section at that offset (7.5.4)")
            }
            DefectKind::PrevCycle => f.write_str("the /Prev chain returns to a section (7.5.6)"),
            DefectKind::SubsectionMalformed => {
                f.write_str("a subsection header is not `first count` (7.5.4)")
            }
            DefectKind::EntryNotTwentyBytes => {
                f.write_str("an entry is not the twenty bytes 7.5.4 requires")
            }
            DefectKind::TableTrailerMissing => {
                f.write_str("the table is not followed by `trailer` (7.5.4)")
            }
            DefectKind::XrefStreamWidthsBad => {
                f.write_str("/W is not three non-negative integers (7.5.8.2)")
            }
            DefectKind::XrefStreamIndexBad => {
                f.write_str("/Index is not pairs of non-negative integers (7.5.8.2)")
            }
            DefectKind::XrefStreamRowsWrong { declared, actual } => write!(
                f,
                "the stream holds {actual} rows where /Index accounts for {declared} (7.5.8)"
            ),
            DefectKind::XrefStreamTypeUnknown { kind } => {
                write!(f, "entry type {kind} is not 0, 1 or 2 (7.5.8.3)")
            }
            DefectKind::TrailerRootMissing => f.write_str("the trailer has no /Root (Table 15)"),
            DefectKind::TrailerSizeMissing => f.write_str("the trailer has no /Size (Table 15)"),
            DefectKind::RootNotACatalog => f.write_str("/Root is not a /Type /Catalog (7.7.2)"),
            DefectKind::EncryptWithoutId => {
                f.write_str("/Encrypt is present and /ID is not (Table 15)")
            }
            DefectKind::IdMalformed => f.write_str("/ID is not two non-empty strings (Table 15)"),
            DefectKind::SizeTooSmall { declared, highest } => write!(
                f,
                "/Size {declared} does not cover object {highest} (Table 15)"
            ),
            DefectKind::FreeHeadMissing => f.write_str("object zero heads no free list (7.5.4)"),
            DefectKind::FreeHeadGeneration { gen } => {
                write!(f, "object zero's generation is {gen}, not 65535 (7.5.4)")
            }
            DefectKind::FreeNextNotFree { next } => {
                write!(f, "the free list points at {next}, which is in use (7.5.4)")
            }
            DefectKind::EntryPastSize { declared } => {
                write!(f, "this object is at or above /Size {declared} (Table 15)")
            }
            DefectKind::ObjectHeaderAbsent => {
                f.write_str("no N G obj header at the offset the table names (7.5.4)")
            }
            DefectKind::ObjectHeaderMismatch { found } => {
                write!(f, "the header at that offset spells {found} (7.5.4)")
            }
            DefectKind::ObjectRepaired(kind) => {
                write!(f, "re-parsing the object needed a repair: {kind}")
            }
            DefectKind::ObjStmNotAStream { container } => {
                write!(
                    f,
                    "object stream {container} is not a /Type /ObjStm (7.5.7)"
                )
            }
            DefectKind::ObjStmHeaderMissing { container } => {
                write!(f, "object stream {container} has no /N and /First (7.5.7)")
            }
            DefectKind::ObjStmIndexOutOfRange { container, index } => write!(
                f,
                "index {index} is past the end of object stream {container} (7.5.7)"
            ),
            DefectKind::ObjStmNumberMismatch { container, found } => write!(
                f,
                "object stream {container} carries {found} at that index (7.5.7)"
            ),
            DefectKind::ObjStmOffsetOutOfRange { container } => write!(
                f,
                "a pair of object stream {container} points outside it (7.5.7)"
            ),
            DefectKind::StreamLengthUnresolved => {
                f.write_str("/Length is absent or does not resolve (7.3.8.2)")
            }
            DefectKind::StreamLengthNotExact { declared, actual } => match actual {
                Some(actual) => write!(
                    f,
                    "/Length {declared} does not reach endstream, which is at {actual} (7.3.8.1)"
                ),
                None => write!(f, "/Length {declared} and no endstream at all (7.3.8.1)"),
            },
            DefectKind::StreamDoesNotDecode => f.write_str("the filter chain refused it (7.4)"),
            DefectKind::LinearizedParameterWrong { entry } => {
                write!(f, "the linearization dictionary's {entry} (F.2.2)")
            }
            DefectKind::HintStreamUnreadable => {
                f.write_str("the primary hint stream cannot be read (F.3)")
            }
            DefectKind::HintValueWrong { entry } => {
                write!(
                    f,
                    "the hint tables' {entry} is not what the file holds (F.3)"
                )
            }
            DefectKind::PageNodeUntyped => {
                f.write_str("a node in the page tree is neither /Page nor /Pages (7.7.3.2)")
            }
            DefectKind::PageParentWrong { found } => match found {
                Some(found) => write!(f, "/Parent names {found}, not the node above (7.7.3.2)"),
                None => f.write_str("no /Parent at all (7.7.3.2)"),
            },
            DefectKind::PageCountWrong { declared, actual } => write!(
                f,
                "/Count {declared} against {actual} leaves below it (7.7.3.2)"
            ),
            DefectKind::KidsMalformed => {
                f.write_str("/Kids is not an array of references (7.7.3.2)")
            }
            DefectKind::PageTreeCycle => f.write_str("the page tree loops (7.7.3.2)"),
            DefectKind::MediaBoxAbsent => {
                f.write_str("no /MediaBox on the page or any ancestor (7.7.3.3)")
            }
            DefectKind::MediaBoxDegenerate => {
                f.write_str("/MediaBox is not four numbers enclosing an area (7.7.3.3)")
            }
            DefectKind::OutlineParentWrong => {
                f.write_str("/Parent does not name the node above (12.3.3)")
            }
            DefectKind::OutlinePrevWrong => {
                f.write_str("/Prev does not name the item before (12.3.3)")
            }
            DefectKind::OutlineNextWrong => {
                f.write_str("/Next does not name the item after (12.3.3)")
            }
            DefectKind::OutlineEndsWrong => {
                f.write_str("/First and /Last do not name the chain's ends (12.3.3)")
            }
            DefectKind::OutlineCountWrong { declared, actual } => write!(
                f,
                "/Count {declared} where the node exposes {actual} (Table 152)"
            ),
            DefectKind::OutlineTitleMissing => {
                f.write_str("an outline item has no /Title (12.3.3)")
            }
            DefectKind::AnnotSubtypeMissing => {
                f.write_str("an annotation has no /Subtype (Table 164)")
            }
            DefectKind::AnnotRectMalformed => f.write_str("/Rect is not four numbers (12.5.2)"),
            DefectKind::AnnotRectUnordered => {
                f.write_str("/Rect's corners are not in increasing order (12.5.2)")
            }
            DefectKind::LinkWithoutTarget => {
                f.write_str("a link names neither /Dest nor /A (12.5.6.5)")
            }
            DefectKind::ResourceUnresolved => {
                f.write_str("a name in a resource dictionary resolves to nothing (7.8.3)")
            }
            DefectKind::ExtGStateMalformed { entry } => {
                write!(f, "the graphics state's {entry} (11.6.4.4)")
            }
            DefectKind::GroupMalformed { entry } => {
                write!(f, "the transparency group's {entry} (11.6.6)")
            }
            DefectKind::ShadingMalformed { entry } => {
                write!(f, "the shading's {entry} (8.7.4.5)")
            }
            DefectKind::FunctionMalformed { entry } => write!(f, "the function's {entry} (7.10)"),
            DefectKind::PatternMalformed { entry } => write!(f, "the pattern's {entry} (8.7.3)"),
            DefectKind::FontMalformed { entry } => write!(f, "the font's {entry} (9.7)"),
            DefectKind::XObjectMalformed { entry } => write!(f, "the XObject's {entry} (8.8)"),
        }
    }
}

/// How many defects of each tier a verdict holds.
#[must_use]
pub fn tier_counts(defects: &[Defect]) -> BTreeMap<Tier, usize> {
    let mut out = BTreeMap::new();
    out.insert(Tier::Structure, 0);
    out.insert(Tier::Semantics, 0);
    for defect in defects {
        *out.entry(defect.kind.tier()).or_default() += 1;
    }
    out
}

/// How many defects of each kind a verdict holds, by label.
#[must_use]
pub fn kind_counts(defects: &[Defect]) -> BTreeMap<&'static str, usize> {
    let mut out = BTreeMap::new();
    for defect in defects {
        *out.entry(defect.kind.as_str()).or_default() += 1;
    }
    out
}

/// Validates an open document.
///
/// An encrypted document should already be authenticated: a stream this
/// validator cannot decrypt reads as one that does not decode, which is the
/// honest answer to being handed a locked file.
#[must_use]
pub fn validate(doc: &CosDocument) -> Vec<Defect> {
    let mut v = Validator {
        doc,
        buf: doc.bytes(),
        out: Vec::new(),
        seen_repairs: BTreeSet::new(),
        visited: BTreeSet::new(),
        pages: Vec::new(),
    };
    let before = doc.warnings().len();
    v.ladder();
    v.frame();
    let sections = v.sections();
    v.trailer(&sections);
    v.entries(&sections);
    v.semantics();
    v.linearization(&sections);
    v.repairs_raised_while_reading(before);
    v.out
}

/// One entry exactly as a section spells it, before any merge or repair.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum RawEntry {
    Free { next: u32, gen: u16 },
    Offset { offset: u64, gen: u16 },
    InStream { container: u32, index: u32 },
}

/// One cross-reference section, read from the bytes rather than from the
/// document's merged view of them.
struct Section {
    /// Where its `xref` keyword or its stream object begins. Annex F's `/T`
    /// names a byte inside the main one, so the offset has to survive the read.
    at: u64,
    entries: Vec<(u32, RawEntry)>,
    trailer: Dict,
}

/// Which tier a repair belongs to, by what was repaired.
///
/// The file's syntax and its tables are the writer's whatever it was handed:
/// a rewrite re-serialises every object, so a lexical repair in the *output*
/// is this engine's own. Stream content and font programs are copied through,
/// and the page and outline trees are the source's dictionaries — a rewrite of
/// a document whose page tree loops produces a rewrite whose page tree loops.
fn repair_tier(kind: WarningKind) -> Tier {
    match kind {
        WarningKind::PageTreeCycle
        | WarningKind::PageTreeTruncated
        | WarningKind::MediaBoxMissing
        | WarningKind::PageCountMismatch
        | WarningKind::TreeCycle
        | WarningKind::TreeTruncated
        | WarningKind::TreeOddEntries
        | WarningKind::OutlineCycle
        | WarningKind::OutlineTruncated
        | WarningKind::FilterUnknown
        | WarningKind::FilterParamsBad
        | WarningKind::ImageCodecNotDecoded
        | WarningKind::Filter(_)
        | WarningKind::CMap(_)
        | WarningKind::PredefinedCMapUnknown(_)
        | WarningKind::PredefinedCMapApproximate(_) => Tier::Semantics,
        _ => Tier::Structure,
    }
}

/// Whether a leniency the reader performed is a defect in the *file*.
///
/// Two are not, and both say something about this build rather than about the
/// document:
///
/// - [`WarningKind::ImageCodecNotDecoded`] is 7.4.9 working. A JPEG stays a
///   JPEG until something wants pixels, and asking a DCT stream for bytes is
///   how the validator checks that the chain resolves at all.
/// - [`WarningKind::PredefinedCMapApproximate`] means the registry defines the
///   CMap and this build did not compile its table in. The file is right; the
///   binary is short. [`WarningKind::PredefinedCMapUnknown`] — no such CMap —
///   stays a defect, because that one is the document's.
fn is_a_defect(kind: WarningKind) -> bool {
    !matches!(
        kind,
        WarningKind::ImageCodecNotDecoded | WarningKind::PredefinedCMapApproximate(_)
    )
}

struct Validator<'a> {
    doc: &'a CosDocument,
    buf: &'a [u8],
    out: Vec<Defect>,
    /// Warning kinds already reported, so a file with ten thousand repairs of
    /// one kind produces one defect rather than ten thousand. The report wants
    /// to know which rules a corpus breaks, not every instance of one.
    seen_repairs: BTreeSet<&'static str>,
    /// Resources already checked. One font shared by a thousand pages is one
    /// font, and a pattern whose own resources name it back is a loop.
    visited: BTreeSet<ObjRef>,
    /// The page objects, in page order, as this module's own walk found them.
    /// Annex F's tables are stated per page, so they need the same order.
    pages: Vec<ObjRef>,
}

impl Validator<'_> {
    fn report(&mut self, object: Option<ObjRef>, offset: Option<u64>, kind: DefectKind) {
        self.out.push(Defect {
            object,
            offset,
            kind,
        });
    }

    // ---- the ladder --------------------------------------------------------

    /// Rule zero: the document opened without help.
    ///
    /// Everything else here reads structures. This reads the *reading* — a
    /// file whose offsets had to be repaired to be found is not one this
    /// engine may claim to have written correctly, whatever the repaired
    /// version then says.
    fn ladder(&mut self) {
        let level = self.doc.ladder_level();
        if level != LadderLevel::Trust {
            self.report(None, None, DefectKind::OpenedLenient(level));
        }
        for warning in self.doc.warnings() {
            if is_a_defect(warning.kind) && self.seen_repairs.insert(warning.kind.as_str()) {
                self.report(
                    warning.object,
                    Some(warning.offset),
                    DefectKind::Repaired(warning.kind),
                );
            }
        }
    }

    /// The repairs that only happen when somebody reads that far.
    ///
    /// Opening is lazy: a stream whose filter chain needs a leniency warns
    /// when it is decoded, not when the file opens. Validating decodes every
    /// stream, so the warnings the document gained in the meantime belong to
    /// the verdict — and they are the ones an ordinary open never shows.
    fn repairs_raised_while_reading(&mut self, before: usize) {
        for warning in self.doc.warnings().into_iter().skip(before) {
            if is_a_defect(warning.kind) && self.seen_repairs.insert(warning.kind.as_str()) {
                self.report(
                    warning.object,
                    Some(warning.offset),
                    DefectKind::RepairedWhileReading(warning.kind),
                );
            }
        }
    }

    // ---- the file frame ----------------------------------------------------

    fn frame(&mut self) {
        self.header();
        self.eof();
    }

    /// 7.5.2: `%PDF-N.M` at byte zero, then a comment of four bytes above 127.
    fn header(&mut self) {
        if !self.buf.starts_with(b"%PDF-") {
            self.report(None, Some(0), DefectKind::HeaderMissing);
            return;
        }
        let first_line_end = self
            .buf
            .iter()
            .position(|b| *b == b'\n' || *b == b'\r')
            .unwrap_or(self.buf.len());
        let version = self.buf.get(5..first_line_end).unwrap_or_default();
        let shaped = match version.iter().position(|b| *b == b'.') {
            Some(at) => {
                let (major, minor) = version.split_at(at);
                !major.is_empty()
                    && major.iter().all(u8::is_ascii_digit)
                    && minor.len() > 1
                    && minor.iter().skip(1).all(u8::is_ascii_digit)
            }
            None => false,
        };
        if !shaped {
            self.report(None, Some(5), DefectKind::HeaderVersionUnreadable);
        }

        // The comment line, wherever the header's end-of-line put it. Four
        // bytes above 127 is what 7.5.2 asks for and what transfer software
        // looks at; without them a well-meaning gateway rewrites the line
        // endings of a binary file and every offset in it moves.
        let mut at = first_line_end;
        while matches!(self.buf.get(at), Some(b'\r' | b'\n')) {
            at += 1;
        }
        let second_line_end = self
            .buf
            .iter()
            .skip(at)
            .position(|b| *b == b'\n' || *b == b'\r')
            .map_or(self.buf.len(), |p| at + p);
        let comment = self.buf.get(at..second_line_end).unwrap_or_default();
        let binary = comment.first() == Some(&b'%')
            && comment.iter().skip(1).filter(|b| **b > 127).count() >= 4;
        if !binary {
            self.report(None, Some(at as u64), DefectKind::BinaryCommentMissing);
        }
    }

    /// 7.5.5: the file ends `%%EOF`, and nothing but end-of-line follows it.
    fn eof(&mut self) {
        let mut end = self.buf.len();
        while let Some(byte) = end.checked_sub(1).and_then(|i| self.buf.get(i)) {
            if matches!(byte, b'\r' | b'\n' | b' ' | b'\t' | 0) {
                end -= 1;
            } else {
                break;
            }
        }
        if self.buf.get(end.saturating_sub(5)..end) == Some(&b"%%EOF"[..]) {
            return;
        }

        // A file that ends with something else may still hold a `%%EOF`
        // further back with a tail appended, and the two are worth telling
        // apart: one is a truncation, the other is somebody's concatenation.
        let window_start = self.buf.len().saturating_sub(limits::STARTXREF_SCAN_MAX);
        let window = self.buf.get(window_start..).unwrap_or_default();
        match rfind(window, b"%%EOF") {
            Some(at) => {
                let count = (window.len() - (at + 5)) as u64;
                self.report(
                    None,
                    Some((window_start + at) as u64),
                    DefectKind::BytesAfterEof { count },
                );
            }
            None => self.report(None, None, DefectKind::EofMissing),
        }
    }

    // ---- the cross-reference sections, read from the bytes -----------------

    /// Every section the file chains together, newest first.
    ///
    /// This is the part that cannot be borrowed from [`CosDocument`]: its
    /// table is merged, repaired and normalised, and several of the things
    /// this module most wants to check are exactly what that hides.
    fn sections(&mut self) -> Vec<Section> {
        let Some(start) = self.startxref() else {
            return Vec::new();
        };
        if !self.section_begins_at(start) {
            self.report(None, Some(start), DefectKind::StartxrefNotASection);
            return Vec::new();
        }

        let mut out = Vec::new();
        let mut visited: BTreeSet<u64> = BTreeSet::new();
        let mut next = Some(start);
        while let Some(at) = next {
            if !visited.insert(at) {
                self.report(None, Some(at), DefectKind::PrevCycle);
                break;
            }
            if visited.len() > limits::MAX_XREF_CHAIN as usize {
                break;
            }
            let Some(section) = self.section_at(at) else {
                self.report(None, Some(at), DefectKind::SectionUnreadable);
                break;
            };

            // 7.5.8.4: a hybrid file hangs a cross-reference stream off the
            // classic table, and the objects only it names are invisible to a
            // reader that ignores it.
            let hybrid = nonnegative(section.trailer.get_int(Name::XREF_STM));
            let prev = nonnegative(section.trailer.get_int(Name::PREV));
            out.push(section);
            if let Some(hybrid) = hybrid {
                if visited.insert(hybrid) {
                    match self.section_at(hybrid) {
                        Some(section) => out.push(section),
                        None => self.report(None, Some(hybrid), DefectKind::SectionUnreadable),
                    }
                }
            }
            next = prev;
        }
        out
    }

    /// 7.5.5: the offset the last `startxref` names.
    fn startxref(&mut self) -> Option<u64> {
        let window_start = self.buf.len().saturating_sub(limits::STARTXREF_SCAN_MAX);
        let window = self.buf.get(window_start..).unwrap_or_default();
        let Some(at) = rfind(window, b"startxref") else {
            self.report(None, None, DefectKind::StartxrefMissing);
            return None;
        };
        let digits: Vec<u8> = window
            .iter()
            .skip(at + 9)
            .skip_while(|b| matches!(b, b'\r' | b'\n' | b' ' | b'\t'))
            .take_while(|b| b.is_ascii_digit())
            .copied()
            .collect();
        match core::str::from_utf8(&digits)
            .ok()
            .and_then(|t| t.parse().ok())
        {
            Some(offset) => Some(offset),
            None => {
                self.report(
                    None,
                    Some((window_start + at) as u64),
                    DefectKind::StartxrefMissing,
                );
                None
            }
        }
    }

    /// Whether either spelling of a section begins exactly at `offset`.
    fn section_begins_at(&self, offset: u64) -> bool {
        let Ok(at) = usize::try_from(offset) else {
            return false;
        };
        if self.buf.get(at..).is_some_and(|r| r.starts_with(b"xref")) {
            return true;
        }
        header_at(self.buf, offset).is_some()
    }

    /// One section, whichever spelling it uses.
    fn section_at(&mut self, offset: u64) -> Option<Section> {
        let at = usize::try_from(offset).ok()?;
        if self.buf.get(at..).is_some_and(|r| r.starts_with(b"xref")) {
            self.classic_at(at)
        } else {
            self.xref_stream_at(offset)
        }
    }

    /// A classic table (7.5.4), entry by twenty-byte entry.
    fn classic_at(&mut self, at: usize) -> Option<Section> {
        let mut cursor = skip_space(self.buf, at + 4);
        let _ = at;
        let mut entries = Vec::new();

        loop {
            if self
                .buf
                .get(cursor..)
                .is_some_and(|r| r.starts_with(b"trailer"))
            {
                let mut sink = WarningSink::new();
                let parsed = parse_object_at(
                    self.buf,
                    (cursor + b"trailer".len()) as u64,
                    self.doc.names_table(),
                    &mut sink,
                );
                self.repairs(None, sink);
                let trailer = parsed.object.as_dict().cloned().unwrap_or_default();
                return Some(Section {
                    at: at as u64,
                    entries,
                    trailer,
                });
            }

            let Some((first, used)) = ascii_int(self.buf, cursor) else {
                self.report(None, Some(cursor as u64), DefectKind::TableTrailerMissing);
                return Some(Section {
                    at: at as u64,
                    entries,
                    trailer: Dict::new(),
                });
            };
            let spaced = skip_space(self.buf, cursor + used);
            let Some((count, used)) = ascii_int(self.buf, spaced) else {
                self.report(None, Some(cursor as u64), DefectKind::SubsectionMalformed);
                return Some(Section {
                    at: at as u64,
                    entries,
                    trailer: Dict::new(),
                });
            };
            cursor = skip_eol(self.buf, spaced + used);

            for index in 0..count {
                let Some(row) = self.buf.get(cursor..cursor.saturating_add(20)) else {
                    self.report(None, Some(cursor as u64), DefectKind::EntryNotTwentyBytes);
                    return Some(Section {
                        at: at as u64,
                        entries,
                        trailer: Dict::new(),
                    });
                };
                match twenty_byte_entry(row) {
                    Some(entry) => {
                        if let Ok(number) = u32::try_from(first.saturating_add(index)) {
                            entries.push((number, entry));
                        }
                    }
                    None => self.report(None, Some(cursor as u64), DefectKind::EntryNotTwentyBytes),
                }
                cursor += 20;
            }
            cursor = skip_space(self.buf, cursor);
        }
    }

    /// A cross-reference stream (7.5.8), unpacked by its own `/W`.
    fn xref_stream_at(&mut self, offset: u64) -> Option<Section> {
        let reference = header_at(self.buf, offset)?;
        let mut sink = WarningSink::new();
        let parsed = parse_indirect_at(self.buf, offset, self.doc.names_table(), &mut sink)?;
        self.repairs(Some(reference), sink);
        let dict = parsed.object.as_stream()?.dict.clone();
        let xref = self.doc.intern(b"XRef");
        if dict.get_name(Name::TYPE) != Some(xref) {
            return None;
        }

        let widths: Option<Vec<usize>> = dict.get_array(Name::W).and_then(|w| {
            if w.len() < 3 {
                return None;
            }
            w.iter()
                .map(|value| {
                    value
                        .as_int()
                        .and_then(|v| usize::try_from(v).ok())
                        .filter(|v| *v <= 8)
                })
                .collect::<Option<Vec<usize>>>()
        });
        let Some(widths) = widths.filter(|w| w.iter().sum::<usize>() > 0) else {
            self.report(
                Some(reference),
                Some(offset),
                DefectKind::XrefStreamWidthsBad,
            );
            return Some(Section {
                at: offset,
                entries: Vec::new(),
                trailer: dict,
            });
        };

        let Ok(data) = self.doc.stream_decoded(reference) else {
            self.report(
                Some(reference),
                Some(offset),
                DefectKind::StreamDoesNotDecode,
            );
            return Some(Section {
                at: offset,
                entries: Vec::new(),
                trailer: dict,
            });
        };

        let row = widths.iter().take(3).sum::<usize>();
        let ranges: Vec<(u64, u64)> = match dict.get_array(Name::INDEX) {
            Some(index) => {
                let pairs: Option<Vec<u64>> = if index.len() % 2 == 0 {
                    index
                        .iter()
                        .map(|value| value.as_int().and_then(|v| u64::try_from(v).ok()))
                        .collect::<Option<Vec<u64>>>()
                } else {
                    None
                };
                match pairs {
                    Some(pairs) => pairs.chunks(2).map(|p| (p[0], p[1])).collect(),
                    None => {
                        self.report(
                            Some(reference),
                            Some(offset),
                            DefectKind::XrefStreamIndexBad,
                        );
                        Vec::new()
                    }
                }
            }
            // 7.5.8.2: the default is one range covering every object.
            None => match nonnegative(dict.get_int(Name::SIZE)) {
                Some(size) => vec![(0, size)],
                None => Vec::new(),
            },
        };

        let declared: u64 = ranges.iter().map(|(_, count)| *count).sum();
        let actual = (data.len() / row) as u64;
        if declared != actual {
            self.report(
                Some(reference),
                Some(offset),
                DefectKind::XrefStreamRowsWrong { declared, actual },
            );
        }

        let mut entries = Vec::new();
        let mut cursor = 0usize;
        for (first, count) in ranges {
            for index in 0..count {
                let Some(bytes) = data.get(cursor..cursor.saturating_add(row)) else {
                    break;
                };
                cursor += row;
                // 7.5.8.2: a zero-width first field means every entry is
                // type 1, which is the one default this format has.
                let kind = if widths[0] == 0 {
                    1
                } else {
                    be(bytes, 0, widths[0])
                };
                let second = be(bytes, widths[0], widths[1]);
                let third = be(bytes, widths[0] + widths[1], widths[2]);
                let Ok(number) = u32::try_from(first.saturating_add(index)) else {
                    continue;
                };
                let entry = match kind {
                    0 => RawEntry::Free {
                        next: u32::try_from(second).unwrap_or(0),
                        gen: u16::try_from(third).unwrap_or(u16::MAX),
                    },
                    1 => RawEntry::Offset {
                        offset: second,
                        gen: u16::try_from(third).unwrap_or(0),
                    },
                    2 => RawEntry::InStream {
                        container: u32::try_from(second).unwrap_or(0),
                        index: u32::try_from(third).unwrap_or(0),
                    },
                    other => {
                        self.report(
                            Some(reference),
                            Some(offset),
                            DefectKind::XrefStreamTypeUnknown { kind: other },
                        );
                        continue;
                    }
                };
                entries.push((number, entry));
            }
        }

        Some(Section {
            at: offset,
            entries,
            trailer: dict,
        })
    }

    /// Parse-time repairs, one defect per distinct kind.
    fn repairs(&mut self, object: Option<ObjRef>, mut sink: WarningSink) {
        for warning in sink.take() {
            if is_a_defect(warning.kind) && self.seen_repairs.insert(warning.kind.as_str()) {
                self.report(
                    object.or(warning.object),
                    Some(warning.offset),
                    DefectKind::ObjectRepaired(warning.kind),
                );
            }
        }
    }

    // ---- the trailer -------------------------------------------------------

    /// The newest section's trailer is the one a reader reads first, so it is
    /// the one held to Table 15.
    fn trailer(&mut self, sections: &[Section]) {
        let Some(section) = sections.first() else {
            return;
        };
        let trailer = &section.trailer;

        if trailer.contains_key(Name::ROOT) {
            let root = self.doc.resolve_key(trailer, Name::ROOT);
            let catalog = self.doc.intern(b"Catalog");
            let is_catalog = root
                .as_dict()
                .and_then(|d| d.get_name(Name::TYPE))
                .is_some_and(|kind| kind == catalog);
            if !is_catalog {
                self.report(None, None, DefectKind::RootNotACatalog);
            }
        } else {
            self.report(None, None, DefectKind::TrailerRootMissing);
        }

        // 7.5.5 Table 15: /ID is required whenever /Encrypt is present. This
        // is the rule an outside reader found broken in this engine's output
        // and no test here did, which is why it is spelled out rather than
        // folded into a general required-keys loop.
        let has_id = trailer.contains_key(Name::ID);
        if trailer.contains_key(Name::ENCRYPT) && !has_id {
            self.report(None, None, DefectKind::EncryptWithoutId);
        }
        if has_id {
            let shaped = trailer.get_array(Name::ID).is_some_and(|id| {
                id.len() == 2
                    && id
                        .iter()
                        .all(|part| part.as_string().is_some_and(|s| !s.bytes.is_empty()))
            });
            if !shaped {
                self.report(None, None, DefectKind::IdMalformed);
            }
        }

        let highest = sections
            .iter()
            .flat_map(|s| s.entries.iter().map(|(num, _)| *num))
            .max()
            .unwrap_or(0);
        match trailer.get_int(Name::SIZE) {
            Some(declared) => {
                if declared <= i64::from(highest) {
                    self.report(None, None, DefectKind::SizeTooSmall { declared, highest });
                }
            }
            None => self.report(None, None, DefectKind::TrailerSizeMissing),
        }
    }

    // ---- the entries, one object at a time ---------------------------------

    fn entries(&mut self, sections: &[Section]) {
        if sections.is_empty() {
            return;
        }
        let declared_size = sections
            .first()
            .and_then(|section| section.trailer.get_int(Name::SIZE));

        // Newest first, first writer wins — the same merge 7.5.6 requires of
        // a reader, performed here over sections this module read itself.
        let mut merged: BTreeMap<u32, RawEntry> = BTreeMap::new();
        for section in sections {
            for (num, entry) in &section.entries {
                merged.entry(*num).or_insert(*entry);
            }
        }

        // 7.5.4: object zero is always the head of the free list, at
        // generation 65535. A table without it is one several readers refuse
        // outright, and nothing in an ordinary read ever looks at it.
        match merged.get(&0) {
            Some(RawEntry::Free { gen, .. }) if *gen == u16::MAX => {}
            Some(RawEntry::Free { gen, .. }) => {
                let gen = *gen;
                self.report(None, None, DefectKind::FreeHeadGeneration { gen });
            }
            _ => self.report(None, None, DefectKind::FreeHeadMissing),
        }

        // Every distinct entry any section spells, deduplicated: a linearized
        // file names its first page's objects in two tables and there is no
        // reason to check them twice, but an *older* revision's entry still
        // has to point at a real header.
        let distinct: BTreeSet<(u32, RawEntry)> = sections
            .iter()
            .flat_map(|section| section.entries.iter().copied())
            .collect();
        for (num, entry) in distinct {
            if let RawEntry::Offset { offset, gen } = entry {
                self.entry_header(num, gen, offset);
            }
        }

        let merged_entries: Vec<(u32, RawEntry)> =
            merged.iter().map(|(num, entry)| (*num, *entry)).collect();
        for (num, entry) in merged_entries {
            if let Some(declared) = declared_size {
                if i64::from(num) >= declared {
                    self.report(
                        Some(ObjRef::new(num, 0)),
                        None,
                        DefectKind::EntryPastSize { declared },
                    );
                }
            }
            match entry {
                RawEntry::Free { next, .. } => {
                    // The chain must stay inside the free entries. A `next` of
                    // zero is the terminator, and an object number with no
                    // entry at all reads as free (7.5.4), so only a number
                    // that is defined and in use is a broken link.
                    let in_use = matches!(
                        merged.get(&next),
                        Some(RawEntry::Offset { .. } | RawEntry::InStream { .. })
                    );
                    if next != 0 && in_use {
                        self.report(
                            Some(ObjRef::new(num, 0)),
                            None,
                            DefectKind::FreeNextNotFree { next },
                        );
                    }
                }
                RawEntry::Offset { offset, gen } => self.body(num, gen, offset),
                RawEntry::InStream { container, index } => self.in_stream(num, container, index),
            }
        }
    }

    /// The header must begin at that byte, not near it.
    ///
    /// This is the rule the tolerant reader cannot have: its own header lookup
    /// lexes, so it skips whitespace and comments first, and an entry pointing
    /// at the binary comment six bytes before object 1 opens at Trust with no
    /// warning at all. 7.5.4 says the entry is the offset *of* the object, and
    /// a table of nearly-right offsets is a file that only opens because
    /// everybody's reader searches.
    fn entry_header(&mut self, num: u32, gen: u16, offset: u64) {
        let r = ObjRef::new(num, gen);
        match header_at(self.buf, offset) {
            Some(found) if found == r => {}
            Some(found) => self.report(
                Some(r),
                Some(offset),
                DefectKind::ObjectHeaderMismatch { found },
            ),
            None => self.report(Some(r), Some(offset), DefectKind::ObjectHeaderAbsent),
        }
    }

    /// The object body at a type-1 entry, and its stream extent.
    fn body(&mut self, num: u32, gen: u16, offset: u64) {
        let r = ObjRef::new(num, gen);
        let mut sink = WarningSink::new();
        let Some(parsed) = parse_indirect_at(self.buf, offset, self.doc.names_table(), &mut sink)
        else {
            return;
        };
        self.repairs(Some(r), sink);
        if parsed.object.as_stream().is_some() {
            self.stream(r, &parsed.object);
        }
    }

    /// 7.3.8: the declared length reaches `endstream` exactly, and the chain
    /// decodes.
    fn stream(&mut self, r: ObjRef, object: &Object) {
        let Some(stream) = object.as_stream() else {
            return;
        };

        // 7.3.8.2: an indirect /Length is legal, so it is resolved here — but
        // through the store's plain lookup, never through a recovery.
        let declared = match stream.len_hint {
            Some(len) => Some(len),
            None => stream
                .dict
                .get_ref(Name::LENGTH)
                .and_then(|at| self.doc.get(at).ok())
                .and_then(|value| value.as_int())
                .and_then(|value| u64::try_from(value).ok()),
        };
        let Some(declared) = declared else {
            self.report(
                Some(r),
                Some(stream.data_start),
                DefectKind::StreamLengthUnresolved,
            );
            return;
        };

        // Where `endstream` must be: immediately after the data, give or take
        // the end-of-line 7.3.8.1 permits. This is the check the reader
        // performs *and repairs*; here the repair is the defect.
        let start = usize::try_from(stream.data_start).unwrap_or(usize::MAX);
        let claimed_end = usize::try_from(stream.data_start.saturating_add(declared))
            .unwrap_or(usize::MAX)
            .min(self.buf.len());
        let mut at = claimed_end;
        let mut skipped = 0usize;
        while skipped < limits::MAX_STREAM_EOL_SKIP
            && matches!(self.buf.get(at), Some(b'\r' | b'\n' | b' ' | b'\t'))
        {
            at += 1;
            skipped += 1;
        }
        if !self
            .buf
            .get(at..)
            .is_some_and(|rest| rest.starts_with(b"endstream"))
        {
            let actual = self
                .buf
                .get(start..)
                .and_then(|rest| find(rest, b"endstream"))
                .map(|found| found as u64);
            self.report(
                Some(r),
                Some(stream.data_start),
                DefectKind::StreamLengthNotExact { declared, actual },
            );
        }

        // 7.4: and the bytes are what the filter chain says they are. An
        // image codec handing its data back still encoded is not a failure —
        // 7.4.9 leaves a JPEG a JPEG until something wants pixels — so this
        // asks the decoder for an error rather than for a size.
        if self.doc.stream_decoded(r).is_err() {
            self.report(
                Some(r),
                Some(stream.data_start),
                DefectKind::StreamDoesNotDecode,
            );
        }
    }

    /// One type-2 entry: the container is real and carries this object at the
    /// index the table names.
    ///
    /// 7.5.7's prologue is read here rather than through `objstm`, which
    /// recovers pairs by lexing until they stop making sense. A container
    /// whose `/N` is a lie is exactly what that recovery hides.
    fn in_stream(&mut self, num: u32, container: u32, index: u32) {
        let r = ObjRef::new(num, 0);
        let holder = ObjRef::new(container, 0);
        let Ok(object) = self.doc.get(holder) else {
            self.report(Some(r), None, DefectKind::ObjStmNotAStream { container });
            return;
        };
        let obj_stm = self.doc.intern(b"ObjStm");
        let is_container = object
            .as_stream()
            .and_then(|s| s.dict.get_name(Name::TYPE))
            .is_some_and(|kind| kind == obj_stm);
        if !is_container {
            self.report(Some(r), None, DefectKind::ObjStmNotAStream { container });
            return;
        }

        let dict = object.as_dict().cloned().unwrap_or_default();
        let (count, first) = match (dict.get_int(Name::N), dict.get_int(Name::FIRST)) {
            (Some(n), Some(first)) if n >= 0 && first >= 0 => (n as u64, first as u64),
            _ => {
                self.report(Some(r), None, DefectKind::ObjStmHeaderMissing { container });
                return;
            }
        };
        if u64::from(index) >= count {
            self.report(
                Some(r),
                None,
                DefectKind::ObjStmIndexOutOfRange { container, index },
            );
            return;
        }

        let Ok(data) = self.doc.stream_decoded(holder) else {
            self.report(Some(holder), None, DefectKind::StreamDoesNotDecode);
            return;
        };
        let first = usize::try_from(first).unwrap_or(usize::MAX);
        if first > data.len() {
            self.report(
                Some(r),
                None,
                DefectKind::ObjStmOffsetOutOfRange { container },
            );
            return;
        }

        let pairs = integers(data.get(..first).unwrap_or_default());
        let wanted = usize::try_from(index).unwrap_or(usize::MAX);
        match (pairs.get(wanted * 2), pairs.get(wanted * 2 + 1)) {
            (Some(found), Some(at)) => {
                if *found != i64::from(num) {
                    self.report(
                        Some(r),
                        None,
                        DefectKind::ObjStmNumberMismatch {
                            container,
                            found: u32::try_from(*found).unwrap_or(u32::MAX),
                        },
                    );
                }
                let body = first.saturating_add(usize::try_from(*at).unwrap_or(usize::MAX));
                if body >= data.len() {
                    self.report(
                        Some(r),
                        None,
                        DefectKind::ObjStmOffsetOutOfRange { container },
                    );
                }
            }
            _ => self.report(
                Some(r),
                None,
                DefectKind::ObjStmIndexOutOfRange { container, index },
            ),
        }
    }
}

/// The names the semantic rules need beyond the pre-interned set.
///
/// Interned once rather than per node: a page tree is walked whole, and a
/// thousand-page document would otherwise take the intern table's lock ten
/// thousand times to ask the same eleven questions.
struct SemNames {
    page: Name,
    resources: Name,
    font: Name,
    x_object: Name,
    ext_g_state: Name,
    shading: Name,
    pattern: Name,
    group: Name,
    s: Name,
    cs: Name,
    transparency: Name,
    ca_lower: Name,
    ca_upper: Name,
    bm: Name,
    smask: Name,
    none: Name,
    g: Name,
    alpha: Name,
    luminosity: Name,
    shading_type: Name,
    coords: Name,
    function: Name,
    function_type: Name,
    domain: Name,
    range: Name,
    c0: Name,
    c1: Name,
    n_key: Name,
    functions: Name,
    bounds: Name,
    encode: Name,
    size: Name,
    bits_per_sample: Name,
    pattern_type: Name,
    paint_type: Name,
    tiling_type: Name,
    bbox: Name,
    x_step: Name,
    y_step: Name,
    form: Name,
    image: Name,
    width: Name,
    height: Name,
    color_space: Name,
    bits_per_component: Name,
    image_mask: Name,
    type0: Name,
    encoding: Name,
    descendant_fonts: Name,
    cid_system_info: Name,
    registry: Name,
    ordering: Name,
    supplement: Name,
    cid_to_gid_map: Name,
    widths_key: Name,
    font_descriptor: Name,
    flags: Name,
    to_unicode: Name,
    base_font: Name,
    annots: Name,
    subtype: Name,
    rect: Name,
    link: Name,
    dest: Name,
    action: Name,
    outlines: Name,
    last: Name,
    next: Name,
    title: Name,
}

impl SemNames {
    fn new(doc: &CosDocument) -> SemNames {
        SemNames {
            page: doc.intern(b"Page"),
            resources: Name::RESOURCES,
            font: doc.intern(b"Font"),
            x_object: doc.intern(b"XObject"),
            ext_g_state: doc.intern(b"ExtGState"),
            shading: doc.intern(b"Shading"),
            pattern: doc.intern(b"Pattern"),
            group: doc.intern(b"Group"),
            s: doc.intern(b"S"),
            cs: doc.intern(b"CS"),
            transparency: doc.intern(b"Transparency"),
            ca_lower: doc.intern(b"ca"),
            ca_upper: doc.intern(b"CA"),
            bm: doc.intern(b"BM"),
            smask: doc.intern(b"SMask"),
            none: doc.intern(b"None"),
            g: doc.intern(b"G"),
            alpha: doc.intern(b"Alpha"),
            luminosity: doc.intern(b"Luminosity"),
            shading_type: doc.intern(b"ShadingType"),
            coords: doc.intern(b"Coords"),
            function: doc.intern(b"Function"),
            function_type: doc.intern(b"FunctionType"),
            domain: doc.intern(b"Domain"),
            range: doc.intern(b"Range"),
            c0: doc.intern(b"C0"),
            c1: doc.intern(b"C1"),
            n_key: doc.intern(b"N"),
            functions: doc.intern(b"Functions"),
            bounds: doc.intern(b"Bounds"),
            encode: doc.intern(b"Encode"),
            size: doc.intern(b"Size"),
            bits_per_sample: doc.intern(b"BitsPerSample"),
            pattern_type: doc.intern(b"PatternType"),
            paint_type: doc.intern(b"PaintType"),
            tiling_type: doc.intern(b"TilingType"),
            bbox: doc.intern(b"BBox"),
            x_step: doc.intern(b"XStep"),
            y_step: doc.intern(b"YStep"),
            form: doc.intern(b"Form"),
            image: doc.intern(b"Image"),
            width: doc.intern(b"Width"),
            height: doc.intern(b"Height"),
            color_space: doc.intern(b"ColorSpace"),
            bits_per_component: doc.intern(b"BitsPerComponent"),
            image_mask: doc.intern(b"ImageMask"),
            type0: doc.intern(b"Type0"),
            encoding: doc.intern(b"Encoding"),
            descendant_fonts: doc.intern(b"DescendantFonts"),
            cid_system_info: doc.intern(b"CIDSystemInfo"),
            registry: doc.intern(b"Registry"),
            ordering: doc.intern(b"Ordering"),
            supplement: doc.intern(b"Supplement"),
            cid_to_gid_map: doc.intern(b"CIDToGIDMap"),
            widths_key: Name::W,
            font_descriptor: doc.intern(b"FontDescriptor"),
            flags: doc.intern(b"Flags"),
            to_unicode: doc.intern(b"ToUnicode"),
            base_font: doc.intern(b"BaseFont"),
            annots: doc.intern(b"Annots"),
            subtype: doc.intern(b"Subtype"),
            rect: doc.intern(b"Rect"),
            link: doc.intern(b"Link"),
            dest: doc.intern(b"Dest"),
            action: doc.intern(b"A"),
            outlines: doc.intern(b"Outlines"),
            last: doc.intern(b"Last"),
            next: doc.intern(b"Next"),
            title: doc.intern(b"Title"),
        }
    }
}

/// How deep either tree may go before this stops walking.
///
/// The reader has its own caps and warns when it hits them; this one is here
/// so a hostile file cannot recurse the validator, and it is deliberately
/// generous — a page tree that deep is already reported by the reader.
const MAX_WALK_DEPTH: u32 = 64;

impl Validator<'_> {
    /// The document's own structures, as against the file's layout.
    ///
    /// Everything here is [`Tier::Semantics`]: a rewrite copies these
    /// dictionaries from whatever it was handed, so a defect found in the
    /// rewrite of somebody else's file is that file's and is reported rather
    /// than ratcheted.
    fn semantics(&mut self) {
        let Some(catalog) = self.doc.catalog() else {
            return;
        };
        let names = SemNames::new(self.doc);

        if let Some(root) = catalog.get_ref(Name::PAGES) {
            let mut seen = BTreeSet::new();
            self.page_node(&names, root, None, Inherited::default(), 0, &mut seen);
        }
        if let Some(root) = catalog.get_ref(names.outlines) {
            self.outline_root(&names, root);
        }
    }

    /// One node of the page tree, returning the leaves below it.
    fn page_node(
        &mut self,
        names: &SemNames,
        node: ObjRef,
        parent: Option<ObjRef>,
        inherited: Inherited,
        depth: u32,
        seen: &mut BTreeSet<ObjRef>,
    ) -> u64 {
        if depth > MAX_WALK_DEPTH {
            return 0;
        }
        if !seen.insert(node) {
            self.report(Some(node), None, DefectKind::PageTreeCycle);
            return 0;
        }
        let Ok(object) = self.doc.get(node) else {
            return 0;
        };
        let Some(dict) = object.as_dict().cloned() else {
            self.report(Some(node), None, DefectKind::PageNodeUntyped);
            return 0;
        };

        // 7.7.3.2: every node but the root names the node above it. The reader
        // walks downwards and never asks, so a tree with every `/Parent`
        // deleted paginates, renders and round-trips — and a viewer that walks
        // up from a page, which is how "which chapter is this" is answered,
        // finds nothing.
        if parent.is_some() {
            let declared = dict.get_ref(Name::PARENT);
            if declared != parent {
                self.report(
                    Some(node),
                    None,
                    DefectKind::PageParentWrong { found: declared },
                );
            }
        }

        // 7.7.3.3: inheritable, so a page with none is only wrong if no
        // ancestor had one either.
        let own_box = dict.contains_key(Name::MEDIA_BOX);
        if own_box {
            self.media_box(node, &dict);
        }
        // 7.7.3.4: `/Resources` is inheritable too, and a page that names none
        // uses the nearest ancestor's rather than having none.
        let own_resources = self
            .doc
            .resolve_key(&dict, names.resources)
            .as_dict()
            .cloned();
        let inherited = Inherited {
            media_box: own_box || inherited.media_box,
            resources: own_resources.or(inherited.resources),
        };

        let kind = dict.get_name(Name::TYPE);
        if kind == Some(names.page) {
            if !inherited.media_box {
                self.report(Some(node), None, DefectKind::MediaBoxAbsent);
            }
            self.pages.push(node);
            self.annotations(names, node, &dict);
            if let Some(resources) = inherited.resources {
                self.resources(names, Some(node), &resources, 0);
            }
            return 1;
        }
        if kind != Some(Name::PAGES) {
            self.report(Some(node), None, DefectKind::PageNodeUntyped);
            return 0;
        }

        // 7.7.3.2: kids are indirect references, which is what lets a tree be
        // shared and a page be found twice.
        let Some(kids) = dict.get_array(Name::KIDS).map(<[Object]>::to_vec) else {
            self.report(Some(node), None, DefectKind::KidsMalformed);
            return 0;
        };
        let mut leaves = 0u64;
        for kid in &kids {
            match kid.as_objref() {
                Some(kid) => {
                    leaves +=
                        self.page_node(names, kid, Some(node), inherited.clone(), depth + 1, seen);
                }
                None => self.report(Some(node), None, DefectKind::KidsMalformed),
            }
        }

        if let Some(declared) = dict.get_int(Name::COUNT) {
            if declared != i64::try_from(leaves).unwrap_or(i64::MAX) {
                self.report(
                    Some(node),
                    None,
                    DefectKind::PageCountWrong {
                        declared,
                        actual: leaves,
                    },
                );
            }
        } else {
            self.report(
                Some(node),
                None,
                DefectKind::PageCountWrong {
                    declared: 0,
                    actual: leaves,
                },
            );
        }
        leaves
    }

    /// 7.7.3.3: four numbers, and an area rather than a line.
    fn media_box(&mut self, node: ObjRef, dict: &Dict) {
        let value = self.doc.resolve_key(dict, Name::MEDIA_BOX);
        let Some(box_) = value.as_array() else {
            self.report(Some(node), None, DefectKind::MediaBoxDegenerate);
            return;
        };
        let numbers: Option<Vec<f64>> = (box_.len() == 4)
            .then(|| {
                box_.iter()
                    .map(Object::as_number)
                    .collect::<Option<Vec<f64>>>()
            })
            .flatten();
        let Some(numbers) = numbers else {
            self.report(Some(node), None, DefectKind::MediaBoxDegenerate);
            return;
        };
        let width = (numbers[2] - numbers[0]).abs();
        let height = (numbers[3] - numbers[1]).abs();
        if !(width.is_finite() && height.is_finite()) || width <= 0.0 || height <= 0.0 {
            self.report(Some(node), None, DefectKind::MediaBoxDegenerate);
        }
    }

    /// 12.5: the annotations a page carries.
    fn annotations(&mut self, names: &SemNames, page: ObjRef, dict: &Dict) {
        let value = self.doc.resolve_key(dict, names.annots);
        let Some(annots) = value.as_array().map(<[Object]>::to_vec) else {
            return;
        };
        for entry in annots {
            let reference = entry.as_objref();
            let resolved = self.doc.resolve(&entry);
            let Some(annot) = resolved.as_dict() else {
                continue;
            };
            let at = reference.or(Some(page));

            let subtype = annot.get_name(names.subtype);
            if subtype.is_none() {
                self.report(at, None, DefectKind::AnnotSubtypeMissing);
            }

            match annot.get_array(names.rect) {
                Some(rect) if rect.len() == 4 => {
                    let numbers: Option<Vec<f64>> = rect
                        .iter()
                        .map(Object::as_number)
                        .collect::<Option<Vec<f64>>>();
                    match numbers {
                        // 12.5.2: the corners are stated lower-left then
                        // upper-right, and a viewer that takes them as written
                        // draws an inverted hot spot from a reversed pair. Our
                        // own reader normalises them on the way out, which is
                        // exactly why nothing else here notices.
                        Some(numbers) => {
                            if numbers[0] > numbers[2] || numbers[1] > numbers[3] {
                                self.report(at, None, DefectKind::AnnotRectUnordered);
                            }
                        }
                        None => self.report(at, None, DefectKind::AnnotRectMalformed),
                    }
                }
                _ => self.report(at, None, DefectKind::AnnotRectMalformed),
            }

            // 12.5.6.5: a link with neither a destination nor an action is a
            // rectangle that does nothing.
            if subtype == Some(names.link)
                && !annot.contains_key(names.dest)
                && !annot.contains_key(names.action)
            {
                self.report(at, None, DefectKind::LinkWithoutTarget);
            }
        }
    }

    /// 12.3.3: the outline's root, and the chain below it.
    fn outline_root(&mut self, names: &SemNames, root: ObjRef) {
        let Ok(object) = self.doc.get(root) else {
            return;
        };
        let Some(dict) = object.as_dict().cloned() else {
            return;
        };

        let mut seen = BTreeSet::new();
        let (visible, ends) =
            self.outline_chain(names, dict.get_ref(Name::FIRST), root, 0, &mut seen);

        // The root states every item the tree exposes, and is the one node
        // that cannot be closed.
        if let Some(declared) = dict.get_int(Name::COUNT) {
            if declared != visible {
                self.report(
                    Some(root),
                    None,
                    DefectKind::OutlineCountWrong {
                        declared,
                        actual: visible,
                    },
                );
            }
        }
        if ends != (dict.get_ref(Name::FIRST), dict.get_ref(names.last)) {
            self.report(Some(root), None, DefectKind::OutlineEndsWrong);
        }
    }

    /// One sibling chain: how many items it exposes, and its two ends.
    fn outline_chain(
        &mut self,
        names: &SemNames,
        first: Option<ObjRef>,
        parent: ObjRef,
        depth: u32,
        seen: &mut BTreeSet<ObjRef>,
    ) -> (i64, (Option<ObjRef>, Option<ObjRef>)) {
        if depth > MAX_WALK_DEPTH {
            return (0, (None, None));
        }

        let mut exposed = 0i64;
        let mut previous: Option<ObjRef> = None;
        let mut cursor = first;
        let mut last = None;
        while let Some(item) = cursor {
            // A `/Next` that names an item already visited, or something that
            // is not an outline item at all, is a chain a reader walks off the
            // end of. Reported against the item that named it.
            if !seen.insert(item) {
                self.report(previous, None, DefectKind::OutlineNextWrong);
                break;
            }
            let Ok(object) = self.doc.get(item) else {
                self.report(previous, None, DefectKind::OutlineNextWrong);
                break;
            };
            let Some(dict) = object.as_dict().cloned() else {
                self.report(previous, None, DefectKind::OutlineNextWrong);
                break;
            };

            if !dict.contains_key(names.title) {
                self.report(Some(item), None, DefectKind::OutlineTitleMissing);
            }
            if dict.get_ref(Name::PARENT) != Some(parent) {
                self.report(Some(item), None, DefectKind::OutlineParentWrong);
            }
            // The back half of the chain. Deleting every `/Prev` survives every
            // round trip this repository had before the validator existed: the
            // reader walks `/Next` forward, which is enough to build the tree,
            // and a viewer walking up from a selected entry is the only thing
            // that ever notices.
            if dict.get_ref(Name::PREV) != previous {
                self.report(Some(item), None, DefectKind::OutlinePrevWrong);
            }

            let children =
                self.outline_chain(names, dict.get_ref(Name::FIRST), item, depth + 1, seen);
            let (below, child_ends) = children;
            if dict.contains_key(Name::FIRST) || dict.contains_key(names.last) {
                if child_ends != (dict.get_ref(Name::FIRST), dict.get_ref(names.last)) {
                    self.report(Some(item), None, DefectKind::OutlineEndsWrong);
                }
                // Table 152: an open item states how many items it exposes, a
                // closed one states the negative of that, and either way the
                // magnitude is the same number.
                match dict.get_int(Name::COUNT) {
                    Some(declared) if declared.abs() == below => {
                        if declared > 0 {
                            exposed += below;
                        }
                    }
                    declared => self.report(
                        Some(item),
                        None,
                        DefectKind::OutlineCountWrong {
                            declared: declared.unwrap_or(0),
                            actual: below,
                        },
                    ),
                }
            }
            exposed += 1;

            let following = dict.get_ref(names.next);
            previous = Some(item);
            last = Some(item);
            cursor = following;
        }

        (exposed, (first, last))
    }
}

/// What 7.7.3.4 lets a page take from the node above it.
#[derive(Clone, Default)]
struct Inherited {
    media_box: bool,
    resources: Option<Dict>,
}

/// 11.3.5's separable and non-separable blend modes, and nothing else.
const BLEND_MODES: &[&[u8]] = &[
    b"Normal",
    b"Compatible",
    b"Multiply",
    b"Screen",
    b"Overlay",
    b"Darken",
    b"Lighten",
    b"ColorDodge",
    b"ColorBurn",
    b"HardLight",
    b"SoftLight",
    b"Difference",
    b"Exclusion",
    b"Hue",
    b"Saturation",
    b"Color",
    b"Luminosity",
];

impl Validator<'_> {
    /// One resource dictionary, and everything it names (7.8.3).
    ///
    /// The tolerant reader consults a *form's* resources nowhere at all, which
    /// `features/rendering.md` records as a gap; this walks them, because a
    /// dictionary nothing reads is a dictionary nothing checks.
    fn resources(&mut self, names: &SemNames, at: Option<ObjRef>, res: &Dict, depth: u32) {
        if depth > MAX_WALK_DEPTH {
            return;
        }
        for category in [
            names.font,
            names.x_object,
            names.ext_g_state,
            names.shading,
            names.pattern,
        ] {
            let value = self.doc.resolve_key(res, category);
            let Some(dict) = value.as_dict().cloned() else {
                continue;
            };
            for (_, entry) in dict.entries().to_vec() {
                let reference = entry.as_objref();
                if let Some(reference) = reference {
                    if !self.visited.insert(reference) {
                        continue;
                    }
                }
                let resolved = self.doc.resolve(&entry);
                let Some(item) = resolved.as_dict().cloned() else {
                    // 7.8.3: a name that resolves to nothing is a name the
                    // content stream will use and the reader will skip.
                    self.report(reference.or(at), None, DefectKind::ResourceUnresolved);
                    continue;
                };
                let at = reference.or(at);
                if category == names.font {
                    self.font(names, at, &item, depth);
                } else if category == names.x_object {
                    self.xobject(names, at, &resolved, &item, depth);
                } else if category == names.ext_g_state {
                    self.ext_gstate(names, at, &item, depth);
                } else if category == names.shading {
                    self.shading(names, at, &item, depth);
                } else {
                    self.pattern(names, at, &resolved, &item, depth);
                }
            }
        }
    }

    /// 11.6.4.4 and 11.3.5: the graphics state parameters.
    fn ext_gstate(&mut self, names: &SemNames, at: Option<ObjRef>, dict: &Dict, depth: u32) {
        for (key, entry) in [(names.ca_lower, "/ca"), (names.ca_upper, "/CA")] {
            if let Some(value) = dict.get(key) {
                let alpha = value.as_number();
                if !alpha.is_some_and(|a| (0.0..=1.0).contains(&a)) {
                    self.report(at, None, DefectKind::ExtGStateMalformed { entry });
                }
            }
        }

        if let Some(value) = dict.get(names.bm) {
            let known = |name: Name| {
                self.doc
                    .name_bytes(name)
                    .is_some_and(|bytes| BLEND_MODES.contains(&&bytes[..]))
            };
            let ok = match value {
                Object::Name(name) => known(*name),
                // 11.6.4.4: an array of names, first supported one wins.
                Object::Array(names) => names
                    .iter()
                    .all(|value| value.as_name().is_some_and(&known)),
                _ => false,
            };
            if !ok {
                self.report(at, None, DefectKind::ExtGStateMalformed { entry: "/BM" });
            }
        }

        // 11.6.5.2: a soft mask is `/None` or a dictionary naming a group.
        if let Some(value) = dict.get(names.smask) {
            let resolved = self.doc.resolve(value);
            match resolved.as_ref() {
                Object::Name(name) if *name == names.none => {}
                Object::Dict(mask) => {
                    let kind = mask.get_name(names.s);
                    if kind != Some(names.alpha) && kind != Some(names.luminosity) {
                        self.report(
                            at,
                            None,
                            DefectKind::ExtGStateMalformed { entry: "/SMask /S" },
                        );
                    }
                    let group = self.doc.resolve_key(mask, names.g);
                    match group.as_dict() {
                        Some(form) => {
                            let form = form.clone();
                            self.group(names, at, &form);
                        }
                        None => self.report(
                            at,
                            None,
                            DefectKind::ExtGStateMalformed { entry: "/SMask /G" },
                        ),
                    }
                }
                _ => self.report(at, None, DefectKind::ExtGStateMalformed { entry: "/SMask" }),
            }
        }

        let _ = depth;
    }

    /// 11.6.6: a transparency group's own dictionary.
    fn group(&mut self, names: &SemNames, at: Option<ObjRef>, holder: &Dict) {
        let value = self.doc.resolve_key(holder, names.group);
        let Some(group) = value.as_dict() else {
            return;
        };
        if group.get_name(names.s) != Some(names.transparency) {
            self.report(at, None, DefectKind::GroupMalformed { entry: "/S" });
        }
        // 11.6.6: the group's colour space is a name or an array, and a group
        // declared in one this engine composites in another is the whole of
        // the roadmap's transparency item — so an unreadable /CS is a defect
        // rather than something to default away.
        if let Some(cs) = group.get(names.cs) {
            let resolved = self.doc.resolve(cs);
            if !matches!(resolved.as_ref(), Object::Name(_) | Object::Array(_)) {
                self.report(at, None, DefectKind::GroupMalformed { entry: "/CS" });
            }
        }
    }

    /// 8.7.4.5: a shading dictionary, by its own type.
    fn shading(&mut self, names: &SemNames, at: Option<ObjRef>, dict: &Dict, depth: u32) {
        let Some(kind) = dict.get_int(names.shading_type) else {
            self.report(
                at,
                None,
                DefectKind::ShadingMalformed {
                    entry: "/ShadingType",
                },
            );
            return;
        };
        if !(1..=7).contains(&kind) {
            self.report(
                at,
                None,
                DefectKind::ShadingMalformed {
                    entry: "/ShadingType",
                },
            );
            return;
        }

        // 8.7.4.5.3 and 8.7.4.5.4: four numbers for an axis, six for two
        // circles, and the arity is the whole difference between them.
        let wanted = match kind {
            2 => Some(4),
            3 => Some(6),
            _ => None,
        };
        if let Some(wanted) = wanted {
            let coords = self.doc.resolve_key(dict, names.coords);
            let ok = coords
                .as_array()
                .is_some_and(|c| c.len() == wanted && c.iter().all(|v| v.as_number().is_some()));
            if !ok {
                self.report(at, None, DefectKind::ShadingMalformed { entry: "/Coords" });
            }
        }

        if matches!(kind, 1..=3) {
            let function = self.doc.resolve_key(dict, names.function);
            match function.as_ref() {
                Object::Dict(_) | Object::Stream(_) => {
                    let function = function.as_dict().cloned().unwrap_or_default();
                    self.function(names, at, &function, depth + 1);
                }
                Object::Array(entries) => {
                    for entry in entries.clone() {
                        let resolved = self.doc.resolve(&entry);
                        match resolved.as_dict() {
                            Some(function) => {
                                let function = function.clone();
                                self.function(names, at, &function, depth + 1);
                            }
                            None => self.report(
                                at,
                                None,
                                DefectKind::ShadingMalformed { entry: "/Function" },
                            ),
                        }
                    }
                }
                _ => self.report(
                    at,
                    None,
                    DefectKind::ShadingMalformed { entry: "/Function" },
                ),
            }
        }
    }

    /// 7.10: a function, by its own type.
    fn function(&mut self, names: &SemNames, at: Option<ObjRef>, dict: &Dict, depth: u32) {
        if depth > MAX_WALK_DEPTH {
            return;
        }
        let numbers = |value: &Object| -> Option<usize> {
            value
                .as_array()
                .filter(|a| a.iter().all(|v| v.as_number().is_some()))
                .map(<[Object]>::len)
        };

        // 7.10.2: every function states its domain. The reader defaults a
        // missing one to [0 1], which is why nothing else notices.
        let domain = self.doc.resolve_key(dict, names.domain);
        match numbers(&domain) {
            Some(len) if len >= 2 && len % 2 == 0 => {}
            _ => self.report(at, None, DefectKind::FunctionMalformed { entry: "/Domain" }),
        }

        let Some(kind) = dict.get_int(names.function_type) else {
            self.report(
                at,
                None,
                DefectKind::FunctionMalformed {
                    entry: "/FunctionType",
                },
            );
            return;
        };

        match kind {
            // 7.10.2: sampled, and the sample table needs its shape stated.
            0 => {
                let size = self.doc.resolve_key(dict, names.size);
                if size.as_array().is_none_or(<[Object]>::is_empty) {
                    self.report(at, None, DefectKind::FunctionMalformed { entry: "/Size" });
                }
                if dict.get_int(names.bits_per_sample).is_none() {
                    self.report(
                        at,
                        None,
                        DefectKind::FunctionMalformed {
                            entry: "/BitsPerSample",
                        },
                    );
                }
                if numbers(&self.doc.resolve_key(dict, names.range)).is_none() {
                    self.report(at, None, DefectKind::FunctionMalformed { entry: "/Range" });
                }
            }
            // 7.10.3: exponential interpolation between two tuples.
            2 => {
                let c0 = numbers(&self.doc.resolve_key(dict, names.c0)).unwrap_or(1);
                let c1 = numbers(&self.doc.resolve_key(dict, names.c1)).unwrap_or(1);
                if c0 != c1 {
                    self.report(
                        at,
                        None,
                        DefectKind::FunctionMalformed {
                            entry: "/C0 and /C1",
                        },
                    );
                }
                if dict.get_number(names.n_key).is_none() {
                    self.report(at, None, DefectKind::FunctionMalformed { entry: "/N" });
                }
            }
            // 7.10.4: k sub-functions, k-1 bounds, k encode pairs.
            3 => {
                let value = self.doc.resolve_key(dict, names.functions);
                let Some(sub) = value.as_array().map(<[Object]>::to_vec) else {
                    self.report(
                        at,
                        None,
                        DefectKind::FunctionMalformed {
                            entry: "/Functions",
                        },
                    );
                    return;
                };
                let k = sub.len();
                if numbers(&self.doc.resolve_key(dict, names.bounds)) != Some(k.saturating_sub(1)) {
                    self.report(at, None, DefectKind::FunctionMalformed { entry: "/Bounds" });
                }
                if numbers(&self.doc.resolve_key(dict, names.encode)) != Some(k * 2) {
                    self.report(at, None, DefectKind::FunctionMalformed { entry: "/Encode" });
                }
                for entry in sub {
                    let resolved = self.doc.resolve(&entry);
                    if let Some(function) = resolved.as_dict() {
                        let function = function.clone();
                        self.function(names, at, &function, depth + 1);
                    }
                }
            }
            // 7.10.5: a PostScript calculator, whose range is not optional.
            4 => {
                if numbers(&self.doc.resolve_key(dict, names.range)).is_none() {
                    self.report(at, None, DefectKind::FunctionMalformed { entry: "/Range" });
                }
            }
            _ => self.report(
                at,
                None,
                DefectKind::FunctionMalformed {
                    entry: "/FunctionType",
                },
            ),
        }
    }

    /// 8.7.3: a tiling pattern's cell and spacing, or a shading pattern.
    fn pattern(
        &mut self,
        names: &SemNames,
        at: Option<ObjRef>,
        object: &Object,
        dict: &Dict,
        depth: u32,
    ) {
        match dict.get_int(names.pattern_type) {
            Some(1) => {
                if object.as_stream().is_none() {
                    self.report(
                        at,
                        None,
                        DefectKind::PatternMalformed {
                            entry: "cell, which is a stream",
                        },
                    );
                }
                self.rectangle(at, dict, names.bbox, |entry| DefectKind::PatternMalformed {
                    entry,
                });
                // 8.7.3.1: the steps are what a cell repeats at, and a zero
                // one is a pattern that paints one cell forever. The reader
                // falls back to the cell's own size, so it draws either way.
                for (key, entry) in [(names.x_step, "/XStep"), (names.y_step, "/YStep")] {
                    let step = self.doc.resolve_key(dict, key);
                    if !step.as_number().is_some_and(|v| v != 0.0 && v.is_finite()) {
                        self.report(at, None, DefectKind::PatternMalformed { entry });
                    }
                }
                if !matches!(dict.get_int(names.paint_type), Some(1 | 2)) {
                    self.report(
                        at,
                        None,
                        DefectKind::PatternMalformed {
                            entry: "/PaintType",
                        },
                    );
                }
                if !matches!(dict.get_int(names.tiling_type), Some(1..=3)) {
                    self.report(
                        at,
                        None,
                        DefectKind::PatternMalformed {
                            entry: "/TilingType",
                        },
                    );
                }
                let resources = self.doc.resolve_key(dict, names.resources);
                if let Some(resources) = resources.as_dict().cloned() {
                    self.resources(names, at, &resources, depth + 1);
                }
            }
            Some(2) => {
                let shading = self.doc.resolve_key(dict, names.shading);
                match shading.as_dict() {
                    Some(shading) => {
                        let shading = shading.clone();
                        self.shading(names, at, &shading, depth + 1);
                    }
                    None => {
                        self.report(at, None, DefectKind::PatternMalformed { entry: "/Shading" })
                    }
                }
            }
            _ => self.report(
                at,
                None,
                DefectKind::PatternMalformed {
                    entry: "/PatternType",
                },
            ),
        }
    }

    /// 8.8: an image or a form.
    fn xobject(
        &mut self,
        names: &SemNames,
        at: Option<ObjRef>,
        object: &Object,
        dict: &Dict,
        depth: u32,
    ) {
        let subtype = dict.get_name(names.subtype);
        if object.as_stream().is_none() {
            self.report(
                at,
                None,
                DefectKind::XObjectMalformed {
                    entry: "body, which is a stream",
                },
            );
            return;
        }

        if subtype == Some(names.form) {
            self.rectangle(at, dict, names.bbox, |entry| DefectKind::XObjectMalformed {
                entry,
            });
            self.group(names, at, dict);
            let resources = self.doc.resolve_key(dict, names.resources);
            if let Some(resources) = resources.as_dict().cloned() {
                self.resources(names, at, &resources, depth + 1);
            }
            return;
        }
        if subtype != Some(names.image) {
            self.report(at, None, DefectKind::XObjectMalformed { entry: "/Subtype" });
            return;
        }

        for (key, entry) in [(names.width, "/Width"), (names.height, "/Height")] {
            let value = self.doc.resolve_key(dict, key);
            if value.as_int().is_none_or(|v| v <= 0) {
                self.report(at, None, DefectKind::XObjectMalformed { entry });
            }
        }

        // 8.9.5: a stencil mask states neither, and a JPEG 2000 image may
        // carry its own colour space inside the codestream (8.9.5.4).
        let stencil = dict.get_bool(names.image_mask) == Some(true);
        let jpx = self
            .doc
            .filter_chain(dict, &mut WarningSink::new())
            .iter()
            .any(|spec| spec.filter == tinker_pdf_filters::Filter::Jpx);
        if !stencil && !jpx {
            if !dict.contains_key(names.color_space) {
                self.report(
                    at,
                    None,
                    DefectKind::XObjectMalformed {
                        entry: "/ColorSpace",
                    },
                );
            }
            if dict.get_int(names.bits_per_component).is_none() {
                self.report(
                    at,
                    None,
                    DefectKind::XObjectMalformed {
                        entry: "/BitsPerComponent",
                    },
                );
            }
        }
    }

    /// 9.5–9.7: a font, by whether it is composite.
    fn font(&mut self, names: &SemNames, at: Option<ObjRef>, dict: &Dict, depth: u32) {
        let Some(subtype) = dict.get_name(names.subtype) else {
            self.report(at, None, DefectKind::FontMalformed { entry: "/Subtype" });
            return;
        };
        if subtype != names.type0 {
            // 9.6.2: a simple font names the face it wants.
            if dict.get_name(names.base_font).is_none() {
                self.report(at, None, DefectKind::FontMalformed { entry: "/BaseFont" });
            }
            return;
        }

        // 9.7.4: the encoding is what turns a string's bytes into CIDs, and
        // without it nothing can say how wide a code even is.
        let encoding = self.doc.resolve_key(dict, names.encoding);
        if !matches!(encoding.as_ref(), Object::Name(_) | Object::Stream(_)) {
            self.report(at, None, DefectKind::FontMalformed { entry: "/Encoding" });
        }

        // 9.7.1: exactly one descendant, which is where the metrics live.
        let descendants = self.doc.resolve_key(dict, names.descendant_fonts);
        let descendant = descendants
            .as_array()
            .filter(|entries| entries.len() == 1)
            .and_then(<[Object]>::first)
            .map(|entry| self.doc.resolve(entry));
        let Some(descendant) = descendant.as_ref().and_then(|d| d.as_dict()).cloned() else {
            self.report(
                at,
                None,
                DefectKind::FontMalformed {
                    entry: "/DescendantFonts",
                },
            );
            return;
        };

        // 9.7.3: the registry, ordering and supplement a CID is meaningful in.
        let info = self.doc.resolve_key(&descendant, names.cid_system_info);
        let complete = info.as_dict().is_some_and(|info| {
            info.get_string(names.registry).is_some()
                && info.get_string(names.ordering).is_some()
                && info.get_int(names.supplement).is_some()
        });
        if !complete {
            self.report(
                at,
                None,
                DefectKind::FontMalformed {
                    entry: "/CIDSystemInfo",
                },
            );
        }

        // 9.7.4.2: a name or a stream. Absent means /Identity, which is legal.
        if let Some(map) = descendant.get(names.cid_to_gid_map) {
            let resolved = self.doc.resolve(map);
            if !matches!(resolved.as_ref(), Object::Name(_) | Object::Stream(_)) {
                self.report(
                    at,
                    None,
                    DefectKind::FontMalformed {
                        entry: "/CIDToGIDMap",
                    },
                );
            }
        }

        // 9.7.4.3: `c [w1 w2 ...]` or `first last w`, and nothing else.
        if let Some(widths) = self
            .doc
            .resolve_key(&descendant, names.widths_key)
            .as_array()
        {
            let mut index = 0usize;
            let mut ok = true;
            while index < widths.len() {
                let start = widths.get(index).and_then(Object::as_int);
                let second = widths.get(index + 1).map(|value| self.doc.resolve(value));
                match (start, second.as_ref().map(|value| value.as_ref())) {
                    (Some(_), Some(Object::Array(run))) => {
                        ok &= run.iter().all(|w| w.as_number().is_some());
                        index += 2;
                    }
                    (Some(first), Some(Object::Int(last))) => {
                        ok &= *last >= first
                            && widths.get(index + 2).and_then(Object::as_number).is_some();
                        index += 3;
                    }
                    _ => {
                        ok = false;
                        break;
                    }
                }
            }
            if !ok {
                self.report(at, None, DefectKind::FontMalformed { entry: "/W" });
            }
        }

        // 9.8: the descriptor, and the one flag a symbolic font cannot omit.
        let descriptor = self.doc.resolve_key(&descendant, names.font_descriptor);
        let flagged = descriptor
            .as_dict()
            .is_some_and(|d| d.get_int(names.flags).is_some());
        if !flagged {
            self.report(
                at,
                None,
                DefectKind::FontMalformed {
                    entry: "/FontDescriptor",
                },
            );
        }

        // 9.10.3: a `/ToUnicode` that is not a CMap maps nothing, and text
        // extraction silently returns the codes instead of the characters.
        if let Some(map) = dict.get_ref(names.to_unicode) {
            let readable = self
                .doc
                .stream_decoded(map)
                .ok()
                .is_some_and(|data| find(&data, b"begincmap").is_some());
            if !readable {
                self.report(
                    at,
                    None,
                    DefectKind::FontMalformed {
                        entry: "/ToUnicode",
                    },
                );
            }
        }

        let _ = depth;
    }

    /// Four numbers enclosing an area, under whichever rule wants them.
    fn rectangle(
        &mut self,
        at: Option<ObjRef>,
        dict: &Dict,
        key: Name,
        kind: impl Fn(&'static str) -> DefectKind,
    ) {
        let value = self.doc.resolve_key(dict, key);
        let numbers: Option<Vec<f64>> = value
            .as_array()
            .filter(|a| a.len() == 4)
            .and_then(|a| a.iter().map(Object::as_number).collect());
        let Some(numbers) = numbers else {
            self.report(at, None, kind("/BBox"));
            return;
        };
        let width = (numbers[2] - numbers[0]).abs();
        let height = (numbers[3] - numbers[1]).abs();
        if !(width.is_finite() && height.is_finite()) || width <= 0.0 || height <= 0.0 {
            self.report(at, None, kind("/BBox"));
        }
    }
}

impl Validator<'_> {
    /// Annex F: what a linearized file promises, against what it holds.
    ///
    /// Every expectation here is recomputed from the *file* — the object
    /// extents recovered from the cross-reference sections, the page order
    /// this module's own walk found — and never from the writer's plan. The
    /// two routes meet at the same numbers or the file is wrong. That is the
    /// whole design: the writer's own round-trip reader already agrees with
    /// the writer, which is the agreement that proves nothing.
    fn linearization(&mut self, sections: &[Section]) {
        // F.2.2: the parameter dictionary is the first object in the file, and
        // a file whose first object is anything else is simply not linearized.
        let Some(first) = self.first_object() else {
            return;
        };
        let linearized = self.doc.intern(b"Linearized");
        let Some(dict) = first.1.as_dict().filter(|d| d.contains_key(linearized)) else {
            return;
        };
        let dict = dict.clone();

        let parameter = |key: &[u8]| -> Option<u64> {
            let name = self.doc.intern(key);
            nonnegative(dict.get_int(name))
        };

        // F.2.2 item 2: the length of the whole file.
        if parameter(b"L") != Some(self.buf.len() as u64) {
            self.report(
                Some(first.0),
                None,
                DefectKind::LinearizedParameterWrong { entry: "/L" },
            );
        }

        // Item 4: how many pages, which this module counted itself.
        let page_count = self.pages.len();
        if parameter(b"N") != Some(page_count as u64) {
            self.report(
                Some(first.0),
                None,
                DefectKind::LinearizedParameterWrong { entry: "/N" },
            );
        }

        // Item 3: the first page's own page object.
        let first_page = self.pages.first().copied();
        if parameter(b"O") != first_page.map(|page| u64::from(page.num)) {
            self.report(
                Some(first.0),
                None,
                DefectKind::LinearizedParameterWrong { entry: "/O" },
            );
        }

        // Item 6: the offset of the first entry of the main cross-reference
        // section — the *second* section in the chain, since the front one is
        // what `startxref` names.
        if let Some(main) = sections.get(1) {
            let wanted = first_entry_of(self.buf, main.at);
            if parameter(b"T") != Some(wanted) {
                self.report(
                    Some(first.0),
                    None,
                    DefectKind::LinearizedParameterWrong { entry: "/T" },
                );
            }
        }

        let extents = self.extents(sections);

        // Item 5: the end of the first page's section. Recomputed as the
        // furthest byte any object of page one reaches, which is a weaker
        // statement than the writer's own arithmetic and an independent one.
        let run = self.page_run(0).unwrap_or_default();
        let reaches = run
            .iter()
            .filter_map(|num| extents.get(num))
            .map(|(at, len)| at + len)
            .max();
        match (parameter(b"E"), reaches) {
            (Some(declared), Some(reaches)) if declared >= reaches => {}
            (_, None) => {}
            _ => self.report(
                Some(first.0),
                None,
                DefectKind::LinearizedParameterWrong { entry: "/E" },
            ),
        }

        // Item 7: where the primary hint stream is, and how long.
        let hint = dict
            .get_array(self.doc.intern(b"H"))
            .map(<[Object]>::to_vec)
            .unwrap_or_default();
        let hint_offset = hint
            .first()
            .and_then(Object::as_int)
            .and_then(|v| u64::try_from(v).ok());
        let hint_length = hint
            .get(1)
            .and_then(Object::as_int)
            .and_then(|v| u64::try_from(v).ok());
        let (Some(hint_offset), Some(hint_length)) = (hint_offset, hint_length) else {
            self.report(
                Some(first.0),
                None,
                DefectKind::LinearizedParameterWrong { entry: "/H" },
            );
            return;
        };
        let Some((stream_num, stream)) = self.object_at(hint_offset) else {
            self.report(
                Some(first.0),
                None,
                DefectKind::LinearizedParameterWrong { entry: "/H" },
            );
            return;
        };
        if extents.get(&stream_num.num).map(|(_, len)| *len) != Some(hint_length) {
            self.report(
                Some(stream_num),
                None,
                DefectKind::LinearizedParameterWrong { entry: "/H" },
            );
        }

        self.hint_tables(stream_num, &stream, page_count, &extents, hint_length);
    }

    /// The two tables inside the primary hint stream, against the file.
    fn hint_tables(
        &mut self,
        stream_num: ObjRef,
        stream: &Object,
        page_count: usize,
        extents: &BTreeMap<u32, (u64, u64)>,
        hint_length: u64,
    ) {
        let shared_at = stream
            .as_dict()
            .and_then(|d| d.get_int(self.doc.intern(b"S")))
            .and_then(|v| usize::try_from(v).ok());
        let data = self.doc.stream_decoded(stream_num).ok();
        let (Some(shared_at), Some(data)) = (shared_at, data) else {
            self.report(Some(stream_num), None, DefectKind::HintStreamUnreadable);
            return;
        };
        let Some(tables) = hints::decode(&data, shared_at, page_count) else {
            self.report(Some(stream_num), None, DefectKind::HintStreamUnreadable);
            return;
        };

        // Table F.3 item 2: a byte offset, and the one field that held an
        // object *number* until an outside reader said otherwise. The two are
        // not confusable in either direction — the first page's object number
        // is a small integer and its object sits hundreds of bytes in.
        // F.4: an offset inside a hint table is measured **as though the
        // primary hint stream were not in the file**, because the tables are
        // built before their own length is known. Everything after the stream
        // is therefore short by exactly that length, and a reader that forgets
        // to add it back lands inside the stream it just read.
        let first_page = self.pages.first().copied();
        let declared_offset = first_page
            .and_then(|page| extents.get(&page.num))
            .map(|(at, _)| *at);
        if declared_offset != Some(u64::from(tables.first_page_offset) + hint_length) {
            self.report(
                Some(stream_num),
                None,
                DefectKind::HintValueWrong {
                    entry: "first page offset",
                },
            );
        }

        for index in 0..page_count {
            let Some(page) = tables.pages.get(index) else {
                break;
            };
            // Table F.4 item 1: a page is a run of consecutive object numbers
            // from its page object, so its count is the gap to the next page's
            // number. The *last* page has no next page to measure against, so
            // its run is the one the table declares — and then every number in
            // it has to be an object the file really carries, which is the
            // half a declared count cannot fake.
            let run = match self.page_run(index) {
                Some(run) => {
                    if u64::from(page.objects) != run.len() as u64 {
                        self.report(
                            Some(stream_num),
                            None,
                            DefectKind::HintValueWrong {
                                entry: "page object count",
                            },
                        );
                    }
                    run
                }
                None => {
                    let start = self.pages.get(index).map_or(0, |page| page.num);
                    if page.objects == 0 {
                        self.report(
                            Some(stream_num),
                            None,
                            DefectKind::HintValueWrong {
                                entry: "last page's object count",
                            },
                        );
                    }
                    (start..start.saturating_add(page.objects)).collect()
                }
            };
            for num in &run {
                if !extents.contains_key(num) {
                    self.report(
                        Some(stream_num),
                        None,
                        DefectKind::HintValueWrong {
                            entry: "page's own objects, which no table carries",
                        },
                    );
                }
            }

            // Item 2: and its length is the bytes those objects occupy.
            let measured: u64 = run
                .iter()
                .filter_map(|num| extents.get(num))
                .map(|(_, len)| *len)
                .sum();
            if u64::from(page.length) != measured {
                self.report(
                    Some(stream_num),
                    None,
                    DefectKind::HintValueWrong {
                        entry: "page length",
                    },
                );
            }

            // Items 3 and 4: every identifier names an entry that exists, and
            // never one of the page's own objects.
            for id in &page.shared {
                if *id as usize >= tables.shared_lengths.len() {
                    self.report(
                        Some(stream_num),
                        None,
                        DefectKind::HintValueWrong {
                            entry: "shared identifier",
                        },
                    );
                }
            }
            if index == 0 && !page.shared.is_empty() {
                // F.4.2: page one's own objects *are* the first shared
                // entries, so it names none of them.
                self.report(
                    Some(stream_num),
                    None,
                    DefectKind::HintValueWrong {
                        entry: "first page's shared references",
                    },
                );
            }
        }

        // Table F.5 item 3: part 6's objects are the first shared entries.
        if u64::from(tables.shared_first_page) != self.page_run(0).map_or(0, |run| run.len() as u64)
        {
            self.report(
                Some(stream_num),
                None,
                DefectKind::HintValueWrong {
                    entry: "first page's shared entry count",
                },
            );
        }

        // Items 1 and 2: part 8's first object, and where it is.
        if tables.shared_lengths.len() > tables.shared_first_page as usize {
            let at = extents.get(&tables.first_shared_object).map(|(at, _)| *at);
            if at != Some(u64::from(tables.first_shared_offset) + hint_length) {
                self.report(
                    Some(stream_num),
                    None,
                    DefectKind::HintValueWrong {
                        entry: "first shared object offset",
                    },
                );
            }
        } else if tables.first_shared_object != 0 || tables.first_shared_offset != 0 {
            self.report(
                Some(stream_num),
                None,
                DefectKind::HintValueWrong {
                    entry: "part 8, which is not there",
                },
            );
        }

        // Table F.6 item 1: each entry's group length is the span of the
        // object it describes.
        for (index, length) in tables.shared_lengths.iter().enumerate() {
            let number = self.shared_entry_object(&tables, index);
            let Some(number) = number else {
                continue;
            };
            if extents.get(&number).map(|(_, len)| *len) != Some(u64::from(*length)) {
                self.report(
                    Some(stream_num),
                    None,
                    DefectKind::HintValueWrong {
                        entry: "shared group length",
                    },
                );
            }
        }
    }

    /// Which object a shared-table entry describes (F.4.2).
    ///
    /// The first `shared_first_page` entries are part 6's objects, numbered
    /// consecutively from the first page's; the rest are part 8's, from
    /// `first_shared_object`.
    fn shared_entry_object(&self, tables: &hints::Hints, index: usize) -> Option<u32> {
        let first_page = self.pages.first()?.num;
        let boundary = tables.shared_first_page as usize;
        let index = u32::try_from(index).ok()?;
        if (index as usize) < boundary {
            first_page.checked_add(index)
        } else {
            tables
                .first_shared_object
                .checked_add(index - boundary as u32)
        }
    }

    /// The consecutive run of object numbers page `index` owns (F.3.8).
    fn page_run(&self, index: usize) -> Option<Vec<u32>> {
        let page = self.pages.get(index)?;
        match self.pages.get(index + 1) {
            Some(next) if next.num > page.num => Some((page.num..next.num).collect()),
            // The last page owns everything from its page object to whatever
            // the layout put next, which the page order alone cannot say.
            _ => None,
        }
    }

    /// Where every object begins and how many bytes it occupies.
    ///
    /// An object's span is the gap to whatever the file holds next — the next
    /// object, or the cross-reference section that follows it. Objects are
    /// written back to back, so this is the same number a writer computes and
    /// arrived at from the other side.
    fn extents(&self, sections: &[Section]) -> BTreeMap<u32, (u64, u64)> {
        let mut offsets: BTreeMap<u32, u64> = BTreeMap::new();
        for section in sections {
            for (num, entry) in &section.entries {
                if let RawEntry::Offset { offset, .. } = entry {
                    offsets.entry(*num).or_insert(*offset);
                }
            }
        }

        let mut boundaries: Vec<u64> = offsets.values().copied().collect();
        boundaries.extend(sections.iter().map(|section| section.at));
        boundaries.sort_unstable();

        offsets
            .iter()
            .map(|(num, at)| {
                let next = boundaries
                    .iter()
                    .copied()
                    .find(|other| *other > *at)
                    .unwrap_or(self.buf.len() as u64);
                (*num, (*at, next.saturating_sub(*at)))
            })
            .collect()
    }

    /// The first indirect object in the file, by position rather than by
    /// number (F.2.2 asks for the first one *written*).
    fn first_object(&self) -> Option<(ObjRef, Object)> {
        let mut at = 0usize;
        while at < self.buf.len() {
            if let Some(reference) = header_at(self.buf, at as u64) {
                let mut sink = WarningSink::new();
                let parsed =
                    parse_indirect_at(self.buf, at as u64, self.doc.names_table(), &mut sink)?;
                return Some((reference, parsed.object));
            }
            at += 1;
            // The parameter dictionary is at the top of the file or nowhere:
            // F.2.1 puts it before everything else, so a scan that has walked
            // past the header and a comment line has already failed.
            if at > 64 {
                return None;
            }
        }
        None
    }

    /// The object whose header begins exactly at `offset`.
    fn object_at(&self, offset: u64) -> Option<(ObjRef, Object)> {
        let reference = header_at(self.buf, offset)?;
        let mut sink = WarningSink::new();
        let parsed = parse_indirect_at(self.buf, offset, self.doc.names_table(), &mut sink)?;
        Some((reference, parsed.object))
    }
}

/// Where a classic table's first entry sits: past `xref` and past the
/// subsection header line (7.5.4). Annex F's `/T` names that byte.
fn first_entry_of(buf: &[u8], section: u64) -> u64 {
    let Ok(at) = usize::try_from(section) else {
        return section;
    };
    if !buf.get(at..).is_some_and(|r| r.starts_with(b"xref")) {
        // A cross-reference stream has no entry line to point at, so `/T`
        // names the section itself.
        return section;
    }
    let mut cursor = skip_space(buf, at + 4);
    // Past `first count` and its end-of-line.
    if let Some((_, used)) = ascii_int(buf, cursor) {
        cursor = skip_space(buf, cursor + used);
        if let Some((_, used)) = ascii_int(buf, cursor) {
            cursor = skip_eol(buf, cursor + used);
        }
    }
    cursor as u64
}

/// The `N G obj` header beginning **exactly** at `offset` (7.5.4).
///
/// The reader's own header lookup lexes from that offset, so it finds a header
/// that starts a little later. That is the right leniency for reading the
/// world and the wrong one for judging an offset. Digit runs are
/// length-limited so a megabyte of zeroes cannot be read as an object number.
fn header_at(buf: &[u8], offset: u64) -> Option<ObjRef> {
    let at = usize::try_from(offset).ok()?;
    let rest = buf.get(at..)?;

    let digits = |from: usize| -> Option<(u64, usize)> {
        let mut value = 0u64;
        let mut len = 0usize;
        while let Some(byte) = rest.get(from + len).filter(|b| b.is_ascii_digit()) {
            if len == 20 {
                return None;
            }
            value = value
                .saturating_mul(10)
                .saturating_add(u64::from(byte - b'0'));
            len += 1;
        }
        (len > 0).then_some((value, len))
    };
    let space = |from: usize| -> Option<usize> {
        let mut len = 0usize;
        while rest
            .get(from + len)
            .is_some_and(|b| matches!(*b, b' ' | 13 | 10 | 9 | 0 | 12))
        {
            len += 1;
        }
        (len > 0).then_some(len)
    };

    let (num, used) = digits(0)?;
    let mut cursor = used + space(used)?;
    let (gen, used) = digits(cursor)?;
    cursor += used;
    cursor += space(cursor)?;
    if !rest.get(cursor..)?.starts_with(b"obj") {
        return None;
    }
    Some(ObjRef::new(
        u32::try_from(num).ok()?,
        u16::try_from(gen).ok()?,
    ))
}

/// One twenty-byte classic entry (7.5.4 Table 18), or nothing.
///
/// The clause is exact about this — ten digits, a space, five digits, a space,
/// the keyword, and a two-byte end-of-line — and the reader deliberately
/// resynchronises on the grammar instead, so nineteen- and twenty-one-byte
/// entries open fine and are never reported. Here the length is the rule.
fn twenty_byte_entry(row: &[u8]) -> Option<RawEntry> {
    if row.len() != 20 {
        return None;
    }
    let offset = row.get(..10)?;
    let gen = row.get(11..16)?;
    if !offset.iter().all(u8::is_ascii_digit) || !gen.iter().all(u8::is_ascii_digit) {
        return None;
    }
    if row.get(10) != Some(&b' ') || row.get(16) != Some(&b' ') {
        return None;
    }
    if !matches!(row.get(18..20)?, b" \r" | b" \n" | b"\r\n") {
        return None;
    }
    let offset: u64 = core::str::from_utf8(offset).ok()?.parse().ok()?;
    let gen: u32 = core::str::from_utf8(gen).ok()?.parse().ok()?;
    let gen = u16::try_from(gen).ok()?;
    match row.get(17) {
        Some(b'n') => Some(RawEntry::Offset { offset, gen }),
        Some(b'f') => Some(RawEntry::Free {
            next: u32::try_from(offset).ok()?,
            gen,
        }),
        _ => None,
    }
}

/// A big-endian field of `width` bytes at `offset`, as 7.5.8.2 packs them.
fn be(row: &[u8], offset: usize, width: usize) -> u64 {
    let mut value = 0u64;
    for index in 0..width {
        let byte = row.get(offset + index).copied().unwrap_or(0);
        value = (value << 8) | u64::from(byte);
    }
    value
}

/// A non-negative integer, or nothing.
fn nonnegative(value: Option<i64>) -> Option<u64> {
    value.and_then(|v| u64::try_from(v).ok())
}

/// One ASCII integer at `at`, and how many bytes it took.
fn ascii_int(buf: &[u8], at: usize) -> Option<(u64, usize)> {
    let mut value = 0u64;
    let mut len = 0usize;
    while let Some(byte) = buf.get(at + len).filter(|b| b.is_ascii_digit()) {
        value = value
            .saturating_mul(10)
            .saturating_add(u64::from(byte - b'0'));
        len += 1;
        if len > 20 {
            return None;
        }
    }
    (len > 0).then_some((value, len))
}

/// Past any run of whitespace.
fn skip_space(buf: &[u8], at: usize) -> usize {
    let mut at = at;
    while buf
        .get(at)
        .is_some_and(|b| matches!(*b, b' ' | 13 | 10 | 9 | 0 | 12))
    {
        at += 1;
    }
    at
}

/// Past one end-of-line, and any spaces before it.
fn skip_eol(buf: &[u8], at: usize) -> usize {
    let mut at = at;
    while buf.get(at) == Some(&b' ') {
        at += 1;
    }
    if buf.get(at) == Some(&b'\r') {
        at += 1;
    }
    if buf.get(at) == Some(&b'\n') {
        at += 1;
    }
    at
}

/// Every whitespace-separated non-negative integer in a prologue.
///
/// Deliberately not the lexer: 7.5.7's prologue is pairs of plain integers,
/// and a container whose prologue holds anything else is one this rule should
/// notice rather than one a general parser should rescue.
fn integers(bytes: &[u8]) -> Vec<i64> {
    let mut out = Vec::new();
    let mut current: Option<i64> = None;
    for byte in bytes {
        if byte.is_ascii_digit() {
            let digit = i64::from(byte - b'0');
            current = Some(
                current
                    .unwrap_or(0)
                    .saturating_mul(10)
                    .saturating_add(digit),
            );
            continue;
        }
        if let Some(value) = current.take() {
            out.push(value);
        }
        // Anything that is neither a digit nor whitespace ends the prologue's
        // grammar; the caller sees a short list and says so.
        if !matches!(*byte, b' ' | 13 | 10 | 9 | 0 | 12) {
            return out;
        }
    }
    out.extend(current);
    out
}

/// The first occurrence of `needle`.
fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || needle.len() > hay.len() {
        return None;
    }
    (0..=hay.len() - needle.len()).find(|&at| hay.get(at..at + needle.len()) == Some(needle))
}

/// The last occurrence of `needle`.
fn rfind(hay: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || needle.len() > hay.len() {
        return None;
    }
    (0..=hay.len() - needle.len())
        .rev()
        .find(|&at| hay.get(at..at + needle.len()) == Some(needle))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The module may not reach for the readers whose defaults it exists to
    /// catch.
    ///
    /// `bounds_ledger.rs` bans `Instant::now` from itself for the same reason:
    /// a rule that quietly grows a dependency on the lenient path stops being
    /// a rule, and the failure is silent — every fixture still passes. So the
    /// source is read and the imports are the assertion.
    ///
    /// `parse` and `warn` are deliberately allowed. `parse_object_at` performs
    /// no repair on its own: it *reports* one, and every report it makes is a
    /// defect here.
    #[test]
    fn nothing_here_reads_through_a_tolerant_reader() {
        let source = include_str!("validate.rs");
        for banned in [
            "use crate::pages",
            "use crate::dest",
            "use crate::font",
            "use crate::form",
            "use crate::outline",
            "use crate::linearize",
            "use crate::objstm",
            "use crate::repair",
            "use crate::trees",
            "use crate::xref",
        ] {
            // Anchored to the start of a line, or this very list would be
            // the match that failed the test.
            let imported = source
                .lines()
                .any(|line| line.trim_start().starts_with(banned));
            assert!(
                !imported,
                "`{banned}` would put a reader that supplies defaults inside the \
                 validator that exists to catch them"
            );
        }
    }

    #[test]
    fn a_header_must_begin_at_the_offset_itself() {
        let buf = b"%PDF-1.7\n12 0 obj\n";
        assert_eq!(header_at(buf, 9), Some(ObjRef::new(12, 0)));
        // One byte early is the whole point: the reader's own lookup lexes
        // forward from here and finds the header anyway.
        assert_eq!(header_at(buf, 8), None);
        assert_eq!(header_at(buf, 0), None);
        assert_eq!(header_at(buf, 9999), None);
    }

    #[test]
    fn a_header_carries_its_generation() {
        assert_eq!(header_at(b"7 65535 obj", 0), Some(ObjRef::new(7, 65535)));
        assert_eq!(header_at(b"7 65536 obj", 0), None, "a generation is u16");
        assert_eq!(header_at(b"7 0 objx", 0), Some(ObjRef::new(7, 0)));
        assert_eq!(header_at(b"7 0 ojb", 0), None);
        assert_eq!(header_at(b"7 0", 0), None);
    }

    #[test]
    fn an_entry_is_exactly_twenty_bytes() {
        assert_eq!(
            twenty_byte_entry(b"0000000015 00000 n \n"),
            Some(RawEntry::Offset { offset: 15, gen: 0 })
        );
        assert_eq!(
            twenty_byte_entry(b"0000000000 65535 f \n"),
            Some(RawEntry::Free {
                next: 0,
                gen: 65535
            })
        );
        assert_eq!(
            twenty_byte_entry(b"0000000015 00000 n\r\n"),
            Some(RawEntry::Offset { offset: 15, gen: 0 }),
            "7.5.4 allows CR LF as the two-byte end"
        );
        assert_eq!(twenty_byte_entry(b"0000000015 00000 n\n"), None, "nineteen");
        assert_eq!(twenty_byte_entry(b"000000001s 00000 n \n"), None);
        assert_eq!(twenty_byte_entry(b"0000000015 00000 x \n"), None);
        assert_eq!(twenty_byte_entry(b"0000000015_00000 n \n"), None);
    }

    #[test]
    fn a_prologue_reads_pairs_and_stops_at_anything_else() {
        assert_eq!(integers(b"1 0 2 37 3 74"), vec![1, 0, 2, 37, 3, 74]);
        assert_eq!(integers(b"1 0 2 /Name"), vec![1, 0, 2]);
        assert_eq!(integers(b""), Vec::<i64>::new());
    }

    #[test]
    fn a_field_is_big_endian() {
        assert_eq!(be(&[0x01, 0x02, 0x03], 0, 3), 0x01_02_03);
        assert_eq!(be(&[0x01, 0x02, 0x03], 1, 2), 0x02_03);
        assert_eq!(be(&[0x01], 0, 0), 0);
        assert_eq!(
            be(&[], 0, 4),
            0,
            "a short row reads as zeroes, never panics"
        );
    }
}
