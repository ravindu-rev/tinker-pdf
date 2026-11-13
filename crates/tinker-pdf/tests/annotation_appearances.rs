//! Annotation appearance streams reach the page correctly (12.5.5, 8.10.2).
//!
//! An appearance stream is a form XObject, and a form XObject is clipped to
//! its `/BBox`. Plenty of real appearance streams paint outside theirs —
//! the box is often the *intended* extent rather than the drawn one — so
//! without the clip an annotation spills across the page over content that
//! has nothing to do with it. It looks like a rendering bug in the page, not
//! in the annotation, which is why it takes so long to find.
//!
//! And an annotation draws only where its flags and its `/OC` say, for the
//! render's usage (12.5.3, `docs/design/render-usage.md`): each case is held
//! to its twin — the page with the annotation dropped where the usage hides
//! it, and with its flags cleared but for `/Print` where it draws — byte for
//! byte, directly and from a display list.

use tinker_pdf::{Document, RenderOptions, RenderWarning, Usage};

fn render(bytes: Vec<u8>) -> tinker_pdf::Bitmap {
    Document::open(bytes)
        .expect("it opens")
        .page(0)
        .expect("a page")
        .render(&RenderOptions::default())
}

fn pixel(bitmap: &tinker_pdf::Bitmap, x: u32, y: u32) -> (u8, u8, u8) {
    let at = (y as usize) * bitmap.stride + (x as usize) * bitmap.components();
    let p = bitmap.data.get(at..at + 3).unwrap_or(&[255, 255, 255]);
    (p[0], p[1], p[2])
}

/// A 40×40 page with one annotation in the lower-left 20×20, whose appearance
/// stream fills a rectangle four times the size of its own bounding box.
fn document(appearance_fill: &str, bbox: &str, rect: &str) -> Vec<u8> {
    let stream = format!("1 0 0 rg {appearance_fill}");
    let page = format!(
        "%PDF-1.7\n\
1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
2 0 obj\n<< /Type /Pages /Count 1 /Kids [3 0 R] >>\nendobj\n\
3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 40 40]\n\
   /Annots [5 0 R] /Contents 4 0 R >>\nendobj\n\
4 0 obj\n<< /Length 0 >>\nstream\n\nendstream\nendobj\n\
5 0 obj\n<< /Type /Annot /Subtype /Square /Rect [{rect}]\n\
   /AP << /N 6 0 R >> >>\nendobj\n\
6 0 obj\n<< /Type /XObject /Subtype /Form /BBox [{bbox}]\n\
   /Length {} >>\nstream\n{stream}\nendstream\nendobj\n\
trailer\n<< /Size 7 /Root 1 0 R >>\n%%EOF\n",
        stream.len()
    );
    page.into_bytes()
}

/// The headline: paint outside the box, and none of it reaches the page.
///
/// The appearance fills 0..40 in form space while its box is 0..20. Fitting
/// maps the *box* onto `/Rect`, so without a clip the fill covers four times
/// the annotation's rectangle — the whole page, here.
#[test]
fn an_appearance_stream_is_clipped_to_its_bounding_box() {
    let bytes = document("0 0 40 40 re f", "0 0 20 20", "0 0 20 20");
    let bitmap = render(bytes);

    // Inside the annotation's rectangle, which is the lower-left quarter.
    // Device space puts y=0 at the top, so the lower-left of the page is the
    // bottom-left of the bitmap.
    let inside = pixel(&bitmap, 10, bitmap.height - 10);
    assert!(
        inside.0 > 200 && inside.1 < 60,
        "the annotation draws inside its rectangle, got {inside:?}"
    );

    // Outside it — the upper-right quarter — must be untouched.
    let outside = pixel(&bitmap, 30, 10);
    assert!(
        outside.0 > 200 && outside.1 > 200 && outside.2 > 200,
        "and nothing spilled outside, got {outside:?}"
    );
}

/// A stream that stays inside its box is unaffected, which is what says the
/// clip is a clip and not a crop of the annotation itself.
#[test]
fn an_appearance_stream_inside_its_box_is_untouched() {
    let bytes = document("0 0 20 20 re f", "0 0 20 20", "0 0 20 20");
    let bitmap = render(bytes);

    for (x, y) in [(2u32, 2u32), (10, 10), (17, 17)] {
        let there = pixel(&bitmap, x, bitmap.height - 1 - y);
        assert!(
            there.0 > 200 && there.1 < 60,
            "({x}, {y}) is inside the box and inside the rectangle: {there:?}"
        );
    }
}

/// The clip is in *form* space, so it has to survive the fitting transform
/// that maps the box onto a rectangle of a different size. Clipping in the
/// wrong space is the subtle version of this bug: the annotation renders,
/// just with the wrong part of it showing.
#[test]
fn the_clip_survives_being_fitted_to_a_different_rectangle() {
    // A 10-unit box scaled up onto a 20-unit rectangle.
    let bytes = document("0 0 40 40 re f", "0 0 10 10", "0 0 20 20");
    let bitmap = render(bytes);

    let inside = pixel(&bitmap, 10, bitmap.height - 10);
    assert!(
        inside.0 > 200 && inside.1 < 60,
        "the annotation still fills its rectangle after scaling, got {inside:?}"
    );

    let outside = pixel(&bitmap, 30, 10);
    assert!(
        outside.0 > 200 && outside.1 > 200 && outside.2 > 200,
        "and still does not spill, got {outside:?}"
    );
}

/// A form with no bounding box has nothing to clip to, and refusing to draw
/// it would lose annotations that render fine everywhere else (ruling 2).
#[test]
fn an_appearance_without_a_box_still_draws() {
    let bytes = b"%PDF-1.7\n\
1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
2 0 obj\n<< /Type /Pages /Count 1 /Kids [3 0 R] >>\nendobj\n\
3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 40 40]\n\
   /Annots [5 0 R] /Contents 4 0 R >>\nendobj\n\
4 0 obj\n<< /Length 0 >>\nstream\n\nendstream\nendobj\n\
5 0 obj\n<< /Type /Annot /Subtype /Square /Rect [0 0 20 20]\n\
   /AP << /N 6 0 R >> >>\nendobj\n\
6 0 obj\n<< /Type /XObject /Subtype /Form /Length 22 >>\n\
stream\n1 0 0 rg 0 0 20 20 re f\nendstream\nendobj\n\
trailer\n<< /Size 7 /Root 1 0 R >>\n%%EOF\n";

    let bitmap = render(bytes.to_vec());
    let there = pixel(&bitmap, 10, bitmap.height - 10);
    assert!(
        there.0 > 200 && there.1 < 60,
        "it draws where its own matrix puts it, got {there:?}"
    );
}

/// A bounding box holding an infinity or a NaN must not be written into a
/// content stream: `NaN` is not a number to a PDF lexer, it is a syntax error
/// that throws away everything after it.
#[test]
fn a_degenerate_bounding_box_does_not_corrupt_the_stream() {
    for bbox in ["0 0 1e9999 20", "0 0 20 20", "-1e9999 0 20 20"] {
        let bytes = document("0 0 20 20 re f", bbox, "0 0 20 20");
        let bitmap = render(bytes);
        assert_eq!(bitmap.data.len() % bitmap.stride, 0, "for /BBox [{bbox}]");
    }
}

// ---- the flags and the usage (RC-13.3) ----------------------------------------

/// A 60 x 40 page with a grey bar of its own and, where `annotation` is
/// some, one annotation over its right half — `annotation` its own entries,
/// `/Subtype`, `/F` and `/OC` among them — whose appearance fills a red
/// square. The catalog lists two groups, `7 0 R` "Notes", which `/D` turns
/// off, and `8 0 R` "Shown", which it leaves on.
fn flagged_page(annotation: Option<&str>) -> Vec<u8> {
    let content = "0.5 g 2 2 20 10 re f";
    let appearance = "1 0 0 rg 0 0 20 20 re f";
    let annots = if annotation.is_some() {
        "/Annots [5 0 R]"
    } else {
        ""
    };
    format!(
        "%PDF-1.7\n\
1 0 obj\n<< /Type /Catalog /Pages 2 0 R /OCProperties << /OCGs [7 0 R 8 0 R] \
/D << /OFF [7 0 R] >> >> >>\nendobj\n\
2 0 obj\n<< /Type /Pages /Count 1 /Kids [3 0 R] >>\nendobj\n\
3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 60 40] {annots} /Contents 4 0 R >>\nendobj\n\
4 0 obj\n<< /Length {} >>\nstream\n{content}\nendstream\nendobj\n\
5 0 obj\n<< /Type /Annot {} /Rect [30 10 50 30] /AP << /N 6 0 R >> >>\nendobj\n\
6 0 obj\n<< /Type /XObject /Subtype /Form /BBox [0 0 20 20] /Length {} >>\n\
stream\n{appearance}\nendstream\nendobj\n\
7 0 obj\n<< /Type /OCG /Name (Notes) >>\nendobj\n\
8 0 obj\n<< /Type /OCG /Name (Shown) >>\nendobj\n\
trailer\n<< /Size 9 /Root 1 0 R >>\n%%EOF\n",
        content.len(),
        annotation.unwrap_or_default(),
        appearance.len(),
    )
    .into_bytes()
}

/// The usages a render can name, and none.
const USAGES: [Option<Usage>; 4] = [
    None,
    Some(Usage::View),
    Some(Usage::Print),
    Some(Usage::Export),
];

/// **Each flag draws as its usage says** (12.5.3 Table 167, as
/// `docs/design/render-usage.md` reads it): `/Hidden`, `/Print`, `/NoView`,
/// `/Print` with `/NoView`, `/Print` clear, `/Invisible` on a known subtype,
/// an unknown one and none, and an `/OC` on and off, each in every usage —
/// no usage, view, print and export — equal to its twin byte for byte: the
/// page with the annotation dropped where the usage hides it, and with its
/// flags cleared but for `/Print` and its `/OC` taken off where it draws;
/// directly and from a display list recorded for that usage, whose warnings
/// are the direct render's. A view, an export and no usage draw alike, a
/// print render only what `/Print` sets, and an annotation its layer hides is
/// reported by the layer's name.
#[test]
fn each_flag_draws_as_its_usage_says() {
    // (what, the annotation's entries, drawn on the screen, drawn in print)
    let cases = [
        ("Hidden", "/Subtype /Square /F 6", false, false),
        ("Print", "/Subtype /Square /F 4", true, true),
        ("NoView", "/Subtype /Square /F 32", false, false),
        ("Print with NoView", "/Subtype /Square /F 36", false, true),
        ("Print clear", "/Subtype /Square", true, false),
        (
            "Invisible on a known subtype",
            "/Subtype /Square /F 5",
            true,
            true,
        ),
        (
            "Invisible on an unknown subtype",
            "/Subtype /Gizmo /F 5",
            false,
            false,
        ),
        ("Invisible with no subtype", "/F 5", false, false),
        ("an /OC on", "/Subtype /Square /F 4 /OC 8 0 R", true, true),
        (
            "an /OC off",
            "/Subtype /Square /F 4 /OC 7 0 R",
            false,
            false,
        ),
    ];
    let open = |bytes: Vec<u8>| Document::open(bytes).expect("it opens");
    let dropped = open(flagged_page(None));
    let plain = open(flagged_page(Some("/Subtype /Square /F 4")));
    let options = |usage| RenderOptions {
        usage,
        ..RenderOptions::default()
    };
    for usage in USAGES {
        let shown = plain.page(0).expect("a page").render(&options(usage));
        let none = dropped.page(0).expect("a page").render(&options(usage));
        assert_ne!(
            shown.data, none.data,
            "{usage:?}: the twins differ by the annotation"
        );
    }
    let notes = RenderWarning::HiddenOptionalContent {
        layer: "Notes".to_string(),
    };
    for (what, entries, screen, print) in cases {
        let document = open(flagged_page(Some(entries)));
        let page = document.page(0).expect("a page");
        for usage in USAGES {
            let options = options(usage);
            let draws = if usage == Some(Usage::Print) {
                print
            } else {
                screen
            };
            let twin_document = if draws { &plain } else { &dropped };
            let twin = twin_document.page(0).expect("a page").render(&options);
            let direct = page.render(&options);
            assert!(
                direct.data == twin.data,
                "{what}, {usage:?}: the page is not its twin, which {} the annotation",
                if draws { "draws" } else { "drops" }
            );
            let listed = page.display_list_for(usage).render(&options);
            assert!(
                listed.data == twin.data,
                "{what}, {usage:?}: the display list is not the twin"
            );
            assert_eq!(listed.warnings, direct.warnings, "{what}, {usage:?}");
            assert_eq!(
                direct.warnings.contains(&notes),
                what == "an /OC off",
                "{what}, {usage:?}: only the hidden layer is reported: {:?}",
                direct.warnings
            );
        }
    }
}
