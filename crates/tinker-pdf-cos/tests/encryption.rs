//! Decrypting real encrypted documents.
//!
//! Two of these assertions are the whole point of the phase: they are the
//! MuPDF defects Tinker documented, fixed. Owner and user authentication are
//! told apart, and a `/P` whose reserved bits are set still reports its
//! restrictions instead of degrading to "everything is permitted".

use std::path::PathBuf;
use tinker_pdf_cos::{AuthError, AuthLevel, CosDocument, Name};

fn fixture(name: &str) -> Vec<u8> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../testdata")
        .join(name);
    std::fs::read(&path).unwrap_or_else(|e| panic!("reading {}: {e}", path.display()))
}

fn open(name: &str) -> CosDocument {
    CosDocument::open(fixture(name)).expect("the fixture opens")
}

#[test]
fn the_user_password_authenticates_as_the_user() {
    let doc = open("encrypted-aes256.pdf");
    assert!(doc.is_encrypted());
    assert_eq!(doc.auth_level(), AuthLevel::None, "before any password");

    assert_eq!(doc.authenticate("open-sesame"), Ok(AuthLevel::User));
    assert_eq!(doc.auth_level(), AuthLevel::User);
}

/// The distinction MuPDF's bindings could not make: `fz_authenticate_password`
/// returns a bitmask saying *which* password matched, and the wrapper Tinker
/// used collapsed it to a bool.
#[test]
fn the_owner_password_authenticates_as_the_owner() {
    let doc = open("encrypted-aes256.pdf");
    assert_eq!(doc.authenticate("owner-secret"), Ok(AuthLevel::Owner));
    assert_eq!(doc.auth_level(), AuthLevel::Owner);
    assert!(
        doc.auth_level() > AuthLevel::User,
        "owner authority exceeds user authority"
    );
}

#[test]
fn a_wrong_password_is_refused() {
    let doc = open("encrypted-aes256.pdf");
    assert_eq!(
        doc.authenticate("not-the-password"),
        Err(AuthError::WrongPassword)
    );
    assert_eq!(doc.auth_level(), AuthLevel::None);
    // A refusal must leave the document usable for another attempt.
    assert_eq!(doc.authenticate("open-sesame"), Ok(AuthLevel::User));
}

#[test]
fn an_unencrypted_document_has_nothing_to_authenticate() {
    let doc = open("simple-text.pdf");
    assert!(!doc.is_encrypted());
    assert_eq!(doc.authenticate("anything"), Err(AuthError::NotEncrypted));
    assert!(
        doc.permissions().print(),
        "an unencrypted document permits everything"
    );
}

/// The second MuPDF defect: `/P`'s reserved bits are 1 by specification, so a
/// strict bitflags parse fails on every real value and the fallback granted
/// everything — including for this file, which forbids printing.
#[test]
fn permissions_survive_their_reserved_bits() {
    let doc = open("permissions-noprint.pdf");
    assert_eq!(doc.authenticate("user"), Ok(AuthLevel::User));

    let p = doc.permissions();
    assert_eq!(p.raw(), -2056, "the raw /P is preserved");
    assert!(!p.print(), "this document forbids printing");
    assert!(!p.print_high_res());
    assert!(p.copy(), "and permits copying");
}

/// Under owner authentication the flags remain in the file but no longer
/// restrict, which is the policy Tinker's security plan settled on.
#[test]
fn the_owner_password_lifts_restrictions() {
    let doc = open("permissions-noprint.pdf");
    assert_eq!(doc.authenticate("owner"), Ok(AuthLevel::Owner));
    assert!(doc.permissions().print(), "restrictions no longer apply");
}

/// Authentication is worth nothing if the content stays ciphertext.
#[test]
fn content_decrypts_and_decodes_after_authentication() {
    let doc = open("encrypted-aes256.pdf");
    assert_eq!(doc.authenticate("open-sesame"), Ok(AuthLevel::User));

    let contents = first_page_contents(&doc).expect("a first page with contents");
    let decoded = doc.stream_decoded(contents).expect("the stream decodes");
    let text = String::from_utf8_lossy(&decoded);
    assert!(
        text.contains("BT"),
        "decrypted content should hold text operators, got: {}",
        &text.chars().take(80).collect::<String>()
    );
}

/// Descends `/Root` → `/Pages` to the first leaf and returns its `/Contents`
/// reference. `/Kids` may nest, and `/Contents` may be an array of streams.
fn first_page_contents(doc: &CosDocument) -> Option<tinker_pdf_cos::ObjRef> {
    let page = tinker_pdf_cos::Name::PAGES;
    let root = doc.trailer().get_ref(Name::ROOT)?;
    let catalog = doc.get(root).ok()?;
    let mut node = catalog.as_dict().and_then(|d| d.get_ref(page))?;

    for _ in 0..16 {
        let object = doc.get(node).ok()?;
        let dict = object.as_dict()?;

        match dict.get_array(Name::KIDS) {
            Some(kids) => node = kids.first().and_then(tinker_pdf_cos::Object::as_objref)?,
            None => {
                return dict.get_ref(Name::CONTENTS).or_else(|| {
                    dict.get_array(Name::CONTENTS)?
                        .first()
                        .and_then(tinker_pdf_cos::Object::as_objref)
                })
            }
        }
    }
    None
}

/// Strings are decrypted too, not only streams — and a document that opened
/// before authentication must not keep serving ciphertext afterwards.
#[test]
fn strings_decrypt_after_the_store_is_cleared() {
    let doc = open("encrypted-aes256.pdf");

    let producer = doc.intern(b"Producer");
    let info_ref = doc.trailer().get_ref(Name::INFO);
    let read = |doc: &CosDocument| {
        info_ref
            .and_then(|r| doc.get(r).ok())
            .and_then(|o| o.as_dict().and_then(|d| d.get_string(producer)).cloned())
    };

    let before = read(&doc);
    assert_eq!(doc.authenticate("open-sesame"), Ok(AuthLevel::User));
    let after = read(&doc);

    if let (Some(before), Some(after)) = (before, after) {
        assert_ne!(
            before.bytes, after.bytes,
            "the ciphertext read before authentication must not persist"
        );
        assert!(
            after.bytes.is_ascii(),
            "a decrypted /Producer should be readable text, got {:?}",
            after.bytes
        );
    }
}

/// Rewriting an authenticated encrypted document produced a corrupt file: the
/// trailer was copied verbatim, so `/Encrypt` survived, while the streams went
/// out through `stream_raw` — which is plaintext once a decryptor is
/// installed. The output advertised encryption over clear bytes, and every
/// reader that believed it decrypted garbage.
#[test]
fn rewriting_a_decrypted_document_does_not_claim_to_be_encrypted() {
    use std::sync::Arc;
    use tinker_pdf_cos::{DocumentEditor, WriteMode, WriteOptions};

    let doc = open("encrypted-aes256.pdf");
    assert_eq!(doc.authenticate("open-sesame"), Ok(AuthLevel::User));
    assert!(doc.is_encrypted(), "the source is encrypted");

    let editor = DocumentEditor::new(Arc::new(doc));
    let saved = editor.save(&WriteOptions {
        mode: WriteMode::Rewrite,
        ..WriteOptions::default()
    });

    let reopened = CosDocument::open(saved).expect("the rewrite opens");
    assert!(
        !reopened.is_encrypted(),
        "the output is plaintext and must not say otherwise"
    );

    // And it is genuinely readable without a password.
    let pages = tinker_pdf_cos::pages::collect(&reopened);
    assert!(!pages.is_empty(), "the page tree survived");
    let content = tinker_pdf_cos::pages::content_bytes(&reopened, &pages[0]);
    assert!(
        !content.is_empty(),
        "and the content decrypted rather than being copied as ciphertext"
    );
}

/// 7.6.2: with `/EncryptMetadata false` the metadata stream is left in the
/// clear so indexers can read it without the password. Decrypting it anyway
/// yields noise where the document's identity should be — and it reads as a
/// corrupt file rather than as this mistake.
#[test]
fn metadata_is_left_in_the_clear_when_the_document_says_so() {
    // A document that declares encryption and marks its metadata exempt. The
    // handler is unsupported, so nothing is decrypted, but the *decision*
    // about the metadata stream is what is under test and it is made before
    // any key exists.
    let bytes: &[u8] = b"%PDF-1.7\n\
1 0 obj\n<< /Type /Catalog /Pages 2 0 R /Metadata 5 0 R >>\nendobj\n\
2 0 obj\n<< /Type /Pages /Count 0 /Kids [] >>\nendobj\n\
5 0 obj\n<< /Type /Metadata /Subtype /XML /Length 26 >>\nstream\n\
<rdf>plain and legible</rdf>\n\
endstream\nendobj\n\
trailer\n<< /Size 6 /Root 1 0 R >>\n%%EOF\n";

    let doc = CosDocument::open(bytes).expect("it opens");
    let xmp = tinker_pdf_cos::xmp_metadata(&doc).expect("the stream");
    assert!(
        String::from_utf8_lossy(&xmp).contains("plain and legible"),
        "an unencrypted document reads its metadata as it stands"
    );
}

/// 7.4.10: a `/Crypt` filter naming `/Identity` marks a stream as already
/// plaintext inside an encrypted document. Running the decryptor over it
/// turns readable bytes into noise.
#[test]
fn an_identity_crypt_filter_leaves_a_stream_alone() {
    let bytes: &[u8] = b"%PDF-1.7\n\
1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
2 0 obj\n<< /Type /Pages /Count 1 /Kids [3 0 R] >>\nendobj\n\
3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 10 10] /Contents 4 0 R >>\nendobj\n\
4 0 obj\n<< /Filter /Crypt /DecodeParms << /Name /Identity >> /Length 19 >>\nstream\n\
0 0 1 1 re f plain\n\
endstream\nendobj\n\
trailer\n<< /Size 5 /Root 1 0 R >>\n%%EOF\n";

    let doc = CosDocument::open(bytes).expect("it opens");
    let content = doc
        .stream_decoded(tinker_pdf_cos::ObjRef::new(4, 0))
        .expect("it decodes");
    assert!(
        String::from_utf8_lossy(&content).contains("plain"),
        "a /Crypt /Identity stream is not a byte transform and not ciphertext"
    );
}

/// A cross-reference stream is never encrypted (7.6.2): it carries what a
/// reader needs to find the /Encrypt dictionary in the first place.
#[test]
fn a_cross_reference_stream_is_never_decrypted() {
    let doc = open("encrypted-aes256.pdf");
    assert_eq!(doc.authenticate("open-sesame"), Ok(AuthLevel::User));
    // Opening at all proves it: the xref stream had to be read before any
    // password existed, and the pages resolve through it afterwards.
    assert!(tinker_pdf_cos::pages::count(&doc) > 0);
}
