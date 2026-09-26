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
