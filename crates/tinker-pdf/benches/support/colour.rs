//! The page of colour glyphs the write and render groups time (FT-06.17a):
//! `inputs::colour_lines()` set in `inputs::colour_face()` on one A4 page.
//!
//! The head writes each line through the colour run writer, so each capital
//! is a Type 3 glyph whose procedure draws its `COLR` graph. The planning
//! baseline has no colour glyphs: under `tinker_planning_baseline` the same
//! lines are set in the same face's outlines, the face embedded as a
//! composite font, and that page is what each operation is compared with.
//! A test reaches this file by path, as it reaches `inputs.rs`.

#[cfg(not(tinker_planning_baseline))]
use tinker_pdf::colour_glyphs::ColourFace;
use tinker_pdf::shaping::{write_run, Run};
use tinker_pdf::Document;
use tinker_pdf_cos::build::DocumentBuilder;
#[cfg(not(tinker_planning_baseline))]
use tinker_pdf_font::PaletteChoice;
use tinker_pdf_font::Sfnt;
use tinker_pdf_shape::bidi::BaseDirection;
#[cfg(not(tinker_planning_baseline))]
use tinker_pdf_shape::Shaper;

use super::inputs;

/// A4, in points: the page the lines are set on.
const PAGE: (f64, f64) = (595.0, 842.0);

/// `lines` set in `program` on one A4 page, each line a run through the
/// colour run writer (`shaping::write_run` with the face's `ColourFace`), the
/// face held to have drawn every glyph's graph.
#[cfg(not(tinker_planning_baseline))]
pub fn colour_page(program: &[u8], lines: &[String]) -> Vec<u8> {
    let face = Sfnt::parse(program).expect("the colour face parses");
    let colour = ColourFace::new(&face, PaletteChoice::Default);
    let mut builder = DocumentBuilder::new();
    assert!(
        colour.register(&mut builder, b"C0", b"BenchColour"),
        "the colour face registers"
    );
    let mut content = Vec::new();
    for (line, text) in lines.iter().enumerate() {
        let run = Run {
            font: b"C0",
            shaper: Shaper::new(&face),
            size: inputs::COLOUR_SIZE,
            matrix: inputs::colour_line_matrix(line),
            text,
            direction: BaseDirection::LeftToRight,
            colour: Some(&colour),
        };
        assert!(
            write_run(&mut builder, &mut content, &run),
            "the line is written"
        );
    }
    assert!(
        colour.warnings().is_empty(),
        "every graph is drawn: {:?}",
        colour.warnings()
    );
    builder.add_page(PAGE.0, PAGE.1, |page| page.raw(&content));
    builder.finish()
}

/// The planning baseline's arm: the same lines in the same face's outlines,
/// the face embedded as a composite font (`add_cid_font`).
#[cfg(tinker_planning_baseline)]
pub fn colour_page(program: &[u8], lines: &[String]) -> Vec<u8> {
    let face = Sfnt::parse(program).expect("the colour face parses");
    let mut builder = DocumentBuilder::new();
    assert!(
        builder.add_cid_font(b"F0", b"BenchColour", program),
        "the face embeds"
    );
    let mut content = Vec::new();
    for (line, text) in lines.iter().enumerate() {
        let run = Run {
            font: b"F0",
            face: &face,
            size: inputs::COLOUR_SIZE,
            matrix: inputs::colour_line_matrix(line),
            text,
            direction: BaseDirection::LeftToRight,
        };
        assert!(
            write_run(&mut builder, &mut content, &run),
            "the line is written"
        );
    }
    builder.add_page(PAGE.0, PAGE.1, |page| page.raw(&content));
    builder.finish()
}

/// What a page of colour glyphs is held to before it is timed: it opens to
/// one page whose text is its lines'.
pub fn holds_its_lines(bytes: &[u8], lines: &[String]) {
    let document = Document::open(bytes.to_vec()).expect("the page of colour glyphs opens");
    assert_eq!(document.page_count(), 1, "one page");
    let text: String = document
        .page(0)
        .expect("a page")
        .text()
        .plain_text()
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect();
    assert_eq!(text, lines.concat(), "the page's text is its lines");
}
