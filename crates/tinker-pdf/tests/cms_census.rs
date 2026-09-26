//! What real producers put in a PDF signature's `/Contents` (RFC 5652).
//!
//! `crates/tinker-pdf-pki/src/cms.rs` is checked against fixtures it builds
//! itself, which proves the parser agrees with the grammar and proves nothing
//! about the grammar agreeing with the world. This is the other half: every
//! CMS blob in the fetched corpora, put through the same parser, with the
//! result printed as a table and pinned by hard counts.
//!
//! ```sh
//! cargo test -p tinker-pdf --test cms_census -- --ignored --nocapture
//! ```
//!
//! # Two routes to the same bytes, and the difference between them is a finding
//!
//! `Signature::contents` is taken from the gap the `/ByteRange` spans leave,
//! which milestone 1 chose deliberately: the bytes a signature covers are the
//! bytes the file says it covers, not the ones the object model happens to
//! hold. The consequence shows up here for the first time — **six of the
//! eighteen signed documents have a `/ByteRange` that does not point at their
//! `/Contents`**, so the supported path hands the CMS parser nothing at all,
//! and those six are exactly the ones `tests/signatures.rs` already classifies
//! as `Coverage::Suspicious`.
//!
//! Those six do carry a CMS blob. So this file censuses twice. The first pass
//! is the supported path, and its numbers say how much of the corpus a caller
//! actually reaches today. The second is an independent raw byte scan for
//! `/Contents <…>` — sharing nothing with the reader, in the same spirit as
//! `tests/signatures.rs`'s `/ByteRange` scan — and its numbers say what is in
//! these files at all. Reporting only the first would credit the CMS parser
//! with a limitation that is not its, and reporting only the second would
//! claim a reach the engine does not have.
//!
//! # What the second pass measures that no fixture can
//!
//! **How much of the corpus is BER, and whether reading it changed anything.**
//! Four of the eighteen blobs open `30 80 … A0 80 30 80`: indefinite lengths
//! on the outermost structural nodes, which RFC 5652 §5.1 permits and ISO
//! 32000-1 12.8.3.3.1 does not. `tinker-pdf-pki` refused all four until
//! `Limits::CMS` gained `allow_indefinite_lengths`; it now reads them, and the
//! two numbers that matter are asserted here rather than described. Every
//! blob parses, and **every one of the four BER blobs' signatures verifies
//! over the §5.4 re-encoding using this engine's own RSA** — which is the
//! evidence that the walker found the right bytes, not merely bytes it liked.
//! The count of BER blobs is pinned too, so a producer's encoding is a
//! measured fact rather than an impression.
//!
//! **What stays refused inside them.** §5.4 digests the DER of `signedAttrs`,
//! and all four write theirs with definite lengths — so the rule that a BER
//! `signedAttrs` is refused by name costs this corpus nothing, which is a
//! measurement and not a guess. `cms.rs` holds it up with a fixture.
//!
//! **Whether the certificate parser works on real certificates.** Milestone 2
//! was gated on RFC 5280's appendix examples, which are hand transcriptions of
//! a document. The certificate sets in these blobs are the first certificates
//! this engine has met that somebody else's software produced.
//!
//! **Whether RFC 5652 §5.4's re-encoding rule is right**, checked the only way
//! it can be checked: by verifying a real signature with it. A signature made
//! by a real signer over a real `signedAttrs` set verifies when the `[0]` tag
//! is replaced by a universal `SET` tag, and does not verify when it is not,
//! so the corpus adjudicates the rule rather than this repository's reading of
//! it. That is first-party verification in ruling 13's sense — the arithmetic
//! is this engine's own, and the evidence is somebody else's bytes.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use tinker_pdf::Document;
use tinker_pdf_crypto::RsaPublicKey;
use tinker_pdf_pki::cms::{CertificateChoice, CmsError, ContentInfo, SignerIdentifier, SignerInfo};
use tinker_pdf_pki::der::{Budget, Cursor, Limits};
use tinker_pdf_pki::x509::{Certificate, PublicKey};

// ---- finding the files ----------------------------------------------------

fn corpus_root() -> Option<PathBuf> {
    if let Ok(path) = std::env::var("TINKER_CORPUS") {
        let path = PathBuf::from(path);
        return path.is_dir().then_some(path);
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/files");
    root.canonicalize().ok().filter(|path| path.is_dir())
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

/// Every file whose raw bytes contain `/ByteRange`, found without the reader —
/// the same byte scan `tests/signatures.rs` uses, and for the reason its
/// header gives.
fn files_naming_a_byte_range(root: &Path) -> Vec<PathBuf> {
    let mut all = Vec::new();
    pdfs_under(root, &mut all);
    all.retain(|path| {
        std::fs::read(path).is_ok_and(|bytes| {
            bytes
                .windows(b"/ByteRange".len())
                .any(|window| window == b"/ByteRange")
        })
    });
    all.sort();
    all
}

/// Every `/Contents <…>` hexadecimal string in a file that decodes to
/// something starting with a SEQUENCE tag.
///
/// A byte scan, sharing nothing with the reader, so that a blob the reader
/// cannot reach is still counted. `/Contents` is also an annotation's text
/// (12.5.2) and a page's content stream reference, so the filter is: a
/// hexadecimal string, at least 64 bytes decoded, and `0x30` first. Nothing
/// else about the bytes is assumed — whether they are a `ContentInfo` is the
/// parser's answer and not this function's.
fn contents_strings(bytes: &[u8]) -> Vec<Vec<u8>> {
    const KEY: &[u8] = b"/Contents";
    let mut out = Vec::new();
    let mut at = 0usize;
    while let Some(found) = bytes
        .get(at..)
        .and_then(|rest| rest.windows(KEY.len()).position(|w| w == KEY))
    {
        let after = at + found + KEY.len();
        at = after;
        let mut i = after;
        while bytes.get(i).is_some_and(u8::is_ascii_whitespace) {
            i += 1;
        }
        if bytes.get(i) != Some(&b'<') {
            continue;
        }
        i += 1;
        let mut nibbles: Vec<u8> = Vec::new();
        while let Some(byte) = bytes.get(i) {
            if *byte == b'>' {
                break;
            }
            if let Some(value) = (*byte as char).to_digit(16) {
                nibbles.push(value as u8);
            } else if !byte.is_ascii_whitespace() {
                nibbles.clear();
                break;
            }
            i += 1;
        }
        let decoded: Vec<u8> = nibbles
            .chunks_exact(2)
            .map(|pair| (pair[0] << 4) | pair[1])
            .collect();
        if decoded.len() >= 64 && decoded.first() == Some(&0x30) {
            out.push(decoded);
        }
    }
    out
}

/// The CMS object inside a `/Contents` string, without its padding.
///
/// 12.8.3.3 has a signer reserve space in `/Contents` before it knows how big
/// the signature will be, so what comes out is the DER followed by zero bytes
/// to the end of the reservation. Trimming trailing `0x00` would be a guess —
/// a DER object may legitimately end in one — so the length is read from the
/// object's own header with the crate's own walker, and the answer is exact.
///
/// A blob whose first node will not read at all is handed over whole, so that
/// the refusal reported is the parser's rather than this function's.
fn without_padding(contents: &[u8]) -> &[u8] {
    let budget = Budget::new(Limits::CMS);
    let mut cursor = Cursor::new(contents, &budget);
    match cursor.read() {
        Ok(node) => contents.get(..node.raw().len()).unwrap_or(contents),
        Err(_) => contents,
    }
}

// ---- what the blobs turned out to be --------------------------------------

/// Whether the outermost node's length octet is X.690 §8.1.3.6's `0x80`.
///
/// A byte look rather than a parse. `census_one` checks the crate's own
/// `Tlv::is_indefinite` against it on every blob, so the two readings hold
/// each other up instead of the parser vouching for itself.
fn opens_with_an_indefinite_length(blob: &[u8]) -> bool {
    blob.first() == Some(&0x30) && blob.get(1) == Some(&0x80)
}

#[derive(Default)]
struct Tally {
    blobs: usize,
    parsed: usize,
    /// Blobs whose outermost SEQUENCE is indefinite-length — BER, which
    /// RFC 5652 §5.1 permits and ISO 32000-1 12.8.3.3.1 does not.
    ber_blobs: Vec<String>,
    /// And how many of those blobs' signatures verified over the §5.4
    /// re-encoding, which is what says the walker located the right bytes.
    ber_verified: usize,
    refused: BTreeMap<String, usize>,
    detached: usize,
    encapsulating: usize,
    signers: usize,
    issuer_and_serial: usize,
    subject_key_identifier: usize,
    with_signed_attrs: usize,
    without_signed_attrs: usize,
    with_message_digest: usize,
    with_signing_time: usize,
    with_signing_certificate_v2: usize,
    content_type_agrees: usize,
    declares_its_own_digest: usize,
    timestamp_tokens: usize,
    nested_tokens_parsed: usize,
    crls: usize,
    signer_digests: BTreeMap<String, usize>,
    signer_signature_algorithms: BTreeMap<String, usize>,
    certificate_algorithms: BTreeMap<String, usize>,
    signed_attribute_types: BTreeMap<String, usize>,
    unsigned_attribute_types: BTreeMap<String, usize>,
    certificates_seen: usize,
    certificates_parsed: usize,
    certificate_failures: Vec<String>,
    non_x509_choices: usize,
    verified: usize,
    not_verified: Vec<String>,
    stored_tag_would_verify: usize,
    signer_certificate_not_found: usize,
    signer_key_not_rsa: usize,
}

fn bump(map: &mut BTreeMap<String, usize>, key: impl Into<String>) {
    *map.entry(key.into()).or_insert(0) += 1;
}

/// The refusal's *kind*, so the table groups rather than lists.
fn refusal_name(error: &CmsError) -> String {
    match error {
        CmsError::Der(inner) => format!("Der({inner:?})"),
        CmsError::TrailingBytes => "TrailingBytes".to_string(),
        CmsError::UnsupportedContentType { .. } => "UnsupportedContentType".to_string(),
        CmsError::UnknownSignerIdentifier { .. } => "UnknownSignerIdentifier".to_string(),
        CmsError::UnknownDigestAlgorithm { .. } => "UnknownDigestAlgorithm".to_string(),
        CmsError::UnknownSignatureAlgorithm { .. } => "UnknownSignatureAlgorithm".to_string(),
        CmsError::EmptyAttributes => "EmptyAttributes".to_string(),
        CmsError::IndefiniteSignedAttributes => "IndefiniteSignedAttributes".to_string(),
        CmsError::DuplicateAttribute { .. } => "DuplicateAttribute".to_string(),
        CmsError::AttributeValueCount { .. } => "AttributeValueCount".to_string(),
        CmsError::BadAttribute { .. } => "BadAttribute".to_string(),
    }
}

/// Reads one blob and adds everything it says to the tally.
///
/// `nested` is true for a timestamp token, which is a whole `ContentInfo`
/// inside an unsigned attribute: it is censused like any other, and counted
/// apart, so the headline numbers stay countable against the file list.
fn census_one(name: &str, blob: &[u8], tally: &mut Tally, nested: bool, verbose: bool) {
    if !nested {
        tally.blobs += 1;
    }
    let info = match ContentInfo::parse(blob) {
        Ok(info) => info,
        Err(error) => {
            if !nested {
                bump(&mut tally.refused, refusal_name(&error));
            }
            if verbose {
                println!("  {name}\n    REFUSED {error} [{error:?}]");
            }
            return;
        }
    };
    if !nested {
        tally.parsed += 1;
    } else {
        tally.nested_tokens_parsed += 1;
    }

    // Which encoding arrived, asked of the parser and of the bytes, so that
    // neither answer stands alone.
    let budget = Budget::new(Limits::CMS);
    let mut outermost = Cursor::new(blob, &budget);
    let ber = outermost.read().is_ok_and(|node| node.is_indefinite());
    assert_eq!(
        ber,
        opens_with_an_indefinite_length(blob),
        "{name}: the walker and the bytes disagree about the length form"
    );
    if ber && !nested {
        tally.ber_blobs.push(name.to_string());
    }

    let signed = info.signed_data();

    let encap = signed.encap_content_info();
    if encap.is_detached() {
        tally.detached += 1;
    } else {
        tally.encapsulating += 1;
    }
    tally.crls += signed.crls().len();

    // Every certificate in the set, through `x509.rs`. This is the first time
    // that parser has met a certificate this repository did not transcribe.
    let mut certificates: Vec<Certificate<'_>> = Vec::new();
    for (index, choice) in signed.certificates().iter().enumerate() {
        match choice {
            CertificateChoice::X509(der) => {
                tally.certificates_seen += 1;
                match Certificate::parse(der) {
                    Ok(certificate) => {
                        tally.certificates_parsed += 1;
                        bump(
                            &mut tally.certificate_algorithms,
                            certificate.signature_algorithm().oid().to_dotted(),
                        );
                        certificates.push(certificate);
                    }
                    Err(error) => tally
                        .certificate_failures
                        .push(format!("{name} certificate {index}: {error} [{error:?}]")),
                }
            }
            CertificateChoice::Other { tag, der } => {
                tally.non_x509_choices += 1;
                if verbose {
                    println!(
                        "  {name}\n    CertificateChoices [{tag}], {} bytes — not an X.509 \
                         certificate, so nothing here reads it",
                        der.len()
                    );
                }
            }
        }
    }

    if verbose {
        println!(
            "  {name}\n    v{} {} eContentType {} certs {} crls {} signers {}",
            signed.version(),
            if encap.is_detached() {
                "detached"
            } else {
                "encapsulating"
            },
            encap.content_type(),
            signed.certificates().len(),
            signed.crls().len(),
            signed.signer_infos().len(),
        );
    }

    for signer in signed.signer_infos() {
        tally.signers += 1;
        match signer.sid() {
            SignerIdentifier::IssuerAndSerialNumber { .. } => tally.issuer_and_serial += 1,
            SignerIdentifier::SubjectKeyIdentifier(_) => tally.subject_key_identifier += 1,
        }
        bump(
            &mut tally.signer_digests,
            signer.digest_algorithm_id().oid().to_dotted(),
        );
        bump(
            &mut tally.signer_signature_algorithms,
            signer.signature_algorithm_id().oid().to_dotted(),
        );
        if signed.declares(signer.digest_algorithm_id().oid()) {
            tally.declares_its_own_digest += 1;
        }
        if signed.content_type_matches(signer) == Some(true) {
            tally.content_type_agrees += 1;
        }

        match signer.signed_attrs() {
            None => tally.without_signed_attrs += 1,
            Some(attributes) => {
                tally.with_signed_attrs += 1;
                for attribute in attributes.all() {
                    bump(
                        &mut tally.signed_attribute_types,
                        attribute.oid().to_dotted(),
                    );
                }
            }
        }
        if let Some(attributes) = signer.unsigned_attrs() {
            for attribute in attributes.all() {
                bump(
                    &mut tally.unsigned_attribute_types,
                    attribute.oid().to_dotted(),
                );
            }
        }
        if signer.message_digest().is_some() {
            tally.with_message_digest += 1;
        }
        if signer.signing_time().is_some() {
            tally.with_signing_time += 1;
        }
        if signer.signing_certificate_v2().is_some() {
            tally.with_signing_certificate_v2 += 1;
        }

        if verbose {
            println!(
                "      signer v{} sid {} digest {} signature {} signedAttrs {} \
                 signature {} bytes",
                signer.version(),
                match signer.sid() {
                    SignerIdentifier::IssuerAndSerialNumber { .. } => "issuerAndSerial",
                    SignerIdentifier::SubjectKeyIdentifier(_) => "subjectKeyIdentifier",
                },
                signer.digest_algorithm_id().oid(),
                signer.signature_algorithm_id().oid(),
                signer.signed_attrs().map_or(0, |a| a.all().len()),
                signer.signature().len(),
            );
            if let Some(seconds) = signer.signing_time() {
                println!(
                    "        claimed signing time {seconds} (unix seconds, the signer's word \
                     and nothing more)"
                );
            }
            if let Some(attribute) = signer.signing_certificate_v2() {
                println!(
                    "        signingCertificateV2 over {} certificate(s)",
                    attribute.certs().len()
                );
            }
        }

        // ---- RFC 5652 §5.4, adjudicated by a real signature --------------
        if let Some(to_digest) = signer.signed_attrs_to_digest() {
            let stored = signer.signed_attrs().expect("there are some").stored_der();
            match (
                signer.effective_digest(),
                find_signer_certificate(signer, &certificates),
            ) {
                (Ok(algorithm), Some(certificate)) => match rsa_key(certificate) {
                    None => {
                        tally.signer_key_not_rsa += 1;
                        if verbose {
                            println!("        (the signer's key is not RSA; not verified here)");
                        }
                    }
                    Some(key) => {
                        let with_set_tag = key
                            .verify_pkcs1_v15_message(algorithm, &to_digest, signer.signature())
                            .is_ok();
                        let with_stored_tag = key
                            .verify_pkcs1_v15_message(algorithm, stored, signer.signature())
                            .is_ok();
                        if with_set_tag {
                            tally.verified += 1;
                            if ber {
                                tally.ber_verified += 1;
                            }
                        } else {
                            tally.not_verified.push(name.to_string());
                        }
                        if with_stored_tag {
                            tally.stored_tag_would_verify += 1;
                        }
                        if verbose {
                            println!(
                                "        §5.4 re-encoded SET OF: {:8}  stored [0] tag: {}",
                                if with_set_tag { "VERIFIES" } else { "no" },
                                if with_stored_tag { "VERIFIES" } else { "no" },
                            );
                        }
                    }
                },
                (Ok(_), None) => {
                    tally.signer_certificate_not_found += 1;
                    if verbose {
                        println!(
                            "        (no certificate in the set matches the signer identifier)"
                        );
                    }
                }
                (Err(error), _) => {
                    if verbose {
                        println!("        (the digest is not resolvable: {error})");
                    }
                }
            }
        }

        // A timestamp token is a `ContentInfo` of its own, so the parser reads
        // one with itself. Counted, and never evaluated.
        for token in signer.timestamp_tokens() {
            tally.timestamp_tokens += 1;
            if nested {
                continue;
            }
            if verbose {
                println!(
                    "        --- RFC 3161 timestamp token, {} bytes, read with this same parser",
                    token.len()
                );
            }
            census_one(
                &format!("{name} [timestamp token]"),
                without_padding(token),
                tally,
                true,
                verbose,
            );
        }
    }
}

/// The certificate a `SignerInfo` names, where the set holds it.
fn find_signer_certificate<'c, 'a>(
    signer: &SignerInfo<'a>,
    certificates: &'c [Certificate<'a>],
) -> Option<&'c Certificate<'a>> {
    match signer.sid() {
        SignerIdentifier::IssuerAndSerialNumber { issuer, serial, .. } => {
            certificates.iter().find(|certificate| {
                certificate.serial().as_bytes() == serial.as_bytes()
                    && certificate.issuer().matches(issuer)
            })
        }
        SignerIdentifier::SubjectKeyIdentifier(wanted) => certificates.iter().find(|certificate| {
            certificate
                .extensions()
                .subject_key_identifier()
                .is_some_and(|found| found == *wanted)
                || certificate.key_identifier_sha1() == *wanted
        }),
    }
}

fn rsa_key(certificate: &Certificate<'_>) -> Option<RsaPublicKey> {
    match certificate.subject_public_key_info().public_key() {
        PublicKey::Rsa { modulus, exponent } => RsaPublicKey::new(modulus, exponent).ok(),
        PublicKey::Ec { .. } | PublicKey::Unrecognised => None,
    }
}

fn table(title: &str, counts: &BTreeMap<String, usize>) {
    println!("{title}");
    if counts.is_empty() {
        println!("     0  (none)");
    }
    for (key, count) in counts {
        println!("  {count:4}  {key}");
    }
}

// ---- the census -----------------------------------------------------------

#[test]
#[ignore = "walks the fetched corpora; run with --ignored --nocapture"]
fn census_of_the_corpus_cms_blobs() {
    let Some(root) = corpus_root() else {
        println!("SKIPPED (no corpus; set TINKER_CORPUS)");
        return;
    };
    let candidates = files_naming_a_byte_range(&root);
    println!("RAN over {} files naming /ByteRange\n", candidates.len());

    // ---- pass 1: what the supported path delivers -------------------------
    let mut supported = Tally::default();
    let mut signatures_seen = 0usize;
    let mut signatures_with_no_bytes = Vec::new();
    let mut documents_with_a_signature = 0usize;

    for path in &candidates {
        let name = display_name(path, &root);
        let Ok(bytes) = std::fs::read(path) else {
            continue;
        };
        let Ok(document) = Document::open(bytes) else {
            continue;
        };
        // An encrypted document's object streams do not decompress until the
        // file key exists, so a signature inside one is invisible until then —
        // the trap `tests/signatures.rs` records.
        if document.is_encrypted() {
            let _ = document.authenticate("");
        }
        let signatures = document.signatures();
        if !signatures.is_empty() {
            documents_with_a_signature += 1;
        }
        for (index, signature) in signatures.iter().enumerate() {
            signatures_seen += 1;
            let blob = without_padding(&signature.contents);
            if blob.is_empty() {
                signatures_with_no_bytes.push(format!("{name} [{index}] {:?}", signature.coverage));
                continue;
            }
            census_one(
                &format!("{name} [{index}]"),
                blob,
                &mut supported,
                false,
                false,
            );
        }
    }

    println!("---- pass 1: through `Signature::contents` (the supported path) ----");
    println!("documents carrying at least one signature: {documents_with_a_signature}");
    println!("signatures found: {signatures_seen}");
    println!(
        "  {} handed the parser bytes — {} parsed, {} refused",
        supported.blobs,
        supported.parsed,
        supported.blobs - supported.parsed
    );
    println!(
        "  {} handed it nothing, because `/ByteRange` does not bracket `/Contents`:",
        signatures_with_no_bytes.len()
    );
    for missing in &signatures_with_no_bytes {
        println!("      {missing}");
    }
    table("  refusals, by kind:", &supported.refused);

    // ---- pass 2: everything the files carry -------------------------------
    println!("\n---- pass 2: every `/Contents <…>` in the same files, by byte scan ----");
    let mut all = Tally::default();
    for path in &candidates {
        let name = display_name(path, &root);
        let Ok(bytes) = std::fs::read(path) else {
            continue;
        };
        for (index, contents) in contents_strings(&bytes).into_iter().enumerate() {
            census_one(
                &format!("{name} [{index}]"),
                without_padding(&contents),
                &mut all,
                false,
                true,
            );
        }
    }

    println!("\n================ census ================");
    println!(
        "blobs offered {} — {} parsed, {} refused",
        all.blobs,
        all.parsed,
        all.blobs - all.parsed
    );
    table("refusals, by kind:", &all.refused);
    println!(
        "length form: {} definite throughout, {} opening with X.690 §8.1.3.6's \
         indefinite length",
        all.blobs - all.ber_blobs.len(),
        all.ber_blobs.len()
    );
    for name in &all.ber_blobs {
        println!("      BER  {name}");
    }
    println!(
        "  of those, {} verified over the §5.4 re-encoding",
        all.ber_verified
    );
    println!(
        "encapsulated content: {} detached, {} carrying content",
        all.detached, all.encapsulating
    );
    println!("signers: {}", all.signers);
    println!(
        "  identified by: {} issuerAndSerialNumber, {} subjectKeyIdentifier",
        all.issuer_and_serial, all.subject_key_identifier
    );
    println!(
        "  signed attributes: {} have them, {} sign the content directly",
        all.with_signed_attrs, all.without_signed_attrs
    );
    println!(
        "  named attributes: {} messageDigest, {} signingTime, {} signingCertificateV2",
        all.with_message_digest, all.with_signing_time, all.with_signing_certificate_v2
    );
    println!(
        "  §5.3 contentType agrees with eContentType: {} of {}",
        all.content_type_agrees, all.with_signed_attrs
    );
    println!(
        "  §5.1 digestAlgorithms lists the signer's own digest: {} of {}",
        all.declares_its_own_digest, all.signers
    );
    println!(
        "RFC 3161 timestamp tokens: {} found, {} read as a ContentInfo of their own",
        all.timestamp_tokens, all.nested_tokens_parsed
    );
    println!("embedded CRLs: {}", all.crls);
    table("signer digestAlgorithm, by OID:", &all.signer_digests);
    table(
        "signer signatureAlgorithm, by OID:",
        &all.signer_signature_algorithms,
    );
    table(
        "certificate signatureAlgorithm, by OID:",
        &all.certificate_algorithms,
    );
    table(
        "signed attribute types, by OID:",
        &all.signed_attribute_types,
    );
    table(
        "unsigned attribute types, by OID:",
        &all.unsigned_attribute_types,
    );
    println!(
        "certificates: {} X.509 offered, {} parsed by x509.rs, {} other CertificateChoices",
        all.certificates_seen, all.certificates_parsed, all.non_x509_choices
    );
    for failure in &all.certificate_failures {
        println!("  UNPARSED  {failure}");
    }
    println!(
        "RFC 5652 §5.4: {} signatures verified over the re-encoded SET OF, {} did not, \
         {} had no certificate in the set, {} had a non-RSA key",
        all.verified,
        all.not_verified.len(),
        all.signer_certificate_not_found,
        all.signer_key_not_rsa,
    );
    for failure in &all.not_verified {
        println!("  NOT VERIFIED  {failure}");
    }
    println!(
        "signatures that would have verified over the stored [0] tag: {}",
        all.stored_tag_would_verify
    );

    // ---- what is pinned ---------------------------------------------------
    //
    // Recorded against the corpora `corpus/corpora.lock` pins, so a shrinking
    // result cannot read as a passing one — the rule `tests/signatures.rs`
    // states, and the reason these are equalities rather than lower bounds.
    // Re-pinning a corpus moves them, and moving them is a deliberate act with
    // its own commit and its own reason. Every number was read off the run
    // that produced it and checked against the file's own bytes.

    assert!(
        candidates.len() >= 25,
        "the fetched corpora carried 25 files naming /ByteRange; found {}",
        candidates.len()
    );

    // Pass 1. The gap route reaches two thirds of them, and the third it does
    // not reach is a property of those files rather than of this parser.
    // **These numbers moved on 6 September 2026 because the corpus grew**, not
    // because anything here changed: `corpus/corpora.lock` gained a fifth
    // entry, the SafeDocs shard of a thousand documents off the open web, and
    // real documents carry real signatures. The four fixture corpora still
    // produce exactly the figures this file recorded before -- measured, by
    // moving the shard out of `corpus/files` and running this again -- so the
    // difference is nine signatures nobody wrote to test a reader.
    assert_eq!(
        signatures_seen, 27,
        "signatures, as `tests/signatures.rs` counts them"
    );
    assert_eq!(
        signatures_with_no_bytes.len(),
        6,
        "signatures whose `/ByteRange` gap holds no hexadecimal string, so \
         `Signature::contents` is empty: {signatures_with_no_bytes:?}"
    );
    assert_eq!(supported.blobs, 21, "blobs the supported path handed over");
    assert_eq!(
        supported.parsed, 21,
        "and parsed — all of them, since `Limits::CMS` reads BER"
    );
    assert!(
        supported.refused.is_empty(),
        "nothing on the supported path is refused any more: {:?}",
        supported.refused
    );

    // Pass 2. Twenty-seven blobs — one per signature, which is the agreement
    // between an independent byte scan and the reader's own field walk that
    // makes either number worth anything.
    assert_eq!(all.blobs, 27, "every `/Contents <…>` in the same files");
    assert_eq!(all.parsed, 27, "and every one of them parses");
    assert!(
        all.refused.is_empty(),
        "nothing in the corpus is refused: {:?}",
        all.refused
    );

    // The BER four, named rather than counted, because which files they are is
    // the fact a re-pinned corpus would move.
    assert_eq!(
        all.ber_blobs,
        vec![
            "pdfjs/test/pdfs/160F-2019.pdf [0]".to_string(),
            "pdfjs/test/pdfs/issue16553.pdf [0]".to_string(),
            "pdfjs/test/pdfs/prefilled_f1040.pdf [0]".to_string(),
            "pdfjs/test/pdfs/xfa_filled_imm1344e.pdf [1]".to_string(),
        ],
        "the blobs whose outermost SEQUENCE carries X.690 §8.1.3.6's \
         indefinite length — 4 of 27, from Acrobat Distiller 5.0.5, Adobe \
         LiveCycle Designer ES 8.2 and 10.0, and LibreOffice 7.5"
    );
    assert_eq!(
        all.ber_verified, 4,
        "**and each of the four verifies over the §5.4 re-encoding**, which is \
         what says the end-of-contents scan located the signer's bytes rather \
         than bytes that merely parsed"
    );

    assert_eq!(all.detached, 26, "detached SignedData, tokens included");
    assert_eq!(all.encapsulating, 8);
    assert_eq!(
        all.signers, 34,
        "one per blob, plus one per timestamp token"
    );
    assert_eq!(
        all.subject_key_identifier, 0,
        "no corpus blob identifies its signer by key identifier, which is why \
         that arm is held up by a fixture in `cms.rs` alone"
    );
    assert_eq!(all.issuer_and_serial, 34);
    assert_eq!(
        all.without_signed_attrs, 1,
        "`bug854315.pdf`'s outer signer has none at all, so §5.4's other half \
         is exercised by real data as well as by a fixture"
    );
    assert_eq!(all.with_signed_attrs, 33);
    assert_eq!(
        all.with_message_digest, 33,
        "§5.3 requires one where there are any"
    );
    assert_eq!(
        all.with_signing_certificate_v2, 10,
        "`issue16553.pdf`'s was the first — reachable only because that blob \
         is one of the four BER ones, so reading the form is what put real \
         evidence under the RFC 5035 decoder that had none. The other nine \
         came with the production corpus, where the attribute is ordinary"
    );
    assert_eq!(all.timestamp_tokens, 7);
    assert_eq!(
        all.nested_tokens_parsed, 7,
        "and each reads as a ContentInfo through this same parser"
    );
    assert_eq!(
        all.crls, 0,
        "no corpus blob embeds a CRL, so `RevocationChoice` is a fixture-only \
         path too"
    );
    assert_eq!(
        all.non_x509_choices, 2,
        "`bug854315.pdf`'s timestamp token carries a `[1]` extended \
         certificate, and the production corpus brought a second — real \
         evidence that arm needs to exist, from two independent sources"
    );

    // The certificate parser, on certificates nobody here transcribed.
    assert!(
        all.certificate_failures.is_empty(),
        "every corpus certificate must parse, or the failures must be named \
         here rather than tolerated: {:?}",
        all.certificate_failures
    );
    assert_eq!(all.certificates_seen, 71, "X.509 certificates offered");
    assert_eq!(
        all.certificates_parsed, 71,
        "and all of them parsed — under `Limits::CERTIFICATE`, which does not \
         allow the indefinite length, so the twelve that arrived inside a BER \
         message were still held to RFC 5280 §4.1's DER"
    );

    // The three assertions this file exists for.
    assert!(
        all.not_verified.is_empty(),
        "a signer whose signature does not verify over the re-encoded \
         attributes means the rule, the digest or the arithmetic is wrong: {:?}",
        all.not_verified
    );
    // **Thirty-three, and fourteen of them arrived with the production
    // corpus on 6 September 2026.** The four fixture corpora gave 19, from six
    // producers written to test a reader; these come from documents signed by
    // people, including qualified signatures issued under national schemes.
    // Every one verifies over the §5.4 re-encoding with this engine's own RSA,
    // and `not_verified` is still empty.
    assert_eq!(
        all.verified, 33,
        "real signatures verified over the §5.4 re-encoding using this \
         engine's own RSA"
    );
    assert_eq!(
        all.stored_tag_would_verify, 0,
        "not one verifies over the stored `[0]` bytes — which is exactly why \
         digesting them is a defect nothing else in the pipeline would report"
    );
}

fn display_name(path: &Path, root: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}
