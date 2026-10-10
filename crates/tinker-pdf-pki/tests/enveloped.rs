//! `EnvelopedData` (RFC 5652 §6) against envelopes this crate did not build.
//!
//! The fixtures under `tests/data/enveloped/` were produced once by
//! **OpenSSL 3.5.5** on 28 August 2026, with a throwaway key generated for the
//! purpose, and committed:
//!
//! ```sh
//! openssl req -x509 -newkey rsa:2048 -keyout key.pem -out recipient-cert.pem \
//!   -days 3650 -nodes -subj "/C=GB/O=tinker-pdf interop/CN=One-time signer"
//! printf 'SEEDSEEDSEEDSEEDSEED\x00\x00\x00\x00' > seed.bin
//! openssl cms -encrypt -binary -in seed.bin -outform DER -out <alg>.der \
//!   -<alg> recipient-cert.pem
//! ```
//!
//! Ruling 13 permits exactly this: a third-party program may supply data, and
//! the committed output of a tool run once is a dated measurement. Nothing
//! here invokes OpenSSL and `cargo xtask oracles` would refuse it if it tried.
//!
//! **Why it matters that these came from elsewhere.** Not one of the 4 594
//! fetched corpus files uses the public-key security handler, so the reader
//! has no real documents to be held to. A fixture written by the same author
//! who wrote the parser proves the two agree; a fixture written by OpenSSL
//! proves the parser reads what a different implementation emits, which is a
//! different and better claim. It is the only part of this feature that gets
//! that claim — the ISO 32000-1 7.6.5 key derivation layered on top has no
//! outside evidence at all, and `docs/features/encryption.md` says so.
//!
//! The 20-byte seed is deliberately the ASCII `SEEDSEEDSEEDSEEDSEED` followed
//! by four zero bytes of permissions, which is what 7.6.5's enveloped content
//! is: a seed and a `/P`. Nothing here decrypts it — that needs the private
//! key, which the engine refuses to hold — so what is asserted is the
//! structure around it.

use tinker_pdf_pki::{Certificate, EnvelopedData, EnvelopedError, RecipientIdentifier};

const AES_256: &[u8] = include_bytes!("data/enveloped/aes-256-cbc.der");
const AES_128: &[u8] = include_bytes!("data/enveloped/aes-128-cbc.der");
const TRIPLE_DES: &[u8] = include_bytes!("data/enveloped/des-ede3-cbc.der");

/// `aes-256-cbc` is `2.16.840.1.101.3.4.1.42`, `aes-128-cbc` is `…1.2`, and
/// `des-ede3-cbc` is `1.2.840.113549.3.7`.
fn algorithm_of(der: &[u8]) -> String {
    EnvelopedData::parse(der)
        .expect("parses")
        .content_algorithm()
        .oid()
        .to_dotted()
}

#[test]
fn every_openssl_envelope_parses_to_one_key_transport_recipient() {
    for (name, der) in [
        ("aes-256-cbc", AES_256),
        ("aes-128-cbc", AES_128),
        ("des-ede3-cbc", TRIPLE_DES),
    ] {
        let enveloped = EnvelopedData::parse(der).unwrap_or_else(|error| {
            panic!("{name}: {error}");
        });
        assert_eq!(enveloped.version(), 0, "{name}: §6.1 version");
        assert_eq!(enveloped.recipients().len(), 1, "{name}");
        assert!(
            enveloped.unsupported_recipients().is_empty(),
            "{name}: nothing was skipped"
        );

        let recipient = &enveloped.recipients()[0];
        assert_eq!(
            recipient.version(),
            0,
            "{name}: §6.2.1 ties 0 to issuerAndSerialNumber"
        );
        assert!(recipient.is_rsa(), "{name}: PKCS#1 key transport");
        // A 2 048-bit modulus wraps to exactly 256 bytes.
        assert_eq!(recipient.encrypted_key().len(), 256, "{name}");
        assert!(
            enveloped.encrypted_content().is_some(),
            "{name}: the seed is in the envelope"
        );
    }
}

/// The recipient identifier must be the certificate's own issuer and serial,
/// which is the check that the parse landed on the right fields rather than on
/// fields of the right shapes.
#[test]
fn the_recipient_names_the_certificate_it_was_sealed_to() {
    let pem = include_str!("data/enveloped/recipient-cert.pem");
    let der = pem_to_der(pem);
    let certificate = Certificate::parse(&der).expect("the fixture certificate parses");

    let enveloped = EnvelopedData::parse(AES_256).expect("parses");
    match enveloped.recipients()[0].rid() {
        RecipientIdentifier::IssuerAndSerialNumber { issuer, serial, .. } => {
            assert_eq!(
                *issuer,
                certificate.issuer().der(),
                "the issuer bytes are the certificate's own"
            );
            assert_eq!(
                serial.as_bytes(),
                certificate.serial().as_bytes(),
                "and so is the serial"
            );
        }
        other => panic!("expected issuerAndSerialNumber, got {other:?}"),
    }
}

/// Three content-encryption algorithms, so the field is read rather than
/// assumed. This build can decrypt two of them; 3DES is named and refused
/// where it is met, which is a different place from here.
#[test]
fn the_content_encryption_algorithm_is_read_from_the_envelope() {
    assert_eq!(algorithm_of(AES_256), "2.16.840.1.101.3.4.1.42");
    assert_eq!(algorithm_of(AES_128), "2.16.840.1.101.3.4.1.2");
    assert_eq!(algorithm_of(TRIPLE_DES), "1.2.840.113549.3.7");
}

#[test]
fn a_signed_data_is_refused_by_name_rather_than_misread() {
    // `id-signedData` where `id-envelopedData` belongs: the same `ContentInfo`
    // shell around a structure with entirely different fields, which is
    // exactly the confusion a content-type check exists to stop.
    let mut der = AES_256.to_vec();
    let at = der
        .windows(9)
        .position(|w| w == [0x2A, 0x86, 0x48, 0x86, 0xF7, 0x0D, 0x01, 0x07, 0x03])
        .expect("the content type OID is in there");
    der[at + 8] = 0x02;
    match EnvelopedData::parse(&der) {
        Err(EnvelopedError::UnsupportedContentType { oid }) => {
            assert_eq!(oid, "1.2.840.113549.1.7.2");
        }
        other => panic!("expected a named refusal, got {other:?}"),
    }
}

#[test]
fn truncation_anywhere_is_an_error_and_never_a_panic() {
    for cut in 0..AES_256.len() {
        let _ = EnvelopedData::parse(&AES_256[..cut]);
    }
    for cut in 0..AES_256.len() {
        let mut der = AES_256.to_vec();
        der.truncate(cut);
        der.push(0xFF);
        let _ = EnvelopedData::parse(&der);
    }
}

#[test]
fn every_single_byte_mutation_is_an_error_or_a_parse_and_never_a_panic() {
    // Cheaper than a fuzz session and it runs on every commit: ruling 1 is
    // about not panicking, and a mutation sweep is the smallest thing that
    // exercises that over a real structure.
    let mut mutated = AES_256.to_vec();
    for index in 0..mutated.len() {
        let original = mutated[index];
        for delta in [1u8, 0x7F, 0xFF] {
            mutated[index] = original.wrapping_add(delta);
            let _ = EnvelopedData::parse(&mutated);
        }
        mutated[index] = original;
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

// ---- the writer, held to RFC 5652's envelope, and OpenSSL's to it ----------

/// A deterministic source: what is under test is the encoding, not the
/// randomness.
struct Counter(u8);

impl tinker_pdf_crypto::EntropySource for Counter {
    fn fill(&mut self, out: &mut [u8]) -> bool {
        for byte in out {
            self.0 = self.0.wrapping_add(1);
            *byte = self.0;
        }
        true
    }
}

/// `der` with the three fields sealing fills from randomness zeroed: the
/// encrypted key, the IV and the ciphertext.
fn without_randomness(der: &[u8]) -> Vec<u8> {
    let parsed = EnvelopedData::parse(der).expect("parses");
    let mut ranges = Vec::new();
    let mut at = |slice: &[u8]| {
        let start = slice.as_ptr() as usize - der.as_ptr() as usize;
        ranges.push(start..start + slice.len());
    };
    at(parsed.recipients()[0].encrypted_key());
    at(parsed
        .content_algorithm()
        .parameters()
        .and_then(|node| node.as_octet_string().ok())
        .expect("an IV"));
    at(parsed.encrypted_content().expect("content"));
    let mut out = der.to_vec();
    for range in ranges {
        out[range].fill(0);
    }
    out
}

/// `id-data`, `id-envelopedData` and `aes256-CBC`, as OID contents.
const ID_DATA: &[u8] = &[0x2A, 0x86, 0x48, 0x86, 0xF7, 0x0D, 0x01, 0x07, 0x01];
const ID_ENVELOPED_DATA: &[u8] = &[0x2A, 0x86, 0x48, 0x86, 0xF7, 0x0D, 0x01, 0x07, 0x03];
const AES_256_CBC: &[u8] = &[0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x01, 0x2A];

/// The envelope RFC 5652 §6 and RFC 3565 describe for `content_len` octets
/// sealed to `certificate` alone under AES-256-CBC, written here from the
/// RFCs rather than from anybody's output, with the three fields randomness
/// fills zeroed at the lengths they must have.
///
/// The issuer and serial come out of the certificate by this file's own DER
/// walk, not by the parser the writer uses.
fn rfc_envelope(certificate: &[u8], content_len: usize) -> Vec<u8> {
    let (_, whole) = element(certificate);
    let (_, tbs) = element(children(whole)[0]);
    let tbs = children(tbs);
    // RFC 5280 §4.1: [0] version, serial, signature, issuer, ...
    let (serial, issuer) = (tbs[1], tbs[3]);
    let (_, spki) = element(tbs[6]);
    let (_, key) = element(children(spki)[1]);
    // The BIT STRING's unused-bits octet, then RSAPublicKey; its modulus with
    // the sign octet a DER INTEGER may carry dropped is RFC 8017 §7.2.1's `k`.
    let (_, rsa_key) = element(&key[1..]);
    let (_, modulus) = element(children(rsa_key)[0]);
    let k = modulus.iter().skip_while(|octet| **octet == 0).count();

    // §6.2.1: version 0 because the recipient is named by issuer and serial;
    // the key encrypted under `rsaEncryption` with NULL parameters (RFC 8017
    // A.1), `k` octets of it.
    let recipient = tlv(
        0x30,
        &[
            tlv(0x02, &[0]),
            tlv(0x30, &[issuer, serial].concat()),
            RSA_ENCRYPTION.to_vec(),
            tlv(0x04, &vec![0; k]),
        ]
        .concat(),
    );
    // RFC 3565 §4.1: the IV is the parameter, sixteen octets; §2.3 and RFC
    // 5652 §6.3 pad to the next whole block, a full one when it is already
    // whole. `encryptedContent` is `[0] IMPLICIT OCTET STRING`, primitive.
    let ciphertext_len = (content_len / 16 + 1) * 16;
    let encrypted_content = tlv(
        0x30,
        &[
            tlv(0x06, ID_DATA),
            tlv(
                0x30,
                &[tlv(0x06, AES_256_CBC), tlv(0x04, &[0; 16])].concat(),
            ),
            tlv(0x80, &vec![0; ciphertext_len]),
        ]
        .concat(),
    );
    // §6.1: version 0 — no originator information, no unprotected
    // attributes, every recipient a version-0 key transport.
    let enveloped = tlv(
        0x30,
        &[tlv(0x02, &[0]), tlv(0x31, &recipient), encrypted_content].concat(),
    );
    tlv(
        0x30,
        &[tlv(0x06, ID_ENVELOPED_DATA), tlv(0xA0, &enveloped)].concat(),
    )
}

/// **The writer's envelope is RFC 5652's, octet for octet, outside the three
/// fields randomness fills — and so is OpenSSL's.** The expected encoding is
/// written above from the RFCs. The writer's envelope is held to it, and
/// OpenSSL 3.5.5's `aes-256-cbc.der`, sealed on 28 August 2026 to the same
/// certificate over the same `SEEDSEEDSEEDSEEDSEED` and four zeros, is held
/// to it too: an input checked against the clause rather than the answer
/// (ruling 13). Both checks passing is what makes the writer's envelope
/// OpenSSL's in all 484 octets but the 256-octet encrypted key, the 16-octet
/// IV and the 32 of ciphertext.
#[test]
fn a_sealed_envelope_is_the_rfcs_and_so_is_openssls_outside_their_random_fields() {
    let certificate = pem_to_der(include_str!("data/enveloped/recipient-cert.pem"));
    let content = b"SEEDSEEDSEEDSEEDSEED\0\0\0\0";
    let expected = rfc_envelope(&certificate, content.len());
    assert_eq!(expected.len(), 484);

    let ours =
        tinker_pdf_pki::seal::seal(content, &[&certificate], &mut Counter(0)).expect("seals");
    assert_eq!(
        without_randomness(&ours),
        expected,
        "the writer's envelope is the one RFC 5652 §6 and RFC 3565 describe"
    );
    assert_eq!(
        without_randomness(AES_256),
        expected,
        "OpenSSL's envelope over the same content is the same shape"
    );
    assert_ne!(ours, AES_256, "and the writer's is not a copy of it");
}

// ---- a key its certificate restricts to signing ----------------------------

/// One DER element at the start of `bytes`: its whole encoding and its
/// contents. Test code over committed bytes of known shape.
fn element(bytes: &[u8]) -> (&[u8], &[u8]) {
    let first = bytes[1];
    let (length, header) = if first < 0x80 {
        (usize::from(first), 2)
    } else {
        let count = usize::from(first & 0x7F);
        let length = bytes[2..2 + count]
            .iter()
            .fold(0usize, |acc, byte| acc << 8 | usize::from(*byte));
        (length, 2 + count)
    };
    (&bytes[..header + length], &bytes[header..header + length])
}

/// The elements inside a constructed element's contents.
fn children(mut contents: &[u8]) -> Vec<&[u8]> {
    let mut out = Vec::new();
    while !contents.is_empty() {
        let (whole, _) = element(contents);
        out.push(whole);
        contents = &contents[whole.len()..];
    }
    out
}

/// A DER element with `tag` around `contents`.
fn tlv(tag: u8, contents: &[u8]) -> Vec<u8> {
    let mut out = vec![tag];
    let length = contents.len();
    if length < 0x80 {
        out.push(length as u8);
    } else {
        let octets: Vec<u8> = length
            .to_be_bytes()
            .into_iter()
            .skip_while(|byte| *byte == 0)
            .collect();
        out.push(0x80 | octets.len() as u8);
        out.extend_from_slice(&octets);
    }
    out.extend_from_slice(contents);
    out
}

/// `rsaEncryption` with its `NULL` parameters, as the recipient's
/// `SubjectPublicKeyInfo` carries it.
const RSA_ENCRYPTION: &[u8] = &[
    0x30, 0x0D, 0x06, 0x09, 0x2A, 0x86, 0x48, 0x86, 0xF7, 0x0D, 0x01, 0x01, 0x01, 0x05, 0x00,
];

/// `id-RSASSA-PSS`, `1.2.840.113549.1.1.10`, with its parameters absent —
/// RFC 4055 §1.2's form for a key restricted to RSASSA-PSS and nothing more.
const RSASSA_PSS: &[u8] = &[
    0x30, 0x0B, 0x06, 0x09, 0x2A, 0x86, 0x48, 0x86, 0xF7, 0x0D, 0x01, 0x01, 0x0A,
];

/// `der` with its key published under `id-RSASSA-PSS` instead of
/// `rsaEncryption`: the same `RSAPublicKey`, its owner having limited it to
/// signing. The issuer's signature no longer covers the result, which
/// nothing here asks about.
fn restricted_to_pss(der: &[u8]) -> Vec<u8> {
    let (_, certificate) = element(der);
    let parts = children(certificate);
    let (_, tbs) = element(parts[0]);
    let mut fields: Vec<Vec<u8>> = children(tbs).iter().map(|field| field.to_vec()).collect();
    // RFC 5280 §4.1: [0] version, serial, signature, issuer, validity,
    // subject, subjectPublicKeyInfo, ...
    assert_eq!(fields[0][0], 0xA0, "a version 3 certificate");
    let (_, spki) = element(&fields[6]);
    let spki = children(spki);
    assert_eq!(spki[0], RSA_ENCRYPTION, "the key was unrestricted");
    fields[6] = tlv(0x30, &[RSASSA_PSS, spki[1]].concat());
    let tbs = tlv(0x30, &fields.concat());
    tlv(0x30, &[tbs.as_slice(), parts[1], parts[2]].concat())
}

/// **RFC 4055 §1.2: a key under `id-RSASSA-PSS` is for RSASSA-PSS
/// signatures only**, so nothing is encrypted to it, though its
/// `RSAPublicKey` is exactly the one an `rsaEncryption` key would carry and
/// this crate reads it as RSA either way. `openssl cms -encrypt` refuses such
/// a certificate too; the writer once sealed to it, and wrote `rsaEncryption`
/// in the recipient info.
#[test]
fn a_key_restricted_to_pss_signatures_is_not_sealed_to() {
    let certificate = pem_to_der(include_str!("data/enveloped/recipient-cert.pem"));
    let restricted = restricted_to_pss(&certificate);
    let parsed = Certificate::parse(&restricted).expect("still a certificate");
    assert_eq!(
        parsed.subject_public_key_info().algorithm().oid(),
        tinker_pdf_pki::oid::RSASSA_PSS
    );
    assert!(
        matches!(
            parsed.subject_public_key_info().public_key(),
            tinker_pdf_pki::PublicKey::Rsa { .. }
        ),
        "the same key, read the same way"
    );

    assert_eq!(
        tinker_pdf_pki::seal::seal(b"content", &[&certificate, &restricted], &mut Counter(0)),
        Err(tinker_pdf_pki::seal::SealError::KeyRestricted { index: 1 }),
        "refused by index, before any entropy is spent"
    );
    assert!(
        tinker_pdf_pki::seal::seal(b"content", &[&certificate], &mut Counter(0)).is_ok(),
        "the unrestricted certificate alone still seals"
    );
}
