//! Digital signatures (12.8), read and verified, as Python objects.
//!
//! Reading only, as on every surface: a `Signer` is a host callback, and the
//! write design keeps callbacks off the bindings. Each enum the facade answers
//! with crosses as its arm's name in kebab case, and an arm's payload — which
//! revision, which defect, whose certificate, how many bits — as a sibling
//! attribute. The C ABI cannot carry a payload because a C enum has none; a
//! Python object can, so dropping it here would be this binding deciding to
//! say less than the facade does (ruling 11).
//!
//! There is no `is_valid`. `is_trusted()` is the facade's own conservative
//! convenience, and the four questions stay four attributes.

use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::PyBytes;

use tinker_pdf::{
    Anchor, Certification, Chain, CmsState, Coverage, CoverageDefect, Document, DocumentDigest,
    FieldLock, SignatureCheck, SignatureWarning, Unchecked, Weakness,
};

fn coverage_kind(coverage: &Coverage) -> &'static str {
    match coverage {
        Coverage::WholeFile => "whole-file",
        Coverage::Revision { .. } => "revision",
        Coverage::Suspicious(_) => "suspicious",
    }
}

fn coverage_revision(coverage: &Coverage) -> Option<usize> {
    match coverage {
        Coverage::Revision { index } => Some(*index),
        _ => None,
    }
}

fn coverage_defect(coverage: &Coverage) -> Option<&'static str> {
    match coverage {
        Coverage::Suspicious(defect) => Some(match defect {
            CoverageDefect::Missing => "missing",
            CoverageDefect::NotFourNumbers { .. } => "not-four-numbers",
            CoverageDefect::NotAnOffset => "not-an-offset",
            CoverageDefect::DoesNotStartAtZero { .. } => "does-not-start-at-zero",
            CoverageDefect::SpansOverlap => "spans-overlap",
            CoverageDefect::PastEndOfFile { .. } => "past-end-of-file",
            CoverageDefect::GapIsNotContents => "gap-is-not-contents",
            CoverageDefect::EndsMidFile { .. } => "ends-mid-file",
        }),
        _ => None,
    }
}

fn unchecked(reason: &Unchecked) -> &'static str {
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
}

/// One signature dictionary, read and checked against the file — not
/// verified; that is `Document.verify_signatures`.
#[pyclass(name = "Signature", frozen)]
pub struct PySignature {
    inner: tinker_pdf::Signature,
}

#[pymethods]
impl PySignature {
    /// How the dictionary was reached: "field", "merged-field" or
    /// "permissions" (a catalog `/Perms` entry, named by `anchor_name`).
    #[getter]
    fn anchor(&self) -> &'static str {
        match self.inner.anchor {
            Anchor::Field => "field",
            Anchor::MergedField => "merged-field",
            Anchor::Permissions(_) => "permissions",
        }
    }

    /// The `/Perms` key, `DocMDP`, `UR3` or `UR`, for a "permissions" anchor.
    #[getter]
    fn anchor_name(&self) -> Option<&str> {
        match &self.inner.anchor {
            Anchor::Permissions(name) => Some(name),
            _ => None,
        }
    }

    /// The fully qualified field name (12.7.3.2), when reached through one.
    #[getter]
    fn field(&self) -> Option<&str> {
        self.inner.field.as_deref()
    }

    /// `(object number, generation)` of the field.
    #[getter]
    fn field_ref(&self) -> Option<(u32, u16)> {
        self.inner.field_ref.map(|r| (r.num, r.gen))
    }

    /// `(object number, generation)` of the signature dictionary.
    #[getter]
    fn value_ref(&self) -> Option<(u32, u16)> {
        self.inner.value_ref.map(|r| (r.num, r.gen))
    }

    /// `/Filter`.
    #[getter]
    fn filter(&self) -> Option<&str> {
        self.inner.filter.as_deref()
    }

    /// `/SubFilter` when it is one this build recognises.
    #[getter]
    fn sub_filter(&self) -> Option<&'static str> {
        self.inner.sub_filter.map(tinker_pdf::SubFilter::name)
    }

    /// `/SubFilter` exactly as written, recognised or not.
    #[getter]
    fn sub_filter_name(&self) -> Option<&str> {
        self.inner.sub_filter_name.as_deref()
    }

    /// The `/ByteRange` spans as `(start, end)` file offsets.
    #[getter]
    fn spans(&self) -> Vec<(u64, u64)> {
        self.inner.spans.iter().map(|r| (r.start, r.end)).collect()
    }

    /// "whole-file", "revision" or "suspicious".
    #[getter]
    fn coverage(&self) -> &'static str {
        coverage_kind(&self.inner.coverage)
    }

    /// Which revision a "revision" coverage ends at, newest first.
    #[getter]
    fn coverage_revision(&self) -> Option<usize> {
        coverage_revision(&self.inner.coverage)
    }

    /// Why a "suspicious" coverage is: "missing", "not-four-numbers",
    /// "not-an-offset", "does-not-start-at-zero", "spans-overlap",
    /// "past-end-of-file", "gap-is-not-contents" or "ends-mid-file".
    #[getter]
    fn coverage_defect(&self) -> Option<&'static str> {
        coverage_defect(&self.inner.coverage)
    }

    /// Whether the coverage is the whole file but the `/Contents` gap.
    #[getter]
    fn covers_whole_file(&self) -> bool {
        self.inner.covers_whole_file()
    }

    /// Whether this is a usage-rights signature, which says nothing about the
    /// content.
    #[getter]
    fn is_usage_rights(&self) -> bool {
        self.inner.is_usage_rights()
    }

    /// The stored `/Contents` bytes — DER for the CMS subfilters.
    #[getter]
    fn contents<'py>(&self, py: Python<'py>) -> Bound<'py, PyBytes> {
        PyBytes::new(py, &self.inner.contents)
    }

    /// `/M`, the time the signer claims, as `(year, month, day, hour,
    /// minute, second, utc_offset_minutes)`; unverified by construction.
    #[getter]
    #[allow(clippy::type_complexity)]
    fn signed_at(&self) -> Option<(i32, u8, u8, u8, u8, u8, Option<i32>)> {
        self.inner.signed_at.map(|d| {
            (
                d.year,
                d.month,
                d.day,
                d.hour,
                d.minute,
                d.second,
                d.utc_offset_minutes,
            )
        })
    }

    /// `/Reason`.
    #[getter]
    fn reason(&self) -> Option<&str> {
        self.inner.reason.as_deref()
    }

    /// `/Location`.
    #[getter]
    fn location(&self) -> Option<&str> {
        self.inner.location.as_deref()
    }

    /// `/Name` — who the signer claims to be, which is not the certificate's
    /// answer (that is `Verdict.signer_subject`).
    #[getter]
    fn name(&self) -> Option<&str> {
        self.inner.name.as_deref()
    }

    /// `/ContactInfo`.
    #[getter]
    fn contact(&self) -> Option<&str> {
        self.inner.contact.as_deref()
    }

    /// The `/DocMDP` level, 1 to 3, for a certifying signature.
    #[getter]
    fn certification_level(&self) -> Option<u8> {
        self.inner.certification.map(Certification::level)
    }

    /// `/FieldMDP`'s action — "all", "include" or "exclude" — with
    /// `field_lock_fields` naming the fields for the last two.
    #[getter]
    fn field_lock(&self) -> Option<&'static str> {
        self.inner.field_lock.as_ref().map(|lock| match lock {
            FieldLock::All => "all",
            FieldLock::Include(_) => "include",
            FieldLock::Exclude(_) => "exclude",
        })
    }

    /// The fields a "include" or "exclude" field lock names.
    #[getter]
    fn field_lock_fields(&self) -> Vec<String> {
        match &self.inner.field_lock {
            Some(FieldLock::Include(names) | FieldLock::Exclude(names)) => names.clone(),
            _ => Vec::new(),
        }
    }

    /// What was read leniently, by name.
    #[getter]
    fn warnings(&self) -> Vec<&'static str> {
        self.inner
            .warnings
            .iter()
            .map(|warning| match warning {
                SignatureWarning::ValueNotADictionary => "value-not-a-dictionary",
                SignatureWarning::ContentsMissing => "contents-missing",
                SignatureWarning::ContentsNotHexadecimal => "contents-not-hexadecimal",
                SignatureWarning::ContentsOddDigitCount => "contents-odd-digit-count",
                SignatureWarning::ContentsGapExcludesDelimiters => {
                    "contents-gap-excludes-delimiters"
                }
                SignatureWarning::SubFilterUnknown(_) => "sub-filter-unknown",
                SignatureWarning::SubFilterMissing => "sub-filter-missing",
            })
            .collect()
    }

    fn __repr__(&self) -> String {
        format!(
            "<tinker_pdf.Signature field={:?} coverage={}>",
            self.inner.field,
            self.coverage()
        )
    }
}

/// Certificates the caller trusts, as DER. Empty is how a caller says it
/// trusts nothing; there is no default, because the engine ships no root
/// store and a binding that invented one would be adding a policy.
#[pyclass(name = "TrustAnchors")]
pub struct PyTrustAnchors {
    inner: tinker_pdf::TrustAnchors,
}

#[pymethods]
impl PyTrustAnchors {
    #[new]
    fn new() -> PyTrustAnchors {
        PyTrustAnchors {
            inner: tinker_pdf::TrustAnchors::new(),
        }
    }

    /// Adds a certificate. Raises `ValueError` — and keeps nothing — when the
    /// bytes are not one.
    fn add(&mut self, der: Vec<u8>) -> PyResult<()> {
        self.inner.add(der).map_err(PyValueError::new_err)
    }

    fn __len__(&self) -> usize {
        self.inner.len()
    }
}

/// Who the signer's certificate says they are.
#[pyclass(name = "Signer", get_all, frozen)]
pub struct PySigner {
    /// The subject, rendered per RFC 4514.
    subject: String,
    /// The issuer.
    issuer: String,
    /// `(not_before, not_after)`, seconds since the Unix epoch.
    validity: (i64, i64),
    /// The `signingTime` the signer claims; nothing countersigned it.
    claimed_signing_time: Option<i64>,
    /// Whether the blob carries an RFC 3161 timestamp token.
    timestamped: bool,
}

/// What one signature turned out to prove: four questions, never one
/// boolean.
#[pyclass(name = "Verdict", frozen)]
pub struct PyVerdict {
    inner: tinker_pdf::Verdict,
}

#[pymethods]
impl PyVerdict {
    /// "whole-file", "revision" or "suspicious".
    #[getter]
    fn coverage(&self) -> &'static str {
        coverage_kind(&self.inner.coverage)
    }

    /// Which revision a "revision" coverage ends at.
    #[getter]
    fn coverage_revision(&self) -> Option<usize> {
        coverage_revision(&self.inner.coverage)
    }

    /// Why a "suspicious" coverage is.
    #[getter]
    fn coverage_defect(&self) -> Option<&'static str> {
        coverage_defect(&self.inner.coverage)
    }

    /// "read", "absent" or "unreadable".
    #[getter]
    fn cms(&self) -> &'static str {
        match self.inner.cms {
            CmsState::Read { .. } => "read",
            CmsState::Absent => "absent",
            CmsState::Unreadable(_) => "unreadable",
        }
    }

    /// How many `SignerInfo`s a "read" CMS carries.
    #[getter]
    fn cms_signers(&self) -> Option<usize> {
        match self.inner.cms {
            CmsState::Read { signers } => Some(signers),
            _ => None,
        }
    }

    /// The parser's reason for "unreadable".
    #[getter]
    fn cms_reason(&self) -> Option<&str> {
        match &self.inner.cms {
            CmsState::Unreadable(reason) => Some(reason),
            _ => None,
        }
    }

    /// "matches", "differs" or "not-checked".
    #[getter]
    fn document_digest(&self) -> &'static str {
        match self.inner.document_digest {
            DocumentDigest::Matches => "matches",
            DocumentDigest::Differs => "differs",
            DocumentDigest::NotChecked(_) => "not-checked",
        }
    }

    /// Why the digest was not checked, by name.
    #[getter]
    fn document_digest_reason(&self) -> Option<&'static str> {
        match &self.inner.document_digest {
            DocumentDigest::NotChecked(reason) => Some(unchecked(reason)),
            _ => None,
        }
    }

    /// "verified", "failed" or "not-checked".
    #[getter]
    fn signature(&self) -> &'static str {
        match self.inner.signature {
            SignatureCheck::Verified => "verified",
            SignatureCheck::Failed => "failed",
            SignatureCheck::NotChecked(_) => "not-checked",
        }
    }

    /// Why the signature was not checked, by name.
    #[getter]
    fn signature_reason(&self) -> Option<&'static str> {
        match &self.inner.signature {
            SignatureCheck::NotChecked(reason) => Some(unchecked(reason)),
            _ => None,
        }
    }

    /// "anchored-to", "self-signed", "incomplete", "broken", "no-anchors" or
    /// "no-signer-certificate".
    #[getter]
    fn chain(&self) -> &'static str {
        match self.inner.chain {
            Chain::AnchoredTo { .. } => "anchored-to",
            Chain::SelfSigned { .. } => "self-signed",
            Chain::Incomplete { .. } => "incomplete",
            Chain::Broken { .. } => "broken",
            Chain::NoAnchors => "no-anchors",
            Chain::NoSignerCertificate => "no-signer-certificate",
        }
    }

    /// The certificate the chain arm names: the anchor reached, the
    /// self-signed root, the missing issuer, or where the path broke.
    #[getter]
    fn chain_subject(&self) -> Option<&str> {
        match &self.inner.chain {
            Chain::AnchoredTo { anchor, .. } => Some(anchor),
            Chain::SelfSigned { subject } => Some(subject),
            Chain::Incomplete { missing_issuer } => Some(missing_issuer),
            Chain::Broken { at } => Some(at),
            _ => None,
        }
    }

    /// How many certificates were between the signer and the anchor.
    #[getter]
    fn chain_links(&self) -> Option<usize> {
        match self.inner.chain {
            Chain::AnchoredTo { links, .. } => Some(links),
            _ => None,
        }
    }

    /// What was accepted and is worth saying, as `(kind, detail)`: the bits
    /// of a "short-rsa-key", the subject of an "outside-validity", `None`
    /// otherwise.
    #[getter]
    fn weaknesses(&self, py: Python<'_>) -> PyResult<Vec<(&'static str, Py<PyAny>)>> {
        self.inner
            .weaknesses
            .iter()
            .map(|weakness| {
                Ok(match weakness {
                    Weakness::Sha1Digest => ("sha1-digest", py.None()),
                    Weakness::Sha1Signature => ("sha1-signature", py.None()),
                    Weakness::ShortRsaKey { bits } => {
                        ("short-rsa-key", bits.into_pyobject(py)?.into_any().unbind())
                    }
                    Weakness::CoversOnlyARevision => ("covers-only-a-revision", py.None()),
                    Weakness::CoverageSuspicious => ("coverage-suspicious", py.None()),
                    Weakness::OutsideValidity { subject } => (
                        "outside-validity",
                        subject.into_pyobject(py)?.into_any().unbind(),
                    ),
                })
            })
            .collect()
    }

    /// Who the signer's certificate says they are, or `None`.
    #[getter]
    fn signer(&self) -> Option<PySigner> {
        self.inner.signer.as_ref().map(|signer| PySigner {
            subject: signer.subject.clone(),
            issuer: signer.issuer.clone(),
            validity: signer.validity,
            claimed_signing_time: signer.claimed_signing_time,
            timestamped: signer.timestamped,
        })
    }

    /// The facade's conservative convenience: whole file, digest matches,
    /// signature verified, chain anchored. It discards every distinction the
    /// attributes make; anything reporting to a person should read those.
    fn is_trusted(&self) -> bool {
        self.inner.is_trusted()
    }

    fn __repr__(&self) -> String {
        format!(
            "<tinker_pdf.Verdict {} {} {} {}>",
            self.coverage(),
            self.document_digest(),
            self.signature(),
            self.chain()
        )
    }
}

pub fn signatures(document: &Document) -> Vec<PySignature> {
    document
        .signatures()
        .into_iter()
        .map(|inner| PySignature { inner })
        .collect()
}

pub fn verify(document: &Document, anchors: &PyTrustAnchors, at: Option<i64>) -> Vec<PyVerdict> {
    document
        .verify_signatures(&anchors.inner, at)
        .into_iter()
        .map(|inner| PyVerdict { inner })
        .collect()
}

pub fn register(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add_class::<PySignature>()?;
    module.add_class::<PyTrustAnchors>()?;
    module.add_class::<PySigner>()?;
    module.add_class::<PyVerdict>()?;
    Ok(())
}
