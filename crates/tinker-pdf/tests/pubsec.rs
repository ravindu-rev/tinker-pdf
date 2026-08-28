//! The public-key security handler (7.6.5), end to end.
//!
//! # Where the fixture came from, and what that is worth
//!
//! `pubsec_support/pubsec-rc4-128.pdf` was built once, on 28 August 2026, by a
//! Python script and **OpenSSL 3.5.5**. OpenSSL sealed a twenty-byte seed to a
//! throwaway certificate as a CMS `EnvelopedData`; the script derived the file
//! key from that seed per ISO 32000-1 7.6.5 and encrypted the document's one
//! string and one stream with per-object RC4 per 7.6.2. The commands are in
//! this repository's history and nothing re-runs them; ruling 13 permits a
//! third-party program to supply data and the committed output of a tool run
//! once is a dated measurement.
//!
//! **The two halves are not equally strong evidence and it matters which is
//! which.**
//!
//! The envelope is OpenSSL's. Reading it exercises `tinker-pdf-pki` against a
//! structure another implementation produced, which is real interop.
//!
//! The key derivation is not. The Python script and `crates/tinker-pdf-cos/
//! src/pubsec.rs` implement 7.6.5 from the same reading of the clause by the
//! same author, so this fixture catches a transcription slip between them and
//! **cannot catch a misreading of the clause**. There was no way to do better:
//! not one of the 4 594 files in the fetched corpora uses this handler, and no
//! tool available here produces one — qpdf 12.3.2 has no public-key support at
//! all. That is the honest position and `docs/features/encryption.md` states
//! it where a caller will see it.
//!
//! # What the stub recipient stands for
//!
//! Unsealing the envelope needs an RSA private-key operation, and this engine
//! holds no key material by design. So the test's `Recipient` is a stub that
//! returns the content-encryption key the generator recorded — which is
//! exactly the division of labour the trait exists for. Everything after the
//! host's one operation is the engine's, and all of it runs here: parsing the
//! envelope, finding the recipient, decrypting the sealed content, deriving
//! the file key, and decrypting the document with it.

use tinker_pdf::{AuthLevel, Document, PubSecError, Recipient};

const FIXTURE: &[u8] = include_bytes!("pubsec_support/pubsec-rc4-128.pdf");
const CONTENT_KEY: &[u8] = include_bytes!("pubsec_support/content-key.bin");

/// The host: it knows one key and says so.
struct Holder {
    key: Vec<u8>,
    /// What it was asked about, so a test can assert the engine handed over
    /// the right identifier rather than merely the right answer.
    seen: std::cell::RefCell<Vec<(usize, bool)>>,
}

impl Holder {
    fn new() -> Holder {
        Holder {
            key: CONTENT_KEY.to_vec(),
            seen: std::cell::RefCell::new(Vec::new()),
        }
    }
}

impl Recipient for Holder {
    fn unseal(
        &self,
        encrypted_key: &[u8],
        issuer_and_serial: Option<&[u8]>,
        subject_key_identifier: Option<&[u8]>,
    ) -> Option<Vec<u8>> {
        self.seen
            .borrow_mut()
            .push((encrypted_key.len(), issuer_and_serial.is_some()));
        assert!(
            subject_key_identifier.is_none(),
            "this envelope identifies by issuer and serial"
        );
        Some(self.key.clone())
    }
}

/// A host that owns nothing.
struct Stranger;

impl Recipient for Stranger {
    fn unseal(&self, _: &[u8], _: Option<&[u8]>, _: Option<&[u8]>) -> Option<Vec<u8>> {
        None
    }
}

#[test]
fn a_public_key_document_opens_for_the_recipient_it_was_sealed_to() {
    let document = Document::open(FIXTURE.to_vec()).expect("it opens as a PDF");
    assert!(document.is_encrypted(), "and it is encrypted");

    let holder = Holder::new();
    assert_eq!(
        document
            .authenticate_with_recipient(&holder)
            .expect("the recipient's key opens it"),
        AuthLevel::User,
        "the public-key handler has no owner tier"
    );

    // The engine asked about the one recipient in the one envelope, and gave
    // it an issuer-and-serial identifier rather than nothing.
    let seen = holder.seen.borrow();
    assert_eq!(seen.len(), 1, "one recipient, asked once");
    assert_eq!(seen[0].0, 256, "a 2 048-bit modulus seals to 256 bytes");
    assert!(seen[0].1, "identified by issuer and serial");
}

/// The whole point: the derived key actually decrypts the document.
///
/// Reading the text back is the assertion that ties every layer together — a
/// wrong file key produces a page of noise rather than an error, so a test
/// that only checked `authenticate_with_recipient` returned `Ok` would pass
/// with the derivation completely wrong.
#[test]
fn the_derived_key_decrypts_the_content_stream_and_the_metadata() {
    let document = Document::open(FIXTURE.to_vec()).expect("opens");
    document
        .authenticate_with_recipient(&Holder::new())
        .expect("authenticates");

    let page = document.page(0).expect("one page");
    let text = page.text().plain_text();
    assert!(
        text.contains("Public key"),
        "the content stream decrypted to text, not noise: {text:?}"
    );

    assert_eq!(
        document.metadata().title.as_deref(),
        Some("PubSec fixture"),
        "and so did the string in /Info"
    );
}

#[test]
fn a_stranger_is_told_the_document_is_not_theirs() {
    let document = Document::open(FIXTURE.to_vec()).expect("opens");
    assert_eq!(
        document.authenticate_with_recipient(&Stranger),
        Err(PubSecError::NoMatchingRecipient),
        "not `WrongPassword`, and not a panic: there is no password here"
    );
}

/// Before authenticating, the document reads as encrypted and its strings do
/// not come back as text. The check that the fixture is really encrypted
/// rather than merely claiming to be.
#[test]
fn the_fixture_is_actually_encrypted() {
    let document = Document::open(FIXTURE.to_vec()).expect("opens");
    assert_ne!(
        document.metadata().title.as_deref(),
        Some("PubSec fixture"),
        "the title must be ciphertext until a key is offered"
    );
}

/// A password is the wrong door, and saying so is the point: `WrongPassword`
/// would send a caller looking for a better password, and there is not one.
#[test]
fn offering_a_password_to_a_public_key_document_is_refused_by_name() {
    let document = Document::open(FIXTURE.to_vec()).expect("opens");
    assert_eq!(
        document.authenticate(""),
        Err(tinker_pdf::AuthError::UnsupportedHandler)
    );
    assert_eq!(
        document.authenticate("anything"),
        Err(tinker_pdf::AuthError::UnsupportedHandler)
    );
}

/// A key that unseals but is the wrong one must not produce a document that
/// looks decrypted. It cannot be caught at authentication — 7.6.5 has no
/// verifier the way the standard handler's `/U` is — so what this pins is that
/// the failure stays visible as garbage rather than becoming a plausible read.
#[test]
fn a_wrong_content_key_does_not_yield_readable_text() {
    struct Wrong;
    impl Recipient for Wrong {
        fn unseal(&self, _: &[u8], _: Option<&[u8]>, _: Option<&[u8]>) -> Option<Vec<u8>> {
            Some(vec![0x00; 32])
        }
    }
    let document = Document::open(FIXTURE.to_vec()).expect("opens");
    // The unsealed content will not be 24 bytes of anything sensible, so this
    // is refused before a key is ever derived.
    let outcome = document.authenticate_with_recipient(&Wrong);
    assert!(
        matches!(outcome, Err(PubSecError::SeedWrongLength { .. }) | Ok(_)),
        "got {outcome:?}"
    );
    if outcome.is_ok() {
        let text = document.page(0).expect("a page").text().plain_text();
        assert!(
            !text.contains("Public key"),
            "a wrong key must not read as the right one"
        );
    }
}
