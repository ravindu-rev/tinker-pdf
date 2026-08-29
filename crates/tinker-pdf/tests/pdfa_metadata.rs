//! The XMP rule group, one fixture per clause (milestone 3 of
//! `docs/design/pdfa.md`).
//!
//! Two things are asserted here that "at least one finding" would not catch.
//!
//! **A wrong flavour yields exactly the metadata finding.** Not "a list
//! including" it — exactly it, by kind, with the length asserted. A validator
//! that reports the right defect alongside three wrong ones has told a caller
//! to go and look at three places that are fine, and a corpus agreement rate
//! cannot tell the two apart.
//!
//! **The `/Info` pairing is checked in both directions.** Every one of the
//! eight entries ISO 19005 pairs with an XMP property gets a fixture where
//! they agree and one where they do not, because a rule that fires on
//! everything agrees with every `-fail-` file in the corpus and is worthless.
//!
//! **Injection, counted.** Making `xmp::agrees` return `true` unconditionally —
//! the shape of the bug where a consistency rule quietly stops comparing —
//! fails **seven of the workspace's 3 375 tests**: four here
//! ([`each_disagreeing_entry_is_its_own_finding_naming_its_key`],
//! [`an_information_entry_with_no_property_to_match_is_a_finding`],
//! [`an_array_property_with_two_members_does_not_match_one_string`],
//! [`a_packet_that_says_more_than_the_information_dictionary_is_not_a_finding`])
//! and three unit tests beside the rule itself. Nothing else in the workspace
//! notices, which is what makes these seven the guard.

use tinker_pdf::{Document, FindingKind, PdfACoverage};

// ---- fixtures -------------------------------------------------------------

/// A document carrying `packet` as its `/Metadata` and `info` as its `/Info`.
///
/// Everything else about it conforms, so any finding is the one the fixture
/// was built to produce. [`a_conforming_document_has_no_findings`] is what
/// keeps that true as the rule set grows.
fn document(packet: &str, info: Option<&str>) -> Vec<u8> {
    let stream = packet.as_bytes();
    let mut objects: Vec<(u32, Vec<u8>)> = vec![
        (
            1,
            b"<< /Type /Catalog /Pages 2 0 R /Metadata 4 0 R >>".to_vec(),
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
    if let Some(info) = info {
        objects.push((5, info.as_bytes().to_vec()));
    }

    let count = objects.len();
    let mut out = b"%PDF-1.7\n%\xE2\xE3\xCF\xD3\n".to_vec();
    let mut offsets = vec![0u64; count + 1];
    for (num, body) in &objects {
        offsets[*num as usize] = out.len() as u64;
        out.extend_from_slice(format!("{num} 0 obj\n").as_bytes());
        out.extend_from_slice(body);
        out.extend_from_slice(b"\nendobj\n");
    }
    let xref_at = out.len() as u64;
    out.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", count + 1).as_bytes());
    for entry in offsets.iter().skip(1) {
        out.extend_from_slice(format!("{entry:010} 00000 n \n").as_bytes());
    }
    let info_key = if info.is_some() { " /Info 5 0 R" } else { "" };
    out.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R /ID [<0102> <0304>]{info_key} >>\nstartxref\n{xref_at}\n%%EOF\n",
            count + 1
        )
        .as_bytes(),
    );
    out
}

/// An XMP packet claiming PDF/A-2b and carrying `properties`.
fn packet(properties: &str) -> String {
    format!(
        r#"<?xpacket begin="" id="W5M0MpCehiHzreSzNTczkc9d"?>
<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF
 xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"
 xmlns:dc="http://purl.org/dc/elements/1.1/"
 xmlns:xmp="http://ns.adobe.com/xap/1.0/"
 xmlns:pdf="http://ns.adobe.com/pdf/1.3/"
 xmlns:pdfaid="http://www.aiim.org/pdfa/ns/id/">
<rdf:Description rdf:about="" pdfaid:part="2" pdfaid:conformance="B">
{properties}
</rdf:Description></rdf:RDF></x:xmpmeta><?xpacket end="w"?>"#
    )
}

/// A packet claiming `part` and `level` verbatim, whatever they are.
fn claiming(part: &str, level: &str) -> String {
    format!(
        r#"<?xpacket begin="" id="W5M0MpCehiHzreSzNTczkc9d"?>
<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF
 xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
<rdf:Description rdf:about="" xmlns:pdfaid="http://www.aiim.org/pdfa/ns/id/"
 pdfaid:part="{part}" pdfaid:conformance="{level}"/>
</rdf:RDF></x:xmpmeta><?xpacket end="w"?>"#
    )
}

fn findings(bytes: Vec<u8>) -> Vec<FindingKind> {
    Document::open(bytes)
        .expect("the fixture opens")
        .validate_pdfa()
        .findings
        .into_iter()
        .map(|finding| finding.kind)
        .collect()
}

// ---- the baseline ---------------------------------------------------------

#[test]
fn a_conforming_document_has_no_findings() {
    assert_eq!(findings(document(&packet(""), None)), Vec::new());
}

// ---- the wrong flavour, asserted by kind and by count ---------------------

/// The milestone's own exit criterion: **exactly** the metadata finding.
///
/// Each of these files is correct in every other respect, so the assertion is
/// that the list has one element and that element is the right kind. A rule
/// set that also reported, say, a missing `/ID` would pass an "any finding"
/// test and fail this one, which is the point.
#[test]
fn a_wrong_flavour_yields_exactly_the_metadata_finding() {
    // A level the part does not define: `U` belongs to parts 2 and 3.
    assert_eq!(
        findings(document(&claiming("1", "U"), None)),
        vec![FindingKind::LevelNotInPart {
            declared: "U".to_string()
        }]
    );

    // A level no part defines.
    assert_eq!(
        findings(document(&claiming("2", "Q"), None)),
        vec![FindingKind::LevelUnknown {
            declared: "Q".to_string()
        }]
    );

    // A part ISO 19005 does not define. The corpus contains both of these.
    for declared in ["9", "0"] {
        assert_eq!(
            findings(document(&claiming(declared, "B"), None)),
            vec![FindingKind::PartUnknown {
                declared: declared.to_string()
            }],
            "part {declared}"
        );
    }

    // Part 4 has no conformance letter of the parts 1-to-3 kind. This one is
    // asserted against the metadata group alone, and the reason is itself
    // worth asserting: the fixture's header is `%PDF-1.7`, so a part 4 claim
    // over it breaks 6.1.2 as well, and **both findings are right**. Scoping
    // the request is how a test says which rule it is about; widening the
    // expected list would have hidden a real second defect behind a first.
    let four = Document::open(document(&claiming("4", "B"), None))
        .expect("opens")
        .validate_pdfa_with(PdfACoverage::METADATA);
    assert_eq!(
        four.findings
            .into_iter()
            .map(|finding| finding.kind)
            .collect::<Vec<_>>(),
        vec![FindingKind::LevelNotInPart {
            declared: "B".to_string()
        }]
    );
}

/// A packet that is there and will not parse is a finding, and a different one
/// from a packet that is not there. "Metadata not checkable" is the design
/// doc's own answer for a packet the subset cannot read, and it is not a pass.
#[test]
fn a_packet_that_will_not_parse_is_its_own_finding() {
    let broken = "<?xpacket begin=\"\"?><x:xmpmeta><rdf:RDF><unclosed>";
    let kinds = findings(document(broken, None));
    assert!(
        kinds.contains(&FindingKind::MetadataUnreadable),
        "a broken packet must not read as an absent one: {kinds:?}"
    );
    assert!(
        !kinds.contains(&FindingKind::MetadataMissing),
        "it is there; it does not parse: {kinds:?}"
    );
}

// ---- 6.7.3 / 6.1.5: the information dictionary and the packet -------------

/// Every pairing, in the direction where they agree.
#[test]
fn an_information_dictionary_that_agrees_with_the_packet_is_not_a_finding() {
    let properties = r#"<dc:title><rdf:Alt><rdf:li xml:lang="x-default">A Title</rdf:li></rdf:Alt></dc:title>
<dc:creator><rdf:Seq><rdf:li>Ada Lovelace</rdf:li></rdf:Seq></dc:creator>
<dc:description><rdf:Alt><rdf:li xml:lang="x-default">A Subject</rdf:li></rdf:Alt></dc:description>
<pdf:Keywords>one two</pdf:Keywords>
<xmp:CreatorTool>Acme Writer</xmp:CreatorTool>
<pdf:Producer>Acme Engine 1.0</pdf:Producer>
<xmp:CreateDate>2026-01-02T03:04:05Z</xmp:CreateDate>
<xmp:ModifyDate>2026-02-03T04:05:06Z</xmp:ModifyDate>"#;
    let info = concat!(
        "<< /Title (A Title) /Author (Ada Lovelace) /Subject (A Subject) ",
        "/Keywords (one two) /Creator (Acme Writer) /Producer (Acme Engine 1.0) ",
        "/CreationDate (D:20260102030405Z) /ModDate (D:20260203040506Z) >>"
    );
    assert_eq!(
        findings(document(&packet(properties), Some(info))),
        Vec::new()
    );
}

/// And every pairing in the direction where they do not, one at a time, with
/// the key named so a caller knows which entry to look at (ruling 10).
#[test]
fn each_disagreeing_entry_is_its_own_finding_naming_its_key() {
    let cases: &[(&str, &str, &str)] = &[
        (
            "Title",
            r#"<dc:title><rdf:Alt><rdf:li>Something Else</rdf:li></rdf:Alt></dc:title>"#,
            "/Title (A Title)",
        ),
        (
            "Author",
            r#"<dc:creator><rdf:Seq><rdf:li>Grace Hopper</rdf:li></rdf:Seq></dc:creator>"#,
            "/Author (Ada Lovelace)",
        ),
        (
            "Subject",
            r#"<dc:description><rdf:Alt><rdf:li>Other</rdf:li></rdf:Alt></dc:description>"#,
            "/Subject (A Subject)",
        ),
        (
            "Keywords",
            "<pdf:Keywords>three four</pdf:Keywords>",
            "/Keywords (one two)",
        ),
        (
            "Creator",
            "<xmp:CreatorTool>Other Writer</xmp:CreatorTool>",
            "/Creator (Acme Writer)",
        ),
        (
            "Producer",
            "<pdf:Producer>Other Engine</pdf:Producer>",
            "/Producer (Acme Engine 1.0)",
        ),
        (
            "CreationDate",
            "<xmp:CreateDate>2020-01-02T03:04:05Z</xmp:CreateDate>",
            "/CreationDate (D:20260102030405Z)",
        ),
        (
            "ModDate",
            "<xmp:ModifyDate>2020-02-03T04:05:06Z</xmp:ModifyDate>",
            "/ModDate (D:20260203040506Z)",
        ),
    ];
    for (key, property, entry) in cases {
        assert_eq!(
            findings(document(&packet(property), Some(&format!("<< {entry} >>")))),
            vec![FindingKind::InfoXmpMismatch {
                key: (*key).to_string()
            }],
            "{key}"
        );
    }
}

/// An `/Info` entry with no XMP property at all is a mismatch, not a pass.
///
/// This is the direction a lenient reading gets wrong: the clause says the
/// entries shall be consistent with the packet, and an absent property is not
/// a value that could be consistent with anything.
#[test]
fn an_information_entry_with_no_property_to_match_is_a_finding() {
    assert_eq!(
        findings(document(&packet(""), Some("<< /Producer (Acme) >>"))),
        vec![FindingKind::InfoXmpMismatch {
            key: "Producer".to_string()
        }]
    );
}

/// The reverse is **not** a finding: a packet may say more than the `/Info`
/// does, and the clause constrains the dictionary rather than the packet.
#[test]
fn a_packet_that_says_more_than_the_information_dictionary_is_not_a_finding() {
    assert_eq!(
        findings(document(
            &packet("<pdf:Producer>Acme</pdf:Producer>"),
            Some("<< /Title (A Title) >>")
        ))
        .len(),
        1,
        "only the Title, which has no dc:title to match"
    );
    assert_eq!(
        findings(document(&packet("<pdf:Producer>Acme</pdf:Producer>"), None)),
        Vec::new(),
        "no /Info at all is no obligation at all"
    );
}

/// Two spellings of one instant are one instant. A rule comparing the strings
/// would report every producer that writes its zone differently from the way
/// it writes its PDF dates.
#[test]
fn a_date_is_compared_as_an_instant_and_not_as_a_string() {
    for (pdf_date, xmp_date) in [
        ("D:20260102030405Z", "2026-01-02T03:04:05Z"),
        ("D:20260102040405+01'00'", "2026-01-02T03:04:05Z"),
        ("D:20260102030405-05'00'", "2026-01-02T08:04:05Z"),
    ] {
        assert_eq!(
            findings(document(
                &packet(&format!("<xmp:CreateDate>{xmp_date}</xmp:CreateDate>")),
                Some(&format!("<< /CreationDate ({pdf_date}) >>"))
            )),
            Vec::new(),
            "{pdf_date} and {xmp_date} are the same moment"
        );
    }
}

/// An array with two members cannot be equivalent to one `/Info` string, and
/// the rule says so by counting rather than by picking a member.
#[test]
fn an_array_property_with_two_members_does_not_match_one_string() {
    let two = concat!(
        "<dc:creator><rdf:Seq><rdf:li>Ada</rdf:li>",
        "<rdf:li>Grace</rdf:li></rdf:Seq></dc:creator>"
    );
    assert_eq!(
        findings(document(&packet(two), Some("<< /Author (Ada) >>"))),
        vec![FindingKind::InfoXmpMismatch {
            key: "Author".to_string()
        }]
    );
}

/// Part 4 has no consistency rule, because 6.1.3 all but forbids the
/// dictionary — and that prohibition is the syntax group's, so asking for the
/// metadata group alone leaves a part 4 `/Info` unremarked.
#[test]
fn part_four_has_no_information_consistency_rule() {
    let four = r#"<?xpacket begin="" id="W5M0MpCehiHzreSzNTczkc9d"?>
<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF
 xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
<rdf:Description rdf:about="" xmlns:pdfaid="http://www.aiim.org/pdfa/ns/id/"
 pdfaid:part="4"/></rdf:RDF></x:xmpmeta><?xpacket end="w"?>"#;
    let bytes = document(four, Some("<< /Producer (Acme) >>"));
    let verdict = Document::open(bytes)
        .expect("opens")
        .validate_pdfa_with(PdfACoverage::METADATA);
    assert!(
        verdict.findings.is_empty(),
        "the metadata group has nothing to say about a part 4 /Info: {:?}",
        verdict.findings
    );
}
