//! Watermarks and stamps on existing pages: `DocumentEditor::add_resource`,
//! `add_form`, `import_page_as_form` and `stamp`, drawn by this repository's
//! renderer and read back through its reader.
//!
//! Every fixture is a 60-point page, one pixel a point at the default
//! resolution, so a colour at a user-space position is a colour at a pixel.
//! The stamp is a red horizontal stripe from y = 25 to y = 35 across the
//! whole page, and the pages underneath draw in blue and green, so which of
//! them is on top at a point is a question with one-channel answers.

use tinker_pdf::{
    Document, DocumentBuilder, DocumentEditor, FormXObject, ImageData, ObjRef, Object,
    StampPlacement, WriteMode, WriteOptions,
};

mod render_support;
use render_support::pixel;

const PAGE: f64 = 60.0;
const WHITE: (u8, u8, u8) = (255, 255, 255);
const RED: (u8, u8, u8) = (255, 0, 0);
const GREEN: (u8, u8, u8) = (0, 255, 0);
const BLUE: (u8, u8, u8) = (0, 0, 255);
const YELLOW: (u8, u8, u8) = (255, 255, 0);

/// The pixel whose centre is at `(x, y)` in user space on page `index`.
fn at(document: &Document, index: u32, x: f64, y: f64) -> (u8, u8, u8) {
    let bitmap = document
        .page(index)
        .expect("the page")
        .render(&tinker_pdf::RenderOptions::default());
    pixel(&bitmap, x as u32, (PAGE - y) as u32)
}

fn incremental(editor: &DocumentEditor) -> Vec<u8> {
    editor.save(&WriteOptions {
        mode: WriteMode::Incremental,
        ..WriteOptions::default()
    })
}

/// Opens saved bytes and asserts the strict validator has nothing to say.
#[track_caller]
fn clean(bytes: Vec<u8>) -> Document {
    let document = Document::open(bytes).expect("the saved document opens");
    let defects = document.validate();
    assert!(defects.is_empty(), "the strict validator: {defects:?}");
    document
}

/// The red stripe, as a form over the whole page.
fn stripe(editor: &mut DocumentEditor) -> ObjRef {
    editor
        .add_form(
            &FormXObject {
                bbox: [0.0, 0.0, PAGE, PAGE],
                matrix: None,
                group: None,
                content: b"1 0 0 rg 0 25 60 10 re f",
            },
            tinker_pdf::Dict::new(),
        )
        .expect("a form")
}

/// A one-page document drawing a blue square from 10 to 50, inside `q`/`Q`.
fn blue_square() -> Vec<u8> {
    let mut builder = DocumentBuilder::new();
    builder.add_page(PAGE, PAGE, |page| {
        page.raw(b"q 0 0 1 rg 10 10 40 40 re f Q")
    });
    builder.finish()
}

/// The streams page `index`'s `/Contents` names.
fn contents(document: &Document, index: u32) -> Vec<ObjRef> {
    let cos = document.cos();
    let pages = tinker_pdf_cos::pages::collect(cos);
    tinker_pdf_cos::pages::contents(cos, &pages[index as usize])
}

/// Over paints on top of the page, under paints beneath it, and outside the
/// page's own drawing both show.
#[test]
fn a_stamp_over_paints_on_top_and_one_under_paints_beneath() {
    for (placement, inside) in [(StampPlacement::Over, RED), (StampPlacement::Under, BLUE)] {
        let document = Document::open(blue_square()).expect("it opens");
        let mut editor = document.editor();
        let form = stripe(&mut editor);
        assert_eq!(
            editor.stamp(0, form, placement).as_deref(),
            Some(&b"Stamp0"[..])
        );
        let stamped = clean(incremental(&editor));
        assert_eq!(
            at(&stamped, 0, 30.0, 30.0),
            inside,
            "{placement:?}, on the square"
        );
        assert_eq!(at(&stamped, 0, 5.0, 30.0), RED, "{placement:?}, beside it");
        assert_eq!(
            at(&stamped, 0, 30.0, 15.0),
            BLUE,
            "{placement:?}, below the stripe"
        );
        assert_eq!(
            at(&stamped, 0, 5.0, 5.0),
            WHITE,
            "{placement:?}, the corner"
        );
    }
}

/// The page's own content streams are the same objects with the same bytes:
/// the update does not redefine them, the page still names them, and the
/// file the update was appended to is its prefix.
#[test]
fn the_original_content_streams_are_unchanged_in_the_saved_file() {
    let original = blue_square();
    let document = Document::open(original.clone()).expect("it opens");
    let before = contents(&document, 0);
    assert_eq!(before.len(), 1);
    let mut editor = document.editor();
    let form = stripe(&mut editor);
    assert!(editor.stamp(0, form, StampPlacement::Over).is_some());
    let saved = incremental(&editor);

    assert!(saved.starts_with(&original), "an incremental save appends");
    let update = String::from_utf8_lossy(&saved[original.len()..]).into_owned();
    let redefined = format!("\n{} {} obj", before[0].num, before[0].gen);
    assert!(
        !update.contains(&redefined),
        "the update does not redefine the content stream: {update}"
    );

    let stamped = clean(saved);
    let after = contents(&stamped, 0);
    assert_eq!(
        after.len(),
        2,
        "a balanced page is not bracketed: its stream, then the stamp"
    );
    assert_eq!(after[0], before[0], "the page still names its own stream");
    assert_eq!(
        stamped.cos().stream_decoded(after[0]).expect("it decodes"),
        document
            .cos()
            .stream_decoded(before[0])
            .expect("it decodes"),
    );
}

/// Two pages sharing one indirect `/Resources` dictionary, which already
/// names a `Stamp0`: stamping one page copies the dictionary onto that page
/// and leaves the other — and the shared object — alone, and the new name
/// does not take the old one's place.
#[test]
fn two_pages_sharing_one_resources_dictionary_are_stamped_independently() {
    let written = shared_resources();
    // A rewrite first, so the file the stamps are appended to has a clean
    // cross-reference table of its own and the validator below judges the
    // stamps rather than the fixture's hand-made framing.
    let normalised = Document::open(written)
        .expect("the fixture opens")
        .editor()
        .save(&WriteOptions {
            mode: WriteMode::Rewrite,
            ..WriteOptions::default()
        });
    let document = clean(normalised);
    let shared = page_resources(&document, 0);
    assert!(matches!(shared, Some(Object::Ref(_))), "the fixture shares");
    assert_eq!(shared, page_resources(&document, 1));

    let mut editor = document.editor();
    let red = stripe(&mut editor);
    assert_eq!(
        editor.stamp(0, red, StampPlacement::Over).as_deref(),
        Some(&b"Stamp1"[..]),
        "Stamp0 is taken"
    );
    let once = clean(incremental(&editor));
    assert_eq!(at(&once, 0, 5.0, 30.0), RED, "page 0 is stamped");
    assert_eq!(at(&once, 1, 5.0, 30.0), WHITE, "page 1 is not");
    assert_eq!(
        at(&once, 1, 2.0, 2.0),
        GREEN,
        "and still draws its own Stamp0"
    );
    assert_eq!(page_resources(&once, 1), shared, "page 1 still shares");
    assert!(
        !matches!(page_resources(&once, 0), Some(Object::Ref(_))),
        "page 0 has its own copy"
    );

    let yellow = editor
        .add_form(
            &FormXObject {
                bbox: [0.0, 0.0, PAGE, PAGE],
                matrix: None,
                group: None,
                content: b"1 1 0 rg 25 0 10 60 re f",
            },
            tinker_pdf::Dict::new(),
        )
        .expect("a form");
    assert_eq!(
        editor.stamp(1, yellow, StampPlacement::Over).as_deref(),
        Some(&b"Stamp1"[..]),
        "page 1's own copy is independent of page 0's"
    );
    let twice = clean(incremental(&editor));
    assert_eq!(at(&twice, 0, 5.0, 30.0), RED);
    assert_eq!(at(&twice, 0, 30.0, 50.0), BLUE, "no yellow bar on page 0");
    assert_eq!(at(&twice, 1, 30.0, 50.0), YELLOW, "page 1's bar");
    assert_eq!(at(&twice, 1, 5.0, 30.0), WHITE, "no red stripe on page 1");
    assert_eq!(
        at(&twice, 1, 2.0, 2.0),
        GREEN,
        "page 1's Stamp0 still draws"
    );
    assert_eq!(at(&twice, 0, 2.0, 2.0), GREEN, "and page 0's");
}

/// Two pages, one `/Resources 5 0 R` between them, which already names a
/// `Stamp0` — a green 5-point square in the corner both pages draw.
fn shared_resources() -> Vec<u8> {
    let content = "q 0 0 1 rg 10 10 40 40 re f Q /Stamp0 Do";
    let corner = "0 1 0 rg 0 0 5 5 re f";
    format!(
        "%PDF-1.7\n\
1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
2 0 obj\n<< /Type /Pages /Count 2 /Kids [3 0 R 4 0 R] >>\nendobj\n\
3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 60 60] /Resources 5 0 R /Contents 6 0 R >>\nendobj\n\
4 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 60 60] /Resources 5 0 R /Contents 7 0 R >>\nendobj\n\
5 0 obj\n<< /XObject << /Stamp0 8 0 R >> >>\nendobj\n\
6 0 obj\n<< /Length {} >>\nstream\n{content}\nendstream\nendobj\n\
7 0 obj\n<< /Length {} >>\nstream\n{content}\nendstream\nendobj\n\
8 0 obj\n<< /Type /XObject /Subtype /Form /BBox [0 0 60 60] /Length {} >>\nstream\n{corner}\nendstream\nendobj\n\
trailer\n<< /Size 9 /Root 1 0 R >>\n%%EOF\n",
        content.len(),
        content.len(),
        corner.len(),
    )
    .into_bytes()
}

/// The `/Resources` entry of page `index`'s own dictionary, as written.
fn page_resources(document: &Document, index: u32) -> Option<Object> {
    let cos = document.cos();
    let pages = tinker_pdf_cos::pages::collect(cos);
    let object = cos.get(pages[index as usize].reference).ok()?;
    object.as_dict()?.get(tinker_pdf::Name::RESOURCES).cloned()
}

/// A page whose content ends with its state changed — a `cm` outside any
/// `q` — has its streams bracketed, so the stamp is drawn at the page's
/// scale and not the content's.
#[test]
fn a_page_that_leaves_its_state_changed_is_bracketed() {
    let mut builder = DocumentBuilder::new();
    // Twice the size, and the colour left set: an over-stamp run straight
    // after this would draw its stripe from y = 50 to y = 70.
    builder.add_page(PAGE, PAGE, |page| {
        page.raw(b"2 0 0 2 0 0 cm 0 0 1 rg 5 5 10 10 re f")
    });
    let document = Document::open(builder.finish()).expect("it opens");
    let before = contents(&document, 0);
    let mut editor = document.editor();
    let form = stripe(&mut editor);
    assert!(editor.stamp(0, form, StampPlacement::Over).is_some());
    let stamped = clean(incremental(&editor));

    let after = contents(&stamped, 0);
    assert_eq!(after.len(), 3, "q, the page's stream, then Q and the stamp");
    assert_eq!(after[1], before[0], "the page's own stream in the middle");
    assert_eq!(
        stamped.cos().stream_decoded(after[0]).expect("q"),
        b"q\n".to_vec()
    );
    assert_eq!(
        at(&stamped, 0, 45.0, 30.0),
        RED,
        "the stripe at the page's scale"
    );
    assert_eq!(
        at(&stamped, 0, 45.0, 55.0),
        WHITE,
        "and not at the content's"
    );
    assert_eq!(at(&stamped, 0, 20.0, 20.0), BLUE, "the page's own square");
}

/// Resources inherited from the page tree are copied onto the page, with
/// what they already held, and the tree node is not part of the update.
#[test]
fn inherited_resources_are_copied_onto_the_page() {
    let corner = "0 1 0 rg 0 0 5 5 re f";
    let content = "/G Do";
    let written = format!(
        "%PDF-1.7\n\
1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
2 0 obj\n<< /Type /Pages /Count 1 /Kids [3 0 R] /Resources << /XObject << /G 5 0 R >> >> >>\nendobj\n\
3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 60 60] /Contents 4 0 R >>\nendobj\n\
4 0 obj\n<< /Length {} >>\nstream\n{content}\nendstream\nendobj\n\
5 0 obj\n<< /Type /XObject /Subtype /Form /BBox [0 0 60 60] /Length {} >>\nstream\n{corner}\nendstream\nendobj\n\
trailer\n<< /Size 6 /Root 1 0 R >>\n%%EOF\n",
        content.len(),
        corner.len()
    );
    let normalised = Document::open(written.into_bytes())
        .expect("the fixture opens")
        .editor()
        .save(&WriteOptions {
            mode: WriteMode::Rewrite,
            ..WriteOptions::default()
        });
    let document = clean(normalised.clone());
    assert!(page_resources(&document, 0).is_none(), "the page inherits");
    let mut editor = document.editor();
    let form = stripe(&mut editor);
    assert!(editor.stamp(0, form, StampPlacement::Under).is_some());
    let saved = incremental(&editor);
    let stamped = clean(saved.clone());
    assert_eq!(
        at(&stamped, 0, 2.0, 2.0),
        GREEN,
        "the inherited form still draws"
    );
    assert_eq!(at(&stamped, 0, 30.0, 30.0), RED, "and so does the stamp");
    assert!(
        matches!(page_resources(&stamped, 0), Some(Object::Dict(_))),
        "the page carries its own copy now"
    );
    let update = String::from_utf8_lossy(&saved[normalised.len()..]).into_owned();
    let tree = stamped.cos().trailer().get_ref(tinker_pdf::Name::ROOT);
    let tree_num = tree
        .and_then(|root| stamped.cos().get(root).ok())
        .and_then(|catalog| catalog.as_dict()?.get_ref(tinker_pdf::Name::PAGES))
        .expect("a page tree")
        .num;
    assert!(
        !update.contains(&format!("\n{tree_num} 0 obj")),
        "the page tree node is not rewritten: {update}"
    );
}

/// A page of another document, stamped as a form: its drawing and the
/// resources it draws with come across.
#[test]
fn a_page_of_another_document_stamps_as_a_form() {
    let mut letterhead = DocumentBuilder::new();
    assert!(letterhead.add_image(
        b"Logo",
        &ImageData::Rgb8 {
            width: 1,
            height: 1,
            data: &[0, 255, 0],
        },
    ));
    letterhead.add_page(PAGE, PAGE, |page| {
        page.image(b"Logo", 45.0, 45.0, 10.0, 10.0)
    });
    let source = Document::open(letterhead.finish()).expect("the source opens");

    let document = Document::open(blue_square()).expect("it opens");
    let mut editor = document.editor();
    let form = editor
        .import_page_as_form(source.cos(), 0, None)
        .expect("the source page");
    assert!(editor.import_page_as_form(source.cos(), 1, None).is_none());
    assert!(editor.stamp(0, form, StampPlacement::Over).is_some());
    let stamped = clean(incremental(&editor));
    assert_eq!(at(&stamped, 0, 50.0, 50.0), GREEN, "the letterhead's logo");
    assert_eq!(at(&stamped, 0, 30.0, 30.0), BLUE, "the page's own square");
}

/// What cannot be stamped is refused before anything changes.
#[test]
fn what_is_not_a_form_or_a_page_is_refused_and_changes_nothing() {
    let document = Document::open(blue_square()).expect("it opens");
    let content = contents(&document, 0)[0];
    let mut editor = document.editor();
    assert!(
        editor.stamp(0, content, StampPlacement::Over).is_none(),
        "a content stream is not a form"
    );
    assert!(!editor.is_dirty());
    let form = stripe(&mut editor);
    let checkpoint = editor.checkpoint();
    assert!(
        editor.stamp(1, form, StampPlacement::Over).is_none(),
        "no page 1"
    );
    assert!(
        editor
            .add_form(
                &FormXObject {
                    bbox: [0.0, 0.0, 0.0, 10.0],
                    matrix: None,
                    group: None,
                    content: b"",
                },
                tinker_pdf::Dict::new(),
            )
            .is_none(),
        "a form with no area"
    );
    assert_eq!(
        format!("{:?}", editor.checkpoint()),
        format!("{checkpoint:?}")
    );
}

/// `add_resource` on its own: any category, a fresh name past the ones the
/// page has, and the object stored under it.
#[test]
fn add_resource_picks_a_name_the_page_does_not_use() {
    let mut builder = DocumentBuilder::new();
    assert!(builder.add_ext_gstate(
        b"GS0",
        &tinker_pdf::ExtGState {
            fill_alpha: Some(0.5),
            ..tinker_pdf::ExtGState::default()
        },
    ));
    builder.add_page(PAGE, PAGE, |page| assert!(page.set_ext_gstate(b"GS0")));
    let document = Document::open(builder.finish()).expect("it opens");
    let mut editor = document.editor();
    let mut state = tinker_pdf::Dict::new();
    state.insert(editor.intern(b"ca"), Object::Real(0.25));
    assert_eq!(
        editor
            .add_resource(0, b"ExtGState", b"GS", Object::Dict(state))
            .as_deref(),
        Some(&b"GS1"[..])
    );
    assert!(editor.add_resource(0, b"", b"GS", Object::Null).is_none());
    assert!(editor
        .add_resource(3, b"ExtGState", b"GS", Object::Null)
        .is_none());
    let saved = clean(incremental(&editor));
    let cos = saved.cos();
    let pages = tinker_pdf_cos::pages::collect(cos);
    let table = cos.resolve_key(
        pages[0].resources.as_ref().expect("resources"),
        cos.intern(b"ExtGState"),
    );
    let table = table.as_dict().expect("an /ExtGState table");
    assert!(
        table.get(cos.intern(b"GS0")).is_some(),
        "the old entry stays"
    );
    assert!(
        table.get(cos.intern(b"GS1")).is_some(),
        "the new one is added"
    );
}
