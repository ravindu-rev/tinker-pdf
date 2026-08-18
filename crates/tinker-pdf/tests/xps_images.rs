//! `ImageBrush`, its two rectangles, its five tile modes and the two formats
//! this build refuses (gap 30, milestone 8).
//!
//! # Why the route is asserted and not only the picture
//!
//! Gap 29 milestone 4 proved the pass-through and the decode produce one
//! picture, and gap 29 milestone 6 then found the consequence: injecting "an
//! indexed file decoded instead of passed through" moves a byte hash and three
//! route assertions **while every rendered comparison still passes**. The whole
//! memory argument of both gaps — that a page costs a multiple of the part
//! rather than *w × h × 3* — is therefore invisible to any test that looks at
//! pixels. So the tests below read `PngRoute` where that is the claim.
//!
//! # The pairs
//!
//! Five milestones running have found the same defect shape, stated in gap 30
//! milestone 5's progress section as one rule: *when a thing has two
//! independent consequences, a test for one of them is not a test.* This file
//! has four such pairs and each gets two tests:
//!
//! - `Viewbox` and `Viewport` are two rectangles in two spaces, and swapping
//!   them is a defect no single-rectangle assertion sees.
//! - `ViewboxUnits` and `ViewportUnits` are two attributes with one grammar.
//! - the content type and the magic bytes are two independent ways to know a
//!   part is a TIFF, and gap 30 milestone 3's survivor was exactly this shape.
//! - a refused image has two consequences: it is named, **and** the rest of the
//!   page still draws.

mod xps_support;

use tinker_pdf::{
    ArchiveWarning, Document, RenderOptions, WriteMode, WriteOptions, XpsElementDefect,
};
use xps_support::{
    archive, before_content_types, binary_part, content_types_with, grey_jpeg, one_page_package,
    rgb_png, with, Part, XPS_NS,
};

/// The resource-dictionary key namespace, which every real package binds.
const KEY_NS: &str = "http://schemas.microsoft.com/xps/2005/06/resourcedictionary-key";

/// A package whose one page fills a rectangle with an `ImageBrush`.
///
/// `attributes` goes on the brush verbatim, which is what lets one helper serve
/// the viewbox tests, the units tests and the five tile modes.
fn package_with(image: Part, attributes: &str, types: Option<&str>) -> Vec<u8> {
    let body = format!(
        r#"<Path Data="M0,0L200,0 200,200 0,200Z"><Path.Fill>
             <ImageBrush ImageSource="/Resources/i.png" {attributes} />
           </Path.Fill></Path>"#
    );
    let markup = format!(
        r#"<FixedPage xmlns="{XPS_NS}" xmlns:x="{KEY_NS}" Width="816" Height="1056">{body}</FixedPage>"#
    );
    let mut parts = with(one_page_package(), "Documents/1/Pages/1.fpage", &markup);
    if let Some(types) = types {
        parts = with(parts, "[Content_Types].xml", types);
    }
    archive(before_content_types(parts, image))
}

/// A 4 × 2 PNG, which passes through, and its part.
fn png_part() -> Part {
    let pixels: Vec<u8> = (0..4 * 2 * 3).map(|i| i as u8).collect();
    binary_part("Resources/i.png", rgb_png(4, 2, &pixels))
}

/// Every element-level defect a package reported.
fn defects(bytes: &[u8]) -> Vec<XpsElementDefect> {
    let document = Document::open(bytes.to_vec()).expect("an XPS");
    document
        .archive()
        .expect("a synthesised document")
        .warnings()
        .iter()
        .filter_map(|w| match w {
            ArchiveWarning::XpsElement { defect, .. } => Some(*defect),
            _ => None,
        })
        .collect()
}

/// The one page's content stream, as text.
fn stream(bytes: &[u8]) -> String {
    let document = Document::open(bytes.to_vec()).expect("an XPS");
    let cos = document.cos();
    let pages = tinker_pdf_cos::pages::collect(cos);
    let page = pages.first().expect("one page");
    String::from_utf8_lossy(&tinker_pdf_cos::pages::content_bytes(cos, page)).into_owned()
}

/// The synthesised document, saved, as text.
///
/// `WriteOptions::default()` leaves compression off — milestone 4 recorded
/// that — so every dictionary is legible in the bytes. Reading the *saved* file
/// rather than the builder's own tables is deliberate: it is the same thing
/// qpdf reads, so an assertion here and an assertion there are about one
/// artefact.
fn saved(bytes: &[u8]) -> String {
    let document = Document::open(bytes.to_vec()).expect("an XPS");
    let out = document.editor().save(&WriteOptions {
        mode: WriteMode::Rewrite,
        ..WriteOptions::default()
    });
    String::from_utf8_lossy(&out).into_owned()
}

/// The tiling pattern's dictionary, out of the saved file.
fn pattern(bytes: &[u8]) -> String {
    let text = saved(bytes);
    let at = text
        .find("/PatternType")
        .unwrap_or_else(|| panic!("no tiling pattern was written"));
    let start = text[..at].rfind("<<").unwrap_or(at);
    let end = (at + 400).min(text.len());
    text[start..end].to_string()
}

// ---- the picture arrives -------------------------------------------------

/// An `ImageBrush` fills its shape with a tiling pattern, and the pattern shows
/// the image.
#[test]
fn an_image_brush_becomes_a_pattern_that_shows_the_image() {
    let bytes = package_with(
        png_part(),
        r#"ViewboxUnits="Absolute" ViewportUnits="Absolute"
           Viewbox="0,0,4,2" Viewport="0,0,200,100" TileMode="None""#,
        None,
    );
    assert_eq!(defects(&bytes), [], "nothing is owed");

    let content = stream(&bytes);
    assert!(
        content.contains("/Pattern cs"),
        "the shape is filled with a pattern: {content}"
    );
    assert!(content.contains(" scn"), "{content}");

    let dict = pattern(&bytes);
    assert!(dict.contains("PatternType"), "{dict}");
}

/// A PNG reaches the page **through gap 29's pass-through**, not decoded.
///
/// The claim this milestone inherits is that a page's peak cost is a multiple
/// of the part rather than *w × h × 3*, and no rendered comparison can see the
/// difference — gap 29 milestone 6 measured that directly. So the assertion is
/// on the image's `/Filter`, which is what the two routes disagree about: a
/// passed-through IDAT is `/FlateDecode` with a `/DecodeParms` naming
/// `/Predictor 15`, and a decoded one is raw samples.
#[test]
fn a_png_reaches_the_page_as_the_bytes_the_part_holds() {
    let bytes = package_with(
        png_part(),
        r#"ViewboxUnits="Absolute" ViewportUnits="Absolute"
           Viewbox="0,0,4,2" Viewport="0,0,200,100""#,
        None,
    );
    let text = saved(&bytes);
    assert!(
        text.contains("/Subtype /Image"),
        "an image XObject was written"
    );
    assert!(
        text.contains("/Predictor"),
        "the IDAT was decoded rather than passed through: {text}"
    );
}

// ---- the two rectangles, which are two rules -----------------------------

/// `Viewbox` is in the image's units and `Viewport` is in the element's, and
/// the scale between them is the pattern's.
///
/// A 4-unit viewbox into a 200-unit viewport is a factor of fifty; at 18.1's
/// 0.75 that is 37.5 in the pattern's `/Matrix`. A build that read the two
/// rectangles the other way round would produce the reciprocal — 0.0267 — and
/// draw the picture too small to see rather than not at all, which is why the
/// number is asserted and not merely its presence.
#[test]
fn the_viewbox_scales_to_the_viewport_and_not_the_other_way() {
    let bytes = package_with(
        png_part(),
        r#"ViewboxUnits="Absolute" ViewportUnits="Absolute"
           Viewbox="0,0,4,2" Viewport="0,0,200,100""#,
        None,
    );
    let dict = pattern(&bytes);
    assert!(
        dict.contains("37.5"),
        "200 over 4 is 50, and 50 at 0.75 is 37.5: {dict}"
    );
    assert!(
        !dict.contains("0.02"),
        "the reciprocal is the swap, and it is not here: {dict}"
    );
}

/// The two `*Units` attributes are independent, and a relative viewbox is a
/// fraction of the image while a relative viewport is a fraction of the shape.
#[test]
fn the_two_units_attributes_are_read_separately() {
    // A relative viewbox of the whole image, absolute viewport: the same
    // picture as `Viewbox="0,0,4,2"` above, so the same scale.
    let relative_box = package_with(
        png_part(),
        r#"ViewboxUnits="RelativeToBoundingBox" ViewportUnits="Absolute"
           Viewbox="0,0,1,1" Viewport="0,0,200,100""#,
        None,
    );
    assert_eq!(defects(&relative_box), []);
    assert!(
        pattern(&relative_box).contains("37.5"),
        "a relative viewbox of the whole image is the image"
    );

    // And a relative viewport of the whole shape, which is 200 x 200.
    let relative_port = package_with(
        png_part(),
        r#"ViewboxUnits="Absolute" ViewportUnits="RelativeToBoundingBox"
           Viewbox="0,0,4,2" Viewport="0,0,1,1""#,
        None,
    );
    assert_eq!(defects(&relative_port), []);
    let dict = pattern(&relative_port);
    assert!(
        dict.contains("37.5"),
        "200 over 4 across is still 37.5: {dict}"
    );
    assert!(
        dict.contains("75"),
        "and 200 over 2 down is 100, which is 75 at 0.75: {dict}"
    );
}

// ---- the five tile modes, which are five rules ---------------------------

/// `TileMode="None"` draws the picture once, and PDF has no such pattern — so
/// the step is made larger than any page and the shape does the clipping.
#[test]
fn tile_mode_none_puts_one_picture_on_the_page() {
    let bytes = package_with(
        png_part(),
        r#"ViewboxUnits="Absolute" ViewportUnits="Absolute"
           Viewbox="0,0,4,2" Viewport="0,0,200,100" TileMode="None""#,
        None,
    );
    let dict = pattern(&bytes);
    assert!(
        dict.contains("1000000"),
        "the step is larger than any page: {dict}"
    );
}

/// `TileMode="Tile"` steps by the cell, so the picture repeats.
#[test]
fn tile_mode_tile_steps_by_the_cell() {
    let bytes = package_with(
        png_part(),
        r#"ViewboxUnits="Absolute" ViewportUnits="Absolute"
           Viewbox="0,0,4,2" Viewport="0,0,200,100" TileMode="Tile""#,
        None,
    );
    let dict = pattern(&bytes);
    assert!(!dict.contains("1000000"), "a tiling brush tiles: {dict}");
}

/// The three flips have no PDF equivalent, so the **cell** carries the
/// reflection — twice the image wide, tall, or both.
///
/// Asserted on the cell's own `/BBox`, because that is the only place the
/// difference between the three appears: all three step by their cell and all
/// three show the same image, and a test that looked at either would call the
/// three one rule.
#[test]
fn each_flip_makes_the_cell_large_enough_to_hold_its_reflections() {
    let cases = [
        ("Tile", "4", "2"),
        ("FlipX", "8", "2"),
        ("FlipY", "4", "4"),
        ("FlipXY", "8", "4"),
    ];
    for (mode, wide, tall) in cases {
        let bytes = package_with(
            png_part(),
            &format!(
                r#"ViewboxUnits="Absolute" ViewportUnits="Absolute"
                   Viewbox="0,0,4,2" Viewport="0,0,200,100" TileMode="{mode}""#
            ),
            None,
        );
        let dict = pattern(&bytes);
        let wanted = format!("/BBox [0 0 {wide} {tall}]");
        assert!(
            dict.contains(&wanted),
            "{mode}: the cell is {wide} x {tall} image units: {dict}"
        );
    }
}

// ---- the two ways to know a part is a TIFF -------------------------------

/// A TIFF **the content type names** is refused, and the rest of the page still
/// draws.
///
/// Two assertions and they are two consequences of one refusal: the picture is
/// named, and the `Path` that wanted it is still on the page in the placeholder
/// grey. Gap 30 milestone 3's survivor was a clause with two consequences
/// tested on one side, so both are here.
#[test]
fn a_tiff_named_by_its_content_type_is_refused_and_the_page_draws() {
    // The bytes say PNG; only the content type says TIFF.
    let types =
        content_types_with(r#"<Override PartName="/Resources/i.png" ContentType="image/tiff" />"#);
    let bytes = package_with(
        png_part(),
        r#"Viewbox="0,0,4,2" Viewport="0,0,200,100"
        ViewboxUnits="Absolute" ViewportUnits="Absolute""#,
        Some(&types),
    );

    assert_eq!(defects(&bytes), [XpsElementDefect::ImageFormatUnsupported]);
    let content = stream(&bytes);
    assert!(
        content.contains("0.749 0.749 0.749 rg"),
        "the shape is grey: {content}"
    );
    assert!(
        content.contains("200 0 l"),
        "and the shape is still drawn: {content}"
    );
}

/// A TIFF **the bytes say** is refused, whatever the content type claims.
#[test]
fn a_tiff_named_by_its_magic_bytes_is_refused_and_the_page_draws() {
    // The content type says PNG; only the bytes say TIFF.
    let mut tiff = b"II\x2A\x00".to_vec();
    tiff.extend_from_slice(&[0u8; 32]);
    let bytes = package_with(
        binary_part("Resources/i.png", tiff),
        r#"Viewbox="0,0,4,2" Viewport="0,0,200,100"
           ViewboxUnits="Absolute" ViewportUnits="Absolute""#,
        None,
    );

    assert_eq!(defects(&bytes), [XpsElementDefect::ImageFormatUnsupported]);
    let content = stream(&bytes);
    assert!(content.contains("0.749 0.749 0.749 rg"), "{content}");
    assert!(content.contains("200 0 l"), "{content}");
}

/// A JPEG XR, which 9.1.5.1 recommends and nothing outside Microsoft's stack
/// implements, is refused by the same name.
#[test]
fn a_jpeg_xr_is_refused_by_name() {
    let mut jxr = vec![0x49, 0x49, 0xBC, 0x01];
    jxr.extend_from_slice(&[0u8; 32]);
    let bytes = package_with(
        binary_part("Resources/i.png", jxr),
        r#"Viewbox="0,0,4,2" Viewport="0,0,200,100"
           ViewboxUnits="Absolute" ViewportUnits="Absolute""#,
        None,
    );
    assert_eq!(defects(&bytes), [XpsElementDefect::ImageFormatUnsupported]);
}

// ---- JPEG, and the resolution --------------------------------------------

/// A JPEG is placed verbatim, through gap 29's own `ImageData::Jpeg`.
#[test]
fn a_jpeg_part_is_placed_verbatim() {
    let types =
        content_types_with(r#"<Override PartName="/Resources/i.png" ContentType="image/jpeg" />"#);
    let bytes = package_with(
        binary_part("Resources/i.png", grey_jpeg(16, 8)),
        r#"Viewbox="0,0,16,8" Viewport="0,0,200,100"
           ViewboxUnits="Absolute" ViewportUnits="Absolute""#,
        Some(&types),
    );
    assert_eq!(defects(&bytes), []);
    assert!(
        saved(&bytes).contains("/DCTDecode"),
        "the JPEG's own bytes are the stream"
    );
}

// ---- the page still renders ----------------------------------------------

/// And the whole of it reaches a raster: the shape is painted, not blank.
#[test]
fn a_page_with_an_image_brush_draws_ink() {
    let bytes = package_with(
        png_part(),
        r#"ViewboxUnits="Absolute" ViewportUnits="Absolute"
           Viewbox="0,0,4,2" Viewport="0,0,200,100""#,
        None,
    );
    let document = Document::open(bytes).expect("an XPS");
    let bitmap = document
        .page(0)
        .expect("a page")
        .render(&RenderOptions::default());
    let ink = bitmap
        .data
        .chunks_exact(3)
        .filter(|px| *px != [0xFF, 0xFF, 0xFF])
        .count();
    assert!(ink > 1_000, "the picture is on the page: {ink} pixels");
}

/// A `VisualBrush` is refused **by name**, and the plan's row 8 is amended
/// rather than claimed.
///
/// Its cell is a subtree of markup rather than a part, so painting one means
/// re-entering the drawing walk from inside a brush and carrying 18.2's
/// cross-part depth with it. That is a milestone's worth of work on its own and
/// it is not done, so the brush says so and the shape keeps the grey — which is
/// the same answer every other unpainted brush gets, rather than a picture the
/// file never described.
#[test]
fn a_visual_brush_is_refused_by_name_and_the_shape_survives() {
    let body = r##"<Path Data="M0,0L200,0 200,200 0,200Z"><Path.Fill>
        <VisualBrush Viewbox="0,0,1,1" Viewport="0,0,1,1">
          <VisualBrush.Visual><Path Data="M0,0L1,0Z" Fill="#FF00FF00" /></VisualBrush.Visual>
        </VisualBrush></Path.Fill></Path>"##;
    let markup = format!(
        r#"<FixedPage xmlns="{XPS_NS}" xmlns:x="{KEY_NS}" Width="816" Height="1056">{body}</FixedPage>"#
    );
    let bytes = archive(with(
        one_page_package(),
        "Documents/1/Pages/1.fpage",
        &markup,
    ));
    assert_eq!(defects(&bytes), [XpsElementDefect::BrushUnsupported]);
    let content = stream(&bytes);
    assert!(content.contains("0.749 0.749 0.749 rg"), "{content}");
    assert!(content.contains("200 0 l"), "{content}");
}
