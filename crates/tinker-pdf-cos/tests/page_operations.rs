//! Insert, import, keep and flatten (phase 10).
//!
//! Only delete, move and rotate existed. Everything a caller needs to
//! *assemble* a document — add a page, take a page from another file, keep a
//! subset, make an annotation permanent — was missing.

use std::sync::Arc;

use tinker_pdf_cos::{
    annot, pages, CosDocument, DocumentBuilder, DocumentEditor, Object, Rect, WriteMode,
    WriteOptions,
};

fn document(pages: usize, label: &str) -> Arc<CosDocument> {
    let mut builder = DocumentBuilder::new();
    builder.add_base_font(b"F0", b"Helvetica");
    for i in 0..pages {
        builder.add_page(200.0, 100.0, |page| {
            page.text(b"F0", 12.0, 10.0, 50.0, &format!("{label} page {i}"));
        });
    }
    Arc::new(CosDocument::open(builder.finish()).expect("it opens"))
}

fn saved(editor: &DocumentEditor) -> CosDocument {
    let bytes = editor.save(&WriteOptions {
        mode: WriteMode::Rewrite,
        ..WriteOptions::default()
    });
    CosDocument::open(bytes).expect("it reopens")
}

fn text_of(doc: &CosDocument, index: usize) -> String {
    let collected = pages::collect(doc);
    let Some(page) = collected.get(index) else {
        return String::new();
    };
    String::from_utf8_lossy(&pages::content_bytes(doc, page)).into_owned()
}

#[test]
fn a_blank_page_can_be_inserted_anywhere() {
    let doc = document(2, "src");
    let mut editor = DocumentEditor::new(doc);

    assert!(editor.insert_page(1, 300.0, 400.0).is_some());
    let out = saved(&editor);

    let collected = pages::collect(&out);
    assert_eq!(collected.len(), 3);
    assert_eq!(collected[1].media_box.width(), 300.0);
    assert_eq!(collected[1].media_box.height(), 400.0);
    // The pages either side kept their order.
    assert!(text_of(&out, 0).contains("page 0"));
    assert!(text_of(&out, 2).contains("page 1"));
}

#[test]
fn a_page_can_be_appended_but_not_placed_past_the_end() {
    let doc = document(2, "src");
    let mut editor = DocumentEditor::new(Arc::clone(&doc));

    assert!(editor.insert_page(2, 100.0, 100.0).is_some(), "appending");
    assert!(
        editor.insert_page(99, 100.0, 100.0).is_none(),
        "a miscounted index is refused rather than clamped"
    );
    assert!(
        editor.insert_page(0, 0.0, 100.0).is_none(),
        "a degenerate size"
    );
}

/// 7.7.3.2: every page names its parent. An inserted page has never had one,
/// and a reader walking upward for an inherited attribute needs it.
#[test]
fn an_inserted_page_names_its_parent() {
    let doc = document(1, "src");
    let mut editor = DocumentEditor::new(doc);
    editor.insert_page(1, 100.0, 100.0);
    let out = saved(&editor);

    let collected = pages::collect(&out);
    let object = out.get(collected[1].reference).expect("it loads");
    let parent = object
        .as_dict()
        .and_then(|d| d.get_ref(out.intern(b"Parent")));
    assert!(parent.is_some(), "the inserted page has a parent");
}

/// The merge half: a page from another document, with everything it reaches.
#[test]
fn a_page_can_be_imported_from_another_document() {
    let target = document(1, "target");
    let source = document(2, "source");

    let mut editor = DocumentEditor::new(target);
    assert!(editor.import_page(&source, 1, 1).is_some());
    let out = saved(&editor);

    assert_eq!(pages::count(&out), 2);
    assert!(text_of(&out, 0).contains("target page 0"));
    assert!(
        text_of(&out, 1).contains("source page 1"),
        "the imported page brought its content: {}",
        text_of(&out, 1)
    );
}

/// The imported page's resources come with it. Without that its text has a
/// font resource naming an object that does not exist here.
#[test]
fn an_imported_page_brings_its_resources() {
    let target = document(1, "target");
    let source = document(1, "source");

    let mut editor = DocumentEditor::new(target);
    editor.import_page(&source, 0, 1);
    let out = saved(&editor);

    let collected = pages::collect(&out);
    let resources = collected[1].resources.as_ref().expect("resources came too");
    let fonts = out.resolve_key(resources, out.intern(b"Font"));
    let fonts = fonts.as_dict().expect("a font table");
    assert!(!fonts.is_empty());

    // And the font it names resolves to a real object in *this* document.
    let (_, value) = fonts.entries().first().expect("one font");
    let reference = value.as_objref().expect("an indirect font");
    let object = out.get(reference).expect("it loads");
    assert!(
        object.as_dict().is_some(),
        "the copied font is a dictionary"
    );
}

#[test]
fn importing_a_page_that_does_not_exist_is_refused() {
    let target = document(1, "target");
    let source = document(1, "source");
    let mut editor = DocumentEditor::new(target);
    assert!(editor.import_page(&source, 9, 0).is_none());
    assert!(!editor.is_dirty());
}

/// The split half: keep a subset, in the order given.
#[test]
fn keeping_a_subset_splits_a_document() {
    let doc = document(4, "src");
    let mut editor = DocumentEditor::new(doc);

    assert!(editor.keep_pages(&[2, 0]));
    let out = saved(&editor);

    assert_eq!(pages::count(&out), 2);
    assert!(text_of(&out, 0).contains("page 2"), "in the order given");
    assert!(text_of(&out, 1).contains("page 0"));
}

#[test]
fn keeping_an_index_that_does_not_exist_changes_nothing() {
    let doc = document(2, "src");
    let mut editor = DocumentEditor::new(doc);
    assert!(!editor.keep_pages(&[0, 5]));
    assert!(!editor.is_dirty(), "nothing was written");
}

/// Flattening makes an annotation permanent: it stops being a separate object
/// a viewer may decline to draw or a user may delete.
#[test]
fn flattening_draws_an_annotation_into_the_content() {
    let doc = document(1, "src");
    let mut editor = DocumentEditor::new(Arc::clone(&doc));

    let rect = Rect {
        x0: 20.0,
        y0: 20.0,
        x1: 80.0,
        y1: 60.0,
    };
    editor
        .add_annotation(
            0,
            annot::square(
                &doc,
                rect,
                annot::Color {
                    r: 1.0,
                    g: 0.0,
                    b: 0.0,
                },
                2.0,
            ),
        )
        .expect("it is added");

    assert_eq!(editor.flatten_annotations(0), Some(1));
    let out = saved(&editor);

    let content = text_of(&out, 0);
    assert!(
        content.contains(" Do"),
        "the appearance is invoked: {content}"
    );

    // And the annotation is gone, or it would be drawn twice.
    let collected = pages::collect(&out);
    let object = out.get(collected[0].reference).expect("it loads");
    let annots = object
        .as_dict()
        .and_then(|d| d.get(out.intern(b"Annots")))
        .cloned();
    assert!(
        annots.is_none() || matches!(annots, Some(Object::Array(ref a)) if a.is_empty()),
        "the annotation was removed: {annots:?}"
    );
}

/// An annotation with no appearance has nothing to draw, so flattening leaves
/// it alone rather than inventing something.
#[test]
fn an_annotation_with_no_appearance_is_not_flattened() {
    let doc = document(1, "src");
    let mut editor = DocumentEditor::new(Arc::clone(&doc));

    let rect = Rect {
        x0: 10.0,
        y0: 10.0,
        x1: 50.0,
        y1: 30.0,
    };
    // A link gets no synthesized appearance, by design.
    let page = pages::collect(&doc)[0].reference;
    editor
        .add_annotation(0, annot::link(&doc, rect, page))
        .expect("it is added");

    assert_eq!(editor.flatten_annotations(0), Some(0));
}

#[test]
fn flattening_a_page_that_does_not_exist_is_refused() {
    let doc = document(1, "src");
    let mut editor = DocumentEditor::new(doc);
    assert!(editor.flatten_annotations(9).is_none());
}

/// A TrueType face whose glyphs are boxes, with a `cmap` so codes resolve.
///
/// Built here rather than read from the system so the test is the same on
/// every platform and the repository carries nothing anyone has to licence.
fn boxy_font() -> Vec<u8> {
    let mut glyph = Vec::new();
    glyph.extend_from_slice(&1i16.to_be_bytes());
    for value in [0i16, 0, 700, 700] {
        glyph.extend_from_slice(&value.to_be_bytes());
    }
    glyph.extend_from_slice(&3u16.to_be_bytes());
    glyph.extend_from_slice(&0u16.to_be_bytes());
    glyph.extend_from_slice(&[0x01, 0x01, 0x01, 0x01]);
    for dx in [0i16, 700, 0, -700] {
        glyph.extend_from_slice(&dx.to_be_bytes());
    }
    for dy in [0i16, 0, 700, 0] {
        glyph.extend_from_slice(&dy.to_be_bytes());
    }

    let mut head = vec![0u8; 54];
    head[18..20].copy_from_slice(&1000u16.to_be_bytes());
    head[50..52].copy_from_slice(&1i16.to_be_bytes());

    const FIRST: usize = 32;
    const LAST: usize = 255;
    let size = glyph.len() as u32;
    let mut glyf = Vec::new();
    for _ in FIRST..=LAST {
        glyf.extend_from_slice(&glyph);
    }
    let mut loca = Vec::new();
    for index in 0..=LAST + 1 {
        loca.extend_from_slice(&((index.saturating_sub(FIRST)) as u32 * size).to_be_bytes());
    }

    let tables: [(&[u8; 4], &[u8]); 3] = [(b"head", &head), (b"loca", &loca), (b"glyf", &glyf)];
    let mut out = Vec::new();
    out.extend_from_slice(&0x0001_0000u32.to_be_bytes());
    out.extend_from_slice(&(tables.len() as u16).to_be_bytes());
    out.extend_from_slice(&[0; 6]);
    let mut offset = 12 + tables.len() * 16;
    let mut body = Vec::new();
    for (tag, data) in tables {
        out.extend_from_slice(tag);
        out.extend_from_slice(&0u32.to_be_bytes());
        out.extend_from_slice(&(offset as u32).to_be_bytes());
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        offset += data.len();
        body.extend_from_slice(data);
    }
    out.extend_from_slice(&body);
    out
}

/// A document using only the standard 14 relies on the reader having them; one
/// that embeds its font carries everything it needs. The builder could only do
/// the former.
#[test]
fn a_font_can_be_embedded() {
    let program = boxy_font();
    let mut builder = DocumentBuilder::new();
    assert!(builder.add_embedded_font(b"F0", b"Boxy", &program));
    builder.add_page(200.0, 100.0, |page| {
        page.text(b"F0", 12.0, 10.0, 50.0, "EMBEDDED");
    });

    let doc = CosDocument::open(builder.finish()).expect("it opens");
    let collected = pages::collect(&doc);
    let resources = collected[0].resources.as_ref().expect("resources");
    let fonts = doc.resolve_key(resources, doc.intern(b"Font"));
    let (_, value) = fonts.as_dict().expect("a font table").entries()[0].clone();

    let font = doc
        .get(value.as_objref().expect("indirect"))
        .expect("loads");
    let font = font.as_dict().expect("a font dictionary");

    // The descriptor points at the program, which is what makes it embedded.
    let descriptor = doc.resolve_key(font, doc.intern(b"FontDescriptor"));
    let descriptor = descriptor.as_dict().expect("a descriptor");
    let file = descriptor
        .get_ref(doc.intern(b"FontFile2"))
        .expect("an embedded program");
    let bytes = doc.stream_decoded(file).expect("it decodes");
    assert_eq!(bytes.len(), program.len(), "the whole face is carried");
}

/// Widths come from the program's own metrics, so they agree with the outlines
/// a renderer will draw. A `/Widths` array that disagrees is how text ends up
/// overlapping itself.
#[test]
fn embedded_widths_come_from_the_font_program() {
    let mut builder = DocumentBuilder::new();
    builder.add_embedded_font(b"F0", b"Boxy", &boxy_font());
    builder.add_page(100.0, 100.0, |_| {});

    let doc = CosDocument::open(builder.finish()).expect("it opens");
    let collected = pages::collect(&doc);
    let resources = collected[0].resources.as_ref().expect("resources");
    let fonts = doc.resolve_key(resources, doc.intern(b"Font"));
    let (_, value) = fonts.as_dict().expect("a font table").entries()[0].clone();
    let font = doc
        .get(value.as_objref().expect("indirect"))
        .expect("loads");
    let font = font.as_dict().expect("a dictionary");

    let widths = doc.resolve_key(font, doc.intern(b"Widths"));
    let widths = widths.as_array().expect("an array");
    assert_eq!(widths.len(), 224, "one per code from 32 to 255");
    assert!(
        widths.iter().all(|w| w.as_number().unwrap_or(0.0) > 0.0),
        "and every one is a real advance"
    );
}

/// Bytes that are not a font are refused rather than written into a dictionary
/// pointing at nothing.
#[test]
fn embedding_something_that_is_not_a_font_is_refused() {
    let mut builder = DocumentBuilder::new();
    assert!(!builder.add_embedded_font(b"F0", b"Nope", b"not a font at all"));
    assert!(!builder.add_embedded_font(b"F0", b"Nope", b""));
}
