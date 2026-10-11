//! The editor reads its own edits.
//!
//! An editor is an overlay, and every read it makes of the document has to go
//! through that overlay or the second edit starts from the file and discards
//! the first. Three places did not: two helpers each re-implemented "read the
//! catalog through the overlay", the `/NeedAppearances` clean-up read the
//! file's catalog and wrote it back, and the field walk read the file's tree —
//! so a field the editor had put was invisible to `fields()` and `fill_field`
//! in the same editor. Each test here is one of those, saved and reopened
//! where the claim is about the file.

use std::path::PathBuf;
use std::sync::Arc;

use tinker_pdf_cos::{
    fields, form::needs_appearances, name_tree, trees, CosDocument, DigestAlgorithm,
    DocumentEditor, FieldValue, Name, ObjRef, Object, PdfString, Resolve, SignRefused, Signer,
    SigningRequest, SigningTarget, WriteMode, WriteOptions,
};

fn form_fixture() -> Arc<CosDocument> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testdata/form-fields.pdf");
    let bytes = std::fs::read(&path).expect("the form fixture");
    Arc::new(CosDocument::open(bytes).expect("it opens"))
}

fn incremental() -> WriteOptions {
    WriteOptions {
        mode: WriteMode::Incremental,
        ..WriteOptions::default()
    }
}

fn reopen(editor: &DocumentEditor) -> CosDocument {
    CosDocument::open(editor.save(&incremental())).expect("the save reopens")
}

fn name_of(doc: &CosDocument, object: Option<&Object>) -> Option<String> {
    let name = object?.as_name()?;
    Some(String::from_utf8_lossy(&doc.name_bytes(name)?).into_owned())
}

/// Two catalog edits compose, and a third made by the form filler — which
/// rewrites the catalog because this fixture's `/AcroForm` is direct — keeps
/// both.
///
/// The filler used to read the catalog from the file and write that back, so
/// every catalog change before a fill was silently undone by it.
#[test]
fn catalog_edits_compose_with_each_other_and_with_a_fill() {
    let doc = form_fixture();
    let mut editor = DocumentEditor::new(Arc::clone(&doc));
    let lang = editor.intern(b"Lang");
    let mode = editor.intern(b"PageMode");
    let outlines = editor.intern(b"UseOutlines");
    assert!(editor.update_catalog(|c| {
        c.insert(lang, Object::String(PdfString::literal(b"en-GB".to_vec())));
    }));
    assert!(editor.update_catalog(|c| {
        c.insert(mode, Object::Name(outlines));
    }));
    let seen = editor.catalog().expect("a catalog");
    assert!(seen.get(lang).is_some() && seen.get(mode).is_some(), "both");

    assert!(editor.set_field_value("notes", "filled"));

    let saved = reopen(&editor);
    let catalog = saved.catalog().expect("the saved catalog");
    assert_eq!(
        catalog
            .get(saved.intern(b"Lang"))
            .and_then(Object::as_string),
        Some(&PdfString::literal(b"en-GB".to_vec())),
        "the first edit survived the second and the fill"
    );
    assert_eq!(
        name_of(&saved, catalog.get(saved.intern(b"PageMode"))).as_deref(),
        Some("UseOutlines")
    );
    assert!(!needs_appearances(&saved), "and the fill cleared the flag");
}

/// A field the editor puts and lists in `/Fields` is a field: `fields()`
/// finds it before anything is saved, `fill_field` fills it and draws its
/// widget, and the saved file agrees.
#[test]
fn a_field_the_editor_puts_is_a_field_before_saving() {
    let doc = form_fixture();
    let mut editor = DocumentEditor::new(Arc::clone(&doc));
    let page = editor.page_refs()[0];

    let field = editor.allocate();
    let mut dict = tinker_pdf_cos::Dict::new();
    dict.insert(Name::TYPE, Object::Name(editor.intern(b"Annot")));
    dict.insert(
        editor.intern(b"Subtype"),
        Object::Name(editor.intern(b"Widget")),
    );
    dict.insert(editor.intern(b"FT"), Object::Name(editor.intern(b"Tx")));
    dict.insert(
        editor.intern(b"T"),
        Object::String(PdfString::literal(b"added".to_vec())),
    );
    dict.insert(
        editor.intern(b"Rect"),
        Object::Array(vec![
            Object::Int(20),
            Object::Int(20),
            Object::Int(200),
            Object::Int(40),
        ]),
    );
    dict.insert(editor.intern(b"P"), Object::Ref(page));
    editor.put(field, Object::Dict(dict));

    let acroform = editor.intern(b"AcroForm");
    let fields_key = editor.intern(b"Fields");
    assert!(editor.update_catalog(|catalog| {
        let Some(Object::Dict(mut form)) = catalog.get(acroform).cloned() else {
            panic!("the fixture's /AcroForm is direct");
        };
        let mut list = form
            .get_array(fields_key)
            .map(<[Object]>::to_vec)
            .unwrap_or_default();
        list.push(Object::Ref(field));
        form.insert(fields_key, Object::Array(list));
        catalog.insert(acroform, Object::Dict(form));
    }));

    let found = editor
        .fields()
        .into_iter()
        .find(|f| f.name == "added")
        .expect("fields() sees a field the editor put");
    assert_eq!(found.reference, field);
    assert_eq!(found.widgets, vec![field], "merged field and widget");

    let skipped = editor
        .fill_field("added", "overlay")
        .expect("the added field takes a value");
    assert!(
        skipped.is_empty(),
        "its /Rect is in the overlay, and the widget is drawn: {skipped:?}"
    );
    assert_eq!(
        editor
            .fields()
            .into_iter()
            .find(|f| f.name == "added")
            .map(|f| f.value),
        Some(FieldValue::Text("overlay".to_string()))
    );

    let saved = reopen(&editor);
    let reread = fields(&saved)
        .into_iter()
        .find(|f| f.name == "added")
        .expect("and the saved file has it");
    assert_eq!(reread.value, FieldValue::Text("overlay".to_string()));
    assert!(fields(&saved).len() > fields(&doc).len());
}

/// `/NeedAppearances` is cleared from the form the editor's catalog names,
/// not the file's.
///
/// Here the editor moves `/AcroForm` into an object of its own and sets the
/// flag there. The clean-up used to find the file's direct `/AcroForm`, clean
/// that, and write the file's catalog back over the editor's — so the flag
/// was "cleared" by pointing the catalog back at a form the editor had
/// replaced.
#[test]
fn clearing_the_rebuild_request_sees_the_editors_catalog() {
    let doc = form_fixture();
    let mut editor = DocumentEditor::new(Arc::clone(&doc));
    let acroform = editor.intern(b"AcroForm");
    let flag = editor.intern(b"NeedAppearances");

    let mut form = editor
        .catalog()
        .and_then(|c| c.get_dict(acroform).cloned())
        .expect("the fixture's /AcroForm is direct");
    form.insert(flag, Object::Bool(true));
    let moved = editor.allocate();
    editor.put(moved, Object::Dict(form));
    assert!(editor.update_catalog(|c| {
        c.insert(acroform, Object::Ref(moved));
    }));

    assert!(editor.set_field_value("notes", "filled"));

    let saved = reopen(&editor);
    let catalog = saved.catalog().expect("a catalog");
    assert_eq!(
        catalog.get_ref(saved.intern(b"AcroForm")),
        Some(moved),
        "the catalog still names the form the editor moved it to"
    );
    assert!(!needs_appearances(&saved), "and that form's flag is gone");
}

/// Trees are read through the overlay too, so a tree the editor has written
/// can be read back before it is saved.
#[test]
fn a_tree_the_editor_wrote_reads_through_the_editor() {
    let mut editor = DocumentEditor::new(form_fixture());
    let entries: Vec<(Vec<u8>, Object)> = (0..100)
        .map(|i| (format!("k{i:03}").into_bytes(), Object::Int(i)))
        .collect();
    let root = editor.add_name_tree(entries.clone()).expect("written");
    assert_eq!(trees::name_tree_in(&editor, root), entries);
    assert_eq!(
        trees::name_tree_lookup_in(&editor, root, b"k042"),
        Some(Object::Int(42))
    );
    // The document underneath has none of it.
    assert!(name_tree(editor.document(), root).is_empty());
}

/// The editor's `Resolve` sees a deletion as null and an overlay stream as
/// its dictionary, which is what `DocumentEditor::get` answers.
#[test]
fn the_view_agrees_with_get() {
    let mut editor = DocumentEditor::new(form_fixture());
    let catalog_ref = editor
        .document()
        .trailer()
        .get_ref(Name::ROOT)
        .expect("a root");
    let seen = Resolve::get(&editor, catalog_ref).expect("the catalog");
    assert_eq!(Some(seen.as_ref().clone()), editor.get(catalog_ref));

    let gone = ObjRef::new(5, 0);
    editor.delete(gone);
    assert_eq!(
        Resolve::get(&editor, gone).expect("null").as_ref(),
        &Object::Null
    );
}

/// Records the digest; the blob is not a signature and nothing parses it.
struct Recorder(std::cell::RefCell<Vec<u8>>);

impl Signer for Recorder {
    fn digest_algorithm(&self) -> DigestAlgorithm {
        DigestAlgorithm::Sha256
    }

    fn sign(&self, digest: &[u8]) -> Result<Vec<u8>, SignRefused> {
        *self.0.borrow_mut() = digest.to_vec();
        Ok(vec![0x30; 64])
    }
}

/// A certifying signature's `/Perms /DocMDP` reaches the file (12.8.4).
///
/// `certify` wrote it into the editor *after* the object set for the update
/// had been taken, so the catalog change was made and never saved: the
/// signature's own `/Reference` still said DocMDP, which is why reading the
/// certification back never noticed.
#[test]
fn a_certified_save_writes_perms_into_the_catalog() {
    let mut editor = DocumentEditor::new(form_fixture());
    let signer = Recorder(std::cell::RefCell::new(Vec::new()));
    let mut request = SigningRequest::new(
        SigningTarget::NewInvisibleField {
            name: "Certifier".to_string(),
        },
        &signer,
    );
    request.certification = Some(tinker_pdf_cos::Certification::NoChanges);
    let signed = editor
        .save_signed(&incremental(), &request)
        .expect("signing succeeds");

    let saved = CosDocument::open(signed).expect("reopens");
    let catalog = saved.catalog().expect("a catalog");
    let perms = saved.resolve_key(&catalog, saved.intern(b"Perms"));
    let docmdp = perms
        .as_dict()
        .and_then(|p| p.get_ref(saved.intern(b"DocMDP")))
        .expect("/Perms /DocMDP names the signature");
    let field = fields(&saved)
        .into_iter()
        .find(|f| f.name == "Certifier")
        .expect("the signature field");
    let value = saved
        .get(field.reference)
        .ok()
        .and_then(|o| o.as_dict().and_then(|d| d.get_ref(saved.intern(b"V"))));
    assert_eq!(Some(docmdp), value, "the same signature dictionary");
    // And the catalog kept the form: certification is one more catalog edit.
    assert!(catalog.get(saved.intern(b"AcroForm")).is_some());
}

/// A stream the editor holds with a `/Filter` is read decoded.
///
/// `import_page` copies a page's content streams as the file stored them —
/// compressed bytes and a dictionary naming the filter — and
/// `stream_bytes`, which every overlay read of a stream goes through, handed
/// those bytes back as if they were content. `append_content` then spliced the
/// deflate stream into the page as operators, and the imported page drew
/// nothing it had drawn.
#[test]
fn an_imported_pages_filtered_content_reads_decoded() {
    let mut builder = tinker_pdf_cos::DocumentBuilder::new();
    builder.add_base_font(b"F0", b"Helvetica");
    builder.add_page(200.0, 100.0, |page| {
        // Enough of it that deflate makes it smaller, or the writer keeps it
        // plain and there is no filter to test.
        for line in 0..40 {
            page.text(b"F0", 12.0, 10.0, f64::from(line) * 2.0, "imported words");
        }
    });
    let plain = Arc::new(CosDocument::open(builder.finish()).expect("it opens"));
    let packed = DocumentEditor::new(plain).save(&WriteOptions {
        compress: true,
        ..WriteOptions::default()
    });
    let source = CosDocument::open(packed).expect("the compressed copy opens");
    let first = &tinker_pdf_cos::pages::collect(&source)[0];
    let content = tinker_pdf_cos::pages::contents(&source, first)
        .first()
        .copied()
        .expect("a content stream");
    let stored = source.get(content).expect("it loads");
    assert!(
        stored
            .as_stream()
            .is_some_and(|s| s.dict.get(Name::FILTER).is_some()),
        "the premise: the source's content is filtered"
    );

    let mut editor = DocumentEditor::new(form_fixture());
    editor.import_page(&source, 0, 1).expect("imported");
    assert!(editor.append_content(1, b"0 0 1 rg 0 0 5 5 re f"));

    let saved = reopen(&editor);
    let page = &tinker_pdf_cos::pages::collect(&saved)[1];
    let text =
        String::from_utf8_lossy(&tinker_pdf_cos::pages::content_bytes(&saved, page)).into_owned();
    assert!(
        text.contains("(imported words) Tj"),
        "the imported drawing survives the append: {text:?}"
    );
}
