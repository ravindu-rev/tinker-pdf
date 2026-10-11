//! Encrypt-on-save (phase 09).
//!
//! `WriteOptions::encryption` existed, was documented, and had no reader at
//! all: setting it produced a plaintext file with no error and no warning. A
//! caller who asked for encryption got a document that looked encrypted in
//! their code and was not on disk, which is the worst possible way for a
//! security option to fail.
//!
//! The test that matters is the round trip: write it encrypted, read it back
//! with this engine's own handler, and check both that the password is
//! required and that the content survives it.

use std::sync::Arc;

use tinker_pdf_cos::{
    pages, AuthLevel, CosDocument, Dict, DocumentBuilder, DocumentEditor, Encryption, Object,
    PdfString, StreamData, WriteMode, WriteOptions,
};

/// Forty-eight deterministic bytes. Real callers pass real randomness; a test
/// wants the same document every run, and the key derivation cannot tell.
fn entropy() -> [u8; 48] {
    let mut bytes = [0u8; 48];
    for (index, byte) in bytes.iter_mut().enumerate() {
        *byte = (index as u8).wrapping_mul(7).wrapping_add(11);
    }
    bytes
}

fn source() -> Arc<CosDocument> {
    let mut builder = DocumentBuilder::new();
    builder.add_base_font(b"F0", b"Helvetica");
    builder.set_info(b"Title", "a secret title");
    builder.add_page(200.0, 100.0, |page| {
        page.text(b"F0", 12.0, 10.0, 50.0, "CONFIDENTIAL CONTENT");
    });
    Arc::new(CosDocument::open(builder.finish()).expect("it opens"))
}

fn encrypted(user: &str, owner: &str, permissions: i32) -> Vec<u8> {
    let editor = DocumentEditor::new(source());
    editor.save(&WriteOptions {
        mode: WriteMode::Rewrite,
        encryption: Some(Encryption {
            user_password: user.to_string(),
            owner_password: owner.to_string(),
            permissions,
            entropy: entropy(),
        }),
        ..WriteOptions::default()
    })
}

/// The headline: the option now does something, and what it does is reversible
/// by this engine's own reader.
#[test]
fn a_written_document_encrypts_and_decrypts() {
    let bytes = encrypted("open-me", "owner-me", -1);
    let doc = CosDocument::open(bytes).expect("it opens");

    assert!(doc.is_encrypted(), "the file declares encryption");
    assert_eq!(doc.auth_level(), AuthLevel::None, "and wants a password");

    assert_eq!(doc.authenticate("open-me"), Ok(AuthLevel::User));

    let collected = pages::collect(&doc);
    assert_eq!(collected.len(), 1);
    let content = pages::content_bytes(&doc, &collected[0]);
    assert!(
        String::from_utf8_lossy(&content).contains("CONFIDENTIAL CONTENT"),
        "and the content decrypts back to itself"
    );
}

/// The content must not be readable without the password. This is the
/// assertion that would have failed before: the bytes were simply plaintext.
#[test]
fn the_content_is_not_in_the_file_in_the_clear() {
    let bytes = encrypted("open-me", "owner-me", -1);
    let text = String::from_utf8_lossy(&bytes);
    assert!(
        !text.contains("CONFIDENTIAL CONTENT"),
        "the page content is ciphertext in the file"
    );
}

/// 7.6.2 encrypts strings as well as streams. Leaving them clear puts titles,
/// form values and annotation contents in plain sight inside a file that
/// claims to be encrypted.
#[test]
fn strings_are_encrypted_too() {
    let bytes = encrypted("open-me", "owner-me", -1);
    assert!(
        !String::from_utf8_lossy(&bytes).contains("a secret title"),
        "the /Info title is ciphertext"
    );

    let doc = CosDocument::open(bytes).expect("it opens");
    assert_eq!(doc.authenticate("open-me"), Ok(AuthLevel::User));
    let metadata = tinker_pdf_cos::metadata(&doc);
    assert_eq!(
        metadata.title.as_deref(),
        Some("a secret title"),
        "and decrypts back"
    );
}

#[test]
fn a_wrong_password_is_refused() {
    let bytes = encrypted("open-me", "owner-me", -1);
    let doc = CosDocument::open(bytes).expect("it opens");
    assert!(doc.authenticate("not-it").is_err());
    assert_eq!(doc.auth_level(), AuthLevel::None);
}

/// The two passwords are distinguishable, which is the whole reason the owner
/// one exists — and the defect that motivated this engine was an API that
/// could not tell them apart.
#[test]
fn the_owner_password_authenticates_as_the_owner() {
    let bytes = encrypted("open-me", "owner-me", -1);
    let doc = CosDocument::open(bytes).expect("it opens");
    assert_eq!(doc.authenticate("owner-me"), Ok(AuthLevel::Owner));
}

/// `/P` survives the round trip, so a document written to forbid printing
/// still forbids it when read back.
#[test]
fn permissions_survive_the_round_trip() {
    // Every bit set except bit 3 (printing), which is 1-based in the spec.
    let no_printing = !0b100i32;
    let bytes = encrypted("open-me", "owner-me", no_printing);

    let doc = CosDocument::open(bytes).expect("it opens");
    assert_eq!(doc.authenticate("open-me"), Ok(AuthLevel::User));
    assert!(!doc.permissions().print(), "printing is denied");
    assert!(doc.permissions().copy(), "and copying is not");
}

/// An empty user password is how a document restricts permissions without
/// asking anyone for anything, and it must still open.
#[test]
fn an_empty_user_password_opens_with_one() {
    let bytes = encrypted("", "owner-me", -1);
    let doc = CosDocument::open(bytes).expect("it opens");
    assert_eq!(doc.authenticate(""), Ok(AuthLevel::User));

    let collected = pages::collect(&doc);
    let content = pages::content_bytes(&doc, &collected[0]);
    assert!(String::from_utf8_lossy(&content).contains("CONFIDENTIAL"));
}

/// **An empty owner password is not a lock every reader opens.**
///
/// Algorithm 2.A tries the owner password first, and every reader tries the
/// empty password first. So a `/O` derived from the empty string handed the
/// owner's authority — and with it the file key — to anybody, whatever the
/// user password was: `user "open-me", owner ""` was a file anyone opened
/// with every permission, and the user password protected nothing. The C
/// ABI documents an empty owner password as "none" and the Python binding
/// defaults to one, so the plainest call made the weakest file. Algorithm 3
/// step (a) answers this for R2 to R4 — "if there is no owner password, use
/// the user password instead" — and the writer now takes that answer for R6:
/// the user password opens the file, with the owner's authority since the two
/// are one, and nothing else does.
#[test]
fn an_empty_owner_password_is_the_user_password() {
    let no_printing = !0b100i32;
    let bytes = encrypted("open-me", "", no_printing);
    let doc = CosDocument::open(bytes.clone()).expect("it opens");
    assert!(
        doc.authenticate("").is_err(),
        "the empty password opens nothing"
    );
    assert_eq!(doc.auth_level(), AuthLevel::None);

    let doc = CosDocument::open(bytes).expect("it opens");
    assert_eq!(
        doc.authenticate("open-me"),
        Ok(AuthLevel::Owner),
        "the user password is the owner's too"
    );
    let collected = pages::collect(&doc);
    let content = pages::content_bytes(&doc, &collected[0]);
    assert!(String::from_utf8_lossy(&content).contains("CONFIDENTIAL"));

    // With neither password there is nothing to substitute: the empty
    // password is both, as it always was, and the restrictions bind nobody.
    let doc = CosDocument::open(encrypted("", "", no_printing)).expect("it opens");
    assert_eq!(doc.authenticate(""), Ok(AuthLevel::Owner));
}

/// Two streams with identical plaintext must not encrypt identically, or the
/// fact that they match leaks without the key.
#[test]
fn identical_content_encrypts_differently() {
    let mut builder = DocumentBuilder::new();
    for _ in 0..2 {
        builder.add_page(100.0, 100.0, |page| {
            page.fill_rect(0.0, 0.0, 50.0, 50.0, 0.0);
        });
    }
    let doc = Arc::new(CosDocument::open(builder.finish()).expect("it opens"));

    let editor = DocumentEditor::new(doc);
    let bytes = editor.save(&WriteOptions {
        mode: WriteMode::Rewrite,
        encryption: Some(Encryption {
            user_password: "x".to_string(),
            owner_password: "y".to_string(),
            permissions: -1,
            entropy: entropy(),
        }),
        ..WriteOptions::default()
    });

    let doc = CosDocument::open(bytes).expect("it opens");
    let first = doc
        .stream_raw(tinker_pdf_cos::ObjRef::new(4, 0))
        .unwrap_or_default();
    let second = doc
        .stream_raw(tinker_pdf_cos::ObjRef::new(6, 0))
        .unwrap_or_default();

    if !first.is_empty() && !second.is_empty() {
        assert_ne!(
            first, second,
            "a per-object initialisation vector keeps identical plaintext apart"
        );
    }
}

/// Encryption together with object streams.
///
/// These two options were tested separately and never together, and together
/// they produced a permanently unreadable file: `/Encrypt` was numbered above
/// the object-stream container, the cross-reference stream was sized from the
/// container alone, and so the one object a reader needs before it can decrypt
/// anything had no entry. The file reopened as *unencrypted with zero pages*
/// while its content was genuinely ciphertext.
#[test]
fn encryption_survives_object_streams() {
    let editor = DocumentEditor::new(source());
    let bytes = editor.save(&WriteOptions {
        mode: WriteMode::Rewrite,
        object_streams: true,
        compress: true,
        encryption: Some(Encryption {
            user_password: "open-me".to_string(),
            owner_password: "owner-me".to_string(),
            permissions: -1,
            entropy: entropy(),
        }),
        ..WriteOptions::default()
    });

    let doc = CosDocument::open(bytes).expect("it opens");
    assert!(doc.is_encrypted(), "the /Encrypt dictionary is reachable");
    assert_eq!(doc.authenticate("open-me"), Ok(AuthLevel::User));

    let collected = pages::collect(&doc);
    assert_eq!(collected.len(), 1, "and the pages are there");
    let content = pages::content_bytes(&doc, &collected[0]);
    assert!(String::from_utf8_lossy(&content).contains("CONFIDENTIAL"));
}

/// 7.5.5 Table 15: `/Size` is one more than the highest object number in the
/// file. Taking it from the object set alone left every encrypted file
/// understating it, because `/Encrypt` is written above that set.
#[test]
fn the_trailer_size_counts_the_encryption_dictionary() {
    let bytes = encrypted("open-me", "owner-me", -1);
    let text = String::from_utf8_lossy(&bytes).into_owned();

    let at = text.rfind("/Encrypt ").expect("an /Encrypt reference");
    let number: u32 = text[at + 9..]
        .chars()
        .take_while(char::is_ascii_digit)
        .collect::<String>()
        .parse()
        .expect("an object number");

    let at = text.rfind("/Size ").expect("a /Size");
    let size: u32 = text[at + 6..]
        .chars()
        .take_while(char::is_ascii_digit)
        .collect::<String>()
        .parse()
        .expect("a number");

    assert!(
        size > number,
        "/Size {size} must exceed the /Encrypt object number {number}"
    );
}

/// **No object number is written twice.**
///
/// 7.3.10 makes an object number the identity of an object, so a file with two
/// `N 0 obj` headers for one `N` has no defined meaning: which one a reader
/// gets depends on which cross-reference entry survived, and the two here were
/// the `/Encrypt` dictionary and the cross-reference stream itself. Both were
/// numbered `max + 2` on an encrypted rewrite that packed objects.
///
/// Nothing caught it because neither object is ever reached *by number* — a
/// reader finds the stream through `startxref` and `/Encrypt` through the
/// trailer — so every existing assertion about this file passed. It surfaced
/// when the cross-reference stream stopped being written densely and the two
/// rows had to share one slot in a map.
#[test]
fn no_object_number_is_written_twice() {
    let editor = DocumentEditor::new(source());
    let bytes = editor.save(&WriteOptions {
        mode: WriteMode::Rewrite,
        object_streams: true,
        compress: true,
        encryption: Some(Encryption {
            user_password: "open-me".to_string(),
            owner_password: "owner-me".to_string(),
            permissions: -1,
            entropy: entropy(),
        }),
        ..WriteOptions::default()
    });

    // Header scan rather than a parse: the point is what is *in the file*, and
    // asking this repository's reader would ask the half that already agrees.
    let text = String::from_utf8_lossy(&bytes).into_owned();
    let mut seen: Vec<u32> = Vec::new();
    for (index, _) in text.match_indices(" 0 obj") {
        let before = &text[..index];
        let digits: String = before
            .chars()
            .rev()
            .take_while(char::is_ascii_digit)
            .collect::<Vec<char>>()
            .into_iter()
            .rev()
            .collect();
        if let Ok(number) = digits.parse::<u32>() {
            seen.push(number);
        }
    }
    assert!(seen.len() > 2, "a rewrite writes objects: {seen:?}");

    let mut sorted = seen.clone();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(
        sorted.len(),
        seen.len(),
        "an object number is written twice: {seen:?}"
    );
}

const STREAM_SECRET: &[u8] = b"A STREAM DICTIONARY SECRET";
const NESTED_SECRET: &[u8] = b"A NESTED CHECKSUM SECRET";

/// Whether `bytes` holds `plaintext` as written in the clear: as a literal
/// string, or as the hex string the writer writes a hex-form one as.
fn in_the_clear(bytes: &[u8], plaintext: &[u8]) -> bool {
    let upper: String = plaintext.iter().map(|b| format!("{b:02X}")).collect();
    let lower = upper.to_ascii_lowercase();
    [plaintext, upper.as_bytes(), lower.as_bytes()]
        .iter()
        .any(|probe| bytes.windows(probe.len()).any(|window| window == *probe))
}

/// `editor`'s first page with its first content stream replaced by `content`
/// under a dictionary carrying two strings: `/Secret` at the top, and
/// `/CheckSum` inside a `/Params` dictionary, where an embedded file stream
/// carries one (7.11.4 Table 45).
fn with_strings_in_a_stream_dictionary(editor: &mut DocumentEditor, content: &[u8]) {
    let doc = editor.shared_document();
    let page = pages::collect(&doc).into_iter().next().expect("a page");
    let target = pages::contents(&doc, &page)
        .into_iter()
        .next()
        .expect("a content stream");
    let mut params = Dict::new();
    params.insert(
        editor.intern(b"CheckSum"),
        Object::String(PdfString::hex(NESTED_SECRET.to_vec())),
    );
    let mut dict = Dict::new();
    dict.insert(
        editor.intern(b"Secret"),
        Object::String(PdfString::literal(STREAM_SECRET.to_vec())),
    );
    dict.insert(editor.intern(b"Params"), Object::Dict(params));
    editor.put_stream(
        target,
        StreamData {
            dict,
            data: content.to_vec(),
        },
    );
}

/// The two strings [`with_strings_in_a_stream_dictionary`] put on the first
/// page's first content stream, as `doc` reads them.
fn stream_dictionary_strings(doc: &CosDocument) -> (Option<Vec<u8>>, Option<Vec<u8>>) {
    let page = pages::collect(doc).into_iter().next().expect("a page");
    let target = pages::contents(doc, &page)
        .into_iter()
        .next()
        .expect("a content stream");
    let object = doc.get(target).expect("the content stream");
    let Object::Stream(stream) = object.as_ref() else {
        panic!("a stream: {object:?}");
    };
    let secret = stream
        .dict
        .get_string(doc.intern(b"Secret"))
        .map(|s| s.bytes.clone());
    let nested = stream
        .dict
        .get_dict(doc.intern(b"Params"))
        .and_then(|params| params.get_string(doc.intern(b"CheckSum")))
        .map(|s| s.bytes.clone());
    (secret, nested)
}

/// 7.6.2: every string is encrypted, and a stream's own dictionary is where
/// an embedded file's `/Params` `/CheckSum` and `/ModDate`, or a form's
/// `/PieceInfo`, sit — and the reader decrypts strings there as it does
/// anywhere. The writer sealed a stream's bytes and wrote its dictionary as
/// given, so every encrypting save put those strings in the clear, and on
/// reopening decrypted the clear bytes as if they were ciphertext. Each path
/// that seals is held to it: a rewrite, one that packs objects, a linearized
/// one, and an incremental update under the key the document was opened with.
#[test]
fn strings_in_a_stream_dictionary_are_encrypted_too() {
    let content = b"BT /F0 12 Tf 10 50 Td (CONFIDENTIAL CONTENT) Tj ET";
    let sealed = |object_streams: bool, linearize: bool| -> Vec<u8> {
        let mut editor = DocumentEditor::new(source());
        with_strings_in_a_stream_dictionary(&mut editor, content);
        editor.save(&WriteOptions {
            mode: WriteMode::Rewrite,
            object_streams,
            linearize,
            compress: object_streams,
            encryption: Some(Encryption {
                user_password: "open-me".to_string(),
                owner_password: "owner-me".to_string(),
                permissions: -1,
                entropy: entropy(),
            }),
            ..WriteOptions::default()
        })
    };
    let mut saves = vec![
        ("a rewrite", sealed(false, false), "open-me"),
        ("a rewrite packing objects", sealed(true, false), "open-me"),
        ("a linearized rewrite", sealed(false, true), "open-me"),
    ];

    let fixture = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../testdata/encrypted-aes256.pdf");
    let original = std::fs::read(&fixture).expect("the fixture");
    let doc = CosDocument::open(original.clone()).expect("it opens");
    doc.authenticate("open-sesame")
        .expect("the password opens it");
    let mut editor = DocumentEditor::new(Arc::new(doc));
    with_strings_in_a_stream_dictionary(&mut editor, content);
    let updated = editor.save(&WriteOptions {
        mode: WriteMode::Incremental,
        ..WriteOptions::default()
    });
    assert!(updated.starts_with(&original), "an update appends");
    saves.push((
        "an incremental update under the inherited key",
        updated,
        "open-sesame",
    ));

    for (how, bytes, password) in saves {
        for secret in [STREAM_SECRET, NESTED_SECRET, &b"CONFIDENTIAL CONTENT"[..]] {
            assert!(
                !in_the_clear(&bytes, secret),
                "{how}: {:?} is in the file in the clear",
                String::from_utf8_lossy(secret)
            );
        }
        let doc = CosDocument::open(bytes).expect("it opens");
        assert_eq!(doc.authenticate(password), Ok(AuthLevel::User), "{how}");
        assert_eq!(
            stream_dictionary_strings(&doc),
            (Some(STREAM_SECRET.to_vec()), Some(NESTED_SECRET.to_vec())),
            "{how}: the strings decrypt back to themselves"
        );
        let page = pages::collect(&doc).into_iter().next().expect("a page");
        assert!(
            String::from_utf8_lossy(&pages::content_bytes(&doc, &page))
                .contains("CONFIDENTIAL CONTENT"),
            "{how}: and so does the stream"
        );
    }
}
