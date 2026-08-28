//! CMS `SignedData` (RFC 5652), which is what a PDF signature's `/Contents`
//! holds.
//!
//! A `SignedData` is the same three things a certificate is, and this module
//! keeps all three for the same reasons [`crate::x509`]'s header gives. It is a
//! **set of decoded values** — who signed, with which digest, at what claimed
//! time — which a verdict reports. It is a **set of located encodings** — a
//! certificate's DER, a timestamp token's DER — which the caller hands on to
//! another parser rather than to a re-serialiser. And it is a **signed
//! object**, so [`SignerInfo::signed_attrs_to_digest`] hands back the exact
//! bytes the signature is over.
//!
//! # The one rule in here that is easy to get wrong and impossible to notice
//!
//! When `signedAttrs` is present, the signature is **not** over the content.
//! RFC 5652 §5.4 says the digest is taken over the DER encoding of a
//! `SET OF Attribute` — and the field is stored as `[0] IMPLICIT`, so the
//! stored tag octet is `0xA0` and the octet that must be digested is `0x31`:
//!
//! > The IMPLICIT \[0\] tag in the signedAttrs is not used for the DER
//! > encoding, rather an EXPLICIT SET OF tag is used. That is, the DER
//! > encoding of the EXPLICIT SET OF tag, rather than of the IMPLICIT \[0\]
//! > tag, MUST be included in the message digest calculation along with the
//! > length and content octets of the SignedAttributes value.
//!
//! Digesting the stored bytes produces a digest that is wrong by one byte in
//! its input and completely different in its output, and **nothing about the
//! failure says so**: every field parses, the certificate is fine, the
//! arithmetic is fine, and the signature simply does not verify. So the
//! re-encoding is done here, once, where the bytes are — never left to a
//! caller — and [`the_signed_attributes_are_digested_as_a_set_of_not_as_a_context_tag`]
//! fails if the tag octet is carried through. The `[0]` node stays reachable
//! as [`Attributes::stored_der`] for anyone who needs the bytes as they
//! arrived.
//!
//! Only the tag octet changes. The length octets are already minimal —
//! [`crate::der`] refused them otherwise — and the content octets are the
//! signer's own, **including the order of the attributes**. X.690 §11.6 wants
//! a `SET OF` sorted by encoding and [`crate::der`] deliberately does not
//! check it; re-sorting here to satisfy the rule would digest bytes the signer
//! never wrote, which is the one way to turn a valid signature into an invalid
//! one on purpose.
//!
//! # What this module refuses, and what each refusal costs
//!
//! **An indefinite length inside `signedAttrs`, and only there.** This module
//! is the one caller in the crate that reads BER, and the reason is measured:
//! RFC 5652 §5.1 permits BER for a `SignedData`, ISO 32000-1 12.8.3.3.1 calls
//! a PDF signature's `/Contents` DER, and **four of the eighteen CMS blobs in
//! the fetched corpora side with the RFC** — they open `30 80 … A0 80 30 80`,
//! naming Acrobat Distiller 5.0.5, Adobe LiveCycle Designer ES 8.2 and 10.0,
//! and **LibreOffice 7.5** as their producers, which is two independent
//! lineages rather than one vendor's quirk. (The producer is the document's,
//! not necessarily the signer's; it is the evidence the files carry.) So
//! [`ContentInfo::parse`] runs under [`Limits::CMS`], which sets
//! [`Limits::allow_indefinite_lengths`], and those four parse.
//!
//! What does **not** widen with it is the encoding a signature is checked
//! against. §5.4 digests the *DER* of the attribute set, so
//! reading a `signedAttrs` holds it — the `[0]` node and its whole subtree,
//! attribute values this crate has no decoder for included — to definite
//! lengths through [`Tlv::require_definite_lengths`], and a BER one
//! is [`CmsError::IndefiniteSignedAttributes`] rather than a signature that
//! quietly fails to verify. All four corpus blobs write their `signedAttrs`
//! with definite lengths, so the rule costs the corpus nothing today and is
//! the thing that would have to give first if it ever did.
//!
//! Two narrower things stay refused, and neither is an oversight.
//! **`unsignedAttrs` is not held to the rule** — nothing digests it, so
//! narrowing it would refuse a legal message for no property. And **a
//! segmented OCTET STRING is still [`DerError::WrongForm`]**: BER's
//! constructed string form is a second spelling of a *value* rather than of a
//! *length*, reassembling one would mean allocating and copying content on a
//! parse path that borrows, and no corpus blob emits one. `eContent` in an
//! `adbe.pkcs7.sha1` message is where that would first bite, and
//! `crates/tinker-pdf/tests/cms_census.rs` counts what actually arrives.
//!
//! **A `contentType` that is not `id-signedData`.** The `[0]` content is then
//! not a `SignedData` at all, and reading it as one would be reading a
//! different structure ([`CmsError::UnsupportedContentType`]).
//!
//! **An algorithm OID with nothing behind it.** Resolving one is
//! [`CmsError::UnknownDigestAlgorithm`] or
//! [`CmsError::UnknownSignatureAlgorithm`], naming the dotted OID — but only
//! at the *accessor*. The identifier itself is always kept, so a caller can
//! report which algorithm it could not check, which is the one thing a parser
//! that refused outright could not do.
//!
//! **The single-instance, single-value attributes.** §11.1, §11.2 and §11.3
//! each say a `contentType`, `messageDigest` or `signingTime` attribute has
//! exactly one value, and RFC 5652 §5.3 does not admit two instances of one.
//! Two `messageDigest` attributes is not a variation in content: it is a
//! question about which digest was signed, and a verifier that picks one is
//! answering it by accident ([`CmsError::DuplicateAttribute`],
//! [`CmsError::AttributeValueCount`]).
//!
//! **An empty `signedAttrs` or `unsignedAttrs`.** Both are
//! `SET SIZE (1..MAX)`, and a present-but-empty `signedAttrs` would be a
//! signature over the encoding of nothing ([`CmsError::EmptyAttributes`]).
//!
//! # What is deliberately *not* refused
//!
//! - **The `version` numbers.** Unlike X.509's, where v3 is what admits
//!   extensions, a `SignedData`'s version gates no field of the grammar
//!   (§5.1): it is a compatibility marker computed from what the message
//!   happens to contain. Refusing on it would discard a structure that reads
//!   perfectly for a number nothing here consults, so both versions are
//!   surfaced and [`SignerInfo::version_matches_sid`] asks §5.3's question
//!   separately, the way [`crate::x509::Certificate::signature_algorithms_agree`]
//!   does.
//! - **A `signerInfos` set with nothing in it.** §5.1 puts no size constraint
//!   on it, and a certificates-only message is a real and legal thing. A PDF
//!   signature with no signer proves nothing, and saying so is the verdict's
//!   job rather than the parser's.
//! - **`digestAlgorithms` that does not list a signer's digest.** §5.1 makes
//!   that a SHOULD; one corpus blob declares SHA-1 and signs with SHA-256.
//!   [`SignedData::declares`] asks the question and nothing here acts on it.
//! - **Certificates.** They are located, not parsed — see
//!   [`SignedData::certificates`] for why one unreadable certificate must not
//!   cost the whole structure.
//!
//! # What this does not do at all
//!
//! It does not verify. Nothing here checks a signature, compares a
//! `messageDigest` against a document, evaluates a certificate, looks at a
//! CRL, or reads a clock — [`SignerInfo::signing_time`] is the signer's
//! unverified claim about when they signed, and a timestamp token is handed
//! over as bytes ([`SignerInfo::timestamp_tokens`]). Timestamp validation is
//! an explicit non-goal of `docs/design/signatures.md`; revocation data is
//! surfaced for the host under the same document's "no I/O" rule.

/// The digest a CMS structure names, which is `tinker-pdf-crypto`'s own enum.
///
/// Re-exported rather than redeclared. A second enum with the same four
/// members would exist only to be converted back at the one place it is used
/// — the verifier — and a conversion table between two identical enums is a
/// place for a typo that maps SHA-384 to SHA-512 and is caught by nothing.
/// The `pki -> crypto` edge already exists for [`crate::x509`]'s key
/// identifier, and this takes one type across it rather than an algorithm.
pub use tinker_pdf_crypto::DigestAlgorithm;

use crate::der::{Budget, Class, Cursor, DerError, Int, Limits, Oid, Tag, Tlv};
use crate::name::Name;
use crate::oid;
use crate::x509::AlgorithmIdentifier;

/// Everything this module refuses a `SignedData` for.
///
/// As in [`crate::der`], there is no variant meaning "malformed". A caller of
/// this parser is usually explaining to somebody why a signature could not be
/// checked, and "malformed" is not an explanation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CmsError {
    /// The encoding itself.
    Der(DerError),
    /// Bytes after the `ContentInfo` SEQUENCE.
    TrailingBytes,
    /// A `ContentInfo` whose `contentType` is not `id-signedData`
    /// (§5.1), named by its dotted OID.
    UnsupportedContentType { oid: String },
    /// A `SignerIdentifier` that is neither an `IssuerAndSerialNumber`
    /// SEQUENCE nor `[0] subjectKeyIdentifier` (§5.3).
    UnknownSignerIdentifier { class: Class, tag: u32 },
    /// A digest algorithm identifier this crate has no digest for.
    UnknownDigestAlgorithm { oid: String },
    /// A signature algorithm identifier this crate cannot name.
    UnknownSignatureAlgorithm { oid: String },
    /// A `SET SIZE (1..MAX) OF Attribute` with nothing in it (§5.3).
    EmptyAttributes,
    /// An indefinite length inside `signedAttrs` — the one place in a
    /// `SignedData` where BER is refused however the rest was encoded.
    ///
    /// §5.4 computes the signature over the **DER** encoding of the attribute
    /// set. A set with two possible encodings has no digest for a verifier and
    /// a signer to agree about, and the failure would be silent: every field
    /// parses, the certificate is fine, the arithmetic is fine, and the
    /// signature simply does not verify. So this is refused at parse time,
    /// with its own name, rather than left to become a verdict nobody can
    /// explain. `unsignedAttrs` carries no such rule and is not held to it —
    /// nothing digests one.
    IndefiniteSignedAttributes,
    /// Two instances of an attribute the specification admits once.
    DuplicateAttribute { oid: String },
    /// An attribute whose `attrValues` set does not hold exactly the one value
    /// its clause requires (§11.1, §11.2, §11.3).
    AttributeValueCount { oid: String, count: usize },
    /// A named attribute whose value did not hold its own syntax.
    BadAttribute { oid: String, error: DerError },
}

impl From<DerError> for CmsError {
    fn from(error: DerError) -> Self {
        Self::Der(error)
    }
}

impl CmsError {
    /// Names an attribute in a refusal by its dotted OID, so a message says
    /// which attribute rather than which position.
    fn attribute(oid: Oid<'_>, error: DerError) -> Self {
        Self::BadAttribute {
            oid: oid.to_dotted(),
            error,
        }
    }
}

impl core::fmt::Display for CmsError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Der(error) => write!(f, "{error}"),
            Self::TrailingBytes => write!(f, "bytes after the ContentInfo"),
            Self::UnsupportedContentType { oid } => {
                write!(f, "content type {oid} is not id-signedData")
            }
            Self::UnknownSignerIdentifier { class, tag } => {
                write!(f, "a signer identified by a {class:?} {tag} tag")
            }
            Self::UnknownDigestAlgorithm { oid } => write!(f, "digest algorithm {oid}"),
            Self::UnknownSignatureAlgorithm { oid } => write!(f, "signature algorithm {oid}"),
            Self::EmptyAttributes => write!(f, "an attribute set with no attributes"),
            Self::IndefiniteSignedAttributes => write!(
                f,
                "an indefinite length inside signedAttrs, which RFC 5652 §5.4 \
                 requires to be DER because the signature is over its encoding"
            ),
            Self::DuplicateAttribute { oid } => {
                write!(f, "two {oid} attributes where one is allowed")
            }
            Self::AttributeValueCount { oid, count } => {
                write!(f, "attribute {oid} carries {count} values, not one")
            }
            Self::BadAttribute { oid, error } => write!(f, "attribute {oid}: {error}"),
        }
    }
}

impl std::error::Error for CmsError {}

/// Which signature algorithm a `SignerInfo` names, as far as this crate reads
/// one.
///
/// Not a bare OID, because the question a verifier asks is "what arithmetic,
/// over which digest" and the OID answers it in two different shapes. RFC 5754
/// §3.2 asks producers for bare `rsaEncryption`, in which case the digest is
/// the `SignerInfo`'s own `digestAlgorithm`; the `sha*WithRSAEncryption` OIDs
/// name both at once and appear at least as often in the fetched corpora.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SignatureAlgorithm {
    /// RSASSA-PKCS1-v1_5 (RFC 8017 §8.2). `digest` is `None` for bare
    /// `rsaEncryption`, where [`SignerInfo::digest_algorithm`] is the answer.
    RsaPkcs1v15 { digest: Option<DigestAlgorithm> },
    /// ECDSA with the digest the OID names (RFC 5758 §3.2).
    Ecdsa { digest: DigestAlgorithm },
    /// RSASSA-PSS. **Named, not decoded**: the salt length, the mask
    /// generation function and the digest all live in an
    /// `RSASSA-PSS-params` structure this crate does not read, so a caller
    /// meeting one knows what it is and knows nothing here can check it.
    RsaPss,
}

/// The digest [`oid`] names, where this crate has one.
#[must_use]
pub fn digest_algorithm(algorithm: Oid<'_>) -> Option<DigestAlgorithm> {
    if algorithm == oid::ID_SHA1 {
        Some(DigestAlgorithm::Sha1)
    } else if algorithm == oid::ID_SHA256 {
        Some(DigestAlgorithm::Sha256)
    } else if algorithm == oid::ID_SHA384 {
        Some(DigestAlgorithm::Sha384)
    } else if algorithm == oid::ID_SHA512 {
        Some(DigestAlgorithm::Sha512)
    } else {
        None
    }
}

/// The signature algorithm [`oid`] names, where this crate has one.
#[must_use]
pub fn signature_algorithm(algorithm: Oid<'_>) -> Option<SignatureAlgorithm> {
    if algorithm == oid::RSA_ENCRYPTION {
        Some(SignatureAlgorithm::RsaPkcs1v15 { digest: None })
    } else if algorithm == oid::SHA1_WITH_RSA {
        Some(SignatureAlgorithm::RsaPkcs1v15 {
            digest: Some(DigestAlgorithm::Sha1),
        })
    } else if algorithm == oid::SHA256_WITH_RSA {
        Some(SignatureAlgorithm::RsaPkcs1v15 {
            digest: Some(DigestAlgorithm::Sha256),
        })
    } else if algorithm == oid::SHA384_WITH_RSA {
        Some(SignatureAlgorithm::RsaPkcs1v15 {
            digest: Some(DigestAlgorithm::Sha384),
        })
    } else if algorithm == oid::SHA512_WITH_RSA {
        Some(SignatureAlgorithm::RsaPkcs1v15 {
            digest: Some(DigestAlgorithm::Sha512),
        })
    } else if algorithm == oid::RSASSA_PSS {
        Some(SignatureAlgorithm::RsaPss)
    } else if algorithm == oid::ECDSA_WITH_SHA1 {
        Some(SignatureAlgorithm::Ecdsa {
            digest: DigestAlgorithm::Sha1,
        })
    } else if algorithm == oid::ECDSA_WITH_SHA256 {
        Some(SignatureAlgorithm::Ecdsa {
            digest: DigestAlgorithm::Sha256,
        })
    } else if algorithm == oid::ECDSA_WITH_SHA384 {
        Some(SignatureAlgorithm::Ecdsa {
            digest: DigestAlgorithm::Sha384,
        })
    } else if algorithm == oid::ECDSA_WITH_SHA512 {
        Some(SignatureAlgorithm::Ecdsa {
            digest: DigestAlgorithm::Sha512,
        })
    } else {
        None
    }
}

/// One `Attribute` (§5.3): a type OID and a `SET OF` values.
///
/// The values are located rather than decoded, because what a value *is* is a
/// function of the type OID and this crate names five of the hundreds that
/// exist. The five it names are decoded onto [`SignerInfo`]; every other
/// attribute — `smimeCapabilities`, Adobe's `revocationInfoArchival`, whatever
/// a producer invents next — arrives here whole, and a caller that knows what
/// one is can read it without this module having heard of it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Attribute<'a> {
    oid: Oid<'a>,
    values: Vec<Tlv<'a>>,
    der: &'a [u8],
}

impl<'a> Attribute<'a> {
    /// The attribute type.
    #[must_use]
    pub const fn oid(&self) -> Oid<'a> {
        self.oid
    }

    /// The values, in encoded order.
    #[must_use]
    pub fn values(&self) -> &[Tlv<'a>] {
        &self.values
    }

    /// The whole `Attribute` encoding.
    #[must_use]
    pub const fn der(&self) -> &'a [u8] {
        self.der
    }

    /// The one value, where the clause admits exactly one.
    ///
    /// # Errors
    ///
    /// [`CmsError::AttributeValueCount`] for none or several, which is a
    /// refusal rather than a choice: §11.1's, §11.2's and §11.3's "a single
    /// attribute value" exists because picking one of two is answering a
    /// question about what was signed by guessing.
    pub fn single_value(&self) -> Result<Tlv<'a>, CmsError> {
        match self.values.as_slice() {
            [one] => Ok(*one),
            other => Err(CmsError::AttributeValueCount {
                oid: self.oid.to_dotted(),
                count: other.len(),
            }),
        }
    }
}

/// A `SignedAttributes` or `UnsignedAttributes` set, and the node it came in.
///
/// The node matters as much as the list: RFC 5652 §5.4's digest is over this
/// set's own octets with one tag replaced, and a set rebuilt from the parsed
/// list would be a re-encoding rather than the bytes the signer signed. See
/// [`SignerInfo::signed_attrs_to_digest`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Attributes<'a> {
    stored: Tlv<'a>,
    attributes: Vec<Attribute<'a>>,
}

/// Which of §5.3's two attribute sets is being read.
///
/// The distinction is not cosmetic: one of them is what §5.4 digests, and the
/// encoding rules that apply to it do not apply to the other.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Role {
    /// `signedAttrs [0]`. Held to DER, whatever encloses it.
    Signed,
    /// `unsignedAttrs [1]`. Nothing digests it, so nothing here narrows it.
    Unsigned,
}

impl<'a> Attributes<'a> {
    /// Reads a `[0]`- or `[1]`-tagged `SET SIZE (1..MAX) OF Attribute`.
    fn parse(tagged: &Tlv<'a>, budget: &Budget, role: Role) -> Result<Self, CmsError> {
        if role == Role::Signed {
            // §5.4's rule, enforced where the bytes are rather than where they
            // are digested — a refusal at digest time would have to be
            // reported as a signature that did not verify, which is the one
            // message that would send a reader looking in the wrong place.
            //
            // The whole subtree, not the `[0]` node's own length: the walk
            // below reads an attribute's *values* without descending into
            // them, so an indefinite length inside an attribute this crate has
            // no decoder for is invisible to everything except this sweep.
            tagged
                .require_definite_lengths(budget)
                .map_err(|error| match error {
                    DerError::IndefiniteLength => CmsError::IndefiniteSignedAttributes,
                    other => CmsError::Der(other),
                })?;
        }
        let mut members = tagged.children(budget)?;
        let mut attributes = Vec::new();
        while !members.is_empty() {
            let node = members.expect(Tag::Sequence)?;
            let mut fields = node.children(budget)?;
            let oid = fields.expect(Tag::Oid)?.as_oid()?;
            let set = fields.expect(Tag::Set)?;
            fields.finish()?;
            let mut values = Vec::new();
            let mut inner = set.children(budget)?;
            while !inner.is_empty() {
                values.push(inner.read()?);
            }
            attributes.push(Attribute {
                oid,
                values,
                der: node.raw(),
            });
        }
        if attributes.is_empty() {
            return Err(CmsError::EmptyAttributes);
        }
        Ok(Self {
            stored: *tagged,
            attributes,
        })
    }

    /// Every attribute, in encoded order.
    #[must_use]
    pub fn all(&self) -> &[Attribute<'a>] {
        &self.attributes
    }

    /// **The set exactly as it arrived**, context tag and all.
    ///
    /// Not what §5.4 digests. [`SignerInfo::signed_attrs_to_digest`] is.
    #[must_use]
    pub const fn stored_der(&self) -> &'a [u8] {
        self.stored.raw()
    }

    /// Where the stored set sits in the buffer that was parsed.
    #[must_use]
    pub fn stored_range(&self) -> core::ops::Range<usize> {
        self.stored.range()
    }

    /// Every attribute of a type, in encoded order.
    pub fn find<'s, 'w: 's>(
        &'s self,
        wanted: Oid<'w>,
    ) -> impl Iterator<Item = &'s Attribute<'a>> + 's {
        self.attributes
            .iter()
            .filter(move |attribute| attribute.oid.as_bytes() == wanted.as_bytes())
    }

    /// The one attribute of a type, where the specification admits one.
    ///
    /// # Errors
    ///
    /// [`CmsError::DuplicateAttribute`] for two or more.
    pub fn one(&self, wanted: Oid<'_>) -> Result<Option<&Attribute<'a>>, CmsError> {
        let mut found = self
            .attributes
            .iter()
            .filter(|attribute| attribute.oid.as_bytes() == wanted.as_bytes());
        let first = found.next();
        if found.next().is_some() {
            return Err(CmsError::DuplicateAttribute {
                oid: wanted.to_dotted(),
            });
        }
        Ok(first)
    }
}

/// How a `SignerInfo` names the certificate that signed it (§5.3).
///
/// Both alternatives occur in the wild and they are answered differently: the
/// first is matched against a certificate's issuer and serial, the second
/// against its `subjectKeyIdentifier` extension — or, where it has none,
/// against [`crate::x509::Certificate::key_identifier_sha1`].
///
/// **Nothing in the fetched corpora uses the second.** All sixteen signers
/// this crate can read there identify themselves by issuer and serial — the
/// four blobs it refuses for BER are not counted, because a refused blob says
/// nothing either way — so the `[0]` arm is held up by a fixture in this file
/// and by nothing else, which is recorded here rather than left to be
/// discovered.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SignerIdentifier<'a> {
    /// `IssuerAndSerialNumber ::= SEQUENCE { issuer Name, serialNumber
    /// CertificateSerialNumber }`.
    IssuerAndSerialNumber {
        issuer: Name<'a>,
        serial: Int<'a>,
        /// The whole SEQUENCE, which is what a producer copies out of a
        /// certificate and therefore what compares most cheaply.
        der: &'a [u8],
    },
    /// `[0] subjectKeyIdentifier`, IMPLICIT over an OCTET STRING.
    SubjectKeyIdentifier(&'a [u8]),
}

/// An `EncapsulatedContentInfo` (§5.2): what was signed, where it is carried
/// at all.
///
/// A PDF's `adbe.pkcs7.detached` (ISO 32000-1 12.8.3.3.1) leaves `eContent`
/// absent and the signed bytes are the document's own `/ByteRange` spans;
/// `adbe.pkcs7.sha1` (12.8.3.3.2) puts the document's SHA-1 digest *in* the
/// `eContent`. So `content` being `None` is not an omission, it is the
/// commoner of the two shapes and the one the caller must be ready for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EncapsulatedContent<'a> {
    content_type: Oid<'a>,
    content: Option<&'a [u8]>,
    der: &'a [u8],
}

impl<'a> EncapsulatedContent<'a> {
    fn parse(tlv: &Tlv<'a>, budget: &Budget) -> Result<Self, CmsError> {
        tlv.require(Tag::Sequence)?;
        let mut fields = tlv.children(budget)?;
        let content_type = fields.expect(Tag::Oid)?.as_oid()?;
        // `[0] EXPLICIT OCTET STRING OPTIONAL`: explicit, so the OCTET STRING
        // is a node inside the tag rather than the tag wearing its content.
        let content = match fields.context_optional(0)? {
            Some(tagged) => Some(tagged.explicit(budget)?.as_octet_string()?),
            None => None,
        };
        fields.finish()?;
        Ok(Self {
            content_type,
            content,
            der: tlv.raw(),
        })
    }

    /// What the content is (`id-data` for every PDF signature seen).
    #[must_use]
    pub const fn content_type(&self) -> Oid<'a> {
        self.content_type
    }

    /// The content octets, where the message carries them.
    #[must_use]
    pub const fn content(&self) -> Option<&'a [u8]> {
        self.content
    }

    /// Whether this is the detached shape: a content type and no content.
    #[must_use]
    pub const fn is_detached(&self) -> bool {
        self.content.is_none()
    }

    /// The whole `EncapsulatedContentInfo` encoding.
    #[must_use]
    pub const fn der(&self) -> &'a [u8] {
        self.der
    }
}

/// One member of a `CertificateSet` (§10.2.3), located rather than decoded.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CertificateChoice<'a> {
    /// An X.509 `Certificate`, as its complete DER.
    /// [`crate::x509::Certificate::parse`] reads one.
    X509(&'a [u8]),
    /// One of `CertificateChoices`' other alternatives — `[1]` an obsolete
    /// PKCS#6 extended certificate, `[2]`/`[3]` attribute certificates, `[4]`
    /// anything else. Kept whole and unread: none is an X.509 certificate, so
    /// decoding one here would be a second certificate parser for a shape no
    /// corpus file emits.
    Other { tag: u32, der: &'a [u8] },
}

/// One member of a `RevocationInfoChoices` (§10.2.1), located rather than
/// evaluated.
///
/// **Surfaced, never consulted.** `docs/design/signatures.md` makes revocation
/// the host's call and the engine performs no I/O; what is embedded is handed
/// over, and whether it is fresh is a question with a clock in it that ruling 4
/// keeps out of this tree.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RevocationChoice<'a> {
    /// A `CertificateList` (RFC 5280 §5.1), as its complete DER.
    Crl(&'a [u8]),
    /// `[1] other`, which is where an OCSP response arrives (RFC 5940).
    Other { tag: u32, der: &'a [u8] },
}

/// One `ESSCertIDv2` (RFC 5035 §4): a digest of a certificate the signer says
/// it used.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EssCertId<'a> {
    hash_algorithm: Option<AlgorithmIdentifier<'a>>,
    hash: &'a [u8],
    issuer_serial: Option<&'a [u8]>,
}

impl<'a> EssCertId<'a> {
    /// The digest algorithm identifier, where the encoding carries one.
    ///
    /// `None` is the DEFAULT rather than an absence: RFC 5035 §4 makes
    /// `id-sha256` the default and DER omits a field at its default (X.690
    /// §11.5), so [`EssCertId::digest`] answers `Sha256` for a `None` here.
    #[must_use]
    pub const fn hash_algorithm(&self) -> Option<AlgorithmIdentifier<'a>> {
        self.hash_algorithm
    }

    /// The digest algorithm, with RFC 5035 §4's default applied.
    ///
    /// # Errors
    ///
    /// [`CmsError::UnknownDigestAlgorithm`] for an OID this crate has no
    /// digest for.
    pub fn digest(&self) -> Result<DigestAlgorithm, CmsError> {
        match self.hash_algorithm {
            None => Ok(DigestAlgorithm::Sha256),
            Some(identifier) => {
                digest_algorithm(identifier.oid()).ok_or_else(|| CmsError::UnknownDigestAlgorithm {
                    oid: identifier.oid().to_dotted(),
                })
            }
        }
    }

    /// The certificate's digest, as the attribute carries it.
    #[must_use]
    pub const fn hash(&self) -> &'a [u8] {
        self.hash
    }

    /// `issuerSerial`, undecoded. A `GeneralNames` and a serial; nothing in
    /// this milestone reads one, and the bytes are here so a later one need
    /// not re-walk the attribute.
    #[must_use]
    pub const fn issuer_serial(&self) -> Option<&'a [u8]> {
        self.issuer_serial
    }
}

/// A `signingCertificateV2` attribute (RFC 5035 §3), decoded and no more.
///
/// It is the signer's own statement of which certificate they meant, as
/// digests. **Nothing here checks it**: comparing a digest against a
/// certificate in the set is a step in verification, and verification is not
/// what this crate does.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SigningCertificateV2<'a> {
    certs: Vec<EssCertId<'a>>,
    policies: Option<&'a [u8]>,
    der: &'a [u8],
}

impl<'a> SigningCertificateV2<'a> {
    /// The certificate identifiers, in encoded order. The first is the
    /// signer's own certificate (§3).
    #[must_use]
    pub fn certs(&self) -> &[EssCertId<'a>] {
        &self.certs
    }

    /// The optional `policies` field, undecoded.
    #[must_use]
    pub const fn policies_der(&self) -> Option<&'a [u8]> {
        self.policies
    }

    /// The whole attribute value's encoding.
    #[must_use]
    pub const fn der(&self) -> &'a [u8] {
        self.der
    }

    /// Reads a `SigningCertificateV2` value.
    fn parse(tlv: &Tlv<'a>, budget: &Budget) -> Result<Self, DerError> {
        tlv.require(Tag::Sequence)?;
        let mut fields = tlv.children(budget)?;
        let mut certs = Vec::new();
        let mut list = fields.expect(Tag::Sequence)?.children(budget)?;
        while !list.is_empty() {
            let node = list.expect(Tag::Sequence)?;
            let mut parts = node.children(budget)?;
            // `hashAlgorithm` is DEFAULT, so the first node is an
            // AlgorithmIdentifier SEQUENCE when it is present and the
            // `certHash` OCTET STRING when it is not — which is what
            // distinguishes them, since the two carry different tags.
            let hash_algorithm = match parts.peek() {
                Some(Ok((Class::Universal, _, number))) if number == Tag::Sequence.number() => {
                    Some(AlgorithmIdentifier::parse(&parts.read()?, budget)?)
                }
                _ => None,
            };
            let hash = parts.expect(Tag::OctetString)?.as_octet_string()?;
            let issuer_serial = parts.expect_optional(Tag::Sequence)?.map(|node| node.raw());
            parts.finish()?;
            certs.push(EssCertId {
                hash_algorithm,
                hash,
                issuer_serial,
            });
        }
        let policies = fields
            .expect_optional(Tag::Sequence)?
            .map(|node| node.raw());
        fields.finish()?;
        Ok(Self {
            certs,
            policies,
            der: tlv.raw(),
        })
    }
}

/// One `SignerInfo` (§5.3).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SignerInfo<'a> {
    version: u64,
    sid: SignerIdentifier<'a>,
    digest_algorithm: AlgorithmIdentifier<'a>,
    signed_attrs: Option<Attributes<'a>>,
    signature_algorithm: AlgorithmIdentifier<'a>,
    signature: &'a [u8],
    unsigned_attrs: Option<Attributes<'a>>,
    content_type: Option<Oid<'a>>,
    message_digest: Option<&'a [u8]>,
    signing_time: Option<i64>,
    signing_certificate_v2: Option<SigningCertificateV2<'a>>,
    timestamp_tokens: Vec<&'a [u8]>,
    der: &'a [u8],
}

impl<'a> SignerInfo<'a> {
    fn parse(tlv: &Tlv<'a>, budget: &Budget) -> Result<Self, CmsError> {
        tlv.require(Tag::Sequence)?;
        let mut fields = tlv.children(budget)?;
        let version = fields.expect(Tag::Integer)?.as_integer()?.as_u64()?;

        let sid_node = fields.read()?;
        let sid = match (sid_node.class(), sid_node.tag()) {
            (Class::Universal, tag) if tag == Tag::Sequence.number() => {
                sid_node.require(Tag::Sequence)?;
                let mut parts = sid_node.children(budget)?;
                let issuer = Name::parse(&parts.expect(Tag::Sequence)?, budget)?;
                let serial = parts.expect(Tag::Integer)?.as_integer()?;
                parts.finish()?;
                SignerIdentifier::IssuerAndSerialNumber {
                    issuer,
                    serial,
                    der: sid_node.raw(),
                }
            }
            (Class::ContextSpecific, 0) => SignerIdentifier::SubjectKeyIdentifier(
                sid_node.implicit(Tag::OctetString).as_octet_string()?,
            ),
            (class, tag) => return Err(CmsError::UnknownSignerIdentifier { class, tag }),
        };

        let digest_algorithm = AlgorithmIdentifier::parse(&fields.read()?, budget)?;
        let signed_attrs = match fields.context_optional(0)? {
            Some(tagged) => Some(Attributes::parse(&tagged, budget, Role::Signed)?),
            None => None,
        };
        let signature_algorithm = AlgorithmIdentifier::parse(&fields.read()?, budget)?;
        let signature = fields.expect(Tag::OctetString)?.as_octet_string()?;
        let unsigned_attrs = match fields.context_optional(1)? {
            Some(tagged) => Some(Attributes::parse(&tagged, budget, Role::Unsigned)?),
            None => None,
        };
        fields.finish()?;

        let mut out = Self {
            version,
            sid,
            digest_algorithm,
            signed_attrs,
            signature_algorithm,
            signature,
            unsigned_attrs,
            content_type: None,
            message_digest: None,
            signing_time: None,
            signing_certificate_v2: None,
            timestamp_tokens: Vec::new(),
            der: tlv.raw(),
        };
        out.decode_named_attributes(budget)?;
        Ok(out)
    }

    /// Decodes the five attributes this crate names, and leaves the rest whole.
    ///
    /// The same arrangement as [`crate::x509::Extensions::decode`], for the
    /// same reason: an attribute with a decoder behind it is worth a name, and
    /// one without is worth its bytes.
    fn decode_named_attributes(&mut self, budget: &Budget) -> Result<(), CmsError> {
        if let Some(attributes) = &self.signed_attrs {
            if let Some(attribute) = attributes.one(oid::AA_CONTENT_TYPE)? {
                let value = attribute.single_value()?;
                self.content_type = Some(
                    value
                        .as_oid()
                        .map_err(|error| CmsError::attribute(attribute.oid, error))?,
                );
            }
            if let Some(attribute) = attributes.one(oid::AA_MESSAGE_DIGEST)? {
                let value = attribute.single_value()?;
                self.message_digest = Some(
                    value
                        .as_octet_string()
                        .map_err(|error| CmsError::attribute(attribute.oid, error))?,
                );
            }
            if let Some(attribute) = attributes.one(oid::AA_SIGNING_TIME)? {
                let value = attribute.single_value()?;
                self.signing_time = Some(
                    value
                        .as_time()
                        .map_err(|error| CmsError::attribute(attribute.oid, error))?,
                );
            }
            if let Some(attribute) = attributes.one(oid::AA_SIGNING_CERTIFICATE_V2)? {
                let value = attribute.single_value()?;
                self.signing_certificate_v2 = Some(
                    SigningCertificateV2::parse(&value, budget)
                        .map_err(|error| CmsError::attribute(attribute.oid, error))?,
                );
            }
        }
        if let Some(attributes) = &self.unsigned_attrs {
            // Several are legal: a document countersigned by two authorities
            // carries two, so this is a list rather than an option.
            for attribute in attributes.find(oid::AA_TIMESTAMP_TOKEN) {
                for value in attribute.values() {
                    self.timestamp_tokens.push(value.raw());
                }
            }
        }
        Ok(())
    }

    /// The `version` field (§5.3), surfaced rather than acted on.
    #[must_use]
    pub const fn version(&self) -> u64 {
        self.version
    }

    /// Whether the version agrees with the signer identifier §5.3 pairs it
    /// with: 1 for `issuerAndSerialNumber`, 3 for `subjectKeyIdentifier`.
    ///
    /// Asked separately rather than enforced, exactly as
    /// [`crate::x509::Certificate::signature_algorithms_agree`] is: a
    /// structure where the two disagree is entirely readable, and a reader
    /// that refuses it cannot say what it refused.
    #[must_use]
    pub const fn version_matches_sid(&self) -> bool {
        match self.sid {
            SignerIdentifier::IssuerAndSerialNumber { .. } => self.version == 1,
            SignerIdentifier::SubjectKeyIdentifier(_) => self.version == 3,
        }
    }

    /// Which certificate the signer says signed this.
    #[must_use]
    pub const fn sid(&self) -> &SignerIdentifier<'a> {
        &self.sid
    }

    /// The `digestAlgorithm` identifier, as encoded.
    #[must_use]
    pub const fn digest_algorithm_id(&self) -> AlgorithmIdentifier<'a> {
        self.digest_algorithm
    }

    /// The digest the signer used.
    ///
    /// # Errors
    ///
    /// [`CmsError::UnknownDigestAlgorithm`], naming the dotted OID, for one
    /// this crate has no digest for.
    pub fn digest_algorithm(&self) -> Result<DigestAlgorithm, CmsError> {
        digest_algorithm(self.digest_algorithm.oid()).ok_or_else(|| {
            CmsError::UnknownDigestAlgorithm {
                oid: self.digest_algorithm.oid().to_dotted(),
            }
        })
    }

    /// The `signatureAlgorithm` identifier, as encoded.
    #[must_use]
    pub const fn signature_algorithm_id(&self) -> AlgorithmIdentifier<'a> {
        self.signature_algorithm
    }

    /// The signature algorithm.
    ///
    /// # Errors
    ///
    /// [`CmsError::UnknownSignatureAlgorithm`], naming the dotted OID.
    pub fn signature_algorithm(&self) -> Result<SignatureAlgorithm, CmsError> {
        signature_algorithm(self.signature_algorithm.oid()).ok_or_else(|| {
            CmsError::UnknownSignatureAlgorithm {
                oid: self.signature_algorithm.oid().to_dotted(),
            }
        })
    }

    /// The digest a verifier should use, resolving RFC 5754 §3.2's split.
    ///
    /// For bare `rsaEncryption` the `signatureAlgorithm` names no digest and
    /// the `digestAlgorithm` field is the answer; for `sha256WithRSAEncryption`
    /// it names one, and this returns *that* one — because it is the algorithm
    /// the signer's own encoding commits to, and a `SignerInfo` whose two
    /// fields disagree should not be checked under the field the verifier
    /// preferred.
    ///
    /// # Errors
    ///
    /// As [`SignerInfo::digest_algorithm`] and
    /// [`SignerInfo::signature_algorithm`].
    pub fn effective_digest(&self) -> Result<DigestAlgorithm, CmsError> {
        match self.signature_algorithm()? {
            SignatureAlgorithm::RsaPkcs1v15 { digest: Some(one) }
            | SignatureAlgorithm::Ecdsa { digest: one } => Ok(one),
            SignatureAlgorithm::RsaPkcs1v15 { digest: None } | SignatureAlgorithm::RsaPss => {
                self.digest_algorithm()
            }
        }
    }

    /// The signed attributes, where there are any.
    #[must_use]
    pub const fn signed_attrs(&self) -> Option<&Attributes<'a>> {
        self.signed_attrs.as_ref()
    }

    /// **The exact bytes RFC 5652 §5.4 says to digest**, or nothing when the
    /// signature is over the content directly.
    ///
    /// The stored `[0] IMPLICIT` tag octet replaced by the universal `SET`
    /// tag, and nothing else touched: the length octets are already minimal
    /// and the content octets are the signer's, attribute order included. See
    /// this module's header for what digesting the stored octet costs, and
    /// [`Attributes::stored_der`] for the bytes as they arrived.
    ///
    /// The only allocation on this path, and it is unavoidable: the byte to be
    /// digested differs from the byte in the buffer, so there is no slice of
    /// the input that is the right answer.
    #[must_use]
    pub fn signed_attrs_to_digest(&self) -> Option<Vec<u8>> {
        let stored = self.signed_attrs.as_ref()?.stored_der();
        let (_context_tag, rest) = stored.split_first()?;
        let mut out = Vec::with_capacity(stored.len());
        out.push(SET_OF_TAG);
        out.extend_from_slice(rest);
        Some(out)
    }

    /// The `signatureAlgorithm`'s output: the signature itself.
    ///
    /// An OCTET STRING in CMS rather than X.509's BIT STRING, so there is no
    /// unused-bit count to reason about.
    #[must_use]
    pub const fn signature(&self) -> &'a [u8] {
        self.signature
    }

    /// The unsigned attributes, where there are any.
    ///
    /// Nothing in them is covered by the signature — a countersignature or a
    /// timestamp is *added* after signing — so a caller must not read one as
    /// something the signer asserted.
    #[must_use]
    pub const fn unsigned_attrs(&self) -> Option<&Attributes<'a>> {
        self.unsigned_attrs.as_ref()
    }

    /// The signed `contentType` attribute (§11.1), which §5.3 requires to
    /// equal the `eContentType` when `signedAttrs` is present.
    ///
    /// Reported rather than checked; [`SignedData::content_type_matches`] is
    /// the check, asked separately.
    #[must_use]
    pub const fn content_type(&self) -> Option<Oid<'a>> {
        self.content_type
    }

    /// The signed `messageDigest` attribute (§11.2): the digest of the content
    /// the signer claims to have digested.
    ///
    /// **This is the value a PDF verdict compares against its own digest of
    /// the `/ByteRange` spans**, and it is why the exact bytes matter twice
    /// over — once here, and once in
    /// [`SignerInfo::signed_attrs_to_digest`].
    #[must_use]
    pub const fn message_digest(&self) -> Option<&'a [u8]> {
        self.message_digest
    }

    /// The signed `signingTime` attribute (§11.3), as Unix seconds.
    ///
    /// **The signer's claim and nothing more.** It is signed, so it was not
    /// altered afterwards; it was never checked against a clock in the first
    /// place, and a timestamp token is the thing that would make it evidence.
    #[must_use]
    pub const fn signing_time(&self) -> Option<i64> {
        self.signing_time
    }

    /// The signed `signingCertificateV2` attribute (RFC 5035 §3).
    #[must_use]
    pub const fn signing_certificate_v2(&self) -> Option<&SigningCertificateV2<'a>> {
        self.signing_certificate_v2.as_ref()
    }

    /// The RFC 3161 timestamp tokens in the unsigned attributes, as DER.
    ///
    /// **Surfaced, never evaluated**, which `docs/design/signatures.md` makes
    /// an explicit non-goal. Each is itself a `ContentInfo`, so
    /// [`ContentInfo::parse`] reads one and the authority's name and claimed
    /// time come out of the token's own `SignerInfo` and `eContent` — this
    /// module reads a timestamp with itself, and validating the authority's
    /// chain is a later tier.
    #[must_use]
    pub fn timestamp_tokens(&self) -> &[&'a [u8]] {
        &self.timestamp_tokens
    }

    /// The whole `SignerInfo` encoding.
    #[must_use]
    pub const fn der(&self) -> &'a [u8] {
        self.der
    }
}

/// The universal `SET` identifier octet: class 0, constructed, tag 17.
///
/// Derived rather than written as `0x31`, so the one byte RFC 5652 §5.4 turns
/// on is tied to [`Tag::Set`] rather than to a number in a comment.
const SET_OF_TAG: u8 = 0x20 | (Tag::Set.number() as u8);

/// A `SignedData` (§5.1).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SignedData<'a> {
    version: u64,
    digest_algorithms: Vec<AlgorithmIdentifier<'a>>,
    encap_content_info: EncapsulatedContent<'a>,
    certificates: Vec<CertificateChoice<'a>>,
    crls: Vec<RevocationChoice<'a>>,
    signer_infos: Vec<SignerInfo<'a>>,
    der: &'a [u8],
}

impl<'a> SignedData<'a> {
    fn parse(tlv: &Tlv<'a>, budget: &Budget) -> Result<Self, CmsError> {
        tlv.require(Tag::Sequence)?;
        let mut fields = tlv.children(budget)?;
        let version = fields.expect(Tag::Integer)?.as_integer()?.as_u64()?;

        let mut digest_algorithms = Vec::new();
        let mut set = fields.expect(Tag::Set)?.children(budget)?;
        while !set.is_empty() {
            digest_algorithms.push(AlgorithmIdentifier::parse(&set.read()?, budget)?);
        }

        let encap_content_info =
            EncapsulatedContent::parse(&fields.expect(Tag::Sequence)?, budget)?;

        // `certificates [0] IMPLICIT CertificateSet OPTIONAL` and
        // `crls [1] IMPLICIT RevocationInfoChoices OPTIONAL`. Both implicit,
        // so the context tag replaces the SET's own and the children are the
        // members directly.
        let mut certificates = Vec::new();
        if let Some(tagged) = fields.context_optional(0)? {
            let mut members = tagged.children(budget)?;
            while !members.is_empty() {
                let node = members.read()?;
                certificates.push(match (node.class(), node.tag()) {
                    (Class::Universal, tag) if tag == Tag::Sequence.number() => {
                        CertificateChoice::X509(node.raw())
                    }
                    (_, tag) => CertificateChoice::Other {
                        tag,
                        der: node.raw(),
                    },
                });
            }
        }
        let mut crls = Vec::new();
        if let Some(tagged) = fields.context_optional(1)? {
            let mut members = tagged.children(budget)?;
            while !members.is_empty() {
                let node = members.read()?;
                crls.push(match (node.class(), node.tag()) {
                    (Class::Universal, tag) if tag == Tag::Sequence.number() => {
                        RevocationChoice::Crl(node.raw())
                    }
                    (_, tag) => RevocationChoice::Other {
                        tag,
                        der: node.raw(),
                    },
                });
            }
        }

        let mut signer_infos = Vec::new();
        let mut set = fields.expect(Tag::Set)?.children(budget)?;
        while !set.is_empty() {
            signer_infos.push(SignerInfo::parse(&set.read()?, budget)?);
        }
        fields.finish()?;

        Ok(Self {
            version,
            digest_algorithms,
            encap_content_info,
            certificates,
            crls,
            signer_infos,
            der: tlv.raw(),
        })
    }

    /// The `version` field (§5.1), surfaced rather than acted on. See this
    /// module's header for why it gates nothing.
    #[must_use]
    pub const fn version(&self) -> u64 {
        self.version
    }

    /// The `digestAlgorithms` set, in encoded order.
    ///
    /// §5.1 makes it the digests of all the signers, so that a reader can
    /// digest the content once per algorithm before it looks at a signer. It
    /// is a SHOULD, and one corpus blob declares SHA-1 while its only signer
    /// uses SHA-256 — which is why nothing here reads a signer's digest out of
    /// this list.
    #[must_use]
    pub fn digest_algorithms(&self) -> &[AlgorithmIdentifier<'a>] {
        &self.digest_algorithms
    }

    /// Whether the `digestAlgorithms` set lists an algorithm, compared as
    /// OIDs.
    #[must_use]
    pub fn declares(&self, algorithm: Oid<'_>) -> bool {
        self.digest_algorithms
            .iter()
            .any(|listed| listed.oid().as_bytes() == algorithm.as_bytes())
    }

    /// What was signed, and whether it is carried here.
    #[must_use]
    pub const fn encap_content_info(&self) -> EncapsulatedContent<'a> {
        self.encap_content_info
    }

    /// The certificates, **located and not parsed**.
    ///
    /// A `CertificateSet` is a bag a producer fills for the reader's
    /// convenience; it is not part of what was signed, it routinely carries
    /// intermediates from issuers this reader has never met, and one of them
    /// failing to parse is not a reason to refuse the signature that the other
    /// three would have supported. So each arrives as its own DER and the
    /// caller runs [`crate::x509::Certificate::parse`] over it, getting one
    /// typed refusal per certificate instead of one for the message.
    #[must_use]
    pub fn certificates(&self) -> &[CertificateChoice<'a>] {
        &self.certificates
    }

    /// Just the X.509 members, as DER, which is what a chain builder wants.
    pub fn x509_certificates(&self) -> impl Iterator<Item = &'a [u8]> + '_ {
        self.certificates.iter().filter_map(|choice| match choice {
            CertificateChoice::X509(der) => Some(*der),
            CertificateChoice::Other { .. } => None,
        })
    }

    /// The revocation information, surfaced and never evaluated.
    #[must_use]
    pub fn crls(&self) -> &[RevocationChoice<'a>] {
        &self.crls
    }

    /// The signers. Possibly none: see this module's header.
    #[must_use]
    pub fn signer_infos(&self) -> &[SignerInfo<'a>] {
        &self.signer_infos
    }

    /// Whether a signer's signed `contentType` attribute equals the
    /// `eContentType`, which §5.3 requires when `signedAttrs` is present.
    ///
    /// `None` where the signer has no `contentType` attribute, which is only
    /// legal when it has no `signedAttrs` at all.
    #[must_use]
    pub fn content_type_matches(&self, signer: &SignerInfo<'a>) -> Option<bool> {
        let signed = signer.content_type()?;
        Some(signed.as_bytes() == self.encap_content_info.content_type.as_bytes())
    }

    /// The whole `SignedData` encoding.
    #[must_use]
    pub const fn der(&self) -> &'a [u8] {
        self.der
    }
}

/// A `ContentInfo` (§3), the outermost structure of a PDF signature's
/// `/Contents`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ContentInfo<'a> {
    content_type: Oid<'a>,
    signed_data: SignedData<'a>,
    der: &'a [u8],
}

impl<'a> ContentInfo<'a> {
    /// Reads a `ContentInfo` under [`Limits::CMS`].
    ///
    /// # Errors
    ///
    /// See [`CmsError`]. Everything here is attacker-supplied — twice over for
    /// a PDF, once by whoever wrote the document and once by whoever edited it
    /// afterwards — so every path out of this is a value, never a panic
    /// (ruling 1).
    pub fn parse(der: &'a [u8]) -> Result<Self, CmsError> {
        Self::parse_with(der, Limits::CMS)
    }

    /// Reads a `ContentInfo` under ceilings of the caller's choosing.
    ///
    /// # Errors
    ///
    /// See [`CmsError`].
    pub fn parse_with(der: &'a [u8], limits: Limits) -> Result<Self, CmsError> {
        let budget = Budget::new(limits);
        let mut outer = Cursor::new(der, &budget);
        let info = outer.expect(Tag::Sequence)?;
        if !outer.is_empty() {
            return Err(CmsError::TrailingBytes);
        }

        let mut fields = info.children(&budget)?;
        let content_type = fields.expect(Tag::Oid)?.as_oid()?;
        if content_type != oid::ID_SIGNED_DATA {
            return Err(CmsError::UnsupportedContentType {
                oid: content_type.to_dotted(),
            });
        }
        // `content [0] EXPLICIT ANY DEFINED BY contentType`.
        let tagged = fields
            .context_optional(0)?
            .ok_or(CmsError::Der(DerError::UnexpectedEnd))?;
        let signed_data = SignedData::parse(&tagged.explicit(&budget)?, &budget)?;
        fields.finish()?;

        Ok(Self {
            content_type,
            signed_data,
            der: info.raw(),
        })
    }

    /// The content type, which [`ContentInfo::parse`] has already established
    /// is `id-signedData`.
    #[must_use]
    pub const fn content_type(&self) -> Oid<'a> {
        self.content_type
    }

    /// The `SignedData`.
    #[must_use]
    pub const fn signed_data(&self) -> &SignedData<'a> {
        &self.signed_data
    }

    /// The whole `ContentInfo` encoding.
    #[must_use]
    pub const fn der(&self) -> &'a [u8] {
        self.der
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::der::tests::unhex;
    use tinker_pdf_crypto::sha2::sha256;

    // ---- a DER writer, for fixtures ---------------------------------------
    //
    // Every fixture below is *built* rather than transcribed, which is a
    // deliberate trade and worth stating because it has a hole in it.
    //
    // Building means no fixture can carry a mistyped length: the writer
    // computes every one, so a structure is right by construction and a test
    // that changes shape does not need its offsets re-counted. That is what
    // makes it possible to have a fixture for every shape at all — including
    // the four the fetched corpora contain none of (a `subjectKeyIdentifier`
    // signer, a CRL, an empty attribute set, a duplicated attribute).
    //
    // The hole is circularity: a parser checked only against its own encoder
    // agrees with itself. Three things close it, and none of them is in this
    // module. `crates/tinker-pdf/tests/cms_census.rs` reads eighteen real
    // blobs from six independent producers; that census **verifies a real
    // corpus signature** through `signed_attrs_to_digest`, and asserts the
    // same signature fails when the stored `[0]` bytes are digested instead —
    // which is the §5.4 rule checked against bytes a real signer produced
    // rather than against this file's opinion of them. And
    // `fuzz/fuzz_targets/pki_cms.rs` puts arbitrary bytes through the same
    // path.

    /// Tag, length, value, with the length in the shortest form DER admits.
    fn tlv(tag: u8, value: &[u8]) -> Vec<u8> {
        let mut out = vec![tag];
        let length = value.len();
        if length < 0x80 {
            out.push(length as u8);
        } else {
            let bytes = length.to_be_bytes();
            let first = bytes
                .iter()
                .position(|byte| *byte != 0)
                .unwrap_or(bytes.len() - 1);
            let significant = &bytes[first..];
            out.push(0x80 | significant.len() as u8);
            out.extend_from_slice(significant);
        }
        out.extend_from_slice(value);
        out
    }

    fn cat(parts: &[Vec<u8>]) -> Vec<u8> {
        parts.concat()
    }

    /// Tag, X.690 §8.1.3.6's indefinite length, value, end-of-contents pair.
    ///
    /// The other half of the writer, and the only way to build the shape four
    /// real producers emit. Nesting one of these inside another is exactly
    /// what `160F-2019.pdf` does five levels over.
    fn indefinite(tag: u8, value: &[u8]) -> Vec<u8> {
        let mut out = vec![tag, 0x80];
        out.extend_from_slice(value);
        out.extend_from_slice(&[0x00, 0x00]);
        out
    }

    fn seq(parts: &[Vec<u8>]) -> Vec<u8> {
        tlv(0x30, &cat(parts))
    }

    fn set(parts: &[Vec<u8>]) -> Vec<u8> {
        tlv(0x31, &cat(parts))
    }

    fn context(n: u8, constructed: bool, value: &[u8]) -> Vec<u8> {
        tlv(0x80 | if constructed { 0x20 } else { 0 } | n, value)
    }

    fn oid_of(value: Oid<'_>) -> Vec<u8> {
        tlv(0x06, value.as_bytes())
    }

    fn int(value: u64) -> Vec<u8> {
        let bytes = value.to_be_bytes();
        let first = bytes.iter().position(|byte| *byte != 0).unwrap_or(7);
        let mut content = bytes[first..].to_vec();
        if content.first().is_some_and(|byte| byte & 0x80 != 0) {
            content.insert(0, 0x00);
        }
        tlv(0x02, &content)
    }

    fn octets(value: &[u8]) -> Vec<u8> {
        tlv(0x04, value)
    }

    fn utc(text: &str) -> Vec<u8> {
        tlv(0x17, text.as_bytes())
    }

    fn null() -> Vec<u8> {
        tlv(0x05, &[])
    }

    fn algorithm(value: Oid<'_>) -> Vec<u8> {
        seq(&[oid_of(value), null()])
    }

    fn attribute(kind: Oid<'_>, values: &[Vec<u8>]) -> Vec<u8> {
        seq(&[oid_of(kind), set(values)])
    }

    /// `smimeCapabilities` (1.2.840.113549.1.9.15).
    ///
    /// Written out here rather than added to [`crate::oid`], because that
    /// module's rule is that a name with no decoder behind it reads like
    /// support — and having no decoder is exactly why this attribute is the
    /// right one to hide an indefinite length inside.
    const SMIME_CAPABILITIES: Oid<'static> =
        Oid::from_content(&[0x2A, 0x86, 0x48, 0x86, 0xF7, 0x0D, 0x01, 0x09, 0x0F]);

    /// `CN=<name>`, the smallest well-formed distinguished name.
    fn common_name(text: &str) -> Vec<u8> {
        seq(&[set(&[seq(&[
            oid_of(oid::AT_COMMON_NAME),
            tlv(0x13, text.as_bytes()),
        ])])])
    }

    /// A certificate-shaped SEQUENCE. Not a certificate: this module locates
    /// members of the certificate set and never reads one, so a placeholder
    /// with the right tag is exactly as much as these tests are entitled to
    /// assume.
    fn certificate_shaped(marker: u8) -> Vec<u8> {
        seq(&[int(u64::from(marker)), common_name("Some Issuer")])
    }

    /// How the fixtures below differ from each other.
    #[derive(Clone)]
    struct Build {
        content_type: Oid<'static>,
        e_content: Option<Vec<u8>>,
        certificates: Vec<Vec<u8>>,
        crls: Vec<Vec<u8>>,
        signed_attrs: Option<Vec<Vec<u8>>>,
        unsigned_attrs: Option<Vec<Vec<u8>>>,
        sid: Vec<u8>,
        digest_algorithm: Oid<'static>,
        signature_algorithm: Oid<'static>,
        signer_version: u64,
        /// Write the five structural nodes the corpus's BER blobs write with
        /// indefinite lengths: the `ContentInfo`, its `[0]`, the `SignedData`,
        /// the `EncapsulatedContentInfo` and the certificate set.
        ber_scaffolding: bool,
        /// Write the `signedAttrs` `[0]` node itself with an indefinite
        /// length. RFC 5652 §5.4 forbids it; this builds one anyway.
        ber_signed_attrs: bool,
        /// Add an attribute this crate has no decoder for, whose value nests
        /// an indefinite length two levels down — the case a walk that does
        /// not descend into unrecognised values cannot see.
        ber_inside_an_attribute_value: bool,
    }

    impl Default for Build {
        fn default() -> Self {
            Self {
                content_type: oid::ID_SIGNED_DATA,
                e_content: None,
                certificates: vec![certificate_shaped(1)],
                crls: Vec::new(),
                signed_attrs: Some(vec![
                    attribute(oid::AA_CONTENT_TYPE, &[oid_of(oid::ID_DATA)]),
                    attribute(oid::AA_SIGNING_TIME, &[utc("260101120000Z")]),
                    attribute(oid::AA_MESSAGE_DIGEST, &[octets(&[0xAB; 32])]),
                ]),
                unsigned_attrs: None,
                sid: seq(&[common_name("Example CA"), int(0x2A)]),
                digest_algorithm: oid::ID_SHA256,
                signature_algorithm: oid::RSA_ENCRYPTION,
                signer_version: 1,
                ber_scaffolding: false,
                ber_signed_attrs: false,
                ber_inside_an_attribute_value: false,
            }
        }
    }

    impl Build {
        fn build(&self) -> Vec<u8> {
            // The scaffolding writer: definite by default, and the five nodes
            // the corpus writes indefinitely when asked.
            let ber = self.ber_scaffolding;
            let scaffold_seq = |parts: &[Vec<u8>]| {
                if ber {
                    indefinite(0x30, &cat(parts))
                } else {
                    seq(parts)
                }
            };
            let scaffold_context = |n: u8, value: &[u8]| {
                if ber {
                    indefinite(0xA0 | n, value)
                } else {
                    context(n, true, value)
                }
            };

            let mut encap = vec![oid_of(oid::ID_DATA)];
            if let Some(content) = &self.e_content {
                encap.push(context(0, true, &octets(content)));
            }

            let mut attributes = self.signed_attrs.clone();
            if self.ber_inside_an_attribute_value {
                // `smimeCapabilities` stands in for "an attribute with no
                // decoder here": the parser reads its `SET OF` values as
                // located nodes and never looks inside one.
                let hidden = seq(&[indefinite(0x30, &seq(&[null()]))]);
                attributes
                    .get_or_insert_with(Vec::new)
                    .push(attribute(SMIME_CAPABILITIES, &[hidden]));
            }

            let mut signer = vec![
                int(self.signer_version),
                self.sid.clone(),
                algorithm(self.digest_algorithm),
            ];
            if let Some(attributes) = &attributes {
                let body = cat(attributes);
                signer.push(if self.ber_signed_attrs {
                    indefinite(0xA0, &body)
                } else {
                    context(0, true, &body)
                });
            }
            signer.push(algorithm(self.signature_algorithm));
            signer.push(octets(&[0xCD; 8]));
            if let Some(attributes) = &self.unsigned_attrs {
                signer.push(context(1, true, &cat(attributes)));
            }

            let mut body = vec![
                int(1),
                set(&[algorithm(self.digest_algorithm)]),
                scaffold_seq(&encap),
            ];
            if !self.certificates.is_empty() {
                body.push(scaffold_context(0, &cat(&self.certificates)));
            }
            if !self.crls.is_empty() {
                body.push(context(1, true, &cat(&self.crls)));
            }
            body.push(set(&[seq(&signer)]));

            scaffold_seq(&[
                oid_of(self.content_type),
                scaffold_context(0, &scaffold_seq(&body)),
            ])
        }
    }

    fn fixture() -> Vec<u8> {
        Build::default().build()
    }

    // ---- what a `SignedData` reads back as --------------------------------

    #[test]
    fn a_detached_signed_data_reads_back_every_field() {
        let der = fixture();
        let info = ContentInfo::parse(&der).expect("the fixture parses");
        assert_eq!(info.content_type(), oid::ID_SIGNED_DATA);
        assert_eq!(info.der(), der.as_slice());

        let signed = info.signed_data();
        assert_eq!(signed.version(), 1);
        assert_eq!(signed.digest_algorithms().len(), 1);
        assert!(signed.declares(oid::ID_SHA256));
        assert!(!signed.declares(oid::ID_SHA1));

        let encap = signed.encap_content_info();
        assert_eq!(encap.content_type(), oid::ID_DATA);
        assert_eq!(encap.content(), None);
        assert!(
            encap.is_detached(),
            "`adbe.pkcs7.detached` is the commoner shape and carries no content"
        );

        assert_eq!(signed.certificates().len(), 1);
        assert_eq!(signed.x509_certificates().count(), 1);
        assert!(signed.crls().is_empty());
        assert_eq!(signed.signer_infos().len(), 1);

        let signer = &signed.signer_infos()[0];
        assert_eq!(signer.version(), 1);
        assert!(signer.version_matches_sid());
        match signer.sid() {
            SignerIdentifier::IssuerAndSerialNumber { issuer, serial, .. } => {
                assert_eq!(issuer.common_name(), Some("Example CA"));
                assert_eq!(serial.as_u64(), Ok(0x2A));
            }
            other => panic!("expected an issuer and serial, got {other:?}"),
        }
        assert_eq!(signer.digest_algorithm(), Ok(DigestAlgorithm::Sha256));
        assert_eq!(
            signer.signature_algorithm(),
            Ok(SignatureAlgorithm::RsaPkcs1v15 { digest: None })
        );
        // Bare `rsaEncryption` names no digest, so RFC 5754 §3.2 sends the
        // question to the `digestAlgorithm` field.
        assert_eq!(signer.effective_digest(), Ok(DigestAlgorithm::Sha256));
        assert_eq!(signer.signature(), &[0xCD; 8]);

        assert_eq!(signer.content_type(), Some(oid::ID_DATA));
        assert_eq!(signed.content_type_matches(signer), Some(true));
        assert_eq!(signer.message_digest(), Some(&[0xABu8; 32][..]));
        // 2026-01-01T12:00:00Z.
        assert_eq!(signer.signing_time(), Some(1_767_268_800));
        assert_eq!(signer.signed_attrs().map(|a| a.all().len()), Some(3));
        assert!(signer.unsigned_attrs().is_none());
        assert!(signer.timestamp_tokens().is_empty());
    }

    /// **The test this milestone exists for.**
    ///
    /// RFC 5652 §5.4 replaces the stored `[0] IMPLICIT` tag with a universal
    /// `SET OF` tag before digesting. Every assertion here fails if the stored
    /// tag is carried through, and the last one is the one that matters: the
    /// two digests are different numbers, so a verifier that digests the
    /// stored bytes gets a signature that never verifies and no clue why.
    #[test]
    fn the_signed_attributes_are_digested_as_a_set_of_not_as_a_context_tag() {
        let der = fixture();
        let info = ContentInfo::parse(&der).expect("parses");
        let signer = &info.signed_data().signer_infos()[0];

        let stored = signer.signed_attrs().expect("there are some").stored_der();
        let digested = signer.signed_attrs_to_digest().expect("there are some");

        assert_eq!(
            stored.first(),
            Some(&0xA0),
            "the field is stored as `[0]` constructed, which is what §5.4 is about"
        );
        assert_eq!(
            digested.first(),
            Some(&0x31),
            "and digested as a universal constructed SET"
        );
        assert_eq!(SET_OF_TAG, 0x31, "X.690 §8.4: universal, constructed, 17");

        // Only the identifier octet moves. The length octets were already
        // minimal — the walker refused them otherwise — and the content octets
        // are the signer's, in the signer's order.
        assert_eq!(
            &digested[1..],
            &stored[1..],
            "nothing but the tag octet may change"
        );
        assert_eq!(digested.len(), stored.len());

        // The whole point, stated as a number: digesting the stored bytes is a
        // different digest, and nothing downstream could tell you why.
        assert_ne!(
            sha256(&digested),
            sha256(stored),
            "if these were equal this rule would not matter"
        );

        // And the re-encoding is a well-formed `SET OF Attribute` in its own
        // right, which is the property that makes it digestible at all.
        let budget = Budget::new(Limits::CMS);
        let mut cursor = Cursor::new(&digested, &budget);
        let node = cursor.expect(Tag::Set).expect("a universal SET");
        assert!(cursor.finish().is_ok(), "and nothing after it");
        assert_eq!(node.raw().len(), stored.len(), "one tag apart, no more");
        let mut inner = node.children(&budget).expect("constructed");
        let mut count = 0usize;
        while !inner.is_empty() {
            inner.read().expect("an attribute");
            count += 1;
        }
        assert_eq!(count, 3, "holding the three attributes it held before");
    }

    /// The `messageDigest` attribute comes out of the exact DER and can be
    /// re-digested from it, which is milestone 3's exit criterion.
    #[test]
    fn the_message_digest_attribute_is_reachable_from_the_exact_der() {
        // A content whose digest is the value the attribute carries, so the
        // relationship a verifier checks is the relationship asserted here.
        let content = b"the bytes a signature is over".to_vec();
        let digest = sha256(&content);
        let build = Build {
            e_content: Some(content.clone()),
            signed_attrs: Some(vec![
                attribute(oid::AA_CONTENT_TYPE, &[oid_of(oid::ID_DATA)]),
                attribute(oid::AA_MESSAGE_DIGEST, &[octets(&digest)]),
            ]),
            ..Build::default()
        };
        let der = build.build();
        let info = ContentInfo::parse(&der).expect("parses");
        let signed = info.signed_data();
        let signer = &signed.signer_infos()[0];

        let carried = signed
            .encap_content_info()
            .content()
            .expect("`adbe.pkcs7.sha1`'s shape carries its content");
        assert_eq!(carried, content.as_slice());
        assert_eq!(
            signer.message_digest(),
            Some(&digest[..]),
            "the attribute holds the digest of the encapsulated content"
        );
        assert_eq!(
            sha256(carried).as_slice(),
            signer.message_digest().expect("present"),
            "and re-digesting the content the parser located reproduces it"
        );

        // Both slices point into the buffer that was parsed rather than into a
        // copy, which is what makes "the exact DER" mean anything: a value
        // re-encoded from what was read would compare equal here and would
        // still be the wrong bytes to digest.
        let within = |slice: &[u8]| {
            let base = der.as_ptr().addr();
            let at = slice.as_ptr().addr();
            at >= base && at.saturating_add(slice.len()) <= base.saturating_add(der.len())
        };
        assert!(within(carried), "the content is a view over the input");
        assert!(within(signer.message_digest().expect("present")));

        // And the signed attributes name their own byte range, the way
        // `Certificate::tbs_range` does, so a caller can quote the span it
        // digested rather than describe it.
        let attributes = signer.signed_attrs().expect("present");
        assert!(within(attributes.stored_der()));
        assert_eq!(&der[attributes.stored_range()], attributes.stored_der());
    }

    // ---- typed refusals ---------------------------------------------------

    #[test]
    fn an_unknown_content_type_is_refused_by_name() {
        // `id-envelopedData` (1.2.840.113549.1.7.3): a real CMS content type,
        // and not one whose `[0]` holds a `SignedData`.
        let enveloped = Oid::from_content(&[0x2A, 0x86, 0x48, 0x86, 0xF7, 0x0D, 0x01, 0x07, 0x03]);
        let der = Build {
            content_type: enveloped,
            ..Build::default()
        }
        .build();
        assert_eq!(
            ContentInfo::parse(&der),
            Err(CmsError::UnsupportedContentType {
                oid: "1.2.840.113549.1.7.3".to_string()
            })
        );
    }

    #[test]
    fn an_unknown_digest_algorithm_is_refused_by_name_and_the_rest_still_reads() {
        // `id-sha3-256` (2.16.840.1.101.3.4.2.8): real, current, and not
        // something this crate has a digest for.
        let sha3 = Oid::from_content(&[0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x02, 0x08]);
        let der = Build {
            digest_algorithm: sha3,
            ..Build::default()
        }
        .build();
        let info = ContentInfo::parse(&der).expect("the structure is fine; the algorithm is not");
        let signer = &info.signed_data().signer_infos()[0];
        assert_eq!(
            signer.digest_algorithm(),
            Err(CmsError::UnknownDigestAlgorithm {
                oid: "2.16.840.1.101.3.4.2.8".to_string()
            })
        );
        // The refusal is at the accessor, so everything else is still
        // reportable — which is the whole reason it is not at the parse.
        assert_eq!(signer.message_digest(), Some(&[0xABu8; 32][..]));
        assert_eq!(
            signer.digest_algorithm_id().oid().to_dotted(),
            "2.16.840.1.101.3.4.2.8"
        );
    }

    #[test]
    fn an_unknown_signature_algorithm_is_refused_by_name() {
        // Ed25519 (1.3.101.112), which RFC 8419 defines for CMS and which this
        // crate has no verifier for.
        let ed25519 = Oid::from_content(&[0x2B, 0x65, 0x70]);
        let der = Build {
            signature_algorithm: ed25519,
            ..Build::default()
        }
        .build();
        let info = ContentInfo::parse(&der).expect("parses");
        let signer = &info.signed_data().signer_infos()[0];
        assert_eq!(
            signer.signature_algorithm(),
            Err(CmsError::UnknownSignatureAlgorithm {
                oid: "1.3.101.112".to_string()
            })
        );
        assert_eq!(
            signer.effective_digest(),
            Err(CmsError::UnknownSignatureAlgorithm {
                oid: "1.3.101.112".to_string()
            })
        );
    }

    /// RSASSA-PSS is named rather than decoded, and the difference is visible.
    #[test]
    fn rsassa_pss_is_named_and_its_digest_falls_back_to_the_signer_field() {
        let der = Build {
            signature_algorithm: oid::RSASSA_PSS,
            ..Build::default()
        }
        .build();
        let info = ContentInfo::parse(&der).expect("parses");
        let signer = &info.signed_data().signer_infos()[0];
        assert_eq!(signer.signature_algorithm(), Ok(SignatureAlgorithm::RsaPss));
        assert_eq!(signer.effective_digest(), Ok(DigestAlgorithm::Sha256));
    }

    #[test]
    fn the_sha_with_rsa_oids_name_their_own_digest() {
        for (algorithm, expected) in [
            (oid::SHA1_WITH_RSA, DigestAlgorithm::Sha1),
            (oid::SHA256_WITH_RSA, DigestAlgorithm::Sha256),
            (oid::SHA384_WITH_RSA, DigestAlgorithm::Sha384),
            (oid::SHA512_WITH_RSA, DigestAlgorithm::Sha512),
        ] {
            let der = Build {
                signature_algorithm: algorithm,
                // Deliberately disagreeing with the OID above, so that the
                // answer shows which field was consulted.
                digest_algorithm: oid::ID_SHA1,
                ..Build::default()
            }
            .build();
            let info = ContentInfo::parse(&der).expect("parses");
            let signer = &info.signed_data().signer_infos()[0];
            assert_eq!(
                signer.effective_digest(),
                Ok(expected),
                "the signature algorithm's own digest wins where it names one"
            );
            assert_eq!(signer.digest_algorithm(), Ok(DigestAlgorithm::Sha1));
        }
    }

    // ---- BER, which four real producers emit and this module reads --------

    /// The shape the four corpus blobs open with, read to the same values the
    /// definite-length fixture reads to.
    ///
    /// The point is not that it parses. It is that the *same* structure in the
    /// *other* encoding produces the same answers, so nothing above this
    /// module has to know which one arrived.
    #[test]
    fn a_ber_signed_data_reads_back_the_same_values_as_its_der_twin() {
        let der = fixture();
        let ber = Build {
            ber_scaffolding: true,
            ..Build::default()
        }
        .build();
        assert_eq!(
            &ber[..15],
            &unhex("30 80 06 09 2A 86 48 86 F7 0D 01 07 02 A0 80")[..],
            "the exact fifteen octets all four corpus blobs open with: an \
             indefinite SEQUENCE, `id-signedData`, an indefinite `[0]`"
        );
        assert_ne!(ber, der, "and the two encodings really are different bytes");

        let from_ber = ContentInfo::parse(&ber).expect("BER SignedData parses");
        let from_der = ContentInfo::parse(&der).expect("its DER twin parses");

        let (left, right) = (from_ber.signed_data(), from_der.signed_data());
        assert_eq!(left.version(), right.version());
        assert_eq!(
            left.encap_content_info().content_type(),
            right.encap_content_info().content_type()
        );
        assert!(left.encap_content_info().is_detached());
        assert_eq!(
            left.x509_certificates().collect::<Vec<_>>(),
            right.x509_certificates().collect::<Vec<_>>(),
            "a certificate located inside a BER message is the same DER"
        );

        let (a, b) = (&left.signer_infos()[0], &right.signer_infos()[0]);
        assert_eq!(a.message_digest(), b.message_digest());
        assert_eq!(a.signing_time(), b.signing_time());
        assert_eq!(a.signature(), b.signature());
        assert_eq!(
            a.signed_attrs_to_digest(),
            b.signed_attrs_to_digest(),
            "**the assertion that matters**: the bytes §5.4 digests do not \
             depend on how the message around them was encoded"
        );
    }

    /// The opt-in is an opt-in: the same bytes, refused under DER ceilings.
    #[test]
    fn the_same_ber_message_is_refused_under_certificate_limits() {
        let ber = Build {
            ber_scaffolding: true,
            ..Build::default()
        }
        .build();
        assert_eq!(
            ContentInfo::parse_with(&ber, Limits::CERTIFICATE),
            Err(CmsError::Der(DerError::IndefiniteLength)),
            "`Limits::CMS` is the only constant that turns the form on"
        );
        // A compile-time check, so a constant that changed would fail the
        // build rather than one test.
        const {
            assert!(
                !Limits::CERTIFICATE.allow_indefinite_lengths,
                "an X.509 path stays DER-only however it was reached"
            );
            assert!(Limits::CMS.allow_indefinite_lengths);
        }
    }

    /// RFC 5652 §5.4, refused by its own name rather than by the walker's.
    #[test]
    fn an_indefinite_length_signed_attributes_set_is_refused_by_name() {
        let der = Build {
            ber_signed_attrs: true,
            ..Build::default()
        }
        .build();
        assert_eq!(
            ContentInfo::parse(&der),
            Err(CmsError::IndefiniteSignedAttributes),
            "a signature is computed over the DER of the attribute set, so a \
             set with two encodings has no digest to agree about"
        );
        // And with the rest of the message in BER too, so the refusal is about
        // the attributes rather than about the envelope.
        let both = Build {
            ber_scaffolding: true,
            ber_signed_attrs: true,
            ..Build::default()
        }
        .build();
        assert_eq!(
            ContentInfo::parse(&both),
            Err(CmsError::IndefiniteSignedAttributes)
        );
    }

    /// The case a walk that does not descend into unrecognised values cannot
    /// see, which is why the check is a sweep of its own.
    #[test]
    fn an_indefinite_length_hidden_in_an_unread_attribute_value_is_refused() {
        let der = Build {
            ber_inside_an_attribute_value: true,
            ..Build::default()
        }
        .build();
        assert_eq!(
            ContentInfo::parse(&der),
            Err(CmsError::IndefiniteSignedAttributes),
            "two levels inside an attribute this crate has no decoder for is \
             still inside what §5.4 digests"
        );

        // The same attribute with definite lengths parses, so what the test
        // above caught is the encoding and not the attribute.
        let harmless = Build {
            signed_attrs: Some(vec![
                attribute(oid::AA_CONTENT_TYPE, &[oid_of(oid::ID_DATA)]),
                attribute(oid::AA_MESSAGE_DIGEST, &[octets(&[0xAB; 32])]),
                attribute(SMIME_CAPABILITIES, &[seq(&[seq(&[null()])])]),
            ]),
            ..Build::default()
        }
        .build();
        let info = ContentInfo::parse(&harmless).expect("parses");
        assert_eq!(
            info.signed_data().signer_infos()[0]
                .signed_attrs()
                .expect("there are some")
                .all()
                .len(),
            3
        );
    }

    /// `unsignedAttrs` is deliberately *not* held to §5.4's rule, because
    /// nothing digests it.
    #[test]
    fn an_indefinite_length_in_unsigned_attributes_is_read_rather_than_refused() {
        let der = Build {
            ber_scaffolding: true,
            unsigned_attrs: Some(vec![attribute(
                SMIME_CAPABILITIES,
                &[seq(&[indefinite(0x30, &seq(&[null()]))])],
            )]),
            ..Build::default()
        }
        .build();
        let info = ContentInfo::parse(&der).expect("parses");
        let signer = &info.signed_data().signer_infos()[0];
        assert_eq!(
            signer.unsigned_attrs().map(|a| a.all().len()),
            Some(1),
            "narrowing this would refuse a legal message for no property"
        );
        assert!(signer.signed_attrs_to_digest().is_some());
    }

    /// A `SignedData` whose terminator never arrives is refused by name, not
    /// read to the end of the buffer.
    #[test]
    fn an_unterminated_ber_message_is_refused_rather_than_read_to_the_end() {
        let mut ber = Build {
            ber_scaffolding: true,
            ..Build::default()
        }
        .build();
        // Drop the outermost end-of-contents pair.
        ber.truncate(ber.len().saturating_sub(2));
        assert_eq!(
            ContentInfo::parse(&ber),
            Err(CmsError::Der(DerError::UnterminatedIndefiniteLength))
        );
    }

    #[test]
    fn bytes_after_the_content_info_are_refused() {
        let mut der = fixture();
        der.push(0x00);
        assert_eq!(ContentInfo::parse(&der), Err(CmsError::TrailingBytes));
    }

    #[test]
    fn an_empty_signed_attributes_set_is_refused() {
        // `SET SIZE (1..MAX)`, and a signature over the encoding of nothing.
        let der = Build {
            signed_attrs: Some(Vec::new()),
            ..Build::default()
        }
        .build();
        assert_eq!(ContentInfo::parse(&der), Err(CmsError::EmptyAttributes));
    }

    #[test]
    fn a_second_message_digest_attribute_is_refused() {
        let der = Build {
            signed_attrs: Some(vec![
                attribute(oid::AA_CONTENT_TYPE, &[oid_of(oid::ID_DATA)]),
                attribute(oid::AA_MESSAGE_DIGEST, &[octets(&[0x11; 32])]),
                attribute(oid::AA_MESSAGE_DIGEST, &[octets(&[0x22; 32])]),
            ]),
            ..Build::default()
        }
        .build();
        assert_eq!(
            ContentInfo::parse(&der),
            Err(CmsError::DuplicateAttribute {
                oid: "1.2.840.113549.1.9.4".to_string()
            }),
            "which of the two was signed is not a question to answer by picking"
        );
    }

    #[test]
    fn a_message_digest_attribute_with_two_values_is_refused() {
        let der = Build {
            signed_attrs: Some(vec![
                attribute(oid::AA_CONTENT_TYPE, &[oid_of(oid::ID_DATA)]),
                attribute(
                    oid::AA_MESSAGE_DIGEST,
                    &[octets(&[0x11; 32]), octets(&[0x22; 32])],
                ),
            ]),
            ..Build::default()
        }
        .build();
        assert_eq!(
            ContentInfo::parse(&der),
            Err(CmsError::AttributeValueCount {
                oid: "1.2.840.113549.1.9.4".to_string(),
                count: 2,
            })
        );
    }

    #[test]
    fn an_attribute_whose_value_is_not_its_own_syntax_is_refused_by_name() {
        let der = Build {
            signed_attrs: Some(vec![
                attribute(oid::AA_CONTENT_TYPE, &[oid_of(oid::ID_DATA)]),
                // A `messageDigest` whose value is an INTEGER, not an OCTET
                // STRING.
                attribute(oid::AA_MESSAGE_DIGEST, &[int(7)]),
            ]),
            ..Build::default()
        }
        .build();
        match ContentInfo::parse(&der) {
            Err(CmsError::BadAttribute { oid, error }) => {
                assert_eq!(oid, "1.2.840.113549.1.9.4");
                assert!(
                    matches!(error, DerError::UnexpectedTag { .. }),
                    "the refusal names the tag mismatch: {error:?}"
                );
            }
            other => panic!("expected a named bad attribute, got {other:?}"),
        }
    }

    #[test]
    fn a_signer_identified_by_something_else_entirely_is_refused() {
        let der = Build {
            // `[7]`, which is not one of §5.3's two alternatives.
            sid: context(7, false, &[0x01, 0x02]),
            ..Build::default()
        }
        .build();
        assert_eq!(
            ContentInfo::parse(&der),
            Err(CmsError::UnknownSignerIdentifier {
                class: Class::ContextSpecific,
                tag: 7,
            })
        );
    }

    // ---- the shapes no corpus file has ------------------------------------

    /// `subjectKeyIdentifier`, which nothing in the fetched corpora uses. This
    /// fixture is the only thing holding the arm up, and the module's
    /// [`SignerIdentifier`] doc says so.
    #[test]
    fn a_signer_identified_by_a_subject_key_identifier_is_read() {
        let der = Build {
            sid: context(0, false, &[0xDE, 0xAD, 0xBE, 0xEF]),
            signer_version: 3,
            ..Build::default()
        }
        .build();
        let info = ContentInfo::parse(&der).expect("parses");
        let signer = &info.signed_data().signer_infos()[0];
        assert_eq!(
            signer.sid(),
            &SignerIdentifier::SubjectKeyIdentifier(&[0xDE, 0xAD, 0xBE, 0xEF])
        );
        assert_eq!(signer.version(), 3);
        assert!(
            signer.version_matches_sid(),
            "§5.3 pairs version 3 with a subjectKeyIdentifier"
        );
    }

    #[test]
    fn a_version_disagreeing_with_the_signer_identifier_reads_and_is_reported() {
        let der = Build {
            sid: context(0, false, &[0x01]),
            signer_version: 1,
            ..Build::default()
        }
        .build();
        let info = ContentInfo::parse(&der).expect("readable, and wrong");
        let signer = &info.signed_data().signer_infos()[0];
        assert!(!signer.version_matches_sid());
    }

    /// A CRL and an OCSP-shaped `[1] other`, both located and neither read.
    #[test]
    fn revocation_information_is_surfaced_and_not_evaluated() {
        let der = Build {
            crls: vec![
                seq(&[int(1), common_name("Example CA")]),
                context(1, true, &seq(&[int(2)])),
            ],
            ..Build::default()
        }
        .build();
        let info = ContentInfo::parse(&der).expect("parses");
        let crls = info.signed_data().crls();
        assert_eq!(crls.len(), 2);
        assert!(matches!(crls[0], RevocationChoice::Crl(_)));
        assert!(matches!(crls[1], RevocationChoice::Other { tag: 1, .. }));
    }

    #[test]
    fn a_certificate_set_keeps_other_choices_apart_from_x509_ones() {
        let der = Build {
            certificates: vec![
                certificate_shaped(1),
                // `[2] v2AttrCert`, which is not an X.509 certificate.
                context(2, true, &seq(&[int(9)])),
                certificate_shaped(3),
            ],
            ..Build::default()
        }
        .build();
        let info = ContentInfo::parse(&der).expect("parses");
        let signed = info.signed_data();
        assert_eq!(signed.certificates().len(), 3);
        assert_eq!(
            signed.x509_certificates().count(),
            2,
            "an attribute certificate is not one a chain builder can use"
        );
        assert!(matches!(
            signed.certificates()[1],
            CertificateChoice::Other { tag: 2, .. }
        ));
    }

    /// `signingCertificateV2` (RFC 5035 §3), including the `hashAlgorithm`
    /// DEFAULT that DER omits.
    #[test]
    fn a_signing_certificate_v2_attribute_is_decoded_including_its_default() {
        let with_algorithm = seq(&[algorithm(oid::ID_SHA512), octets(&[0x01; 64])]);
        let defaulted = seq(&[octets(&[0x02; 32])]);
        let value = seq(&[seq(&[with_algorithm, defaulted])]);
        let der = Build {
            signed_attrs: Some(vec![
                attribute(oid::AA_CONTENT_TYPE, &[oid_of(oid::ID_DATA)]),
                attribute(oid::AA_MESSAGE_DIGEST, &[octets(&[0xAB; 32])]),
                attribute(oid::AA_SIGNING_CERTIFICATE_V2, &[value]),
            ]),
            ..Build::default()
        }
        .build();
        let info = ContentInfo::parse(&der).expect("parses");
        let signer = &info.signed_data().signer_infos()[0];
        let attribute = signer
            .signing_certificate_v2()
            .expect("the attribute is there");
        assert_eq!(attribute.certs().len(), 2);
        assert_eq!(attribute.certs()[0].digest(), Ok(DigestAlgorithm::Sha512));
        assert_eq!(attribute.certs()[0].hash(), &[0x01; 64]);
        assert!(attribute.certs()[1].hash_algorithm().is_none());
        assert_eq!(
            attribute.certs()[1].digest(),
            Ok(DigestAlgorithm::Sha256),
            "X.690 §11.5 omits a DEFAULT, and RFC 5035 §4's default is SHA-256"
        );
        assert!(attribute.policies_der().is_none());
    }

    /// A timestamp token is a `ContentInfo` in an unsigned attribute, so this
    /// module reads one with itself — and reads nothing into it.
    #[test]
    fn a_timestamp_token_is_surfaced_and_is_itself_a_content_info() {
        let token = fixture();
        let der = Build {
            unsigned_attrs: Some(vec![attribute(
                oid::AA_TIMESTAMP_TOKEN,
                std::slice::from_ref(&token),
            )]),
            ..Build::default()
        }
        .build();
        let info = ContentInfo::parse(&der).expect("parses");
        let signer = &info.signed_data().signer_infos()[0];
        assert_eq!(signer.timestamp_tokens().len(), 1);
        assert_eq!(signer.timestamp_tokens()[0], token.as_slice());

        let inner = ContentInfo::parse(signer.timestamp_tokens()[0])
            .expect("a token is a ContentInfo of its own");
        assert_eq!(inner.signed_data().signer_infos().len(), 1);

        // And the unsigned attributes are not part of what was signed, which
        // the byte ranges say plainly: the token sits outside the signed set.
        let signed_range = signer.signed_attrs().expect("present").stored_range();
        let unsigned_range = signer.unsigned_attrs().expect("present").stored_range();
        assert!(signed_range.end <= unsigned_range.start);
    }

    /// Several timestamp tokens are legal — two authorities, two tokens.
    #[test]
    fn several_timestamp_tokens_are_all_surfaced() {
        let token = fixture();
        let der = Build {
            unsigned_attrs: Some(vec![
                attribute(oid::AA_TIMESTAMP_TOKEN, &[token.clone(), token.clone()]),
                attribute(oid::AA_TIMESTAMP_TOKEN, std::slice::from_ref(&token)),
            ]),
            ..Build::default()
        }
        .build();
        let info = ContentInfo::parse(&der).expect("parses");
        assert_eq!(
            info.signed_data().signer_infos()[0]
                .timestamp_tokens()
                .len(),
            3
        );
    }

    /// A signer with no `signedAttrs` signs the content directly (§5.4's other
    /// half), so there are no bytes to re-encode.
    #[test]
    fn a_signer_without_signed_attributes_has_nothing_to_re_encode() {
        let der = Build {
            signed_attrs: None,
            e_content: Some(b"signed directly".to_vec()),
            ..Build::default()
        }
        .build();
        let info = ContentInfo::parse(&der).expect("parses");
        let signer = &info.signed_data().signer_infos()[0];
        assert!(signer.signed_attrs().is_none());
        assert_eq!(signer.signed_attrs_to_digest(), None);
        assert_eq!(signer.message_digest(), None);
        assert_eq!(info.signed_data().content_type_matches(signer), None);
    }

    #[test]
    fn a_content_type_attribute_disagreeing_with_the_encapsulated_one_is_reported() {
        let der = Build {
            signed_attrs: Some(vec![
                // `id-signedData` where the encapsulated type is `id-data`.
                attribute(oid::AA_CONTENT_TYPE, &[oid_of(oid::ID_SIGNED_DATA)]),
                attribute(oid::AA_MESSAGE_DIGEST, &[octets(&[0xAB; 32])]),
            ]),
            ..Build::default()
        }
        .build();
        let info = ContentInfo::parse(&der).expect("readable, and disagreeing");
        let signer = &info.signed_data().signer_infos()[0];
        assert_eq!(info.signed_data().content_type_matches(signer), Some(false));
    }

    /// Unknown attributes arrive whole rather than being refused, which is
    /// what lets a corpus blob carrying Adobe's `revocationInfoArchival` and
    /// `smimeCapabilities` read at all.
    #[test]
    fn an_attribute_this_crate_has_never_heard_of_is_kept_rather_than_refused() {
        // `adbe-revocationInfoArchival` (1.2.840.113583.1.1.8), which eleven
        // of the corpus's sixteen readable signers carry and nothing here
        // decodes — the commonest attribute in the corpus after the two §5.3
        // requires.
        let adobe = Oid::from_content(&[0x2A, 0x86, 0x48, 0x86, 0xF7, 0x2F, 0x01, 0x01, 0x08]);
        let der = Build {
            signed_attrs: Some(vec![
                attribute(oid::AA_CONTENT_TYPE, &[oid_of(oid::ID_DATA)]),
                attribute(oid::AA_MESSAGE_DIGEST, &[octets(&[0xAB; 32])]),
                attribute(adobe, &[seq(&[octets(&[0x01, 0x02])])]),
            ]),
            ..Build::default()
        }
        .build();
        let info = ContentInfo::parse(&der).expect("parses");
        let attributes = info.signed_data().signer_infos()[0]
            .signed_attrs()
            .expect("present");
        assert_eq!(attributes.all().len(), 3);
        let kept = attributes
            .one(adobe)
            .expect("one of them")
            .expect("present");
        assert_eq!(kept.values().len(), 1);
        assert_eq!(kept.oid().to_dotted(), "1.2.840.113583.1.1.8");
        // And it is still just bytes: nothing here claims to know what it says.
        assert_eq!(kept.single_value().expect("one value").raw().len(), 6);
    }

    #[test]
    fn a_signed_data_with_no_signers_reads_rather_than_being_refused() {
        // A certificates-only message, which §5.1 admits and which a PDF
        // verdict has to report on rather than fail to parse.
        let body = seq(&[
            int(1),
            set(&[]),
            seq(&[oid_of(oid::ID_DATA)]),
            context(0, true, &certificate_shaped(1)),
            set(&[]),
        ]);
        let der = seq(&[oid_of(oid::ID_SIGNED_DATA), context(0, true, &body)]);
        let info = ContentInfo::parse(&der).expect("legal, and proving nothing");
        assert!(info.signed_data().signer_infos().is_empty());
        assert_eq!(info.signed_data().certificates().len(), 1);
    }

    // ---- limits -----------------------------------------------------------

    #[test]
    fn the_depth_cap_is_reachable_and_refuses_rather_than_recursing() {
        let der = fixture();
        assert_eq!(
            ContentInfo::parse_with(&der, Limits::new(2, 65_536)),
            Err(CmsError::Der(DerError::DepthExceeded))
        );
    }

    #[test]
    fn the_node_budget_refuses_a_structure_it_cannot_afford() {
        let der = fixture();
        assert_eq!(
            ContentInfo::parse_with(&der, Limits::new(64, 4)),
            Err(CmsError::Der(DerError::NodeBudgetExceeded))
        );
    }

    /// The seeds `fuzz/corpus/pki_cms/` carries, written from the fixtures
    /// above so the two cannot drift apart.
    ///
    /// Run with `--ignored` when a fixture changes; the corpus is committed,
    /// and a run that rewrites it is a diff to look at rather than to apply
    /// blindly. The same arrangement `pki_der`, `crypt` and `cff` use, for the
    /// reason those files record: a hand-laid corpus that no longer reaches
    /// what it was chosen for looks exactly like one that does.
    ///
    /// Ten seeds, each for a region a mutation is unlikely to reach on its
    /// own.
    #[test]
    #[ignore = "writes into fuzz/corpus/, which is committed"]
    fn write_the_fuzz_seeds() {
        let base =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fuzz/corpus/pki_cms");
        std::fs::create_dir_all(&base).expect("the corpus directory is creatable");

        let token = fixture();
        let seeds: [(&str, Vec<u8>); 10] = [
            // The everyday shape: detached, signed attributes, one certificate.
            ("detached-signed-attrs", fixture()),
            // The other half of §5.4: no signed attributes at all.
            (
                "no-signed-attrs",
                Build {
                    signed_attrs: None,
                    e_content: Some(b"signed directly".to_vec()),
                    ..Build::default()
                }
                .build(),
            ),
            // `adbe.pkcs7.sha1`'s shape, where the content is carried.
            (
                "encapsulated-content",
                Build {
                    e_content: Some(b"the bytes a signature is over".to_vec()),
                    ..Build::default()
                }
                .build(),
            ),
            // The signer identifier no corpus file uses.
            (
                "subject-key-identifier-sid",
                Build {
                    sid: context(0, false, &[0xDE, 0xAD, 0xBE, 0xEF]),
                    signer_version: 3,
                    ..Build::default()
                }
                .build(),
            ),
            // Certificate and revocation choices other than the plain ones.
            (
                "other-choices",
                Build {
                    certificates: vec![certificate_shaped(1), context(2, true, &seq(&[int(9)]))],
                    crls: vec![
                        seq(&[int(1), common_name("Example CA")]),
                        context(1, true, &seq(&[int(2)])),
                    ],
                    ..Build::default()
                }
                .build(),
            ),
            // A nested `ContentInfo`, which is the deepest thing a real blob
            // contains and the reason `Limits::CMS` is not the certificate cap.
            (
                "timestamp-token",
                Build {
                    unsigned_attrs: Some(vec![attribute(oid::AA_TIMESTAMP_TOKEN, &[token])]),
                    ..Build::default()
                }
                .build(),
            ),
            // RFC 5035's attribute, with and without its DEFAULT.
            (
                "signing-certificate-v2",
                Build {
                    signed_attrs: Some(vec![
                        attribute(oid::AA_CONTENT_TYPE, &[oid_of(oid::ID_DATA)]),
                        attribute(oid::AA_MESSAGE_DIGEST, &[octets(&[0xAB; 32])]),
                        attribute(
                            oid::AA_SIGNING_CERTIFICATE_V2,
                            &[seq(&[seq(&[
                                seq(&[algorithm(oid::ID_SHA512), octets(&[0x01; 64])]),
                                seq(&[octets(&[0x02; 32])]),
                            ])])],
                        ),
                    ]),
                    ..Build::default()
                }
                .build(),
            ),
            // BER, which four of the eighteen real blobs in the fetched
            // corpora are: the five structural nodes written with indefinite
            // lengths, everything below them definite. A mutation will not
            // find a balanced set of terminators on its own.
            (
                "ber-indefinite-length",
                Build {
                    ber_scaffolding: true,
                    ..Build::default()
                }
                .build(),
            ),
            // The one BER shape this module refuses: RFC 5652 §5.4 digests the
            // DER of `signedAttrs`, so an indefinite length inside it — here,
            // two levels down in an attribute nothing decodes — is
            // `IndefiniteSignedAttributes`. The seed keeps that refusal on the
            // path an ordinary input takes.
            (
                "ber-indefinite-signed-attrs",
                Build {
                    ber_scaffolding: true,
                    ber_inside_an_attribute_value: true,
                    ..Build::default()
                }
                .build(),
            ),
            // A `SignedData` with no signers: legal, and proving nothing.
            (
                "certificates-only",
                seq(&[
                    oid_of(oid::ID_SIGNED_DATA),
                    context(
                        0,
                        true,
                        &seq(&[
                            int(1),
                            set(&[]),
                            seq(&[oid_of(oid::ID_DATA)]),
                            context(0, true, &certificate_shaped(1)),
                            set(&[]),
                        ]),
                    ),
                ]),
            ),
        ];

        for (name, bytes) in seeds {
            std::fs::write(base.join(name), bytes).expect("the corpus directory is there");
        }
    }
}
