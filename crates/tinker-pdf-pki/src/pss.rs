//! `RSASSA-PSS-params` (RFC 4055 §3.1, RFC 8017 A.2.3): what an
//! `id-RSASSA-PSS` algorithm identifier says about how to verify.
//!
//! PKCS#1 v1.5 names everything in its OID — `sha256WithRSAEncryption` is the
//! whole of the instruction. PSS names one OID for every variant and puts the
//! choices in a parameter structure:
//!
//! ```text
//! RSASSA-PSS-params ::= SEQUENCE {
//!     hashAlgorithm      [0] HashAlgorithm      DEFAULT sha1,
//!     maskGenAlgorithm   [1] MaskGenAlgorithm   DEFAULT mgf1SHA1,
//!     saltLength         [2] INTEGER            DEFAULT 20,
//!     trailerField       [3] TrailerField       DEFAULT trailerFieldBC }
//! ```
//!
//! The tags are explicit (RFC 4055's module is `EXPLICIT TAGS`), and every
//! field has a default, so an empty `SEQUENCE` is a complete and legal
//! instruction: SHA-1, MGF1 over SHA-1, a 20-octet salt.
//!
//! # What is refused, and why each is a refusal rather than a guess
//!
//! - **Absent parameters.** RFC 4055 §3.1: "the parameters MUST be present
//!   when used in the algorithm identifier associated with a signature
//!   value". Absent is not the same as the empty `SEQUENCE` — the empty one
//!   says "the defaults"; the absent one says nothing, and reading it as the
//!   defaults would be choosing a salt length on the signer's behalf.
//! - **A mask generation function other than MGF1.** RFC 8017 defines one and
//!   no other has ever been registered for this structure; its OID is named
//!   in the refusal so a caller can say which.
//! - **A digest this build has no implementation of**, in either slot. SHA-224
//!   is the one a real signer might choose; it is refused by OID.
//! - **A trailer field that is not 1.** `trailerFieldBC(1)` is the only value
//!   RFC 4055 defines, and it is what puts `0xbc` at the end of the encoded
//!   message. Any other number describes an encoding this verifier would not
//!   be checking.
//!
//! # What is deliberately not refused
//!
//! **A field written out at its default value.** X.690 §11.5 says DER omits a
//! component equal to its default, so `[0] sha1` written explicitly is not
//! DER. It is read anyway, for a reason that is specific to this structure:
//! the parameters decide *how* the signature is checked, never *what* it is
//! over, and an explicit `sha1` means exactly what the omitted one means.
//! There is no second reading of the value for an attacker to choose between,
//! which is the thing the DER rule exists to prevent everywhere else in this
//! crate.

use tinker_pdf_crypto::rsa::PssParameters;

use crate::cms::{digest_algorithm, DigestAlgorithm};
use crate::der::{Budget, DerError, Limits, Tag, Tlv};
use crate::oid;
use crate::x509::AlgorithmIdentifier;

/// Why an `RSASSA-PSS-params` could not be read as an instruction.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PssError {
    /// The encoding itself.
    Der(DerError),
    /// The algorithm identifier is not `id-RSASSA-PSS`, named by its dotted
    /// OID.
    NotPss { oid: String },
    /// No parameters, which RFC 4055 §3.1 forbids beside a signature value.
    ParametersAbsent,
    /// A hash this crate has no digest for, in `hashAlgorithm` or inside the
    /// mask generation function.
    UnknownHash { oid: String },
    /// A mask generation function that is not MGF1.
    UnknownMaskGeneration { oid: String },
    /// `trailerField` is not `trailerFieldBC(1)`.
    TrailerField { value: u64 },
    /// `saltLength` does not fit this machine's `usize`.
    SaltLength,
}

impl From<DerError> for PssError {
    fn from(error: DerError) -> Self {
        Self::Der(error)
    }
}

impl core::fmt::Display for PssError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Der(error) => write!(f, "{error}"),
            Self::NotPss { oid } => write!(f, "algorithm {oid} is not RSASSA-PSS"),
            Self::ParametersAbsent => write!(
                f,
                "RSASSA-PSS with no parameters, which RFC 4055 §3.1 forbids beside a signature"
            ),
            Self::UnknownHash { oid } => write!(f, "RSASSA-PSS hash algorithm {oid}"),
            Self::UnknownMaskGeneration { oid } => {
                write!(f, "RSASSA-PSS mask generation function {oid}")
            }
            Self::TrailerField { value } => {
                write!(f, "RSASSA-PSS trailer field {value}, not trailerFieldBC(1)")
            }
            Self::SaltLength => write!(f, "an RSASSA-PSS salt length past this machine's word"),
        }
    }
}

impl std::error::Error for PssError {}

/// Ceilings for a structure of at most a dozen nodes four levels deep.
const LIMITS: Limits = Limits::new(8, 64);

/// Reads the parameters of an `id-RSASSA-PSS` algorithm identifier — a
/// `SignerInfo`'s `signatureAlgorithm` or a certificate's — into the three
/// choices [`tinker_pdf_crypto::RsaPublicKey::verify_pss`] takes.
///
/// # Errors
///
/// [`PssError`], one variant per refusal this module's header lists.
pub fn parameters(identifier: &AlgorithmIdentifier<'_>) -> Result<PssParameters, PssError> {
    if identifier.oid() != oid::RSASSA_PSS {
        return Err(PssError::NotPss {
            oid: identifier.oid().to_dotted(),
        });
    }
    let node = identifier.parameters().ok_or(PssError::ParametersAbsent)?;
    // A node's depth is counted from the top of the parse it came from — a
    // signer's parameters sit eight levels inside a `ContentInfo` — so the
    // ceiling is this structure's own four levels, measured from there.
    let budget = Budget::new(Limits::new(
        node.depth().saturating_add(LIMITS.max_depth),
        LIMITS.max_nodes,
    ));
    node.require(Tag::Sequence)?;
    let mut fields = node.children(&budget)?;

    // RFC 4055 §3.1's defaults: SHA-1, MGF1 with SHA-1, 20, 1.
    let mut parameters = PssParameters {
        hash: DigestAlgorithm::Sha1,
        mask_hash: DigestAlgorithm::Sha1,
        salt_length: 20,
    };
    if let Some(tagged) = fields.context_optional(0)? {
        parameters.hash = hash(&tagged.explicit(&budget)?, &budget)?;
    }
    if let Some(tagged) = fields.context_optional(1)? {
        let generator = AlgorithmIdentifier::parse(&tagged.explicit(&budget)?, &budget)?;
        if generator.oid() != oid::MGF1 {
            return Err(PssError::UnknownMaskGeneration {
                oid: generator.oid().to_dotted(),
            });
        }
        // MGF1's parameter is itself a `HashAlgorithm`, and unlike the outer
        // fields it has no default: `mgf1SHA1` is the default of the whole
        // `maskGenAlgorithm`, not of MGF1's own parameter.
        let inner = generator
            .parameters()
            .ok_or(PssError::UnknownMaskGeneration {
                oid: generator.oid().to_dotted(),
            })?;
        parameters.mask_hash = hash(&inner, &budget)?;
    }
    if let Some(tagged) = fields.context_optional(2)? {
        let length = tagged.explicit(&budget)?.as_integer()?.as_u64()?;
        parameters.salt_length = usize::try_from(length).map_err(|_| PssError::SaltLength)?;
    }
    if let Some(tagged) = fields.context_optional(3)? {
        let value = tagged.explicit(&budget)?.as_integer()?.as_u64()?;
        if value != 1 {
            return Err(PssError::TrailerField { value });
        }
    }
    fields.finish()?;
    Ok(parameters)
}

/// A `HashAlgorithm` — an `AlgorithmIdentifier` whose parameters are `NULL`
/// or absent (RFC 4055 §2.1, which permits both and asks producers for
/// absent).
fn hash(node: &Tlv<'_>, budget: &Budget) -> Result<DigestAlgorithm, PssError> {
    let identifier = AlgorithmIdentifier::parse(node, budget)?;
    if let Some(parameters) = identifier.parameters() {
        parameters.as_null()?;
    }
    digest_algorithm(identifier.oid()).ok_or_else(|| PssError::UnknownHash {
        oid: identifier.oid().to_dotted(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::der::Cursor;

    fn unhex(text: &str) -> Vec<u8> {
        let digits: String = text.chars().filter(char::is_ascii_hexdigit).collect();
        digits
            .as_bytes()
            .chunks_exact(2)
            .filter_map(|pair| u8::from_str_radix(core::str::from_utf8(pair).ok()?, 16).ok())
            .collect()
    }

    /// Reads `der` as an `AlgorithmIdentifier` and its parameters.
    fn read(der: &[u8]) -> Result<PssParameters, PssError> {
        let budget = Budget::new(LIMITS);
        let mut cursor = Cursor::new(der, &budget);
        let node = cursor.read()?;
        let identifier = AlgorithmIdentifier::parse(&node, &budget)?;
        parameters(&identifier)
    }

    /// `id-RSASSA-PSS` around a parameter body.
    fn identifier(body: &str) -> Vec<u8> {
        let body = unhex(body);
        let oid = unhex("06 09 2A 86 48 86 F7 0D 01 01 0A");
        let mut params = vec![0x30, body.len() as u8];
        params.extend_from_slice(&body);
        let mut out = vec![0x30, (oid.len() + params.len()) as u8];
        out.extend_from_slice(&oid);
        out.extend_from_slice(&params);
        out
    }

    /// RFC 4055 §6's `rSASSA-PSS-SHA256-Params` — `sha256Identifier`,
    /// `mgf1SHA256Identifier`, `saltLength 20`, `trailerField 1` — encoded
    /// as DER encodes that value: the two hash identifiers with the `NULL`
    /// §2.1 gives them, and the last two fields omitted because they are the
    /// defaults (X.690 §11.5).
    #[test]
    fn rfc_4055s_sha_256_parameter_set_is_read() {
        let der = identifier(
            "A0 0F 30 0D 06 09 60 86 48 01 65 03 04 02 01 05 00
             A1 1C 30 1A 06 09 2A 86 48 86 F7 0D 01 01 08
                30 0D 06 09 60 86 48 01 65 03 04 02 01 05 00",
        );
        assert_eq!(
            read(&der),
            Ok(PssParameters {
                hash: DigestAlgorithm::Sha256,
                mask_hash: DigestAlgorithm::Sha256,
                salt_length: 20,
            })
        );
    }

    /// The shape OpenSSL writes for `rsa_pss_saltlen:32`: the same set with a
    /// salt as long as the digest, which is what RFC 4055 §3.1 recommends and
    /// what the committed `rsa-pss.pdf` fixture carries.
    #[test]
    fn a_salt_as_long_as_the_digest_is_read() {
        let der = identifier(
            "A0 0F 30 0D 06 09 60 86 48 01 65 03 04 02 01 05 00
             A1 1C 30 1A 06 09 2A 86 48 86 F7 0D 01 01 08
                30 0D 06 09 60 86 48 01 65 03 04 02 01 05 00
             A2 03 02 01 20",
        );
        assert_eq!(read(&der).map(|p| p.salt_length), Ok(32));
    }

    /// An empty `SEQUENCE` is every default at once — which is exactly the
    /// RSA Laboratories vector set's choice.
    #[test]
    fn an_empty_sequence_is_the_defaults() {
        assert_eq!(
            read(&identifier("")),
            Ok(PssParameters {
                hash: DigestAlgorithm::Sha1,
                mask_hash: DigestAlgorithm::Sha1,
                salt_length: 20,
            })
        );
    }

    /// The defaults written out — not DER, and read anyway, for the reason
    /// the module header gives.
    #[test]
    fn a_default_written_out_explicitly_is_read_as_the_default() {
        let der = identifier(
            "A0 0B 30 09 06 05 2B 0E 03 02 1A 05 00
             A2 03 02 01 14
             A3 03 02 01 01",
        );
        assert_eq!(read(&der).map(|p| p.salt_length), Ok(20));
    }

    #[test]
    fn a_hash_and_a_mask_hash_may_differ() {
        let der = identifier(
            "A0 0D 30 0B 06 09 60 86 48 01 65 03 04 02 03
             A1 1A 30 18 06 09 2A 86 48 86 F7 0D 01 01 08
                30 0B 06 09 60 86 48 01 65 03 04 02 01",
        );
        assert_eq!(
            read(&der),
            Ok(PssParameters {
                hash: DigestAlgorithm::Sha512,
                mask_hash: DigestAlgorithm::Sha256,
                salt_length: 20,
            })
        );
    }

    #[test]
    fn each_refusal_is_named() {
        // No parameters at all.
        let bare = unhex("30 0B 06 09 2A 86 48 86 F7 0D 01 01 0A");
        assert_eq!(read(&bare), Err(PssError::ParametersAbsent));

        // Not PSS.
        let pkcs1 = unhex("30 0D 06 09 2A 86 48 86 F7 0D 01 01 0B 05 00");
        assert!(matches!(read(&pkcs1), Err(PssError::NotPss { .. })));

        // SHA-224, which this build has no digest for.
        let sha224 = identifier("A0 0D 30 0B 06 09 60 86 48 01 65 03 04 02 04");
        assert_eq!(
            read(&sha224),
            Err(PssError::UnknownHash {
                oid: "2.16.840.1.101.3.4.2.4".into()
            })
        );

        // A mask generation function that is not MGF1 (`.1.1.9`).
        let mgf = identifier(
            "A1 1A 30 18 06 09 2A 86 48 86 F7 0D 01 01 09
                30 0B 06 09 60 86 48 01 65 03 04 02 01",
        );
        assert!(matches!(
            read(&mgf),
            Err(PssError::UnknownMaskGeneration { .. })
        ));

        // MGF1 with nothing to say which hash.
        let bare_mgf = identifier("A1 0D 30 0B 06 09 2A 86 48 86 F7 0D 01 01 08");
        assert!(matches!(
            read(&bare_mgf),
            Err(PssError::UnknownMaskGeneration { .. })
        ));

        // trailerField 2.
        let trailer = identifier("A3 03 02 01 02");
        assert_eq!(read(&trailer), Err(PssError::TrailerField { value: 2 }));

        // A hash whose parameters are something other than NULL.
        let params = identifier("A0 0E 30 0C 06 09 60 86 48 01 65 03 04 02 01 02 01 00");
        assert!(matches!(read(&params), Err(PssError::Der(_))));

        // Fields out of order are not the grammar.
        let order = identifier("A2 03 02 01 20 A0 0B 30 09 06 05 2B 0E 03 02 1A 05 00");
        assert!(matches!(read(&order), Err(PssError::Der(_))));
    }

    /// Every truncation of a real parameter block is a refusal, not a panic
    /// and not a reading.
    #[test]
    fn no_prefix_of_the_parameters_reads() {
        let der = identifier(
            "A0 0F 30 0D 06 09 60 86 48 01 65 03 04 02 01 05 00
             A1 1C 30 1A 06 09 2A 86 48 86 F7 0D 01 01 08
                30 0D 06 09 60 86 48 01 65 03 04 02 01 05 00
             A2 03 02 01 20",
        );
        for cut in 0..der.len() {
            assert!(read(&der[..cut]).is_err(), "a {cut}-octet prefix");
        }
    }
}
