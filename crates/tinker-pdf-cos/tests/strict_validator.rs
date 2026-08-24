//! The strict validator's rules, each fired by an injected defect.
//!
//! Ruling 13 retires the subprocess oracles, and the roadmap's order is fixed:
//! nothing is deleted before the first-party check replacing it exists **and
//! has been injection-counted**. This file is the counting.
//!
//! Every rule here appears twice: once as a document this engine wrote, which
//! must validate clean, and once as the same document with one defect put back
//! into it, which must be refused *by name*. A rule that fires on the twin as
//! well is not a rule, and a rule that fires on nothing is not one either.
//!
//! # Where the injections aim
//!
//! Byte patches keep their own length wherever they can, so the file's offsets
//! do not move and the defect under test is the only thing that changed. Where
//! that is impossible — a table with no free head, an entry of nineteen bytes —
//! the file is hand-built here rather than written by this crate's writer,
//! because the point of those rules is that the writer cannot produce them.
//!
//! # The two the tolerant reader cannot see
//!
//! Two injections below open at [`LadderLevel::Trust`] with no warning at all,
//! and no other test in this repository notices them:
//!
//! - an entry whose offset points a few bytes *before* its object, which the
//!   reader accepts because its header lookup lexes forward;
//! - an entry whose generation the table invented, which the reader silently
//!   normalises to the one the header spells.
//!
//! Both were found by running the injections, not by reading the code.

mod surface_support;

use std::sync::Arc;

use surface_support::whole_surface_document;
use tinker_pdf_cos::dest::DestKind;
use tinker_pdf_cos::{
    CosDocument, Defect, DocumentBuilder, DocumentEditor, Encryption, LadderLevel, OutlineEntry,
    Target, WriteMode, WriteOptions,
};

// ---- the documents under test ----------------------------------------------

/// A small document with one font shared by every page.
fn document(pages: usize) -> Vec<u8> {
    let mut builder = DocumentBuilder::new();
    builder.add_base_font(b"F0", b"Helvetica");
    builder.set_info(b"Title", "strict");
    for index in 0..pages {
        builder.add_page(200.0, 100.0, |page| {
            page.text(b"F0", 12.0, 10.0, 50.0, &format!("page {index}"));
        });
    }
    builder.finish()
}

/// The same document saved through one of the writer's layouts.
fn saved(options: &WriteOptions) -> Vec<u8> {
    let doc = Arc::new(CosDocument::open(document(3)).expect("it opens"));
    DocumentEditor::new(doc).save(options)
}

fn rewritten() -> Vec<u8> {
    saved(&WriteOptions {
        mode: WriteMode::Rewrite,
        object_streams: false,
        ..WriteOptions::default()
    })
}

/// The object-stream layout, left uncompressed so the container's own prologue
/// is legible to an injection.
fn packed() -> Vec<u8> {
    saved(&WriteOptions {
        mode: WriteMode::Rewrite,
        compress: false,
        object_streams: true,
        ..WriteOptions::default()
    })
}

/// The two halves of the trailer's `/ID`.
fn identifier(doc: &CosDocument) -> (Vec<u8>, Vec<u8>) {
    let id = doc
        .trailer()
        .get_array(tinker_pdf_cos::Name::ID)
        .expect("every file this engine writes carries one");
    let part = |index: usize| -> Vec<u8> {
        id.get(index)
            .and_then(tinker_pdf_cos::Object::as_string)
            .map(|s| s.bytes.clone())
            .expect("two strings")
    };
    (part(0), part(1))
}

/// The same document, sealed with the deterministic entropy gap 19 uses.
fn encrypted() -> Vec<u8> {
    let mut entropy = [0u8; 48];
    for (index, byte) in entropy.iter_mut().enumerate() {
        *byte = (index as u8).wrapping_mul(7).wrapping_add(11);
    }
    let doc = Arc::new(CosDocument::open(document(3)).expect("it opens"));
    DocumentEditor::new(doc).save(&WriteOptions {
        mode: WriteMode::Rewrite,
        object_streams: false,
        encryption: Some(Encryption {
            user_password: "open-me".to_string(),
            owner_password: "owner-me".to_string(),
            permissions: -1,
            entropy,
        }),
        ..WriteOptions::default()
    })
}

// ---- reading a verdict ------------------------------------------------------

fn verdict(bytes: Vec<u8>) -> Vec<Defect> {
    let doc = CosDocument::open(bytes).expect("it opens");
    tinker_pdf_cos::validate(&doc)
}

fn labels(defects: &[Defect]) -> Vec<&'static str> {
    defects.iter().map(|d| d.kind.as_str()).collect()
}

/// Asserts that the injection is refused by the named rule, and says what was
/// found when it is not.
#[track_caller]
fn refused(bytes: Vec<u8>, rule: &str) {
    let defects = verdict(bytes);
    assert!(
        labels(&defects).contains(&rule),
        "expected `{rule}`, found {:?}",
        defects.iter().map(Defect::to_string).collect::<Vec<_>>()
    );
}

#[track_caller]
fn clean(bytes: Vec<u8>) {
    let defects = verdict(bytes);
    assert!(
        defects.is_empty(),
        "expected no defects, found {:?}",
        defects.iter().map(Defect::to_string).collect::<Vec<_>>()
    );
}

// ---- byte surgery -----------------------------------------------------------

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.len() > hay.len() {
        return None;
    }
    (0..=hay.len() - needle.len()).find(|&at| &hay[at..at + needle.len()] == needle)
}

/// Replaces one occurrence with something exactly as long, so every offset in
/// the file still means what it meant.
fn patch(bytes: &[u8], needle: &[u8], with: &[u8]) -> Vec<u8> {
    assert_eq!(
        needle.len(),
        with.len(),
        "an injection may not move offsets"
    );
    let at = find(bytes, needle)
        .unwrap_or_else(|| panic!("no {:?} in the fixture", String::from_utf8_lossy(needle)));
    let mut out = bytes.to_vec();
    out[at..at + with.len()].copy_from_slice(with);
    out
}

/// Replaces the bytes at a known position, rather than at the first place a
/// needle happens to match: `/Size 10`'s digits also occur inside the table,
/// and patching those instead produced a rescued file and a passing test that
/// was measuring nothing.
fn patch_at(bytes: &[u8], at: usize, with: &[u8]) -> Vec<u8> {
    let mut out = bytes.to_vec();
    out[at..at + with.len()].copy_from_slice(with);
    out
}

/// The run of digits beginning at `at`.
fn digits_at(bytes: &[u8], at: usize) -> Vec<u8> {
    bytes[at..]
        .iter()
        .copied()
        .take_while(u8::is_ascii_digit)
        .collect()
}

/// Where the first classic entry of the table begins, and the table's own
/// offset. Entries are twenty bytes each from there (7.5.4).
fn first_entry(bytes: &[u8]) -> usize {
    let table = find(bytes, b"xref\n").expect("a classic table");
    find(
        &bytes[table + 5..],
        b"
",
    )
    .expect("a subsection header")
        + table
        + 6
}

/// Rewrites the ten-digit offset field of the entry for object `num`.
fn entry_offset(bytes: &[u8], num: u32, offset: u64) -> Vec<u8> {
    let at = first_entry(bytes) + (num as usize) * 20;
    let mut out = bytes.to_vec();
    out[at..at + 10].copy_from_slice(format!("{offset:010}").as_bytes());
    out
}

/// Patches the first occurrence of `needle` **after** `anchor`.
///
/// `/Count` appears in a page tree and in an outline, `/Dest` in a link and in
/// an outline item: a needle alone would hit whichever comes first in the file,
/// which is not the structure the test is about.
fn patch_after(bytes: &[u8], anchor: &[u8], needle: &[u8], with: &[u8]) -> Vec<u8> {
    assert_eq!(
        needle.len(),
        with.len(),
        "an injection may not move offsets"
    );
    let from = find(bytes, anchor).expect("the anchor is in the fixture");
    let at = from + find(&bytes[from..], needle).expect("the needle follows it");
    patch_at(bytes, at, with)
}

/// A document with two links and a two-branch outline, one open and one closed.
///
/// The same shape the deleted qpdf oracle used, for the same reason: `/Prev`,
/// `/Count`'s sign and a link's target are the structures a reader that walks
/// forward and normalises as it goes cannot check.
fn navigation() -> Vec<u8> {
    let mut builder = DocumentBuilder::new();
    for _ in 0..3 {
        builder.add_page(200.0, 300.0, |_| {});
    }
    builder.add_page(200.0, 300.0, |page| {
        assert!(page.link(
            10.0,
            20.0,
            90.0,
            40.0,
            &Target::Page {
                index: 2,
                view: DestKind::Fit,
            }
        ));
        assert!(page.link(
            10.0,
            50.0,
            90.0,
            70.0,
            &Target::Uri("https://example.org/".to_string())
        ));
    });
    assert!(builder.set_outline(vec![
        OutlineEntry {
            title: "Open".to_string(),
            target: None,
            open: true,
            children: vec![OutlineEntry {
                title: "Leaf".to_string(),
                target: Some(Target::Page {
                    index: 1,
                    view: DestKind::Fit,
                }),
                open: true,
                children: Vec::new(),
            }],
        },
        OutlineEntry {
            title: "Closed".to_string(),
            target: None,
            open: false,
            children: vec![OutlineEntry {
                title: "Hidden".to_string(),
                target: Some(Target::Page {
                    index: 2,
                    view: DestKind::Fit,
                }),
                open: true,
                children: Vec::new(),
            }],
        },
    ]));
    builder.finish()
}

// ---- the clean twins --------------------------------------------------------

/// Every layout the writer can produce, validated with nothing wrong.
///
/// This is the assertion the rest of the file rests on: an injection that is
/// refused proves nothing unless the same document without it is accepted.
#[test]
fn every_layout_this_writer_produces_validates_clean() {
    clean(document(3));
    clean(rewritten());
    clean(packed());
    clean(saved(&WriteOptions {
        mode: WriteMode::Rewrite,
        compress: true,
        object_streams: true,
        ..WriteOptions::default()
    }));
    clean(saved(&WriteOptions {
        mode: WriteMode::Rewrite,
        linearize: true,
        object_streams: false,
        ..WriteOptions::default()
    }));
}

/// An incremental update is two revisions chained by `/Prev`, and both of them
/// are held to the same rules.
#[test]
fn an_incremental_update_validates_clean() {
    let original = document(3);
    let doc = Arc::new(CosDocument::open(original).expect("it opens"));
    let updated = DocumentEditor::new(doc).save(&WriteOptions {
        mode: WriteMode::Incremental,
        ..WriteOptions::default()
    });
    clean(updated);
}

// ---- the file frame ---------------------------------------------------------

#[test]
fn a_file_that_does_not_begin_with_the_header_is_refused() {
    refused(patch(&rewritten(), b"%PDF-", b"%PDX-"), "header-missing");
}

#[test]
fn a_header_that_states_no_version_is_refused() {
    refused(
        patch(&rewritten(), b"%PDF-1", b"%PDF-x"),
        "header-version-unreadable",
    );
}

/// 7.5.2's binary comment is what stops transfer software treating the file as
/// text and rewriting its line endings, which moves every offset in it.
#[test]
fn a_file_with_no_binary_comment_is_refused() {
    refused(
        patch(&rewritten(), &[b'%', 0xE2, 0xE3, 0xCF, 0xD3], b"%pdfx"),
        "binary-comment-missing",
    );
}

#[test]
fn a_file_that_does_not_end_at_eof_is_refused() {
    refused(patch(&rewritten(), b"%%EOF", b"%%EOG"), "eof-missing");
}

#[test]
fn bytes_appended_after_eof_are_refused() {
    let mut bytes = rewritten();
    bytes.extend_from_slice(b"junk");
    refused(bytes, "bytes-after-eof");
}

#[test]
fn a_file_with_no_startxref_is_refused() {
    refused(
        patch(&rewritten(), b"startxref", b"startxrXf"),
        "startxref-missing",
    );
}

#[test]
fn a_startxref_that_names_no_section_is_refused() {
    let bytes = rewritten();
    let at = find(&bytes, b"startxref\n").expect("a startxref");
    let digits: Vec<u8> = bytes[at + 10..]
        .iter()
        .copied()
        .take_while(u8::is_ascii_digit)
        .collect();
    let moved = format!("{:0width$}", 7, width = digits.len());
    refused(
        patch(&bytes, &digits, moved.as_bytes()),
        "startxref-not-a-section",
    );
}

// ---- the entries ------------------------------------------------------------

/// **The reader cannot see this one.** An offset that points at the binary
/// comment six bytes before object 1 opens at Trust with no warning, because
/// the reader's header lookup lexes forward from wherever it lands.
#[test]
fn an_offset_that_points_just_before_its_object_is_refused() {
    let bytes = entry_offset(&rewritten(), 1, 9);
    let doc = CosDocument::open(bytes.clone()).expect("it opens");
    assert_eq!(
        doc.ladder_level(),
        LadderLevel::Trust,
        "the tolerant reader is untroubled by it"
    );
    assert!(doc.warnings().is_empty(), "and says nothing");

    refused(bytes, "object-header-absent");
}

/// **Nor this one.** The table says generation 1, the header says 0, and the
/// reader stores the header's — so the file this engine hands back is not the
/// file it was given, and nothing reports the difference.
#[test]
fn a_generation_the_table_invented_is_refused() {
    let bytes = rewritten();
    let at = first_entry(&bytes) + 20;
    let mut damaged = bytes.clone();
    damaged[at + 11..at + 16].copy_from_slice(b"00001");

    let doc = CosDocument::open(damaged.clone()).expect("it opens");
    assert_eq!(doc.ladder_level(), LadderLevel::Trust);
    assert!(doc.warnings().is_empty());

    refused(damaged, "object-header-mismatch");
}

#[test]
fn an_offset_that_points_at_no_object_at_all_is_refused() {
    let bytes = rewritten();
    let past = bytes.len() as u64 - 4;
    refused(entry_offset(&bytes, 2, past), "object-header-absent");
}

/// The ladder is itself a rule: a file that only opened because the tables
/// were thrown away is not one this engine may claim to have written.
#[test]
fn a_document_that_needed_the_ladder_is_refused() {
    let bytes = patch(&rewritten(), b"startxref", b"startxrXf");
    let doc = CosDocument::open(bytes.clone()).expect("it opens");
    assert_eq!(doc.ladder_level(), LadderLevel::Rescan);
    refused(bytes.clone(), "opened-lenient");
    refused(bytes, "repaired");
}

/// 7.3.10: `endobj` closes an object, and the reader repairs its absence.
#[test]
fn an_object_that_is_never_closed_is_refused() {
    refused(patch(&rewritten(), b"endobj", b"endobX"), "object-repaired");
}

// ---- the trailer ------------------------------------------------------------

#[test]
fn a_trailer_with_no_size_is_refused() {
    refused(
        patch(&rewritten(), b"/Size", b"/Siza"),
        "trailer-size-missing",
    );
}

#[test]
fn a_size_that_does_not_cover_every_object_is_refused() {
    let bytes = rewritten();
    let at = find(&bytes, b"/Size ").expect("a /Size") + 6;
    let smaller = format!("{:0width$}", 4, width = digits_at(&bytes, at).len());
    let damaged = patch_at(&bytes, at, smaller.as_bytes());
    refused(damaged.clone(), "size-too-small");
    refused(damaged, "entry-past-size");
}

#[test]
fn a_trailer_whose_root_is_not_a_catalog_is_refused() {
    // The catalog's own `/Type`, changed to something no reader recognises.
    refused(
        patch(&rewritten(), b"/Type /Catalog", b"/Type /Catalop"),
        "root-not-a-catalog",
    );
}

/// Encryption moves every length in the file and the hint tables measure the
/// ciphertext, so the sealed linearized layout is held to the same rules.
#[test]
fn an_encrypted_linearized_file_validates_clean() {
    let mut entropy = [0u8; 48];
    for (index, byte) in entropy.iter_mut().enumerate() {
        *byte = (index as u8).wrapping_mul(7).wrapping_add(11);
    }
    let doc = Arc::new(CosDocument::open(document(6)).expect("it opens"));
    let sealed = DocumentEditor::new(doc).save(&WriteOptions {
        mode: WriteMode::Rewrite,
        linearize: true,
        object_streams: false,
        encryption: Some(Encryption {
            user_password: "open-me".to_string(),
            owner_password: "owner-me".to_string(),
            permissions: -1,
            entropy,
        }),
        ..WriteOptions::default()
    });

    let doc = CosDocument::open(sealed).expect("it opens");
    assert!(
        doc.authenticate("open-me").is_ok(),
        "it really is encrypted"
    );
    let defects = tinker_pdf_cos::validate(&doc);
    assert!(
        defects.is_empty(),
        "found {:?}",
        defects.iter().map(Defect::to_string).collect::<Vec<_>>()
    );
}

/// 7.5.5 Table 15 requires `/ID` whenever `/Encrypt` is present. Until this
/// engine wrote one, an outside reader was the only thing that said so.
#[test]
fn an_encrypted_file_with_no_id_is_refused() {
    let sealed = encrypted();

    // The writer's own output, first: it carries the identifier now, and the
    // rule below would pass for the wrong reason if it did not.
    let doc = CosDocument::open(sealed.clone()).expect("it opens");
    assert!(
        doc.authenticate("open-me").is_ok(),
        "it really is encrypted"
    );
    assert!(
        tinker_pdf_cos::validate(&doc).is_empty(),
        "an encrypted file validates clean"
    );

    // And with the identifier taken back out of the trailer, by a name change
    // that leaves every offset where it was.
    let damaged = patch(&sealed, b"/ID [", b"/IE [");
    let doc = CosDocument::open(damaged).expect("it opens");
    assert!(doc.authenticate("open-me").is_ok());
    let defects = tinker_pdf_cos::validate(&doc);
    assert!(
        labels(&defects).contains(&"encrypt-without-id"),
        "found {:?}",
        defects.iter().map(Defect::to_string).collect::<Vec<_>>()
    );
}

/// 14.4: the first string is the document's permanent identifier and the
/// second changes with each revision.
#[test]
fn an_id_that_is_not_two_strings_is_refused() {
    let bytes = rewritten();
    // `/ID [<..> <..>]` with the second string's opening delimiter turned into
    // part of the first, which leaves one string where Table 15 wants two.
    let at = find(&bytes, b"/ID [").expect("an /ID") + 5;
    let end = bytes[at..]
        .iter()
        .position(|b| *b == b']')
        .expect("the array ends")
        + at;
    let mut damaged = bytes.clone();
    for byte in &mut damaged[at..end] {
        if *byte == b'>' || *byte == b'<' {
            *byte = b'0';
        }
    }
    refused(damaged, "id-malformed");
}

/// The identifier is a property of the document, so the half 14.4 calls
/// permanent survives being saved again and the half it calls the revision's
/// does not.
#[test]
fn a_rewrite_keeps_the_permanent_identifier_and_moves_the_other() {
    let first = rewritten();
    let doc = CosDocument::open(first.clone()).expect("it opens");
    let before = identifier(&doc);

    let mut editor = DocumentEditor::new(Arc::new(doc));
    assert!(editor.delete_page(2), "the document is not the one it was");
    let second = editor.save(&WriteOptions {
        mode: WriteMode::Rewrite,
        object_streams: false,
        ..WriteOptions::default()
    });
    let after = identifier(&CosDocument::open(second).expect("it opens"));

    assert_eq!(before.0, after.0, "the permanent half is the document's");
    assert_ne!(before.1, after.1, "the other half is the revision's");
    assert_eq!(before.0.len(), 16, "sixteen bytes, as everybody writes");
}

/// Two saves of the same document produce the same identifier, on every
/// target: it is a hash of the document rather than of the moment (ruling 4).
#[test]
fn the_identifier_is_a_function_of_the_document() {
    let once = identifier(&CosDocument::open(rewritten()).expect("it opens"));
    let again = identifier(&CosDocument::open(rewritten()).expect("it opens"));
    assert_eq!(once, again);

    // A different document, a different identifier — or the hash is not
    // reading the document at all.
    let doc = Arc::new(CosDocument::open(document(4)).expect("it opens"));
    let other = DocumentEditor::new(doc).save(&WriteOptions {
        mode: WriteMode::Rewrite,
        object_streams: false,
        ..WriteOptions::default()
    });
    let other = identifier(&CosDocument::open(other).expect("it opens"));
    assert_ne!(once.0, other.0);
}

/// An encrypted file's identifier is not a hash of its plaintext.
///
/// It would otherwise be a confirmation oracle: anybody holding a candidate
/// document could hash it and check the `/ID` in the clear, with no password.
/// The caller's entropy goes into the hash for exactly this reason, so the
/// same document sealed with different entropy is a different identifier.
#[test]
fn an_encrypted_identifier_is_not_the_plaintexts() {
    let plain = identifier(&CosDocument::open(rewritten()).expect("it opens"));
    let sealed = identifier(&CosDocument::open(encrypted()).expect("it opens"));
    assert_ne!(plain.0, sealed.0, "the entropy is in the hash");
}

// ---- streams ----------------------------------------------------------------

#[test]
fn a_stream_with_no_length_at_all_is_refused() {
    refused(
        patch(&rewritten(), b"/Length", b"/Lengtj"),
        "stream-length-unresolved",
    );
}

#[test]
fn a_length_that_stops_short_of_endstream_is_refused() {
    let bytes = rewritten();
    let at = find(&bytes, b"/Length ").expect("a /Length") + 8;
    let digits = digits_at(&bytes, at);
    assert!(
        digits.len() >= 2,
        "the fixture's first stream is long enough"
    );
    let shorter = format!("{:0width$}", 1, width = digits.len());
    refused(
        patch_at(&bytes, at, shorter.as_bytes()),
        "stream-length-not-exact",
    );
}

// ---- object streams ---------------------------------------------------------

/// 7.5.7: the prologue pairs an object number with where its value begins, and
/// the reader recovers from a pair that disagrees by searching the container
/// for the number it wanted.
#[test]
fn a_container_that_carries_the_wrong_object_number_is_refused() {
    let bytes = packed();
    let at = find(&bytes, b"/Type /ObjStm").expect("a container");
    let data = find(&bytes[at..], b"stream\n").expect("its data") + at + 7;
    // The first pair's object number, rewritten to one the container does not
    // hold. Same width, so nothing moves.
    let digits = digits_at(&bytes, data);
    let other = format!("{:0width$}", 9, width = digits.len());
    refused(
        patch_at(&bytes, data, other.as_bytes()),
        "objstm-number-mismatch",
    );
}

/// The container's `/N`, lowered so the last entries it really holds are past
/// the end it declares.
#[test]
fn a_container_that_understates_its_count_is_refused() {
    let bytes = packed();
    let at = find(&bytes, b"/Type /ObjStm").expect("a container");
    let n_at = find(&bytes[at..], b"/N ").expect("its /N") + at + 3;
    let fewer = format!("{:0width$}", 1, width = digits_at(&bytes, n_at).len());
    refused(
        patch_at(&bytes, n_at, fewer.as_bytes()),
        "objstm-index-out-of-range",
    );
}

// ---- cross-reference streams ------------------------------------------------

/// 7.5.8.3 defines three entry types. A fourth is a file no reader can walk,
/// and the entries after it are anybody's guess.
#[test]
fn an_entry_type_that_does_not_exist_is_refused() {
    let bytes = packed();
    let at = find(&bytes, b"/Type /XRef").expect("a cross-reference stream");
    let data = find(&bytes[at..], b"stream\n").expect("its data") + at + 7;
    // The first row's type field. `/W [1 4 2]`, so one byte, and the first row
    // is object zero's free entry.
    refused(patch_at(&bytes, data, &[9]), "xref-stream-type-unknown");
}

#[test]
fn a_cross_reference_stream_whose_widths_are_wrong_is_refused() {
    let bytes = packed();
    refused(
        patch(&bytes, b"/W [1 4 2]", b"/W [1 4 3]"),
        "xref-stream-rows-wrong",
    );
}

#[test]
fn a_cross_reference_stream_with_no_widths_is_refused() {
    let bytes = packed();
    refused(
        patch(&bytes, b"/W [1 4 2]", b"/Q [1 4 2]"),
        "xref-stream-widths-bad",
    );
}

// ---- tables this writer cannot produce --------------------------------------

/// Three objects and a header, with the table left to the caller.
///
/// Hand-built because every rule below is one the writer will not break, and a
/// rule with no fixture is a rule with no evidence.
fn hand_built(table: impl Fn(&[(u32, u64)]) -> String, trailer: &str) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(b"%PDF-1.7\n");
    out.extend_from_slice(&[b'%', 0xE2, 0xE3, 0xCF, 0xD3, b'\n']);

    let bodies: [&[u8]; 3] = [
        b"<< /Type /Catalog /Pages 2 0 R >>",
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 100] >>",
    ];
    let mut offsets = Vec::new();
    for (index, body) in bodies.iter().enumerate() {
        let num = index as u32 + 1;
        offsets.push((num, out.len() as u64));
        out.extend_from_slice(format!("{num} 0 obj\n").as_bytes());
        out.extend_from_slice(body);
        out.extend_from_slice(b"\nendobj\n");
    }

    let table_at = out.len();
    out.extend_from_slice(table(&offsets).as_bytes());
    out.extend_from_slice(trailer.as_bytes());
    out.extend_from_slice(format!("startxref\n{table_at}\n%%EOF\n").as_bytes());
    out
}

/// The table a conforming writer would produce for those three objects.
fn correct_table(offsets: &[(u32, u64)]) -> String {
    let mut table = String::from("xref\n0 4\n0000000000 65535 f \n");
    for (_, offset) in offsets {
        table.push_str(&format!("{offset:010} 00000 n \n"));
    }
    table
}

const TRAILER: &str = "trailer\n<< /Size 4 /Root 1 0 R >>\n";

/// The hand-built file is itself clean, or nothing below means anything.
#[test]
fn the_hand_built_fixture_validates_clean() {
    clean(hand_built(correct_table, TRAILER));
}

/// 7.5.4: entries are exactly twenty bytes. The reader resynchronises on the
/// grammar instead, so a nineteen-byte entry opens fine and is never reported.
#[test]
fn an_entry_of_the_wrong_length_is_refused() {
    let bytes = hand_built(
        |offsets| {
            let mut table = String::from("xref\n0 4\n0000000000 65535 f \n");
            for (index, (_, offset)) in offsets.iter().enumerate() {
                if index == 1 {
                    // Nineteen bytes: the trailing space is gone.
                    table.push_str(&format!("{offset:010} 00000 n\n"));
                } else {
                    table.push_str(&format!("{offset:010} 00000 n \n"));
                }
            }
            table
        },
        TRAILER,
    );
    refused(bytes, "entry-not-twenty-bytes");
}

#[test]
fn a_subsection_header_that_is_not_two_numbers_is_refused() {
    let bytes = hand_built(
        |offsets| {
            let mut table = String::from("xref\n0 four\n0000000000 65535 f \n");
            for (_, offset) in offsets {
                table.push_str(&format!("{offset:010} 00000 n \n"));
            }
            table
        },
        TRAILER,
    );
    refused(bytes, "subsection-malformed");
}

#[test]
fn a_table_that_is_not_followed_by_a_trailer_is_refused() {
    let bytes = hand_built(correct_table, "<< /Size 4 /Root 1 0 R >>\n");
    refused(bytes, "table-trailer-missing");
}

/// 7.5.4: object zero heads the free list at generation 65535. A table that
/// starts at object one has no free head at all, and nothing in an ordinary
/// read ever looks.
#[test]
fn a_table_with_no_free_head_is_refused() {
    let bytes = hand_built(
        |offsets| {
            let mut table = String::from("xref\n1 3\n");
            for (_, offset) in offsets {
                table.push_str(&format!("{offset:010} 00000 n \n"));
            }
            table
        },
        TRAILER,
    );
    refused(bytes, "free-head-missing");
}

#[test]
fn a_free_head_at_the_wrong_generation_is_refused() {
    let bytes = hand_built(
        |offsets| {
            let mut table = String::from("xref\n0 4\n0000000000 65530 f \n");
            for (_, offset) in offsets {
                table.push_str(&format!("{offset:010} 00000 n \n"));
            }
            table
        },
        TRAILER,
    );
    refused(bytes, "free-head-generation");
}

/// 7.5.4: the free list is a chain through free entries. One that points at a
/// live object is a file where deleting an object hands a reader a live one.
#[test]
fn a_free_list_that_points_at_a_live_object_is_refused() {
    let bytes = hand_built(
        |offsets| {
            let mut table = String::from("xref\n0 4\n0000000002 65535 f \n");
            for (_, offset) in offsets {
                table.push_str(&format!("{offset:010} 00000 n \n"));
            }
            table
        },
        TRAILER,
    );
    refused(bytes, "free-next-not-free");
}

#[test]
fn a_prev_that_names_no_section_is_refused() {
    let bytes = hand_built(
        correct_table,
        "trailer\n<< /Size 4 /Root 1 0 R /Prev 7 >>\n",
    );
    refused(bytes, "section-unreadable");
}

// ---- what the document says (7.7.3, 12.3.3, 12.5) ---------------------------

/// The navigation fixture is itself clean, or nothing below means anything.
#[test]
fn a_document_with_links_and_an_outline_validates_clean() {
    clean(navigation());
}

/// 7.7.3.2: the reader walks the page tree downwards and never asks a page
/// what is above it, so a tree with no `/Parent` anywhere paginates, renders
/// and round-trips.
#[test]
fn a_page_that_does_not_name_its_parent_is_refused() {
    let bytes = navigation();
    refused(
        patch_after(&bytes, b"/Type /Page", b"/Parent", b"/Parenu"),
        "page-parent-wrong",
    );
}

#[test]
fn a_count_that_is_not_the_leaves_below_it_is_refused() {
    let bytes = navigation();
    let at = find(&bytes, b"/Type /Pages").expect("a page tree node");
    let count = at + find(&bytes[at..], b"/Count ").expect("its /Count") + 7;
    let wider = format!("{:0width$}", 9, width = digits_at(&bytes, count).len());
    refused(
        patch_at(&bytes, count, wider.as_bytes()),
        "page-count-wrong",
    );
}

#[test]
fn a_page_tree_node_with_no_kids_is_refused() {
    refused(patch(&navigation(), b"/Kids", b"/Kidz"), "kids-malformed");
}

#[test]
fn a_page_with_no_media_box_anywhere_is_refused() {
    refused(
        patch(&navigation(), b"/MediaBox", b"/MediaBoy"),
        "media-box-absent",
    );
}

#[test]
fn a_media_box_that_encloses_nothing_is_refused() {
    refused(
        patch(&navigation(), b"[0 0 200 300]", b"[0 0 000 300]"),
        "media-box-degenerate",
    );
}

/// 12.5.2: the corners are stated lower-left then upper-right. This engine's
/// own reader normalises a reversed pair on the way out, which is exactly why
/// nothing else here notices one.
#[test]
fn a_link_rectangle_written_backwards_is_refused() {
    let bytes = navigation();
    refused(
        patch_after(&bytes, b"/Link", b"[10 20 90 40]", b"[90 20 10 40]"),
        "annot-rect-unordered",
    );
}

#[test]
fn an_annotation_with_no_subtype_is_refused() {
    let bytes = navigation();
    refused(
        patch_after(&bytes, b"/Annots", b"/Subtype", b"/Subtypf"),
        "annot-subtype-missing",
    );
}

#[test]
fn a_link_that_names_nothing_to_go_to_is_refused() {
    let bytes = navigation();
    refused(
        patch_after(&bytes, b"/Link", b"/Dest", b"/Desu"),
        "link-without-target",
    );
}

/// **The fault only the outside reader ever caught.** 12.3.3's siblings link
/// both ways; this reader walks `/Next` forward, which is enough to build the
/// tree, so deleting every `/Prev` survived every round trip in this
/// repository. A viewer walking up from a selected entry is what notices.
#[test]
fn an_outline_that_does_not_link_backwards_is_refused() {
    let bytes = navigation();
    let doc = CosDocument::open(bytes.clone()).expect("it opens");
    let before = tinker_pdf_cos::outline(&doc).len();

    let damaged = patch_after(&bytes, b"/Title", b"/Prev", b"/Preu");
    let doc = CosDocument::open(damaged.clone()).expect("it opens");
    assert_eq!(
        tinker_pdf_cos::outline(&doc).len(),
        before,
        "the reader builds the same outline either way"
    );
    assert!(doc.warnings().is_empty(), "and says nothing");

    refused(damaged, "outline-prev-wrong");
}

/// 12.3.3 Table 152: an open item states how many items it exposes and a
/// closed one states the negative of that. The reader takes only the sign.
#[test]
fn an_outline_count_of_the_wrong_size_is_refused() {
    let bytes = navigation();
    let at = find(&bytes, b"/Type /Outlines").expect("an outline root");
    let count = at + find(&bytes[at..], b"/Count ").expect("its /Count") + 7;
    let wrong = format!("{:0width$}", 9, width = digits_at(&bytes, count).len());
    refused(
        patch_at(&bytes, count, wrong.as_bytes()),
        "outline-count-wrong",
    );
}

#[test]
fn an_outline_item_with_no_title_is_refused() {
    refused(
        patch(&navigation(), b"/Title", b"/Titlf"),
        "outline-title-missing",
    );
}

/// The chain's ends are stated as well as walked, and a `/Next` that stops
/// early leaves `/Last` naming an item the chain never reaches.
#[test]
fn an_outline_whose_ends_are_not_its_chain_is_refused() {
    let bytes = navigation();
    refused(
        patch_after(&bytes, b"/Title", b"/Next", b"/Nexu"),
        "outline-ends-wrong",
    );
}

/// A `/Next` that names something which is not an outline item at all.
#[test]
fn an_outline_next_that_names_nothing_is_refused() {
    let bytes = navigation();
    let at = find(&bytes, b"/Next ").expect("a forward link") + 6;
    let absent = format!("{:0width$}", 0, width = digits_at(&bytes, at).len());
    refused(
        patch_at(&bytes, at, absent.as_bytes()),
        "outline-next-wrong",
    );
}

// ---- the writer's whole surface ---------------------------------------------

/// The document that uses every construct this writer can emit.
///
/// Its dictionaries are the ones the deleted oracle read back through
/// `--show-object`, and they were read *there* because this engine's own
/// reader supplies a default for most of them: `/Extend`, `/Domain`, `/XStep`
/// and `/Encode` all have a reader-side fallback, so a shading written with the
/// wrong key names round-trips through this crate and is an empty dictionary to
/// anybody else.
#[test]
fn the_writers_whole_surface_validates_clean() {
    clean(whole_surface_document());
}

#[test]
fn an_alpha_outside_its_range_is_refused() {
    refused(
        patch(&whole_surface_document(), b"/ca 0.5", b"/ca 1.5"),
        "ext-gstate-malformed",
    );
}

#[test]
fn a_blend_mode_that_is_not_one_of_the_sixteen_is_refused() {
    refused(
        patch(
            &whole_surface_document(),
            b"/BM /Multiply",
            b"/BM /Multiplx",
        ),
        "ext-gstate-malformed",
    );
}

#[test]
fn a_soft_mask_of_no_known_kind_is_refused() {
    refused(
        patch(
            &whole_surface_document(),
            b"/S /Luminosity",
            b"/S /Luminositx",
        ),
        "ext-gstate-malformed",
    );
}

/// 11.6.6: a group that does not say it is a transparency group is one a
/// reader composites as ordinary content.
#[test]
fn a_group_that_is_not_a_transparency_group_is_refused() {
    refused(
        patch(
            &whole_surface_document(),
            b"/S /Transparency",
            b"/S /Transparencx",
        ),
        "group-malformed",
    );
}

/// 8.7.4.5.3: an axial shading is four numbers and a radial one is six, and
/// the arity is the whole difference between them.
#[test]
fn a_shading_with_the_wrong_number_of_coordinates_is_refused() {
    refused(
        patch(
            &whole_surface_document(),
            b"/Coords [0 0 300 0]",
            b"/Coords [0 0 300  ]",
        ),
        "shading-malformed",
    );
}

/// 7.10.4: k sub-functions want k-1 bounds. The reader defaults a missing
/// `/Bounds` and draws a gradient either way.
#[test]
fn a_stitching_function_with_the_wrong_bounds_is_refused() {
    refused(
        patch(
            &whole_surface_document(),
            b"/Bounds [0.35]",
            b"/Bounds [    ]",
        ),
        "function-malformed",
    );
}

/// 7.10.3: `/C0` and `/C1` are one colour each, in the same space.
#[test]
fn an_exponential_function_whose_ends_disagree_is_refused() {
    refused(
        patch(&whole_surface_document(), b"/C0 [1 0 0]", b"/C0 [1 0  ]"),
        "function-malformed",
    );
}

/// 8.7.3.1: a zero step paints one cell forever. The reader falls back to the
/// cell's own size, so it draws a pattern either way.
#[test]
fn a_tiling_pattern_that_never_repeats_is_refused() {
    refused(
        patch(&whole_surface_document(), b"/XStep 12", b"/XStep 00"),
        "pattern-malformed",
    );
}

/// 9.7.1: a Type0 font has exactly one descendant, and the metrics live in it.
#[test]
fn a_composite_font_with_no_descendant_is_refused() {
    refused(
        patch(
            &whole_surface_document(),
            b"/DescendantFonts",
            b"/DescendantFonty",
        ),
        "font-malformed",
    );
}

/// 9.7.4.3: `/W` is `c [w1 w2 ...]` or `first last w`, and nothing else.
#[test]
fn a_width_array_of_the_wrong_shape_is_refused() {
    refused(
        patch(&whole_surface_document(), b"/W [1 [700", b"/W [1 (700"),
        "font-malformed",
    );
}

/// 9.10.3: a `/ToUnicode` that is not a CMap maps nothing, and extraction
/// hands back the codes instead of the characters without saying so.
#[test]
fn a_to_unicode_that_is_not_a_cmap_is_refused() {
    refused(
        patch(&whole_surface_document(), b"begincmap", b"beginXmap"),
        "font-malformed",
    );
}

/// 8.10.1: a form states the box its content is clipped to.
#[test]
fn a_form_with_no_bounding_box_is_refused() {
    refused(
        patch(
            &whole_surface_document(),
            b"/BBox [0 0 150 200]",
            b"/BBox [0 0 150    ]",
        ),
        "xobject-malformed",
    );
}

/// 7.8.3: a name a content stream will use, resolving to nothing. Object zero
/// is always free, so naming it is naming the null object (7.3.10).
#[test]
fn a_resource_name_that_resolves_to_nothing_is_refused() {
    let bytes = whole_surface_document();
    let at = find(&bytes, b"/Font <</C0 ").expect("the font resource") + 12;
    let digits = digits_at(&bytes, at);
    let zero = format!("{:0width$}", 0, width = digits.len());
    refused(patch_at(&bytes, at, zero.as_bytes()), "resource-unresolved");
}

// ---- Annex F, whose tables nothing had ever read ----------------------------

/// The six-page linearized layout.
fn linearized() -> Vec<u8> {
    saved(&WriteOptions {
        mode: WriteMode::Rewrite,
        linearize: true,
        object_streams: false,
        ..WriteOptions::default()
    })
}

/// A document with a real part 8: page one uses one font, the rest use another
/// it never touches, so that font is shared between pages two onward and
/// belongs nowhere else (F.3.8).
fn shared_resource() -> Vec<u8> {
    let mut builder = DocumentBuilder::new();
    builder.add_base_font(b"F0", b"Helvetica");
    builder.add_page(200.0, 100.0, |page| {
        page.text(b"F0", 12.0, 10.0, 50.0, "page 0");
    });
    builder.add_base_font(b"F1", b"Courier");
    for index in 1..6 {
        builder.add_page(200.0, 100.0, |page| {
            page.text(b"F1", 12.0, 10.0, 50.0, &format!("page {index}"));
        });
    }
    let doc = Arc::new(CosDocument::open(builder.finish()).expect("it opens"));
    DocumentEditor::new(doc).save(&WriteOptions {
        mode: WriteMode::Rewrite,
        linearize: true,
        object_streams: false,
        ..WriteOptions::default()
    })
}

/// Rewrites one of the parameter dictionary's fixed-width integers.
///
/// F.2.1 writes every one of them to the same width so the dictionary's own
/// length is known before the layout is: an injection can therefore change a
/// value without moving a byte, which is exactly what these rules need.
fn parameter(bytes: &[u8], key: &[u8], value: u64) -> Vec<u8> {
    let at = find(bytes, key).expect("the parameter dictionary") + key.len();
    let width = digits_at(bytes, at).len();
    assert!(width > 0, "the value is written as digits");
    patch_at(bytes, at, format!("{value:0width$}").as_bytes())
}

/// Where the primary hint stream's data begins.
fn hint_data(bytes: &[u8]) -> usize {
    let object = find(bytes, b"2 0 obj").expect("the hint stream is object two");
    object + find(&bytes[object..], b"stream\n").expect("its data") + 7
}

/// Rewrites one thirty-two bit field of a hint table.
fn hint_field(bytes: &[u8], at: usize, value: u32) -> Vec<u8> {
    patch_at(bytes, hint_data(bytes) + at, &value.to_be_bytes())
}

#[test]
fn both_linearized_layouts_validate_clean() {
    clean(linearized());
    clean(shared_resource());
}

#[test]
fn a_declared_file_length_that_is_not_the_files_is_refused() {
    refused(
        parameter(&linearized(), b"/L ", 99),
        "linearized-parameter-wrong",
    );
}

#[test]
fn a_first_page_object_that_is_not_the_first_pages_is_refused() {
    refused(
        parameter(&linearized(), b"/O ", 9),
        "linearized-parameter-wrong",
    );
}

#[test]
fn a_page_count_the_tree_does_not_agree_with_is_refused() {
    refused(
        parameter(&linearized(), b"/N ", 5),
        "linearized-parameter-wrong",
    );
}

/// F.2.2 item 6: `/T` names the first *entry* of the main table, not the
/// `xref` keyword above it — a distinction worth a rule, because a reader
/// seeking there lands one line early and reads the subsection header as an
/// entry.
#[test]
fn a_main_table_offset_that_names_the_wrong_byte_is_refused() {
    refused(
        parameter(&linearized(), b"/T ", 7),
        "linearized-parameter-wrong",
    );
}

/// F.2.2 item 5: `/E` is the end of the first page's section, so a value
/// before the first page's own objects end is one that cuts them off.
#[test]
fn a_first_page_end_before_the_first_page_is_refused() {
    refused(
        parameter(&linearized(), b"/E ", 1),
        "linearized-parameter-wrong",
    );
}

#[test]
fn a_hint_stream_that_is_not_where_h_says_is_refused() {
    refused(
        parameter(&linearized(), b"/H [ ", 9),
        "linearized-parameter-wrong",
    );
}

/// Table F.3 item 2 held an object *number* until an outside reader said
/// otherwise, and nothing in this repository could tell: the number and the
/// offset are both small integers in a small file.
#[test]
fn a_first_page_offset_that_is_not_the_first_pages_is_refused() {
    let bytes = linearized();
    refused(hint_field(&bytes, 4, 12), "hint-value-wrong");
}

/// Table F.3 item 1: every page's object count is stated as a delta from this,
/// so moving it moves all of them at once.
#[test]
fn page_object_counts_that_are_not_the_runs_are_refused() {
    let bytes = linearized();
    refused(hint_field(&bytes, 0, 7), "hint-value-wrong");
}

/// Item 4, the least page length, for the same reason.
#[test]
fn page_lengths_that_are_not_the_bytes_are_refused() {
    let bytes = linearized();
    refused(hint_field(&bytes, 10, 4096), "hint-value-wrong");
}

/// Table F.6 item 1: a shared entry's group length is the span of the object
/// it describes.
#[test]
fn a_shared_group_length_that_is_not_the_objects_is_refused() {
    let bytes = shared_resource();
    let shared_at = {
        let at = find(&bytes, b"/S ").expect("the hint stream states where") + 3;
        digits_at(&bytes, at)
            .iter()
            .fold(0usize, |acc, b| acc * 10 + usize::from(b - b'0'))
    };
    // Table F.5 item 6: the least group length, which every entry is a delta
    // from. Four thirty-two bit items, then a sixteen bit one, so it begins at
    // byte eighteen of the table -- and patching byte twenty instead lands
    // half in it and half in the width that follows, which decodes as a
    // hundred-bit field and fails the whole read rather than one rule.
    refused(hint_field(&bytes, shared_at + 18, 4096), "hint-value-wrong");
}

/// F.4.2: part 8's first object and its offset are both zero when there is no
/// part 8, and a file that claims one has a shared table nobody can walk.
#[test]
fn a_part_eight_that_is_not_there_is_refused() {
    let bytes = linearized();
    let shared_at = {
        let at = find(&bytes, b"/S ").expect("the hint stream states where") + 3;
        digits_at(&bytes, at)
            .iter()
            .fold(0usize, |acc, b| acc * 10 + usize::from(b - b'0'))
    };
    refused(hint_field(&bytes, shared_at, 6), "hint-value-wrong");
}

/// The shared table is addressed by the stream's own `/S`, so a reader that
/// trusted its own arithmetic instead would never notice the two disagreeing.
#[test]
fn a_hint_stream_that_cannot_be_walked_is_refused() {
    let bytes = linearized();
    let at = find(&bytes, b"/S ").expect("the hint stream states where") + 3;
    let width = digits_at(&bytes, at).len();
    let past = format!("{:0width$}", 999, width = width);
    refused(
        patch_at(&bytes, at, past.as_bytes()),
        "hint-stream-unreadable",
    );
}

/// And an ordinary file is held to none of it, which is what says these rules
/// are about linearization rather than about every file that happens to pass.
#[test]
fn an_ordinary_layout_is_not_held_to_annex_f() {
    let bytes = rewritten();
    assert!(
        find(&bytes, b"/Linearized").is_none(),
        "the ordinary layout claims nothing"
    );
    clean(bytes);
}

/// A document whose lowest object number is not one still gets a free head.
///
/// The writer merges object zero into a subsection that starts at one, so a
/// set numbered from two produced a table with no free head at all — which is
/// a table several readers refuse outright, and which this engine's own reader
/// never looks at. Seventy-two rewrites of corpus files had one before the
/// strict pass over the corpus found it.
#[test]
fn a_table_whose_objects_start_above_one_still_heads_its_free_list() {
    use tinker_pdf_cos::write::{rewrite, ObjectSet};
    use tinker_pdf_cos::{Name, NameTable, Object};

    let names = NameTable::new();
    let mut objects = ObjectSet::new();
    let mut catalog = tinker_pdf_cos::Dict::new();
    catalog.insert(Name::TYPE, Object::Name(names.intern(b"Catalog")));
    catalog.insert(Name::PAGES, Object::Ref(tinker_pdf_cos::ObjRef::new(3, 0)));
    objects.insert(2, Object::Dict(catalog));

    let mut pages = tinker_pdf_cos::Dict::new();
    pages.insert(Name::TYPE, Object::Name(Name::PAGES));
    pages.insert(Name::COUNT, Object::Int(1));
    pages.insert(
        Name::KIDS,
        Object::Array(vec![Object::Ref(tinker_pdf_cos::ObjRef::new(4, 0))]),
    );
    objects.insert(3, Object::Dict(pages));

    let mut page = tinker_pdf_cos::Dict::new();
    page.insert(Name::TYPE, Object::Name(names.intern(b"Page")));
    page.insert(Name::PARENT, Object::Ref(tinker_pdf_cos::ObjRef::new(3, 0)));
    page.insert(
        Name::MEDIA_BOX,
        Object::Array(vec![
            Object::Int(0),
            Object::Int(0),
            Object::Int(200),
            Object::Int(100),
        ]),
    );
    objects.insert(4, Object::Dict(page));

    let mut trailer = tinker_pdf_cos::Dict::new();
    trailer.insert(Name::ROOT, Object::Ref(tinker_pdf_cos::ObjRef::new(2, 0)));

    let bytes = rewrite(&objects, &trailer, &WriteOptions::default(), &names);
    clean(bytes);
}

/// A rewrite of a linearized file does not claim to be linearized.
///
/// F.2.2's parameter dictionary describes *that* file's layout: where the
/// hint stream is, where the first page's section ends, where the main table
/// starts. An ordinary rewrite has none of those, and carrying the dictionary
/// through — which every rewrite of a linearized source did — makes the new
/// file claim a fast-web-view layout it does not have. The pass over the
/// corpus is what found it.
#[test]
fn a_rewrite_of_a_linearized_file_makes_no_claim_about_annex_f() {
    let linearized = linearized();
    assert!(
        find(&linearized, b"/Linearized").is_some(),
        "the source claims it"
    );

    let doc = Arc::new(CosDocument::open(linearized).expect("it opens"));
    let plain = DocumentEditor::new(doc).save(&WriteOptions {
        mode: WriteMode::Rewrite,
        object_streams: false,
        ..WriteOptions::default()
    });
    assert!(
        find(&plain, b"/Linearized").is_none(),
        "and the rewrite does not"
    );
    clean(plain);
}
