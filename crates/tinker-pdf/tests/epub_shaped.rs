//! Milestone 6 of `docs/design/shaping.md`: an Arabic book, paginated.
//!
//! The `Shaper` seam has existed since the milestone was opened and
//! `BookMetrics` has filled it, but the seam was only ever exercised on faces
//! with **no `GSUB`** — so nothing in this repository said that an embedded
//! face's run actually joins on the layout path, and nothing said which order
//! its glyphs were drawn in. This file is the demonstration the milestone asks
//! for: *"an Arabic EPUB fixture paginates with joined forms and RTL line
//! order, pinned by a render fingerprint"*.
//!
//! # What had to change for it, and it was not the seam
//!
//! `epub/paint.rs`'s `draw_run` resolved each character's face and then asked
//! that face's `cmap` for a glyph, one character at a time. That draws the
//! **isolated** form of every Arabic letter, in the order it was typed, while
//! the line it sits on was measured through the shaper — two measurement paths
//! disagreeing, which is the failure `tinker-pdf-layout`'s `metrics.rs` names.
//!
//! So drawing now walks the same segments measurement does (`face_runs`), and
//! an embedded face's segment is shaped whole: `GSUB` runs, UAX #9's rule L2
//! orders the runs, and a right-to-left run's glyphs are walked backwards.
//!
//! # Two limits this file does not hide
//!
//! - **Reordering is per face segment.** Fallback is resolved before shaping,
//!   because a glyph index means nothing outside its own face, so a
//!   right-to-left line whose characters need two faces is drawn in two
//!   left-to-right pieces. The fixture face below covers its space for exactly
//!   this reason, and `docs/features/fonts.md` records the limit.
//! - **`GPOS` offsets are not carried.** `PageBuilder::glyphs` writes one hex
//!   string at one origin, so a mark sits where its advance puts it rather
//!   than where its anchor does. The fixture face has no marks; the limit is
//!   recorded rather than demonstrated.

mod epub_support;

use epub_support::book::one_face_book;
use epub_support::typeface::{text_objects, Face, Form, Joining};
use tinker_pdf::{Document, OpenOptions, RenderOptions};

/// Beh, hah and meem: three Arabic letters that join on both sides, and a
/// space, which joins on neither.
///
/// The space is **covered by the fixture face on purpose**. Fallback is
/// resolved before shaping, so a space the face did not cover would split the
/// line into three segments and the two words would be drawn left to right
/// with only their letters reordered — a half-right page that this suite would
/// have had to describe as a pass.
const COVERS: &str = " \u{628}\u{62D}\u{645}";

/// Two Arabic words, written with the same three letters in opposite orders,
/// so the pair says both things at once: the **letters** of each word are
/// drawn last-to-first, and the **words** are too.
const LINE: &str = "\u{628}\u{62D}\u{645} \u{645}\u{62D}\u{628}";

/// The face: [`COVERS`], joining under `arab`.
fn arabic_face() -> Face {
    Face::new("Fixture Arabic", COVERS).with_joining(Joining { script: *b"arab" })
}

/// The page every test here lays the book into, in points.
///
/// Narrow on purpose: [`the_line_is_measured_the_way_it_is_drawn`] needs a
/// measure that falls **between** the two ways the same line can be measured,
/// so that which one the engine used decides how many lines come out.
const PAGE: (f64, f64) = (120.0, 400.0);

/// A book of one Arabic paragraph in that face.
fn arabic_book(body: &str) -> Vec<u8> {
    one_face_book("Fixture Arabic", &arabic_face().build(), 24, body)
}

/// The first page's content stream, through this repository's own reader.
fn page_content(doc: &Document) -> String {
    let cos = doc.cos();
    let pages = tinker_pdf_cos::pages::collect(cos);
    let page = pages.first().expect("one page");
    String::from_utf8_lossy(&tinker_pdf_cos::pages::content_bytes(cos, page)).into_owned()
}

/// The hex string of glyph indices one text object shows.
fn shown(object: &str) -> String {
    let at = object.find('<').expect("a hex string");
    let end = object[at..].find('>').expect("a closed hex string");
    object[at + 1..at + end].to_owned()
}

/// **The headline: the letters are joined, and the line is drawn backwards.**
///
/// Seven glyphs, and every one of them is an assertion:
///
/// - beh, hah and meem take their *initial*, *medial* and *final* forms, which
///   are three glyph indices the `cmap` alone never returns;
/// - the space keeps its isolated glyph, because `Joining_Type` `U` is what it
///   is and no joining feature is masked onto it;
/// - and the whole line comes out reversed — the second word first and each
///   word's last letter first — which is UAX #9's rule L2 applied to a run the
///   shaper handed back in logical order.
#[test]
fn an_arabic_paragraph_is_drawn_joined_and_right_to_left() {
    let face = arabic_face();
    let doc = Document::open(arabic_book(LINE)).expect("a book");
    let content = page_content(&doc);
    let objects = text_objects(&content);
    assert_eq!(
        objects.len(),
        1,
        "the line is one face and must be one text object: {content}"
    );

    let form = |ch: char, form: Form| {
        face.form_glyph(ch, form)
            .unwrap_or_else(|| panic!("{ch:?} has no {form:?} form"))
    };
    let space = face.glyph_of(' ').expect("the face covers its space");
    // Logical: init(beh) medi(hah) fina(meem) space init(meem) medi(hah)
    // fina(beh). Drawn: that, reversed.
    let logical = [
        form('\u{628}', Form::Initial),
        form('\u{62D}', Form::Medial),
        form('\u{645}', Form::Final),
        space,
        form('\u{645}', Form::Initial),
        form('\u{62D}', Form::Medial),
        form('\u{628}', Form::Final),
    ];
    let expected: String = logical
        .iter()
        .rev()
        .map(|glyph| format!("{glyph:04X}"))
        .collect();
    assert_eq!(
        shown(&objects[0].1),
        expected,
        "the paragraph is not drawn joined and reversed: {content}"
    );

    // And the isolated forms are absent, which is what makes the assertion
    // above about shaping rather than about encoding.
    for ch in "\u{628}\u{62D}\u{645}".chars() {
        let plain = face.glyph_of(ch).expect("the face covers the letter");
        assert!(
            !shown(&objects[0].1).contains(&format!("{plain:04X}")),
            "the unjoined form of {ch:?} reached the page: {content}"
        );
    }
}

/// **One path owns the run**, and the page is where it shows.
///
/// The fixture face here makes its joining forms a fifth of the width of its
/// plain glyphs, so the two measurements of the same line differ by a factor
/// of nearly five. The measure is chosen between them: a build that measured
/// the line the shaper's way sets it as **one** line, and a build that summed
/// `Metrics::advance` a character at a time thinks it is too wide and breaks
/// it into two.
///
/// That is the failure `tinker-pdf-layout`'s `metrics.rs` warns about, made
/// visible: *"a ligature is narrower than its components and a joined Arabic
/// word is narrower still"*.
#[test]
fn the_line_is_measured_the_way_it_is_drawn() {
    let face = arabic_face().with_advance(500).with_joined_advance(100);
    let book = one_face_book("Fixture Arabic", &face.build(), 24, LINE);
    let doc = Document::open_with(book, &OpenOptions::at_page(PAGE.0, PAGE.1)).expect("a book");
    let content = page_content(&doc);
    let objects = text_objects(&content);
    assert_eq!(
        objects.len(),
        1,
        "the line was broken, so it was measured a character at a time and \
         not the way it is drawn: {content}"
    );
}

/// **The book paginates, and the text survives.**
///
/// A page that drew the right glyphs and lost the words would be a book that
/// looks right and searches as nothing — the failure milestone 7's round trip
/// exists for, met here from the layout side.
#[test]
fn the_arabic_book_paginates_and_its_text_extracts() {
    let doc = Document::open(arabic_book(LINE)).expect("a book");
    assert_eq!(doc.page_count(), 1, "one paragraph is one page");
    let text = doc.page(0).expect("a page").text().plain_text();
    for ch in LINE.chars().filter(|c| *c != ' ') {
        assert!(
            text.contains(ch),
            "{ch:?} did not survive into the page's text: {text:?}"
        );
    }
}

/// The rendered page, hashed — ruling 4's contract over the shaped path.
///
/// # When this fails
///
/// The same two readings `crates/tinker-pdf/tests/determinism.rs` gives, and
/// they need opposite responses: **the same target renders differently** means
/// a deliberate change to shaping or drawing, and the hash moves in the commit
/// that caused it; **two targets disagree** is a determinism bug and the hash
/// is not the thing to change.
///
/// The ink floor is here for that file's reason too: a page that stopped
/// drawing would otherwise become the new baseline in perfect silence, and a
/// blank page is extremely stable.
const SHAPED_PAGE: &str = "ac015d584f0e9cee9bf89695ad632761f7ff619c17dc8d5d60511e9f8ff56d4b";

/// The least ink [`SHAPED_PAGE`] may be the hash of.
const LEAST_INK: usize = 200;

#[test]
fn the_shaped_page_renders_to_the_bytes_it_always_did() {
    let doc = Document::open(arabic_book(LINE)).expect("a book");
    let bitmap = doc
        .page(0)
        .expect("a page")
        .render(&RenderOptions::default());
    let drawn = bitmap
        .data
        .chunks_exact(3)
        .filter(|pixel| pixel.iter().any(|c| *c != 0xFF))
        .count();
    assert!(
        drawn >= LEAST_INK,
        "the shaped page painted {drawn} pixels, fewer than the {LEAST_INK} \
         it is supposed to: its fingerprint is not evidence about anything \
         until that is fixed. Warnings: {:?}",
        bitmap.warnings
    );

    let mut input = Vec::with_capacity(bitmap.data.len() + 8);
    input.extend_from_slice(&bitmap.width.to_be_bytes());
    input.extend_from_slice(&bitmap.height.to_be_bytes());
    input.extend_from_slice(&bitmap.data);
    let hash: String = tinker_pdf_crypto::sha2::sha256(&input)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    assert_eq!(
        hash, SHAPED_PAGE,
        "the shaped Arabic page renders differently. Two targets disagreeing \
         is a determinism bug and this number is not the thing to change; a \
         deliberate change to shaping moves it in the commit that caused it."
    );
}
