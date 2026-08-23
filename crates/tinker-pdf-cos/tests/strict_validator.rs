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

use std::sync::Arc;

use tinker_pdf_cos::{
    CosDocument, Defect, DocumentBuilder, DocumentEditor, Encryption, LadderLevel, WriteMode,
    WriteOptions,
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
