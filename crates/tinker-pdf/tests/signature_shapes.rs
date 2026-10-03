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
//!
//! **One section is the exception, and says so**: shapes OpenSSL will not
//! sign — a PKCS#1 v1.5 signature under a key restricted to RSASSA-PSS, signed
//! attributes with no `messageDigest` — are assembled in this file and signed
//! with the committed throwaway key. Each is a refusal with a control beside
//! it, and what holds them up is this engine's own reading of RFC 5652.

use std::ops::Range;

use tinker_pdf::{
    AuthorityCertificate, Chain, CmsState, Coverage, DigestAlgorithm, Document, DocumentDigest,
    SignRefused, SignatureCheck, Signer, SigningRequest, SigningTarget, Stamped, SubFilter,
    TimestampRequest, Timestamper, TrustAnchors, Unchecked, Verdict, Weakness, WriteMode,
    WriteOptions,
};
use tinker_pdf_crypto::bignum::{Modulus, Uint};
use tinker_pdf_crypto::{DigestAlgorithm as CryptoDigest, PssParameters};
use tinker_pdf_pki::{
    oid, pss, Certificate, ContentInfo, GeneralName, SignatureAlgorithm, TimeStampToken,
};

const PSS_PDF: &[u8] = include_bytes!("signature_support/rsa-pss.pdf");
const PSS_ROOT: &[u8] = include_bytes!("signature_support/rsa-pss-root.der");
const NO_ATTRS_PDF: &[u8] = include_bytes!("signature_support/no-signed-attributes.pdf");
const NO_ATTRS_ROOT: &[u8] = include_bytes!("signature_support/no-signed-attributes-root.der");
const SHA1_PDF: &[u8] = include_bytes!("signature_support/pkcs7-sha1.pdf");
const SHA1_ROOT: &[u8] = include_bytes!("signature_support/pkcs7-sha1-root.der");
const SHA1_BARE_PDF: &[u8] = include_bytes!("signature_support/pkcs7-sha1-no-attributes.pdf");
const SHA1_BARE_ROOT: &[u8] = include_bytes!("signature_support/pkcs7-sha1-no-attributes-root.der");
const NAMES_PDF: &[u8] = include_bytes!("signature_support/cades-general-names.pdf");
const NAMES_ROOT: &[u8] = include_bytes!("signature_support/cades-general-names-root.der");
const STAMPED_PDF: &[u8] = include_bytes!("signature_support/signature-timestamp.pdf");
const STAMPED_ROOT: &[u8] = include_bytes!("signature_support/signature-timestamp-root.der");
const STAMPED_TSA_ROOT: &[u8] =
    include_bytes!("signature_support/signature-timestamp-tsa-root.der");

/// The fixture token's `genTime` as its own `GeneralizedTime` spells it,
/// `20261002094730Z` (`the_token_carries_what_its_authority_was_told_to_write`
/// finds the digits in the token).
const STAMPED_AT: i64 = unix_time(2026, 10, 2, 9, 47, 30);

/// Inside every fixture certificate's validity window and nothing to do with
/// now: 1 January 2027. Ruling 4 keeps the clock out of the engine.
const AT: i64 = 1_798_761_600;

/// Seconds since 1970 for a UTC calendar time: Howard Hinnant's
/// `days_from_civil`, proleptic Gregorian, written out here so that an
/// expected `genTime` is arithmetic on the token's own digits and not a
/// program's printout of them (ruling 13).
const fn unix_time(year: i64, month: i64, day: i64, hour: i64, minute: i64, second: i64) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let doy = (153 * ((month + 9) % 12) + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    (era * 146_097 + doe - 719_468) * 86_400 + hour * 3_600 + minute * 60 + second
}

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

// ---- no signed attributes --------------------------------------------------

/// OpenSSL's `-noattr`: a detached signer with no `signedAttrs`, so RFC 5652
/// §5.4's signature is over the digest of the content itself.
#[test]
fn the_unattributed_fixture_is_the_shape_it_claims_to_be() {
    let der = cms_of(NO_ATTRS_PDF);
    let content = ContentInfo::parse(&der).expect("the CMS parses");
    let signed = content.signed_data();
    assert!(signed.encap_content_info().is_detached());
    let signer = signed.signer_infos().first().expect("one signer");
    assert!(signer.signed_attrs().is_none());
    assert!(signer.message_digest().is_none());
    assert_eq!(
        signer.signature_algorithm(),
        Ok(SignatureAlgorithm::RsaPkcs1v15 { digest: None }),
        "bare rsaEncryption, so the digest is the `digestAlgorithm` field's"
    );
}

#[test]
fn a_signature_with_no_signed_attributes_is_over_the_covered_bytes_and_verifies() {
    let verdict = verdict_for(NO_ATTRS_PDF, NO_ATTRS_ROOT);
    assert_eq!(verdict.coverage, Coverage::WholeFile);
    assert_eq!(verdict.cms, CmsState::Read { signers: 1 });
    assert_eq!(
        verdict.signature,
        SignatureCheck::Verified,
        "the signature is over SHA-256 of the covered bytes, and nothing else"
    );
    assert_eq!(
        verdict.document_digest,
        DocumentDigest::Matches,
        "a signature over the document's digest that verifies answers question 2 too"
    );
    assert!(matches!(verdict.chain, Chain::AnchoredTo { links: 0, .. }));
    assert!(verdict.weaknesses.is_empty(), "{:?}", verdict.weaknesses);
    assert!(verdict.is_trusted());
}

#[test]
fn a_changed_document_fails_an_unattributed_signature_and_leaves_the_digest_unanswered() {
    // With no `messageDigest` the two questions cannot be told apart: the
    // signature is over the digest of the bytes that changed. So the
    // signature fails — it is not over these bytes — and the digest is not
    // given the signature's answer, because "the document changed" and "the
    // signature was never this document's" are indistinguishable here.
    let mut tampered = NO_ATTRS_PDF.to_vec();
    let at = find(&tampered, b"0.2 0.6 0.3 rg").expect("the content stream");
    tampered[at + 2] = b'9';
    let verdict = verdict_for(&tampered, NO_ATTRS_ROOT);
    assert_eq!(verdict.signature, SignatureCheck::Failed);
    assert_eq!(
        verdict.document_digest,
        DocumentDigest::NotChecked(Unchecked::NoSignedAttributes)
    );
    assert!(!verdict.is_trusted());
}

#[test]
fn a_flipped_bit_in_an_unattributed_signature_fails_rather_than_going_unchecked() {
    let mut der = cms_of(NO_ATTRS_PDF);
    let at = signature_value_at(&der);
    der[at.end - 1] ^= 0x01;
    let verdict = verdict_for(&replace_cms(NO_ATTRS_PDF, &der), NO_ATTRS_ROOT);
    assert_eq!(verdict.signature, SignatureCheck::Failed);
    assert_eq!(
        verdict.document_digest,
        DocumentDigest::NotChecked(Unchecked::NoSignedAttributes)
    );
}

#[test]
fn an_unattributed_signature_is_read_under_the_digest_its_signer_names() {
    // `digestAlgorithm` is outside anything signed, so naming SHA-384 there
    // moves no covered byte and changes which digest the verifier must take.
    // A verifier that assumed SHA-256, or took the digest from anywhere but
    // the signer, would still say `Verified`.
    let mut der = cms_of(NO_ATTRS_PDF);
    let content = ContentInfo::parse(&der).expect("the CMS parses");
    let signer = content
        .signed_data()
        .signer_infos()
        .first()
        .expect("one signer");
    let range = offset_in(&der, signer.digest_algorithm_id().der());
    let oid_at = range.start
        + find(
            &der[range.clone()],
            &[
                0x06, 0x09, 0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x02, 0x01,
            ],
        )
        .expect("id-sha256");
    drop(content);
    der[oid_at + 10] = 0x02; // id-sha384
    let verdict = verdict_for(&replace_cms(NO_ATTRS_PDF, &der), NO_ATTRS_ROOT);
    assert_eq!(verdict.signature, SignatureCheck::Failed);
}

// ---- adbe.pkcs7.sha1 ---------------------------------------------------------

/// 12.8.3.3.1's shape, as OpenSSL wrote it: the `eContent` is the SHA-1 of the
/// covered bytes, and the signer digests those twenty octets with SHA-256.
#[test]
fn the_sha1_fixtures_are_the_shape_they_claim_to_be() {
    for (pdf, attributes) in [(SHA1_PDF, true), (SHA1_BARE_PDF, false)] {
        let document = Document::open(pdf.to_vec()).expect("opens");
        let signature = &document.signatures()[0];
        assert_eq!(signature.sub_filter, Some(SubFilter::Pkcs7Sha1));
        let digest = signature
            .digest(&document, DigestAlgorithm::Sha1)
            .expect("the spans fit");

        let der = cms_of(pdf);
        let content = ContentInfo::parse(&der).expect("the CMS parses");
        let signed = content.signed_data();
        assert_eq!(
            signed.encap_content_info().content(),
            Some(digest.as_slice()),
            "the encapsulated content is the covered bytes' SHA-1"
        );
        let signer = signed.signer_infos().first().expect("one signer");
        assert_eq!(signer.signed_attrs().is_some(), attributes);
        assert_eq!(
            signer.digest_algorithm(),
            Ok(tinker_pdf_pki::DigestAlgorithm::Sha256),
            "the signer's own digest is not the document's"
        );
    }
}

#[test]
fn an_adbe_pkcs7_sha1_signature_reaches_every_one_of_the_four_answers() {
    for (pdf, root) in [(SHA1_PDF, SHA1_ROOT), (SHA1_BARE_PDF, SHA1_BARE_ROOT)] {
        let verdict = verdict_for(pdf, root);
        assert_eq!(verdict.coverage, Coverage::WholeFile);
        assert_eq!(verdict.document_digest, DocumentDigest::Matches);
        assert_eq!(verdict.signature, SignatureCheck::Verified);
        assert!(matches!(verdict.chain, Chain::AnchoredTo { links: 0, .. }));
        assert_eq!(
            verdict.weaknesses,
            [Weakness::Sha1Digest],
            "the subfilter fixes the document digest at SHA-1, and says so"
        );
        assert!(verdict.is_trusted());
    }
}

#[test]
fn a_changed_document_differs_and_its_sha1_signature_still_verifies() {
    // The two questions come apart here exactly as they do for a detached
    // signature: the signature is over the encapsulated digest, which did not
    // change, and the document is not what that digest describes.
    for (pdf, root) in [(SHA1_PDF, SHA1_ROOT), (SHA1_BARE_PDF, SHA1_BARE_ROOT)] {
        let verdict = verdict_for(&tampered(pdf), root);
        assert_eq!(verdict.document_digest, DocumentDigest::Differs);
        assert_eq!(verdict.signature, SignatureCheck::Verified);
        assert!(!verdict.is_trusted());
    }
}

#[test]
fn a_document_and_its_encapsulated_digest_replaced_together_are_caught() {
    // The attack the second link exists for. Change the document, then write
    // its new SHA-1 into the `eContent`: the first link — the twenty octets
    // are the covered bytes' digest — holds again. With signed attributes the
    // `messageDigest` still names the old content, so the document digest is
    // `Differs` while the signature over the attributes still verifies; with
    // none the signature is over the twenty octets, so it fails. Either way
    // nothing reports a forged document as signed — and a reader that checked
    // only the first link would have reported this one `Matches` and
    // `Verified`.
    for (pdf, root, attributes) in [
        (SHA1_PDF, SHA1_ROOT, true),
        (SHA1_BARE_PDF, SHA1_BARE_ROOT, false),
    ] {
        let changed = tampered(pdf);
        let document = Document::open(changed.clone()).expect("opens");
        let digest = document.signatures()[0]
            .digest(&document, DigestAlgorithm::Sha1)
            .expect("the spans fit");

        let mut der = cms_of(pdf);
        let at = {
            let content = ContentInfo::parse(&der).expect("the CMS parses");
            let econtent = content
                .signed_data()
                .encap_content_info()
                .content()
                .expect("it encapsulates");
            offset_in(&der, econtent)
        };
        der[at].copy_from_slice(&digest);
        let verdict = verdict_for(&replace_cms(&changed, &der), root);
        if attributes {
            assert_eq!(verdict.document_digest, DocumentDigest::Differs);
            assert_eq!(verdict.signature, SignatureCheck::Verified);
        } else {
            assert_eq!(verdict.document_digest, DocumentDigest::Matches);
            assert_eq!(verdict.signature, SignatureCheck::Failed);
        }
        assert!(!verdict.is_trusted());
    }
}

#[test]
fn a_detached_message_under_the_sha1_subfilter_has_no_digest_to_compare() {
    // The no-signed-attributes fixture's blob is detached. Renaming its
    // subfilter in place — padded with spaces so no offset moves — makes a
    // document that claims `adbe.pkcs7.sha1` and carries no digest in its
    // message, which is a named refusal rather than a comparison with nothing.
    let mut pdf = NO_ATTRS_PDF.to_vec();
    let at = find(&pdf, b"/adbe.pkcs7.detached").expect("the subfilter");
    pdf[at..at + 20].copy_from_slice(b"/adbe.pkcs7.sha1    ");
    let verdict = verdict_for(&pdf, NO_ATTRS_ROOT);
    assert_eq!(
        verdict.document_digest,
        DocumentDigest::NotChecked(Unchecked::ContentNotEncapsulated)
    );
    assert_eq!(
        verdict.signature,
        SignatureCheck::Failed,
        "the renaming is inside the covered bytes, so the signature over them fails too"
    );
}

// ---- GeneralNames ------------------------------------------------------------

/// The leaf certificate of the `GeneralNames` fixture, and the root.
fn names_certificates(der: &[u8]) -> (Certificate<'_>, Certificate<'_>) {
    let mut certificates = ContentInfo::parse(der)
        .expect("the CMS parses")
        .signed_data()
        .x509_certificates()
        .map(|der| Certificate::parse(der).expect("each certificate parses"))
        .collect::<Vec<_>>();
    assert_eq!(certificates.len(), 2);
    // A `CertificateSet` is a SET OF, so DER sorts it by encoding and the
    // order says nothing about which is which; the root is the self-issued
    // one.
    certificates.sort_by_key(Certificate::is_self_issued);
    let root = certificates.pop().expect("two");
    let leaf = certificates.pop().expect("two");
    assert!(root.is_self_issued() && !leaf.is_self_issued());
    (leaf, root)
}

/// Eight of the nine alternatives, as OpenSSL 3.0.13 wrote them into the
/// signer's certificate from `signature-fixtures.py`'s extension file — so
/// what is asserted here is what OpenSSL was asked for, read back by this
/// crate rather than by OpenSSL.
#[test]
fn a_certificates_alternative_names_read_as_openssl_wrote_them() {
    let der = cms_of(NAMES_PDF);
    let (leaf, root) = names_certificates(&der);
    let names = leaf
        .extensions()
        .subject_alt_names()
        .expect("subjectAltName is present")
        .expect("and decodes");
    let rendered: Vec<String> = names.names().iter().map(ToString::to_string).collect();
    assert_eq!(
        rendered,
        [
            "email:signer@example.com",
            "DNS:signer.example.com",
            "URI:https://example.com/signer",
            "IP:192.0.2.7",
            "IP:2001:db8:0:0:0:0:0:7",
            "RID:1.2.3.4",
            "othername:1.3.6.1.4.1.311.20.2.3",
            "DirName:O=tinker-pdf test fixture,CN=Tinker PDF Directory Name",
        ]
    );
    match &names.names()[6] {
        GeneralName::Other { value, .. } => {
            // `UTF8:signer@example.com`: a UTF8String, carried whole.
            assert_eq!(value.first(), Some(&0x0C));
            assert_eq!(&value[2..], b"signer@example.com");
        }
        other => panic!("expected the otherName, got {other:?}"),
    }

    let issuer_names = leaf
        .extensions()
        .issuer_alt_names()
        .expect("issuerAltName is present")
        .expect("and decodes");
    assert_eq!(
        issuer_names.names(),
        [GeneralName::Uri("https://example.com/root".into())]
    );

    // `authorityKeyIdentifier = keyid:always, issuer:always`: the issuer's own
    // issuer and serial, which for a self-signed root are its subject and its
    // serial.
    let authority = leaf
        .extensions()
        .authority_key_identifier()
        .expect("authorityKeyIdentifier is present");
    let issuer = authority
        .issuer()
        .expect("authorityCertIssuer is present")
        .expect("and decodes");
    assert!(issuer
        .directory_name()
        .expect("a directory name")
        .matches(root.issuer()));
    assert_eq!(
        authority.serial().map(|serial| serial.as_bytes().to_vec()),
        Some(root.serial().as_bytes().to_vec())
    );
}

/// RFC 5035's `issuerSerial`, as OpenSSL's `-cades` writes it: a directory
/// name inside `[4]`, which is explicit because `Name` is a CHOICE.
#[test]
fn an_ess_issuer_serial_names_the_signers_certificate_and_no_other() {
    let der = cms_of(NAMES_PDF);
    let (leaf, root) = names_certificates(&der);
    let content = ContentInfo::parse(&der).expect("the CMS parses");
    let signer = content
        .signed_data()
        .signer_infos()
        .first()
        .expect("one signer")
        .clone();
    let ess = signer
        .signing_certificate_v2()
        .expect("-cades writes signingCertificateV2");
    let first = ess.certs().first().expect("one ESSCertIDv2");
    let issuer_serial = first
        .issuer_serial_decoded()
        .expect("OpenSSL writes issuerSerial")
        .expect("and it decodes");
    assert_eq!(issuer_serial.issuer().names().len(), 1);
    assert!(issuer_serial.identifies(&leaf));
    assert!(
        !issuer_serial.identifies(&root),
        "the root has the same issuer name and a different serial"
    );
}

#[test]
fn the_cades_fixture_verifies_like_any_other() {
    let verdict = verdict_for(NAMES_PDF, NAMES_ROOT);
    assert_eq!(verdict.document_digest, DocumentDigest::Matches);
    assert_eq!(verdict.signature, SignatureCheck::Verified);
    assert!(verdict.is_trusted());
}

// ---- RFC 3161 signature timestamps ------------------------------------------

/// The token, located inside the outer blob.
fn token_at(der: &[u8]) -> Range<usize> {
    let content = ContentInfo::parse(der).expect("the CMS parses");
    let signer = content
        .signed_data()
        .signer_infos()
        .first()
        .expect("one signer")
        .clone();
    let token = *signer.timestamp_tokens().first().expect("one token");
    offset_in(der, token)
}

#[test]
fn a_signature_timestamp_is_validated_against_its_authority() {
    let verdict = verdict_with(STAMPED_PDF, &[STAMPED_ROOT, STAMPED_TSA_ROOT]);
    assert!(verdict.is_trusted(), "the signature itself: {verdict:?}");
    assert!(verdict.signer.as_ref().is_some_and(|s| s.timestamped));
    assert_eq!(verdict.timestamps.len(), 1);
    let stamp = &verdict.timestamps[0];
    assert_eq!(stamp.stamps, Stamped::Signature);
    assert_eq!(stamp.token, CmsState::Read { signers: 1 });
    assert_eq!(stamp.time, Some(STAMPED_AT));
    assert_eq!(
        stamp.authority.as_deref(),
        Some("DirName:O=tinker-pdf test fixture,CN=Tinker PDF Timestamping Authority"),
        "the TSTInfo's own `tsa` hint, as OpenSSL's `tsa_name = yes` wrote it"
    );
    assert_eq!(
        stamp.imprint,
        DocumentDigest::Matches,
        "SHA-256 of the signer's signature octets"
    );
    assert_eq!(stamp.signature, SignatureCheck::Verified);
    assert_eq!(
        stamp.authority_certificate,
        AuthorityCertificate::Fit,
        "a critical timeStamping-only EKU, named by RFC 2634's first-version ESS attribute"
    );
    match &stamp.chain {
        Chain::AnchoredTo { anchor, links } => {
            assert!(anchor.contains("Timestamp Root"), "{anchor}");
            assert_eq!(*links, 0);
        }
        other => panic!("expected the authority's chain to reach its root, got {other:?}"),
    }
    assert!(stamp.weaknesses.is_empty(), "{:?}", stamp.weaknesses);
    assert!(stamp.is_trusted());
}

#[test]
fn an_authority_the_caller_does_not_trust_is_not_anchored() {
    let verdict = verdict_with(STAMPED_PDF, &[STAMPED_ROOT]);
    let stamp = &verdict.timestamps[0];
    assert_eq!(stamp.signature, SignatureCheck::Verified);
    assert!(
        matches!(stamp.chain, Chain::SelfSigned { .. }),
        "the token carries the authority's root, which is not an anchor: {:?}",
        stamp.chain
    );
    assert!(!stamp.is_trusted());
    assert!(
        verdict.is_trusted(),
        "and the signature it stamps is untouched"
    );
}

#[test]
fn a_token_over_a_different_signature_does_not_match_its_imprint() {
    // The signer's signature octets changed, the token did not: the token
    // still verifies, and it is a timestamp of some other signature.
    let mut der = cms_of(STAMPED_PDF);
    let at = signature_value_at(&der);
    der[at.end - 1] ^= 0x01;
    let verdict = verdict_with(
        &replace_cms(STAMPED_PDF, &der),
        &[STAMPED_ROOT, STAMPED_TSA_ROOT],
    );
    assert_eq!(verdict.signature, SignatureCheck::Failed);
    let stamp = &verdict.timestamps[0];
    assert_eq!(stamp.imprint, DocumentDigest::Differs);
    assert_eq!(stamp.signature, SignatureCheck::Verified);
    assert!(!stamp.is_trusted());
}

#[test]
fn a_flipped_bit_in_the_tokens_signature_fails_it() {
    let mut der = cms_of(STAMPED_PDF);
    let token = token_at(&der);
    let signature = {
        let parsed = TimeStampToken::parse(&der[token.clone()]).expect("the token parses");
        let signer = parsed
            .content_info()
            .signed_data()
            .signer_infos()
            .first()
            .expect("one signer")
            .clone();
        let inner = offset_in(&der[token.clone()], signer.signature());
        token.start + inner.start..token.start + inner.end
    };
    der[signature.end - 1] ^= 0x01;
    let verdict = verdict_with(
        &replace_cms(STAMPED_PDF, &der),
        &[STAMPED_ROOT, STAMPED_TSA_ROOT],
    );
    let stamp = &verdict.timestamps[0];
    assert_eq!(stamp.imprint, DocumentDigest::Matches);
    assert_eq!(stamp.signature, SignatureCheck::Failed);
    assert!(
        verdict.is_trusted(),
        "an unsigned attribute is outside the signature"
    );
}

#[test]
fn a_changed_tstinfo_is_not_what_the_authority_signed() {
    // One digit of `genTime` moved. The signed attributes are untouched, so
    // the arithmetic over them still verifies — and their `messageDigest` is
    // no longer this `TSTInfo`'s, which is the one check that catches a time
    // rewritten after stamping.
    let mut der = cms_of(STAMPED_PDF);
    let token = token_at(&der);
    let at = token.start
        + find(&der[token.clone()], b"20261002094730Z").expect("genTime, as OpenSSL wrote it");
    der[at + 3] = b'7';
    let verdict = verdict_with(
        &replace_cms(STAMPED_PDF, &der),
        &[STAMPED_ROOT, STAMPED_TSA_ROOT],
    );
    let stamp = &verdict.timestamps[0];
    assert_ne!(
        stamp.time,
        Some(STAMPED_AT),
        "the time read is the changed one"
    );
    assert_eq!(stamp.signature, SignatureCheck::Failed);
    assert!(!stamp.is_trusted());
}

#[test]
fn an_authority_certificate_without_a_critical_timestamping_purpose_is_not_fit() {
    // The authority's certificate inside the token, its extended key usage
    // made non-critical. Its own signature no longer covers it, so the
    // chain breaks too; what is asserted is the first requirement RFC 3161
    // §2.3 puts on it, which a reader that skipped it would have passed to
    // the ESS check and called `NotBound`.
    let mut der = cms_of(STAMPED_PDF);
    let token = token_at(&der);
    let eku = [0x06, 0x03, 0x55, 0x1D, 0x25, 0x01, 0x01, 0xFF];
    let at = token.start + find(&der[token.clone()], &eku).expect("a critical EKU");
    der[at + 7] = 0x00;
    let verdict = verdict_with(
        &replace_cms(STAMPED_PDF, &der),
        &[STAMPED_ROOT, STAMPED_TSA_ROOT],
    );
    let stamp = &verdict.timestamps[0];
    assert_eq!(stamp.signature, SignatureCheck::Verified);
    assert_eq!(
        stamp.authority_certificate,
        AuthorityCertificate::NotForTimestamping
    );
    assert!(!stamp.is_trusted());
}

#[test]
fn a_token_whose_ess_attribute_names_another_certificate_is_not_bound() {
    // The first `ESSCertID`'s hash, one bit changed. That is inside the
    // signed attributes, so the token's signature fails as well; the binding
    // is asked separately, and a reader that never looked at it would still
    // call the certificate fit.
    let mut der = cms_of(STAMPED_PDF);
    let token = token_at(&der);
    let hash = {
        let parsed = TimeStampToken::parse(&der[token.clone()]).expect("the token parses");
        let signer = parsed
            .content_info()
            .signed_data()
            .signer_infos()
            .first()
            .expect("one signer")
            .clone();
        let ess = signer
            .signing_certificate()
            .expect("OpenSSL's default ESS attribute");
        let id = ess.certs().first().expect("one ESSCertID");
        assert_eq!(id.digest(), Ok(tinker_pdf_pki::DigestAlgorithm::Sha1));
        let inner = offset_in(&der[token.clone()], id.hash());
        token.start + inner.start..token.start + inner.end
    };
    der[hash.start] ^= 0x01;
    let verdict = verdict_with(
        &replace_cms(STAMPED_PDF, &der),
        &[STAMPED_ROOT, STAMPED_TSA_ROOT],
    );
    let stamp = &verdict.timestamps[0];
    assert_eq!(stamp.authority_certificate, AuthorityCertificate::NotBound);
    assert_eq!(stamp.signature, SignatureCheck::Failed);
}

/// The token read back to what its authority was told to write, which is the
/// generator's input rather than any program's reading of the output (ruling
/// 13). `tsa()` in `signature-fixtures.py` configured OpenSSL's TSA with
/// `default_policy = 1.3.6.1.4.1.55555.1.1`, `accuracy = secs:1,
/// millisecs:500, microsecs:100`, `ordering = yes`, and a serial file holding
/// `2026100201` — the *last* serial issued, which `openssl ts -reply`
/// increments before it issues the next, as its manual says. The query asked
/// for a SHA-256 imprint and, by default, a nonce. The one field nobody
/// configured is `genTime`: the token spells it `20261002094730Z`, and
/// [`STAMPED_AT`] is those digits through the calendar arithmetic above.
#[test]
fn the_token_carries_what_its_authority_was_told_to_write() {
    let der = cms_of(STAMPED_PDF);
    let token = token_at(&der);
    assert!(
        find(&der[token.clone()], b"\x18\x0f20261002094730Z").is_some(),
        "the GeneralizedTime STAMPED_AT is computed from"
    );
    let parsed = TimeStampToken::parse(&der[token]).expect("the token parses");
    let info = parsed.info();
    assert_eq!(info.policy().to_dotted(), "1.3.6.1.4.1.55555.1.1");
    assert_eq!(
        info.imprint_digest(),
        Ok(tinker_pdf_pki::DigestAlgorithm::Sha256)
    );
    assert_eq!(info.serial().as_bytes(), &[0x20, 0x26, 0x10, 0x02, 0x02]);
    assert_eq!(info.time(), STAMPED_AT);
    assert_eq!(
        info.accuracy(),
        Some(tinker_pdf_pki::Accuracy {
            seconds: 1,
            millis: 500,
            micros: 100
        })
    );
    assert!(info.ordering());
    assert!(info.nonce().is_some());
}

// ---- shapes no outside tool will sign, assembled here ----------------------
//
// OpenSSL refuses to make some shapes a reader still has to judge: a PKCS#1
// v1.5 signature under a key restricted to RSASSA-PSS, signed attributes with
// no `messageDigest`, an `adbe.pkcs7.sha1` message left detached. These are
// assembled below and signed with the committed throwaway key
// `visible-signer-key.der` through `tinker_pdf_crypto::bignum`, the
// arrangement `visible_signature.rs` signs with, and `save_signed` lays them
// out. So each is this engine agreeing with its own reading of RFC 5652 —
// which is what these tests ask about, since each is a refusal — and every
// refusal has a control beside it, signed the same way, that verifies.

const SIGNER_KEY: &[u8] = include_bytes!("signature_support/visible-signer-key.der");
const SIGNER: &[u8] = include_bytes!("signature_support/visible-signer.der");
const UNSIGNED: &[u8] = include_bytes!("../../../testdata/simple-text.pdf");

const OID_DATA: &[u8] = &[
    0x06, 0x09, 0x2A, 0x86, 0x48, 0x86, 0xF7, 0x0D, 0x01, 0x07, 0x01,
];
const OID_SIGNED_DATA: &[u8] = &[
    0x06, 0x09, 0x2A, 0x86, 0x48, 0x86, 0xF7, 0x0D, 0x01, 0x07, 0x02,
];
/// `id-ct-TSTInfo`, `1.2.840.113549.1.9.16.1.4`.
const OID_TST_INFO: &[u8] = &[
    0x06, 0x0B, 0x2A, 0x86, 0x48, 0x86, 0xF7, 0x0D, 0x01, 0x09, 0x10, 0x01, 0x04,
];
const OID_CONTENT_TYPE: &[u8] = &[
    0x06, 0x09, 0x2A, 0x86, 0x48, 0x86, 0xF7, 0x0D, 0x01, 0x09, 0x03,
];
const OID_MESSAGE_DIGEST: &[u8] = &[
    0x06, 0x09, 0x2A, 0x86, 0x48, 0x86, 0xF7, 0x0D, 0x01, 0x09, 0x04,
];
const SHA256_ALGORITHM: &[u8] = &[
    0x30, 0x0D, 0x06, 0x09, 0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x02, 0x01, 0x05, 0x00,
];
const RSA_ENCRYPTION_ALGORITHM: &[u8] = &[
    0x30, 0x0D, 0x06, 0x09, 0x2A, 0x86, 0x48, 0x86, 0xF7, 0x0D, 0x01, 0x01, 0x01, 0x05, 0x00,
];
/// `id-RSASSA-PSS` with its parameters absent: RFC 4055 §1.2's key
/// restricted to RSASSA-PSS and to nothing narrower.
const RSASSA_PSS_ALGORITHM: &[u8] = &[
    0x30, 0x0B, 0x06, 0x09, 0x2A, 0x86, 0x48, 0x86, 0xF7, 0x0D, 0x01, 0x01, 0x0A,
];

/// One DER element at the start of `bytes`: its whole encoding and its
/// contents. Test code over bytes this file built or this repository
/// committed, so an index out of range is a statement about those bytes.
fn element(bytes: &[u8]) -> (&[u8], &[u8]) {
    let first = bytes[1];
    let (length, header) = if first < 0x80 {
        (usize::from(first), 2)
    } else {
        let count = usize::from(first & 0x7F);
        let length = bytes[2..2 + count]
            .iter()
            .fold(0usize, |acc, byte| acc << 8 | usize::from(*byte));
        (length, 2 + count)
    };
    (&bytes[..header + length], &bytes[header..header + length])
}

/// The elements inside a constructed element's contents.
fn children(mut contents: &[u8]) -> Vec<&[u8]> {
    let mut out = Vec::new();
    while !contents.is_empty() {
        let (whole, _) = element(contents);
        out.push(whole);
        contents = &contents[whole.len()..];
    }
    out
}

/// A DER element with `tag` around `contents`.
fn tlv(tag: u8, contents: &[u8]) -> Vec<u8> {
    let mut out = vec![tag];
    let length = contents.len();
    if length < 0x80 {
        out.push(length as u8);
    } else {
        let octets: Vec<u8> = length
            .to_be_bytes()
            .into_iter()
            .skip_while(|byte| *byte == 0)
            .collect();
        out.push(0x80 | octets.len() as u8);
        out.extend_from_slice(&octets);
    }
    out.extend_from_slice(contents);
    out
}

fn sha256(bytes: &[u8]) -> Vec<u8> {
    CryptoDigest::Sha256.digest(bytes).as_bytes().to_vec()
}

/// RSASSA-PKCS1-v1_5 with SHA-256 (RFC 8017 §8.2.1, §9.2) over `digest`, with
/// the committed key's private exponent.
fn rsa_sign(digest: &[u8]) -> Vec<u8> {
    // RFC 8017 A.1.2: version, n, e, d, ...
    let (_, key) = element(SIGNER_KEY);
    let fields = children(key);
    let modulus = Modulus::<32>::new(Uint::from_be_bytes(element(fields[1]).1).expect("n fits"))
        .expect("an odd modulus");
    let exponent = Uint::<32>::from_be_bytes(element(fields[3]).1).expect("d fits");
    let info = tlv(0x30, &[SHA256_ALGORITHM, &tlv(0x04, digest)].concat());
    let length = modulus.byte_len();
    let mut block = vec![0x00, 0x01];
    block.resize(length - info.len() - 1, 0xFF);
    block.push(0x00);
    block.extend_from_slice(&info);
    let signature = modulus.pow(&Uint::from_be_bytes(&block).expect("fits"), &exponent);
    let mut out = vec![0; length];
    assert!(signature.to_be_bytes(&mut out));
    out
}

/// The contents of a `SET OF Attribute`: `contentType`, and `messageDigest`
/// where there is one to give. DER's order is by encoding, and the shorter
/// `contentType` sorts first.
fn attributes(content_type: &[u8], message_digest: Option<&[u8]>) -> Vec<u8> {
    let mut out = tlv(0x30, &[OID_CONTENT_TYPE, &tlv(0x31, content_type)].concat());
    if let Some(digest) = message_digest {
        out.extend(tlv(
            0x30,
            &[OID_MESSAGE_DIGEST, &tlv(0x31, &tlv(0x04, digest))].concat(),
        ));
    }
    out
}

/// A `ContentInfo` around a `SignedData` (RFC 5652 §5) with one SHA-256
/// signer named by `certificate`'s issuer and serial, `certificate` the one
/// certificate carried, and `content` encapsulated where it is given.
fn signed_data(
    certificate: &[u8],
    content_type: &[u8],
    content: Option<&[u8]>,
    signed_attributes: Option<&[u8]>,
    signature: &[u8],
) -> Vec<u8> {
    let (_, whole) = element(certificate);
    let (_, tbs) = element(children(whole)[0]);
    let tbs = children(tbs);
    let (serial, issuer) = (tbs[1], tbs[3]);

    let mut signer = vec![
        tlv(0x02, &[1]),
        tlv(0x30, &[issuer, serial].concat()),
        SHA256_ALGORITHM.to_vec(),
    ];
    if let Some(attributes) = signed_attributes {
        signer.push(tlv(0xA0, attributes));
    }
    signer.push(RSA_ENCRYPTION_ALGORITHM.to_vec());
    signer.push(tlv(0x04, signature));

    let mut encapsulated = content_type.to_vec();
    if let Some(content) = content {
        encapsulated.extend(tlv(0xA0, &tlv(0x04, content)));
    }
    // §5.1: version 3 for any `eContentType` other than `id-data`.
    let version = if content_type == OID_DATA { 1 } else { 3 };
    let signed = tlv(
        0x30,
        &[
            tlv(0x02, &[version]),
            tlv(0x31, SHA256_ALGORITHM),
            tlv(0x30, &encapsulated),
            tlv(0xA0, certificate),
            tlv(0x31, &tlv(0x30, &signer.concat())),
        ]
        .concat(),
    );
    tlv(0x30, &[OID_SIGNED_DATA, &tlv(0xA0, &signed)].concat())
}

/// `der` with its key published under `id-RSASSA-PSS` where it said
/// `rsaEncryption`: the same `RSAPublicKey`, its owner having restricted it
/// to RSASSA-PSS (RFC 4055 §1.2). The certificate's own signature no longer
/// covers the result.
fn restricted_to_pss(der: &[u8]) -> Vec<u8> {
    let (_, certificate) = element(der);
    let parts = children(certificate);
    let (_, tbs) = element(parts[0]);
    let mut fields: Vec<Vec<u8>> = children(tbs).iter().map(|field| field.to_vec()).collect();
    // RFC 5280 §4.1: [0] version, serial, signature, issuer, validity,
    // subject, subjectPublicKeyInfo, ...
    assert_eq!(fields[0][0], 0xA0, "a version 3 certificate");
    let (_, spki) = element(&fields[6]);
    let spki = children(spki);
    assert_eq!(
        spki[0], RSA_ENCRYPTION_ALGORITHM,
        "the key was unrestricted"
    );
    fields[6] = tlv(0x30, &[RSASSA_PSS_ALGORITHM, spki[1]].concat());
    let tbs = tlv(0x30, &fields.concat());
    tlv(0x30, &[tbs.as_slice(), parts[1], parts[2]].concat())
}

/// What the assembled signer puts in its `SignerInfo`.
#[derive(Clone, Copy)]
enum Shape {
    /// Detached, with `contentType` and `messageDigest`: the ordinary shape,
    /// for the controls.
    Ordinary,
    /// Detached, with signed attributes and no `messageDigest` among them.
    NoMessageDigest,
    /// The covered bytes' SHA-1 encapsulated, as `adbe.pkcs7.sha1` asks, and
    /// signed attributes with no `messageDigest` naming it.
    EncapsulatedWithNoMessageDigest,
    /// Detached, with no signed attributes: the signature is over the
    /// covered bytes' own SHA-256 (RFC 5652 §5.4).
    Unattributed,
}

struct Assembled {
    shape: Shape,
    certificate: Vec<u8>,
}

impl Signer for Assembled {
    fn digest_algorithm(&self) -> DigestAlgorithm {
        match self.shape {
            // 12.8.3.3.1: the encapsulated digest is the covered bytes' SHA-1.
            Shape::EncapsulatedWithNoMessageDigest => DigestAlgorithm::Sha1,
            _ => DigestAlgorithm::Sha256,
        }
    }

    fn sign(&self, digest: &[u8]) -> Result<Vec<u8>, SignRefused> {
        let over_attributes = |attributes: &[u8]| rsa_sign(&sha256(&tlv(0x31, attributes)));
        let certificate = &self.certificate;
        Ok(match self.shape {
            Shape::Ordinary => {
                let attributes = attributes(OID_DATA, Some(digest));
                let signature = over_attributes(&attributes);
                signed_data(certificate, OID_DATA, None, Some(&attributes), &signature)
            }
            Shape::NoMessageDigest => {
                let attributes = attributes(OID_DATA, None);
                let signature = over_attributes(&attributes);
                signed_data(certificate, OID_DATA, None, Some(&attributes), &signature)
            }
            Shape::EncapsulatedWithNoMessageDigest => {
                let attributes = attributes(OID_DATA, None);
                let signature = over_attributes(&attributes);
                signed_data(
                    certificate,
                    OID_DATA,
                    Some(digest),
                    Some(&attributes),
                    &signature,
                )
            }
            Shape::Unattributed => {
                signed_data(certificate, OID_DATA, None, None, &rsa_sign(digest))
            }
        })
    }
}

fn incremental() -> WriteOptions {
    WriteOptions {
        mode: WriteMode::Incremental,
        ..WriteOptions::default()
    }
}

/// `testdata/simple-text.pdf`, signed under `sub_filter` by a signer of
/// `shape` that names `certificate`.
fn assembled(shape: Shape, certificate: &[u8], sub_filter: &str) -> Vec<u8> {
    let signer = Assembled {
        shape,
        certificate: certificate.to_vec(),
    };
    let mut request = SigningRequest::new(
        SigningTarget::NewInvisibleField {
            name: "Assembled".into(),
        },
        &signer,
    );
    request.sub_filter = sub_filter.into();
    request.reserve = 8192;
    Document::open(UNSIGNED.to_vec())
        .expect("the fixture opens")
        .editor()
        .save_signed(&incremental(), &request)
        .expect("the signature is written")
}

#[test]
fn the_assembled_signer_verifies_when_its_shape_is_an_ordinary_one() {
    let verdict = verdict_for(
        &assembled(Shape::Ordinary, SIGNER, "adbe.pkcs7.detached"),
        SIGNER,
    );
    assert_eq!(verdict.document_digest, DocumentDigest::Matches);
    assert_eq!(verdict.signature, SignatureCheck::Verified);
    assert!(
        matches!(verdict.chain, Chain::AnchoredTo { .. }),
        "{:?}",
        verdict.chain
    );
    assert!(verdict.is_trusted(), "{verdict:?}");

    let verdict = verdict_for(
        &assembled(Shape::Unattributed, SIGNER, "adbe.pkcs7.detached"),
        SIGNER,
    );
    assert_eq!(verdict.document_digest, DocumentDigest::Matches);
    assert_eq!(verdict.signature, SignatureCheck::Verified);
}

/// RFC 4055 §1.2: a key published under `id-RSASSA-PSS` is for RSASSA-PSS
/// and nothing else. The PKCS#1 v1.5 arithmetic cannot tell — the
/// `RSAPublicKey` is the same either way, and the signature below is a
/// perfectly good one under it — so only the OID can, and the signature is
/// `Failed`, as RFC 4056 §3 has a restriction the parameters break answered.
#[test]
fn a_pkcs1_v15_signature_under_a_key_restricted_to_pss_fails() {
    let restricted = restricted_to_pss(SIGNER);
    let verdict = verdict_for(
        &assembled(Shape::Ordinary, &restricted, "adbe.pkcs7.detached"),
        SIGNER,
    );
    assert_eq!(
        verdict.document_digest,
        DocumentDigest::Matches,
        "the attributes still name these bytes"
    );
    assert_eq!(verdict.signature, SignatureCheck::Failed);
    assert!(!verdict.is_trusted());
}

/// The same restriction on a chain link: the committed certificate is
/// self-signed with `sha256WithRSAEncryption`, and an anchor that is the same
/// subject and the same key, restricted to RSASSA-PSS, did not make that
/// signature.
#[test]
fn a_pkcs1_v15_link_under_an_issuer_restricted_to_pss_breaks_the_path() {
    let signed = assembled(Shape::Ordinary, SIGNER, "adbe.pkcs7.detached");
    let restricted = restricted_to_pss(SIGNER);
    let verdict = verdict_for(&signed, &restricted);
    assert!(
        matches!(verdict.chain, Chain::Broken { .. }),
        "{:?}",
        verdict.chain
    );
    assert_eq!(verdict.signature, SignatureCheck::Verified);
    assert!(!verdict.is_trusted());
}

/// RFC 5652 §5.3: where there are signed attributes there is a
/// `messageDigest`. Without one, the signature is over attributes that name
/// no content: it verifies, and it binds nothing.
#[test]
fn signed_attributes_with_no_message_digest_bind_no_document() {
    let verdict = verdict_for(
        &assembled(Shape::NoMessageDigest, SIGNER, "adbe.pkcs7.detached"),
        SIGNER,
    );
    assert_eq!(
        verdict.document_digest,
        DocumentDigest::NotChecked(Unchecked::NoMessageDigest),
        "there are signed attributes, so not `NoSignedAttributes`"
    );
    assert_eq!(
        verdict.signature,
        SignatureCheck::Verified,
        "the key did sign the attributes"
    );
    assert!(!verdict.is_trusted());
}

/// The `adbe.pkcs7.sha1` form of the same gap, which was worse: the message
/// carries the covered bytes' right SHA-1 and nothing signs it, and the
/// verdict read `Matches`, `Verified` and anchored — trusted.
#[test]
fn an_encapsulated_digest_no_message_digest_names_is_not_a_match() {
    let verdict = verdict_for(
        &assembled(
            Shape::EncapsulatedWithNoMessageDigest,
            SIGNER,
            "adbe.pkcs7.sha1",
        ),
        SIGNER,
    );
    assert_eq!(
        verdict.document_digest,
        DocumentDigest::NotChecked(Unchecked::NoMessageDigest)
    );
    assert_eq!(verdict.signature, SignatureCheck::Verified);
    assert!(!verdict.is_trusted(), "{verdict:?}");
}

/// A detached message under `adbe.pkcs7.sha1` with no signed attributes. The
/// signature is over the covered bytes' own SHA-256 and verifies; the
/// subfilter still asks for an encapsulated digest, so question 2 stays
/// unanswered rather than borrowing question 3's answer as a detached
/// signature's does.
#[test]
fn a_detached_message_under_the_sha1_subfilter_stays_unchecked_when_it_verifies() {
    let verdict = verdict_for(
        &assembled(Shape::Unattributed, SIGNER, "adbe.pkcs7.sha1"),
        SIGNER,
    );
    assert_eq!(verdict.signature, SignatureCheck::Verified);
    assert_eq!(
        verdict.document_digest,
        DocumentDigest::NotChecked(Unchecked::ContentNotEncapsulated)
    );
    assert!(!verdict.is_trusted());
}

/// A [`Timestamper`] that answers with a token it assembles over the digest
/// it is handed: a `TSTInfo`, and signed attributes with or without the
/// `messageDigest` that binds it.
struct AssembledAuthority {
    message_digest: bool,
}

impl Timestamper for AssembledAuthority {
    fn digest_algorithm(&self) -> DigestAlgorithm {
        DigestAlgorithm::Sha256
    }

    fn timestamp(&self, digest: &[u8]) -> Result<Vec<u8>, SignRefused> {
        // RFC 3161 §2.4.2: version 1, a policy, the imprint, a serial, a
        // `genTime`.
        let info = tlv(
            0x30,
            &[
                tlv(0x02, &[1]),
                tlv(
                    0x06,
                    &[0x2B, 0x06, 0x01, 0x04, 0x01, 0x83, 0xB2, 0x03, 0x01],
                ),
                tlv(0x30, &[SHA256_ALGORITHM, &tlv(0x04, digest)].concat()),
                tlv(0x02, &[0x01]),
                tlv(0x18, b"20261003120000Z"),
            ]
            .concat(),
        );
        let bound = sha256(&info);
        let attributes = attributes(
            OID_TST_INFO,
            self.message_digest.then_some(bound.as_slice()),
        );
        let signature = rsa_sign(&sha256(&tlv(0x31, &attributes)));
        Ok(signed_data(
            SIGNER,
            OID_TST_INFO,
            Some(&info),
            Some(&attributes),
            &signature,
        ))
    }
}

fn assembled_timestamp(message_digest: bool) -> Verdict {
    let authority = AssembledAuthority { message_digest };
    let mut request = TimestampRequest::new(
        SigningTarget::NewInvisibleField {
            name: "Timestamp".into(),
        },
        &authority,
    );
    request.reserve = 8192;
    let stamped = Document::open(UNSIGNED.to_vec())
        .expect("the fixture opens")
        .editor()
        .save_timestamped(&incremental(), &request)
        .expect("the timestamp is written");
    verdict_for(&stamped, SIGNER)
}

/// RFC 5652 §5.3 again, in a timestamp token: the authority's signature is
/// over its signed attributes, and only their `messageDigest` ties those to
/// the `TSTInfo` — its `genTime` and its imprint. Without one, a signature
/// that verifies has signed no time at all.
#[test]
fn a_token_with_no_message_digest_has_not_signed_its_tstinfo() {
    let control = assembled_timestamp(true);
    assert_eq!(control.timestamps[0].imprint, DocumentDigest::Matches);
    assert_eq!(control.timestamps[0].signature, SignatureCheck::Verified);

    let unbound = assembled_timestamp(false);
    let stamp = &unbound.timestamps[0];
    assert_eq!(
        stamp.imprint,
        DocumentDigest::Matches,
        "the imprint is right"
    );
    assert_eq!(
        stamp.signature,
        SignatureCheck::NotChecked(Unchecked::NoMessageDigest)
    );
    assert_eq!(unbound.signature, stamp.signature);
    assert!(!stamp.is_trusted());
    assert!(!unbound.is_trusted());
}

// ---- reading and rewriting the fixtures -----------------------------------

fn verdict_with(pdf: &[u8], roots: &[&[u8]]) -> Verdict {
    let document = Document::open(pdf.to_vec()).expect("the fixture opens");
    let mut anchors = TrustAnchors::new();
    for root in roots {
        anchors.add(root.to_vec()).expect("the root parses");
    }
    let mut verdicts = document.verify_signatures(&anchors, Some(AT));
    assert_eq!(verdicts.len(), 1);
    verdicts.remove(0)
}

/// The fixture with one byte of its page content changed, inside the first
/// covered span.
fn tampered(pdf: &[u8]) -> Vec<u8> {
    let mut out = pdf.to_vec();
    let at = find(&out, b"0.2 0.6 0.3 rg").expect("the content stream");
    out[at + 2] = b'9';
    out
}

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
