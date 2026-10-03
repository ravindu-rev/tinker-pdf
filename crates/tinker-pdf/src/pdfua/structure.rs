//! The structure group: what ISO 14289 asks of the logical structure tree
//! that a walk over the tree decides.
//!
//! These are the rules `crates/tinker-pdf/tests/pdfua.rs` ran as a census
//! before the validator existed, moved here unchanged in what they decide
//! (milestone 1 of `docs/design/pdfua.md`) and numbered by the claimed part.
//!
//! The tree is read once, by [`crate::structure::bind`] — the reader
//! `Document::structure` returns and the PDF/A level A group uses — so this
//! validator and `Document::structure` cannot disagree about what a file's
//! tagging says.

use super::{clauses, UaPart, UaRaw};
use crate::pdfa::{FindingKind, Machinery, RuleGroup};
use crate::structure::{StructElement, StructKid, StructureTree, StructureWarning};
use crate::Document;

/// How many findings of one kind one document contributes.
///
/// The structure reader bounds its own walk; this bounds the finding list a
/// hostile tree can produce, which is a different thing. A file with a
/// quarter of a million undescribed figures has one defect, and a verdict
/// carrying a quarter of a million copies of it is not provenance (ruling
/// 10).
pub(super) const MAX_FINDINGS_PER_RULE: usize = 64;

/// Runs the structure group.
pub(super) fn rules(
    document: &Document,
    machinery: &Machinery,
    part: UaPart,
    out: &mut Vec<UaRaw>,
) {
    if !machinery.reach(RuleGroup::Structure) {
        return;
    }
    let Some(tree) = crate::structure::bind(&document.inner) else {
        out.push(UaRaw::file(
            clauses::STRUCTURE_HIERARCHY,
            FindingKind::StructureTreeMissing,
        ));
        return;
    };
    if !tree.marked {
        out.push(UaRaw::file(clauses::MARKED, FindingKind::NotMarkedAsTagged));
    }
    if tree.suspects {
        out.push(UaRaw::file(clauses::SUSPECTS, FindingKind::MarkedSuspects));
    }
    // A dropped attribute value or namespace is not a walk that could not be
    // completed: the element and everything under it were read. Excluded by
    // name, as the census excluded them, so this rule still decides what it
    // was measured deciding.
    if tree.warnings.iter().any(|warning| {
        !matches!(
            warning,
            StructureWarning::AttributeIgnored { .. } | StructureWarning::NamespaceIgnored { .. }
        )
    }) {
        out.push(UaRaw::file(
            clauses::STRUCTURE_HIERARCHY,
            FindingKind::StructureTreeUnwalkable,
        ));
    }
    figures(&tree, out);
    if part == UaPart::One {
        headings(&tree, out);
    }
    if !states_a_language(document, &tree) {
        out.push(UaRaw::file(
            clauses::NATURAL_LANGUAGE,
            FindingKind::NaturalLanguageMissing,
        ));
    }
}

/// ISO 32000-1 14.9.3, as ISO 14289-1 7.3 requires it: a `Figure` stands for
/// content that is not text, so something has to say what it is.
/// `/ActualText` counts as well as `/Alt` — a figure that *is* a word, which
/// is what a dropped capital is, says so with 14.9.4.
fn figures(tree: &StructureTree, out: &mut Vec<UaRaw>) {
    let mut reported = 0usize;
    for element in tree.elements() {
        if reported >= MAX_FINDINGS_PER_RULE {
            break;
        }
        if element.standard_type == "Figure"
            && element.alt.is_none()
            && element.actual_text.is_none()
        {
            reported += 1;
            out.push(UaRaw {
                rule: clauses::FIGURE_ALTERNATIVE,
                object: element.reference,
                kind: FindingKind::AlternativeDescriptionMissing {
                    structure_type: element.standard_type.clone(),
                },
            });
        }
    }
}

/// ISO 14289-1 7.4.2: heading levels descend one at a time, and the first is
/// `H1`.
///
/// Measured over the headings in reading order (14.8) whatever contains them,
/// because the outline a reader hears is that sequence: `7.4.2-t01-pass-d`
/// puts its `H1`, `H2` and `H3` in three sibling `Sect`s, one each, and a
/// rule that started each `Sect` from nothing reported a conforming file.
fn headings(tree: &StructureTree, out: &mut Vec<UaRaw>) {
    let mut previous = 0u8;
    let mut reported = 0usize;
    visit_headings(&tree.kids, &mut |element, level| {
        if level > previous.saturating_add(1) && reported < MAX_FINDINGS_PER_RULE {
            reported += 1;
            out.push(UaRaw {
                rule: clauses::HEADING_LEVELS,
                object: element.reference,
                kind: FindingKind::HeadingLevelSkipped { previous, level },
            });
        }
        previous = level;
    });
}

/// Every `Hn` in the tree, in reading order, to a visitor.
fn visit_headings(kids: &[StructKid], visit: &mut impl FnMut(&StructElement, u8)) {
    for kid in kids {
        let StructKid::Element(element) = kid else {
            continue;
        };
        if let Some(level) = heading_level(element) {
            visit(element, level);
        }
        visit_headings(&element.kids, visit);
    }
}

/// `H1`…`H6` (ISO 32000-1 Table 335), and PDF 2.0's unbounded `Hn`.
///
/// Two digits at most: `H99` is already past anything a document means, and
/// an unbounded parse would let `H4294967296` decide the answer.
pub(super) fn heading_level(element: &StructElement) -> Option<u8> {
    let rest = element.standard_type.strip_prefix('H')?;
    if rest.is_empty() || rest.len() > 2 || !rest.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let level: u8 = rest.parse().ok()?;
    (level > 0).then_some(level)
}

/// Whether the document says what language it is in, anywhere.
///
/// ISO 14289-1 7.2 wants the natural language determinable for all text,
/// which in general is per-item and needs the marked-content scopes a page
/// draws inside. What is decidable from the tree is the weakest form of the
/// clause — a document that states no language at all, anywhere, cannot have
/// stated one for its text — and it is deliberately the loosest reading that
/// is still a rule: it caught the census's fixtures that say nothing and
/// abstained on every document that says something.
fn states_a_language(document: &Document, tree: &StructureTree) -> bool {
    let doc = &document.inner;
    if let Some(catalog) = doc.catalog() {
        if !doc.resolve_key(&catalog, doc.intern(b"Lang")).is_null() {
            return true;
        }
    }
    tree.elements().iter().any(|element| element.lang.is_some())
}
