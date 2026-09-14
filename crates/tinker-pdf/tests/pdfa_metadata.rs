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
//! **Injection, counted, on the three defects the corpus census found.**
//! Trimming both sides of the comparison again, and skipping an /Info entry
//! that is not a string again, fails two tests here and nothing else - 2 of
//! the workspace 3 375. Removing the header version upper bound fails one
//! test in pdfa_syntax.rs and nothing else - 1 of 3 375. Each defect had a
//! corpus fixture and now has a guard, which is the difference between a fix
//! and a fix that stays fixed.
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

//! **Injection, counted, on the value-type rule.** Nine defects re-introduced
//! one at a time, each run as `cargo test --no-fail-fast -p tinker-pdf`: the
//! attribute shorthand for a structure not detected, 3; every `rdf:Alt` read
//! as a language alternative, 3; the RDF and XML attribute exclusions dropped,
//! 3; part 4 running the rule, 2; the revision-drift row ignoring which part
//! asked, 2; the finding naming the metadata clause instead of its own, 1; the
//! top-level match keyed to depth instead of structure, 1; the rule never
//! called at all, 10. The ninth is the one worth keeping: dropping the
//! `rdf:RDF` grandparent test failed **nothing**, because a structure's fields
//! are reached with a property already open and the test is never consulted
//! for them. [`a_property_one_level_too_deep_is_not_a_property_of_the_document`]
//! is the fixture that took it to 1, and it exists because the zero was
//! reported rather than rounded up.

use tinker_pdf::{Document, FindingKind, PdfACoverage};

// ---- fixtures -------------------------------------------------------------

/// A document carrying `packet` as its `/Metadata` and `info` as its `/Info`.
///
/// Everything else about it conforms, so any finding is the one the fixture
/// was built to produce. [`a_conforming_document_has_no_findings`] is what
/// keeps that true as the rule set grows.
fn document(packet: &str, info: Option<&str>) -> Vec<u8> {
    document_with(packet, info, None)
}

/// The same, with `page` as the **page's** own `/Metadata` alongside the
/// catalog's.
///
/// Two packets in one file is the shape ISO 19005-2 6.6.2.3.2 is about: a
/// property a page's packet uses and the catalog's packet describes. One page
/// is enough, because the extra term is "the main package" rather than "some
/// other page's".
fn document_with_page_metadata(packet: &str, page: &str) -> Vec<u8> {
    document_with(packet, None, Some(page))
}

fn document_with(packet: &str, info: Option<&str>, page: Option<&str>) -> Vec<u8> {
    let stream = packet.as_bytes();
    let metadata = |bytes: &[u8]| {
        let mut body = format!(
            "<< /Type /Metadata /Subtype /XML /Length {} >>\nstream\n",
            bytes.len()
        )
        .into_bytes();
        body.extend_from_slice(bytes);
        body.extend_from_slice(b"\nendstream");
        body
    };
    let page_dict = if page.is_some() {
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Metadata 6 0 R >>".to_vec()
    } else {
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] >>".to_vec()
    };
    let mut objects: Vec<(u32, Vec<u8>)> = vec![
        (
            1,
            b"<< /Type /Catalog /Pages 2 0 R /Metadata 4 0 R >>".to_vec(),
        ),
        (2, b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec()),
        (3, page_dict),
        (4, metadata(stream)),
    ];
    if let Some(info) = info {
        objects.push((5, info.as_bytes().to_vec()));
    }
    if let Some(page) = page {
        objects.push((6, metadata(page.as_bytes())));
    }

    // The highest object number rather than the count of them, because these
    // fixtures number sparsely: a file with a page packet and no `/Info`
    // defines 1 to 4 and 6, and an xref subsection sized by the count would
    // stop at 5 and leave the last object unreachable.
    let count = objects
        .iter()
        .map(|(num, _)| *num as usize)
        .max()
        .unwrap_or(0);
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
    // 7.5.4: an object number the file does not define is a **free**
    // entry, not an in-use one pointing at offset zero. Writing `n` for
    // a gap names the file header as an object header, which the strict
    // structural validator reports as `ObjectHeaderAbsent` -- and it was
    // right: these fixtures number their objects sparsely, so every one
    // of them carried two such entries until the PDF/A group learned to
    // ask it.
    for entry in offsets.iter().skip(1) {
        if *entry == 0 {
            out.extend_from_slice(b"0000000000 65535 f \n");
        } else {
            out.extend_from_slice(format!("{entry:010} 00000 n \n").as_bytes());
        }
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

/// An XMP packet claiming PDF/A-1b and carrying `properties`.
///
/// Part 1 because ISO 19005-1 6.7.3 is where the `/Info` consistency
/// requirement is stated without ambiguity, and it is the only part this
/// build enforces it for — see `PDFA_STAGED` and
/// [`the_consistency_rule_is_staged_outside_part_one`].
fn packet(properties: &str) -> String {
    format!(
        r#"<?xpacket begin="" id="W5M0MpCehiHzreSzNTczkc9d"?>
<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF
 xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"
 xmlns:dc="http://purl.org/dc/elements/1.1/"
 xmlns:xmp="http://ns.adobe.com/xap/1.0/"
 xmlns:pdf="http://ns.adobe.com/pdf/1.3/"
 xmlns:pdfaid="http://www.aiim.org/pdfa/ns/id/">
<rdf:Description rdf:about="" pdfaid:part="1" pdfaid:conformance="B">
{properties}
</rdf:Description></rdf:RDF></x:xmpmeta><?xpacket end="w"?>"#
    )
}

/// A packet claiming `part` and `level` verbatim, whatever they are.
fn claiming(part: &str, level: &str) -> String {
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
 pdfaid:part="{part}" pdfaid:conformance="{level}"{revision}/>
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
            r#"<dc:title><rdf:Alt><rdf:li xml:lang="x-default">Something Else</rdf:li></rdf:Alt></dc:title>"#,
            "/Title (A Title)",
        ),
        (
            "Author",
            r#"<dc:creator><rdf:Seq><rdf:li>Grace Hopper</rdf:li></rdf:Seq></dc:creator>"#,
            "/Author (Ada Lovelace)",
        ),
        (
            "Subject",
            r#"<dc:description><rdf:Alt><rdf:li xml:lang="x-default">Other</rdf:li></rdf:Alt></dc:description>"#,
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
 pdfaid:part="4" pdfaid:rev="2020"/></rdf:RDF></x:xmpmeta><?xpacket end="w"?>"#;
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

/// The rule runs for part 1 and is a **named refusal** everywhere else.
///
/// The same file, claiming part 2 instead of part 1, produces no finding —
/// and that silence is only readable because `PDFA_STAGED` says out loud
/// that the requirement's survival into ISO 19005-2 could not be
/// established here. A staged rule that nothing names is indistinguishable
/// from a rule that ran.
#[test]
fn the_consistency_rule_is_staged_outside_part_one() {
    let two = r#"<?xpacket begin="" id="W5M0MpCehiHzreSzNTczkc9d"?>
<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF
 xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
<rdf:Description rdf:about="" xmlns:pdfaid="http://www.aiim.org/pdfa/ns/id/"
 pdfaid:part="2" pdfaid:conformance="B"/></rdf:RDF></x:xmpmeta><?xpacket end="w"?>"#;
    assert_eq!(
        findings(document(two, Some("<< /Producer (Acme) >>"))),
        Vec::new(),
        "no consistency rule runs for part 2"
    );

    let named = tinker_pdf::PDFA_STAGED
        .iter()
        .find(|rule| rule.clause == "6.1.5")
        .expect("the refusal is named");
    assert!(
        named.because.contains("19005-2"),
        "the refusal has to say which standard it could not establish: {named:?}"
    );
}

/// Padding a value with spaces makes it a different value.
///
/// The rule trimmed both sides and reported these as equal, which passed a
/// corpus fixture whose `/Author` is ` veraPDF Consortium ` against an XMP
/// `veraPDF Consortium`. Both directions are asserted, because the fix has to
/// keep agreeing where the values really are the same.
#[test]
fn whitespace_around_an_information_value_is_part_of_the_value() {
    let creator = "<dc:creator><rdf:Seq><rdf:li>Ada Lovelace</rdf:li></rdf:Seq></dc:creator>";
    assert_eq!(
        findings(document(
            &packet(creator),
            Some("<< /Author ( Ada Lovelace ) >>")
        )),
        vec![FindingKind::InfoXmpMismatch {
            key: "Author".to_string()
        }]
    );
    assert_eq!(
        findings(document(
            &packet(creator),
            Some("<< /Author (Ada Lovelace) >>")
        )),
        Vec::new()
    );
}

/// An attribute's trailing space is the value; a pretty-printed element's
/// indentation is not.
///
/// This is the heuristic `xmp::normalise` names, asserted rather than
/// described. Comparing both sides verbatim without it reported five
/// conforming corpus files whose `/Producer` ends in a space that the XMP
/// attribute ends in too.
#[test]
fn an_attribute_keeps_its_spaces_and_a_wrapped_element_does_not() {
    // The attribute form, trailing space on both sides: equal.
    let attribute = format!(
        "{}{}{}",
        r#"<?xpacket begin="" id="W5M0MpCehiHzreSzNTczkc9d"?>
<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF
 xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"
 xmlns:pdf="http://ns.adobe.com/pdf/1.3/"
 xmlns:pdfaid="http://www.aiim.org/pdfa/ns/id/">
<rdf:Description rdf:about="" pdfaid:part="1" pdfaid:conformance="B"
 pdf:Producer="Acme 1.0 "/>"#,
        "</rdf:RDF></x:xmpmeta>",
        "<?xpacket end=\"w\"?>"
    );
    assert_eq!(
        findings(document(&attribute, Some("<< /Producer (Acme 1.0 ) >>"))),
        Vec::new(),
        "the attribute declares the trailing space and the /Info carries it"
    );

    // The same value in a wrapped element: the indentation is layout.
    assert_eq!(
        findings(document(
            &packet("<pdf:Producer>\n            Acme 1.0\n         </pdf:Producer>"),
            Some("<< /Producer (Acme 1.0) >>")
        )),
        Vec::new()
    );
}

/// An `/Info` entry that is present and is not a string is a mismatch, not an
/// absence.
///
/// One corpus fixture's `/Title` is an indirect reference to a font program,
/// and reading that as "the entry is absent, so nothing is required" passed a
/// file the clause fails. Three states, because the clause has three.
#[test]
fn an_information_entry_that_is_not_a_string_is_a_finding() {
    let title =
        r#"<dc:title><rdf:Alt><rdf:li xml:lang="x-default">A Title</rdf:li></rdf:Alt></dc:title>"#;
    // `/Title` points at the metadata stream, which is not a string.
    assert_eq!(
        findings(document(&packet(title), Some("<< /Title 4 0 R >>"))),
        vec![FindingKind::InfoXmpMismatch {
            key: "Title".to_string()
        }]
    );
    // An entry that is simply absent requires nothing.
    assert_eq!(
        findings(document(&packet(title), Some("<< >>"))),
        Vec::new()
    );
}

// ---- 6.7.2 / 6.6.2.3: the predefined schemas' value types -----------------
//
// The membership half of the clause is staged and `PDFA_STAGED` names it; what
// runs is the value-type half, over the two revision tables in
// `pdfa/xmp_schemas.rs` — part 1 against January 2004, parts 2 and 3 against
// September 2005. Two things are asserted throughout, and the second is the
// one that matters.
//
// **Every case has a twin that must stay silent.** The table judges every
// property of every packet in the corpus, `pass` files included, so the failure
// mode this rule has and the earlier groups did not is reporting a conforming
// file. A fixture that fires proves the rule can see; its twin proves it can
// tell.
//
// **A property the cited revision's table does not name is never judged.**
// That is the membership half staying staged, and it is asserted against names
// the corpus actually carries rather than against an invented namespace — and
// against the names one revision has and the other does not, which is where
// the routing shows.

/// The Dublin Core binding.
const DC_NS: &str = r#"xmlns:dc="http://purl.org/dc/elements/1.1/""#;
/// The Adobe PDF binding.
const PDF_NS: &str = r#"xmlns:pdf="http://ns.adobe.com/pdf/1.3/""#;
/// The media-management binding, with the resource-reference structure's own.
const MM_NS: &str = r##"xmlns:xmpMM="http://ns.adobe.com/xap/1.0/mm/" xmlns:stRef="http://ns.adobe.com/xap/1.0/sType/ResourceRef#""##;
/// The Photoshop binding, which is where the one property the two revisions
/// give different forms lives.
const PS_NS: &str = r#"xmlns:photoshop="http://ns.adobe.com/photoshop/1.0/""#;

/// A packet claiming `part`, whose subject properties sit on a **second**
/// `rdf:Description` under `bindings`.
///
/// Two descriptions rather than one so the flavour claim and the subject
/// cannot interfere. `pdfaid:part` is an attribute like any other and the
/// attribute form of a property is judged, so a table that named `pdfaid`
/// would judge the claim itself — it does not, and
/// [`a_property_the_table_does_not_name_is_never_judged`] is what keeps that
/// from being a coincidence.
fn typed(part: &str, bindings: &str, properties: &str) -> String {
    // Part 4 takes `pdfaid:rev` and no conformance letter; parts 1 to 3 take
    // the letter and no revision. `claiming` above makes the same split.
    let flavour = if part == "4" {
        r#" pdfaid:rev="2020""#
    } else {
        r#" pdfaid:conformance="B""#
    };
    format!(
        r#"<?xpacket begin="" id="W5M0MpCehiHzreSzNTczkc9d"?>
<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF
 xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"
 xmlns:pdfaid="http://www.aiim.org/pdfa/ns/id/">
<rdf:Description rdf:about="" pdfaid:part="{part}"{flavour}/>
<rdf:Description rdf:about="" {bindings}>
{properties}
</rdf:Description></rdf:RDF></x:xmpmeta><?xpacket end="w"?>"#
    )
}

/// Every finding a part-1, -2 or -3 fixture produces, through the whole
/// validator rather than through the rule alone.
fn typed_findings(part: &str, bindings: &str, properties: &str) -> Vec<FindingKind> {
    findings(document(&typed(part, bindings, properties), None))
}

/// The same, scoped to the metadata group.
///
/// Part 4 needs this and the reason is the one
/// [`a_wrong_flavour_yields_exactly_the_metadata_finding`] already records:
/// the fixture's header is `%PDF-1.7`, so a part 4 claim over it breaks 6.1.2
/// as well, and both findings would be right. Scoping is how a test says which
/// rule it is about.
fn metadata_findings(part: &str, bindings: &str, properties: &str) -> Vec<FindingKind> {
    Document::open(document(&typed(part, bindings, properties), None))
        .expect("the fixture opens")
        .validate_pdfa_with(PdfACoverage::METADATA)
        .findings
        .into_iter()
        .map(|finding| finding.kind)
        .collect()
}

/// One value-type finding, spelled the way the rule spells it.
fn mismatch(property: &str, expected: &'static str, found: &'static str) -> Vec<FindingKind> {
    vec![FindingKind::XmpValueTypeMismatch {
        property: property.to_string(),
        expected,
        found,
    }]
}

#[test]
fn a_simple_value_where_an_array_is_declared_is_a_finding() {
    assert_eq!(
        typed_findings("1", DC_NS, "<dc:subject>archival</dc:subject>"),
        mismatch("dc:subject", "an array", "a simple value")
    );
    // The twin: the same value, written as the schema declares it.
    assert_eq!(
        typed_findings(
            "1",
            DC_NS,
            "<dc:subject><rdf:Bag><rdf:li>archival</rdf:li></rdf:Bag></dc:subject>"
        ),
        Vec::new()
    );
}

#[test]
fn an_array_where_a_simple_value_is_declared_is_a_finding() {
    // No `/Info`, so `InfoXmpMismatch` cannot fire beside it and the assertion
    // stays an equality rather than a `contains`.
    assert_eq!(
        typed_findings(
            "1",
            PDF_NS,
            "<pdf:Producer><rdf:Seq><rdf:li>Acme 1.0</rdf:li></rdf:Seq></pdf:Producer>"
        ),
        mismatch("pdf:Producer", "a simple value", "an array")
    );
    assert_eq!(
        typed_findings("1", PDF_NS, "<pdf:Producer>Acme 1.0</pdf:Producer>"),
        Vec::new()
    );
}

/// A language alternative is an `rdf:Alt` **whose items carry `xml:lang`**, and
/// the two ways of getting it wrong are different findings.
///
/// An `rdf:Alt` with untagged items is an array, not a language alternative:
/// the container is right and the thing that makes it a Lang Alt is missing.
/// Reporting both as "a simple value" would have told a caller to go and add a
/// container that is already there.
#[test]
fn a_language_alternative_is_distinguished_from_a_bare_alternative() {
    assert_eq!(
        typed_findings("1", DC_NS, "<dc:rights>Public domain</dc:rights>"),
        mismatch("dc:rights", "a language alternative", "a simple value")
    );
    assert_eq!(
        typed_findings(
            "1",
            DC_NS,
            "<dc:rights><rdf:Alt><rdf:li>Public domain</rdf:li></rdf:Alt></dc:rights>"
        ),
        mismatch("dc:rights", "a language alternative", "an array")
    );
    // The twin.
    assert_eq!(
        typed_findings(
            "1",
            DC_NS,
            r#"<dc:rights><rdf:Alt><rdf:li xml:lang="x-default">Public domain</rdf:li></rdf:Alt></dc:rights>"#
        ),
        Vec::new()
    );
}

/// The assertion behind the repair to the three `/Info` fixtures above.
///
/// `dc:title` written as a bare `rdf:Alt` is a finding, which is why those
/// fixtures now tag their items: a fixture must be correct in every respect
/// but the one under test. Asserting it here is what makes the repair a guard
/// rather than a note.
#[test]
fn a_title_without_a_language_is_a_finding_which_is_why_the_info_fixtures_tag_theirs() {
    assert_eq!(
        typed_findings(
            "1",
            DC_NS,
            "<dc:title><rdf:Alt><rdf:li>A Title</rdf:li></rdf:Alt></dc:title>"
        ),
        mismatch("dc:title", "a language alternative", "an array")
    );
    assert_eq!(
        typed_findings(
            "1",
            DC_NS,
            r#"<dc:title><rdf:Alt><rdf:li xml:lang="x-default">A Title</rdf:li></rdf:Alt></dc:title>"#
        ),
        Vec::new()
    );
}

/// A structure has three serialisations and all three are the same value.
///
/// The nested `rdf:Description` case carries a second assertion: the
/// structure's own fields are **not** judged as properties. `stRef:instanceID`
/// is not a top-level property of the document, and a rule that read it as one
/// would be judging a namespace the packet never claimed at document scope.
#[test]
fn every_serialisation_of_a_structure_is_a_structure() {
    // Written as a string, which it is not.
    assert_eq!(
        typed_findings("2", MM_NS, "<xmpMM:DerivedFrom>uuid:1</xmpMM:DerivedFrom>"),
        mismatch("xmpMM:DerivedFrom", "a structure", "a simple value")
    );

    // `rdf:parseType="Resource"`.
    assert_eq!(
        typed_findings(
            "2",
            MM_NS,
            r#"<xmpMM:DerivedFrom rdf:parseType="Resource"><stRef:instanceID>uuid:1</stRef:instanceID></xmpMM:DerivedFrom>"#
        ),
        Vec::new()
    );

    // A nested `rdf:Description`, and its fields judged by nobody.
    assert_eq!(
        typed_findings(
            "2",
            MM_NS,
            "<xmpMM:DerivedFrom><rdf:Description><stRef:instanceID>uuid:1</stRef:instanceID>\
             <stRef:documentID>uuid:2</stRef:documentID></rdf:Description></xmpMM:DerivedFrom>"
        ),
        Vec::new()
    );
}

/// The shorthand: a structure whose fields are written as attributes.
///
/// This is the serialisation a real producer writes, and until the commit that
/// landed this suite the walk read it as a simple value — so a conforming
/// `xmpMM:DerivedFrom` was reported. The exclusions are asserted beside it,
/// because each one is a way of turning that false positive back on: an
/// attribute that describes the *statement* rather than the value must not
/// make its element a structure.
#[test]
fn the_attribute_shorthand_is_a_structure_and_the_exclusions_are_not() {
    assert_eq!(
        typed_findings(
            "2",
            MM_NS,
            r#"<xmpMM:DerivedFrom stRef:instanceID="uuid:1" stRef:documentID="uuid:2"/>"#
        ),
        Vec::new()
    );

    // `rdf:resource` makes the value a reference, not a structure. It is still
    // a simple value written where a structure is declared, so the finding
    // stands — counting it as a field would have silenced a real defect.
    assert_eq!(
        typed_findings("2", MM_NS, r#"<xmpMM:DerivedFrom rdf:resource="uuid:1"/>"#),
        mismatch("xmpMM:DerivedFrom", "a structure", "a simple value")
    );

    // `xml:lang` likewise: a qualifier on the value, not part of it.
    assert_eq!(
        typed_findings(
            "2",
            MM_NS,
            r#"<xmpMM:DerivedFrom xml:lang="en">uuid:1</xmpMM:DerivedFrom>"#
        ),
        mismatch("xmpMM:DerivedFrom", "a structure", "a simple value")
    );

    // And the other direction: the shorthand on a property the table declares
    // **simple** is a finding, which is what says the detector is not simply
    // answering "structure" to everything it is asked about.
    assert_eq!(
        typed_findings(
            "2",
            format!("{DC_NS} {MM_NS}").as_str(),
            r#"<dc:format xmpMM:DerivedFrom="uuid:1"/>"#
        ),
        mismatch("dc:format", "a simple value", "a structure")
    );
}

/// A property the cited revision's table does not name is **reported**, and
/// that is the membership half.
///
/// Four cases. The third is the one the corpus pins by name —
/// `6-7-2-t03-fail-r` says `pdf:Trapped` is "not permitted in Adobe PDF
/// Schema in XMP 2004", and the string occurs in neither revision, so it is a
/// finding under every part that carries the rule. The fourth is the twin that
/// matters: `6-7-2-t04-fail-a` says "The Camera Raw Schema is not defined in
/// XMP 2004" and the September 2005 revision defines it, so the same packet is
/// a finding under part 1 and **silence** under parts 2 and 3. A membership
/// rule that read one table for every part would report a conforming part-2
/// file there.
#[test]
fn a_property_the_table_does_not_name_is_reported_as_a_non_member() {
    // An entirely unknown schema. No predefined schema declares the
    // namespace, so there is no preferred prefix and the finding names the
    // namespace instead.
    assert_eq!(
        typed_findings(
            "1",
            r#"xmlns:zz="http://example.invalid/ns/""#,
            "<zz:whatever>x</zz:whatever>"
        ),
        vec![FindingKind::XmpPropertyUndescribed {
            property: "{http://example.invalid/ns/}whatever".to_string()
        }]
    );

    // The sharper case: a schema the table **does** know, and a property in it
    // that the table does not. The namespace has a preferred prefix, so the
    // finding uses it.
    assert_eq!(
        typed_findings("1", PDF_NS, "<pdf:Whatever>x</pdf:Whatever>"),
        vec![FindingKind::XmpPropertyUndescribed {
            property: "pdf:Whatever".to_string()
        }]
    );

    // `pdf:Trapped`, which neither revision printed, under every part that
    // carries the rule.
    for part in ["1", "2", "3"] {
        assert_eq!(
            typed_findings(part, PDF_NS, "<pdf:Trapped>False</pdf:Trapped>"),
            vec![FindingKind::XmpPropertyUndescribed {
                property: "pdf:Trapped".to_string()
            }],
            "part {part}"
        );
    }

    // And the revision boundary, from both sides. The Camera Raw schema
    // arrives in September 2005, so `crs:Version` written as the simple value
    // that revision declares is a membership finding under part 1 and nothing
    // at all under parts 2 and 3.
    const CRS: &str = r#"xmlns:crs="http://ns.adobe.com/camera-raw-settings/1.0/""#;
    assert_eq!(
        typed_findings("1", CRS, "<crs:Version>3.7</crs:Version>"),
        vec![FindingKind::XmpPropertyUndescribed {
            property: "crs:Version".to_string()
        }]
    );
    for part in ["2", "3"] {
        assert_eq!(
            typed_findings(part, CRS, "<crs:Version>3.7</crs:Version>"),
            Vec::new(),
            "part {part}"
        );
    }
}

/// The schemas ISO 19005 defines for itself are not judged, and the twin of
/// that exemption is that everything else in the packet still is.
///
/// `pdfaid:part` is on every conforming file there is and no revision of the
/// XMP specification names it, so a membership rule that judged it would
/// report all of them: the conformance suite's 831 `pass` files carry 1 646
/// `pdfaid` properties between them. Asserted here rather than left to the
/// corpus, because the corpus would say it by failing 830 files at once and
/// this says it in one line.
#[test]
fn the_schemas_iso_19005_defines_for_itself_are_not_judged() {
    for part in ["1", "2", "3"] {
        // `typed` writes the flavour claim as `pdfaid` attributes on their own
        // `rdf:Description`, which the attribute form of the walk reads as
        // properties like any other.
        assert_eq!(
            typed_findings(part, DC_NS, "<dc:format>application/pdf</dc:format>"),
            Vec::new(),
            "part {part}"
        );
    }
}

/// `xmpMM:InstanceID` under part 1: the one name where two published sources
/// disagree, and the rule takes the suite's side.
///
/// The string does not occur in the 94 pages of the January 2004 XMP
/// specification. `PDF_A-1b` `6-7-2-t09-pass-q` writes it, says in its own
/// outline that it "is permitted in XMP Media Management Schema in XMP 2004",
/// and is annotated conforming — so a strict reading of the table reports a
/// file a conformance suite calls conforming, and that is the one outcome
/// this rule group is held to avoid.
///
/// The exception is **one name**, which is what the second half asserts:
/// `xmpMM:Manifest` is equally absent from the January 2004 table and is
/// equally reported, so this is not the media-management namespace being
/// waved through.
#[test]
fn the_one_name_the_two_published_sources_disagree_about_is_admitted() {
    const MM: &str = r#"xmlns:xmpMM="http://ns.adobe.com/xap/1.0/mm/""#;
    for part in ["1", "2", "3"] {
        assert_eq!(
            typed_findings(part, MM, "<xmpMM:InstanceID>uuid:1</xmpMM:InstanceID>"),
            Vec::new(),
            "part {part}"
        );
    }
    // `6-7-2-t03-fail-g` says `xmpMM:Manifest` is "not permitted in XMP Media
    // Management schema in XMP 2004", and nothing contradicts it.
    assert_eq!(
        typed_findings(
            "1",
            MM,
            "<xmpMM:Manifest><rdf:Bag><rdf:li>x</rdf:li></rdf:Bag></xmpMM:Manifest>"
        ),
        vec![FindingKind::XmpPropertyUndescribed {
            property: "xmpMM:Manifest".to_string()
        }]
    );
}

/// An extension schema describes a property into membership, and taking the
/// description away takes the membership with it.
///
/// This is ISO 19005-1 6.7.8 and ISO 19005-2 6.6.2.3.2, and it is the pair
/// that had to land with the rule: without the first assertion the rule
/// reports every conforming file that carries a custom property, and without
/// the second the exception swallows the rule.
#[test]
fn an_extension_schema_describes_a_property_into_membership() {
    const CUSTOM: &str = r#"xmlns:cs="http://example.invalid/ns/""#;
    const USE: &str = "<cs:Machine>M17</cs:Machine>";

    for part in ["1", "2", "3"] {
        assert_eq!(
            typed_findings(part, CUSTOM, USE),
            vec![FindingKind::XmpPropertyUndescribed {
                property: "{http://example.invalid/ns/}Machine".to_string()
            }],
            "part {part}"
        );
        assert_eq!(
            typed_findings(
                part,
                CUSTOM,
                &format!(
                    "{USE}{}",
                    describing("http://example.invalid/ns/", "Machine")
                )
            ),
            Vec::new(),
            "part {part}"
        );
    }

    // And the description is matched on the namespace as well as the name: a
    // schema that describes `Machine` in one namespace does not describe
    // another namespace's `Machine`.
    assert_eq!(
        typed_findings(
            "1",
            CUSTOM,
            &format!("{USE}{}", describing("http://other.invalid/ns/", "Machine"))
        ),
        vec![FindingKind::XmpPropertyUndescribed {
            property: "{http://example.invalid/ns/}Machine".to_string()
        }]
    );
}

/// A `pdfaExtension:schemas` description, complete, for `name` in `namespace`.
///
/// Written as a second top-level property of the same `rdf:Description` the
/// subject sits on, which is where a producer puts it.
fn describing(namespace: &str, name: &str) -> String {
    format!(
        r##"<pdfaExtension:schemas xmlns:pdfaExtension="http://www.aiim.org/pdfa/ns/extension/"
 xmlns:pdfaSchema="http://www.aiim.org/pdfa/ns/schema#"
 xmlns:pdfaProperty="http://www.aiim.org/pdfa/ns/property#"><rdf:Bag>
<rdf:li rdf:parseType="Resource">
<pdfaSchema:schema>An example schema</pdfaSchema:schema>
<pdfaSchema:namespaceURI>{namespace}</pdfaSchema:namespaceURI>
<pdfaSchema:prefix>cs</pdfaSchema:prefix>
<pdfaSchema:property><rdf:Seq><rdf:li rdf:parseType="Resource">
<pdfaProperty:name>{name}</pdfaProperty:name>
<pdfaProperty:valueType>Text</pdfaProperty:valueType>
<pdfaProperty:category>external</pdfaProperty:category>
<pdfaProperty:description>the machine</pdfaProperty:description>
</rdf:li></rdf:Seq></pdfaSchema:property>
</rdf:li></rdf:Bag></pdfaExtension:schemas>"##
    )
}

/// An incomplete extension schema description is a finding of its own, and the
/// complete one beside it is silent.
///
/// The entry list is the conformance suite's: `6-6-2-3-3-t01-fail-c` says a
/// description with no `pdfaSchema:schema` fails, and `t05-pass-a` says one
/// with no `pdfaSchema:property` passes. Both directions are asserted, because
/// a required-entry list that is too long reports a conforming file and one
/// that is too short reports nothing at all.
#[test]
fn an_extension_schema_description_carries_the_entries_the_suite_requires() {
    const CUSTOM: &str = r#"xmlns:cs="http://example.invalid/ns/""#;
    let complete = describing("http://example.invalid/ns/", "Machine");
    let body = format!("<cs:Machine>M17</cs:Machine>{complete}");
    assert_eq!(typed_findings("2", CUSTOM, &body), Vec::new());

    let without = body.replace(
        "<pdfaSchema:schema>An example schema</pdfaSchema:schema>",
        "",
    );
    assert_ne!(without, body);
    assert_eq!(
        typed_findings("2", CUSTOM, &without),
        vec![FindingKind::XmpExtensionEntryMissing {
            entry: "pdfaSchema:schema".to_string()
        }]
    );

    // The prefix the standard fixes, bound to the namespace the standard
    // fixes — which is exactly how the eight corpus fixtures write it, and is
    // why the check cannot be a namespace comparison.
    let renamed = body
        .replace("xmlns:pdfaProperty=", "xmlns:nonpdfaProperty=")
        .replace("<pdfaProperty:", "<nonpdfaProperty:")
        .replace("</pdfaProperty:", "</nonpdfaProperty:");
    assert_ne!(renamed, body);
    assert_eq!(
        typed_findings("2", CUSTOM, &renamed),
        vec![FindingKind::XmpExtensionPrefix {
            expected: "pdfaProperty",
            found: "nonpdfaProperty".to_string()
        }]
    );
}

/// The clause an extension-schema finding names follows the part, and the two
/// numbers differ.
///
/// Part 1 gives the description a clause of its own, 6.7.8. Parts 2 and 3
/// number it 6.6.2.3.3, which is a *different* number from the rule it is the
/// exception to — so a build that reused one `ClauseTable` for both would
/// number half of these wrong.
#[test]
fn the_clause_an_extension_schema_finding_names_follows_the_part() {
    const CUSTOM: &str = r#"xmlns:cs="http://example.invalid/ns/""#;
    let body = format!(
        "<cs:Machine>M17</cs:Machine>{}",
        describing("http://example.invalid/ns/", "Machine")
    )
    .replace("<pdfaSchema:prefix>cs</pdfaSchema:prefix>", "");
    for (part, clause) in [("1", "6.7.8"), ("2", "6.6.2.3.3"), ("3", "6.6.2.3.3")] {
        let verdict = Document::open(document(&typed(part, CUSTOM, &body), None))
            .expect("the fixture opens")
            .validate_pdfa_with(PdfACoverage::METADATA);
        let clauses: Vec<String> = verdict
            .findings
            .iter()
            .map(|finding| finding.clause.to_string())
            .collect();
        assert_eq!(clauses, vec![clause.to_string()], "part {part}");
    }
}

/// Parts 2 and 3 let the **catalog's** packet describe a property a **page's**
/// packet uses. Part 1 does not.
///
/// veraPDF's profiles say so structurally: part 1 asks
/// `isPredefinedInXMP2004 || isDefinedInCurrentPackage` and part 2 asks
/// `isPredefinedInXMP2005 || isDefinedInMainPackage || isDefinedInCurrentPackage`.
/// `6-6-2-3-2-t01-pass-b` is the fixture, and its own outline states the case
/// in words: *"The Catalog metadata defines custom property, which is used in
/// the page metadata"*, annotated conforming under part 2.
///
/// The same file under part 1 is a finding, which is what makes the extra term
/// a term rather than a spelling.
#[test]
fn only_parts_two_and_three_read_the_main_packages_descriptions() {
    const CUSTOM: &str = r#"xmlns:cs="http://example.invalid/ns/""#;
    let catalog = |part: &str| {
        typed(
            part,
            CUSTOM,
            &describing("http://example.invalid/ns/", "Machine"),
        )
    };
    let page = typed("2", CUSTOM, "<cs:Machine>M17</cs:Machine>");

    for part in ["2", "3"] {
        assert_eq!(
            findings(document_with_page_metadata(&catalog(part), &page)),
            Vec::new(),
            "part {part}"
        );
    }
    assert_eq!(
        findings(document_with_page_metadata(&catalog("1"), &page)),
        vec![FindingKind::XmpPropertyUndescribed {
            property: "{http://example.invalid/ns/}Machine".to_string()
        }],
        "part 1 has no main-package term"
    );

    // And the twin that keeps the extra term from meaning "anything goes": a
    // catalog that describes nothing leaves the page's property a finding
    // under every part, including the two that read the main package.
    for part in ["1", "2", "3"] {
        assert_eq!(
            findings(document_with_page_metadata(&typed(part, CUSTOM, ""), &page)),
            vec![FindingKind::XmpPropertyUndescribed {
                property: "{http://example.invalid/ns/}Machine".to_string()
            }],
            "part {part}"
        );
    }

    // A page packet with no property of its own is silent under every part,
    // so the findings above are the page's property and not the page packet
    // existing.
    let empty = typed("2", CUSTOM, "");
    for part in ["1", "2", "3"] {
        assert_eq!(
            findings(document_with_page_metadata(&catalog(part), &empty)),
            Vec::new(),
            "part {part}"
        );
    }
}

/// The table a property is judged against is **the one its part cites**, and a
/// name only the later revision carries reaches a different rule under each.
///
/// `xmp:Rating` is in the September 2005 XMP Basic schema (p41) and not in the
/// January 2004 one (p38-39). Written as an array where September 2005
/// declares a closed choice of Integer, it is a **value-type** finding under
/// parts 2 and 3 and a **membership** finding under part 1 — two different
/// kinds from one packet, which is the routing showing itself. The Camera Raw,
/// Dynamic Media and additional-Exif namespaces are the same case at schema
/// scope: all three arrive in September 2005.
#[test]
fn a_property_only_the_later_revision_carries_is_judged_only_under_the_later_parts() {
    const XMP_NS: &str = r#"xmlns:xmp="http://ns.adobe.com/xap/1.0/""#;
    let rating = "<xmp:Rating><rdf:Bag><rdf:li>3</rdf:li></rdf:Bag></xmp:Rating>";
    assert_eq!(
        typed_findings("1", XMP_NS, rating),
        vec![FindingKind::XmpPropertyUndescribed {
            property: "xmp:Rating".to_string()
        }]
    );
    for part in ["2", "3"] {
        assert_eq!(
            typed_findings(part, XMP_NS, rating),
            mismatch("xmp:Rating", "a simple value", "an array"),
            "part {part}"
        );
    }

    // Three whole schemas, at schema scope. Each is written in a form the
    // September 2005 table contradicts, so part 1 reporting membership rather
    // than nothing is the routing rather than a packet nobody could object to.
    for (bindings, properties, property, expected, found) in [
        (
            r#"xmlns:crs="http://ns.adobe.com/camera-raw-settings/1.0/""#,
            "<crs:Version><rdf:Bag><rdf:li>3.7</rdf:li></rdf:Bag></crs:Version>",
            "crs:Version",
            "a simple value",
            "an array",
        ),
        (
            r#"xmlns:aux="http://ns.adobe.com/exif/1.0/aux/""#,
            "<aux:Lens><rdf:Seq><rdf:li>50mm</rdf:li></rdf:Seq></aux:Lens>",
            "aux:Lens",
            "a simple value",
            "an array",
        ),
        (
            r#"xmlns:xmpDM="http://ns.adobe.com/xmp/1.0/DynamicMedia/""#,
            "<xmpDM:projectRef>x</xmpDM:projectRef>",
            "xmpDM:projectRef",
            "a structure",
            "a simple value",
        ),
    ] {
        // Under part 1 the whole schema is unknown, so the property is a
        // membership finding. It is still named with the preferred prefix:
        // `prefix_of` is deliberately not routed by part, because a prefix is
        // a spelling rather than a judgement and September 2005 is where the
        // reader will look this schema up.
        assert_eq!(
            typed_findings("1", bindings, properties),
            vec![FindingKind::XmpPropertyUndescribed {
                property: property.to_string()
            }],
            "{property} under part 1"
        );
        assert_eq!(
            typed_findings("2", bindings, properties),
            mismatch(property, expected, found),
            "{property} under part 2"
        );
    }
}
// Part 4 does not carry the requirement, and the silence is a decision.
///
/// ISO 19005-4 dropped it rather than renumbering it — the conformance suite
/// has directories for it under `PDF_A-1b` and `PDF_A-2b` and nothing anywhere
/// under `PDF_A-4`. The same body under parts 1, 2 and 3 is exactly one
/// finding each, which is what makes the part 4 silence a decision rather than
/// a rule that never runs.
#[test]
fn part_four_does_not_carry_the_predefined_schema_rule() {
    let body = "<dc:subject>archival</dc:subject>";
    assert_eq!(metadata_findings("4", DC_NS, body), Vec::new());

    for part in ["1", "2", "3"] {
        assert_eq!(
            metadata_findings(part, DC_NS, body),
            mismatch("dc:subject", "an array", "a simple value"),
            "part {part}"
        );
    }
}

/// The clause a finding names follows the part, and the two numbers differ.
///
/// Part 1 gives the whole requirement 6.7.2 and no sub-clause; parts 2 and 3
/// split it out as 6.6.2.3. One `ClauseTable` serves both, and this is what
/// says the routing is real rather than a constant that happens to read right
/// under the part every other test uses.
#[test]
fn the_clause_a_value_type_finding_names_follows_the_part() {
    for (part, clause) in [("1", "6.7.2"), ("2", "6.6.2.3"), ("3", "6.6.2.3")] {
        let verdict = Document::open(document(
            &typed(part, DC_NS, "<dc:subject>archival</dc:subject>"),
            None,
        ))
        .expect("the fixture opens")
        .validate_pdfa_with(PdfACoverage::METADATA);
        let clauses: Vec<String> = verdict
            .findings
            .iter()
            .map(|finding| finding.clause.to_string())
            .collect();
        assert_eq!(clauses, vec![clause.to_string()], "part {part}");
    }
}

/// The one property whose form the revisions disagree about, reached through
/// the flavour claim rather than through the rule directly.
///
/// `photoshop:SupplementalCategories` is `Text` on page 47 of the January 2004
/// revision and `bag Text` on page 55 of the September 2005 one. The two
/// spellings are each other's twin: whichever is conforming under one part is
/// a finding under the other, so a build that routed both parts to one table
/// would fail this in both directions at once.
#[test]
fn the_one_property_the_revisions_retyped_is_reached_through_the_flavour_claim() {
    let simple = "<photoshop:SupplementalCategories>x</photoshop:SupplementalCategories>";
    let bag = "<photoshop:SupplementalCategories><rdf:Bag><rdf:li>x</rdf:li></rdf:Bag>\
               </photoshop:SupplementalCategories>";

    assert_eq!(typed_findings("1", PS_NS, simple), Vec::new());
    assert_eq!(
        typed_findings("1", PS_NS, bag),
        mismatch(
            "photoshop:SupplementalCategories",
            "a simple value",
            "an array"
        )
    );

    assert_eq!(typed_findings("2", PS_NS, bag), Vec::new());
    assert_eq!(
        typed_findings("2", PS_NS, simple),
        mismatch(
            "photoshop:SupplementalCategories",
            "an array",
            "a simple value"
        )
    );
}

/// The attribute form of a property, written on the `rdf:Description` itself.
///
/// An attribute value is one string, so the form is simple by construction —
/// which makes it a finding for a property declared as anything else, and
/// silence for one declared simple.
#[test]
fn the_attribute_form_of_a_property_is_judged_too() {
    assert_eq!(
        typed_findings("1", format!(r#"{DC_NS} dc:title="A Title""#).as_str(), ""),
        mismatch("dc:title", "a language alternative", "a simple value")
    );
    assert_eq!(
        typed_findings(
            "1",
            format!(r#"{DC_NS} {PDF_NS} dc:format="application/pdf" pdf:Producer="Acme""#).as_str(),
            ""
        ),
        Vec::new()
    );
}

/// "Top level" is matched structurally, and the structure is both halves.
///
/// A property is one whose parent is an `rdf:Description` **that is itself a
/// child of `rdf:RDF`**. Dropping the grandparent half leaves a rule that
/// judges anything under any `rdf:Description`, and this is the shape that
/// tells the two apart: a `dc:title` one level too deep is not a property of
/// the document, so nothing is said about it. Without the test that sentence
/// was true and nothing asserted it.
#[test]
fn a_property_one_level_too_deep_is_not_a_property_of_the_document() {
    let nested = format!(
        r#"<?xpacket begin="" id="W5M0MpCehiHzreSzNTczkc9d"?>
<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF
 xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"
 xmlns:pdfaid="http://www.aiim.org/pdfa/ns/id/">
<rdf:Description rdf:about="" pdfaid:part="1" pdfaid:conformance="B"/>
<rdf:Description rdf:about="" {DC_NS}>
<rdf:Description><dc:title>A Title</dc:title></rdf:Description>
</rdf:Description></rdf:RDF></x:xmpmeta><?xpacket end="w"?>"#
    );
    assert_eq!(findings(document(&nested, None)), Vec::new());

    // The twin, one level up: the same `dc:title`, the same wrong form, and
    // now it **is** a property of the document.
    assert_eq!(
        typed_findings("1", DC_NS, "<dc:title>A Title</dc:title>"),
        mismatch("dc:title", "a language alternative", "a simple value")
    );
}

/// A packet carrying one property of every form, asserted silent.
///
/// The in-suite false-positive guard. Every other test here fires on one
/// property; this one is the shape a real producer writes, and the rule must
/// have nothing to say about it.
#[test]
fn a_conforming_packet_of_every_form_is_silent() {
    assert_eq!(
        typed_findings(
            "2",
            format!("{DC_NS} {PDF_NS} {MM_NS}").as_str(),
            r#"<dc:title><rdf:Alt><rdf:li xml:lang="x-default">A Title</rdf:li></rdf:Alt></dc:title>
<dc:creator><rdf:Seq><rdf:li>Ada Lovelace</rdf:li></rdf:Seq></dc:creator>
<dc:format>application/pdf</dc:format>
<pdf:Producer>Acme 1.0</pdf:Producer>
<xmpMM:DerivedFrom stRef:instanceID="uuid:1"/>"#
        ),
        Vec::new()
    );
}
