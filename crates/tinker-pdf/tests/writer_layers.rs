//! Optional content on write, read back by `Document::layers()` and drawn by
//! this repository's renderer (8.11).
//!
//! The reader half landed first and is the adjudicator here: `layers()` lists
//! the catalog's `/OCGs` with the visibility the renderer acts on, from the
//! one `OptionalContent::bind` both of them call. So a layer written, listed
//! and drawn is three views of one statement, and a writer that got the
//! configuration wrong would be caught by the list and the page together.

use tinker_pdf::{
    ArchivalLevel, ArchivalPart, ArchivalProfile, ArchivalRefusal, DeviceSpace, Document,
    DocumentBuilder, Tag, WriteMode, WriteOptions,
};

mod pdfa_support;
mod render_support;
use render_support::{curvy_font, pixel, render};

const PAGE: f64 = 60.0;

/// The pixel whose centre is at `(x, y)` in user space.
fn at(bitmap: &tinker_pdf::Bitmap, x: f64, y: f64) -> (u8, u8, u8) {
    pixel(bitmap, x as u32, (PAGE - y) as u32)
}

const WHITE: (u8, u8, u8) = (255, 255, 255);
const RED: (u8, u8, u8) = (255, 0, 0);
const BLUE: (u8, u8, u8) = (0, 0, 255);

/// Every layer the builder writes comes back through `layers()` in the order
/// it was added, with its name decoded as the text it was given and the
/// default visibility it was given.
#[test]
fn layers_lists_each_written_group_with_its_name_and_default() {
    for version in [(1, 7), (2, 0)] {
        let mut builder = DocumentBuilder::with_version(version.0, version.1);
        let construction = builder.add_layer("Construction", false).expect("a layer");
        let notes = builder.add_layer("Notes — über", true).expect("a layer");
        let grid = builder.add_layer("Grid", true).expect("a layer");
        builder.add_page(PAGE, PAGE, |page| {
            for layer in [construction, notes, grid] {
                assert!(page.optional(layer, |page| page.fill_rect(0.0, 0.0, 1.0, 1.0, 0.0)));
            }
        });
        let document = Document::open(builder.finish()).expect("it opens");
        let listed: Vec<(String, bool)> = document
            .layers()
            .into_iter()
            .map(|group| (group.name, group.visible))
            .collect();
        assert_eq!(
            listed,
            [
                ("Construction".to_string(), false),
                ("Notes — über".to_string(), true),
                ("Grid".to_string(), true),
            ],
            "PDF {}.{}",
            version.0,
            version.1
        );
        assert!(document.validate().is_empty(), "{:?}", document.validate());
    }
}

/// Content in a hidden layer is not painted; the editor turns it on in the
/// default configuration, and the saved document paints it. The other layer
/// goes the other way in the same save, so a build that flipped every layer,
/// or none, fails one half.
#[test]
fn a_hidden_layer_is_not_painted_until_the_editor_shows_it() {
    let mut builder = DocumentBuilder::new();
    let hidden = builder.add_layer("Hidden", false).expect("a layer");
    let shown = builder.add_layer("Shown", true).expect("a layer");
    builder.add_page(PAGE, PAGE, |page| {
        assert!(page.optional(hidden, |page| {
            page.set_fill_rgb(1.0, 0.0, 0.0);
            page.raw(b"5 20 20 20 re f");
        }));
        assert!(page.optional(shown, |page| {
            page.set_fill_rgb(0.0, 0.0, 1.0);
            page.raw(b"35 20 20 20 re f");
        }));
    });
    let bytes = builder.finish();

    let before = render(bytes.clone());
    assert_eq!(
        at(&before, 15.0, 30.0),
        WHITE,
        "the hidden layer is not painted"
    );
    assert_eq!(at(&before, 45.0, 30.0), BLUE, "the shown layer is");

    let document = Document::open(bytes).expect("it opens");
    let layers = document.layers();
    let mut editor = document.editor();
    assert!(editor.set_layer_visible(layers[0].reference, true));
    assert!(editor.set_layer_visible(layers[1].reference, false));
    let saved = editor.save(&WriteOptions {
        mode: WriteMode::Incremental,
        ..WriteOptions::default()
    });

    let reopened = Document::open(saved.clone()).expect("the update opens");
    let visible: Vec<bool> = reopened.layers().iter().map(|g| g.visible).collect();
    assert_eq!(visible, [true, false], "the configuration was toggled");
    assert!(reopened.validate().is_empty(), "{:?}", reopened.validate());
    let after = render(saved);
    assert_eq!(
        at(&after, 15.0, 30.0),
        RED,
        "the layer shown now is painted"
    );
    assert_eq!(
        at(&after, 45.0, 30.0),
        WHITE,
        "and the one hidden now is not"
    );
}

/// Tagged content inside a layer, and a layer inside tagged content, read
/// back in structure order with nothing orphaned and nothing unmarked, and
/// the strict validator has nothing to say — which is what the builder's
/// splitting of a sequence around a layer is for.
#[test]
fn nested_tagged_and_optional_content_reads_in_order_and_validates() {
    let mut builder = DocumentBuilder::new();
    assert!(builder.add_embedded_font(b"F1", b"Curvy", &curvy_font()));
    let layer = builder.add_layer("Aside", true).expect("a layer");
    builder.add_page(200.0, 100.0, |page| {
        page.tagged(b"P", |page| {
            page.text(b"F1", 10.0, 5.0, 80.0, "AB");
            assert!(page.optional(layer, |page| {
                page.text(b"F1", 10.0, 45.0, 80.0, "CD");
            }));
            page.text(b"F1", 10.0, 85.0, 80.0, "EF");
        });
        assert!(page.optional(layer, |page| {
            page.tagged(b"H1", |page| page.text(b"F1", 10.0, 5.0, 40.0, "GH"));
        }));
    });
    let document = Document::open(builder.finish()).expect("it opens");
    assert!(document.validate().is_empty(), "{:?}", document.validate());

    let page = document.page(0).expect("a page");
    let structured = page.structured_text().expect("the document is tagged");
    assert_eq!(structured.orphans, 0, "every marked run is claimed");
    assert_eq!(structured.unmarked, 0, "and nothing was drawn unmarked");
    let text: String = structured
        .plain_text()
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect();
    assert_eq!(
        text, "ABCDEFGH",
        "the paragraph reads in order around its layer"
    );
}

/// ISO 19005-1 6.1.13 forbids optional content and the builder refuses a
/// layer by that clause; ISO 19005-2 admits it, and a level A document with
/// a layer inside a tagged element and a tagged element inside a layer
/// conforms to its own validator with no findings.
#[test]
fn part_one_refuses_a_layer_and_part_two_a_keeps_one_and_conforms() {
    let profile = |part, level| ArchivalProfile {
        part,
        level: Some(level),
        destination_profile: pdfa_support::srgb_like(),
        destination_space: DeviceSpace::Rgb,
        output_condition: "Custom".to_string(),
        language: Some("en-GB".to_string()),
    };

    let mut one = DocumentBuilder::archival(profile(ArchivalPart::One, ArchivalLevel::B));
    assert_eq!(one.add_layer("Refused", true), None);
    assert_eq!(one.refusals(), &[ArchivalRefusal::OptionalContent]);

    let mut two = DocumentBuilder::archival(profile(ArchivalPart::Two, ArchivalLevel::A));
    let layer = two.add_layer("Kept", false).expect("part 2 admits layers");
    two.add_page(PAGE, PAGE, |page| {
        page.tagged(b"P", |page| {
            page.fill_rect(1.0, 1.0, 5.0, 5.0, 0.0);
            assert!(page.optional(layer, |page| page.fill_rect(10.0, 10.0, 5.0, 5.0, 0.0)));
        });
        assert!(page.optional(layer, |page| {
            page.tagged(b"Figure", |page| page.fill_rect(20.0, 20.0, 5.0, 5.0, 0.0));
        }));
    });
    assert!(two.refusals().is_empty(), "{:?}", two.refusals());
    let bytes = two.finish_archival().expect("the profile is satisfiable");
    let document = Document::open(bytes).expect("it opens");
    let verdict = document.validate_pdfa();
    assert!(verdict.coverage.is_complete());
    assert_eq!(verdict.findings.len(), 0, "{:#?}", verdict.findings);
    assert_eq!(
        verdict.flavour.map(|f| f.to_string()),
        Some("PDF/A-2A".to_string())
    );
    assert!(document.validate().is_empty(), "{:?}", document.validate());
}

/// An element whose child sits in a hidden layer: the child is hidden and
/// the element's own content either side of it is not.
///
/// This is the nesting the builder has to get right. The child's `tagged`
/// closes its parent's sequence before opening its own, and if that parent
/// sequence were outside the layer the close would be an `EMC` ending the
/// layer instead — the child would be drawn in plain view, and every byte of
/// the stream would still balance.
#[test]
fn an_element_in_a_hidden_layer_inside_a_visible_element_is_hidden() {
    let mut builder = DocumentBuilder::new();
    let layer = builder.add_layer("Hidden", false).expect("a layer");
    builder.add_page(PAGE, PAGE, |page| {
        page.tagged(b"Div", |page| {
            page.set_fill_rgb(0.0, 0.0, 1.0);
            page.raw(b"5 5 10 50 re f");
            assert!(page.optional(layer, |page| {
                page.tagged(b"Span", |page| {
                    page.set_fill_rgb(1.0, 0.0, 0.0);
                    page.raw(b"25 5 10 50 re f");
                });
                page.raw(b"25 5 10 10 re f");
            }));
            page.raw(b"45 5 10 50 re f");
        });
    });
    let bytes = builder.finish();
    let document = Document::open(bytes.clone()).expect("it opens");
    assert!(document.validate().is_empty(), "{:?}", document.validate());
    let bitmap = render(bytes);
    assert_eq!(at(&bitmap, 10.0, 30.0), BLUE, "the element's first piece");
    assert_eq!(
        at(&bitmap, 30.0, 30.0),
        WHITE,
        "the child in the hidden layer"
    );
    assert_eq!(at(&bitmap, 30.0, 10.0), WHITE, "and what follows it there");
    // A layer is marked content, not a graphics-state scope: 8.11.3.2 hides
    // what it marks and still applies what it sets, so the colour the hidden
    // child chose is the one the element's last piece is painted in.
    assert_eq!(at(&bitmap, 50.0, 30.0), RED, "the element's last piece");
}

/// The first page's content stream, as bytes.
fn page_content(document: &Document) -> Vec<u8> {
    let cos = document.cos();
    let pages = tinker_pdf_cos::pages::collect(cos);
    let page = pages.first().expect("a page");
    tinker_pdf_cos::pages::content_bytes(cos, page)
}

/// How many marked-content sequences a content stream opens, and how many it
/// closes. Counted as operator tokens, so a `BDC` inside a string is not one.
fn sequences(content: &[u8]) -> (usize, usize) {
    let tokens = content
        .split(|byte| byte.is_ascii_whitespace())
        .filter(|token| !token.is_empty());
    let (mut open, mut close) = (0, 0);
    for token in tokens {
        match token {
            b"BDC" | b"BMC" => open += 1,
            b"EMC" => close += 1,
            _ => {}
        }
    }
    (open, close)
}

/// A layer's closure is a scope the way a `tagged_with` closure is:
/// `close_tag` inside it cannot reach an element opened outside it, and an
/// element it opens and leaves open is closed when it returns.
///
/// Both are what its `EMC` needs. A layer and an element are both
/// marked-content sequences and `EMC` closes whichever is innermost, so an
/// element closed from inside the layer left the layer one `EMC` too many —
/// three sequences opened against four closed — and an element left open
/// inside it took the layer's `EMC` for its own, so what was drawn after the
/// closure returned stayed inside the layer and hid with it. The review of
/// the tagged-writing lane found both through public calls and nothing
/// refused either.
#[test]
fn a_layer_closure_closes_what_it_opened_and_nothing_outside_it() {
    // An element opened outside the layer and closed after it.
    let mut builder = DocumentBuilder::new();
    assert!(builder.add_embedded_font(b"F1", b"Curvy", &curvy_font()));
    let layer = builder.add_layer("Aside", true).expect("a layer");
    builder.add_page(200.0, 100.0, |page| {
        assert!(page.open_tag(&Tag::new(b"P")));
        page.text(b"F1", 10.0, 5.0, 80.0, "AB");
        assert!(page.optional(layer, |page| {
            page.text(b"F1", 10.0, 45.0, 80.0, "CD");
            assert!(!page.close_tag(), "the P was opened outside the layer");
            page.text(b"F1", 10.0, 85.0, 80.0, "EF");
        }));
        page.text(b"F1", 10.0, 125.0, 80.0, "GH");
        assert!(page.close_tag(), "the P, after the layer");
        assert!(!page.close_tag(), "nothing else is open");
    });
    let document = Document::open(builder.finish()).expect("it opens");
    let (open, close) = sequences(&page_content(&document));
    assert_eq!(open, close, "every sequence opened is closed once");
    let page = document.page(0).expect("a page");
    let structured = page.structured_text().expect("the document is tagged");
    assert_eq!(structured.orphans, 0, "every marked run is claimed");
    assert_eq!(structured.unmarked, 0, "and nothing was drawn unmarked");
    let text: String = structured
        .plain_text()
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect();
    assert_eq!(
        text, "ABCDEFGH",
        "one paragraph, in order, around its layer"
    );
    let tree = document.structure().expect("a tree");
    let paragraphs = tree
        .elements()
        .into_iter()
        .filter(|element| element.standard_type == "P")
        .count();
    assert_eq!(paragraphs, 1, "the P is one element");

    // An element opened inside a hidden layer and left open there.
    let mut builder = DocumentBuilder::new();
    let layer = builder.add_layer("Hidden", false).expect("a layer");
    builder.add_page(PAGE, PAGE, |page| {
        assert!(page.optional(layer, |page| {
            assert!(page.open_tag(&Tag::new(b"Figure")));
            page.set_fill_rgb(1.0, 0.0, 0.0);
            page.raw(b"5 5 10 50 re f");
        }));
        page.set_fill_rgb(0.0, 0.0, 1.0);
        page.raw(b"25 5 10 50 re f");
        assert!(!page.close_tag(), "the layer closed the Figure");
    });
    let bytes = builder.finish();
    let document = Document::open(bytes.clone()).expect("it opens");
    let (open, close) = sequences(&page_content(&document));
    assert_eq!(open, close, "every sequence opened is closed once");
    let bitmap = render(bytes);
    assert_eq!(at(&bitmap, 10.0, 30.0), WHITE, "the Figure, in the layer");
    assert_eq!(
        at(&bitmap, 30.0, 30.0),
        BLUE,
        "what was drawn after the layer's closure returned is not in it"
    );
}
