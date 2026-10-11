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

use std::collections::BTreeMap;

use tinker_pdf_cos::{decode_text_string, ObjRef, STANDARD_STRUCTURE_TYPES};

use super::{clauses, UaPart, UaRaw};
use crate::pdfa::logical::{language_is_well_formed, LanguageGrammar};
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
    // The catalog's `/Lang` is read whether or not there is a tree: a file
    // with no structure at all still states, or fails to state, a language.
    catalog_language(document, part, out);
    let Some(tree) = crate::structure::bind(&document.inner) else {
        out.push(UaRaw::file(
            clauses::STRUCTURE_HIERARCHY,
            FindingKind::StructureTreeMissing,
        ));
        if part == UaPart::One {
            super::annotations::rules(&document.inner, None, out);
        }
        return;
    };
    if part == UaPart::One {
        super::annotations::rules(&document.inner, Some(&tree), out);
    }
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
    figures(&tree, part, out);
    if part == UaPart::One {
        headings(&tree, out);
    }
    if !states_a_language(document, &tree) {
        out.push(UaRaw::file(
            clauses::NATURAL_LANGUAGE,
            FindingKind::NaturalLanguageMissing,
        ));
    }
    element_languages(&tree, out);
    if part == UaPart::One {
        structure_types(&tree, out);
        grammar(&tree, out);
    }
    parents(document, &tree, out);
}

/// ISO 14289-1 7.2-29 and ISO 14289-2 8.4.4-2 over the catalog: "If the Lang
/// entry is present in the document's Catalog dictionary … its value shall be
/// a language identifier", RFC 3066's and never empty
/// ([`LanguageGrammar::PDF_UA`]). Part 2 also requires the entry itself
/// (8.4.4-1: "specified using the Lang entry, with a non-empty value, in the
/// catalog dictionary").
fn catalog_language(document: &Document, part: UaPart, out: &mut Vec<UaRaw>) {
    let doc = &document.inner;
    let Some(catalog) = doc.catalog() else {
        return;
    };
    let value = doc.resolve_key(&catalog, doc.intern(b"Lang"));
    let declared = value
        .as_string()
        .map(|string| decode_text_string(&string.bytes));
    if part == UaPart::Two && declared.as_deref().is_none_or(str::is_empty) {
        out.push(UaRaw::file(
            clauses::CATALOG_LANGUAGE,
            FindingKind::CatalogLanguageMissing,
        ));
        // An empty value is the finding just made; reporting it a second
        // time as malformed would be one defect told twice.
        return;
    }
    if let Some(declared) = declared {
        if !language_is_well_formed(&declared, LanguageGrammar::PDF_UA) {
            out.push(UaRaw::file(
                clauses::LANGUAGE_IDENTIFIER,
                FindingKind::LanguageMalformed { declared },
            ));
        }
    }
}

/// The same rule over every structure element's `/Lang`. A `/Lang` in a
/// marked-content property list is the content walk's, and staged.
fn element_languages(tree: &StructureTree, out: &mut Vec<UaRaw>) {
    let mut reported = 0usize;
    for element in tree.elements() {
        if reported >= MAX_FINDINGS_PER_RULE {
            break;
        }
        let Some(declared) = element.lang.as_deref() else {
            continue;
        };
        if !language_is_well_formed(declared, LanguageGrammar::PDF_UA) {
            reported += 1;
            out.push(UaRaw {
                rule: clauses::LANGUAGE_IDENTIFIER,
                object: element.reference,
                kind: FindingKind::LanguageMalformed {
                    declared: declared.to_string(),
                },
            });
        }
    }
}

/// ISO 14289-1 7.1-5 and 7.1-7: every structure type resolves through the
/// `/RoleMap` to one of ISO 32000-1 14.8.4's standard types, and a standard
/// type is not remapped.
///
/// The first half is the PDF/A level A rule (`pdfa::logical`), run here and
/// re-numbered: the two clauses say the same sentence about the same table,
/// and one function is what keeps them saying it alike. The second half is
/// new — ISO 19005 does not forbid remapping a standard type — and reads what
/// the structure reader already resolved: an element whose own `/S` is
/// standard and whose resolved type is something else was remapped. An
/// identity entry (`/P /P`) resolves to itself and is not reported, which is
/// the reader's reading and the conservative one; an element in a PDF 2.0
/// namespace is part 2's question, and part 2's rules are not this one.
fn structure_types(tree: &StructureTree, out: &mut Vec<UaRaw>) {
    let mut shared = Vec::new();
    crate::pdfa::logical::structure_types(tree, &mut shared);
    for raw in shared {
        out.push(UaRaw {
            rule: clauses::STRUCTURE_TYPES,
            object: raw.object,
            kind: raw.kind,
        });
    }
    let mut reported = 0usize;
    for element in tree.elements() {
        if reported >= MAX_FINDINGS_PER_RULE {
            break;
        }
        if element.namespace.is_some()
            || !STANDARD_STRUCTURE_TYPES.contains(&element.raw_type.as_str())
            || element.standard_type == element.raw_type
        {
            continue;
        }
        reported += 1;
        out.push(UaRaw {
            rule: clauses::STRUCTURE_TYPES,
            object: element.reference,
            kind: FindingKind::StandardTypeRemapped {
                declared: element.raw_type.clone(),
                mapped: element.standard_type.clone(),
            },
        });
    }
}

/// ISO 14289-1 7.1-12, ISO 14289-2 8.2.1-2: "A structure element dictionary
/// shall contain the P (parent) entry". Read from the element's own
/// dictionary, since the structure reader reaches an element from its parent
/// and records no `/P`; an element written in place rather than as an
/// indirect object has no dictionary of its own to ask, and is not judged.
fn parents(document: &Document, tree: &StructureTree, out: &mut Vec<UaRaw>) {
    let doc = &document.inner;
    let key = doc.intern(b"P");
    let mut reported = 0usize;
    for element in tree.elements() {
        if reported >= MAX_FINDINGS_PER_RULE {
            break;
        }
        let Some(reference) = element.reference else {
            continue;
        };
        let Ok(object) = doc.get(reference) else {
            continue;
        };
        let Some(dict) = object.as_dict() else {
            continue;
        };
        if dict.get(key).is_none() {
            reported += 1;
            out.push(UaRaw {
                rule: clauses::STRUCTURE_PARENT,
                object: Some(reference),
                kind: FindingKind::StructureParentMissing,
            });
        }
    }
}

/// ISO 32000-1 14.9.3, as ISO 14289-1 7.3 requires it: a `Figure` stands for
/// content that is not text, so something has to say what it is.
/// `/ActualText` counts as well as `/Alt` — a figure that *is* a word, which
/// is what a dropped capital is, says so with 14.9.4. And 7.7 asks the same
/// of a `Formula` (part 1; part 2's profile states no such rule).
///
/// **An empty `/Alt` is no description under part 1, and an empty
/// `/ActualText` is a replacement** — the corpus's reading, which the design
/// records (7.3-t01-pass-c against -fail-b, 7.7-t01-pass-c against -fail-b)
/// and veraPDF's conditions state for both rules: `(Alt != null && Alt != '')
/// || ActualText != null`. Part 2's 8.2.5.28.2 condition is `Alt != null ||
/// ActualText != null`, so an empty `/Alt` stands there. The first version
/// of this rule read both keys alike under both parts, which passed the
/// part 1 fixture whose `/Alt` is empty.
fn figures(tree: &StructureTree, part: UaPart, out: &mut Vec<UaRaw>) {
    let mut reported = 0usize;
    for element in tree.elements() {
        if reported >= MAX_FINDINGS_PER_RULE {
            break;
        }
        let rule = match (element.standard_type.as_str(), part) {
            ("Figure", _) => clauses::FIGURE_ALTERNATIVE,
            ("Formula", UaPart::One) => clauses::FORMULA_ALTERNATIVE,
            _ => continue,
        };
        let described = match part {
            UaPart::One => element.alt.as_deref().is_some_and(|alt| !alt.is_empty()),
            UaPart::Two => element.alt.is_some(),
        };
        if !described && element.actual_text.is_none() {
            reported += 1;
            out.push(UaRaw {
                rule,
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

// ---- 7.2, 7.4.4, 7.9: the structure grammar --------------------------------

/// Which parents an element of a standard type may sit in, where ISO 32000-1
/// 14.8.4 constrains it: veraPDF's statements of ISO 14289-1 rules 7.2-4 to
/// 7.2-9, 7.2-17, 7.2-18 and 7.2-26 ("TR element should be contained in
/// Table, THead, TBody or TFoot element", and the rest).
const PARENTS: &[(&str, &[&str])] = &[
    ("TR", &["Table", "THead", "TBody", "TFoot"]),
    ("THead", &["Table"]),
    ("TBody", &["Table"]),
    ("TFoot", &["Table"]),
    ("TH", &["TR"]),
    ("TD", &["TR"]),
    ("LI", &["L"]),
    ("LBody", &["LI"]),
    ("TOCI", &["TOC"]),
];

/// Which kids an element of a standard type may have, where 14.8.4
/// constrains it: rules 7.2-3, 7.2-10, 7.2-19, 7.2-20, 7.2-27 and 7.2-36 to
/// 7.2-38 ("Table element may contain only TR, THead, TBody, TFoot and
/// Caption elements", and the rest).
const KIDS: &[(&str, &[&str])] = &[
    ("Table", &["TR", "THead", "TBody", "TFoot", "Caption"]),
    ("TR", &["TH", "TD"]),
    ("THead", &["TR"]),
    ("TBody", &["TR"]),
    ("TFoot", &["TR"]),
    ("L", &["L", "LI", "Caption"]),
    ("LI", &["Lbl", "LBody"]),
    ("TOC", &["TOC", "TOCI", "Caption"]),
];

/// The structure tree root, as a parent's name in a finding: it has no
/// structure type, and 14.8.4 admits no constrained element directly under it.
const ROOT: &str = "StructTreeRoot";

/// ISO 14289-1 7.2's content models, 7.4.4's heading kinds and 7.9's note
/// identifiers: every rule veraPDF's PDF/UA-1 profile states over an
/// element's standard type, its parent's and its kids' — read as data, never
/// run (ruling 13), and each held to a fixture and twin in `pdfua_rules.rs`.
///
/// **Kids are structure elements whose standard type is one of 14.8.4's.**
/// A marked-content or object reference is content, not a kid with a type,
/// and the profile's `kidsStandardTypes` lists types; a kid whose type
/// resolves to nothing standard is the structure-type rule's finding
/// already, and judging it here as well would say one defect twice.
///
/// One finding per element per rule, the first offending kid named: a table
/// with forty `P` kids has one defect of shape.
///
/// What is **not** here is the half of 7.2 that needs the table's grid —
/// cells that intersect, rows and columns that disagree across `/RowSpan`
/// and `/ColSpan` (7.2-15, 7.2-41 to 7.2-43) — which `super::STAGED` names.
fn grammar(tree: &StructureTree, out: &mut Vec<UaRaw>) {
    let mut reported = 0usize;
    let mut headings = (None::<Option<ObjRef>>, None::<Option<ObjRef>>);
    let mut notes: BTreeMap<Vec<u8>, usize> = BTreeMap::new();
    let mut note_findings: Vec<UaRaw> = Vec::new();
    visit_with_parents(&tree.kids, ROOT, &mut |element, parent| {
        let own = element.standard_type.as_str();
        let at = element.reference;

        if let Some((_, admitted)) = PARENTS.iter().find(|(t, _)| *t == own) {
            if !admitted.contains(&parent) {
                push(
                    UaRaw {
                        rule: clauses::STRUCTURE_GRAMMAR,
                        object: at,
                        kind: FindingKind::StructureParentNotAdmitted {
                            element: own.to_string(),
                            parent: parent.to_string(),
                        },
                    },
                    &mut reported,
                    out,
                );
            }
        }

        let kids: Vec<&str> = element
            .kids
            .iter()
            .filter_map(|kid| match kid {
                StructKid::Element(kid) => Some(kid.standard_type.as_str()),
                _ => None,
            })
            .filter(|kid| STANDARD_STRUCTURE_TYPES.contains(kid))
            .collect();
        let count = |name: &str| kids.iter().filter(|kid| **kid == name).count();

        if let Some((_, admitted)) = KIDS.iter().find(|(t, _)| *t == own) {
            if let Some(kid) = kids.iter().find(|kid| !admitted.contains(kid)) {
                push(
                    UaRaw {
                        rule: clauses::STRUCTURE_GRAMMAR,
                        object: at,
                        kind: FindingKind::StructureKidNotAdmitted {
                            element: own.to_string(),
                            kid: (*kid).to_string(),
                        },
                    },
                    &mut reported,
                    out,
                );
            }
        }

        // 7.2-11, 7.2-12, 7.2-39: at most one THead, one TFoot, one Caption
        // in a Table. 7.4.4-1: "Each node in the tag tree shall contain at
        // most one child H tag" — every element, not only a table.
        let mut limits: Vec<(&str, ClausesFor)> = vec![("H", ClausesFor::Headings)];
        if own == "Table" {
            limits.extend([
                ("THead", ClausesFor::Grammar),
                ("TFoot", ClausesFor::Grammar),
                ("Caption", ClausesFor::Grammar),
            ]);
        }
        for (kid, clause) in limits {
            let n = count(kid);
            if n > 1 {
                push(
                    UaRaw {
                        rule: clause.table(),
                        object: at,
                        kind: FindingKind::StructureKidRepeated {
                            element: own.to_string(),
                            kid: kid.to_string(),
                            count: u32::try_from(n).unwrap_or(u32::MAX),
                        },
                    },
                    &mut reported,
                    out,
                );
            }
        }

        if own == "Table" {
            // 7.2-13, 7.2-14: a THead or a TFoot needs a TBody beside it.
            for beside in ["THead", "TFoot"] {
                if count(beside) > 0 && count("TBody") == 0 {
                    push(
                        UaRaw {
                            rule: clauses::STRUCTURE_GRAMMAR,
                            object: at,
                            kind: FindingKind::TableBodyMissing {
                                beside: beside.to_string(),
                            },
                        },
                        &mut reported,
                        out,
                    );
                }
            }
        }

        // 7.2-16: a Table's Caption first or last; 7.2-28 and 7.2-40: a
        // TOC's and an L's first only.
        let misplaced = match own {
            "Table" => kids.len() > 2 && kids[1..kids.len() - 1].contains(&"Caption"),
            "TOC" | "L" => kids.iter().skip(1).any(|kid| *kid == "Caption"),
            _ => false,
        };
        if misplaced {
            push(
                UaRaw {
                    rule: clauses::STRUCTURE_GRAMMAR,
                    object: at,
                    kind: FindingKind::CaptionMisplaced {
                        element: own.to_string(),
                    },
                },
                &mut reported,
                out,
            );
        }

        // 7.4.4-2 and 7.4.4-3: "All documents shall be either strongly or
        // weakly structured, but not both" — an unnumbered H and a numbered
        // Hn in one document.
        if own == "H" && headings.0.is_none() {
            headings.0 = Some(at);
        }
        if heading_level(element).is_some() && headings.1.is_none() {
            headings.1 = Some(at);
        }

        // 7.9-1 and 7.9-2: "Note tag shall have ID entry", and "Each Note
        // tag shall have unique ID key".
        if own == "Note" {
            match element.id.as_deref() {
                None | Some([]) => note_findings.push(UaRaw {
                    rule: clauses::NOTE_IDS,
                    object: at,
                    kind: FindingKind::NoteIdMissing,
                }),
                Some(id) => {
                    let seen = notes.entry(id.to_vec()).or_default();
                    *seen += 1;
                    if *seen == 2 {
                        note_findings.push(UaRaw {
                            rule: clauses::NOTE_IDS,
                            object: at,
                            kind: FindingKind::NoteIdDuplicate {
                                id: String::from_utf8_lossy(id).into_owned(),
                            },
                        });
                    }
                }
            }
        }
    });
    if let (Some(first_h), Some(_)) = headings {
        out.push(UaRaw {
            rule: clauses::HEADING_KINDS,
            object: first_h,
            kind: FindingKind::HeadingKindsMixed,
        });
    }
    out.extend(note_findings.into_iter().take(MAX_FINDINGS_PER_RULE));
}

/// One grammar finding, while the grammar's cap allows.
fn push(raw: UaRaw, reported: &mut usize, out: &mut Vec<UaRaw>) {
    if *reported < MAX_FINDINGS_PER_RULE {
        *reported += 1;
        out.push(raw);
    }
}

/// Which clause table a kid-count finding belongs to.
#[derive(Clone, Copy)]
enum ClausesFor {
    Grammar,
    Headings,
}

impl ClausesFor {
    fn table(self) -> super::UaClauses {
        match self {
            ClausesFor::Grammar => clauses::STRUCTURE_GRAMMAR,
            ClausesFor::Headings => clauses::HEADING_KINDS,
        }
    }
}

/// Every element in the tree with its parent's standard type, in reading
/// order. The root's kids have the root as their parent.
fn visit_with_parents(
    kids: &[StructKid],
    parent: &str,
    visit: &mut impl FnMut(&StructElement, &str),
) {
    for kid in kids {
        let StructKid::Element(element) = kid else {
            continue;
        };
        visit(element, parent);
        visit_with_parents(&element.kids, &element.standard_type, visit);
    }
}
