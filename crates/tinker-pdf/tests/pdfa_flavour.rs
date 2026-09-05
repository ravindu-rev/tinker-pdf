//! What flavour each corpus file claims, read through `Document::validate_pdfa`.
//!
//! Milestone 1 of `docs/design/pdfa.md` reads the claim and nothing else, so
//! this is the only thing there is to measure yet — and it is worth measuring
//! on its own, because the claim is what every later rule group is checked
//! *against*. A validator that misreads the flavour reports the wrong clauses
//! for the whole file.
//!
//! The independent number to check against is the veraPDF corpus's own
//! directory layout: it files each fixture under `PDF_A-1b`, `PDF_A-2u`,
//! `PDF_A-4f` and so on. That is the suite's opinion about what each file is,
//! recorded by its publishers rather than derived from the bytes here — so a
//! flavour this build reads that disagrees with the directory it sits in is a
//! disagreement worth printing, and the count of them is asserted.
//!
//! *Amended, milestone 2.* Every assertion here now asks for
//! [`PdfACoverage::METADATA`] rather than for the default groups. The syntax
//! group has landed and the fixtures below are minimal by construction — no
//! `/ID`, no binary comment after the header — so a full validation of them
//! reports file-structure findings that have nothing to do with what this file
//! measures. Narrowing the request keeps each test about the one thing it is
//! named for, and `pdfa_syntax.rs` is where those findings are asserted, on
//! fixtures built to carry them.
//!
//! ```sh
//! cargo test -p tinker-pdf --test pdfa_flavour -- --ignored --nocapture
//! ```

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use tinker_pdf::{Document, FindingKind, Level, Part, PdfACoverage};

// ---- what a claim looks like, without a corpus ----------------------------

/// A one-page document carrying `packet` as its `/Metadata`.
fn with_metadata(packet: &str) -> Vec<u8> {
    let stream = packet.as_bytes();
    let objects: Vec<(u32, Vec<u8>)> = vec![
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

    let mut out = b"%PDF-1.7\n".to_vec();
    let mut offsets = vec![0u64; objects.len() + 1];
    for (num, body) in &objects {
        offsets[*num as usize] = out.len() as u64;
        out.extend_from_slice(format!("{num} 0 obj\n").as_bytes());
        out.extend_from_slice(body);
        out.extend_from_slice(b"\nendobj\n");
    }
    let xref_at = out.len() as u64;
    out.extend_from_slice(
        format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes(),
    );
    for entry in offsets.iter().skip(1) {
        out.extend_from_slice(format!("{entry:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref_at}\n%%EOF\n",
            objects.len() + 1
        )
        .as_bytes(),
    );
    out
}

fn packet(part: &str, conformance: Option<&str>) -> String {
    let level = match conformance {
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
 pdfaid:part="{part}"{level}{revision}/></rdf:RDF></x:xmpmeta><?xpacket end="w"?>"#
    )
}

#[test]
fn a_declared_flavour_reads_back_as_itself() {
    for (part, letter, expected) in [
        ("1", Some("B"), (Part::One, Some(Level::B))),
        ("1", Some("A"), (Part::One, Some(Level::A))),
        ("2", Some("U"), (Part::Two, Some(Level::U))),
        ("3", Some("B"), (Part::Three, Some(Level::B))),
        ("4", None, (Part::Four, None)),
    ] {
        let document = Document::open(with_metadata(&packet(part, letter))).expect("opens");
        let verdict = document.validate_pdfa_with(PdfACoverage::METADATA);
        let flavour = verdict
            .flavour
            .unwrap_or_else(|| panic!("part {part} claimed nothing: {:?}", verdict.findings));
        assert_eq!((flavour.part, flavour.level), expected, "part {part}");
        assert!(
            verdict.found_nothing(),
            "a well-formed claim is not a finding: {:?}",
            verdict.findings
        );
    }
}

/// The two shapes the corpus actually contains that do not exist.
#[test]
fn a_part_iso_19005_does_not_define_is_a_finding_and_not_an_absence() {
    for declared in ["9", "0", "", "two"] {
        let document = Document::open(with_metadata(&packet(declared, Some("B")))).expect("opens");
        let verdict = document.validate_pdfa_with(PdfACoverage::METADATA);
        assert_eq!(verdict.flavour, None, "{declared:?}");
        assert!(
            verdict.findings.iter().any(|finding| matches!(
                &finding.kind,
                FindingKind::PartUnknown { .. } | FindingKind::NoFlavourClaimed
            )),
            "{declared:?}: {:?}",
            verdict.findings
        );
    }
}

/// Part 4's own two levels, which the first version of this table denied it
/// had. The corpus has 16 files declaring `E` and 10 declaring `F`.
#[test]
fn part_four_has_its_own_two_levels_and_refuses_the_others() {
    for letter in ["E", "F"] {
        let document = Document::open(with_metadata(&packet("4", Some(letter)))).expect("opens");
        let verdict = document.validate_pdfa_with(PdfACoverage::METADATA);
        assert!(
            verdict.found_nothing(),
            "PDF/A-4{letter} is a flavour: {:?}",
            verdict.findings
        );
        assert_eq!(
            verdict.flavour.and_then(|f| f.level).map(Level::letter),
            Some(letter.chars().next().unwrap())
        );
    }
    // And `U` is not one of part 4's, nor `E` one of part 2's.
    for (part, letter) in [("4", "U"), ("2", "E"), ("1", "U")] {
        let document = Document::open(with_metadata(&packet(part, Some(letter)))).expect("opens");
        assert!(
            document
                .validate_pdfa_with(PdfACoverage::METADATA)
                .findings
                .iter()
                .any(|f| matches!(f.kind, FindingKind::LevelNotInPart { .. })),
            "part {part} does not have level {letter}"
        );
    }
}

/// A version number from a different standard is not this one's.
#[test]
fn pdfuaid_part_is_not_pdfaid_part() {
    let ua = r#"<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF
 xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
<rdf:Description rdf:about=""
 xmlns:pdfuaid="http://www.aiim.org/pdfua/ns/id/"
 pdfuaid:part="1"/></rdf:RDF></x:xmpmeta>"#;
    let verdict = Document::open(with_metadata(ua))
        .expect("opens")
        .validate_pdfa_with(PdfACoverage::METADATA);
    assert_eq!(
        verdict.flavour, None,
        "a PDF/UA version number claimed a PDF/A part: {:?}",
        verdict.flavour
    );

    // And the URI without the conventional prefix still counts as a claim.
    let bound = r#"<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF
 xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
<rdf:Description rdf:about="" xmlns:whatever="http://www.aiim.org/pdfa/ns/id/"
 whatever:part="2" whatever:conformance="B"/></rdf:RDF></x:xmpmeta>"#;
    let verdict = Document::open(with_metadata(bound))
        .expect("opens")
        .validate_pdfa_with(PdfACoverage::METADATA);
    assert_eq!(
        verdict.flavour.map(|f| f.to_string()).as_deref(),
        Some("PDF/A-2B"),
        "the namespace is what identifies it, not the prefix"
    );
}

#[test]
fn a_missing_level_is_a_finding_where_the_part_requires_one() {
    let two = Document::open(with_metadata(&packet("2", None))).expect("opens");
    assert!(
        two.validate_pdfa_with(PdfACoverage::METADATA)
            .findings
            .iter()
            .any(|f| f.kind == FindingKind::LevelMissing),
        "parts 1 to 3 must declare one"
    );

    // Part 4 without one is a flavour, not a finding.
    let four = Document::open(with_metadata(&packet("4", None))).expect("opens");
    assert!(
        four.validate_pdfa_with(PdfACoverage::METADATA)
            .found_nothing(),
        "plain PDF/A-4 is correct"
    );
}

#[test]
fn a_document_with_no_metadata_claims_nothing_and_says_so() {
    let bytes = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../testdata/simple-text.pdf"
    ))
    .expect("testdata");
    let verdict = Document::open(bytes).expect("opens").validate_pdfa();
    assert_eq!(verdict.flavour, None);
    assert!(verdict.findings.iter().any(|f| matches!(
        f.kind,
        FindingKind::MetadataMissing | FindingKind::NoFlavourClaimed
    )));
    // Milestone 5 landed the fourth group, so a default validation is now
    // complete — and this document is still not conforming, which is the
    // point: completeness makes an *empty* list mean conformance and says
    // nothing at all about a list with findings in it.
    assert!(
        verdict.coverage.is_complete(),
        "all four rule groups have landed"
    );
    assert!(
        !verdict.found_nothing(),
        "a file that claims no flavour has a finding, complete coverage or not"
    );
}

// ---- the corpus -----------------------------------------------------------

fn corpus_root() -> Option<PathBuf> {
    if let Ok(path) = std::env::var("TINKER_CORPUS") {
        let path = PathBuf::from(path);
        return path.is_dir().then_some(path);
    }
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../corpus/files")
        .canonicalize()
        .ok()
        .filter(|path| path.is_dir())
}

fn pdfs_under(root: &Path, into: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            pdfs_under(&path, into);
        } else if path
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("pdf"))
        {
            into.push(path);
        }
    }
}

/// What the veraPDF corpus's own directory says a file is, e.g. `PDF_A-2b`.
///
/// Its publishers' opinion, recorded in the layout rather than derived from
/// the bytes — which is what makes it something to check against.
fn declared_by_directory(relative: &str) -> Option<(Part, Option<Level>)> {
    // `verapdf/PDF_A-2b/6.1 File structure/…`: the flavour directory is not
    // the first component, and looking only at the first found nothing at all.
    let rest = relative
        .split('/')
        .find_map(|part| part.strip_prefix("PDF_A-"))?;
    let mut chars = rest.chars();
    let part = match chars.next()? {
        '1' => Part::One,
        '2' => Part::Two,
        '3' => Part::Three,
        '4' => Part::Four,
        _ => return None,
    };
    let level = match chars.next() {
        Some('a' | 'A') => Some(Level::A),
        Some('b' | 'B') => Some(Level::B),
        Some('u' | 'U') => Some(Level::U),
        // `PDF_A-4e` and `PDF_A-4f` are part 4's own two levels. Reading them
        // as "no level" was the mirror of the same mistake the validator made,
        // and it turned 27 files that agree into 27 that disagree.
        Some('e' | 'E') => Some(Level::E),
        Some('f' | 'F') => Some(Level::F),
        _ => None,
    };
    Some((part, level))
}

#[test]
#[ignore = "walks the fetched corpora; run with --ignored --nocapture"]
fn census_of_the_flavours_the_corpus_claims() {
    let Some(root) = corpus_root() else {
        println!("SKIPPED (no corpus; set TINKER_CORPUS)");
        return;
    };
    let mut files = Vec::new();
    pdfs_under(&root, &mut files);
    files.sort();
    println!("RAN over {} files", files.len());

    let mut claimed: BTreeMap<String, usize> = BTreeMap::new();
    let mut kinds: BTreeMap<String, usize> = BTreeMap::new();
    let (mut with_claim, mut without) = (0usize, 0usize);
    let (mut agreed, mut disagreed, mut undirected) = (0usize, 0usize, 0usize);
    let mut disagreements = Vec::new();
    let mut unexpected: Vec<String> = Vec::new();
    let mut examples: Vec<String> = Vec::new();

    for path in &files {
        let relative = path
            .strip_prefix(&root)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/");
        let Ok(bytes) = std::fs::read(path) else {
            continue;
        };
        let Ok(document) = Document::open(bytes) else {
            continue;
        };
        let verdict = document.validate_pdfa_with(PdfACoverage::METADATA);

        for finding in &verdict.findings {
            let label = match &finding.kind {
                FindingKind::PartUnknown { declared } => format!("PartUnknown({declared:?})"),
                FindingKind::LevelUnknown { declared } => format!("LevelUnknown({declared:?})"),
                FindingKind::LevelNotInPart { declared } => {
                    format!("LevelNotInPart({declared:?})")
                }
                other => format!("{other:?}"),
            };
            *kinds.entry(label).or_default() += 1;
        }

        match verdict.flavour {
            Some(flavour) => {
                with_claim += 1;
                *claimed.entry(flavour.to_string()).or_default() += 1;
                match declared_by_directory(&relative) {
                    Some(expected) => {
                        if (flavour.part, flavour.level) == expected {
                            agreed += 1;
                        } else {
                            disagreed += 1;
                            let line = format!(
                                "{relative}\n    directory says {expected:?}, file claims \
                                 {flavour}"
                            );
                            if !relative.contains("-fail-") {
                                unexpected.push(line.clone());
                            }
                            if disagreements.len() < 20 {
                                disagreements.push(line);
                            }
                        }
                    }
                    None => undirected += 1,
                }
            }
            None => without += 1,
        }

        if verdict
            .findings
            .iter()
            .any(|f| f.kind == FindingKind::LevelMissing)
            && examples.len() < 8
        {
            examples.push(relative.clone());
        }
    }

    println!("\nfiles claiming a flavour: {with_claim}; claiming none: {without}");
    for (flavour, count) in &claimed {
        println!("  {flavour:<10} {count}");
    }
    if !examples.is_empty() {
        println!("\nfiles claiming a part with no conformance level:");
        for name in &examples {
            println!("  {name}");
        }
    }
    println!("\nfindings by kind:");
    for (kind, count) in &kinds {
        println!("  {kind:<40} {count}");
    }
    println!(
        "\nagainst veraPDF's own directory layout: {agreed} agree, {disagreed} disagree, \
         {undirected} sit outside a PDF_A-* directory"
    );
    for line in &disagreements {
        println!("  {line}");
    }

    // Recorded against the corpora `corpus/corpora.lock` pins.
    //
    // **Re-recorded 6 September 2026, and the move has two causes worth
    // keeping apart.** Twenty-one of the twenty-nine are the production
    // corpus: real documents claiming PDF/A, including two that name a part
    // and no conformance level at all, which no fixture in the suite does
    // except deliberately. The other eight are drift that predates this
    // session -- the four fixture corpora alone now give 2 474 against the
    // 2 466 recorded here, measured by moving the shard out of `corpus/files`
    // and running this again. Nothing caught it because this census is not one
    // of the four `corpus.yml` ran; all thirteen run there now.
    assert_eq!(with_claim, 2495, "files declaring a PDF/A part");
    assert_eq!(
        agreed, 2148,
        "claims agreeing with the directory they sit in"
    );

    // **The property, rather than the count.** veraPDF files each fixture
    // under the flavour it is a test *for*, and a fixture testing version
    // identification is deliberately broken in exactly the respect this
    // census reads — so a disagreement is expected precisely when the file
    // name says `fail`. Every one of the ten is such a file, and asserting
    // that rather than the number is what makes this a check on the reader
    // rather than a record of today's total.
    assert!(
        unexpected.is_empty(),
        "a flavour disagreed with its directory on a file that is not a          deliberate failure: {unexpected:#?}"
    );
    assert_eq!(disagreed, 10, "deliberate version-identification failures");
}
