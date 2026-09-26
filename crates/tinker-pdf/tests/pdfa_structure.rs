//! The structural rule group: a defect in the bytes, under its ISO 19005 clause.
//!
//! `crates/tinker-pdf/src/pdfa/structure.rs` reports the strict validator's
//! structure tier through `validate_pdfa`. What it maps and what it refuses to
//! map is argued there; this file is what holds the mapping to something.
//!
//! # Why it exists, and the zero that made it
//!
//! The counted injection over the veraPDF corpus says the three mapped
//! families are not equally reachable:
//!
//! | Mapping removed | The bar, of 2 371 |
//! | --- | ---: |
//! | none (control) | 1 211 |
//! | indirect-object framing | 1 204 |
//! | stream extents | 1 208 |
//! | **the cross-reference table** | **1 211** |
//!
//! The last row is the point. Removing the 6.1.4 mapping entirely changes the
//! bar by **nothing**: not one of the 2 371 files annotated by somebody else
//! reaches it, so a rule that ran and a rule that did not would have measured
//! the same. `CONTRIBUTING.md` is explicit that a guard which catches nothing
//! when its defect is injected is not a guard — so the three fixtures below are
//! documents built here with exactly one thing wrong in their bytes, and each
//! carries the **undamaged twin** the rest of this suite's fixtures carry, so
//! "it reported something" cannot pass for "it reported this".
//!
//! Every fixture is damaged **after** the writer has finished with it, because
//! that is the only way to produce these defects at all: this repository's own
//! writer cannot emit a malformed subsection header or a `/Length` that does
//! not reach `endstream`, which is the property `validated_output.rs` exists to
//! keep true.

use tinker_pdf::{Document, FindingKind, PdfACoverage};

/// A minimal part 2 level B document, well formed in every respect.
///
/// Hand-written rather than built, so that the damage below is a byte edit with
/// a known target rather than a search through whatever the writer emitted.
fn document() -> Vec<u8> {
    let packet = "<?xpacket begin=\"\" id=\"W5M0MpCehiHzreSzNTczkc9d\"?>\
<x:xmpmeta xmlns:x=\"adobe:ns:meta/\"><rdf:RDF \
xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\">\
<rdf:Description rdf:about=\"\" xmlns:pdfaid=\"http://www.aiim.org/pdfa/ns/id/\">\
<pdfaid:part>2</pdfaid:part><pdfaid:conformance>B</pdfaid:conformance>\
</rdf:Description></rdf:RDF></x:xmpmeta><?xpacket end=\"w\"?>";
    // No colour operator at all: a device space in a file with no PDF/A
    // output intent is a *colour* finding (6.2.4.3), and this file is about
    // structure. The page draws nothing, which is exactly what is wanted.
    let content = "q 1 0 0 1 0 0 cm Q";

    let objects: Vec<(u32, Vec<u8>)> = vec![
        (
            1,
            b"<< /Type /Catalog /Pages 2 0 R /Metadata 4 0 R >>".to_vec(),
        ),
        (2, b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec()),
        (
            3,
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Resources << >> \
              /Contents 5 0 R >>"
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

/// Replaces the first occurrence of `from` with `to`, which must be the same
/// length: every offset in the cross-reference table is a byte count, so a
/// substitution that moved anything would be damaging two things at once.
fn damage(mut bytes: Vec<u8>, from: &[u8], to: &[u8]) -> Vec<u8> {
    assert_eq!(from.len(), to.len(), "the damage must not move any offset");
    let at = bytes
        .windows(from.len())
        .position(|window| window == from)
        .unwrap_or_else(|| {
            panic!(
                "the fixture does not contain {:?}",
                String::from_utf8_lossy(from)
            )
        });
    bytes[at..at + from.len()].copy_from_slice(to);
    bytes
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

/// What clause each finding was reported under, in order.
fn clauses(bytes: Vec<u8>) -> Vec<String> {
    Document::open(bytes)
        .expect("the fixture opens")
        .validate_pdfa()
        .findings
        .into_iter()
        .map(|finding| finding.clause.to_string())
        .collect()
}

/// The undamaged twin. Without this every assertion below could be passing
/// because the fixture was broken in some other way all along.
#[test]
fn the_undamaged_document_is_clean() {
    assert_eq!(
        findings(document()),
        Vec::new(),
        "the fixture this file damages must itself conform"
    );
}

/// 6.1.4: a subsection header that is not `first count`.
///
/// This is the family the corpus does not reach — the whole reason this file
/// exists — and the one the reader cannot see at all, because it merges every
/// revision's table into one before a rule could ask how any of them was
/// spelled.
#[test]
fn a_malformed_subsection_header_is_a_cross_reference_finding() {
    // `0 6` becomes `0 x`, so the count is not a number. Same length, so every
    // offset in the table still lands where it did.
    let damaged = damage(document(), b"xref\n0 6\n", b"xref\n0 x\n");
    assert_eq!(
        findings(damaged.clone()),
        vec![FindingKind::Structural {
            defect: tinker_pdf::DefectKind::SubsectionMalformed
        }],
        "the subsection header is the finding, and the only one"
    );
    assert_eq!(clauses(damaged), vec!["6.1.4".to_string()]);
}

/// 6.1.8: a cross-reference entry whose offset is not an object header.
///
/// Parts 2 and 3 number indirect objects at 6.1.9; this fixture claims part 2,
/// so that is the clause it must be reported under — which is what says the
/// per-part table is being consulted rather than a constant printed.
#[test]
fn an_entry_that_does_not_name_an_object_header_is_an_indirect_object_finding() {
    // Object 5's entry, moved two bytes earlier so it lands inside object 4's
    // `endobj` rather than on `5 0 obj`.
    let bytes = document();
    let at = bytes
        .windows(9)
        .position(|w| w == b"5 0 obj\n<")
        .expect("object 5 is there") as u64;
    let entry = format!("{at:010} 00000 n ");
    let moved = format!("{:010} 00000 n ", at - 2);
    let damaged = damage(bytes, entry.as_bytes(), moved.as_bytes());
    assert_eq!(
        findings(damaged.clone()),
        vec![FindingKind::Structural {
            defect: tinker_pdf::DefectKind::ObjectHeaderAbsent
        }],
        "the entry that names no header is the finding, and the only one"
    );
    assert_eq!(
        clauses(damaged),
        vec!["6.1.9".to_string()],
        "parts 2 and 3 number indirect objects at 6.1.9"
    );
}

/// 6.1.7: a `/Length` that does not reach `endstream`.
///
/// The one of the three the corpus does reach, and the one whose part 4 ledger
/// row closed when this group landed. Held here as well, because a rule the
/// corpus happens to exercise today is a rule nothing holds tomorrow if the
/// corpus changes.
#[test]
fn a_length_that_does_not_reach_endstream_is_a_stream_finding() {
    // The content stream is eighteen bytes and the newline before `endstream`
    // makes nineteen; claiming eleven leaves the keyword eight bytes further on
    // than the declared extent says.
    let damaged = damage(document(), b"<< /Length 18 >>", b"<< /Length 11 >>");
    let found = findings(damaged.clone());
    assert_eq!(
        found,
        vec![FindingKind::Structural {
            defect: tinker_pdf::DefectKind::StreamLengthNotExact {
                declared: 11,
                actual: Some(19)
            }
        }],
        "the declared extent is the finding, and the only one"
    );
    assert_eq!(
        clauses(damaged),
        vec!["6.1.7.1".to_string()],
        "parts 2 and 3 number stream objects at 6.1.7.1"
    );
}

/// The group is off in a syntax-only sweep, which is what makes it affordable.
///
/// Not a style point: `Coverage::SYNTAX` exists so the corpus can be swept
/// without building machinery, and this group parses the whole file a second
/// time with the leniency ladder off. A sweep that quietly did that would be
/// the design doc's laziness requirement broken where nobody would look.
#[test]
fn a_syntax_only_sweep_does_not_run_the_structural_group() {
    let damaged = damage(document(), b"xref\n0 6\n", b"xref\n0 x\n");
    let verdict = Document::open(damaged)
        .expect("it opens")
        .validate_pdfa_with(PdfACoverage::SYNTAX);
    assert!(!verdict.coverage.structure, "the group did not run");
    assert!(
        !verdict
            .findings
            .iter()
            .any(|finding| matches!(finding.kind, FindingKind::Structural { .. })),
        "and reported nothing from it: {:#?}",
        verdict.findings
    );
    assert!(
        !verdict.coverage.is_complete(),
        "a sweep missing a group says so"
    );
}
