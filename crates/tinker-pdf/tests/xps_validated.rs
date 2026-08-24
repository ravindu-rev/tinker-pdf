//! The document a fixed document is synthesised into, held to ISO 32000.
//!
//! This replaces `xps_qpdf.rs`, which asked qpdf whether the synthesised
//! document was a valid PDF and what its pages, gradients, groups and fonts
//! said. Ruling 13 retires that; the strict validator answers the first half
//! and raw dictionary reads answer the second.
//!
//! **What is lost is worth naming**, and it is more here than anywhere else:
//! the deleted `xps_mutool.rs` is a second *reading of the XPS*, which is a
//! different claim from this one and leaves separately. What went with qpdf is
//! narrower — that a reader nobody here wrote accepts the PDF this engine
//! synthesised. Nothing below is that reader.
//!
//! What survives is what the oracle was actually catching: every value here is
//! read out of a dictionary as the file spells it, never through the typed
//! readers, because 11.6.6 defaults `/I` and `/K` to false and 8.7.4.5 defaults
//! a missing `/Extend` — so a round trip through this repository's own reader
//! would agree with itself about a form that carried none of them.

mod xps_support;

use tinker_pdf::{cbz, xps};
use tinker_pdf::{CosDocument, Defect, Document, WriteMode, WriteOptions};
// Through `xps_support` rather than by its own `mod`, because the conservation
// harness beside it reads a document the same way and a file loaded as two
// modules in one binary is two copies of the same helpers.
use xps_support::validated::{category, flat, has, name, numbers, pages, resource, value};
use xps_support::{archive, document, fixed_page, fixed_page_with, one_page_package, with};

fn corpus(name: &str) -> Vec<u8> {
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/xps")
        .join(name);
    std::fs::read(&path).unwrap_or_else(|e| panic!("reading {}: {e}", path.display()))
}

/// The bytes the synthesiser hands the parser, before the parser sees them.
fn synthesise(package: &[u8]) -> Vec<u8> {
    let opened =
        cbz::open_archive(package, &tinker_pdf_zip::Limits::DEFAULT).expect("a readable ZIP");
    match xps::route(opened, &xps::Limits::DEFAULT) {
        xps::Routing::Document(pdf, _) => pdf,
        xps::Routing::Refused(why) => panic!("the package was refused: {why}"),
        xps::Routing::NotXps(_) => panic!("the package is an XPS"),
    }
}

#[track_caller]
fn valid(label: &str, bytes: &[u8]) -> CosDocument {
    let doc = CosDocument::open(bytes.to_vec()).expect("the document opens");
    let defects = tinker_pdf_cos::validate(&doc);
    assert!(
        defects.is_empty(),
        "{label}: {:?}",
        defects.iter().map(Defect::to_string).collect::<Vec<_>>()
    );
    doc
}

/// A three-page document whose markup order agrees with no order over its
/// names, and whose three pages are three different sizes.
fn out_of_order() -> Vec<u8> {
    let parts = with(
        one_page_package(),
        "Documents/1/FixedDocument.fdoc",
        &document(&["Pages/p10.fpage", "Pages/p1.fpage", "Pages/p2.fpage"]),
    );
    let parts = with(
        parts,
        "Documents/1/Pages/p10.fpage",
        &fixed_page("816", "1056"),
    );
    let parts = with(
        parts,
        "Documents/1/Pages/p1.fpage",
        &fixed_page("400", "600"),
    );
    let parts = with(
        parts,
        "Documents/1/Pages/p2.fpage",
        &fixed_page("200", "300"),
    );
    archive(parts)
}

/// A one-page package whose markup uses every construct the painter builds.
fn drawn_page() -> Vec<u8> {
    let body = concat!(
        // A canvas opacity over children that overlap, which is a group.
        r##"<Canvas Opacity="0.5">"##,
        r##"<Path Fill="#FF0000" Data="M0,0L100,0 100,100 0,100Z" />"##,
        r##"<Path Fill="#0000FF" Data="M50,50L150,50 150,150 50,150Z" />"##,
        r##"</Canvas>"##,
        // A three-stop linear gradient, which is a stitching function.
        r##"<Path Data="M0,300L400,300 400,500 0,500Z"><Path.Fill>"##,
        r##"<LinearGradientBrush StartPoint="0,300" EndPoint="400,500">"##,
        r##"<LinearGradientBrush.GradientStops>"##,
        r##"<GradientStop Color="#FF0000" Offset="0" />"##,
        r##"<GradientStop Color="#00FF00" Offset="0.5" />"##,
        r##"<GradientStop Color="#0000FF" Offset="1" />"##,
        r##"</LinearGradientBrush.GradientStops>"##,
        r##"</LinearGradientBrush></Path.Fill></Path>"##,
    );
    let markup = format!(
        r#"<FixedPage xmlns="http://schemas.microsoft.com/xps/2005/06" Width="816" Height="1056">{body}</FixedPage>"#
    );
    archive(with(
        one_page_package(),
        "Documents/1/Pages/1.fpage",
        &markup,
    ))
}

// ---- the structure ----------------------------------------------------------

#[test]
fn a_synthesised_fixed_document_validates() {
    valid("the synthesised document", &synthesise(&out_of_order()));
}

/// And so does the same document saved back, in every layout — including the
/// one where the `/ToUnicode` CMap and the embedded font program both become
/// `/FlateDecode` streams, which is where a subset that was almost a font would
/// show up.
#[test]
fn a_saved_fixed_document_validates() {
    let document = Document::open(corpus("xpsom-image-and-text.oxps")).expect("a fixed document");
    for (label, options) in [
        (
            "rewritten",
            WriteOptions {
                mode: WriteMode::Rewrite,
                ..WriteOptions::default()
            },
        ),
        (
            "linearized",
            WriteOptions {
                mode: WriteMode::Rewrite,
                linearize: true,
                ..WriteOptions::default()
            },
        ),
        (
            "compressed",
            WriteOptions {
                mode: WriteMode::Rewrite,
                compress: true,
                object_streams: true,
                ..WriteOptions::default()
            },
        ),
    ] {
        valid(label, &document.editor().save(&options));
    }
}

/// Two real packages whose pages paint, and the built page that uses every
/// construct at once.
#[test]
fn a_page_that_draws_validates() {
    for name in ["wpf-shapes-only.xps", "wpf-gradients.xps"] {
        valid(name, &synthesise(&corpus(name)));
    }

    let pdf = synthesise(&drawn_page());
    valid("the built page", &pdf);
    let document = Document::open(pdf).expect("the synthesised document reopens");
    for compress in [false, true] {
        let saved = document.editor().save(&WriteOptions {
            mode: WriteMode::Rewrite,
            compress,
            object_streams: compress,
            ..WriteOptions::default()
        });
        valid("saved", &saved);
    }
}

// ---- what the structure says ------------------------------------------------

/// The pages are in markup order, at the sizes the markup states, and no page
/// of a fixed document carries an image XObject.
///
/// That last clause is the defect this whole feature exists for: before the
/// router could tell a fixed document from a comic, this same package opened as
/// a one-page comic whose page *was* a resource.
#[test]
fn the_pages_are_in_markup_order_at_the_sizes_the_markup_states() {
    let doc = valid("the synthesised document", &synthesise(&out_of_order()));
    let pages = pages(&doc);
    assert_eq!(pages.len(), 3, "three pages");

    // Markup order is p10, p1, p2 — 816 x 1056, 400 x 600, 200 x 300 in units
    // of 1/96 inch. Natural order would be 612, 450, 225 tall and lexicographic
    // order 450, 792, 225.
    let wanted = [
        [0.0, 0.0, 612.0, 792.0],
        [0.0, 0.0, 300.0, 450.0],
        [0.0, 0.0, 150.0, 225.0],
    ];
    for (index, (reference, page)) in pages.iter().enumerate() {
        let box_ = numbers(&doc, page, b"MediaBox")
            .unwrap_or_else(|| panic!("page {index} ({reference}) states no /MediaBox"));
        assert_eq!(box_, wanted[index], "page {index}'s /MediaBox");
        assert!(
            category(&doc, page, b"XObject").is_empty(),
            "page {index} of a fixed document carries an image: {}",
            flat(&doc, &value(&doc, page, b"Resources"))
        );
    }
}

/// The two rectangles a fixed page stated, and none where a page stated none.
///
/// 10.3.1's `ContentBox` becomes `/CropBox` and 10.3.2's `BleedBox` becomes
/// `/BleedBox`, once scaled from 1/96 to 1/72 and once flipped from XPS's
/// top-left origin to PDF's bottom-left. `tests/xps_spine.rs` asserts those
/// numbers through this engine's own reader, which normalises a rectangle and
/// clips a crop box to the media box on the way — so it cannot see a box
/// written back to front or one written outside the page. This reads the array
/// as the file holds it.
///
/// The second page states nothing, and carries no key at all rather than a box
/// equal to the media box: 7.7.3.3 makes those two different documents.
#[test]
fn a_fixed_page_states_the_boxes_it_has_and_no_others() {
    let parts = with(
        with(
            one_page_package(),
            "Documents/1/FixedDocument.fdoc",
            &document(&["Pages/1.fpage", "Pages/2.fpage"]),
        ),
        "Documents/1/Pages/1.fpage",
        &fixed_page_with(
            "816",
            "1056",
            r#"ContentBox="96,48,192,96" BleedBox="48,24,720,1008" "#,
        ),
    );
    let parts = with(
        parts,
        "Documents/1/Pages/2.fpage",
        &fixed_page("816", "1056"),
    );
    let doc = valid("the synthesised document", &synthesise(&archive(parts)));

    let pages = pages(&doc);
    assert_eq!(pages.len(), 2, "two pages");

    let (_, stated) = &pages[0];
    assert_eq!(
        numbers(&doc, stated, b"MediaBox"),
        Some(vec![0.0, 0.0, 612.0, 792.0]),
        "the page keeps its own size"
    );
    assert_eq!(
        numbers(&doc, stated, b"CropBox"),
        Some(vec![72.0, 684.0, 216.0, 756.0]),
        "ContentBox=\"96,48,192,96\" scaled once and flipped once"
    );
    assert_eq!(
        numbers(&doc, stated, b"BleedBox"),
        Some(vec![36.0, 18.0, 576.0, 774.0]),
        "BleedBox=\"48,24,720,1008\" likewise"
    );

    let (_, bare) = &pages[1];
    assert_eq!(
        numbers(&doc, bare, b"MediaBox"),
        Some(vec![0.0, 0.0, 612.0, 792.0]),
        "the same page size"
    );
    assert!(
        !has(&doc, bare, b"CropBox") && !has(&doc, bare, b"BleedBox"),
        "a page that stated no box carries no key: {}",
        flat(&doc, &tinker_pdf::Object::Dict(bare.clone()))
    );
}

/// A `LinearGradientBrush` becomes an axial shading over a stitching function.
#[test]
fn a_gradient_becomes_a_shading_over_a_stitching_function() {
    let doc = valid("the built page", &synthesise(&drawn_page()));
    let (_, page) = pages(&doc).into_iter().next().expect("one page");

    let (_, shading) = resource(&doc, &page, b"Shading", b"Sh2").unwrap_or_else(|| {
        panic!(
            "the page carries no shading: {}",
            flat(&doc, &value(&doc, &page, b"Resources"))
        )
    });
    let shading = shading.as_dict().expect("a shading dictionary").clone();

    assert_eq!(
        value(&doc, &shading, b"ShadingType").as_int(),
        Some(2),
        "8.7.4.5.3's axial type"
    );
    assert_eq!(
        numbers(&doc, &shading, b"Coords"),
        Some(vec![0.0, 300.0, 400.0, 500.0]),
        "the axis is `StartPoint` to `EndPoint`, in the markup's own numbers"
    );
    // 8.7.4.5.3 defaults a missing `/Extend` to `(false, false)`, so this is
    // one of the entries a round trip through this crate's own reader could
    // not have noticed the absence of.
    let extend = value(&doc, &shading, b"Extend");
    let extend: Vec<bool> = extend
        .as_array()
        .expect("a `Pad` spread is 8.7.4.5.3's extend")
        .iter()
        .filter_map(tinker_pdf::Object::as_bool)
        .collect();
    assert_eq!(extend, vec![true, true]);

    let function = value(&doc, &shading, b"Function");
    let function = function.as_dict().expect("a function").clone();
    assert_eq!(
        value(&doc, &function, b"FunctionType").as_int(),
        Some(3),
        "three stops are two ramps stitched"
    );
    assert_eq!(
        numbers(&doc, &function, b"Bounds"),
        Some(vec![0.5]),
        "the middle stop's own offset"
    );
    assert_eq!(
        numbers(&doc, &function, b"Encode"),
        Some(vec![0.0, 1.0, 0.0, 1.0]),
        "each ramp is reached forwards"
    );
}

/// A `Canvas` `Opacity` over overlapping children becomes a transparency
/// group, and the alpha is on the form rather than in the colours.
#[test]
fn a_canvas_opacity_becomes_a_transparency_group() {
    let doc = valid("the built page", &synthesise(&drawn_page()));
    let (_, page) = pages(&doc).into_iter().next().expect("one page");

    let (_, form) = resource(&doc, &page, b"XObject", b"Fm0").unwrap_or_else(|| {
        panic!(
            "the page carries no form: {}",
            flat(&doc, &value(&doc, &page, b"Resources"))
        )
    });
    let form = form.as_dict().expect("a form dictionary").clone();
    assert_eq!(name(&doc, &form, b"Subtype").as_deref(), Some(&b"Form"[..]));
    assert_eq!(
        numbers(&doc, &form, b"BBox"),
        Some(vec![0.0, 0.0, 150.0, 150.0]),
        "the box is the union of what the canvas drew"
    );

    // 11.6.6 defaults `/I` and `/K` to false and a form with no `/Group` is an
    // ordinary form, so all three of these are entries whose *absence* this
    // engine's own reader would have supplied for itself.
    let group = value(&doc, &form, b"Group");
    let group = group.as_dict().expect("11.6.6's group dictionary").clone();
    assert_eq!(
        name(&doc, &group, b"S").as_deref(),
        Some(&b"Transparency"[..])
    );
    assert_eq!(
        name(&doc, &group, b"CS").as_deref(),
        Some(&b"DeviceRGB"[..])
    );
    assert_eq!(
        value(&doc, &group, b"I").as_bool(),
        Some(true),
        "an isolated group"
    );

    let (_, state) = resource(&doc, &page, b"ExtGState", b"GS1").expect("a graphics state");
    let state = state.as_dict().expect("a state dictionary").clone();
    assert_eq!(
        (
            value(&doc, &state, b"ca").as_number(),
            value(&doc, &state, b"CA").as_number()
        ),
        (Some(0.5), Some(0.5)),
        "11.6.4.4's two alphas are two entries, which a case-insensitive \
         reader would lose"
    );
}

/// The font a real `Glyphs` run drew with, out of a package whose font program
/// was thirty-two XORs away from being unreadable.
///
/// Every number here is one the package chose: `/Identity-H` over a
/// `/CIDFontType2` descendant with `/CIDToGIDMap /Identity`, which is what
/// makes `Indices` addressable at all; `/W` carrying 586 for each of the seven
/// glyphs the run drew, which is Cascadia Mono's own 1200 `hmtx` units over
/// 2048 per em; and a `/ToUnicode` of exactly seven entries, which is what
/// `UnicodeString="Page one"` is once its two `e`s are one glyph.
#[test]
fn a_glyphs_run_reaches_the_page_through_a_composite_font() {
    let doc = valid(
        "the synthesised document",
        &synthesise(&corpus("wpf-image-and-text.xps")),
    );
    let (_, page) = pages(&doc).into_iter().next().expect("one page");

    let (_, font) = resource(&doc, &page, b"Font", b"XF0").expect("the run's font");
    let font = font.as_dict().expect("a font dictionary").clone();
    assert_eq!(
        name(&doc, &font, b"Subtype").as_deref(),
        Some(&b"Type0"[..])
    );
    assert_eq!(
        name(&doc, &font, b"Encoding").as_deref(),
        Some(&b"Identity-H"[..])
    );
    let base = name(&doc, &font, b"BaseFont").expect("a base font");
    assert!(
        base.ends_with(b"+XpsFont") && base.len() == b"WZKFDK+XpsFont".len(),
        "9.6.4's six-letter subset tag, because the program is cut down to \
         seven glyphs out of a face that carries a whole `gvar`: {}",
        String::from_utf8_lossy(&base)
    );

    let descendants = value(&doc, &font, b"DescendantFonts");
    let descendants = descendants.as_array().expect("9.7.1's one descendant");
    assert_eq!(descendants.len(), 1);
    let descendant = doc.resolve(&descendants[0]);
    let descendant = descendant.as_dict().expect("a CID font").clone();
    assert_eq!(
        name(&doc, &descendant, b"Subtype").as_deref(),
        Some(&b"CIDFontType2"[..])
    );
    assert_eq!(
        name(&doc, &descendant, b"CIDToGIDMap").as_deref(),
        Some(&b"Identity"[..])
    );
    assert_eq!(value(&doc, &descendant, b"DW").as_int(), Some(1000));

    let info = value(&doc, &descendant, b"CIDSystemInfo");
    let info = info
        .as_dict()
        .expect("9.7.3's registry and ordering")
        .clone();
    assert_eq!(
        info.get_string(doc.intern(b"Ordering"))
            .map(|s| s.bytes.clone()),
        Some(b"Identity".to_vec()),
        "a descendant claiming another ordering under `/Identity-H` is a font \
         whose two halves disagree"
    );

    // 9.7.4.3's `c [w]` runs, seven of them, each at the face's own advance.
    let widths = value(&doc, &descendant, b"W");
    let widths = widths.as_array().expect("a /W array").to_vec();
    let mut runs: Vec<(i64, Vec<f64>)> = Vec::new();
    let mut index = 0;
    while index + 1 < widths.len() {
        let first = widths[index].as_int().expect("a starting CID");
        let run = doc.resolve(&widths[index + 1]);
        let run: Vec<f64> = run
            .as_array()
            .expect("`c [w1 w2 ...]`")
            .iter()
            .filter_map(tinker_pdf::Object::as_number)
            .collect();
        runs.push((first, run));
        index += 2;
    }
    assert_eq!(
        runs,
        vec![
            (146, vec![586.0]),
            (222, vec![586.0]),
            (260, vec![586.0]),
            (284, vec![586.0]),
            (336, vec![586.0]),
            (345, vec![586.0]),
            (861, vec![586.0]),
        ],
        "seven glyphs, at Cascadia Mono's own 1200/2048 em"
    );

    // The CMap's own text, decoded: a `/ToUnicode` is a stream, and a test that
    // read only its dictionary would see nothing at all.
    let map = font
        .get_ref(doc.intern(b"ToUnicode"))
        .expect("a /ToUnicode stream");
    let cmap = doc.stream_decoded(map).expect("it decodes");
    let cmap = String::from_utf8_lossy(&cmap).into_owned();
    assert!(cmap.contains("7 beginbfchar"), "{cmap}");
    for entry in [
        "<0092> <0050>",
        "<00DE> <0061>",
        "<0104> <0065>",
        "<011C> <0067>",
        "<0150> <006E>",
        "<0159> <006F>",
        "<035D> <0020>",
    ] {
        assert!(cmap.contains(entry), "{entry} in {cmap}");
    }
    assert!(
        cmap.contains("<0000> <FFFF>"),
        "the codespace is two bytes wide, which is what says a code is a CID: {cmap}"
    );
}
