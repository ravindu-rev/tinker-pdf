//! One fixture per syntax rule, each built at the edge of its clause, and each
//! with the near-miss twin that must **not** fire.
//!
//! The discipline is the design doc's own, borrowed a milestone early from
//! milestone 6: a rule shown only to fire on a broken file has not been shown
//! to discriminate. Every test here starts from [`conforming`] — a document
//! this build finds nothing wrong with — makes exactly one change, and asserts
//! exactly one finding of exactly one kind. When the baseline itself grows a
//! finding, [`the_baseline_is_clean`] fails first and says so, which is what
//! keeps the single-finding assertions honest rather than accidental.
//!
//! Asserting the *count* matters. "At least one finding" passes when a rule
//! fires for the wrong reason, and a validator whose rules fire for the wrong
//! reasons agrees with the corpus by coincidence.

use tinker_pdf::{Document, FindingKind, PdfACoverage};

// ---- building a document at the edge of a clause --------------------------

/// The XMP packet claiming `part` and `level`.
fn packet(part: &str, level: Option<&str>) -> String {
    let conformance = match level {
        Some(letter) => format!(r#" pdfaid:conformance="{letter}""#),
        None => String::new(),
    };
    // ISO 19005-4 6.7.3 asks a part 4 file for `pdfaid:rev` as well as
    // `pdfaid:part` — the four-digit year of the amendment it claims.
    // Parts 1 to 3 have no equivalent, so it is emitted only for part 4
    // and a fixture that wants the missing-revision finding removes it.
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

/// A document under construction, with every piece a rule looks at exposed.
struct Fixture {
    /// Everything before the first object, including the binary comment.
    header: Vec<u8>,
    /// The catalog's entries, beyond `/Type`, `/Pages` and `/Metadata`.
    catalog_extra: String,
    /// Objects 5 and up, for fixtures that need one.
    extra: Vec<(u32, String)>,
    /// The trailer's entries, beyond `/Size` and `/Root`.
    trailer_extra: String,
    /// The XMP packet.
    packet: String,
}

impl Fixture {
    /// A document this build finds nothing wrong with, claiming `part`/`level`.
    ///
    /// The header carries the four bytes above 127 that 6.1.2 asks for, the
    /// trailer carries the two-string `/ID` that 6.1.3 asks for, and there is
    /// nothing else in it at all — which is the point. Every fixture below is
    /// this document plus one defect.
    fn new(part: &str, level: Option<&str>) -> Fixture {
        Fixture {
            header: b"%PDF-1.7\n%\xE2\xE3\xCF\xD3\n".to_vec(),
            catalog_extra: String::new(),
            extra: Vec::new(),
            trailer_extra: "/ID [<0102030405060708090A0B0C0D0E0F10> \
                            <0102030405060708090A0B0C0D0E0F10>]"
                .to_string(),
            packet: packet(part, level),
        }
    }

    fn build(&self) -> Vec<u8> {
        let stream = self.packet.as_bytes();
        let mut objects: Vec<(u32, Vec<u8>)> = vec![
            (
                1,
                format!(
                    "<< /Type /Catalog /Pages 2 0 R /Metadata 4 0 R {} >>",
                    self.catalog_extra
                )
                .into_bytes(),
            ),
            (2, b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec()),
            (
                3,
                b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] >>".to_vec(),
            ),
            (4, {
                let mut body = format!(
                    "<< /Type /Metadata /Subtype /XML /Length {} >>\nstream\n",
                    stream.len()
                )
                .into_bytes();
                body.extend_from_slice(stream);
                body.extend_from_slice(b"\nendstream");
                body
            }),
        ];
        for (num, body) in &self.extra {
            objects.push((*num, body.clone().into_bytes()));
        }

        let highest = objects.iter().map(|(n, _)| *n).max().unwrap_or(0) as usize;
        let mut out = self.header.clone();
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
                "trailer\n<< /Size {} /Root 1 0 R {} >>\nstartxref\n{xref_at}\n%%EOF\n",
                highest + 1,
                self.trailer_extra
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
    ///
    /// Panics with the whole list when there is not exactly one, because the
    /// list is what says whether the rule fired for the reason it exists.
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
    // And in every flavour, because the rules branch on the part.
    for (part, level) in [
        ("1", Some("B")),
        ("2", Some("U")),
        ("3", Some("B")),
        ("4", None),
    ] {
        let mut fixture = Fixture::new(part, level);
        if part == "4" {
            // Part 4 is defined on PDF 2.0 and 6.1.2 says its header says so.
            fixture.header = b"%PDF-2.0\n%\xE2\xE3\xCF\xD3\n".to_vec();
        }
        assert_eq!(
            fixture.findings(),
            Vec::<FindingKind>::new(),
            "clean baseline for part {part}"
        );
    }
}

// ---- 6.1.2 File header ----------------------------------------------------

#[test]
fn a_header_that_does_not_begin_at_byte_zero_is_a_finding() {
    let mut fixture = conforming();
    fixture.header = b"   %PDF-1.7\n%\xE2\xE3\xCF\xD3\n".to_vec();
    assert_eq!(
        fixture.one_finding(),
        FindingKind::HeaderNotAtStart { at: 3 }
    );
}

/// The near-miss: one byte of leading whitespace is a finding, zero is not.
/// The baseline test is that twin, asserted once for every rule here.
#[test]
fn a_header_naming_a_version_the_part_does_not_admit_is_a_finding() {
    let mut fixture = conforming();
    fixture.header = b"%PDF-2.0\n%\xE2\xE3\xCF\xD3\n".to_vec();
    assert_eq!(
        fixture.one_finding(),
        FindingKind::HeaderVersionNotInPart {
            declared: "2.0".to_string()
        }
    );

    // And the mirror: a part 4 file with a part 1-to-3 header.
    let mut four = Fixture::new("4", None);
    four.header = b"%PDF-1.7\n%\xE2\xE3\xCF\xD3\n".to_vec();
    assert_eq!(
        four.one_finding(),
        FindingKind::HeaderVersionNotInPart {
            declared: "1.7".to_string()
        }
    );
}

/// The header line ends at **one** EOL marker and the comment is the next
/// thing in the file. Two corpus fixtures are the two ways to get that wrong,
/// and the first version of this rule passed both: it looked for the next
/// *line* rather than requiring the next *bytes*.
#[test]
fn a_header_line_that_does_not_end_at_a_single_eol_is_a_finding() {
    // Trailing spaces before the EOL.
    let mut spaces = conforming();
    spaces.header = b"%PDF-1.7   \n%\xE2\xE3\xCF\xD3\n".to_vec();
    assert_eq!(
        spaces.one_finding(),
        FindingKind::HeaderNotFollowedBySingleEol
    );

    // A blank line between the header and its comment. The single EOL after
    // the version *is* there, so this is caught by the other half of the rule:
    // what follows it is not a comment. Asserting the kind that actually
    // happens rather than the one that felt likely is the difference between a
    // test and a wish — this expectation was wrong when it was written.
    let mut blank = conforming();
    blank.header = b"%PDF-1.7\n\n%\xE2\xE3\xCF\xD3\n".to_vec();
    assert_eq!(blank.one_finding(), FindingKind::HeaderCommentMissing);

    // Every EOL spelling the standard admits, and none of them is a finding.
    for eol in [&b"\n"[..], b"\r", b"\r\n"] {
        let mut fixture = conforming();
        let mut header = b"%PDF-1.7".to_vec();
        header.extend_from_slice(eol);
        header.extend_from_slice(b"%\xE2\xE3\xCF\xD3\n");
        fixture.header = header;
        assert_eq!(fixture.findings(), Vec::<FindingKind>::new(), "{eol:?}");
    }
}

#[test]
fn a_header_with_no_comment_after_it_is_a_finding() {
    let mut fixture = conforming();
    fixture.header = b"%PDF-1.7\n".to_vec();
    assert_eq!(fixture.one_finding(), FindingKind::HeaderCommentMissing);
}

/// Four printable bytes are a comment; four bytes above 127 are the comment
/// 6.1.2 asks for, and the difference is the whole rule.
#[test]
fn a_comment_whose_first_four_bytes_are_ascii_is_a_finding() {
    let mut fixture = conforming();
    fixture.header = b"%PDF-1.7\n%ABCD\n".to_vec();
    assert_eq!(fixture.one_finding(), FindingKind::HeaderCommentNotBinary);

    // Three high bytes rather than four: still short, still a finding.
    let mut three = conforming();
    three.header = b"%PDF-1.7\n%\xE2\xE3\xCF\n".to_vec();
    assert_eq!(three.one_finding(), FindingKind::HeaderCommentNotBinary);
}

// ---- 6.1.3 File trailer ---------------------------------------------------

#[test]
fn a_trailer_without_an_identifier_is_a_finding() {
    let mut fixture = conforming();
    fixture.trailer_extra = String::new();
    assert_eq!(fixture.one_finding(), FindingKind::FileIdentifierMissing);
}

/// Having the keyword is not having the identifier, and the two kinds say
/// which of the two happened.
#[test]
fn an_identifier_that_is_not_two_strings_is_a_different_finding() {
    for spelling in ["/ID [<0102>]", "/ID <0102>", "/ID [<0102> <0304> <0506>]"] {
        let mut fixture = conforming();
        fixture.trailer_extra = spelling.to_string();
        assert_eq!(
            fixture.one_finding(),
            FindingKind::FileIdentifierMalformed,
            "{spelling}"
        );
    }
}

// ---- 6.1.7 Stream objects -------------------------------------------------

#[test]
fn a_stream_whose_data_lives_in_another_file_is_a_finding() {
    let mut fixture = conforming();
    fixture.extra.push((
        5,
        "<< /Length 0 /F (elsewhere.bin) >>\nstream\n\nendstream".to_string(),
    ));
    assert_eq!(
        fixture.one_finding(),
        FindingKind::ExternalStream {
            key: "F".to_string()
        }
    );
}

// ---- 6.1.10 Filters -------------------------------------------------------

#[test]
fn the_lzw_filter_is_a_finding_under_both_its_spellings() {
    for spelling in ["LZWDecode", "LZW"] {
        let mut fixture = conforming();
        fixture.extra.push((
            5,
            format!("<< /Length 0 /Filter /{spelling} >>\nstream\n\nendstream"),
        ));
        assert_eq!(
            fixture.one_finding(),
            FindingKind::FilterForbidden {
                filter: spelling.to_string()
            }
        );
    }

    // The near-miss: the filter the standard does admit.
    let mut flate = conforming();
    flate.extra.push((
        5,
        "<< /Length 0 /Filter /FlateDecode >>\nstream\n\nendstream".to_string(),
    ));
    assert_eq!(flate.findings(), Vec::<FindingKind>::new());
}

/// A `/Crypt` filter with no `/DecodeParms` is the `/Identity` filter by
/// default (ISO 32000-1 7.4.10), so the bare form is not a finding and the
/// named form is.
#[test]
fn a_crypt_filter_is_judged_by_the_name_it_gives() {
    let mut named = conforming();
    named.extra.push((
        5,
        "<< /Length 0 /Filter /Crypt /DecodeParms << /Name /StdCF >> >>\nstream\n\nendstream"
            .to_string(),
    ));
    assert_eq!(named.one_finding(), FindingKind::CryptFilterNotIdentity);

    for spelling in [
        "<< /Length 0 /Filter /Crypt >>",
        "<< /Length 0 /Filter /Crypt /DecodeParms << /Name /Identity >> >>",
    ] {
        let mut fixture = conforming();
        fixture
            .extra
            .push((5, format!("{spelling}\nstream\n\nendstream")));
        assert_eq!(fixture.findings(), Vec::<FindingKind>::new(), "{spelling}");
    }
}

/// Part 4 admits only the filters ISO 32000-2 defines, so an invented one is a
/// finding there and nowhere else.
#[test]
fn a_filter_outside_the_standard_set_is_a_part_four_finding() {
    let mut four = Fixture::new("4", None);
    four.header = b"%PDF-2.0\n%\xE2\xE3\xCF\xD3\n".to_vec();
    four.extra.push((
        5,
        "<< /Length 0 /Filter /AcmeSqueeze >>\nstream\n\nendstream".to_string(),
    ));
    assert_eq!(
        four.one_finding(),
        FindingKind::FilterNotStandard {
            filter: "AcmeSqueeze".to_string()
        }
    );

    let mut two = conforming();
    two.extra.push((
        5,
        "<< /Length 0 /Filter /AcmeSqueeze >>\nstream\n\nendstream".to_string(),
    ));
    assert_eq!(
        two.findings(),
        Vec::<FindingKind>::new(),
        "parts 1 to 3 name what they forbid rather than what they admit"
    );
}

// ---- 6.6.1 / 6.5.1 Actions ------------------------------------------------

#[test]
fn each_forbidden_action_type_is_its_own_finding() {
    for action in [
        "Launch",
        "Sound",
        "Movie",
        "ResetForm",
        "ImportData",
        "Hide",
        "SetOCGState",
        "Rendition",
        "Trans",
        "GoTo3DView",
        "SetState",
        "NOP",
    ] {
        let mut fixture = conforming();
        fixture.catalog_extra = "/OpenAction 5 0 R".to_string();
        fixture
            .extra
            .push((5, format!("<< /Type /Action /S /{action} >>")));
        assert_eq!(
            fixture.one_finding(),
            FindingKind::ActionForbidden {
                action: action.to_string()
            }
        );
    }
}

/// The one difference between part 4's sentence and part 2's, asserted from
/// both directions so a change to either shows up here.
#[test]
fn javascript_is_forbidden_everywhere_except_part_four() {
    let mut two = conforming();
    two.catalog_extra = "/OpenAction 5 0 R".to_string();
    two.extra
        .push((5, "<< /Type /Action /S /JavaScript /JS () >>".to_string()));
    assert_eq!(
        two.one_finding(),
        FindingKind::ActionForbidden {
            action: "JavaScript".to_string()
        }
    );

    let mut four = Fixture::new("4", None);
    four.header = b"%PDF-2.0\n%\xE2\xE3\xCF\xD3\n".to_vec();
    four.catalog_extra = "/OpenAction 5 0 R".to_string();
    four.extra
        .push((5, "<< /Type /Action /S /JavaScript /JS () >>".to_string()));
    assert_eq!(four.findings(), Vec::<FindingKind>::new());
}

#[test]
fn a_named_action_outside_the_four_page_ones_is_a_finding() {
    let mut fixture = conforming();
    fixture.catalog_extra = "/OpenAction 5 0 R".to_string();
    fixture
        .extra
        .push((5, "<< /Type /Action /S /Named /N /Print >>".to_string()));
    assert_eq!(
        fixture.one_finding(),
        FindingKind::NamedActionForbidden {
            name: "Print".to_string()
        }
    );

    for permitted in ["NextPage", "PrevPage", "FirstPage", "LastPage"] {
        let mut fixture = conforming();
        fixture.catalog_extra = "/OpenAction 5 0 R".to_string();
        fixture
            .extra
            .push((5, format!("<< /Type /Action /S /Named /N /{permitted} >>")));
        assert_eq!(fixture.findings(), Vec::<FindingKind>::new(), "{permitted}");
    }
}

#[test]
fn an_additional_actions_dictionary_is_a_finding_in_parts_one_to_three() {
    let mut fixture = conforming();
    fixture.catalog_extra = "/AA << /WC 5 0 R >>".to_string();
    fixture
        .extra
        .push((5, "<< /Type /Action /S /GoTo >>".to_string()));
    assert_eq!(fixture.one_finding(), FindingKind::TriggerEventsForbidden);
}

/// The staged half, asserted as staged. ISO 19005-4 6.6.3 admits some trigger
/// events, this build has not read which, and a silent pass would be
/// indistinguishable from a rule that ran. `PDFA_STAGED` is what tells them
/// apart, and `the_staged_rules_are_named` is where that is checked.
#[test]
fn part_four_trigger_events_are_not_checked_yet() {
    let mut four = Fixture::new("4", None);
    four.header = b"%PDF-2.0\n%\xE2\xE3\xCF\xD3\n".to_vec();
    four.catalog_extra = "/AA << /WS 5 0 R >>".to_string();
    four.extra
        .push((5, "<< /Type /Action /S /GoTo >>".to_string()));
    assert_eq!(
        four.findings(),
        Vec::<FindingKind>::new(),
        "no rule ran, and PDFA_STAGED is what says so"
    );
}

// ---- forms, optional content, embedded files ------------------------------

#[test]
fn an_xfa_form_is_a_finding() {
    let mut fixture = conforming();
    fixture.catalog_extra = "/AcroForm << /Fields [] /XFA [(x) 5 0 R] >>".to_string();
    fixture
        .extra
        .push((5, "<< /Length 0 >>\nstream\n\nendstream".to_string()));
    assert_eq!(fixture.one_finding(), FindingKind::XfaForbidden);
}

#[test]
fn a_catalog_that_needs_rendering_is_a_finding() {
    let mut fixture = conforming();
    fixture.catalog_extra = "/NeedsRendering true".to_string();
    assert_eq!(fixture.one_finding(), FindingKind::NeedsRenderingForbidden);

    let mut explicit_false = conforming();
    explicit_false.catalog_extra = "/NeedsRendering false".to_string();
    assert_eq!(explicit_false.findings(), Vec::<FindingKind>::new());
}

#[test]
fn optional_content_is_a_finding_in_part_one_only() {
    let mut one = Fixture::new("1", Some("B"));
    one.catalog_extra = "/OCProperties << /OCGs [] /D << >> >>".to_string();
    assert_eq!(one.one_finding(), FindingKind::OptionalContentForbidden);

    let mut two = conforming();
    two.catalog_extra = "/OCProperties << /OCGs [] /D << >> >>".to_string();
    assert_eq!(two.findings(), Vec::<FindingKind>::new());
}

#[test]
fn a_permissions_entry_outside_the_two_admitted_keys_is_a_finding() {
    let mut fixture = conforming();
    fixture.catalog_extra = "/Perms << /Acme 5 0 R >>".to_string();
    fixture.extra.push((5, "<< >>".to_string()));
    assert_eq!(
        fixture.one_finding(),
        FindingKind::PermissionsEntryForbidden {
            key: "Acme".to_string()
        }
    );

    for permitted in ["DocMDP", "UR3"] {
        let mut fixture = conforming();
        fixture.catalog_extra = format!("/Perms << /{permitted} 5 0 R >>");
        fixture.extra.push((5, "<< >>".to_string()));
        assert_eq!(fixture.findings(), Vec::<FindingKind>::new(), "{permitted}");
    }
}

#[test]
fn an_embedded_file_is_forbidden_in_part_one_and_described_in_parts_three_and_four() {
    let filespec = "<< /Type /Filespec /F (a.txt) /UF (a.txt) /EF << /F 6 0 R >> >>";
    let stream = "<< /Length 0 >>\nstream\n\nendstream";

    let mut one = Fixture::new("1", Some("B"));
    one.extra.push((5, filespec.to_string()));
    one.extra.push((6, stream.to_string()));
    assert_eq!(one.one_finding(), FindingKind::EmbeddedFileForbidden);

    // Part 3 wants the relationship and nothing else.
    let mut three = Fixture::new("3", Some("B"));
    three.extra.push((5, filespec.to_string()));
    three.extra.push((6, stream.to_string()));
    assert_eq!(
        three.one_finding(),
        FindingKind::EmbeddedFileKeyMissing {
            key: "AFRelationship".to_string()
        }
    );

    let mut described = Fixture::new("3", Some("B"));
    described.extra.push((
        5,
        "<< /Type /Filespec /F (a.txt) /UF (a.txt) /AFRelationship /Data \
         /EF << /F 6 0 R >> >>"
            .to_string(),
    ));
    described.extra.push((6, stream.to_string()));
    assert_eq!(described.findings(), Vec::<FindingKind>::new());
}

// ---- part 4's own file-structure rules ------------------------------------

#[test]
fn a_part_four_information_dictionary_needs_a_piece_info_to_justify_it() {
    let mut four = Fixture::new("4", None);
    four.header = b"%PDF-2.0\n%\xE2\xE3\xCF\xD3\n".to_vec();
    four.extra
        .push((5, "<< /ModDate (D:20260101000000Z) >>".to_string()));
    four.trailer_extra.push_str(" /Info 5 0 R");
    assert_eq!(four.one_finding(), FindingKind::InfoDictionaryForbidden);

    // With a `/PieceInfo`, `/ModDate` alone is admitted and anything else is
    // its own finding.
    let mut justified = Fixture::new("4", None);
    justified.header = b"%PDF-2.0\n%\xE2\xE3\xCF\xD3\n".to_vec();
    justified.catalog_extra = "/PieceInfo << >>".to_string();
    justified
        .extra
        .push((5, "<< /ModDate (D:20260101000000Z) >>".to_string()));
    justified.trailer_extra.push_str(" /Info 5 0 R");
    assert_eq!(justified.findings(), Vec::<FindingKind>::new());

    let mut extra_entry = Fixture::new("4", None);
    extra_entry.header = b"%PDF-2.0\n%\xE2\xE3\xCF\xD3\n".to_vec();
    extra_entry.catalog_extra = "/PieceInfo << >>".to_string();
    extra_entry.extra.push((
        5,
        "<< /ModDate (D:20260101000000Z) /Producer (Acme) >>".to_string(),
    ));
    extra_entry.trailer_extra.push_str(" /Info 5 0 R");
    assert_eq!(
        extra_entry.one_finding(),
        FindingKind::InfoEntryForbidden {
            key: "Producer".to_string()
        }
    );
}

#[test]
fn a_part_four_catalog_version_is_two_point_something_or_a_finding() {
    for (declared, ok) in [
        ("2.0", true),
        ("2.9", true),
        ("1.7", false),
        ("2.01", false),
    ] {
        let mut four = Fixture::new("4", None);
        four.header = b"%PDF-2.0\n%\xE2\xE3\xCF\xD3\n".to_vec();
        four.catalog_extra = format!("/Version /{declared}");
        if ok {
            assert_eq!(four.findings(), Vec::<FindingKind>::new(), "{declared}");
        } else {
            assert_eq!(
                four.one_finding(),
                FindingKind::CatalogVersionMalformed {
                    declared: declared.to_string()
                },
                "{declared}"
            );
        }
    }
}

// ---- the group is lazy, and the file that proves it -----------------------

/// The design doc requires a syntax-only sweep never to build the machinery
/// the other groups need. The runtime half of that is counted inside the
/// crate; this is the static half, and it is the stronger of the two: a
/// counter proves what ran on one document, a grep proves what *could* run on
/// any of them.
///
/// **Injection, counted.** A font rule added to `syntax::dictionary` — one
/// that calls `tinker_pdf_font::Sfnt::parse` on any `/FontFile2` — fails this
/// test and three others, 4 of the workspace's 3 375. This one is the only
/// half of the guard that fires on an import the code never executes, which is
/// why both halves exist.
#[test]
fn the_syntax_group_names_no_font_or_colour_machinery() {
    let source =
        std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/src/pdfa/syntax.rs"))
            .expect("the module is beside its test");
    // Comments are stripped first: the module's own documentation explains
    // which crates it does not reach for, by name, and a guard that could not
    // tell an explanation from a dependency would forbid the explanation.
    let code: String = source
        .lines()
        .filter(|line| !line.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n");
    for crate_name in ["tinker_pdf_font", "tinker_pdf_color", "tinker_pdf_render"] {
        assert!(
            !code.contains(crate_name),
            "the syntax group reached for {crate_name}; the sweep over 2 907 \
             files is no longer syntax-only"
        );
    }
}

/// A syntax-only request still reads the flavour claim — which part a file
/// says it is decides which rules apply — but runs none of the metadata
/// group's rules, and the verdict says which groups ran rather than leaving a
/// caller to infer it from an empty list.
#[test]
fn a_syntax_only_request_runs_the_syntax_group_and_says_so() {
    let mut fixture = conforming();
    fixture.trailer_extra = String::new();
    let document = Document::open(fixture.build()).expect("opens");

    let verdict = document.validate_pdfa_with(PdfACoverage::SYNTAX);
    assert!(verdict.coverage.syntax);
    assert!(!verdict.coverage.metadata);
    assert!(!verdict.coverage.is_complete());
    assert_eq!(
        verdict.flavour.map(|f| f.to_string()).as_deref(),
        Some("PDF/A-2B"),
        "the claim decides which rules apply, so it is read for every group"
    );
    assert!(verdict
        .findings
        .iter()
        .any(|f| f.kind == FindingKind::FileIdentifierMissing));

    // The mirror: the metadata group alone reads the claim and no structure.
    let verdict = document.validate_pdfa_with(PdfACoverage::METADATA);
    assert!(verdict.coverage.metadata);
    assert!(!verdict.coverage.syntax);
    assert_eq!(
        verdict.flavour.map(|f| f.to_string()).as_deref(),
        Some("PDF/A-2B")
    );
    assert!(
        verdict.findings.is_empty(),
        "the file-structure finding belongs to the other group: {:?}",
        verdict.findings
    );
}

/// Asking for a group this build has no rules for must not make the verdict
/// claim it ran. A caller who reads `coverage` is reading what happened.
#[test]
fn a_group_with_no_rules_never_reports_itself_as_having_run() {
    let document = Document::open(conforming().build()).expect("opens");
    let everything = PdfACoverage {
        metadata: true,
        syntax: true,
        fonts: true,
        colour: true,
    };
    let verdict = document.validate_pdfa_with(everything);
    assert!(!verdict.coverage.fonts);
    assert!(!verdict.coverage.colour);
    assert!(!verdict.coverage.is_complete());
}

/// Every staged rule carries a clause and a reason, and the list is not empty.
///
/// A staged rule with no reason is a rule nobody will remember was staged, and
/// milestone 4's ledger classifies disagreements against this list — so a row
/// claiming "a known staged rule" has something to point at.
#[test]
fn the_staged_rules_are_named() {
    assert!(!tinker_pdf::PDFA_STAGED.is_empty());
    for staged in tinker_pdf::PDFA_STAGED {
        assert!(!staged.clause.is_empty(), "{staged:?}");
        assert!(staged.rule.len() > 16, "{staged:?}");
        assert!(
            staged.because.len() > 32,
            "a staged rule's reason is what makes it a refusal rather than a \
             gap: {staged:?}"
        );
    }
}

/// ISO 19005-4 6.7.3: a part 4 file identifies its amendment as well as its
/// part, with a four-digit year in `pdfaid:rev`. Parts 1 to 3 have no
/// equivalent, so the rule must not fire for them — asserted in both
/// directions, because a rule that fired everywhere would report every
/// conforming PDF/A-1 file.
#[test]
fn a_part_four_file_names_the_amendment_it_claims() {
    let mut missing = Fixture::new("4", None);
    missing.header = b"%PDF-2.0\n%\xE2\xE3\xCF\xD3\n".to_vec();
    missing.packet = missing.packet.replace(r#" pdfaid:rev="2020""#, "");
    assert_eq!(missing.one_finding(), FindingKind::RevisionMissing);

    for declared in ["20", "20_y", "", "twenty20"] {
        let mut malformed = Fixture::new("4", None);
        malformed.header = b"%PDF-2.0\n%\xE2\xE3\xCF\xD3\n".to_vec();
        malformed.packet = malformed.packet.replace(
            r#"pdfaid:rev="2020""#,
            &format!(r#"pdfaid:rev="{declared}""#),
        );
        // An empty attribute value is a value: the reader saw the attribute and
        // it said nothing, which is a different fact from the attribute being
        // absent, and `RevisionMalformed { declared: "" }` is the finding that
        // says which of the two happened.
        assert_eq!(
            malformed.one_finding(),
            FindingKind::RevisionMalformed {
                declared: declared.to_string()
            },
            "{declared:?}"
        );
    }

    // Parts 1 to 3 do not have the entry and must not be asked for it.
    for (part, level) in [("1", Some("B")), ("2", Some("U")), ("3", Some("B"))] {
        assert_eq!(
            Fixture::new(part, level).findings(),
            Vec::<FindingKind>::new(),
            "part {part}"
        );
    }
}

/// The digit after `1.` names a version of PDF, and 1.8 and 1.9 are not
/// versions of PDF.
///
/// The first rule accepted any digit, and a `%PDF-1.9` fixture went through it
/// untouched. Asserting the boundary from both sides is what makes this a rule
/// about the standard rather than about a regular expression.
#[test]
fn the_header_version_names_a_version_that_exists() {
    for digit in 0..=7 {
        let mut fixture = conforming();
        let mut header = b"%PDF-1.".to_vec();
        header.push(b'0' + digit);
        header.extend_from_slice(b"\n%\xE2\xE3\xCF\xD3\n");
        fixture.header = header;
        assert_eq!(fixture.findings(), Vec::<FindingKind>::new(), "1.{digit}");
    }
    for digit in [8u8, 9] {
        let mut fixture = conforming();
        let mut header = b"%PDF-1.".to_vec();
        header.push(b'0' + digit);
        header.extend_from_slice(b"\n%\xE2\xE3\xCF\xD3\n");
        fixture.header = header;
        assert_eq!(
            fixture.one_finding(),
            FindingKind::HeaderVersionNotInPart {
                declared: format!("1.{digit}")
            }
        );
    }
}
