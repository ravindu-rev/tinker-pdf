//! The public-key security handler (7.6.5), `/Filter /Adobe.PubSec`.
//!
//! Where the standard handler asks "does this password produce the file key",
//! this one asks "can you unseal one of these envelopes". The document carries
//! a `/Recipients` array of CMS `EnvelopedData` blobs, one per group of
//! recipients; a recipient unseals the one addressed to them, gets a 20-byte
//! seed and four bytes of permissions, and the file key is a digest over that
//! seed and **every** envelope in the array.
//!
//! # The private key stays outside
//!
//! Unsealing needs an RSA private-key operation, and this engine holds no key
//! material — the same rule signing follows, for the same reasons. So the
//! caller implements [`Recipient`]: it is handed the sealed key and the
//! identifier saying whose it is, and returns the content-encryption key or
//! nothing. `docs/design/pubsec.md` records why that boundary is where it is.
//!
//! # What has no evidence behind it, stated first rather than last
//!
//! **Not one of the 4 594 files in the fetched corpora uses this handler.**
//! Ruling 3 schedules capabilities by corpus hit-rate and this one measures
//! zero; it is built because it was asked for, not because the evidence said
//! so, and the consequence is that its verification is circular in a way the
//! rest of this crate's is not.
//!
//! The layers are worth separating, because they are not equally weak:
//!
//! - **The envelope** is parsed by `tinker-pdf-pki` and is checked against
//!   `EnvelopedData` structures **OpenSSL produced** — real evidence that the
//!   parser reads what another implementation writes.
//! - **The key derivation below** is checked against a second implementation
//!   written from the same clause by the same author. That catches a
//!   transcription slip and cannot catch a misreading, and there is no third
//!   party available to catch one: no tool on the machine this was written on
//!   produces a public-key-encrypted PDF, and the corpus has none.
//!
//! So a document this code opens is a document this code agrees with itself
//! about. [`docs/features/encryption.md`] says so where a caller will see it.

use std::sync::Arc;

use tinker_pdf_crypto::handler::{CryptMethod, FileKey};
use tinker_pdf_crypto::{sha1, sha2, AuthOutcome};

use crate::decrypt::{Decryptor, EncryptParams};
use crate::security::{AuthLevel, Authenticated, StandardDecryptor};

/// The caller's private key, held by the caller.
///
/// One call, because there is only one thing the engine cannot do for itself.
pub trait Recipient {
    /// Unseals `encrypted_key`, or returns `None` if this recipient is not the
    /// one the envelope was addressed to.
    ///
    /// `issuer_and_serial` and `subject_key_identifier` are whichever
    /// identifier the envelope used (RFC 5652 §6.2.1), so an implementation
    /// holding several keys can pick without trial decryption. Exactly one is
    /// `Some`.
    ///
    /// Returning `None` must mean "not mine", not "mine and it failed" — the
    /// two are indistinguishable to the caller of this trait, and a wrong
    /// answer here reads as a document addressed to somebody else.
    fn unseal(
        &self,
        encrypted_key: &[u8],
        issuer_and_serial: Option<&[u8]>,
        subject_key_identifier: Option<&[u8]>,
    ) -> Option<Vec<u8>>;
}

/// Why the public-key handler could not open the document.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PubSecError {
    /// `/Encrypt` names `/Adobe.PubSec` and carries no `/Recipients`, on the
    /// dictionary or on any crypt filter.
    NoRecipients,
    /// Every envelope parsed and none was addressed to a key the caller
    /// offered. The document is somebody else's.
    NoMatchingRecipient,
    /// An envelope would not parse, with the reason.
    UnreadableEnvelope(String),
    /// The envelope's content-encryption algorithm is not one this build
    /// implements. Named rather than skipped, because the difference between
    /// "not yours" and "yours, in a cipher I do not have" is the whole of what
    /// a caller can act on.
    UnsupportedContentCipher {
        /// The algorithm's dotted OID.
        oid: String,
    },
    /// The unsealed content is not the twenty-byte seed plus four bytes of
    /// permissions that 7.6.5 describes.
    SeedWrongLength {
        /// How many bytes came back.
        length: usize,
    },
    /// `/V` or `/Length` describe a key this build cannot derive.
    UnsupportedKeyLength {
        /// The length in bits that was asked for.
        bits: usize,
    },
}

impl core::fmt::Display for PubSecError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            PubSecError::NoRecipients => f.write_str("no /Recipients"),
            PubSecError::NoMatchingRecipient => {
                f.write_str("no envelope was addressed to the key offered")
            }
            PubSecError::UnreadableEnvelope(why) => write!(f, "unreadable envelope: {why}"),
            PubSecError::UnsupportedContentCipher { oid } => {
                write!(f, "content cipher {oid} is not implemented")
            }
            PubSecError::SeedWrongLength { length } => {
                write!(f, "the unsealed content is {length} bytes, not 24")
            }
            PubSecError::UnsupportedKeyLength { bits } => write!(f, "a {bits}-bit file key"),
        }
    }
}

impl std::error::Error for PubSecError {}

/// AES-128-CBC, `2.16.840.1.101.3.4.1.2`.
const AES_128_CBC: &[u8] = &[0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x01, 0x02];
/// AES-256-CBC, `2.16.840.1.101.3.4.1.42`.
const AES_256_CBC: &[u8] = &[0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x01, 0x2A];
/// RC4, `1.2.840.113549.3.4`.
const RC4: &[u8] = &[0x2A, 0x86, 0x48, 0x86, 0xF7, 0x0D, 0x03, 0x04];

/// The envelopes this document carries, wherever it put them.
///
/// Before `/V 4` they are on `/Encrypt`; from `/V 4` a crypt filter carries
/// its own. A file with both is malformed and this prefers the filter's,
/// because the filter is what the stream in front of you is encrypted under.
fn envelopes(params: &EncryptParams) -> &[Vec<u8>] {
    for filter in &params.crypt_filters {
        if !filter.recipients.is_empty() {
            return &filter.recipients;
        }
    }
    &params.recipients
}

/// Unseals whichever envelope the caller has a key for, and returns its
/// content: 7.6.5's twenty-byte seed followed by four bytes of `/P`.
fn unseal(envelopes: &[Vec<u8>], recipient: &dyn Recipient) -> Result<Vec<u8>, PubSecError> {
    use tinker_pdf_pki::{EnvelopedData, RecipientIdentifier};

    let mut unreadable = None;
    for envelope in envelopes {
        let parsed = match EnvelopedData::parse(envelope) {
            Ok(parsed) => parsed,
            Err(error) => {
                unreadable.get_or_insert_with(|| error.to_string());
                continue;
            }
        };
        for candidate in parsed.recipients() {
            let (issuer_and_serial, ski) = match candidate.rid() {
                RecipientIdentifier::IssuerAndSerialNumber { der, .. } => (Some(*der), None),
                RecipientIdentifier::SubjectKeyIdentifier(id) => (None, Some(*id)),
            };
            let Some(content_key) =
                recipient.unseal(candidate.encrypted_key(), issuer_and_serial, ski)
            else {
                continue;
            };
            let Some(sealed) = parsed.encrypted_content() else {
                // §6.1 allows the content to be carried elsewhere; a PDF's
                // `/Recipients` never does that, and inventing where it went
                // is not a reading.
                return Err(PubSecError::SeedWrongLength { length: 0 });
            };
            return decrypt_content(&parsed, &content_key, sealed);
        }
    }
    match unreadable {
        Some(why) => Err(PubSecError::UnreadableEnvelope(why)),
        None => Err(PubSecError::NoMatchingRecipient),
    }
}

fn decrypt_content(
    parsed: &tinker_pdf_pki::EnvelopedData<'_>,
    key: &[u8],
    sealed: &[u8],
) -> Result<Vec<u8>, PubSecError> {
    let algorithm = parsed.content_algorithm();
    let oid = algorithm.oid();
    let plain = if oid.as_bytes() == AES_128_CBC || oid.as_bytes() == AES_256_CBC {
        // RFC 3565: the parameter is the sixteen-byte initialisation vector.
        let iv = algorithm
            .parameters()
            .and_then(|node| node.as_octet_string().ok())
            .and_then(|bytes| <[u8; 16]>::try_from(bytes).ok())
            .ok_or_else(|| PubSecError::UnsupportedContentCipher {
                oid: oid.to_dotted(),
            })?;
        let (plain, _notes) = {
            let mut buffer = Vec::with_capacity(16 + sealed.len());
            buffer.extend_from_slice(&iv);
            buffer.extend_from_slice(sealed);
            tinker_pdf_crypto::aes::cbc_decrypt_with_iv_prefix(key, &buffer)
        };
        plain
    } else if oid.as_bytes() == RC4 {
        tinker_pdf_crypto::rc4::rc4(key, sealed)
    } else {
        // Triple DES is the one real gap: OpenSSL emits it by default for
        // older recipients and this crate has no DES. Named, because a caller
        // meeting it can re-seal with something else, where a silent failure
        // tells them nothing.
        return Err(PubSecError::UnsupportedContentCipher {
            oid: oid.to_dotted(),
        });
    };

    // 7.6.5: twenty bytes of seed and four of permissions. Anything else means
    // the unsealing produced something that is not what the handler expects,
    // which is far likelier to be a wrong key than a novel layout.
    if plain.len() < 24 {
        return Err(PubSecError::SeedWrongLength {
            length: plain.len(),
        });
    }
    Ok(plain)
}

/// 7.6.5's file key: a digest over the seed and every envelope.
///
/// # What this is, precisely
///
/// SHA-1 (or SHA-256 from `/V 5`) is fed, in this order:
///
/// 1. the twenty-byte seed from the unsealed content;
/// 2. **every** `/Recipients` string, in the order the file lists them, in
///    full — the whole DER of each envelope, not a field of it;
/// 3. four bytes of `0xFF`, when `/EncryptMetadata` is false.
///
/// The file key is the first `/Length / 8` bytes of the digest, or all 32 for
/// `/V 5`.
///
/// Step 2 is why [`EncryptParams::recipients`] keeps the stored bytes and
/// their order: a reader that re-serialised an envelope, or sorted the array,
/// would digest different bytes and derive a key that decrypts nothing —
/// with no error anywhere to say why.
fn derive(
    seed: &[u8],
    envelopes: &[Vec<u8>],
    encrypt_metadata: bool,
    version: i64,
    length_bits: usize,
) -> Result<Vec<u8>, PubSecError> {
    let mut tail: Vec<u8> = Vec::new();
    for envelope in envelopes {
        tail.extend_from_slice(envelope);
    }
    if !encrypt_metadata {
        tail.extend_from_slice(&[0xFF; 4]);
    }

    if version >= 5 {
        let mut hasher = sha2::Sha256::new();
        hasher.update(seed);
        hasher.update(&tail);
        return Ok(hasher.finish().to_vec());
    }

    let bytes = length_bits / 8;
    if bytes == 0 || bytes > 20 {
        return Err(PubSecError::UnsupportedKeyLength { bits: length_bits });
    }
    let mut hasher = sha1::Sha1::new();
    hasher.update(seed);
    hasher.update(&tail);
    Ok(hasher.finish()[..bytes].to_vec())
}

/// Which cipher the crypt filters say to use, translated for the key.
fn methods(params: &EncryptParams) -> (CryptMethod, CryptMethod) {
    if params.v.unwrap_or(0) >= 4 {
        (
            crate::security::method_for(params, params.stm_f.as_deref()),
            crate::security::method_for(params, params.str_f.as_deref()),
        )
    } else {
        (CryptMethod::Rc4, CryptMethod::Rc4)
    }
}

/// Opens a `/Adobe.PubSec` document with the caller's key.
///
/// # Errors
/// [`PubSecError`], which distinguishes "not addressed to you" from every
/// other way this can fail.
pub fn authenticate(
    params: &EncryptParams,
    recipient: &dyn Recipient,
) -> Result<Authenticated, PubSecError> {
    let envelopes = envelopes(params);
    if envelopes.is_empty() {
        return Err(PubSecError::NoRecipients);
    }

    let content = unseal(envelopes, recipient)?;
    let seed = &content[..20];
    let version = params.v.unwrap_or(0);
    // 7.6.5's default is 40 bits, as everywhere else in 7.6.
    let length_bits = usize::try_from(params.length.unwrap_or(40)).unwrap_or(40);
    let key = derive(
        seed,
        envelopes,
        params.encrypt_metadata,
        version,
        length_bits,
    )?;

    let (stream_method, string_method) = methods(params);
    let key = FileKey::from_derived(
        key,
        params.r.unwrap_or(if version >= 5 { 6 } else { 4 }),
        stream_method,
        string_method,
        // The public-key handler has no owner password and no second tier: a
        // recipient who can unseal is a recipient, and saying `Owner` would
        // claim an authority the document never granted.
        AuthOutcome::User,
    );
    let decryptor = StandardDecryptor::from_key(key.clone());
    Ok(Authenticated {
        decryptor: Arc::new(decryptor) as Arc<dyn Decryptor>,
        key,
        level: AuthLevel::User,
        notes: Vec::new(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The derivation's shape, independent of any document.
    #[test]
    fn the_key_is_the_digest_of_the_seed_and_every_envelope_in_order() {
        let seed = [0x11u8; 20];
        let a = vec![0xAAu8; 8];
        let b = vec![0xBBu8; 8];

        let mut expected = sha1::Sha1::new();
        expected.update(&seed);
        expected.update(&a);
        expected.update(&b);
        let expected = expected.finish();

        let key = derive(&seed, &[a.clone(), b.clone()], true, 2, 128).expect("derives");
        assert_eq!(key, expected[..16], "128 bits is the first sixteen bytes");

        // Order is load-bearing, and swapping it must change the key.
        let swapped = derive(&seed, &[b, a], true, 2, 128).expect("derives");
        assert_ne!(key, swapped, "the array's order is part of the input");
    }

    #[test]
    fn encrypt_metadata_false_adds_four_bytes_of_ff() {
        let seed = [0x22u8; 20];
        let envelope = vec![0xCCu8; 4];
        let with = derive(&seed, std::slice::from_ref(&envelope), false, 2, 128).unwrap();
        let without = derive(&seed, std::slice::from_ref(&envelope), true, 2, 128).unwrap();
        assert_ne!(with, without);

        let mut expected = sha1::Sha1::new();
        expected.update(&seed);
        expected.update(&envelope);
        expected.update(&[0xFF; 4]);
        assert_eq!(with, expected.finish()[..16]);
    }

    #[test]
    fn version_five_uses_sha_256_and_the_whole_digest() {
        let seed = [0x33u8; 20];
        let envelope = vec![0xDDu8; 4];
        let key = derive(&seed, std::slice::from_ref(&envelope), true, 5, 256).unwrap();
        assert_eq!(key.len(), 32);

        let mut expected = sha2::Sha256::new();
        expected.update(&seed);
        expected.update(&envelope);
        assert_eq!(key, expected.finish());
    }

    #[test]
    fn a_key_length_this_build_cannot_cut_from_sha_1_is_refused_by_name() {
        let seed = [0x44u8; 20];
        assert_eq!(
            derive(&seed, &[vec![0]], true, 2, 512),
            Err(PubSecError::UnsupportedKeyLength { bits: 512 }),
            "SHA-1 is twenty bytes and cannot yield sixty-four"
        );
        assert_eq!(
            derive(&seed, &[vec![0]], true, 2, 0),
            Err(PubSecError::UnsupportedKeyLength { bits: 0 })
        );
    }

    /// A recipient that never claims anything must produce the "somebody
    /// else's document" answer, not a panic and not a key.
    #[test]
    fn a_recipient_that_owns_no_key_is_told_the_document_is_not_theirs() {
        struct Nobody;
        impl Recipient for Nobody {
            fn unseal(&self, _: &[u8], _: Option<&[u8]>, _: Option<&[u8]>) -> Option<Vec<u8>> {
                None
            }
        }
        let envelope = include_bytes!("../../tinker-pdf-pki/tests/data/enveloped/aes-256-cbc.der");
        let outcome = unseal(&[envelope.to_vec()], &Nobody);
        assert_eq!(outcome, Err(PubSecError::NoMatchingRecipient));
    }

    #[test]
    fn an_envelope_that_will_not_parse_is_named_rather_than_treated_as_unaddressed() {
        struct Nobody;
        impl Recipient for Nobody {
            fn unseal(&self, _: &[u8], _: Option<&[u8]>, _: Option<&[u8]>) -> Option<Vec<u8>> {
                None
            }
        }
        let outcome = unseal(&[vec![0x30, 0x03, 0x02, 0x01, 0x00]], &Nobody);
        assert!(
            matches!(outcome, Err(PubSecError::UnreadableEnvelope(_))),
            "got {outcome:?}"
        );
    }
}
