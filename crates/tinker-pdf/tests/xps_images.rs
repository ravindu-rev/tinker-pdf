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

/// The 4 x 2 PNG's bytes, without a `pHYs` chunk.
fn png_bytes() -> Vec<u8> {
    let pixels: Vec<u8> = (0..4 * 2 * 3).map(|i| i as u8).collect();
    rgb_png(4, 2, &pixels)
}

/// The same PNG with a `pHYs` chunk stating pixels per metre (11.3.5.3).
///
/// Inserted immediately after `IHDR`, which is where the chunk has to be: 5.6
/// makes `pHYs` an ancillary chunk that precedes the first `IDAT`, and a reader
/// that scanned past the image data for it would not find one there.
fn with_phys(png: Vec<u8>, x: u32, y: u32) -> Vec<u8> {
    let mut data = Vec::new();
    data.extend_from_slice(&x.to_be_bytes());
    data.extend_from_slice(&y.to_be_bytes());
    data.push(1); // the unit is the metre
    let mut chunk = Vec::new();
    chunk.extend_from_slice(&(data.len() as u32).to_be_bytes());
    chunk.extend_from_slice(b"pHYs");
    chunk.extend_from_slice(&data);
    let mut crc_over = b"pHYs".to_vec();
    crc_over.extend_from_slice(&data);
    chunk.extend_from_slice(&tinker_pdf_filters::crc32(&crc_over).to_be_bytes());

    // 8 bytes of signature, then IHDR: 4 length + 4 tag + 13 data + 4 CRC.
    let at = 8 + 4 + 4 + 13 + 4;
    let mut out = png[..at].to_vec();
    out.extend_from_slice(&chunk);
    out.extend_from_slice(&png[at..]);
    out
}

/// The horizontal scale out of a pattern's `/Matrix`.
fn scale(dict: &str) -> f64 {
    let at = dict.find("/Matrix [").expect("a matrix");
    dict[at + 9..]
        .split_whitespace()
        .next()
        .and_then(|n| n.parse().ok())
        .expect("a number")
}

/// The tiling pattern's own content stream, out of the saved file.
fn cell_stream(saved: &str) -> String {
    let at = saved
        .find("/PatternType")
        .unwrap_or_else(|| panic!("no tiling pattern"));
    let stream = saved[at..]
        .find("stream")
        .unwrap_or_else(|| panic!("the pattern has no stream"));
    let from = at + stream + "stream".len();
    let end = saved[from..]
        .find("endstream")
        .unwrap_or_else(|| panic!("unterminated"));
    saved[from..from + end].to_string()
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

/// The cell **draws** its reflections, and each sits beside the original rather
/// than on top of it.
///
/// The `/BBox` test above says the cell is large enough; this says something is
/// in it. They are two rules and the injection matrix proved it: deleting the
/// mirrored copies, and drawing them at the original's own origin, both left
/// that test passing. A cell twice the image wide with one copy in it tiles
/// with a gap; a cell with two copies stacked in the same place tiles with a
/// gap and a double exposure. Neither is a flip.
///
/// The count is the number of `Do` operators and the placement is the `cm`
/// before each: a mirrored copy has a negative scale and an origin on the far
/// side of the cell, which is the only arrangement that reflects rather than
/// translates.
#[test]
fn each_flip_draws_its_copies_and_puts_them_beside_the_original() {
    let cases = [("Tile", 1), ("FlipX", 2), ("FlipY", 2), ("FlipXY", 4)];
    for (mode, copies) in cases {
        let bytes = package_with(
            png_part(),
            &format!(
                r#"ViewboxUnits="Absolute" ViewportUnits="Absolute"
                   Viewbox="0,0,4,2" Viewport="0,0,200,100" TileMode="{mode}""#
            ),
            None,
        );
        let text = saved(&bytes);
        let cell = cell_stream(&text);
        assert_eq!(
            cell.matches(" Do").count(),
            copies,
            "{mode}: {copies} copies of the image: {cell}"
        );
        if mode != "Tile" {
            assert!(
                cell.contains("-4 0 0 2") || cell.contains("4 0 0 -2"),
                "{mode}: a copy is mirrored: {cell}"
            );
            // 8 across for FlipX and FlipXY, 4 down for FlipY and FlipXY: the
            // mirrored copy starts on the far side and draws back.
            let far = if mode == "FlipY" { " 0 4" } else { " 8 0" };
            assert!(
                cell.contains(far) || cell.contains("8 4"),
                "{mode}: the mirror sits beside the original, at {far}: {cell}"
            );
        }
    }
}

/// 13.4.1's resolution decides how large an image is in XPS units, and 96 dpi
/// is only the **default**.
///
/// The resolution is invisible to an **absolute** viewbox — that one is already
/// stated in image units, and the scale is viewport over viewbox with no
/// picture size in it. It is a *relative* viewbox that asks how big the image
/// is, because `Viewbox="0,0,1,1"` means "all of it". A 4 × 2 pixel PNG is four
/// units across at 96 dpi and two at 192, so the same relative viewbox names a
/// different extent and the scale into a 200-unit viewport doubles.
///
/// This distinction is why the injection matrix could report "13.4.1's
/// resolution ignored" as surviving while three tests exercised viewboxes: all
/// three were absolute, and no absolute viewbox can see it.
#[test]
fn an_images_own_resolution_decides_its_size_in_units() {
    // 192 dpi, as pixels per metre: 192 / 0.0254.
    let dense = with_phys(png_bytes(), 7559, 7559);
    let relative = r#"ViewboxUnits="RelativeToBoundingBox" ViewportUnits="Absolute"
                      Viewbox="0,0,1,1" Viewport="0,0,200,100""#;

    let at_192 = package_with(binary_part("Resources/i.png", dense), relative, None);
    assert_eq!(defects(&at_192), []);
    let at_96 = package_with(png_part(), relative, None);
    assert_eq!(defects(&at_96), []);

    // 96 dpi: the image is 4 x 2 units, so 200 over 4 is 50, and 50 at 18.1's
    // 0.75 is 37.5. 192 dpi: the same pixels are 2 x 1 units, so 75.
    //
    // Compared with a tolerance rather than as text, and the reason is in the
    // format: `pHYs` states **integral pixels per metre**, so 192 dots to the
    // inch is 7559.055... and not a number the chunk can hold. The nearest it
    // has is 7559, which is 191.9986 dpi and comes out as 74.999453. A test
    // asserting the string "75" would fail on a correct decode, which is the
    // shape gap 18's own oracle comparison had.
    assert!(
        (scale(&pattern(&at_96)) - 37.5).abs() < 0.01,
        "at 96 dpi the image is four units across: {}",
        pattern(&at_96)
    );
    assert!(
        (scale(&pattern(&at_192)) - 75.0).abs() < 0.01,
        "at 192 dpi the same pixels are two units across: {}",
        pattern(&at_192)
    );
    assert_ne!(
        pattern(&at_96),
        pattern(&at_192),
        "one `pHYs` chunk is the whole difference"
    );
}

/// An unknown `TileMode` is refused rather than taken for `None`.
///
/// 15.3.1 gives five spellings and a sixth is not one of them. Taking an
/// unrecognised value for `None` would draw one picture where the file asked
/// for something this build does not know about — plausible, and wrong in a way
/// nobody would look for.
#[test]
fn an_unknown_tile_mode_is_refused_rather_than_taken_for_none() {
    let bytes = package_with(
        png_part(),
        r#"ViewboxUnits="Absolute" ViewportUnits="Absolute"
           Viewbox="0,0,4,2" Viewport="0,0,200,100" TileMode="FlipZ""#,
        None,
    );
    assert_eq!(defects(&bytes), [XpsElementDefect::BrushUnreadable]);
}

/// 7.2.3.5's case-insensitive match is what identifies a part whose **bytes**
/// say nothing.
///
/// The injection matrix reported "the content type matched case-sensitively" as
/// surviving even with a test that opened an `IMAGE/PNG` package, and the reason
/// is worth keeping: every one of 9.1.5's four formats has magic bytes, so a
/// mis-cased content type is rescued by the bytes and nothing is observable. The
/// case that *can* see it is a part whose bytes identify nothing — there the
/// content type is the only witness, and the difference between believing it and
/// not is the difference between two named refusals.
///
/// `ImageUnreadable` means "this is a PNG and it will not decode".
/// `ImageFormatUnsupported` means "nothing here says what this is". A reader
/// deciding whether the package is damaged or merely exotic needs them apart.
#[test]
fn a_mis_cased_content_type_still_identifies_a_part_the_bytes_do_not() {
    let types =
        content_types_with(r#"<Override PartName="/Resources/i.png" ContentType="IMAGE/PNG" />"#);
    let bytes = package_with(
        binary_part("Resources/i.png", vec![0u8; 64]),
        r#"Viewbox="0,0,4,2" Viewport="0,0,200,100"
           ViewboxUnits="Absolute" ViewportUnits="Absolute""#,
        Some(&types),
    );
    assert_eq!(
        defects(&bytes),
        [XpsElementDefect::ImageUnreadable],
        "the content type was believed, and then the bytes were not a PNG"
    );
}

/// A part nothing identifies is refused rather than guessed at.
///
/// Neither a content type this build knows nor magic bytes it recognises: 9.1.5
/// says an image part is one of four formats and this is none of them, so
/// assuming PNG and letting the decoder fail would report the wrong fault.
#[test]
fn a_part_with_no_content_type_and_no_magic_is_refused() {
    // An `Override` with a media type naming nothing, so 7.2.3.5 resolves and
    // the answer is still not an image format.
    let types = content_types_with(
        r#"<Override PartName="/Resources/i.png" ContentType="application/octet-stream" />"#,
    );
    let bytes = package_with(
        binary_part("Resources/i.png", vec![0u8; 64]),
        r#"Viewbox="0,0,4,2" Viewport="0,0,200,100"
           ViewboxUnits="Absolute" ViewportUnits="Absolute""#,
        Some(&types),
    );
    assert_eq!(defects(&bytes), [XpsElementDefect::ImageFormatUnsupported]);
}

/// 7.2.3.5 matches a content type case-insensitively, and a package that
/// shouts is still a package.
#[test]
fn a_content_type_in_another_case_still_names_its_format() {
    let types =
        content_types_with(r#"<Override PartName="/Resources/i.png" ContentType="IMAGE/PNG" />"#);
    let bytes = package_with(
        png_part(),
        r#"Viewbox="0,0,4,2" Viewport="0,0,200,100"
           ViewboxUnits="Absolute" ViewportUnits="Absolute""#,
        Some(&types),
    );
    assert_eq!(defects(&bytes), [], "IMAGE/PNG is image/png");
}

/// An `ImageSource` behind a colour profile is refused by its own name.
///
/// 9.1.5's `{ColorConvertedBitmap ...}` names an ICC profile, which is a
/// non-goal of this whole plan, and the syntax has nowhere to put an sRGB
/// fallback — so drawing the picture unconverted would be colours the file did
/// not ask for, which is the shape gap 18a's plausible photograph had.
#[test]
fn a_colour_converted_bitmap_is_refused_by_its_own_name() {
    let bytes = package_with(
        png_part(),
        r#"Viewbox="0,0,4,2" Viewport="0,0,200,100"
           ViewboxUnits="Absolute" ViewportUnits="Absolute""#,
        None,
    );
    // The helper writes a plain `ImageSource`; this rewrites it in place.
    let text = String::from_utf8_lossy(&bytes).into_owned();
    let _ = text;
    let body = r#"<Path Data="M0,0L200,0 200,200 0,200Z"><Path.Fill>
        <ImageBrush ImageSource="{ColorConvertedBitmap /Resources/i.png /Resources/p.icc}"
                    Viewbox="0,0,4,2" Viewport="0,0,200,100"
                    ViewboxUnits="Absolute" ViewportUnits="Absolute" />
        </Path.Fill></Path>"#;
    let markup = format!(
        r#"<FixedPage xmlns="{XPS_NS}" xmlns:x="{KEY_NS}" Width="816" Height="1056">{body}</FixedPage>"#
    );
    let parts = with(one_page_package(), "Documents/1/Pages/1.fpage", &markup);
    let bytes = archive(before_content_types(parts, png_part()));
    assert_eq!(defects(&bytes), [XpsElementDefect::ImageProfileUnsupported]);
}

/// A viewport with no extent paints nothing, so it is refused at the brush
/// rather than handed to a writer that would refuse the pattern.
///
/// Both rectangles are checked and they are two rules: a zero viewbox names no
/// part of the image and a zero viewport names nowhere to put it.
#[test]
fn a_rectangle_with_no_extent_is_refused_at_the_brush() {
    for (attributes, what) in [
        (r#"Viewbox="0,0,0,2" Viewport="0,0,200,100""#, "viewbox"),
        (r#"Viewbox="0,0,4,2" Viewport="0,0,0,100""#, "viewport"),
        (
            r#"Viewbox="0,0,4,2" Viewport="0,0,200,0""#,
            "viewport height",
        ),
    ] {
        let bytes = package_with(
            png_part(),
            &format!(r#"{attributes} ViewboxUnits="Absolute" ViewportUnits="Absolute""#),
            None,
        );
        assert_eq!(
            defects(&bytes),
            [XpsElementDefect::BrushUnreadable],
            "a degenerate {what} is not a brush"
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
