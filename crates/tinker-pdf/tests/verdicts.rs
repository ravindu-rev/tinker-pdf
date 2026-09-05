//! What `Document::verify_signatures` says about real signed documents.
//!
//! Milestone 6 of `docs/design/signatures.md` is the assembly: the coverage
//! classifier, the CMS parser, the RSA verifier and the chain walk all exist
//! and each is gated on its own evidence. What this file measures is whether
//! putting them together produces answers about documents nobody here wrote.
//!
//! Two of the four questions can be adjudicated by the corpus and two cannot.
//! **The document digest can**: a `messageDigest` attribute equalling a digest
//! recomputed from the file's own `/ByteRange` is a fact about seventeen
//! documents from six producers, and no reading of a clause can make it come
//! out right by accident. **The signature check can** likewise. The chain
//! cannot, because a chain needs a trust anchor and this repository has no
//! business shipping one — so its evidence is a fixture where the anchor is
//! the corpus's own root, which proves the walk terminates where it should and
//! nothing about whether that root deserves trust.
//!
//! ```sh
//! cargo test -p tinker-pdf --test verdicts -- --ignored --nocapture
//! ```

use std::path::{Path, PathBuf};

use tinker_pdf::{
    Chain, CmsState, Coverage, Document, DocumentDigest, SignatureCheck, TrustAnchors, Unchecked,
    Weakness,
};

// ---- what a caller gets with nothing real ---------------------------------

#[test]
fn an_unsigned_document_has_no_verdicts() {
    let bytes = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../testdata/simple-text.pdf"
    ))
    .expect("testdata/simple-text.pdf");
    let document = Document::open(bytes).expect("opens");
    assert!(document
        .verify_signatures(&TrustAnchors::new(), None)
        .is_empty());
}

#[test]
fn an_anchor_that_is_not_a_certificate_is_refused_rather_than_ignored() {
    let mut anchors = TrustAnchors::new();
    assert!(anchors.add(b"not a certificate".to_vec()).is_err());
    assert!(anchors.is_empty());
}

// ---- the corpus -----------------------------------------------------------

fn corpus_root() -> Option<PathBuf> {
    if let Ok(path) = std::env::var("TINKER_CORPUS") {
        let path = PathBuf::from(path);
        return path.is_dir().then_some(path);
    }
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../corpus/files")
        .canonicalize()
        .ok()
        .filter(|path| path.is_dir())
}

fn pdfs_under(root: &Path, into: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            pdfs_under(&path, into);
        } else if path
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("pdf"))
        {
            into.push(path);
        }
    }
}

#[derive(Default)]
struct Tally {
    signatures: usize,
    cms_read: usize,
    cms_absent: usize,
    cms_unreadable: usize,
    digest_matches: usize,
    digest_differs: usize,
    digest_unchecked: usize,
    signature_verified: usize,
    signature_failed: usize,
    signature_unchecked: usize,
    no_anchors: usize,
    sha1_digest: usize,
    sha1_signature: usize,
    short_key: usize,
    timestamped: usize,
}

#[test]
#[ignore = "walks the fetched corpora; run with --ignored --nocapture"]
fn every_corpus_signature_gets_a_verdict() {
    let Some(root) = corpus_root() else {
        println!("SKIPPED (no corpus; set TINKER_CORPUS)");
        return;
    };
    let mut all = Vec::new();
    pdfs_under(&root, &mut all);
    all.retain(|path| {
        std::fs::read(path).is_ok_and(|bytes| {
            bytes
                .windows(b"/ByteRange".len())
                .any(|window| window == b"/ByteRange")
        })
    });
    all.sort();
    println!("RAN over {} files naming /ByteRange", all.len());

    let anchors = TrustAnchors::new();
    let mut tally = Tally::default();

    for path in &all {
        let name = path
            .strip_prefix(&root)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/");
        let Ok(bytes) = std::fs::read(path) else {
            continue;
        };
        let Ok(document) = Document::open(bytes) else {
            continue;
        };
        if document.is_encrypted() {
            let _ = document.authenticate("");
        }

        for verdict in document.verify_signatures(&anchors, None) {
            tally.signatures += 1;
            match &verdict.cms {
                CmsState::Read { .. } => tally.cms_read += 1,
                CmsState::Absent => tally.cms_absent += 1,
                CmsState::Unreadable(_) => tally.cms_unreadable += 1,
            }
            match &verdict.document_digest {
                DocumentDigest::Matches => tally.digest_matches += 1,
                DocumentDigest::Differs => tally.digest_differs += 1,
                DocumentDigest::NotChecked(_) => tally.digest_unchecked += 1,
            }
            match &verdict.signature {
                SignatureCheck::Verified => tally.signature_verified += 1,
                SignatureCheck::Failed => tally.signature_failed += 1,
                SignatureCheck::NotChecked(_) => tally.signature_unchecked += 1,
            }
            if verdict.chain == Chain::NoAnchors {
                tally.no_anchors += 1;
            }
            for weakness in &verdict.weaknesses {
                match weakness {
                    Weakness::Sha1Digest => tally.sha1_digest += 1,
                    Weakness::Sha1Signature => tally.sha1_signature += 1,
                    Weakness::ShortRsaKey { .. } => tally.short_key += 1,
                    _ => {}
                }
            }
            if verdict
                .signer
                .as_ref()
                .is_some_and(|signer| signer.timestamped)
            {
                tally.timestamped += 1;
            }

            println!(
                "  {name}\n    coverage {:?}, cms {:?}, digest {:?}, signature {:?}",
                short(&verdict.coverage),
                short(&verdict.cms),
                short(&verdict.document_digest),
                short(&verdict.signature)
            );
            if let Some(signer) = &verdict.signer {
                println!("    signer {:?}", signer.subject);
            }
        }
    }

    println!(
        "\nsignatures {}\n  cms: {} read, {} absent, {} unreadable\n  \
         document digest: {} match, {} differ, {} unchecked\n  \
         signature: {} verified, {} failed, {} unchecked\n  \
         weak: {} sha-1 digests, {} sha-1 signatures, {} short keys\n  \
         {} carry a timestamp token; {} reported no anchors",
        tally.signatures,
        tally.cms_read,
        tally.cms_absent,
        tally.cms_unreadable,
        tally.digest_matches,
        tally.digest_differs,
        tally.digest_unchecked,
        tally.signature_verified,
        tally.signature_failed,
        tally.signature_unchecked,
        tally.sha1_digest,
        tally.sha1_signature,
        tally.short_key,
        tally.timestamped,
        tally.no_anchors,
    );

    // Recorded against the corpora `corpus/corpora.lock` pins. Every number
    // below was checked by hand against the files when it was written.
    // **These numbers moved on 6 September 2026 because the corpus grew**, not
    // because anything here changed: `corpus/corpora.lock` gained a fifth
    // entry, the SafeDocs shard of a thousand documents off the open web, and
    // real documents carry real signatures. The four fixture corpora still
    // produce exactly the figures this file recorded before -- measured, by
    // moving the shard out of `corpus/files` and running this again -- so the
    // difference is nine signatures nobody wrote to test a reader.
    assert_eq!(tally.signatures, 27, "signatures found");
    assert_eq!(tally.cms_read, 21, "blobs that parsed");
    assert_eq!(
        tally.cms_absent, 6,
        "blobs the coverage classifier would not vouch for: a `/ByteRange` \
         that does not bracket a hexadecimal string yields no bytes, and \
         handing a parser whatever happens to be at those offsets would be a \
         verdict about the wrong data"
    );
    assert_eq!(
        tally.cms_unreadable, 0,
        "every blob the coverage classifier vouches for now parses; this was          3 until BER indefinite lengths were read, and those three files came          from two independent producer lineages"
    );

    // Twenty of twenty-seven, and nine of those twenty arrived with the
    // production corpus on 6 September 2026: real signatures on real
    // documents, including a Romanian qualified signature on
    // `safedocs/0000020.pdf` that verifies against the key in its own
    // certificate. The fixture corpora alone still give 11.
    assert_eq!(
        tally.signature_verified, 20,
        "signatures that verify against the key in their own certificate"
    );
    assert_eq!(
        tally.signature_failed, 0,
        "and none that parses fails to verify, which would be a finding about \
         a real document rather than about this code"
    );

    // **The finding.** Four documents carry a signature that verifies and a
    // `messageDigest` that does not match the bytes the `/ByteRange` covers.
    //
    // They are veraPDF's `6.1.12 Permissions` and `6.1.11 Permissions`
    // fixtures, and the cause is visible in the bytes: three of them — of
    // 6 706, 7 207 and 11 516 bytes — carry the **byte-identical** CMS blob.
    // One signature cannot cover three different documents, so the suite
    // copied a signature between files. It had no reason not to: those
    // fixtures test a permissions rule, not signature validity.
    //
    // This is the check working, on its first contact with documents nobody
    // here wrote, and it is why questions 2 and 3 are asked separately: every
    // one of these reports a *verified signature* over a *changed document*,
    // which a single boolean would have to call one thing or the other and
    // would be wrong either way.
    assert_eq!(
        tally.digest_differs, 4,
        "veraPDF's permission fixtures share signatures between documents"
    );
    assert_eq!(tally.digest_matches, 16, "documents that still hash right");

    assert_eq!(
        tally.no_anchors, tally.cms_read,
        "no anchors were supplied, so every chain that got as far as being \
         walked must say so rather than reaching one"
    );
}

/// The first word of a `Debug`, so a per-file line stays one line.
fn short<T: std::fmt::Debug>(value: &T) -> String {
    let text = format!("{value:?}");
    text.split(['(', ' ', '{'])
        .next()
        .unwrap_or(&text)
        .to_string()
}

/// The one shape the corpus cannot show: a chain that reaches an anchor.
///
/// Built from the corpus's own certificates — the signer's issuer becomes the
/// anchor — so what it proves is that the walk terminates where it is told to
/// and verifies each link on the way. It proves nothing whatever about
/// whether that issuer deserves trust, which is the caller's question and
/// stays the caller's.
#[test]
#[ignore = "reads the fetched corpora"]
fn a_chain_reaches_an_anchor_when_one_is_supplied() {
    let Some(root) = corpus_root() else {
        println!("SKIPPED (no corpus; set TINKER_CORPUS)");
        return;
    };
    let path = root.join("qpdf/qpdf/qtest/qpdf/digitally-signed.pdf");
    let Ok(bytes) = std::fs::read(&path) else {
        println!("SKIPPED (the qpdf corpus is not fetched)");
        return;
    };
    let document = Document::open(bytes).expect("opens");
    let signature = document.signatures().into_iter().next().expect("one");
    assert_eq!(signature.coverage, Coverage::WholeFile);

    // Without anchors: no path attempted, and it says so.
    let without = document.verify_signatures(&TrustAnchors::new(), None);
    assert_eq!(without[0].chain, Chain::NoAnchors);
    assert_eq!(
        without[0].document_digest,
        DocumentDigest::Matches,
        "the document still hashes to what was signed"
    );
    assert_eq!(without[0].signature, SignatureCheck::Verified);

    // With every certificate in the blob offered as an anchor, the walk must
    // reach one rather than running to `SelfSigned` or `Incomplete`.
    let mut anchors = TrustAnchors::new();
    let content = tinker_pdf_pki::ContentInfo::parse(signature.cms()).expect("the CMS parses");
    let mut offered = 0;
    for der in content.signed_data().x509_certificates() {
        if anchors.add(der.to_vec()).is_ok() {
            offered += 1;
        }
    }
    assert!(offered > 0, "the blob carries certificates");

    let with = document.verify_signatures(&anchors, None);
    match &with[0].chain {
        Chain::AnchoredTo { anchor, links } => {
            println!("anchored to {anchor:?} in {links} links, from {offered} offered");
        }
        other => panic!("expected the walk to reach an anchor, got {other:?}"),
    }
    assert!(
        with[0].is_trusted(),
        "whole-file coverage, a matching digest, a verified signature and an \
         anchored chain is the one combination that means what a caller hopes"
    );

    // And a document changed after signing must stop meaning that. Flipping a
    // byte inside the covered range is the minimal tamper.
    let mut tampered = std::fs::read(&path).expect("read again");
    tampered[9] ^= 0x01;
    let tampered = Document::open(tampered).expect("still opens");
    let verdicts = tampered.verify_signatures(&anchors, None);
    assert_eq!(
        verdicts[0].document_digest,
        DocumentDigest::Differs,
        "one flipped byte inside the signed range"
    );
    assert_eq!(
        verdicts[0].signature,
        SignatureCheck::Verified,
        "and the signature itself is still a valid signature — over bytes \
         this document no longer has, which is exactly why the two questions \
         are not one question"
    );
    assert!(!verdicts[0].is_trusted());
}

/// A document this engine signed with a stub that returns bytes rather than
/// CMS: the blob is unreadable, and every dependent check says so by name.
#[test]
fn a_signature_whose_contents_are_not_cms_is_unreadable_rather_than_invalid() {
    use tinker_pdf::{DigestAlgorithm, SignRefused, Signer, SigningRequest, SigningTarget};

    struct Nonsense;
    impl Signer for Nonsense {
        fn digest_algorithm(&self) -> DigestAlgorithm {
            DigestAlgorithm::Sha256
        }
        fn sign(&self, _digest: &[u8]) -> Result<Vec<u8>, SignRefused> {
            Ok(b"this is not a SignedData".to_vec())
        }
    }

    let bytes = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../testdata/simple-text.pdf"
    ))
    .expect("testdata/simple-text.pdf");
    let signer = Nonsense;
    let mut request = SigningRequest::new(
        SigningTarget::NewInvisibleField {
            name: "Signature1".to_string(),
        },
        &signer,
    );
    request.reserve = 256;
    let signed = Document::open(bytes)
        .expect("opens")
        .editor()
        .save_signed(
            &tinker_pdf::WriteOptions {
                mode: tinker_pdf::WriteMode::Incremental,
                ..Default::default()
            },
            &request,
        )
        .expect("signing");

    let document = Document::open(signed).expect("reopens");
    let verdicts = document.verify_signatures(&TrustAnchors::new(), None);
    assert_eq!(verdicts.len(), 1);
    assert!(
        matches!(verdicts[0].cms, CmsState::Unreadable(_)),
        "got {:?}",
        verdicts[0].cms
    );
    assert!(
        matches!(
            verdicts[0].document_digest,
            DocumentDigest::NotChecked(Unchecked::UnsupportedAlgorithm(_))
        ),
        "an unreadable blob leaves the digest unchecked, not differing: {:?}",
        verdicts[0].document_digest
    );
    assert!(!verdicts[0].is_trusted());
}
