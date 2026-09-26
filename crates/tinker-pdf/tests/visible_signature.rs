//! A visible signature: `SigningTarget::NewVisibleField` (12.7.4.5, 12.5.5).
//!
//! The roadmap row's exit criterion, clause by clause: the widget carries an
//! `/AP /N`; it **renders non-blank inside its rectangle** and nowhere else;
//! the signature **still verifies** — `Document::verify_signatures` answers
//! `Verified` over a `Matches` digest and a chain anchored to the signer's own
//! certificate; and the **`/ByteRange` covers the appearance**, shown two ways:
//! the appearance's object lies inside a covered span, and changing one byte
//! of what the seal draws turns the digest to `Differs`.
//!
//! # The signer, and what it is worth
//!
//! Verifying needs a real signature over the digest this engine computes when
//! it lays the file out, so the signer here is real: RSASSA-PKCS1-v1_5 over a
//! CMS `SignedData` this file assembles, with the private exponent of a
//! throwaway 2048-bit key OpenSSL generated once
//! (`tests/signature_support/README.md` records the command). The modular
//! exponentiation is `tinker_pdf_crypto::bignum::Modulus::pow` — the same
//! arithmetic the verifier runs with the public exponent — so the assembly and
//! the check share a primitive. What keeps that from being circular is that the
//! primitive is held to NIST CAVP's vectors elsewhere, and that the key and the
//! certificate are OpenSSL's: a certificate this engine could not read, or a
//! key whose `e` and `d` were not inverses, would fail here rather than agree
//! with itself. It is still this engine agreeing with its own reading of RFC
//! 5652, and `docs/features/signatures.md` says so.
//!
//! No program is spawned (ruling 13): the key and certificate are committed
//! bytes, and everything else happens in this process.
//!
//! # Injections
//!
//! Put back one at a time, `cargo test --no-fail-fast -p tinker-pdf --test
//! visible_signature`, 26 September 2026:
//!
//! | Defect | Fires (of 7) |
//! | --- | --- |
//! | the widget written with no `/AP` | 3 |
//! | the appearance drawn with no text (`lines` ignored) | 3 |
//! | the image left out of the appearance's resources | 1 |
//! | the invisible field's zero `/Rect` kept for the visible one | 3 |
//! | the seal's date written without its zone | 1 |
//! | a character the seal drew as `?` not reported | 1 |
//!
//! The image injection fired **zero** on its first run, and the reason is
//! worth keeping: a missing image draws ruling 2's neutral placeholder, which
//! is ink, and the assertion then counted ink. It now asks for the picture's
//! own two greys.
//!
//! The one clause no injection here can reach is the `/ByteRange`'s: the seal
//! is written by the same update as the signature dictionary, and there is no
//! plausible edit to this code that writes it anywhere else. What
//! `the_byte_range_covers_the_appearance` holds is the consequence — change a
//! byte of the seal and the digest differs — so a writer that one day moved
//! the appearance out of the signed update would fail it.

use std::sync::Arc;

use tinker_pdf::{
    AnnotationKind, Chain, Coverage, Date, DigestAlgorithm, Document, DocumentDigest, FontProvider,
    Rect, RenderOptions, SignError, SignRefused, SignatureAppearance, SignatureCheck,
    SignatureImage, Signer, SigningRequest, SigningTarget, SimpleFontProvider, TrustAnchors,
    WarningKind, WriteMode, WriteOptions,
};
use tinker_pdf_crypto::bignum::{Modulus, Uint};
use tinker_pdf_crypto::sha2::Sha256;

mod render_support;
use render_support::curvy_font;

// ---- a signer that holds a real key ----------------------------------------

const KEY: &[u8] = include_bytes!("signature_support/visible-signer-key.der");
const CERTIFICATE: &[u8] = include_bytes!("signature_support/visible-signer.der");

/// One DER element: its whole encoding and its contents.
///
/// Enough of X.690 to walk two committed files of known shape. Test code over
/// bytes this repository committed, so an `expect` is a statement about those
/// bytes rather than about a document.
fn element(bytes: &[u8]) -> (&[u8], &[u8]) {
    let first = bytes[1];
    let (length, header) = if first < 0x80 {
        (usize::from(first), 2)
    } else {
        let count = usize::from(first & 0x7F);
        let mut length = 0usize;
        for byte in &bytes[2..2 + count] {
            length = length << 8 | usize::from(*byte);
        }
        (length, 2 + count)
    };
    (&bytes[..header + length], &bytes[header..header + length])
}

/// The children of a constructed element's contents.
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
fn der(tag: u8, contents: &[u8]) -> Vec<u8> {
    let mut out = vec![tag];
    let n = contents.len();
    if n < 0x80 {
        out.push(n as u8);
    } else {
        let bytes: Vec<u8> = n
            .to_be_bytes()
            .into_iter()
            .skip_while(|b| *b == 0)
            .collect();
        out.push(0x80 | bytes.len() as u8);
        out.extend_from_slice(&bytes);
    }
    out.extend_from_slice(contents);
    out
}

const OID_SIGNED_DATA: &[u8] = &[
    0x06, 0x09, 0x2A, 0x86, 0x48, 0x86, 0xF7, 0x0D, 0x01, 0x07, 0x02,
];
const OID_DATA: &[u8] = &[
    0x06, 0x09, 0x2A, 0x86, 0x48, 0x86, 0xF7, 0x0D, 0x01, 0x07, 0x01,
];
const OID_SHA256: &[u8] = &[
    0x06, 0x09, 0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x02, 0x01,
];
const OID_RSA: &[u8] = &[
    0x06, 0x09, 0x2A, 0x86, 0x48, 0x86, 0xF7, 0x0D, 0x01, 0x01, 0x01,
];
const OID_CONTENT_TYPE: &[u8] = &[
    0x06, 0x09, 0x2A, 0x86, 0x48, 0x86, 0xF7, 0x0D, 0x01, 0x09, 0x03,
];
const OID_MESSAGE_DIGEST: &[u8] = &[
    0x06, 0x09, 0x2A, 0x86, 0x48, 0x86, 0xF7, 0x0D, 0x01, 0x09, 0x04,
];
const NULL: &[u8] = &[0x05, 0x00];

fn sha256(bytes: &[u8]) -> Vec<u8> {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hasher.finish().to_vec()
}

/// RSASSA-PKCS1-v1_5 with the committed key over a CMS `SignedData`
/// (RFC 5652 §5) whose `messageDigest` is the digest the engine hands over.
struct RsaSigner {
    modulus: Modulus<32>,
    private_exponent: Uint<32>,
    bytes: usize,
}

impl RsaSigner {
    fn committed() -> RsaSigner {
        // RFC 8017 A.1.2: version, n, e, d, ...
        let (_, key) = element(KEY);
        let fields = children(key);
        let n = element(fields[1]).1;
        let d = element(fields[3]).1;
        let modulus = Modulus::new(Uint::from_be_bytes(n).expect("a 2048-bit modulus"))
            .expect("an odd modulus");
        RsaSigner {
            bytes: modulus.byte_len(),
            modulus,
            private_exponent: Uint::from_be_bytes(d).expect("d fits"),
        }
    }

    fn rsa(&self, message: &[u8]) -> Vec<u8> {
        // RFC 8017 9.2: 00 01 FF..FF 00 DigestInfo.
        let info = der(
            0x30,
            &[
                der(0x30, &[OID_SHA256, NULL].concat()),
                der(0x04, &sha256(message)),
            ]
            .concat(),
        );
        let mut block = vec![0x00, 0x01];
        block.resize(self.bytes - info.len() - 1, 0xFF);
        block.push(0x00);
        block.extend_from_slice(&info);
        let m = Uint::from_be_bytes(&block).expect("fits");
        let s = self.modulus.pow(&m, &self.private_exponent);
        let mut out = vec![0; self.bytes];
        assert!(s.to_be_bytes(&mut out));
        out
    }
}

impl Signer for RsaSigner {
    fn digest_algorithm(&self) -> DigestAlgorithm {
        DigestAlgorithm::Sha256
    }

    fn sign(&self, digest: &[u8]) -> Result<Vec<u8>, SignRefused> {
        // The certificate's issuer and serial number name the signer (RFC
        // 5652 §5.3's IssuerAndSerialNumber).
        let (_, certificate) = element(CERTIFICATE);
        let tbs = element(children(certificate)[0]).1;
        let tbs_fields = children(tbs);
        let (serial, issuer) = (tbs_fields[1], tbs_fields[3]);

        let content_type = der(0x30, &[OID_CONTENT_TYPE, &der(0x31, OID_DATA)].concat());
        let message_digest = der(
            0x30,
            &[OID_MESSAGE_DIGEST, &der(0x31, &der(0x04, digest))].concat(),
        );
        // DER's SET OF is sorted by encoding; the content-type attribute is
        // the shorter and sorts first.
        let attributes = [content_type, message_digest].concat();
        // §5.4: the signature is over the attributes as a SET, and they are
        // stored under [0] IMPLICIT.
        let signature = self.rsa(&der(0x31, &attributes));

        let algorithm = der(0x30, &[OID_SHA256, NULL].concat());
        let signer_info = der(
            0x30,
            &[
                der(0x02, &[1]),
                der(0x30, &[issuer, serial].concat()),
                algorithm.clone(),
                der(0xA0, &attributes),
                der(0x30, &[OID_RSA, NULL].concat()),
                der(0x04, &signature),
            ]
            .concat(),
        );
        let signed_data = der(
            0x30,
            &[
                der(0x02, &[1]),
                der(0x31, &algorithm),
                der(0x30, OID_DATA),
                der(0xA0, CERTIFICATE),
                der(0x31, &signer_info),
            ]
            .concat(),
        );
        Ok(der(
            0x30,
            &[OID_SIGNED_DATA, &der(0xA0, &signed_data)].concat(),
        ))
    }
}

// ---- helpers ---------------------------------------------------------------

fn simple_text() -> Vec<u8> {
    std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../testdata/simple-text.pdf"
    ))
    .expect("testdata/simple-text.pdf")
}

fn incremental() -> WriteOptions {
    WriteOptions {
        mode: WriteMode::Incremental,
        ..WriteOptions::default()
    }
}

/// Where the seal goes on page 0 of the A4 fixture: clear of its text.
const SEAL: Rect = Rect {
    x0: 300.0,
    y0: 100.0,
    x1: 540.0,
    y1: 180.0,
};

/// A 48 by 32 greyscale picture: a dark disc on a mid-grey ground.
fn picture() -> SignatureImage {
    let (width, height) = (48u32, 32u32);
    let mut data = Vec::new();
    for y in 0..height {
        for x in 0..width {
            let (dx, dy) = (f64::from(x) - 24.0, f64::from(y) - 16.0);
            data.push(if dx * dx + dy * dy < 144.0 { 20 } else { 140 });
        }
    }
    SignatureImage::Gray8 {
        width,
        height,
        data,
    }
}

fn request<'a>(signer: &'a dyn Signer, appearance: SignatureAppearance) -> SigningRequest<'a> {
    let mut request = SigningRequest::new(
        SigningTarget::NewVisibleField {
            name: "Seal".to_string(),
            page: 0,
            rect: SEAL,
            appearance,
        },
        signer,
    );
    request.reserve = 4096;
    request.name = Some("Ada Lovelace".to_string());
    request.reason = Some("I approve this document".to_string());
    request.location = Some("London".to_string());
    request.signed_at = Some(Date {
        year: 2026,
        month: 9,
        day: 26,
        hour: 10,
        minute: 30,
        second: 0,
        utc_offset_minutes: Some(60),
    });
    request
}

fn signed(appearance: SignatureAppearance) -> (Vec<u8>, usize) {
    let original = simple_text();
    let signer = RsaSigner::committed();
    let bytes = Document::open(original.clone())
        .expect("the fixture opens")
        .editor()
        .save_signed(&incremental(), &request(&signer, appearance))
        .expect("signing succeeds");
    (bytes, original.len())
}

/// The seal widget's `/AP /N` object number, found through the facade's
/// annotation model and the object model under it.
fn appearance_object(document: &Document) -> u32 {
    let page = document.page(0).expect("a page");
    let widget = page
        .annotations()
        .into_iter()
        .find(|a| a.kind == AnnotationKind::Widget && a.rect.2 > a.rect.0)
        .expect("a visible widget on page 0");
    assert!(widget.has_appearance, "the widget has an /AP /N");
    assert_eq!(
        widget.rect,
        (SEAL.x0, SEAL.y0, SEAL.x1, SEAL.y1),
        "in the rectangle asked for"
    );
    assert!(widget.flags.print() && !widget.flags.hidden() && !widget.flags.no_view());
    let cos = document.cos();
    let object = cos
        .get(widget.reference.expect("an indirect widget"))
        .expect("the widget object");
    let ap = cos.resolve_key(object.as_dict().expect("a dictionary"), cos.intern(b"AP"));
    ap.as_dict()
        .and_then(|ap| ap.get_ref(cos.intern(b"N")))
        .expect("/AP /N is a stream reference")
        .num
}

/// How many pixels inside `r` are not near-white.
fn inked(bitmap: &tinker_pdf::Bitmap, page_height: f64, r: Rect) -> usize {
    let components = bitmap.components();
    let mut count = 0;
    for y in (page_height - r.y1) as u32..(page_height - r.y0) as u32 {
        for x in r.x0 as u32..r.x1 as u32 {
            let at = y as usize * bitmap.stride + x as usize * components;
            if bitmap
                .data
                .get(at..at + 3)
                .is_some_and(|p| p.iter().any(|&c| c < 200))
            {
                count += 1;
            }
        }
    }
    count
}

/// Whether `r` holds [`picture`] rather than a flat stand-in for it: the
/// disc's near-black and the ground's mid-grey both, as grey pixels.
///
/// Counting ink alone cannot tell the picture from ruling 2's neutral
/// placeholder, which a missing image draws in its place.
fn shows_the_picture(bitmap: &tinker_pdf::Bitmap, page_height: f64, r: Rect) -> bool {
    let components = bitmap.components();
    let (mut disc, mut ground) = (0, 0);
    for y in (page_height - r.y1) as u32..(page_height - r.y0) as u32 {
        for x in r.x0 as u32..r.x1 as u32 {
            let at = y as usize * bitmap.stride + x as usize * components;
            if let Some(p) = bitmap.data.get(at..at + 3) {
                let grey = p[0] == p[1] && p[1] == p[2];
                if grey && p[0] < 50 {
                    disc += 1;
                } else if grey && (120..=160).contains(&p[0]) {
                    ground += 1;
                }
            }
        }
    }
    disc > 100 && ground > 100
}

// ---- the exit criterion ----------------------------------------------------

/// The signature over a document with a seal on it verifies, and covers the
/// whole file.
#[test]
fn a_visible_signature_verifies() {
    let (bytes, _) = signed(SignatureAppearance::with_image(picture()));
    let document = Document::open(bytes).expect("the signed file reopens");

    let signatures = document.signatures();
    assert_eq!(signatures.len(), 1);
    assert_eq!(signatures[0].coverage, Coverage::WholeFile);
    assert_eq!(signatures[0].field.as_deref(), Some("Seal"));

    let mut anchors = TrustAnchors::new();
    anchors
        .add(CERTIFICATE.to_vec())
        .expect("the committed certificate parses");
    let verdicts = document.verify_signatures(&anchors, None);
    assert_eq!(verdicts.len(), 1);
    let verdict = &verdicts[0];
    assert_eq!(verdict.document_digest, DocumentDigest::Matches);
    assert_eq!(verdict.signature, SignatureCheck::Verified);
    assert!(
        matches!(verdict.chain, Chain::AnchoredTo { .. }),
        "{:?}",
        verdict.chain
    );
    assert!(verdict.is_trusted(), "{verdict:?}");
    assert!(document.validate().is_empty(), "{:?}", document.validate());
}

/// The appearance is an object of the signed update, and the `/ByteRange`
/// covers it: its offset is inside a covered span, and changing one byte of
/// what the seal draws is a change the signature detects.
#[test]
fn the_byte_range_covers_the_appearance() {
    let (bytes, original) = signed(SignatureAppearance::new());
    let document = Document::open(bytes.clone()).expect("reopens");
    let number = appearance_object(&document);

    let header = format!("\n{number} 0 obj");
    let at = bytes
        .windows(header.len())
        .position(|w| w == header.as_bytes())
        .expect("the appearance object is in the file");
    assert!(
        at >= original,
        "written by the signed update, not before it"
    );

    let spans = document.signatures()[0].spans.clone();
    let covered = |offset: usize| {
        spans
            .iter()
            .any(|s| (s.start as usize) <= offset && offset < s.end as usize)
    };
    let stream = bytes[at..]
        .windows(b"stream\n".len())
        .position(|w| w == b"stream\n")
        .map(|p| at + p + b"stream\n".len())
        .expect("its stream");
    let end = bytes[stream..]
        .windows(b"endstream".len())
        .position(|w| w == b"endstream")
        .map(|p| stream + p)
        .expect("its end");
    assert!(
        (at..end).all(covered),
        "every byte of the appearance object is covered"
    );

    // Tamper with the seal: one bit of the appearance's stream data, so the
    // seal draws something else and no offset in the file moves.
    let mut tampered = bytes.clone();
    tampered[stream] ^= 0x01;
    let document = Document::open(tampered).expect("still opens");
    let verdicts = document.verify_signatures(&TrustAnchors::new(), None);
    assert_eq!(
        verdicts[0].document_digest,
        DocumentDigest::Differs,
        "a changed seal is a changed document"
    );
    assert_eq!(verdicts[0].signature, SignatureCheck::Verified);
}

/// The seal draws inside its rectangle and nowhere it was not asked to.
///
/// Drawn twice: once with the synthetic face `render_support` builds standing
/// in for Helvetica, which is not embedded, so the text is ink; and once with
/// no face at all, where the image alone is what shows — which is the case a
/// host with no fonts would see.
#[test]
fn the_seal_renders_in_its_rectangle() {
    let (bytes, _) = signed(SignatureAppearance::with_image(picture()));
    let face: Arc<dyn FontProvider> = Arc::new(SimpleFontProvider::new(curvy_font()));
    let document = Document::open(bytes.clone())
        .expect("reopens")
        .with_fonts(face);
    let page = document.page(0).expect("a page");
    let height = page.size().1;
    let bitmap = page.render(&RenderOptions::default());

    let image_part = Rect {
        x0: SEAL.x0,
        y0: SEAL.y0,
        x1: SEAL.x0 + (SEAL.x1 - SEAL.x0) * 0.4,
        y1: SEAL.y1,
    };
    let text_part = Rect {
        x0: image_part.x1,
        ..SEAL
    };
    assert!(
        shows_the_picture(&bitmap, height, image_part),
        "the picture"
    );
    assert!(inked(&bitmap, height, text_part) > 200, "the text");
    let beside = Rect {
        x0: SEAL.x0,
        y0: SEAL.y1 + 2.0,
        x1: SEAL.x1,
        y1: SEAL.y1 + 40.0,
    };
    assert_eq!(inked(&bitmap, height, beside), 0, "nothing above it");

    // The same page drawn without the seal: the rectangle is empty there.
    let unsigned = Document::open(simple_text()).expect("opens");
    let page = unsigned.page(0).expect("a page");
    let bare = page.render(&RenderOptions::default());
    assert_eq!(inked(&bare, height, SEAL), 0, "the fixture leaves it blank");

    // No face at all: the picture still draws.
    let document = Document::open(bytes).expect("reopens");
    let page = document.page(0).expect("a page");
    let bitmap = page.render(&RenderOptions::default());
    assert!(shows_the_picture(&bitmap, height, image_part));
}

/// The seal says what the signature dictionary says.
#[test]
fn the_seal_text_is_the_requests_own() {
    let (bytes, _) = signed(SignatureAppearance::new());
    let document = Document::open(bytes).expect("reopens");
    let number = appearance_object(&document);
    let content = document
        .cos()
        .stream_decoded(tinker_pdf::ObjRef::new(number, 0))
        .expect("the appearance decodes");
    let content = String::from_utf8_lossy(&content);
    for line in [
        "(Digitally signed by Ada Lovelace) Tj",
        "(Date: 2026-09-26 10:30:00 +01:00) Tj",
        "(Reason: I approve this document) Tj",
        "(Location: London) Tj",
    ] {
        assert!(content.contains(line), "{line} in {content}");
    }
}

/// A JPEG is placed as it is, its size read from its own frame header.
#[test]
fn a_jpeg_seal_is_placed_as_it_is() {
    let jpeg = include_bytes!("cbz/source/page3.jpg").to_vec();
    let (bytes, _) = signed(SignatureAppearance::with_image(SignatureImage::Jpeg(
        jpeg.clone(),
    )));
    let document = Document::open(bytes.clone()).expect("reopens");
    assert!(
        bytes.windows(jpeg.len()).any(|w| w == jpeg.as_slice()),
        "the JPEG's bytes are in the file unchanged"
    );
    let verdicts = document.verify_signatures(&TrustAnchors::new(), None);
    assert_eq!(verdicts[0].signature, SignatureCheck::Verified);
    assert_eq!(verdicts[0].document_digest, DocumentDigest::Matches);
}

/// Each refusal by name.
#[test]
fn a_seal_that_cannot_be_drawn_is_refused() {
    let signer = RsaSigner::committed();
    let attempt = |target: SigningTarget| {
        let mut request = request(&signer, SignatureAppearance::new());
        request.target = target;
        Document::open(simple_text())
            .expect("opens")
            .editor()
            .save_signed(&incremental(), &request)
    };
    let visible =
        |page: u32, rect: Rect, appearance: SignatureAppearance| SigningTarget::NewVisibleField {
            name: "Seal".to_string(),
            page,
            rect,
            appearance,
        };
    assert_eq!(
        attempt(visible(9, SEAL, SignatureAppearance::new())),
        Err(SignError::NoSuchPage(9))
    );
    assert_eq!(
        attempt(visible(
            0,
            Rect {
                x1: SEAL.x0,
                ..SEAL
            },
            SignatureAppearance::new()
        )),
        Err(SignError::RectUnusable)
    );
    assert_eq!(
        attempt(visible(
            0,
            SEAL,
            SignatureAppearance::with_image(SignatureImage::Jpeg(b"not a jpeg".to_vec()))
        )),
        Err(SignError::ImageUnusable)
    );
    assert_eq!(
        attempt(visible(
            0,
            SEAL,
            SignatureAppearance::with_image(SignatureImage::Rgb8 {
                width: 4,
                height: 4,
                data: vec![0; 47],
            })
        )),
        Err(SignError::ImageUnusable),
        "one sample short of 4 x 4 x 3"
    );
}

/// A character the seal's font cannot draw is drawn as `?` and named against
/// the widget (ruling 10).
#[test]
fn a_name_the_font_cannot_draw_is_reported() {
    let signer = RsaSigner::committed();
    let mut request = request(&signer, SignatureAppearance::new());
    request.name = Some("\u{674E}\u{767D}".to_string());
    let document = Document::open(simple_text()).expect("opens");
    let bytes = document
        .editor()
        .save_signed(&incremental(), &request)
        .expect("signed");
    let named: Vec<char> = document
        .warnings()
        .into_iter()
        .filter_map(|w| match w.kind {
            WarningKind::FieldCharacterUnrepresentable { character } => {
                assert!(w.object.is_some(), "named against the widget");
                Some(character)
            }
            _ => None,
        })
        .collect();
    assert_eq!(named, vec!['\u{674E}', '\u{767D}']);

    let reopened = Document::open(bytes).expect("reopens");
    let verdicts = reopened.verify_signatures(&TrustAnchors::new(), None);
    assert_eq!(verdicts[0].signature, SignatureCheck::Verified);
}
