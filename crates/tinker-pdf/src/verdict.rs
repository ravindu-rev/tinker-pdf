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
//! 3. **Was the signature made by the key in the certificate?** RSASSA-PKCS1,
//!    RSASSA-PSS or ECDSA over the re-encoded signed attributes (RFC 5652
//!    §5.4).
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
//! at; one that does not gets the window reported and decides for itself. An
//! RFC 3161 token's authority is judged at the token's own `genTime`, which
//! is the token's claim about when it stamped rather than a reading of any
//! clock.
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
//!
//! The ECDSA and RSASSA-PSS arms are where that sentence needs a second
//! clause, because no corpus signature uses either and none of the four
//! questions could be asked of a real one. Its evidence is split three ways and
//! `crates/tinker-pdf/tests/ecdsa_verdict.rs` states the split in full: the
//! curve arithmetic is NIST CAVP's, the CMS and the certificates are OpenSSL's,
//! and the `/ByteRange` spans are this repository's own on both sides.
//! `crates/tinker-pdf/tests/signature_shapes.rs` makes the same split for PSS,
//! whose arithmetic is CAVP's and RSA Laboratories'.

use tinker_pdf_crypto::{Curve, DigestAlgorithm as CryptoDigest, EcPublicKey, RsaPublicKey};
use tinker_pdf_pki::{
    oid, pss, Certificate, ContentInfo, DigestAlgorithm as CmsDigest, PublicKey,
    SignatureAlgorithm, SignerInfo, TimeStampToken,
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
    /// not RSA, not an elliptic-curve point on P-256 or P-384, or a point
    /// this build declines to read. The string says which.
    UnsupportedKey(String),
    /// The `signatureValue` did not hold the structure its algorithm's
    /// signature is encoded in — today, an ECDSA signature that is not RFC
    /// 3279 §2.2.3's `SEQUENCE { r INTEGER, s INTEGER }`.
    ///
    /// Distinct from [`SignatureCheck::Failed`] on purpose, and the
    /// distinction is the module's own: `Failed` means the arithmetic ran and
    /// disagreed, and nothing ran here. Both are safe answers; only one of
    /// them is true.
    MalformedSignatureValue(String),
    /// There are no signed attributes, so there is no `messageDigest` to
    /// compare: the signature is over the content's digest directly (RFC 5652
    /// §5.4), and questions 2 and 3 are one question.
    ///
    /// Only ever the document digest's reason, and only when the signature
    /// did not verify. A detached signature with no signed attributes that
    /// verifies is a signature over these covered bytes, and the digest then
    /// reads [`DocumentDigest::Matches`]; one that does not verify cannot say
    /// whether the bytes changed or the signature was never theirs, so the
    /// digest is left unanswered rather than given the signature's answer.
    NoSignedAttributes,
    /// The signature is `adbe.pkcs7.sha1` (12.8.3.3.1), whose `SignedData`
    /// must encapsulate the SHA-1 digest of the covered bytes — and this one
    /// is detached, so there is no document digest in it to compare.
    ContentNotEncapsulated,
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
    /// The signature algorithm digests with SHA-1 — `sha1WithRSAEncryption`
    /// or `ecdsa-with-SHA1`. The curve or the modulus may be fine; what the
    /// signer committed to is a 160-bit digest either way.
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
    /// Whether the blob carries an RFC 3161 timestamp token.
    /// [`Verdict::timestamps`] says what each one proves.
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
    /// What each RFC 3161 timestamp token reached proves, in the order the
    /// signer's unsigned attributes carry them.
    pub timestamps: Vec<TimestampVerdict>,
}

impl Verdict {
    /// Whether all four questions came back the way a caller hoping for
    /// "this document is intact and signed by someone I trust" needs.
    ///
    /// A convenience, not the answer: it is deliberately conservative and it
    /// discards every distinction the fields make. Anything reporting to a
    /// person should read the fields.
    #[must_use]
    ///
    /// For a document timestamp the four answers describe the token, and its
    /// [`TimestampVerdict`] must be trusted too — the authority's certificate
    /// fit for timestamping is part of what a document timestamp is, where
    /// for a signature's own countersignature it is a separate matter.
    pub fn is_trusted(&self) -> bool {
        self.coverage == Coverage::WholeFile
            && self.document_digest == DocumentDigest::Matches
            && self.signature == SignatureCheck::Verified
            && matches!(self.chain, Chain::AnchoredTo { .. })
            && self
                .timestamps
                .iter()
                .filter(|stamp| stamp.stamps == Stamped::Document)
                .all(TimestampVerdict::is_trusted)
    }
}

/// What an RFC 3161 timestamp token stamps: what its `messageImprint` must
/// be the digest of.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Stamped {
    /// A signature's `signature` octets, the token being an unsigned
    /// attribute of that signer (RFC 3161 Appendix A): it says the signature
    /// existed by `time`.
    Signature,
    /// The bytes a document timestamp's `/ByteRange` covers (ISO 32000-2
    /// 12.8.5, `/SubFilter /ETSI.RFC3161`): it says the document, and every
    /// signature already in it, existed by `time`.
    Document,
}

/// RFC 3161 §2.3 and §2.4.1's two requirements on the certificate a token
/// was signed with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum AuthorityCertificate {
    /// Its only extended key usage is `id-kp-timeStamping`, marked critical,
    /// and the token's ESS `signingCertificate` or `signingCertificateV2`
    /// attribute names it by digest.
    Fit,
    /// The certificate's extended key usage is absent, not critical, or names
    /// a purpose besides `id-kp-timeStamping` — not a certificate §2.3 lets
    /// an authority stamp with.
    NotForTimestamping,
    /// No ESS signing-certificate attribute names the certificate the token
    /// was signed with: the token does not bind itself to the key that signed
    /// it, which §2.4.1 requires so a certificate cannot be substituted.
    NotBound,
    /// The token's certificate set does not carry the signer's certificate.
    Missing,
}

/// What one RFC 3161 timestamp token turned out to prove.
///
/// The same refusal to collapse as [`Verdict`]: the imprint, the signature,
/// the certificate's fitness and the chain are asked separately, because a
/// token correctly signed over a different digest and a token over the right
/// digest signed by a stranger are different findings.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct TimestampVerdict {
    /// What the token stamps.
    pub stamps: Stamped,
    /// Whether the token read as a `TimeStampToken`.
    pub token: CmsState,
    /// `genTime`, Unix seconds: the instant the authority asserts.
    pub time: Option<i64>,
    /// The authority as the token names it: the `TSTInfo`'s `tsa` hint where
    /// there is one, otherwise the signing certificate's subject.
    pub authority: Option<String>,
    /// Whether `messageImprint` is the digest of what the token stamps.
    pub imprint: DocumentDigest,
    /// Whether the authority's key signed this `TSTInfo`: the signature
    /// verifies, and the `messageDigest` it covers is the `TSTInfo`'s own.
    pub signature: SignatureCheck,
    /// RFC 3161's requirements on the authority's certificate.
    pub authority_certificate: AuthorityCertificate,
    /// How far the authority's chain reached, its validity judged at `time`.
    pub chain: Chain,
    /// What was accepted and is worth saying.
    pub weaknesses: Vec<Weakness>,
}

impl TimestampVerdict {
    /// Whether every answer came back the way "a trusted authority vouches
    /// for this time" needs. The same convenience, with the same caveat, as
    /// [`Verdict::is_trusted`].
    #[must_use]
    pub fn is_trusted(&self) -> bool {
        matches!(self.token, CmsState::Read { .. })
            && self.imprint == DocumentDigest::Matches
            && self.signature == SignatureCheck::Verified
            && self.authority_certificate == AuthorityCertificate::Fit
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
        timestamps: Vec::new(),
    };
    let blob = signature.cms();
    if blob.is_empty() {
        return verdict;
    }
    if signature.sub_filter == Some(SubFilter::EtsiRfc3161) {
        return document_timestamp(document, signature, blob, anchors, verdict);
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
        check_document_digest(document, signature, signed, signer, &mut verdict.weaknesses);

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

    let message = Signed::of(signer, signed, document, signature);
    verdict.signature = match signer_certificate {
        Some(certificate) => {
            check_signature(signer, certificate, &message, &mut verdict.weaknesses)
        }
        None => SignatureCheck::NotChecked(Unchecked::SignerCertificateMissing),
    };
    // RFC 5652 §5.4's other case: with no signed attributes on a detached
    // signature, the signature *is* over the document's digest, so questions
    // 2 and 3 are one question. A signature that verifies answers both; one
    // that does not cannot say which half failed, and the digest stays
    // unchecked rather than borrowing the signature's answer.
    if matches!(message, Signed::Covered { .. }) && verdict.signature == SignatureCheck::Verified {
        verdict.document_digest = DocumentDigest::Matches;
    }
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
    // RFC 3161 Appendix A: a token in the signer's unsigned attributes stamps
    // the `signature` octets, so its imprint is their digest.
    verdict.timestamps = signer
        .timestamp_tokens()
        .iter()
        .map(|token| {
            timestamp(
                token,
                Stamped::Signature,
                &|algorithm| Some(algorithm.digest(signer.signature()).as_bytes().to_vec()),
                anchors,
            )
            .0
        })
        .collect();
    verdict
}

/// A document timestamp (ISO 32000-2 12.8.5): `/Contents` is the token
/// itself, its imprint the digest of the covered bytes.
///
/// The four answers then describe the token — question 2 is the imprint
/// against the covered bytes, question 3 the authority's signature over its
/// `TSTInfo`, question 4 the authority's chain at `genTime` — and the token's
/// own [`TimestampVerdict`] rides in `timestamps` beside them, which is where
/// its time and its certificate's fitness are.
fn document_timestamp(
    document: &Document,
    signature: &Signature,
    blob: &[u8],
    anchors: &TrustAnchors,
    mut verdict: Verdict,
) -> Verdict {
    let (stamp, authority) = timestamp(
        blob,
        Stamped::Document,
        &|algorithm| signature.digest(document, cos_digest(cms_digest(algorithm))),
        anchors,
    );
    verdict.cms = stamp.token.clone();
    verdict.document_digest = stamp.imprint.clone();
    verdict.signature = stamp.signature.clone();
    verdict.chain = stamp.chain.clone();
    verdict.weaknesses.extend(stamp.weaknesses.iter().cloned());
    verdict.signer = authority;
    verdict.timestamps = vec![stamp];
    verdict
}

/// The verdict for one RFC 3161 token, whose imprint must be `imprinted`
/// under the token's own hash.
///
/// The authority's chain is judged at the token's own `genTime` rather than
/// at the caller's instant: §2.3 asks whether the certificate was valid when
/// it stamped, and the token says when that was. That is the token's claim,
/// not a clock, so ruling 4 is not touched by it.
fn timestamp(
    der: &[u8],
    stamps: Stamped,
    imprinted: &dyn Fn(CryptoDigest) -> Option<Vec<u8>>,
    anchors: &TrustAnchors,
) -> (TimestampVerdict, Option<SignerDescription>) {
    let mut verdict = TimestampVerdict {
        stamps,
        token: CmsState::Absent,
        time: None,
        authority: None,
        imprint: DocumentDigest::NotChecked(Unchecked::NoCms),
        signature: SignatureCheck::NotChecked(Unchecked::NoCms),
        authority_certificate: AuthorityCertificate::Missing,
        chain: Chain::NoSignerCertificate,
        weaknesses: Vec::new(),
    };
    let token = match TimeStampToken::parse(der) {
        Ok(token) => token,
        Err(error) => {
            verdict.token = CmsState::Unreadable(format!("{error}"));
            let why = Unchecked::UnsupportedAlgorithm(format!("{error}"));
            verdict.imprint = DocumentDigest::NotChecked(why.clone());
            verdict.signature = SignatureCheck::NotChecked(why);
            return (verdict, None);
        }
    };
    let info = token.info();
    let signed = token.content_info().signed_data();
    verdict.token = CmsState::Read {
        signers: signed.signer_infos().len(),
    };
    verdict.time = Some(info.time());
    verdict.imprint = match info.imprint_digest() {
        Ok(algorithm) => {
            if algorithm == CmsDigest::Sha1 {
                verdict.weaknesses.push(Weakness::Sha1Digest);
            }
            match imprinted(crypto_digest(algorithm)) {
                Some(expected) if expected == info.imprint() => DocumentDigest::Matches,
                Some(_) => DocumentDigest::Differs,
                None => DocumentDigest::NotChecked(Unchecked::CoverageUnusable),
            }
        }
        Err(error) => {
            DocumentDigest::NotChecked(Unchecked::UnsupportedAlgorithm(format!("{error:?}")))
        }
    };

    let Some(signer) = signed.signer_infos().first() else {
        verdict.signature = SignatureCheck::NotChecked(Unchecked::NoSigner);
        return (verdict, None);
    };
    let certificates: Vec<Certificate<'_>> = signed
        .x509_certificates()
        .filter_map(|der| Certificate::parse(der).ok())
        .collect();
    let certificate = find_signer(signer, &certificates);
    verdict.authority = info
        .tsa()
        .map(ToString::to_string)
        .or_else(|| certificate.map(|certificate| certificate.subject().to_rfc4514()));
    let Some(certificate) = certificate else {
        verdict.signature = SignatureCheck::NotChecked(Unchecked::SignerCertificateMissing);
        return (verdict, None);
    };
    let description = describe(certificate, signer, signed.signer_infos());

    // The token's `messageDigest` must be the digest of this `TSTInfo`, or
    // the signature is over some other one; with no signed attributes the
    // signature is over the `TSTInfo` octets themselves.
    let message = match signer.signed_attrs_to_digest() {
        Some(attributes) => Signed::Attributes(attributes),
        None => Signed::Content(info.der()),
    };
    let content_matches = match (signer.message_digest(), signer.effective_digest()) {
        (None, _) => true,
        (Some(expected), Ok(algorithm)) => {
            crypto_digest(algorithm).digest(info.der()).as_bytes() == expected
        }
        (Some(_), Err(_)) => false,
    };
    verdict.signature =
        match check_signature(signer, certificate, &message, &mut verdict.weaknesses) {
            SignatureCheck::Verified if !content_matches => SignatureCheck::Failed,
            other => other,
        };
    verdict.authority_certificate = authority_certificate(signer, certificate);
    verdict.chain = walk(
        certificate,
        &certificates,
        anchors,
        Some(info.time()),
        &mut verdict.weaknesses,
    );
    if !certificate.validity().contains(info.time()) {
        verdict.weaknesses.push(Weakness::OutsideValidity {
            subject: certificate.subject().to_rfc4514(),
        });
    }
    (verdict, Some(description))
}

/// RFC 3161 §2.3 and §2.4.1, asked of the certificate a token was signed
/// with.
fn authority_certificate(
    signer: &SignerInfo<'_>,
    certificate: &Certificate<'_>,
) -> AuthorityCertificate {
    // §2.3: "The corresponding certificate MUST contain only one instance of
    // the extended key usage field extension ... with KeyPurposeID having
    // value id-kp-timeStamping. This extension MUST be critical."
    let extensions = certificate.extensions();
    let critical = extensions
        .find(oid::CE_EXT_KEY_USAGE)
        .is_some_and(|extension| extension.is_critical());
    let only_timestamping = extensions
        .extended_key_usage()
        .is_some_and(|usage| usage.purposes().len() == 1 && usage.has(oid::KP_TIME_STAMPING));
    if !critical || !only_timestamping {
        return AuthorityCertificate::NotForTimestamping;
    }
    // §2.4.1 (and RFC 5816 for the second version): the signed attributes
    // name the signing certificate by digest, and the first `ESSCertID` is
    // the one that signed. Where an `issuerSerial` is given it must name the
    // same certificate too.
    let ess = signer
        .signing_certificate_v2()
        .or_else(|| signer.signing_certificate());
    let Some(first) = ess.and_then(|ess| ess.certs().first()) else {
        return AuthorityCertificate::NotBound;
    };
    let Ok(algorithm) = first.digest() else {
        return AuthorityCertificate::NotBound;
    };
    if crypto_digest(algorithm)
        .digest(certificate.der())
        .as_bytes()
        != first.hash()
    {
        return AuthorityCertificate::NotBound;
    }
    match first.issuer_serial_decoded() {
        None => AuthorityCertificate::Fit,
        Some(Ok(issuer_serial)) if issuer_serial.identifies(certificate) => {
            AuthorityCertificate::Fit
        }
        Some(_) => AuthorityCertificate::NotBound,
    }
}

/// What a signer's signature value was computed over (RFC 5652 §5.4).
///
/// Three shapes, and the subfilter does not decide between them — the
/// `SignerInfo` does. A verifier that assumed signed attributes, which every
/// corpus signer but one carries, refused the other two by name; one that
/// guessed would have verified a signature over the wrong bytes.
enum Signed<'a> {
    /// The signed attributes, as §5.4 re-encodes them for digesting: the
    /// stored `[0] IMPLICIT` tag replaced by `SET OF`.
    Attributes(Vec<u8>),
    /// No signed attributes, and the message carries its content: the
    /// signature is over the digest of the `eContent` octets.
    Content(&'a [u8]),
    /// No signed attributes, and the message is detached: the signature is
    /// over the digest of the content itself, which for a PDF is the bytes
    /// the `/ByteRange` covers.
    Covered {
        document: &'a Document,
        signature: &'a Signature,
    },
}

impl<'a> Signed<'a> {
    fn of(
        signer: &SignerInfo<'a>,
        signed: &tinker_pdf_pki::SignedData<'a>,
        document: &'a Document,
        signature: &'a Signature,
    ) -> Signed<'a> {
        if let Some(attributes) = signer.signed_attrs_to_digest() {
            return Signed::Attributes(attributes);
        }
        match signed.encap_content_info().content() {
            Some(content) => Signed::Content(content),
            None => Signed::Covered {
                document,
                signature,
            },
        }
    }

    /// The digest the signature value is over, under `algorithm`.
    ///
    /// `None` only for [`Signed::Covered`] whose spans do not fit the file —
    /// a digest over less than the signature covers is not one.
    fn digest(&self, algorithm: CryptoDigest) -> Option<Vec<u8>> {
        match self {
            Signed::Attributes(bytes) => Some(algorithm.digest(bytes).as_bytes().to_vec()),
            Signed::Content(bytes) => Some(algorithm.digest(bytes).as_bytes().to_vec()),
            Signed::Covered {
                document,
                signature,
            } => signature.digest(document, cos_digest(cms_digest(algorithm))),
        }
    }
}

/// Question 2: do the covered bytes still hash to what was signed?
fn check_document_digest(
    document: &Document,
    signature: &Signature,
    signed: &tinker_pdf_pki::SignedData<'_>,
    signer: &SignerInfo<'_>,
    weaknesses: &mut Vec<Weakness>,
) -> DocumentDigest {
    if signature.sub_filter == Some(SubFilter::Pkcs7Sha1) {
        return check_encapsulated_sha1(document, signature, signed, signer, weaknesses);
    }
    let Some(expected) = signer.message_digest() else {
        // No `messageDigest` to compare: the signature is over the content
        // directly and question 3 answers this one too (see `verdict`). The
        // digest it is over is still a document digest, and still worth
        // calling weak.
        if signer.signed_attrs().is_none() && signer.effective_digest() == Ok(CmsDigest::Sha1) {
            weaknesses.push(Weakness::Sha1Digest);
        }
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

/// Question 2 for `adbe.pkcs7.sha1` (ISO 32000-1 12.8.3.3.1), where the
/// document's digest is *inside* the message: the `eContent` is the SHA-1
/// digest of the covered bytes, and the signer signs that content like any
/// other.
///
/// So there are two links where a detached signature has one, and both must
/// hold. The twenty octets the message carries must be the covered bytes'
/// SHA-1; and where the signer has signed attributes, its `messageDigest` must
/// be the digest of those twenty octets under the signer's own algorithm —
/// otherwise the message carries a document digest that nothing signed. With
/// no signed attributes the signature is over the twenty octets directly, and
/// question 3 is what checks that link.
fn check_encapsulated_sha1(
    document: &Document,
    signature: &Signature,
    signed: &tinker_pdf_pki::SignedData<'_>,
    signer: &SignerInfo<'_>,
    weaknesses: &mut Vec<Weakness>,
) -> DocumentDigest {
    // The subfilter fixes the document digest at SHA-1, whatever the signer
    // used for the rest — which is why ISO 32000-2 deprecates it.
    weaknesses.push(Weakness::Sha1Digest);
    let Some(content) = signed.encap_content_info().content() else {
        return DocumentDigest::NotChecked(Unchecked::ContentNotEncapsulated);
    };
    let Some(actual) = signature.digest(document, tinker_pdf_cos::DigestAlgorithm::Sha1) else {
        return DocumentDigest::NotChecked(Unchecked::CoverageUnusable);
    };
    if content != actual.as_slice() {
        return DocumentDigest::Differs;
    }
    let Some(expected) = signer.message_digest() else {
        return DocumentDigest::Matches;
    };
    let algorithm = match signer.effective_digest() {
        Ok(algorithm) => algorithm,
        Err(error) => {
            return DocumentDigest::NotChecked(Unchecked::UnsupportedAlgorithm(format!(
                "{error:?}"
            )))
        }
    };
    if crypto_digest(algorithm).digest(content).as_bytes() == expected {
        DocumentDigest::Matches
    } else {
        DocumentDigest::Differs
    }
}

/// Question 3: was the signature made by the key in the certificate?
fn check_signature(
    signer: &SignerInfo<'_>,
    certificate: &Certificate<'_>,
    message: &Signed<'_>,
    weaknesses: &mut Vec<Weakness>,
) -> SignatureCheck {
    // RFC 5652 §5.4: with signed attributes present the signature is over the
    // DER of a `SET OF Attribute`, which is the stored `[0] IMPLICIT` bytes
    // with the tag replaced. `signed_attrs_to_digest` is the one place that
    // substitution happens, and fifteen real signatures say it is required.
    // Without them it is over the content's own digest; `Signed` says which.
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
    // One `match` per algorithm this build has arithmetic for, and a named
    // refusal for the rest. **No arm may fall through to a positive answer**:
    // an unwired algorithm that returns `NotChecked` is a gap, and one that
    // returns `Verified` is a forgery accepted, so every arm below ends in a
    // call into `tinker-pdf-crypto` or in a refusal, and there is no `_ =>`.
    match algorithm {
        SignatureAlgorithm::RsaPkcs1v15 { .. } => {
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
            let Some(digest_value) = message.digest(crypto_digest(digest)) else {
                return SignatureCheck::NotChecked(Unchecked::CoverageUnusable);
            };
            match key.verify_pkcs1_v15(crypto_digest(digest), &digest_value, signer.signature()) {
                Ok(()) => SignatureCheck::Verified,
                Err(_) => SignatureCheck::Failed,
            }
        }
        // RFC 5758 §3.2's OIDs name the digest, so `effective_digest` above
        // already resolved it; the curve comes from the certificate and never
        // from the signature, which is what stops a signer choosing the group
        // its own signature is checked in.
        SignatureAlgorithm::Ecdsa { .. } => {
            if digest == CmsDigest::Sha1 {
                weaknesses.push(Weakness::Sha1Signature);
            }
            let key = match ec_key(certificate) {
                Ok(key) => key,
                Err(why) => return SignatureCheck::NotChecked(Unchecked::UnsupportedKey(why)),
            };
            let (r, s) = match tinker_pdf_pki::cms::ecdsa_signature_value(signer.signature()) {
                Ok(pair) => pair,
                Err(error) => {
                    return SignatureCheck::NotChecked(Unchecked::MalformedSignatureValue(format!(
                        "{error}"
                    )))
                }
            };
            let Some(digest_value) = message.digest(crypto_digest(digest)) else {
                return SignatureCheck::NotChecked(Unchecked::CoverageUnusable);
            };
            match key.verify(&digest_value, r, s) {
                Ok(()) => SignatureCheck::Verified,
                Err(_) => SignatureCheck::Failed,
            }
        }
        // RFC 4056 §3: the parameters are the signer's and come with the
        // signature; the hash in them is what digests the signed attributes.
        // That hash and the `digestAlgorithm` that reduced the document are
        // only a SHOULD apart, so a signer may use two — the document digest
        // above used the one `effective_digest` names and the signature below
        // uses the parameters', each where RFC 4056 puts it.
        SignatureAlgorithm::RsaPss => {
            let parameters = match pss::parameters(&signer.signature_algorithm_id()) {
                Ok(parameters) => parameters,
                Err(error) => {
                    return SignatureCheck::NotChecked(Unchecked::UnsupportedAlgorithm(format!(
                        "{error}"
                    )))
                }
            };
            if parameters.hash == CryptoDigest::Sha1 {
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
            if !key_permits_pss(certificate, parameters) {
                // RFC 4056 §3: "If any of the above four steps is not true,
                // the signature checking algorithm MUST fail validation." A
                // key that restricted itself to other parameters did not make
                // this signature, whatever the arithmetic would say.
                return SignatureCheck::Failed;
            }
            let Some(digest_value) = message.digest(parameters.hash) else {
                return SignatureCheck::NotChecked(Unchecked::CoverageUnusable);
            };
            match key.verify_pss(parameters, &digest_value, signer.signature()) {
                Ok(()) => SignatureCheck::Verified,
                Err(_) => SignatureCheck::Failed,
            }
        }
    }
}

/// RFC 4056 §3's four checks, where the key's own `SubjectPublicKeyInfo` is
/// `id-RSASSA-PSS` with parameters: the same hash, the same mask generation,
/// a salt at least as long as the key's, and the same trailer.
///
/// A key under `rsaEncryption`, or under `id-RSASSA-PSS` with no parameters,
/// restricts nothing (RFC 4055 §3.3, cases 1 and 2). Parameters the key
/// carries and this build cannot read are a refusal, not a pass — a
/// restriction nobody read is not one anybody honoured.
fn key_permits_pss(
    certificate: &Certificate<'_>,
    signature: tinker_pdf_crypto::PssParameters,
) -> bool {
    let algorithm = certificate.subject_public_key_info().algorithm();
    if algorithm.oid() != oid::RSASSA_PSS || algorithm.parameters().is_none() {
        return true;
    }
    let Ok(key) = pss::parameters(&algorithm) else {
        return false;
    };
    // Step 4, the trailer field, is `trailerFieldBC(1)` on both sides by
    // construction: `pss::parameters` refuses any other value.
    key.hash == signature.hash
        && key.mask_hash == signature.mask_hash
        && signature.salt_length >= key.salt_length
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
    // A certificate's `signatureAlgorithm` is a *signature* OID —
    // `sha256WithRSAEncryption` — not a digest one, and resolving it through
    // the digest table returns nothing for every certificate ever issued.
    // Doing exactly that made every chain in the corpus read `Broken`, which
    // is the failure mode a chain walk should have: it does not crash and it
    // does not accept, it reports a path that is not a path.
    let algorithm = tinker_pdf_pki::cms::signature_algorithm(child.signature_algorithm().oid());
    let Ok(signature) = child.signature().whole_bytes() else {
        return false;
    };
    match algorithm {
        Some(SignatureAlgorithm::RsaPkcs1v15 {
            digest: Some(digest),
        }) => {
            let Some(key) = rsa_key(issuer) else {
                return false;
            };
            key.verify_pkcs1_v15_message(crypto_digest(digest), child.tbs(), signature)
                .is_ok()
        }
        Some(SignatureAlgorithm::Ecdsa { digest }) => {
            let Ok(key) = ec_key(issuer) else {
                return false;
            };
            let Ok((r, s)) = tinker_pdf_pki::cms::ecdsa_signature_value(signature) else {
                return false;
            };
            key.verify_message(crypto_digest(digest), child.tbs(), r, s)
                .is_ok()
        }
        // RFC 4055 §3.2: a certificate's PSS signature is the same octet
        // string a CMS one is, carried in a BIT STRING; its parameters are the
        // child's `signatureAlgorithm`, and the issuer's key may restrict them.
        Some(SignatureAlgorithm::RsaPss) => {
            let Ok(parameters) = pss::parameters(&child.signature_algorithm()) else {
                return false;
            };
            let Some(key) = rsa_key(issuer) else {
                return false;
            };
            key_permits_pss(issuer, parameters)
                && key
                    .verify_pss_message(parameters, child.tbs(), signature)
                    .is_ok()
        }
        // Bare `rsaEncryption` names no digest and is not a legal certificate
        // `signatureAlgorithm`; anything else this build cannot name. The walk
        // reports the path as broken rather than pretending to have checked
        // it.
        Some(SignatureAlgorithm::RsaPkcs1v15 { digest: None }) | None => false,
    }
}

fn rsa_key(certificate: &Certificate<'_>) -> Option<RsaPublicKey> {
    match certificate.subject_public_key_info().public_key() {
        PublicKey::Rsa { modulus, exponent } => RsaPublicKey::new(modulus, exponent).ok(),
        _ => None,
    }
}

/// The certificate's elliptic-curve key, or why it is not one to verify with.
///
/// Three refusals, and each is a different attack if it is skipped rather than
/// made:
///
/// * **The curve comes from `parameters` and nowhere else.** RFC 5480 §2.1.1
///   puts a named-curve OID there and this reads it; a build that assumed
///   P-256 because the point happened to be 65 bytes would verify a P-384
///   signature in the wrong group, and a build that let the *signature*
///   algorithm pick the curve would let the signer pick it.
/// * **Only SEC 1 §2.3.3's uncompressed form is read.** A compressed point
///   needs a square root in the field to recover `y`, and guessing the sign
///   would produce a different key half the time. Declining is the honest
///   answer; there is no compressed point in the corpus to decline.
/// * **The point is checked against the curve equation**, inside
///   [`EcPublicKey::new`]. A point off the curve lies in a group where the
///   discrete logarithm may be easy, which is the invalid-curve attack.
fn ec_key(certificate: &Certificate<'_>) -> Result<EcPublicKey, String> {
    let PublicKey::Ec { curve, point } = certificate.subject_public_key_info().public_key() else {
        return Err("the subject public key is not an elliptic-curve point".to_string());
    };
    let Some(named) = curve else {
        return Err(
            "the key's parameters do not name a curve, which RFC 5480 §2.1.1 requires".to_string(),
        );
    };
    let curve = if named == oid::SECP256R1 {
        Curve::P256
    } else if named == oid::SECP384R1 {
        Curve::P384
    } else {
        return Err(format!(
            "the named curve {} is not one this build implements",
            named.to_dotted()
        ));
    };

    let Some((form, coordinates)) = point.split_first() else {
        return Err("the subject public key holds no point".to_string());
    };
    if *form != 0x04 {
        return Err(format!(
            "the point's form octet is 0x{form:02x}; only SEC 1 §2.3.3's uncompressed 0x04 is read"
        ));
    }
    let width = curve.field_bytes();
    if coordinates.len() != width * 2 {
        return Err(format!(
            "the point is {} octets after its form octet, not the {} {curve:?} takes",
            coordinates.len(),
            width * 2
        ));
    }
    let (x, y) = coordinates.split_at(width);
    EcPublicKey::new(curve, x, y).map_err(|refusal| format!("the point was refused: {refusal:?}"))
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

/// The other direction, for the one place a digest the arithmetic chose —
/// RSASSA-PSS's, from its parameters — has to be taken over the covered bytes.
fn cms_digest(digest: CryptoDigest) -> CmsDigest {
    match digest {
        CryptoDigest::Sha1 => CmsDigest::Sha1,
        CryptoDigest::Sha256 => CmsDigest::Sha256,
        CryptoDigest::Sha384 => CmsDigest::Sha384,
        CryptoDigest::Sha512 => CmsDigest::Sha512,
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
            timestamps: Vec::new(),
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

        // A token whose authority certificate is not fit for timestamping:
        // as a signature's countersignature it says nothing about the
        // signature, and as a document timestamp it is the signature.
        let unfit = |stamps| TimestampVerdict {
            stamps,
            token: CmsState::Read { signers: 1 },
            time: Some(0),
            authority: None,
            imprint: DocumentDigest::Matches,
            signature: SignatureCheck::Verified,
            authority_certificate: AuthorityCertificate::NotForTimestamping,
            chain: Chain::AnchoredTo {
                anchor: "CN=TSA".into(),
                links: 0,
            },
            weaknesses: Vec::new(),
        };
        let mut countersigned = good.clone();
        countersigned.timestamps = vec![unfit(Stamped::Signature)];
        assert!(
            countersigned.is_trusted(),
            "a bad countersignature is not a bad signature"
        );
        let mut document = good.clone();
        document.timestamps = vec![unfit(Stamped::Document)];
        assert!(
            !document.is_trusted(),
            "a document timestamp's authority must be fit to stamp"
        );
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
