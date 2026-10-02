//! Signature shapes the corpus has too few of, each held to a signature a
//! second implementation made.
//!
//! The fetched corpora carry 34 `SignerInfo`s and every one is PKCS#1 v1.5
//! with signed attributes over a detached `adbe.pkcs7.detached` blob, bar one
//! `adbe.pkcs7.sha1` file that is a fuzzer's output and one signer with no
//! signed attributes (`docs/features/signatures.md`). So the shapes below
//! cannot be adjudicated by the corpus, and the roadmap's other route applies:
//! each is wired against a published vector or a real signature.
//!
//! # Which link is adjudicated by what
//!
//! **The arithmetic is published vectors'.** RSASSA-PSS is gated in
//! `tinker-pdf-crypto` on 360 NIST CAVP `SigVerPSS` vectors and on RSA
//! Laboratories' 60, whose 1 025- to 1 031-bit keys are the edge NIST's file
//! never reaches. Nothing below re-proves that.
//!
//! **The CMS, the certificates and the signature values are OpenSSL's.**
//! `signature_support/signature-fixtures.py` ran once, on 2 October 2026, with
//! **OpenSSL 3.0.13**: key generation, the certificate chains, the
//! `SignedData` and every signature in it. A reading of the parameters, the
//! encoding or the attribute set that disagreed with OpenSSL's would stop the
//! signature verifying.
//!
//! **The `/ByteRange` spans are this repository's on both sides**, exactly as
//! in `ecdsa_verdict.rs`: the generator computed them and
//! `crates/tinker-pdf/src/signature.rs` recomputes them, one author's reading
//! of 12.8.1 twice. A `Matches` below catches a slip between the two and not a
//! misreading of the clause — and the covered bytes reach OpenSSL as bytes, so
//! a disagreement about *which* bytes would surface as `Differs`.

use std::ops::Range;

use tinker_pdf::{
    Chain, CmsState, Coverage, Document, DocumentDigest, SignatureCheck, TrustAnchors, Verdict,
    Weakness,
};
use tinker_pdf_crypto::{DigestAlgorithm as CryptoDigest, PssParameters};
use tinker_pdf_pki::{oid, pss, Certificate, ContentInfo, SignatureAlgorithm};

const PSS_PDF: &[u8] = include_bytes!("signature_support/rsa-pss.pdf");
const PSS_ROOT: &[u8] = include_bytes!("signature_support/rsa-pss-root.der");

/// Inside every fixture certificate's validity window and nothing to do with
/// now: 1 January 2027. Ruling 4 keeps the clock out of the engine.
const AT: i64 = 1_798_761_600;

// ---- RSASSA-PSS ------------------------------------------------------------

/// The parameters OpenSSL wrote, read back: SHA-256, MGF1 over SHA-256, a
/// 32-octet salt — on the signer, on both certificates' signatures, and as
/// the leaf key's own restriction.
#[test]
fn the_pss_fixture_is_the_shape_it_claims_to_be() {
    let der = cms_of(PSS_PDF);
    let content = ContentInfo::parse(&der).expect("the CMS parses");
    let signed = content.signed_data();
    let signer = signed.signer_infos().first().expect("one signer");
    let expected = PssParameters {
        hash: CryptoDigest::Sha256,
        mask_hash: CryptoDigest::Sha256,
        salt_length: 32,
    };
    assert_eq!(signer.signature_algorithm(), Ok(SignatureAlgorithm::RsaPss));
    assert_eq!(
        pss::parameters(&signer.signature_algorithm_id()),
        Ok(expected)
    );

    let certificates: Vec<Certificate<'_>> = signed
        .x509_certificates()
        .map(|der| Certificate::parse(der).expect("each certificate parses"))
        .collect();
    assert_eq!(certificates.len(), 2, "the signer's and the root's");
    for certificate in &certificates {
        assert_eq!(certificate.signature_algorithm().oid(), oid::RSASSA_PSS);
        assert_eq!(
            pss::parameters(&certificate.signature_algorithm()),
            Ok(expected)
        );
    }
    // The signer's key is an `id-RSASSA-PSS` key with restrictions, which is
    // what RFC 4056 §3's checks are about; the root's is plain RSA.
    let spki = certificates[0].subject_public_key_info();
    assert_eq!(spki.algorithm().oid(), oid::RSASSA_PSS);
    assert_eq!(pss::parameters(&spki.algorithm()), Ok(expected));
    assert!(matches!(
        spki.public_key(),
        tinker_pdf_pki::PublicKey::Rsa { .. }
    ));
    assert_eq!(
        certificates[1].subject_public_key_info().algorithm().oid(),
        oid::RSA_ENCRYPTION
    );
}

#[test]
fn an_rsa_pss_signed_document_reaches_every_one_of_the_four_answers() {
    let document = Document::open(PSS_PDF.to_vec()).expect("the fixture opens");
    assert_eq!(document.signatures()[0].coverage, Coverage::WholeFile);

    let verdict = verdict_for(PSS_PDF, PSS_ROOT);
    assert_eq!(verdict.cms, CmsState::Read { signers: 1 });
    assert_eq!(
        verdict.document_digest,
        DocumentDigest::Matches,
        "the covered bytes still hash to OpenSSL's messageDigest"
    );
    assert_eq!(
        verdict.signature,
        SignatureCheck::Verified,
        "the arm this row exists to wire"
    );
    match &verdict.chain {
        Chain::AnchoredTo { anchor, links } => {
            assert!(anchor.contains("RSASSA-PSS Test Root"), "{anchor}");
            assert_eq!(*links, 0, "the signer's issuer is the anchor");
        }
        other => panic!("expected the walk to reach the anchor over a PSS link, got {other:?}"),
    }
    assert!(verdict.weaknesses.is_empty(), "{:?}", verdict.weaknesses);
    assert!(verdict.is_trusted());
}

#[test]
fn a_changed_document_differs_while_the_pss_signature_still_verifies() {
    let mut tampered = PSS_PDF.to_vec();
    let at = find(&tampered, b"0.2 0.6 0.3 rg").expect("the content stream");
    tampered[at + 2] = b'9';
    let verdict = verdict_for(&tampered, PSS_ROOT);
    assert_eq!(verdict.document_digest, DocumentDigest::Differs);
    assert_eq!(verdict.signature, SignatureCheck::Verified);
    assert!(!verdict.is_trusted());
}

#[test]
fn a_flipped_bit_in_the_pss_signature_fails_rather_than_going_unchecked() {
    // Tells "verified" from "reached the arm": only an arm that runs the
    // EMSA-PSS unmasking says `Failed` here.
    let mut der = cms_of(PSS_PDF);
    let at = signature_value_at(&der);
    der[at.end - 1] ^= 0x01;
    let verdict = verdict_for(&replace_cms(PSS_PDF, &der), PSS_ROOT);
    assert_eq!(verdict.document_digest, DocumentDigest::Matches);
    assert_eq!(verdict.signature, SignatureCheck::Failed);
    assert!(!verdict.is_trusted());
}

#[test]
fn the_signature_is_read_under_the_salt_length_its_parameters_declare() {
    // `signatureAlgorithm` sits outside the signed attributes, so changing its
    // salt length moves nothing the signature covers — only the instruction
    // for reading it. 32 octets declared as 33 must not verify: the padding
    // would end one octet late, and a verifier that inferred the salt from
    // the block instead of the parameters would not notice.
    //
    // Longer rather than shorter on purpose. The leaf key restricts itself to
    // salts of at least 32 (RFC 4056 §3), so a declared 31 is refused by that
    // check before the arithmetic runs — which is what the first version of
    // this test did, and a counted injection that made the PSS arm answer
    // `Verified` without verifying found it still passing. 33 is a salt the
    // key permits, so only EMSA-PSS-VERIFY can refuse it.
    let mut der = cms_of(PSS_PDF);
    let at = signer_salt_length_at(&der);
    assert_eq!(der[at], 0x20);
    der[at] = 0x21;
    let verdict = verdict_for(&replace_cms(PSS_PDF, &der), PSS_ROOT);
    assert_eq!(verdict.document_digest, DocumentDigest::Matches);
    assert_eq!(verdict.signature, SignatureCheck::Failed);
}

#[test]
fn a_key_restricted_to_a_longer_salt_refuses_a_signature_the_arithmetic_accepts() {
    // RFC 4056 §3 step 3: the signature's salt must be at least the key's
    // minimum. Raising the minimum the leaf certificate states to 33 leaves
    // the signature itself intact and verifiable — so a build without the
    // check says `Verified`, and RFC 4056 says that "MUST fail validation".
    // The certificate's own signature no longer covers what it says, so the
    // chain breaks too; the assertion that matters is the first.
    let mut der = cms_of(PSS_PDF);
    let at = leaf_key_salt_length_at(&der);
    assert_eq!(der[at], 0x20);
    der[at] = 0x21;
    let verdict = verdict_for(&replace_cms(PSS_PDF, &der), PSS_ROOT);
    assert_eq!(verdict.signature, SignatureCheck::Failed);
    assert!(
        matches!(verdict.chain, Chain::Broken { .. }),
        "{:?}",
        verdict.chain
    );
}

#[test]
fn a_pss_chain_link_whose_signature_is_wrong_breaks_the_path() {
    // `verdict::verifies`' PSS arm: the root's PSS signature over the leaf's
    // stored `TBSCertificate`. Before this arm the walk reported every PSS
    // certificate as `Broken`, good or forged.
    let mut der = cms_of(PSS_PDF);
    let at = leaf_certificate_signature_at(&der);
    der[at.end - 1] ^= 0x01;
    let verdict = verdict_for(&replace_cms(PSS_PDF, &der), PSS_ROOT);
    assert_eq!(verdict.signature, SignatureCheck::Verified);
    match &verdict.chain {
        Chain::Broken { at } => assert!(at.contains("Root"), "broken at {at:?}"),
        other => panic!("expected a broken path, got {other:?}"),
    }
}

#[test]
fn without_anchors_the_pss_chain_is_not_walked() {
    let document = Document::open(PSS_PDF.to_vec()).expect("opens");
    let verdicts = document.verify_signatures(&TrustAnchors::new(), Some(AT));
    assert_eq!(verdicts[0].signature, SignatureCheck::Verified);
    assert_eq!(verdicts[0].chain, Chain::NoAnchors);
}

// ---- reading and rewriting the fixtures -----------------------------------

fn verdict_for(pdf: &[u8], root: &[u8]) -> Verdict {
    let document = Document::open(pdf.to_vec()).expect("the fixture opens");
    let mut anchors = TrustAnchors::new();
    anchors.add(root.to_vec()).expect("the root parses");
    let mut verdicts = document.verify_signatures(&anchors, Some(AT));
    assert_eq!(verdicts.len(), 1);
    verdicts.remove(0)
}

/// The CMS blob, trimmed of the reservation's zero fill.
fn cms_of(pdf: &[u8]) -> Vec<u8> {
    let document = Document::open(pdf.to_vec()).expect("the fixture opens");
    document.signatures()[0].cms().to_vec()
}

/// The same document with a different blob in `/Contents`, in the same
/// reservation, so no offset in the file moves.
fn replace_cms(pdf: &[u8], der: &[u8]) -> Vec<u8> {
    let start = find(pdf, b"/Contents <").expect("the fixture has one") + b"/Contents <".len();
    let end = start + find(&pdf[start..], b">").expect("it is closed");
    assert!(der.len() * 2 <= end - start, "the reservation holds it");
    let mut hex = String::with_capacity(end - start);
    for byte in der {
        hex.push_str(&format!("{byte:02X}"));
    }
    while hex.len() < end - start {
        hex.push('0');
    }
    let mut out = pdf.to_vec();
    out[start..end].copy_from_slice(hex.as_bytes());
    out
}

/// Where `slice` — which the parser handed back from `whole` — sits in it.
fn offset_in(whole: &[u8], slice: &[u8]) -> Range<usize> {
    let at = slice.as_ptr() as usize - whole.as_ptr() as usize;
    at..at + slice.len()
}

fn signature_value_at(der: &[u8]) -> Range<usize> {
    let content = ContentInfo::parse(der).expect("the CMS parses");
    let signer = content
        .signed_data()
        .signer_infos()
        .first()
        .expect("one signer");
    offset_in(der, signer.signature())
}

/// The salt-length octet of `A2 03 02 01 xx` inside `algorithm`'s DER.
fn salt_octet(der: &[u8], algorithm: &[u8]) -> usize {
    let range = offset_in(der, algorithm);
    let inside = find(&der[range.clone()], &[0xA2, 0x03, 0x02, 0x01]).expect("[2] saltLength");
    range.start + inside + 4
}

fn signer_salt_length_at(der: &[u8]) -> usize {
    let content = ContentInfo::parse(der).expect("the CMS parses");
    let signer = content
        .signed_data()
        .signer_infos()
        .first()
        .expect("one signer");
    salt_octet(der, signer.signature_algorithm_id().der())
}

fn leaf_key_salt_length_at(der: &[u8]) -> usize {
    let content = ContentInfo::parse(der).expect("the CMS parses");
    let leaf = content
        .signed_data()
        .x509_certificates()
        .next()
        .expect("the leaf");
    let certificate = Certificate::parse(leaf).expect("it parses");
    salt_octet(der, certificate.subject_public_key_info().algorithm().der())
}

fn leaf_certificate_signature_at(der: &[u8]) -> Range<usize> {
    let content = ContentInfo::parse(der).expect("the CMS parses");
    let leaf = content
        .signed_data()
        .x509_certificates()
        .next()
        .expect("the leaf");
    let certificate = Certificate::parse(leaf).expect("it parses");
    offset_in(
        der,
        certificate.signature().whole_bytes().expect("whole octets"),
    )
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len())
        .position(|window| window == needle)
}

/// Referenced so that a new weakness is a compile error here, where the
/// shapes above would have to decide whether it applies.
#[allow(dead_code)]
fn weaknesses_are_exhaustive(weakness: &Weakness) -> &'static str {
    match weakness {
        Weakness::Sha1Digest => "sha-1 digest",
        Weakness::Sha1Signature => "sha-1 signature",
        Weakness::ShortRsaKey { .. } => "short rsa key",
        Weakness::CoversOnlyARevision => "a revision",
        Weakness::CoverageSuspicious => "suspicious coverage",
        Weakness::OutsideValidity { .. } => "outside validity",
    }
}
