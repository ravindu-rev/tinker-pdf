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

// ---- what travels out ------------------------------------------------------------

/// Every subsystem the leaf crate declines reaches the caller as an
/// `ArchiveWarning::Svg` naming which — and naming the **item**, because a book
/// has many and *"a filter was not drawn"* is not something a host can act on.
#[test]
fn every_refusal_travels_out_named_with_its_item() {
    let doc = open(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10">
             <filter id="f"/><mask id="m"/><pattern id="p"/><marker id="k"/>
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
        tinker_pdf_svg::Warning::MaskUnsupported,
        tinker_pdf_svg::Warning::PatternUnsupported,
        tinker_pdf_svg::Warning::MarkerUnsupported,
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
