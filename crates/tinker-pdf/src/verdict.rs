//! What a signature turns out to prove (12.8), assembled from the parts.
//!
//! Reading a signature is four independent questions, and a verdict that
//! collapses them into one answer throws away the only information a caller
//! can act on:
//!
//! 1. **Which bytes does it cover?** [`crate::Coverage`], from the
//!    `/ByteRange` checked against the file. Answered without any
//!    cryptography, and the answer a whole class of attacks lives in.
//! 2. **Do those bytes still hash to what the signature says?** The CMS's
//!    `messageDigest` signed attribute against a digest recomputed here.
//! 3. **Was the signature made by the key in the certificate?** RSASSA-PKCS1
//!    over the re-encoded signed attributes (RFC 5652 §5.4).
//! 4. **Whose key is it?** How far the certificate chain reaches toward an
//!    anchor the *caller* supplied.
//!
//! Question 3 can hold while question 2 fails — that is a signature correctly
//! made over a document that has since changed. Question 2 can hold while
//! question 4 says nothing — that is an intact document signed by a stranger.
//! Neither is "invalid" and neither is "valid", and there is no `bool` here.
//!
//! # What the engine will not decide
//!
//! **Trust.** The chain is walked, each link's signature verified, and the
//! result says how far it got. Which anchors to trust is the caller's, and the
//! anchors arrive as DER bytes from the caller — the same inversion as
//! `FontProvider`. There is no bundled root store and there will not be one.
//!
//! **Time.** Ruling 4 bans a clock from this engine, and here that is not only
//! about determinism: "expired" is a claim about now, and a library that
//! invents a now gives a different answer on a different day for the same
//! bytes. A caller that wants validity judged passes the instant to judge it
//! at; one that does not gets the window reported and decides for itself.
//!
//! **Revocation.** No CRL is fetched and no OCSP responder is asked, because
//! the engine performs no I/O. Embedded revocation data is surfaced by
//! `tinker-pdf-pki` and evaluating its freshness is the host's.
//!
//! # The limit ruling 13 imposes, stated once
//!
//! Nothing outside this repository has ever agreed that a signature this code
//! accepts is acceptable, or that one it rejects is not. The primitives are
//! gated on NIST's published vectors and RFC 5652 §5.4 is adjudicated by
//! fifteen real signatures from six producers — but the assembly below is this
//! engine agreeing with itself.

use tinker_pdf_crypto::{DigestAlgorithm as CryptoDigest, RsaPublicKey};
use tinker_pdf_pki::{
    Certificate, ContentInfo, DigestAlgorithm as CmsDigest, PublicKey, SignatureAlgorithm,
    SignerInfo,
};

use crate::signature::{Coverage, Signature, SubFilter};
use crate::Document;

/// Whether the CMS blob could be read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CmsState {
    /// Parsed, with `signers` signer informations in it.
    Read {
        /// How many `SignerInfo`s the `SignedData` carries. More than one is
        /// legal and rare; the verdict below describes the first.
        signers: usize,
    },
    /// `/Contents` held no bytes to parse — either the dictionary has none, or
    /// the `/ByteRange` gap this build trusts is not where they are.
    Absent,
    /// Bytes were there and would not parse, with the parser's own reason.
    Unreadable(String),
}

/// Whether the document still hashes to what the signature was made over.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DocumentDigest {
    /// The `messageDigest` attribute equals a digest recomputed over the
    /// covered bytes. The document has not changed inside the signed range.
    Matches,
    /// It does not. Either the bytes changed or the signature was never over
    /// them.
    Differs,
    /// Not checked, and why.
    NotChecked(Unchecked),
}

/// Whether the signature verifies against the signer's own public key.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SignatureCheck {
    /// It verifies.
    Verified,
    /// The arithmetic ran and the signature is not the one that key would
    /// have made.
    Failed,
    /// Not checked, and why.
    NotChecked(Unchecked),
}

/// Why a check did not produce an answer.
///
/// Always a named reason rather than a `false`, because "we did not look" and
/// "we looked and it was wrong" are the two answers a caller must never
/// confuse — and collapsing them is the shape of every convincing forgery.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Unchecked {
    /// There was no CMS to check against.
    NoCms,
    /// The `/ByteRange` does not describe a range of this file, so there are
    /// no covered bytes to digest.
    CoverageUnusable,
    /// The `SignedData` carries no `SignerInfo`.
    NoSigner,
    /// The signer names an algorithm this build does not implement.
    UnsupportedAlgorithm(String),
    /// The signer's certificate is not in the blob, so there is no key.
    SignerCertificateMissing,
    /// The certificate's public key is not one this build verifies with —
    /// which today means it is not RSA.
    UnsupportedKey(String),
    /// There are no signed attributes, so there is no `messageDigest` to
    /// compare and the signature is over the content directly. Reported
    /// rather than approximated: one corpus signature is this shape, and
    /// guessing at what it covers would be a verdict about the wrong bytes.
    NoSignedAttributes,
    /// The blob is an `adbe.pkcs7.sha1` (12.8.3.3.1), whose encapsulated
    /// content is the document digest rather than a detached signature over
    /// it. Deprecated in ISO 32000-2, one corpus file, and that file is a
    /// fuzzer's output — so it is named rather than implemented on a sample
    /// of one.
    LegacySha1SubFilter,
}

/// How far the certificate chain reached.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Chain {
    /// A path was built to an anchor the caller supplied, and every link's
    /// signature verified. `anchor` is the anchor's subject, rendered.
    AnchoredTo {
        /// The trusted certificate's subject.
        anchor: String,
        /// How many certificates were between the signer and the anchor.
        links: usize,
    },
    /// The path ends at a self-signed certificate that is not an anchor.
    /// Cryptographically a chain; as evidence, nothing.
    SelfSigned {
        /// That certificate's subject.
        subject: String,
    },
    /// No issuer for some certificate was among the blob's own certificates
    /// or the anchors, so the path stops.
    Incomplete {
        /// The issuer that could not be found.
        missing_issuer: String,
    },
    /// A link's signature did not verify, which means the path is not a path.
    Broken {
        /// The certificate whose signature over its child failed.
        at: String,
    },
    /// No anchors were supplied, so no path was attempted. Distinct from
    /// `Incomplete`: the caller declined to say what it trusts.
    NoAnchors,
    /// The signer's certificate was not found, so there was nothing to start
    /// from.
    NoSignerCertificate,
}

/// Something accepted that a caller should be told about.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Weakness {
    /// The document digest is SHA-1, which is not collision-resistant.
    /// Twelve of the corpus's seventeen signatures are, so it is read — and
    /// said.
    Sha1Digest,
    /// The signature algorithm is SHA-1 with RSA.
    Sha1Signature,
    /// The signer's RSA modulus is under 2 048 bits.
    ShortRsaKey {
        /// How many bits it is.
        bits: usize,
    },
    /// The signature covers an earlier revision, so later ones are outside it.
    CoversOnlyARevision,
    /// The `/ByteRange` did not hold up (see [`Coverage::Suspicious`]).
    CoverageSuspicious,
    /// A certificate's validity window does not contain the instant the
    /// caller asked about.
    OutsideValidity {
        /// The certificate's subject.
        subject: String,
    },
}

/// Who the signer's certificate says they are.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SignerDescription {
    /// The certificate's subject, rendered per RFC 4514.
    pub subject: String,
    /// Its issuer.
    pub issuer: String,
    /// `notBefore` and `notAfter`, as seconds since the Unix epoch.
    pub validity: (i64, i64),
    /// The signing time the signer *claims*, from the `signingTime` signed
    /// attribute. Nothing countersigned it; it is a number the signer wrote.
    pub claimed_signing_time: Option<i64>,
    /// Whether the blob carries an RFC 3161 timestamp token. Surfaced, never
    /// evaluated — validating a token means validating the authority's own
    /// chain, which is a later tier.
    pub timestamped: bool,
}

/// Everything the four questions came back with.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Verdict {
    /// Which bytes it covers, from the `/ByteRange` checked against the file.
    pub coverage: Coverage,
    /// Whether the CMS could be read.
    pub cms: CmsState,
    /// Whether the covered bytes still hash to what was signed.
    pub document_digest: DocumentDigest,
    /// Whether the signature verifies against the signer's key.
    pub signature: SignatureCheck,
    /// How far the chain reached.
    pub chain: Chain,
    /// What was accepted and is worth saying.
    pub weaknesses: Vec<Weakness>,
    /// Who the signer's certificate says they are.
    pub signer: Option<SignerDescription>,
}

impl Verdict {
    /// Whether all four questions came back the way a caller hoping for
    /// "this document is intact and signed by someone I trust" needs.
    ///
    /// A convenience, not the answer: it is deliberately conservative and it
    /// discards every distinction the fields make. Anything reporting to a
    /// person should read the fields.
    #[must_use]
    pub fn is_trusted(&self) -> bool {
        self.coverage == Coverage::WholeFile
            && self.document_digest == DocumentDigest::Matches
            && self.signature == SignatureCheck::Verified
            && matches!(self.chain, Chain::AnchoredTo { .. })
    }
}

/// Certificates the caller trusts, as DER.
///
/// Empty by default and never populated by the engine. A caller with no
/// anchors gets [`Chain::NoAnchors`], which is honest: without something
/// trusted to reach, a chain proves that a key signed something, not whose
/// key it was.
#[derive(Clone, Debug, Default)]
pub struct TrustAnchors {
    certificates: Vec<Vec<u8>>,
}

impl TrustAnchors {
    /// No anchors.
    #[must_use]
    pub fn new() -> TrustAnchors {
        TrustAnchors::default()
    }

    /// Adds a DER certificate. Bytes that do not parse are refused here
    /// rather than silently ignored at verification time.
    ///
    /// # Errors
    /// The parser's own reason, rendered.
    pub fn add(&mut self, der: impl Into<Vec<u8>>) -> Result<(), String> {
        let der = der.into();
        Certificate::parse(&der).map_err(|error| format!("{error:?}"))?;
        self.certificates.push(der);
        Ok(())
    }

    /// How many anchors there are.
    #[must_use]
    pub fn len(&self) -> usize {
        self.certificates.len()
    }

    /// Whether there are none.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.certificates.is_empty()
    }
}

/// The verdict for one signature.
pub(crate) fn verdict(
    document: &Document,
    signature: &Signature,
    anchors: &TrustAnchors,
    at: Option<i64>,
) -> Verdict {
    let mut weaknesses = Vec::new();
    match &signature.coverage {
        Coverage::WholeFile => {}
        Coverage::Revision { .. } => weaknesses.push(Weakness::CoversOnlyARevision),
        Coverage::Suspicious(_) => weaknesses.push(Weakness::CoverageSuspicious),
    }

    let mut verdict = Verdict {
        coverage: signature.coverage.clone(),
        cms: CmsState::Absent,
        document_digest: DocumentDigest::NotChecked(Unchecked::NoCms),
        signature: SignatureCheck::NotChecked(Unchecked::NoCms),
        chain: Chain::NoSignerCertificate,
        weaknesses,
        signer: None,
    };
    let blob = signature.cms();
    if blob.is_empty() {
        return verdict;
    }

    let content = match ContentInfo::parse(blob) {
        Ok(content) => content,
        Err(error) => {
            verdict.cms = CmsState::Unreadable(format!("{error:?}"));
            let why = Unchecked::UnsupportedAlgorithm(format!("{error:?}"));
            verdict.document_digest = DocumentDigest::NotChecked(why.clone());
            verdict.signature = SignatureCheck::NotChecked(why);
            return verdict;
        }
    };
    let signed = content.signed_data();
    verdict.cms = CmsState::Read {
        signers: signed.signer_infos().len(),
    };

    let Some(signer) = signed.signer_infos().first() else {
        verdict.document_digest = DocumentDigest::NotChecked(Unchecked::NoSigner);
        verdict.signature = SignatureCheck::NotChecked(Unchecked::NoSigner);
        return verdict;
    };

    // The certificates, parsed once and shared by the key lookup and the walk.
    let certificates: Vec<Certificate<'_>> = signed
        .x509_certificates()
        .filter_map(|der| Certificate::parse(der).ok())
        .collect();

    verdict.document_digest =
        check_document_digest(document, signature, signer, &mut verdict.weaknesses);

    let signer_certificate = find_signer(signer, &certificates);
    if let Some(certificate) = signer_certificate {
        verdict.signer = Some(describe(certificate, signer, signed.signer_infos()));
        if let Some(instant) = at {
            if !certificate.validity().contains(instant) {
                verdict.weaknesses.push(Weakness::OutsideValidity {
                    subject: certificate.subject().to_rfc4514(),
                });
            }
        }
    }

    verdict.signature = match signer_certificate {
        Some(certificate) => check_signature(signer, certificate, &mut verdict.weaknesses),
        None => SignatureCheck::NotChecked(Unchecked::SignerCertificateMissing),
    };
    verdict.chain = match signer_certificate {
        Some(certificate) => walk(
            certificate,
            &certificates,
            anchors,
            at,
            &mut verdict.weaknesses,
        ),
        None => Chain::NoSignerCertificate,
    };
    verdict
}

/// Question 2: do the covered bytes still hash to what was signed?
fn check_document_digest(
    document: &Document,
    signature: &Signature,
    signer: &SignerInfo<'_>,
    weaknesses: &mut Vec<Weakness>,
) -> DocumentDigest {
    if signature.sub_filter == Some(SubFilter::Pkcs7Sha1) {
        return DocumentDigest::NotChecked(Unchecked::LegacySha1SubFilter);
    }
    let Some(expected) = signer.message_digest() else {
        return DocumentDigest::NotChecked(Unchecked::NoSignedAttributes);
    };
    let algorithm = match signer.effective_digest() {
        Ok(algorithm) => algorithm,
        Err(error) => {
            return DocumentDigest::NotChecked(Unchecked::UnsupportedAlgorithm(format!(
                "{error:?}"
            )))
        }
    };
    if algorithm == CmsDigest::Sha1 {
        weaknesses.push(Weakness::Sha1Digest);
    }
    let Some(actual) = signature.digest(document, cos_digest(algorithm)) else {
        return DocumentDigest::NotChecked(Unchecked::CoverageUnusable);
    };
    // Not a constant-time comparison, and deliberately: both values are public
    // — one is in the document and the other is computed from it — so there is
    // no secret for a timing difference to leak.
    if actual == expected {
        DocumentDigest::Matches
    } else {
        DocumentDigest::Differs
    }
}

/// Question 3: was the signature made by the key in the certificate?
fn check_signature(
    signer: &SignerInfo<'_>,
    certificate: &Certificate<'_>,
    weaknesses: &mut Vec<Weakness>,
) -> SignatureCheck {
    // RFC 5652 §5.4: with signed attributes present the signature is over the
    // DER of a `SET OF Attribute`, which is the stored `[0] IMPLICIT` bytes
    // with the tag replaced. `signed_attrs_to_digest` is the one place that
    // substitution happens, and fifteen real signatures say it is required.
    let Some(message) = signer.signed_attrs_to_digest() else {
        return SignatureCheck::NotChecked(Unchecked::NoSignedAttributes);
    };
    let algorithm = match signer.signature_algorithm() {
        Ok(algorithm) => algorithm,
        Err(error) => {
            return SignatureCheck::NotChecked(Unchecked::UnsupportedAlgorithm(format!(
                "{error:?}"
            )))
        }
    };
    let digest = match signer.effective_digest() {
        Ok(digest) => digest,
        Err(error) => {
            return SignatureCheck::NotChecked(Unchecked::UnsupportedAlgorithm(format!(
                "{error:?}"
            )))
        }
    };
    // ECDSA is implemented in `tinker-pdf-crypto` and gated on CAVP, but no
    // corpus signature uses it and wiring it here would be an untested path in
    // the one place an untested path is worst. It is named, not guessed.
    if !matches!(algorithm, SignatureAlgorithm::RsaPkcs1v15 { .. }) {
        return SignatureCheck::NotChecked(Unchecked::UnsupportedAlgorithm(format!(
            "{algorithm:?}"
        )));
    }
    if digest == CmsDigest::Sha1 {
        weaknesses.push(Weakness::Sha1Signature);
    }

    let Some(key) = rsa_key(certificate) else {
        return SignatureCheck::NotChecked(Unchecked::UnsupportedKey(
            "the subject public key is not RSA".to_string(),
        ));
    };
    if key.modulus_bits() < 2048 {
        weaknesses.push(Weakness::ShortRsaKey {
            bits: key.modulus_bits(),
        });
    }
    match key.verify_pkcs1_v15_message(crypto_digest(digest), &message, signer.signature()) {
        Ok(()) => SignatureCheck::Verified,
        Err(_) => SignatureCheck::Failed,
    }
}

/// Question 4: how far up does the chain go?
///
/// Each link is verified: the issuer's key must actually have signed the
/// child's `TBSCertificate`. A path assembled by name alone is a list of
/// certificates, not a chain, and the difference is the whole point.
fn walk(
    signer: &Certificate<'_>,
    blob: &[Certificate<'_>],
    anchors: &TrustAnchors,
    at: Option<i64>,
    weaknesses: &mut Vec<Weakness>,
) -> Chain {
    if anchors.is_empty() {
        return Chain::NoAnchors;
    }
    let parsed_anchors: Vec<Certificate<'_>> = anchors
        .certificates
        .iter()
        .filter_map(|der| Certificate::parse(der).ok())
        .collect();

    let mut current = signer.clone();
    // Bounded because a certificate set is attacker-supplied and two
    // certificates can name each other as issuer. Eight is past any real
    // hierarchy; the web's longest are four.
    const MAX_LINKS: usize = 8;
    for links in 0..MAX_LINKS {
        if let Some(anchor) = parsed_anchors
            .iter()
            .find(|anchor| anchor.subject().matches(current.issuer()))
        {
            if !verifies(&current, anchor) {
                return Chain::Broken {
                    at: anchor.subject().to_rfc4514(),
                };
            }
            if let Some(instant) = at {
                if !anchor.validity().contains(instant) {
                    weaknesses.push(Weakness::OutsideValidity {
                        subject: anchor.subject().to_rfc4514(),
                    });
                }
            }
            return Chain::AnchoredTo {
                anchor: anchor.subject().to_rfc4514(),
                links,
            };
        }
        // An anchor may be the signer's certificate itself.
        if parsed_anchors
            .iter()
            .any(|anchor| anchor.der() == current.der())
        {
            return Chain::AnchoredTo {
                anchor: current.subject().to_rfc4514(),
                links,
            };
        }
        if current.is_self_issued() {
            return Chain::SelfSigned {
                subject: current.subject().to_rfc4514(),
            };
        }
        let Some(issuer) = blob
            .iter()
            .find(|candidate| candidate.subject().matches(current.issuer()))
        else {
            return Chain::Incomplete {
                missing_issuer: current.issuer().to_rfc4514(),
            };
        };
        if !verifies(&current, issuer) {
            return Chain::Broken {
                at: issuer.subject().to_rfc4514(),
            };
        }
        if let Some(instant) = at {
            if !issuer.validity().contains(instant) {
                weaknesses.push(Weakness::OutsideValidity {
                    subject: issuer.subject().to_rfc4514(),
                });
            }
        }
        current = issuer.clone();
    }
    Chain::Incomplete {
        missing_issuer: format!("more than {MAX_LINKS} links from the signer"),
    }
}

/// Whether `issuer`'s key signed `child`'s `TBSCertificate`.
///
/// Over `child.tbs()`, which is the *stored* encoding: re-encoding it would
/// produce different bytes for any certificate whose DER is not exactly what
/// this engine would emit, and the signature is over what the issuer saw.
fn verifies(child: &Certificate<'_>, issuer: &Certificate<'_>) -> bool {
    let Some(key) = rsa_key(issuer) else {
        return false;
    };
    // A certificate's `signatureAlgorithm` is a *signature* OID —
    // `sha256WithRSAEncryption` — not a digest one, and resolving it through
    // the digest table returns nothing for every certificate ever issued.
    // Doing exactly that made every chain in the corpus read `Broken`, which
    // is the failure mode a chain walk should have: it does not crash and it
    // does not accept, it reports a path that is not a path.
    let algorithm = tinker_pdf_pki::cms::signature_algorithm(child.signature_algorithm().oid());
    let digest = match algorithm {
        Some(SignatureAlgorithm::RsaPkcs1v15 { digest }) => digest,
        // ECDSA and PSS reach here from a real certificate and are simply not
        // verified by this build; the walk reports the path as broken rather
        // than pretending to have checked it.
        _ => None,
    };
    let Some(digest) = digest else {
        return false;
    };
    let Ok(signature) = child.signature().whole_bytes() else {
        return false;
    };
    key.verify_pkcs1_v15_message(crypto_digest(digest), child.tbs(), signature)
        .is_ok()
}

fn rsa_key(certificate: &Certificate<'_>) -> Option<RsaPublicKey> {
    match certificate.subject_public_key_info().public_key() {
        PublicKey::Rsa { modulus, exponent } => RsaPublicKey::new(modulus, exponent).ok(),
        _ => None,
    }
}

/// The crypto crate's digest selector for the CMS crate's.
///
/// Two enums for one concept, and they are deliberately not unified: the CMS
/// one is what an OID resolved to and the crypto one is what the arithmetic
/// takes. A `From` impl would put the mapping out of sight; here it is one
/// `match` that the compiler makes exhaustive.
fn crypto_digest(digest: CmsDigest) -> CryptoDigest {
    match digest {
        CmsDigest::Sha1 => CryptoDigest::Sha1,
        CmsDigest::Sha256 => CryptoDigest::Sha256,
        CmsDigest::Sha384 => CryptoDigest::Sha384,
        CmsDigest::Sha512 => CryptoDigest::Sha512,
    }
}

/// And the same for the object model's, which is what `Signature::digest`
/// takes because the writer and the reader of a `/ByteRange` share it.
///
/// Three enums naming four hashes is a smell, and it is the right smell: each
/// belongs to a crate that must not depend on the others, and collapsing them
/// would be an edge in the dependency graph bought to save two `match`es.
fn cos_digest(digest: CmsDigest) -> tinker_pdf_cos::DigestAlgorithm {
    match digest {
        CmsDigest::Sha1 => tinker_pdf_cos::DigestAlgorithm::Sha1,
        CmsDigest::Sha256 => tinker_pdf_cos::DigestAlgorithm::Sha256,
        CmsDigest::Sha384 => tinker_pdf_cos::DigestAlgorithm::Sha384,
        CmsDigest::Sha512 => tinker_pdf_cos::DigestAlgorithm::Sha512,
    }
}

/// The signer's own certificate, found by whichever identifier it used.
fn find_signer<'a, 'b>(
    signer: &SignerInfo<'_>,
    certificates: &'b [Certificate<'a>],
) -> Option<&'b Certificate<'a>> {
    use tinker_pdf_pki::SignerIdentifier;
    match signer.sid() {
        SignerIdentifier::IssuerAndSerialNumber { issuer, serial, .. } => {
            certificates.iter().find(|certificate| {
                certificate.serial().as_bytes() == serial.as_bytes()
                    && certificate.issuer().matches(issuer)
            })
        }
        SignerIdentifier::SubjectKeyIdentifier(wanted) => {
            certificates.iter().find(|certificate| {
                certificate.extensions().subject_key_identifier() == Some(*wanted)
                    // RFC 5280 §4.2.1.2 method (1) for a certificate that
                    // carries no extension, which is what a version 1
                    // certificate is.
                    || certificate.key_identifier_sha1().as_slice() == *wanted
            })
        }
    }
}

fn describe(
    certificate: &Certificate<'_>,
    signer: &SignerInfo<'_>,
    _all: &[SignerInfo<'_>],
) -> SignerDescription {
    let validity = certificate.validity();
    SignerDescription {
        subject: certificate.subject().to_rfc4514(),
        issuer: certificate.issuer().to_rfc4514(),
        validity: (validity.not_before, validity.not_after),
        claimed_signing_time: signer.signing_time(),
        timestamped: !signer.timestamp_tokens().is_empty(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_anchor_that_is_not_a_certificate_is_refused_when_it_is_added() {
        let mut anchors = TrustAnchors::new();
        assert!(anchors.add(vec![0x30, 0x00]).is_err(), "an empty SEQUENCE");
        assert!(anchors.add(Vec::new()).is_err(), "nothing at all");
        assert!(anchors.is_empty(), "and neither was kept");
    }

    #[test]
    fn the_convenience_needs_all_four_questions_to_agree() {
        let good = Verdict {
            coverage: Coverage::WholeFile,
            cms: CmsState::Read { signers: 1 },
            document_digest: DocumentDigest::Matches,
            signature: SignatureCheck::Verified,
            chain: Chain::AnchoredTo {
                anchor: "CN=A".into(),
                links: 1,
            },
            weaknesses: Vec::new(),
            signer: None,
        };
        assert!(good.is_trusted());

        // Each of the four, spoiled on its own.
        let mut coverage = good.clone();
        coverage.coverage = Coverage::Revision { index: 1 };
        assert!(!coverage.is_trusted(), "a revision is not the document");

        let mut digest = good.clone();
        digest.document_digest = DocumentDigest::Differs;
        assert!(!digest.is_trusted());

        let mut signed = good.clone();
        signed.signature = SignatureCheck::Failed;
        assert!(!signed.is_trusted());

        let mut chain = good.clone();
        chain.chain = Chain::SelfSigned {
            subject: "CN=A".into(),
        };
        assert!(!chain.is_trusted(), "self-signed is not anchored");
    }

    /// The distinction the whole module exists to keep: "we did not look" is
    /// not "we looked and it was wrong".
    #[test]
    fn a_check_that_did_not_run_is_not_a_failure() {
        let unchecked = DocumentDigest::NotChecked(Unchecked::NoCms);
        assert_ne!(unchecked, DocumentDigest::Differs);
        assert_ne!(unchecked, DocumentDigest::Matches);

        let unchecked = SignatureCheck::NotChecked(Unchecked::SignerCertificateMissing);
        assert_ne!(unchecked, SignatureCheck::Failed);
        assert_ne!(unchecked, SignatureCheck::Verified);
    }
}
