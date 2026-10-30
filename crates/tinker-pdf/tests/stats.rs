//! Counted work through every door (`docs/design/performance.md`): a render,
//! a page's text and its images, a save and a merge's save, and the PDF/A,
//! PDF/UA and PDF/X validators.
//!
//! A door asked for its stats hands back what it did, counted where the work
//! is done — the resource scopes it builds, the caches they keep, the
//! renderer, the walks of the page tree — and a door not asked hands back none
//! and produces the same bytes.
//!
//! **Every expected count is derived here** from what the fixture holds and
//! what today's code does with it, and the reason is written beside each
//! figure. A count that moves is a change in what a render does, which is the
//! finding the tally exists to make, and is read as one: never re-pinned to
//! match.
//!
//! What today's code does, once, so the derivations below can cite it:
//!
//! - a scope reads every font its `/Font` dictionary names when it is built;
//! - the interpreter asks for each glyph whether its font is a Type 3 font,
//!   which reads the font's dictionary by name again (`type3_glyph`);
//! - the renderer asks for each glyph's outline, and an outline not yet in
//!   the scope's cache is extracted from the font's program, which the scope
//!   reads once a font and keeps — a font with no program, kept as absent too;
//! - a font with no program of its own asks for a substitute once, reading
//!   its dictionary for the request: in a `bundled-fonts` build the bundled
//!   faces answer, and in the default build there is no one to ask;
//! - `Do` on a form asks for its scope twice, the interpreter and the
//!   renderer each, and a form's scope is built once and kept;
//! - an image XObject is decoded once a page and kept, an inline image every
//!   time, and a sampled image's decode reads its colour space once;
//! - text and image extraction interpret the page as a render does, over a
//!   scope with no font provider, so no substitute is asked for in either
//!   build and nothing is drawn: no glyph is shown to a renderer, no outline
//!   is extracted, and an image is decoded by the extraction, once for each
//!   entry it lists, never by a cache;
//! - a save's font pass walks the page tree once and interprets every page
//!   over a scope of its own, as text extraction does; the image pass walks
//!   it only to find placements for a resolution limit;
//! - each validator rule that reads every page walks the page tree for
//!   itself;
//! - a walk of the page tree reads every node until it has the pages it
//!   wants, and stops before the next; an editor walks its own tree for its
//!   page order until an edit first sets it, and an import walks its
//!   source's.

mod render_support;

use std::sync::Arc;

use tinker_pdf::write::{
    save, BilevelCodec, ContinuousCodec, ImagePolicy, ImageRecoding, SaveOptions, Saved,
};
use tinker_pdf::{
    Bitmap, Document, DocumentBuilder, Page, PdfACoverage, PdfUaCoverage, PdfXCoverage,
    PixelRegion, RenderOptions, SliceSource, Stats, TextOptions, WriteMode, WriteOptions,
};

/// Whether this build carries the bundled faces, which answer for a font with
/// no program of its own.
const BUNDLED: bool = cfg!(feature = "bundled-fonts");

/// A PDF of `objects`, numbered from 1 in order, with a cross-reference table
/// so that it opens without repair.
fn pdf(objects: &[Vec<u8>]) -> Vec<u8> {
    pdf_trailing(objects, "")
}

/// [`pdf`], with `extra` entries in its trailer.
fn pdf_trailing(objects: &[Vec<u8>], extra: &str) -> Vec<u8> {
    let mut out = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (index, body) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n", index + 1).as_bytes());
        out.extend_from_slice(body);
        out.extend_from_slice(b"\nendobj\n");
    }
    let xref = out.len();
    out.extend_from_slice(
        format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes(),
    );
    for offset in offsets {
        out.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R{extra} >>\nstartxref\n{xref}\n%%EOF\n",
            objects.len() + 1
        )
        .as_bytes(),
    );
    out
}

/// A stream object: `dict`'s entries, its `/Length`, and `data`.
fn stream(dict: &str, data: &[u8]) -> Vec<u8> {
    let mut out = format!("<< {dict} /Length {} >>\nstream\n", data.len()).into_bytes();
    out.extend_from_slice(data);
    out.extend_from_slice(b"\nendstream");
    out
}

fn object(text: &str) -> Vec<u8> {
    text.as_bytes().to_vec()
}

/// The objects every fixture starts with: 1 the catalog, 2 the page tree,
/// 3 the page — whose resources and annotations are `resources` and `annots`
/// — 4 its content, and 5 to 7 the synthetic TrueType face
/// (`render_support::curvy_font`) as a simple font with its descriptor and
/// program, which draws an outline for every printable code but the space.
/// A fixture's own objects follow, from 8.
fn fixture(resources: &str, annots: &str, content: &str, own: Vec<Vec<u8>>) -> Vec<u8> {
    let mut objects = vec![
        object("<< /Type /Catalog /Pages 2 0 R >>"),
        object("<< /Type /Pages /Count 1 /Kids [3 0 R] >>"),
        object(&format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 100]\n\
             /Resources << {resources} >> {annots} /Contents 4 0 R >>"
        )),
        stream("", content.as_bytes()),
        object(&format!(
            "<< /Type /Font /Subtype /TrueType /BaseFont /Curvy /FirstChar 32 /LastChar 126\n\
             /Widths [320{}] /Encoding /WinAnsiEncoding /FontDescriptor 6 0 R >>",
            " 640".repeat(94)
        )),
        object(
            "<< /Type /FontDescriptor /FontName /Curvy /Flags 32 /FontBBox [0 0 1000 1000]\n\
             /ItalicAngle 0 /Ascent 800 /Descent -200 /CapHeight 700 /StemV 80 /FontFile2 7 0 R >>",
        ),
        stream("", &render_support::curvy_font()),
    ];
    objects.extend(own);
    pdf(&objects)
}

/// A 2 x 2 sampled image in `space`.
fn image(space: &str) -> Vec<u8> {
    stream(
        &format!(
            "/Type /XObject /Subtype /Image /Width 2 /Height 2 /ColorSpace {space} /BitsPerComponent 8"
        ),
        &[0, 0, 0, 255, 255, 255, 255, 255, 255, 0, 0, 0],
    )
}

/// The first page of `bytes`.
fn first_page(bytes: Vec<u8>) -> (Document, u32) {
    (Document::open(bytes).expect("the fixture opens"), 0)
}

/// `page` rendered at 72 dpi, counting.
fn counted(page: &Page) -> Stats {
    page.render(&asking())
        .stats
        .expect("a render asked for its stats hands them back")
}

/// The default options, asking for stats.
fn asking() -> RenderOptions {
    RenderOptions {
        stats: true,
        ..RenderOptions::default()
    }
}

/// The text the exit's page shows, one string a font: `(resource, string)`.
const SHOWN: [(&str, &str); 3] = [("F1", "Curvy!"), ("F2", "Hello"), ("F3", "aaa")];

/// The exit's page: `SHOWN`'s glyphs in three fonts — `F1` the embedded face,
/// `F2` Helvetica and `F3` Courier with no program of their own — and one
/// image drawn twice.
fn glyphs_and_an_image_page() -> Vec<u8> {
    let mut content = String::new();
    for (line, (font, text)) in SHOWN.iter().enumerate() {
        content.push_str(&format!(
            "BT /{font} 12 Tf 10 {} Td ({text}) Tj ET\n",
            80 - line * 20
        ));
    }
    content.push_str("q 20 0 0 20 120 10 cm /Im1 Do Q\nq 20 0 0 20 150 10 cm /Im1 Do Q\n");
    fixture(
        "/Font << /F1 5 0 R /F2 << /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>\n\
                  /F3 << /Type /Font /Subtype /Type1 /BaseFont /Courier >> >>\n\
         /XObject << /Im1 8 0 R >>",
        "",
        &content,
        vec![image("/DeviceRGB")],
    )
}

/// What [`glyphs_and_an_image_page`] costs, derived from `SHOWN`.
fn glyphs_and_an_image_stats() -> Stats {
    let fonts = SHOWN.len() as u64;
    let shown: u64 = SHOWN.iter().map(|(_, text)| text.len() as u64).sum();
    // An outline is extracted once a (font, code) pair; the rest are hits.
    let distinct: u64 = SHOWN
        .iter()
        .map(|(_, text)| {
            let mut codes: Vec<u8> = text.bytes().collect();
            codes.sort_unstable();
            codes.dedup();
            codes.len() as u64
        })
        .sum();
    let embedded = 1;
    let without_a_program = fonts - embedded;
    let mut expected = Stats::default();
    expected.glyphs_shown = shown;
    // Every glyph the embedded face has an outline for; the two fonts with no
    // program draw only where the bundled faces stand in.
    expected.outlines_filled = if BUNDLED {
        shown
    } else {
        SHOWN[0].1.len() as u64
    };
    // The page's scope reads its three fonts, each glyph reads its font's
    // dictionary to ask whether it is a Type 3 font, and in a bundled build
    // each font with no program reads its own again to ask for a substitute.
    expected.font_dictionaries_resolved =
        fonts + shown + if BUNDLED { without_a_program } else { 0 };
    expected.glyph_outline_misses = distinct;
    expected.glyph_outline_hits = shown - distinct;
    // One program read a font, at its first outline; every other extraction
    // finds it, or finds it absent.
    expected.font_program_misses = fonts;
    expected.font_program_hits = distinct - fonts;
    // Decoded at the first `Do` and found at the second.
    expected.images_decoded = 1;
    expected.image_misses = 1;
    expected.image_hits = 1;
    expected
}

/// The bitmap without its stats, for comparing a counted render with an
/// uncounted one: every other field must be the same.
#[track_caller]
fn same_bitmap(counted: &Bitmap, plain: &Bitmap, what: &str) {
    assert_eq!(
        (
            counted.width,
            counted.height,
            counted.stride,
            counted.format
        ),
        (plain.width, plain.height, plain.stride, plain.format),
        "{what}: a different shape"
    );
    assert!(counted.data == plain.data, "{what}: the pixels differ");
    assert_eq!(
        counted.warnings, plain.warnings,
        "{what}: the warnings differ"
    );
    assert_eq!(
        counted.premultiplied, plain.premultiplied,
        "{what}: premultiplied differs"
    );
}

#[test]
fn render_stats_are_none_unless_asked() {
    let (document, index) = first_page(every_counted_path_page());
    let page = document.page(index).expect("a page");
    let plain = RenderOptions::default();
    assert!(!plain.stats, "stats are off by default");

    let list = page.display_list();
    let region = RenderOptions {
        region: Some(PixelRegion::new(0, 0, 50, 50)),
        ..RenderOptions::default()
    };
    let unasked = [
        ("the page", page.render(&plain)),
        ("a tile", page.render(&region)),
        ("a display list", list.render(&plain)),
        (
            "a form",
            page.render_form(b"Fm1", &plain).expect("the form draws"),
        ),
        (
            "an annotation",
            page.render_annotation(0, &plain)
                .expect("the annotation draws"),
        ),
    ];
    for (what, bitmap) in &unasked {
        assert_eq!(bitmap.stats, None, "{what}: stats nobody asked for");
    }

    let asked = asking();
    let region = RenderOptions {
        stats: true,
        ..region
    };
    let answered = [
        ("the page", page.render(&asked)),
        ("a tile", page.render(&region)),
        ("a display list", list.render(&asked)),
        (
            "a form",
            page.render_form(b"Fm1", &asked).expect("the form draws"),
        ),
        (
            "an annotation",
            page.render_annotation(0, &asked)
                .expect("the annotation draws"),
        ),
    ];
    for (what, bitmap) in &answered {
        let stats = bitmap
            .stats
            .unwrap_or_else(|| panic!("{what}: asked, and no stats"));
        assert!(stats.work() > 0, "{what}: asked, and nothing counted");
    }

    let png = unasked[0].1.to_png().expect("a picture");
    let read = Bitmap::from_png(&png).expect("it reads back");
    assert_eq!(read.stats, None, "a bitmap read from a PNG counted nothing");
}

/// The exit's page: n glyphs in k fonts and one image drawn twice report n
/// glyphs shown, the font resolutions today's code makes, one decode and one
/// cache hit — every other counter derived alike.
#[test]
fn render_stats_count_what_a_generated_page_draws() {
    let (document, index) = first_page(glyphs_and_an_image_page());
    let page = document.page(index).expect("a page");
    let stats = counted(&page);
    let expected = glyphs_and_an_image_stats();
    assert_eq!(stats, expected, "bundled faces: {BUNDLED}");
    assert_eq!(stats.glyphs_shown, 14, "six, five and three glyphs");
    assert_eq!(stats.images_decoded, 1, "one decode");
    assert_eq!(stats.image_hits, 1, "one cache hit");
    assert_eq!(
        stats.work(),
        stats.glyphs_shown
            + stats.outlines_filled
            + stats.font_dictionaries_resolved
            + stats.images_decoded
            + stats.font_program_hits
            + stats.font_program_misses
            + stats.image_hits
            + stats.image_misses
            + stats.glyph_outline_hits
            + stats.glyph_outline_misses,
        "the total is every step counted, and nothing else counted"
    );

    // The page again is the same work again: nothing is kept between two
    // direct renders, and a count depends on nothing but the page.
    assert_eq!(counted(&page), expected, "a second render");
    // At another scale and in a region the work is the page's, not the
    // canvas's: every glyph and image here is drawn wherever it lands.
    let scaled = page
        .render(&RenderOptions {
            scale: 3.0,
            ..asking()
        })
        .stats;
    assert_eq!(scaled, Some(expected), "at three times the scale");
}

/// A form invoked three times builds its scope once, and what runs inside it
/// counts into the page's tally through the scope the cache keeps.
#[test]
fn a_form_shown_three_times_builds_its_scope_once() {
    let (document, index) = first_page(form_page());
    let page = document.page(index).expect("a page");
    let shown = 3 * FORM_TEXT.len() as u64;
    let distinct = 3;
    let mut expected = Stats::default();
    expected.glyphs_shown = shown;
    expected.outlines_filled = shown;
    // The page's scope reads its one font, the form's scope its own one once,
    // and every glyph its font's dictionary.
    expected.font_dictionaries_resolved = 1 + 1 + shown;
    // Each `Do` asks twice; the first ask builds the scope.
    expected.form_scope_misses = 1;
    expected.form_scope_hits = 2 * 3 - 1;
    // The form's scope keeps its outlines across the three invocations.
    expected.glyph_outline_misses = distinct;
    expected.glyph_outline_hits = shown - distinct;
    expected.font_program_misses = 1;
    expected.font_program_hits = distinct - 1;
    assert_eq!(counted(&page), expected);
}

/// The text the form shows: four glyphs, three of them distinct.
const FORM_TEXT: &str = "abca";

/// A page invoking `Fm1` three times; the form's own resources name the
/// embedded face, and the page's name Helvetica, which nothing on it uses.
fn form_page() -> Vec<u8> {
    fixture(
        "/Font << /F2 << /Type /Font /Subtype /Type1 /BaseFont /Helvetica >> >>\n\
         /XObject << /Fm1 8 0 R >>",
        "",
        "q 1 0 0 1 10 10 cm /Fm1 Do Q\nq 1 0 0 1 10 40 cm /Fm1 Do Q\nq 1 0 0 1 10 70 cm /Fm1 Do Q\n",
        vec![form_with_text(FORM_TEXT)],
    )
}

/// A form XObject whose own resources name the embedded face as `F1`, showing
/// `text`.
fn form_with_text(text: &str) -> Vec<u8> {
    stream(
        "/Type /XObject /Subtype /Form /BBox [0 0 80 20] /Resources << /Font << /F1 5 0 R >> >>",
        format!("BT /F1 10 Tf 2 5 Td ({text}) Tj ET").as_bytes(),
    )
}

/// `render_form` reads the form's name in a scope of its own before it
/// renders, and that scope's fonts are the door's work too.
#[test]
fn render_form_counts_the_scope_it_reads_the_name_in() {
    let (document, index) = first_page(form_page());
    let page = document.page(index).expect("a page");
    let stats = page
        .render_form(b"Fm1", &asking())
        .expect("the form draws")
        .stats
        .expect("asked");
    let shown = FORM_TEXT.len() as u64;
    let distinct = 3;
    let mut expected = Stats::default();
    expected.glyphs_shown = shown;
    expected.outlines_filled = shown;
    // The scope the name is read in and the render's own each read the
    // page's one font; the form's scope reads its own once; every glyph reads
    // its font's dictionary.
    expected.font_dictionaries_resolved = 1 + 1 + 1 + shown;
    expected.form_scope_misses = 1;
    expected.form_scope_hits = 1;
    expected.glyph_outline_misses = distinct;
    expected.glyph_outline_hits = shown - distinct;
    expected.font_program_misses = 1;
    expected.font_program_hits = distinct - 1;
    assert_eq!(stats, expected);
}

/// Two images in one ICC space compile its profile once. The profile here is
/// one no parser accepts, which is refused once and found refused after.
#[test]
fn a_profile_two_images_share_is_compiled_once() {
    let bytes = fixture(
        "/XObject << /Im1 8 0 R /Im2 9 0 R >>",
        "",
        "q 20 0 0 20 10 10 cm /Im1 Do Q\nq 20 0 0 20 40 10 cm /Im2 Do Q\n\
         q 20 0 0 20 70 10 cm /Im1 Do Q\n",
        vec![
            image("[/ICCBased 10 0 R]"),
            image("[/ICCBased 10 0 R]"),
            stream("/N 3", b"not an ICC profile"),
        ],
    );
    let (document, index) = first_page(bytes);
    let page = document.page(index).expect("a page");
    let mut expected = Stats::default();
    // Each image decoded once, the third `Do` finding the first.
    expected.images_decoded = 2;
    expected.image_misses = 2;
    expected.image_hits = 1;
    // Each decode reads its space once: the first compiles the profile.
    expected.icc_transform_misses = 1;
    expected.icc_transform_hits = 1;
    assert_eq!(counted(&page), expected);
}

/// An inline image is never cached, so each one is a decode.
#[test]
fn every_inline_image_is_decoded() {
    let inline = "BI /W 2 /H 2 /CS /RGB /BPC 8 /F /AHx ID 000000FFFFFFFFFFFF000000> EI";
    let bytes = fixture(
        "",
        "",
        &format!(
            "q 20 0 0 20 10 10 cm {inline} Q\nq 20 0 0 20 40 10 cm {inline} Q\n\
             q 20 0 0 20 70 10 cm {inline} Q\n"
        ),
        Vec::new(),
    );
    let (document, index) = first_page(bytes);
    let page = document.page(index).expect("a page");
    let mut expected = Stats::default();
    expected.images_decoded = 3;
    assert_eq!(counted(&page), expected);
}

/// A tiling pattern's cell is drawn by a renderer of its own over a scope of
/// its own, and both count into the page's tally.
#[test]
fn a_pattern_cell_counts_what_it_draws() {
    let bytes = fixture(
        "/Pattern << /P1 8 0 R >>",
        "",
        "/Pattern cs /P1 scn 0 0 200 100 re f\n",
        vec![stream(
            "/Type /Pattern /PatternType 1 /PaintType 1 /TilingType 1 /BBox [0 0 20 10]\n\
             /XStep 20 /YStep 10 /Resources << /Font << /F1 5 0 R >> >>",
            b"BT /F1 6 Tf 1 2 Td (ab) Tj ET",
        )],
    );
    let (document, index) = first_page(bytes);
    let page = document.page(index).expect("a page");
    // One fill, so one cell: drawn once and repeated across the fill.
    let mut expected = Stats::default();
    expected.glyphs_shown = 2;
    expected.outlines_filled = 2;
    // The cell's scope reads its one font; each glyph reads it again.
    expected.font_dictionaries_resolved = 1 + 2;
    expected.glyph_outline_misses = 2;
    expected.font_program_misses = 1;
    expected.font_program_hits = 1;
    assert_eq!(counted(&page), expected);
}

/// An annotation's appearance is drawn over two scopes of its own, the
/// interpreter's and the renderer's, and both count — through the page's
/// render and through the annotation's own door alike.
#[test]
fn an_annotation_counts_both_scopes_its_appearance_builds() {
    let (document, index) = first_page(annotation_page());
    let page = document.page(index).expect("a page");
    let shown = ANNOTATION_TEXT.len() as u64;
    let mut expected = Stats::default();
    expected.glyphs_shown = shown;
    expected.outlines_filled = shown;
    // Both of the appearance's scopes read its one font; every glyph reads
    // it again.
    expected.font_dictionaries_resolved = 2 + shown;
    expected.glyph_outline_misses = shown;
    expected.font_program_misses = 1;
    expected.font_program_hits = shown - 1;
    assert_eq!(counted(&page), expected, "the page, annotations drawn");
    let part = page
        .render_annotation(0, &asking())
        .expect("the annotation draws")
        .stats;
    assert_eq!(part, Some(expected), "the annotation's own door");

    let without = page
        .render(&RenderOptions {
            annotations: false,
            ..asking()
        })
        .stats;
    assert_eq!(without, Some(Stats::default()), "annotations not drawn");
}

/// The text the annotation's appearance shows, every glyph distinct.
const ANNOTATION_TEXT: &str = "abc";

/// A page with nothing on it but one square annotation whose appearance shows
/// `ANNOTATION_TEXT` in the embedded face, named in its own resources.
fn annotation_page() -> Vec<u8> {
    fixture(
        "",
        "/Annots [8 0 R]",
        "",
        vec![
            object("<< /Type /Annot /Subtype /Square /Rect [20 20 120 60] /AP << /N 9 0 R >> >>"),
            stream(
                "/Type /XObject /Subtype /Form /BBox [0 0 100 40] /Resources << /Font << /F1 5 0 R >> >>",
                format!("BT /F1 12 Tf 4 10 Td ({ANNOTATION_TEXT}) Tj ET").as_bytes(),
            ),
        ],
    )
}

/// A display list renders through the resources it keeps: its first replay
/// pays for what the recording did not draw, its second finds all of it, and
/// neither interprets the page again.
#[test]
fn a_display_list_counts_through_the_resources_it_keeps() {
    let (document, index) = first_page(glyphs_and_an_image_page());
    let page = document.page(index).expect("a page");
    let list = page.display_list();
    assert!(list.is_retained(), "the page is small enough to keep");
    let direct = glyphs_and_an_image_stats();

    let first = list.render(&asking()).stats.expect("asked");
    let mut expected = direct;
    // The interpretation ran when the list was recorded, so no scope is built
    // and no glyph asks whether its font is a Type 3 font; only a bundled
    // build's substitute requests, made at the first outline, read a
    // dictionary.
    let without_a_program = 2;
    expected.font_dictionaries_resolved = if BUNDLED { without_a_program } else { 0 };
    assert_eq!(first, expected, "the first replay");

    let second = list.render(&asking()).stats.expect("asked");
    let mut expected = Stats::default();
    expected.glyphs_shown = direct.glyphs_shown;
    expected.outlines_filled = direct.outlines_filled;
    // Every outline and the image are the list's now.
    expected.glyph_outline_hits = direct.glyphs_shown;
    expected.image_hits = 2;
    assert_eq!(second, expected, "the second replay");

    // A replay not asked counts nothing, and does not disturb what the next
    // one that is asked finds.
    assert_eq!(list.render(&RenderOptions::default()).stats, None);
    assert_eq!(list.render(&asking()).stats, Some(expected), "a third");
}

/// An annotation a display list keeps is replayed through the scope it was
/// recorded with, and what its appearance draws counts into each replay's
/// tally all the same.
#[test]
fn a_display_list_counts_the_annotations_it_keeps() {
    let (document, index) = first_page(annotation_page());
    let page = document.page(index).expect("a page");
    let list = page.display_list();
    assert!(list.is_retained(), "the page is small enough to keep");
    let shown = ANNOTATION_TEXT.len() as u64;

    let mut expected = Stats::default();
    expected.glyphs_shown = shown;
    expected.outlines_filled = shown;
    // The appearance's scopes were built, and its glyphs asked about their
    // font, when the list was recorded; the replay extracts the outlines.
    expected.glyph_outline_misses = shown;
    expected.font_program_misses = 1;
    expected.font_program_hits = shown - 1;
    assert_eq!(
        list.render(&asking()).stats,
        Some(expected),
        "the first replay"
    );

    let mut expected = Stats::default();
    expected.glyphs_shown = shown;
    expected.outlines_filled = shown;
    expected.glyph_outline_hits = shown;
    assert_eq!(
        list.render(&asking()).stats,
        Some(expected),
        "the second replay"
    );
}

/// A page that reaches every place the render counts: three fonts, an image
/// drawn twice, two images in one ICC space, an inline image, a form with
/// resources of its own shown three times, a tiling pattern whose cell shows
/// text, and an annotation whose appearance does.
fn every_counted_path_page() -> Vec<u8> {
    let inline = "BI /W 2 /H 2 /CS /RGB /BPC 8 /F /AHx ID 000000FFFFFFFFFFFF000000> EI";
    let mut content = String::new();
    for (line, (font, text)) in SHOWN.iter().enumerate() {
        content.push_str(&format!(
            "BT /{font} 12 Tf 10 {} Td ({text}) Tj ET\n",
            90 - line * 12
        ));
    }
    content.push_str(
        "q 10 0 0 10 100 80 cm /Im1 Do Q\nq 10 0 0 10 115 80 cm /Im1 Do Q\n\
         q 10 0 0 10 130 80 cm /Im2 Do Q\nq 10 0 0 10 145 80 cm /Im3 Do Q\n",
    );
    content.push_str(&format!("q 10 0 0 10 160 80 cm {inline} Q\n"));
    content.push_str(
        "q 1 0 0 1 100 40 cm /Fm1 Do Q\nq 1 0 0 1 100 55 cm /Fm1 Do Q\n\
         q 0.5 0 0 0.5 150 50 cm /Fm1 Do Q\n",
    );
    content.push_str("/Pattern cs /P1 scn 5 5 90 40 re f\n");
    fixture(
        "/Font << /F1 5 0 R /F2 << /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>\n\
                  /F3 << /Type /Font /Subtype /Type1 /BaseFont /Courier >> >>\n\
         /XObject << /Im1 8 0 R /Im2 9 0 R /Im3 10 0 R /Fm1 12 0 R >>\n\
         /Pattern << /P1 13 0 R >>",
        "/Annots [14 0 R]",
        &content,
        vec![
            image("/DeviceRGB"),
            image("[/ICCBased 11 0 R]"),
            image("[/ICCBased 11 0 R]"),
            stream("/N 3", b"not an ICC profile"),
            form_with_text(FORM_TEXT),
            stream(
                "/Type /Pattern /PatternType 1 /PaintType 1 /TilingType 1 /BBox [0 0 20 10]\n\
                 /XStep 20 /YStep 10 /Resources << /Font << /F1 5 0 R >> >>",
                b"BT /F1 6 Tf 1 2 Td (ab) Tj ET",
            ),
            object("<< /Type /Annot /Subtype /Square /Rect [120 5 195 35] /AP << /N 15 0 R >> >>"),
            stream(
                "/Type /XObject /Subtype /Form /BBox [0 0 75 30] /Resources << /Font << /F1 5 0 R >> >>",
                b"BT /F1 12 Tf 4 10 Td (abc) Tj ET",
            ),
        ],
    )
}

/// Stats on and off draw the same bitmap — the page, each tile of a lattice
/// over it, a display list's replays, a form and an annotation on their own —
/// at two scales.
#[test]
fn stats_do_not_change_a_pixel() {
    let (document, index) = first_page(every_counted_path_page());
    let page = document.page(index).expect("a page");
    let list = page.display_list();
    for scale in [1.0, 2.5] {
        let plain = RenderOptions {
            scale,
            ..RenderOptions::default()
        };
        let asked = RenderOptions {
            stats: true,
            ..plain.clone()
        };
        let whole = page.render(&plain);
        let counted = page.render(&asked);
        assert!(counted.stats.is_some(), "asked at {scale}x");
        same_bitmap(&counted, &whole, &format!("the page at {scale}x"));
        assert!(
            whole.data.iter().any(|&b| b != 255),
            "the page draws something at {scale}x"
        );

        // Tiles of a lattice that does not divide the page, so the last row
        // and column are trimmed.
        let (width, height) = (whole.width, whole.height);
        let step = (width / 3).max(1);
        for top in (0..height).step_by(step as usize) {
            for left in (0..width).step_by(step as usize) {
                let region = Some(PixelRegion::new(left, top, step, step));
                let tile = |options: &RenderOptions| {
                    page.render(&RenderOptions {
                        region,
                        ..options.clone()
                    })
                };
                same_bitmap(
                    &tile(&asked),
                    &tile(&plain),
                    &format!("the tile at ({left}, {top}), {scale}x"),
                );
            }
        }

        for round in 0..2 {
            same_bitmap(
                &list.render(&asked),
                &list.render(&plain),
                &format!("replay {round} at {scale}x"),
            );
        }
        same_bitmap(
            &list.render(&asked),
            &whole,
            &format!("a replay against the page at {scale}x"),
        );
        same_bitmap(
            &page.render_form(b"Fm1", &asked).expect("the form draws"),
            &page.render_form(b"Fm1", &plain).expect("the form draws"),
            &format!("the form at {scale}x"),
        );
        same_bitmap(
            &page
                .render_annotation(0, &asked)
                .expect("the annotation draws"),
            &page
                .render_annotation(0, &plain)
                .expect("the annotation draws"),
            &format!("the annotation at {scale}x"),
        );
    }
}

/// Two renders at once — of one page, and of one display list from two
/// threads — each count their own work and nothing of the other's: a tally is
/// the render's, never the page's or the list's, and a scope the list keeps is
/// handed to each replay bound to that replay's.
#[test]
fn two_renders_at_once_count_apart() {
    let (document, index) = first_page(every_counted_path_page());
    let page = document.page(index).expect("a page");
    let list = page.display_list();
    assert!(list.is_retained(), "the page is small enough to keep");
    // The first replay fills what the recording left unfilled; from the
    // second on, every replay does the same work, as every render of the page
    // does.
    let _ = list.render(&asking());
    let replay = || list.render(&asking()).stats.expect("asked");
    let direct = || counted(&page);
    let renders: [(&str, &(dyn Fn() -> Stats + Sync)); 2] =
        [("the page", &direct), ("a display list", &replay)];
    for (what, render) in renders {
        let alone = render();
        let start = std::sync::Barrier::new(2);
        let together: Vec<Stats> = std::thread::scope(|scope| {
            let threads: Vec<_> = (0..2)
                .map(|_| {
                    scope.spawn(|| {
                        start.wait();
                        render()
                    })
                })
                .collect();
            threads
                .into_iter()
                .map(|thread| thread.join().expect("a render"))
                .collect()
        });
        assert_eq!(together, [alone, alone], "{what}: two at once");
    }
}

/// How many glyphs `SHOWN` shows: one a byte, the space included.
fn shown() -> u64 {
    SHOWN.iter().map(|(_, text)| text.len() as u64).sum()
}

/// A page's text, as extraction reports it, for comparing a counted
/// extraction with an uncounted one: every field, to the bit.
fn text_of(page: &tinker_pdf::TextPage) -> String {
    format!("{page:?}")
}

/// Text extraction counts what its scope reads: each font the page's
/// resources name when the scope is built, the font dictionary each glyph
/// reads to ask whether it is a Type 3 font, and each form's scope the
/// interpreter asks for. Nothing is drawn, so nothing else counts. The text
/// is the uncounted text, in either order.
#[test]
fn text_stats_count_font_resolutions() {
    let (document, index) = first_page(glyphs_and_an_image_page());
    let page = document.page(index).expect("a page");
    let content_order = TextOptions {
        content_order: true,
    };
    for options in [TextOptions::default(), content_order] {
        let (text, stats) = page.text_counted(&options);
        assert_eq!(
            text_of(&text),
            text_of(&page.text_with(&options)),
            "{options:?}: counting changed the text"
        );
        let mut expected = Stats::default();
        // Three fonts read when the scope is built, and one lookup a glyph.
        // The image is drawn twice and decoded by nobody.
        expected.font_dictionaries_resolved = SHOWN.len() as u64 + shown();
        assert_eq!(stats, expected, "{options:?}");
    }

    let (document, index) = first_page(form_page());
    let page = document.page(index).expect("a page");
    let (text, stats) = page.text_counted(&TextOptions::default());
    assert_eq!(text_of(&text), text_of(&page.text()));
    let shown = 3 * FORM_TEXT.len() as u64;
    let mut expected = Stats::default();
    // The page's scope reads its one font, the form's scope its own one once,
    // and every glyph its font's dictionary.
    expected.font_dictionaries_resolved = 1 + 1 + shown;
    // Only the interpreter asks for the form's scope at each `Do`; the first
    // ask builds it.
    expected.form_scope_misses = 1;
    expected.form_scope_hits = 3 - 1;
    assert_eq!(stats, expected);
}

/// The images [`images_page`] draws: `Im1` three times, once inside a form
/// with no resources of its own, and `Im2`, which has a soft mask, once.
const IMAGE_XOBJECTS: [(&str, u64); 2] = [("Im1", 3), ("Im2", 1)];

/// How many inline images [`images_page`] draws.
const INLINE_IMAGES: u64 = 2;

/// A page of two image XObjects — `Im1` drawn twice on the page and once in a
/// form that keeps the page's scope, `Im2` soft-masked — and two inline
/// images.
fn images_page() -> Vec<u8> {
    let inline = "BI /W 2 /H 2 /CS /RGB /BPC 8 /F /AHx ID 000000FFFFFFFFFFFF000000> EI";
    let mut content = String::new();
    content.push_str("q 20 0 0 20 10 10 cm /Im1 Do Q\nq 20 0 0 20 40 10 cm /Im2 Do Q\n");
    content.push_str("q 20 0 0 20 70 10 cm /Im1 Do Q\nq 1 0 0 1 100 10 cm /Fm1 Do Q\n");
    for at in 0..INLINE_IMAGES {
        content.push_str(&format!("q 20 0 0 20 {} 60 cm {inline} Q\n", 10 + at * 30));
    }
    let mut masked = image("/DeviceRGB");
    let at = masked
        .windows(9)
        .position(|w| w == b"/Length 1")
        .expect("the image's dictionary");
    masked.splice(at..at, b"/SMask 11 0 R ".iter().copied());
    fixture(
        "/XObject << /Im1 8 0 R /Im2 9 0 R /Fm1 10 0 R >>",
        "",
        &content,
        vec![
            image("/DeviceRGB"),
            masked,
            stream(
                "/Type /XObject /Subtype /Form /BBox [0 0 30 30]",
                b"q 20 0 0 20 5 5 cm /Im1 Do Q",
            ),
            image("/DeviceGray"),
        ],
    )
}

/// Image extraction counts a decoder run for each entry it lists — an image
/// XObject once however often it is drawn in one space, its soft mask part of
/// its decode — and for each inline image, which is its own entry; and the
/// form's scope, which the interpreter and the extraction each ask for. The
/// list is the uncounted list.
#[test]
fn image_stats_count_the_decodes_made() {
    let (document, index) = first_page(images_page());
    let page = document.page(index).expect("a page");
    let (images, stats) = page.images_counted();
    assert_eq!(images, page.images(), "counting changed the list");
    let listed = IMAGE_XOBJECTS.len() as u64 + INLINE_IMAGES;
    assert_eq!(images.len() as u64, listed);
    for (name, drawn) in IMAGE_XOBJECTS {
        let entry = images
            .iter()
            .find(|image| image.name == name.as_bytes())
            .unwrap_or_else(|| panic!("{name} is listed"));
        assert_eq!(entry.placements.len() as u64, drawn, "{name}");
    }
    assert!(
        images.iter().any(|image| image.soft_mask.is_some()),
        "Im2's soft mask is described"
    );
    let mut expected = Stats::default();
    expected.images_decoded = listed;
    // The form brings no resources, so it draws in the page's scope: the
    // interpreter's ask records that, and the extraction's at the form's
    // start finds it recorded.
    expected.form_scope_misses = 1;
    expected.form_scope_hits = 1;
    assert_eq!(stats, expected);
}

/// A save not asked hands back no stats, a counting editor's included, and a
/// save asked writes the same bytes and reports the same passes. What it
/// counts is its passes': the font pass walks the page tree, reads each
/// page's fonts and each glyph's font dictionary, and selects a glyph in the
/// program of each glyph a font that is an object shows; the image pass walks
/// the tree when a resolution limit asks it to find placements.
#[test]
fn save_stats_are_none_unless_asked() {
    let document = Document::open(glyphs_and_an_image_page()).expect("the fixture opens");
    let plain = SaveOptions::default();
    assert!(!plain.stats, "stats are off by default");
    let downsampling = ImagePolicy::Recode(
        ImageRecoding::new(ContinuousCodec::Keep, BilevelCodec::Keep).with_max_ppi(72.0),
    );
    for (what, images, image_walks) in [
        ("the default passes", ImagePolicy::Keep, 0),
        ("a resolution limit", downsampling, 1),
    ] {
        let unasked = SaveOptions {
            images,
            ..SaveOptions::default()
        };
        let asked = SaveOptions {
            stats: true,
            ..unasked.clone()
        };
        let plain = save(&mut document.editor(), &unasked);
        assert_eq!(plain.stats, None, "{what}: stats nobody asked for");
        let mut editor = document.editor();
        editor.count_work();
        let quiet = save(&mut editor, &unasked);
        assert_eq!(quiet.stats, None, "{what}: a counting editor, not asked");
        same_save(&quiet, &plain, what);

        let counted = save(&mut document.editor(), &asked);
        same_save(&counted, &plain, what);
        let mut expected = Stats::default();
        // Each walk reads the root and the one page.
        expected.page_walks = 1 + image_walks;
        expected.page_nodes_visited = 2 * expected.page_walks;
        expected.font_dictionaries_resolved = SHOWN.len() as u64 + shown();
        // `F1` is the one font that is an object, the others written into the
        // resource dictionary: the first glyph it shows reads its program and
        // the rest find it.
        let embedded = SHOWN[0].1.len() as u64;
        expected.font_program_misses = 1;
        expected.font_program_hits = embedded - 1;
        assert_eq!(counted.stats, Some(expected), "{what}");
    }
}

/// Two saves are the same save but for their stats.
#[track_caller]
fn same_save(counted: &Saved, plain: &Saved, what: &str) {
    assert!(counted.bytes == plain.bytes, "{what}: the bytes differ");
    assert_eq!(counted.fonts, plain.fonts, "{what}: the font pass differs");
    assert_eq!(
        counted.images, plain.images,
        "{what}: the image pass differs"
    );
}

/// The text page `page` of part `part` shows.
fn part_line(part: usize, page: usize) -> String {
    format!("Page {page} of part {part}")
}

/// A document of ten pages, each with resources of its own naming the
/// embedded face, showing [`part_line`].
fn part(part: usize) -> Vec<u8> {
    const PAGES: usize = 10;
    // 1 the catalog, 2 the page tree, 3 to 5 the face, then each page and its
    // content.
    let kids: Vec<String> = (0..PAGES).map(|at| format!("{} 0 R", 6 + 2 * at)).collect();
    let mut objects = vec![
        object("<< /Type /Catalog /Pages 2 0 R >>"),
        object(&format!(
            "<< /Type /Pages /Count {PAGES} /Kids [{}] >>",
            kids.join(" ")
        )),
        object(&format!(
            "<< /Type /Font /Subtype /TrueType /BaseFont /Curvy /FirstChar 32 /LastChar 126\n\
             /Widths [320{}] /Encoding /WinAnsiEncoding /FontDescriptor 4 0 R >>",
            " 640".repeat(94)
        )),
        object(
            "<< /Type /FontDescriptor /FontName /Curvy /Flags 32 /FontBBox [0 0 1000 1000]\n\
             /ItalicAngle 0 /Ascent 800 /Descent -200 /CapHeight 700 /StemV 80 /FontFile2 5 0 R >>",
        ),
        stream("", &render_support::curvy_font()),
    ];
    for at in 0..PAGES {
        objects.push(object(&format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 100]\n\
             /Resources << /Font << /F1 3 0 R >> >> /Contents {} 0 R >>",
            7 + 2 * at
        )));
        objects.push(stream(
            "",
            format!("BT /F1 12 Tf 10 50 Td ({}) Tj ET", part_line(part, at)).as_bytes(),
        ));
    }
    pdf(&objects)
}

/// Four parts' pages imported into a document of none, and saved.
fn merge(parts: &[Document], counting: bool, options: &SaveOptions) -> Saved {
    let empty = pdf(&[
        object("<< /Type /Catalog /Pages 2 0 R >>"),
        object("<< /Type /Pages /Count 0 /Kids [] >>"),
    ]);
    let mut editor = Document::open(empty).expect("the base opens").editor();
    if counting {
        editor.count_work();
    }
    for part in parts {
        for page in 0..part.page_count() {
            let at = editor.page_refs().len() as u32;
            editor
                .import_page(part.cos(), page, at)
                .expect("the page imports");
        }
    }
    assert_eq!(editor.page_refs().len(), 40, "every page imported");
    save(&mut editor, options)
}

/// A merge is pages imported and then saved, and it reads its stats from the
/// save: what was counted over the editor since `count_work` — each import's
/// walk of its source, the editor's walks of its own tree, and the save's
/// passes. A counted merge writes the bytes an uncounted one writes.
#[test]
fn a_merge_reads_its_stats_from_the_save() {
    let parts: Vec<Document> = (0..4)
        .map(|at| Document::open(part(at)).expect("a part opens"))
        .collect();
    let asked = SaveOptions {
        stats: true,
        ..SaveOptions::default()
    };
    let plain = merge(&parts, false, &SaveOptions::default());
    assert_eq!(plain.stats, None);
    for (what, counting) in [("a counting editor", true), ("a save of its own", false)] {
        let counted = merge(&parts, counting, &asked);
        same_save(&counted, &plain, what);
        // The editor reads its page order from its tree, a root of no kids,
        // until the first import sets it: for the loop's question before that
        // import, for the import's own bound check, and when it inserts. Each
        // import walks its source, the root and ten pages.
        let (editor_walks, editor_nodes) = if counting {
            (3 + 40, 3 + 40 * 11)
        } else {
            (0, 0)
        };
        let shown: u64 = (0..4)
            .flat_map(|part| (0..10).map(move |page| part_line(part, page).len() as u64))
            .sum();
        let mut expected = Stats::default();
        // The font pass walks the merged tree once, its root and forty pages;
        // each page's scope reads its one font, and each glyph its font's
        // dictionary; each scope reads the face's program at its first glyph
        // and finds it at the rest.
        expected.page_walks = editor_walks + 1;
        expected.page_nodes_visited = editor_nodes + 41;
        expected.font_dictionaries_resolved = 40 + shown;
        expected.font_program_misses = 40;
        expected.font_program_hits = shown - 40;
        assert_eq!(counted.stats, Some(expected), "{what}");
    }
}

/// The line [`claiming_page`] shows in the embedded face: six glyphs, six
/// distinct codes, each with a width its dictionary states.
const CLAIMING_LINE: &str = "Curvy!";

/// A tagged page of [`CLAIMING_LINE`], under a structure tree with no
/// elements, claiming PDF/A-2a and PDF/UA-1 in its XMP packet, and
/// PDF/X-1a:2003 in `/Info` when `pdfx` is set.
fn claiming_page(pdfx: bool) -> Vec<u8> {
    let packet = r#"<?xpacket begin="" id="W5M0MpCehiHzreSzNTczkc9d"?>
<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF
 xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
<rdf:Description rdf:about="" xmlns:pdfaid="http://www.aiim.org/pdfa/ns/id/"
 xmlns:pdfuaid="http://www.aiim.org/pdfua/ns/id/"
 pdfaid:part="2" pdfaid:conformance="A" pdfuaid:part="1"/>
</rdf:RDF></x:xmpmeta><?xpacket end="w"?>"#;
    let objects = vec![
        object(
            "<< /Type /Catalog /Pages 2 0 R /Metadata 8 0 R /StructTreeRoot 9 0 R\n\
             /MarkInfo << /Marked true >> /Lang (en-GB) >>",
        ),
        object("<< /Type /Pages /Count 1 /Kids [3 0 R] >>"),
        object(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 100]\n\
             /Resources << /Font << /F1 5 0 R >> >> /Contents 4 0 R >>",
        ),
        stream(
            "",
            format!("BT /F1 12 Tf 10 50 Td ({CLAIMING_LINE}) Tj ET").as_bytes(),
        ),
        object(&format!(
            "<< /Type /Font /Subtype /TrueType /BaseFont /Curvy /FirstChar 32 /LastChar 126\n\
             /Widths [320{}] /Encoding /WinAnsiEncoding /FontDescriptor 6 0 R >>",
            " 640".repeat(94)
        )),
        object(
            "<< /Type /FontDescriptor /FontName /Curvy /Flags 32 /FontBBox [0 0 1000 1000]\n\
             /ItalicAngle 0 /Ascent 800 /Descent -200 /CapHeight 700 /StemV 80 /FontFile2 7 0 R >>",
        ),
        stream("", &render_support::curvy_font()),
        stream("/Type /Metadata /Subtype /XML", packet.as_bytes()),
        object("<< /Type /StructTreeRoot >>"),
        object("<< /Title (Counted) /GTS_PDFXVersion (PDF/X-1a:2003) >>"),
    ];
    pdf_trailing(&objects, if pdfx { " /Info 10 0 R" } else { "" })
}

/// What a validation of [`claiming_page`] counts: `walks` walks of the page
/// tree, two nodes each, and, for one that ran PDF/A's width rule, that rule's scope over the
/// page's resources — which reads their one font — and the glyph it selects
/// for each distinct code with a stated width, the first reading the face's
/// program and the rest finding it.
fn walked(walks: u64, widths: bool) -> Stats {
    let mut expected = Stats::default();
    // Each walk reads the root and the one page.
    expected.page_walks = walks;
    expected.page_nodes_visited = 2 * walks;
    if widths {
        let mut codes: Vec<u8> = CLAIMING_LINE.bytes().collect();
        codes.sort_unstable();
        codes.dedup();
        expected.font_dictionaries_resolved = 1;
        expected.font_program_misses = 1;
        expected.font_program_hits = codes.len() as u64 - 1;
    }
    expected
}

/// Each validator rule that reads every page walks the page tree itself, and
/// a validation counts every walk its rules made — the width rule's scope
/// over the page's one font too. The verdict is the uncounted verdict.
#[test]
fn validation_stats_count_its_page_walks() {
    let document = Document::open(claiming_page(true)).expect("the fixture opens");

    // PDF/A-2a. Metadata: the packet header rule reads every page's packet,
    // and part 2's predefined-schema rule reads them again. Structure: level
    // A binds the structure tree, which maps every page. Fonts: the width
    // rule and the embedding rules each walk every content stream. Colour:
    // the scan of every operator, the transparency groups' spaces, and part
    // 2's blending spaces; the page-level output intent is part 4's and the
    // transparency rule part 1's.
    let colour = PdfACoverage {
        colour: true,
        ..PdfACoverage::default()
    };
    let groups = [
        ("syntax", PdfACoverage::SYNTAX, walked(0, false)),
        ("metadata", PdfACoverage::METADATA, walked(2, false)),
        ("structure", PdfACoverage::STRUCTURE, walked(1, false)),
        ("fonts", PdfACoverage::FONTS, walked(2, true)),
        ("colour", colour, walked(3, false)),
        ("every group", PdfACoverage::IMPLEMENTED, walked(8, true)),
    ];
    for (what, coverage, expected) in groups {
        let (verdict, stats) = document.validate_pdfa_counted(coverage);
        assert_eq!(
            verdict,
            document.validate_pdfa_with(coverage),
            "PDF/A {what}"
        );
        assert_eq!(stats, expected, "PDF/A {what}");
    }

    // PDF/UA-1. Structure: binding the tree, and part 1's annotation rule
    // reading every page's annotations. Fonts: PDF/A's width and embedding
    // rules, and the `/ToUnicode` rule's walk of every content stream.
    let groups = [
        ("syntax", PdfUaCoverage::SYNTAX, walked(0, false)),
        ("metadata", PdfUaCoverage::METADATA, walked(0, false)),
        ("structure", PdfUaCoverage::STRUCTURE, walked(2, false)),
        ("fonts", PdfUaCoverage::FONTS, walked(3, true)),
        ("every group", PdfUaCoverage::IMPLEMENTED, walked(5, true)),
    ];
    for (what, coverage, expected) in groups {
        let (verdict, stats) = document.validate_pdfua_counted(coverage);
        assert_eq!(
            verdict,
            document.validate_pdfua_with(coverage),
            "PDF/UA {what}"
        );
        assert_eq!(stats, expected, "PDF/UA {what}");
    }

    // PDF/X-1a:2003. Syntax: the boxes and annotations of every page. Print:
    // PDF/A's operator scan and its transparency rule. Fonts: the embedding
    // rule.
    let groups = [
        ("syntax", PdfXCoverage::SYNTAX, walked(1, false)),
        ("print", PdfXCoverage::PRINT, walked(2, false)),
        ("fonts", PdfXCoverage::FONTS, walked(1, false)),
        ("every group", PdfXCoverage::IMPLEMENTED, walked(4, false)),
    ];
    for (what, coverage, expected) in groups {
        let (verdict, stats) = document.validate_pdfx_counted(coverage);
        assert_eq!(
            verdict,
            document.validate_pdfx_with(coverage),
            "PDF/X {what}"
        );
        assert_eq!(stats, expected, "PDF/X {what}");
    }

    // A file that claims no PDF/X level is not judged, so nothing walks.
    let unclaimed = Document::open(claiming_page(false)).expect("the fixture opens");
    let (_, stats) = unclaimed.validate_pdfx_counted(PdfXCoverage::IMPLEMENTED);
    assert_eq!(stats, Stats::default());
}

/// A page tree `depth` levels of `fan_out` kids under its root, every level
/// full, whose leaves are its `fan_out^depth` pages, one content stream among
/// them all.
fn balanced(depth: u32, fan_out: usize) -> Vec<u8> {
    // 1 the catalog, then the tree breadth first from 2, its root, and the
    // content stream last.
    let mut levels: Vec<Vec<usize>> = vec![vec![2]];
    let mut next = 3;
    for _ in 0..depth {
        let width = levels[levels.len() - 1].len() * fan_out;
        levels.push((next..next + width).collect());
        next += width;
    }
    let content = next;
    let mut objects = vec![object("<< /Type /Catalog /Pages 2 0 R >>")];
    for (level, nodes) in levels.iter().enumerate() {
        for at in 0..nodes.len() {
            let parent = match level {
                0 => String::new(),
                _ => format!(" /Parent {} 0 R", levels[level - 1][at / fan_out]),
            };
            if level as u32 == depth {
                objects.push(object(&format!(
                    "<< /Type /Page{parent} /MediaBox [0 0 200 100] /Resources << >>\n\
                     /Contents {content} 0 R >>"
                )));
            } else {
                let kids: Vec<String> = levels[level + 1][at * fan_out..(at + 1) * fan_out]
                    .iter()
                    .map(|kid| format!("{kid} 0 R"))
                    .collect();
                objects.push(object(&format!(
                    "<< /Type /Pages{parent} /Kids [{}] /Count {} >>",
                    kids.join(" "),
                    fan_out.pow(depth - level as u32)
                )));
            }
        }
    }
    objects.push(stream("", b"0 0 10 10 re f"));
    pdf(&objects)
}

/// The nodes a walk of [`balanced`] reads to gather `wanted` pages: on each
/// level, every node whose subtree starts before the last page wanted, and
/// at most the level's every node — a walk stops before the next sibling and
/// reads it not at all.
fn nodes_to_gather(depth: u32, fan_out: u64, wanted: u64) -> u64 {
    (0..=depth)
        .map(|level| {
            wanted
                .div_ceil(fan_out.pow(depth - level))
                .min(fan_out.pow(level))
        })
        .sum()
}

/// A document of `pages` pages, saved linearized by this engine.
fn linearized(pages: usize) -> Vec<u8> {
    let mut builder = DocumentBuilder::new();
    for _ in 0..pages {
        builder.add_page(200.0, 100.0, |page| {
            page.raw(b"0 0 10 10 re f");
        });
    }
    let base = Document::open(builder.finish()).expect("it opens");
    base.editor().save(&WriteOptions {
        mode: WriteMode::Rewrite,
        linearize: true,
        object_streams: false,
        ..WriteOptions::default()
    })
}

/// How many ancestors page `index` of `document` has along `/Parent`.
fn ancestors(document: &Document, index: u32) -> u64 {
    let cos = document.cos();
    let parent = cos.intern(b"Parent");
    let mut at = tinker_pdf_cos::pages::at(cos, index)
        .expect("a page")
        .reference;
    let mut count = 0;
    while let Some(next) = cos
        .get(at)
        .ok()
        .and_then(|object| object.as_dict().and_then(|dict| dict.get_ref(parent)))
    {
        count += 1;
        at = next;
    }
    count
}

/// `page(i)` reads the nodes today's walk visits to gather `i + 1` pages —
/// a lookup past the last page reads them all and finds nothing — in one walk
/// of the tree; and on a streamed linearized file, the first page and a page
/// the hint tables place are read from the head, each with its ancestors,
/// without a walk.
#[test]
fn page_lookup_counts_the_nodes_it_visits() {
    for (depth, fan_out) in [(1, 5), (2, 10), (3, 4)] {
        let document = Document::open(balanced(depth, fan_out)).expect("the tree opens");
        let pages = (fan_out as u64).pow(depth);
        let fan = fan_out as u64;
        for index in [0, 1, fan - 1, fan, pages - 1, pages] {
            let index = index as u32;
            let (page, stats) = document.page_counted(index);
            assert_eq!(
                page.as_ref().map(Page::index),
                document.page(index).as_ref().map(Page::index),
                "depth {depth}, fan-out {fan_out}: page {index}"
            );
            assert_eq!(
                page.is_some(),
                u64::from(index) < pages,
                "page {index} of {pages}"
            );
            let mut expected = Stats::default();
            expected.page_walks = 1;
            expected.page_nodes_visited = nodes_to_gather(depth, fan, u64::from(index) + 1);
            assert_eq!(
                stats, expected,
                "depth {depth}, fan-out {fan_out}: page {index}"
            );
        }
    }

    let bytes = linearized(5);
    let whole = Document::open(bytes.clone()).expect("it opens");
    let streamed =
        Document::open_streaming(Arc::new(SliceSource::new(bytes))).expect("it opens streamed");
    assert!(
        streamed.first_page_end().is_some(),
        "the linearized path engaged"
    );
    // The page the first-page entry or the hint tables name, and each of its
    // ancestors, which this engine's linearizer places in the head with it.
    for index in [0, 3] {
        let (page, stats) = streamed.page_counted(index);
        assert_eq!(page.map(|page| page.index()), Some(index));
        let mut expected = Stats::default();
        expected.page_nodes_visited = 1 + ancestors(&whole, index);
        assert_eq!(stats, expected, "streamed page {index}");
    }
}
