//! One fixture per font rule, each built at the edge of its clause, and each
//! with the near-miss twin that must **not** fire (milestone 5 of
//! `docs/design/pdfa.md`).
//!
//! The discipline is `pdfa_syntax.rs`'s: every test starts from [`conforming`]
//! — a document this build finds nothing wrong with, which draws text with an
//! embedded TrueType font — makes exactly one change, and asserts exactly one
//! finding of exactly one kind. [`the_baseline_is_clean`] fails first when the
//! baseline itself grows a finding, which is what keeps the single-finding
//! assertions honest rather than accidental.
//!
//! # The one test here that is not about a clause
//!
//! [`a_font_drawn_only_at_rendering_mode_three_is_not_used_for_rendering`] is
//! about the qualifier every clause in ISO 19005 6.3 opens with. It is the
//! test that would have caught the first version of this rule group, which
//! judged every font dictionary in the file and reported twelve conforming
//! corpus files for carrying a `/Helvetica` nobody draws with.

use tinker_pdf::{Document, FindingKind, PdfACoverage};

// ---- building a document at the edge of a clause --------------------------

/// A real `sfnt` header: the version tag, one table, and a directory entry.
///
/// Twenty-eight bytes, and every one of them load-bearing —
/// `tinker_pdf_font::Sfnt::parse` reads the tag, the table count and the
/// directory before it answers, so filler would be refused at the first byte
/// and the "the program parses" half of the embedding rule would be testing
/// nothing.
const SFNT: &[u8] = &[
    0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x00, 0x10, 0x00, 0x03, 0x00, 0x04, b'h', b'e', b'a', b'd',
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x1C, 0x00, 0x00, 0x00, 0x36,
];

/// The XMP packet claiming `part` and `level`.
fn packet(part: &str, level: Option<&str>) -> String {
    let conformance = match level {
        Some(letter) => format!(r#" pdfaid:conformance="{letter}""#),
        None => String::new(),
    };
    let revision = if part == "4" {
        r#" pdfaid:rev="2020""#
    } else {
        ""
    };
    format!(
        r#"<?xpacket begin="" id="W5M0MpCehiHzreSzNTczkc9d"?>
<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF
 xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
<rdf:Description rdf:about="" xmlns:pdfaid="http://www.aiim.org/pdfa/ns/id/"
 pdfaid:part="{part}"{conformance}{revision}/></rdf:RDF></x:xmpmeta><?xpacket end="w"?>"#
    )
}

/// A document under construction, with every piece a font rule looks at
/// exposed.
struct Fixture {
    part: String,
    level: Option<String>,
    /// The page's `/Resources`, as written.
    resources: String,
    /// The page's content stream.
    content: String,
    /// The font dictionary, object 5, without its enclosing `<< >>`.
    font: String,
    /// The font descriptor, object 6, without its enclosing `<< >>`. Empty
    /// means the font has none, which is what a standard-14 font looks like.
    descriptor: String,
    /// The embedded program, object 7: its dictionary entries and its bytes.
    program: Option<(String, Vec<u8>)>,
    /// Objects 8 and up, for fixtures that need one.
    extra: Vec<(u32, Vec<u8>)>,
}

impl Fixture {
    /// A document this build finds nothing wrong with, claiming `part`/`level`.
    ///
    /// One page, one non-symbolic TrueType font with `/WinAnsiEncoding`, an
    /// embedded program that parses, and a content stream that draws one
    /// character with it at a visible rendering mode. Everything below is this
    /// document plus one defect.
    fn new(part: &str, level: Option<&str>) -> Fixture {
        Fixture {
            part: part.to_string(),
            level: level.map(str::to_string),
            resources: "<< /Font << /F1 5 0 R >> >>".to_string(),
            content: "BT /F1 12 Tf 10 10 Td (A) Tj ET".to_string(),
            font: "/Type /Font /Subtype /TrueType /BaseFont /ABCDEF+Acme \
                   /Encoding /WinAnsiEncoding /FirstChar 65 /LastChar 65 \
                   /Widths [500] /FontDescriptor 6 0 R"
                .to_string(),
            descriptor: "/Type /FontDescriptor /FontName /ABCDEF+Acme /Flags 32 \
                         /FontBBox [0 0 1000 1000] /ItalicAngle 0 /Ascent 800 \
                         /Descent -200 /CapHeight 700 /StemV 80 /FontFile2 7 0 R"
                .to_string(),
            program: Some((String::new(), SFNT.to_vec())),
            extra: Vec::new(),
        }
    }

    fn build(&self) -> Vec<u8> {
        let packet = packet(&self.part, self.level.as_deref());
        let mut objects: Vec<(u32, Vec<u8>)> = vec![
            (
                1,
                b"<< /Type /Catalog /Pages 2 0 R /Metadata 4 0 R >>".to_vec(),
            ),
            (2, b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec()),
            (
                3,
                format!(
                    "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] \
                     /Resources {} /Contents 9 0 R >>",
                    self.resources
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
        if let Some((entries, bytes)) = &self.program {
            objects.push((7, stream(entries, bytes)));
        }
        objects.extend(self.extra.iter().cloned());
        objects.sort_by_key(|(num, _)| *num);

        let highest = objects.iter().map(|(n, _)| *n).max().unwrap_or(0) as usize;
        let header: &[u8] = if self.part == "4" {
            b"%PDF-2.0\n%\xE2\xE3\xCF\xD3\n"
        } else {
            b"%PDF-1.7\n%\xE2\xE3\xCF\xD3\n"
        };
        let mut out = header.to_vec();
        let mut offsets = vec![0u64; highest + 1];
        for (num, body) in &objects {
            offsets[*num as usize] = out.len() as u64;
            out.extend_from_slice(format!("{num} 0 obj\n").as_bytes());
            out.extend_from_slice(body);
            out.extend_from_slice(b"\nendobj\n");
        }
        let xref_at = out.len() as u64;
        out.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", highest + 1).as_bytes());
        for entry in offsets.iter().skip(1) {
            out.extend_from_slice(format!("{entry:010} 00000 n \n").as_bytes());
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

    /// Every finding a full validation of this fixture reports.
    fn findings(&self) -> Vec<FindingKind> {
        Document::open(self.build())
            .expect("the fixture opens")
            .validate_pdfa()
            .findings
            .into_iter()
            .map(|finding| finding.kind)
            .collect()
    }

    /// The one finding this fixture is built to produce.
    #[track_caller]
    fn one_finding(&self) -> FindingKind {
        let findings = self.findings();
        assert_eq!(
            findings.len(),
            1,
            "expected exactly one finding, got {findings:#?}"
        );
        findings.into_iter().next().expect("one")
    }
}

/// A stream object: `entries` inside the dictionary, `data` after it.
fn stream(entries: &str, data: &[u8]) -> Vec<u8> {
    let mut body = format!("<< {} /Length {} >>\nstream\n", entries, data.len()).into_bytes();
    body.extend_from_slice(data);
    body.extend_from_slice(b"\nendstream");
    body
}

/// A conforming part 2 level B document.
fn conforming() -> Fixture {
    Fixture::new("2", Some("B"))
}

// ---- the baseline ---------------------------------------------------------

/// Without this, every `one_finding` below could be passing by accident.
#[test]
fn the_baseline_is_clean() {
    assert_eq!(
        conforming().findings(),
        Vec::<FindingKind>::new(),
        "the fixture every other test mutates must itself be clean"
    );
    for (part, level) in [
        ("1", Some("B")),
        ("2", Some("U")),
        ("3", Some("B")),
        ("4", None),
    ] {
        assert_eq!(
            Fixture::new(part, level).findings(),
            Vec::<FindingKind>::new(),
            "clean baseline for part {part}"
        );
    }
}

/// And the group says it ran, which is the half of the answer that stops an
/// empty finding list being read as conformance.
#[test]
fn the_verdict_names_the_font_group_as_having_run() {
    let document = Document::open(conforming().build()).expect("opens");
    assert!(document.validate_pdfa().coverage.fonts);
    assert!(
        !document
            .validate_pdfa_with(PdfACoverage::SYNTAX)
            .coverage
            .fonts
    );
}

// ---- "used for rendering", which is every clause's own qualifier ----------

/// ISO 19005-1 6.3.4 constrains fonts **used for rendering**, and 9.3.6's
/// rendering mode 3 paints nothing.
///
/// This is the fixture the corpus wrote for us: `6-2-10-4-1-t01-pass-a.pdf`
/// carries an unembedded Type 1 font and is annotated `pass`, and its own
/// title says why — *"the text rendering mode is 3"*. The near-miss twin is
/// the same document with the `3 Tr` removed, and it must fire.
#[test]
fn a_font_drawn_only_at_rendering_mode_three_is_not_used_for_rendering() {
    let mut invisible = conforming();
    invisible.descriptor = String::new();
    invisible.program = None;
    invisible.font = "/Type /Font /Subtype /Type1 /BaseFont /Helvetica".to_string();
    invisible.content = "BT 3 Tr /F1 12 Tf 10 10 Td (A) Tj ET".to_string();
    assert_eq!(
        invisible.findings(),
        Vec::<FindingKind>::new(),
        "a font whose glyphs are never painted is not used for rendering"
    );

    let mut visible = invisible;
    visible.content = "BT /F1 12 Tf 10 10 Td (A) Tj ET".to_string();
    assert_eq!(
        visible.one_finding(),
        FindingKind::FontNotEmbedded {
            subtype: "Type1".to_string()
        }
    );
}

/// `q` and `Q` restore the rendering mode, so a font drawn after a `Q` that
/// popped a `3 Tr` **is** used for rendering.
///
/// The twin is the same stream without the `Q`, where the mode is still 3 when
/// the text is shown. Two streams differing by one byte, opposite verdicts.
#[test]
fn the_rendering_mode_is_restored_by_q_like_the_rest_of_the_graphics_state() {
    let mut base = conforming();
    base.descriptor = String::new();
    base.program = None;
    base.font = "/Type /Font /Subtype /Type1 /BaseFont /Helvetica".to_string();

    let mut popped = base.clone_shallow();
    popped.content = "q 3 Tr Q BT /F1 12 Tf 10 10 Td (A) Tj ET".to_string();
    assert_eq!(
        popped.one_finding(),
        FindingKind::FontNotEmbedded {
            subtype: "Type1".to_string()
        },
        "the Q restored the visible mode"
    );

    let mut kept = base;
    kept.content = "q 3 Tr BT /F1 12 Tf 10 10 Td (A) Tj ET".to_string();
    assert_eq!(
        kept.findings(),
        Vec::<FindingKind>::new(),
        "without the Q the mode is still 3 when the text is shown"
    );
}

/// A font a resource dictionary names and no content stream selects is not
/// used for rendering either — which is what an interactive form's `/DR`
/// holds, and what cost the first version of this group twelve false
/// positives.
#[test]
fn a_font_named_in_the_resources_and_never_selected_is_not_judged() {
    let mut fixture = conforming();
    fixture.descriptor = String::new();
    fixture.program = None;
    fixture.font = "/Type /Font /Subtype /Type1 /BaseFont /Helvetica".to_string();
    // A path and no colour operator: the colour group landed beside this one
    // and a `rg` here would fire its rule instead, which would make this test
    // pass or fail for a reason that has nothing to do with fonts.
    fixture.content = "10 10 50 50 re f".to_string();
    assert_eq!(fixture.findings(), Vec::<FindingKind>::new());
}

// ---- 6.3.4 / 6.2.11.4.1 Embedding -----------------------------------------

/// The standard 14 are not an exception, because ISO 19005 has no such list.
#[test]
fn a_standard_font_with_no_descriptor_is_a_font_with_no_program() {
    let mut fixture = conforming();
    fixture.descriptor = String::new();
    fixture.program = None;
    fixture.font = "/Type /Font /Subtype /Type1 /BaseFont /Helvetica".to_string();
    assert_eq!(
        fixture.one_finding(),
        FindingKind::FontNotEmbedded {
            subtype: "Type1".to_string()
        }
    );
}

/// A descriptor with no font-file key at all, and the near-miss: a descriptor
/// whose `/FontFile2` is present and **null**, which one corpus fixture
/// writes and which is a key present with no program behind it.
#[test]
fn a_descriptor_without_a_program_is_a_finding_however_the_key_is_written() {
    let mut absent = conforming();
    absent.descriptor = "/Type /FontDescriptor /FontName /ABCDEF+Acme /Flags 32".to_string();
    absent.program = None;
    assert_eq!(
        absent.one_finding(),
        FindingKind::FontNotEmbedded {
            subtype: "TrueType".to_string()
        }
    );

    let mut null = conforming();
    null.descriptor =
        "/Type /FontDescriptor /FontName /ABCDEF+Acme /Flags 32 /FontFile2 null".to_string();
    null.program = None;
    assert_eq!(
        null.one_finding(),
        FindingKind::FontNotEmbedded {
            subtype: "TrueType".to_string()
        }
    );
}

/// ISO 32000-1 9.9 Table 126 pairs the key with the font kind: a TrueType
/// font's program does not live under `/FontFile`.
#[test]
fn a_program_under_a_key_the_font_subtype_does_not_admit_is_a_finding() {
    let mut fixture = conforming();
    fixture.descriptor = "/Type /FontDescriptor /FontName /ABCDEF+Acme /Flags 32 \
                          /FontFile 7 0 R"
        .to_string();
    assert_eq!(
        fixture.one_finding(),
        FindingKind::FontProgramSubtypeMismatch {
            key: "FontFile".to_string(),
            declared: "TrueType".to_string()
        }
    );
}

/// And a `/FontFile3` whose own `/Subtype` is outside Table 126's set. The
/// near-miss is `/OpenType`, which Table 126 admits for every font kind.
#[test]
fn a_font_file3_naming_a_subtype_outside_the_set_is_a_finding() {
    let mut fixture = conforming();
    fixture.descriptor = "/Type /FontDescriptor /FontName /ABCDEF+Acme /Flags 32 \
                          /FontFile3 7 0 R"
        .to_string();
    fixture.program = Some(("/Subtype /Type42C".to_string(), SFNT.to_vec()));
    assert_eq!(
        fixture.one_finding(),
        FindingKind::FontProgramSubtypeMismatch {
            key: "FontFile3".to_string(),
            declared: "Type42C".to_string()
        }
    );

    let mut open_type = conforming();
    open_type.descriptor = "/Type /FontDescriptor /FontName /ABCDEF+Acme /Flags 32 \
                            /FontFile3 7 0 R"
        .to_string();
    open_type.program = Some(("/Subtype /OpenType".to_string(), SFNT.to_vec()));
    assert_eq!(open_type.findings(), Vec::<FindingKind>::new());
}

/// The rule that needs the font machinery: the bytes behind `/FontFile2` must
/// be an `sfnt`. The near-miss twin is the baseline, whose 28 bytes parse.
#[test]
fn a_program_that_is_not_the_format_its_key_names_is_a_finding() {
    let mut fixture = conforming();
    fixture.program = Some((String::new(), b"not a font program at all".to_vec()));
    assert_eq!(
        fixture.one_finding(),
        FindingKind::FontProgramUnreadable {
            key: "FontFile2".to_string()
        }
    );
}

// ---- 6.3.5 / 6.2.11.4.2 Font subsets --------------------------------------

/// Six upper-case letters and a `+`. The near-miss is the baseline's
/// `ABCDEF+Acme`; three ways to get it wrong are asserted here.
#[test]
fn a_subset_tag_that_is_not_six_upper_case_letters_is_a_finding() {
    for name in ["AB+Acme", "abcdef+Acme", "ABCDEF+"] {
        let mut fixture = conforming();
        fixture.font = fixture.font.replace("/ABCDEF+Acme", &format!("/{name}"));
        assert_eq!(
            fixture.one_finding(),
            FindingKind::SubsetTagMalformed {
                declared: name.to_string()
            },
            "for {name}"
        );
    }

    // And a name with no `+` at all is not a subset and is not reported.
    let mut whole = conforming();
    whole.font = whole.font.replace("/ABCDEF+Acme", "/Acme");
    assert_eq!(whole.findings(), Vec::<FindingKind>::new());
}

// ---- 6.3.7 / 6.2.11.6 Character encodings ---------------------------------

/// A symbolic TrueType font's `/Encoding` shall not be present: the program's
/// own `cmap` is the mapping, and a dictionary entry would be a second and
/// disagreeing one.
#[test]
fn a_symbolic_truetype_font_carrying_an_encoding_is_a_finding() {
    let mut fixture = conforming();
    fixture.descriptor = fixture.descriptor.replace("/Flags 32", "/Flags 4");
    assert_eq!(fixture.one_finding(), FindingKind::SymbolicFontHasEncoding);

    // The near-miss: the same symbolic font with no `/Encoding`.
    let mut without = conforming();
    without.descriptor = without.descriptor.replace("/Flags 32", "/Flags 4");
    without.font = without.font.replace("/Encoding /WinAnsiEncoding ", "");
    assert_eq!(without.findings(), Vec::<FindingKind>::new());
}

/// A non-symbolic TrueType font's `/Encoding` shall be one of the two the
/// clause names, and it shall be there.
#[test]
fn a_non_symbolic_truetype_font_outside_the_two_encodings_is_a_finding() {
    let mut named = conforming();
    named.font = named
        .font
        .replace("/Encoding /WinAnsiEncoding", "/Encoding /StandardEncoding");
    assert_eq!(
        named.one_finding(),
        FindingKind::EncodingNotStandard {
            declared: "StandardEncoding".to_string()
        }
    );

    let mut absent = conforming();
    absent.font = absent.font.replace("/Encoding /WinAnsiEncoding ", "");
    assert_eq!(
        absent.one_finding(),
        FindingKind::EncodingNotStandard {
            declared: String::new()
        }
    );

    // The near-miss on the other side: `/MacRomanEncoding` is the second of
    // the two and is clean.
    let mut mac = conforming();
    mac.font = mac
        .font
        .replace("/Encoding /WinAnsiEncoding", "/Encoding /MacRomanEncoding");
    assert_eq!(mac.findings(), Vec::<FindingKind>::new());
}

/// `/Differences` on a non-symbolic TrueType font is a part 1 rule, and the
/// near-miss twin is the same document claiming part 2 — where this build does
/// not read the prohibition as having survived the clause's move.
#[test]
fn differences_on_a_non_symbolic_truetype_font_is_a_part_one_finding() {
    let encoding = "/Encoding << /Type /Encoding /BaseEncoding /WinAnsiEncoding \
                    /Differences [65 /A] >>";

    let mut one = Fixture::new("1", Some("B"));
    one.font = one.font.replace("/Encoding /WinAnsiEncoding", encoding);
    assert_eq!(one.one_finding(), FindingKind::EncodingDifferencesForbidden);

    let mut two = conforming();
    two.font = two.font.replace("/Encoding /WinAnsiEncoding", encoding);
    assert_eq!(two.findings(), Vec::<FindingKind>::new());
}

// ---- 6.3.8 / 6.2.11.7 Unicode character maps ------------------------------

/// A level U TrueType font with no named encoding and no `/ToUnicode` has said
/// nothing about what its codes mean.
///
/// Symbolic, so that the encoding rule above does not fire as well and the
/// single-finding assertion stays about this clause. The near-miss twin is the
/// same document with a `/ToUnicode`, and the second twin is the same document
/// at level B, where the clause does not apply.
#[test]
fn a_level_u_font_with_no_route_to_unicode_is_a_finding() {
    let mut unmapped = Fixture::new("2", Some("U"));
    unmapped.descriptor = unmapped.descriptor.replace("/Flags 32", "/Flags 4");
    unmapped.font = unmapped.font.replace("/Encoding /WinAnsiEncoding ", "");
    assert_eq!(unmapped.one_finding(), FindingKind::ToUnicodeMissing);

    let mut mapped = Fixture::new("2", Some("U"));
    mapped.descriptor = mapped.descriptor.replace("/Flags 32", "/Flags 4");
    mapped.font = mapped
        .font
        .replace("/Encoding /WinAnsiEncoding ", "/ToUnicode 8 0 R ");
    mapped
        .extra
        .push((8, stream("", b"/CIDInit /ProcSet findresource begin end")));
    assert_eq!(mapped.findings(), Vec::<FindingKind>::new());

    let mut level_b = Fixture::new("2", Some("B"));
    level_b.descriptor = level_b.descriptor.replace("/Flags 32", "/Flags 4");
    level_b.font = level_b.font.replace("/Encoding /WinAnsiEncoding ", "");
    assert_eq!(
        level_b.findings(),
        Vec::<FindingKind>::new(),
        "level B does not require the mapping"
    );
}

/// A Type 1 font is exempt: its program is keyed by glyph name, and a glyph
/// name is a route to Unicode through the Adobe Glyph List.
///
/// The corpus is why this exemption exists and the module note names the two
/// fixtures. Here it is asserted directly, so a future change that removed it
/// fails in this file rather than only in the census.
#[test]
fn a_type_one_font_is_exempt_from_the_unicode_rule() {
    let mut fixture = Fixture::new("2", Some("U"));
    fixture.font = "/Type /Font /Subtype /Type1 /BaseFont /ABCDEF+Acme \
                    /FirstChar 65 /LastChar 65 /Widths [500] /FontDescriptor 6 0 R"
        .to_string();
    fixture.descriptor = "/Type /FontDescriptor /FontName /ABCDEF+Acme /Flags 4 \
                          /FontBBox [0 0 1000 1000] /ItalicAngle 0 /Ascent 800 \
                          /Descent -200 /CapHeight 700 /StemV 80 /FontFile3 7 0 R"
        .to_string();
    fixture.program = Some(("/Subtype /OpenType".to_string(), SFNT.to_vec()));
    assert_eq!(fixture.findings(), Vec::<FindingKind>::new());
}

// ---- 6.3.3.2 / 6.2.11.3.2 CIDFonts ----------------------------------------

/// A composite font whose descendant is missing the character-collection
/// identification ISO 32000-1 9.7.3 requires.
#[test]
fn a_cid_font_without_a_complete_cid_system_info_is_a_finding() {
    let mut absent = composite();
    absent.extra = vec![(
        8,
        b"<< /Type /Font /Subtype /CIDFontType2 /BaseFont /ABCDEF+Acme \
          /CIDToGIDMap /Identity /FontDescriptor 6 0 R >>"
            .to_vec(),
    )];
    assert_eq!(
        absent.one_finding(),
        FindingKind::CidSystemInfoIncomplete {
            key: "CIDSystemInfo".to_string()
        }
    );

    // One key at a time, and by type rather than only by presence: a
    // `/Registry` that is a name is as unlookupable as one that is missing.
    let mut wrong_type = composite();
    wrong_type.extra = vec![(
        8,
        b"<< /Type /Font /Subtype /CIDFontType2 /BaseFont /ABCDEF+Acme \
          /CIDToGIDMap /Identity /FontDescriptor 6 0 R \
          /CIDSystemInfo << /Registry /Adobe /Ordering (Identity) /Supplement 0 >> >>"
            .to_vec(),
    )];
    assert_eq!(
        wrong_type.one_finding(),
        FindingKind::CidSystemInfoIncomplete {
            key: "Registry".to_string()
        }
    );
}

/// ISO 19005-1 6.3.3.2 requires the entry even though ISO 32000-1 9.7.4.2
/// defaults it, and the corpus agrees from the other side: the fixture that
/// omits it passes **only** because its font is drawn at rendering mode 3.
#[test]
fn a_cid_font_type2_without_a_cid_to_gid_map_is_a_finding() {
    let mut absent = composite();
    absent.extra = vec![(
        8,
        b"<< /Type /Font /Subtype /CIDFontType2 /BaseFont /ABCDEF+Acme \
          /FontDescriptor 6 0 R \
          /CIDSystemInfo << /Registry (Adobe) /Ordering (Identity) /Supplement 0 >> >>"
            .to_vec(),
    )];
    assert_eq!(
        absent.one_finding(),
        FindingKind::CidToGidMapMalformed {
            declared: String::new()
        }
    );

    let mut named = composite();
    named.extra = vec![(
        8,
        b"<< /Type /Font /Subtype /CIDFontType2 /BaseFont /ABCDEF+Acme \
          /CIDToGIDMap /Custom /FontDescriptor 6 0 R \
          /CIDSystemInfo << /Registry (Adobe) /Ordering (Identity) /Supplement 0 >> >>"
            .to_vec(),
    )];
    assert_eq!(
        named.one_finding(),
        FindingKind::CidToGidMapMalformed {
            declared: "Custom".to_string()
        }
    );

    // The near-miss: `/Identity`, which is the baseline of `composite`.
    assert_eq!(composite().findings(), Vec::<FindingKind>::new());
}

/// A conforming composite font: a Type 0 with one CIDFontType2 descendant.
fn composite() -> Fixture {
    let mut fixture = conforming();
    fixture.font = "/Type /Font /Subtype /Type0 /BaseFont /ABCDEF+Acme \
                    /Encoding /Identity-H /DescendantFonts [8 0 R] /ToUnicode 10 0 R"
        .to_string();
    fixture.content = "BT /F1 12 Tf 10 10 Td <0041> Tj ET".to_string();
    fixture.extra = vec![(
        8,
        b"<< /Type /Font /Subtype /CIDFontType2 /BaseFont /ABCDEF+Acme \
          /CIDToGIDMap /Identity /FontDescriptor 6 0 R \
          /CIDSystemInfo << /Registry (Adobe) /Ordering (Identity) /Supplement 0 >> >>"
            .to_vec(),
    )];
    fixture
        .extra
        .push((10, stream("", b"/CIDInit /ProcSet findresource begin end")));
    fixture
}

// ---- the staged rules are named, not silent -------------------------------

/// Every font rule this build does not run is in `PDFA_STAGED` with a clause
/// and a reason.
///
/// A staged rule that nobody can find is a silent pass with extra steps, which
/// is what the milestone's exit criterion is about. Asserted by clause number
/// so that deleting an entry fails here rather than quietly widening what a
/// clean verdict claims.
#[test]
fn every_staged_font_rule_is_named_with_its_clause_and_its_reason() {
    for clause in ["6.3.2", "6.3.3.3", "6.3.6", "6.3.9"] {
        let found = tinker_pdf::PDFA_STAGED
            .iter()
            .find(|rule| rule.clause == clause)
            .unwrap_or_else(|| panic!("no staged rule names clause {clause}"));
        assert!(
            found.because.len() >= 40,
            "a reason short enough to be a shrug is not a reason: {found:?}"
        );
        assert!(!found.rule.is_empty());
    }
}

impl Fixture {
    /// A copy, for tests that build two documents from one base.
    fn clone_shallow(&self) -> Fixture {
        Fixture {
            part: self.part.clone(),
            level: self.level.clone(),
            resources: self.resources.clone(),
            content: self.content.clone(),
            font: self.font.clone(),
            descriptor: self.descriptor.clone(),
            program: self.program.clone(),
            extra: self.extra.clone(),
        }
    }
}
