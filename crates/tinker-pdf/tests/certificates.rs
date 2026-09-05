//! Milestone 2's sidecar: what `tinker-pdf-pki` reads out of the corpus's
//! certificates, against values it did not produce.
//!
//! `cms_census.rs` already asserts that every certificate in the fetched
//! corpora *parses*, which is worth a great deal and is not the same claim.
//! A parser can read every certificate it meets and read them all wrong; the
//! only thing that catches that is comparing what it read against a value
//! from somewhere else.
//!
//! The somewhere else is `signature_support/certificates.tsv`, produced once
//! by OpenSSL and committed — its header records the commands, the version and
//! the date. Ruling 13 permits precisely that and no more: a third-party
//! program may supply data, and the committed output of a tool run once is a
//! dated measurement. Nothing here invokes OpenSSL, and `cargo xtask oracles`
//! would refuse it if it tried.
//!
//! ```sh
//! cargo test -p tinker-pdf --test certificates -- --ignored --nocapture
//! ```

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use tinker_pdf_crypto::sha2::sha256;
use tinker_pdf_pki::{Certificate, ContentInfo};

/// One row of the sidecar.
#[derive(Debug, PartialEq, Eq)]
struct Expected {
    serial: String,
    not_before: i64,
    not_after: i64,
    spki_sha256: String,
    self_issued: bool,
    subject_cn: String,
    issuer_cn: String,
}

fn sidecar() -> BTreeMap<String, Expected> {
    let text = include_str!("signature_support/certificates.tsv");
    let mut rows = BTreeMap::new();
    for line in text.lines() {
        if line.starts_with('#') || line.is_empty() || line.starts_with("sha256\t") {
            continue;
        }
        let fields: Vec<&str> = line.split('\t').collect();
        assert_eq!(fields.len(), 8, "eight columns per row: {line}");
        rows.insert(
            fields[0].to_string(),
            Expected {
                serial: fields[1].to_string(),
                not_before: fields[2].parse().expect("not_before"),
                not_after: fields[3].parse().expect("not_after"),
                spki_sha256: fields[4].to_string(),
                self_issued: fields[5] == "true",
                subject_cn: fields[6].to_string(),
                issuer_cn: fields[7].to_string(),
            },
        );
    }
    rows
}

#[test]
fn the_sidecar_is_well_formed_and_not_empty() {
    let rows = sidecar();
    assert_eq!(rows.len(), 24, "certificates recorded");
    for (digest, row) in &rows {
        assert_eq!(digest.len(), 64, "a SHA-256 in hex");
        assert!(
            row.not_before < row.not_after,
            "a validity window: {digest}"
        );
        assert_eq!(row.spki_sha256.len(), 64, "a SHA-256 in hex: {digest}");
        assert!(!row.serial.is_empty(), "a serial: {digest}");
    }
}

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

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// The common name, or empty where there is none.
fn common_name(name: &tinker_pdf_pki::Name<'_>) -> String {
    name.common_name().unwrap_or_default().to_string()
}

#[test]
#[ignore = "walks the fetched corpora; run with --ignored --nocapture"]
fn every_recorded_certificate_reads_back_as_recorded() {
    let Some(root) = corpus_root() else {
        println!("SKIPPED (no corpus; set TINKER_CORPUS)");
        return;
    };
    let expected = sidecar();

    let mut files = Vec::new();
    pdfs_under(&root, &mut files);
    files.sort();

    // Every certificate the corpus's CMS blobs carry, by the digest of its DER.
    let mut seen: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    for path in &files {
        let Ok(bytes) = std::fs::read(path) else {
            continue;
        };
        if !bytes
            .windows(b"/ByteRange".len())
            .any(|window| window == b"/ByteRange")
        {
            continue;
        }
        let Ok(document) = tinker_pdf::Document::open(bytes) else {
            continue;
        };
        if document.is_encrypted() {
            let _ = document.authenticate("");
        }
        for signature in document.signatures() {
            let Ok(content) = ContentInfo::parse(signature.cms()) else {
                continue;
            };
            for der in content.signed_data().x509_certificates() {
                seen.insert(hex(&sha256(der)), der.to_vec());
            }
        }
    }
    println!("RAN over {} certificates found in the corpora", seen.len());

    let mut checked = 0;
    let mut missing = Vec::new();
    for (digest, row) in &expected {
        let Some(der) = seen.get(digest) else {
            missing.push(digest.clone());
            continue;
        };
        let certificate = Certificate::parse(der)
            .unwrap_or_else(|error| panic!("{digest} does not parse: {error:?}"));

        // Serial, compared as the DER content octets rather than as either
        // side's rendering of the number.
        //
        // The two renderings genuinely disagree, on a real certificate: one in
        // the corpus carries a serial whose top bit is set with no leading
        // zero, so it *is* a negative integer in DER. RFC 5280 §4.1.2.2 says a
        // serial "MUST be a positive integer" and this issuer emitted one that
        // is not. OpenSSL prints the octets; this crate reads the number and
        // prints `-0603E746…`. Both are right about different questions, and
        // neither is the certificate's identity — the octets are, which is
        // what `issuerAndSerialNumber` matching compares.
        //
        // The sidecar's column is octets too, read from the DER by an
        // independent scanner, so this is byte-for-byte with no allowance —
        // including DER's leading zero on a positive integer whose top bit is
        // set, which is part of the encoding and part of the identity.
        assert_eq!(
            hex(certificate.serial().as_bytes()).to_uppercase(),
            row.serial,
            "{digest}: serial"
        );
        let validity = certificate.validity();
        assert_eq!(validity.not_before, row.not_before, "{digest}: notBefore");
        assert_eq!(validity.not_after, row.not_after, "{digest}: notAfter");
        assert_eq!(
            hex(&sha256(certificate.subject_public_key_info().der())),
            row.spki_sha256,
            "{digest}: SubjectPublicKeyInfo"
        );
        assert_eq!(
            certificate.is_self_issued(),
            row.self_issued,
            "{digest}: self-issued"
        );
        assert_eq!(
            common_name(certificate.subject()),
            row.subject_cn,
            "{digest}: subject CN"
        );
        assert_eq!(
            common_name(certificate.issuer()),
            row.issuer_cn,
            "{digest}: issuer CN"
        );
        checked += 1;
    }

    println!("checked {checked} of {} recorded", expected.len());
    if !missing.is_empty() {
        println!("recorded but not found in this corpus: {missing:#?}");
    }
    // The sidecar records 24 and this build reaches 17. The gap has moved
    // twice: it was 13 until BER indefinite lengths were read, which put two
    // more blobs' certificates in reach, and 15 until 6 September 2026, when
    // the corpus gained the SafeDocs shard and two of the recorded
    // certificates turned up again inside real-world signatures whose
    // coverage *does* hold. Both re-records are what this pair of assertions
    // exists to force, and both times the test failed the moment the
    // population moved.
    //
    // The seven still out of reach sit in blobs whose `/ByteRange` does not
    // bracket their `/Contents`, so `Signature::cms()` hands back nothing.
    // They are readable by a scanner that ignores the coverage classifier, and
    // the sidecar was built by exactly such a scanner — but reading a CMS the
    // classifier will not vouch for is the thing milestone 1 exists to refuse,
    // so this test declines to do it too.
    //
    // A sidecar that silently matched nothing would be worse than none, and
    // one that quietly matched fewer than it used to would be worse still.
    assert_eq!(
        checked, 17,
        "certificates checked against the sidecar; the rest sit inside blobs \
         whose coverage does not hold up"
    );
    assert_eq!(missing.len(), 7, "recorded but unreachable: {missing:#?}");
}
