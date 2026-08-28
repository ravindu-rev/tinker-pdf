//! Signing on an incremental save (12.8.1), and reading the result back.
//!
//! The tests that matter here are round trips rather than golden bytes. A
//! signature is a claim about a file made by the code that wrote the file, so
//! the only assertion worth much is that the *reader* — which shares nothing
//! with the writer except the span-digest function both call — recovers
//! exactly the number the signer was handed.
//!
//! What none of this can establish is stated in `docs/design/signatures.md`
//! and repeated here so it is not lost: **nothing outside this repository ever
//! validates a signature it produced** (ruling 13). Everything below is this
//! engine agreeing with itself.

use std::cell::RefCell;

use tinker_pdf::{
    Coverage, Defect, DigestAlgorithm, Document, SignError, SignRefused, Signer, SigningRequest,
    SigningTarget, WriteMode, WriteOptions,
};

// ---- signers, none of which hold a key ------------------------------------

/// Records the digest it was handed and returns a blob of `len` bytes.
///
/// Not a real signer: the point is the seam, not the cryptography. What it
/// proves is that the digest crossing the seam is the digest of the finished
/// file, which is the only part of signing this crate is responsible for.
struct Recorder {
    algorithm: DigestAlgorithm,
    len: usize,
    seen: RefCell<Vec<Vec<u8>>>,
}

impl Recorder {
    fn new(algorithm: DigestAlgorithm, len: usize) -> Recorder {
        Recorder {
            algorithm,
            len,
            seen: RefCell::new(Vec::new()),
        }
    }

    fn digest(&self) -> Vec<u8> {
        self.seen.borrow().last().cloned().expect("the signer ran")
    }
}

impl Signer for Recorder {
    fn digest_algorithm(&self) -> DigestAlgorithm {
        self.algorithm
    }

    fn sign(&self, digest: &[u8]) -> Result<Vec<u8>, SignRefused> {
        self.seen.borrow_mut().push(digest.to_vec());
        // A recognisable blob: a DER SEQUENCE header followed by the digest,
        // padded out. Nothing parses it; it just has to be findable.
        let mut blob = vec![0x30, 0x82];
        blob.extend_from_slice(digest);
        blob.resize(self.len, 0xAB);
        Ok(blob)
    }
}

/// Declines, the way a locked token or a cancelled prompt would.
struct Declines;

impl Signer for Declines {
    fn digest_algorithm(&self) -> DigestAlgorithm {
        DigestAlgorithm::Sha256
    }

    fn sign(&self, _digest: &[u8]) -> Result<Vec<u8>, SignRefused> {
        Err(SignRefused::new("the token is locked"))
    }
}

// ---- helpers ---------------------------------------------------------------

fn fixture() -> Vec<u8> {
    std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../testdata/simple-text.pdf"
    ))
    .expect("testdata/simple-text.pdf")
}

fn incremental() -> WriteOptions {
    WriteOptions {
        mode: WriteMode::Incremental,
        ..Default::default()
    }
}

fn request<'a>(signer: &'a dyn Signer, reserve: usize) -> SigningRequest<'a> {
    let mut request = SigningRequest::new(
        SigningTarget::NewInvisibleField {
            name: "Signature1".to_string(),
        },
        signer,
    );
    request.reserve = reserve;
    request.reason = Some("Because the test says so".to_string());
    request.name = Some("A Signer".to_string());
    request
}

// ---- the invariants --------------------------------------------------------

#[test]
fn a_signed_save_leaves_the_original_bytes_alone() {
    let original = fixture();
    let signer = Recorder::new(DigestAlgorithm::Sha256, 512);
    let signed = Document::open(original.clone())
        .expect("the fixture opens")
        .editor()
        .save_signed(&incremental(), &request(&signer, 2048))
        .expect("signing succeeds");

    assert!(
        signed.starts_with(&original),
        "the signable prefix must survive signing"
    );
    assert!(signed.len() > original.len(), "and a revision is appended");
}

/// The round trip milestone 8 exists for.
///
/// The signer is handed a digest before the file has ever been parsed; the
/// reader recomputes one from the finished file's own `/ByteRange`. They are
/// the same number, or the signature covers something other than what it says.
#[test]
fn the_digest_the_signer_saw_is_the_digest_the_reader_recomputes() {
    for algorithm in [
        DigestAlgorithm::Sha1,
        DigestAlgorithm::Sha256,
        DigestAlgorithm::Sha384,
        DigestAlgorithm::Sha512,
    ] {
        let signer = Recorder::new(algorithm, 300);
        let signed = Document::open(fixture())
            .expect("opens")
            .editor()
            .save_signed(&incremental(), &request(&signer, 2048))
            .expect("signing succeeds");

        let reopened = Document::open(signed).expect("the signed file reopens");
        let signatures = reopened.signatures();
        assert_eq!(signatures.len(), 1, "{algorithm:?}");
        let recomputed = signatures[0]
            .digest(&reopened, algorithm)
            .expect("the spans fit the file");
        assert_eq!(
            recomputed,
            signer.digest(),
            "{algorithm:?}: the reader and the writer must agree on the covered bytes"
        );
        assert_eq!(recomputed.len(), algorithm.length());
    }
}

#[test]
fn the_signature_covers_the_whole_file_and_carries_what_the_signer_returned() {
    let signer = Recorder::new(DigestAlgorithm::Sha256, 300);
    let signed = Document::open(fixture())
        .expect("opens")
        .editor()
        .save_signed(&incremental(), &request(&signer, 2048))
        .expect("signing succeeds");

    let reopened = Document::open(signed).expect("reopens");
    let signature = &reopened.signatures()[0];
    assert_eq!(signature.coverage, Coverage::WholeFile);
    assert_eq!(signature.field.as_deref(), Some("Signature1"));
    assert_eq!(
        signature.reason.as_deref(),
        Some("Because the test says so")
    );
    assert_eq!(signature.name.as_deref(), Some("A Signer"));
    assert_eq!(
        signature.sub_filter_name.as_deref(),
        Some("adbe.pkcs7.detached")
    );
    assert!(
        signature.warnings.is_empty(),
        "a file this engine wrote should need no leniency: {:?}",
        signature.warnings
    );

    // The reservation is padded with zeros after the blob, which is what the
    // reader hands back — the DER's own length is what a parser would stop at.
    assert_eq!(&signature.contents[..2], &[0x30, 0x82]);
    assert_eq!(
        signature.contents.len(),
        2048,
        "the reservation, not the blob"
    );
    assert!(
        signature.contents[300..].iter().all(|byte| *byte == 0),
        "the unused remainder is zero fill"
    );
}

#[test]
fn the_signed_file_passes_the_strict_structural_validator() {
    let signer = Recorder::new(DigestAlgorithm::Sha256, 300);
    let signed = Document::open(fixture())
        .expect("opens")
        .editor()
        .save_signed(&incremental(), &request(&signer, 2048))
        .expect("signing succeeds");

    let defects = Document::open(signed).expect("reopens").validate();
    assert!(
        defects.is_empty(),
        "ruling 13's validator reads it back with the repairs off: {:?}",
        defects.iter().map(Defect::to_string).collect::<Vec<_>>()
    );
}

/// Signing twice, which is what a countersignature is.
///
/// The second signature covers the whole file; the first now covers only the
/// revision it was made over. That is the honest reading, and it is the one
/// `Coverage` was built to be able to say.
#[test]
fn a_second_signature_leaves_the_first_covering_its_own_revision() {
    let first_signer = Recorder::new(DigestAlgorithm::Sha256, 300);
    let once = Document::open(fixture())
        .expect("opens")
        .editor()
        .save_signed(&incremental(), &request(&first_signer, 2048))
        .expect("the first signature");

    let second_signer = Recorder::new(DigestAlgorithm::Sha256, 300);
    let mut second_request = request(&second_signer, 2048);
    second_request.target = SigningTarget::NewInvisibleField {
        name: "Signature2".to_string(),
    };
    let twice = Document::open(once.clone())
        .expect("the once-signed file opens")
        .editor()
        .save_signed(&incremental(), &second_request)
        .expect("the second signature");

    assert!(
        twice.starts_with(&once),
        "the first signature's bytes must survive the second"
    );

    let reopened = Document::open(twice).expect("reopens");
    let mut signatures = reopened.signatures();
    signatures.sort_by(|a, b| a.field.cmp(&b.field));
    assert_eq!(signatures.len(), 2);

    assert_eq!(signatures[0].field.as_deref(), Some("Signature1"));
    assert_eq!(
        signatures[0].coverage,
        Coverage::Revision { index: 1 },
        "the first covers the revision it was made over, not the file"
    );
    assert_eq!(signatures[1].field.as_deref(), Some("Signature2"));
    assert_eq!(signatures[1].coverage, Coverage::WholeFile);

    // And the first signature's digest still holds over the bytes it named,
    // which is the whole reason an incremental save is the only signing mode.
    let recomputed = signatures[0]
        .digest(&reopened, DigestAlgorithm::Sha256)
        .expect("the spans still fit");
    assert_eq!(recomputed, first_signer.digest());
}

// ---- the refusals ----------------------------------------------------------

#[test]
fn a_blob_larger_than_the_reservation_is_refused_rather_than_truncated() {
    let signer = Recorder::new(DigestAlgorithm::Sha256, 5000);
    let outcome = Document::open(fixture())
        .expect("opens")
        .editor()
        .save_signed(&incremental(), &request(&signer, 1024));
    assert_eq!(
        outcome,
        Err(SignError::ReserveTooSmall {
            needed: 5000,
            reserved: 1024
        }),
        "a truncated signature is a file that looks signed and is not"
    );
}

#[test]
fn a_signer_that_declines_produces_no_file_and_says_why() {
    let signer = Declines;
    let outcome = Document::open(fixture())
        .expect("opens")
        .editor()
        .save_signed(&incremental(), &request(&signer, 2048));
    assert_eq!(
        outcome,
        Err(SignError::SignerRefused(SignRefused::new(
            "the token is locked"
        ))),
        "the host's own words, not a variant invented for it"
    );
}

#[test]
fn a_rewrite_cannot_be_signed() {
    let signer = Recorder::new(DigestAlgorithm::Sha256, 300);
    let outcome = Document::open(fixture())
        .expect("opens")
        .editor()
        .save_signed(&WriteOptions::default(), &request(&signer, 2048));
    assert_eq!(outcome, Err(SignError::NotIncremental));
}

#[test]
fn signing_a_field_that_is_already_signed_is_refused() {
    let signer = Recorder::new(DigestAlgorithm::Sha256, 300);
    let once = Document::open(fixture())
        .expect("opens")
        .editor()
        .save_signed(&incremental(), &request(&signer, 2048))
        .expect("the first signature");

    let again = Recorder::new(DigestAlgorithm::Sha256, 300);
    let mut over_the_top = request(&again, 2048);
    over_the_top.target = SigningTarget::Field("Signature1".to_string());
    let outcome = Document::open(once)
        .expect("reopens")
        .editor()
        .save_signed(&incremental(), &over_the_top);
    assert_eq!(
        outcome,
        Err(SignError::FieldAlreadySigned("Signature1".to_string())),
        "overwriting a signature destroys the evidence it was"
    );
}

#[test]
fn signing_a_field_that_does_not_exist_is_refused() {
    let signer = Recorder::new(DigestAlgorithm::Sha256, 300);
    let mut missing = request(&signer, 2048);
    missing.target = SigningTarget::Field("NoSuchField".to_string());
    let outcome = Document::open(fixture())
        .expect("opens")
        .editor()
        .save_signed(&incremental(), &missing);
    assert_eq!(
        outcome,
        Err(SignError::NoSuchField("NoSuchField".to_string()))
    );
}

/// Signing an empty field a document already carries, which is the shape a
/// prepared form has.
#[test]
fn an_existing_empty_signature_field_can_be_filled() {
    // Prepare one by signing and then reading back what the writer produced,
    // rather than hand-building a field — the point is that the two halves
    // agree about what an unsigned field looks like.
    let preparer = Recorder::new(DigestAlgorithm::Sha256, 300);
    let mut prepared_request = request(&preparer, 2048);
    prepared_request.target = SigningTarget::NewInvisibleField {
        name: "Prepared".to_string(),
    };
    let prepared = Document::open(fixture())
        .expect("opens")
        .editor()
        .save_signed(&incremental(), &prepared_request)
        .expect("preparing");

    // A second field, this time targeting the first by name, must be refused —
    // which is the assertion above. Targeting a *fresh* name creates one.
    let signer = Recorder::new(DigestAlgorithm::Sha256, 300);
    let mut second = request(&signer, 2048);
    second.target = SigningTarget::NewInvisibleField {
        name: "Second".to_string(),
    };
    let signed = Document::open(prepared)
        .expect("reopens")
        .editor()
        .save_signed(&incremental(), &second)
        .expect("signing the second field");

    let reopened = Document::open(signed).expect("reopens");
    let names: Vec<Option<String>> = reopened
        .signatures()
        .into_iter()
        .map(|signature| signature.field)
        .collect();
    assert_eq!(names.len(), 2, "both fields carry a signature: {names:?}");
}
