//! What `Document::signatures` makes of real signed documents (12.8).
//!
//! Two kinds of test live here and they answer different questions. The
//! fixtures built inline answer "does the coverage classifier say the right
//! thing about a shape I chose", and every shape worth naming is here because
//! the corpus does not contain one of each. The corpus census answers the
//! question no fixture can — "is the shape I chose the shape real producers
//! emit" — and it is `#[ignore]`d because it walks a fetched tree.
//!
//! ```sh
//! cargo test -p tinker-pdf --test signatures -- --ignored --nocapture
//! ```
//!
//! **The census reads bytes and nothing else.** Its `/ByteRange` scanner is a
//! regex-free byte scan written here, sharing nothing with
//! `crates/tinker-pdf/src/signature.rs`, so a file the reader fails to see is
//! a disagreement rather than an agreement — a census taken with the reader's
//! own field walk would find exactly the signatures the reader finds, which
//! proves nothing about the ones it misses.

use std::path::{Path, PathBuf};

use tinker_pdf::{Anchor, Coverage, DigestAlgorithm, Document, SubFilter};

// ---- fixtures, built here so every shape has one --------------------------

/// Where the signature dictionary sits relative to the field.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Shape {
    /// 12.7.4.5's shape: a `/FT /Sig` field whose `/V` is the dictionary.
    Separate,
    /// The field dictionary carrying the signature entries itself, which is
    /// the shape [`Anchor::MergedField`] exists for and which no corpus file
    /// emits — so this fixture is the only thing that exercises it.
    Merged,
}

/// A minimal one-page document with a signature field whose `/ByteRange`
/// leaves a gap where `/Contents` sits.
///
/// Built by assembling the body, measuring it, and patching the four numbers
/// in — which is exactly what a signer does (12.8.1) and the only way to get a
/// self-consistent `/ByteRange` without hand-counting bytes.
fn signed_document(contents_hex: &str, cover_whole_file: bool, shape: Shape) -> Vec<u8> {
    const SIGNATURE_ENTRIES: &str = "/Filter /Adobe.PPKLite /SubFilter /adbe.pkcs7.detached \
                                     /M (D:20260101120000Z) /Reason (Because) /Location (Here) \
                                     /Name (A Signer) \
                                     /ByteRange [0000000000 0000000000 0000000000 0000000000] \
                                     /Contents <";
    let count: u32 = if shape == Shape::Separate { 6 } else { 5 };

    let mut fixed: Vec<(u32, String)> = vec![
        (
            1,
            "<< /Type /Catalog /Pages 2 0 R /AcroForm << /Fields [4 0 R] /SigFlags 3 >> >>".into(),
        ),
        (2, "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".into()),
        (
            3,
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] >>".into(),
        ),
    ];
    if shape == Shape::Separate {
        fixed.push((
            4,
            "<< /Type /Annot /Subtype /Widget /FT /Sig /T (Signature1) /V 5 0 R \
             /Rect [0 0 0 0] /F 4 /P 3 0 R >>"
                .into(),
        ));
    }

    // The signature object is written last, because the `/ByteRange` numbers
    // cannot be known until everything around them has a length.
    let mut out = b"%PDF-1.7\n".to_vec();
    let mut offsets = vec![0u64; count as usize + 1];
    for (num, body) in &fixed {
        offsets[*num as usize] = out.len() as u64;
        out.extend_from_slice(format!("{num} 0 obj\n{body}\nendobj\n").as_bytes());
    }

    let signature_num = if shape == Shape::Separate { 5 } else { 4 };
    offsets[signature_num as usize] = out.len() as u64;
    let head = match shape {
        Shape::Separate => format!("{signature_num} 0 obj\n<< /Type /Sig {SIGNATURE_ENTRIES}"),
        Shape::Merged => format!(
            "{signature_num} 0 obj\n<< /Type /Annot /Subtype /Widget /FT /Sig /T (Signature1) \
             /Rect [0 0 0 0] /F 4 /P 3 0 R {SIGNATURE_ENTRIES}"
        ),
    };
    let byte_range_at = out.len() + head.find("[0000000000").expect("the placeholder is there");
    out.extend_from_slice(head.as_bytes());
    let gap_start = out.len() - 1; // the `<` itself is inside the gap
    out.extend_from_slice(contents_hex.as_bytes());
    out.extend_from_slice(b">");
    let gap_end = out.len();
    out.extend_from_slice(b" >>\nendobj\n");

    let xref_at = out.len() as u64;
    out.extend_from_slice(format!("xref\n0 {count}\n0000000000 65535 f \n").as_bytes());
    for entry in offsets.iter().take(count as usize).skip(1) {
        out.extend_from_slice(format!("{entry:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(
        format!("trailer\n<< /Size {count} /Root 1 0 R >>\nstartxref\n{xref_at}\n%%EOF\n")
            .as_bytes(),
    );

    let tail_len = if cover_whole_file {
        out.len() - gap_end
    } else {
        // Stop ten bytes short of the end, which is neither the whole file nor
        // a revision boundary nor an end-of-line marker.
        out.len() - gap_end - 10
    };
    let numbers = format!(
        "[{:010} {:010} {:010} {:010}]",
        0, gap_start, gap_end, tail_len
    );
    assert_eq!(
        numbers.len(),
        "[0000000000 0000000000 0000000000 0000000000]".len(),
        "the patch must be exactly as wide as the placeholder it replaces"
    );
    out[byte_range_at..byte_range_at + numbers.len()].copy_from_slice(numbers.as_bytes());
    out
}

#[test]
fn a_whole_file_signature_reads_back_with_its_dictionary_and_its_coverage() {
    let bytes = signed_document("30820102000000", true, Shape::Separate);
    let document = Document::open(bytes).expect("the fixture opens");
    let signatures = document.signatures();
    assert_eq!(signatures.len(), 1, "one signature field, one signature");

    let signature = &signatures[0];
    assert_eq!(signature.field.as_deref(), Some("Signature1"));
    assert_eq!(signature.anchor, Anchor::Field);
    assert_eq!(signature.sub_filter, Some(SubFilter::Pkcs7Detached));
    assert_eq!(signature.filter.as_deref(), Some("Adobe.PPKLite"));
    assert_eq!(signature.reason.as_deref(), Some("Because"));
    assert_eq!(signature.location.as_deref(), Some("Here"));
    assert_eq!(signature.name.as_deref(), Some("A Signer"));
    assert_eq!(
        signature
            .signed_at
            .map(|date| (date.year, date.month, date.day)),
        Some((2026, 1, 1))
    );
    assert_eq!(
        signature.contents,
        vec![0x30, 0x82, 0x01, 0x02, 0x00, 0x00, 0x00],
        "the CMS comes out of the gap, not out of the object model"
    );
    assert_eq!(signature.coverage, Coverage::WholeFile);
    assert!(signature.warnings.is_empty(), "{:?}", signature.warnings);
    assert!(signature
        .digest(&document, DigestAlgorithm::Sha256)
        .is_some());
}

#[test]
fn a_signature_that_stops_short_of_the_end_is_suspicious_rather_than_whole() {
    let bytes = signed_document("3082010200", false, Shape::Separate);
    let document = Document::open(bytes).expect("the fixture opens");
    let signatures = document.signatures();
    assert_eq!(signatures.len(), 1);
    match &signatures[0].coverage {
        Coverage::Suspicious(defect) => {
            let rendered = format!("{defect:?}");
            assert!(
                rendered.starts_with("EndsMidFile"),
                "unsigned trailing bytes must be named: {rendered}"
            );
        }
        other => panic!("expected suspicious coverage, got {other:?}"),
    }
}

#[test]
fn flipping_a_covered_byte_of_a_real_fixture_changes_its_digest() {
    let bytes = signed_document("30820102", true, Shape::Separate);
    let original = Document::open(bytes.clone()).expect("opens");
    let before = original.signatures()[0]
        .digest(&original, DigestAlgorithm::Sha256)
        .expect("the spans fit");

    // Byte 4 is inside `%PDF-1.7`, which the first span covers.
    let mut tampered = bytes;
    tampered[4] = b'2';
    let after_document = Document::open(tampered).expect("still opens");
    let after = after_document.signatures()[0]
        .digest(&after_document, DigestAlgorithm::Sha256)
        .expect("the spans fit");
    assert_ne!(before, after, "a covered byte must move the digest");
}

#[test]
fn a_signature_field_with_no_value_is_not_a_signature() {
    let body = b"%PDF-1.7\n\
1 0 obj\n<< /Type /Catalog /Pages 2 0 R /AcroForm << /Fields [4 0 R] >> >>\nendobj\n\
2 0 obj\n<< /Type /Pages /Kids [3 0 R] /Count 1 >>\nendobj\n\
3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] >>\nendobj\n\
4 0 obj\n<< /Type /Annot /Subtype /Widget /FT /Sig /T (Unsigned) /Rect [0 0 0 0] /P 3 0 R >>\nendobj\n\
trailer\n<< /Size 5 /Root 1 0 R >>\n%%EOF\n"
        .to_vec();
    let document = Document::open(body).expect("the repair ladder opens it");
    assert!(
        document.signatures().is_empty(),
        "an empty signature field is a place for one"
    );
    assert_eq!(
        document.form_fields().len(),
        1,
        "and it is still a form field"
    );
}

/// The shape no corpus file emits, so the fixture is the only thing holding
/// [`Anchor::MergedField`] up.
#[test]
fn a_field_carrying_the_signature_entries_itself_is_still_a_signature() {
    let bytes = signed_document("30820102", true, Shape::Merged);
    let document = Document::open(bytes).expect("the fixture opens");
    let signatures = document.signatures();
    assert_eq!(signatures.len(), 1, "one field, still one signature");

    let signature = &signatures[0];
    assert_eq!(signature.anchor, Anchor::MergedField);
    assert_eq!(signature.field.as_deref(), Some("Signature1"));
    assert_eq!(signature.coverage, Coverage::WholeFile);
    assert_eq!(signature.contents, vec![0x30, 0x82, 0x01, 0x02]);
    assert_eq!(
        signature.field_ref, signature.value_ref,
        "merged means the two are the same object"
    );
}

/// A signature, then an incremental update over it (7.5.6).
///
/// This is the `Coverage::Revision` path, and **no file in the fetched corpora
/// reaches it** — every signed corpus file either covers its whole file or has
/// a `/ByteRange` that points somewhere the file no longer is. So this fixture
/// is the only thing that exercises the classification, and it builds the
/// second revision with the engine's own incremental writer rather than by
/// appending bytes by hand, so the boundary it lands on is a real one.
#[test]
fn a_signature_survived_by_an_incremental_update_covers_a_revision() {
    let original = signed_document("30820102", true, Shape::Separate);
    let document = Document::open(original.clone()).expect("the fixture opens");
    assert_eq!(document.signatures()[0].coverage, Coverage::WholeFile);

    let mut editor = document.editor();
    assert!(editor.rotate_page(0, 90), "the page rotates");
    let updated = editor.save(&tinker_pdf::WriteOptions {
        mode: tinker_pdf::WriteMode::Incremental,
        ..Default::default()
    });
    assert!(
        updated.starts_with(&original),
        "an incremental save leaves the signed prefix alone"
    );
    assert!(updated.len() > original.len(), "and appends a revision");

    let after = Document::open(updated).expect("the updated document opens");
    let signatures = after.signatures();
    assert_eq!(signatures.len(), 1);
    assert_eq!(
        signatures[0].coverage,
        Coverage::Revision { index: 1 },
        "the signature still covers the revision it was made over, \
         and revisions are newest first"
    );
    assert!(
        !signatures[0].covers_whole_file(),
        "what it does not cover is the update"
    );
}

/// Writes the `signatures` fuzz target's seed corpus.
///
/// The seeds are written by the test that owns the fixture builder, which is
/// this repository's convention (`verification.md`) and the reason for it:
/// a seed corpus assembled somewhere else drifts from the fixtures it was
/// derived from, silently, and a fuzzer starting from stale seeds explores the
/// shape a parser used to have.
///
/// Each seed is a whole document, because that is what the target takes. They
/// are chosen for the branches they reach rather than for being realistic:
/// every one of them is a *shape* the coverage classifier has an arm for.
#[test]
#[ignore = "writes into fuzz/corpus/, which is committed"]
fn write_the_fuzz_seeds() {
    let base =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fuzz/corpus/signatures");
    std::fs::create_dir_all(&base).expect("the corpus directory");

    let whole = signed_document("308201020000", true, Shape::Separate);
    let short = signed_document("308201020000", false, Shape::Separate);
    let merged = signed_document("30820102", true, Shape::Merged);

    // A `/ByteRange` whose numbers are arithmetically fine and point at
    // something that is not a hexadecimal string: the `GapIsNotContents` arm,
    // which three corpus files reach and no other seed here does.
    let mut misdirected = whole.clone();
    if let Some(at) = find(&misdirected, b"[00000000") {
        misdirected[at..at + 45].copy_from_slice(b"[0000000000 0000000009 0000000030 0000000004]");
    }

    // Two spans that overlap, which no producer emits and a reader must not
    // read as coverage.
    let mut overlapping = whole.clone();
    if let Some(at) = find(&overlapping, b"[00000000") {
        overlapping[at..at + 45].copy_from_slice(b"[0000000000 0000000100 0000000050 0000000004]");
    }

    for (name, bytes) in [
        ("whole-file", &whole),
        ("ends-mid-file", &short),
        ("merged-field", &merged),
        ("gap-is-not-contents", &misdirected),
        ("overlapping-spans", &overlapping),
    ] {
        std::fs::write(base.join(name), bytes).expect("the corpus directory is there");
    }

    // The seeds must be what they claim, or they are five copies of one path.
    for (bytes, expected) in [
        (&whole, "WholeFile"),
        (&short, "EndsMidFile"),
        (&misdirected, "GapIsNotContents"),
        (&overlapping, "SpansOverlap"),
    ] {
        let document = Document::open(bytes.clone()).expect("the seed opens");
        let signatures = document.signatures();
        assert_eq!(signatures.len(), 1, "{expected}");
        assert!(
            format!("{:?}", signatures[0].coverage).contains(expected),
            "the {expected} seed reaches {:?} instead",
            signatures[0].coverage
        );
    }
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len())
        .position(|window| window == needle)
}

// ---- the corpus census ----------------------------------------------------

fn corpus_root() -> Option<PathBuf> {
    if let Ok(path) = std::env::var("TINKER_CORPUS") {
        let path = PathBuf::from(path);
        return path.is_dir().then_some(path);
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/files");
    root.canonicalize().ok().filter(|path| path.is_dir())
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

/// Every file whose raw bytes contain `/ByteRange`, found without the reader.
fn files_naming_a_byte_range(root: &Path) -> Vec<PathBuf> {
    let mut all = Vec::new();
    pdfs_under(root, &mut all);
    all.retain(|path| {
        std::fs::read(path).is_ok_and(|bytes| {
            bytes
                .windows(b"/ByteRange".len())
                .any(|window| window == b"/ByteRange")
        })
    });
    all.sort();
    all
}

#[test]
#[ignore = "walks the fetched corpora; run with --ignored --nocapture"]
fn census_of_the_corpus_signatures() {
    let Some(root) = corpus_root() else {
        println!("SKIPPED (no corpus; set TINKER_CORPUS)");
        return;
    };
    let candidates = files_naming_a_byte_range(&root);
    println!("RAN over {} files naming /ByteRange", candidates.len());
    assert!(
        candidates.len() >= 25,
        "the fetched corpora carried 25 when this was written; found {}",
        candidates.len()
    );

    let mut seen = 0usize;
    let mut whole = 0usize;
    let mut revision = 0usize;
    let mut suspicious = 0usize;
    let mut unreadable = Vec::new();
    let mut routes: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();

    for path in &candidates {
        let name = path
            .strip_prefix(&root)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/");
        let Ok(bytes) = std::fs::read(path) else {
            continue;
        };
        let Ok(document) = Document::open(bytes) else {
            unreadable.push(name);
            continue;
        };
        // An encrypted document's object streams do not decompress until the
        // file key exists, so a signature living in one is invisible until
        // then. Most encrypted documents open on the empty user password, and
        // a caller reading signatures has authenticated before asking.
        if document.is_encrypted() {
            let _ = document.authenticate("");
        }
        let signatures = document.signatures();
        if signatures.is_empty() {
            // A file naming `/ByteRange` that yields no signature is either a
            // file with no signature in it (qpdf commits one deliberately) or
            // a route this reader does not walk. Print enough to tell them
            // apart rather than leaving the difference to guesswork.
            println!("  {name}: /ByteRange present, no signature reached");
            for field in document.form_fields() {
                println!(
                    "    field {:?} kind {:?} at {:?} widgets {:?}",
                    field.name, field.kind, field.reference, field.widgets
                );
            }
            let catalog = document.cos().catalog();
            println!("    catalog keys {:?}", catalog.is_some());
        }
        for signature in &signatures {
            seen += 1;
            let coverage = match &signature.coverage {
                Coverage::WholeFile => {
                    whole += 1;
                    "whole-file".to_string()
                }
                Coverage::Revision { index } => {
                    revision += 1;
                    format!("revision {index}")
                }
                Coverage::Suspicious(defect) => {
                    suspicious += 1;
                    // What the revisions actually are, so a near miss is
                    // distinguishable from a range pointing nowhere.
                    let ends: Vec<u64> = document
                        .cos()
                        .revisions()
                        .iter()
                        .map(|revision| revision.byte_range.end)
                        .collect();
                    format!("suspicious {defect:?}; revision ends {ends:?}")
                }
            };
            *routes
                .entry(match &signature.anchor {
                    Anchor::Field => "the field's /V".to_string(),
                    Anchor::MergedField => "a field with the dictionary merged in".to_string(),
                    Anchor::Permissions(key) => format!("/Perms /{key}"),
                })
                .or_insert(0usize) += 1;
            println!(
                "  {name}\n    via {:?}, field {:?}, subfilter {:?}, contents {} bytes, {coverage}",
                signature.anchor,
                signature.field.as_deref().unwrap_or("-"),
                signature.sub_filter_name.as_deref().unwrap_or("-"),
                signature.contents.len()
            );
            for warning in &signature.warnings {
                println!("    warn {warning:?}");
            }
        }
    }

    println!(
        "signatures {seen}: {whole} whole-file, {revision} over a revision, {suspicious} suspicious"
    );
    for (route, count) in &routes {
        println!("  reached through {route}: {count}");
    }
    if !unreadable.is_empty() {
        println!("files that would not open at all: {unreadable:?}");
    }

    // Recorded against the corpora `corpus/corpora.lock` pins, so a shrinking
    // result cannot read as a passing one. Each number was checked by hand
    // against the file's bytes when it was written:
    //
    // - 27 signatures in 24 files, of 25 naming `/ByteRange`. The file naming
    //   one and carrying no signature is qpdf's `bad-content.pdf`, which has
    //   no `/Type /Sig`, no `/Perms` and no CMS blob — it is deliberately not
    //   a signature.
    // - 6 suspicious: two `/ByteRange`s that run past the end of a file edited
    //   after signing, three gaps that land in XMP or XFA rather than on a
    //   hexadecimal string, and one fuzzed file.
    //
    // Re-pinning a corpus moves these, and moving them is a deliberate act
    // with its own commit and its own reason.
    //
    // **They moved on 6 September 2026, and the reason is worth keeping.** It
    // was 18 signatures, 11 whole-file and 1 over a revision, until the corpus
    // gained its fifth entry: the SafeDocs shard, a thousand documents off the
    // open web. Nine of the nine new signatures are real-world ones, and four
    // of them sign an *earlier revision* — a case the fixture corpora had
    // exactly one of. Measured rather than assumed: moving the shard out of
    // `corpus/files` and running this again reproduces 18/11/1/6 exactly.
    assert_eq!(seen, 27, "signatures found across the fetched corpora");
    assert_eq!(whole, 16, "signatures covering their whole file");
    assert_eq!(revision, 5, "signatures over an earlier revision");
    assert_eq!(suspicious, 6, "signatures whose coverage does not hold up");
    assert_eq!(
        routes.get("/Perms /UR3").copied().unwrap_or_default(),
        5,
        "usage-rights signatures no field walk reaches"
    );
    assert!(
        unreadable.is_empty(),
        "every candidate opened: {unreadable:?}"
    );
}
