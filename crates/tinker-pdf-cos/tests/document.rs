//! The canonical fixtures, opened end to end.
//!
//! These four files are MuPDF-written and honest, so every one of them must
//! open at [`LadderLevel::Trust`] with an empty warning list. A warning here
//! is a regression in this crate, not damage in the file.

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::Arc;

use tinker_pdf_cos::{CosDocument, LadderLevel, Name, ObjRef, Object, XrefEntry};

fn fixture(name: &str) -> Vec<u8> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../testdata")
        .join(name);
    std::fs::read(&path).unwrap_or_else(|e| panic!("reading {}: {e}", path.display()))
}

fn open(name: &str) -> CosDocument {
    CosDocument::open(fixture(name)).unwrap_or_else(|e| panic!("opening {name}: {e}"))
}

/// Walks `/Root` → `/Pages` → `/Kids` with a visited set, counting leaves.
///
/// A test predicate, not an API: page-tree semantics belong to phase 04. The
/// visited set is the point — a `/Kids` cycle must terminate the walk, not the
/// process.
fn count_pages(doc: &CosDocument) -> usize {
    let root = doc.resolve_key(doc.trailer(), Name::ROOT);
    let Some(catalog) = root.as_dict() else {
        return 0;
    };
    let pages = doc.resolve_key(catalog, Name::PAGES);
    let mut visited: BTreeSet<ObjRef> = BTreeSet::new();
    let mut leaves = 0usize;
    let mut stack: Vec<Arc<Object>> = vec![pages];
    while let Some(node) = stack.pop() {
        let Some(dict) = node.as_dict() else { continue };
        match dict.get(Name::KIDS) {
            Some(kids) => {
                let kids = doc.resolve(kids);
                let Some(items) = kids.as_array() else {
                    continue;
                };
                for kid in items {
                    if let Some(r) = kid.as_objref() {
                        if !visited.insert(r) {
                            continue;
                        }
                    }
                    stack.push(doc.resolve(kid));
                }
            }
            None => leaves += 1,
        }
    }
    leaves
}

#[test]
fn simple_text_opens_clean_and_resolves_everything() {
    let doc = open("simple-text.pdf");
    assert_eq!(doc.ladder_level(), LadderLevel::Trust);
    assert_eq!(doc.warnings(), Vec::new(), "an honest file must be silent");

    // 13 entries: object 0 free plus objects 1..=12.
    assert_eq!(doc.xref().len(), 13);
    for (num, entry) in doc.xref().iter() {
        let object = doc.get(ObjRef::new(num, 0)).expect("a value");
        match entry {
            XrefEntry::Free { .. } => assert!(object.is_null()),
            _ => assert!(!object.is_null(), "object {num} resolved to null"),
        }
    }
    assert_eq!(doc.warnings(), Vec::new(), "resolving must stay silent");
    assert_eq!(count_pages(&doc), 3);
    assert_eq!(doc.revisions().len(), 1);
    assert_eq!(doc.revisions()[0].byte_range.start, 0);
    assert!(doc.encrypt_params().is_none());
}

#[test]
fn outline_fixture_has_six_pages() {
    let doc = open("outline-3level.pdf");
    assert_eq!(doc.ladder_level(), LadderLevel::Trust);
    assert_eq!(doc.warnings(), Vec::new());
    assert_eq!(count_pages(&doc), 6);
    for (num, _) in doc.xref().iter() {
        let _ = doc.get(ObjRef::new(num, 0)).expect("a value");
    }
    assert_eq!(doc.warnings(), Vec::new());
}

#[test]
fn a_content_stream_decodes_to_text_operators() {
    let doc = open("simple-text.pdf");
    // Object 6 is the first page's content stream.
    let decoded = doc.stream_decoded(ObjRef::new(6, 0)).expect("a stream");
    let text = String::from_utf8_lossy(&decoded);
    assert!(text.contains("BT"), "{text}");
    assert!(text.contains("Tj"), "{text}");
    assert!(text.contains("ET"), "{text}");

    // Nothing was compressed, so all three tiers agree byte for byte.
    let raw = doc.stream_raw(ObjRef::new(6, 0)).expect("a stream");
    let encrypted = doc
        .stream_raw_encrypted(ObjRef::new(6, 0))
        .expect("a stream");
    assert_eq!(raw, decoded);
    assert_eq!(encrypted, &raw[..]);
    assert_eq!(doc.warnings(), Vec::new());
}

#[test]
fn the_forensic_tier_is_the_declared_range() {
    let bytes = fixture("simple-text.pdf");
    let doc = CosDocument::open(bytes.clone()).expect("opens");
    let object = doc.get(ObjRef::new(6, 0)).expect("a value");
    let stream = object.as_stream().expect("a stream");
    let start = stream.data_start as usize;
    let len = stream.len_hint.expect("a direct /Length") as usize;
    // Independently extracted from the buffer, not through the API.
    assert_eq!(
        doc.stream_raw_encrypted(ObjRef::new(6, 0))
            .expect("a stream"),
        &bytes[start..start + len]
    );
}

#[test]
fn a_non_stream_object_reports_it() {
    let doc = open("simple-text.pdf");
    let err = doc
        .stream_decoded(ObjRef::new(1, 0))
        .expect_err("a catalog");
    assert_eq!(err, tinker_pdf_cos::CosError::NotAStream(ObjRef::new(1, 0)));
    // A reference to nothing is null, not a stream, and still not a panic.
    assert!(doc.stream_raw(ObjRef::new(9999, 0)).is_err());
}

#[test]
fn the_encrypted_fixture_reaches_the_crypto_handoff() {
    let doc = open("encrypted-aes256.pdf");
    assert_eq!(doc.ladder_level(), LadderLevel::Trust);

    let params = doc.encrypt_params().expect("an /Encrypt dictionary");
    assert_eq!(params.filter.as_deref(), Some(&b"Standard"[..]));
    assert_eq!(params.v, Some(5));
    assert_eq!(params.r, Some(6));
    assert_eq!(params.length, Some(256));
    assert_eq!(params.p, Some(-4));
    assert!(params.encrypt_metadata);
    assert_eq!(params.stm_f.as_deref(), Some(&b"StdCF"[..]));
    assert_eq!(params.str_f.as_deref(), Some(&b"StdCF"[..]));
    assert_eq!(params.o.as_ref().map(Vec::len), Some(48));
    assert_eq!(params.u.as_ref().map(Vec::len), Some(48));
    assert_eq!(params.oe.as_ref().map(Vec::len), Some(32));
    assert_eq!(params.ue.as_ref().map(Vec::len), Some(32));
    assert_eq!(params.perms.as_ref().map(Vec::len), Some(16));
    assert_eq!(params.id_first.as_ref().map(Vec::len), Some(16));
    // The /Encrypt dictionary is direct here, so there is no object to exempt.
    assert_eq!(params.encrypt_ref, None);

    let filters = &params.crypt_filters;
    assert_eq!(filters.len(), 1);
    assert_eq!(filters[0].name, b"StdCF".to_vec());
    assert_eq!(filters[0].cfm.as_deref(), Some(&b"AESV3"[..]));
    assert_eq!(filters[0].auth_event.as_deref(), Some(&b"DocOpen"[..]));
    assert_eq!(filters[0].length, Some(32));

    // With the identity decryptor the strings are still ciphertext, which is
    // fine: the contract is that nothing panics and every object is reachable.
    for (num, _) in doc.xref().iter() {
        let _ = doc.get(ObjRef::new(num, 0)).expect("a value");
    }
    let info = doc.resolve_key(doc.trailer(), Name::ROOT);
    assert!(info.as_dict().is_some());
    let content = doc.stream_decoded(ObjRef::new(6, 0)).expect("a stream");
    assert_eq!(content.len(), 96);
}

#[test]
fn the_permissions_fixture_opens_too() {
    let doc = open("permissions-noprint.pdf");
    assert_eq!(doc.ladder_level(), LadderLevel::Trust);
    let params = doc.encrypt_params().expect("an /Encrypt dictionary");
    assert_eq!(params.filter.as_deref(), Some(&b"Standard"[..]));
    assert_eq!(params.p, Some(-2056));
}

#[test]
fn every_fixture_opens_without_warnings() {
    for name in [
        "simple-text.pdf",
        "outline-3level.pdf",
        "encrypted-aes256.pdf",
        "permissions-noprint.pdf",
    ] {
        let doc = open(name);
        for (num, _) in doc.xref().iter() {
            let _ = doc.get(ObjRef::new(num, 0)).expect("a value");
        }
        assert_eq!(
            doc.warnings(),
            Vec::new(),
            "{name} produced warnings: {:?}",
            doc.warnings()
        );
        assert_eq!(doc.ladder_level(), LadderLevel::Trust, "{name}");
    }
}
