//! Conformance level A's logical structure rules, one defect at a time.
//!
//! `crates/tinker-pdf/src/pdfa/logical.rs` argues which clause each rule
//! implements and which veraPDF fixture decided it. This file is what holds
//! the rules to something without a corpus, and **every fixture here has its
//! twin**: the conforming spelling of the same document, asserted silent.
//!
//! The twins are not symmetry for its own sake. This rule group's failure mode
//! is one-directional — a structure tree the rule cannot follow reads exactly
//! like a file with none, and a level B file reads exactly like a level A file
//! that forgot to tag itself — so "it reported something" is worth nothing here
//! without "and it reports nothing on the file that is right". Two of the
//! twins below are not a spelling at all but a *level*: the same broken bytes
//! claiming `B` instead of `A`, and the same claiming part 4, are silent,
//! because level A is the only thing that makes any of this required.

use tinker_pdf::{Document, FindingKind, PdfACoverage};

/// A level A document, with every knob these rules read.
///
/// The defaults conform. A test changes one field, which is the whole
/// discipline: a fixture with two things wrong cannot say which one was
/// reported.
#[derive(Clone)]
struct Fixture {
    /// `pdfaid:part`.
    part: &'static str,
    /// `pdfaid:conformance`.
    conformance: &'static str,
    /// The `/MarkInfo` value, or `None` to leave the key out of the catalog.
    mark_info: Option<&'static str>,
    /// The catalog's `/Lang` as it is written in the file — a literal string,
    /// a hexadecimal one — or `None` to leave the key out.
    lang: Option<&'static str>,
    /// Whether the catalog names a `/StructTreeRoot`.
    structure_tree: bool,
    /// The structure tree root's `/RoleMap` value.
    role_map: &'static str,
    /// The child element's `/S`.
    child_type: &'static str,
    /// The child element's `/Lang`, or `None` to leave the key out.
    child_lang: Option<&'static str>,
    /// How many times the `/Document` element names the child in its `/K`.
    ///
    /// 14.7.2 gives an element one parent and nothing in a *file* enforces it,
    /// so naming one element many times is a shape a hostile file can take and
    /// the structure reader walks it. It is here to reach the finding cap.
    children: usize,
}

impl Default for Fixture {
    fn default() -> Fixture {
        Fixture {
            part: "2",
            conformance: "A",
            mark_info: Some("<< /Marked true >>"),
            lang: Some("(en-GB)"),
            structure_tree: true,
            // `/Chapitre` is a producer's own name for a section, which 14.7.3
            // exists for and which the default fixture maps — so the
            // conforming twin of the structure-type rule is a file that uses a
            // custom type rather than a file that avoids one.
            role_map: "<< /Chapitre /Sect >>",
            child_type: "/Chapitre",
            child_lang: Some("(fr-ca)"),
            children: 1,
        }
    }
}

impl Fixture {
    fn bytes(&self) -> Vec<u8> {
        let packet = format!(
            "<?xpacket begin=\"\" id=\"W5M0MpCehiHzreSzNTczkc9d\"?>\
<x:xmpmeta xmlns:x=\"adobe:ns:meta/\"><rdf:RDF \
xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\">\
<rdf:Description rdf:about=\"\" xmlns:pdfaid=\"http://www.aiim.org/pdfa/ns/id/\">\
<pdfaid:part>{}</pdfaid:part><pdfaid:conformance>{}</pdfaid:conformance>\
</rdf:Description></rdf:RDF></x:xmpmeta><?xpacket end=\"w\"?>",
            self.part, self.conformance
        );
        // A marked-content sequence and no colour operator: the element below
        // claims id 0, and a device colour space in a file with no PDF/A
        // output intent would be a *colour* finding, which this file is not
        // about.
        let content = "/P <</MCID 0>> BDC Q EMC";

        let mut catalog = String::from("<< /Type /Catalog /Pages 2 0 R /Metadata 4 0 R");
        if let Some(mark_info) = self.mark_info {
            catalog.push_str(&format!(" /MarkInfo {mark_info}"));
        }
        if let Some(lang) = self.lang {
            catalog.push_str(&format!(" /Lang {lang}"));
        }
        if self.structure_tree {
            catalog.push_str(" /StructTreeRoot 6 0 R");
        }
        catalog.push_str(" >>");

        let mut child = format!(
            "<< /Type /StructElem /S {} /P 7 0 R /Pg 3 0 R /K [0]",
            self.child_type
        );
        if let Some(lang) = self.child_lang {
            child.push_str(&format!(" /Lang {lang}"));
        }
        child.push_str(" >>");

        let objects: Vec<(u32, Vec<u8>)> = vec![
            (1, catalog.into_bytes()),
            (2, b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec()),
            (
                3,
                b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Resources << >> \
                  /StructParents 0 /Contents 5 0 R >>"
                    .to_vec(),
            ),
            (
                4,
                format!(
                    "<< /Type /Metadata /Subtype /XML /Length {} >>\nstream\n{packet}\nendstream",
                    packet.len()
                )
                .into_bytes(),
            ),
            (
                5,
                format!(
                    "<< /Length {} >>\nstream\n{content}\nendstream",
                    content.len()
                )
                .into_bytes(),
            ),
            (
                6,
                format!(
                    "<< /Type /StructTreeRoot /K [7 0 R] /ParentTree 9 0 R \
                     /ParentTreeNextKey 1 /RoleMap {} >>",
                    self.role_map
                )
                .into_bytes(),
            ),
            (
                7,
                format!(
                    "<< /Type /StructElem /S /Document /P 6 0 R /K [{}] >>",
                    "8 0 R ".repeat(self.children)
                )
                .into_bytes(),
            ),
            (8, child.into_bytes()),
            (9, b"<< /Nums [0 [8 0 R]] >>".to_vec()),
        ];

        let mut out = b"%PDF-1.7\n%\xE2\xE3\xCF\xD3\n".to_vec();
        let mut offsets = vec![0u64; objects.len() + 1];
        for (num, body) in &objects {
            offsets[*num as usize] = out.len() as u64;
            out.extend_from_slice(format!("{num} 0 obj\n").as_bytes());
            out.extend_from_slice(body);
            out.extend_from_slice(b"\nendobj\n");
        }
        let xref_at = out.len();
        out.extend_from_slice(
            format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes(),
        );
        for entry in offsets.iter().skip(1) {
            out.extend_from_slice(format!("{entry:010} 00000 n \n").as_bytes());
        }
        out.extend_from_slice(
            format!(
                "trailer\n<< /Size {} /Root 1 0 R \
                 /ID [<0102030405060708090A0B0C0D0E0F10> <0102030405060708090A0B0C0D0E0F10>] >>\n\
                 startxref\n{xref_at}\n%%EOF\n",
                objects.len() + 1
            )
            .as_bytes(),
        );
        out
    }

    fn findings(&self) -> Vec<FindingKind> {
        Document::open(self.bytes())
            .expect("the fixture opens")
            .validate_pdfa()
            .findings
            .into_iter()
            .map(|finding| finding.kind)
            .collect()
    }

    /// What clause each finding was reported under, in order.
    fn clauses(&self) -> Vec<String> {
        Document::open(self.bytes())
            .expect("the fixture opens")
            .validate_pdfa()
            .findings
            .into_iter()
            .map(|finding| finding.clause.to_string())
            .collect()
    }

    /// The twin assertion, spelled once: this spelling is reported on by
    /// nobody.
    fn is_silent(&self) {
        assert_eq!(
            self.findings(),
            Vec::new(),
            "this spelling conforms and must be reported by nothing"
        );
    }
}

// ---- the twin every other test is measured against ------------------------

/// The undamaged document. Without this every assertion below could be passing
/// because the fixture was broken in some other way all along.
#[test]
fn the_conforming_document_is_clean() {
    Fixture::default().is_silent();
}

// ---- 6.7.2.2 / 6.8.2.2, the mark information dictionary --------------------

/// `6-8-2-2-t01-fail-a`: "the document catalog dictionary does not contain a
/// MarkInfo dictionary".
#[test]
fn a_catalog_with_no_mark_info_is_a_tagged_pdf_finding() {
    let broken = Fixture {
        mark_info: None,
        ..Fixture::default()
    };
    assert_eq!(broken.findings(), vec![FindingKind::MarkInfoMissing]);
    assert_eq!(broken.clauses(), vec!["6.7.2.2"]);
}

/// `6-8-2-2-t01-fail-b`: a `/MarkInfo` "whose sole entry, /Marked has value
/// false", and `-fail-c`: "Marked entry in MarkInfo is missing". One finding
/// for both, because 14.7.1 Table 321 defaults the entry to false.
///
/// The twin is `/Marked true`, which is the default fixture — asserted here as
/// well rather than only by inheritance, because what separates these three
/// files is one token.
#[test]
fn a_mark_info_that_does_not_say_marked_true_is_a_tagged_pdf_finding() {
    for mark_info in ["<< /Marked false >>", "<< /Suspects false >>", "<< >>"] {
        let broken = Fixture {
            mark_info: Some(mark_info),
            ..Fixture::default()
        };
        assert_eq!(
            broken.findings(),
            vec![FindingKind::NotMarkedAsTagged],
            "{mark_info}"
        );
    }
    Fixture {
        mark_info: Some("<< /Marked true >>"),
        ..Fixture::default()
    }
    .is_silent();
}

/// A `/MarkInfo` that is not a dictionary at all is the *missing* finding
/// rather than the unmarked one: nothing there says the document is tagged.
#[test]
fn a_mark_info_that_is_not_a_dictionary_is_the_missing_finding() {
    let broken = Fixture {
        mark_info: Some("true"),
        ..Fixture::default()
    };
    assert_eq!(broken.findings(), vec![FindingKind::MarkInfoMissing]);
}

// ---- 6.7.3.3 / 6.8.3.3, the structure hierarchy ---------------------------

/// `6-8-3-3-t01-fail-a`: "the structure tree root entry is missing in the
/// catalog dictionary".
#[test]
fn a_catalog_with_no_structure_tree_root_is_a_hierarchy_finding() {
    let broken = Fixture {
        structure_tree: false,
        ..Fixture::default()
    };
    assert_eq!(broken.findings(), vec![FindingKind::StructureTreeMissing]);
    assert_eq!(broken.clauses(), vec!["6.7.3.3"]);
}

// ---- 6.7.3.4 / 6.8.3.4, structure types -----------------------------------

/// `6-8-3-4-t01-fail-a`: "a Structure element uses a non-standard type and the
/// StructTreeRoot does not contain the RoleMap".
///
/// The twin is the default fixture, whose `/Chapitre` is the same non-standard
/// type **with** the role-map entry that explains it — so this pair separates
/// "uses a custom type", which is permitted, from "uses a custom type nobody
/// can resolve", which is not.
#[test]
fn a_type_no_role_map_explains_is_a_structure_types_finding() {
    let broken = Fixture {
        role_map: "<< >>",
        ..Fixture::default()
    };
    assert_eq!(
        broken.findings(),
        vec![FindingKind::StructureTypeNotStandard {
            declared: "Chapitre".to_string(),
            mapped: "Chapitre".to_string(),
        }]
    );
    assert_eq!(broken.clauses(), vec!["6.7.3.4"]);
    Fixture::default().is_silent();
}

/// `6-8-3-4-t02-fail-a`: "a circular mapping shall not exist", whose role map
/// is `<< /Document /Document /Span /Span /Standard /Standard >>` over
/// elements typed `/Standard`.
///
/// **The identity entry is the whole test.** `crates/tinker-pdf/src/structure`
/// treats `/X → /X` as a termination rather than as a cycle, and is right to —
/// `/P /P` is the commonest role-map entry in the wild and calling it a loop
/// produced 63 warnings over the fetched corpora against a handful of real
/// ones. So a rule written as "the role map shall not cycle" reports nothing
/// here, and the rule that is written — the resolved type shall be a standard
/// one — reports it.
///
/// Its twin is the second assertion: the same identity entry on a type that
/// *is* standard is silent, which is the case that would make this rule
/// intolerable if it were not.
#[test]
fn a_type_that_a_role_map_maps_to_itself_is_still_not_standard() {
    let broken = Fixture {
        role_map: "<< /Document /Document /Chapitre /Chapitre >>",
        ..Fixture::default()
    };
    assert_eq!(
        broken.findings(),
        vec![FindingKind::StructureTypeNotStandard {
            declared: "Chapitre".to_string(),
            mapped: "Chapitre".to_string(),
        }]
    );

    // The twin: an identity entry over standard types, which is what a
    // producer writes when its role map covers every type it emits.
    Fixture {
        role_map: "<< /Document /Document /Span /Span >>",
        child_type: "/Span",
        ..Fixture::default()
    }
    .is_silent();
}

/// A role map that takes two hops still arrives, and one that arrives nowhere
/// is reported under the type the file actually wrote.
#[test]
fn a_role_map_is_followed_to_the_end_of_its_chain() {
    Fixture {
        role_map: "<< /Chapitre /Partie /Partie /Sect >>",
        ..Fixture::default()
    }
    .is_silent();

    let broken = Fixture {
        role_map: "<< /Chapitre /Partie /Partie /Chapitre >>",
        ..Fixture::default()
    };
    assert_eq!(
        broken.findings(),
        vec![FindingKind::StructureTypeNotStandard {
            declared: "Chapitre".to_string(),
            // The loop is cut by the structure reader at the name it closed
            // on, and the finding says what the type resolved to rather than
            // describing the shape of the map.
            mapped: "Partie".to_string(),
        }]
    );
}

/// Every one of the 49 names ISO 32000-1 14.8.4 defines is admitted with no
/// role map at all.
///
/// The list is a whitelist and a name missing from it turns a conforming file
/// into a finding, which is this group's failure mode — so the list is checked
/// against documents rather than against itself.
#[test]
fn every_standard_structure_type_is_admitted_unmapped() {
    const STANDARD: &[&str] = &[
        "/Document",
        "/Part",
        "/Art",
        "/Sect",
        "/Div",
        "/BlockQuote",
        "/Caption",
        "/TOC",
        "/TOCI",
        "/Index",
        "/NonStruct",
        "/Private",
        "/P",
        "/H",
        "/H1",
        "/H2",
        "/H3",
        "/H4",
        "/H5",
        "/H6",
        "/L",
        "/LI",
        "/Lbl",
        "/LBody",
        "/Table",
        "/TR",
        "/TH",
        "/TD",
        "/THead",
        "/TBody",
        "/TFoot",
        "/Span",
        "/Quote",
        "/Note",
        "/Reference",
        "/BibEntry",
        "/Code",
        "/Link",
        "/Annot",
        "/Ruby",
        "/RB",
        "/RT",
        "/RP",
        "/Warichu",
        "/WT",
        "/WP",
        "/Figure",
        "/Formula",
        "/Form",
    ];
    assert_eq!(STANDARD.len(), 49);
    for name in STANDARD {
        let fixture = Fixture {
            role_map: "<< >>",
            child_type: name,
            ..Fixture::default()
        };
        assert_eq!(fixture.findings(), Vec::new(), "{name}");
    }
}

// ---- 6.7.4 / 6.8.4, natural language specification ------------------------

/// `6-8-4-t01-fail-a` and its siblings, in the catalog.
#[test]
fn a_malformed_catalog_language_is_a_natural_language_finding() {
    for (written, declared) in [
        ("(15-HR)", "15-HR"),     // digits in the primary tag
        ("(-BG)", "-BG"),         // an empty primary tag
        ("(az/Latn)", "az/Latn"), // a delimiter that is not a hyphen
    ] {
        let broken = Fixture {
            lang: Some(written),
            ..Fixture::default()
        };
        assert_eq!(
            broken.findings(),
            vec![FindingKind::LanguageMalformed {
                declared: declared.to_string(),
            }],
            "{written}"
        );
        assert_eq!(broken.clauses(), vec!["6.7.4"], "{written}");
    }
}

/// `6-8-4-t01-pass-b`: the language is on a structure element and the catalog
/// has none — annotated `pass`, which is why the absent catalog entry is not a
/// finding and why the element's own entry is still judged.
///
/// Both halves are here: the well-formed element language with no catalog
/// language at all is silent, and the malformed one is reported against the
/// element's own object.
#[test]
fn a_structure_elements_language_is_judged_and_the_catalogs_is_not_required() {
    Fixture {
        lang: None,
        child_lang: Some("(sl)"),
        ..Fixture::default()
    }
    .is_silent();

    let broken = Fixture {
        lang: None,
        child_lang: Some("(en-)"),
        ..Fixture::default()
    };
    assert_eq!(
        broken.findings(),
        vec![FindingKind::LanguageMalformed {
            declared: "en-".to_string(),
        }]
    );
    let verdict = Document::open(broken.bytes())
        .expect("opens")
        .validate_pdfa();
    assert_eq!(
        verdict.findings[0].object.map(|at| at.num),
        Some(8),
        "the finding names the element that carries the entry"
    );
}

/// `6-8-4-t01-pass-d`: "its value is empty text string which is permitted".
///
/// No grammar for a language tag admits the empty string, so it is admitted by
/// name — and this is the assertion that keeps it admitted.
#[test]
fn an_empty_language_is_permitted() {
    Fixture {
        lang: Some("()"),
        ..Fixture::default()
    }
    .is_silent();
    Fixture {
        lang: None,
        child_lang: Some("()"),
        ..Fixture::default()
    }
    .is_silent();
}

/// `6-8-4-t01-pass-f` against `6-8-4-t01-fail-c`: two hexadecimal strings, both
/// UTF-16BE with a byte-order mark, and what separates them is only visible
/// after 7.9.2.2's decoding.
///
/// A rule reading the bytes rather than the text string reports both or
/// neither.
#[test]
fn a_language_in_a_utf16_hex_string_is_decoded_before_it_is_judged() {
    // <FEFF 0065 006E 002D 0047 0042> — "en-GB".
    Fixture {
        lang: Some("<FEFF0065006E002D00470042>"),
        ..Fixture::default()
    }
    .is_silent();

    // <feff 0430 043d 002d 0421 0410> — Cyrillic "ан-СА".
    let broken = Fixture {
        lang: Some("<feff0430043d002d04210410>"),
        ..Fixture::default()
    };
    assert_eq!(
        broken.findings(),
        vec![FindingKind::LanguageMalformed {
            declared: "\u{430}\u{43d}-\u{421}\u{410}".to_string(),
        }]
    );
}

/// The one case the two parts answer differently, asserted on documents.
///
/// Part 1 cites PDF Reference 9.8.1, which is RFC 1766, where a subtag is
/// `1*8ALPHA`; parts 2 and 3 cite ISO 32000-1 14.9.2, which is RFC 3066, where
/// it is `1*8(ALPHA / DIGIT)`. Part 1 has a fixture for the difference,
/// `6-8-4-t01-fail-b` (`en-12`, annotated `fail`); parts 2 and 3 have the
/// opposite one, `6-7-4-t01-pass-c` (`ru-petr1708`, annotated `pass`).
#[test]
fn digits_in_a_subtag_are_part_ones_defect_and_nobody_elses() {
    let part_one = Fixture {
        part: "1",
        lang: Some("(en-12)"),
        ..Fixture::default()
    };
    assert_eq!(
        part_one.findings(),
        vec![FindingKind::LanguageMalformed {
            declared: "en-12".to_string(),
        }]
    );
    assert_eq!(part_one.clauses(), vec!["6.8.4"]);

    for part in ["2", "3"] {
        Fixture {
            part,
            lang: Some("(ru-petr1708)"),
            ..Fixture::default()
        }
        .is_silent();
    }
}

// ---- the level and the part are what make any of it required ---------------

/// **The twin that matters most.** The same broken bytes at level B and level
/// U are silent.
///
/// Levels B and U require no structure tree, no `/MarkInfo` and no natural
/// language, so a rule that ran for them would report a conforming file for
/// every untagged PDF/A-1b in existence — which is most of the corpus.
#[test]
fn a_document_that_did_not_claim_level_a_is_reported_by_none_of_these_rules() {
    for conformance in ["B", "U"] {
        let broken = Fixture {
            conformance,
            mark_info: None,
            lang: Some("(15-HR)"),
            structure_tree: false,
            ..Fixture::default()
        };
        assert_eq!(broken.findings(), Vec::new(), "level {conformance}");
    }

    // And the same document at level A, so the silence above is the level
    // rather than the fixture having stopped being broken.
    let broken = Fixture {
        mark_info: None,
        lang: Some("(15-HR)"),
        structure_tree: false,
        ..Fixture::default()
    };
    assert_eq!(
        broken.findings(),
        vec![
            FindingKind::MarkInfoMissing,
            FindingKind::LanguageMalformed {
                declared: "15-HR".to_string()
            },
            FindingKind::StructureTreeMissing,
        ]
    );
}

/// ISO 19005-4 defines no level A, so a part 4 file never reaches these rules
/// — and the `four` arm of each clause table is never resolved.
#[test]
fn a_part_four_document_is_reported_by_none_of_these_rules() {
    for conformance in ["E", "F"] {
        let broken = Fixture {
            part: "4",
            conformance,
            mark_info: None,
            structure_tree: false,
            ..Fixture::default()
        };
        // Part 4 asks for a `pdfaid:rev` this fixture does not carry, which is
        // a metadata finding and not one of these; nothing from this group is
        // in the list.
        let findings = broken.findings();
        assert!(
            !findings.contains(&FindingKind::MarkInfoMissing)
                && !findings.contains(&FindingKind::StructureTreeMissing),
            "part 4 level {conformance}: {findings:#?}"
        );
    }
}

/// Part 1 numbers logical structure 6.8 and parts 2 and 3 number it 6.7.
#[test]
fn the_clause_numbers_follow_the_part() {
    for (part, expected) in [
        ("1", ["6.8.2.2", "6.8.3.3"]),
        ("2", ["6.7.2.2", "6.7.3.3"]),
        ("3", ["6.7.2.2", "6.7.3.3"]),
    ] {
        let broken = Fixture {
            part,
            mark_info: None,
            structure_tree: false,
            ..Fixture::default()
        };
        assert_eq!(broken.clauses(), expected.to_vec(), "part {part}");
    }
}

/// One defect repeated 200 times is one defect, and the verdict says so 64
/// times rather than 200 (ruling 10).
///
/// 14.7.2 gives a structure element one parent and nothing in a file enforces
/// it, so a tree can name the same subtree from everywhere and a validator
/// with no bound of its own would return a finding list as long as the tree.
/// Both loops are capped and both are exercised here — the elements carry a
/// non-standard type *and* a malformed `/Lang`, so the cap is asserted on the
/// structure-type rule and on the element-language rule at once.
#[test]
fn one_defect_repeated_is_capped_rather_than_reported_per_element() {
    let hostile = Fixture {
        role_map: "<< >>",
        child_lang: Some("(15-HR)"),
        children: 200,
        ..Fixture::default()
    };
    let findings = hostile.findings();
    let types = findings
        .iter()
        .filter(|kind| matches!(kind, FindingKind::StructureTypeNotStandard { .. }))
        .count();
    let languages = findings
        .iter()
        .filter(|kind| matches!(kind, FindingKind::LanguageMalformed { .. }))
        .count();
    assert_eq!(types, 64, "{findings:#?}");
    assert_eq!(languages, 64, "{findings:#?}");

    // The twin: the same 200 elements, conforming, are silent — so the number
    // above is a cap on a defect rather than a cap on a walk that always
    // reports something.
    Fixture {
        children: 200,
        ..Fixture::default()
    }
    .is_silent();
}

// ---- the group is a group -------------------------------------------------

/// A syntax-only sweep does not run these rules, which is the laziness
/// requirement the `structure` flag exists for.
#[test]
fn a_syntax_only_sweep_does_not_run_the_logical_group() {
    let broken = Fixture {
        mark_info: None,
        structure_tree: false,
        ..Fixture::default()
    };
    let verdict = Document::open(broken.bytes())
        .expect("opens")
        .validate_pdfa_with(PdfACoverage::SYNTAX);
    assert_eq!(verdict.findings, Vec::new());
    assert!(!verdict.coverage.structure);

    // And the structural group alone does run them, so the assertion above is
    // about the flag rather than about the fixture.
    let verdict = Document::open(broken.bytes())
        .expect("opens")
        .validate_pdfa_with(PdfACoverage::STRUCTURE);
    assert_eq!(
        verdict
            .findings
            .into_iter()
            .map(|finding| finding.kind)
            .collect::<Vec<_>>(),
        vec![
            FindingKind::MarkInfoMissing,
            FindingKind::StructureTreeMissing
        ]
    );
}
