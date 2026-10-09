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

// ---- shadows ----------------------------------------------------------------------

/// The colour at one point of the first page, in CSS pixels from the content
/// area's top left.
fn rgb_at(doc: &Document, x: f64, y: f64) -> [u8; 3] {
    let bitmap = doc
        .page(0)
        .expect("a page")
        .render(&RenderOptions::default());
    let (x, y) = at(x, y);
    let scale = f64::from(bitmap.height) / PAGE_HEIGHT;
    let column = (x * scale) as usize;
    let row = ((PAGE_HEIGHT - y) * scale) as usize;
    let start = row * bitmap.stride + column * bitmap.components();
    let pixel = bitmap
        .data
        .get(start..start + 3)
        .expect("the point is on the page");
    [pixel[0], pixel[1], pixel[2]]
}

/// The index of the first `rg` naming `colour`, and the operands of the first
/// `m` after it — where a filled shape in that colour starts: its top-left
/// corner, less the corner's radius along the top edge.
fn first_move_after(words: &[String], colour: [&str; 3]) -> (usize, [f64; 2]) {
    let set = words
        .windows(4)
        .position(|w| w[..3] == colour && w[3] == "rg")
        .unwrap_or_else(|| panic!("no fill in {colour:?}"));
    let at = set + words[set..].iter().position(|w| w == "m").expect("a path");
    let read = |i: usize| words[i].parse::<f64>().expect("a number");
    (set, [read(at - 2), read(at - 1)])
}

/// **An outer box shadow is the border box offset and grown by the spread,
/// drawn under the background and only outside the border box**
/// (`css-backgrounds-3` §7.1.1): the clip is the page less the border box,
/// even-odd, and the shape's corner is the box's moved by the offset and out
/// by the spread.
///
/// The render says the knockout is real: a box with no background does not
/// show its own shadow through itself, where the shadow covers the page just
/// past the box's bottom right.
#[test]
fn a_box_shadow_is_the_border_box_offset_and_spread_outside_the_box() {
    let doc = open(
        "div { width: 100px; height: 40px; box-shadow: 4px 6px 0 2px #00ff00 }",
        "<div></div>",
    );
    let words = tokens(&doc);
    let (set, corner) = first_move_after(&words, ["0", "1", "0"]);
    // Offset (4, 6) less the spread on the left, and on top: the shape's top
    // edge is six pixels down, less two.
    let expected = at(4.0 - 2.0, 6.0 - 2.0);
    assert!(
        (corner[0] - expected.0).abs() < 1e-9 && (corner[1] - expected.1).abs() < 1e-9,
        "{corner:?} against {expected:?}"
    );
    let clip = words[..set]
        .iter()
        .rposition(|w| w == "W*")
        .expect("an even-odd clip before the fill");
    let page = words[..clip]
        .windows(5)
        .rposition(|w| w == ["0", "0", "432", "648", "re"])
        .expect("the page's rectangle in the clip");
    assert!(
        words[page..clip].iter().any(|w| w == "m"),
        "the page, less the border box's path after it"
    );
    assert_eq!(rgb_at(&doc, 103.0, 44.0), [0, 255, 0], "past the box");
    assert_eq!(rgb_at(&doc, 50.0, 46.0), [0, 255, 0], "below it");
    assert_eq!(
        rgb_at(&doc, 50.0, 20.0),
        [255, 255, 255],
        "not through the box itself"
    );
    let plain = open("div { width: 100px; height: 40px }", "<div></div>");
    assert_eq!(rgb_at(&plain, 103.0, 44.0), [255, 255, 255]);
}

/// **A shadow's corner grows by the spread, and a small radius by less**
/// (`css-backgrounds-3` §7.1.1): `r + s` where the radius is at least the
/// spread, and `r + s(1 + (r/s − 1)³)` where it is not — so a 2-pixel corner
/// under an 8-pixel spread is 6.625 pixels and not 10, and a square corner
/// stays square.
#[test]
fn a_shadows_corners_grow_by_the_spread_and_a_small_one_by_less() {
    let doc = open(
        "div { width: 100px; height: 40px; border-radius: 10px 2px 0 2px; \
         box-shadow: 0 0 0 8px #00ff00 }",
        "<div></div>",
    );
    let found = curves(&doc);
    // The clip's three curves, then the shape's three: top right, bottom left,
    // top left, in the order the path is written.
    assert_eq!(found.len(), 6, "{found:?}");
    let ratio: f64 = 2.0 / 8.0;
    let small = 2.0 + 8.0 * (1.0 + (ratio - 1.0).powi(3));
    assert!((small - 6.625).abs() < 1e-12);
    // The shape is the border box grown by 8 on every side: 116 wide, its
    // top 8 above the box's.
    let right = MARGIN + 108.0 * PX;
    let top = PAGE_HEIGHT - MARGIN + 8.0 * PX;
    let r = small * PX;
    let k = quarter_arc();
    close(
        found[3],
        [
            right - r + k * r,
            top,
            right,
            top - r + k * r,
            right,
            top - r,
        ],
    );
    let left = MARGIN - 8.0 * PX;
    let big = 18.0 * PX;
    close(
        found[5],
        [
            left,
            top - big + k * big,
            left + big - k * big,
            top,
            left + big,
            top,
        ],
    );
}

/// **An inset shadow is drawn over the background and inside the padding box**
/// (§7.1.1): clipped to the padding box, filled even-odd between it and the
/// padding box offset — so the shadow is the band the offset uncovers, along
/// the top and left edges for a positive offset — and the border is drawn over
/// it.
#[test]
fn an_inset_shadow_is_drawn_inside_the_padding_box_over_the_background() {
    let doc = open(
        "div { width: 100px; height: 40px; background-color: #ff0000; \
         border: 2px solid #000000; box-shadow: inset 5px 5px #0000ff }",
        "<div></div>",
    );
    let words = tokens(&doc);
    let background = words
        .windows(4)
        .position(|w| w == ["1", "0", "0", "rg"])
        .expect("the background");
    let (shadow, _) = first_move_after(&words, ["0", "0", "1"]);
    let border = words
        .windows(4)
        .rposition(|w| w == ["0", "0", "0", "rg"])
        .expect("the border");
    assert!(
        background < shadow && shadow < border,
        "background, then the inset shadow, then the border"
    );
    assert!(
        words[shadow..].iter().any(|w| w == "f*"),
        "an even-odd fill"
    );
    // The padding box starts two pixels in; the band is five pixels deep.
    assert_eq!(rgb_at(&doc, 4.0, 20.0), [0, 0, 255], "the left band");
    assert_eq!(rgb_at(&doc, 50.0, 4.0), [0, 0, 255], "the top band");
    assert_eq!(
        rgb_at(&doc, 50.0, 20.0),
        [255, 0, 0],
        "the background inside it"
    );
    assert_eq!(rgb_at(&doc, 1.0, 20.0), [0, 0, 0], "the border over it");
}

/// **A text shadow is the run drawn again, offset, in the shadow's colour and
/// under the text** (`css-text-decor-3` §4) — and an artifact, so the page's
/// text is read once.
#[test]
fn a_text_shadow_is_the_run_again_under_the_text_and_is_not_read_twice() {
    let doc = open("p { text-shadow: 2px 3px #ff0000 }", "<p>Shadowed</p>");
    let words = tokens(&doc);
    let plain = tokens(&open("", "<p>Shadowed</p>"));
    let place = |words: &[String], from: usize| -> [f64; 2] {
        let at = from + words[from..].iter().position(|w| w == "Td").expect("a run");
        [at - 2, at - 1].map(|i| words[i].parse().expect("a number"))
    };
    let artifact = words
        .windows(2)
        .position(|w| w == ["/Artifact", "BMC"])
        .expect("the shadow is an artifact");
    let red = words[artifact..]
        .windows(4)
        .position(|w| w == ["1", "0", "0", "rg"])
        .expect("in the shadow's colour")
        + artifact;
    let shadow = place(&words, red);
    let text = place(&plain, 0);
    assert!(
        (shadow[0] - (text[0] + 2.0 * PX)).abs() < 1e-9,
        "{shadow:?} {text:?}"
    );
    assert!(
        (shadow[1] - (text[1] - 3.0 * PX)).abs() < 1e-9,
        "{shadow:?} {text:?}"
    );
    let shadow_end = red
        + words[red..]
            .iter()
            .position(|w| w == "EMC")
            .expect("closed");
    let real = shadow_end
        + words[shadow_end..]
            .iter()
            .position(|w| w == "BT")
            .expect("the text");
    assert_eq!(place(&words, real), text, "the text itself has not moved");
    let read = doc.page(0).expect("a page").text().plain_text();
    assert_eq!(read.trim(), "Shadowed", "read once");
}

/// **A translucent shadow's alpha is its colour's times its element's
/// opacity**, because an `/ExtGState`'s `/ca` replaces the one in force: a
/// half-transparent shadow in a half-opaque box is a quarter.
#[test]
fn a_translucent_shadow_is_its_alpha_times_the_elements_opacity() {
    let doc = open(
        "div { width: 100px; height: 40px; opacity: 0.5; \
         box-shadow: 4px 4px rgba(0, 0, 0, 0.5) }",
        "<div></div>",
    );
    // `0.5` is the byte 128 (`css-color-4` rounds 127.5 up), so the colour's
    // alpha is 128/255; the product is quantised to ten-thousandths.
    let expected = 0.5 * 128.0 / 255.0;
    let found = alphas(&doc);
    assert_eq!(found.len(), 2, "the box's own alpha, then the shadow's");
    assert!((found[0].0 - 0.5).abs() < 1e-9, "{found:?}");
    assert!(
        (found[1].0 - expected).abs() <= 0.5e-4 && found[1].0 == found[1].1,
        "{found:?} against {expected}"
    );
}

/// **A blurred shadow is counted and not drawn**: a hard shadow in its place
/// would be a picture the author did not ask for.
#[test]
fn a_blurred_shadow_is_counted_and_not_drawn() {
    let doc = open(
        "div { width: 100px; height: 40px; box-shadow: 4px 4px 3px #00ff00 } \
         p { text-shadow: 1px 1px 1px #00ff00 }",
        "<div></div><p>a</p><p>b</p>",
    );
    assert_eq!(counted(&doc, "box-shadow"), Some(1));
    assert_eq!(counted(&doc, "text-shadow"), Some(2));
    assert!(!tokens(&doc).windows(4).any(|w| w == ["0", "1", "0", "rg"]));
}

/// **An outline or shadow whose geometry is past what a number holds draws
/// nothing** rather than writing `inf` or `NaN`, which are not PDF numbers
/// (7.3.3): `1e400px` reads as infinite, and an offset, width or spread built
/// from it is not finite. The text is still drawn, and the same properties at
/// a finite size still draw — the mismatch, so the assertion is not met by a
/// page on which nothing is painted at all.
#[test]
fn outline_and_shadow_geometry_past_a_numbers_range_draws_nothing() {
    let non_finite = |words: &[String]| {
        words
            .iter()
            .filter(|w| w.ends_with("inf") || w.contains("NaN"))
            .count()
    };
    let green = ["0", "1", "0", "rg"];
    for (huge, finite) in [
        (
            "outline: 1px solid #00ff00; outline-offset: 1e400px",
            "outline: 1px solid #00ff00; outline-offset: 2px",
        ),
        (
            "outline: 1e400px solid #00ff00",
            "outline: 2px solid #00ff00",
        ),
        ("box-shadow: 1e400px 0 #00ff00", "box-shadow: 4px 0 #00ff00"),
        (
            "box-shadow: -1e400px 1e400px #00ff00",
            "box-shadow: -4px 4px #00ff00",
        ),
        (
            "box-shadow: 0 0 0 1e400px #00ff00",
            "box-shadow: 0 0 0 4px #00ff00",
        ),
        (
            "box-shadow: inset 1e400px 0 #00ff00",
            "box-shadow: inset 4px 0 #00ff00",
        ),
        (
            "box-shadow: inset 0 0 0 1e400px #00ff00",
            "box-shadow: inset 0 0 0 4px #00ff00",
        ),
        (
            "text-shadow: 1e400px 0 #00ff00",
            "text-shadow: 2px 0 #00ff00",
        ),
        (
            "text-shadow: 0 -1e400px #00ff00",
            "text-shadow: 0 -2px #00ff00",
        ),
    ] {
        let words = tokens(&open(
            &format!("div {{ width: 100px; {huge} }}"),
            "<div>hello world</div>",
        ));
        assert_eq!(non_finite(&words), 0, "{huge}: {}", words.join(" "));
        assert!(words.iter().any(|w| w == "Tj"), "{huge}: the text is gone");
        assert!(
            !words.windows(4).any(|w| w == green),
            "{huge}: something was drawn in its colour"
        );
        let drawn = tokens(&open(
            &format!("div {{ width: 100px; {finite} }}"),
            "<div>hello world</div>",
        ));
        assert!(
            drawn.windows(4).any(|w| w == green),
            "{finite}: the finite one draws nothing either"
        );
    }
}

// ---- transform --------------------------------------------------------------------

/// Every `cm` on the first page, as its six operands, in stream order.
fn matrices(doc: &Document) -> Vec<[f64; 6]> {
    let words = tokens(doc);
    let mut out = Vec::new();
    for (at, word) in words.iter().enumerate() {
        if word != "cm" || at < 6 {
            continue;
        }
        let mut operands = [0.0; 6];
        for (slot, text) in operands.iter_mut().zip(&words[at - 6..at]) {
            *slot = text.parse().expect("a `cm` operand");
        }
        out.push(operands);
    }
    out
}

/// A CSS matrix `[a, b, c, d, e, f]` (CSS pixels, y downwards) about a page
/// point, as the page's `cm`: the closed form, from first principles — flip
/// the y axis, scale the offsets to points, conjugate by the origin.
fn on_page(css: [f64; 6], origin: (f64, f64)) -> [f64; 6] {
    let [a, b, c, d, e, f] = css;
    let (pa, pb, pc, pd) = (a, -b, -c, d);
    let (ox, oy) = origin;
    [
        pa,
        pb,
        pc,
        pd,
        ox - pa * ox - pc * oy + e * PX,
        oy - pb * ox - pd * oy - f * PX,
    ]
}

#[track_caller]
fn near6(actual: [f64; 6], expected: [f64; 6]) {
    for (a, e) in actual.iter().zip(expected) {
        assert!((a - e).abs() < 1e-9, "{actual:?} against {expected:?}");
    }
}

/// **A rotation is one `cm` about the border box's centre**
/// (`css-transforms-1` §6's initial `transform-origin`), clockwise on the
/// page: the matrix is the closed form, and the render says the box now
/// stands on its end — red below where it was and white beside where it was.
#[test]
fn a_rotation_is_a_cm_about_the_border_boxs_centre() {
    let style = "div { width: 100px; height: 40px; background-color: #ff0000; \
                 transform: rotate(90deg) }";
    let doc = open(style, "<div></div>");
    let found = matrices(&doc);
    assert_eq!(found.len(), 1, "{found:?}");
    let centre = at(50.0, 20.0);
    near6(found[0], on_page([0.0, 1.0, -1.0, 0.0, 0.0, 0.0], centre));
    assert_eq!(rgb_at(&doc, 50.0, 60.0), [255, 0, 0], "below the old box");
    assert_eq!(
        rgb_at(&doc, 5.0, 20.0),
        [255, 255, 255],
        "beside the centre"
    );
    let plain = open(
        "div { width: 100px; height: 40px; background-color: #ff0000 }",
        "<div></div>",
    );
    assert!(matrices(&plain).is_empty(), "no transform, no `cm`");
    assert_eq!(rgb_at(&plain, 5.0, 20.0), [255, 0, 0]);
    // A box with nothing of its own to paint still turns its text: its
    // fragment is the reference box, so the layout leaves one.
    let bare = open(
        "div { width: 100px; height: 40px; transform: rotate(90deg) }",
        "<div>turned</div>",
    );
    let found = matrices(&bare);
    assert!(!found.is_empty(), "the text is turned");
    for matrix in found {
        near6(matrix, on_page([0.0, 1.0, -1.0, 0.0, 0.0, 0.0], centre));
    }
}

/// **The list applies rightmost first, about `transform-origin`**:
/// `translate(30px) scale(2)` about the top-left corner doubles the box and
/// then moves it, where `scale(2) translate(30px)` moves it sixty pixels.
#[test]
fn the_list_applies_rightmost_first_about_the_origin() {
    let corner = at(0.0, 0.0);
    let first = open(
        "div { width: 100px; height: 40px; background-color: #ff0000; \
         transform: translate(30px) scale(2); transform-origin: left top }",
        "<div></div>",
    );
    near6(
        matrices(&first)[0],
        on_page([2.0, 0.0, 0.0, 2.0, 30.0, 0.0], corner),
    );
    let second = open(
        "div { width: 100px; height: 40px; background-color: #ff0000; \
         transform: scale(2) translate(30px); transform-origin: 0 0 }",
        "<div></div>",
    );
    near6(
        matrices(&second)[0],
        on_page([2.0, 0.0, 0.0, 2.0, 60.0, 0.0], corner),
    );
    // And a percentage translation is of the border box: half of 100 + 2 × 10.
    let percent = open(
        "div { width: 100px; height: 40px; padding: 0 10px; \
         background-color: #ff0000; transform: translate(50%, 25%) }",
        "<div></div>",
    );
    let centre = at(60.0, 20.0);
    near6(
        matrices(&percent)[0],
        on_page([1.0, 0.0, 0.0, 1.0, 60.0, 10.0], centre),
    );
}

/// **A transform turns everything inside the element, its text included, and
/// a nested one composes inside it**: the inner box is drawn under the outer
/// matrix and then its own, in that order, and its text under both.
#[test]
fn a_nested_transform_composes_inside_its_ancestors() {
    let doc = open(
        ".o { width: 200px; height: 80px; background-color: #ff0000; \
         transform: rotate(90deg) } \
         .i { width: 50px; height: 20px; background-color: #0000ff; \
         transform: translate(10px, 5px) }",
        r#"<div class="o"><div class="i">x</div></div>"#,
    );
    let words = tokens(&doc);
    let found = matrices(&doc);
    let outer = on_page([0.0, 1.0, -1.0, 0.0, 0.0, 0.0], at(100.0, 40.0));
    let inner = on_page([1.0, 0.0, 0.0, 1.0, 10.0, 5.0], at(25.0, 10.0));
    // The outer box; the inner box (both matrices); its text (both again).
    assert_eq!(found.len(), 5, "{found:?}");
    near6(found[0], outer);
    near6(found[1], outer);
    near6(found[2], inner);
    near6(found[3], outer);
    near6(found[4], inner);
    let text = words.iter().position(|w| w == "BT").expect("the text");
    let last_cm = words.iter().rposition(|w| w == "cm").expect("a cm");
    assert!(last_cm < text, "the text is drawn under the transforms");
}

/// **A transform that flattens the plane draws nothing**: `scale(0)` has no
/// inverse, so the element's fragments are clipped to nothing rather than
/// handed to a reader as a singular matrix.
#[test]
fn a_transform_with_no_inverse_draws_nothing() {
    let doc = open(
        "div { width: 100px; height: 40px; background-color: #ff0000; \
         transform: scale(0) }",
        "<div>gone</div>",
    );
    assert!(matrices(&doc).is_empty());
    let words = tokens(&doc);
    assert!(
        words
            .windows(6)
            .any(|w| w == ["0", "0", "0", "0", "re", "W"]),
        "an empty clip"
    );
    assert_eq!(darkest(&doc), u8::MAX, "nothing is drawn");
}

/// **A three-dimensional transform is counted and not drawn**, and a link
/// under a two-dimensional one is counted, since its active area is the
/// run's rectangle before the transform.
#[test]
fn a_three_d_transform_and_a_turned_link_are_counted() {
    let doc = open("div { transform: rotateX(30deg) }", "<div>flat</div>");
    assert_eq!(counted(&doc, "transform"), Some(1));
    assert!(matrices(&doc).is_empty());
    let linked = open(
        "div { transform: rotate(5deg) }",
        r##"<div><a href="#x">here</a></div><p id="x">there</p>"##,
    );
    assert_eq!(counted(&linked, "transform"), Some(1));
}

// ---- color-scheme ---------------------------------------------------------------

/// **`color-scheme` names the scheme a printed page is already in, and draws
/// nothing differently** (`css-color-adjust-1` §2.1).
///
/// Paper is the light canvas and print media's `prefers-color-scheme` is
/// `light`, so pandoc's `:root { color-scheme: light dark }` chooses the
/// scheme this build draws: the page is the same content stream, operator for
/// operator, as the page without it, and nothing is counted. The claim's
/// other side is the value that would make it false — `dark`, a list without
/// `light`, asks for the dark canvas and system colours this build does not
/// have, so it is refused by value and counted by element rather than drawn
/// light and called honoured.
#[test]
fn color_scheme_is_the_light_scheme_a_page_is_printed_in() {
    let body = "<p>Ink on <em>paper</em>.</p>";
    let plain = open("", body);
    let light = open(":root { color-scheme: light dark }", body);
    assert_eq!(
        tokens(&light),
        tokens(&plain),
        "`color-scheme: light dark` changed what the page draws"
    );
    assert_eq!(counted(&light, "color-scheme"), None);

    let dark = open(":root { color-scheme: dark }", body);
    assert_eq!(
        tokens(&dark),
        tokens(&plain),
        "the refused value still draws the page in the light scheme"
    );
    // One element: the root the rule matched. A refused declaration is
    // counted where it was written, not where it would have been inherited.
    assert_eq!(
        counted(&dark, "color-scheme"),
        Some(1),
        "`color-scheme: dark` was not counted once: {:?}",
        warnings(&dark)
    );
}

// ---- font-kerning and font-feature-settings on a face that is not shaped ---------

/// **A feature asked of the standard 14 is counted, by element, and the page
/// is drawn as it would be without it** (`css-fonts-4` §6.4, §6.12).
///
/// The standard 14 are drawn a character at a time from their widths: no
/// `GSUB`, no `GPOS`, and no AFM kerning pairs, which this build does not
/// carry. So `font-kerning: normal` and a feature switched on cannot be met
/// on text set in one of them, and each element whose text asked is counted
/// against the property — here the `<p>` and the `<em>` that inherits from
/// it. A feature switched off is met there already, and `auto` kerning is the
/// user agent's to decide, so neither is counted.
#[test]
fn a_feature_asked_of_a_face_this_build_does_not_shape_is_counted() {
    let body = "<p>plain <em>emphasis</em></p>";
    let plain = open("", body);
    let small_caps = open("p { font-feature-settings: \"smcp\" }", body);
    assert_eq!(counted(&small_caps, "font-feature-settings"), Some(2));
    assert_eq!(
        tokens(&small_caps),
        tokens(&plain),
        "an unmet feature changed the page"
    );
    let off = open("p { font-feature-settings: \"liga\" 0 }", body);
    assert_eq!(counted(&off, "font-feature-settings"), None);
    let kerned = open("p { font-kerning: normal }", body);
    assert_eq!(counted(&kerned, "font-kerning"), Some(2));
    let auto = open("p { font-kerning: auto }", body);
    assert_eq!(counted(&auto, "font-kerning"), None);
    let none = open("p { font-kerning: none }", body);
    assert_eq!(counted(&none, "font-kerning"), None);
}

// ---- direction and unicode-bidi where this layout does not meet them -------------

/// **What `direction` and `unicode-bidi` ask of a box this layout does not
/// do is counted, by element, and what it does is not**
/// (`css-writing-modes-3` §2.1, §2.2).
///
/// A right-to-left paragraph is met — its lines' level and the side they
/// start from, which `epub_shaped.rs` asserts position by position — and
/// counts nothing, nor does an isolate inside it. A right-to-left table lays
/// its columns from the left here (CSS 2.2 §17.5), a flex row its items
/// (`css-flexbox-1` §2) and a multi-column container its columns
/// (`css-multicol-1` §3); and a block-level box over-constrained in a
/// right-to-left containing block — a block, a table or a flex container
/// with a definite `width`, neither margin `auto` — gives up its right
/// margin where §10.3.3 gives up its left. Each such element is counted
/// against `direction`, and one with a margin `auto` is not
/// over-constrained. **The margin is the containing block's to decide**, not
/// the box's own (review of lane 8C): a right-to-left `div` with a width
/// in the left-to-right body gives up its right margin, as this layout does,
/// and is not counted; a left-to-right one inside a right-to-left `div` gives
/// up its left, which this layout does not, and is — as is a left-to-right
/// table or flex container with a width there. A relatively positioned box
/// with `left` and `right` both stated in a right-to-left block is offset by
/// `left` here, where §9.4.3 lets `right` win, and is counted. An absolutely positioned
/// box in a right-to-left block is placed from the right by §10.3.7 — at its
/// static position with both insets `auto`, and by `right` when `left`,
/// `width` and `right` are all stated — and counted; with `right` alone it
/// is placed from the right here too. An
/// override is drawn by nobody: `<bdo>`'s `isolate-override` and an
/// author's `bidi-override` are refused by value and counted against
/// `unicode-bidi`, per element they reached.
#[test]
fn direction_and_unicode_bidi_count_what_this_layout_does_not_do() {
    let met = open(
        "",
        "<p dir=\"rtl\">Ink <span dir=\"ltr\">on</span> <bdi>paper</bdi>.</p>",
    );
    assert_eq!(counted(&met, "direction"), None, "{:?}", warnings(&met));
    assert_eq!(counted(&met, "unicode-bidi"), None, "{:?}", warnings(&met));

    for (style, body, name, elements) in [
        (
            "",
            "<table dir=\"rtl\"><tr><td>a</td><td>b</td></tr></table>",
            "direction",
            Some(1),
        ),
        (
            "",
            "<div dir=\"rtl\" style=\"display: flex\"><p>a</p><p>b</p></div>",
            "direction",
            Some(1),
        ),
        (
            "",
            "<div dir=\"rtl\" style=\"column-count: 2\"><p>a</p></div>",
            "direction",
            Some(1),
        ),
        (
            "",
            "<div dir=\"rtl\" style=\"width: 50%\"><p>a</p></div>",
            "direction",
            None,
        ),
        (
            "",
            "<div dir=\"rtl\"><div style=\"width: 50%\"><p>a</p></div></div>",
            "direction",
            Some(1),
        ),
        (
            "",
            "<div dir=\"rtl\"><div dir=\"ltr\" style=\"width: 50%\"><p>a</p></div></div>",
            "direction",
            Some(1),
        ),
        (
            "",
            "<div dir=\"rtl\"><div style=\"width: 50%; margin-left: auto\"><p>a</p></div></div>",
            "direction",
            None,
        ),
        // The same over-constrained margin, on the other block-level boxes in
        // normal flow (review of lane 8C): a table and a flex container with a
        // definite width. Without one a flex container fills its line, and so
        // does a table here, so neither gives up a margin.
        (
            "",
            "<div dir=\"rtl\"><table dir=\"ltr\" style=\"width: 50%\"><tr><td>a</td></tr></table></div>",
            "direction",
            Some(1),
        ),
        (
            "",
            "<div dir=\"rtl\"><table dir=\"ltr\"><tr><td>a</td></tr></table></div>",
            "direction",
            None,
        ),
        (
            "",
            "<table dir=\"ltr\" style=\"width: 50%\"><tr><td>a</td></tr></table>",
            "direction",
            None,
        ),
        (
            "",
            "<div dir=\"rtl\"><div dir=\"ltr\" style=\"display: flex; width: 50%\"><p>a</p></div></div>",
            "direction",
            Some(1),
        ),
        (
            "",
            "<div dir=\"rtl\"><div dir=\"ltr\" style=\"display: flex\"><p>a</p></div></div>",
            "direction",
            None,
        ),
        // §10.4 runs §10.3.3 again with a clamping `max-width`, or a widening
        // `min-width`, as the `width` — so an `auto` width that either clamps
        // is over-constrained as a stated one is (review of lane 8C). Whether
        // it clamps is the containing block's width to say, which the cascade
        // does not know, so a box either may clamp is counted; a `max-width`
        // of 100% or more over margins that are not negative never does, a
        // `min-width` of zero never does, and `auto` margins still decide for
        // themselves.
        (
            "",
            "<div dir=\"rtl\"><div style=\"max-width: 50%\"><p dir=\"ltr\">a</p></div></div>",
            "direction",
            Some(1),
        ),
        (
            "",
            "<div dir=\"rtl\"><p style=\"max-width: 50%\">a</p></div>",
            "direction",
            Some(1),
        ),
        (
            "",
            "<div dir=\"rtl\"><p style=\"max-width: 20em\">a</p></div>",
            "direction",
            Some(1),
        ),
        (
            "",
            "<div dir=\"rtl\"><p style=\"min-width: 120%\">a</p></div>",
            "direction",
            Some(1),
        ),
        (
            "",
            "<div dir=\"rtl\"><table dir=\"ltr\" style=\"max-width: 50%\"><tr><td>a</td></tr></table></div>",
            "direction",
            Some(1),
        ),
        (
            "",
            "<div dir=\"rtl\"><p style=\"max-width: 100%\">a</p></div>",
            "direction",
            None,
        ),
        (
            "",
            "<div dir=\"rtl\"><p style=\"max-width: 100%; margin-right: -10px\">a</p></div>",
            "direction",
            Some(1),
        ),
        (
            "",
            "<div dir=\"rtl\"><p style=\"min-width: 0\">a</p></div>",
            "direction",
            None,
        ),
        (
            "",
            "<div dir=\"rtl\"><p style=\"max-width: 50%; margin: 0 auto\">a</p></div>",
            "direction",
            None,
        ),
        (
            "",
            "<p style=\"max-width: 50%\">a</p>",
            "direction",
            None,
        ),
        (
            "",
            "<div dir=\"rtl\"><p style=\"max-width: 50%; float: left\">a</p></div>",
            "direction",
            None,
        ),
        // And a relatively positioned box with both `left` and `right` in a
        // right-to-left containing block, where §9.4.3 lets `right` win and
        // this layout applies `left` — a block, and an inline block, whose
        // containing block is the block it sits in. One inset alone moves it
        // as §9.4.3 says in either direction, and `left` winning in a
        // left-to-right block is §9.4.3's own answer.
        (
            "",
            "<div dir=\"rtl\"><p style=\"position: relative; left: 10px; right: 20px\">a</p></div>",
            "direction",
            Some(1),
        ),
        (
            "",
            "<div dir=\"rtl\"><span style=\"display: inline-block; position: relative; \
             left: 10px; right: 20px\">a</span></div>",
            "direction",
            Some(1),
        ),
        (
            "",
            "<div dir=\"rtl\"><p style=\"position: relative; right: 20px\">a</p></div>",
            "direction",
            None,
        ),
        (
            "",
            "<p style=\"position: relative; left: 10px; right: 20px\">a</p>",
            "direction",
            None,
        ),
        (
            "",
            "<div dir=\"rtl\"><p style=\"position: absolute; width: 50px\">a</p></div>",
            "direction",
            Some(1),
        ),
        (
            "",
            "<div dir=\"rtl\" style=\"position: relative\">\
             <p style=\"position: absolute; left: 0; right: 0; width: 50px\">a</p></div>",
            "direction",
            Some(1),
        ),
        (
            "",
            "<div dir=\"rtl\"><p style=\"position: absolute; right: 0; width: 50px\">a</p></div>",
            "direction",
            None,
        ),
        (
            "",
            "<p>a <bdo dir=\"rtl\">bc</bdo> d</p>",
            "unicode-bidi",
            Some(1),
        ),
        (
            "span { unicode-bidi: bidi-override; direction: rtl }",
            "<p><span>a</span> <span>b</span></p>",
            "unicode-bidi",
            Some(2),
        ),
    ] {
        let doc = open(style, body);
        assert_eq!(
            counted(&doc, name),
            elements,
            "`{body}` under `{style}`: {:?}",
            warnings(&doc)
        );
    }
}

// ---- gradients -----------------------------------------------------------------

/// The colour at each point, in CSS pixels from the content area's top left,
/// of the first page rendered once.
fn colours(doc: &Document, points: &[(f64, f64)]) -> Vec<[f64; 3]> {
    let bitmap = doc
        .page(0)
        .expect("a page")
        .render(&RenderOptions::default());
    let scale = f64::from(bitmap.height) / PAGE_HEIGHT;
    points
        .iter()
        .map(|&(x, y)| {
            let (x, y) = at(x, y);
            let column = (x * scale) as usize;
            let row = ((PAGE_HEIGHT - y) * scale) as usize;
            let start = row * bitmap.stride + column * bitmap.components();
            let pixel = bitmap
                .data
                .get(start..start + 3)
                .expect("the point is on the page");
            [pixel[0], pixel[1], pixel[2]].map(f64::from)
        })
        .collect()
}

/// `from` blended `t` of the way to `to`, clamped as a gradient is past its
/// ends.
fn blend(from: [f64; 3], to: [f64; 3], t: f64) -> [f64; 3] {
    let t = t.clamp(0.0, 1.0);
    [0, 1, 2].map(|i| from[i] + (to[i] - from[i]) * t)
}

/// Asserts each sampled colour is the expected one to within four levels of
/// 255 — a pixel's width of the ramp, and the renderer's rounding.
fn near_colours(actual: &[[f64; 3]], expected: &[[f64; 3]], what: &str) {
    assert_eq!(actual.len(), expected.len());
    for (at, (a, e)) in actual.iter().zip(expected).enumerate() {
        assert!(
            a.iter().zip(e).all(|(a, e)| (a - e).abs() <= 4.0),
            "{what}, point {at}: drew {a:?}, expected {e:?}"
        );
    }
}

const RED: [f64; 3] = [255.0, 0.0, 0.0];
const GREEN: [f64; 3] = [0.0, 255.0, 0.0];
const BLUE: [f64; 3] = [0.0, 0.0, 255.0];
const WHITE: [f64; 3] = [255.0, 255.0, 255.0];
const BLACK: [f64; 3] = [0.0, 0.0, 0.0];

/// A 200 by 100 pixel box whose `background-image` is `image`, followed by
/// whatever else `image` goes on to declare.
fn gradient_box(image: &str) -> Document {
    open(
        &format!(".g {{ width: 200px; height: 100px; background-image: {image} }}"),
        "<div class=\"g\"></div>",
    )
}

/// **A linear gradient is an axial shading along its gradient line**
/// (`css-images-3` §3.1), sampled on the page against the colour CSS says
/// each point is.
///
/// The expected colours are worked out here, not read back: a point's place
/// on the gradient line is its offset from the box's centre along the line's
/// direction — `(sin A, −cos A)` in CSS's downward y, `A` clockwise from up —
/// over the line's length `|w sin A| + |h cos A|`, plus a half. `to right` is
/// the plain case, `45deg` one whose line is longer than either side, and the
/// third and fourth have §3.5.3's fix-up: a stop with no position spread
/// between its neighbours, two at one place an edge, and the last stop's
/// unwritten position 100%.
#[test]
fn a_linear_gradient_is_the_colour_css_says_at_every_point() {
    let line = |degrees: f64, (x, y): (f64, f64)| {
        let (sin, cos) = degrees.to_radians().sin_cos();
        let length = (200.0 * sin).abs() + (100.0 * cos).abs();
        ((x - 100.0) * sin - (y - 50.0) * cos) / length + 0.5
    };
    let points = [
        (20.0, 50.0),
        (100.0, 50.0),
        (180.0, 50.0),
        (40.0, 80.0),
        (160.0, 20.0),
    ];

    let doc = gradient_box("linear-gradient(to right, #ff0000, #0000ff)");
    let expected: Vec<[f64; 3]> = points
        .iter()
        .map(|p| blend(RED, BLUE, line(90.0, *p)))
        .collect();
    near_colours(&colours(&doc, &points), &expected, "to right");

    let doc = gradient_box("linear-gradient(45deg, #ff0000, #0000ff)");
    let expected: Vec<[f64; 3]> = points
        .iter()
        .map(|p| blend(RED, BLUE, line(45.0, *p)))
        .collect();
    near_colours(&colours(&doc, &points), &expected, "45deg");

    // A stop with no position is spread between its neighbours: green at
    // the middle.
    let doc = gradient_box("linear-gradient(to right, #ff0000, #00ff00, #0000ff)");
    near_colours(
        &colours(&doc, &[(50.0, 50.0), (100.0, 50.0), (150.0, 50.0)]),
        &[blend(RED, GREEN, 0.5), GREEN, blend(GREEN, BLUE, 0.5)],
        "an unplaced stop",
    );

    let doc =
        gradient_box("linear-gradient(to bottom, #ff0000 25%, #00ff00 25%, #00ff00 50%, #0000ff)");
    near_colours(
        &colours(
            &doc,
            &[(100.0, 10.0), (100.0, 40.0), (100.0, 75.0), (100.0, 98.0)],
        ),
        &[
            RED,
            GREEN,
            blend(GREEN, BLUE, 0.5),
            blend(GREEN, BLUE, 0.96),
        ],
        "hard stops",
    );
}

/// **`to` a corner is the angle that puts the other two corners on the 50%
/// line** (§3.1.1), so it depends on the box: in a 200 by 100 box `to top
/// right` is not 45°. Checked by that property rather than by the angle: the
/// points on the diagonal from the top left to the bottom right are all the
/// midpoint colour, and a pixel in from the bottom-left corner is nearly the
/// start and one in from the top-right nearly the end — 0.0076 of the line
/// from either, the line being 178.9 pixels long and each point 88.1 short
/// of its end.
#[test]
fn a_corner_gradient_puts_the_other_two_corners_on_its_midline() {
    let doc = gradient_box("linear-gradient(to top right, #ffffff, #000000)");
    let middle = blend(WHITE, BLACK, 0.5);
    near_colours(
        &colours(
            &doc,
            &[
                (50.0, 25.0),
                (100.0, 50.0),
                (150.0, 75.0),
                (1.0, 99.0),
                (199.0, 1.0),
            ],
        ),
        &[
            middle,
            middle,
            middle,
            blend(WHITE, BLACK, 0.0076),
            blend(WHITE, BLACK, 0.9924),
        ],
        "to top right",
    );
}

/// **A radial gradient is a radial shading, an ellipse a circle squashed**
/// (§3.2): `circle closest-side` in a 200 by 100 box centred is a circle of
/// radius 50, red at the centre and blue from 50 out; the default, an
/// ellipse to the farthest corner, keeps farthest-side's 2:1 ratio and passes
/// through the corners — radii 141.42 and 70.71 — so the midpoint grey lies
/// half each radius from the centre on both axes. A stop before the centre,
/// an ending shape of no size and `at` are the cases after.
#[test]
fn a_radial_gradient_is_the_colour_css_says_at_every_point() {
    let doc = gradient_box("radial-gradient(circle closest-side, #ff0000, #0000ff)");
    near_colours(
        &colours(
            &doc,
            &[(100.0, 50.0), (125.0, 50.0), (100.0, 30.0), (180.0, 50.0)],
        ),
        &[RED, blend(RED, BLUE, 0.5), blend(RED, BLUE, 0.4), BLUE],
        "circle closest-side",
    );

    let doc = gradient_box("radial-gradient(#ffffff, #000000)");
    let ry = 50.0_f64.hypot(50.0);
    let rx = 2.0 * ry;
    near_colours(
        &colours(
            &doc,
            &[
                (100.0 + rx / 2.0, 50.0),
                (100.0, 50.0 + ry / 2.0),
                (100.0 - rx / 4.0, 50.0),
            ],
        ),
        &[
            blend(WHITE, BLACK, 0.5),
            blend(WHITE, BLACK, 0.5),
            blend(WHITE, BLACK, 0.25),
        ],
        "an ellipse to the farthest corner",
    );

    // A stop before the centre is no circle PDF can draw: the centre is the
    // colour interpolated there, half way from -25 to 25 pixels.
    let doc = gradient_box("radial-gradient(circle closest-side, #ff0000 -50%, #0000ff 50%)");
    near_colours(
        &colours(&doc, &[(100.0, 50.0), (112.5, 50.0), (130.0, 50.0)]),
        &[blend(RED, BLUE, 0.5), blend(RED, BLUE, 0.75), BLUE],
        "a stop before the centre",
    );

    // Centred on a corner, the closest side is no distance away: §3.2.4's
    // degenerate shape, drawn as its last colour everywhere.
    let doc = gradient_box("radial-gradient(circle closest-side at left top, #ff0000, #0000ff)");
    near_colours(
        &colours(&doc, &[(10.0, 10.0), (150.0, 80.0)]),
        &[BLUE, BLUE],
        "a ring of no size",
    );
    // And an ellipse centred on the top edge has no height to its closest
    // side, whatever its width: the same.
    let doc = gradient_box("radial-gradient(closest-side at top, #ff0000, #0000ff)");
    near_colours(
        &colours(&doc, &[(100.0, 50.0), (20.0, 80.0)]),
        &[BLUE, BLUE],
        "an ellipse of no height",
    );

    // Centred on the left edge's midpoint, the farthest side is 200 pixels
    // away: half way across is half way along the ray.
    let doc = gradient_box("radial-gradient(circle farthest-side at left, #ff0000, #0000ff)");
    near_colours(
        &colours(&doc, &[(1.0, 50.0), (100.0, 50.0)]),
        &[blend(RED, BLUE, 0.005), blend(RED, BLUE, 0.5)],
        "at left",
    );
}

/// **A gradient is an image with no size of its own, so `background-size`
/// gives it one and it repeats** (§5.3; `css-backgrounds-3` §2.3): fifty
/// pixels wide and `to right`, it starts again every fifty pixels.
#[test]
fn a_sized_gradient_repeats_as_an_image_does() {
    let doc =
        gradient_box("linear-gradient(to right, #ff0000, #0000ff); background-size: 50px 100%");
    near_colours(
        &colours(
            &doc,
            &[(10.0, 50.0), (60.0, 50.0), (160.0, 50.0), (40.0, 50.0)],
        ),
        &[
            blend(RED, BLUE, 0.2),
            blend(RED, BLUE, 0.2),
            blend(RED, BLUE, 0.2),
            blend(RED, BLUE, 0.8),
        ],
        "a fifty-pixel tile",
    );
}

/// **A gradient this build cannot draw is counted and draws nothing**: a
/// translucent stop (a PDF shading has no alpha) and a repeating gradient are
/// refused by value, one element each, and the page is the page with no
/// image. The one it can draw is not counted.
#[test]
fn a_gradient_this_build_does_not_draw_is_counted() {
    let plain = open(
        ".g { width: 200px; height: 100px }",
        "<div class=\"g\"></div>",
    );
    for image in [
        "linear-gradient(#ff0000, transparent)",
        "repeating-linear-gradient(#ff0000, #0000ff 20px)",
    ] {
        let doc = gradient_box(image);
        assert_eq!(counted(&doc, "background-image"), Some(1), "{image}");
        assert_eq!(tokens(&doc), tokens(&plain), "{image} drew something");
    }
    let drawn = gradient_box("linear-gradient(#ff0000, #0000ff)");
    assert_eq!(counted(&drawn, "background-image"), None);
    assert_ne!(tokens(&drawn), tokens(&plain), "a gradient drew nothing");
}

/// Every stream of a small document, decoded and joined: the page's content
/// and every pattern cell a gradient wrote.
fn every_stream(doc: &Document) -> String {
    let cos = doc.cos();
    let mut out = String::new();
    for num in 1..=cos.max_object_number() {
        if let Ok(data) = cos.stream_decoded(tinker_pdf_cos::ObjRef { num, gen: 0 }) {
            out.push_str(&String::from_utf8_lossy(&data));
            out.push('\n');
        }
    }
    out
}

/// **A gradient whose numbers are not finite writes no infinity, and is
/// counted** (ruling 1's arithmetic and ruling 10; review of lane 8C).
///
/// Each of these is a finite length in the book that makes an infinite one
/// on the page: radii of `1e-200px` and `1e200px` squash the circle by their
/// ratio, `1e308%` of the box's width puts the centre past `f64::MAX`, and
/// a stop at `1e308%` of the gradient line is past it too. The radial ones
/// wrote `inf` into the pattern cell's `cm`, which no reader parses; the
/// linear one drew nothing and said nothing. Each is now refused before it
/// is written and counted against `background-image`, one element.
#[test]
fn a_gradient_whose_numbers_are_not_finite_is_counted_and_writes_no_infinity() {
    for image in [
        "radial-gradient(1e-200px 1e200px, #ff0000, #0000ff)",
        "radial-gradient(circle 10px at 1e308% 50%, #ff0000 0px, #0000ff 10px)",
        "linear-gradient(#ff0000 1e308%, #0000ff)",
    ] {
        let doc = gradient_box(image);
        let streams = every_stream(&doc);
        let bad: Vec<&str> = streams
            .split_whitespace()
            .filter(|token| token.contains("inf") || token.contains("NaN"))
            .collect();
        assert!(bad.is_empty(), "{image} wrote {bad:?}");
        assert_eq!(counted(&doc, "background-image"), Some(1), "{image}");
    }
}

/// **§3.2.4's degenerate radial gradients, as its three cases say** (review
/// of lane 8C).
///
/// - A **circle** of no radius — `closest-side` at a corner — is a circle of
///   a vanishing one, so stops placed by length still ring out from the
///   centre: red at the top-left corner to blue 100 px from it, half way at
///   50 px (30, 40) and a tenth at 10 px (6, 8).
/// - An ending shape of **no width** — an ellipse's `closest-side` on the
///   left edge — is a horizontal gradient mirrored about the centre: the
///   colour is the distance across from the left edge, whatever the height.
/// - Only one of **no height** with width — `closest-side` on the top edge —
///   is its last colour throughout, which
///   `a_radial_gradient_is_the_colour_css_says_at_every_point` holds.
///
/// Every degenerate shape used to be drawn as its last colour; with
/// percentage stops, which all resolve to the centre, the first two are that
/// too.
#[test]
fn a_degenerate_radial_gradient_is_drawn_as_section_3_2_4_says() {
    let doc = gradient_box(
        "radial-gradient(circle closest-side at left top, #ff0000 0px, #0000ff 100px)",
    );
    near_colours(
        &colours(&doc, &[(30.0, 40.0), (6.0, 8.0), (120.0, 90.0)]),
        &[blend(RED, BLUE, 0.5), blend(RED, BLUE, 0.1), BLUE],
        "a circle of no radius",
    );
    let doc = gradient_box("radial-gradient(closest-side at left, #ff0000 0px, #0000ff 100px)");
    near_colours(
        &colours(
            &doc,
            &[(50.0, 10.0), (50.0, 90.0), (20.0, 50.0), (150.0, 30.0)],
        ),
        &[
            blend(RED, BLUE, 0.5),
            blend(RED, BLUE, 0.5),
            blend(RED, BLUE, 0.2),
            BLUE,
        ],
        "an ending shape of no width",
    );
    for image in [
        "radial-gradient(circle closest-side at left top, #ff0000, #0000ff)",
        "radial-gradient(closest-side at left, #ff0000, #0000ff)",
    ] {
        let doc = gradient_box(image);
        near_colours(
            &colours(&doc, &[(30.0, 40.0), (150.0, 80.0)]),
            &[BLUE, BLUE],
            image,
        );
    }
}

// ---- hyphens -------------------------------------------------------------------

/// **A soft hyphen is drawn as a hyphen where its line breaks at it, and
/// nowhere else** (`css-text-3` §5.4), read back off the page.
///
/// Courier at sixteen pixels is 9.6 pixels a character, so a hundred-pixel
/// measure holds ten: `aaaa&#173;bbbbbbbb` breaks at its soft hyphen, and
/// `ab&#173;cd` after it fits whole on its own line, where its soft hyphen is
/// not drawn at all. The hyphen is U+002D, so it reads back as one.
#[test]
fn a_soft_hyphen_is_drawn_only_where_its_line_breaks() {
    let doc = open(
        "p { width: 100px; font-family: monospace; font-size: 16px }",
        "<p>aaaa\u{AD}bbbbbbbb ab\u{AD}cd</p>",
    );
    let text = doc.page(0).expect("a page").text().plain_text();
    assert_eq!(text.trim_end(), "aaaa-\nbbbbbbbb\nabcd");
}

/// **A break inside a word that lands after a soft hyphen draws no hyphen
/// under `hyphens: none`** (`css-text-3` §5.4; review of lane 8C).
///
/// Forty pixels hold four Courier characters at sixteen pixels. Under `none`
/// the soft hyphen in `aaaa&#173;bbbbbbbb` is no break, so `overflow-wrap:
/// anywhere` breaks the word inside, four characters a line; the first
/// break falls just after the soft hyphen, and was drawn with a hyphen there
/// — `aaaa-` — which `none` never shows.
#[test]
fn an_emergency_break_after_a_soft_hyphen_under_none_draws_no_hyphen() {
    let doc = open(
        "p { width: 40px; hyphens: none; overflow-wrap: anywhere; \
         font-family: monospace; font-size: 16px }",
        "<p>aaaa\u{AD}bbbbbbbb</p>",
    );
    let text = doc.page(0).expect("a page").text().plain_text();
    assert_eq!(text.trim_end(), "aaaa\nbbbb\nbbbb");
}
