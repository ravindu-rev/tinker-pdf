//! Appearance synthesis, subtype by subtype, measured in pixels (12.5.5,
//! 12.5.6).
//!
//! Every test here starts from a fixture: a one-page file whose annotation
//! dictionary is written out as PDF text and carries **no `/AP`**. The
//! dictionary is handed to `DocumentEditor::add_annotation`, which is what
//! calls `tinker_pdf_cos::appearance::synthesize`; the file is saved, opened
//! again through the facade, and rendered at one pixel per point. What the
//! page then shows is checked at points 12.5.6 puts inside the shape and at
//! points it puts outside — so a test passes because the appearance is where
//! the dictionary says, not because a stream with some operators in it was
//! written.
//!
//! The page is 100 by 100 points and white. A pixel is read at the point it
//! covers, in default user space, so `(50.5, 20.5)` is the pixel whose centre
//! is half a point right of x = 50 and half a point above y = 20 — the y axis
//! is flipped onto the bitmap here, once.

use std::sync::Arc;

use tinker_pdf::{Bitmap, Document, RenderOptions};
use tinker_pdf_cos::{CosDocument, DocumentEditor, ObjRef, WriteMode, WriteOptions};

/// A one-page file: the catalog (with `catalog_extra` inside it), the page,
/// the annotation as object 4 — on no page's `/Annots`, so it is only drawn
/// once the editor has added it — and `objects` from object 5 on.
fn fixture(annotation: &str, catalog_extra: &str, objects: &[&str]) -> Vec<u8> {
    let mut bodies = vec![
        format!("<< /Type /Catalog /Pages 2 0 R {catalog_extra} >>"),
        "<< /Type /Pages /Count 1 /Kids [3 0 R] >>".to_owned(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] /Resources << >> >>".to_owned(),
        annotation.to_owned(),
    ];
    bodies.extend(objects.iter().map(|o| (*o).to_owned()));

    let mut out = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (index, body) in bodies.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n{body}\nendobj\n", index + 1).as_bytes());
    }
    let xref = out.len();
    out.extend_from_slice(
        format!("xref\n0 {}\n0000000000 65535 f \n", bodies.len() + 1).as_bytes(),
    );
    for offset in offsets {
        out.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            bodies.len() + 1
        )
        .as_bytes(),
    );
    out
}

/// What synthesis made of a fixture: the page as rendered, and the
/// appearance's own content stream.
struct Synthesized {
    bitmap: Bitmap,
    content: String,
}

/// Adds the fixture's annotation through the editor, saves, reopens and
/// renders. Fails if the annotation came in with an `/AP` — the fixture would
/// then be testing nothing — or went out without one.
fn synthesized_in(annotation: &str, catalog_extra: &str, objects: &[&str]) -> Synthesized {
    let doc = Arc::new(
        CosDocument::open(fixture(annotation, catalog_extra, objects)).expect("the fixture opens"),
    );
    let dict = doc
        .get(ObjRef { num: 4, gen: 0 })
        .expect("object 4 is the annotation")
        .as_dict()
        .cloned()
        .expect("and it is a dictionary");
    let ap = doc.intern(b"AP");
    assert!(!dict.contains_key(ap), "the fixture must carry no /AP");

    let mut editor = DocumentEditor::new(Arc::clone(&doc));
    editor
        .add_annotation(0, dict)
        .expect("the annotation is added");
    let saved = editor.save(&WriteOptions {
        mode: WriteMode::Rewrite,
        ..WriteOptions::default()
    });

    let reopened = Document::open(saved).expect("the saved file opens");
    let page = reopened.page(0).expect("the page");
    let annotations = page.annotations();
    assert_eq!(annotations.len(), 1, "the one annotation is on the page");
    assert!(
        annotations[0].has_appearance,
        "and it was given an appearance"
    );
    let content = appearance_content(&reopened);
    Synthesized {
        bitmap: page.render(&RenderOptions::default()),
        content,
    }
}

fn synthesized(annotation: &str) -> Synthesized {
    synthesized_in(annotation, "", &[])
}

/// The decoded content of the page's one annotation's `/AP /N`.
fn appearance_content(doc: &Document) -> String {
    let cos = doc.cos();
    // A rewrite may renumber, so the page is found through the tree rather
    // than by its number in the fixture.
    let page = tinker_pdf_cos::pages::collect(cos)
        .first()
        .and_then(|p| cos.get(p.reference).ok())
        .and_then(|o| o.as_dict().cloned())
        .expect("the page dictionary");
    let annots = cos.resolve_key(&page, cos.intern(b"Annots"));
    let annotation = annots
        .as_array()
        .and_then(|a| a.first())
        .map(|a| cos.resolve(a))
        .expect("an annotation");
    let ap = cos.resolve_key(
        annotation.as_dict().expect("a dictionary"),
        cos.intern(b"AP"),
    );
    let normal = ap
        .as_dict()
        .and_then(|d| d.get_ref(cos.intern(b"N")))
        .expect("a normal appearance by reference");
    let bytes = cos.stream_decoded(normal).expect("the appearance decodes");
    String::from_utf8_lossy(&bytes).into_owned()
}

/// The pixel covering `(x, y)` in default user space.
fn at(bitmap: &Bitmap, x: f64, y: f64) -> [u8; 3] {
    let column = x.floor() as usize;
    let row = (f64::from(bitmap.height) - y).floor() as usize;
    let index = row * bitmap.stride + column * bitmap.components();
    let pixel = bitmap
        .data
        .get(index..index + 3)
        .expect("the point is on the page");
    [pixel[0], pixel[1], pixel[2]]
}

fn is_white(p: [u8; 3]) -> bool {
    p.iter().all(|c| *c > 245)
}

fn is_red(p: [u8; 3]) -> bool {
    p[0] > 220 && p[1] < 40 && p[2] < 40
}

fn is_blue(p: [u8; 3]) -> bool {
    p[2] > 220 && p[0] < 40 && p[1] < 40
}

fn assert_points(bitmap: &Bitmap, what: &str, points: &[(f64, f64)], test: fn([u8; 3]) -> bool) {
    for (x, y) in points {
        let pixel = at(bitmap, *x, *y);
        assert!(test(pixel), "({x}, {y}) should be {what}, got {pixel:?}");
    }
}

// ---------------------------------------------------------------- Line

/// 12.5.6.7: `/L` is the line, `/BS /W` its width, `/C` its colour. Four
/// points wide from (10, 50) to (90, 50), so it covers y 48 to 52 between
/// x 10 and 90, and nothing past either end — butt caps are the default.
#[test]
fn a_line_is_drawn_along_l_at_its_width() {
    let page = synthesized(
        "<< /Type /Annot /Subtype /Line /Rect [0 0 100 100] /L [10 50 90 50] \
         /BS << /W 4 >> /C [1 0 0] /F 4 >>",
    );
    assert_points(
        &page.bitmap,
        "red, on the line",
        &[(10.5, 50.5), (50.5, 48.5), (50.5, 51.5), (89.5, 49.5)],
        is_red,
    );
    assert_points(
        &page.bitmap,
        "white, off the line",
        &[(50.5, 53.5), (50.5, 46.5), (8.5, 50.5), (91.5, 50.5)],
        is_white,
    );
}

/// Table 176's endings: a closed arrow at the first point, pointing away
/// from the line, and a square at the last, both filled with `/IC` and
/// stroked with `/C`. At width 2 an ending reaches six points from its point
/// (`appearance.rs`'s `ending`), so the arrowhead runs from its tip at x 10
/// back to x 22, thirty degrees either side, and the square is x 84 to 96,
/// y 44 to 56.
#[test]
fn a_lines_endings_are_drawn_and_filled_with_the_interior_colour() {
    let page = synthesized(
        "<< /Type /Annot /Subtype /Line /Rect [0 0 100 100] /L [10 50 90 50] \
         /BS << /W 2 >> /C [1 0 0] /IC [0 0 1] /LE [/ClosedArrow /Square] >>",
    );
    // Inside the arrowhead, clear of the line and of the arrowhead's edges.
    assert_points(
        &page.bitmap,
        "blue, inside the arrowhead",
        &[(19.5, 52.5), (19.5, 46.5)],
        is_blue,
    );
    // Inside the square's corners.
    assert_points(
        &page.bitmap,
        "blue, inside the square",
        &[(86.5, 46.5), (93.5, 53.5)],
        is_blue,
    );
    // The arrowhead's tip is the line's first point: nothing before it, and
    // nothing above the arrowhead's edge near the tip.
    assert_points(
        &page.bitmap,
        "white, outside both endings",
        &[(7.5, 50.5), (12.5, 55.5), (98.5, 50.5), (90.5, 58.5)],
        is_white,
    );
    // The square's edge is stroked in the line's colour.
    assert_points(
        &page.bitmap,
        "red, the square's border",
        &[(90.5, 44.5)],
        is_red,
    );
}

/// `/BS /S /D` with `/D [6 4]`: six points on, four off, from the first
/// point.
#[test]
fn a_dashed_line_is_dashed_from_its_first_point() {
    let page = synthesized(
        "<< /Type /Annot /Subtype /Line /Rect [0 0 100 100] /L [10 50 90 50] \
         /BS << /W 4 /S /D /D [6 4] >> /C [1 0 0] >>",
    );
    assert_points(
        &page.bitmap,
        "red, a dash",
        &[(12.5, 50.5), (22.5, 50.5), (32.5, 50.5)],
        is_red,
    );
    assert_points(
        &page.bitmap,
        "white, a gap",
        &[(17.5, 50.5), (27.5, 50.5), (37.5, 50.5)],
        is_white,
    );
}

/// Figure 60's leader lines. `/LL -20` from a line along y = 30 travelling
/// east puts the leaders counter-clockwise of the direction of travel — up —
/// so the line proper is drawn along y = 30 + 4 + 20 = 54 (`/LLO 4` of gap
/// first), and each leader runs from y 34 to 54 + 5 (`/LLE 5`). The points
/// in `/L` are not themselves on anything drawn.
#[test]
fn leader_lines_lift_the_line_proper_off_its_points() {
    let page = synthesized(
        "<< /Type /Annot /Subtype /Line /Rect [0 0 100 100] /L [20 30 80 30] \
         /LL -20 /LLE 5 /LLO 4 /BS << /W 2 >> /C [1 0 0] >>",
    );
    assert_points(
        &page.bitmap,
        "red, the line proper and the leaders",
        &[(50.5, 54.5), (20.5, 40.5), (79.5, 40.5), (20.5, 58.5)],
        is_red,
    );
    assert_points(
        &page.bitmap,
        "white, the points themselves, the offset gap and past the extension",
        &[(50.5, 30.5), (20.5, 32.5), (20.5, 60.5), (50.5, 40.5)],
        is_white,
    );
}

/// 12.5.6.2's `/CA`: a red line at half opacity over white is half red.
#[test]
fn a_lines_opacity_reaches_the_page() {
    let page = synthesized(
        "<< /Type /Annot /Subtype /Line /Rect [0 0 100 100] /L [10 50 90 50] \
         /BS << /W 10 >> /C [1 0 0] /CA 0.5 >>",
    );
    let pixel = at(&page.bitmap, 50.5, 50.5);
    assert!(
        pixel[0] > 245 && (118..=138).contains(&pixel[1]) && (118..=138).contains(&pixel[2]),
        "half red over white, got {pixel:?}"
    );
    assert!(page.content.starts_with("/GS0 gs\n"), "{}", page.content);
}

// ------------------------------------------------------- Square, Circle

/// 12.5.6.8: the rectangle is inscribed in `/Rect`, its border `/BS /W`
/// wide in `/C` and its inside filled with `/IC`. The border is inset by
/// half its width so all of it is inside the rectangle: four wide in
/// `[20 20 80 60]` covers x 20 to 24 and 76 to 80, y 20 to 24 and 56 to 60.
#[test]
fn a_square_is_bordered_inside_its_rect_and_filled() {
    let page = synthesized(
        "<< /Type /Annot /Subtype /Square /Rect [20 20 80 60] /C [1 0 0] /IC [0 0 1] \
         /BS << /W 4 >> >>",
    );
    assert_points(
        &page.bitmap,
        "blue, inside",
        &[(50.5, 40.5), (25.5, 25.5), (74.5, 54.5)],
        is_blue,
    );
    assert_points(
        &page.bitmap,
        "red, on the border",
        &[(21.5, 40.5), (78.5, 40.5), (50.5, 21.5), (50.5, 58.5)],
        is_red,
    );
    assert_points(
        &page.bitmap,
        "white, outside the rectangle",
        &[(18.5, 40.5), (81.5, 40.5), (50.5, 18.5), (50.5, 61.5)],
        is_white,
    );
}

/// Table 177's `/RD` — left, top, right, bottom — moves the shape inside
/// `/Rect`: `[10 5 10 5]` in `[20 20 80 60]` is x 30 to 70, y 25 to 55,
/// less half the default border of one, which with no `/C` is not stroked.
#[test]
fn a_squares_rect_differences_move_it_inside_its_rect() {
    let page = synthesized(
        "<< /Type /Annot /Subtype /Square /Rect [20 20 80 60] /IC [0 0 1] /RD [10 5 10 5] >>",
    );
    assert_points(
        &page.bitmap,
        "blue, inside the differences",
        &[(31.5, 40.5), (68.5, 53.5), (50.5, 26.5)],
        is_blue,
    );
    assert_points(
        &page.bitmap,
        "white, between /Rect and the shape",
        &[
            (29.5, 40.5),
            (70.5, 40.5),
            (50.5, 55.5),
            (50.5, 24.5),
            (21.5, 21.5),
        ],
        is_white,
    );
}

/// A dashed border starts where `re` starts, the lower-left corner, and
/// runs along the bottom: on from x 21 to 25, off to 29, on to 33.
#[test]
fn a_squares_border_is_dashed_from_its_lower_left_corner() {
    let page = synthesized(
        "<< /Type /Annot /Subtype /Square /Rect [20 20 80 60] /C [1 0 0] \
         /BS << /W 2 /S /D /D [4 4] >> >>",
    );
    assert_points(
        &page.bitmap,
        "red, a dash",
        &[(23.5, 20.5), (31.5, 21.5), (39.5, 20.5)],
        is_red,
    );
    assert_points(
        &page.bitmap,
        "white, a gap",
        &[(27.5, 20.5), (35.5, 21.5), (43.5, 20.5)],
        is_white,
    );
}

/// The ellipse is inscribed in `/Rect`, so the corners of the rectangle are
/// outside it: a circle in `[20 20 80 80]` at width 2 has its stroke
/// between radius 28 and 30 of (50, 50).
#[test]
fn a_circle_is_inscribed_in_its_rect() {
    let page = synthesized(
        "<< /Type /Annot /Subtype /Circle /Rect [20 20 80 80] /C [1 0 0] /IC [0 0 1] \
         /BS << /W 2 >> >>",
    );
    assert_points(
        &page.bitmap,
        "blue, inside",
        &[(50.5, 50.5), (30.5, 50.5), (50.5, 75.5)],
        is_blue,
    );
    assert_points(
        &page.bitmap,
        "red, on the ellipse",
        &[(50.5, 20.5), (79.5, 50.5), (20.5, 50.5), (50.5, 79.5)],
        is_red,
    );
    assert_points(
        &page.bitmap,
        "white, the rectangle's corners",
        &[(22.5, 22.5), (77.5, 77.5), (22.5, 77.5), (77.5, 22.5)],
        is_white,
    );
}

/// `/CA` alone governs the fill too, as ISO 32000-1 has it: half blue.
#[test]
fn a_circles_opacity_reaches_its_fill() {
    let page =
        synthesized("<< /Type /Annot /Subtype /Circle /Rect [20 20 80 80] /IC [0 0 1] /CA 0.5 >>");
    let pixel = at(&page.bitmap, 50.5, 50.5);
    assert!(
        pixel[2] > 245 && (118..=138).contains(&pixel[0]) && (118..=138).contains(&pixel[1]),
        "half blue over white, got {pixel:?}"
    );
}

// ---------------------------------------------------- Polygon, PolyLine

/// 12.5.6.9: a polygon's vertices are joined and closed — the edge from the
/// last vertex back to the first is drawn — and its inside is filled with
/// `/IC`. The triangle (20, 20), (80, 20), (50, 80), at width 2.
#[test]
fn a_polygon_is_closed_and_filled() {
    let page = synthesized(
        "<< /Type /Annot /Subtype /Polygon /Rect [0 0 100 100] \
         /Vertices [20 20 80 20 50 80] /C [1 0 0] /IC [0 0 1] /BS << /W 2 >> >>",
    );
    assert_points(
        &page.bitmap,
        "blue, inside",
        &[(50.5, 40.5), (35.5, 25.5), (50.5, 70.5)],
        is_blue,
    );
    // The base, the right edge, and the closing edge from (50, 80) back to
    // (20, 20), which crosses y 50.5 at x 35.25.
    assert_points(
        &page.bitmap,
        "red, on the edges",
        &[(50.5, 20.5), (64.5, 50.5), (35.5, 50.5)],
        is_red,
    );
    assert_points(
        &page.bitmap,
        "white, outside the triangle",
        &[(25.5, 70.5), (75.5, 70.5), (50.5, 17.5), (50.5, 84.5)],
        is_white,
    );
}

/// A polyline is not closed and not filled — its `/IC` fills only its
/// endings. (20, 20) east to (80, 20), then north to (80, 70); a square
/// ending at the first vertex and a closed arrow at the last, each six
/// points from its vertex at width 2, the arrow pointing on along the last
/// segment and drawn over it.
#[test]
fn a_polyline_is_open_and_its_endings_face_along_its_end_segments() {
    let page = synthesized(
        "<< /Type /Annot /Subtype /PolyLine /Rect [0 0 100 100] \
         /Vertices [20 20 80 20 80 70] /C [1 0 0] /IC [0 0 1] /BS << /W 2 >> \
         /LE [/Square /ClosedArrow] >>",
    );
    assert_points(
        &page.bitmap,
        "red, along the path and the square's border",
        &[(50.5, 20.5), (79.5, 40.5), (14.5, 20.5)],
        is_red,
    );
    assert_points(
        &page.bitmap,
        "white, where a closing edge or a fill would be",
        &[(50.5, 45.5), (60.5, 30.5), (8.5, 20.5), (80.5, 73.5)],
        is_white,
    );
    assert_points(
        &page.bitmap,
        "blue, inside the square and the arrowhead",
        &[
            (16.5, 23.5),
            (23.5, 16.5),
            (77.5, 61.5),
            (82.5, 61.5),
            (80.5, 62.5),
        ],
        is_blue,
    );
}
