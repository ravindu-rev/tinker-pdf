//! Linearized output (Annex F, phase 09).
//!
//! MuPDF 1.26 removed linearized writing, and Tinker's own plans had to shell
//! out to qpdf for it. This is the superiority item.
//!
//! The point of a linearized file is that a reader holding only its first few
//! kilobytes can draw page one. That is a claim about *byte offsets*, so
//! every test here checks offsets against the bytes rather than checking that
//! the file merely opens — a file can open perfectly and still have its first
//! page scattered through the middle, which is the failure this feature
//! exists to prevent and the one a round-trip test cannot see.
//!
//! `qpdf --check` and `--show-linearization` are the external arbiters named
//! in the plan and are not run here; what is checked here is everything the
//! engine can verify about its own output without them.

use std::sync::Arc;

use tinker_pdf_cos::{
    pages, CosDocument, DocumentBuilder, DocumentEditor, WriteMode, WriteOptions,
};

fn document(page_count: usize) -> Arc<CosDocument> {
    let mut builder = DocumentBuilder::new();
    builder.add_base_font(b"F0", b"Helvetica");
    builder.set_info(b"Title", "linearized");
    for index in 0..page_count {
        builder.add_page(200.0, 100.0, |page| {
            page.text(b"F0", 12.0, 10.0, 50.0, &format!("page {index}"));
        });
    }
    Arc::new(CosDocument::open(builder.finish()).expect("it opens"))
}

fn linearized(page_count: usize) -> Vec<u8> {
    let editor = DocumentEditor::new(document(page_count));
    editor.save(&WriteOptions {
        mode: WriteMode::Rewrite,
        linearize: true,
        // Object streams pack objects into a container, which would put the
        // first page's objects inside the same blob as everything else and
        // defeat the layout entirely.
        object_streams: false,
        ..WriteOptions::default()
    })
}

/// Reads one integer out of the linearization parameter dictionary.
fn parameter(bytes: &[u8], key: &str) -> u64 {
    let text = String::from_utf8_lossy(&bytes[..bytes.len().min(512)]).into_owned();
    let at = text
        .find(&format!("/{key} "))
        .unwrap_or_else(|| panic!("no /{key} in the parameter dictionary: {text}"));
    let rest = &text[at + key.len() + 2..];
    let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
    digits
        .parse()
        .unwrap_or_else(|_| panic!("/{key} is not a number: {rest}"))
}

/// The first `/H` element: where the hint stream begins.
fn hint_offset(bytes: &[u8]) -> u64 {
    let text = String::from_utf8_lossy(&bytes[..bytes.len().min(512)]).into_owned();
    let at = text.find("/H [ ").expect("an /H array");
    let rest = &text[at + 5..];
    let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
    digits.parse().expect("a number")
}

#[test]
fn a_linearized_file_still_opens_and_reads() {
    let bytes = linearized(4);
    let doc = CosDocument::open(bytes).expect("it opens");
    let collected = pages::collect(&doc);
    assert_eq!(collected.len(), 4, "every page is still there");

    let content = pages::content_bytes(&doc, &collected[0]);
    assert!(
        String::from_utf8_lossy(&content).contains("page 0"),
        "and page one still draws what it drew"
    );

    let metadata = tinker_pdf_cos::metadata(&doc);
    assert_eq!(metadata.title.as_deref(), Some("linearized"));
}

/// F.2.2: the parameter dictionary is the first object in the file, before
/// anything else a reader would have to skip.
#[test]
fn the_parameter_dictionary_comes_first() {
    let bytes = linearized(3);
    let head = String::from_utf8_lossy(&bytes[..bytes.len().min(200)]).into_owned();

    assert!(head.starts_with("%PDF-"), "the header: {head}");
    assert!(
        head.contains("1 0 obj\n<< /Linearized 1"),
        "then the parameter dictionary, immediately: {head}"
    );
}

/// The headline claim: page one's objects are at the front. Without this the
/// file is an ordinary PDF wearing a `/Linearized` key, which is worse than
/// an ordinary PDF because a reader will trust it.
#[test]
fn the_first_pages_objects_precede_everything_else() {
    let bytes = linearized(6);
    let end_of_first_page = parameter(&bytes, "E");
    let first_page_object = parameter(&bytes, "O");

    let doc = CosDocument::open(bytes.clone()).expect("it opens");
    let collected = pages::collect(&doc);
    assert_eq!(collected.len(), 6);

    // The object `/O` names really is the first page.
    let first = collected[0].reference;
    assert_eq!(
        u64::from(first.num),
        first_page_object,
        "/O names the first page's object"
    );

    // Everything page one needs is inside the first `/E` bytes, and at least
    // one later page's content is not. The needle has to be the text that
    // distinguishes the pages: they share their opening operators, so a
    // prefix would match page one wherever it was really looking.
    let found = find(&bytes, b"(page 0)").expect("page one's content is in the file");
    assert!(
        (found as u64) < end_of_first_page,
        "page one's content is inside the first {end_of_first_page} bytes, at {found}"
    );

    let found = find(&bytes, b"(page 5)").expect("the last page's content is in the file");
    assert!(
        (found as u64) > end_of_first_page,
        "and the last page's content is after it, at {found}"
    );
}

/// `/L` is the file's own length. A reader uses it to tell a complete file
/// from a truncated one before parsing anything.
#[test]
fn the_declared_length_is_the_real_length() {
    let bytes = linearized(3);
    assert_eq!(parameter(&bytes, "L"), bytes.len() as u64);
}

/// `/H` points at the hint stream. Pointing anywhere else sends a reader
/// looking for a stream in the middle of another object.
#[test]
fn the_hint_stream_is_where_the_dictionary_says() {
    let bytes = linearized(4);
    let at = hint_offset(&bytes) as usize;

    let there = String::from_utf8_lossy(&bytes[at..bytes.len().min(at + 40)]).into_owned();
    assert!(
        there.starts_with("2 0 obj"),
        "an indirect object begins at /H: {there}"
    );
    assert!(there.contains("stream"), "and it is a stream: {there}");
}

/// `/T` points at the main cross-reference table, and `/E` at the end of the
/// first page's material — which must come before it.
#[test]
fn the_declared_offsets_are_ordered_the_way_the_layout_is() {
    let bytes = linearized(5);
    let end_of_first_page = parameter(&bytes, "E");
    let main_xref = parameter(&bytes, "T");
    let hint = hint_offset(&bytes);

    assert!(hint < end_of_first_page, "the hint stream is inside part 6");
    assert!(
        end_of_first_page <= main_xref,
        "the first page ends before the main table begins: {end_of_first_page} vs {main_xref}"
    );
    assert!(main_xref < bytes.len() as u64);

    let there = String::from_utf8_lossy(
        &bytes[main_xref as usize..bytes.len().min(main_xref as usize + 8)],
    )
    .into_owned();
    assert!(there.starts_with("xref"), "/T points at a table: {there}");
}

/// F.3.4: the last line points back at the *first-page* table near the front.
/// That is the whole trick — a reader with the file's tail resolves page one
/// without ever seeing the middle.
#[test]
fn the_final_startxref_points_at_the_front_of_the_file() {
    let bytes = linearized(8);
    let text = String::from_utf8_lossy(&bytes).into_owned();
    let at = text.rfind("startxref\n").expect("a startxref");
    let digits: String = text[at + 10..]
        .chars()
        .take_while(char::is_ascii_digit)
        .collect();
    let offset: u64 = digits.parse().expect("a number");

    assert!(
        offset < parameter(&bytes, "E"),
        "it points into the front of the file, not the back: {offset}"
    );

    let there =
        String::from_utf8_lossy(&bytes[offset as usize..bytes.len().min(offset as usize + 8)])
            .into_owned();
    assert!(
        there.starts_with("xref"),
        "and at a cross-reference table: {there}"
    );
}

/// `/N` is the page count, which a reader shows before it has the pages.
#[test]
fn the_page_count_is_declared() {
    for count in [1usize, 2, 7] {
        let bytes = linearized(count);
        assert_eq!(parameter(&bytes, "N"), count as u64, "for {count} pages");
    }
}

/// Ruling 4: the same document written twice is the same bytes.
#[test]
fn linearized_output_is_deterministic() {
    assert_eq!(linearized(4), linearized(4));
}

/// A document with nothing to linearize around gets an ordinary file rather
/// than one claiming `/Linearized` and not being (ruling 2).
#[test]
fn a_document_with_no_pages_is_written_plainly() {
    let bytes = b"%PDF-1.7\n\
1 0 obj\n<< /Type /Catalog >>\nendobj\n\
trailer\n<< /Size 2 /Root 1 0 R >>\n%%EOF\n";
    let doc = Arc::new(CosDocument::open(bytes.to_vec()).expect("it opens"));

    let editor = DocumentEditor::new(doc);
    let written = editor.save(&WriteOptions {
        mode: WriteMode::Rewrite,
        linearize: true,
        ..WriteOptions::default()
    });

    assert!(
        !String::from_utf8_lossy(&written).contains("/Linearized"),
        "no claim is made that cannot be kept"
    );
    assert!(CosDocument::open(written).is_ok(), "and it still opens");
}

/// Turning it off gives the ordinary layout, which is the default.
#[test]
fn linearization_is_off_by_default() {
    let editor = DocumentEditor::new(document(3));
    let plain = editor.save(&WriteOptions {
        mode: WriteMode::Rewrite,
        ..WriteOptions::default()
    });
    assert!(!String::from_utf8_lossy(&plain).contains("/Linearized"));
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || needle.len() > haystack.len() {
        return None;
    }
    (0..=haystack.len() - needle.len()).find(|&at| &haystack[at..at + needle.len()] == needle)
}

/// `compress` and `linearize` together.
///
/// `compress` had no effect at all on the linearized path: the ordinary
/// writer compresses inside `write_entry`, which this layout does not use, so
/// asking for both produced an uncompressed file and no error.
#[test]
fn compression_applies_to_a_linearized_file() {
    // Content long enough that deflate wins. A twenty-byte content stream
    // compresses to more than twenty bytes, and `maybe_compress` is right to
    // decline it, so a small fixture would prove nothing either way.
    let build = || {
        let mut builder = DocumentBuilder::new();
        builder.add_base_font(b"F0", b"Helvetica");
        for index in 0..6 {
            builder.add_page(400.0, 400.0, |page| {
                for line in 0..30 {
                    page.text(
                        b"F0",
                        10.0,
                        10.0,
                        f64::from(line) * 12.0,
                        &format!("page {index} line {line} of repeated filler text"),
                    );
                }
            });
        }
        Arc::new(CosDocument::open(builder.finish()).expect("it opens"))
    };

    let save = |compress: bool| -> Vec<u8> {
        let editor = DocumentEditor::new(build());
        editor.save(&WriteOptions {
            mode: WriteMode::Rewrite,
            linearize: true,
            object_streams: false,
            compress,
            ..WriteOptions::default()
        })
    };

    let plain = save(false);
    let squeezed = save(true);

    assert!(
        squeezed.len() < plain.len(),
        "compression shrinks it: {} against {}",
        squeezed.len(),
        plain.len()
    );
    assert!(
        String::from_utf8_lossy(&squeezed).contains("/FlateDecode"),
        "and says so in the stream dictionaries"
    );

    // And it is still a linearized file that opens.
    let doc = CosDocument::open(squeezed.clone()).expect("it opens");
    assert_eq!(pages::collect(&doc).len(), 6);
    assert_eq!(
        parameter(&squeezed, "L"),
        squeezed.len() as u64,
        "with /L still describing the file it is in"
    );
}
