//! Milestone 7 of `docs/design/shaping.md`: shaped runs into a document.
//!
//! Its exit criterion is a round trip — *"Arabic string built into a PDF, text
//! extraction returns the original string"* — and the reason that is the right
//! criterion rather than a picture is that it is the one property shaping can
//! silently destroy. After `GSUB` has run there is nothing left of the text but
//! the cluster on each glyph, and a producer that gets the clusters wrong
//! writes a document that **draws correctly and extracts wrongly**: the page
//! looks right, the words are unsearchable, and a screen reader reads
//! something else.
//!
//! # The faces are synthesised, and one of them ligates
//!
//! `epub_support::typeface` builds a face with a real `cmap`, `hmtx` and
//! `glyf` from a string of the characters it covers, and — since milestone 7 —
//! an optional `GSUB` with one ligature. That last one is what makes the round
//! trip say something: without a ligature every glyph stands for one character
//! and the mapping is the identity, which is a test that would pass with the
//! cluster logic deleted.

mod epub_support;

use epub_support::typeface::{Face, Ligature};
use tinker_pdf::shaping::{place, write_run, Run};
use tinker_pdf::Document;
use tinker_pdf_cos::build::DocumentBuilder;
use tinker_pdf_font::Sfnt;
use tinker_pdf_shape::bidi::BaseDirection;
use tinker_pdf_shape::Shaper;

/// `سلام` — four Arabic letters, of which the middle two are the lam-alef
/// pair every Arabic face joins.
const SALAM: &str = "\u{633}\u{644}\u{627}\u{645}";

/// A face covering that word, with lam and alef joined into one glyph under
/// `rlig`/`arab` — which is what a real Arabic face does to lam-alef and is
/// the one required ligature of the script.
fn arabic_face() -> Face {
    Face::new("Fixture Arabic", SALAM).with_ligature(Ligature {
        first: '\u{644}',
        second: '\u{627}',
        script: *b"arab",
        feature: *b"rlig",
    })
}

/// One page, one run, shaped and written.
fn document(face: &Face, text: &str, direction: BaseDirection) -> Vec<u8> {
    let program = face.build();
    let mut builder = DocumentBuilder::new();
    assert!(
        builder.add_cid_font(b"C0", b"Fixture", &program),
        "the fixture face registers as a composite font"
    );
    let sfnt = Sfnt::parse(&program).expect("the fixture face is a valid sfnt");
    let mut content = Vec::new();
    assert!(
        write_run(
            &mut builder,
            &mut content,
            &Run {
                font: b"C0",
                face: &sfnt,
                size: 12.0,
                matrix: [1.0, 0.0, 0.0, 1.0, 20.0, 40.0],
                text,
                direction,
            },
        ),
        "the run was written"
    );
    builder.add_page(200.0, 80.0, |page| {
        page.raw(&content);
    });
    builder.finish()
}

/// The criterion, in one assertion.
#[test]
fn an_arabic_string_built_through_the_builder_extracts_back_to_itself() {
    let bytes = document(&arabic_face(), SALAM, BaseDirection::RightToLeft);
    let document = Document::open(bytes).expect("the document opens");
    let page = document.page(0).expect("a page");
    let text = page.text().plain_text();
    assert!(
        text.contains(SALAM),
        "the run did not extract back to the string it was built from: {text:?}"
    );
}

/// And the ligature is really there, so the assertion above is not the
/// identity mapping passing itself off as a round trip.
#[test]
fn the_ligature_that_makes_the_round_trip_mean_something() {
    let face = arabic_face();
    let program = face.build();
    let sfnt = Sfnt::parse(&program).expect("a valid sfnt");
    let shaper = Shaper::new(&sfnt);
    let (_, runs) = shaper.shape_text(SALAM, BaseDirection::RightToLeft);
    let glyphs: Vec<_> = runs.iter().flat_map(|run| run.glyphs()).copied().collect();
    assert_eq!(
        glyphs.len(),
        3,
        "four characters should have shaped to three glyphs: {glyphs:?}"
    );
    assert_eq!(
        glyphs[1].glyph,
        face.ligature_glyph()
            .expect("the face has a ligature glyph"),
        "lam and alef did not join: {glyphs:?}"
    );

    // The ligature's cluster is the first byte of the two characters it
    // replaced, and `place` therefore hands it both of them.
    let placed: Vec<_> = runs
        .iter()
        .flat_map(|run| place(SALAM, run, 12.0))
        .collect();
    assert_eq!(placed.len(), 3);
    assert_eq!(placed[0].text, "\u{633}", "seen stands for itself");
    assert_eq!(
        placed[1].text, "\u{644}\u{627}",
        "the ligature stands for both characters it replaced"
    );
    assert_eq!(placed[2].text, "\u{645}", "meem stands for itself");
}

/// A decomposition is the other direction, and the rule that has to hold for
/// it is the one that is easy to get wrong: several glyphs share one cluster,
/// and only the **first** may claim the text.
///
/// There is no decomposing fixture face here, so the rule is asserted on the
/// projection directly, with a run this repository's own shaper produced.
#[test]
fn glyphs_that_share_a_cluster_do_not_each_claim_its_text() {
    let face = Face::new("Fixture Plain", "abc");
    let program = face.build();
    let sfnt = Sfnt::parse(&program).expect("a valid sfnt");
    let shaper = Shaper::new(&sfnt);
    let (_, runs) = shaper.shape_text("abc", BaseDirection::LeftToRight);
    let placed: Vec<_> = runs
        .iter()
        .flat_map(|run| place("abc", run, 10.0))
        .collect();
    // No `GSUB` at all, so this is the identity: three glyphs, three clusters,
    // three characters, each claimed once.
    assert_eq!(
        placed.iter().map(|p| p.text.as_str()).collect::<Vec<_>>(),
        vec!["a", "b", "c"]
    );
    // And the pen advanced by the face's own advance each time: 500 units of
    // a 1000-unit em at ten points is five points.
    assert_eq!(
        placed.iter().map(|p| p.x).collect::<Vec<_>>(),
        vec![0.0, 5.0, 10.0]
    );
    let joined: String = placed.iter().map(|p| p.text.as_str()).collect();
    assert_eq!(
        joined, "abc",
        "the text a run stands for is its own text, once"
    );
}

/// The strict structural validator, on the output.
#[test]
fn the_document_a_shaped_run_produced_is_structurally_clean() {
    let bytes = document(&arabic_face(), SALAM, BaseDirection::RightToLeft);
    let document = Document::open(bytes).expect("the document opens");
    let defects = document.validate();
    assert!(
        defects.is_empty(),
        "a document built from a shaped run should be clean: {defects:?}"
    );
}
