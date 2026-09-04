//! The strict structural validator, reported under ISO 19005's own clauses.
//!
//! # The defect this group exists for
//!
//! A rule engine that reads the object graph cannot see a defect that exists
//! only in the bytes. By the time an object exists to apply a rule to, the
//! lexer has normalised a hexadecimal string, the parser has consumed the
//! end-of-line markers around `obj`, and the reader has merged every
//! revision's cross-reference table into one — so ISO 19005's file-structure
//! clauses were staged, not because they are hard, but because this group had
//! already lost the evidence by the time it ran.
//!
//! `tinker_pdf_cos::validate` has not. It reads the file again with the
//! leniency ladder **off**, walks the cross-reference *sections* out of the
//! bytes one `/Prev` link at a time, and holds every entry to the byte it
//! names. Joining the two is what closes that family, and it is the join
//! `docs/features/pdfa.md` already called "the honest way".
//!
//! # What is mapped, and what deliberately is not
//!
//! Only the **structure** tier: `Tier::Semantics` is what the document says
//! rather than how the file is laid out, and a rewrite inherits it from its
//! source, so it belongs to the document rather than to a conformance claim.
//!
//! Inside that tier, three families are **not** mapped, and each absence is a
//! decision rather than an oversight:
//!
//! - **The repairs** — `Repaired`, `RepairedWhileReading`, `ObjectRepaired`,
//!   `OpenedLenient`. A leniency is a statement about this reader, not about
//!   the file: ISO 19005 requires the file to be right, and where it is not,
//!   the underlying defect has a rule of its own below. Reporting the repair
//!   as well would report one defect twice and would make the finding list a
//!   function of how forgiving this build happens to be.
//! - **Linearization** — `LinearizedParameterWrong`, `HintStreamUnreadable`,
//!   `HintValueWrong`. ISO 19005 requires no linearization at all, so a wrong
//!   hint table is a defect in the file and conformance has nothing to say
//!   about it.
//! - **The header and the trailer** — `HeaderMissing`,
//!   `HeaderVersionUnreadable`, `BinaryCommentMissing`, `EofMissing`,
//!   `BytesAfterEof`, `TrailerRootMissing`, `TrailerSizeMissing`,
//!   `EncryptWithoutId`, `IdMalformed`, `SizeTooSmall`. The syntax group
//!   already has rules for 6.1.2 and 6.1.3 — `HeaderCommentMissing`,
//!   `HeaderNotAtStart`, `FileIdentifierMalformed` and the rest — and it can,
//!   because a header and a trailer survive the parse as things a rule can
//!   still ask about. Mapping them here as well reports one defect twice under
//!   one clause, which the first draft did: five fixtures came back with two
//!   findings apiece, and the count is what said so.
//!
//! What is left is exactly the class the staged rules named: **the
//! cross-reference table's own spelling, the framing of an indirect object,
//! and a stream's extent**. Those are the three the object graph cannot see,
//! and they are the three this group is for.
//!
//! # The cost, and why this is a group of its own
//!
//! Running these rules means **parsing the file a second time**, with the
//! ladder off. That is the most expensive thing in this validator after the
//! font group, and the design doc's laziness requirement is that a syntax-only
//! sweep over the corpus never builds machinery it did not ask for. So this is
//! a separate flag on [`super::Coverage`] rather than part of `syntax`, and
//! `Coverage::SYNTAX` leaves it off.

use tinker_pdf_cos::{validate, CosDocument, DefectKind, Tier};

use super::{clauses, ClauseTable, FindingKind, Flavour, Machinery, Raw, RuleGroup};

/// Reports the strict validator's structural findings under ISO 19005 clauses.
pub(super) fn rules(
    doc: &CosDocument,
    machinery: &Machinery,
    _flavour: Option<Flavour>,
    out: &mut Vec<Raw>,
) {
    if !machinery.reach(RuleGroup::Structure) {
        return;
    }
    for defect in validate(doc) {
        if defect.kind.tier() != Tier::Structure {
            continue;
        }
        let Some(rule) = clause_of(defect.kind) else {
            continue;
        };
        out.push(Raw {
            rule,
            object: defect.object,
            kind: FindingKind::Structural { defect: defect.kind },
        });
    }
}

/// Which ISO 19005 clause a structural defect belongs to, or `None` where the
/// standard requires nothing about it.
///
/// The match is exhaustive on purpose. `DefectKind` is a closed set for the
/// reason `WarningKind` is — a new rule is a deliberate change to what "valid"
/// means — and this function is where a new one has to be given a clause or
/// explicitly refused one, rather than falling silently into either.
fn clause_of(kind: DefectKind) -> Option<ClauseTable> {
    match kind {
        // 6.1.4: the cross-reference table's own syntax — the subsection
        // headers, the twenty-byte entries, the `trailer` keyword after a
        // classic table, and the free list's own head. This is the family the
        // rule engine could not see at all, because the reader merges every
        // revision's table into one before a rule could ask how any of them
        // was spelled.
        DefectKind::StartxrefMissing
        | DefectKind::SectionUnreadable
        | DefectKind::PrevCycle
        | DefectKind::SubsectionMalformed
        | DefectKind::EntryNotTwentyBytes
        | DefectKind::TableTrailerMissing
        | DefectKind::XrefStreamWidthsBad
        | DefectKind::XrefStreamIndexBad
        | DefectKind::XrefStreamRowsWrong { .. }
        | DefectKind::XrefStreamTypeUnknown { .. }
        | DefectKind::FreeNextNotFree { .. } => Some(clauses::CROSS_REFERENCE),

        // Indirect objects: 6.1.8 in part 1, 6.1.9 in parts 2 and 3, 6.1.8 in
        // part 4. An object stream is a container of indirect objects, so its
        // own framing is the same clause; that part 1 forbids object streams
        // outright is a different rule, in the syntax group.
        DefectKind::ObjectHeaderAbsent
        | DefectKind::ObjectHeaderMismatch { .. }
        | DefectKind::ObjStmNotAStream { .. }
        | DefectKind::ObjStmHeaderMissing { .. }
        | DefectKind::ObjStmIndexOutOfRange { .. }
        | DefectKind::ObjStmNumberMismatch { .. }
        | DefectKind::ObjStmOffsetOutOfRange { .. } => Some(clauses::INDIRECT_OBJECTS),

        // Stream objects: `/Length` against where `endstream` actually is.
        DefectKind::StreamLengthUnresolved
        | DefectKind::StreamLengthNotExact { .. } => Some(clauses::STREAM_OBJECTS),

        // Measured against the corpus and refused a clause, each for a reason
        // of its own. Between them the first two account for 48 of the 52
        // false positives the first draft of this group added, and the whole
        // discipline of the PDF/A work is that a join closes by adding rules
        // and fails by adding false positives.
        //
        // `StreamDoesNotDecode` is a statement about **this reader**: the
        // strict tier decodes with the leniency ladder off, and a stream that
        // needs a repair is a fact about how forgiving a decoder has to be
        // rather than about what ISO 19005 requires of the file.
        //
        // `FreeHeadMissing` and `FreeHeadGeneration` are 7.5.4's free-list
        // head, which is a **classic table's** concept. A file whose
        // cross-references are a stream has no twenty-byte entry to put it in,
        // and parts 2 to 4 permit cross-reference streams, so the rule cannot
        // be an ISO 19005 finding without knowing which spelling the file used
        // -- which this defect does not carry.
        //
        // `EntryPastSize` is 7.5.5's instruction to a *reader*: an entry at or
        // above `/Size` is one a conforming reader ignores. The file is still
        // readable and the corpus annotates two such files `pass`.
        //
        // `StartxrefNotASection` fires on one `pass` file, and one is not
        // enough to tell a defect in that file from a reading of Annex F this
        // build takes and the suite does not. Named here rather than mapped on
        // a sample of one.
        DefectKind::StreamDoesNotDecode
        | DefectKind::FreeHeadMissing
        | DefectKind::FreeHeadGeneration { .. }
        | DefectKind::EntryPastSize { .. }
        | DefectKind::StartxrefNotASection => None,

        // The header and the trailer, refused a clause here because the
        // syntax group already has rules for 6.1.2 and 6.1.3 and reports them
        // against the same clause. Two findings for one defect is worse than
        // one from the group that can say more about it.
        DefectKind::HeaderMissing
        | DefectKind::HeaderVersionUnreadable
        | DefectKind::BinaryCommentMissing
        | DefectKind::EofMissing
        | DefectKind::BytesAfterEof { .. }
        | DefectKind::TrailerRootMissing
        | DefectKind::TrailerSizeMissing
        | DefectKind::EncryptWithoutId
        | DefectKind::IdMalformed
        | DefectKind::SizeTooSmall { .. } => None,

        // The repairs, refused a clause: see this module's header.
        DefectKind::OpenedLenient(_)
        | DefectKind::Repaired(_)
        | DefectKind::RepairedWhileReading(_)
        | DefectKind::ObjectRepaired(_) => None,

        // Linearization, refused a clause: ISO 19005 requires none of it.
        DefectKind::LinearizedParameterWrong { .. }
        | DefectKind::HintStreamUnreadable
        | DefectKind::HintValueWrong { .. } => None,

        // Everything else is `Tier::Semantics` and never reaches here. Named
        // rather than wildcarded, so a kind that changes tier is a compile
        // error in this file rather than a finding that quietly stops being
        // reported.
        DefectKind::RootNotACatalog
        | DefectKind::PageNodeUntyped
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
        | DefectKind::XObjectMalformed { .. } => None,
    }
}
