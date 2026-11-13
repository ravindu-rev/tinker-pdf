//! Optional content, from the catalog to the pixels (8.11).
//!
//! Everything here is built from bytes rather than from `testdata/`, whose
//! four files are mutool-written and are not to be modified — and none of
//! which has an `/OCProperties` anyway.
//!
//! Every fixture draws something *outside* the layer it is about. "The page
//! is white" is what a render that failed outright also produces, and a suite
//! that hides content correctly for the wrong reason is exactly the failure
//! this gap exists to prevent.

use tinker_pdf::{
    Bitmap, CacheLimits, Document, LayerOptions, LayerUser, ObjRef, Parallelism, RenderOptions,
    RenderWarning, Usage,
};

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
    // The same text in a visible layer *does* reach the glyph path, and the
    // evidence for that depends on whether this build has a face to draw with:
    // without one the path reports `UnreadableFont`, with one it puts ink past
    // the rectangle at x = 9. Either is the glyph path having been entered,
    // which is the property; asserting only the warning made this test a
    // statement about the font policy instead, and a `bundled-fonts` build
    // failed it for doing the right thing.
    let shown = shown.render(&RenderOptions::default());
    let reached = shown.warnings.contains(&RenderWarning::UnreadableFont)
        || ink_bounds(&shown).is_some_and(|(_, _, x1, _)| x1 > 9);
    assert!(
        reached,
        "the same text in a visible layer does reach the glyph path: {:?} {:?}",
        shown.warnings,
        ink_bounds(&shown)
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

// ---- Rendering for a usage (RC-13.2) ----------------------------------------

/// A 40x40 page whose optional content group 5 is "Construction lines" with
/// `usage` as its `/Usage` dictionary's entries, and whose catalog's `/D` is
/// `d`, around `content` and the page resources `resources`; `objects` follow
/// from 6.
fn with_usage(d: &str, usage: &str, resources: &str, content: &str, objects: &str) -> Vec<u8> {
    format!(
        "%PDF-1.7\n\
1 0 obj\n<< /Type /Catalog /Pages 2 0 R /OCProperties << /OCGs [5 0 R] /D << {d} >> >> >>\nendobj\n\
2 0 obj\n<< /Type /Pages /Count 1 /Kids [3 0 R] >>\nendobj\n\
3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 40 40]\n\
   /Resources << {resources} >> /Contents 4 0 R >>\nendobj\n\
4 0 obj\n<< /Length {} >>\nstream\n{content}\nendstream\nendobj\n\
5 0 obj\n<< /Type /OCG /Name (Construction lines) /Usage << {usage} >> >>\nendobj\n\
{objects}\
trailer\n<< /Size 20 /Root 1 0 R >>\n%%EOF\n",
        content.len()
    )
    .into_bytes()
}

/// The first page of `bytes`, rendered for `usage`.
fn render_for(bytes: &[u8], usage: Option<Usage>) -> Bitmap {
    Document::open(bytes.to_vec())
        .expect("it opens")
        .page(0)
        .expect("a page")
        .render(&RenderOptions {
            usage,
            ..RenderOptions::default()
        })
}

/// Holds two renders to one picture: the same size, the same bytes and the
/// same warnings.
#[track_caller]
fn same_render(got: &Bitmap, want: &Bitmap, what: &str) {
    assert_eq!((got.width, got.height), (want.width, want.height), "{what}");
    assert!(got.data == want.data, "{what}: the pixels differ");
    assert_eq!(got.warnings, want.warnings, "{what}: the warnings differ");
}

/// `/D`'s body, setting group 5 on or off.
fn d_with(on: bool) -> String {
    format!("/{} [5 0 R]", if on { "ON" } else { "OFF" })
}

/// **A print-usage render of an OCMD over groups whose `/Usage /Print
/// /PrintState` is `/OFF` is its `/D` twin** (RC-13.2, ISO 32000-2
/// 8.11.4.4): under an `/AS` entry for print, both groups the membership
/// dictionary names are off in a print render, so the red rectangle it marks
/// is hidden there exactly as on the same page whose `/D` turns both groups
/// off and says nothing of usage; and a view render, an export render and a
/// render naming no usage draw it as the page always has, both groups on.
#[test]
fn a_print_usage_render_of_an_ocmd_with_print_usage_is_its_d_twin() {
    let as_print = "/AS [<< /Event /Print /Category [/Print] /OCGs [5 0 R 6 0 R] >>]";
    let page = |d: &str| {
        with_usage(
            d,
            "/Print << /PrintState /OFF >>",
            "/Properties << /MC0 7 0 R >>",
            TWO_RECTANGLES,
            "6 0 obj\n<< /Type /OCG /Name (Second) /Usage << /Print << /PrintState /OFF >> >> >>\nendobj\n\
             7 0 obj\n<< /Type /OCMD /OCGs [5 0 R 6 0 R] /P /AnyOn >>\nendobj\n",
        )
    };
    let original = page(&format!("/ON [5 0 R 6 0 R] {as_print}"));
    let printed = render_for(&original, Some(Usage::Print));
    assert_eq!(
        pixel(&printed, 25, 20),
        vec![255, 255, 255],
        "hidden in print"
    );
    assert_eq!(
        pixel(&printed, 5, 20),
        vec![0, 255, 0],
        "the unmarked rectangle paints"
    );
    let twin = render_for(&page("/OFF [5 0 R 6 0 R]"), None);
    same_render(&printed, &twin, "print usage against its /D twin");

    let as_before = render_for(&page("/ON [5 0 R 6 0 R]"), None);
    assert_eq!(pixel(&as_before, 25, 20), vec![255, 0, 0], "both groups on");
    for usage in [None, Some(Usage::View), Some(Usage::Export)] {
        same_render(
            &render_for(&original, usage),
            &as_before,
            &format!("{usage:?}: as before"),
        );
    }
}

/// What a render of `bytes` for `usage` must equal: its twin, the page whose
/// `/D` sets group 5 to `expected` and has no `/AS`.
#[track_caller]
fn renders_as_its_twin(bytes: &[u8], usage: Option<Usage>, expected: bool, what: &str) {
    let twin = with_usage(
        &d_with(expected),
        "",
        "/Properties << /MC0 5 0 R >>",
        TWO_RECTANGLES,
        "",
    );
    same_render(&render_for(bytes, usage), &render_for(&twin, None), what);
}

/// **Each usage and state category renders as its `/D` twin** (RC-13.2):
/// for every usage a render names, every event an `/AS` entry names, every
/// state category it lists and both states, over a group `/D` turns on and
/// one it turns off, a render equals the page whose `/D` is written as the
/// states 8.11.4.4 gives — the entry's state where its event is the render's
/// usage, and `/D`'s own elsewhere and with no usage.
#[test]
fn each_usage_and_state_category_renders_as_its_d_twin() {
    let usages = [Usage::View, Usage::Print, Usage::Export];
    let mut cases = 0;
    for event in usages {
        let event_name = format!("{event:?}");
        for category in ["View", "Print", "Export"] {
            for state in [true, false] {
                for initially in [true, false] {
                    let bytes = with_usage(
                        &format!(
                            "{} /AS [<< /Event /{event_name} /Category [/{category}] /OCGs [5 0 R] >>]",
                            d_with(initially)
                        ),
                        &format!(
                            "/{category} << /{category}State /{} >>",
                            if state { "ON" } else { "OFF" }
                        ),
                        "/Properties << /MC0 5 0 R >>",
                        TWO_RECTANGLES,
                        "",
                    );
                    for usage in usages.map(Some).into_iter().chain([None]) {
                        let expected = if usage == Some(event) {
                            state
                        } else {
                            initially
                        };
                        renders_as_its_twin(
                            &bytes,
                            usage,
                            expected,
                            &format!(
                                "a {event_name} entry's {category} state {state} over {initially}, \
                                 rendered for {usage:?}"
                            ),
                        );
                        cases += 1;
                    }
                }
            }
        }
    }
    assert_eq!(cases, 3 * 3 * 2 * 2 * 4, "every case ran");
}

/// **Recommendations combine as 8.11.4.4 says, and what is malformed leaves
/// the group as `/D` set it** (RC-13.2): a group is on only if every
/// recommendation it has, across its entry's categories and across every
/// entry of the event, is on; a category with no entry, or no state, in its
/// usage dictionary recommends nothing, and so does one whose input the
/// render does not state — a language or a user — while a `/Zoom` is read at
/// the render's magnification, 1 here; and each malformed shape — an `/AS` that is no array, an entry that
/// is no dictionary or names no event this build knows, a `/Category` that is
/// absent or no array, a `/Usage` or a category that is no dictionary, a
/// state that is neither name — is the `/D` twin, in a print render.
#[test]
fn recommendations_combine_and_what_is_malformed_leaves_d() {
    let print = Some(Usage::Print);
    let entry =
        |categories: &str| format!("<< /Event /Print /Category [{categories}] /OCGs [5 0 R] >>");
    // (what, /D's state, /AS, the group's usage entries, the state expected)
    let cases: Vec<(&str, bool, String, &str, bool)> =
        vec![
        (
            "two categories, both on",
            false,
            format!("[{}]", entry("/View /Print")),
            "/View << /ViewState /ON >> /Print << /PrintState /ON >>",
            true,
        ),
        (
            "two categories, one off",
            true,
            format!("[{}]", entry("/View /Print")),
            "/View << /ViewState /ON >> /Print << /PrintState /OFF >>",
            false,
        ),
        (
            "two entries of the event, one off",
            true,
            format!("[{} {}]", entry("/View"), entry("/Print")),
            "/View << /ViewState /ON >> /Print << /PrintState /OFF >>",
            false,
        ),
        (
            "two entries of the event, both on",
            false,
            format!("[{} {}]", entry("/View"), entry("/Print")),
            "/View << /ViewState /ON >> /Print << /PrintState /ON >>",
            true,
        ),
        (
            "a category the group has no entry for, beside one it has",
            true,
            format!("[{}]", entry("/Export /Print")),
            "/Print << /PrintState /OFF >>",
            false,
        ),
        ("no PrintState", false, format!("[{}]", entry("/Print")), "/Print << >>", false),
        ("no PrintState, on", true, format!("[{}]", entry("/Print")), "/Print << >>", true),
        (
            "a zoom category whose /min is above the magnification",
            true,
            format!("[{}]", entry("/Zoom")),
            "/Zoom << /min 4 >>",
            false,
        ),
        (
            "categories that carry no state, and a language and a user the render does not state",
            false,
            format!("[{}]", entry("/CreatorInfo /PageElement /Language /User")),
            "/CreatorInfo << /Creator (x) /Subtype /Artwork >> /PageElement << /Subtype /HF >> \
             /Language << /Lang (en-US) >> /User << /Type /Ind /Name (x) >>",
            false,
        ),
        ("an /AS that is no array", true, "5".to_string(), "/Print << /PrintState /OFF >>", true),
        (
            "an entry that is no dictionary",
            true,
            "[5 [/Print]]".to_string(),
            "/Print << /PrintState /OFF >>",
            true,
        ),
        (
            "an event this build does not know",
            true,
            "[<< /Event /Display /Category [/Print] /OCGs [5 0 R] >>]".to_string(),
            "/Print << /PrintState /OFF >>",
            true,
        ),
        (
            "no event",
            true,
            "[<< /Category [/Print] /OCGs [5 0 R] >>]".to_string(),
            "/Print << /PrintState /OFF >>",
            true,
        ),
        (
            "a category that is no array",
            true,
            "[<< /Event /Print /Category /Print /OCGs [5 0 R] >>]".to_string(),
            "/Print << /PrintState /OFF >>",
            true,
        ),
        (
            "no category",
            true,
            "[<< /Event /Print /OCGs [5 0 R] >>]".to_string(),
            "/Print << /PrintState /OFF >>",
            true,
        ),
        (
            "no groups",
            true,
            "[<< /Event /Print /Category [/Print] >>]".to_string(),
            "/Print << /PrintState /OFF >>",
            true,
        ),
        (
            "a category entry that is no dictionary",
            true,
            format!("[{}]", entry("/Print")),
            "/Print /OFF",
            true,
        ),
        (
            "a state that is neither name",
            false,
            format!("[{}]", entry("/Print")),
            "/Print << /PrintState /Maybe >>",
            false,
        ),
        (
            "a state written as a string",
            true,
            format!("[{}]", entry("/Print")),
            "/Print << /PrintState (OFF) >>",
            true,
        ),
    ];
    for (what, initially, auto_state, usage_entries, expected) in &cases {
        let bytes = with_usage(
            &format!("{} /AS {auto_state}", d_with(*initially)),
            usage_entries,
            "/Properties << /MC0 5 0 R >>",
            TWO_RECTANGLES,
            "",
        );
        renders_as_its_twin(&bytes, print, *expected, what);
        renders_as_its_twin(&bytes, None, *initially, &format!("{what}, no usage"));
    }

    // A `/Usage` that is no dictionary at all.
    let bytes = format!(
        "%PDF-1.7\n\
1 0 obj\n<< /Type /Catalog /Pages 2 0 R /OCProperties << /OCGs [5 0 R] /D << /ON [5 0 R] \
/AS [<< /Event /Print /Category [/Print] /OCGs [5 0 R] >>] >> >> >>\nendobj\n\
2 0 obj\n<< /Type /Pages /Count 1 /Kids [3 0 R] >>\nendobj\n\
3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 40 40]\n\
   /Resources << /Properties << /MC0 5 0 R >> >> /Contents 4 0 R >>\nendobj\n\
4 0 obj\n<< /Length {} >>\nstream\n{TWO_RECTANGLES}\nendstream\nendobj\n\
5 0 obj\n<< /Type /OCG /Name (Construction lines) /Usage 7 >>\nendobj\n\
trailer\n<< /Size 20 /Root 1 0 R >>\n%%EOF\n",
        TWO_RECTANGLES.len()
    )
    .into_bytes();
    renders_as_its_twin(&bytes, print, true, "a /Usage that is no dictionary");
}

/// A page whose group 5 marks content in every scope a render opens: a form's
/// own content, a form whose `/OC` names it, a tiling pattern's cell and an
/// annotation's appearance, beside an unmarked band; the catalog's `/D` is
/// `d`, and the group's `/Usage` turns it off in print. The annotation's `/F`
/// sets Print, so a print render draws it (12.5.3).
fn every_scope_page(d: &str) -> Vec<u8> {
    let properties = "/Properties << /MC0 5 0 R >>";
    let marked = "/OC /MC0 BDC 1 0 0 rg 0 0 10 10 re f EMC";
    let content = "0 1 0 rg 0 30 40 10 re f\n\
                   q 1 0 0 1 0 0 cm /Fm Do Q\n\
                   q 1 0 0 1 12 0 cm /Hid Do Q\n\
                   /Pattern cs /P0 scn 24 0 10 10 re f";
    let form = |extra: &str| {
        format!(
            "<< /Type /XObject /Subtype /Form /BBox [0 0 10 10] {extra} \
             /Resources << {properties} >> /Length {} >>\nstream\n{marked}\nendstream",
            marked.len()
        )
    };
    let cell = format!(
        "<< /PatternType 1 /PaintType 1 /TilingType 1 /BBox [0 0 10 10] /XStep 10 /YStep 10 \
         /Resources << {properties} >> /Length {} >>\nstream\n{marked}\nendstream",
        marked.len()
    );
    let shown = "1 0 0 rg 0 0 10 10 re f";
    let objects = format!(
        "6 0 obj\n{}\nendobj\n\
         7 0 obj\n{}\nendobj\n\
         8 0 obj\n{cell}\nendobj\n\
         9 0 obj\n<< /Type /Annot /Subtype /Square /F 4 /Rect [30 12 40 22] /AP << /N 10 0 R >> >>\nendobj\n\
         10 0 obj\n{}\nendobj\n",
        form(""),
        format!(
            "<< /Type /XObject /Subtype /Form /BBox [0 0 10 10] /OC 5 0 R /Length {} >>\nstream\n{shown}\nendstream",
            shown.len()
        ),
        form(""),
    );
    let mut bytes = with_usage(
        d,
        "/Print << /PrintState /OFF >>",
        &format!("{properties} /XObject << /Fm 6 0 R /Hid 7 0 R >> /Pattern << /P0 8 0 R >>"),
        content,
        &objects,
    );
    // The page carries the annotation.
    let at = bytes
        .windows(b"/Contents 4 0 R".len())
        .position(|window| window == b"/Contents 4 0 R")
        .expect("the page dictionary");
    bytes.splice(at..at, b"/Annots [9 0 R] ".iter().copied());
    bytes
}

/// Where [`every_scope_page`] draws each scope's marked content: user y 0 to
/// 10 is rows 30 to 40, and the annotation's `/Rect`, 12 to 22 up, rows 18 to
/// 28.
const EVERY_SCOPE: [(&str, u32, u32); 4] = [
    ("the form's marked content", 5, 35),
    ("the form the group hides", 17, 35),
    ("the tiling cell's marked content", 29, 35),
    ("the appearance's marked content", 35, 23),
];

/// **A usage's states reach every scope a render opens** (RC-13.2): a
/// form's own content marking the group, a form whose `/OC` names it, a
/// tiling pattern's cell marking it and an annotation's appearance marking
/// it each hide in a print render where the group's print state is off, as
/// on the page whose `/D` turns the group off — the one binding of the
/// render read by the page's scope, each form's, the cell's and the
/// appearance's. The annotation's `/F` sets Print, so a print render draws
/// it (12.5.3) and its appearance's scope is read.
#[test]
fn a_usages_states_reach_every_scope_a_render_opens() {
    let page = every_scope_page;
    let original = page("/ON [5 0 R] /AS [<< /Event /Print /Category [/Print] /OCGs [5 0 R] >>]");
    let printed = render_for(&original, Some(Usage::Print));
    // The page's foot is the bottom of the picture.
    for (x, y) in [(5, 5), (35, 5)] {
        assert_eq!(pixel(&printed, x, y), vec![0, 255, 0], "the unmarked band");
    }
    for (what, x, y) in EVERY_SCOPE {
        assert_eq!(
            pixel(&printed, x, y),
            vec![255, 255, 255],
            "{what}: hidden in print"
        );
    }
    same_render(
        &printed,
        &render_for(&page("/OFF [5 0 R]"), None),
        "every scope against the /D twin",
    );
    let viewed = render_for(&original, Some(Usage::View));
    for (what, x, y) in EVERY_SCOPE {
        assert_eq!(
            pixel(&viewed, x, y),
            vec![255, 0, 0],
            "{what}: drawn in view"
        );
    }
    same_render(
        &viewed,
        &render_for(&page("/ON [5 0 R]"), None),
        "view usage as before",
    );
}

// ---- A render's configuration (RC-14.2) ---------------------------------------

/// The page `configured` builds: an unmarked green band, then a band in each
/// of groups 5 "A", 6 "B" and 7 "C".
const BANDS: &str = "0 1 0 rg 0 0 10 40 re f\n\
                     /OC /A BDC 1 0 0 rg 10 0 10 40 re f EMC\n\
                     /OC /B BDC 0 0 1 rg 20 0 10 40 re f EMC\n\
                     /OC /C BDC 1 1 0 rg 30 0 10 40 re f EMC";

/// A 40x40 page drawing [`BANDS`], whose groups 5 "A", 6 "B" and 7 "C" carry
/// `groups`' entries beside their `/Type` and `/Name`; the catalog's `/D` is
/// `d` and its `/Configs` holds `configs`, where `9 0 R` is a configuration
/// reached by reference, `/BaseState /OFF` with `/ON [5 0 R]` and its own
/// print entry over group 5.
fn configured(groups: [&str; 3], d: &str, configs: &str) -> Vec<u8> {
    let configs = if configs.is_empty() {
        String::new()
    } else {
        format!("/Configs [{configs}]")
    };
    format!(
        "%PDF-1.7\n\
1 0 obj\n<< /Type /Catalog /Pages 2 0 R /OCProperties << /OCGs [5 0 R 6 0 R 7 0 R] \
/D << {d} >> {configs} >> >>\nendobj\n\
2 0 obj\n<< /Type /Pages /Count 1 /Kids [3 0 R] >>\nendobj\n\
3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 40 40]\n\
   /Resources << /Properties << /A 5 0 R /B 6 0 R /C 7 0 R >> >> /Contents 4 0 R >>\nendobj\n\
4 0 obj\n<< /Length {} >>\nstream\n{BANDS}\nendstream\nendobj\n\
5 0 obj\n<< /Type /OCG /Name (A) {} >>\nendobj\n\
6 0 obj\n<< /Type /OCG /Name (B) {} >>\nendobj\n\
7 0 obj\n<< /Type /OCG /Name (C) {} >>\nendobj\n\
9 0 obj\n<< /Name (Indirect) /BaseState /OFF /ON [5 0 R] \
/AS [<< /Event /Print /Category [/Print] /OCGs [5 0 R] >>] >>\nendobj\n\
trailer\n<< /Size 20 /Root 1 0 R >>\n%%EOF\n",
        BANDS.len(),
        groups[0],
        groups[1],
        groups[2],
    )
    .into_bytes()
}

/// `/D`'s body writing groups 5, 6 and 7 in `states`, and nothing else.
fn d_of(states: [bool; 3]) -> String {
    let on: Vec<&str> = ["5 0 R", "6 0 R", "7 0 R"]
        .into_iter()
        .zip(states)
        .filter_map(|(group, on)| on.then_some(group))
        .collect();
    format!("/BaseState /OFF /ON [{}]", on.join(" "))
}

/// Groups A and C turned off by a print state, which an `/AS` print entry
/// over them applies.
const PRINT_OFF: [&str; 3] = [
    "/Usage << /Print << /PrintState /OFF >> >>",
    "",
    "/Usage << /Print << /PrintState /OFF >> >>",
];

/// The options rendering `configuration` for `usage`.
fn under(configuration: Option<usize>, usage: Option<Usage>) -> RenderOptions {
    RenderOptions {
        usage,
        layers: LayerOptions {
            configuration,
            ..LayerOptions::default()
        },
        ..RenderOptions::default()
    }
}

/// **Each configuration renders as its `/D` twin** (RC-14.2,
/// `docs/design/optional-content-config.md`): `/D` and `/Configs` entries of
/// base state `/ON` with `/OFF`, `/OFF` with `/ON`, `/Unchanged` with both,
/// none with `/OFF`, and one reached by reference with an `/AS` of its own,
/// each rendered with no usage, for view and for print, equal — pixels and
/// warnings — to the page whose `/D` is written as that configuration's
/// states and says nothing of usage: directly, from a display list recorded
/// under the configuration, and from one recorded under `/D`. A chosen
/// configuration's states are built over `/D`'s, its `/AS` is its own — the
/// print entry `/D` has over C is not applied under a configuration that has
/// none — and `/Unchanged` keeps `/D`'s. An index past `/Configs`, of an
/// entry that is no dictionary, or past every bound renders as `/D` and is
/// reported by its index.
#[test]
fn each_configuration_renders_as_its_d_twin() {
    let d = "/OFF [6 0 R] /AS [<< /Event /Print /Category [/Print] /OCGs [7 0 R] >>]";
    let configs = "<< /Name (Zero) /BaseState /ON /OFF [5 0 R] >> \
                   << /Name (One) /BaseState /OFF /ON [6 0 R] >> \
                   << /Name (Two) /BaseState /Unchanged /ON [6 0 R] /OFF [7 0 R] >> \
                   << /Name (Three) /OFF [6 0 R 7 0 R] >> \
                   5 9 0 R";
    let document = Document::open(configured(PRINT_OFF, d, configs)).expect("it opens");
    let page = document.page(0).expect("a page");
    // (configuration, A B C with no usage and in view, A B C in print)
    let cases = [
        (None, [true, false, true], [true, false, false]),
        (Some(0), [false, true, true], [false, true, true]),
        (Some(1), [false, true, false], [false, true, false]),
        (Some(2), [true, true, false], [true, true, false]),
        (Some(3), [true, false, false], [true, false, false]),
        (Some(5), [true, false, false], [false, false, false]),
    ];
    for (configuration, shown, printed) in cases {
        for (usage, states) in [
            (None, shown),
            (Some(Usage::View), shown),
            (Some(Usage::Print), printed),
        ] {
            let what = format!("configuration {configuration:?}, {usage:?}");
            let twin = render_for(&configured(PRINT_OFF, &d_of(states), ""), None);
            let options = under(configuration, usage);
            same_render(&page.render(&options), &twin, &format!("{what}, directly"));
            same_render(
                &page
                    .display_list_with(usage, &options.layers)
                    .render(&options),
                &twin,
                &format!("{what}, from its own list"),
            );
            same_render(
                &page.display_list_for(usage).render(&options),
                &twin,
                &format!("{what}, from a list under /D"),
            );
        }
    }

    let as_d = page.render(&RenderOptions::default());
    let list = page.display_list();
    for index in [4, 6, 255, 256, usize::MAX] {
        let options = under(Some(index), None);
        let mut warnings = as_d.warnings.clone();
        warnings.push(RenderWarning::UnreadableLayerConfiguration { index });
        for (how, got) in [
            ("directly", page.render(&options)),
            ("from a list", list.render(&options)),
            (
                "from its own list",
                page.display_list_with(None, &options.layers)
                    .render(&options),
            ),
        ] {
            assert!(got.data == as_d.data, "{index}, {how}: drawn as /D");
            assert_eq!(got.warnings, warnings, "{index}, {how}: reported by index");
        }
    }
}

/// **An intent the configuration does not hold has no effect** (8.11.2.3):
/// group A is a `/Design` layer, B one of the default `/View`, and C both,
/// all three off in `/D`. Under `/D`, whose intents are `/View`, A has no
/// effect and draws while B and C stay hidden; under a configuration whose
/// `/Intent` is `/Design`, A and C count and are hidden while B draws; under
/// an empty `/Intent` nothing counts and everything draws; under `/All`
/// everything counts; and an `/Intent` that is neither a name nor an array
/// is `/View`. Each equals its twin, the page with no `/Intent` anywhere
/// whose `/D` shows what the intents leave visible.
#[test]
fn an_intent_the_configuration_does_not_hold_has_no_effect() {
    let groups = ["/Intent /Design", "", "/Intent [/View /Design]"];
    let d = "/OFF [5 0 R 6 0 R 7 0 R]";
    let configs = "<< /Intent /Design /BaseState /OFF >> \
                   << /Intent [] /BaseState /OFF >> \
                   << /Intent /All /BaseState /OFF >> \
                   << /Intent 7 /BaseState /OFF >>";
    let document = Document::open(configured(groups, d, configs)).expect("it opens");
    let page = document.page(0).expect("a page");
    // (configuration, what draws of A, B and C)
    for (configuration, drawn) in [
        (None, [true, false, false]),
        (Some(0), [false, true, false]),
        (Some(1), [true, true, true]),
        (Some(2), [false, false, false]),
        (Some(3), [true, false, false]),
    ] {
        let what = format!("configuration {configuration:?}");
        let twin = render_for(&configured(["", "", ""], &d_of(drawn), ""), None);
        let options = under(configuration, None);
        same_render(&page.render(&options), &twin, &what);
        same_render(
            &page.display_list().render(&options),
            &twin,
            &format!("{what}, from a list"),
        );
    }
    // The Design layer `/D` turns off draws, which it did not before `/D`'s
    // intents were read: the correction 8.11.2.3 states.
    let plain = page.render(&RenderOptions::default());
    assert_eq!(pixel(&plain, 15, 20), vec![255, 0, 0], "A draws under /D");
    assert_eq!(
        pixel(&plain, 25, 20),
        vec![255, 255, 255],
        "B is hidden under /D"
    );
}

// ---- A render's overrides (RC-14.3) -------------------------------------------

/// The page [`radio`] builds: an unmarked green band, then a band in each of
/// groups 5 "A", 6 "B" and 7 "C", which `/OCGs` lists, and one in group 8
/// "U", which it does not.
const RADIO_BANDS: &str = "0 1 0 rg 0 0 8 40 re f\n\
                           /OC /A BDC 1 0 0 rg 8 0 8 40 re f EMC\n\
                           /OC /B BDC 0 0 1 rg 16 0 8 40 re f EMC\n\
                           /OC /C BDC 1 1 0 rg 24 0 8 40 re f EMC\n\
                           /OC /U BDC 1 0 1 rg 32 0 8 40 re f EMC";

/// A 40x40 page drawing [`RADIO_BANDS`]. `/OCGs` lists groups 5 "A", 6 "B"
/// and 7 "C", which carry `groups`' entries beside their `/Type` and `/Name`,
/// and 9, a group with no `/Name` that marks nothing; group 8 "U" marks its
/// band and is not listed. The catalog's `/D` is `d`, and its `/Configs`
/// holds `configs` where there are any.
fn radio(groups: [&str; 3], d: &str, configs: &str) -> Vec<u8> {
    let configs = if configs.is_empty() {
        String::new()
    } else {
        format!("/Configs [{configs}]")
    };
    format!(
        "%PDF-1.7\n\
1 0 obj\n<< /Type /Catalog /Pages 2 0 R /OCProperties << /OCGs [5 0 R 6 0 R 7 0 R 9 0 R] \
/D << {d} >> {configs} >> >>\nendobj\n\
2 0 obj\n<< /Type /Pages /Count 1 /Kids [3 0 R] >>\nendobj\n\
3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 40 40]\n\
   /Resources << /Properties << /A 5 0 R /B 6 0 R /C 7 0 R /U 8 0 R >> >> /Contents 4 0 R >>\nendobj\n\
4 0 obj\n<< /Length {} >>\nstream\n{RADIO_BANDS}\nendstream\nendobj\n\
5 0 obj\n<< /Type /OCG /Name (A) {} >>\nendobj\n\
6 0 obj\n<< /Type /OCG /Name (B) {} >>\nendobj\n\
7 0 obj\n<< /Type /OCG /Name (C) {} >>\nendobj\n\
8 0 obj\n<< /Type /OCG /Name (U) >>\nendobj\n\
9 0 obj\n<< /Type /OCG >>\nendobj\n\
trailer\n<< /Size 20 /Root 1 0 R >>\n%%EOF\n",
        RADIO_BANDS.len(),
        groups[0],
        groups[1],
        groups[2],
    )
    .into_bytes()
}

/// `/D`'s body writing groups 5, 6, 7 and 9 in `states`, and nothing else:
/// no `/RBGroups`, no `/Locked`, no `/AS`.
fn radio_d(states: [bool; 4]) -> String {
    let on: Vec<&str> = ["5 0 R", "6 0 R", "7 0 R", "9 0 R"]
        .into_iter()
        .zip(states)
        .filter_map(|(group, on)| on.then_some(group))
        .collect();
    format!("/BaseState /OFF /ON [{}]", on.join(" "))
}

/// The options rendering `configuration` for `usage` with `overrides`, each
/// group by its object number.
fn overriding(
    configuration: Option<usize>,
    usage: Option<Usage>,
    overrides: &[(u32, bool)],
) -> RenderOptions {
    RenderOptions {
        usage,
        layers: LayerOptions {
            configuration,
            overrides: overrides
                .iter()
                .map(|&(num, on)| (ObjRef::new(num, 0), on))
                .collect(),
            ..LayerOptions::default()
        },
        ..RenderOptions::default()
    }
}

/// Holds a render to its twin's pixels, and to its twin's warnings followed by
/// `more`, in order: what a render reports of its configuration and its
/// refused overrides, which the twin, choosing and overriding nothing, does
/// not.
#[track_caller]
fn same_render_and(got: &Bitmap, twin: &Bitmap, more: &[RenderWarning], what: &str) {
    assert_eq!((got.width, got.height), (twin.width, twin.height), "{what}");
    assert!(got.data == twin.data, "{what}: the pixels differ");
    let mut warnings = twin.warnings.clone();
    warnings.extend(more.iter().cloned());
    assert_eq!(got.warnings, warnings, "{what}: the warnings differ");
}

/// **Overrides, radio buttons and locks render as their `/D` twins** (RC-14.3,
/// `docs/design/optional-content-config.md`): each row's overrides, applied in
/// order after the configuration and its auto-state, give the states a page
/// whose `/D` is written as them draws — directly, from a display list
/// recorded under the same options, and from one recorded under `/D` — and
/// each override a lock refuses is reported once by the group's `/Name`, or by
/// its reference where it has none. Turning a group on turns off the other
/// members of every `/RBGroups` collection it is in, in either order and in
/// two collections at once, and turning one off changes nothing else; the
/// collections constrain overrides and not a configuration's own `/ON`; a
/// locked group keeps its state whatever an override asks, and a locked
/// member of a collection is not turned off; an override of a group `/OCGs`
/// does not list — locked or not, or no object at all — changes nothing; a
/// configuration other than `/D` takes `/D`'s `/RBGroups` where it has none and
/// never its `/Locked`; and a `/RBGroups` or `/Locked` this build cannot read
/// constrains nothing.
#[test]
fn overrides_radio_buttons_and_locks_render_as_their_d_twins() {
    let print_off = ["/Usage << /Print << /PrintState /OFF >> >>", "", ""];
    // (what, groups' entries, /D, /Configs, configuration, usage, overrides,
    // A B C 9 as drawn, the locked groups reported)
    type Case<'a> = (
        &'a str,
        [&'a str; 3],
        &'a str,
        &'a str,
        Option<usize>,
        Option<Usage>,
        &'a [(u32, bool)],
        [bool; 4],
        &'a [&'a str],
    );
    let none = ["", "", ""];
    let inherits = "<< /Name (Inherits) >> << /RBGroups [] /Locked [5 0 R] >>";
    let cases: &[Case] = &[
        (
            "one override on and one off",
            none,
            "/OFF [5 0 R]",
            "",
            None,
            None,
            &[(5, true), (7, false)],
            [true, true, false, true],
            &[],
        ),
        (
            "a later override of a group wins",
            none,
            "",
            "",
            None,
            None,
            &[(6, false), (7, false), (6, true)],
            [true, true, false, true],
            &[],
        ),
        (
            "two overrides into one radio-button group, A then B",
            none,
            "/OFF [5 0 R 6 0 R] /RBGroups [[5 0 R 6 0 R 7 0 R]]",
            "",
            None,
            None,
            &[(5, true), (6, true)],
            [false, true, false, true],
            &[],
        ),
        (
            "two overrides into one radio-button group, B then A",
            none,
            "/OFF [5 0 R 6 0 R] /RBGroups [[5 0 R 6 0 R 7 0 R]]",
            "",
            None,
            None,
            &[(6, true), (5, true)],
            [true, false, false, true],
            &[],
        ),
        (
            "turning a member off turns no other on",
            none,
            "/OFF [6 0 R 7 0 R] /RBGroups [[5 0 R 6 0 R 7 0 R]]",
            "",
            None,
            None,
            &[(5, false)],
            [false, false, false, true],
            &[],
        ),
        (
            "a group in two radio-button groups, turned on",
            none,
            "/RBGroups [[5 0 R 6 0 R] [6 0 R 7 0 R]]",
            "",
            None,
            None,
            &[(6, true)],
            [false, true, false, true],
            &[],
        ),
        (
            "a member of one of the two, turned on",
            none,
            "/RBGroups [[5 0 R 6 0 R] [6 0 R 7 0 R]]",
            "",
            None,
            None,
            &[(5, true)],
            [true, false, true, true],
            &[],
        ),
        (
            "a collection reached by reference, a member listed twice",
            none,
            "/RBGroups [10 0 R]",
            "",
            None,
            None,
            &[(7, true)],
            [false, false, true, true],
            &[],
        ),
        (
            "a configuration whose /ON turns two members on shows both",
            none,
            "",
            "<< /BaseState /OFF /ON [5 0 R 6 0 R] /RBGroups [[5 0 R 6 0 R]] >>",
            Some(0),
            None,
            &[(7, true)],
            [true, true, true, false],
            &[],
        ),
        (
            "until an override turns one of them on",
            none,
            "",
            "<< /BaseState /OFF /ON [5 0 R 6 0 R] /RBGroups [[5 0 R 6 0 R]] >>",
            Some(0),
            None,
            &[(6, true)],
            [false, true, false, false],
            &[],
        ),
        (
            "a locked group overridden on",
            none,
            "/OFF [6 0 R] /Locked [6 0 R]",
            "",
            None,
            None,
            &[(6, true)],
            [true, false, true, true],
            &["B"],
        ),
        (
            "a locked group overridden to the state it has",
            none,
            "/OFF [6 0 R] /Locked [6 0 R]",
            "",
            None,
            None,
            &[(6, false)],
            [true, false, true, true],
            &["B"],
        ),
        (
            "a locked group overridden twice, beside another override",
            none,
            "/OFF [6 0 R] /Locked [6 0 R]",
            "",
            None,
            None,
            &[(6, true), (5, false), (6, false)],
            [false, false, true, true],
            &["B"],
        ),
        (
            "a locked member of a radio-button group stays on",
            none,
            "/RBGroups [[5 0 R 6 0 R 7 0 R]] /Locked [6 0 R]",
            "",
            None,
            None,
            &[(5, true)],
            [true, true, false, true],
            &["B"],
        ),
        (
            "a locked member already off is no change",
            none,
            "/OFF [6 0 R] /RBGroups [[5 0 R 6 0 R]] /Locked [6 0 R]",
            "",
            None,
            None,
            &[(5, true)],
            [true, false, true, true],
            &[],
        ),
        (
            "two locked groups, each reported once in the order met",
            none,
            "/RBGroups [[5 0 R 6 0 R 7 0 R]] /Locked [7 0 R 6 0 R]",
            "",
            None,
            None,
            &[(7, false), (5, true), (6, false)],
            [true, true, true, true],
            &["C", "B"],
        ),
        (
            "a locked group with no /Name is reported by its reference",
            none,
            "/Locked [9 0 R]",
            "",
            None,
            None,
            &[(9, false)],
            [true, true, true, true],
            &["9 0 R"],
        ),
        (
            "an unlisted group",
            none,
            "",
            "",
            None,
            None,
            &[(8, false)],
            [true, true, true, true],
            &[],
        ),
        (
            "an unlisted group a configuration locks",
            none,
            "/Locked [8 0 R]",
            "",
            None,
            None,
            &[(8, true)],
            [true, true, true, true],
            &[],
        ),
        (
            "a reference to no object",
            none,
            "/RBGroups [[5 0 R 99 0 R]]",
            "",
            None,
            None,
            &[(99, true), (5, true)],
            [true, true, true, true],
            &[],
        ),
        (
            "a configuration takes /D's radio buttons and not its locks",
            none,
            "/RBGroups [[5 0 R 6 0 R]] /Locked [7 0 R]",
            inherits,
            Some(0),
            None,
            &[(5, true), (7, false)],
            [true, false, false, true],
            &[],
        ),
        (
            "its own empty radio buttons and its own locks",
            none,
            "/RBGroups [[5 0 R 6 0 R]] /Locked [7 0 R]",
            inherits,
            Some(1),
            None,
            &[(5, false), (6, true)],
            [true, true, true, true],
            &["A"],
        ),
        (
            "and /D's own, under /D",
            none,
            "/RBGroups [[5 0 R 6 0 R]] /Locked [7 0 R]",
            inherits,
            None,
            None,
            &[(5, true), (7, false)],
            [true, false, true, true],
            &["C"],
        ),
        (
            "an unreadable configuration takes /D's locks",
            none,
            "/Locked [7 0 R]",
            inherits,
            Some(7),
            None,
            &[(7, false)],
            [true, true, true, true],
            &["C"],
        ),
        (
            "a /RBGroups and a /Locked that are no arrays",
            none,
            "/RBGroups 5 /Locked (x)",
            "",
            None,
            None,
            &[(5, true), (6, false)],
            [true, false, true, true],
            &[],
        ),
        (
            "radio-button entries that are no collections",
            none,
            "/RBGroups [5 0 R [5 0 R] (x) [6 0 R 6 0 R]]",
            "",
            None,
            None,
            &[(5, true), (6, true)],
            [true, true, true, true],
            &[],
        ),
        (
            "an override after the auto-state, in print",
            print_off,
            "/AS [<< /Event /Print /Category [/Print] /OCGs [5 0 R] >>]",
            "",
            None,
            Some(Usage::Print),
            &[(5, true)],
            [true, true, true, true],
            &[],
        ),
        (
            "the auto-state with no override, in print",
            print_off,
            "/AS [<< /Event /Print /Category [/Print] /OCGs [5 0 R] >>]",
            "",
            None,
            Some(Usage::Print),
            &[(6, false)],
            [false, false, true, true],
            &[],
        ),
    ];
    for (what, groups, d, configs, configuration, usage, overrides, drawn, locked) in cases {
        let mut bytes = radio(*groups, d, configs);
        // Object 10, the collection `/RBGroups [10 0 R]` reaches by
        // reference, C listed in it twice.
        let at = bytes
            .windows(b"trailer".len())
            .position(|window| window == b"trailer")
            .expect("the trailer");
        bytes.splice(
            at..at,
            b"10 0 obj\n[7 0 R 5 0 R 6 0 R 7 0 R]\nendobj\n"
                .iter()
                .copied(),
        );
        let document = Document::open(bytes).expect("it opens");
        let page = document.page(0).expect("a page");
        let twin = render_for(&radio(none, &radio_d(*drawn), ""), None);
        let options = overriding(*configuration, *usage, overrides);
        // A configuration the listing holds no dictionary at renders as `/D`
        // and is reported first.
        let listing = document.layer_configurations();
        let mut more: Vec<RenderWarning> = configuration
            .filter(|index| !listing.get(*index).is_some_and(Option::is_some))
            .map(|index| RenderWarning::UnreadableLayerConfiguration { index })
            .into_iter()
            .collect();
        more.extend(locked.iter().map(|layer| RenderWarning::LockedLayer {
            layer: (*layer).to_string(),
        }));
        same_render_and(
            &page.render(&options),
            &twin,
            &more,
            &format!("{what}, directly"),
        );
        same_render_and(
            &page
                .display_list_with(*usage, &options.layers)
                .render(&options),
            &twin,
            &more,
            &format!("{what}, from its own list"),
        );
        same_render_and(
            &page.display_list_for(*usage).render(&options),
            &twin,
            &more,
            &format!("{what}, from a list under /D"),
        );
    }
}

/// **An override reaches every scope a render opens** (RC-14.3): the one
/// binding of the render's states, overrides and all, is what a form's own
/// content, a form whose `/OC` names the group, a tiling cell and an
/// annotation's appearance read, so each hides where an override turns the
/// group off and draws where one turns it on, as the page whose `/D` says so.
#[test]
fn an_override_reaches_every_scope_a_render_opens() {
    for (d, on, twin) in [
        ("/ON [5 0 R]", false, "/OFF [5 0 R]"),
        ("/OFF [5 0 R]", true, "/ON [5 0 R]"),
    ] {
        let document = Document::open(every_scope_page(d)).expect("it opens");
        let page = document.page(0).expect("a page");
        let options = overriding(None, None, &[(5, on)]);
        let got = page.render(&options);
        for (what, x, y) in EVERY_SCOPE {
            let want = if on {
                vec![255, 0, 0]
            } else {
                vec![255, 255, 255]
            };
            assert_eq!(pixel(&got, x, y), want, "{what}: overridden {on}");
        }
        let twin = render_for(&every_scope_page(twin), None);
        same_render(&got, &twin, &format!("every scope overridden {on}"));
        same_render(
            &page.display_list().render(&options),
            &twin,
            &format!("every scope overridden {on}, from a list under /D"),
        );
    }
}

// ---- The zoom, language and user categories (RC-14.4) -------------------------

/// The options rendering for `usage` at `magnification`, stating `language`
/// and `user` and nothing else of the layers.
fn stating(
    usage: Option<Usage>,
    magnification: f64,
    language: Option<&str>,
    user: &LayerUser,
) -> RenderOptions {
    RenderOptions {
        usage,
        magnification,
        layers: LayerOptions {
            language: language.map(str::to_string),
            user: user.clone(),
            ..LayerOptions::default()
        },
        ..RenderOptions::default()
    }
}

/// A user stating each kind of identification given.
fn user(name: Option<&str>, title: Option<&str>, organisation: Option<&str>) -> LayerUser {
    LayerUser {
        name: name.map(str::to_string),
        title: title.map(str::to_string),
        organisation: organisation.map(str::to_string),
    }
}

/// Holds [`configured`]'s page `bytes`, rendered with `options`, to its twin,
/// the page whose `/D` writes A, B and C as `states` and says nothing of
/// usage: directly; from a list recorded for the usage at magnification 1
/// under `/D`, and from one recorded under the render's own layer options,
/// each replayed only where its states are the render's; in bands, from the
/// list a render at its magnification records; and in its analysis.
#[track_caller]
fn renders_as_states(bytes: &[u8], options: &RenderOptions, states: [bool; 3], what: &str) {
    let document = Document::open(bytes.to_vec()).expect("it opens");
    let page = document.page(0).expect("a page");
    let twin_bytes = configured(["", "", ""], &d_of(states), "");
    let twin = render_for(&twin_bytes, None);
    same_render(&page.render(options), &twin, &format!("{what}, directly"));
    same_render(
        &page.display_list_for(options.usage).render(options),
        &twin,
        &format!("{what}, from a list at magnification 1"),
    );
    same_render(
        &page
            .display_list_with(options.usage, &options.layers)
            .render(options),
        &twin,
        &format!("{what}, from a list under its layer options"),
    );
    same_render(
        &page.render(&RenderOptions {
            parallelism: Parallelism::Threads(2),
            ..options.clone()
        }),
        &twin,
        &format!("{what}, in bands"),
    );
    let twin_document = Document::open(twin_bytes).expect("the twin opens");
    assert_eq!(
        page.analysis(options),
        twin_document
            .page(0)
            .expect("a page")
            .analysis(&RenderOptions::default()),
        "{what}: its analysis"
    );
}

/// **Each auto-state category renders as its `/D` twin** (RC-14.4, 8.11.4.4,
/// `docs/design/optional-content-config.md`): for every event an `/AS` entry
/// names and every usage a render names, groups A, B and C under an entry
/// whose `/Category` is `/Zoom`, `/Language` or `/User` render as the page
/// whose `/D` writes the states the clause gives — the entry's where its
/// event is the render's usage, `/D`'s own, A and C on and B off, elsewhere
/// and with no usage.
///
/// `/Zoom`: A for `/min 1 /max 2`, B for `/min 2`, C for `/max 1`, at
/// magnifications below, at and above each bound, and at ones that are no
/// finite number above zero, read as 1. `/Language`: A `en-US`, B `en-GB`
/// preferred and C `fr`, for an exact tag in either case, two partial ones,
/// a partial `fr-CA` that C does not prefer, an unmatched `de`, and an empty
/// tag and none, which state nothing; and beside them a group with no
/// `/Language`, which keeps its state. `/User`: an individual, a title from
/// an array of two and an organisation, matched, unmatched, matched by case
/// alone, and stated in part or not at all. Two categories in one entry are
/// both read. And a usage dictionary this build cannot read leaves its group
/// as configured whatever the render states.
#[test]
fn each_auto_state_category_renders_as_its_d_twin() {
    let usages = [Usage::View, Usage::Print, Usage::Export];
    let initially = [true, false, true];
    let d = |event: Usage, categories: &str| {
        format!(
            "/OFF [6 0 R] /AS [<< /Event /{event:?} /Category [{categories}] \
             /OCGs [5 0 R 6 0 R 7 0 R] >>]"
        )
    };
    let nobody = LayerUser::default();
    let mut cases = 0;
    // Every usage and none, for one page and one set of inputs: the entry's
    // states where its event is the usage, `/D`'s elsewhere.
    let mut each_usage = |bytes: &[u8],
                          event: Usage,
                          magnification: f64,
                          language: Option<&str>,
                          who: &LayerUser,
                          states: [bool; 3],
                          what: &str| {
        for usage in usages.map(Some).into_iter().chain([None]) {
            let expected = if usage == Some(event) {
                states
            } else {
                initially
            };
            renders_as_states(
                bytes,
                &stating(usage, magnification, language, who),
                expected,
                &format!("{what}, a {event:?} entry rendered for {usage:?}"),
            );
            cases += 1;
        }
    };

    let zoom = [
        "/Usage << /Zoom << /min 1 /max 2 >> >>",
        "/Usage << /Zoom << /min 2 >> >>",
        "/Usage << /Zoom << /max 1 >> >>",
    ];
    for event in usages {
        let bytes = configured(zoom, &d(event, "/Zoom"), "");
        for (magnification, states) in [
            (0.5, [false, false, true]),
            (1.0, [true, false, false]),
            (1.5, [true, false, false]),
            (2.0, [false, true, false]),
            (3.0, [false, true, false]),
            (0.0, [true, false, false]),
            (-2.0, [true, false, false]),
            (f64::NAN, [true, false, false]),
            (f64::INFINITY, [true, false, false]),
        ] {
            each_usage(
                &bytes,
                event,
                magnification,
                None,
                &nobody,
                states,
                &format!("zoom at {magnification}"),
            );
        }
    }

    let speaking = [
        "/Usage << /Language << /Lang (en-US) >> >>",
        "/Usage << /Language << /Lang (en-GB) /Preferred /ON >> >>",
        "/Usage << /Language << /Lang (fr) >> >>",
    ];
    let beside = [
        "/Usage << /Language << /Lang (en-US) >> >>",
        "/Usage << /Language << /Lang (de) /Preferred /ON >> >>",
        "",
    ];
    for event in usages {
        let bytes = configured(speaking, &d(event, "/Language"), "");
        for (language, states) in [
            (Some("en-US"), [true, false, false]),
            (Some("EN-us"), [true, false, false]),
            (Some("en-AU"), [false, true, false]),
            (Some("en"), [false, true, false]),
            (Some("fr"), [false, false, true]),
            (Some("fr-CA"), [false, false, false]),
            (Some("de"), [false, false, false]),
            (Some(""), initially),
            (None, initially),
        ] {
            each_usage(
                &bytes,
                event,
                1.0,
                language,
                &nobody,
                states,
                &format!("language {language:?}"),
            );
        }
        let bytes = configured(beside, &d(event, "/Language"), "");
        for (language, states) in [
            (Some("en-US"), [true, false, true]),
            (Some("de-AT"), [false, true, true]),
            (Some("it"), [false, false, true]),
        ] {
            each_usage(
                &bytes,
                event,
                1.0,
                language,
                &nobody,
                states,
                &format!("language {language:?} beside a group with none"),
            );
        }
    }

    let users = [
        "/Usage << /User << /Type /Ind /Name (Ada) >> >>",
        "/Usage << /User << /Type /Ttl /Name [(Editor) (Reviewer)] >> >>",
        "/Usage << /User << /Type /Org /Name (Tinker) >> >>",
    ];
    for event in usages {
        let bytes = configured(users, &d(event, "/User"), "");
        for (who, states) in [
            (
                user(Some("Ada"), Some("Reviewer"), Some("Tinker")),
                [true, true, true],
            ),
            (
                user(Some("Bob"), Some("Author"), Some("Other")),
                [false, false, false],
            ),
            (user(Some("ada"), None, None), [false, false, true]),
            (user(None, Some("Editor"), None), [true, true, true]),
            (user(None, None, Some("tinker")), [true, false, false]),
            (LayerUser::default(), initially),
        ] {
            each_usage(&bytes, event, 1.0, None, &who, states, &format!("{who:?}"));
        }
    }

    // Two categories in one entry: A on only where both recommend it.
    let both = [
        "/Usage << /Zoom << /min 1 /max 2 >> /Language << /Lang (en) >> >>",
        "",
        "",
    ];
    for event in usages {
        let bytes = configured(both, &d(event, "/Zoom /Language"), "");
        for (magnification, language, states) in [
            (1.5, Some("en"), [true, false, true]),
            (3.0, Some("en"), [false, false, true]),
            (1.5, Some("fr"), [false, false, true]),
            (1.5, None, [true, false, true]),
        ] {
            each_usage(
                &bytes,
                event,
                magnification,
                language,
                &nobody,
                states,
                &format!("zoom at {magnification} and language {language:?}"),
            );
        }
    }

    // A usage dictionary this build cannot read leaves its group as
    // configured, whatever the render states.
    let everyone = user(Some("Ada"), Some("Editor"), Some("Tinker"));
    for malformed in [
        [
            "/Usage << /Zoom << /min (x) >> >>",
            "/Usage << /Zoom 5 >>",
            "/Usage << /Zoom << /max /Infinity >> >>",
        ],
        [
            "/Usage << /Language << /Lang 5 >> >>",
            "/Usage << /Language << /Lang (en) /Preferred /Yes >> >>",
            "/Usage << /Language (en) >>",
        ],
        [
            "/Usage << /User << /Type /Person /Name (Ada) >> >>",
            "/Usage << /User << /Type /Ttl /Name 5 >> >>",
            "/Usage << /User << /Type /Org /Name [(Tinker) 5] >> >>",
        ],
        [
            "/Usage << /User << /Name (Ada) >> >>",
            "/Usage << /Language << /Preferred /ON >> >>",
            "/Usage << /Zoom (wide) >>",
        ],
    ] {
        for event in usages {
            let bytes = configured(malformed, &d(event, "/Zoom /Language /User"), "");
            each_usage(
                &bytes,
                event,
                1.5,
                Some("en"),
                &everyone,
                initially,
                &format!("{malformed:?}"),
            );
        }
    }
    assert_eq!(
        cases,
        4 * 3 * (9 + 9 + 3 + 6 + 4 + 4),
        "every case ran, for every usage and none"
    );
}

/// **A form's kept drawing is keyed by the magnification its zoom reads**
/// (RC-14.4): a document with a cache keeps a form's recorded drawing across
/// renders, and a form whose content is marked by a group a view `/Zoom`
/// entry decides is drawn at magnification 1, 3 and 1 again in turn, each as
/// its `/D` twin draws it — never with the drawing another magnification
/// kept — while a document whose states read no `/Zoom` keeps one drawing for
/// every magnification.
#[test]
fn a_forms_kept_drawing_is_keyed_by_the_magnification_its_zoom_reads() {
    let form_page = |d: &str| {
        let marked = "/OC /A BDC 1 0 0 rg 0 0 10 40 re f EMC";
        let content = "0 1 0 rg 0 0 10 40 re f q 1 0 0 1 20 0 cm /Fm Do Q";
        format!(
            "%PDF-1.7\n\
1 0 obj\n<< /Type /Catalog /Pages 2 0 R /OCProperties << /OCGs [5 0 R] /D << {d} >> >> >>\nendobj\n\
2 0 obj\n<< /Type /Pages /Count 1 /Kids [3 0 R] >>\nendobj\n\
3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 40 40]\n\
   /Resources << /XObject << /Fm 6 0 R >> >> /Contents 4 0 R >>\nendobj\n\
4 0 obj\n<< /Length {} >>\nstream\n{content}\nendstream\nendobj\n\
5 0 obj\n<< /Type /OCG /Name (A) /Usage << /Zoom << /max 2 >> >> >>\nendobj\n\
6 0 obj\n<< /Type /XObject /Subtype /Form /BBox [0 0 10 40] \
/Resources << /Properties << /A 5 0 R >> >> /Length {} >>\nstream\n{marked}\nendstream\nendobj\n\
trailer\n<< /Size 7 /Root 1 0 R >>\n%%EOF\n",
            content.len(),
            marked.len()
        )
        .into_bytes()
    };
    let zooming = "/AS [<< /Event /View /Category [/Zoom] /OCGs [5 0 R] >>]";
    let document = Document::open(form_page(zooming))
        .expect("it opens")
        .with_cache_limits(CacheLimits::new(1 << 20));
    let page = document.page(0).expect("a page");
    let shown = render_for(&form_page("/ON [5 0 R]"), None);
    let hidden = render_for(&form_page("/OFF [5 0 R]"), None);
    assert!(
        shown.data != hidden.data,
        "the form's band is the difference"
    );
    for (magnification, twin) in [(1.0, &shown), (3.0, &hidden), (1.0, &shown), (3.0, &hidden)] {
        same_render(
            &page.render(&stating(
                Some(Usage::View),
                magnification,
                None,
                &LayerUser::default(),
            )),
            twin,
            &format!("the form at magnification {magnification}"),
        );
    }
    let still = Document::open(form_page("/ON [5 0 R]"))
        .expect("it opens")
        .with_cache_limits(CacheLimits::new(1 << 20));
    let page = still.page(0).expect("a page");
    for magnification in [1.0, 3.0, 0.5] {
        same_render(
            &page.render(&stating(
                Some(Usage::View),
                magnification,
                None,
                &LayerUser::default(),
            )),
            &shown,
            &format!("no zoom read, at magnification {magnification}"),
        );
    }
}
