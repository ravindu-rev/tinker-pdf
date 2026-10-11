package tinkerpdf

/*
#include <stdlib.h>
#include "tinker_pdf.h"
*/
import "C"

// Coverage is what a signature's /ByteRange covers, checked against the file.
type Coverage int

// The answers: the header's own TpdfCoverage constants, so cgo checks each number.
const (
	CoverageWholeFile  Coverage = C.TPDF_COVERAGE_WHOLE_FILE
	CoverageRevision   Coverage = C.TPDF_COVERAGE_REVISION
	CoverageSuspicious Coverage = C.TPDF_COVERAGE_SUSPICIOUS
)

// CmsState is whether the CMS blob could be read.
type CmsState int

// The answers: the header's own TpdfCmsState constants, so cgo checks each number.
const (
	CmsRead       CmsState = C.TPDF_CMS_STATE_READ
	CmsAbsent     CmsState = C.TPDF_CMS_STATE_ABSENT
	CmsUnreadable CmsState = C.TPDF_CMS_STATE_UNREADABLE
)

// DocumentDigest is whether the covered bytes still hash to what was signed.
type DocumentDigest int

// The answers: the header's own TpdfDocumentDigest constants, so cgo checks each number.
const (
	DigestMatches    DocumentDigest = C.TPDF_DOCUMENT_DIGEST_MATCHES
	DigestDiffers    DocumentDigest = C.TPDF_DOCUMENT_DIGEST_DIFFERS
	DigestNotChecked DocumentDigest = C.TPDF_DOCUMENT_DIGEST_NOT_CHECKED
)

// SignatureCheck is whether the signature verifies against the signer's key.
type SignatureCheck int

// The answers: the header's own TpdfSignatureCheck constants, so cgo checks each number.
const (
	SignatureVerified   SignatureCheck = C.TPDF_SIGNATURE_CHECK_VERIFIED
	SignatureFailed     SignatureCheck = C.TPDF_SIGNATURE_CHECK_FAILED
	SignatureNotChecked SignatureCheck = C.TPDF_SIGNATURE_CHECK_NOT_CHECKED
)

// Chain is how far the certificate chain reached.
type Chain int

// The answers: the header's own TpdfChain constants, so cgo checks each number.
const (
	ChainAnchoredTo          Chain = C.TPDF_CHAIN_ANCHORED_TO
	ChainSelfSigned          Chain = C.TPDF_CHAIN_SELF_SIGNED
	ChainIncomplete          Chain = C.TPDF_CHAIN_INCOMPLETE
	ChainBroken              Chain = C.TPDF_CHAIN_BROKEN
	ChainNoAnchors           Chain = C.TPDF_CHAIN_NO_ANCHORS
	ChainNoSignerCertificate Chain = C.TPDF_CHAIN_NO_SIGNER_CERTIFICATE
)

// Weakness is something accepted that a caller should be told about.
type Weakness int

// The weaknesses: the header's own TpdfWeakness constants, so cgo checks each number.
const (
	WeaknessSha1Digest          Weakness = C.TPDF_WEAKNESS_SHA1_DIGEST
	WeaknessSha1Signature       Weakness = C.TPDF_WEAKNESS_SHA1_SIGNATURE
	WeaknessShortRsaKey         Weakness = C.TPDF_WEAKNESS_SHORT_RSA_KEY
	WeaknessCoversOnlyARevision Weakness = C.TPDF_WEAKNESS_COVERS_ONLY_A_REVISION
	WeaknessCoverageSuspicious  Weakness = C.TPDF_WEAKNESS_COVERAGE_SUSPICIOUS
	WeaknessOutsideValidity     Weakness = C.TPDF_WEAKNESS_OUTSIDE_VALIDITY
)

// Span is one /ByteRange span.
type Span struct {
	Start, Length uint64
}

// Signature is one signature dictionary, read — not verified.
type Signature struct {
	FieldName, SubFilter, Reason, Location, Name *string
	Coverage                                     Coverage
	CoversWholeFile, IsUsageRights               bool
	// CertificationLevel is 1 to 3, or 0 for none.
	CertificationLevel uint32
	Spans              []Span
}

// Verdict is what one signature turned out to prove: four answers, never
// one boolean.
type Verdict struct {
	Cms                         CmsState
	DocumentDigest              DocumentDigest
	Signature                   SignatureCheck
	Chain                       Chain
	SignerSubject, SignerIssuer *string
	// SignerValidity is (notBefore, notAfter) in seconds since the Unix
	// epoch, or nil when there is no signer certificate.
	SignerValidity *[2]int64
	Weaknesses     []Weakness
}

// TrustAnchors is the certificates the caller trusts, as DER. Empty is how a
// caller says it trusts nothing; the engine ships no root store.
type TrustAnchors struct {
	ptr *C.TpdfTrustAnchors
}

// NewTrustAnchors starts an empty set.
func NewTrustAnchors() *TrustAnchors {
	return &TrustAnchors{ptr: C.tpdf_trust_anchors_new()}
}

// Add adds a certificate; bytes that are not one are refused and not kept.
func (t *TrustAnchors) Add(der []byte) error {
	return call(func() C.enum_TpdfStatus {
		data, length := bytesArg(der)
		return C.tpdf_trust_anchors_add(t.ptr, data, length)
	})
}

// Count is how many anchors.
func (t *TrustAnchors) Count() uint32 { return uint32(C.tpdf_trust_anchors_count(t.ptr)) }

// Close releases the set.
func (t *TrustAnchors) Close() {
	if t != nil && t.ptr != nil {
		C.tpdf_trust_anchors_free(t.ptr)
		t.ptr = nil
	}
}

func signatureText(signatures *C.TpdfSignatures, index C.uint32_t,
	f func(*C.TpdfSignatures, C.uint32_t, **C.char) C.enum_TpdfStatus) (*string, error) {
	var out *C.char
	if err := call(func() C.enum_TpdfStatus { return f(signatures, index, &out) }); err != nil {
		return nil, err
	}
	return takeString(out), nil
}

// Signatures reads every signature the document carries (12.8).
func (d *Document) Signatures() ([]Signature, error) {
	var signatures *C.TpdfSignatures
	if err := call(func() C.enum_TpdfStatus { return C.tpdf_document_signatures(d.ptr, &signatures) }); err != nil {
		return nil, err
	}
	defer C.tpdf_signatures_free(signatures)
	count := uint32(C.tpdf_signatures_count(signatures))
	found := make([]Signature, 0, count)
	for i := uint32(0); i < count; i++ {
		index := C.uint32_t(i)
		var s Signature
		var err error
		if s.FieldName, err = signatureText(signatures, index, func(p *C.TpdfSignatures, i C.uint32_t, o **C.char) C.enum_TpdfStatus {
			return C.tpdf_signature_field_name(p, i, o)
		}); err != nil {
			return nil, err
		}
		if s.SubFilter, err = signatureText(signatures, index, func(p *C.TpdfSignatures, i C.uint32_t, o **C.char) C.enum_TpdfStatus {
			return C.tpdf_signature_sub_filter(p, i, o)
		}); err != nil {
			return nil, err
		}
		if s.Reason, err = signatureText(signatures, index, func(p *C.TpdfSignatures, i C.uint32_t, o **C.char) C.enum_TpdfStatus {
			return C.tpdf_signature_reason(p, i, o)
		}); err != nil {
			return nil, err
		}
		if s.Location, err = signatureText(signatures, index, func(p *C.TpdfSignatures, i C.uint32_t, o **C.char) C.enum_TpdfStatus {
			return C.tpdf_signature_location(p, i, o)
		}); err != nil {
			return nil, err
		}
		if s.Name, err = signatureText(signatures, index, func(p *C.TpdfSignatures, i C.uint32_t, o **C.char) C.enum_TpdfStatus {
			return C.tpdf_signature_name(p, i, o)
		}); err != nil {
			return nil, err
		}
		var coverage C.enum_TpdfCoverage
		if err := call(func() C.enum_TpdfStatus { return C.tpdf_signature_coverage(signatures, index, &coverage) }); err != nil {
			return nil, err
		}
		s.Coverage = Coverage(coverage)
		s.CoversWholeFile = C.tpdf_signature_covers_whole_file(signatures, index) != 0
		s.IsUsageRights = C.tpdf_signature_is_usage_rights(signatures, index) != 0
		s.CertificationLevel = uint32(C.tpdf_signature_certification_level(signatures, index))
		spans := uint32(C.tpdf_signature_span_count(signatures, index))
		for span := uint32(0); span < spans; span++ {
			var start, length C.uint64_t
			if err := call(func() C.enum_TpdfStatus {
				return C.tpdf_signature_span(signatures, index, C.uint32_t(span), &start, &length)
			}); err != nil {
				return nil, err
			}
			s.Spans = append(s.Spans, Span{Start: uint64(start), Length: uint64(length)})
		}
		found = append(found, s)
	}
	return found, nil
}

// VerifySignatures answers what every signature proves, in Signatures'
// order. at is the instant to judge certificate validity at, in seconds since
// the Unix epoch; nil judges nothing, because "expired" is a claim about a
// moment the caller has to name.
func (d *Document) VerifySignatures(anchors *TrustAnchors, at *int64) ([]Verdict, error) {
	judge, instant := C.int(0), C.int64_t(0)
	if at != nil {
		judge, instant = 1, C.int64_t(*at)
	}
	var verdicts *C.TpdfVerdicts
	if err := call(func() C.enum_TpdfStatus {
		return C.tpdf_document_verify_signatures(d.ptr, anchors.ptr, judge, instant, &verdicts)
	}); err != nil {
		return nil, err
	}
	defer C.tpdf_verdicts_free(verdicts)
	count := uint32(C.tpdf_verdicts_count(verdicts))
	found := make([]Verdict, 0, count)
	for i := uint32(0); i < count; i++ {
		index := C.uint32_t(i)
		var v Verdict
		var cms C.enum_TpdfCmsState
		var digest C.enum_TpdfDocumentDigest
		var check C.enum_TpdfSignatureCheck
		var chain C.enum_TpdfChain
		var subject, issuer *C.char
		if err := call(func() C.enum_TpdfStatus { return C.tpdf_verdict_cms_state(verdicts, index, &cms) }); err != nil {
			return nil, err
		}
		if err := call(func() C.enum_TpdfStatus { return C.tpdf_verdict_document_digest(verdicts, index, &digest) }); err != nil {
			return nil, err
		}
		if err := call(func() C.enum_TpdfStatus { return C.tpdf_verdict_signature_check(verdicts, index, &check) }); err != nil {
			return nil, err
		}
		if err := call(func() C.enum_TpdfStatus { return C.tpdf_verdict_chain(verdicts, index, &chain) }); err != nil {
			return nil, err
		}
		if err := call(func() C.enum_TpdfStatus { return C.tpdf_verdict_signer_subject(verdicts, index, &subject) }); err != nil {
			return nil, err
		}
		if err := call(func() C.enum_TpdfStatus { return C.tpdf_verdict_signer_issuer(verdicts, index, &issuer) }); err != nil {
			return nil, err
		}
		v.Cms, v.DocumentDigest, v.Signature, v.Chain = CmsState(cms), DocumentDigest(digest), SignatureCheck(check), Chain(chain)
		v.SignerSubject, v.SignerIssuer = takeString(subject), takeString(issuer)
		// The flag says whether the engine wrote anything; when it is 0 the
		// slots keep the zeros given here.
		var notBefore, notAfter C.int64_t
		if C.tpdf_verdict_signer_validity(verdicts, index, &notBefore, &notAfter) != 0 {
			v.SignerValidity = &[2]int64{int64(notBefore), int64(notAfter)}
		}
		weaknesses := uint32(C.tpdf_verdict_weakness_count(verdicts, index))
		for w := uint32(0); w < weaknesses; w++ {
			var weakness C.enum_TpdfWeakness
			if err := call(func() C.enum_TpdfStatus {
				return C.tpdf_verdict_weakness(verdicts, index, C.uint32_t(w), &weakness)
			}); err != nil {
				return nil, err
			}
			v.Weaknesses = append(v.Weaknesses, Weakness(weakness))
		}
		found = append(found, v)
	}
	return found, nil
}
