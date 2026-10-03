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
        let mut catalog = "/Type /Catalog /Pages 2 0 R /Metadata 4 0 R \
                           /ViewerPreferences << /DisplayDocTitle true >>"
            .to_string();
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
            (
                4,
                stream("/Type /Metadata /Subtype /XML", packet.as_bytes()),
            ),
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
            objects.push((
                10,
                b"<< /Type /StructTreeRoot /K 11 0 R /ParentTree 12 0 R >>".to_vec(),
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
