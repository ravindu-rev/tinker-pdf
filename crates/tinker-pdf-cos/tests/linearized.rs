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
    pages, AuthLevel, CosDocument, DocumentBuilder, DocumentEditor, Encryption, ObjRef, Object,
    WriteMode, WriteOptions,
};

/// The password every encrypted fixture here opens with.
const PASSWORD: &str = "open-me";

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
    save(page_count, None)
}

/// Forty-eight deterministic bytes. Real callers pass real randomness; a test
/// wants the same document every run, and the key derivation cannot tell.
///
/// This is also what makes the reproducibility assertion meaningful: the
/// padding AES adds is a function of the plaintext's length, so the layout is
/// deterministic exactly when the entropy is.
fn entropy() -> [u8; 48] {
    let mut bytes = [0u8; 48];
    for (index, byte) in bytes.iter_mut().enumerate() {
        *byte = (index as u8).wrapping_mul(7).wrapping_add(11);
    }
    bytes
}

fn encrypted_linearized(page_count: usize) -> Vec<u8> {
    save(
        page_count,
        Some(Encryption {
            user_password: PASSWORD.to_string(),
            owner_password: "owner-me".to_string(),
            permissions: -1,
            entropy: entropy(),
        }),
    )
}

fn save(page_count: usize, encryption: Option<Encryption>) -> Vec<u8> {
    let editor = DocumentEditor::new(document(page_count));
    editor.save(&WriteOptions {
        mode: WriteMode::Rewrite,
        linearize: true,
        // Object streams pack objects into a container, which would put the
        // first page's objects inside the same blob as everything else and
        // defeat the layout entirely.
        object_streams: false,
        encryption,
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
    hint_element(bytes, 0)
}

/// The second `/H` element: how long the hint stream object is, in the file.
fn hint_length(bytes: &[u8]) -> u64 {
    hint_element(bytes, 1)
}

fn hint_element(bytes: &[u8], index: usize) -> u64 {
    let text = String::from_utf8_lossy(&bytes[..bytes.len().min(512)]).into_owned();
    let at = text.find("/H [ ").expect("an /H array");
    let rest = &text[at + 5..];
    let digits: String = rest
        .split_whitespace()
        .nth(index)
        .expect("both /H elements")
        .chars()
        .take_while(char::is_ascii_digit)
        .collect();
    digits.parse().expect("a number")
}

/// The offset the file's last line points at, which is the first-page
/// cross-reference table (F.3.4).
fn final_startxref(bytes: &[u8]) -> usize {
    let tail_at = bytes.len().saturating_sub(64);
    let tail = String::from_utf8_lossy(&bytes[tail_at..]).into_owned();
    let at = tail.rfind("startxref\n").expect("a startxref");
    tail[at + 10..]
        .chars()
        .take_while(char::is_ascii_digit)
        .collect::<String>()
        .parse()
        .expect("a number")
}

/// Parses a classic cross-reference table straight out of the raw bytes.
///
/// Byte-level rather than through `from_utf8_lossy`, because a lossy decode
/// of a file with ciphertext in it changes the byte count and every offset
/// after that would be measured against the wrong string. It also means the
/// parse is a genuine claim about the table being in the clear: 7.6.1 exempts
/// both tables from encryption, and this function cannot succeed on a table
/// that is not.
///
/// Returns the in-use entries only, as (object number, offset).
fn classic_xref(bytes: &[u8], at: usize) -> Vec<(u32, u64)> {
    let head = bytes.get(at..).unwrap_or_default();
    assert!(
        head.starts_with(b"xref\n"),
        "a cross-reference table begins at {at}"
    );
    let rest = &head[5..];
    let eol = rest
        .iter()
        .position(|b| *b == b'\n')
        .expect("a subsection header line");
    let header = std::str::from_utf8(&rest[..eol]).expect("the header is ASCII");
    let mut parts = header.split(' ');
    let start: u32 = parts
        .next()
        .and_then(|t| t.parse().ok())
        .expect("a first object number");
    let count: usize = parts
        .next()
        .and_then(|t| t.parse().ok())
        .expect("an entry count");

    let entries = &rest[eol + 1..];
    let mut out = Vec::with_capacity(count);
    for index in 0..count {
        // 7.5.4: exactly twenty bytes, and the field positions are fixed.
        let entry = entries
            .get(index * 20..index * 20 + 20)
            .unwrap_or_else(|| panic!("entry {index} of {count} is missing"));
        assert_eq!(entry[10], b' ', "entry {index}: {entry:?}");
        assert_eq!(entry[16], b' ', "entry {index}: {entry:?}");
        assert_eq!(entry[18], b' ', "entry {index}: {entry:?}");
        assert!(
            matches!(entry[17], b'n' | b'f'),
            "entry {index} has no type: {entry:?}"
        );
        if entry[17] == b'f' {
            continue;
        }
        let offset: u64 = std::str::from_utf8(&entry[..10])
            .ok()
            .and_then(|t| t.parse().ok())
            .unwrap_or_else(|| panic!("entry {index} has no offset: {entry:?}"));
        out.push((start + index as u32, offset));
    }
    out
}

/// The trailer dictionary that follows a table, as text.
fn trailer_after(bytes: &[u8], at: usize) -> String {
    let head = bytes.get(at..).unwrap_or_default();
    let found = find(head, b"trailer\n").expect("a trailer follows the table");
    let end = find(&head[found..], b">>\n").expect("the trailer ends") + found + 2;
    String::from_utf8_lossy(&head[found..end]).into_owned()
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

// ---- The offset assertions -------------------------------------------------
//
// Each of these is the body of one test, factored out so the encrypted
// fixtures can be held to the *same* assertion rather than to a weaker new
// one written beside it. Encryption changes every length in the file and none
// of the claims, so an encrypted file that cannot pass these has a layout
// that describes bytes it does not contain.

/// F.2.2: the parameter dictionary is the first object in the file, before
/// anything else a reader would have to skip.
fn assert_the_parameter_dictionary_comes_first(bytes: &[u8]) {
    let head = String::from_utf8_lossy(&bytes[..bytes.len().min(200)]).into_owned();

    assert!(head.starts_with("%PDF-"), "the header: {head}");
    assert!(
        head.contains("1 0 obj\n<< /Linearized 1"),
        "then the parameter dictionary, immediately: {head}"
    );
}

/// `/L` is the file's own length. A reader uses it to tell a complete file
/// from a truncated one before parsing anything.
fn assert_the_declared_length_is_the_real_length(bytes: &[u8]) {
    assert_eq!(parameter(bytes, "L"), bytes.len() as u64);
}

/// `/H` points at the hint stream. Pointing anywhere else sends a reader
/// looking for a stream in the middle of another object.
fn assert_the_hint_stream_is_where_the_dictionary_says(bytes: &[u8]) {
    let at = hint_offset(bytes) as usize;

    let there = String::from_utf8_lossy(&bytes[at..bytes.len().min(at + 40)]).into_owned();
    assert!(
        there.starts_with("2 0 obj"),
        "an indirect object begins at /H: {there}"
    );
    assert!(there.contains("stream"), "and it is a stream: {there}");

    // The second element is the object's length, so the two together frame
    // exactly one object. On an encrypted file this is the assertion that
    // `/H` was measured from the ciphertext: the hint stream is encrypted,
    // and a length taken from the plaintext lands short of `endobj`.
    let length = hint_length(bytes) as usize;
    let end = at + length;
    let tail =
        String::from_utf8_lossy(&bytes[end.saturating_sub(8)..bytes.len().min(end)]).into_owned();
    assert!(
        tail.ends_with("endobj\n"),
        "/H's length ends the hint stream object exactly, not at {tail:?}"
    );
}

/// `/T` points at the main cross-reference table, and `/E` at the end of the
/// first page's material — which must come before it.
fn assert_the_declared_offsets_are_ordered(bytes: &[u8]) {
    let end_of_first_page = parameter(bytes, "E");
    let main_xref = parameter(bytes, "T");
    let hint = hint_offset(bytes);

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
fn assert_the_final_startxref_points_at_the_front(bytes: &[u8]) {
    let offset = final_startxref(bytes);

    assert!(
        (offset as u64) < parameter(bytes, "E"),
        "it points into the front of the file, not the back: {offset}"
    );

    let there = String::from_utf8_lossy(&bytes[offset..bytes.len().min(offset + 8)]).into_owned();
    assert!(
        there.starts_with("xref"),
        "and at a cross-reference table: {there}"
    );
}

/// Every entry in both tables points at the object it names.
///
/// This is the one that tells encrypt-then-measure from the reverse. The
/// declared offsets are all correct or all wrong together in the other
/// assertions; here each of a dozen entries is checked against the header it
/// claims to sit on, so an accumulating error shows up at the object where it
/// first exceeds one object's length and at every object after it.
fn assert_every_cross_reference_entry_points_at_its_object(bytes: &[u8]) {
    let mut checked = 0usize;
    for at in [final_startxref(bytes), parameter(bytes, "T") as usize] {
        for (number, offset) in classic_xref(bytes, at) {
            let want = format!("{number} 0 obj\n");
            let there = bytes
                .get(offset as usize..)
                .and_then(|s| s.get(..want.len()))
                .unwrap_or_default();
            assert_eq!(
                String::from_utf8_lossy(there),
                want,
                "the entry for object {number} points at {offset}, where object {number} is not"
            );
            checked += 1;
        }
    }
    assert!(
        checked >= 12,
        "only {checked} entries were checked, which is too few to catch a drift"
    );
}

/// The trailer's `/Size` covers every object the tables name (7.5.5 Table 15).
fn assert_the_trailer_size_covers_every_object(bytes: &[u8]) {
    let front_at = final_startxref(bytes);
    let trailer = trailer_after(bytes, front_at);
    let at = trailer.rfind("/Size ").expect("a /Size");
    let size: u32 = trailer[at + 6..]
        .chars()
        .take_while(char::is_ascii_digit)
        .collect::<String>()
        .parse()
        .expect("a number");

    let highest = classic_xref(bytes, front_at)
        .into_iter()
        .chain(classic_xref(bytes, parameter(bytes, "T") as usize))
        .map(|(number, _)| number)
        .max()
        .expect("some entries");
    assert!(
        size > highest,
        "/Size {size} must exceed the highest object number {highest}"
    );
}

#[test]
fn the_parameter_dictionary_comes_first() {
    assert_the_parameter_dictionary_comes_first(&linearized(3));
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

#[test]
fn the_declared_length_is_the_real_length() {
    assert_the_declared_length_is_the_real_length(&linearized(3));
}

#[test]
fn the_hint_stream_is_where_the_dictionary_says() {
    assert_the_hint_stream_is_where_the_dictionary_says(&linearized(4));
}

#[test]
fn the_declared_offsets_are_ordered_the_way_the_layout_is() {
    assert_the_declared_offsets_are_ordered(&linearized(5));
}

#[test]
fn the_final_startxref_points_at_the_front_of_the_file() {
    assert_the_final_startxref_points_at_the_front(&linearized(8));
}

#[test]
fn every_cross_reference_entry_points_at_its_object() {
    assert_every_cross_reference_entry_points_at_its_object(&linearized(6));
}

#[test]
fn the_trailer_size_covers_every_object() {
    assert_the_trailer_size_covers_every_object(&linearized(6));
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

// ---- Encryption and linearization together ---------------------------------

/// The headline.
///
/// Asking for both used to give an encrypted file with an ordinary layout:
/// `rewrite` guarded the linearized path with `encryption.is_none()`, so
/// encryption won silently and the layout request was discarded with no error
/// and no warning.
#[test]
fn an_encrypted_file_can_also_be_linearized() {
    let bytes = encrypted_linearized(6);
    let head = String::from_utf8_lossy(&bytes[..bytes.len().min(200)]).into_owned();
    assert!(
        head.contains("/Linearized 1"),
        "the layout request survived the encryption request: {head}"
    );

    let doc = CosDocument::open(bytes).expect("it opens");
    assert!(
        doc.is_encrypted(),
        "and the encryption request survived too"
    );
    assert_eq!(doc.auth_level(), AuthLevel::None, "it wants a password");
    assert_eq!(doc.authenticate(PASSWORD), Ok(AuthLevel::User));

    let collected = pages::collect(&doc);
    assert_eq!(collected.len(), 6, "every page is reachable");
    for (index, page) in collected.iter().enumerate() {
        let content = pages::content_bytes(&doc, page);
        assert!(
            String::from_utf8_lossy(&content).contains(&format!("page {index}")),
            "page {index} decrypts back to what it drew"
        );
    }

    assert_eq!(
        tinker_pdf_cos::metadata(&doc).title.as_deref(),
        Some("linearized"),
        "and so do the strings"
    );
}

/// The round trip above must not be passing through a plaintext file that
/// merely carries an `/Encrypt` dictionary. Each needle is proved present in
/// the unencrypted layout first, so an assertion cannot pass because the
/// needle was never there.
#[test]
fn a_linearized_encrypted_file_holds_no_plaintext_content() {
    let plain = linearized(6);
    let sealed = encrypted_linearized(6);

    for needle in [&b"(page 0)"[..], b"(page 5)", b"(linearized)"] {
        let shown = String::from_utf8_lossy(needle).into_owned();
        assert!(
            find(&plain, needle).is_some(),
            "{shown} is in the plaintext layout, so its absence below means something"
        );
        assert!(
            find(&sealed, needle).is_none(),
            "{shown} must be ciphertext in the encrypted layout"
        );
    }
}

// ---- The offsets, measured against the encrypted bytes ---------------------

/// The same assertions the unencrypted tests make, on an encrypted file.
///
/// Deliberately the *same* functions rather than weaker copies: the claims
/// linearization makes do not change when the file is encrypted, only the
/// lengths do, and a file that cannot meet them has a layout describing bytes
/// it does not contain. Six pages so a drift has room to accumulate.
#[test]
fn an_encrypted_file_makes_every_offset_claim_an_unencrypted_one_does() {
    let bytes = encrypted_linearized(6);

    assert_the_parameter_dictionary_comes_first(&bytes);
    assert_the_declared_length_is_the_real_length(&bytes);
    assert_the_hint_stream_is_where_the_dictionary_says(&bytes);
    assert_the_declared_offsets_are_ordered(&bytes);
    assert_the_final_startxref_points_at_the_front(&bytes);
    assert_every_cross_reference_entry_points_at_its_object(&bytes);
    assert_the_trailer_size_covers_every_object(&bytes);
}

/// The fixture's ciphertext really is longer than its plaintext, and by
/// enough, in enough places, for the wrong measuring order to show.
///
/// This is the trap the plan names. With a one-stream fixture, measuring
/// before encrypting moves only the objects after that stream, and with a
/// small enough file it can move them by an amount no assertion looks at. So
/// the numbers are asserted rather than assumed: every stream is at least
/// seventeen bytes longer encrypted — sixteen for the initialisation vector
/// and at least one for the padding, which is never empty because CBC pads a
/// whole block when the plaintext already fits — and the total across the
/// file is hundreds of bytes, which is more than one object's length.
#[test]
fn the_fixture_streams_encrypt_to_genuinely_different_lengths() {
    let bytes = encrypted_linearized(6);
    let doc = CosDocument::open(bytes).expect("it opens");
    assert_eq!(doc.authenticate(PASSWORD), Ok(AuthLevel::User));

    let mut streams = 0usize;
    let mut total_growth = 0u64;
    for num in 1..=doc.max_object_number() {
        let r = ObjRef::new(num, 0);
        let Ok(object) = doc.get(r) else { continue };
        let Object::Stream(stream) = object.as_ref() else {
            continue;
        };
        // /Length is what the file says, which is the encrypted length;
        // `stream_raw` hands back the plaintext now that we have the key.
        let declared = stream
            .dict
            .get_int(tinker_pdf_cos::Name::LENGTH)
            .expect("a /Length") as u64;
        let plain = doc.stream_raw(r).expect("it decrypts").len() as u64;
        assert!(
            declared >= plain + 17,
            "object {num}: /Length {declared} against {plain} plaintext bytes"
        );
        streams += 1;
        total_growth += declared - plain;
    }

    assert!(
        streams >= 7,
        "only {streams} streams, which is too few for a drift to accumulate"
    );
    assert!(
        total_growth > 150,
        "the ciphertext is only {total_growth} bytes longer in total"
    );
}

/// Ruling 4, in the form this gap needs: the same fixture written twice is
/// the same file, byte for byte.
///
/// Padding makes the layout depend on the plaintext's length, and the
/// initialisation vectors are derived from the file key — so if the entropy
/// were not deterministic neither would the offsets be. The entropy is
/// caller-supplied precisely so a caller who needs reproducibility can have
/// it, and this is the assertion that it is not accidental.
#[test]
fn an_encrypted_linearized_file_is_byte_for_byte_reproducible() {
    let first = encrypted_linearized(6);
    let second = encrypted_linearized(6);
    assert_eq!(
        first.len(),
        second.len(),
        "the same document twice is the same length"
    );
    assert!(first == second, "and the same bytes");
}

// ---- What stays in the clear (7.6.1) ---------------------------------------

/// The parameter dictionary is consulted *before* a reader authenticates, so
/// it has to be legible without the password. 7.6.1 does not exempt it; what
/// makes that sound is that it contains nothing encryption would touch, which
/// `linearize.rs` asserts for itself.
#[test]
fn the_parameter_dictionary_is_readable_without_the_password() {
    let bytes = encrypted_linearized(6);
    let head = String::from_utf8_lossy(&bytes[..bytes.len().min(200)]).into_owned();

    assert!(head.starts_with("%PDF-"), "the header: {head}");
    assert!(
        head.contains("1 0 obj\n<< /Linearized 1"),
        "then the parameter dictionary, immediately: {head}"
    );

    // Every field parses out of the raw bytes, with no key involved.
    for key in ["L", "O", "E", "N", "T"] {
        assert!(parameter(&bytes, key) > 0, "/{key} is legible and non-zero");
    }
    assert!(hint_offset(&bytes) > 0, "and so is /H");
}

/// Both tables stay clear because a reader finds them before it knows there
/// is an `/Encrypt` dictionary to look for. `classic_xref` cannot parse a
/// table that is not, so the parse is the assertion.
#[test]
fn both_cross_reference_tables_are_readable_without_the_password() {
    let bytes = encrypted_linearized(6);

    let front_at = final_startxref(&bytes);
    let front = classic_xref(&bytes, front_at);
    assert!(
        front.len() >= 4,
        "the first-page table carries its entries: {front:?}"
    );

    let main_at = parameter(&bytes, "T") as usize;
    let main = classic_xref(&bytes, main_at);
    assert!(
        !main.is_empty(),
        "and so does the main table: {main:?} at {main_at}"
    );

    // Their trailers are clear with them; a table a reader cannot follow to
    // /Root and /Encrypt is no more use than an encrypted one.
    let front_trailer = trailer_after(&bytes, front_at);
    assert!(front_trailer.contains("/Root "), "{front_trailer}");
    assert!(
        front_trailer.contains("/Encrypt 3 0 R"),
        "the first trailer names the encryption dictionary: {front_trailer}"
    );
    assert!(
        front_trailer.contains("/Prev "),
        "and chains to the main table: {front_trailer}"
    );

    let main_trailer = trailer_after(&bytes, main_at);
    assert!(main_trailer.contains("/Root "), "{main_trailer}");
    assert!(
        main_trailer.contains("/Encrypt 3 0 R"),
        "the main trailer names it too, for a reader that started at /T: {main_trailer}"
    );
}

/// The `/Encrypt` dictionary is the one object that can never be encrypted,
/// since a reader needs it before it can decrypt anything at all. It leads
/// part 4, so a reader holding the front of the file already has it.
#[test]
fn the_encryption_dictionary_is_readable_without_the_password() {
    let bytes = encrypted_linearized(6);

    let at = find(&bytes, b"\n3 0 obj\n").expect("object 3 is in the file") + 1;
    let end = find(&bytes[at..], b"\nendobj\n").expect("and it ends") + at;
    let dict = String::from_utf8_lossy(&bytes[at..end]).into_owned();

    for needle in [
        "/Filter /Standard",
        "/V 5",
        "/R 6",
        "/CFM /AESV3",
        "/StmF /StdCF",
        "/StrF /StdCF",
        "/U <",
        "/O <",
        "/Perms <",
    ] {
        assert!(dict.contains(needle), "{needle} is in the clear: {dict}");
    }

    // And it is at the front, ahead of everything page one needs.
    assert!(
        (at as u64) < parameter(&bytes, "E"),
        "the /Encrypt dictionary is inside the first /E bytes, at {at}"
    );
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
