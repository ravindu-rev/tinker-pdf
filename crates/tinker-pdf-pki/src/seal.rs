//! Sealing content to certificates: CMS `EnvelopedData` (RFC 5652 §6),
//! **written** — the one structure this crate encodes as well as reads.
//!
//! A public-key encrypted document puts the key that opens it inside an
//! `EnvelopedData` for each group of recipients (ISO 32000-2 7.6.5), and
//! writing one is the public half of key transport: a fresh content key,
//! encrypted to each recipient's RSA key with RSAES-PKCS1-v1_5, and the
//! content encrypted under it with AES-256-CBC. No private key is involved
//! anywhere, which is why this belongs beside the reader rather than outside
//! the engine with the signing key.
//!
//! # Exactly the shape OpenSSL writes
//!
//! The encoding is the one `openssl cms -encrypt -aes256` emits, field for
//! field, because that is the shape [`crate::enveloped`] is held to and the
//! shape every reader of these documents has met:
//!
//! ```text
//! ContentInfo { id-envelopedData, [0] EnvelopedData {
//!     version 0,
//!     SET OF KeyTransRecipientInfo {
//!         version 0, issuerAndSerialNumber, rsaEncryption NULL, encryptedKey },
//!     EncryptedContentInfo {
//!         id-data, aes256-CBC with its 16-octet IV, [0] IMPLICIT ciphertext } } }
//! ```
//!
//! Version 0 is RFC 5652 §6.1's for exactly this content: no originator
//! information, no unprotected attributes, every recipient a version-0 key
//! transport identified by issuer and serial number. The recipients' `SET OF`
//! is sorted by encoding, as X.690 §11.6 requires of DER.
//!
//! # Randomness is the caller's
//!
//! The content key, the IV and every recipient's padding come from an
//! [`EntropySource`] the caller supplies, for the reason `tinker-pdf-crypto`
//! gives for its own: this tree has no source of randomness and will not
//! pretend to one. A source that cannot fill is a refusal, never a weaker key.

use tinker_pdf_crypto::rsa::RsaPublicKey;
use tinker_pdf_crypto::EntropySource;

use crate::oid;
use crate::x509::{Certificate, PublicKey};

/// Why content could not be sealed.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum SealError {
    /// No recipient was given: an envelope nobody can open.
    NoRecipients,
    /// A recipient's certificate would not parse.
    Certificate {
        /// Which recipient, counting from zero.
        index: usize,
        /// The parser's reason, rendered.
        reason: String,
    },
    /// A recipient's key is not RSA. Key transport here is RSAES-PKCS1-v1_5
    /// and nothing else, which is every PDF public-key handler's.
    NotRsa {
        /// Which recipient.
        index: usize,
    },
    /// A recipient's RSA key cannot be used — too wide for this build, or too
    /// narrow to carry a 32-octet content key.
    KeyUnusable {
        /// Which recipient.
        index: usize,
    },
    /// A recipient's RSA key is published under `id-RSASSA-PSS`, which RFC
    /// 4055 §1.2 says restricts it to RSASSA-PSS signatures: its holder
    /// declared that nothing is to be encrypted to it. Only an
    /// `rsaEncryption` key is unrestricted.
    KeyRestricted {
        /// Which recipient.
        index: usize,
    },
    /// The entropy source could not supply what sealing needs.
    NoEntropy,
}

impl core::fmt::Display for SealError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::NoRecipients => f.write_str("no recipients to seal to"),
            Self::Certificate { index, reason } => {
                write!(f, "recipient {index}'s certificate: {reason}")
            }
            Self::NotRsa { index } => write!(f, "recipient {index}'s key is not RSA"),
            Self::KeyUnusable { index } => {
                write!(f, "recipient {index}'s RSA key cannot carry a content key")
            }
            Self::KeyRestricted { index } => {
                write!(
                    f,
                    "recipient {index}'s RSA key is restricted to RSASSA-PSS signatures"
                )
            }
            Self::NoEntropy => f.write_str("the entropy source declined"),
        }
    }
}

impl std::error::Error for SealError {}

/// AES-256-CBC (RFC 3565 §4.1), `2.16.840.1.101.3.4.1.42`.
const AES_256_CBC: &[u8] = &[0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x01, 0x2A];

/// How many times one zero octet of PKCS#1 padding is redrawn before the
/// source is taken to be broken: a working source returns a non-zero octet
/// 255 times in 256, so 64 zeros running is a source returning constants.
const REDRAWS: usize = 64;

/// Seals `content` to every certificate in `recipients` (each its DER), in
/// one `EnvelopedData` they can all open, and returns the `ContentInfo`'s DER.
///
/// # Errors
///
/// [`SealError`], checked recipient by recipient before any randomness is
/// spent.
pub fn seal(
    content: &[u8],
    recipients: &[&[u8]],
    entropy: &mut dyn EntropySource,
) -> Result<Vec<u8>, SealError> {
    if recipients.is_empty() {
        return Err(SealError::NoRecipients);
    }
    // Every recipient read and checked first, so a bad one costs no entropy
    // and leaves nothing half-sealed.
    let mut keys = Vec::with_capacity(recipients.len());
    for (index, der) in recipients.iter().enumerate() {
        let certificate = Certificate::parse(der).map_err(|error| SealError::Certificate {
            index,
            reason: error.to_string(),
        })?;
        let PublicKey::Rsa { modulus, exponent } =
            certificate.subject_public_key_info().public_key()
        else {
            return Err(SealError::NotRsa { index });
        };
        // RFC 4055 §1.2: the same `RSAPublicKey` under `id-RSASSA-PSS` is a key
        // whose owner limited it to RSASSA-PSS, and `rsaEncryption` is the one
        // OID that leaves it free for key transport. `openssl cms -encrypt`
        // refuses such a certificate for the same reason.
        if certificate.subject_public_key_info().algorithm().oid() != oid::RSA_ENCRYPTION {
            return Err(SealError::KeyRestricted { index });
        }
        let key =
            RsaPublicKey::new(modulus, exponent).map_err(|_| SealError::KeyUnusable { index })?;
        let padding = key
            .pkcs1_v15_padding_len(32)
            .ok_or(SealError::KeyUnusable { index })?;
        let identifier = sequence(&[
            certificate.issuer().der().to_vec(),
            tlv(0x02, certificate.serial().as_bytes()),
        ]);
        keys.push((key, padding, identifier));
    }

    let mut content_key = [0u8; 32];
    let mut iv = [0u8; 16];
    if !entropy.fill(&mut content_key) || !entropy.fill(&mut iv) {
        return Err(SealError::NoEntropy);
    }

    let mut infos = Vec::with_capacity(keys.len());
    for (index, (key, padding_len, identifier)) in keys.iter().enumerate() {
        let padding = nonzero(entropy, *padding_len)?;
        let encrypted = key
            .encrypt_pkcs1_v15(&content_key, &padding)
            .map_err(|_| SealError::KeyUnusable { index })?;
        infos.push(sequence(&[
            tlv(0x02, &[0]),
            identifier.clone(),
            sequence(&[tlv(0x06, oid::RSA_ENCRYPTION.as_bytes()), tlv(0x05, &[])]),
            tlv(0x04, &encrypted),
        ]));
    }
    // X.690 §11.6: a DER `SET OF` is in ascending order of its members'
    // encodings, compared as octet strings.
    infos.sort();

    // RFC 3565 §2.3: CBC with the PKCS#7 padding RFC 5652 §6.3 specifies.
    let sealed = tinker_pdf_crypto::aes::cbc_encrypt_with_iv_prefix(&content_key, &iv, content)
        .ok_or(SealError::NoEntropy)?;
    let ciphertext = sealed.get(16..).unwrap_or(&[]);

    let encrypted_content_info = sequence(&[
        tlv(0x06, oid::ID_DATA.as_bytes()),
        sequence(&[tlv(0x06, AES_256_CBC), tlv(0x04, &iv)]),
        // `encryptedContent [0] IMPLICIT OCTET STRING`: primitive, context 0.
        tlv(0x80, ciphertext),
    ]);
    let enveloped = sequence(&[
        tlv(0x02, &[0]),
        tlv(0x31, &infos.concat()),
        encrypted_content_info,
    ]);
    Ok(sequence(&[
        tlv(0x06, oid::ID_ENVELOPED_DATA.as_bytes()),
        tlv(0xA0, &enveloped),
    ]))
}

/// `count` octets from `entropy`, none of them zero: RSAES-PKCS1-v1_5's
/// `PS`. A zero is redrawn rather than replaced with a constant, so the
/// padding stays as unpredictable as the source.
fn nonzero(entropy: &mut dyn EntropySource, count: usize) -> Result<Vec<u8>, SealError> {
    let mut out = vec![0u8; count];
    if !entropy.fill(&mut out) {
        return Err(SealError::NoEntropy);
    }
    for slot in &mut out {
        let mut tries = 0;
        while *slot == 0 {
            if tries == REDRAWS {
                return Err(SealError::NoEntropy);
            }
            let mut one = [0u8; 1];
            if !entropy.fill(&mut one) {
                return Err(SealError::NoEntropy);
            }
            *slot = one[0];
            tries += 1;
        }
    }
    Ok(out)
}

/// One DER node: `tag`, the definite length in its shortest form (X.690
/// §10.1), and `content`.
fn tlv(tag: u8, content: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(content.len() + 6);
    out.push(tag);
    let length = content.len();
    if length < 0x80 {
        // Below 128 by the branch, so it fits the one octet.
        out.push(length as u8);
    } else {
        let octets = length.to_be_bytes();
        let skip = octets.iter().take_while(|&&octet| octet == 0).count();
        let significant = octets.get(skip..).unwrap_or(&[]);
        // At most eight octets of `usize`, so the count fits seven bits.
        out.push(0x80 | significant.len() as u8);
        out.extend_from_slice(significant);
    }
    out.extend_from_slice(content);
    out
}

fn sequence(parts: &[Vec<u8>]) -> Vec<u8> {
    tlv(0x30, &parts.concat())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::enveloped::{EnvelopedData, RecipientIdentifier};

    /// A deterministic source, so a test can say exactly what was sealed.
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

    /// Returns zeros forever: a broken source, not a weak one.
    struct Zeros;

    impl EntropySource for Zeros {
        fn fill(&mut self, out: &mut [u8]) -> bool {
            out.fill(0);
            true
        }
    }

    /// The 2048-bit RSA certificate OpenSSL's committed envelopes were sealed
    /// to, out of its PEM.
    fn recipient() -> Vec<u8> {
        const PEM: &str = include_str!("../tests/data/enveloped/recipient-cert.pem");
        let body: String = PEM
            .lines()
            .filter(|line| !line.starts_with("-----"))
            .collect();
        base64(&body)
    }

    /// RFC 4648 §4, for the one PEM above.
    fn base64(text: &str) -> Vec<u8> {
        const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut out = Vec::new();
        let mut buffer = 0u32;
        let mut bits = 0;
        for byte in text.bytes().filter(|byte| *byte != b'=') {
            let Some(value) = ALPHABET.iter().position(|a| *a == byte) else {
                continue;
            };
            buffer = (buffer << 6) | value as u32;
            bits += 6;
            if bits >= 8 {
                bits -= 8;
                out.push((buffer >> bits) as u8);
                buffer &= (1 << bits) - 1;
            }
        }
        out
    }

    #[test]
    fn the_length_octets_are_the_shortest_form() {
        assert_eq!(tlv(0x04, &[]), [0x04, 0x00]);
        assert_eq!(&tlv(0x04, &[0; 127])[..2], [0x04, 0x7F]);
        assert_eq!(&tlv(0x04, &[0; 128])[..3], [0x04, 0x81, 0x80]);
        assert_eq!(&tlv(0x04, &[0; 256])[..4], [0x04, 0x82, 0x01, 0x00]);
        assert_eq!(
            &tlv(0x04, &[0; 65_536])[..5],
            [0x04, 0x83, 0x01, 0x00, 0x00]
        );
    }

    #[test]
    fn a_sealed_envelope_reads_back_as_what_it_was_sealed_to() {
        let certificate_der = recipient();
        let certificate = Certificate::parse(&certificate_der).expect("the certificate parses");
        let der = seal(
            b"SEEDSEEDSEEDSEEDSEED\0\0\0\0",
            &[&certificate_der],
            &mut Counter(0),
        )
        .expect("seals");
        let parsed = EnvelopedData::parse(&der).expect("the reader reads what the writer wrote");
        assert_eq!(parsed.recipients().len(), 1);
        let recipient = &parsed.recipients()[0];
        assert!(recipient.is_rsa());
        match recipient.rid() {
            RecipientIdentifier::IssuerAndSerialNumber { issuer, serial, .. } => {
                assert_eq!(*issuer, certificate.issuer().der());
                assert_eq!(serial.as_bytes(), certificate.serial().as_bytes());
            }
            other => panic!("expected issuer and serial, got {other:?}"),
        }
        assert_eq!(recipient.encrypted_key().len(), 256, "a 2048-bit modulus");
        assert_eq!(parsed.content_algorithm().oid().as_bytes(), AES_256_CBC);
        assert_eq!(
            parsed.encrypted_content().map(<[u8]>::len),
            Some(32),
            "24 octets padded to two blocks"
        );
    }

    #[test]
    fn two_recipients_share_one_envelope_in_der_order() {
        let a = recipient();
        let der = seal(b"content", &[&a, &a], &mut Counter(7)).expect("seals");
        let parsed = EnvelopedData::parse(&der).expect("parses");
        assert_eq!(parsed.recipients().len(), 2);
        let keys: Vec<&[u8]> = parsed
            .recipients()
            .iter()
            .map(|recipient| recipient.encrypted_key())
            .collect();
        assert_ne!(keys[0], keys[1], "each padded with its own draw");
        assert!(keys[0] < keys[1], "a DER SET OF is sorted");
    }

    #[test]
    fn each_refusal_is_named_and_none_spends_entropy() {
        assert_eq!(
            seal(b"x", &[], &mut Counter(0)),
            Err(SealError::NoRecipients)
        );
        assert!(matches!(
            seal(b"x", &[&[0x30, 0x00]], &mut Counter(0)),
            Err(SealError::Certificate { index: 0, .. })
        ));
        let good = recipient();
        assert_eq!(seal(b"x", &[&good], &mut Dry), Err(SealError::NoEntropy));
        assert_eq!(seal(b"x", &[&good], &mut Zeros), Err(SealError::NoEntropy));
    }
}
