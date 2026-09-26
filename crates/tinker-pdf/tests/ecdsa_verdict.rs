//! An ECDSA-signed document, from the `/ByteRange` to the anchored chain.
//!
//! `tinker-pdf-crypto` has verified ECDSA over P-256 and P-384 against NIST's
//! CAVP vectors since milestone 5, and until this file nothing reached it: a
//! signer naming `ecdsa-with-SHA256` came back
//! `NotChecked(UnsupportedAlgorithm)` because `verdict::check_signature` had
//! one arm and it was RSA's. The reason recorded for that was "zero corpus
//! signatures use it", and **that is still true** — measured again on
//! 14 September 2026 over the corpora `corpus/corpora.lock` pins: of 34
//! `SignerInfo`s, 20 name `rsaEncryption`, 13 `sha256WithRSAEncryption` and 1
//! `sha1WithRSAEncryption`; of 71 certificates, every one is signed with RSA.
//! Not one ECDSA signature and not one ECDSA certificate. So the roadmap's
//! other route applies: wired against a published vector rather than against a
//! corpus file that does not exist.
//!
//! # Which link of the chain is adjudicated by what
//!
//! This is the whole point of the file and it is stated before the tests
//! because the three links are **not** equally strong evidence.
//!
//! **The curve arithmetic is NIST's.** `EcPublicKey::verify` — the group law,
//! the scalar multiplication, the `[1, n-1]` range checks, the digest
//! truncation of FIPS 186-4 §6.4.2, the curve-membership check — is gated on
//! 120 CAVP `SigVer` vectors in `crates/tinker-pdf-crypto/src/ecdsa.rs` — 24
//! that must verify and 96 that must not, each with NIST's own reason — plus
//! 24 `PKV` vectors, 8 of which are points off the curve and 8 coordinates at
//! or above `p`. Nothing below re-proves that and nothing below could: a wrong
//! group law would fail those vectors first.
//!
//! **The CMS, the certificates and the signature value are OpenSSL's.**
//! The two fixtures were signed once, on 14 September 2026, by **OpenSSL
//! 3.5.5** — key generation, the certificate chain, the `SignedData`, the
//! `signedAttributes` and the ECDSA signature over their DER. So four things
//! below are checked against bytes a second implementation produced, and each
//! is a real disagreement if this engine reads it differently:
//!
//! * RFC 3279 §2.2.3's `SEQUENCE { r INTEGER, s INTEGER }`, `r` first. Swap
//!   them and OpenSSL's signature stops verifying.
//! * RFC 5480 §2.1.1's `SubjectPublicKeyInfo`: the named curve in
//!   `parameters`, the uncompressed point in the BIT STRING.
//! * RFC 5652 §5.4: the signature is over the DER of a `SET OF Attribute`,
//!   which is the stored `[0] IMPLICIT` bytes with the tag replaced.
//!   `the_signature_is_over_the_re_encoded_set_and_not_over_the_stored_tag`
//!   below shows OpenSSL's signature verifying over one and not the other.
//! * X.509 §4.1.1.3: an ECDSA certificate signature over a stored
//!   `TBSCertificate`, which is what the chain walk checks per link.
//!
//! **The `/ByteRange` spans are this repository's on both sides, and that half
//! proves less.** The fixture generator computed the four numbers and handed
//! the covered bytes to OpenSSL, which digested them into `messageDigest`;
//! `crates/tinker-pdf/src/signature.rs` recomputes them here. The generator
//! and the reader are the same author's reading of 12.8.1, so a `Matches`
//! below catches a transcription slip between them and **cannot catch a
//! misreading of the clause**. That is the same honest position
//! `crates/tinker-pdf/tests/pubsec.rs` takes about its own generator, and for
//! the same reason: nothing available here produces an ECDSA-signed PDF.
//! What keeps that half from being worthless is that the other half is not
//! ours — the covered bytes reach OpenSSL as bytes, and if this engine
//! disagreed about *which* bytes those are, `messageDigest` would differ.
//!
//! # Provenance
//!
//! `signature_support/README.md` records the commands, and `THIRDPARTY.md`
//! records why the output of a tool run once is admissible under ruling 13
//! while the tool itself is never asked whether a verdict is right.

use std::ops::Range;

use tinker_pdf::{
    Chain, CmsState, Coverage, Document, DocumentDigest, SignatureCheck, TrustAnchors, Unchecked,
    Weakness,
};
use tinker_pdf_crypto::{Curve, DigestAlgorithm as CryptoDigest, EcPublicKey};
use tinker_pdf_pki::{ContentInfo, PublicKey};

const P256_PDF: &[u8] = include_bytes!("signature_support/ecdsa-p256.pdf");
const P256_ROOT: &[u8] = include_bytes!("signature_support/ecdsa-p256-root.der");
const P384_PDF: &[u8] = include_bytes!("signature_support/ecdsa-p384.pdf");
const P384_ROOT: &[u8] = include_bytes!("signature_support/ecdsa-p384-root.der");

/// Inside both certificates' validity window and nothing to do with now.
/// Ruling 4 keeps the platform clock out of the engine, so the instant a
/// verdict is judged at is a number a test writes down: 1 January 2027.
const AT: i64 = 1_798_761_600;

// ---- the two documents, end to end ----------------------------------------

#[test]
fn a_p256_signed_document_reaches_every_one_of_the_four_answers() {
    check_whole_document(P256_PDF, P256_ROOT, "P256", 32);
}

#[test]
fn a_p384_signed_document_reaches_every_one_of_the_four_answers() {
    // Not a duplicate of the P-256 case, and the difference is the point: a
    // build that read the named curve and then used P-256's parameters anyway
    // would verify the first fixture and refuse this one, because a 96-octet
    // point is not a 64-octet one. The curve OID has to be *read*, not
    // assumed.
    check_whole_document(P384_PDF, P384_ROOT, "P384", 48);
}

fn check_whole_document(pdf: &[u8], root: &[u8], curve_name: &str, coordinate_octets: usize) {
    let document = Document::open(pdf.to_vec()).expect("the fixture opens");
    let signatures = document.signatures();
    assert_eq!(signatures.len(), 1, "one signature field, one signature");
    assert_eq!(signatures[0].coverage, Coverage::WholeFile);

    // The key really is on the curve the name says, and really is the width
    // that curve's field is -- so the test below is not quietly a P-256 test
    // twice.
    let content = ContentInfo::parse(signatures[0].cms()).expect("the CMS parses");
    let signer_der = content
        .signed_data()
        .x509_certificates()
        .next()
        .expect("the blob carries the signer's certificate");
    let certificate = tinker_pdf_pki::Certificate::parse(signer_der).expect("it parses");
    match certificate.subject_public_key_info().public_key() {
        PublicKey::Ec { curve, point } => {
            assert!(curve.is_some(), "RFC 5480 §2.1.1's named curve is there");
            assert_eq!(point[0], 0x04, "SEC 1 §2.3.3's uncompressed form");
            assert_eq!(point.len(), 1 + coordinate_octets * 2, "{curve_name}");
        }
        other => panic!("expected an elliptic-curve key, got {other:?}"),
    }

    let mut anchors = TrustAnchors::new();
    anchors.add(root.to_vec()).expect("the root parses");

    let verdicts = document.verify_signatures(&anchors, Some(AT));
    assert_eq!(verdicts.len(), 1);
    let verdict = &verdicts[0];

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
            assert!(anchor.contains(curve_name), "anchored to {anchor:?}");
            assert_eq!(*links, 0, "the signer's issuer is the anchor");
        }
        other => panic!("expected the walk to reach the anchor, got {other:?}"),
    }
    assert!(
        verdict.weaknesses.is_empty(),
        "nothing to report: SHA-256 or better, no RSA modulus, whole-file \
         coverage, and both certificates valid at the instant asked about — \
         got {:?}",
        verdict.weaknesses
    );
    assert!(verdict.is_trusted());
}

// ---- the four ways it must stop meaning that ------------------------------

#[test]
fn a_changed_document_differs_while_the_signature_still_verifies() {
    // The distinction the verdict type exists for, now on the ECDSA path: a
    // valid signature over bytes this document no longer has. The byte changed
    // is in the page's content stream — the colour the one rectangle is filled
    // with — which is in the `/ByteRange`'s second span and changes what the
    // document *shows*, which is the change a reader cares about.
    let mut tampered = P256_PDF.to_vec();
    let at = find(&tampered, b"0.2 0.3 0.8 rg").expect("the content stream");
    tampered[at + 2] = b'9';
    let verdict = verdict_for(&tampered, P256_ROOT);
    assert_eq!(verdict.document_digest, DocumentDigest::Differs);
    assert_eq!(verdict.signature, SignatureCheck::Verified);
    assert!(!verdict.is_trusted());
}

#[test]
fn a_flipped_bit_in_s_fails_rather_than_going_unchecked() {
    // **The test that tells "verified" from "reached the arm".** An arm that
    // returned `Verified` without calling into the arithmetic would pass every
    // positive test above and fail here, and an arm that returned
    // `NotChecked` for everything would fail here too. Only an arm that
    // actually runs `EcPublicKey::verify` gives `Failed`.
    let mut der = cms_of(P256_PDF);
    let at = signature_value_at(&der);
    let last = at.end - 1;
    der[last] ^= 0x01;

    let verdict = verdict_for(&replace_cms(P256_PDF, &der), P256_ROOT);
    assert_eq!(
        verdict.document_digest,
        DocumentDigest::Matches,
        "the covered bytes did not move: /Contents is the gap the /ByteRange \
         leaves out"
    );
    assert_eq!(
        verdict.signature,
        SignatureCheck::Failed,
        "the arithmetic ran and said no"
    );
    assert!(!verdict.is_trusted());
}

#[test]
fn r_and_s_exchanged_do_not_verify() {
    // Nothing in the encoding distinguishes the two integers; RFC 3279 §2.2.3
    // fixes the order positionally and this is what a build that read them the
    // other way round would be accepting. Done in place, both magnitudes of
    // this fixture being 32 octets, so no enclosing length has to move.
    let mut der = cms_of(P256_PDF);
    let at = signature_value_at(&der);
    let value = &der[at.clone()];
    let (r, s) = tinker_pdf_pki::cms::ecdsa_signature_value(value).expect("it decodes");
    assert_eq!((r.len(), s.len()), (32, 32), "the fixture's own shape");

    let swapped = {
        let mut out = value.to_vec();
        // `30 44 02 20 <r:32> 02 20 <s:32>`: the two runs at 4 and 38.
        assert_eq!(&out[..4], &[0x30, 0x44, 0x02, 0x20]);
        assert_eq!(&out[36..38], &[0x02, 0x20]);
        let (head, tail) = out.split_at_mut(36);
        head[4..36].copy_from_slice(s);
        tail[2..34].copy_from_slice(r);
        out
    };
    der.splice(at, swapped);

    let verdict = verdict_for(&replace_cms(P256_PDF, &der), P256_ROOT);
    assert_eq!(verdict.signature, SignatureCheck::Failed);
    assert!(!verdict.is_trusted());
}

#[test]
fn a_named_curve_this_build_does_not_implement_is_refused_by_name() {
    // `prime192v1` is `1.2.840.10045.3.1.1` and `prime256v1` is
    // `1.2.840.10045.3.1.7`: one octet apart and the same encoded length, so
    // the certificate's shape is untouched and only the claim about the group
    // changes. A build that ignored the OID and used P-256 because the point
    // is 65 octets would report this `Verified`.
    const PRIME256V1: &[u8] = &[0x06, 0x08, 0x2A, 0x86, 0x48, 0xCE, 0x3D, 0x03, 0x01, 0x07];
    let mut der = cms_of(P256_PDF);
    let mut changed = 0;
    let mut from = 0;
    while let Some(at) = find(&der[from..], PRIME256V1) {
        let last = from + at + PRIME256V1.len() - 1;
        der[last] = 0x01;
        from = last + 1;
        changed += 1;
    }
    assert_eq!(changed, 2, "the signer's certificate and the root's");

    let verdict = verdict_for(&replace_cms(P256_PDF, &der), P256_ROOT);
    match &verdict.signature {
        SignatureCheck::NotChecked(Unchecked::UnsupportedKey(why)) => {
            assert!(
                why.contains("1.2.840.10045.3.1.1"),
                "the refusal names the curve it would not use: {why}"
            );
        }
        other => panic!("expected a named refusal, got {other:?}"),
    }
    assert!(!verdict.is_trusted());
}

#[test]
fn a_point_whose_form_octet_says_compressed_is_refused_rather_than_read_as_a_pair() {
    // SEC 1 §2.3.3: `0x04` means the two coordinates follow in full, `0x02`
    // and `0x03` mean only `x` does and `y` must be recovered by a square root
    // whose sign the octet carries. This build reads only `0x04`.
    //
    // **This test exists because the check caught nothing.** The counted
    // injection that dropped the form-octet comparison failed zero tests: the
    // fixture's octets after it are a real uncompressed pair, so ignoring the
    // octet still verified. Changing one byte makes the certificate *claim*
    // compression while carrying a pair, and a build that skips the check then
    // reports `Verified` for a key whose own encoding it disagreed with.
    let mut der = cms_of(P256_PDF);
    let at = signer_point_at(&der);
    assert_eq!(der[at.start], 0x04);
    der[at.start] = 0x02;

    let verdict = verdict_for(&replace_cms(P256_PDF, &der), P256_ROOT);
    match &verdict.signature {
        SignatureCheck::NotChecked(Unchecked::UnsupportedKey(why)) => {
            assert!(why.contains("0x02"), "the refusal names the octet: {why}");
        }
        other => panic!("expected a named refusal, got {other:?}"),
    }
    assert_ne!(verdict.signature, SignatureCheck::Verified);
    assert!(!verdict.is_trusted());
}

#[test]
fn a_signature_value_that_is_not_two_integers_is_not_a_failed_signature() {
    // "We did not look" and "we looked and it was wrong" are the two answers a
    // caller must never confuse, and a malformed `ECDSA-Sig-Value` never
    // reaches the arithmetic. Neither answer is `Verified`, which is what
    // matters; naming which one it is, is what the type is for.
    let mut der = cms_of(P256_PDF);
    let at = signature_value_at(&der);
    der[at.start] = 0x31; // a SET where RFC 3279 §2.2.3 writes a SEQUENCE

    let verdict = verdict_for(&replace_cms(P256_PDF, &der), P256_ROOT);
    assert!(
        matches!(
            verdict.signature,
            SignatureCheck::NotChecked(Unchecked::MalformedSignatureValue(_))
        ),
        "got {:?}",
        verdict.signature
    );
    assert_ne!(verdict.signature, SignatureCheck::Verified);
    assert!(!verdict.is_trusted());
}

#[test]
fn a_chain_link_whose_ecdsa_signature_is_wrong_breaks_the_path() {
    // The other ECDSA arm: `verdict::verifies`, which asks whether the
    // issuer's key signed the child's stored `TBSCertificate`. Before this
    // commit it answered `false` for every ECDSA certificate, so a chain of
    // them read `Broken` whether it was or not — and `Broken` for a good path
    // and `Broken` for a forged one are the same word.
    //
    // The certificates are not covered by the `SignerInfo`'s signature, so
    // corrupting one leaves question 3 alone and moves only question 4. That
    // separation is the assertion.
    let mut der = cms_of(P256_PDF);
    let at = leaf_certificate_signature_at(&der);
    der[at.end - 1] ^= 0x01;

    let verdict = verdict_for(&replace_cms(P256_PDF, &der), P256_ROOT);
    assert_eq!(
        verdict.signature,
        SignatureCheck::Verified,
        "the signer's own signature is untouched"
    );
    match &verdict.chain {
        Chain::Broken { at } => assert!(at.contains("Root"), "broken at {at:?}"),
        other => panic!("expected a broken path, got {other:?}"),
    }
    assert!(!verdict.is_trusted());
}

#[test]
fn an_anchor_the_caller_did_not_supply_is_not_reached() {
    // The P-384 root does not issue the P-256 leaf, and offering it must not
    // produce a path. `SelfSigned` rather than `Incomplete`: the walk steps
    // from the leaf to the root in the blob, verifies that link, and stops at
    // a self-signed certificate nobody trusted.
    let verdict = verdict_for(P256_PDF, P384_ROOT);
    assert_eq!(verdict.signature, SignatureCheck::Verified);
    match &verdict.chain {
        Chain::SelfSigned { subject } => assert!(subject.contains("P256 Test Root")),
        other => panic!("expected the path to stop unanchored, got {other:?}"),
    }
    assert!(!verdict.is_trusted());
}

// ---- RFC 5652 §5.4, on bytes this repository did not write -----------------

/// The signature is over the re-encoded `SET OF`, not over the stored `[0]`.
///
/// The one clause in this file that OpenSSL adjudicates outright: it signed
/// the DER of a `SET OF Attribute`, and the bytes in the blob carry the
/// `[0] IMPLICIT` tag instead. If `signed_attrs_to_digest` did not substitute
/// the tag — or substituted it into the wrong byte — the first assertion would
/// fail; if the substitution were a no-op the second would.
#[test]
fn the_signature_is_over_the_re_encoded_set_and_not_over_the_stored_tag() {
    for (pdf, digest) in [
        (P256_PDF, CryptoDigest::Sha256),
        (P384_PDF, CryptoDigest::Sha384),
    ] {
        let der = cms_of(pdf);
        let content = ContentInfo::parse(&der).expect("the CMS parses");
        let signed = content.signed_data();
        let signer = signed.signer_infos().first().expect("one signer");

        let certificate =
            tinker_pdf_pki::Certificate::parse(signed.x509_certificates().next().expect("a cert"))
                .expect("it parses");
        let PublicKey::Ec {
            curve: Some(_),
            point,
        } = certificate.subject_public_key_info().public_key()
        else {
            panic!("an elliptic-curve key with a named curve")
        };
        let width = (point.len() - 1) / 2;
        let curve = if width == 32 {
            Curve::P256
        } else {
            Curve::P384
        };
        let key = EcPublicKey::new(curve, &point[1..=width], &point[1 + width..])
            .expect("a point on its curve");

        let (r, s) =
            tinker_pdf_pki::cms::ecdsa_signature_value(signer.signature()).expect("r and s");
        let re_encoded = signer.signed_attrs_to_digest().expect("signed attributes");
        let stored = signer.signed_attrs().expect("they are there").stored_der();

        assert!(
            key.verify_message(digest, &re_encoded, r, s).is_ok(),
            "§5.4's SET OF re-encoding"
        );
        assert!(
            key.verify_message(digest, stored, r, s).is_err(),
            "and not the stored [0] IMPLICIT bytes, which differ from it in \
             exactly one octet"
        );
        assert_eq!(re_encoded.len(), stored.len(), "one octet, not one length");
        assert_eq!(re_encoded[0], 0x31, "the universal SET tag");
        assert_eq!(stored[0], 0xA0, "the [0] IMPLICIT constructed tag");
    }
}

// ---- reading and rewriting the fixtures -----------------------------------

fn verdict_for(pdf: &[u8], root: &[u8]) -> tinker_pdf::Verdict {
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
    let signatures = document.signatures();
    signatures[0].cms().to_vec()
}

/// The same document with a different blob in `/Contents`, hex-encoded back
/// into the reservation the fixture already has.
///
/// The reservation's width does not change, so neither do the `/ByteRange`
/// numbers or any offset in the file — which is what makes a mutated blob a
/// test of the verdict rather than a test of the parser.
fn replace_cms(pdf: &[u8], der: &[u8]) -> Vec<u8> {
    let span = contents_span(pdf);
    assert!(der.len() * 2 <= span.len(), "the reservation holds it");
    let mut hex = String::with_capacity(span.len());
    for byte in der {
        hex.push_str(&format!("{byte:02X}"));
    }
    while hex.len() < span.len() {
        hex.push('0');
    }
    let mut out = pdf.to_vec();
    out[span].copy_from_slice(hex.as_bytes());
    out
}

/// The hexadecimal digits between `/Contents <` and its `>`.
fn contents_span(pdf: &[u8]) -> Range<usize> {
    let start = find(pdf, b"/Contents <").expect("the fixture has one") + b"/Contents <".len();
    let end = start + find(&pdf[start..], b">").expect("it is closed");
    start..end
}

/// Where the `SignerInfo`'s `signature` value sits inside the blob.
///
/// Found by searching for the bytes the parser handed back rather than by
/// counting offsets, so this does not have to know the blob's layout.
fn signature_value_at(der: &[u8]) -> Range<usize> {
    let content = ContentInfo::parse(der).expect("the CMS parses");
    let signature = content
        .signed_data()
        .signer_infos()
        .first()
        .expect("one signer")
        .signature()
        .to_vec();
    let at = find(der, &signature).expect("the signature is in the blob");
    at..at + signature.len()
}

/// And where the signer's certificate's own `signatureValue` sits.
fn leaf_certificate_signature_at(der: &[u8]) -> Range<usize> {
    let content = ContentInfo::parse(der).expect("the CMS parses");
    let leaf = content
        .signed_data()
        .x509_certificates()
        .next()
        .expect("the signer's certificate is first");
    let certificate = tinker_pdf_pki::Certificate::parse(leaf).expect("it parses");
    let signature = certificate
        .signature()
        .whole_bytes()
        .expect("a whole number of octets")
        .to_vec();
    let at = find(der, &signature).expect("it is in the blob");
    at..at + signature.len()
}

/// Where the signer's certificate's elliptic-curve point sits in the blob,
/// starting at SEC 1 §2.3.3's form octet.
fn signer_point_at(der: &[u8]) -> Range<usize> {
    let content = ContentInfo::parse(der).expect("the CMS parses");
    let leaf = content
        .signed_data()
        .x509_certificates()
        .next()
        .expect("the signer's certificate is first");
    let certificate = tinker_pdf_pki::Certificate::parse(leaf).expect("it parses");
    let PublicKey::Ec { point, .. } = certificate.subject_public_key_info().public_key() else {
        panic!("an elliptic-curve key")
    };
    let point = point.to_vec();
    let at = find(der, &point).expect("it is in the blob");
    at..at + point.len()
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len())
        .position(|window| window == needle)
}

/// A weakness variant this file never expects, referenced so that adding one
/// to the enum without thinking about the ECDSA path is a compile error here.
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
