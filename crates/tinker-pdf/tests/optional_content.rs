//! Optional content, from the catalog to the pixels (8.11).
//!
//! Everything here is built from bytes rather than from `testdata/`, whose
//! four files are MuPDF's and are not to be modified — and none of which has
//! an `/OCProperties` anyway.
//!
//! Every fixture draws something *outside* the layer it is about. "The page
//! is white" is what a render that failed outright also produces, and a suite
//! that hides content correctly for the wrong reason is exactly the failure
//! this gap exists to prevent.

use tinker_pdf::{Bitmap, Document, RenderOptions, RenderWarning};

/// A 40x40 page with one optional content group at 5 0 R, called
/// "Construction lines".
///
/// `catalog` supplies the `/OCProperties`, `resources` the page's resource
/// dictionary body, `content` the content stream, and `objects` anything
/// further, numbered from 6.
fn build(catalog: &str, resources: &str, content: &str, objects: &str) -> Vec<u8> {
    format!(
        "%PDF-1.7\n\
1 0 obj\n<< /Type /Catalog /Pages 2 0 R {catalog} >>\nendobj\n\
2 0 obj\n<< /Type /Pages /Count 1 /Kids [3 0 R] >>\nendobj\n\
3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 40 40]\n\
   /Resources << {resources} >> /Contents 4 0 R >>\nendobj\n\
4 0 obj\n<< /Length {} >>\nstream\n{content}\nendstream\nendobj\n\
5 0 obj\n<< /Type /OCG /Name (Construction lines) >>\nendobj\n\
{objects}\
trailer\n<< /Size 20 /Root 1 0 R >>\n%%EOF\n",
        content.len()
    )
    .into_bytes()
}

/// `/OCProperties` with the one group off, or on.
fn properties(on: bool) -> String {
    let state = if on { "ON" } else { "OFF" };
    format!("/OCProperties << /OCGs [5 0 R] /D << /{state} [5 0 R] >> >>")
}

fn render(bytes: Vec<u8>) -> Bitmap {
    Document::open(bytes)
        .expect("it opens")
        .page(0)
        .expect("a page")
        .render(&RenderOptions::default())
}

fn pixel(bitmap: &Bitmap, x: u32, y: u32) -> Vec<u8> {
    let n = bitmap.components();
    let at = y as usize * bitmap.stride + x as usize * n;
    bitmap.data.get(at..at + n).unwrap_or(&[]).to_vec()
}

/// The smallest rectangle containing every pixel that is not the white the
/// page started as, as `(x0, y0, x1, y1)` inclusive.
///
/// A count of inked pixels says *how much* drew; this says **where**, which
/// is the only way to tell "the right thing was hidden" from "something was".
fn ink_bounds(bitmap: &Bitmap) -> Option<(u32, u32, u32, u32)> {
    let n = bitmap.components();
    let mut bounds: Option<(u32, u32, u32, u32)> = None;
    for y in 0..bitmap.height {
        for x in 0..bitmap.width {
            let at = y as usize * bitmap.stride + x as usize * n;
            let Some(px) = bitmap.data.get(at..at + n) else {
                continue;
            };
            if px.iter().all(|value| *value == 255) {
                continue;
            }
            bounds = Some(match bounds {
                None => (x, y, x, y),
                Some((x0, y0, x1, y1)) => (x0.min(x), y0.min(y), x1.max(x), y1.max(y)),
            });
        }
    }
    bounds
}

fn hid_construction_lines(bitmap: &Bitmap) -> bool {
    bitmap
        .warnings
        .contains(&RenderWarning::HiddenOptionalContent {
            layer: "Construction lines".to_string(),
        })
}

/// A red rectangle on the right, a green one on the left, and the red one
/// marked as belonging to the group.
const TWO_RECTANGLES: &str = "0 1 0 rg 0 0 10 40 re f\n\
                              /OC /MC0 BDC 1 0 0 rg 20 0 10 40 re f EMC";

/// Milestone 3's exit criterion, both halves.
///
/// The same content stream, the same resources, one byte of difference in the
/// catalog: `/OFF` renders white where the rectangle is, `/ON` renders red.
/// The green rectangle is in neither layer and paints in both, so a build that
/// stopped rendering altogether fails rather than passing the first half.
#[test]
fn a_rectangle_in_an_off_layer_renders_white_and_in_an_on_layer_renders_red() {
    let off = render(build(
        &properties(false),
        "/Properties << /MC0 5 0 R >>",
        TWO_RECTANGLES,
        "",
    ));
    assert_eq!(
        pixel(&off, 5, 20),
        vec![0, 255, 0],
        "the unmarked rectangle"
    );
    assert_eq!(
        pixel(&off, 25, 20),
        vec![255, 255, 255],
        "and the one in the /OFF layer is not drawn"
    );
    assert_eq!(
        ink_bounds(&off),
        Some((0, 0, 9, 39)),
        "the ink stops at the left rectangle's right edge"
    );
    assert!(hid_construction_lines(&off), "{:?}", off.warnings);

    let on = render(build(
        &properties(true),
        "/Properties << /MC0 5 0 R >>",
        TWO_RECTANGLES,
        "",
    ));
    assert_eq!(pixel(&on, 25, 20), vec![255, 0, 0], "/ON paints it red");
    assert_eq!(
        ink_bounds(&on),
        Some((0, 0, 29, 39)),
        "and the ink now reaches the second rectangle"
    );
    assert!(
        on.warnings.is_empty(),
        "a layer that is on is not a leniency: {:?}",
        on.warnings
    );
}

/// A document with no `/OCProperties` draws its marked content, because a
/// `BDC` that names a group the catalog never declared is not a hidden layer.
#[test]
fn marked_content_without_an_ocproperties_still_paints() {
    let bitmap = render(build(
        "",
        "/Properties << /MC0 5 0 R >>",
        TWO_RECTANGLES,
        "",
    ));
    assert_eq!(pixel(&bitmap, 25, 20), vec![255, 0, 0]);
    assert!(bitmap.warnings.is_empty());
}

/// A `/Properties` sub-dictionary that does not define the name, and a page
/// with no `/Properties` at all. Both paint.
#[test]
fn a_property_name_that_resolves_to_nothing_paints() {
    for resources in ["/Properties << /Other 5 0 R >>", ""] {
        let bitmap = render(build(&properties(false), resources, TWO_RECTANGLES, ""));
        assert_eq!(
            pixel(&bitmap, 25, 20),
            vec![255, 0, 0],
            "with resources {resources:?}"
        );
    }
}

/// The membership policies, through the renderer rather than through the
/// binder, so the whole path is exercised on pixels at least once.
///
/// Two groups: 5 0 R on, 6 0 R off. `/AnyOff` is the row that separates the
/// four — it is visible because *one* group is off, which is the reading a
/// naive implementation inverts.
#[test]
fn the_membership_policies_reach_the_page() {
    for (policy, visible) in [
        ("AnyOn", true),
        ("AllOn", false),
        ("AnyOff", true),
        ("AllOff", false),
    ] {
        let bitmap = render(build(
            "/OCProperties << /OCGs [5 0 R 6 0 R] /D << /OFF [6 0 R] >> >>",
            "/Properties << /MC0 7 0 R >>",
            TWO_RECTANGLES,
            &format!(
                "6 0 obj\n<< /Type /OCG /Name (Second) >>\nendobj\n\
                 7 0 obj\n<< /Type /OCMD /OCGs [5 0 R 6 0 R] /P /{policy} >>\nendobj\n"
            ),
        ));

        let expected = if visible {
            vec![255, 0, 0]
        } else {
            vec![255, 255, 255]
        };
        assert_eq!(pixel(&bitmap, 25, 20), expected, "/P /{policy}");
        assert_eq!(
            pixel(&bitmap, 5, 20),
            vec![0, 255, 0],
            "/P /{policy}: the unmarked rectangle always paints"
        );
    }
}

/// The risk this gap turns on: everything not understood renders.
///
/// Each entry is a membership dictionary this build cannot read, over two
/// groups that are both **off** — so the `/AnyOn` default would hide, and a
/// build that fell back to it rather than to visible fails every row.
#[test]
fn nothing_this_build_cannot_read_is_ever_hidden() {
    for (name, ocmd) in [
        ("an unknown /P", "/OCGs [5 0 R 6 0 R] /P /SomeFuturePolicy"),
        ("a malformed /VE", "/VE [/Xor 5 0 R 6 0 R]"),
        ("a /Not with two operands", "/VE [/Not 5 0 R 6 0 R]"),
        ("a /VE that is not an array", "/VE /Not"),
        (
            "a group the catalog never listed",
            "/OCGs [5 0 R 9 0 R] /P /AllOn",
        ),
        ("an empty /OCGs", "/OCGs [] /P /AllOff"),
        ("no /OCGs at all", "/P /AnyOn"),
    ] {
        let bitmap = render(build(
            "/OCProperties << /OCGs [5 0 R 6 0 R] /D << /OFF [5 0 R 6 0 R] >> >>",
            "/Properties << /MC0 7 0 R >>",
            TWO_RECTANGLES,
            &format!(
                "6 0 obj\n<< /Type /OCG /Name (Second) >>\nendobj\n\
                 7 0 obj\n<< /Type /OCMD {ocmd} >>\nendobj\n"
            ),
        ));
        assert_eq!(
            pixel(&bitmap, 25, 20),
            vec![255, 0, 0],
            "{name} should render"
        );
        assert!(
            bitmap.warnings.is_empty(),
            "{name} reported something: {:?}",
            bitmap.warnings
        );
    }
}

/// An inline property list renders. 7.3.10 keeps indirect references out of
/// content streams and 8.11.3.2 makes an `/OC` entry one, so an inline
/// dictionary names no layer however it is written — including when it is
/// written badly.
#[test]
fn an_inline_property_list_renders() {
    for list in [
        "<< /Type /OCMD >>",
        "<< /Type /OCMD /OCGs [] /P /AllOff >>",
        "<< /Type /OCMD /Nested << /A 1 >> >>",
        "/Type /OCMD >>",
    ] {
        let content = format!(
            "0 1 0 rg 0 0 10 40 re f\n\
             /OC {list} BDC 1 0 0 rg 20 0 10 40 re f EMC"
        );
        let bitmap = render(build(
            &properties(false),
            "/Properties << /MC0 5 0 R >>",
            &content,
            "",
        ));
        assert_eq!(pixel(&bitmap, 25, 20), vec![255, 0, 0], "/OC {list} BDC");
    }
}

/// Text extraction and rendering see the same page.
///
/// The page's only text is inside the `/OFF` layer, in Helvetica — a font
/// this build has metrics for and no outlines for, because it bundles no
/// faces. So the renderer, had it looked for a glyph, would have failed to
/// find one and reported [`RenderWarning::UnreadableFont`]; it does not,
/// because suppression happens at the paint and the outline is never asked
/// for. Extraction reads the same stream through the same interpreter and
/// returns the words.
///
/// The `/ON` half is the decoy. Identical bytes but for the catalog: there
/// the warning *is* raised, so the absence above is a fact about the layer
/// rather than about the fixture.
#[test]
fn hidden_text_is_still_extracted_and_still_not_drawn() {
    let content = "0 1 0 rg 0 0 10 40 re f\n\
                   /OC /MC0 BDC BT /F0 12 Tf 14 20 Td (hidden words) Tj ET EMC";
    let resources = "/Properties << /MC0 5 0 R >>\n\
                     /Font << /F0 6 0 R >>";
    let helvetica = "6 0 obj\n<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>\nendobj\n";

    let make = |on: bool| {
        Document::open(build(&properties(on), resources, content, helvetica))
            .expect("it opens")
            .page(0)
            .expect("a page")
    };

    let hidden = make(false);
    let bitmap = hidden.render(&RenderOptions::default());
    assert!(
        hidden.text().plain_text().contains("hidden words"),
        "extraction still sees the text: {:?}",
        hidden.text().plain_text()
    );
    assert_eq!(
        ink_bounds(&bitmap),
        Some((0, 0, 9, 39)),
        "and the only ink on the page is the rectangle outside the layer"
    );
    assert!(
        !bitmap.warnings.contains(&RenderWarning::UnreadableFont),
        "no glyph was looked for, so no font was found wanting: {:?}",
        bitmap.warnings
    );
    assert!(hid_construction_lines(&bitmap));

    let shown = make(true);
    assert!(shown.text().plain_text().contains("hidden words"));
    assert!(
        shown
            .render(&RenderOptions::default())
            .warnings
            .contains(&RenderWarning::UnreadableFont),
        "the same text in a visible layer does reach the glyph path"
    );
}

/// Milestone 4, the form half: an `/OC` on a form XObject hides the whole
/// form (8.11.4.4).
///
/// Two identical forms, invoked at different places on the page, differing
/// only in whether the XObject dictionary carries `/OC`. The plain one is the
/// decoy: without it, "nothing drew" would pass on a build where forms are
/// broken generally.
#[test]
fn an_oc_entry_on_a_form_xobject_hides_the_form() {
    let form = "6 0 obj\n<< /Type /XObject /Subtype /Form /BBox [0 0 10 40]\n\
                   /Length 26 >>\nstream\n1 0 0 rg 0 0 10 40 re f\nendstream\nendobj\n\
                7 0 obj\n<< /Type /XObject /Subtype /Form /BBox [0 0 10 40] /OC 5 0 R\n\
                   /Length 26 >>\nstream\n1 0 0 rg 0 0 10 40 re f\nendstream\nendobj\n";

    let bitmap = render(build(
        &properties(false),
        "/XObject << /Plain 6 0 R /Hidden 7 0 R >>",
        "q 1 0 0 1 0 0 cm /Plain Do Q\nq 1 0 0 1 20 0 cm /Hidden Do Q",
        form,
    ));

    assert_eq!(
        pixel(&bitmap, 5, 20),
        vec![255, 0, 0],
        "the plain form drew"
    );
    assert_eq!(
        ink_bounds(&bitmap),
        Some((0, 0, 9, 39)),
        "and the one with /OC drew nothing at all"
    );
    assert!(hid_construction_lines(&bitmap), "{:?}", bitmap.warnings);

    // The same file with the group on: both forms draw.
    let bitmap = render(build(
        &properties(true),
        "/XObject << /Plain 6 0 R /Hidden 7 0 R >>",
        "q 1 0 0 1 0 0 cm /Plain Do Q\nq 1 0 0 1 20 0 cm /Hidden Do Q",
        form,
    ));
    assert_eq!(ink_bounds(&bitmap), Some((0, 0, 29, 39)));
}

/// Milestone 4, the image half: a hidden image draws nothing **and is not
/// reported as an unsupported codec**.
///
/// The image is `/JPXDecode`, which this build does not decode. Outside a
/// layer it produces `UnsupportedImage` and ruling 2's grey placeholder —
/// asserted first, so the second half cannot pass because the image path is
/// broken. Inside an `/OFF` layer neither happens: the decode is never
/// attempted, because whether this build could have drawn an image nobody was
/// going to see is not a fact about the page.
#[test]
fn a_hidden_image_draws_nothing_and_reports_no_codec() {
    let image = |oc: &str| {
        format!(
            "6 0 obj\n<< /Type /XObject /Subtype /Image /Width 2 /Height 2\n\
                /ColorSpace /DeviceGray /BitsPerComponent 8 /Filter /JPXDecode {oc}\n\
                /Length 4 >>\nstream\n\x01\x02\x03\x04\nendstream\nendobj\n"
        )
    };
    let content = "0 1 0 rg 0 0 10 40 re f\nq 20 0 0 40 20 0 cm /Im0 Do Q";

    let shown = render(build(
        &properties(false),
        "/XObject << /Im0 6 0 R >>",
        content,
        &image(""),
    ));
    assert!(
        shown
            .warnings
            .iter()
            .any(|w| matches!(w, RenderWarning::UnsupportedImage { .. })),
        "an unhidden JPX is reported: {:?}",
        shown.warnings
    );
    assert_eq!(
        ink_bounds(&shown),
        Some((0, 0, 39, 39)),
        "and its placeholder covers the right half"
    );

    let hidden = render(build(
        &properties(false),
        "/XObject << /Im0 6 0 R >>",
        content,
        &image("/OC 5 0 R"),
    ));
    assert!(
        !hidden
            .warnings
            .iter()
            .any(|w| matches!(w, RenderWarning::UnsupportedImage { .. })),
        "a hidden image is not a missing codec: {:?}",
        hidden.warnings
    );
    assert_eq!(
        ink_bounds(&hidden),
        Some((0, 0, 9, 39)),
        "and no placeholder was painted"
    );
    assert!(hid_construction_lines(&hidden));
}

/// An `/OC` on a form hides its painting and not its text: the form is run,
/// not skipped, for the same reason a `BDC` scope is.
#[test]
fn a_hidden_forms_text_is_still_extracted() {
    // 8.10.2: a form with no `/Resources` of its own inherits the page's, so
    // the font lives there.
    let inner = "BT /F0 12 Tf 2 20 Td (inside the form) Tj ET";
    let form = format!(
        "6 0 obj\n<< /Type /XObject /Subtype /Form /BBox [0 0 40 40] /OC 5 0 R\n\
            /Length {} >>\nstream\n{inner}\nendstream\nendobj\n\
         7 0 obj\n<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>\nendobj\n",
        inner.len()
    );

    let page = Document::open(build(
        &properties(false),
        "/XObject << /Frm 6 0 R >> /Font << /F0 7 0 R >>",
        "0 1 0 rg 0 0 10 40 re f\n/Frm Do",
        &form,
    ))
    .expect("it opens")
    .page(0)
    .expect("a page");

    assert!(
        page.text().plain_text().contains("inside the form"),
        "got {:?}",
        page.text().plain_text()
    );
    let bitmap = page.render(&RenderOptions::default());
    assert_eq!(
        ink_bounds(&bitmap),
        Some((0, 0, 9, 39)),
        "and nothing of the form reached the page"
    );
}

/// The seed `fuzz/corpus/render_page/` carries for 8.11, written from the
/// same helpers as the tests above so the two cannot drift apart.
///
/// Worth having because nothing in any corpus had an `/OCProperties` before
/// it: the group table, the four membership policies, `/VE` evaluation and
/// the `/OC` lookups on `/Properties` and on an XObject were reachable from
/// no fuzz target at all, and every one of them walks
/// attacker-controlled indirect references.
///
/// The seed is **reachable by construction rather than measured**: the
/// assertion below proves this document takes the hidden path before a single
/// byte is mutated. No campaign has been run against it — gap 24 M5 owns
/// those, and `cargo-fuzz` needs libFuzzer, which `x86_64-pc-windows-msvc`
/// does not have.
///
/// Run with `--ignored` when the fixture changes; the corpus is committed,
/// and a run that rewrites it is a diff to look at rather than apply blindly.
#[test]
#[ignore = "writes into fuzz/corpus/render_page, which is committed"]
fn write_the_fuzz_seed() {
    let bytes = fuzz_seed();
    // A seed that no longer reaches what it was chosen for is worse than no
    // seed, because it looks like coverage.
    let bitmap = render(bytes.clone());
    assert!(
        hid_construction_lines(&bitmap),
        "the seed must still take the hidden path: {:?}",
        bitmap.warnings
    );

    let base =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fuzz/corpus/render_page");
    std::fs::write(base.join("optional-content.pdf"), bytes)
        .expect("the corpus directory is there");
}

/// Every branch of 8.11 this build has, in one small document: a group that
/// is off and one that is on, a membership dictionary with a `/P`, one with a
/// nested `/VE`, an `/OC` on a form XObject, and a `BMC`/`EMC` pair with no
/// property list at all.
fn fuzz_seed() -> Vec<u8> {
    let form = "1 0 0 rg 0 0 40 40 re f";
    let content = "0 1 0 rg 0 0 10 40 re f\n\
                   /OC /Hidden BDC 0.9 0 0 rg 0 0 40 40 re f EMC\n\
                   /OC /Shown BDC 0 0 1 rg 12 2 6 36 re f EMC\n\
                   /OC /Policy BDC 0 0 0 RG 2 w 2 38 m 38 38 l S EMC\n\
                   /OC /Expression BDC 0.5 g 30 2 8 8 re f EMC\n\
                   BMC 0.2 g 2 2 4 4 re f EMC\n\
                   /Span << /Inline 1 >> BDC EMC\n\
                   /Frm Do";
    build(
        "/OCProperties << /OCGs [5 0 R 6 0 R] \
         /D << /BaseState /ON /ON [6 0 R] /OFF [5 0 R] >> >>",
        "/Properties << /Hidden 5 0 R /Shown 6 0 R /Policy 7 0 R \
         /Expression 8 0 R >>\n/XObject << /Frm 9 0 R >>",
        content,
        &format!(
            "6 0 obj\n<< /Type /OCG /Name (Base) >>\nendobj\n\
             7 0 obj\n<< /Type /OCMD /OCGs [5 0 R 6 0 R] /P /AnyOff >>\nendobj\n\
             8 0 obj\n<< /Type /OCMD /VE [/And [/Not 5 0 R] 6 0 R] >>\nendobj\n\
             9 0 obj\n<< /Type /XObject /Subtype /Form /BBox [0 0 40 40] /OC 5 0 R\n\
                /Length {} >>\nstream\n{form}\nendstream\nendobj\n",
            form.len()
        ),
    )
}

/// Hidden content still *runs*. Only its painting stops.
///
/// The scope sets a colour, saves the state, scales it, fills, and restores.
/// Two things after `EMC` measure whether the operators actually executed:
///
/// - the rectangle is red, which it can only be because the `rg` inside the
///   hidden scope reached the graphics state. A build that skipped the scope
///   rather than suppressing its paints fills with the initial black;
/// - the rectangle is 20 by 40 rather than 5 by 10, which is the `q` and `Q`
///   having balanced around the `cm`.
///
/// This is the test that separates "suppress at the paint" from "do not
/// interpret", and neither assertion can be satisfied by a page that drew
/// nothing.
#[test]
fn a_hidden_scope_still_runs_the_operators_inside_it() {
    let bitmap = render(build(
        &properties(false),
        "/Properties << /MC0 5 0 R >>",
        "/OC /MC0 BDC 1 0 0 rg q 0.25 0 0 0.25 0 0 cm 0 0 40 40 re f Q EMC\n\
         0 0 20 40 re f",
        "",
    ));

    assert_eq!(
        pixel(&bitmap, 10, 20),
        vec![255, 0, 0],
        "the colour the hidden scope set is in force after it"
    );
    assert_eq!(
        ink_bounds(&bitmap),
        Some((0, 0, 19, 39)),
        "and the fill is at full size, so the q/Q balanced"
    );
}
