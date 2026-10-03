//! Digital signatures (12.8), read and verified, as JavaScript classes.
//!
//! Reading only, as on every surface: a `Signer` is a host callback, and the
//! write design keeps callbacks off the bindings. Each enum the facade answers
//! with crosses as its arm's name in kebab case, and an arm's payload — which
//! revision, which defect, whose certificate, how many bits — as a sibling
//! getter. The C ABI cannot carry a payload because a C enum has none; a
//! JavaScript object can, so dropping it here would be this binding deciding
//! to say less than the facade does (ruling 11).
//!
//! There is no `isValid`. `isTrusted()` is the facade's own conservative
//! convenience, and the four questions stay four getters.

use wasm_bindgen::prelude::*;

use tinker_pdf::{
    Anchor, Certification, Chain, CmsState, Coverage, CoverageDefect, DocumentDigest, FieldLock,
    SignatureCheck, SignatureWarning, Unchecked, Weakness,
};

use crate::PdfDocument;

fn coverage_kind(coverage: &Coverage) -> String {
    match coverage {
        Coverage::WholeFile => "whole-file",
        Coverage::Revision { .. } => "revision",
        Coverage::Suspicious(_) => "suspicious",
    }
    .to_string()
}

fn coverage_revision(coverage: &Coverage) -> Option<u32> {
    match coverage {
        Coverage::Revision { index } => Some(u32::try_from(*index).unwrap_or(u32::MAX)),
        _ => None,
    }
}

fn coverage_defect(coverage: &Coverage) -> Option<String> {
    match coverage {
        Coverage::Suspicious(defect) => Some(
            match defect {
                CoverageDefect::Missing => "missing",
                CoverageDefect::NotFourNumbers { .. } => "not-four-numbers",
                CoverageDefect::NotAnOffset => "not-an-offset",
                CoverageDefect::DoesNotStartAtZero { .. } => "does-not-start-at-zero",
                CoverageDefect::SpansOverlap => "spans-overlap",
                CoverageDefect::PastEndOfFile { .. } => "past-end-of-file",
                CoverageDefect::GapIsNotContents => "gap-is-not-contents",
                CoverageDefect::EndsMidFile { .. } => "ends-mid-file",
            }
            .to_string(),
        ),
        _ => None,
    }
}

fn unchecked(reason: &Unchecked) -> String {
    match reason {
        Unchecked::NoCms => "no-cms",
        Unchecked::CoverageUnusable => "coverage-unusable",
        Unchecked::NoSigner => "no-signer",
        Unchecked::UnsupportedAlgorithm(_) => "unsupported-algorithm",
        Unchecked::SignerCertificateMissing => "signer-certificate-missing",
        Unchecked::UnsupportedKey(_) => "unsupported-key",
        Unchecked::MalformedSignatureValue(_) => "malformed-signature-value",
        Unchecked::NoSignedAttributes => "no-signed-attributes",
        Unchecked::ContentNotEncapsulated => "content-not-encapsulated",
        Unchecked::NoMessageDigest => "no-message-digest",
    }
    .to_string()
}

/// A JavaScript number from a file offset: exact below 2^53, which no
/// document held in wasm memory reaches.
fn offset(value: u64) -> f64 {
    value as f64
}

/// One signature dictionary, read and checked against the file — not
/// verified; that is `PdfDocument.verifySignatures`.
#[wasm_bindgen]
pub struct PdfSignature {
    inner: tinker_pdf::Signature,
}

#[wasm_bindgen]
impl PdfSignature {
    /// How the dictionary was reached: `"field"`, `"merged-field"` or
    /// `"permissions"` (a catalog `/Perms` entry, named by `anchorName`).
    #[wasm_bindgen(getter)]
    pub fn anchor(&self) -> String {
        match self.inner.anchor {
            Anchor::Field => "field",
            Anchor::MergedField => "merged-field",
            Anchor::Permissions(_) => "permissions",
        }
        .to_string()
    }

    /// The `/Perms` key for a `"permissions"` anchor.
    #[wasm_bindgen(getter, js_name = anchorName)]
    pub fn anchor_name(&self) -> Option<String> {
        match &self.inner.anchor {
            Anchor::Permissions(name) => Some(name.clone()),
            _ => None,
        }
    }

    /// The fully qualified field name (12.7.3.2).
    #[wasm_bindgen(getter)]
    pub fn field(&self) -> Option<String> {
        self.inner.field.clone()
    }

    /// `[objectNumber, generation]` of the field.
    #[wasm_bindgen(getter, js_name = fieldRef)]
    pub fn field_ref(&self) -> Option<Vec<u32>> {
        self.inner.field_ref.map(|r| vec![r.num, u32::from(r.gen)])
    }

    /// `[objectNumber, generation]` of the signature dictionary.
    #[wasm_bindgen(getter, js_name = valueRef)]
    pub fn value_ref(&self) -> Option<Vec<u32>> {
        self.inner.value_ref.map(|r| vec![r.num, u32::from(r.gen)])
    }

    /// `/Filter`.
    #[wasm_bindgen(getter)]
    pub fn filter(&self) -> Option<String> {
        self.inner.filter.clone()
    }

    /// `/SubFilter` when this build recognises it.
    #[wasm_bindgen(getter, js_name = subFilter)]
    pub fn sub_filter(&self) -> Option<String> {
        self.inner
            .sub_filter
            .map(|sub| tinker_pdf::SubFilter::name(sub).to_string())
    }

    /// `/SubFilter` exactly as written, recognised or not.
    #[wasm_bindgen(getter, js_name = subFilterName)]
    pub fn sub_filter_name(&self) -> Option<String> {
        self.inner.sub_filter_name.clone()
    }

    /// The `/ByteRange` spans, flattened: `[start0, end0, start1, end1]`.
    #[wasm_bindgen(getter)]
    pub fn spans(&self) -> Vec<f64> {
        self.inner
            .spans
            .iter()
            .flat_map(|span| [offset(span.start), offset(span.end)])
            .collect()
    }

    /// `"whole-file"`, `"revision"` or `"suspicious"`.
    #[wasm_bindgen(getter)]
    pub fn coverage(&self) -> String {
        coverage_kind(&self.inner.coverage)
    }

    /// Which revision a `"revision"` coverage ends at, newest first.
    #[wasm_bindgen(getter, js_name = coverageRevision)]
    pub fn coverage_revision(&self) -> Option<u32> {
        coverage_revision(&self.inner.coverage)
    }

    /// Why a `"suspicious"` coverage is.
    #[wasm_bindgen(getter, js_name = coverageDefect)]
    pub fn coverage_defect(&self) -> Option<String> {
        coverage_defect(&self.inner.coverage)
    }

    /// Whether the coverage is the whole file but the `/Contents` gap.
    #[wasm_bindgen(getter, js_name = coversWholeFile)]
    pub fn covers_whole_file(&self) -> bool {
        self.inner.covers_whole_file()
    }

    /// Whether this is a usage-rights signature, which says nothing about
    /// the content.
    #[wasm_bindgen(getter, js_name = isUsageRights)]
    pub fn is_usage_rights(&self) -> bool {
        self.inner.is_usage_rights()
    }

    /// The stored `/Contents` bytes, copied.
    #[wasm_bindgen(getter)]
    pub fn contents(&self) -> Vec<u8> {
        self.inner.contents.clone()
    }

    /// `/M`, the time the signer claims, as `[year, month, day, hour,
    /// minute, second]` with the zone in `signedAtOffset`; unverified.
    #[wasm_bindgen(getter, js_name = signedAt)]
    pub fn signed_at(&self) -> Option<Vec<i32>> {
        self.inner.signed_at.map(|d| {
            vec![
                d.year,
                i32::from(d.month),
                i32::from(d.day),
                i32::from(d.hour),
                i32::from(d.minute),
                i32::from(d.second),
            ]
        })
    }

    /// `/M`'s offset from UT in minutes, when it states one.
    #[wasm_bindgen(getter, js_name = signedAtOffset)]
    pub fn signed_at_offset(&self) -> Option<i32> {
        self.inner.signed_at.and_then(|d| d.utc_offset_minutes)
    }

    /// `/Reason`.
    #[wasm_bindgen(getter)]
    pub fn reason(&self) -> Option<String> {
        self.inner.reason.clone()
    }

    /// `/Location`.
    #[wasm_bindgen(getter)]
    pub fn location(&self) -> Option<String> {
        self.inner.location.clone()
    }

    /// `/Name` — who the signer claims to be.
    #[wasm_bindgen(getter)]
    pub fn name(&self) -> Option<String> {
        self.inner.name.clone()
    }

    /// `/ContactInfo`.
    #[wasm_bindgen(getter)]
    pub fn contact(&self) -> Option<String> {
        self.inner.contact.clone()
    }

    /// The `/DocMDP` level, 1 to 3, for a certifying signature.
    #[wasm_bindgen(getter, js_name = certificationLevel)]
    pub fn certification_level(&self) -> Option<u8> {
        self.inner.certification.map(Certification::level)
    }

    /// `/FieldMDP`'s action — `"all"`, `"include"` or `"exclude"`.
    #[wasm_bindgen(getter, js_name = fieldLock)]
    pub fn field_lock(&self) -> Option<String> {
        self.inner.field_lock.as_ref().map(|lock| {
            match lock {
                FieldLock::All => "all",
                FieldLock::Include(_) => "include",
                FieldLock::Exclude(_) => "exclude",
            }
            .to_string()
        })
    }

    /// The fields an `"include"` or `"exclude"` lock names.
    #[wasm_bindgen(getter, js_name = fieldLockFields)]
    pub fn field_lock_fields(&self) -> Vec<String> {
        match &self.inner.field_lock {
            Some(FieldLock::Include(names) | FieldLock::Exclude(names)) => names.clone(),
            _ => Vec::new(),
        }
    }

    /// What was read leniently, by name.
    #[wasm_bindgen(getter)]
    pub fn warnings(&self) -> Vec<String> {
        self.inner
            .warnings
            .iter()
            .map(|warning| {
                match warning {
                    SignatureWarning::ValueNotADictionary => "value-not-a-dictionary",
                    SignatureWarning::ContentsMissing => "contents-missing",
                    SignatureWarning::ContentsNotHexadecimal => "contents-not-hexadecimal",
                    SignatureWarning::ContentsOddDigitCount => "contents-odd-digit-count",
                    SignatureWarning::ContentsGapExcludesDelimiters => {
                        "contents-gap-excludes-delimiters"
                    }
                    SignatureWarning::SubFilterUnknown(_) => "sub-filter-unknown",
                    SignatureWarning::SubFilterMissing => "sub-filter-missing",
                }
                .to_string()
            })
            .collect()
    }
}

/// Certificates the caller trusts, as DER. Empty is how a caller says it
/// trusts nothing; there is no default root store.
#[wasm_bindgen]
pub struct PdfTrustAnchors {
    inner: tinker_pdf::TrustAnchors,
}

#[wasm_bindgen]
impl PdfTrustAnchors {
    /// No anchors.
    #[wasm_bindgen(constructor)]
    pub fn new() -> PdfTrustAnchors {
        PdfTrustAnchors {
            inner: tinker_pdf::TrustAnchors::new(),
        }
    }

    /// Adds a certificate; throws, and keeps nothing, when the bytes are not
    /// one.
    pub fn add(&mut self, der: &[u8]) -> Result<(), JsError> {
        self.inner.add(der.to_vec()).map_err(|e| JsError::new(&e))
    }

    /// How many anchors.
    #[wasm_bindgen(getter)]
    pub fn length(&self) -> u32 {
        u32::try_from(self.inner.len()).unwrap_or(u32::MAX)
    }
}

/// What one signature turned out to prove: four questions, never one
/// boolean.
#[wasm_bindgen]
pub struct PdfVerdict {
    inner: tinker_pdf::Verdict,
}

#[wasm_bindgen]
impl PdfVerdict {
    /// `"whole-file"`, `"revision"` or `"suspicious"`.
    #[wasm_bindgen(getter)]
    pub fn coverage(&self) -> String {
        coverage_kind(&self.inner.coverage)
    }

    /// Which revision a `"revision"` coverage ends at.
    #[wasm_bindgen(getter, js_name = coverageRevision)]
    pub fn coverage_revision(&self) -> Option<u32> {
        coverage_revision(&self.inner.coverage)
    }

    /// Why a `"suspicious"` coverage is.
    #[wasm_bindgen(getter, js_name = coverageDefect)]
    pub fn coverage_defect(&self) -> Option<String> {
        coverage_defect(&self.inner.coverage)
    }

    /// `"read"`, `"absent"` or `"unreadable"`.
    #[wasm_bindgen(getter)]
    pub fn cms(&self) -> String {
        match self.inner.cms {
            CmsState::Read { .. } => "read",
            CmsState::Absent => "absent",
            CmsState::Unreadable(_) => "unreadable",
        }
        .to_string()
    }

    /// How many `SignerInfo`s a `"read"` CMS carries.
    #[wasm_bindgen(getter, js_name = cmsSigners)]
    pub fn cms_signers(&self) -> Option<u32> {
        match self.inner.cms {
            CmsState::Read { signers } => Some(u32::try_from(signers).unwrap_or(u32::MAX)),
            _ => None,
        }
    }

    /// The parser's reason for `"unreadable"`.
    #[wasm_bindgen(getter, js_name = cmsReason)]
    pub fn cms_reason(&self) -> Option<String> {
        match &self.inner.cms {
            CmsState::Unreadable(reason) => Some(reason.clone()),
            _ => None,
        }
    }

    /// `"matches"`, `"differs"` or `"not-checked"`.
    #[wasm_bindgen(getter, js_name = documentDigest)]
    pub fn document_digest(&self) -> String {
        match self.inner.document_digest {
            DocumentDigest::Matches => "matches",
            DocumentDigest::Differs => "differs",
            DocumentDigest::NotChecked(_) => "not-checked",
        }
        .to_string()
    }

    /// Why the digest was not checked, by name.
    #[wasm_bindgen(getter, js_name = documentDigestReason)]
    pub fn document_digest_reason(&self) -> Option<String> {
        match &self.inner.document_digest {
            DocumentDigest::NotChecked(reason) => Some(unchecked(reason)),
            _ => None,
        }
    }

    /// `"verified"`, `"failed"` or `"not-checked"`.
    #[wasm_bindgen(getter)]
    pub fn signature(&self) -> String {
        match self.inner.signature {
            SignatureCheck::Verified => "verified",
            SignatureCheck::Failed => "failed",
            SignatureCheck::NotChecked(_) => "not-checked",
        }
        .to_string()
    }

    /// Why the signature was not checked, by name.
    #[wasm_bindgen(getter, js_name = signatureReason)]
    pub fn signature_reason(&self) -> Option<String> {
        match &self.inner.signature {
            SignatureCheck::NotChecked(reason) => Some(unchecked(reason)),
            _ => None,
        }
    }

    /// `"anchored-to"`, `"self-signed"`, `"incomplete"`, `"broken"`,
    /// `"no-anchors"` or `"no-signer-certificate"`.
    #[wasm_bindgen(getter)]
    pub fn chain(&self) -> String {
        match self.inner.chain {
            Chain::AnchoredTo { .. } => "anchored-to",
            Chain::SelfSigned { .. } => "self-signed",
            Chain::Incomplete { .. } => "incomplete",
            Chain::Broken { .. } => "broken",
            Chain::NoAnchors => "no-anchors",
            Chain::NoSignerCertificate => "no-signer-certificate",
        }
        .to_string()
    }

    /// The certificate the chain arm names.
    #[wasm_bindgen(getter, js_name = chainSubject)]
    pub fn chain_subject(&self) -> Option<String> {
        match &self.inner.chain {
            Chain::AnchoredTo { anchor, .. } => Some(anchor.clone()),
            Chain::SelfSigned { subject } => Some(subject.clone()),
            Chain::Incomplete { missing_issuer } => Some(missing_issuer.clone()),
            Chain::Broken { at } => Some(at.clone()),
            _ => None,
        }
    }

    /// How many certificates were between the signer and the anchor.
    #[wasm_bindgen(getter, js_name = chainLinks)]
    pub fn chain_links(&self) -> Option<u32> {
        match self.inner.chain {
            Chain::AnchoredTo { links, .. } => Some(u32::try_from(links).unwrap_or(u32::MAX)),
            _ => None,
        }
    }

    /// The weaknesses, by name.
    #[wasm_bindgen(getter)]
    pub fn weaknesses(&self) -> Vec<String> {
        self.inner
            .weaknesses
            .iter()
            .map(|weakness| {
                match weakness {
                    Weakness::Sha1Digest => "sha1-digest",
                    Weakness::Sha1Signature => "sha1-signature",
                    Weakness::ShortRsaKey { .. } => "short-rsa-key",
                    Weakness::CoversOnlyARevision => "covers-only-a-revision",
                    Weakness::CoverageSuspicious => "coverage-suspicious",
                    Weakness::OutsideValidity { .. } => "outside-validity",
                }
                .to_string()
            })
            .collect()
    }

    /// Each weakness's payload, beside `weaknesses`: the bits of a
    /// `"short-rsa-key"`, the subject of an `"outside-validity"`, `""`
    /// otherwise.
    #[wasm_bindgen(getter, js_name = weaknessDetails)]
    pub fn weakness_details(&self) -> Vec<String> {
        self.inner
            .weaknesses
            .iter()
            .map(|weakness| match weakness {
                Weakness::ShortRsaKey { bits } => bits.to_string(),
                Weakness::OutsideValidity { subject } => subject.clone(),
                _ => String::new(),
            })
            .collect()
    }

    /// The signer certificate's subject, rendered per RFC 4514.
    #[wasm_bindgen(getter, js_name = signerSubject)]
    pub fn signer_subject(&self) -> Option<String> {
        self.inner.signer.as_ref().map(|s| s.subject.clone())
    }

    /// Its issuer.
    #[wasm_bindgen(getter, js_name = signerIssuer)]
    pub fn signer_issuer(&self) -> Option<String> {
        self.inner.signer.as_ref().map(|s| s.issuer.clone())
    }

    /// `[notBefore, notAfter]`, seconds since the Unix epoch.
    #[wasm_bindgen(getter, js_name = signerValidity)]
    pub fn signer_validity(&self) -> Option<Vec<f64>> {
        // Exact: certificate times are far inside 2^53 seconds.
        self.inner
            .signer
            .as_ref()
            .map(|s| vec![s.validity.0 as f64, s.validity.1 as f64])
    }

    /// The `signingTime` the signer claims; nothing countersigned it.
    #[wasm_bindgen(getter, js_name = claimedSigningTime)]
    pub fn claimed_signing_time(&self) -> Option<f64> {
        self.inner
            .signer
            .as_ref()
            .and_then(|s| s.claimed_signing_time)
            .map(|t| t as f64)
    }

    /// Whether the blob carries an RFC 3161 timestamp token.
    #[wasm_bindgen(getter)]
    pub fn timestamped(&self) -> bool {
        self.inner.signer.as_ref().is_some_and(|s| s.timestamped)
    }

    /// The facade's conservative convenience; read the four answers when
    /// reporting to a person.
    #[wasm_bindgen(js_name = isTrusted)]
    pub fn is_trusted(&self) -> bool {
        self.inner.is_trusted()
    }
}

#[wasm_bindgen]
impl PdfDocument {
    /// The document's digital signatures (12.8), read — not verified.
    #[wasm_bindgen]
    pub fn signatures(&self) -> Vec<PdfSignature> {
        self.inner
            .signatures()
            .into_iter()
            .map(|inner| PdfSignature { inner })
            .collect()
    }

    /// What every signature turns out to prove, in `signatures()`'s order.
    ///
    /// `at` is the instant to judge certificate validity at, in seconds since
    /// the Unix epoch; `undefined` judges nothing, because "expired" is a
    /// claim about a moment the caller has to name.
    #[wasm_bindgen(js_name = verifySignatures)]
    pub fn verify_signatures(&self, anchors: &PdfTrustAnchors, at: Option<f64>) -> Vec<PdfVerdict> {
        // A JavaScript instant is a number; the facade's is whole seconds.
        // A fraction is the caller's precision, not this binding's to round,
        // so it is truncated exactly as `Math.trunc` would.
        let at = at.map(|seconds| seconds.trunc() as i64);
        self.inner
            .verify_signatures(&anchors.inner, at)
            .into_iter()
            .map(|inner| PdfVerdict { inner })
            .collect()
    }
}
