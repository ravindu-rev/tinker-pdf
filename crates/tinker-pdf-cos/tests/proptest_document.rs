//! The never-panic contract, at the document layer.
//!
//! Ruling 1 says untrusted input must never panic. The fuzz targets prove that
//! over long runs; these properties prove it on every commit, over the shapes
//! that actually reach this layer: arbitrary bytes, a real file truncated
//! anywhere, and a real file with one byte changed anywhere.

use proptest::prelude::*;

use tinker_pdf_cos::{CosDocument, LadderLevel, Name, ObjRef};

/// A small, honest document to mutate.
fn baseline() -> Vec<u8> {
    let objects: [&[u8]; 4] = [
        b"<< /Type /Catalog /Pages 2 0 R >>",
        b"<< /Type /Pages /Count 1 /Kids [3 0 R] >>",
        b"<< /Type /Page /Parent 2 0 R /Contents 4 0 R >>",
        b"<< /Length 9 >>\nstream\nBT () Tj\nendstream",
    ];
    let mut buf = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (i, body) in objects.iter().enumerate() {
        offsets.push(buf.len());
        buf.extend_from_slice(format!("{} 0 obj\n", i + 1).as_bytes());
        buf.extend_from_slice(body);
        buf.extend_from_slice(b"\nendobj\n");
    }
    let xref_at = buf.len();
    buf.extend_from_slice(b"xref\n0 5\n0000000000 65535 f \n");
    for offset in &offsets {
        buf.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    buf.extend_from_slice(
        format!("trailer\n<< /Size 5 /Root 1 0 R >>\nstartxref\n{xref_at}\n%%EOF\n").as_bytes(),
    );
    buf
}

/// Reads everything a document can be asked for. Any panic in here is the
/// failure the property is looking for.
fn exercise(doc: &CosDocument) {
    for (num, _) in doc.xref().iter() {
        let r = ObjRef::new(num, 0);
        let object = doc.get(r).expect("get never errors");
        if let Some(dict) = object.as_dict() {
            for (_, value) in dict.iter() {
                let _ = doc.resolve(value);
            }
        }
        if object.as_stream().is_some() {
            let _ = doc.stream_raw_encrypted(r);
            let _ = doc.stream_raw(r);
            let _ = doc.stream_decoded(r);
        }
    }
    let _ = doc.resolve_key(doc.trailer(), Name::ROOT);
    let _ = doc.warnings();
    let _ = doc.revisions();
    let _ = doc.encrypt_params();
}

proptest! {
    #[test]
    fn arbitrary_bytes_never_panic(bytes in proptest::collection::vec(any::<u8>(), 0..2048)) {
        if let Ok(doc) = CosDocument::open(bytes) {
            exercise(&doc);
        }
    }

    /// Random bytes are mostly noise; bytes drawn from the PDF alphabet hit
    /// the object and cross-reference grammars far more often.
    #[test]
    fn pdf_shaped_bytes_never_panic(
        bytes in proptest::collection::vec(
            prop::sample::select(
                b"0123456789 \n\r\t<>[]()/%RobjendstreamxrfPDF-.+#\\".to_vec()
            ),
            0..2048,
        )
    ) {
        if let Ok(doc) = CosDocument::open(bytes) {
            exercise(&doc);
        }
    }

    #[test]
    fn truncating_anywhere_never_panics(cut in 0usize..400) {
        let bytes = baseline();
        let cut = cut.min(bytes.len());
        if let Ok(doc) = CosDocument::open(bytes[..cut].to_vec()) {
            exercise(&doc);
        }
    }

    #[test]
    fn flipping_one_byte_never_panics(at in 0usize..400, value in any::<u8>()) {
        let mut bytes = baseline();
        let at = at % bytes.len();
        bytes[at] = value;
        if let Ok(doc) = CosDocument::open(bytes) {
            exercise(&doc);
        }
    }

    /// Bytes prepended before `%PDF-` shift every offset by exactly their
    /// length, and the ladder must absorb that without repairing anything.
    #[test]
    fn any_amount_of_leading_junk_still_opens_at_trust(junk in 1usize..500) {
        let mut bytes = vec![b'z'; junk];
        bytes.extend_from_slice(&baseline());
        let doc = CosDocument::open(bytes).expect("opens");
        prop_assert_eq!(doc.ladder_level(), LadderLevel::Trust);
        prop_assert!(!doc.get(ObjRef::new(1, 0)).expect("a catalog").is_null());
    }

    /// The ladder is a function of the bytes, not of what was read first.
    #[test]
    fn opening_twice_agrees(cut in 0usize..400) {
        let bytes = baseline();
        let cut = cut.min(bytes.len());
        let damaged = bytes[..cut].to_vec();
        match (CosDocument::open(damaged.clone()), CosDocument::open(damaged)) {
            (Ok(a), Ok(b)) => {
                prop_assert_eq!(a.ladder_level(), b.ladder_level());
                prop_assert_eq!(a.warnings(), b.warnings());
                prop_assert_eq!(a.xref().len(), b.xref().len());
            }
            (Err(a), Err(b)) => prop_assert_eq!(a, b),
            (a, b) => prop_assert!(false, "{a:?} vs {b:?}"),
        }
    }
}
