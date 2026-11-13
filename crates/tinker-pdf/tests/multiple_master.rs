//! An `/MMType1` font draws the instance its `/BaseFont` names
//! (`docs/design/type42-and-multiple-master.md`).
//!
//! 9.6.2.3 has an `/MMType1` font embed a snapshot — an ordinary Type 1
//! program of one instance — and a page that embeds the whole multiple-master
//! program instead has its instance only in the font's name, by the
//! convention TN #5088 writes. Each page here embeds `type1_support`'s
//! two-axis, four-master program, and its twin embeds a plain program whose
//! one master is the instance's outlines, blended in that module from the
//! blend design by the supplement's arithmetic; the two must render to the
//! same bytes, write the same SVG and read the same text. Nothing here depends
//! on a feature, and the file runs alike in the default and `bundled-fonts`
//! builds.

mod type1_support;

use tinker_pdf::{
    Document, MultipleMasterInstance, RenderOptions, RenderWarning, SvgOptions, WarningKind,
};
use type1_support::{
    content_page, instance_program, multiple_master_program, weights_at, WEIGHT_VECTOR,
};

/// What a page draws: its bitmap's size and bytes, its SVG, its text, and the
/// document's warnings once all three are read.
struct Drawn {
    width: u32,
    height: u32,
    data: Vec<u8>,
    /// The pixels other than the white background.
    ink: usize,
    svg: String,
    text: String,
    warnings: Vec<WarningKind>,
    unreadable: bool,
}

fn drawn(pdf: Vec<u8>) -> Drawn {
    let document = Document::open(pdf).expect("it opens");
    let page = document.page(0).expect("a page");
    let bitmap = page.render(&RenderOptions::default());
    let svg = page.to_svg(&SvgOptions::default()).markup;
    let text = page.text().plain_text();
    let ink = bitmap
        .data
        .chunks_exact(bitmap.components())
        .filter(|pixel| pixel.iter().any(|value| *value != 255))
        .count();
    Drawn {
        ink,
        width: bitmap.width,
        height: bitmap.height,
        unreadable: bitmap.warnings.contains(&RenderWarning::UnreadableFont),
        data: bitmap.data,
        svg,
        text,
        warnings: document.warnings().into_iter().map(|w| w.kind).collect(),
    }
}

/// The page's instance reports, in the order the document holds them.
fn instances(warnings: &[WarningKind]) -> Vec<MultipleMasterInstance> {
    warnings
        .iter()
        .filter_map(|kind| match kind {
            WarningKind::MultipleMasterInstance(instance) => Some(*instance),
            _ => None,
        })
        .collect()
}

/// Holds a page and its twin to one drawing.
fn same_drawing(page: &Drawn, twin: &Drawn, what: &str) {
    assert!(!page.unreadable, "{what}: the font is readable");
    assert!(!twin.unreadable, "{what}: the twin's font is readable");
    assert!(page.ink > 0, "{what}: the page draws its glyphs");
    assert_eq!(
        (page.width, page.height),
        (twin.width, twin.height),
        "{what}"
    );
    assert!(
        page.data == twin.data,
        "{what}: the page renders other bytes than its twin"
    );
    assert_eq!(page.svg, twin.svg, "{what}: and writes other SVG");
    assert_eq!(page.text, twin.text, "{what}: and reads other text");
}

/// **FT-10.5's exit.** An `/MMType1` font embedding the multiple-master
/// program draws the instance its name gives, byte for byte as a plain Type 1
/// twin whose one master is that instance's outlines, computed from the
/// blend design; under each of the name's spellings the convention allows,
/// and behind a subset tag.
#[test]
fn an_mmtype1_font_draws_the_instance_its_name_gives() {
    for (name, design) in [
        ("TestMM_500_700_", [500.0, 700.0]),
        ("TestMM_300wt_400wd", [300.0, 400.0]),
        ("ABCDEF+TestMM_700_100_", [700.0, 100.0]),
        ("TestMM Regular_900 wt 1000 wd", [900.0, 1000.0]),
    ] {
        let page = drawn(content_page("MMType1", name, &multiple_master_program()));
        let twin = drawn(content_page(
            "Type1",
            name,
            &instance_program(&weights_at(design)),
        ));
        same_drawing(&page, &twin, name);
        assert_eq!(
            instances(&page.warnings),
            vec![MultipleMasterInstance::Design],
            "{name}: drawn at the name's instance, reported once"
        );
        assert!(
            instances(&twin.warnings).is_empty(),
            "a plain program blends nothing"
        );
    }

    // Two instances are two drawings: the name is what chooses.
    let a = drawn(content_page(
        "MMType1",
        "TestMM_500_700_",
        &multiple_master_program(),
    ));
    let b = drawn(content_page(
        "MMType1",
        "TestMM_300_400_",
        &multiple_master_program(),
    ));
    assert!(a.data != b.data, "two instances draw two glyphs");
}

/// A name without design coordinates draws the program's default instance,
/// its own `/WeightVector`, and says so.
#[test]
fn an_mmtype1_font_without_coordinates_draws_its_default_instance() {
    let page = drawn(content_page(
        "MMType1",
        "TestMM",
        &multiple_master_program(),
    ));
    let twin = drawn(content_page(
        "Type1",
        "TestMM",
        &instance_program(&WEIGHT_VECTOR),
    ));
    same_drawing(&page, &twin, "no coordinates");
    assert_eq!(
        instances(&page.warnings),
        vec![MultipleMasterInstance::DefaultNoCoordinates]
    );
}

/// A name with another number of coordinates than the program's axes names
/// no instance of it: the default is drawn, and reported.
#[test]
fn a_name_with_another_number_of_coordinates_draws_the_default() {
    let twin = drawn(content_page(
        "Type1",
        "TestMM_500_",
        &instance_program(&WEIGHT_VECTOR),
    ));
    for name in ["TestMM_500_", "TestMM_100_200_300_"] {
        let page = drawn(content_page("MMType1", name, &multiple_master_program()));
        assert!(page.data == twin.data, "{name}: the default instance");
        assert_eq!(
            instances(&page.warnings),
            vec![MultipleMasterInstance::DefaultAxisCount],
            "{name}"
        );
    }
}

/// Coordinates past the program's maps are taken at their ends — here the
/// third master's corner — and the instance says it was clamped.
#[test]
fn coordinates_past_the_maps_are_taken_at_their_ends() {
    let weights = weights_at([50.0, 2000.0]);
    assert_eq!(weights, [0.0, 0.0, 1.0, 0.0]);
    let page = drawn(content_page(
        "MMType1",
        "TestMM_50_2000_",
        &multiple_master_program(),
    ));
    let twin = drawn(content_page(
        "Type1",
        "TestMM_50_2000_",
        &instance_program(&weights),
    ));
    same_drawing(&page, &twin, "clamped");
    assert_eq!(
        instances(&page.warnings),
        vec![MultipleMasterInstance::Clamped]
    );
}

/// A `/Type1` dictionary over a multiple-master program names no instance,
/// whatever its name holds: the program draws its default, reported.
#[test]
fn a_type1_dictionary_over_a_multiple_master_program_draws_its_default() {
    let page = drawn(content_page(
        "Type1",
        "TestMM_500_700_",
        &multiple_master_program(),
    ));
    let twin = drawn(content_page(
        "Type1",
        "TestMM_500_700_",
        &instance_program(&WEIGHT_VECTOR),
    ));
    same_drawing(&page, &twin, "a /Type1 dictionary");
    assert_eq!(
        instances(&page.warnings),
        vec![MultipleMasterInstance::DefaultType1Font]
    );
}

/// The conforming case: an `/MMType1` font embedding a snapshot, an ordinary
/// Type 1 program, draws that program as a `/Type1` font does, and nothing is
/// reported.
#[test]
fn a_snapshot_draws_as_any_type1_program_and_is_not_reported() {
    let snapshot = instance_program(&weights_at([500.0, 700.0]));
    let page = drawn(content_page("MMType1", "TestMM_500_700_", &snapshot));
    let twin = drawn(content_page("Type1", "TestMM_500_700_", &snapshot));
    same_drawing(&page, &twin, "a snapshot");
    assert!(instances(&page.warnings).is_empty());
    assert!(page.warnings.is_empty(), "{:?}", page.warnings);
}

/// The report is once a font, however many glyphs, renders and readings ask
/// for an outline.
#[test]
fn the_instance_is_reported_once_a_font() {
    let document = Document::open(content_page(
        "MMType1",
        "TestMM_500_700_",
        &multiple_master_program(),
    ))
    .expect("it opens");
    let page = document.page(0).expect("a page");
    for _ in 0..3 {
        let _ = page.render(&RenderOptions::default());
        let _ = page.to_svg(&SvgOptions::default());
    }
    let _ = page.text().plain_text();
    let kinds: Vec<WarningKind> = document.warnings().into_iter().map(|w| w.kind).collect();
    assert_eq!(instances(&kinds), vec![MultipleMasterInstance::Design]);
    assert_eq!(
        kinds.len(),
        1,
        "the page opens and draws with nothing else to report: {kinds:?}"
    );
}
