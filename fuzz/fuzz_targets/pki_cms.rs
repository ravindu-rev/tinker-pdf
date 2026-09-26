//! CMS `SignedData`, which is the shape a PDF signature's `/Contents` is
//! supposed to have.
//!
//! `pki_der` points at raw DER and the X.509 profile drawn over it; this
//! points at the other profile, and the two are different attack surfaces
//! rather than two spellings of one. A `SignedData` nests deeper than a
//! certificate, holds *lists* whose lengths the input chooses — a certificate
//! set, a revocation set, two attribute sets, a signer set — and contains, in
//! an unsigned attribute, a whole `ContentInfo` of its own. Every one of those
//! is a place where a length or a count could be trusted, and none of them is
//! reachable through `pki_der`.
//!
//! The bytes here are attacker-supplied twice over: once by whoever wrote the
//! document, and once by whoever edited it afterwards. Ruling 1 makes a crash
//! here a release blocker.
//!
//! ## What the input is
//!
//! The bytes, unmodified, three ways — no carving and no control byte, because
//! DER *is* the whole input and a mutation anywhere in it is a different tag,
//! length or depth, which is the dimension worth exploring.
//!
//! 1. **The parser as `ContentInfo::parse` runs it** — `Limits::CMS`, which
//!    allows X.690 §8.1.3.6's indefinite length, because that is what a PDF's
//!    `/Contents` is actually read under and four of the eighteen corpus blobs
//!    need it. Every accessor is tried on every signer, every attribute and
//!    every certificate reference it hands back. The refusal paths are as much
//!    of the surface as the acceptance paths: an algorithm OID with nothing
//!    behind it, an attribute with two values, a `SET SIZE (1..MAX)` with
//!    none, a BER `signedAttrs`.
//! 2. **The same parser with the BER form off**, which is what every other
//!    caller of this crate gets. Two readings of one input is the differential
//!    worth hunting for, and running both here is the only place a fuzzer can
//!    reach it.
//! 3. **The parser under a two-level depth cap**, so `DepthExceeded` is on the
//!    path an ordinary input takes rather than only an adversarial one.
//!    Without this the cap is a branch nothing takes.
//! 4. **Every X.509 certificate the message located**, through
//!    `Certificate::parse`. This is how a certificate reaches this engine in
//!    practice — inside a CMS blob inside a PDF — and reaching it through the
//!    set means the *offsets* the CMS walker computed are what the certificate
//!    parser is handed. `Certificate::parse` uses `Limits::CERTIFICATE`, so a
//!    certificate found inside a BER message is still held to RFC 5280 §4.1's
//!    DER — which this target exercises and nothing else does.
//!
//! ## What is asserted beyond "it did not panic"
//!
//! **RFC 5652 §5.4's re-encoding is exactly one byte different from what was
//! stored.** This is the invariant the whole verify path rests on, it is
//! checked here on every input that produces a signer with signed attributes,
//! and it is the one that is invisible when it breaks: a re-encoding that is
//! two bytes different, or a byte shorter, produces a digest that is simply
//! wrong, and a signature that fails to verify for a reason no message names.
//! Specifically: the same length, a `0x31` first octet, an identical tail, and
//! a result that re-reads as a universal `SET` filling itself exactly.
//!
//! **Located bytes lie inside the buffer they were located in.** Every slice a
//! caller is handed — a certificate's DER, a timestamp token's, a signature,
//! an attribute's encoding — must be a subslice of the input. A caller that
//! quotes a byte range for something outside the structure would be quoting
//! bytes the structure does not contain.
//!
//! **A message's own fields agree with the accessors that ask about them.**
//! `x509_certificates()` never yields more than `certificates()` holds;
//! `signed_attrs_to_digest()` is `Some` exactly when `signed_attrs()` is.
//!
//! **What is digested is DER, whatever the message around it was.** A stored
//! `signedAttrs` that parsed must be definite-length throughout — the whole
//! subtree, by `require_definite_lengths` — and the `SET OF` the re-encoding
//! reads back as must itself be definite. That is the one property widening
//! the walker to BER could have quietly broken, and it is checked on every
//! input that produces a signer with signed attributes.

//! # What this target cannot find, and what covers it instead
//!
//! Every assertion above is **structural**: every parsed node lies inside the
//! input, a detached `EncapContentInfo` has no content, and the clauses about
//! BER `signedAttrs` and empty `SET SIZE (1..MAX)` are enforced. None of them
//! asks whether the result is the one the input describes, so an answer that
//! is well-formed and *wrong* passes exactly as a correct one does.
//!
//! A structurally valid `SignedData` whose digest or signer was read wrongly
//! passes all of it — and for a signature reader, a wrong answer reported
//! confidently is worse than a refusal. Correctness lives in `crates/tinker-
//! pdf-pki/tests/enveloped.rs` and in `crates/tinker-
//! pdf/tests/certification.rs`.
//!
//! # The first session
//!
//! **4 September 2026: 58 453 573 executions in 601 seconds — 97 260 a
//! second — and nothing found.** The corpus went from 10 seeds to 1 476.
//! `rustlang/rust:nightly`, rustc 1.100.0-nightly (a69a63265 2026-09-03),
//! cargo-fuzz 0.13.2, one core, on `x86_64-unknown-linux-gnu` in Docker
//! because libFuzzer does not build for `x86_64-pc-windows-msvc`.
//!
//! The rate is the reason this result carries weight. `crypt`'s first session
//! managed 8 a second and a clean run at that rate says almost nothing; at
//! 97 260 this is the fastest target in the tree and 58 million executions is
//! real coverage of the DER walker underneath.
//!
//! What it is *not* is evidence about the assertions above being the right
//! ones. The sibling `pki_der` session found a defect in
//! `require_definite_lengths` on the same day, in its first second — and this
//! target leans on that method for the one property that keeps a BER
//! `signedAttrs` out of a digest. A clean 58 million here did not reach it,
//! because reaching it needs a `SignedData` well-formed enough to produce a
//! signer with signed attributes, and mutation from ten seeds does not get
//! there often. That is the gap to close next, and it is a seed-corpus
//! problem rather than a session-length one.
//!
#![no_main]
use libfuzzer_sys::fuzz_target;

use tinker_pdf_pki::cms::{CertificateChoice, ContentInfo, RevocationChoice, SignerIdentifier};
use tinker_pdf_pki::der::{Budget, Cursor, Limits, Tag};
use tinker_pdf_pki::x509::Certificate;

/// Deep enough for a timestamp token, shallow enough to run fast.
///
/// The BER form is on, because `ContentInfo::parse` runs under `Limits::CMS`
/// and a target that left it off would be fuzzing a mode no caller uses.
const WALK: Limits = Limits::new(40, 16_384).allowing_indefinite_lengths();

/// Ceilings for the assertions rather than for the parse.
///
/// A check that shares the pass's budget would report a spent allowance as a
/// broken invariant, and a fuzz target that can fail for running second is
/// worse than no target. A subtree of the input holds fewer nodes than the
/// input has bytes, so this is unreachable by anything the harness feeds.
const CHECK: Limits = Limits::new(64, 1 << 20).allowing_indefinite_lengths();

/// Whether `slice` is a subslice of `data`, by address.
fn inside(data: &[u8], slice: &[u8]) -> bool {
    let base = data.as_ptr().addr();
    let at = slice.as_ptr().addr();
    at >= base && at.saturating_add(slice.len()) <= base.saturating_add(data.len())
}

/// Reads one message every way there is, asserting as it goes.
fn read_every_way(data: &[u8], limits: Limits) {
    let Ok(info) = ContentInfo::parse_with(data, limits) else {
        return;
    };
    assert!(inside(data, info.der()), "the ContentInfo is not in the input");
    let _ = info.content_type().to_dotted();

    let signed = info.signed_data();
    assert!(inside(data, signed.der()));
    let _ = signed.version();
    for algorithm in signed.digest_algorithms() {
        assert!(inside(data, algorithm.der()));
        assert!(signed.declares(algorithm.oid()));
    }

    let encap = signed.encap_content_info();
    assert!(inside(data, encap.der()));
    assert_eq!(encap.is_detached(), encap.content().is_none());
    if let Some(content) = encap.content() {
        assert!(inside(data, content));
    }

    let mut x509 = 0usize;
    for choice in signed.certificates() {
        match choice {
            CertificateChoice::X509(der) => {
                x509 += 1;
                assert!(inside(data, der));
                // The way a certificate actually reaches this engine: through
                // an offset the CMS walker computed, not through a whole
                // buffer somebody handed over.
                if let Ok(certificate) = Certificate::parse(der) {
                    assert_eq!(certificate.der().len(), der.len());
                    assert_eq!(&der[certificate.tbs_range()], certificate.tbs());
                    let _ = certificate.key_identifier_sha1();
                    let _ = certificate.subject_public_key_info().public_key();
                }
            }
            CertificateChoice::Other { der, .. } => assert!(inside(data, der)),
        }
    }
    assert_eq!(
        signed.x509_certificates().count(),
        x509,
        "the filtered view disagrees with the set it filters"
    );
    for choice in signed.crls() {
        match choice {
            RevocationChoice::Crl(der) | RevocationChoice::Other { der, .. } => {
                assert!(inside(data, der));
            }
        }
    }

    for signer in signed.signer_infos() {
        assert!(inside(data, signer.der()));
        assert!(inside(data, signer.signature()));
        let _ = signer.version();
        let _ = signer.version_matches_sid();
        let _ = signer.digest_algorithm();
        let _ = signer.signature_algorithm();
        let _ = signer.effective_digest();
        let _ = signed.content_type_matches(signer);
        match signer.sid() {
            SignerIdentifier::IssuerAndSerialNumber { issuer, serial, der } => {
                assert!(inside(data, der));
                assert!(issuer.matches(issuer), "a name that does not match itself");
                let _ = issuer.to_rfc4514();
                let _ = serial.to_hex();
            }
            SignerIdentifier::SubjectKeyIdentifier(id) => assert!(inside(data, id)),
        }
        if let Some(digest) = signer.message_digest() {
            assert!(inside(data, digest));
        }
        let _ = signer.signing_time();
        if let Some(attribute) = signer.signing_certificate_v2() {
            assert!(inside(data, attribute.der()));
            for id in attribute.certs() {
                assert!(inside(data, id.hash()));
                let _ = id.digest();
            }
        }
        // RFC 5652 §5.4: whatever the envelope was encoded as, what a
        // signature is computed over has to be DER, all the way down.
        if let Some(attributes) = signer.signed_attrs() {
            // Ceilings of this check's own, wide enough that a `Budget`
            // already spent by the pass above cannot make the answer look
            // like a refusal. The BER form is *allowed* here, so meeting one
            // is refused by name rather than by the reader's mode.
            let budget = Budget::new(CHECK);
            let mut cursor = Cursor::new(attributes.stored_der(), &budget);
            let stored = cursor
                .read()
                .expect("the set parsed once, so its own bytes parse again");
            assert!(
                !stored.is_indefinite(),
                "a BER signedAttrs must have been refused by name"
            );
            assert_eq!(
                stored.require_definite_lengths(&budget),
                Ok(()),
                "an indefinite length survived inside what §5.4 digests"
            );
        }

        for attributes in [signer.signed_attrs(), signer.unsigned_attrs()]
            .into_iter()
            .flatten()
        {
            assert!(inside(data, attributes.stored_der()));
            assert_eq!(
                &data[attributes.stored_range()],
                attributes.stored_der(),
                "the attribute set's range does not name its bytes"
            );
            assert!(
                !attributes.all().is_empty(),
                "`SET SIZE (1..MAX)` must have refused an empty one"
            );
            for attribute in attributes.all() {
                assert!(inside(data, attribute.der()));
                for value in attribute.values() {
                    assert!(inside(data, value.raw()));
                }
                let _ = attribute.single_value();
                let _ = attributes.one(attribute.oid());
                assert!(
                    attributes.find(attribute.oid()).count() >= 1,
                    "an attribute the set holds is not found in it"
                );
            }
        }

        // ---- the invariant the verify path rests on ----------------------
        match (signer.signed_attrs(), signer.signed_attrs_to_digest()) {
            (None, None) => {}
            (Some(attributes), Some(digested)) => {
                let stored = attributes.stored_der();
                assert_eq!(
                    digested.len(),
                    stored.len(),
                    "§5.4 replaces one tag octet; it does not change the length"
                );
                assert_eq!(digested.first(), Some(&0x31), "the universal SET tag");
                assert_eq!(
                    digested.get(1..),
                    stored.get(1..),
                    "nothing but the identifier octet may differ"
                );
                // And the result is a `SET OF` that fills itself exactly,
                // which is what makes it digestible rather than merely
                // different.
                let budget = Budget::new(CHECK);
                let mut cursor = Cursor::new(&digested, &budget);
                let node = cursor
                    .expect(Tag::Set)
                    .expect("the re-encoding is a universal SET");
                assert!(cursor.finish().is_ok(), "with nothing after it");
                assert_eq!(node.raw().len(), digested.len());
                assert!(
                    !node.is_indefinite(),
                    "a re-encoding whose own length is indefinite is not DER, \
                     and §5.4 asks for DER"
                );
            }
            (left, right) => panic!(
                "signed attributes and their re-encoding disagree about existing: \
                 {} vs {}",
                left.is_some(),
                right.is_some()
            ),
        }

        // A timestamp token is a whole `ContentInfo`, so it goes back through
        // the front door — which is also how a nested one would be reached.
        for token in signer.timestamp_tokens() {
            assert!(inside(data, token));
            if let Ok(inner) = ContentInfo::parse_with(token, limits) {
                assert!(inside(data, inner.der()));
                for nested in inner.signed_data().signer_infos() {
                    assert!(inside(data, nested.signature()));
                    let _ = nested.signed_attrs_to_digest();
                    let _ = nested.effective_digest();
                }
            }
        }
    }
}

fuzz_target!(|data: &[u8]| {
    read_every_way(data, WALK);
    // The same input with the BER form off — the reading every other caller of
    // this crate gets, and the half of the differential the pass above cannot
    // see on its own.
    read_every_way(data, Limits::new(40, 16_384));
    // Two levels of nesting allowed, which is past a `ContentInfo` and into
    // its `[0]` and no further, so `DepthExceeded` is on the path an ordinary
    // input takes. With the form on, because an indefinite length reaches the
    // ceiling during the end-of-contents scan rather than on descent, which is
    // a different code path to the same refusal.
    read_every_way(data, Limits::new(2, 16_384).allowing_indefinite_lengths());
    // And a node budget too small for anything real, so the other ceiling is
    // reached as well — including by the scan, which spends against it.
    read_every_way(data, Limits::new(40, 8).allowing_indefinite_lengths());
});
