//! Signing on an incremental save (12.8.1).
//!
//! A signature is a chicken-and-egg problem written into a file format: the
//! `/Contents` string holds a signature over bytes that include the string's
//! own surroundings, so it cannot be written until the file is finished and
//! the file cannot be finished until it is written. 12.8.1's answer is to
//! reserve the space, finish the file, then patch the reservation without
//! changing its length — which is why [`crate::write::SignaturePlaceholder`]
//! records four offsets rather than a value.
//!
//! # What this module refuses to hold
//!
//! **Key material never enters the engine.** A [`Signer`] is handed a digest
//! and returns finished CMS bytes; it is the same inversion as `FontProvider`
//! and `EntropySource`, and it is what keeps PKCS#8 and PKCS#12 parsing,
//! private-key arithmetic and passphrase handling outside a crate whose job is
//! file syntax.
//!
//! **The clock.** `/M` is a parameter, not a reading. Ruling 4 bans a clock
//! from this engine's output, and a signing time is the one field where that
//! is not merely a determinism rule: a time the engine invented is a claim it
//! is not entitled to make.
//!
//! # One digest, computed once
//!
//! [`digest_spans`] is the only implementation of "what a `/ByteRange`
//! covers" in the workspace. The reader calls it to recompute what a signature
//! covered and the writer calls it to produce what the signer signs, so the
//! two cannot drift — which is the failure mode a second implementation would
//! have, invisibly, in exactly one direction.

use std::ops::Range;

use tinker_pdf_crypto::{sha1, sha2};

use crate::text_string::Date;

/// Which digest reduces a signature's covered bytes.
///
/// The set is what the corpus asks for: of the seventeen CMS blobs in the
/// fetched corpora, twelve sign with SHA-1, eleven with SHA-256 and one each
/// with SHA-384 and SHA-512.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DigestAlgorithm {
    /// SHA-1 (FIPS 180-4). Read because real documents use it, and reported as
    /// weak wherever a verdict is formed.
    Sha1,
    /// SHA-256.
    Sha256,
    /// SHA-384.
    Sha384,
    /// SHA-512.
    Sha512,
}

impl DigestAlgorithm {
    /// How many bytes the digest occupies.
    #[must_use]
    pub fn length(self) -> usize {
        match self {
            DigestAlgorithm::Sha1 => 20,
            DigestAlgorithm::Sha256 => 32,
            DigestAlgorithm::Sha384 => 48,
            DigestAlgorithm::Sha512 => 64,
        }
    }
}

/// The digest of `spans` of `bytes`, or `None` if any span falls outside them.
///
/// Returning `None` rather than digesting what fits is the whole point: a
/// digest over less than a signature covers is a number that looks like an
/// answer and is not one.
#[must_use]
pub fn digest_spans(
    bytes: &[u8],
    spans: &[Range<u64>],
    algorithm: DigestAlgorithm,
) -> Option<Vec<u8>> {
    if spans.is_empty() {
        return None;
    }
    // Collect before hashing, so a span that does not fit is discovered before
    // any of the document has been fed to the hasher.
    let mut pieces: Vec<&[u8]> = Vec::with_capacity(spans.len());
    for span in spans {
        let start = usize::try_from(span.start).ok()?;
        let end = usize::try_from(span.end).ok()?;
        pieces.push(bytes.get(start..end)?);
    }

    Some(match algorithm {
        DigestAlgorithm::Sha1 => {
            let mut hasher = sha1::Sha1::new();
            for piece in pieces {
                hasher.update(piece);
            }
            hasher.finish().to_vec()
        }
        DigestAlgorithm::Sha256 => {
            let mut hasher = sha2::Sha256::new();
            for piece in pieces {
                hasher.update(piece);
            }
            hasher.finish().to_vec()
        }
        DigestAlgorithm::Sha384 => {
            let mut hasher = sha2::Sha512::new_384();
            for piece in pieces {
                hasher.update(piece);
            }
            hasher.finish()[..48].to_vec()
        }
        DigestAlgorithm::Sha512 => {
            let mut hasher = sha2::Sha512::new();
            for piece in pieces {
                hasher.update(piece);
            }
            hasher.finish().to_vec()
        }
    })
}

/// Why a [`Signer`] declined to sign.
///
/// A refusal is not an engine error — the host said no, for a reason only the
/// host knows (a locked token, a cancelled prompt, an expired certificate) —
/// so it carries the host's own words rather than a variant this crate
/// invented for it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SignRefused {
    /// What the host wants the caller told.
    pub reason: String,
}

impl SignRefused {
    /// A refusal with `reason` as its explanation.
    pub fn new(reason: impl Into<String>) -> SignRefused {
        SignRefused {
            reason: reason.into(),
        }
    }
}

/// The host's signing key, held by the host.
///
/// The engine hands over a digest and receives finished CMS bytes: for
/// `adbe.pkcs7.detached` that is a DER `SignedData` with no encapsulated
/// content whose `messageDigest` signed attribute is the digest given here.
/// The engine never sees a private key, never parses one, and never needs a
/// random number to do its half.
pub trait Signer {
    /// Which digest the host wants the covered bytes reduced with.
    ///
    /// Asked before the file is laid out, because it decides nothing about the
    /// layout — it is asked early so a host that cannot sign at all can say so
    /// before a document is serialised.
    fn digest_algorithm(&self) -> DigestAlgorithm;

    /// The CMS blob over `digest`, or the host's refusal.
    ///
    /// # Errors
    /// Whatever the host decides, carried verbatim.
    fn sign(&self, digest: &[u8]) -> Result<Vec<u8>, SignRefused>;
}

/// Where the signature goes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SigningTarget {
    /// An existing, empty `/FT /Sig` field, by its fully qualified name.
    ///
    /// Refused if the field is already signed, because overwriting a signature
    /// destroys evidence and the caller almost certainly meant to add one.
    Field(String),
    /// A new invisible signature field, added to the first page.
    ///
    /// Invisible — a zero `/Rect` and the hidden flag — because generating a
    /// visible appearance is a separate capability and a signature that draws
    /// nothing is honest about that, where an empty box would not be.
    NewInvisibleField {
        /// The field's `/T`.
        name: String,
    },
}

/// What a caller supplies to sign on save.
pub struct SigningRequest<'a> {
    /// Where the signature goes.
    pub target: SigningTarget,
    /// The host's key.
    pub signer: &'a dyn Signer,
    /// How many bytes to reserve for the CMS blob.
    ///
    /// The reservation cannot grow after the file is laid out — every offset
    /// in the cross-reference table would move — so a blob that does not fit
    /// is [`SignError::ReserveTooSmall`] and never a truncation. The corpus's
    /// blobs run from 1 919 to 33 680 bytes, so a caller with no better
    /// information should reserve generously; the unused remainder costs two
    /// hexadecimal digits a byte and nothing else.
    pub reserve: usize,
    /// `/SubFilter` (12.8.3). `adbe.pkcs7.detached` unless the host says
    /// otherwise; whatever is written here must match what the signer returns.
    pub sub_filter: String,
    /// `/Reason`.
    pub reason: Option<String>,
    /// `/Location`.
    pub location: Option<String>,
    /// `/Name` — who the signer claims to be.
    pub name: Option<String>,
    /// `/ContactInfo`.
    pub contact: Option<String>,
    /// `/M`, the signing time. Supplied, never read from a clock.
    pub signed_at: Option<Date>,
}

impl<'a> SigningRequest<'a> {
    /// A request with `adbe.pkcs7.detached`, no metadata and a 16 KiB
    /// reservation, which fits every CMS blob in the fetched corpora bar one.
    pub fn new(target: SigningTarget, signer: &'a dyn Signer) -> SigningRequest<'a> {
        SigningRequest {
            target,
            signer,
            reserve: 16 * 1024,
            sub_filter: "adbe.pkcs7.detached".to_string(),
            reason: None,
            location: None,
            name: None,
            contact: None,
            signed_at: None,
        }
    }
}

/// Why a signing save produced no file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SignError {
    /// Signing needs [`crate::WriteMode::Incremental`]: a rewrite renumbers
    /// and relocates objects, so it cannot preserve a signature already in the
    /// file, and a signature over a rewrite of a signed document would assert
    /// something about bytes that no longer exist.
    NotIncremental,
    /// [`SigningTarget::Field`] named a field the document does not have.
    NoSuchField(String),
    /// The named field is not a signature field.
    NotASignatureField(String),
    /// The named field already carries a signature.
    FieldAlreadySigned(String),
    /// [`SigningTarget::NewInvisibleField`] on a document with no page to put
    /// the widget on.
    NoPages,
    /// The CMS blob the signer returned does not fit the reservation. The
    /// alternative is a truncated signature, which is a file that looks signed
    /// and is not.
    ReserveTooSmall {
        /// How many bytes the blob needs.
        needed: usize,
        /// How many were reserved.
        reserved: usize,
    },
    /// The host declined.
    SignerRefused(SignRefused),
    /// The digest could not be taken, which means the layout produced a
    /// `/ByteRange` that does not fit its own file. A bug here rather than in
    /// the document, and refusing beats signing something unknown.
    RangeDoesNotFit,
}

impl std::fmt::Display for SignError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SignError::NotIncremental => f.write_str("signing requires an incremental save"),
            SignError::NoSuchField(name) => write!(f, "no field named {name:?}"),
            SignError::NotASignatureField(name) => write!(f, "{name:?} is not a signature field"),
            SignError::FieldAlreadySigned(name) => write!(f, "{name:?} is already signed"),
            SignError::NoPages => f.write_str("the document has no page to place a field on"),
            SignError::ReserveTooSmall { needed, reserved } => write!(
                f,
                "the signature needs {needed} bytes and {reserved} were reserved"
            ),
            SignError::SignerRefused(refusal) => {
                write!(f, "the signer refused: {}", refusal.reason)
            }
            SignError::RangeDoesNotFit => {
                f.write_str("the computed byte range does not fit the file")
            }
        }
    }
}

impl std::error::Error for SignError {}

/// 7.9.4: `D:YYYYMMDDHHmmSSOHH'mm`.
///
/// Written here rather than reused because the only other date formatter in
/// the tree is the form-script interpreter's, which implements JavaScript's
/// `util.printd` and has nothing to do with this.
#[must_use]
pub fn pdf_date(date: Date) -> String {
    let mut out = format!(
        "D:{:04}{:02}{:02}{:02}{:02}{:02}",
        date.year, date.month, date.day, date.hour, date.minute, date.second
    );
    match date.utc_offset_minutes {
        // 7.9.4: `Z` alone, with no `HH'mm` after it.
        Some(0) => out.push('Z'),
        Some(offset) => {
            let sign = if offset < 0 { '-' } else { '+' };
            let magnitude = offset.abs();
            out.push(sign);
            out.push_str(&format!("{:02}'{:02}", magnitude / 60, magnitude % 60));
        }
        // An unspecified zone is legal and means local time, which is what a
        // signer who did not say meant.
        None => {}
    }
    out
}

/// A signature dictionary serialised with its `/Contents` and `/ByteRange`
/// reserved, and the offsets of both within it.
///
/// Serialised here rather than through [`crate::write::write_dict`] for one
/// reason that decides it: the writer cannot report where inside its output a
/// particular value landed, and patching by searching the finished file for a
/// run of zeros would find the first plausible match rather than the right
/// one. A signature dictionary is names, text strings, an integer array and a
/// hexadecimal string — a shape small enough to own.
///
/// It is also the object that must **not** be encrypted. 7.6.2 exempts a
/// signature dictionary's `/Contents` from encryption, and writing it by hand
/// is what makes skipping the cipher a decision rather than an omission.
pub(crate) struct Reserved {
    pub(crate) bytes: Vec<u8>,
    pub(crate) contents_at: usize,
    pub(crate) contents_len: usize,
    pub(crate) byte_range_at: usize,
    pub(crate) byte_range_len: usize,
}

/// The widest `/ByteRange` this writer emits: four ten-digit numbers.
///
/// Ten digits reach 9 999 999 999 bytes, which is about 9.3 GiB — past
/// anything this engine will hold in memory, since a document is an
/// `Arc<[u8]>`. Fixed width is not decoration: the array is patched in place
/// after the file is laid out, so a narrower number would shift every offset
/// the cross-reference table just recorded.
const BYTE_RANGE_TEMPLATE: &str = "[0000000000 0000000000 0000000000 0000000000]";

/// A text string as `(...)`, escaped per 7.3.4.2.
fn literal(out: &mut Vec<u8>, text: &str) {
    out.push(b'(');
    for byte in text.as_bytes() {
        match byte {
            b'(' | b')' | b'\\' => {
                out.push(b'\\');
                out.push(*byte);
            }
            b'\r' => out.extend_from_slice(b"\\r"),
            other => out.push(*other),
        }
    }
    out.push(b')');
}

/// A name as `/Foo`, with anything outside 7.3.5's regular characters written
/// as `#xx`.
fn name(out: &mut Vec<u8>, text: &str) {
    out.push(b'/');
    for byte in text.as_bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_' | b'+') {
            out.push(*byte);
        } else {
            out.extend_from_slice(format!("#{byte:02X}").as_bytes());
        }
    }
}

impl Reserved {
    /// The dictionary for `request`, with both fields reserved.
    pub(crate) fn build(request: &SigningRequest<'_>) -> Reserved {
        let mut bytes = Vec::with_capacity(request.reserve * 2 + 512);
        bytes.extend_from_slice(b"<< /Type /Sig /Filter /Adobe.PPKLite /SubFilter ");
        name(&mut bytes, &request.sub_filter);

        for (key, value) in [
            ("Reason", &request.reason),
            ("Location", &request.location),
            ("Name", &request.name),
            ("ContactInfo", &request.contact),
        ] {
            if let Some(value) = value {
                bytes.extend_from_slice(format!(" /{key} ").as_bytes());
                literal(&mut bytes, value);
            }
        }
        if let Some(date) = request.signed_at {
            bytes.extend_from_slice(b" /M ");
            literal(&mut bytes, &pdf_date(date));
        }

        bytes.extend_from_slice(b" /ByteRange ");
        let byte_range_at = bytes.len();
        bytes.extend_from_slice(BYTE_RANGE_TEMPLATE.as_bytes());
        let byte_range_len = BYTE_RANGE_TEMPLATE.len();

        bytes.extend_from_slice(b" /Contents ");
        let contents_at = bytes.len();
        bytes.push(b'<');
        bytes.resize(bytes.len() + request.reserve * 2, b'0');
        bytes.push(b'>');
        let contents_len = bytes.len() - contents_at;

        bytes.extend_from_slice(b" >>");
        Reserved {
            bytes,
            contents_at,
            contents_len,
            byte_range_at,
            byte_range_len,
        }
    }
}

/// Patches `/ByteRange` and `/Contents` into a finished file.
///
/// The order matters and is not interchangeable: `/ByteRange` is written
/// first because it is *inside* the range it describes, so a signature taken
/// before it was patched would be a signature over a placeholder.
pub(crate) fn seal(
    out: &mut [u8],
    at: &crate::write::SignaturePlaceholder,
    signer: &dyn Signer,
) -> Result<(), SignError> {
    let file_len = out.len() as u64;
    let first_end = at.contents_at as u64;
    let second_at = (at.contents_at + at.contents_len) as u64;
    let second_len = file_len.saturating_sub(second_at);

    let numbers = format!(
        "[{:010} {:010} {:010} {:010}]",
        0, first_end, second_at, second_len
    );
    if numbers.len() != at.byte_range_len {
        // Only reachable if the file outgrew ten digits, which needs a
        // document larger than this engine can hold.
        return Err(SignError::RangeDoesNotFit);
    }
    out[at.byte_range_at..at.byte_range_at + at.byte_range_len].copy_from_slice(numbers.as_bytes());

    let spans = [0..first_end, second_at..second_at + second_len];
    let algorithm = signer.digest_algorithm();
    let digest = digest_spans(out, &spans, algorithm).ok_or(SignError::RangeDoesNotFit)?;
    let cms = signer.sign(&digest).map_err(SignError::SignerRefused)?;

    // The reservation holds `<` + two hexadecimal digits a byte + `>`.
    let room = at.contents_len.saturating_sub(2) / 2;
    if cms.len() > room {
        return Err(SignError::ReserveTooSmall {
            needed: cms.len(),
            reserved: room,
        });
    }

    let mut written = at.contents_at + 1;
    for byte in &cms {
        let pair = format!("{byte:02X}");
        out[written..written + 2].copy_from_slice(pair.as_bytes());
        written += 2;
    }
    // The remainder stays as the `0` fill it was built with, which decodes to
    // trailing zero bytes after the DER the blob declares — invisible to a
    // parser that reads the SEQUENCE's own length, and to the digest, which
    // does not cover the gap at all.
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_digest_over_two_spans_ignores_the_gap_between_them() {
        let bytes = b"abcdefghij".to_vec();
        let split = digest_spans(&bytes, &[0..3, 7..10], DigestAlgorithm::Sha256).unwrap();
        let joined = 0u64..6;
        let whole = digest_spans(
            b"abchij",
            std::slice::from_ref(&joined),
            DigestAlgorithm::Sha256,
        )
        .unwrap();
        assert_eq!(split, whole);
    }

    #[test]
    fn a_span_past_the_end_digests_to_nothing() {
        assert_eq!(
            digest_spans(b"short", &[0..3, 4..99], DigestAlgorithm::Sha256),
            None
        );
    }

    #[test]
    fn every_algorithm_produces_its_declared_length() {
        for algorithm in [
            DigestAlgorithm::Sha1,
            DigestAlgorithm::Sha256,
            DigestAlgorithm::Sha384,
            DigestAlgorithm::Sha512,
        ] {
            let whole = 0u64..8;
            let digest =
                digest_spans(b"whatever", std::slice::from_ref(&whole), algorithm).unwrap();
            assert_eq!(digest.len(), algorithm.length(), "{algorithm:?}");
        }
    }

    #[test]
    fn a_date_writes_the_shape_7_9_4_describes() {
        let date = Date {
            year: 2026,
            month: 8,
            day: 28,
            hour: 14,
            minute: 5,
            second: 9,
            utc_offset_minutes: Some(-330),
        };
        assert_eq!(pdf_date(date), "D:20260828140509-05'30");

        let zulu = Date {
            utc_offset_minutes: Some(0),
            ..date
        };
        assert_eq!(pdf_date(zulu), "D:20260828140509Z");

        let unzoned = Date {
            utc_offset_minutes: None,
            ..date
        };
        assert_eq!(pdf_date(unzoned), "D:20260828140509");
    }

    /// The reservation's two fields must be findable by offset, because the
    /// finished file is patched by offset and nothing searches it.
    #[test]
    fn the_reservation_reports_where_its_two_fields_are() {
        struct Nothing;
        impl Signer for Nothing {
            fn digest_algorithm(&self) -> DigestAlgorithm {
                DigestAlgorithm::Sha256
            }
            fn sign(&self, _digest: &[u8]) -> Result<Vec<u8>, SignRefused> {
                Ok(Vec::new())
            }
        }
        let signer = Nothing;
        let mut request = SigningRequest::new(
            SigningTarget::Field("Signature1".into()),
            &signer as &dyn Signer,
        );
        request.reserve = 8;
        request.reason = Some("a (parenthesised) reason".into());

        let reserved = Reserved::build(&request);
        assert_eq!(
            &reserved.bytes
                [reserved.byte_range_at..reserved.byte_range_at + reserved.byte_range_len],
            BYTE_RANGE_TEMPLATE.as_bytes()
        );
        assert_eq!(reserved.bytes[reserved.contents_at], b'<');
        assert_eq!(
            reserved.bytes[reserved.contents_at + reserved.contents_len - 1],
            b'>'
        );
        assert_eq!(
            reserved.contents_len,
            2 + 8 * 2,
            "angle brackets plus two digits a byte"
        );
        let text = String::from_utf8_lossy(&reserved.bytes);
        assert!(
            text.contains(r"(a \(parenthesised\) reason)"),
            "7.3.4.2 escaping: {text}"
        );
    }

    #[test]
    fn a_name_with_delimiters_in_it_is_written_as_hex_escapes() {
        let mut out = Vec::new();
        name(&mut out, "adbe.pkcs7.detached");
        assert_eq!(out, b"/adbe.pkcs7.detached");

        let mut out = Vec::new();
        name(&mut out, "a b");
        assert_eq!(out, b"/a#20b", "7.3.5 writes an irregular byte as #xx");
    }
}
