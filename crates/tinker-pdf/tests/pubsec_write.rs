//! Public-key encryption on write (ISO 32000-2 7.6.5): a document sealed to
//! caller-supplied certificates, opened again by the holder of a key.
//!
//! # Which link is adjudicated by what
//!
//! **The envelope is held to OpenSSL's.** `tinker-pdf-pki`'s
//! `a_sealed_envelope_is_openssls_outside_its_random_fields` seals the same
//! content OpenSSL 3.5.5 did to the same certificate and finds the same 484
//! octets outside the encrypted key, the IV and the ciphertext; this file does
//! not re-prove that.
//!
//! **The arithmetic is published vectors'.** RSAES-PKCS1-v1_5 is held to all
//! 300 of RSA Laboratories' encryption known answers, AES-256-CBC to FIPS 197
//! and SP 800-38A.
//!
//! **The key holder is this test, and so is the 7.6.5 derivation on both
//! sides.** Unsealing needs the RSA private key, which the engine refuses to
//! hold, so the [`Recipient`] here raises the encrypted key to the committed
//! `visible-signer-key.der`'s private exponent with `tinker_pdf_crypto::
//! bignum` — the same arrangement `visible_signature.rs` signs with. And the
//! file key the writer seals and the reader derives are one function,
//! `pubsec::derive`, so a misreading of the clause would be shared; that is
//! the risk `docs/design/pubsec.md` already records, and nothing written here
//! closes it.

use std::cell::RefCell;

use tinker_pdf::{
    AuthError, AuthLevel, Document, EntropySource, PubSecError, PublicKeyEncryption, Recipient,
    SealError, WriteMode, WriteOptions,
};
use tinker_pdf_crypto::bignum::{Modulus, Uint};

const SIMPLE_TEXT: &[u8] = include_bytes!("../../../testdata/simple-text.pdf");
const CERTIFICATE: &[u8] = include_bytes!("signature_support/visible-signer.der");
const KEY: &[u8] = include_bytes!("signature_support/visible-signer-key.der");
/// An EC certificate, which no key transport here can seal to.
const EC_CERTIFICATE: &[u8] = include_bytes!("signature_support/ecdsa-p256-root.der");
/// A second RSA certificate, whose key nobody kept: OpenSSL's envelope
/// fixtures' recipient.
const OTHER_PEM: &str =
    include_str!("../../tinker-pdf-pki/tests/data/enveloped/recipient-cert.pem");

/// A deterministic source, which is what a test wants and what nothing else
/// should use: a document sealed with it is as predictable as its counter.
struct Counter(u8);

impl EntropySource for Counter {
    fn fill(&mut self, out: &mut [u8]) -> bool {
        for byte in out {
            self.0 = self.0.wrapping_add(1);
            *byte = self.0;
        }
        true
    }
}

struct Dry;

impl EntropySource for Dry {
    fn fill(&mut self, _: &mut [u8]) -> bool {
        false
    }
}

/// One DER element: its whole encoding and its contents. Test code over the
/// committed key, whose shape is known.
fn element(bytes: &[u8]) -> (&[u8], &[u8]) {
    let first = bytes[1];
    let (length, header) = if first < 0x80 {
        (usize::from(first), 2)
    } else {
        let count = usize::from(first & 0x7F);
        let mut length = 0usize;
        for byte in &bytes[2..2 + count] {
            length = length << 8 | usize::from(*byte);
        }
        (length, 2 + count)
    };
    (&bytes[..header + length], &bytes[header..header + length])
}

fn children(mut contents: &[u8]) -> Vec<&[u8]> {
    let mut out = Vec::new();
    while !contents.is_empty() {
        let (whole, _) = element(contents);
        out.push(whole);
        contents = &contents[whole.len()..];
    }
    out
}

/// The holder of `visible-signer-key.der`: RSAES-PKCS1-v1_5 decryption with
/// the private exponent (RFC 8017 §7.2.2), done here because the engine will
/// not.
struct KeyHolder {
    modulus: Modulus<32>,
    private_exponent: Uint<32>,
    bytes: usize,
    /// The identifiers it was offered, so a test can assert the engine handed
    /// over the right one rather than merely that the key worked.
    offered: RefCell<Vec<Vec<u8>>>,
}

impl KeyHolder {
    fn committed() -> KeyHolder {
        // RFC 8017 A.1.2: version, n, e, d, ...
        let (_, key) = element(KEY);
        let fields = children(key);
        let n = element(fields[1]).1;
        let d = element(fields[3]).1;
        let modulus = Modulus::new(Uint::from_be_bytes(n).expect("a 2048-bit modulus"))
            .expect("an odd modulus");
        KeyHolder {
            bytes: modulus.byte_len(),
            modulus,
            private_exponent: Uint::from_be_bytes(d).expect("d fits"),
            offered: RefCell::new(Vec::new()),
        }
    }
}

impl Recipient for KeyHolder {
    fn unseal(
        &self,
        encrypted_key: &[u8],
        issuer_and_serial: Option<&[u8]>,
        _subject_key_identifier: Option<&[u8]>,
    ) -> Option<Vec<u8>> {
        self.offered
            .borrow_mut()
            .push(issuer_and_serial.unwrap_or_default().to_vec());
        let c = Uint::from_be_bytes(encrypted_key)?;
        let m = self.modulus.pow(&c, &self.private_exponent);
        let mut block = vec![0u8; self.bytes];
        if !m.to_be_bytes(&mut block) {
            return None;
        }
        // 0x00 0x02 PS 0x00 M, with at least eight octets of PS. Anything
        // else is a key that was not sealed to this holder.
        if block.first() != Some(&0) || block.get(1) != Some(&2) {
            return None;
        }
        let end = block.iter().skip(2).position(|&byte| byte == 0)? + 2;
        if end < 10 {
            return None;
        }
        Some(block[end + 1..].to_vec())
    }
}

struct Stranger;

impl Recipient for Stranger {
    fn unseal(&self, _: &[u8], _: Option<&[u8]>, _: Option<&[u8]>) -> Option<Vec<u8>> {
        None
    }
}

fn pem_to_der(pem: &str) -> Vec<u8> {
    let body: String = pem
        .lines()
        .filter(|line| !line.starts_with("-----"))
        .collect();
    let table = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = Vec::new();
    let (mut acc, mut bits) = (0u32, 0u32);
    for byte in body.bytes() {
        let Some(value) = table.iter().position(|c| *c == byte) else {
            continue;
        };
        acc = (acc << 6) | value as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
    }
    out
}

/// "Print, copy" and nothing else, as `/P` stores it.
const PERMISSIONS: i32 = -3392;

fn sealed_to(certificates: &[Vec<u8>]) -> PublicKeyEncryption {
    PublicKeyEncryption::seal(certificates, PERMISSIONS, &mut Counter(0)).expect("seals")
}

fn saved(sealed: &PublicKeyEncryption, options: &WriteOptions) -> Vec<u8> {
    Document::open(SIMPLE_TEXT.to_vec())
        .expect("the fixture opens")
        .editor()
        .save_sealed(options, sealed)
        .expect("a sealed rewrite")
}

fn text_of(document: &Document) -> String {
    document.page(0).expect("a page").text().plain_text()
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len())
        .position(|window| window == needle)
}

#[test]
fn a_document_sealed_to_a_certificate_opens_for_its_key_holder() {
    let original = text_of(&Document::open(SIMPLE_TEXT.to_vec()).expect("opens"));
    assert!(
        !original.trim().is_empty(),
        "the fixture has text to compare"
    );

    let out = saved(
        &sealed_to(&[CERTIFICATE.to_vec()]),
        &WriteOptions::default(),
    );
    let document = Document::open(out).expect("the sealed file opens as a PDF");
    assert!(document.is_encrypted());

    let holder = KeyHolder::committed();
    assert_eq!(
        document.authenticate_with_recipient(&holder),
        Ok(AuthLevel::User)
    );
    assert_eq!(
        text_of(&document),
        original,
        "every string and stream decrypts"
    );
    assert_eq!(document.permissions().raw(), PERMISSIONS);

    // The engine named the recipient by the certificate's own issuer and
    // serial, the identifier the key holder matches on.
    let certificate = tinker_pdf_pki::Certificate::parse(CERTIFICATE).expect("parses");
    let offered = holder.offered.borrow();
    assert_eq!(offered.len(), 1);
    let (_, identifier) = element(&offered[0]);
    let parts = children(identifier);
    assert_eq!(parts[0], certificate.issuer().der());
}

#[test]
fn the_file_is_sealed_and_says_how() {
    let out = saved(
        &sealed_to(&[CERTIFICATE.to_vec()]),
        &WriteOptions::default(),
    );
    let original = text_of(&Document::open(SIMPLE_TEXT.to_vec()).expect("opens"));
    let word = original
        .split_whitespace()
        .find(|word| word.len() >= 4)
        .expect("a word to look for");
    assert!(
        find(&out, word.as_bytes()).is_none(),
        "{word:?} must not be in the file in the clear"
    );

    let document = Document::open(out).expect("opens");
    let cos = document.cos();
    let encrypt = cos.resolve_key(cos.trailer(), cos.intern(b"Encrypt"));
    let encrypt = encrypt.as_dict().expect("/Encrypt");
    let name = |key: &[u8]| {
        encrypt
            .get_name(cos.intern(key))
            .and_then(|name| cos.name_bytes(name))
            .map(|bytes| bytes.to_vec())
    };
    assert_eq!(name(b"Filter").as_deref(), Some(&b"Adobe.PubSec"[..]));
    assert_eq!(name(b"SubFilter").as_deref(), Some(&b"adbe.pkcs7.s5"[..]));
    assert_eq!(name(b"StmF").as_deref(), Some(&b"DefaultCryptFilter"[..]));
    assert_eq!(
        encrypt.get(cos.intern(b"V")).and_then(|v| v.as_int()),
        Some(5)
    );
    let cf = cos.resolve_key(encrypt, cos.intern(b"CF"));
    let filter = cos.resolve_key(
        cf.as_dict().expect("/CF"),
        cos.intern(b"DefaultCryptFilter"),
    );
    let filter = filter.as_dict().expect("the crypt filter");
    let method = filter
        .get_name(cos.intern(b"CFM"))
        .and_then(|name| cos.name_bytes(name));
    assert_eq!(method.as_deref(), Some(&b"AESV3"[..]));
    let recipients = cos.resolve_key(filter, cos.intern(b"Recipients"));
    let recipients = recipients.as_array().expect("/Recipients");
    assert_eq!(recipients.len(), 1, "one envelope for one group");
    let envelope = &recipients[0].as_string().expect("a string").bytes;
    assert!(tinker_pdf_pki::EnvelopedData::parse(envelope).is_ok());
}

/// The envelope opened by the key holder's own hands, to what 7.6.5 says a
/// recipient finds there: twenty octets of seed and then `/P`'s four, most
/// significant first. The engine's reader takes the seed and leaves the
/// permission octets alone (`docs/design/pubsec.md`), so this is the only
/// test that reads them.
#[test]
fn the_envelope_carries_the_seed_and_then_the_permissions_most_significant_first() {
    let sealed = sealed_to(&[CERTIFICATE.to_vec()]);
    let envelope = tinker_pdf_pki::EnvelopedData::parse(&sealed.recipients()[0]).expect("parses");
    let recipient = &envelope.recipients()[0];
    let content_key = KeyHolder::committed()
        .unseal(recipient.encrypted_key(), None, None)
        .expect("addressed to the holder");
    assert_eq!(content_key.len(), 32, "an AES-256 content key");

    let iv = envelope
        .content_algorithm()
        .parameters()
        .expect("RFC 3565's IV")
        .as_octet_string()
        .expect("an octet string");
    let mut buffer = iv.to_vec();
    buffer.extend_from_slice(
        envelope
            .encrypted_content()
            .expect("carried in the envelope"),
    );
    let (content, notes) =
        tinker_pdf_crypto::aes::cbc_decrypt_with_iv_prefix(&content_key, &buffer);
    assert!(notes.is_empty(), "{notes:?}");
    assert_eq!(content.len(), 24, "seed and permissions, nothing else");
    assert_eq!(content[20..], PERMISSIONS.to_be_bytes());
}

/// The `/ID` of a sealed file is mixed with the sealing's own entropy, as a
/// password-encrypted rewrite's is, so that it cannot confirm a guess at the
/// plaintext to someone who has the plaintext and not the key.
#[test]
fn the_identifier_is_mixed_with_the_sealings_entropy() {
    let identifier = |sealed: &PublicKeyEncryption| {
        let document = Document::open(saved(sealed, &WriteOptions::default())).expect("opens");
        let cos = document.cos();
        let id = cos
            .trailer()
            .get_array(cos.intern(b"ID"))
            .expect("/ID")
            .to_vec();
        id.iter()
            .map(|part| part.as_string().expect("a string").bytes.clone())
            .collect::<Vec<_>>()
    };
    let a = identifier(&sealed_to(&[CERTIFICATE.to_vec()]));
    let b = identifier(
        &PublicKeyEncryption::seal(&[CERTIFICATE.to_vec()], PERMISSIONS, &mut Counter(100))
            .expect("seals"),
    );
    assert_eq!(a.len(), 2);
    assert_ne!(a[0], b[0], "the permanent identifier");
    assert_ne!(a[1], b[1], "the changing one");
}

#[test]
fn a_stranger_is_told_the_document_is_not_theirs() {
    let out = saved(
        &sealed_to(&[CERTIFICATE.to_vec()]),
        &WriteOptions::default(),
    );
    let document = Document::open(out).expect("opens");
    assert_eq!(
        document.authenticate_with_recipient(&Stranger),
        Err(PubSecError::NoMatchingRecipient)
    );
    assert!(
        matches!(
            document.authenticate(""),
            Err(AuthError::UnsupportedHandler)
        ),
        "a password is no use against recipients"
    );
}

#[test]
fn every_recipient_is_in_the_one_envelope_and_each_opens_it() {
    let other = pem_to_der(OTHER_PEM);
    let sealed = sealed_to(&[other.clone(), CERTIFICATE.to_vec()]);
    let envelope = tinker_pdf_pki::EnvelopedData::parse(&sealed.recipients()[0]).expect("parses");
    assert_eq!(envelope.recipients().len(), 2);

    let document = Document::open(saved(&sealed, &WriteOptions::default())).expect("opens");
    let holder = KeyHolder::committed();
    assert_eq!(
        document.authenticate_with_recipient(&holder),
        Ok(AuthLevel::User),
        "found among two"
    );
}

#[test]
fn sealing_composes_with_object_streams_and_a_linearized_layout() {
    let original = text_of(&Document::open(SIMPLE_TEXT.to_vec()).expect("opens"));
    for options in [
        WriteOptions {
            object_streams: true,
            compress: true,
            ..WriteOptions::default()
        },
        WriteOptions {
            linearize: true,
            ..WriteOptions::default()
        },
    ] {
        let out = saved(&sealed_to(&[CERTIFICATE.to_vec()]), &options);
        let document = Document::open(out).expect("opens");
        assert_eq!(
            document.authenticate_with_recipient(&KeyHolder::committed()),
            Ok(AuthLevel::User)
        );
        assert_eq!(text_of(&document), original, "{options:?}");
    }
}

#[test]
fn the_same_entropy_seals_the_same_bytes() {
    // Ruling 4: nothing here reads a clock or the platform's randomness, so
    // the same input and the same source are the same file.
    let a = saved(
        &sealed_to(&[CERTIFICATE.to_vec()]),
        &WriteOptions::default(),
    );
    let b = saved(
        &sealed_to(&[CERTIFICATE.to_vec()]),
        &WriteOptions::default(),
    );
    assert_eq!(a, b);
}

#[test]
fn what_cannot_be_sealed_is_refused_by_name_before_anything_is_written() {
    assert!(matches!(
        PublicKeyEncryption::seal(&[], 0, &mut Counter(0)),
        Err(SealError::NoRecipients)
    ));
    assert!(matches!(
        PublicKeyEncryption::seal(&[vec![0x30, 0x00]], 0, &mut Counter(0)),
        Err(SealError::Certificate { index: 0, .. })
    ));
    assert_eq!(
        PublicKeyEncryption::seal(
            &[CERTIFICATE.to_vec(), EC_CERTIFICATE.to_vec()],
            0,
            &mut Counter(0)
        )
        .map(|_| ()),
        Err(SealError::NotRsa { index: 1 })
    );
    assert_eq!(
        PublicKeyEncryption::seal(&[CERTIFICATE.to_vec()], 0, &mut Dry).map(|_| ()),
        Err(SealError::NoEntropy)
    );

    let sealed = sealed_to(&[CERTIFICATE.to_vec()]);
    let editor = Document::open(SIMPLE_TEXT.to_vec())
        .expect("opens")
        .editor();
    assert_eq!(
        editor.save_sealed(
            &WriteOptions {
                mode: WriteMode::Incremental,
                ..WriteOptions::default()
            },
            &sealed
        ),
        Err(SealError::NotRewrite)
    );
    let with_password = WriteOptions {
        encryption: Some(tinker_pdf::Encryption {
            user_password: "a".into(),
            owner_password: "b".into(),
            permissions: -1,
            entropy: [7; 48],
        }),
        ..WriteOptions::default()
    };
    assert_eq!(
        editor.save_sealed(&with_password, &sealed),
        Err(SealError::PasswordAlsoRequested)
    );
}

#[test]
fn the_debug_form_does_not_print_the_key() {
    let sealed = sealed_to(&[CERTIFICATE.to_vec()]);
    let shown = format!("{sealed:?}");
    assert!(shown.contains("PublicKeyEncryption"), "{shown}");
    assert!(!shown.contains("file_key"), "{shown}");
}
