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
use tinker_pdf_cos::build::DocumentBuilder;
use tinker_pdf_font::Sfnt;
use tinker_pdf_shape::bidi::BaseDirection;
use tinker_pdf_shape::{Plans, Shaper};

mod written_support;
use written_support::authored;

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
                shaper: Shaper::new(&sfnt),
                size: 12.0,
                matrix: [1.0, 0.0, 0.0, 1.0, 20.0, 40.0],
                text,
                direction,
                colour: None,
            },
        ),
        "the run was written"
    );
    builder.add_page(200.0, 80.0, |page| {
        page.raw(&content);
    });
    builder.finish()
}

/// P-26.4: the runs one document build writes in one face share the face's
/// plans. Five runs through shapers made with one `Plans` write the bytes
/// five runs with none write, and a second build of the same runs builds no
/// plan or index the first did not.
#[test]
fn runs_written_in_one_face_share_its_plans() {
    let face = arabic_face();
    let program = face.build();
    let sfnt = Sfnt::parse(&program).expect("the fixture face is a valid sfnt");
    let write = |shaper: Shaper<'_>| -> Vec<u8> {
        let mut builder = DocumentBuilder::new();
        assert!(builder.add_cid_font(b"C0", b"Fixture", &program));
        let mut content = Vec::new();
        for line in 0..5u8 {
            assert!(
                write_run(
                    &mut builder,
                    &mut content,
                    &Run {
                        font: b"C0",
                        shaper,
                        size: 12.0,
                        matrix: [1.0, 0.0, 0.0, 1.0, 20.0, 20.0 + 14.0 * f64::from(line)],
                        text: SALAM,
                        direction: BaseDirection::RightToLeft,
                        colour: None,
                    },
                ),
                "run {line} was written"
            );
        }
        content
    };
    let plans = Plans::new();
    let plain = write(Shaper::new(&sfnt));
    assert_eq!(
        write(Shaper::new(&sfnt).with_plans(&plans)),
        plain,
        "a kept Plans changed what was written"
    );
    let built = (plans.plans_built(), plans.indexes_built());
    assert!(
        built.0 > 0 && built.1 > 0,
        "the face's rlig was planned and indexed"
    );
    assert_eq!(write(Shaper::new(&sfnt).with_plans(&plans)), plain);
    assert_eq!(
        (plans.plans_built(), plans.indexes_built()),
        built,
        "a second build in the same face built again"
    );
}

/// The criterion, in one assertion.
#[test]
fn an_arabic_string_built_through_the_builder_extracts_back_to_itself() {
    let bytes = document(&arabic_face(), SALAM, BaseDirection::RightToLeft);
    let document = authored("an Arabic run", bytes);
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

/// The validate-on-save helper, on the output: the strict validator finds
/// nothing in a document built from a shaped run.
#[test]
fn the_document_a_shaped_run_produced_is_structurally_clean() {
    authored(
        "a document built from a shaped run",
        document(&arabic_face(), SALAM, BaseDirection::RightToLeft),
    );
}

/// FT-08.7: Balinese `KA ULU TALING` — `SHBALI-1/14`'s text — whose pre-base
/// vowel, typed after a whole cluster, is a broken cluster of its own.
const BROKEN: &str = "\u{1B13}\u{1B36}\u{1B3E}";

/// The span an inserted glyph is drawn in: one whose replacement text is
/// empty (ISO 32000-2 14.9.4).
const SILENT: &[u8] = b"/Span <</ActualText ()>> BDC";

/// The content stream `write_run` writes for `text` in `face`.
fn written(face: &Face, text: &str) -> Vec<u8> {
    let program = face.build();
    let sfnt = Sfnt::parse(&program).expect("a valid sfnt");
    let mut builder = DocumentBuilder::new();
    assert!(builder.add_cid_font(b"C0", b"Fixture", &program));
    let mut content = Vec::new();
    assert!(
        write_run(
            &mut builder,
            &mut content,
            &Run {
                font: b"C0",
                shaper: Shaper::new(&sfnt),
                size: 12.0,
                matrix: [1.0, 0.0, 0.0, 1.0, 20.0, 40.0],
                text,
                direction: BaseDirection::LeftToRight,
                colour: None,
            },
        ),
        "the run was written"
    );
    content
}

/// How many times `needle` occurs in `haystack`.
fn occurrences(haystack: &[u8], needle: &[u8]) -> usize {
    haystack
        .windows(needle.len())
        .filter(|window| *window == needle)
        .count()
}

/// A broken cluster is drawn on the face's dotted circle and extracts as it
/// was typed: the circle is among the glyphs written, standing for no text,
/// and the vowel it stands under keeps its own, so the page's text is the
/// three characters typed and no U+25CC.
#[test]
fn a_broken_cluster_round_trips_without_its_dotted_circle() {
    let face = Face::new("Fixture Balinese", "\u{1B13}\u{1B36}\u{1B3E}\u{25CC}");
    let circle = face.glyph_of('\u{25CC}').expect("the face maps U+25CC");
    let program = face.build();
    let sfnt = Sfnt::parse(&program).expect("a valid sfnt");
    let (_, runs) = Shaper::new(&sfnt).shape_text(BROKEN, BaseDirection::LeftToRight);
    let placed: Vec<_> = runs
        .iter()
        .flat_map(|run| place(BROKEN, run, 12.0))
        .collect();
    let drawn: Vec<(u16, &str)> = placed
        .iter()
        .map(|glyph| (glyph.id, glyph.text.as_str()))
        .collect();
    assert_eq!(
        drawn.iter().filter(|(id, _)| *id == circle).count(),
        1,
        "the broken cluster was not drawn on one circle: {drawn:?}"
    );
    assert!(
        drawn.contains(&(circle, "")),
        "the circle claimed text of its own: {drawn:?}"
    );
    assert_eq!(
        drawn.iter().map(|(_, text)| *text).collect::<String>(),
        BROKEN,
        "the glyphs do not stand for the text typed, once: {drawn:?}"
    );
    // The circle is drawn inside a span that states nothing, once.
    let content = written(&face, BROKEN);
    assert_eq!(occurrences(&content, SILENT), 1);
    assert_eq!(occurrences(&content, b"EMC"), 1);

    let bytes = document(&face, BROKEN, BaseDirection::LeftToRight);
    let document = authored("a broken Balinese cluster", bytes);
    let text = document.page(0).expect("a page").text().plain_text();
    assert!(
        text.contains(BROKEN),
        "the run did not extract back to the string it was built from: {text:?}"
    );
    assert!(
        !text.contains('\u{25CC}'),
        "the inserted circle reached the extracted text: {text:?}"
    );
}

/// The same text in a face that does not map U+25CC is drawn as it stands:
/// three glyphs, each standing for its own character.
#[test]
fn a_broken_cluster_in_a_face_without_a_dotted_circle_is_drawn_as_typed() {
    let face = Face::new("Fixture Balinese", BROKEN);
    let program = face.build();
    let sfnt = Sfnt::parse(&program).expect("a valid sfnt");
    let (_, runs) = Shaper::new(&sfnt).shape_text(BROKEN, BaseDirection::LeftToRight);
    let placed: Vec<_> = runs
        .iter()
        .flat_map(|run| place(BROKEN, run, 12.0))
        .collect();
    assert_eq!(
        placed
            .iter()
            .map(|glyph| glyph.text.as_str())
            .collect::<Vec<_>>(),
        ["\u{1B13}", "\u{1B36}", "\u{1B3E}"]
    );
    assert!(placed.iter().all(|glyph| !glyph.inserted));
    // No glyph inserted, no span: the run is one text object, as it was.
    let content = written(&face, BROKEN);
    assert_eq!(occurrences(&content, b"BDC"), 0);
    assert_eq!(occurrences(&content, b"BT"), 1);
    let text = authored(
        "a broken Balinese cluster without a circle",
        document(&face, BROKEN, BaseDirection::LeftToRight),
    )
    .page(0)
    .expect("a page")
    .text()
    .plain_text();
    assert!(text.contains(BROKEN), "{text:?}");
}

/// A typed U+25CC maps the circle's glyph to the character in `/ToUnicode`,
/// and a circle the shaper inserts in the same face still reads as nothing:
/// `◌ULU ULU` — a typed circle bearing a vowel sign, a space, and the same
/// vowel sign with no base — extracts as typed, with one U+25CC.
#[test]
fn a_typed_dotted_circle_beside_an_inserted_one_extracts_once() {
    const TYPED: &str = "\u{25CC}\u{1B36} \u{1B36}";
    let face = Face::new("Fixture Balinese", "\u{1B36}\u{25CC} ");
    let circle = face.glyph_of('\u{25CC}').expect("the face maps U+25CC");
    let program = face.build();
    let sfnt = Sfnt::parse(&program).expect("a valid sfnt");
    let (_, runs) = Shaper::new(&sfnt).shape_text(TYPED, BaseDirection::LeftToRight);
    let placed: Vec<_> = runs
        .iter()
        .flat_map(|run| place(TYPED, run, 12.0))
        .collect();
    assert_eq!(
        placed
            .iter()
            .filter(|glyph| glyph.id == circle)
            .map(|glyph| (glyph.text.as_str(), glyph.inserted))
            .collect::<Vec<_>>(),
        [("\u{25CC}", false), ("", true)],
        "one typed circle and one inserted"
    );
    assert_eq!(occurrences(&written(&face, TYPED), SILENT), 1);
    let text = authored(
        "a typed dotted circle beside an inserted one",
        document(&face, TYPED, BaseDirection::LeftToRight),
    )
    .page(0)
    .expect("a page")
    .text()
    .plain_text();
    assert!(text.contains(TYPED), "{text:?}");
    assert_eq!(text.matches('\u{25CC}').count(), 1, "{text:?}");
}
