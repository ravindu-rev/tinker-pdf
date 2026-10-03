//! §6.2's other content-document language, end to end (Tier 4's SVG lane,
//! milestone 7).
//!
//! Every book here is built in this file rather than fetched, so the claims
//! hold on a machine with no network. What the *fetched* corpus adds is the
//! only thing a hand-written fixture cannot: a real producer's output —
//! `sample-svg-in-spine.epub`'s Illustrator cover of 339 gradient-filled paths
//! under a 58-class `<style>` element, and three pages that are one `<image>`
//! and nothing else. `epub_fetched.rs` renders all six and asserts each is more
//! than one colour.
//!
//! # What this file asserts that the leaf crate's own suite cannot
//!
//! `crates/tinker-pdf-svg/tests/` proves what a document *says*. This proves
//! what reaches paper: that a scene becomes operators, that an SVG spine item
//! is a page rather than a placeholder, that its text extracts, and that every
//! refusal the leaf crate names travels out through `ArchiveWarning`.
//!
//! # Counted injection
//!
//! | Defect injected | Tests that failed |
//! | --- | ---: |
//! | **the registry is built after `begin_page`** | **4** |
//! | the SVG branch never runs and every item placeholders | 11 |
//! | `place_text` advances the pen twice | 1 |
//! | the placement stretches to fill instead of fitting | 1 |
//! | the clip is not applied | 1 |
//! | a gradient's `/Matrix` is not composed with the page mapping | 1 |
//! | an `<image>` is not fitted by `preserveAspectRatio` | 1 |
//! | an unresolved `<image>` is not counted | 1 |
//! | `text-anchor`'s shift is not applied | 2 |
//! | a translucent group is drawn inline, each node at full alpha | 1 |
//! | a group's form is painted under the page mapping | 1 |
//! | a group's clip is not written | 1 |
//! | the leaf crate draws no marker instance | 1 |
//! | `reflect` and `repeat` are written as `pad` | 3 |
//! | the leaf crate reads `repeat` as `reflect` | 2 |
//! | a reflected period is not turned back | 1 |
//! | a linear domain reaches only the stated axis | 2 |
//! | a radial gradient's rings stop at the stated circle | 1 |
//! | a `y`-only chunk resets the pen's `x` | 1 |
//! | a glyph's rotation is applied after the move to its origin | 1 |
//! | a mask's `gs` is set under the page mapping | 1 |
//! | a mask's colours are not turned to their grey | 1 |
//! | the mask region is not clipped | 1 |
//! | a masked group is drawn unmasked | 4 |
//! | a pattern's tile does not carry the page mapping | 1 |
//! | a pattern is painted as nothing | 1 |
//! | the registry does not note a tile's runs | 1 |
//! | the registry does not note a mask's runs | 1 |
//! | the reader ignores a container's `clip-path`, as it did until groups | 1 |
//!
//! The form row fired **zero** the first time: its fixture's shapes covered
//! the page top to bottom, and the page mapping composed twice is a flip
//! composed twice — the identity, on a square page — so the shapes landed
//! where they belonged. They cover the top half now, and the bottom half has
//! to be white.
//!
//! The first row is the one this file exists for. **It was a real defect, not
//! a hypothetical**: `begin_page` snapshots the document's resource set, so the
//! first draft — which registered while drawing — wrote pages naming patterns,
//! ext-gstates and images that were not in them. The gradient, the transparency
//! and the photograph were silently gone; the fetched-corpus test passed
//! throughout, because `cover.svg` strokes its 339 paths black and a page with
//! strokes and no fills still has more than one colour.
//!
//! Three of these fired **zero** the first time and each was a hole in a
//! fixture: the fit test had only a scene *wider* than the page, where both
//! scales agree; the image-fit test sampled a row above the scene entirely,
//! which nothing can ever ink; and there was no gradient test at all, in a file
//! about the format whose corpus is almost nothing but gradients. The
//! gradient-matrix row needed one more correction after that — a *horizontal*
//! axis cannot see a y-flip, because the flip touches one coordinate.

mod cbz_support;
mod epub_support;

use cbz_support::rgb_png;
use epub_support::{ocf_zip, OcfEntry};
use tinker_pdf::epub::SpineDefect;
use tinker_pdf::{ArchiveWarning, Document, OpenOptions, RenderOptions};

const CONTAINER: &str = concat!(
    r##"<?xml version="1.0" encoding="utf-8"?>"##,
    r##"<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">"##,
    r##"<rootfiles><rootfile full-path="EPUB/content.opf" "##,
    r##"media-type="application/oebps-package+xml"/></rootfiles></container>"##
);

/// A book of one SVG spine item, plus whatever extra entries a test needs.
fn book(svg: &str, extra: &[(&str, Vec<u8>)]) -> Vec<u8> {
    let package = concat!(
        r##"<?xml version="1.0" encoding="utf-8"?>"##,
        r##"<package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="id">"##,
        r##"<metadata xmlns:dc="http://purl.org/dc/elements/1.1/">"##,
        r##"<dc:identifier id="id">urn:uuid:1f0c2c1e-0000-4000-8000-0000000000ff</dc:identifier>"##,
        r##"<dc:title>A Drawing</dc:title><dc:language>en</dc:language>"##,
        r##"</metadata>"##,
        r##"<manifest>"##,
        r##"<item id="d" href="draw.svg" media-type="image/svg+xml"/>"##,
        r##"<item id="p" href="photo.png" media-type="image/png"/>"##,
        r##"</manifest>"##,
        r##"<spine><itemref idref="d"/></spine></package>"##
    );
    let mut entries = vec![
        OcfEntry::stored("mimetype", b"application/epub+zip"),
        OcfEntry::deflated("META-INF/container.xml", CONTAINER.as_bytes()),
        OcfEntry::deflated("EPUB/content.opf", package.as_bytes()),
        OcfEntry::deflated("EPUB/draw.svg", svg.as_bytes()),
    ];
    for (name, bytes) in extra {
        entries.push(OcfEntry::deflated(name, bytes));
    }
    let directory: Vec<usize> = (0..entries.len()).collect();
    ocf_zip(&entries, &directory)
}

fn open(svg: &str) -> Document {
    Document::open(book(svg, &[])).expect("the book opens")
}

/// Every distinct grey on a page, which is how a page that drew is told from
/// one that did not.
fn greys(doc: &Document, page: u32) -> Vec<u8> {
    let bitmap = doc
        .page(page)
        .expect("a page")
        .render(&RenderOptions::default());
    let components = bitmap.components();
    let mut out: Vec<u8> = bitmap
        .data
        .chunks_exact(components)
        .map(|pixel| pixel[0])
        .collect();
    out.sort_unstable();
    out.dedup();
    out
}

/// Where each text object was placed, as the `e` term of the `cm` that
/// immediately precedes its `BT`.
///
/// **Read out of the operators rather than off the page, and that is a
/// measurement rather than a preference.** `bundled-fonts` is off by default,
/// so the standard 14 have no outlines in this build and base-14 text renders
/// as nothing at all -- a chapter of ordinary prose included, which
/// `diag_chapter_text_ink` was written to check and did: an XHTML paragraph
/// renders one flat white. Asserting on pixels would make every text test in
/// this file a test of that feature flag. What decides where a run is set is
/// its text matrix, and this reads that matrix.
fn text_origins(doc: &Document) -> Vec<f64> {
    let pdf = doc.editor().save(&Default::default());
    let text = String::from_utf8_lossy(&pdf);
    let mut out = Vec::new();
    let mut previous: Option<f64> = None;
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with("BT ") {
            if let Some(at) = previous.take() {
                out.push(at);
            }
            continue;
        }
        // The run's own matrix, which `epub::svg` writes as `1 0 0 -1 x y cm`
        // for a run under no transform of its own.
        if let Some(rest) = line.strip_suffix(" cm") {
            let numbers: Vec<&str> = rest.split_whitespace().collect();
            if let [a, b, c, d, e, _] = numbers[..] {
                if (a, b, c, d) == ("1", "0", "0", "-1") {
                    previous = e.parse().ok();
                }
            }
        }
    }
    out
}

/// The whole `cm` that immediately precedes each text object, as six numbers
/// — [`text_origins`]' reading, for runs that are turned as well as moved.
fn text_matrices(doc: &Document) -> Vec<[f64; 6]> {
    let pdf = doc.editor().save(&Default::default());
    let text = String::from_utf8_lossy(&pdf);
    let mut out = Vec::new();
    let mut previous: Option<[f64; 6]> = None;
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with("BT ") {
            if let Some(matrix) = previous.take() {
                out.push(matrix);
            }
            continue;
        }
        if let Some(rest) = line.strip_suffix(" cm") {
            let numbers: Vec<f64> = rest
                .split_whitespace()
                .filter_map(|n| n.parse().ok())
                .collect();
            if let [a, b, c, d, e, f] = numbers[..] {
                previous = Some([a, b, c, d, e, f]);
            }
        }
    }
    out
}

fn warnings(doc: &Document) -> Vec<ArchiveWarning> {
    doc.archive()
        .expect("a book carries a report")
        .warnings()
        .to_vec()
}

// ---- the page draws ------------------------------------------------------------

/// **An SVG spine item is a page that draws.**
///
/// The row this lane closed said *"placeholder page; no SVG renderer"*, and the
/// placeholder was one flat `0xBF` grey. This is the assertion that used to be
/// impossible: a black square on a white page is two colours and neither of
/// them is that grey.
#[test]
fn an_svg_spine_item_draws_instead_of_placeholding() {
    let doc = open(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="100" viewBox="0 0 100 100">
             <rect x="10" y="10" width="80" height="80" fill="#000000"/>
           </svg>"##,
    );
    assert_eq!(doc.page_count(), 1, "one page per spine itemref");
    let seen = greys(&doc, 0);
    assert!(seen.len() > 1, "the page drew something: {seen:?}");
    assert_ne!(seen, [0xBF], "and it is not the placeholder grey");
    assert!(seen.contains(&0), "a black fill reached the page");
    assert!(
        warnings(&doc)
            .iter()
            .all(|warning| !matches!(warning, ArchiveWarning::SpinePage { .. })),
        "and nothing named it a placeholder: {:?}",
        warnings(&doc)
    );
}

/// §7.7's `meet` mapping at the outside edge: a picture that is not the page's
/// shape is **fitted and centred**, never stretched.
///
/// A wide drawing on a tall page leaves white above and below and touches the
/// left and right edges. A build that stretched to fill would put ink in the
/// corners, which is the same picture at the wrong proportions — visible only
/// against a shape somebody can measure.
#[test]
fn a_scene_is_fitted_to_the_page_rather_than_stretched() {
    // **Both directions, because a fit that took one axis is right on half the
    // documents there are.** A drawing wider than the page and one taller than
    // it disagree about which scale wins, and a build that always divided by
    // the width would pass the first and overflow the second.
    let band = |markup: &str| -> (u8, u8, u8) {
        let doc = Document::open_with(book(markup, &[]), &OpenOptions::at_page(200.0, 200.0))
            .expect("the book opens");
        let bitmap = doc
            .page(0)
            .expect("a page")
            .render(&RenderOptions::default());
        let components = bitmap.components();
        let width = bitmap.width as usize;
        let height = bitmap.height as usize;
        let at = |x: usize, y: usize| bitmap.data[(y * width + x) * components];
        (
            at(width / 2, 2),
            at(width / 2, height / 2),
            at(2, height / 2),
        )
    };

    // Two-to-one on a square page: a band across the middle, white above.
    let (top, middle, left) = band(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="200" height="100" viewBox="0 0 200 100">
              <rect x="0" y="0" width="200" height="100" fill="#000000"/>
            </svg>"##,
    );
    assert!(middle < 0x40, "the middle of the page is inked");
    assert!(top > 0xC0, "and the top is not");
    assert!(left < 0x40, "the drawing reaches the left edge");

    // One-to-two on the same page: a band down the middle, white at the left.
    let (top, middle, left) = band(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="200" viewBox="0 0 100 200">
              <rect x="0" y="0" width="100" height="200" fill="#000000"/>
            </svg>"##,
    );
    assert!(middle < 0x40, "the middle of the page is inked");
    assert!(top < 0x40, "the drawing reaches the top edge");
    assert!(
        left > 0xC0,
        "and the left is white, because the height is what had to fit"
    );
}

/// **A gradient reaches the page as a gradient.**
///
/// `cover.svg` in the fetched corpus is 339 paths filled from sixteen
/// `<linearGradient>`s, so this is the feature that book *is*. It is also the
/// one the resource-ordering defect dropped in silence: a `/Pattern` registered
/// after `begin_page` is named by an operator no reader can resolve, the fill
/// simply does not happen, and the page still has ink on it from every stroke.
/// So the assertion is on a **ramp** — three sample points across the shape
/// that are ordered light to dark — rather than on the presence of ink.
#[test]
fn a_gradient_fills_a_shape_as_a_ramp() {
    let doc = Document::open_with(
        book(
            r##"<svg xmlns="http://www.w3.org/2000/svg" width="200" height="200" viewBox="0 0 200 200">
                  <defs>
                    <linearGradient id="ramp" gradientUnits="userSpaceOnUse"
                                    x1="0" y1="0" x2="200" y2="0">
                      <stop offset="0" stop-color="#ffffff"/>
                      <stop offset="1" stop-color="#000000"/>
                    </linearGradient>
                  </defs>
                  <rect x="0" y="0" width="200" height="200" fill="url(#ramp)"/>
                </svg>"##,
            &[],
        ),
        &OpenOptions::at_page(200.0, 200.0),
    )
    .expect("the book opens");
    let bitmap = doc
        .page(0)
        .expect("a page")
        .render(&RenderOptions::default());
    let components = bitmap.components();
    let width = bitmap.width as usize;
    let row = bitmap.height as usize / 2;
    let at = |x: usize| bitmap.data[(row * width + x) * components];
    let (left, centre, right) = (at(width / 8), at(width / 2), at(width - width / 8));
    assert!(
        left > centre && centre > right,
        "the ramp runs light to dark across the shape: {left}, {centre}, {right}"
    );
    assert!(
        left > 0xC0 && right < 0x40,
        "and it reaches both stops: {left} to {right}"
    );
}

/// A gradient's `/Matrix` is composed with the page mapping.
///
/// 8.7.3.1 makes pattern space the page's **default** coordinate system, which
/// the `cm` carrying the y-flip and the fit does not reach — so the mapping has
/// to be written into the pattern too.
///
/// **The axis is vertical, and that is the whole design of the test.** SVG's
/// `y` grows downward and a PDF page's grows upward, so a build that left the
/// mapping out draws this ramp *upside down* — light at the foot of the page
/// where the document said light at the head. A horizontal axis cannot see it
/// at all, because the flip touches only one coordinate, and the first draft of
/// this test used one and caught nothing.
#[test]
fn a_gradients_matrix_carries_the_page_mapping() {
    let doc = Document::open_with(
        book(
            r##"<svg xmlns="http://www.w3.org/2000/svg" width="200" height="200" viewBox="0 0 200 200">
                  <defs>
                    <linearGradient id="down" gradientUnits="userSpaceOnUse"
                                    x1="0" y1="0" x2="0" y2="200">
                      <stop offset="0" stop-color="#ffffff"/>
                      <stop offset="1" stop-color="#000000"/>
                    </linearGradient>
                  </defs>
                  <rect x="0" y="0" width="200" height="200" fill="url(#down)"/>
                </svg>"##,
            &[],
        ),
        &OpenOptions::at_page(200.0, 200.0),
    )
    .expect("the book opens");
    let bitmap = doc
        .page(0)
        .expect("a page")
        .render(&RenderOptions::default());
    let components = bitmap.components();
    let width = bitmap.width as usize;
    let height = bitmap.height as usize;
    let at = |y: usize| bitmap.data[(y * width + width / 2) * components];
    let (top, bottom) = (at(height / 8), at(height - height / 8));
    assert!(
        top > bottom,
        "the first stop is at the *top* of the page, because SVG's y grows          downward and the page's grows up: {top} at the head against {bottom}          at the foot"
    );
    assert!(
        top > 0xC0 && bottom < 0x40,
        "and it reaches both stops: {top} to {bottom}"
    );
}

/// The red channel along the page's middle row, at fractions of its width.
fn row(doc: &Document, at: &[f64]) -> Vec<u8> {
    at.iter().map(|x| rgb_at(doc, *x, 0.5)[0]).collect()
}

/// A black-to-white ramp fifty wide, across a rectangle two hundred wide, with
/// the given `spreadMethod`.
fn spread_page(method: &str) -> Document {
    square(&format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="200" height="200" viewBox="0 0 200 200">
              <defs>
                <linearGradient id="g" gradientUnits="userSpaceOnUse" x1="0" y1="0" x2="50" y2="0"
                                spreadMethod="{method}">
                  <stop offset="0" stop-color="#000000"/>
                  <stop offset="1" stop-color="#ffffff"/>
                </linearGradient>
              </defs>
              <rect width="200" height="200" fill="url(#g)"/>
            </svg>"##
    ))
}

/// §13.2.3's `repeat`: the ramp again from its start, every fifty units.
///
/// Sampled a fifth of a period into each of the first three periods (x = 10,
/// 60 and 110) and four fifths in (x = 40, 90, 140): dark, dark, dark and
/// light, light, light. Padded — what this build drew until now — everything
/// past fifty is the last stop's white.
#[test]
fn a_repeated_gradient_starts_again_every_period() {
    let doc = spread_page("repeat");
    let early = row(&doc, &[0.05, 0.3, 0.55]);
    let late = row(&doc, &[0.2, 0.45, 0.7]);
    for value in &early {
        assert!((40..=62).contains(value), "a fifth of the way: {early:?}");
    }
    for value in &late {
        assert!(
            (193..=215).contains(value),
            "four fifths of the way: {late:?}"
        );
    }
}

/// §13.2.3's `reflect`: forwards, then backwards, then forwards.
///
/// A fifth into the second period (x = 60) is the ramp read **backwards**, so
/// it is light where `repeat` is dark; a fifth into the third (x = 110) is
/// forwards again and dark.
#[test]
fn a_reflected_gradient_runs_back_on_every_other_period() {
    let doc = spread_page("reflect");
    let [first, second, third] = row(&doc, &[0.05, 0.3, 0.55])[..] else {
        unreachable!("three samples")
    };
    assert!((40..=62).contains(&first), "forwards: {first}");
    assert!((193..=215).contains(&second), "backwards: {second}");
    assert!((40..=62).contains(&third), "forwards again: {third}");
}

/// `pad`, the initial value, is unchanged: the last stop's white past the axis.
#[test]
fn a_padded_gradient_is_its_end_colour_past_the_axis() {
    let doc = spread_page("pad");
    for value in row(&doc, &[0.3, 0.55, 0.9]) {
        assert!(value > 0xF0, "white past the axis: {value}");
    }
}

/// `repeat` on a **radial** gradient is rings: the stated circle, and then a
/// second ring as wide outside it, from black again.
///
/// The circle is twenty in radius at the page's centre, so a sample four
/// units out and one twenty-four units out are both a fifth into a period —
/// dark — and sixteen and thirty-six units out are both four fifths — light.
#[test]
fn a_repeated_radial_gradient_is_rings() {
    let doc = square(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="200" height="200" viewBox="0 0 200 200">
              <defs>
                <radialGradient id="g" gradientUnits="userSpaceOnUse" cx="100" cy="100" r="20"
                                spreadMethod="repeat">
                  <stop offset="0" stop-color="#000000"/>
                  <stop offset="1" stop-color="#ffffff"/>
                </radialGradient>
              </defs>
              <rect width="200" height="200" fill="url(#g)"/>
            </svg>"##,
    );
    let at = |units: f64| rgb_at(&doc, (100.0 + units) / 200.0, 0.5)[0];
    for units in [4.0, 24.0] {
        let value = at(units);
        assert!((40..=70).contains(&value), "{units} out, dark: {value}");
    }
    for units in [16.0, 36.0] {
        let value = at(units);
        assert!((185..=215).contains(&value), "{units} out, light: {value}");
    }
}

// ---- §14.5's groups --------------------------------------------------------------

/// A page's colour at a fraction of its width and height, as `[r, g, b]`.
fn rgb_at(doc: &Document, x: f64, y: f64) -> [u8; 3] {
    let bitmap = doc
        .page(0)
        .expect("a page")
        .render(&RenderOptions::default());
    let components = bitmap.components();
    let width = bitmap.width as usize;
    let height = bitmap.height as usize;
    let column = ((width as f64 * x) as usize).min(width - 1);
    let row = ((height as f64 * y) as usize).min(height - 1);
    let at = (row * width + column) * components;
    if components >= 3 {
        [bitmap.data[at], bitmap.data[at + 1], bitmap.data[at + 2]]
    } else {
        [bitmap.data[at]; 3]
    }
}

fn square(svg: &str) -> Document {
    Document::open_with(book(svg, &[]), &OpenOptions::at_page(200.0, 200.0))
        .expect("the book opens")
}

/// **A group's opacity is composited once**, which is the whole of §14.5.
///
/// A red and a blue rectangle overlap inside one `<g opacity="0.5">`. Inside
/// the group the blue covers the red, so where they overlap the group is
/// *blue*, and faded once over white that is `(128, 128, 255)`. Faded one
/// shape at a time — which is what this build drew until the group became a
/// node — the blue is laid at a half over a red already laid at a half, and
/// the overlap is a purple `(128, 64, 191)` the file never described.
#[test]
fn a_groups_opacity_is_composited_once() {
    let doc = square(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="200" height="200" viewBox="0 0 200 200">
              <g opacity="0.5">
                <rect x="0" y="0" width="120" height="200" fill="#ff0000"/>
                <rect x="80" y="0" width="120" height="200" fill="#0000ff"/>
              </g>
            </svg>"##,
    );
    let red = rgb_at(&doc, 0.2, 0.5);
    let overlap = rgb_at(&doc, 0.5, 0.5);
    let blue = rgb_at(&doc, 0.8, 0.5);
    let near = |got: [u8; 3], want: [u8; 3]| {
        got.iter()
            .zip(want)
            .all(|(g, w)| (i32::from(*g) - i32::from(w)).abs() <= 3)
    };
    assert!(
        near(red, [255, 128, 128]),
        "red at a half over white: {red:?}"
    );
    assert!(
        near(blue, [128, 128, 255]),
        "blue at a half over white: {blue:?}"
    );
    assert!(
        near(overlap, [128, 128, 255]),
        "and where they overlap, the group is blue before it is faded: {overlap:?}"
    );
}

/// A shape and its gradient **inside a group's form** land where they would
/// outside it.
///
/// 8.7.3.1 reads a pattern used in a form against the form's default space at
/// the moment it is painted, and this writer paints every form with the page's
/// own default space in force so that the two are one. The shapes cover only
/// the **top** half of the drawing, so a form painted under the page mapping —
/// which composes the flip twice — puts them in the bottom half, where the
/// page must be white; and the axis is vertical, for
/// `a_gradients_matrix_carries_the_page_mapping`'s reason.
#[test]
fn a_gradient_inside_a_group_keeps_the_page_mapping() {
    let doc = square(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="200" height="200" viewBox="0 0 200 200">
              <defs>
                <linearGradient id="down" gradientUnits="userSpaceOnUse"
                                x1="0" y1="0" x2="0" y2="200">
                  <stop offset="0" stop-color="#ffffff"/>
                  <stop offset="1" stop-color="#000000"/>
                </linearGradient>
              </defs>
              <g opacity="0.999">
                <rect x="0" y="0" width="100" height="100" fill="url(#down)"/>
                <rect x="100" y="0" width="100" height="100" fill="url(#down)"/>
              </g>
            </svg>"##,
    );
    let head = rgb_at(&doc, 0.25, 0.05)[0];
    let middle = rgb_at(&doc, 0.25, 0.45)[0];
    let foot = rgb_at(&doc, 0.25, 0.9)[0];
    assert!(
        head > 0xD8 && middle < 0xA0 && head > middle,
        "the ramp runs light to dark down the top half, inside the group as \
         outside: {head} at the head, {middle} at the middle"
    );
    assert!(foot > 0xF0, "and the bottom half is the page: {foot}");
}

/// §14.3.5: a `clip-path` on a `<g>` clips the group's rendering.
///
/// It used to clip nothing: the property does not inherit, so the children
/// were drawn whole and the clip — on an element that is not a shape — went
/// nowhere. The left half of a black square is kept and the right is white.
#[test]
fn a_groups_clip_path_reaches_the_page() {
    let doc = square(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="200" height="200" viewBox="0 0 200 200">
              <clipPath id="left"><rect width="100" height="200"/></clipPath>
              <g clip-path="url(#left)">
                <rect width="200" height="200" fill="#000000"/>
              </g>
            </svg>"##,
    );
    assert!(rgb_at(&doc, 0.25, 0.5)[0] < 0x40, "the left half is kept");
    assert!(
        rgb_at(&doc, 0.75, 0.5)[0] > 0xC0,
        "and the right half is clipped away"
    );
}

// ---- §13.3's patterns ---------------------------------------------------------

/// Every font resource a `Tf` names in the saved file, and whether a `/Font`
/// dictionary somewhere defines it.
fn fonts_named_and_defined(doc: &Document) -> Vec<(String, bool)> {
    let pdf = doc.editor().save(&Default::default());
    let text = String::from_utf8_lossy(&pdf);
    let mut named: Vec<String> = Vec::new();
    for line in text.lines() {
        let words: Vec<&str> = line.split_whitespace().collect();
        for (at, word) in words.iter().enumerate() {
            if *word == "Tf" && at >= 2 {
                if let Some(font) = words[at - 2].strip_prefix('/') {
                    if !named.iter().any(|n| n == font) {
                        named.push(font.to_owned());
                    }
                }
            }
        }
    }
    named
        .into_iter()
        .map(|font| {
            let defined = text.split("/Font").skip(1).any(|after| {
                let dictionary = after.split(">>").next().unwrap_or("");
                dictionary.contains(&format!("/{font} "))
            });
            (font, defined)
        })
        .collect()
}

/// **Text that only a pattern's tile or a mask draws has its face
/// registered.** The registry notes every run it will draw so that each face
/// is written once; a run inside a tile or a mask was not noted, so its `Tf`
/// named a resource no `/Font` dictionary held, and the text was gone.
#[test]
fn text_inside_a_tile_or_a_mask_names_a_font_the_file_has() {
    for markup in [
        r##"<pattern id="p" patternUnits="userSpaceOnUse" width="50" height="50">
              <text x="5" y="20" font-family="serif" font-size="12">Tile</text>
            </pattern>
            <rect width="100" height="100" fill="url(#p)"/>"##,
        r##"<mask id="m" maskUnits="userSpaceOnUse" x="0" y="0" width="100" height="100">
              <text x="5" y="20" font-family="serif" font-size="12" fill="white">Mask</text>
            </mask>
            <rect width="100" height="100" mask="url(#m)"/>"##,
    ] {
        let doc = open(&format!(
            r##"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="100">{markup}</svg>"##
        ));
        let fonts = fonts_named_and_defined(&doc);
        assert!(!fonts.is_empty(), "the run is written: {markup}");
        assert!(
            fonts.iter().all(|(_, defined)| *defined),
            "{fonts:?} in {markup}"
        );
    }
}

/// **A pattern reaches the page as tiles**: a checkerboard twenty units a
/// tile, black in its top-left and bottom-right quarters.
///
/// Sampled in the first tile and in the next one along and the next one
/// down, so a tile that did not repeat, repeated at the wrong step, or came
/// out upside down — the page mapping composed twice, or not at all — each
/// puts black where white is asserted.
#[test]
fn a_pattern_fills_a_shape_with_its_tiles() {
    let doc = square(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="200" height="200" viewBox="0 0 200 200">
              <pattern id="p" patternUnits="userSpaceOnUse" width="20" height="20">
                <rect width="10" height="10" fill="#000000"/>
                <rect x="10" y="10" width="10" height="10" fill="#000000"/>
              </pattern>
              <rect width="200" height="200" fill="url(#p)"/>
            </svg>"##,
    );
    let at = |x: f64, y: f64| rgb_at(&doc, x / 200.0, y / 200.0)[0];
    for (x, y, black) in [
        (5.0, 5.0, true),
        (15.0, 5.0, false),
        (5.0, 15.0, false),
        (15.0, 15.0, true),
        (25.0, 5.0, true),
        (35.0, 5.0, false),
        (5.0, 25.0, true),
        (125.0, 185.0, true),
        (135.0, 185.0, false),
        (135.0, 195.0, true),
    ] {
        let value = at(x, y);
        if black {
            assert!(value < 0x20, "black at ({x}, {y}): {value}");
        } else {
            assert!(value > 0xE0, "white at ({x}, {y}): {value}");
        }
    }
}

// ---- §14.4's masks ------------------------------------------------------------

/// **A mask reaches the page**: what is under its white is kept, what is under
/// its black is gone.
///
/// A black square fills the page; its mask is white on the left half and
/// nothing — black, the backdrop — on the right.
#[test]
fn a_mask_keeps_what_is_under_its_white() {
    let doc = square(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="200" height="200" viewBox="0 0 200 200">
              <mask id="m" maskUnits="userSpaceOnUse" x="0" y="0" width="200" height="200">
                <rect width="100" height="200" fill="#ffffff"/>
              </mask>
              <rect width="200" height="200" fill="#000000" mask="url(#m)"/>
            </svg>"##,
    );
    assert!(rgb_at(&doc, 0.25, 0.5)[0] < 0x10, "kept under the white");
    assert!(
        rgb_at(&doc, 0.75, 0.5)[0] > 0xF0,
        "and gone under the black"
    );
}

/// The mask lands **where the drawing is**, the right way up.
///
/// Its white covers the drawing's top half. 11.6.5.2 places a soft mask's
/// group in the space in force when the `gs` is set, so a state set under the
/// page mapping — which the mask's form then applies again — composes the
/// flip twice and turns the mask over: the bottom kept and the top gone. A
/// mask symmetric about the page's middle cannot see that, and the first
/// three tests here are.
#[test]
fn a_mask_is_placed_the_right_way_up() {
    let doc = square(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="200" height="200" viewBox="0 0 200 200">
              <mask id="m" maskUnits="userSpaceOnUse" x="0" y="0" width="200" height="200">
                <rect width="200" height="100" fill="#ffffff"/>
              </mask>
              <rect width="200" height="200" fill="#000000" mask="url(#m)"/>
            </svg>"##,
    );
    assert!(rgb_at(&doc, 0.5, 0.25)[0] < 0x10, "the top half is kept");
    assert!(
        rgb_at(&doc, 0.5, 0.75)[0] > 0xF0,
        "and the bottom half is gone"
    );
}

/// The mask is a **luminance**, and CSS Masking's: a pure green mask keeps
/// 0.7154 of what is under it.
///
/// A black square under it is `255 × (1 − 0.7154) = 72.6` over white. Read by
/// 11.6.5.3's own RGB weights the green would be 0.59 and the square 104 —
/// which is why the writer turns every colour in a mask into its grey first.
#[test]
fn a_masks_luminance_is_css_maskings() {
    let doc = square(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="200" height="200" viewBox="0 0 200 200">
              <mask id="m" maskUnits="userSpaceOnUse" x="0" y="0" width="200" height="200">
                <rect width="200" height="200" fill="#00ff00"/>
              </mask>
              <rect width="200" height="200" fill="#000000" mask="url(#m)"/>
            </svg>"##,
    );
    let value = rgb_at(&doc, 0.5, 0.5)[0];
    assert!(
        (68..=78).contains(&value),
        "a black square kept at 0.7154: {value}"
    );
}

/// The mask **region** bounds it, and outside the region the mask is black.
///
/// The mask's white covers the whole page, but its region — in user space, x
/// from 0 to 100 — is half of it: the square keeps its left half and loses its
/// right however white the content is there.
#[test]
fn a_masks_region_bounds_it() {
    let doc = square(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="200" height="200" viewBox="0 0 200 200">
              <mask id="m" maskUnits="userSpaceOnUse" x="0" y="0" width="100" height="200">
                <rect width="200" height="200" fill="#ffffff"/>
              </mask>
              <rect width="200" height="200" fill="#000000" mask="url(#m)"/>
            </svg>"##,
    );
    assert!(rgb_at(&doc, 0.25, 0.5)[0] < 0x10, "inside the region");
    assert!(
        rgb_at(&doc, 0.75, 0.5)[0] > 0xF0,
        "and outside it, black, however white the content"
    );
}

// ---- §11.6's markers ----------------------------------------------------------

/// **A marker reaches the page**: an arrowhead drawn past the end of a line.
///
/// The arrow is ten by ten in marker units with its reference at its back
/// edge's middle, and `markerUnits` is the initial `strokeWidth`, so on a line
/// four wide it is forty long — from x = 100, where the line ends, to 140. The
/// page is white there unless the marker was drawn, because the line itself
/// stops at 100.
#[test]
fn a_marker_is_drawn_at_the_end_of_its_line() {
    let doc = square(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="200" height="200" viewBox="0 0 200 200">
              <defs>
                <marker id="arrow" markerWidth="10" markerHeight="10" refX="0" refY="5"
                        orient="auto">
                  <path d="M0 0 L10 5 L0 10 z" fill="#000000"/>
                </marker>
              </defs>
              <line x1="20" y1="100" x2="100" y2="100" stroke="#000000" stroke-width="4"
                    marker-end="url(#arrow)"/>
            </svg>"##,
    );
    assert!(rgb_at(&doc, 0.3, 0.5)[0] < 0x40, "the line is drawn");
    assert!(
        rgb_at(&doc, 0.6, 0.5)[0] < 0x40,
        "and the arrowhead past its end: {:?}",
        rgb_at(&doc, 0.6, 0.5)
    );
    assert!(
        rgb_at(&doc, 0.75, 0.5)[0] > 0xC0,
        "and nothing past the arrow's tip"
    );
}

// ---- what travels out ------------------------------------------------------------

/// Every subsystem the leaf crate declines reaches the caller as an
/// `ArchiveWarning::Svg` naming which — and naming the **item**, because a book
/// has many and *"a filter was not drawn"* is not something a host can act on.
#[test]
fn every_refusal_travels_out_named_with_its_item() {
    let doc = open(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10">
             <filter id="f"/>
             <foreignObject width="1" height="1"/><animate/><script/>
             <rect width="10" height="10" fill="#000"/>
           </svg>"##,
    );
    let named: Vec<tinker_pdf_svg::Warning> = warnings(&doc)
        .into_iter()
        .filter_map(|warning| match warning {
            ArchiveWarning::Svg { item, warning } => {
                assert_eq!(item, "EPUB/draw.svg", "the item is named");
                Some(warning)
            }
            _ => None,
        })
        .collect();
    for expected in [
        tinker_pdf_svg::Warning::FilterUnsupported,
        tinker_pdf_svg::Warning::ForeignObjectUnsupported,
        tinker_pdf_svg::Warning::AnimationIgnored,
        tinker_pdf_svg::Warning::ScriptIgnored,
    ] {
        assert!(named.contains(&expected), "{expected:?} did not travel out");
    }
    // And the document still drew: a refusal is a picture with something
    // missing, not a page nobody wrote.
    assert!(greys(&doc, 0).contains(&0), "the rectangle is still there");
}

/// A document that produces no picture at all is a placeholder **named by the
/// reader that refused it**, and the refusal travels.
///
/// The `<use>` bomb is the case worth having: it is well-formed XML, every
/// element in it is legal, and a renderer that recursed would not return. What
/// reaches the caller is `Refusal::TooManyUses` rather than a timeout.
#[test]
fn a_document_that_produces_no_picture_is_named_with_its_refusal() {
    for (markup, expected) in [
        (
            r##"<svg xmlns="http://www.w3.org/2000/svg"><g id="l"><use href="#l"/></g></svg>"##,
            tinker_pdf_svg::Refusal::TooManyUses,
        ),
        (
            r##"<html xmlns="http://www.w3.org/1999/xhtml"><body/></html>"##,
            tinker_pdf_svg::Refusal::NotAnSvg,
        ),
        ("<svg", tinker_pdf_svg::Refusal::Unreadable),
    ] {
        let doc = open(markup);
        assert_eq!(doc.page_count(), 1, "it still keeps its page");
        let named: Vec<SpineDefect> = warnings(&doc)
            .into_iter()
            .filter_map(|warning| match warning {
                ArchiveWarning::SpinePage { defect, .. } => Some(defect),
                _ => None,
            })
            .collect();
        assert_eq!(named, [SpineDefect::SvgUnreadable(expected)], "{markup}");
        assert_eq!(
            greys(&doc, 0),
            [0xBF],
            "and the page is the placeholder grey, which is what a page that \
             could not be read has always been"
        );
    }
}

// ---- §5.7's images ---------------------------------------------------------------

/// An `<image>` whose reference the container answers is **embedded**, and one
/// it does not is counted.
///
/// This is what makes three of the fetched corpus's six pages draw: they are an
/// `<image>` and nothing else, so without this half the book is blank and the
/// row in `docs/features/epub.md` would still be true.
#[test]
fn an_image_reference_is_resolved_against_the_container() {
    // Black pixels, so the assertion is that the picture reached the page and
    // not merely that a rectangle did.
    let png = rgb_png(2, 2, &[0; 12]);
    let doc = Document::open(book(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="100" viewBox="0 0 100 100">
             <image x="0" y="0" width="100" height="100" href="photo.png"
                    preserveAspectRatio="none"/>
           </svg>"##,
        &[("EPUB/photo.png", png)],
    ))
    .expect("the book opens");
    let seen = greys(&doc, 0);
    assert!(
        seen.contains(&0),
        "the photograph reached the page: {seen:?}"
    );
    assert!(
        !seen.contains(&0xBF),
        "and it decoded rather than being the grey a raster the renderer \
         could not resolve gets -- which is what an `/XObject` registered \
         after `begin_page` produces, because the page's resource set was \
         already snapshotted"
    );
    assert!(
        !warnings(&doc)
            .iter()
            .any(|warning| matches!(warning, ArchiveWarning::SvgImageUnresolved { .. })),
        "and nothing was left unresolved"
    );
}

/// Section 7.8's fit, applied with the **image's own** proportions.
///
/// A square box and a two-to-one picture under the default `xMidYMid meet`
/// leaves the top and bottom of the box empty. The intrinsic size lives inside
/// the bytes, which is why the leaf crate carries `preserveAspectRatio` as a
/// string and this file resolves it: a build that stretched to fill would put
/// ink across the whole box, which is the same picture at the wrong
/// proportions.
#[test]
fn an_image_is_fitted_by_its_own_proportions() {
    let png = rgb_png(4, 2, &[0; 24]);
    let doc = Document::open(book(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="100" viewBox="0 0 100 100">
             <image x="0" y="0" width="100" height="100" href="photo.png"/>
           </svg>"##,
        &[("EPUB/photo.png", png)],
    ))
    .expect("the book opens");
    let bitmap = doc
        .page(0)
        .expect("a page")
        .render(&RenderOptions::default());
    let components = bitmap.components();
    let width = bitmap.width as usize;
    let at = |x: usize, y: usize| bitmap.data[(y * width + x) * components];
    // **Sampled inside the scene, which is the correction the injection matrix
    // asked for.** A 100-unit square scene on a 432-by-648 page is scaled by
    // 4.32 and centred, so it occupies rows 108 to 540 and *nothing* above row
    // 108 can ever be inked — the first draft sampled row 81 and passed
    // whatever the fit did. A two-to-one picture under `xMidYMid meet` fills
    // the box's width and half its height, centred: user y from 25 to 75, which
    // is rows 216 to 432.
    assert!(at(width / 2, 324) < 0x40, "the middle of the box is inked");
    assert!(
        at(width / 2, 150) > 0xC0,
        "and the top of the box is not, because the picture is half as tall \
         as it is wide"
    );
}

/// A reference the container does not answer is **counted per page**, not
/// silently drawn as nothing.
#[test]
fn an_unresolved_image_is_counted_rather_than_silent() {
    let doc = open(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="100">
             <image width="50" height="50" href="missing.png"/>
             <image width="50" height="50" href="photo.jpg"/>
           </svg>"##,
    );
    let counted: Vec<usize> = warnings(&doc)
        .into_iter()
        .filter_map(|warning| match warning {
            ArchiveWarning::SvgImageUnresolved { item, images } => {
                assert_eq!(item, "EPUB/draw.svg");
                Some(images)
            }
            _ => None,
        })
        .collect();
    assert_eq!(
        counted,
        [2],
        "one that names nothing and one whose entry is not in the container: \
         both are references this page did not draw"
    );
}

// ---- §10's text -------------------------------------------------------------------

/// **An SVG label extracts as text.**
///
/// This is the property the whole text seam exists for. A build that
/// rasterized a scene, or that drew glyph outlines as paths, would put the same
/// ink on the page and produce a document whose text cannot be searched,
/// selected or read aloud — and no picture would say so.
#[test]
fn svg_text_reaches_the_page_as_text() {
    let doc = open(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="200" height="100">
             <text x="10" y="50" font-family="serif" font-size="24">Hello</text>
           </svg>"##,
    );
    let text = doc
        .page(0)
        .expect("a page")
        .text()
        .plain_text()
        .trim()
        .to_owned();
    assert_eq!(text, "Hello");
    assert_eq!(
        text_origins(&doc),
        [10.0],
        "one text object, at the anchor the document stated"
    );
}

/// §10.9's `text-anchor` is applied by the caller, which is the half of the
/// seam the leaf crate cannot do.
///
/// Three runs at the same anchor, one per value: `start` puts its ink to the
/// right of the anchor, `end` to the left, `middle` across it. The assertion is
/// on the ink's own extent, because that is the only thing a build with a
/// guessed advance gets wrong in a way a coordinate would not show.
#[test]
fn text_anchor_is_applied_where_the_metrics_are() {
    let placed = |anchor: &str| -> f64 {
        let doc = open(&format!(
            r##"<svg xmlns="http://www.w3.org/2000/svg" width="200" height="40">
                 <text x="100" y="30" font-family="serif" font-size="20"
                       text-anchor="{anchor}">MMMM</text>
               </svg>"##
        ));
        let origins = text_origins(&doc);
        assert_eq!(origins.len(), 1, "one run");
        origins[0]
    };
    let start = placed("start");
    let middle = placed("middle");
    let end = placed("end");
    assert!(
        (start - 100.0).abs() < 1e-6,
        "`start` sets the run at the anchor: {start}"
    );
    // Four `M`s of Times-Roman at twenty units. `M` is 889 thousandths of an
    // em, so the run is 4 x 0.889 x 20 = 71.12 units wide. Section 10.9 puts
    // `middle` half a width left of the anchor and `end` a whole width left of
    // it -- arithmetic from Adobe's published AFM number, not a figure read
    // back out of this build.
    let width = 4.0 * 0.889 * 20.0;
    assert!(
        (middle - (100.0 - width / 2.0)).abs() < 0.5,
        "`middle` is half a width left of the anchor: {middle} against {}",
        100.0 - width / 2.0
    );
    assert!(
        (end - (100.0 - width)).abs() < 0.5,
        "`end` is a whole width left of it: {end} against {}",
        100.0 - width
    );
}

/// §10.4's per-glyph `x`: a number per character, each set where its number
/// says — which a build that took the first number set as one word at 10.
#[test]
fn an_x_per_character_sets_each_where_it_says() {
    let doc = open(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="300" height="60">
             <text x="10 50 90" y="40" font-family="serif" font-size="20">abc</text>
           </svg>"##,
    );
    let origins = text_origins(&doc);
    assert_eq!(origins.len(), 3, "a text object per character: {origins:?}");
    for (got, want) in origins.iter().zip([10.0, 50.0, 90.0]) {
        assert!((got - want).abs() < 1e-6, "{origins:?}");
    }
}

/// §10.5's rule (b), where the metrics are: a `<tspan>` with a `y` and no `x`
/// starts where the run before it ended, at its own `y`.
#[test]
fn a_y_without_an_x_continues_where_the_pen_is() {
    let doc = open(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="300" height="80">
             <text x="10" y="40" font-family="serif" font-size="20">One<tspan y="60">two</tspan></text>
           </svg>"##,
    );
    let matrices = text_matrices(&doc);
    assert_eq!(matrices.len(), 2, "{matrices:?}");
    // `One` in Times-Roman at twenty: 0.722 + 0.5 + 0.444 em.
    let advance = (0.722 + 0.5 + 0.444) * 20.0;
    assert!(
        (matrices[1][4] - (10.0 + advance)).abs() < 0.5,
        "after `One`, not back at 10: {matrices:?}"
    );
    assert!((matrices[1][5] - 60.0).abs() < 1e-6, "at the y it stated");
}

/// §10.5's `rotate`: the glyph turns about its own origin, which is the run
/// matrix's linear part turned and its translation untouched.
#[test]
fn a_rotated_glyph_turns_about_its_own_origin() {
    let doc = open(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="300" height="80">
             <text x="10" y="40" rotate="90" font-family="serif" font-size="20">a</text>
           </svg>"##,
    );
    let matrices = text_matrices(&doc);
    let [a, b, c, d, e, f] = matrices[0];
    // The flip, then a quarter turn clockwise in the downward space:
    // [1 0 0 -1] after [0 1 -1 0] is [0 1 1 0].
    for (got, want) in [a, b, c, d].iter().zip([0.0, 1.0, 1.0, 0.0]) {
        assert!((got - want).abs() < 1e-9, "the turn: {:?}", matrices[0]);
    }
    assert!(
        (e - 10.0).abs() < 1e-9 && (f - 40.0).abs() < 1e-9,
        "about the glyph's own origin: {:?}",
        matrices[0]
    );
}

/// A `<tspan>` that states no position of its own continues the run before it.
///
/// The pen is the caller's, because where a continuation begins depends on how
/// wide the text before it was. Asserted through extraction, which is the one
/// place a run drawn on top of the previous one would still look plausible.
#[test]
fn a_continuing_run_is_set_after_the_one_before_it() {
    let doc = open(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="300" height="60">
             <text x="10" y="40" font-family="serif" font-size="20">One<tspan>Two</tspan></text>
           </svg>"##,
    );
    // **The whitespace is collapsed before comparing, and that is the
    // extractor's convention rather than a defect.** Two text objects are two
    // runs to 9.10's extraction whatever their positions, and it separates them
    // with a newline; the question this test asks is where the second one was
    // *set*, which the newline says nothing about either way.
    assert_eq!(
        doc.page(0)
            .expect("a page")
            .text()
            .plain_text()
            .split_whitespace()
            .collect::<String>(),
        "OneTwo"
    );
    let origins = text_origins(&doc);
    assert_eq!(origins.len(), 2, "two runs: {origins:?}");
    // `One` in Times-Roman at twenty units: O is 722 thousandths of an em, n is
    // 500 and e is 444, so 1.666 em and 33.32 units. The continuation starts
    // there -- not at the anchor, and not on top of the run before it.
    let advance = (0.722 + 0.5 + 0.444) * 20.0;
    assert!((origins[0] - 10.0).abs() < 1e-6, "{origins:?}");
    assert!(
        (origins[1] - (10.0 + advance)).abs() < 0.5,
        "the second run begins where the first ended: {} against {}",
        origins[1],
        10.0 + advance
    );
}
