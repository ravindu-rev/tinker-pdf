//! ASN.1 DER, X.509 and CMS, for the structure side of a PDF signature.
//!
//! A leaf crate: bytes and plain scalars in, values out, no PDF or COS types
//! anywhere in its surface (ruling 8). A certificate is not a PDF concept and
//! nothing here knows what a document is — which is what lets this be fuzzed,
//! read and published on its own.
//!
//! Feature documentation: `docs/design/signatures.md`.
//!
//! # Why this is not part of `tinker-pdf-crypto`
//!
//! The two crates fail in different ways, and the review each needs follows
//! from that. `tinker-pdf-crypto` is **arithmetic**: its failure mode is a
//! wrong number, and published vectors — FIPS 197, RFC 6229, FIPS 180-4 —
//! catch a wrong number completely. This crate is **untrusted-input structure
//! walking**: its failure mode is a panic or a read past the end of a buffer
//! on bytes an attacker chose, which no vector catches and which ruling 1's
//! per-format fuzzers exist for. Merging them would put the larger attack
//! surface inside the crate whose review story is "small, vector-gated
//! arithmetic", and would leave DER reachable by a fuzzer only through
//! crypto's API. Apart, this one gets `fuzz/fuzz_targets/pki_der.rs` pointed
//! straight at raw DER.
//!
//! # What it refuses, and where the line is
//!
//! **DER by default.** X.509 is DER (RFC 5280 §4.1) and DER admits one
//! encoding per value, so non-minimal lengths, non-minimal tags, non-minimal
//! INTEGERs, non-canonical BOOLEANs and BER's segmented string forms are all
//! refused by name. A second reading of a signed structure is a signature
//! bypass, not a leniency, and [`der`]'s header says which rules are enforced
//! and which two deliberately are not.
//!
//! **One BER form is read, by one caller, on measured evidence.** RFC 5652
//! §5.1 permits BER for a `SignedData`, ISO 32000-1 12.8.3.3.1 calls a PDF
//! signature's `/Contents` DER, and four of the eighteen CMS blobs in the
//! fetched corpora side with the RFC — indefinite lengths on their outermost
//! structural nodes, from two independent producer lineages. So
//! [`der::Limits::allow_indefinite_lengths`] exists, [`der::Limits::CMS`] is
//! the only constant that sets it, and [`cms::ContentInfo::parse`] is the only
//! caller that gets it. [`x509::Certificate::parse`] does not, however it was
//! reached.
//!
//! The narrowing that matters is in [`cms`]: what RFC 5652 §5.4 digests is
//! held to DER by [`der::Tlv::require_definite_lengths`], so a BER
//! `signedAttrs` is [`cms::CmsError::IndefiniteSignedAttributes`] rather than
//! a signature that fails to verify for a reason nothing names.
//! `crates/tinker-pdf/tests/cms_census.rs` keeps every one of those numbers
//! honest.
//!
//! **Depth-capped and budgeted.** [`der::Limits`] bounds nesting and total
//! nodes; the walker itself does not recurse, so the cap is about bounding
//! work rather than about the stack, and the reasoning for each number is on
//! [`der::Limits::CERTIFICATE`].
//!
//! **Every refusal is named.** There is no variant meaning "malformed": the
//! caller of a certificate parser is usually reporting to a user why a
//! signature could not be checked, and "malformed" is not a report.
//!
//! # What this crate does *not* claim
//!
//! It parses; it does not adjudicate. Nothing here checks a signature — the
//! arithmetic for that arrives in `tinker-pdf-crypto` at milestones 4 and 5 of
//! the design — nothing here holds a trust anchor, and nothing here consults a
//! clock. [`x509::Certificate::validity`] hands back the certificate's own
//! claim about its window; whether *now* is inside it is a question this crate
//! cannot ask, because the instant is the host's to supply and ruling 4 keeps
//! the platform clock out of the tree.
//!
//! The name matching in [`name`] is likewise a reduction of RFC 5280 §7.1
//! rather than the whole of it, and that module's header lists what it does
//! not do before it lists what it does.

#![forbid(unsafe_code)]

pub mod cms;
pub mod der;
pub mod name;
pub mod oid;
pub mod x509;

pub use cms::{
    Attributes, CertificateChoice, CmsError, ContentInfo, DigestAlgorithm, EncapsulatedContent,
    EssCertId, RevocationChoice, SignatureAlgorithm, SignedData, SignerIdentifier, SignerInfo,
    SigningCertificateV2,
};
pub use der::{BitString, Budget, Class, Cursor, DerError, Int, Limits, Oid, Tag, TimeFault, Tlv};
pub use name::{Attribute, AttributeText, Name, Rdn};

/// RFC 5652 §5.3's `Attribute`, re-exported under a distinguishing name.
///
/// The flat name is taken by [`name::Attribute`], which is RFC 5280 §4.1.2.4's
/// `AttributeTypeAndValue` — a different structure in a different namespace
/// that happens to be called the same thing by a different specification.
/// Renaming either at the module level would be renaming a specification's own
/// term, so the collision is resolved here, where it exists, and
/// [`cms::Attribute`] keeps the name RFC 5652 gives it.
pub use cms::Attribute as CmsAttribute;
pub use x509::{
    AlgorithmIdentifier, AuthorityKeyIdentifier, BasicConstraints, Certificate, ExtendedKeyUsage,
    Extension, ExtensionFault, Extensions, KeyFault, KeyUsage, PublicKey, SubjectPublicKeyInfo,
    Validity, Version, X509Error,
};
