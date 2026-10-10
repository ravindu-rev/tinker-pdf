//! `DocumentEditor::view`: a `CosDocument` in which the editor's own objects
//! resolve.
//!
//! What needs it is anything built over an `Arc<CosDocument>` — a page's
//! resources, an interpreter — handed a reference the editor has only just
//! allocated. Through `shared_document` that reference names nothing; through
//! the view it names what the editor put there, at the same number.

use std::path::PathBuf;
use std::sync::Arc;

use tinker_pdf_cos::{
    pages, CosDocument, DocumentBuilder, DocumentEditor, Name, ObjRef, Object, PdfString,
    StreamData,
};

fn one_page() -> Arc<CosDocument> {
    let mut builder = DocumentBuilder::new();
    builder.add_page(100.0, 100.0, |page| {
        page.fill_rect(10.0, 10.0, 20.0, 20.0, 0.0);
    });
    Arc::new(CosDocument::open(builder.finish()).expect("it opens"))
}

/// Allocates a form XObject in `editor` and names it `/Fm9` in page 0's
/// resources, returning the form's reference.
fn add_form(editor: &mut DocumentEditor, drawing: &[u8]) -> ObjRef {
    let form = editor.allocate();
    let mut dict = tinker_pdf_cos::Dict::new();
    dict.insert(Name::TYPE, Object::Name(editor.intern(b"XObject")));
    dict.insert(
        editor.intern(b"Subtype"),
        Object::Name(editor.intern(b"Form")),
    );
    dict.insert(
        editor.intern(b"BBox"),
        Object::Array(vec![
            Object::Int(0),
            Object::Int(0),
            Object::Int(10),
            Object::Int(10),
        ]),
    );
    editor.put_stream(
        form,
        StreamData {
            dict,
            data: drawing.to_vec(),
        },
    );

    let page = editor.page_refs()[0];
    let Some(Object::Dict(mut page_dict)) = editor.get(page) else {
        panic!("a page dictionary");
    };
    let xobject = editor.intern(b"XObject");
    let mut resources = page_dict
        .get_dict(Name::RESOURCES)
        .cloned()
        .unwrap_or_default();
    let mut names = resources.get_dict(xobject).cloned().unwrap_or_default();
    names.insert(editor.intern(b"Fm9"), Object::Ref(form));
    resources.insert(xobject, Object::Dict(names));
    page_dict.insert(Name::RESOURCES, Object::Dict(resources));
    editor.put(page, Object::Dict(page_dict));
    form
}

/// The form a page's resources name resolves in the view, at the number the
/// editor gave it, with the bytes the editor wrote.
#[test]
fn an_allocated_form_resolves_through_the_page_that_names_it() {
    let doc = one_page();
    let mut editor = DocumentEditor::new(Arc::clone(&doc));
    let form = add_form(&mut editor, b"0 0 1 rg 0 0 10 10 re f");

    // The premise: the document the editor was opened with has no such
    // object, which is what every resolver built over it sees.
    assert!(matches!(
        editor.shared_document().get(form).expect("null").as_ref(),
        Object::Null
    ));

    let view = editor.view().expect("the view opens");
    let page = &pages::collect(&view)[0];
    let resources = page.resources.as_ref().expect("resources");
    let named = view
        .resolve_key(resources, view.intern(b"XObject"))
        .as_dict()
        .and_then(|x| x.get_ref(view.intern(b"Fm9")))
        .expect("the page names the form");
    assert_eq!(named, form, "the same object number the editor handed out");

    let object = view.get(form).expect("it loads");
    let stream = object.as_stream().expect("a stream, with its data");
    assert_eq!(
        stream
            .dict
            .get_name(view.intern(b"Subtype"))
            .and_then(|n| view.name_bytes(n))
            .as_deref(),
        Some(b"Form".as_slice())
    );
    assert_eq!(
        view.stream_decoded(form).expect("it decodes"),
        b"0 0 1 rg 0 0 10 10 re f"
    );
    assert!(view.warnings().is_empty(), "{:?}", view.warnings());

    // Everything else is where it was.
    assert_eq!(pages::count(&view), pages::count(&doc));
    let original = pages::content_bytes(&doc, &pages::collect(&doc)[0]);
    assert_eq!(pages::content_bytes(&view, page), original);
}

/// A view is a snapshot, and an untouched editor's view is its document.
#[test]
fn a_view_is_taken_when_asked_for() {
    let doc = one_page();
    let mut editor = DocumentEditor::new(Arc::clone(&doc));
    assert!(
        Arc::ptr_eq(&editor.view().expect("opens"), &doc),
        "nothing to add, nothing copied"
    );

    let before = editor.view().expect("opens");
    let form = add_form(&mut editor, b"0 g");
    assert!(matches!(
        before.get(form).expect("null").as_ref(),
        Object::Null
    ));
    let after = editor.view().expect("opens");
    assert!(after.get(form).expect("loads").as_stream().is_some());

    // The trailer is part of what a view shows: a new `/Info`, and a trailer
    // entry set with no object behind it at all.
    let mut editor = DocumentEditor::new(Arc::clone(&doc));
    editor.set_info(b"Title", "Viewed");
    let view = editor.view().expect("opens");
    assert_eq!(
        tinker_pdf_cos::metadata(&view).title.as_deref(),
        Some("Viewed")
    );

    let mut editor = DocumentEditor::new(Arc::clone(&doc));
    let note = editor.intern(b"TpdfNote");
    assert!(editor.set_trailer_entry(note, Object::Int(3)));
    let view = editor.view().expect("opens");
    assert_eq!(view.trailer().get_int(view.intern(b"TpdfNote")), Some(3));
}

/// An encrypted document's view decrypts: the original objects and the ones
/// the editor added, alike, without a password being asked for again.
#[test]
fn an_encrypted_documents_view_decrypts_old_and_new() {
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testdata/encrypted-aes256.pdf");
    let doc =
        Arc::new(CosDocument::open(std::fs::read(&path).expect("the fixture")).expect("it opens"));
    doc.authenticate("open-sesame").expect("the user password");
    let original = pages::content_bytes(&doc, &pages::collect(&doc)[0]);
    assert!(!original.is_empty(), "the premise: the page has content");

    let mut editor = DocumentEditor::new(Arc::clone(&doc));
    let note = editor.allocate();
    editor.put(
        note,
        Object::String(PdfString::literal(b"added by the editor".to_vec())),
    );
    let form = add_form(&mut editor, b"1 0 0 rg 0 0 5 5 re f");

    let view = editor.view().expect("the view opens");
    assert_eq!(view.auth_level(), doc.auth_level());
    assert!(view.is_encrypted());
    assert_eq!(
        view.get(note)
            .expect("loads")
            .as_string()
            .map(|s| s.bytes.clone()),
        Some(b"added by the editor".to_vec()),
        "a string the update sealed reads back as plaintext"
    );
    assert_eq!(
        view.stream_decoded(form).expect("decodes"),
        b"1 0 0 rg 0 0 5 5 re f"
    );
    let page = &pages::collect(&view)[0];
    assert_eq!(
        pages::content_bytes(&view, page),
        original,
        "and the file's own content still decrypts"
    );
}
