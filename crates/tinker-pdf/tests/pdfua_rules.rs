//! One fixture per PDF/UA rule, each built at the edge of its clause, and each
//! with the near-miss twin that must **not** fire (`docs/design/pdfua.md`).
//!
//! The discipline is `pdfa_fonts.rs`'s: every test starts from [`Ua::new`] —
//! a document this build finds nothing wrong with under the part it claims —
//! makes exactly one change, and asserts exactly one finding of exactly one
//! kind under exactly one clause. [`the_baseline_is_clean_under_both_parts`]
//! fails first when the baseline itself grows a finding, which is what keeps
//! the single-finding assertions honest rather than accidental.
//!
//! # Where each clause comes from
//!
//! ISO 14289 is sold and is not in this environment. Each test cites the
//! clause as veraPDF's published PDF/UA profiles number and quote it
//! (`PDFUA-1.xml`, `PDFUA-2.xml`, `veraPDF-validation-profiles` at `070d39f`,
//! read as data and never run — ruling 13), and where the design records a
//! corpus fixture's outline settling a reading, the fixture is named.
//!
//! # What these fixtures cannot do
//!
//! A twin built here is this project's reading of the clause in both
//! directions, so a clause read wrongly is read wrongly twice and the pair
//! agrees with itself. The corpus census in `pdfua.rs` is the measurement
//! that breaks that symmetry, over files somebody else annotated; the rules
//! these fixtures hold were written where the corpus was not reachable, and
//! its false-alarm count over the 195 conforming fixtures is owed by the next
//! nightly run.

use tinker_pdf::{ConformanceFinding, Document, FindingKind, PdfUaPart};

// A face with real `cmap`, `glyf` and `hmtx` tables, for the width rule.
#[path = "epub_support/mod.rs"]
mod epub_support;

/// A real `sfnt` header: the version tag, one table, and a directory entry —
/// the bytes `pdfa_fonts.rs` uses, for the reason it gives: filler would be
/// refused at the first byte.
const SFNT: &[u8] = &[
    0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x00, 0x10, 0x00, 0x03, 0x00, 0x04, b'h', b'e', b'a', b'd',
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x1C, 0x00, 0x00, 0x00, 0x36,
];

/// A document under construction, with every piece a PDF/UA rule looks at
/// exposed.
///
/// Objects: 1 catalog, 2 pages, 3 the page, 4 the metadata stream, 5 the
/// font, 6 its descriptor, 7 its program, 9 the page's content, 10 the
/// structure tree root, 11 the `Document` element, 12 the parent tree, 13
/// the one `P` it holds. Objects 20 and up are a test's own.
struct Ua {
    /// `pdfuaid:part`, or `None` for a packet that claims nothing.
    part: Option<String>,
    /// The packet's `rdf:Description` children beside the claim.
    xmp: String,
    /// Entries appended inside the catalog dictionary.
    catalog: String,
    /// The catalog's `/MarkInfo`, or `None` for none.
    mark_info: Option<String>,
    /// The catalog's `/Lang`, or `None` for none.
    lang: Option<String>,
    /// Whether the catalog names a `/StructTreeRoot`.
    tagged: bool,
    /// The structure tree root's `/RoleMap` entries, or `None` for none.
    role_map: Option<String>,
    /// The metadata stream's dictionary entries.
    metadata_dict: String,
    /// The catalog's `/ViewerPreferences`.
    viewer_preferences: String,
    /// Structure elements by object number, replacing the defaults.
    elements: Vec<(u32, String)>,
    /// The page's content stream.
    content: String,
    /// The page's `/Resources`.
    resources: String,
    /// Entries appended inside the page dictionary.
    page: String,
    /// The font dictionary's entries.
    font: String,
    /// The descriptor's entries; empty for none.
    descriptor: String,
    /// The embedded program, or `None` for none.
    program: Option<Vec<u8>>,
    /// Objects 20 and up.
    extra: Vec<(u32, Vec<u8>)>,
}

impl Ua {
    /// A document claiming `part` that this build finds nothing wrong with:
    /// tagged, marked, one paragraph of text in an embedded TrueType font,
    /// a language, a title shown in the window's title bar.
    fn new(part: &str) -> Ua {
        Ua {
            part: Some(part.to_string()),
            xmp: r#"<dc:title><rdf:Alt><rdf:li xml:lang="x-default">A title</rdf:li></rdf:Alt></dc:title>"#
                .to_string(),
            catalog: String::new(),
            mark_info: Some("<< /Marked true >>".to_string()),
            lang: Some("(en)".to_string()),
            tagged: true,
            role_map: None,
            metadata_dict: "/Type /Metadata /Subtype /XML".to_string(),
            viewer_preferences: "<< /DisplayDocTitle true >>".to_string(),
            elements: vec![
                (
                    11,
                    "<< /Type /StructElem /S /Document /P 10 0 R /K [13 0 R] >>".to_string(),
                ),
                (
                    13,
                    "<< /Type /StructElem /S /P /P 11 0 R /Pg 3 0 R /K 0 >>".to_string(),
                ),
            ],
            content: "/P << /MCID 0 >> BDC BT /F1 12 Tf 10 10 Td (A) Tj ET EMC".to_string(),
            resources: "<< /Font << /F1 5 0 R >> >>".to_string(),
            page: String::new(),
            font: "/Type /Font /Subtype /TrueType /BaseFont /ABCDEF+Acme \
                   /Encoding /WinAnsiEncoding /FirstChar 65 /LastChar 65 \
                   /Widths [500] /FontDescriptor 6 0 R"
                .to_string(),
            descriptor: "/Type /FontDescriptor /FontName /ABCDEF+Acme /Flags 32 \
                         /FontBBox [0 0 1000 1000] /ItalicAngle 0 /Ascent 800 \
                         /Descent -200 /CapHeight 700 /StemV 80 /FontFile2 7 0 R"
                .to_string(),
            program: Some(SFNT.to_vec()),
            extra: Vec::new(),
        }
    }

    /// Replaces (or adds) structure element `num`.
    fn element(mut self, num: u32, body: &str) -> Ua {
        self.elements.retain(|(n, _)| *n != num);
        self.elements.push((num, body.to_string()));
        self
    }

    fn packet(&self) -> String {
        let claim = match self.part.as_deref() {
            Some("2") => r#" pdfuaid:part="2" pdfuaid:rev="2024""#.to_string(),
            Some(part) => format!(r#" pdfuaid:part="{part}""#),
            None => String::new(),
        };
        format!(
            r#"<?xpacket begin="" id="W5M0MpCehiHzreSzNTczkc9d"?>
<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF
 xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
<rdf:Description rdf:about="" xmlns:pdfuaid="http://www.aiim.org/pdfua/ns/id/"
 xmlns:dc="http://purl.org/dc/elements/1.1/"{claim}>{}</rdf:Description>
</rdf:RDF></x:xmpmeta><?xpacket end="w"?>"#,
            self.xmp
        )
    }

    fn build(&self) -> Vec<u8> {
        self.build_with_packet(&self.packet())
    }

    fn build_with_packet(&self, packet: &str) -> Vec<u8> {
        let mut catalog = format!(
            "/Type /Catalog /Pages 2 0 R /Metadata 4 0 R /ViewerPreferences {}",
            self.viewer_preferences
        );
        if let Some(mark_info) = &self.mark_info {
            catalog.push_str(&format!(" /MarkInfo {mark_info}"));
        }
        if let Some(lang) = &self.lang {
            catalog.push_str(&format!(" /Lang {lang}"));
        }
        if self.tagged {
            catalog.push_str(" /StructTreeRoot 10 0 R");
        }
        catalog.push(' ');
        catalog.push_str(&self.catalog);

        let mut objects: Vec<(u32, Vec<u8>)> = vec![
            (1, format!("<< {catalog} >>").into_bytes()),
            (2, b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec()),
            (
                3,
                format!(
                    "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] \
                     /Resources {} /Contents 9 0 R /StructParents 0 {} >>",
                    self.resources, self.page
                )
                .into_bytes(),
            ),
            (4, stream(&self.metadata_dict, packet.as_bytes())),
            (9, stream("", self.content.as_bytes())),
        ];
        if !self.font.is_empty() {
            objects.push((5, format!("<< {} >>", self.font).into_bytes()));
        }
        if !self.descriptor.is_empty() {
            objects.push((6, format!("<< {} >>", self.descriptor).into_bytes()));
        }
        if let Some(program) = &self.program {
            objects.push((7, stream(&format!("/Length1 {}", program.len()), program)));
        }
        if self.tagged {
            let role_map = self
                .role_map
                .as_ref()
                .map(|entries| format!(" /RoleMap << {entries} >>"))
                .unwrap_or_default();
            objects.push((
                10,
                format!("<< /Type /StructTreeRoot /K 11 0 R /ParentTree 12 0 R{role_map} >>")
                    .into_bytes(),
            ));
            objects.push((12, b"<< /Nums [0 [13 0 R]] >>".to_vec()));
            for (num, body) in &self.elements {
                objects.push((*num, body.clone().into_bytes()));
            }
        }
        objects.extend(self.extra.iter().cloned());
        objects.sort_by_key(|(num, _)| *num);
        objects.dedup_by_key(|(num, _)| *num);
        assemble(&objects, self.part.as_deref() == Some("2"))
    }

    /// Every finding a full validation reports.
    fn findings(&self) -> Vec<ConformanceFinding> {
        Document::open(self.build())
            .expect("the fixture opens")
            .validate_pdfua()
            .findings
    }

    /// The one finding this fixture is built to produce, as `(clause, kind)`.
    #[track_caller]
    fn one_finding(&self) -> (String, FindingKind) {
        let findings = self.findings();
        assert_eq!(
            findings.len(),
            1,
            "expected exactly one finding, got {findings:#?}"
        );
        let finding = findings.into_iter().next().expect("one");
        (finding.clause.0, finding.kind)
    }

    #[track_caller]
    fn clean(&self) {
        let findings = self.findings();
        assert!(
            findings.is_empty(),
            "expected no finding, got {findings:#?}"
        );
    }
}

/// A stream object: `entries` inside the dictionary, `data` after it.
fn stream(entries: &str, data: &[u8]) -> Vec<u8> {
    let mut body = format!("<< {} /Length {} >>\nstream\n", entries, data.len()).into_bytes();
    body.extend_from_slice(data);
    body.extend_from_slice(b"\nendstream");
    body
}

/// A classic cross-reference table around `objects`, gaps written free.
fn assemble(objects: &[(u32, Vec<u8>)], pdf_two: bool) -> Vec<u8> {
    let highest = objects.iter().map(|(n, _)| *n).max().unwrap_or(0) as usize;
    let mut out = if pdf_two {
        b"%PDF-2.0\n%\xE2\xE3\xCF\xD3\n".to_vec()
    } else {
        b"%PDF-1.7\n%\xE2\xE3\xCF\xD3\n".to_vec()
    };
    let mut offsets = vec![0u64; highest + 1];
    for (num, body) in objects {
        offsets[*num as usize] = out.len() as u64;
        out.extend_from_slice(format!("{num} 0 obj\n").as_bytes());
        out.extend_from_slice(body);
        out.extend_from_slice(b"\nendobj\n");
    }
    let xref_at = out.len() as u64;
    out.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", highest + 1).as_bytes());
    for entry in offsets.iter().skip(1) {
        if *entry == 0 {
            out.extend_from_slice(b"0000000000 65535 f \n");
        } else {
            out.extend_from_slice(format!("{entry:010} 00000 n \n").as_bytes());
        }
    }
    out.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R /ID [<0102030405060708090A0B0C0D0E0F10> \
             <0102030405060708090A0B0C0D0E0F10>] >>\nstartxref\n{xref_at}\n%%EOF\n",
            highest + 1
        )
        .as_bytes(),
    );
    out
}

// ---- the baseline ---------------------------------------------------------

/// Without this, every `one_finding` below could be passing by accident.
#[test]
fn the_baseline_is_clean_under_both_parts() {
    for part in ["1", "2"] {
        let fixture = Ua::new(part);
        fixture.clean();
        let verdict = Document::open(fixture.build())
            .expect("opens")
            .validate_pdfua();
        assert_eq!(
            verdict.part,
            Some(if part == "1" {
                PdfUaPart::One
            } else {
                PdfUaPart::Two
            })
        );
        assert!(verdict.coverage.is_complete());
        assert!(
            !verdict.abstained.is_empty(),
            "a clean verdict still names what it did not decide"
        );
    }
}

// ---- 5: version identification ----------------------------------------------

/// UA-1 5-1: "The PDF/UA version and conformance level of a file shall be
/// specified using the PDF/UA Identification extension schema". The twin is
/// the baseline, which claims part 1.
#[test]
fn a_packet_with_no_pdfuaid_part_is_reported_under_clause_5() {
    let mut fixture = Ua::new("1");
    fixture.part = None;
    assert_eq!(
        fixture.one_finding(),
        ("5".to_string(), FindingKind::PdfUaIdentifierMissing)
    );
}

/// A part ISO 14289 does not define is a finding about the claim, not an
/// absence of one.
#[test]
fn a_part_that_does_not_exist_is_reported_by_what_it_said() {
    let mut fixture = Ua::new("1");
    fixture.part = Some("3".to_string());
    assert_eq!(
        fixture.one_finding(),
        (
            "5".to_string(),
            FindingKind::PdfUaPartUnknown {
                declared: "3".to_string()
            }
        )
    );
}

/// UA-2 5-5: part 2 names its revision, a four-digit year. Missing, and a
/// value that is not a year, are two findings; the baseline's `2024` is the
/// twin.
#[test]
fn a_part_two_claim_names_its_revision_as_a_year() {
    let fixture = Ua::new("2");
    let packet = fixture.packet().replace(r#" pdfuaid:rev="2024""#, "");
    let missing = Document::open(fixture.build_with_packet(&packet))
        .expect("opens")
        .validate_pdfua()
        .findings;
    assert_eq!(missing.len(), 1, "{missing:#?}");
    assert_eq!(missing[0].kind, FindingKind::PdfUaRevisionMissing);

    let packet = fixture
        .packet()
        .replace(r#"pdfuaid:rev="2024""#, r#"pdfuaid:rev="24""#);
    let malformed = Document::open(fixture.build_with_packet(&packet))
        .expect("opens")
        .validate_pdfua()
        .findings;
    assert_eq!(malformed.len(), 1, "{malformed:#?}");
    assert_eq!(
        malformed[0].kind,
        FindingKind::PdfUaRevisionMalformed {
            declared: "24".to_string()
        }
    );
}

/// UA-1 5-3: "Property "part" of the PDF/UA Identification Schema shall have
/// namespace prefix "pdfuaid"". The claim is still read — the namespace is
/// the schema's — and the spelling is the finding.
#[test]
fn the_claim_under_another_prefix_is_read_and_its_spelling_reported() {
    let fixture = Ua::new("1");
    let packet = fixture
        .packet()
        .replace(
            r#"xmlns:pdfuaid="http://www.aiim.org/pdfua/ns/id/""#,
            r#"xmlns:ua="http://www.aiim.org/pdfua/ns/id/""#,
        )
        .replace("pdfuaid:part", "ua:part");
    let verdict = Document::open(fixture.build_with_packet(&packet))
        .expect("opens")
        .validate_pdfua();
    assert_eq!(verdict.part, Some(PdfUaPart::One), "the claim is read");
    assert_eq!(verdict.findings.len(), 1, "{:#?}", verdict.findings);
    assert_eq!(
        verdict.findings[0].kind,
        FindingKind::PdfUaIdentifierPrefix {
            property: "part".to_string(),
            found: "ua".to_string()
        }
    );
}

// ---- the structure tree -------------------------------------------------------

/// UA-1 7.1-11 / UA-2 8.2.1-1: "The logical structure of the conforming file
/// shall be described by a structure hierarchy rooted in the StructTreeRoot".
#[test]
fn an_untagged_file_is_reported_under_each_parts_number() {
    for (part, clause) in [("1", "7.1"), ("2", "8.2.1")] {
        let mut fixture = Ua::new(part);
        fixture.tagged = false;
        assert_eq!(
            fixture.one_finding(),
            (clause.to_string(), FindingKind::StructureTreeMissing)
        );
    }
}

/// Both profiles' 6.2-1: "The document catalog dictionary shall include a
/// MarkInfo dictionary containing an entry, Marked, whose value shall be
/// true".
#[test]
fn a_tree_that_is_not_marked_is_reported() {
    let mut fixture = Ua::new("1");
    fixture.mark_info = Some("<< /Marked false >>".to_string());
    assert_eq!(
        fixture.one_finding(),
        ("6.2".to_string(), FindingKind::NotMarkedAsTagged)
    );
}

/// UA-1 7.1-4: "Files shall have a Suspects value of false". The UA-2
/// profile states no such rule, so the same bytes claiming part 2 are the
/// twin.
#[test]
fn suspects_true_is_a_part_one_finding_and_nothing_under_part_two() {
    let mut one = Ua::new("1");
    one.mark_info = Some("<< /Marked true /Suspects true >>".to_string());
    assert_eq!(
        one.one_finding(),
        ("7.1".to_string(), FindingKind::MarkedSuspects)
    );
    let mut two = Ua::new("2");
    two.mark_info = Some("<< /Marked true /Suspects true >>".to_string());
    two.clean();
}

/// A `/K` graph that loops is not a hierarchy. The twin is the baseline,
/// whose tree is a tree.
#[test]
fn a_tree_whose_kids_loop_is_not_walkable() {
    let fixture = Ua::new("1").element(
        13,
        "<< /Type /StructElem /S /P /P 11 0 R /Pg 3 0 R /K [0 11 0 R] >>",
    );
    let findings = fixture.findings();
    assert!(
        findings
            .iter()
            .any(|f| f.kind == FindingKind::StructureTreeUnwalkable && f.clause.0 == "7.1"),
        "{findings:#?}"
    );
}

/// UA-1 7.3-1: "Figure tags shall include an alternative representation or
/// replacement text". `/ActualText` is the twin: a figure that *is* a word
/// says so with it (14.9.4).
#[test]
fn a_figure_with_no_alternative_is_reported_and_one_with_actual_text_is_not() {
    let bare = Ua::new("1").element(
        13,
        "<< /Type /StructElem /S /Figure /P 11 0 R /Pg 3 0 R /K 0 >>",
    );
    assert_eq!(
        bare.one_finding(),
        (
            "7.3".to_string(),
            FindingKind::AlternativeDescriptionMissing {
                structure_type: "Figure".to_string()
            }
        )
    );
    Ua::new("1")
        .element(
            13,
            "<< /Type /StructElem /S /Figure /P 11 0 R /Pg 3 0 R /K 0 /ActualText (A) >>",
        )
        .clean();
    Ua::new("1")
        .element(
            13,
            "<< /Type /StructElem /S /Figure /P 11 0 R /Pg 3 0 R /K 0 /Alt (A letter) >>",
        )
        .clean();
}

/// UA-1 7.4.2-1: "If any heading tags are used, H1 shall be the first". An
/// `H1` is the twin.
#[test]
fn a_first_heading_that_is_not_h1_is_reported_under_part_one_only() {
    let h2 = |part: &str| {
        Ua::new(part).element(
            13,
            "<< /Type /StructElem /S /H2 /P 11 0 R /Pg 3 0 R /K 0 >>",
        )
    };
    assert_eq!(
        h2("1").one_finding(),
        (
            "7.4.2".to_string(),
            FindingKind::HeadingLevelSkipped {
                previous: 0,
                level: 2
            }
        )
    );
    // The UA-2 profile carries no heading-order rule.
    h2("2").clean();
    Ua::new("1")
        .element(
            13,
            "<< /Type /StructElem /S /H1 /P 11 0 R /Pg 3 0 R /K 0 >>",
        )
        .clean();
}

/// UA-1 7.2: a document that states no natural language anywhere cannot
/// have stated one for its text. A `/Lang` on an element alone is the twin —
/// the loosest reading that is still a rule.
#[test]
fn no_language_anywhere_is_reported_and_one_on_an_element_is_enough() {
    let mut nowhere = Ua::new("1");
    nowhere.lang = None;
    assert_eq!(
        nowhere.one_finding(),
        ("7.2".to_string(), FindingKind::NaturalLanguageMissing)
    );
    let mut on_an_element = Ua::new("1").element(
        13,
        "<< /Type /StructElem /S /P /P 11 0 R /Pg 3 0 R /K 0 /Lang (en) >>",
    );
    on_an_element.lang = None;
    on_an_element.clean();
}

// ---- fonts ------------------------------------------------------------------

/// UA-1 7.21.4.1-1: "The font programs for all fonts used for rendering
/// within a conforming file shall be embedded". The standard 14 are not an
/// exception; the baseline's embedded TrueType program is the twin.
#[test]
fn an_unembedded_font_is_reported_under_each_parts_number() {
    for (part, clause) in [("1", "7.21.4.1"), ("2", "8.4.5.5.1")] {
        let mut fixture = Ua::new(part);
        fixture.font = "/Type /Font /Subtype /Type1 /BaseFont /Helvetica \
                        /Encoding /WinAnsiEncoding"
            .to_string();
        fixture.descriptor = String::new();
        fixture.program = None;
        let (found, kind) = fixture.one_finding();
        assert_eq!(found, clause);
        assert!(
            matches!(kind, FindingKind::FontNotEmbedded { ref subtype } if subtype == "Type1"),
            "{kind:?}"
        );
    }
}

// ---- milestone 2: the PDF/A font group, run for a PDF/UA claim ---------------
//
// Each of these is a rule `pdfa_fonts.rs` already holds under ISO 19005's
// numbering; the assertion here is that it now runs for a file that claims
// PDF/UA and nothing else, under ISO 14289's number.

/// A ToUnicode CMap mapping each `(code, unicode)` pair, with a one-byte or
/// two-byte codespace.
fn to_unicode(two_byte: bool, pairs: &[(u32, &str)]) -> Vec<u8> {
    let (low, high, width) = if two_byte {
        ("<0000>", "<FFFF>", 4)
    } else {
        ("<00>", "<FF>", 2)
    };
    let mut entries = String::new();
    for (code, unicode) in pairs {
        entries.push_str(&format!("<{code:0width$X}> <{unicode}>\n"));
    }
    format!(
        "/CIDInit /ProcSet findresource begin 12 dict begin begincmap\n\
         /CMapName /Adobe-Identity-UCS def\n\
         1 begincodespacerange {low} {high} endcodespacerange\n\
         {} beginbfchar\n{entries}endbfchar\n\
         endcmap CMapName currentdict /CMap defineresource pop end end",
        pairs.len()
    )
    .into_bytes()
}

/// The baseline with its one font replaced by a Type 0 font over an embedded
/// CIDFontType2: object 20 the descendant, 21 its `/ToUnicode`. Clean under
/// both parts, which [`a_type0_baseline_is_clean`] asserts.
fn type0(part: &str) -> Ua {
    let mut fixture = Ua::new(part);
    fixture.font = "/Type /Font /Subtype /Type0 /BaseFont /ABCDEF+Acme \
                    /Encoding /Identity-H /DescendantFonts [20 0 R] /ToUnicode 21 0 R"
        .to_string();
    fixture.content = "/P << /MCID 0 >> BDC BT /F1 12 Tf 10 10 Td <0041> Tj ET EMC".to_string();
    fixture.extra = vec![
        (
            20,
            format!("<< {DESCENDANT} /CIDToGIDMap /Identity >>").into_bytes(),
        ),
        (21, stream("", &to_unicode(true, &[(0x41, "0041")]))),
    ];
    fixture
}

/// The descendant's dictionary entries, replacing object 20.
fn with_descendant(mut fixture: Ua, entries: &str) -> Ua {
    fixture.extra.retain(|(n, _)| *n != 20);
    fixture
        .extra
        .push((20, format!("<< {entries} >>").into_bytes()));
    fixture
}

/// An embedded CIDFontType2 with no `/CIDToGIDMap`.
const DESCENDANT: &str = "/Type /Font /Subtype /CIDFontType2 /BaseFont /ABCDEF+Acme \
     /CIDSystemInfo << /Registry (Adobe) /Ordering (Identity) /Supplement 0 >> \
     /FontDescriptor 6 0 R /DW 1000";

#[test]
fn a_type0_baseline_is_clean() {
    type0("1").clean();
    type0("2").clean();
}

/// UA-1 7.21.3.2-1 / UA-2 8.4.5.3.2-1: an embedded Type 2 CIDFont "shall
/// contain a CIDToGIDMap entry that shall be a stream … or the name
/// Identity". The PDF/A group's `CidToGidMapMalformed`, re-numbered; the twin
/// is `type0`'s own descendant, which names `/Identity`.
#[test]
fn a_cidfont_type2_with_no_cid_to_gid_map_is_reported_under_each_part() {
    for (part, clause) in [("1", "7.21.3.2"), ("2", "8.4.5.3.2")] {
        let fixture = with_descendant(type0(part), DESCENDANT);
        let (found, kind) = fixture.one_finding();
        assert_eq!(found, clause);
        assert!(
            matches!(kind, FindingKind::CidToGidMapMalformed { .. }),
            "{kind:?}"
        );
    }
}

/// UA-1 7.21.6-2 / UA-2 8.4.5.7-2: a non-symbolic TrueType font's encoding is
/// WinAnsi or MacRoman. `StandardEncoding` is neither.
#[test]
fn a_non_symbolic_truetype_font_in_standard_encoding_is_reported() {
    for (part, clause) in [("1", "7.21.6"), ("2", "8.4.5.7")] {
        let mut fixture = Ua::new(part);
        fixture.font = fixture
            .font
            .replace("/WinAnsiEncoding", "/StandardEncoding");
        let (found, kind) = fixture.one_finding();
        assert_eq!(found, clause);
        assert_eq!(
            kind,
            FindingKind::EncodingNotStandard {
                declared: "StandardEncoding".to_string()
            }
        );
    }
}

/// The baseline's TrueType font made symbolic (descriptor flag 3), with or
/// without its `/Encoding`, and carrying a `/ToUnicode` (object 22).
fn symbolic(part: &str, encoding: bool) -> Ua {
    let mut fixture = Ua::new(part);
    fixture.descriptor = fixture.descriptor.replace("/Flags 32", "/Flags 4");
    if !encoding {
        fixture.font = fixture.font.replace("/Encoding /WinAnsiEncoding ", "");
    }
    fixture.font.push_str(" /ToUnicode 22 0 R");
    fixture
        .extra
        .push((22, stream("", &to_unicode(false, &[(0x41, "0041")]))));
    fixture
}

/// UA-1 7.21.6-3: "Symbolic TrueType fonts shall not contain an Encoding
/// entry". The twin drops the `/Encoding` and is clean.
#[test]
fn a_symbolic_truetype_font_with_an_encoding_is_reported_and_one_without_is_not() {
    assert_eq!(
        symbolic("1", true).one_finding(),
        ("7.21.6".to_string(), FindingKind::SymbolicFontHasEncoding)
    );
    symbolic("1", false).clean();
    symbolic("2", false).clean();
}

/// UA-1 7.21.7-1 / UA-2 8.4.5.8-1: every drawn code mapped to Unicode. A
/// symbolic TrueType font with no `/ToUnicode` maps nothing — its program's
/// `cmap` maps codes to glyphs, and a glyph is not a character. The twin is
/// [`symbolic`]'s clean fixture, which carries one.
#[test]
fn a_symbolic_truetype_font_with_no_to_unicode_is_reported() {
    for (part, clause) in [("1", "7.21.7"), ("2", "8.4.5.8")] {
        let mut fixture = Ua::new(part);
        fixture.descriptor = fixture.descriptor.replace("/Flags 32", "/Flags 4");
        fixture.font = fixture.font.replace("/Encoding /WinAnsiEncoding ", "");
        assert_eq!(
            fixture.one_finding(),
            (clause.to_string(), FindingKind::ToUnicodeMissing)
        );
    }
}

/// "Used for rendering", as the PDF/A group reads it: an unembedded font a
/// page names and never draws, or draws only at rendering mode 3, is not
/// reported. The census rule this group replaced judged every font a page's
/// resources named; veraPDF's 7.21.4.1-1 is stated over fonts used for
/// rendering and exempts `renderingMode == 3`.
#[test]
fn an_unembedded_font_that_is_never_drawn_visibly_is_not_reported() {
    let mut unused = Ua::new("1");
    unused.resources = "<< /Font << /F1 5 0 R /F2 23 0 R >> >>".to_string();
    unused.extra.push((
        23,
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_vec(),
    ));
    unused.clean();

    let mut invisible = Ua::new("1");
    invisible.font = "/Type /Font /Subtype /Type1 /BaseFont /Helvetica \
                      /Encoding /WinAnsiEncoding"
        .to_string();
    invisible.descriptor = String::new();
    invisible.program = None;
    invisible.content = "/P << /MCID 0 >> BDC BT 3 Tr /F1 12 Tf 10 10 Td (A) Tj ET EMC".to_string();
    invisible.clean();
}

// ---- milestone 2: the encoding CMap -----------------------------------------

/// A CMap stream: `dict` inside its dictionary, the identity codespace and
/// one `cidrange`, and `program_extra` in the program body.
fn cmap_stream(dict: &str, program_extra: &str) -> Vec<u8> {
    let program = format!(
        "/CIDInit /ProcSet findresource begin 12 dict begin begincmap\n\
         {program_extra}\n\
         /CMapName /Acme-H def\n\
         1 begincodespacerange <0000> <FFFF> endcodespacerange\n\
         1 begincidrange <0000> <FFFF> 0 endcidrange\n\
         endcmap CMapName currentdict /CMap defineresource pop end end"
    );
    stream(
        &format!("/Type /CMap /CMapName /Acme-H {dict}"),
        program.as_bytes(),
    )
}

/// [`type0`] with its `/Encoding` an embedded CMap, object 24.
fn with_cmap(part: &str, dict: &str, program_extra: &str) -> Ua {
    let mut fixture = type0(part);
    fixture.font = fixture.font.replace("/Identity-H", "24 0 R");
    fixture.extra.push((24, cmap_stream(dict, program_extra)));
    fixture
}

const IDENTITY_COLLECTION: &str =
    "/CIDSystemInfo << /Registry (Adobe) /Ordering (Identity) /Supplement 0 >>";

/// UA-1 7.21.3.3-1 / UA-2 8.4.5.4-1: a CMap outside Table 118 is embedded. A
/// name nobody predefines is the finding; `Identity-H`, in the baseline, is
/// one twin, and an embedded CMap of the same name is the other.
#[test]
fn a_cmap_named_but_not_predefined_is_reported_and_one_embedded_is_not() {
    for (part, clause) in [("1", "7.21.3.3"), ("2", "8.4.5.4")] {
        let mut fixture = type0(part);
        fixture.font = fixture.font.replace("/Identity-H", "/Acme-H");
        assert_eq!(
            fixture.one_finding(),
            (
                clause.to_string(),
                FindingKind::CMapNotEmbedded {
                    name: "Acme-H".to_string()
                }
            )
        );
        with_cmap(part, IDENTITY_COLLECTION, "").clean();
    }
}

/// UA-1 7.21.3.3-2: an embedded CMap's dictionary `/WMode` is "identical to
/// the WMode value in the embedded CMap stream". Both default to 0; a
/// dictionary saying 1 over a program saying nothing is the finding, and the
/// program saying 1 as well is the twin.
#[test]
fn an_embedded_cmap_whose_writing_modes_disagree_is_reported() {
    let disagreeing = with_cmap("1", &format!("{IDENTITY_COLLECTION} /WMode 1"), "");
    assert_eq!(
        disagreeing.one_finding(),
        (
            "7.21.3.3".to_string(),
            FindingKind::CMapWritingModeMismatch {
                dictionary: 1,
                program: 0
            }
        )
    );
    with_cmap(
        "1",
        &format!("{IDENTITY_COLLECTION} /WMode 1"),
        "/WMode 1 def",
    )
    .clean();
}

/// UA-1 7.21.3.3-3 / UA-2 8.4.5.4-3: "A CMap shall not reference any other
/// CMap except those listed in … Table 118" — by the dictionary's `/UseCMap`
/// or by the program's `usecmap`. `Identity-H` is on the list and is the
/// twin.
#[test]
fn an_embedded_cmap_using_a_cmap_off_the_list_is_reported() {
    let by_dictionary = with_cmap(
        "1",
        &format!("{IDENTITY_COLLECTION} /UseCMap /Acme-Base"),
        "",
    );
    assert_eq!(
        by_dictionary.one_finding(),
        (
            "7.21.3.3".to_string(),
            FindingKind::CMapReferenceNotStandard {
                name: "Acme-Base".to_string()
            }
        )
    );
    let by_program = with_cmap("2", IDENTITY_COLLECTION, "/Acme-Base usecmap");
    assert_eq!(
        by_program.one_finding(),
        (
            "8.4.5.4".to_string(),
            FindingKind::CMapReferenceNotStandard {
                name: "Acme-Base".to_string()
            }
        )
    );
    with_cmap(
        "1",
        &format!("{IDENTITY_COLLECTION} /UseCMap /Identity-H"),
        "",
    )
    .clean();
}

/// UA-1 7.21.3.1-1: with a CMap other than the identity ones, "the
/// corresponding Registry and Ordering strings in both CIDSystemInfo
/// dictionaries shall be identical, and the value of the Supplement key in
/// the CIDSystemInfo dictionary of the CIDFont shall be less than or equal
/// to the Supplement key in the CIDSystemInfo dictionary of the CMap".
#[test]
fn an_embedded_cmap_of_another_collection_is_reported() {
    let other = with_cmap(
        "1",
        "/CIDSystemInfo << /Registry (Adobe) /Ordering (Japan1) /Supplement 2 >>",
        "",
    );
    assert_eq!(
        other.one_finding(),
        (
            "7.21.3.1".to_string(),
            FindingKind::CidSystemInfoMismatch {
                key: "Ordering".to_string()
            }
        )
    );
    // The CIDFont's supplement above the CMap's.
    let newer = with_descendant(
        with_cmap("2", IDENTITY_COLLECTION, ""),
        &format!(
            "{} /CIDToGIDMap /Identity",
            DESCENDANT.replace("/Supplement 0", "/Supplement 3")
        ),
    );
    assert_eq!(
        newer.one_finding(),
        (
            "8.4.5.3.1".to_string(),
            FindingKind::CidSystemInfoMismatch {
                key: "Supplement".to_string()
            }
        )
    );
    // The twin: the CMap's supplement above the CIDFont's, which the clause
    // admits.
    with_cmap(
        "1",
        "/CIDSystemInfo << /Registry (Adobe) /Ordering (Identity) /Supplement 5 >>",
        "",
    )
    .clean();
}

// ---- milestone 2: ToUnicode values ------------------------------------------

/// The baseline with a `/ToUnicode` (object 22) mapping `pairs`.
fn mapped(part: &str, pairs: &[(u32, &str)]) -> Ua {
    let mut fixture = Ua::new(part);
    fixture.font.push_str(" /ToUnicode 22 0 R");
    fixture
        .extra
        .push((22, stream("", &to_unicode(false, pairs))));
    fixture
}

/// UA-1 7.21.7-2 / UA-2 8.4.5.8-2: "The Unicode values specified in the
/// ToUnicode CMap shall all be greater than zero (0), but not equal to either
/// U+FEFF or U+FFFE." Judged over the codes a page draws: an entry for a code
/// nothing shows is one twin, and a drawn code mapped to `A` is the other.
#[test]
fn a_drawn_code_mapped_to_a_forbidden_value_is_reported() {
    for (part, clause, value) in [("1", "7.21.7", 0x0000u32), ("2", "8.4.5.8", 0xFEFF)] {
        let hex = format!("{value:04X}");
        assert_eq!(
            mapped(part, &[(0x41, &hex)]).one_finding(),
            (
                clause.to_string(),
                FindingKind::ToUnicodeValueForbidden { code: 0x41, value }
            )
        );
    }
    mapped("1", &[(0x41, "0041"), (0x42, "FFFE")]).clean();
}

// ---- milestone 2: natural language ------------------------------------------

/// UA-1 7.2-29 / UA-2 8.4.4-2: a `/Lang` is RFC 3066's language tag, and the
/// empty string is not one — the reading PDF/A does not take
/// (`pdfa::logical`'s `the_empty_language_is_pdfa_conforming_and_pdfua_not`
/// holds both sides of the flag).
#[test]
fn an_empty_or_malformed_lang_is_reported_and_a_well_formed_one_is_not() {
    let mut empty = Ua::new("1");
    empty.lang = Some("()".to_string());
    assert_eq!(
        empty.one_finding(),
        (
            "7.2".to_string(),
            FindingKind::LanguageMalformed {
                declared: String::new()
            }
        )
    );
    let on_an_element = Ua::new("1").element(
        13,
        "<< /Type /StructElem /S /P /P 11 0 R /Pg 3 0 R /K 0 /Lang (en-) >>",
    );
    assert_eq!(
        on_an_element.one_finding(),
        (
            "7.2".to_string(),
            FindingKind::LanguageMalformed {
                declared: "en-".to_string()
            }
        )
    );
    // RFC 3066's alphanumeric subtag, eight long: `1234abcd` passes in the
    // corpus's 7.2-t29 outlines as the design records them.
    let mut alphanumeric = Ua::new("2");
    alphanumeric.lang = Some("(en-1234abcd)".to_string());
    alphanumeric.clean();
}

/// UA-2 8.4.4-1: "The default natural language … shall be specified using
/// the Lang entry, with a non-empty value, in the catalog dictionary". Under
/// part 1 a `/Lang` on an element suffices, which is the twin.
#[test]
fn part_two_requires_the_catalogs_own_language() {
    for lang in [None, Some("()")] {
        let mut fixture = Ua::new("2").element(
            13,
            "<< /Type /StructElem /S /P /P 11 0 R /Pg 3 0 R /K 0 /Lang (en) >>",
        );
        fixture.lang = lang.map(str::to_string);
        assert_eq!(
            fixture.one_finding(),
            ("8.4.4".to_string(), FindingKind::CatalogLanguageMissing)
        );
    }
    let mut one = Ua::new("1").element(
        13,
        "<< /Type /StructElem /S /P /P 11 0 R /Pg 3 0 R /K 0 /Lang (en) >>",
    );
    one.lang = None;
    one.clean();
}

// ---- milestone 2: structure types and parents -------------------------------

/// The baseline with a `/RoleMap` on the structure tree root and element 13
/// replaced.
fn with_role_map(part: &str, role_map: &str, element: &str) -> Ua {
    let mut fixture = Ua::new(part).element(13, element);
    fixture.role_map = Some(role_map.to_string());
    fixture
}

/// UA-1 7.1-5: a non-standard type "shall be mapped to the nearest
/// functionally equivalent standard type". The PDF/A level A rule,
/// re-numbered; the twin maps it. The UA-2 rules are stated over PDF 2.0's
/// namespaces and are staged, so the same bytes claiming part 2 are clean.
#[test]
fn a_non_standard_type_with_no_mapping_is_reported_and_a_mapped_one_is_not() {
    let unmapped = |part: &str| {
        Ua::new(part).element(
            13,
            "<< /Type /StructElem /S /Para /P 11 0 R /Pg 3 0 R /K 0 >>",
        )
    };
    assert_eq!(
        unmapped("1").one_finding(),
        (
            "7.1".to_string(),
            FindingKind::StructureTypeNotStandard {
                declared: "Para".to_string(),
                mapped: "Para".to_string()
            }
        )
    );
    unmapped("2").clean();
    with_role_map(
        "1",
        "/Para /P",
        "<< /Type /StructElem /S /Para /P 11 0 R /Pg 3 0 R /K 0 >>",
    )
    .clean();
}

/// UA-1 7.1-7: "Standard tags … shall not be remapped." An identity entry
/// (`/P /P`), the commonest role-map entry in the wild, is the twin.
#[test]
fn a_remapped_standard_type_is_reported_and_an_identity_entry_is_not() {
    let remapped = with_role_map(
        "1",
        "/P /Span",
        "<< /Type /StructElem /S /P /P 11 0 R /Pg 3 0 R /K 0 >>",
    );
    assert_eq!(
        remapped.one_finding(),
        (
            "7.1".to_string(),
            FindingKind::StandardTypeRemapped {
                declared: "P".to_string(),
                mapped: "Span".to_string()
            }
        )
    );
    with_role_map(
        "1",
        "/P /P",
        "<< /Type /StructElem /S /P /P 11 0 R /Pg 3 0 R /K 0 >>",
    )
    .clean();
}

/// UA-1 7.1-12 / UA-2 8.2.1-2: "A structure element dictionary shall contain
/// the P (parent) entry".
#[test]
fn a_structure_element_with_no_parent_entry_is_reported() {
    for (part, clause) in [("1", "7.1"), ("2", "8.2.1")] {
        let orphan = Ua::new(part).element(13, "<< /Type /StructElem /S /P /Pg 3 0 R /K 0 >>");
        assert_eq!(
            orphan.one_finding(),
            (clause.to_string(), FindingKind::StructureParentMissing)
        );
    }
}

// ---- milestone 2: the catalog and its metadata -------------------------------

/// UA-1 7.1-9 / UA-2 8.11.1-1: the packet carries `dc:title`.
#[test]
fn a_packet_with_no_title_is_reported() {
    for (part, clause) in [("1", "7.1"), ("2", "8.11.1")] {
        let mut fixture = Ua::new(part);
        fixture.xmp = String::new();
        assert_eq!(
            fixture.one_finding(),
            (clause.to_string(), FindingKind::DocumentTitleMissing)
        );
    }
}

/// UA-1 7.1-8 / UA-2 8.11.1-2: the metadata stream's `/Type /Metadata` and
/// `/Subtype /XML`. The twin is the baseline's stream.
#[test]
fn a_metadata_stream_of_the_wrong_subtype_is_reported() {
    for (part, clause) in [("1", "7.1"), ("2", "8.11.1")] {
        let mut fixture = Ua::new(part);
        fixture.metadata_dict = "/Type /Metadata /Subtype /XYZ".to_string();
        let findings = fixture.findings();
        assert!(
            findings.iter().any(|f| f.clause.0 == clause
                && f.kind
                    == FindingKind::MetadataStreamMalformed {
                        key: "Subtype".to_string()
                    }),
            "{findings:#?}"
        );
    }
}

/// UA-1 7.1-10 / UA-2 8.11.2-1: `/DisplayDocTitle true`.
#[test]
fn display_doc_title_false_is_reported() {
    for (part, clause) in [("1", "7.1"), ("2", "8.11.2")] {
        let mut fixture = Ua::new(part);
        fixture.viewer_preferences = "<< /DisplayDocTitle false >>".to_string();
        assert_eq!(
            fixture.one_finding(),
            (clause.to_string(), FindingKind::DisplayDocTitleNotSet)
        );
    }
}

// ---- milestone 2: optional content, embedded files, XObjects, XFA -----------

/// The baseline with an `/OCProperties` whose configurations are `oc`.
fn with_oc(part: &str, oc: &str) -> Ua {
    let mut fixture = Ua::new(part);
    fixture.catalog = format!("/OCProperties << /OCGs [25 0 R] {oc} >>");
    fixture
        .extra
        .push((25, b"<< /Type /OCG /Name (Layer) >>".to_vec()));
    fixture
}

/// UA-1 7.10-1 and -2 / UA-2 8.7-1 and -2.
///
/// The parts differ on naming and the test asserts the difference: part 1
/// names the default configuration always, part 2 only once `/Configs` holds
/// one. `/AS` is forbidden under both.
#[test]
fn optional_content_configurations_are_named_and_carry_no_auto_state() {
    let unnamed_default = "/D << /Order [25 0 R] >>";
    assert_eq!(
        with_oc("1", unnamed_default).one_finding(),
        (
            "7.10".to_string(),
            FindingKind::OptionalContentConfigUnnamed
        )
    );
    with_oc("2", unnamed_default).clean();
    assert_eq!(
        with_oc(
            "2",
            "/D << /Order [25 0 R] >> /Configs [<< /Name (Other) /Order [25 0 R] >>]",
        )
        .one_finding(),
        ("8.7".to_string(), FindingKind::OptionalContentConfigUnnamed)
    );
    for (part, clause) in [("1", "7.10"), ("2", "8.7")] {
        assert_eq!(
            with_oc(
                part,
                "/D << /Name (Default) /AS [<< /Event /View /Category [/Zoom] /OCGs [25 0 R] >>] >>"
            )
            .one_finding(),
            (clause.to_string(), FindingKind::OptionalContentConfigAutoState)
        );
    }
    with_oc("1", "/D << /Name (Default) /Order [25 0 R] >>").clean();
}

/// An embedded file (object 26), its specification (27, carrying `spec`) and
/// the `/EmbeddedFiles` name tree naming it (28).
fn with_attachment(part: &str, spec: &str) -> Ua {
    let mut fixture = Ua::new(part);
    fixture.catalog = "/Names << /EmbeddedFiles 28 0 R >>".to_string();
    fixture
        .extra
        .push((26, stream("/Type /EmbeddedFile", b"hello")));
    fixture.extra.push((
        27,
        format!("<< /Type /Filespec /EF << /F 26 0 R >> {spec} >>").into_bytes(),
    ));
    fixture
        .extra
        .push((28, b"<< /Names [(a.txt) 27 0 R] >>".to_vec()));
    fixture
}

/// UA-1 7.11-1: "The file specification dictionary for an embedded file
/// shall contain the non-empty F and UF keys". UA-2 8.14.1-1 asks a
/// different thing — a `/Desc` — and each part's fixture is the other's
/// twin.
#[test]
fn an_embedded_file_specification_carries_what_its_part_asks() {
    let empty_uf = "/F (a.txt) /UF () /Desc (A note)";
    assert_eq!(
        with_attachment("1", empty_uf).one_finding(),
        (
            "7.11".to_string(),
            FindingKind::EmbeddedFileKeyMissing {
                key: "UF".to_string()
            }
        )
    );
    with_attachment("2", empty_uf).clean();

    let no_desc = "/F (a.txt) /UF (a.txt)";
    assert_eq!(
        with_attachment("2", no_desc).one_finding(),
        (
            "8.14".to_string(),
            FindingKind::EmbeddedFileKeyMissing {
                key: "Desc".to_string()
            }
        )
    );
    with_attachment("1", no_desc).clean();
}

/// UA-1 7.20-1: "A conforming file shall not contain any reference XObjects".
/// The UA-2 profile states no such rule, so the same bytes claiming part 2
/// are the twin.
#[test]
fn a_reference_xobject_is_a_part_one_finding() {
    let with_ref = |part: &str| {
        let mut fixture = Ua::new(part);
        fixture.extra.push((
            29,
            stream(
                "/Type /XObject /Subtype /Form /BBox [0 0 10 10] \
                 /Ref << /F (other.pdf) /Page 0 >>",
                b"",
            ),
        ));
        fixture
    };
    assert_eq!(
        with_ref("1").one_finding(),
        ("7.20".to_string(), FindingKind::ReferenceXObjectForbidden)
    );
    with_ref("2").clean();
}

/// UA-2 8.10.1-3: "XFA forms shall not be present." Part 1 forbids only
/// dynamic XFA, which is staged, so the same bytes claiming part 1 are the
/// twin.
#[test]
fn an_xfa_form_is_a_part_two_finding() {
    let with_xfa = |part: &str| {
        let mut fixture = Ua::new(part);
        fixture.catalog = "/AcroForm << /Fields [] /XFA 30 0 R >>".to_string();
        fixture.extra.push((30, stream("", b"<xdp:xdp/>")));
        fixture
    };
    assert_eq!(
        with_xfa("2").one_finding(),
        ("8.10.1".to_string(), FindingKind::XfaForbidden)
    );
    with_xfa("1").clean();
}

/// UA-1 7.16-1: an encrypted file's `/P` sets bit 10, which ISO 32000-1 Table
/// 22 says permits extraction "in support of accessibility". The file is the
/// baseline re-saved by this engine's own writer with an empty user
/// password, which the reader is then given; `-1`, every bit, is the twin.
#[test]
fn an_encrypted_file_that_withholds_accessibility_is_reported() {
    let encrypted = |permissions: i32| {
        let bytes = Document::open(Ua::new("1").build())
            .expect("opens")
            .editor()
            .save(&tinker_pdf::WriteOptions {
                mode: tinker_pdf::WriteMode::Rewrite,
                encryption: Some(tinker_pdf::Encryption {
                    user_password: String::new(),
                    owner_password: "owner".to_string(),
                    permissions,
                    entropy: [9; 48],
                }),
                ..tinker_pdf::WriteOptions::default()
            });
        let document = Document::open(bytes).expect("it opens");
        document
            .authenticate("")
            .expect("the empty user password authenticates");
        document
    };
    let withheld = encrypted(!(1 << 9)).validate_pdfua().findings;
    assert_eq!(withheld.len(), 1, "{withheld:#?}");
    assert_eq!(withheld[0].clause.0, "7.16");
    assert!(
        matches!(
            withheld[0].kind,
            FindingKind::AccessibilityPermissionWithheld {
                permissions: Some(_)
            }
        ),
        "{:?}",
        withheld[0].kind
    );
    let clean = encrypted(-1).validate_pdfua().findings;
    assert!(clean.is_empty(), "{clean:#?}");
}

// ---- the width rule, run for a PDF/UA claim -----------------------------------

/// UA-1 7.21.5 / UA-2 8.4.5.6: "the glyph width information in the font
/// dictionary and in the embedded font program shall be consistent" — the
/// PDF/A group's width rule, run for a PDF/UA claim and re-numbered. The
/// baseline's program has no `hmtx` to read, so this fixture embeds a real
/// face; the same face with the matching width is the twin.
#[test]
fn a_width_the_program_disagrees_with_is_reported_under_each_part() {
    let measured = |part: &str, widths: &str| {
        let mut fixture = Ua::new(part);
        fixture.program = Some(
            epub_support::typeface::Face::new("Acme", "A")
                .with_advance(600)
                .build(),
        );
        fixture.font = fixture
            .font
            .replace("/Widths [500]", &format!("/Widths [{widths}]"));
        fixture
    };
    for (part, clause) in [("1", "7.21.5"), ("2", "8.4.5.6")] {
        assert_eq!(
            measured(part, "500").one_finding(),
            (
                clause.to_string(),
                FindingKind::GlyphWidthInconsistent {
                    code: 65,
                    dictionary: 500,
                    program: 600
                }
            )
        );
        measured(part, "600").clean();
    }
}

// ---- 7.2, 7.4.4, 7.9: the structure grammar -----------------------------------

/// The baseline with `elements` under its `Document` beside the paragraph:
/// each `(number, structure type, parent number, kid numbers, extra)` written
/// as an indirect element with its `/P`, its `/K` the kids given.
fn grammar(part: &str, elements: &[(u32, &str, u32, &[u32], &str)]) -> Ua {
    let top: Vec<String> = elements
        .iter()
        .filter(|(_, _, parent, _, _)| *parent == 11)
        .map(|(num, _, _, _, _)| format!("{num} 0 R"))
        .collect();
    let mut fixture = Ua::new(part).element(
        11,
        &format!(
            "<< /Type /StructElem /S /Document /P 10 0 R /K [13 0 R {}] >>",
            top.join(" ")
        ),
    );
    for (num, structure_type, parent, kids, extra) in elements {
        let kids: Vec<String> = kids.iter().map(|k| format!("{k} 0 R")).collect();
        fixture = fixture.element(
            *num,
            &format!(
                "<< /Type /StructElem /S /{structure_type} /P {parent} 0 R /K [{}] {extra} >>",
                kids.join(" ")
            ),
        );
    }
    fixture
}

/// A table of one row and one cell, under `parent`, numbered from `first`.
fn table(first: u32, parent: u32) -> Vec<(u32, &'static str, u32, Vec<u32>, &'static str)> {
    vec![
        (first, "Table", parent, vec![first + 1], ""),
        (first + 1, "TR", first, vec![first + 2], ""),
        (first + 2, "TD", first + 1, vec![], ""),
    ]
}

/// [`grammar`] over owned rows.
fn grammar_of(part: &str, rows: &[(u32, &str, u32, Vec<u32>, &str)]) -> Ua {
    let borrowed: Vec<(u32, &str, u32, &[u32], &str)> = rows
        .iter()
        .map(|(n, s, p, k, e)| (*n, *s, *p, k.as_slice(), *e))
        .collect();
    grammar(part, &borrowed)
}

/// UA-1 7.2-3 and 7.2-10, veraPDF's statements: "Table element may contain
/// only TR, THead, TBody, TFoot and Caption elements", "TR element may
/// contain only TH and TD elements". The twin is the well-formed table.
#[test]
fn a_table_and_a_row_contain_only_what_14_8_4_admits() {
    grammar_of("1", &table(20, 11)).clean();

    let mut extra_kid = table(20, 11);
    extra_kid[0].3.push(30);
    extra_kid.push((30, "P", 20, vec![], ""));
    assert_eq!(
        grammar_of("1", &extra_kid).one_finding(),
        (
            "7.2".to_string(),
            FindingKind::StructureKidNotAdmitted {
                element: "Table".to_string(),
                kid: "P".to_string(),
            }
        )
    );

    let mut row_kid = table(20, 11);
    row_kid[1].3.push(30);
    row_kid.push((30, "Span", 21, vec![], ""));
    assert_eq!(
        grammar_of("1", &row_kid).one_finding(),
        (
            "7.2".to_string(),
            FindingKind::StructureKidNotAdmitted {
                element: "TR".to_string(),
                kid: "Span".to_string(),
            }
        )
    );

    // The same defect claiming part 2 is not 7.2's: ISO 14289-2 states its
    // grammar differently, and that is staged under 8.2.
    grammar_of("2", &extra_kid).clean();
}

/// UA-1 7.2-4 to 7.2-9, 7.2-17, 7.2-18 and 7.2-26: a row in a table or a
/// table section, a cell in a row, a list item in a list, a list body in an
/// item, a TOC item in a TOC.
#[test]
fn a_constrained_element_sits_in_the_parent_14_8_4_gives_it() {
    for (structure_type, kids) in [
        ("TR", vec![]),
        ("LI", vec![]),
        ("TOCI", vec![]),
        ("LBody", vec![]),
    ] {
        let rows = vec![(20, structure_type, 11, kids, "")];
        assert_eq!(
            grammar_of("1", &rows).one_finding(),
            (
                "7.2".to_string(),
                FindingKind::StructureParentNotAdmitted {
                    element: structure_type.to_string(),
                    parent: "Document".to_string(),
                }
            ),
            "{structure_type}"
        );
    }
    grammar_of(
        "1",
        &[
            (20, "L", 11, vec![21], ""),
            (21, "LI", 20, vec![22, 23], ""),
            (22, "Lbl", 21, vec![], ""),
            (23, "LBody", 21, vec![], ""),
            (24, "TOC", 11, vec![25], ""),
            (25, "TOCI", 24, vec![], ""),
        ],
    )
    .clean();
}

/// UA-1 7.2-11 to 7.2-14 and 7.2-39: one `THead`, one `TFoot` and one
/// `Caption` at most, and a `TBody` beside a `THead` or a `TFoot`.
#[test]
fn a_tables_sections_and_caption_are_counted() {
    let sections = |kinds: &[&'static str]| {
        let mut rows = vec![(20, "Table", 11, Vec::new(), "")];
        let mut next = 30;
        for kind in kinds {
            rows[0].3.push(next);
            if *kind == "Caption" {
                rows.push((next, "Caption", 20, vec![], ""));
                next += 1;
            } else {
                rows.push((next, *kind, 20, vec![next + 1], ""));
                rows.push((next + 1, "TR", next, vec![], ""));
                next += 2;
            }
        }
        grammar_of("1", &rows)
    };
    sections(&["THead", "TBody", "TFoot"]).clean();
    sections(&["Caption", "THead", "TBody"]).clean();
    assert_eq!(
        sections(&["THead", "THead", "TBody"]).one_finding(),
        (
            "7.2".to_string(),
            FindingKind::StructureKidRepeated {
                element: "Table".to_string(),
                kid: "THead".to_string(),
                count: 2,
            }
        )
    );
    assert_eq!(
        sections(&["TBody", "Caption"]).findings().len(),
        0,
        "a caption last is admitted"
    );
    assert_eq!(
        sections(&["Caption", "TBody", "Caption"]).one_finding(),
        (
            "7.2".to_string(),
            FindingKind::StructureKidRepeated {
                element: "Table".to_string(),
                kid: "Caption".to_string(),
                count: 2,
            }
        )
    );
    for beside in ["THead", "TFoot"] {
        assert_eq!(
            sections(&[beside]).one_finding(),
            (
                "7.2".to_string(),
                FindingKind::TableBodyMissing {
                    beside: beside.to_string(),
                }
            )
        );
    }
}

/// UA-1 7.2-16, 7.2-28 and 7.2-40: a table's caption first or last, a TOC's
/// and a list's first only.
#[test]
fn a_caption_is_where_14_8_4_puts_it() {
    let middle = grammar_of(
        "1",
        &[
            (20, "Table", 11, vec![21, 23, 24], ""),
            (21, "TR", 20, vec![], ""),
            (23, "Caption", 20, vec![], ""),
            (24, "TR", 20, vec![], ""),
        ],
    );
    assert_eq!(
        middle.one_finding(),
        (
            "7.2".to_string(),
            FindingKind::CaptionMisplaced {
                element: "Table".to_string(),
            }
        )
    );
    for (container, item) in [("L", "LI"), ("TOC", "TOCI")] {
        let last = grammar_of(
            "1",
            &[
                (20, container, 11, vec![21, 22], ""),
                (21, item, 20, vec![], ""),
                (22, "Caption", 20, vec![], ""),
            ],
        );
        assert_eq!(
            last.one_finding(),
            (
                "7.2".to_string(),
                FindingKind::CaptionMisplaced {
                    element: container.to_string(),
                }
            ),
            "{container}"
        );
        grammar_of(
            "1",
            &[
                (20, container, 11, vec![22, 21], ""),
                (21, item, 20, vec![], ""),
                (22, "Caption", 20, vec![], ""),
            ],
        )
        .clean();
    }
}

/// UA-1 7.4.4-1: "Each node in the tag tree shall contain at most one child
/// H tag"; 7.4.4-2 and -3: "All documents shall be either strongly or weakly
/// structured, but not both" — an `H` and an `Hn` in one document. The twins
/// are one `H`, and `H` in two sibling sections.
#[test]
fn unnumbered_headings_are_one_per_node_and_never_beside_numbered_ones() {
    assert_eq!(
        grammar_of("1", &[(20, "H", 11, vec![], ""), (21, "H", 11, vec![], "")]).one_finding(),
        (
            "7.4.4".to_string(),
            FindingKind::StructureKidRepeated {
                element: "Document".to_string(),
                kid: "H".to_string(),
                count: 2,
            }
        )
    );
    grammar_of(
        "1",
        &[
            (20, "Sect", 11, vec![21], ""),
            (21, "H", 20, vec![], ""),
            (22, "Sect", 11, vec![23], ""),
            (23, "H", 22, vec![], ""),
        ],
    )
    .clean();
    assert_eq!(
        grammar_of(
            "1",
            &[(20, "H", 11, vec![], ""), (21, "H1", 11, vec![], "")]
        )
        .one_finding(),
        ("7.4.4".to_string(), FindingKind::HeadingKindsMixed)
    );
}

/// UA-1 7.9-1 and 7.9-2: "Note tag shall have ID entry", "Each Note tag
/// shall have unique ID key". The twin is two notes with two identifiers.
#[test]
fn a_note_carries_an_id_of_its_own() {
    grammar_of(
        "1",
        &[
            (20, "Note", 11, vec![], "/ID (n1)"),
            (21, "Note", 11, vec![], "/ID (n2)"),
        ],
    )
    .clean();
    assert_eq!(
        grammar_of("1", &[(20, "Note", 11, vec![], "")]).one_finding(),
        ("7.9".to_string(), FindingKind::NoteIdMissing)
    );
    assert_eq!(
        grammar_of("1", &[(20, "Note", 11, vec![], "/ID ()")]).one_finding(),
        ("7.9".to_string(), FindingKind::NoteIdMissing)
    );
    assert_eq!(
        grammar_of(
            "1",
            &[
                (20, "Note", 11, vec![], "/ID (n1)"),
                (21, "Note", 11, vec![], "/ID (n1)"),
            ],
        )
        .one_finding(),
        (
            "7.9".to_string(),
            FindingKind::NoteIdDuplicate {
                id: "n1".to_string(),
            }
        )
    );
}

/// UA-1 7.3-1 and 7.7-1, veraPDF's condition for both: `(Alt != null && Alt
/// != '') || ActualText != null`. An empty `/Alt` is no description and an
/// empty `/ActualText` is a replacement (7.3-t01-pass-c against -fail-b, and
/// 7.7's pair). Part 2's 8.2.5.28.2 condition is `Alt != null`, so the empty
/// `/Alt` is the twin there; and part 2 states no `Formula` rule.
#[test]
fn an_empty_alt_describes_nothing_under_part_one_and_an_empty_actual_text_stands() {
    for (structure_type, clause) in [("Figure", "7.3"), ("Formula", "7.7")] {
        let with = |part: &str, entries: &str| {
            grammar_of(part, &[(20, structure_type, 11, vec![], entries)])
        };
        assert_eq!(
            with("1", "/Alt ()").one_finding(),
            (
                clause.to_string(),
                FindingKind::AlternativeDescriptionMissing {
                    structure_type: structure_type.to_string(),
                }
            ),
            "{structure_type}"
        );
        assert_eq!(
            with("1", "").one_finding().0,
            clause,
            "{structure_type} with neither"
        );
        with("1", "/ActualText ()").clean();
        with("1", "/Alt (A sum)").clean();
    }
    grammar_of("2", &[(20, "Figure", 11, vec![], "/Alt ()")]).clean();
    grammar_of("2", &[(20, "Formula", 11, vec![], "")]).clean();
}

// ---- 7.18: annotations against the structure ---------------------------------

/// The baseline with one annotation, object 30, on the page: `annotation` its
/// dictionary's entries after the subtype, `tabs` the page's own entries
/// (normally `/Tabs /S`), and, when `enclosing` names a structure type, an
/// element 20 of that type under the `Document` whose one kid is an `/OBJR`
/// to the annotation, with `element` in its dictionary. Objects 31 and up are
/// the test's own.
fn annotated(
    part: &str,
    subtype: &str,
    annotation: &str,
    tabs: &str,
    enclosing: Option<(&str, &str)>,
) -> Ua {
    let mut fixture = Ua::new(part);
    fixture.page = format!("/Annots [30 0 R] {tabs}");
    fixture.extra.push((
        30,
        format!("<< /Type /Annot /Subtype /{subtype} /Rect [10 10 50 50] {annotation} >>")
            .into_bytes(),
    ));
    if let Some((structure_type, element)) = enclosing {
        fixture = fixture
            .element(
                11,
                "<< /Type /StructElem /S /Document /P 10 0 R /K [13 0 R 20 0 R] >>",
            )
            .element(
                20,
                &format!(
                    "<< /Type /StructElem /S /{structure_type} /P 11 0 R /Pg 3 0 R \
                     /K [<< /Type /OBJR /Obj 30 0 R >>] {element} >>"
                ),
            );
    }
    fixture
}

/// UA-1 7.18.1-1 and 7.18.1-2: "An annotation, excluding annotations of
/// subtype Widget, PrinterMark or Link, shall be nested within an Annot tag",
/// with `/Contents` or the enclosing element's `/Alt`. The twins: the
/// annotation tagged and described either way, hidden, and off the page
/// (7.18.1-t02-pass-c, -d).
#[test]
fn an_annotation_sits_in_an_annot_element_and_says_what_it_is() {
    let tagged = Some(("Annot", ""));
    annotated("1", "Text", "/Contents (A note)", "/Tabs /S", tagged).clean();
    annotated(
        "1",
        "Text",
        "",
        "/Tabs /S",
        Some(("Annot", "/Alt (A note)")),
    )
    .clean();
    assert_eq!(
        annotated("1", "Text", "/Contents (A note)", "/Tabs /S", None).one_finding(),
        (
            "7.18.1".to_string(),
            FindingKind::AnnotationNotEnclosed {
                subtype: "Text".to_string(),
                expected: "Annot".to_string(),
                enclosing: None,
            }
        )
    );
    assert_eq!(
        annotated(
            "1",
            "Text",
            "/Contents (A note)",
            "/Tabs /S",
            Some(("Span", ""))
        )
        .one_finding(),
        (
            "7.18.1".to_string(),
            FindingKind::AnnotationNotEnclosed {
                subtype: "Text".to_string(),
                expected: "Annot".to_string(),
                enclosing: Some("Span".to_string()),
            }
        )
    );
    assert_eq!(
        annotated("1", "Text", "", "/Tabs /S", tagged).one_finding(),
        (
            "7.18.1".to_string(),
            FindingKind::AnnotationDescriptionMissing {
                subtype: "Text".to_string(),
            }
        )
    );
    // Hidden, and off the page: neither tagged nor described, and silent.
    annotated("1", "Text", "/F 2", "/Tabs /S", None).clean();
    let mut off_page = annotated("1", "Text", "", "/Tabs /S", None);
    off_page.extra = vec![(
        30,
        b"<< /Type /Annot /Subtype /Text /Rect [300 300 400 400] >>".to_vec(),
    )];
    off_page.clean();
    // A subtype Table 169 does not define is not judged (the FREETEXT pass
    // fixtures), and a popup is its parent's window, staged by name.
    annotated("1", "FREETEXT", "", "/Tabs /S", None).clean();
    annotated("1", "Popup", "", "/Tabs /S", None).clean();
    // Part 2 states its annotation rules under 8.9, staged.
    annotated("2", "Text", "", "/Tabs /S", None).clean();
}

/// UA-1 7.18.3-1: "Every page on which there is an annotation shall contain
/// in its page dictionary the key Tabs, and its value shall be S."
#[test]
fn a_page_with_annotations_tabs_in_structure_order() {
    let tagged = Some(("Annot", ""));
    for (tabs, found) in [("", None), ("/Tabs /R", Some("R")), ("/Tabs /C", Some("C"))] {
        assert_eq!(
            annotated("1", "Text", "/Contents (A note)", tabs, tagged).one_finding(),
            (
                "7.18.3".to_string(),
                FindingKind::TabOrderNotStructure {
                    found: found.map(str::to_string),
                }
            ),
            "{tabs:?}"
        );
    }
}

/// UA-1 7.18.4-1 and 7.18.1-3: a widget in a `Form` element, and its field's
/// `/TU` or the enclosing `/Alt`. The field's, not the widget's: a field
/// carrying `/TU` passes (7.18.1-t03-pass-e) and a field without one whose
/// widget carries it fails (-fail-d).
#[test]
fn a_widget_sits_in_a_form_and_its_field_has_a_tooltip() {
    let form = Some(("Form", ""));
    annotated("1", "Widget", "/T (f) /FT /Tx /TU (Name)", "/Tabs /S", form).clean();
    annotated(
        "1",
        "Widget",
        "/T (f) /FT /Tx",
        "/Tabs /S",
        Some(("Form", "/Alt (Name)")),
    )
    .clean();
    assert_eq!(
        annotated("1", "Widget", "/T (f) /FT /Tx", "/Tabs /S", form).one_finding(),
        (
            "7.18.1".to_string(),
            FindingKind::AnnotationDescriptionMissing {
                subtype: "Widget".to_string(),
            }
        )
    );
    let kid_of = |field: &str| {
        let mut fixture = annotated("1", "Widget", "/Parent 31 0 R /TU (Name)", "/Tabs /S", form);
        fixture.extra.push((
            31,
            format!("<< /T (f) /FT /Tx /Kids [30 0 R] {field} >>").into_bytes(),
        ));
        fixture
    };
    kid_of("/TU (Name)").clean();
    assert_eq!(
        kid_of("").one_finding().1,
        FindingKind::AnnotationDescriptionMissing {
            subtype: "Widget".to_string(),
        }
    );
    assert_eq!(
        annotated(
            "1",
            "Widget",
            "/T (f) /FT /Tx /TU (Name)",
            "/Tabs /S",
            Some(("Div", ""))
        )
        .one_finding(),
        (
            "7.18.4".to_string(),
            FindingKind::AnnotationNotEnclosed {
                subtype: "Widget".to_string(),
                expected: "Form".to_string(),
                enclosing: Some("Div".to_string()),
            }
        )
    );
}

/// UA-1 7.18.5-1 and 7.18.5-2: a link in a `Link` element, with `/Contents`
/// — the enclosing `/Alt` satisfies 7.18.1-2 and does not stand in for this.
#[test]
fn a_link_sits_in_a_link_element_and_carries_contents() {
    annotated(
        "1",
        "Link",
        "/Contents (Home)",
        "/Tabs /S",
        Some(("Link", "")),
    )
    .clean();
    assert_eq!(
        annotated("1", "Link", "", "/Tabs /S", Some(("Link", "/Alt (Home)"))).one_finding(),
        ("7.18.5".to_string(), FindingKind::LinkContentsMissing)
    );
    assert_eq!(
        annotated(
            "1",
            "Link",
            "/Contents (Home)",
            "/Tabs /S",
            Some(("Span", ""))
        )
        .one_finding(),
        (
            "7.18.5".to_string(),
            FindingKind::AnnotationNotEnclosed {
                subtype: "Link".to_string(),
                expected: "Link".to_string(),
                enclosing: Some("Span".to_string()),
            }
        )
    );
}

/// UA-1 7.18.2-1, "Annotations of subtype TrapNet shall not be permitted",
/// and 7.18.8-1, a printer's mark an artifact and in no structure element —
/// both unless hidden or off the page.
#[test]
fn no_trap_network_and_a_printers_mark_is_no_structure() {
    assert_eq!(
        annotated(
            "1",
            "TrapNet",
            "/Contents (traps)",
            "/Tabs /S",
            Some(("Annot", ""))
        )
        .one_finding(),
        (
            "7.18.2".to_string(),
            FindingKind::AnnotationForbidden {
                subtype: "TrapNet".to_string(),
            }
        )
    );
    annotated("1", "TrapNet", "/F 2", "/Tabs /S", None).clean();

    annotated("1", "PrinterMark", "/Contents (marks)", "/Tabs /S", None).clean();
    assert_eq!(
        annotated(
            "1",
            "PrinterMark",
            "/Contents (marks)",
            "/Tabs /S",
            Some(("Annot", ""))
        )
        .one_finding(),
        ("7.18.8".to_string(), FindingKind::PrinterMarkInStructure)
    );
}

/// With no structure tree at all, nothing encloses anything, and the missing
/// tree is the one finding: each annotation reported unenclosed beside it
/// would be the same defect again. The tree-free rules still run — a page
/// with an annotation and no `/Tabs /S` is reported either way.
#[test]
fn with_no_tree_an_annotation_is_not_reported_unenclosed_as_well() {
    let mut untagged = annotated("1", "Text", "/Contents (A note)", "/Tabs /S", None);
    untagged.tagged = false;
    assert_eq!(
        untagged.one_finding(),
        ("7.1".to_string(), FindingKind::StructureTreeMissing)
    );
    untagged.page = "/Annots [30 0 R]".to_string();
    let kinds: Vec<FindingKind> = untagged.findings().into_iter().map(|f| f.kind).collect();
    assert_eq!(
        kinds,
        vec![
            FindingKind::StructureTreeMissing,
            FindingKind::TabOrderNotStructure { found: None },
        ]
    );
}
