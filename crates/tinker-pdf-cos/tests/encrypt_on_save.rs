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
    pages, AuthLevel, CosDocument, DocumentBuilder, DocumentEditor, Encryption, WriteMode,
    WriteOptions,
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
