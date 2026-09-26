//! Conformance **level A**: the logical structure a tagged file has to carry
//! (ISO 19005-1 6.8, ISO 19005-2/3 6.7).
//!
//! # What level A adds, and what it does not
//!
//! Level A is level B plus accessibility: the file has to say what its content
//! *is* rather than only how it looks. ISO 19005 spells that out as four
//! requirements over the object graph — the catalog declares the file tagged,
//! a structure tree exists, every element's type is one a consumer knows, and
//! any natural language the file names is named in a syntax a consumer can
//! parse — and one more over what the page draws, which is not here and says
//! so in [`super::STAGED`].
//!
//! Every rule below runs **only for a file that claimed level A**. That is not
//! a shortcut: a level B file is not required to be tagged and reporting one
//! for having no structure tree would be reporting a conforming file. Part 4
//! has no level A at all ([`super::Part::allows`] admits only `E` and `F`
//! there), so nothing here ever runs under part 4's numbering.
//!
//! # The tree is read once, by the reader that already reads it
//!
//! [`crate::structure::bind`] is the engine's structure-tree reader: it
//! resolves `/RoleMap`, bounds the `/K` graph against a cyclic or exponential
//! one, and reports what it had to tolerate. Re-walking `/StructTreeRoot` here
//! would be a second reader to keep in step with the first, and the drift
//! would show up as a validator disagreeing with `Document::structure` about
//! what a file's tagging says.
//!
//! # The fixtures decided four things the clause text did not
//!
//! Each is a place where reading ISO 19005 alone would have produced a rule
//! that reports conforming files, and the veraPDF suite's own annotations are
//! what said so. The first three are about `/Lang`; the fourth is below.
//!
//! 1. **`/Lang` is not required — only well formed.** `6-8-4-t01-pass-b`
//!    carries `/Lang (sl)` on a structure element and **nothing** in its
//!    catalog, and the suite annotates it `pass`; `6-8-4-t01-pass-c` carries
//!    its only `/Lang` inside a marked-content property list. A rule reading
//!    "the document shall specify its natural language" as "the catalog shall
//!    carry `/Lang`" reports both.
//! 2. **The empty string is a permitted `/Lang`.** `6-8-4-t01-pass-d` writes
//!    `/Lang ()` and its own outline says why: "its value is empty text string
//!    which is permitted". No grammar for a language tag admits it, so it is
//!    admitted here by name.
//! 3. **The value is a text string, not a byte string.** `6-8-4-t01-pass-f`
//!    writes `/Lang <FEFF0065006E002D00470042>` — UTF-16BE for `en-GB` — and
//!    is annotated `pass`, while `6-8-4-t01-fail-c` writes the same encoding
//!    around Cyrillic and is annotated `fail`. Both are hexadecimal strings;
//!    what separates them is only visible after 7.9.2.2's decoding.
//!
//! The fourth is about which *part* is being checked: part 1 cites PDF
//! Reference 9.8.1 and parts 2 and 3 cite ISO 32000-1 14.9.2, which are
//! RFC 1766 and RFC 3066 — see [`language_is_well_formed`]. And a fifth the
//! suite states in words this build cannot implement literally: see
//! [`structure_types`] on "a circular mapping shall not exist".

use tinker_pdf_cos::{decode_text_string, CosDocument, Dict, ObjRef};

use super::{clauses, FindingKind, Flavour, Level, Machinery, Part, Raw, RuleGroup};
use crate::structure::StructureTree;
use crate::Document;

/// The standard structure types (ISO 32000-1 14.8.4, Tables 334 to 337).
///
/// **One list for parts 1 to 3, and that is the safe direction.** Part 1's
/// reference specification is PDF 1.4, which did not define `Annot`, `THead`,
/// `TBody` or `TFoot`; parts 2 and 3's is ISO 32000-1, which does. Modelling
/// that as two lists would make a part 1 file carrying `/TBody` a finding, and
/// the cost of being wrong about it is a conforming file reported — so the
/// wider list is used for every part and the narrower reading is not taken on
/// four names no corpus fixture tests.
///
/// `H1` to `H6` and no further: ISO 32000-1 stops there, and the open-ended
/// `Hn` arrived with ISO 32000-2, which no part that has a level A is defined
/// on.
const STANDARD_STRUCTURE_TYPES: &[&str] = &[
    // 14.8.4.2, grouping elements.
    "Document",
    "Part",
    "Art",
    "Sect",
    "Div",
    "BlockQuote",
    "Caption",
    "TOC",
    "TOCI",
    "Index",
    "NonStruct",
    "Private",
    // 14.8.4.3, block-level: paragraphlike.
    "P",
    "H",
    "H1",
    "H2",
    "H3",
    "H4",
    "H5",
    "H6",
    // 14.8.4.3, block-level: lists.
    "L",
    "LI",
    "Lbl",
    "LBody",
    // 14.8.4.3, block-level: tables.
    "Table",
    "TR",
    "TH",
    "TD",
    "THead",
    "TBody",
    "TFoot",
    // 14.8.4.4, inline-level.
    "Span",
    "Quote",
    "Note",
    "Reference",
    "BibEntry",
    "Code",
    "Link",
    "Annot",
    "Ruby",
    "RB",
    "RT",
    "RP",
    "Warichu",
    "WT",
    "WP",
    // 14.8.4.5, illustrations.
    "Figure",
    "Formula",
    "Form",
];

/// How many structure elements one document is judged over.
///
/// [`crate::structure`] already bounds its own walk; this bounds the *finding
/// list* a hostile tree can produce, which is a different thing. A file
/// describing a quarter of a million elements of one non-standard type is one
/// defect, and a verdict carrying a quarter of a million copies of it is not
/// provenance (ruling 10).
const MAX_TYPE_FINDINGS: usize = 64;

/// Runs level A's logical structure rules.
pub(super) fn rules(
    document: &Document,
    machinery: &Machinery,
    flavour: Option<Flavour>,
    out: &mut Vec<Raw>,
) {
    if !machinery.reach(RuleGroup::Structure) {
        return;
    }
    // The gate, and the reason this group cannot report a level B file: a
    // claim of level A is the only thing that makes any of it required.
    let Some(flavour) = flavour else {
        return;
    };
    if flavour.level != Some(Level::A) {
        return;
    }

    let doc = &document.inner;
    let Some(catalog) = doc.catalog() else {
        // No catalog at all is 6.1.x's finding and the syntax group's; a
        // level A rule that also reported it would report one defect twice.
        return;
    };

    mark_info(doc, &catalog, out);
    language(doc, &catalog, flavour, out);

    // `bind` returns `None` for exactly one reason a rule cares about — the
    // catalog names no `/StructTreeRoot` — which is what 6.8.3.3 is about.
    let Some(tree) = crate::structure::bind(doc) else {
        out.push(Raw::file(
            clauses::STRUCTURE_HIERARCHY,
            FindingKind::StructureTreeMissing,
        ));
        return;
    };
    structure_types(&tree, out);
    element_languages(&tree, flavour, out);
}

/// ISO 19005-1 6.8.2.2, ISO 19005-2/3 6.7.2.2: the mark information
/// dictionary.
///
/// Three fixtures per part and they say the requirement in three pieces:
/// "the document catalog dictionary does not contain a MarkInfo dictionary",
/// "includes a MarkInfo dictionary whose sole entry, /Marked has value false",
/// and "Marked entry in MarkInfo is missing". The conforming twin,
/// `6-8-2-2-t01-pass-a`, "includes a MarkInfo dictionary whose sole entry,
/// Marked have value true".
fn mark_info(doc: &CosDocument, catalog: &Dict, out: &mut Vec<Raw>) {
    let value = doc.resolve_key(catalog, doc.intern(b"MarkInfo"));
    let Some(dict) = value.as_dict() else {
        out.push(Raw::file(clauses::TAGGED_PDF, FindingKind::MarkInfoMissing));
        return;
    };
    // 14.7.1 Table 321 defaults `/Marked` to false, so an absent entry and a
    // `false` one are the same statement about the document.
    if doc.resolve_key(dict, doc.intern(b"Marked")).as_bool() != Some(true) {
        out.push(Raw::file(
            clauses::TAGGED_PDF,
            FindingKind::NotMarkedAsTagged,
        ));
    }
}

/// ISO 19005-1 6.8.3.4, ISO 19005-2/3 6.7.3.4: every structure type is a
/// standard one, or is mapped to one.
///
/// **Two fixtures, one rule, and the second is why it is one rule.**
/// `6-8-3-4-t01-fail-a` says "a Structure element uses a non-standard type and
/// the StructTreeRoot does not contain the RoleMap"; `6-8-3-4-t02-fail-a` says
/// "a circular mapping shall not exist" and its role map is
/// `<< /Document /Document /Span /Span /Standard /Standard >>` over elements
/// typed `/Standard`. Written as two rules — one for an unmapped type and one
/// for a cyclic role map — the second reports nothing, because
/// [`crate::structure`] treats `/X → /X` as a *termination* rather than as a
/// cycle and is right to: `/P /P` is the commonest entry in the wild and
/// calling it a loop produced 63 warnings over the fetched corpora against a
/// handful of real ones.
///
/// Asking instead what the type resolved *to* answers both. `/Standard` maps
/// to `/Standard`, which is still not a standard type, and the file is
/// reported under the requirement it actually breaks — that a consumer cannot
/// tell what the element is — rather than under a description of how its role
/// map is spelled.
fn structure_types(tree: &StructureTree, out: &mut Vec<Raw>) {
    let mut reported = 0usize;
    for element in tree.elements() {
        if reported >= MAX_TYPE_FINDINGS {
            break;
        }
        // An element with no `/S` at all is `UntypedElement` in the structure
        // reader's own warnings and arrives here with an empty type. It is
        // still an element whose type a consumer cannot resolve, which is what
        // this clause is about, so it is reported — with the empty string in
        // `declared`, saying exactly what the file said.
        if STANDARD_STRUCTURE_TYPES.contains(&element.standard_type.as_str()) {
            continue;
        }
        reported += 1;
        out.push(Raw {
            rule: clauses::STRUCTURE_TYPES,
            object: element.reference,
            kind: FindingKind::StructureTypeNotStandard {
                declared: element.raw_type.clone(),
                mapped: element.standard_type.clone(),
            },
        });
    }
}

/// ISO 19005-1 6.8.4, ISO 19005-2/3 6.7.4: the catalog's `/Lang`.
fn language(doc: &CosDocument, catalog: &Dict, flavour: Flavour, out: &mut Vec<Raw>) {
    let value = doc.resolve_key(catalog, doc.intern(b"Lang"));
    let Some(string) = value.as_string() else {
        // Absent, which `6-8-4-t01-pass-b` and `-pass-c` show is conforming:
        // both carry their only `/Lang` somewhere other than the catalog and
        // both are annotated `pass`.
        return;
    };
    report_language(&decode_text_string(&string.bytes), flavour, None, out);
}

/// The same rule over every structure element's `/Lang` (14.9.2).
///
/// `6-8-4-t01-pass-b` is why this walk exists at all: its language is on a
/// `/Span` element and nowhere else. A rule that read only the catalog would
/// pass every file that put a malformed tag on an element instead.
fn element_languages(tree: &StructureTree, flavour: Flavour, out: &mut Vec<Raw>) {
    let mut reported = 0usize;
    for element in tree.elements() {
        if reported >= MAX_TYPE_FINDINGS {
            break;
        }
        // Already decoded by 7.9.2.2's rules where the structure reader read
        // it, which is why the hexadecimal spelling needs nothing here.
        let Some(language) = element.lang.as_deref() else {
            continue;
        };
        let before = out.len();
        report_language(language, flavour, element.reference, out);
        if out.len() != before {
            reported += 1;
        }
    }
}

/// Pushes a finding when `value` is not a language identifier.
fn report_language(value: &str, flavour: Flavour, object: Option<ObjRef>, out: &mut Vec<Raw>) {
    if language_is_well_formed(value, flavour.part) {
        return;
    }
    out.push(Raw {
        rule: clauses::NATURAL_LANGUAGE,
        object,
        kind: FindingKind::LanguageMalformed {
            declared: value.to_string(),
        },
    });
}

/// Whether `value` is a language identifier the part's own reference
/// specification defines.
///
/// **The two parts cite two RFCs and the corpus is what proves it matters.**
/// Part 1 sends a reader to PDF Reference 9.8.1, which defines the value by
/// RFC 1766, where every subtag is `1*8ALPHA`. Parts 2 and 3 send a reader to
/// ISO 32000-1 14.9.2, which defines it by RFC 3066, where a subtag is
/// `1*8(ALPHA / DIGIT)`. The primary tag is `1*8ALPHA` in both.
///
/// Reading only the clause, that difference is invisible — both clauses are
/// one sentence naming a specification — and the suite states it twice over.
/// Part 1 has a fixture for it, `6-8-4-t01-fail-b`, whose `/Lang` is `en-12`
/// and whose outline says "Subtag of Lang entry contains digits"; **parts 2
/// and 3 have no such fixture**, and instead `6-7-4-t01-pass-c` writes
/// `/Lang (ru-petr1708)` and is annotated `pass`. One list of five fail
/// fixtures under part 1 against four under part 2, differing by exactly this
/// case, is the whole evidence, and a single grammar for both parts would have
/// to choose which of those two files to report.
///
/// The empty string is admitted before the grammar is reached: `pass-d` in
/// both parts writes `/Lang ()` and says in its own outline that an empty text
/// string is permitted.
fn language_is_well_formed(value: &str, part: Part) -> bool {
    if value.is_empty() {
        return true;
    }
    let digits_in_subtags = part != Part::One;
    let mut tags = value.split('-');
    let Some(primary) = tags.next() else {
        return false;
    };
    if !is_tag(primary, false) {
        return false;
    }
    tags.all(|subtag| is_tag(subtag, digits_in_subtags))
}

/// One tag of a language identifier: one to eight letters, or letters and
/// digits where the citing RFC allows them.
///
/// ASCII deliberately. RFC 1766 and RFC 3066 both say `ALPHA`, which is
/// `A-Za-z` and not "a letter" — `6-8-4-t01-fail-c` writes Cyrillic `ан-СА` in
/// UTF-16BE and is annotated `fail`, and `char::is_alphabetic` would admit it.
fn is_tag(tag: &str, digits: bool) -> bool {
    if tag.is_empty() || tag.len() > 8 {
        return false;
    }
    tag.bytes()
        .all(|byte| byte.is_ascii_alphabetic() || (digits && byte.is_ascii_digit()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The grammar, tag by tag, against every value the corpus's own fixtures
    /// carry — so the table below can be read against the outlines that
    /// produced it without opening a PDF.
    #[test]
    fn the_language_grammar_matches_every_corpus_fixture_value() {
        // Part 1: PDF Reference 9.8.1, RFC 1766.
        for (value, well_formed) in [
            ("15-HR", false), // 6-8-4-t01-fail-a: digits in the primary tag
            ("en-12", false), // 6-8-4-t01-fail-b: digits in a subtag
            ("\u{430}\u{43d}-\u{421}\u{410}", false), // -fail-c: not ASCII
            ("-BG", false),   // 6-8-4-t01-fail-d: an empty primary tag
            ("az/Latn", false), // 6-8-4-t01-fail-e: a delimiter that is not `-`
            ("en-GB", true),  // 6-8-4-t01-pass-a and -pass-f
            ("sl", true),     // 6-8-4-t01-pass-b
            ("i-cherokee", true), // 6-8-4-t01-pass-c
            ("", true),       // 6-8-4-t01-pass-d
            ("fr-ca", true),  // 6-8-4-t01-pass-e
        ] {
            assert_eq!(
                language_is_well_formed(value, Part::One),
                well_formed,
                "part 1: {value:?}"
            );
        }

        // Parts 2 and 3: ISO 32000-1 14.9.2, RFC 3066.
        for (value, well_formed) in [
            ("12-BE", false),                         // 6-7-4-t01-fail-a
            ("de/AT", false),                         // 6-7-4-t01-fail-b
            ("\u{430}\u{43d}-\u{421}\u{410}", false), // 6-7-4-t01-fail-c
            ("-BG", false),                           // 6-7-4-t01-fail-d
            ("zh-Hant-HK", true),                     // 6-7-4-t01-pass-a
            ("lv", true),                             // 6-7-4-t01-pass-b
            ("ru-petr1708", true),                    // 6-7-4-t01-pass-c: digits in a subtag
            ("", true),                               // 6-7-4-t01-pass-d
            ("hr-ba", true),                          // 6-7-4-t01-pass-e
        ] {
            for part in [Part::Two, Part::Three] {
                assert_eq!(
                    language_is_well_formed(value, part),
                    well_formed,
                    "part {}: {value:?}",
                    part.number()
                );
            }
        }
    }

    /// The one value the two grammars disagree about, asserted in both
    /// directions.
    ///
    /// Without this the per-part split is a comment: a single grammar admitting
    /// digits everywhere passes every part 2 fixture and `en-12` with them, and
    /// a single grammar refusing them passes every part 1 fixture and reports
    /// `ru-petr1708`, which the suite annotates `pass`.
    #[test]
    fn digits_in_a_subtag_are_part_ones_defect_and_nobody_elses() {
        assert!(!language_is_well_formed("en-12", Part::One));
        assert!(language_is_well_formed("en-12", Part::Two));
        assert!(language_is_well_formed("en-12", Part::Three));
        // The primary tag is `1*8ALPHA` in both RFCs, so this is not a split.
        assert!(!language_is_well_formed("12-BE", Part::One));
        assert!(!language_is_well_formed("12-BE", Part::Two));
    }

    /// Eight characters is the cap both RFCs put on a tag, and nine is not.
    #[test]
    fn a_tag_is_one_to_eight_characters() {
        assert!(language_is_well_formed("abcdefgh", Part::One));
        assert!(!language_is_well_formed("abcdefghi", Part::One));
        assert!(language_is_well_formed("en-abcdefgh", Part::One));
        assert!(!language_is_well_formed("en-abcdefghi", Part::One));
        // A trailing hyphen leaves an empty subtag, which is not `1*8`.
        assert!(!language_is_well_formed("en-", Part::One));
    }

    /// Every name in the table is distinct, and the count is the one ISO
    /// 32000-1's four tables add up to.
    ///
    /// A duplicate would be invisible — `contains` answers the same either way
    /// — and a name silently lost in an edit would turn a conforming type into
    /// a finding, which is this group's whole failure mode.
    #[test]
    fn the_standard_structure_types_are_forty_nine_distinct_names() {
        let mut sorted = STANDARD_STRUCTURE_TYPES.to_vec();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), STANDARD_STRUCTURE_TYPES.len());
        assert_eq!(STANDARD_STRUCTURE_TYPES.len(), 49);
    }
}
