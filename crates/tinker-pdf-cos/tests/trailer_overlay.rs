//! Trailer entries set through the editor reach every kind of save, and a
//! rollback takes them back (7.5.5).
//!
//! `save` used to write the document's own trailer and nothing else, so an
//! editor had no way to give a file an `/Info` it did not have: the
//! dictionary could be put, and nothing would ever point at it.

use std::cell::RefCell;
use std::sync::Arc;

use tinker_pdf_cos::{
    digest_spans, fields, metadata, CosDocument, DigestAlgorithm, DocumentBuilder, DocumentEditor,
    Name, Object, SignRefused, Signer, SigningRequest, SigningTarget, WriteMode, WriteOptions,
};

/// A one-page document with no `/Info` at all.
fn bare() -> Arc<CosDocument> {
    let mut builder = DocumentBuilder::new();
    builder.add_page(100.0, 100.0, |page| {
        page.fill_rect(10.0, 10.0, 20.0, 20.0, 0.0);
    });
    let doc = CosDocument::open(builder.finish()).expect("it opens");
    assert!(doc.trailer().get(Name::INFO).is_none(), "the premise");
    Arc::new(doc)
}

fn saved(editor: &DocumentEditor, mode: WriteMode) -> CosDocument {
    CosDocument::open(editor.save(&WriteOptions {
        mode,
        ..WriteOptions::default()
    }))
    .expect("the save reopens")
}

#[test]
fn info_given_to_a_file_without_one_survives_an_incremental_save() {
    let doc = bare();
    let original = doc.bytes().to_vec();
    let mut editor = DocumentEditor::new(doc);
    editor.set_info(b"Title", "Ärger im Paradies");
    editor.set_info(b"Author", "日本語");
    assert!(editor.is_dirty());

    let bytes = editor.save(&WriteOptions {
        mode: WriteMode::Incremental,
        ..WriteOptions::default()
    });
    assert!(bytes.starts_with(&original), "an update, not a rewrite");
    let reopened = CosDocument::open(bytes).expect("reopens");
    let info = metadata(&reopened);
    assert_eq!(info.title.as_deref(), Some("Ärger im Paradies"));
    assert_eq!(info.author.as_deref(), Some("日本語"));
    assert!(reopened.warnings().is_empty(), "{:?}", reopened.warnings());
}

#[test]
fn info_given_to_a_file_without_one_survives_a_rewrite() {
    let mut editor = DocumentEditor::new(bare());
    editor.set_info(b"Title", "Rewritten");
    for collect in [false, true] {
        let reopened = CosDocument::open(editor.save(&WriteOptions {
            mode: WriteMode::Rewrite,
            garbage_collect: collect,
            ..WriteOptions::default()
        }))
        .expect("reopens");
        assert_eq!(
            metadata(&reopened).title.as_deref(),
            Some("Rewritten"),
            "garbage_collect: {collect} — the trailer reaches the new dictionary"
        );
    }
}

/// An existing `/Info` is updated where it is, keeping what it had.
#[test]
fn an_existing_info_is_updated_in_place() {
    let mut builder = DocumentBuilder::new();
    builder.add_page(100.0, 100.0, |_| {});
    assert!(builder.set_info(b"Title", "Kept"));
    let doc = Arc::new(CosDocument::open(builder.finish()).expect("opens"));
    let info_ref = doc.trailer().get_ref(Name::INFO).expect("an /Info");

    let mut editor = DocumentEditor::new(doc);
    editor.set_info(b"Subject", "Added");
    let reopened = saved(&editor, WriteMode::Incremental);
    assert_eq!(reopened.trailer().get_ref(Name::INFO), Some(info_ref));
    let info = metadata(&reopened);
    assert_eq!(info.title.as_deref(), Some("Kept"));
    assert_eq!(info.subject.as_deref(), Some("Added"));
}

/// The trailer entries are editor state: a checkpoint restores them, and so
/// does a transaction that fails.
#[test]
fn a_rollback_takes_the_new_info_back() {
    let mut editor = DocumentEditor::new(bare());
    let before = editor.checkpoint();
    editor.set_info(b"Title", "Gone");
    editor.restore(&before);
    assert!(!editor.is_dirty(), "the entry and its object are both gone");
    assert_eq!(metadata(&saved(&editor, WriteMode::Rewrite)).title, None);

    let failed: Result<(), ()> = editor.transaction(|tx| {
        tx.set_info(b"Title", "Also gone");
        Err(())
    });
    assert!(failed.is_err());
    assert!(!editor.is_dirty());
    assert_eq!(
        metadata(&saved(&editor, WriteMode::Incremental)).title,
        None
    );

    // A checkpoint taken after an entry was set keeps it: restoring puts back
    // what was there, not an empty trailer.
    let note = editor.intern(b"TpdfNote");
    assert!(editor.set_trailer_entry(note, Object::Int(1)));
    let kept = editor.checkpoint();
    editor.set_info(b"Title", "Rolled back");
    editor.restore(&kept);
    let reopened = saved(&editor, WriteMode::Incremental);
    assert_eq!(
        reopened.trailer().get_int(reopened.intern(b"TpdfNote")),
        Some(1)
    );
    assert_eq!(metadata(&reopened).title, None);
}

/// The writer's own keys are refused, and refusing changes nothing.
#[test]
fn the_writers_own_keys_cannot_be_set() {
    let mut editor = DocumentEditor::new(bare());
    for key in [
        &b"Size"[..],
        b"Prev",
        b"XRefStm",
        b"ID",
        b"Encrypt",
        b"Type",
        b"W",
        b"Index",
    ] {
        let name = editor.intern(key);
        assert!(
            !editor.set_trailer_entry(name, Object::Int(1)),
            "/{} is the writer's",
            String::from_utf8_lossy(key)
        );
    }
    assert!(!editor.is_dirty());

    // Anything else is the caller's, and is written.
    let custom = editor.intern(b"TpdfNote");
    assert!(editor.set_trailer_entry(custom, Object::Int(7)));
    assert!(editor.is_dirty(), "a trailer entry is an edit");
    let reopened = saved(&editor, WriteMode::Incremental);
    assert_eq!(
        reopened.trailer().get_int(reopened.intern(b"TpdfNote")),
        Some(7)
    );
}

/// Records the digest it was handed.
struct Recorder(RefCell<Vec<u8>>);

impl Signer for Recorder {
    fn digest_algorithm(&self) -> DigestAlgorithm {
        DigestAlgorithm::Sha256
    }

    fn sign(&self, digest: &[u8]) -> Result<Vec<u8>, SignRefused> {
        *self.0.borrow_mut() = digest.to_vec();
        Ok(vec![0x30; 64])
    }
}

/// A signed save carries the new `/Info`, and the signature still covers the
/// whole file: the reader recomputes, from the finished file's own
/// `/ByteRange`, the digest the signer was handed.
#[test]
fn a_signed_save_carries_the_trailer_and_still_verifies() {
    let mut editor = DocumentEditor::new(bare());
    editor.set_info(b"Title", "Signed");
    let signer = Recorder(RefCell::new(Vec::new()));
    let request = SigningRequest::new(
        SigningTarget::NewInvisibleField {
            name: "Seal".to_string(),
        },
        &signer,
    );
    let bytes = editor
        .save_signed(
            &WriteOptions {
                mode: WriteMode::Incremental,
                ..WriteOptions::default()
            },
            &request,
        )
        .expect("signing succeeds");

    let reopened = CosDocument::open(bytes.clone()).expect("reopens");
    assert_eq!(metadata(&reopened).title.as_deref(), Some("Signed"));

    let field = fields(&reopened)
        .into_iter()
        .find(|f| f.name == "Seal")
        .expect("the signature field");
    let signature = reopened
        .get(field.reference)
        .ok()
        .and_then(|o| o.as_dict().and_then(|d| d.get_ref(reopened.intern(b"V"))))
        .and_then(|r| reopened.get(r).ok())
        .expect("the signature dictionary");
    let range: Vec<u64> = signature
        .as_dict()
        .and_then(|d| d.get_array(reopened.intern(b"ByteRange")))
        .expect("a /ByteRange")
        .iter()
        .filter_map(Object::as_int)
        .filter_map(|v| u64::try_from(v).ok())
        .collect();
    let [a, b, c, d] = range[..] else {
        panic!("four numbers: {range:?}");
    };
    assert_eq!(a, 0, "from the first byte");
    assert_eq!(c + d, bytes.len() as u64, "to the last");
    let recomputed =
        digest_spans(&bytes, &[a..a + b, c..c + d], DigestAlgorithm::Sha256).expect("spans fit");
    assert_eq!(recomputed, *signer.0.borrow(), "the digest the signer saw");
}
