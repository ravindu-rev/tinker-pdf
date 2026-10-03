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

// ---- border-radius and outline -----------------------------------------------------

/// The page box's margin and height, in points, which place every box below.
const MARGIN: f64 = 36.0;
const PAGE_HEIGHT: f64 = 648.0;
/// CSS 2.2 §4.3.2's reference pixel against a point.
const PX: f64 = 0.75;
/// `4(√2 − 1) / 3`, the quarter-arc constant, computed rather than copied.
fn quarter_arc() -> f64 {
    4.0 * (2.0f64.sqrt() - 1.0) / 3.0
}

/// Every `c` on the first page, as its six operands, in stream order.
fn curves(doc: &Document) -> Vec<[f64; 6]> {
    let words = tokens(doc);
    let mut out = Vec::new();
    for (at, word) in words.iter().enumerate() {
        if word != "c" || at < 6 {
            continue;
        }
        let mut operands = [0.0; 6];
        for (slot, text) in operands.iter_mut().zip(&words[at - 6..at]) {
            *slot = text.parse().expect("a `c` operand");
        }
        out.push(operands);
    }
    out
}

#[track_caller]
fn close(actual: [f64; 6], expected: [f64; 6]) {
    for (a, e) in actual.iter().zip(expected) {
        assert!((a - e).abs() < 1e-9, "{actual:?} against {expected:?}");
    }
}

/// The top-right corner's cubic for a box at the content area's top left,
/// `width` CSS pixels wide, with semi-axes `(rx, ry)` CSS pixels: the closed
/// form, from the box's own edges.
fn top_right(width: f64, rx: f64, ry: f64) -> [f64; 6] {
    let right = MARGIN + width * PX;
    let top = PAGE_HEIGHT - MARGIN;
    let (rx, ry) = (rx * PX, ry * PX);
    let k = quarter_arc();
    [
        right - rx + k * rx,
        top,
        right,
        top - ry + k * ry,
        right,
        top - ry,
    ]
}

/// **A rounded corner is a cubic whose control points sit `4(√2−1)/3` of the
/// way along its tangents** (`css-backgrounds-3` §5), drawn for the
/// background; a square box writes no curve at all.
#[test]
fn a_rounded_corner_is_the_quarter_arc_cubic() {
    let doc = open(
        "div { width: 100px; height: 40px; background-color: #ff0000; border-radius: 10px }",
        "<div></div>",
    );
    let found = curves(&doc);
    assert_eq!(found.len(), 4, "four corners: {found:?}");
    close(found[0], top_right(100.0, 10.0, 10.0));
    let square = open(
        "div { width: 100px; height: 40px; background-color: #ff0000 }",
        "<div></div>",
    );
    assert!(
        curves(&square).is_empty(),
        "a square box is the old rectangle"
    );
}

/// **§5.5's overlap scaling scales all four radii by one factor**: 30 pixels
/// on a box 40 high need 60 of a 40-pixel side, so every radius is two thirds
/// of itself — 20 — and the horizontal ones too, which a per-side clamp would
/// have left at 30.
#[test]
fn overlapping_radii_are_scaled_by_one_factor() {
    let doc = open(
        "div { width: 100px; height: 40px; background-color: #ff0000; border-radius: 30px }",
        "<div></div>",
    );
    close(curves(&doc)[0], top_right(100.0, 20.0, 20.0));
}

/// **`h / v` is an ellipse**, and the shorthand's lists expand clockwise:
/// `40px 0 0 0 / 10px` is one wide, flat corner at the top left.
#[test]
fn a_slash_radius_is_an_elliptical_corner() {
    let doc = open(
        "div { width: 100px; height: 40px; background-color: #ff0000; \
               border-radius: 40px 0 0 0 / 10px }",
        "<div></div>",
    );
    let found = curves(&doc);
    assert_eq!(found.len(), 1, "{found:?}");
    let left = MARGIN;
    let top = PAGE_HEIGHT - MARGIN;
    let (rx, ry, k) = (40.0 * PX, 10.0 * PX, quarter_arc());
    close(
        found[0],
        [
            left,
            top - ry + k * ry,
            left + rx - k * rx,
            top,
            left + rx,
            top,
        ],
    );
}

/// **A rounded border is a ring between two rounded paths**, filled even-odd,
/// one clipped region per side; the padding edge's radius is the border
/// edge's less the border width (§5.3).
#[test]
fn a_rounded_border_is_a_ring_per_side() {
    let doc = open(
        "div { width: 100px; height: 40px; border: 4px solid #0000ff; border-radius: 10px }",
        "<div></div>",
    );
    let words = tokens(&doc);
    let rings = words.iter().filter(|w| *w == "f*").count();
    let clips = words.iter().filter(|w| *w == "W").count();
    assert_eq!((rings, clips), (4, 4), "{words:?}");
    // Eight cubics per ring — four outer, four inner — and the first inner one
    // is the padding box's top-right corner at a radius of 10 − 4 = 6. The
    // `div` is content-box sized, so its border box is 108 wide.
    let found = curves(&doc);
    assert_eq!(found.len(), 32);
    let right = MARGIN + 108.0 * PX - 4.0 * PX;
    let top = PAGE_HEIGHT - MARGIN - 4.0 * PX;
    let (r, k) = (6.0 * PX, quarter_arc());
    close(
        found[4],
        [
            right - r + k * r,
            top,
            right,
            top - r + k * r,
            right,
            top - r,
        ],
    );
}

/// **An outline is four bands `outline-offset` out from the border edge and
/// `outline-width` wide**, in its colour, drawn after the text (CSS 2.2
/// Appendix E's last step), and moving nothing.
#[test]
fn an_outline_is_drawn_outside_the_border_edge_after_the_text() {
    let doc = open(
        "div { width: 100px; outline: 2px solid #0000ff; outline-offset: 4px }",
        "<div>outlined</div>",
    );
    let words = tokens(&doc);
    let colour = words
        .windows(4)
        .position(|w| w == ["0", "0", "1", "rg"])
        .expect("the outline's colour");
    let last_text = words.iter().rposition(|w| w == "ET").expect("the text");
    assert!(colour > last_text, "the outline is drawn after the text");
    let rects: Vec<[f64; 4]> = words[colour..]
        .windows(5)
        .filter(|w| w[4] == "re")
        .map(|w| [0, 1, 2, 3].map(|i| w[i].parse::<f64>().expect("a number")))
        .take(4)
        .collect();
    // The first band is the top one: its left edge 4 + 2 pixels out, its
    // bottom 4 pixels above the border edge.
    let left = MARGIN - 6.0 * PX;
    let width = (100.0 + 12.0) * PX;
    let bottom = PAGE_HEIGHT - MARGIN + 4.0 * PX;
    assert_eq!(rects.len(), 4, "{rects:?}");
    for (actual, expected) in rects[0].iter().zip([left, bottom, width, 2.0 * PX]) {
        assert!((actual - expected).abs() < 1e-9, "{rects:?}");
    }
    // And no box moved for it: the text is where it is without one.
    let plain = tokens(&open("div { width: 100px }", "<div>outlined</div>"));
    let place = |words: &[String]| {
        words
            .iter()
            .position(|w| w == "Td")
            .map(|at| words[at - 2..at].to_vec())
    };
    assert_eq!(place(&words), place(&plain));
}

/// **A rounded box cut by a page boundary is sliced, not cloned**
/// (`css-break-3` §5.4's `box-decoration-break: slice`, the initial value): the
/// first page's fragment rounds only its top corners and the last page's only
/// its bottom ones, so the curves are at the box's real ends and not at the
/// page's.
#[test]
fn a_rounded_box_cut_across_pages_rounds_only_its_real_ends() {
    let lines = "<p>line</p>".repeat(120);
    let doc = open(
        "div { background-color: #ff0000; border-radius: 10px }",
        &format!("<div>{lines}</div>"),
    );
    let on = |page: usize| -> Vec<String> {
        let cos = doc.cos();
        let pages = tinker_pdf_cos::pages::collect(cos);
        String::from_utf8_lossy(&tinker_pdf_cos::pages::content_bytes(cos, &pages[page]))
            .split_whitespace()
            .map(str::to_owned)
            .collect()
    };
    let count = |words: &[String]| words.iter().filter(|w| *w == "c").count();
    assert!(doc.page_count() >= 2, "the box is taller than a page");
    let last = doc.page_count() as usize - 1;
    assert_eq!(
        count(&on(0)),
        2,
        "the first page rounds its top two corners"
    );
    assert_eq!(count(&on(last)), 2, "the last page rounds its bottom two");
    for middle in 1..last {
        assert_eq!(count(&on(middle)), 0, "a middle page is a square slice");
    }
}

// ---- overflow ---------------------------------------------------------------------

/// The grey level at one point of the first page, in PDF points from its bottom
/// left: 0 is black ink, 255 none.
fn ink_at(doc: &Document, x: f64, y: f64) -> u8 {
    let bitmap = doc
        .page(0)
        .expect("a page")
        .render(&RenderOptions::default());
    let scale = f64::from(bitmap.height) / PAGE_HEIGHT;
    let column = (x * scale) as usize;
    let row = ((PAGE_HEIGHT - y) * scale) as usize;
    let components = bitmap.components();
    let start = row * bitmap.stride + column * components;
    bitmap
        .data
        .get(start..start + components)
        .map_or(u8::MAX, |pixel| {
            pixel.iter().take(3).copied().min().unwrap_or(u8::MAX)
        })
}

/// A CSS pixel position in the content area, as page points.
fn at(x: f64, y: f64) -> (f64, f64) {
    (MARGIN + x * PX, PAGE_HEIGHT - MARGIN - y * PX)
}

/// A box that clips and a black box three times wider and five times taller
/// inside it.
const CLIPPING: &str = ".c { width: 100px; height: 40px; overflow: hidden } \
                        .ink { width: 300px; height: 200px; background-color: #000000 }";

/// **Content past an `overflow: hidden` box's padding box is not drawn**
/// (`css-overflow-3` §3.1), and the clip is that padding box: `re W n` at its
/// four edges, computed here from the content area's corner.
///
/// The render says the operator does what it claims: black inside the box,
/// white beside it and below it, where the same book with `overflow: visible`
/// draws the black box whole.
#[test]
fn an_overflowing_box_clips_its_content_to_its_padding_box() {
    let body = r#"<div class="c"><div class="ink"></div></div>"#;
    let clipped = open(CLIPPING, body);
    let words = tokens(&clipped);
    let w = words.iter().position(|word| word == "W").expect("a clip");
    let (left, top) = at(0.0, 0.0);
    let expected = [
        left.to_string(),
        (top - 40.0 * PX).to_string(),
        (100.0 * PX).to_string(),
        (40.0 * PX).to_string(),
        "re".to_owned(),
    ];
    assert_eq!(words[w - 5..w], expected, "the padding box: {words:?}");
    assert_eq!(words[w + 1], "n");

    let inside = at(50.0, 20.0);
    let beside = at(200.0, 20.0);
    let below = at(50.0, 100.0);
    assert_eq!(ink_at(&clipped, inside.0, inside.1), 0, "inside the box");
    assert_eq!(ink_at(&clipped, beside.0, beside.1), 255, "beside it");
    assert_eq!(ink_at(&clipped, below.0, below.1), 255, "below it");

    let spilled = open(
        &CLIPPING.replace("overflow: hidden", "overflow: visible"),
        body,
    );
    assert!(!tokens(&spilled).contains(&"W".to_owned()), "nothing clips");
    assert_eq!(ink_at(&spilled, beside.0, beside.1), 0, "drawn beside it");
}

/// **An element's own clip cuts its own text**, which its box does not: the
/// glyphs of a line too long for an `overflow: hidden` box are drawn inside
/// the clip, where the box's background is drawn outside it.
#[test]
fn an_elements_clip_cuts_its_own_text() {
    let doc = open(
        ".c { width: 40px; overflow: hidden; white-space: nowrap }",
        r#"<div class="c">a line much wider than its box</div>"#,
    );
    let words = tokens(&doc);
    let bt = words
        .iter()
        .position(|word| word == "BT")
        .expect("the text");
    let w = words[..bt]
        .iter()
        .rposition(|word| word == "W")
        .expect("a clip before the text");
    assert!(
        !words[w..bt].contains(&"Q".to_owned()),
        "and still in force when the text is drawn: {words:?}"
    );
}

/// **A box whose content fits writes no clip at all** — not a clip that
/// removes nothing — so `pre { overflow: auto }` round code that fits costs a
/// book nothing, and nothing about `overflow` is counted as unimplemented.
#[test]
fn a_box_whose_content_fits_writes_no_clip() {
    let doc = open(
        "div { width: 200px; overflow: hidden } pre { overflow: auto }",
        "<div><p>short</p></div><pre>code</pre>",
    );
    assert!(
        !tokens(&doc).contains(&"W".to_owned()),
        "{:?}",
        tokens(&doc)
    );
    assert_eq!(counted(&doc, "overflow"), None);
}

/// **A rounded box clips to its padding edge's curve** (`css-backgrounds-3`
/// §5.3): each radius less the border width, so a 10-pixel corner inside a
/// 2-pixel border clips on an 8-pixel quarter arc.
#[test]
fn a_rounded_box_clips_to_its_padding_edges_curve() {
    let doc = open(
        &format!("{CLIPPING} .c {{ border: 2px solid #ff0000; border-radius: 10px }}"),
        r#"<div class="c"><div class="ink"></div></div>"#,
    );
    // The border's own sides are drawn inside polygon clips of their own
    // (`a_rounded_border_is_a_ring_per_side`); the overflow clip is the one
    // whose path is curved.
    let words = tokens(&doc);
    let mut found = Vec::new();
    for (w, word) in words.iter().enumerate() {
        if word != "W" {
            continue;
        }
        let q = words[..w]
            .iter()
            .rposition(|word| word == "q")
            .expect("a q");
        found.clear();
        for (at, word) in words[..w].iter().enumerate().skip(q) {
            if word == "c" {
                let operands: Vec<f64> = words[at - 6..at]
                    .iter()
                    .map(|text| text.parse().expect("a `c` operand"))
                    .collect();
                found.push([
                    operands[0],
                    operands[1],
                    operands[2],
                    operands[3],
                    operands[4],
                    operands[5],
                ]);
            }
        }
        if !found.is_empty() {
            break;
        }
    }
    assert_eq!(found.len(), 4, "four curved corners: {words:?}");
    let right = MARGIN + 102.0 * PX;
    let top = PAGE_HEIGHT - MARGIN - 2.0 * PX;
    let r = 8.0 * PX;
    let k = quarter_arc();
    close(
        found[0],
        [
            right - r + k * r,
            top,
            right,
            top - r + k * r,
            right,
            top - r,
        ],
    );
}

/// **An absolutely positioned box escapes the clip of a box that is not its
/// containing block** (CSS 2.2 §11.1.1: clipping applies to *"all descendants
/// except those whose containing block is the viewport or an ancestor of the
/// element"*), and is clipped by one that is.
#[test]
fn a_positioned_box_is_clipped_only_through_its_containing_block() {
    let body = r#"<div class="c"><div class="ink"></div><div class="abs"></div></div>"#;
    let positioned = ".abs { position: absolute; top: 100px; left: 200px; width: 50px; \
                      height: 50px; background-color: #000000 }";
    let (x, y) = at(225.0, 125.0);
    let escaped = open(&format!("{CLIPPING} {positioned}"), body);
    assert_eq!(
        ink_at(&escaped, x, y),
        0,
        "its containing block is the page"
    );
    let held = open(
        &format!("{CLIPPING} {positioned} .c {{ position: relative }}"),
        body,
    );
    assert_eq!(ink_at(&held, x, y), 255, "its containing block clips it");
}
