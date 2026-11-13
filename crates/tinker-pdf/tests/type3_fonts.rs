//! Type 3 glyphs are content streams, and they run (9.6.5).
//!
//! Every other font hands back an outline. A Type 3 glyph is a *procedure*: it
//! can fill, stroke, draw an image, do anything a page can. There was no path
//! for that at all, so a Type 3 font drew nothing — and, unlike a missing font
//! program, it did not even report `UnreadableFont`, because nothing was
//! missing. The document was complete and the glyphs were simply skipped.
//!
//! What a glyph stands for is read as well: once its procedure has run, the
//! interpreter reports the glyph itself to the text device
//! (`Device::show_type3_glyph`), so a Type 3 run's `/ToUnicode` text is
//! extracted and found where its glyphs stand, and nothing it draws changes.

use tinker_pdf::{Document, RenderOptions, SvgOptions};

/// A one-page document with one Type 3 font, whose glyph `a` runs `proc`.
///
/// `matrix` is the `/FontMatrix`; a Type 3 font picks its own glyph-space
/// units, which is why the entry is required rather than assumed to be 1/1000.
fn document(proc_bytes: &str, matrix: &str, content: &str) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"%PDF-1.7\n");
    bytes.extend_from_slice(b"1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n");
    bytes.extend_from_slice(b"2 0 obj\n<< /Type /Pages /Count 1 /Kids [3 0 R] >>\nendobj\n");
    bytes.extend_from_slice(
        b"3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100]\n\
          /Resources << /Font << /F0 4 0 R >> >> /Contents 7 0 R >>\nendobj\n",
    );
    bytes.extend_from_slice(
        format!(
            "4 0 obj\n<< /Type /Font /Subtype /Type3 /FontBBox [0 0 1000 1000]\n\
             /FontMatrix {matrix}\n\
             /CharProcs << /a 5 0 R >>\n\
             /Encoding << /Type /Encoding /Differences [97 /a] >>\n\
             /FirstChar 97 /LastChar 97 /Widths [1000] /Resources << >> >>\nendobj\n"
        )
        .as_bytes(),
    );
    bytes.extend_from_slice(
        format!(
            "5 0 obj\n<< /Length {} >>\nstream\n{proc_bytes}\nendstream\nendobj\n",
            proc_bytes.len() + 1
        )
        .as_bytes(),
    );
    bytes.extend_from_slice(
        format!(
            "7 0 obj\n<< /Length {} >>\nstream\n{content}\nendstream\nendobj\n",
            content.len() + 1
        )
        .as_bytes(),
    );
    bytes.extend_from_slice(b"trailer\n<< /Size 8 /Root 1 0 R >>\n%%EOF\n");
    bytes
}

fn render(bytes: Vec<u8>) -> tinker_pdf::Bitmap {
    Document::open(bytes)
        .expect("it opens")
        .page(0)
        .expect("a page")
        .render(&RenderOptions::default())
}

fn inked(bitmap: &tinker_pdf::Bitmap) -> usize {
    bitmap
        .data
        .chunks_exact(bitmap.components())
        .filter(|p| p[0] < 200)
        .count()
}

fn pixel(bitmap: &tinker_pdf::Bitmap, x: u32, y: u32) -> (u8, u8, u8) {
    let at = (y as usize) * bitmap.stride + (x as usize) * bitmap.components();
    let p = bitmap.data.get(at..at + 3).unwrap_or(&[255, 255, 255]);
    (p[0], p[1], p[2])
}

/// The headline: a glyph whose procedure fills a box actually fills one.
#[test]
fn a_glyph_procedure_runs() {
    // `d0` declares the width; the box is the whole glyph square.
    let bitmap = render(document(
        "1000 0 d0 0 0 1000 1000 re f",
        "[0.001 0 0 0.001 0 0]",
        "BT /F0 50 Tf 10 20 Td (a) Tj ET",
    ));

    assert!(
        inked(&bitmap) > 500,
        "the procedure drew nothing: {} inked pixels",
        inked(&bitmap)
    );
}

/// The `/FontMatrix` maps glyph space to text space, so halving it halves the
/// glyph. Ignoring it would draw both at the same size — or, at 1/1000
/// assumed, draw the second one a thousand times too large.
#[test]
fn the_font_matrix_scales_the_glyph() {
    let big = render(document(
        "1000 0 d0 0 0 1000 1000 re f",
        "[0.001 0 0 0.001 0 0]",
        "BT /F0 50 Tf 10 20 Td (a) Tj ET",
    ));
    let small = render(document(
        "1000 0 d0 0 0 1000 1000 re f",
        "[0.0005 0 0 0.0005 0 0]",
        "BT /F0 50 Tf 10 20 Td (a) Tj ET",
    ));

    let (big, small) = (inked(&big), inked(&small));
    let ratio = big as f64 / small.max(1) as f64;
    assert!(
        (3.0..5.0).contains(&ratio),
        "half the matrix is a quarter the area: {big} against {small}"
    );
}

/// The glyph lands where the text is, not at the page origin.
#[test]
fn a_glyph_lands_at_the_text_position() {
    let bitmap = render(document(
        "1000 0 d0 0 0 1000 1000 re f",
        "[0.001 0 0 0.001 0 0]",
        "BT /F0 20 Tf 60 70 Td (a) Tj ET",
    ));

    // Text at (60, 70) with a 20-unit em: the glyph occupies roughly
    // x 60..80, y 70..90 in user space — the upper right, and the raster
    // counts y downward.
    let inside = pixel(&bitmap, 70, 100 - 80);
    assert!(
        inside.0 < 200,
        "the glyph is up and to the right: {inside:?}"
    );

    let origin = pixel(&bitmap, 5, 95);
    assert!(
        origin.0 > 200,
        "and nothing was drawn at the page origin: {origin:?}"
    );
}

/// A procedure may use the whole graphics language, including colour — this
/// is the point of Type 3 and what separates it from an outline font.
#[test]
fn a_procedure_may_set_its_own_colour() {
    let bitmap = render(document(
        "1000 0 d0 1 0 0 rg 0 0 1000 1000 re f",
        "[0.001 0 0 0.001 0 0]",
        "BT /F0 50 Tf 10 20 Td (a) Tj ET",
    ));

    let (r, g, b) = pixel(&bitmap, 30, 55);
    assert!(
        r > 200 && g < 60 && b < 60,
        "the glyph painted its own red, got ({r}, {g}, {b})"
    );
}

/// A procedure must not leak its state onto the page. `q`/`Q` inside a glyph
/// is the caller's business, but the glyph's own transform and colour are not.
#[test]
fn a_procedure_does_not_leak_its_state() {
    let bitmap = render(document(
        "1000 0 d0 1 0 0 rg 0 0 1000 1000 re f",
        "[0.001 0 0 0.001 0 0]",
        "BT /F0 20 Tf 10 70 Td (a) Tj ET 0 0 10 10 re f",
    ));

    // The rectangle after `ET` is drawn in the page's colour, which is still
    // black — the glyph's red must not have escaped.
    let (r, g, b) = pixel(&bitmap, 5, 95);
    assert!(
        r < 60 && g < 60 && b < 60,
        "the page kept its own colour, got ({r}, {g}, {b})"
    );
}

/// Two glyphs in a row advance by `/Widths` through the font matrix, not by
/// the 1/1000 every other font uses.
#[test]
fn glyphs_advance_through_the_font_matrix() {
    let one = render(document(
        "1000 0 d0 0 0 400 1000 re f",
        "[0.001 0 0 0.001 0 0]",
        "BT /F0 20 Tf 10 40 Td (a) Tj ET",
    ));
    let two = render(document(
        "1000 0 d0 0 0 400 1000 re f",
        "[0.001 0 0 0.001 0 0]",
        "BT /F0 20 Tf 10 40 Td (aa) Tj ET",
    ));

    let (one, two) = (inked(&one), inked(&two));
    assert!(
        two > one + one / 2,
        "the second glyph landed beside the first rather than on it: \
         {two} against {one}"
    );
}

/// A code with no entry in `/Differences` names no procedure, so nothing is
/// drawn and nothing panics.
#[test]
fn an_unmapped_code_draws_nothing() {
    let bitmap = render(document(
        "1000 0 d0 0 0 1000 1000 re f",
        "[0.001 0 0 0.001 0 0]",
        "BT /F0 50 Tf 10 20 Td (b) Tj ET",
    ));
    assert_eq!(inked(&bitmap), 0);
}

/// A glyph whose procedure shows the same glyph has no last run (9.6.5):
/// showing itself twice, it was two to the sixteenth runs inside the depth
/// cap. The procedure's own identity says it is already running, so it draws
/// once, and the render says the repeat was refused.
#[test]
fn a_glyph_that_shows_itself_draws_once_and_says_so() {
    // The procedure fills its glyph's lower half, then shows itself twice at
    // the size of the whole glyph, over the same square and the next.
    let bitmap = render(document(
        "1000 0 d0 0 0 1000 500 re f BT /F0 1000 Tf (aa) Tj ET",
        "[0.001 0 0 0.001 0 0]",
        "BT /F0 50 Tf 10 20 Td (a) Tj ET",
    ));
    assert!(
        bitmap
            .warnings
            .contains(&tinker_pdf::RenderWarning::RecursiveContent),
        "{:?}",
        bitmap.warnings
    );
    // The lower half of a 50-point glyph, at the page's default scale.
    assert!(
        inked(&bitmap) > 500,
        "the glyph's own fill drew: {}",
        inked(&bitmap)
    );
}

/// `/ToUnicode` for [`mapped_document`]'s font: `a` stands for `H` and `b`
/// for `i`, which its `/Differences` names would read as `a` and `b`.
const TO_UNICODE: &str = "/CIDInit /ProcSet findresource begin\n\
12 dict begin\n\
begincmap\n\
/CIDSystemInfo << /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def\n\
/CMapName /Adobe-Identity-UCS def\n\
/CMapType 2 def\n\
1 begincodespacerange\n\
<00> <FF>\n\
endcodespacerange\n\
2 beginbfchar\n\
<61> <0048>\n\
<62> <0069>\n\
endbfchar\n\
endcmap\n\
CMapName currentdict /CMap defineresource pop\n\
end\n\
end";

/// A one-page document whose Type 3 font `/F0` has glyphs `a` and `b`, a full
/// em and half an em wide, running `a_proc` and `b_proc`, with
/// [`TO_UNICODE`] where `mapped` says so; `/F1` is an unembedded Helvetica,
/// which the font's procedures may show text in, and `/Fm` a form that shows
/// `(ab)` in `/F0`.
fn mapped_document(a_proc: &str, b_proc: &str, mapped: bool, content: &str) -> Vec<u8> {
    let stream = |number: u32, dict: &str, body: &str| {
        format!(
            "{number} 0 obj\n<< {dict} /Length {} >>\nstream\n{body}\nendstream\nendobj\n",
            body.len() + 1
        )
    };
    let to_unicode = if mapped { "/ToUnicode 8 0 R" } else { "" };
    let mut out = String::from("%PDF-1.7\n");
    out.push_str("1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n");
    out.push_str("2 0 obj\n<< /Type /Pages /Count 1 /Kids [3 0 R] >>\nendobj\n");
    out.push_str(
        "3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100]\n\
         /Resources << /Font << /F0 4 0 R /F1 9 0 R >> /XObject << /Fm 10 0 R >> >>\n\
         /Contents 7 0 R >>\nendobj\n",
    );
    out.push_str(&format!(
        "4 0 obj\n<< /Type /Font /Subtype /Type3 /FontBBox [0 0 1000 1000]\n\
         /FontMatrix [0.001 0 0 0.001 0 0]\n\
         /CharProcs << /a 5 0 R /b 6 0 R >>\n\
         /Encoding << /Type /Encoding /Differences [97 /a /b] >>\n\
         /FirstChar 97 /LastChar 98 /Widths [1000 500] {to_unicode}\n\
         /Resources << /Font << /F1 9 0 R >> >> >>\nendobj\n"
    ));
    out.push_str(&stream(5, "", a_proc));
    out.push_str(&stream(6, "", b_proc));
    out.push_str(&stream(7, "", content));
    out.push_str(&stream(8, "", TO_UNICODE));
    out.push_str(
        "9 0 obj\n<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica \
         /Encoding /WinAnsiEncoding >>\nendobj\n",
    );
    out.push_str(&stream(
        10,
        "/Type /XObject /Subtype /Form /BBox [0 0 100 100] \
         /Resources << /Font << /F0 4 0 R >> >>",
        "BT /F0 10 Tf 10 10 Td (ab) Tj ET",
    ));
    out.push_str("trailer\n<< /Size 11 /Root 1 0 R >>\n%%EOF\n");
    out.into_bytes()
}

/// A procedure that fills its glyph's em square.
const SQUARE: &str = "1000 0 d0 0 0 1000 1000 re f";

fn page_of(bytes: Vec<u8>) -> tinker_pdf::Page {
    Document::open(bytes)
        .expect("it opens")
        .page(0)
        .expect("a page")
}

/// **FT-06.2's defect, fixed.** A Type 3 run's `/ToUnicode` text is extracted
/// where its glyphs stand — `a` a full em of a twenty-unit size from the
/// text's origin, `b` half an em after it — and `search` finds it, though the
/// interpreter runs each glyph's procedure rather than showing an outline; and
/// nothing about it is reported, since nothing was missing.
#[test]
fn a_glyph_procedures_text_extracts_and_is_found() {
    let page = page_of(mapped_document(
        SQUARE,
        SQUARE,
        true,
        "BT /F0 20 Tf 10 50 Td (ab) Tj ET",
    ));
    let text = page.text();
    assert_eq!(text.plain_text().trim(), "Hi");
    let lines = text.lines();
    assert_eq!(lines.len(), 1, "{:?}", text.plain_text());
    let chars = &lines[0].chars;
    assert_eq!(chars.len(), 2);
    let near = |a: (f64, f64), b: (f64, f64)| (a.0 - b.0).abs() < 1e-6 && (a.1 - b.1).abs() < 1e-6;
    assert!(near(chars[0].origin, (10.0, 50.0)), "{:?}", chars[0].origin);
    assert!(near(chars[1].origin, (30.0, 50.0)), "{:?}", chars[1].origin);

    let found = text.search("hi");
    assert_eq!(found.len(), 1, "the run is found: {found:?}");
    let (x0, _, x1, _) = found[0].bounds();
    assert!(
        (x0 - 10.0).abs() < 1e-6 && (x1 - 40.0).abs() < 1e-6,
        "over both glyphs: {x0} to {x1}"
    );
    assert!(text.warnings.is_empty(), "{:?}", text.warnings);

    let bitmap = page.render(&RenderOptions::default());
    assert!(inked(&bitmap) > 0, "the procedures drew");
    assert!(
        !bitmap
            .warnings
            .contains(&tinker_pdf::RenderWarning::UnreadableFont),
        "{:?}",
        bitmap.warnings
    );
}

/// A procedure that draws its glyph with text of its own has that text
/// extracted, and the glyph's `/ToUnicode` entry — the same `H` — is not read
/// again on top of it.
#[test]
fn a_procedure_that_shows_text_extracts_it_once() {
    let page = page_of(mapped_document(
        "1000 0 d0 BT /F1 1000 Tf (H) Tj ET",
        SQUARE,
        true,
        "BT /F0 20 Tf 10 50 Td (a) Tj ET",
    ));
    let text = page.text().plain_text();
    assert_eq!(text.trim(), "H", "{text:?}");
    assert_eq!(text.matches('H').count(), 1, "{text:?}");
}

/// The text a Type 3 glyph stands for is text and nothing else: the same page
/// with and without its `/ToUnicode` renders to the same bytes, writes the same
/// SVG and records the same display list, and only what it extracts differs —
/// the stated `Hi`, or the `/Differences` names' `ab`.
#[test]
fn a_glyph_procedures_text_changes_nothing_it_draws() {
    let content = "0.2 0.2 0.6 rg BT /F0 20 Tf 10 50 Td (ab) Tj ET";
    let stated = page_of(mapped_document(SQUARE, SQUARE, true, content));
    let named = page_of(mapped_document(SQUARE, SQUARE, false, content));

    let options = RenderOptions::default();
    let (one, two) = (stated.render(&options), named.render(&options));
    assert_eq!(one.data, two.data, "the same pixels");
    assert_eq!(one.warnings, two.warnings);
    assert_eq!(
        stated.to_svg(&SvgOptions::default()).markup,
        named.to_svg(&SvgOptions::default()).markup,
        "the same SVG"
    );
    let (one, two) = (stated.display_list(), named.display_list());
    assert_eq!(one.len(), two.len(), "the same recording");
    assert_eq!(one.render(&options).data, two.render(&options).data);

    assert_eq!(stated.text().plain_text().trim(), "Hi");
    assert_eq!(named.text().plain_text().trim(), "ab");
}

/// Invisible text is text (9.3.6's `Tr 3`), a Type 3 run's as much as any:
/// an OCR layer set in a Type 3 font extracts.
#[test]
fn an_invisible_type3_run_extracts() {
    let page = page_of(mapped_document(
        SQUARE,
        SQUARE,
        true,
        "BT 3 Tr /F0 20 Tf 10 50 Td (ab) Tj ET",
    ));
    assert_eq!(page.text().plain_text().trim(), "Hi");
}

/// A form showing a Type 3 run extracts it at each of its draws, as a form's
/// ordinary text does.
#[test]
fn a_type3_run_in_a_form_extracts_at_each_draw() {
    let page = page_of(mapped_document(
        SQUARE,
        SQUARE,
        true,
        "/Fm Do q 1 0 0 1 0 40 cm /Fm Do Q",
    ));
    let text = page.text().plain_text();
    assert_eq!(text.matches("Hi").count(), 2, "{text:?}");
}
