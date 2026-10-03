//! What the EPUB painter writes for the properties that move no box: asserted
//! on the **operators** and the resources they name.
//!
//! A reftest compares where two spellings put their lines, and every property
//! here leaves every line where it was: `opacity` changes an alpha, and a page
//! drawn without it has the same geometry exactly. So this file reads the page
//! back out of the PDF the book became — the content stream, tokenised, and the
//! `/ExtGState` each `gs` names — and asserts the number the specification
//! gives, computed beside the assertion. Where a pixel can say what an operator
//! cannot (that the name resolves at all), a render says it.
//!
//! Every test pairs its claim with the page that would make the claim false:
//! the same book without the property must not write what this one writes.

mod epub_support;

use epub_support::{ocf_zip, OcfEntry};
use tinker_pdf::{ArchiveWarning, Document, RenderOptions};

const CONTAINER: &str = concat!(
    r#"<?xml version="1.0" encoding="UTF-8"?>"#,
    r#"<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">"#,
    r#"<rootfiles><rootfile full-path="EPUB/content.opf" media-type="application/oebps-package+xml"/>"#,
    r#"</rootfiles></container>"#
);

const PACKAGE: &str = concat!(
    r#"<?xml version="1.0" encoding="utf-8"?>"#,
    r#"<package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="id">"#,
    r#"<metadata xmlns:dc="http://purl.org/dc/elements/1.1/">"#,
    r#"<dc:identifier id="id">urn:uuid:6a6a6a6a-0000-4000-8000-000000000001</dc:identifier>"#,
    r#"<dc:title>Paint</dc:title><dc:language>en</dc:language></metadata><manifest>"#,
    r#"<item id="c1" href="ch1.xhtml" media-type="application/xhtml+xml"/>"#,
    r#"</manifest><spine><itemref idref="c1"/></spine></package>"#
);

/// A one-chapter book set by `style`, whose body is `body`.
fn book(style: &str, body: &str) -> Vec<u8> {
    let chapter = format!(
        concat!(
            r#"<?xml version="1.0" encoding="utf-8"?>"#,
            r#"<html xmlns="http://www.w3.org/1999/xhtml"><head><title>A Chapter</title>"#,
            r#"<style>body {{ margin: 0 }} p {{ margin: 0 }} {}</style></head><body>{}</body></html>"#
        ),
        style, body
    );
    let entries = vec![
        OcfEntry::stored("mimetype", b"application/epub+zip"),
        OcfEntry::deflated("META-INF/container.xml", CONTAINER.as_bytes()),
        OcfEntry::deflated("EPUB/content.opf", PACKAGE.as_bytes()),
        OcfEntry::deflated("EPUB/ch1.xhtml", chapter.as_bytes()),
    ];
    let directory: Vec<usize> = (0..entries.len()).collect();
    ocf_zip(&entries, &directory)
}

fn open(style: &str, body: &str) -> Document {
    Document::open(book(style, body)).expect("the book opens")
}

/// The first page's content stream, as whitespace-separated tokens.
fn tokens(doc: &Document) -> Vec<String> {
    let cos = doc.cos();
    let pages = tinker_pdf_cos::pages::collect(cos);
    let page = pages.first().expect("a page");
    String::from_utf8_lossy(&tinker_pdf_cos::pages::content_bytes(cos, page))
        .split_whitespace()
        .map(str::to_owned)
        .collect()
}

/// Every `gs` on the first page, as the `(/ca, /CA)` its resource states, in
/// the order the stream applies them.
fn alphas(doc: &Document) -> Vec<(f64, f64)> {
    let cos = doc.cos();
    let pages = tinker_pdf_cos::pages::collect(cos);
    let page = pages.first().expect("a page");
    let resources = page.resources.as_ref().expect("the page has resources");
    let states = cos.resolve_key(resources, cos.intern(b"ExtGState"));
    let words = tokens(doc);
    let mut out = Vec::new();
    for (at, word) in words.iter().enumerate() {
        if word != "gs" || at == 0 {
            continue;
        }
        let name = words[at - 1].trim_start_matches('/');
        let states = states.as_dict().expect("an /ExtGState dictionary");
        let state = cos.resolve_key(states, cos.intern(name.as_bytes()));
        let state = state
            .as_dict()
            .expect("the `gs` names a resource the page holds");
        let read = |key: &[u8]| {
            state
                .get(cos.intern(key))
                .and_then(|value| value.as_number())
                .expect("an alpha")
        };
        out.push((read(b"ca"), read(b"CA")));
    }
    out
}

fn warnings(doc: &Document) -> Vec<ArchiveWarning> {
    doc.archive().expect("a report").warnings().to_vec()
}

fn counted(doc: &Document, name: &str) -> Option<usize> {
    warnings(doc).into_iter().find_map(|warning| match warning {
        ArchiveWarning::UnimplementedProperty { property, elements } if property == name => {
            Some(elements)
        }
        _ => None,
    })
}

/// The darkest grey level on the first page, rendered: 0 is black ink, 255 no
/// ink at all.
fn darkest(doc: &Document) -> u8 {
    let bitmap = doc
        .page(0)
        .expect("a page")
        .render(&RenderOptions::default());
    let components = bitmap.components();
    let mut darkest = u8::MAX;
    for y in 0..bitmap.height as usize {
        for x in 0..bitmap.width as usize {
            let start = y * bitmap.stride + x * components;
            if let Some(pixel) = bitmap.data.get(start..start + components) {
                let level = pixel.iter().take(3).copied().min().unwrap_or(u8::MAX);
                darkest = darkest.min(level);
            }
        }
    }
    darkest
}

// ---- opacity --------------------------------------------------------------------

/// **`opacity` is an `/ExtGState` whose `/ca` and `/CA` are the value**, set
/// around the glyphs of the element it is on (`css-color-4` §15.1, ISO 32000
/// 11.6.4.4).
///
/// And it is drawn: black text at a half renders no darker than mid-grey,
/// where the same book without the property renders black.
#[test]
fn opacity_is_an_alpha_on_the_elements_fragments() {
    let faded = open("p { opacity: 0.5; font-size: 48px }", "<p>faded</p>");
    assert_eq!(alphas(&faded), [(0.5, 0.5)]);
    let words = tokens(&faded);
    let gs = words.iter().position(|w| w == "gs").expect("a gs");
    let bt = words.iter().position(|w| w == "BT").expect("a text object");
    assert!(gs < bt, "the alpha is set before the glyphs: {words:?}");
    let plain = open("p { font-size: 48px }", "<p>faded</p>");
    assert!(alphas(&plain).is_empty(), "no opacity, no `gs`");

    // The render, on a box rather than on glyphs: the standard 14 carry no
    // outlines here, so text draws no pixels to measure, and a black box is
    // the same `gs` around a different fragment.
    let block = "div { height: 40px; background-color: #000000 }";
    let faded_box = open(&format!("{block} div {{ opacity: 0.5 }}"), "<div></div>");
    let plain_box = open(block, "<div></div>");
    let (faded_ink, plain_ink) = (darkest(&faded_box), darkest(&plain_box));
    assert!(
        (120..=136).contains(&faded_ink),
        "half-alpha black over white is mid-grey: {faded_ink}"
    );
    assert_eq!(plain_ink, 0, "opaque black is black");
}

/// **Opacity composes down the tree** and is not inherited: a paragraph at
/// one half inside a section at one half is at a quarter, which is the group
/// result §15.1 states for content that does not overlap — and a percentage
/// is the same number.
#[test]
fn nested_opacities_multiply() {
    let doc = open(
        "div { opacity: 50% } p { opacity: 0.5 }",
        "<div><p>quarter</p></div>",
    );
    assert_eq!(alphas(&doc), [(0.25, 0.25)]);
    assert_eq!(
        counted(&doc, "opacity"),
        None,
        "nothing overlaps, so it is exact"
    );
    // A value past one is valid and clamped (§15.1), so it is opaque.
    let clamped = open("p { opacity: 3 }", "<p>opaque</p>");
    assert!(alphas(&clamped).is_empty());
}

/// **A background with content over it is where per-fragment alpha and the
/// group differ**, and that element is counted against `opacity` rather than
/// drawn as though exact. Its background and its text are both still faded.
#[test]
fn opacity_over_a_painted_box_with_content_is_counted() {
    let doc = open(
        "div { opacity: 0.5; background-color: #ff0000 }",
        "<div><p>over red</p></div>",
    );
    assert_eq!(counted(&doc, "opacity"), Some(1));
    assert_eq!(
        alphas(&doc),
        [(0.5, 0.5), (0.5, 0.5)],
        "the box and the text"
    );
    // A painted box with nothing in it overlaps nothing.
    let empty = open(
        "div { opacity: 0.5; background-color: #ff0000; height: 20px }",
        "<div></div><p>beside</p>",
    );
    assert_eq!(counted(&empty, "opacity"), None);
}
