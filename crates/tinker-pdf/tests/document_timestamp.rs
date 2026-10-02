//! Document timestamps (ISO 32000-2 12.8.5): written through the
//! [`Timestamper`] seam, and read back and validated, each held to a token a
//! second implementation made.
//!
//! # Which link is adjudicated by what
//!
//! **The tokens are OpenSSL's.** OpenSSL 3.0.13's own timestamping authority
//! (`openssl ts -reply`) made both, on 2 October 2026, under a root of its
//! own (`signature_support/README.md`): the `TSTInfo`, the ESS
//! `signingCertificateV2`, the signature and the certificates. A reading of
//! any of them that disagreed with OpenSSL's would stop the token validating.
//!
//! **`document-timestamp.pdf` is the fixture script's layout around one**,
//! the same split as `signature_shapes.rs`: the script and
//! `crates/tinker-pdf/src/signature.rs` are one author's reading of 12.8.1's
//! `/ByteRange`, twice.
//!
//! **The engine-written one is this engine's layout around one**, which is
//! the stronger half. `save_timestamped` laid the file out and handed its
//! [`Timestamper`] a digest; that digest went to OpenSSL as a `TimeStampReq`
//! once, and the token that came back is committed. The writer is
//! deterministic (ruling 4), so every run hands the timestamper the same
//! digest — and if a change to the writer ever moves it, the first test here
//! says so in as many words, with the command that makes a new token.

use std::cell::RefCell;
use std::ops::Range;

use tinker_pdf::{
    AuthorityCertificate, Chain, CmsState, Coverage, DigestAlgorithm, Document, DocumentDigest,
    SignError, SignRefused, SignatureAppearance, SignatureCheck, SigningTarget, Stamped, SubFilter,
    TimestampRequest, Timestamper, TrustAnchors, Verdict, WriteMode, WriteOptions,
};
use tinker_pdf_pki::TimeStampToken;

const STAMPED_PDF: &[u8] = include_bytes!("signature_support/document-timestamp.pdf");
const STAMPED_TSA_ROOT: &[u8] = include_bytes!("signature_support/document-timestamp-tsa-root.der");

/// The fixture's `genTime`, as `openssl ts -reply -text` printed it:
/// `Oct  2 09:58:34 2026 GMT`.
const STAMPED_AT: i64 = 1_790_935_114;

/// The document the engine timestamps: already signed, so the timestamp is
/// laid over a signature the way a long-term archive lays one.
const BASE: &[u8] = include_bytes!("signature_support/no-signed-attributes.pdf");
const BASE_ROOT: &[u8] = include_bytes!("signature_support/no-signed-attributes-root.der");
const ENGINE_TOKEN: &[u8] = include_bytes!("signature_support/engine-timestamp-token.der");
const ENGINE_TSA_ROOT: &[u8] = include_bytes!("signature_support/engine-timestamp-tsa-root.der");

/// Inside every certificate's validity window: 1 January 2027.
const AT: i64 = 1_798_761_600;

// ---- reading one ----------------------------------------------------------

#[test]
fn a_document_timestamp_reaches_every_one_of_the_four_answers() {
    let document = Document::open(STAMPED_PDF.to_vec()).expect("the fixture opens");
    let signature = &document.signatures()[0];
    assert_eq!(signature.sub_filter, Some(SubFilter::EtsiRfc3161));
    assert_eq!(signature.coverage, Coverage::WholeFile);

    let verdict = verdict_with(STAMPED_PDF, &[STAMPED_TSA_ROOT]);
    assert_eq!(verdict.cms, CmsState::Read { signers: 1 });
    assert_eq!(
        verdict.document_digest,
        DocumentDigest::Matches,
        "the token's imprint is SHA-256 of the covered bytes"
    );
    assert_eq!(verdict.signature, SignatureCheck::Verified);
    match &verdict.chain {
        Chain::AnchoredTo { anchor, .. } => assert!(anchor.contains("Timestamp Root"), "{anchor}"),
        other => panic!("expected the authority's chain to reach its root, got {other:?}"),
    }
    assert!(
        verdict
            .signer
            .as_ref()
            .is_some_and(|signer| signer.subject.contains("Timestamping Authority")),
        "the signer a document timestamp names is its authority: {:?}",
        verdict.signer
    );

    assert_eq!(verdict.timestamps.len(), 1);
    let stamp = &verdict.timestamps[0];
    assert_eq!(stamp.stamps, Stamped::Document);
    assert_eq!(stamp.time, Some(STAMPED_AT));
    assert_eq!(
        stamp.authority_certificate,
        AuthorityCertificate::Fit,
        "named by RFC 5816's signingCertificateV2 this time"
    );
    assert!(stamp.is_trusted());
    assert!(verdict.is_trusted(), "{verdict:?}");
}

#[test]
fn the_token_names_its_certificate_with_the_second_ess_version() {
    let document = Document::open(STAMPED_PDF.to_vec()).expect("opens");
    let signatures = document.signatures();
    let token = TimeStampToken::parse(signatures[0].cms()).expect("a token");
    let signer = token
        .content_info()
        .signed_data()
        .signer_infos()
        .first()
        .expect("one signer")
        .clone();
    assert!(signer.signing_certificate().is_none());
    let ess = signer
        .signing_certificate_v2()
        .expect("ess_cert_id_alg = sha256 writes the second version");
    assert_eq!(
        ess.certs().first().map(|id| id.digest()),
        Some(Ok(tinker_pdf_pki::DigestAlgorithm::Sha256))
    );
}

#[test]
fn a_changed_document_is_not_what_the_authority_stamped() {
    let mut tampered = STAMPED_PDF.to_vec();
    let at = find(&tampered, b"0.2 0.6 0.3 rg").expect("the content stream");
    tampered[at + 2] = b'9';
    let verdict = verdict_with(&tampered, &[STAMPED_TSA_ROOT]);
    assert_eq!(verdict.document_digest, DocumentDigest::Differs);
    assert_eq!(verdict.timestamps[0].imprint, DocumentDigest::Differs);
    assert_eq!(
        verdict.signature,
        SignatureCheck::Verified,
        "the token itself is intact"
    );
    assert!(!verdict.is_trusted());
}

#[test]
fn an_authority_nobody_anchored_proves_no_time() {
    let verdict = verdict_with(STAMPED_PDF, &[]);
    assert_eq!(verdict.chain, Chain::NoAnchors);
    assert!(!verdict.is_trusted());

    let verdict = verdict_with(STAMPED_PDF, &[BASE_ROOT]);
    assert!(matches!(verdict.chain, Chain::SelfSigned { .. }));
    assert!(!verdict.is_trusted());
}

#[test]
fn a_flipped_bit_in_the_authoritys_signature_fails_the_timestamp() {
    let document = Document::open(STAMPED_PDF.to_vec()).expect("opens");
    let mut der = document.signatures()[0].cms().to_vec();
    let at = {
        let token = TimeStampToken::parse(&der).expect("a token");
        let signer = token
            .content_info()
            .signed_data()
            .signer_infos()
            .first()
            .expect("one signer")
            .clone();
        offset_in(&der, signer.signature())
    };
    der[at.end - 1] ^= 0x01;
    let verdict = verdict_with(&replace_contents(STAMPED_PDF, &der), &[STAMPED_TSA_ROOT]);
    assert_eq!(verdict.document_digest, DocumentDigest::Matches);
    assert_eq!(verdict.signature, SignatureCheck::Failed);
    assert!(!verdict.is_trusted());
}

// ---- writing one -----------------------------------------------------------

/// A [`Timestamper`] that answers with the committed token and remembers the
/// digest it was asked to stamp.
#[derive(Default)]
struct Committed {
    asked: RefCell<Option<Vec<u8>>>,
}

impl Timestamper for Committed {
    fn digest_algorithm(&self) -> DigestAlgorithm {
        DigestAlgorithm::Sha256
    }

    fn timestamp(&self, digest: &[u8]) -> Result<Vec<u8>, SignRefused> {
        *self.asked.borrow_mut() = Some(digest.to_vec());
        Ok(ENGINE_TOKEN.to_vec())
    }
}

fn incremental() -> WriteOptions {
    WriteOptions {
        mode: WriteMode::Incremental,
        ..WriteOptions::default()
    }
}

/// `BASE` with a document timestamp added, and the digest the engine handed
/// its timestamper.
fn engine_timestamped() -> (Vec<u8>, Vec<u8>) {
    let document = Document::open(BASE.to_vec()).expect("the base opens");
    let mut editor = document.editor();
    let stamper = Committed::default();
    let mut request = TimestampRequest::new(
        SigningTarget::NewInvisibleField {
            name: "DocumentTimestamp".into(),
        },
        &stamper,
    );
    request.reserve = 8192;
    let out = editor
        .save_timestamped(&incremental(), &request)
        .expect("the timestamp is written");
    let asked = stamper
        .asked
        .borrow()
        .clone()
        .expect("the timestamper was asked");
    (out, asked)
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[test]
fn the_digest_the_engine_hands_its_timestamper_is_the_one_the_committed_token_stamps() {
    let (_, asked) = engine_timestamped();
    let token = TimeStampToken::parse(ENGINE_TOKEN).expect("the committed token parses");
    assert_eq!(
        hex(&asked),
        hex(token.info().imprint()),
        "the writer's output for this input changed, so the committed token stamps \
         other bytes. If the change was meant, make a new token over the digest on \
         the left: python3 crates/tinker-pdf/tests/signature_support/signature-fixtures.py \
         crates/tinker-pdf/tests/signature_support <work> engine-timestamp=<digest>"
    );
}

#[test]
fn a_document_timestamp_this_engine_wrote_validates_against_a_real_token() {
    let (out, _) = engine_timestamped();
    assert!(
        out.starts_with(BASE),
        "an incremental save keeps the signed prefix"
    );
    assert!(
        find(
            &out,
            b"/Type /DocTimeStamp /Filter /Adobe.PPKLite /SubFilter /ETSI.RFC3161"
        )
        .is_some(),
        "the dictionary 12.8.5 describes"
    );

    let document = Document::open(out.clone()).expect("the output opens");
    let signatures = document.signatures();
    assert_eq!(signatures.len(), 2, "the signature and the timestamp");
    assert_eq!(signatures[1].field.as_deref(), Some("DocumentTimestamp"));

    let mut anchors = TrustAnchors::new();
    anchors.add(BASE_ROOT.to_vec()).expect("parses");
    anchors.add(ENGINE_TSA_ROOT.to_vec()).expect("parses");
    let verdicts = document.verify_signatures(&anchors, Some(AT));

    // The signature the timestamp was laid over: now over a revision, and
    // still exactly what it was.
    let signed = &verdicts[0];
    assert!(
        matches!(signed.coverage, Coverage::Revision { .. }),
        "{:?}",
        signed.coverage
    );
    assert_eq!(signed.signature, SignatureCheck::Verified);
    assert_eq!(signed.document_digest, DocumentDigest::Matches);

    // The timestamp, over everything including that signature.
    let stamped = &verdicts[1];
    assert_eq!(stamped.coverage, Coverage::WholeFile);
    assert_eq!(stamped.document_digest, DocumentDigest::Matches);
    assert_eq!(stamped.signature, SignatureCheck::Verified);
    assert!(
        matches!(stamped.chain, Chain::AnchoredTo { .. }),
        "{:?}",
        stamped.chain
    );
    assert_eq!(stamped.timestamps[0].stamps, Stamped::Document);
    assert_eq!(
        stamped.timestamps[0].authority_certificate,
        AuthorityCertificate::Fit
    );
    assert!(stamped.is_trusted(), "{stamped:?}");

    // The strict structural validator reads it back with the repairs off and
    // finds nothing the update added: the base's own one defect — the fixture
    // script writes no binary comment after the header — is all there is.
    let rendered = |pdf: &[u8]| {
        Document::open(pdf.to_vec())
            .expect("opens")
            .validate()
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
    };
    assert_eq!(rendered(&out), rendered(BASE));
    assert_eq!(rendered(BASE).len(), 1, "{:?}", rendered(BASE));

    // And the reader agrees with the writer about what was covered: the
    // same spans, digested again, are the imprint.
    let token = TimeStampToken::parse(ENGINE_TOKEN).expect("parses");
    assert_eq!(
        signatures[1]
            .digest(&document, DigestAlgorithm::Sha256)
            .as_deref(),
        Some(token.info().imprint())
    );
}

#[test]
fn a_document_timestamp_is_never_drawn() {
    let document = Document::open(BASE.to_vec()).expect("opens");
    let mut editor = document.editor();
    let stamper = Committed::default();
    let request = TimestampRequest::new(
        SigningTarget::NewVisibleField {
            name: "Seal".into(),
            page: 0,
            rect: tinker_pdf::Rect {
                x0: 10.0,
                y0: 10.0,
                x1: 100.0,
                y1: 50.0,
            },
            appearance: SignatureAppearance::new(),
        },
        &stamper,
    );
    assert_eq!(
        editor.save_timestamped(&incremental(), &request),
        Err(SignError::VisibleTimestamp)
    );
    assert!(
        stamper.asked.borrow().is_none(),
        "refused before anything was laid out"
    );
}

#[test]
fn a_rewrite_cannot_carry_a_timestamp_and_a_refusal_is_carried_verbatim() {
    let document = Document::open(BASE.to_vec()).expect("opens");
    let stamper = Committed::default();
    let request = TimestampRequest::new(
        SigningTarget::NewInvisibleField {
            name: "DocumentTimestamp".into(),
        },
        &stamper,
    );
    assert_eq!(
        document
            .editor()
            .save_timestamped(&WriteOptions::default(), &request),
        Err(SignError::NotIncremental)
    );

    struct Unreachable;
    impl Timestamper for Unreachable {
        fn digest_algorithm(&self) -> DigestAlgorithm {
            DigestAlgorithm::Sha256
        }
        fn timestamp(&self, _digest: &[u8]) -> Result<Vec<u8>, SignRefused> {
            Err(SignRefused::new("the authority did not answer"))
        }
    }
    let unreachable = Unreachable;
    let request = TimestampRequest::new(
        SigningTarget::NewInvisibleField {
            name: "DocumentTimestamp".into(),
        },
        &unreachable,
    );
    assert_eq!(
        document.editor().save_timestamped(&incremental(), &request),
        Err(SignError::SignerRefused(SignRefused::new(
            "the authority did not answer"
        )))
    );

    // A token larger than its reservation is refused, never truncated.
    let mut small = TimestampRequest::new(
        SigningTarget::NewInvisibleField {
            name: "DocumentTimestamp".into(),
        },
        &stamper,
    );
    small.reserve = 64;
    assert!(matches!(
        document.editor().save_timestamped(&incremental(), &small),
        Err(SignError::ReserveTooSmall { reserved: 64, .. })
    ));
}

// ---- helpers ----------------------------------------------------------------

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

/// The same document with a different blob in `/Contents`, in the same
/// reservation.
fn replace_contents(pdf: &[u8], der: &[u8]) -> Vec<u8> {
    let start = find(pdf, b"/Contents <").expect("the fixture has one") + b"/Contents <".len();
    let end = start + find(&pdf[start..], b">").expect("it is closed");
    let mut text = hex(der).to_uppercase();
    assert!(text.len() <= end - start, "the reservation holds it");
    while text.len() < end - start {
        text.push('0');
    }
    let mut out = pdf.to_vec();
    out[start..end].copy_from_slice(text.as_bytes());
    out
}

fn offset_in(whole: &[u8], slice: &[u8]) -> Range<usize> {
    let at = slice.as_ptr() as usize - whole.as_ptr() as usize;
    at..at + slice.len()
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len())
        .position(|window| window == needle)
}
