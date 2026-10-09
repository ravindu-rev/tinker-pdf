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
//! # Two limits that closed, and each needed a fixture that did not exist
//!
//! **`GPOS` offsets were not carried.** `PageBuilder::glyphs` writes one hex
//! string at one origin, so a mark sat where its advance put it rather than
//! where its anchor did — a vowelled Arabic or Devanagari book rendered wrong
//! while every test here passed. Drawing goes through
//! `DocumentBuilder::glyph_run` now, which states each glyph's own position;
//! [`a_positioned_glyph_is_drawn_where_its_anchor_puts_it`] is the
//! demonstration. It needed a **new face**, because the Arabic one below has no
//! marks: the page a build that dropped every offset drew was byte for byte the
//! page a build that carried them drew, and [`SHAPED_PAGE`] did not move when
//! the defect was fixed.
//!
//! **Reordering was per face segment.** Fallback is resolved before shaping,
//! because a glyph index means nothing outside its own face, so a right-to-left
//! line whose characters need two faces was drawn as two left-to-right pieces,
//! each internally correct. The Arabic face below covers its own space **for
//! exactly that reason** — the fixture was arranged around the limit rather
//! than testing past it — so closing it needed a two-face fixture as well:
//! [`a_right_to_left_line_in_two_faces_is_drawn_right_to_left`], with a
//! left-to-right control beside it.
//!
//! # Two more that closed in October 2026, both about a styled span
//!
//! **The unit of reordering was the `TextRun`, not the visual line.**
//! `flow.rs` breaks lines over logical text and resolves no levels, so a
//! right-to-left line made of two styled spans was two runs laid left to
//! right in the order written. `paint::visual_lines` now resolves UAX #9 over
//! each visual line's whole text and lays its runs out again in rule L2's
//! order, before anything reads a position:
//! [`a_right_to_left_line_of_two_styled_spans_is_drawn_right_to_left`], with
//! a left-to-right control.
//!
//! **Shaping below the run's own level.** A run shaped alone sees nothing
//! either side of it, so a word with a coloured letter was three isolated
//! letters and a glyph positioned by its neighbour lost the position. The
//! painter shapes each run against its neighbours' text in the same face and
//! draws its own glyphs: [`a_word_split_by_a_span_is_drawn_joined`] (GSUB) and
//! [`a_pair_across_a_span_boundary_is_positioned`] (a `GPOS` `PairPos` offset).
//!
//! # Corrected on review, the same month
//!
//! The context reached too far: a neighbour within a font size of the run's
//! baseline is the **next line** at a `line-height` of 1 or less, so a word
//! ending a line was joined to the line below it
//! ([`a_word_ending_a_line_is_not_joined_to_the_next_line`]); a context is now
//! a neighbour the run touches on its line. A context in the other direction
//! could split the run's own glyphs and overprint its neighbour's
//! ([`a_styled_letter_in_a_mixed_line_overprints_nothing`]); such a run is
//! shaped alone. And `word-spacing`, which every justified line but the last
//! carries, drew a right-to-left run's words in written order
//! ([`a_word_spaced_right_to_left_run_draws_its_words_right_to_left`],
//! [`a_justified_arabic_paragraph_reads_in_order`]).
//!
//! # Closed in October 2026's eighth wave: a run that mixes directions
//!
//! A run was the unit of the line's reordering, ordered inside itself by its
//! own P2 and P3, so a span boundary inside a word of the other direction
//! (`a ب<span>ح</span>م b`) left that word drawn in the order it was written.
//! Runs are cut at their line's level boundaries before the line is ordered
//! (`paint::split_at_levels`), each piece carrying its share of the run's
//! measured width:
//! [`a_span_inside_a_word_of_the_other_direction_keeps_the_word_in_order`],
//! with a number in a right-to-left run and a joiner inside a word as its
//! controls. The cut draws a right-to-left word of three text objects in
//! reading order inside a left-to-right line, which the extractor read as
//! three lines on one baseline; ruling 14's reader now joins such pieces
//! before ordering them. And a context in the other paragraph direction no
//! longer turns a run round: a slice is shaped in its own direction, whatever
//! its neighbours' text would make of the whole.
//!
//! # And the measurement, the same wave
//!
//! Layout **measured** each run alone — its `Shaper` seam took no context —
//! so a context that changed an *advance* (a joined form wider than the
//! isolated one, a pair that kerns) left that difference between the run and
//! the next. The seam takes the run's neighbours on its line now
//! (`Shaper::shape_in`), and `BookMetrics` shapes against them by the
//! painter's own rule:
//! [`a_kerned_pair_across_a_span_is_measured_kerned`],
//! [`a_joined_form_across_a_span_is_measured_joined`],
//! [`the_line_breaker_measures_a_kerned_pair_across_a_span`] and
//! [`a_mixed_line_is_cut_and_measured_in_one_context`], every expected
//! position worked from the fixture face's `hmtx` and `GPOS` values.

mod epub_support;

use epub_support::book::{faces_book, faces_book_with, one_face_book};
use epub_support::typeface::{
    origin_of, shown_glyphs, text_objects, Face, Form, Joining, Kern, Ligature, Pair, Placement,
};
use tinker_pdf::{Document, OpenOptions, RenderOptions, TextOptions};

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
        shown_glyphs(&objects[0].1),
        expected,
        "the paragraph is not drawn joined and reversed: {content}"
    );

    // And the isolated forms are absent, which is what makes the assertion
    // above about shaping rather than about encoding.
    for ch in "\u{628}\u{62D}\u{645}".chars() {
        let plain = face.glyph_of(ch).expect("the face covers the letter");
        assert!(
            !shown_glyphs(&objects[0].1).contains(&format!("{plain:04X}")),
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

// ---- a right-to-left line in two faces --------------------------------------

/// What the first face of the two-face pair covers: beh and hah.
const FIRST_HALF: &str = "\u{628}\u{62D}";

/// And the second: meem and noon. **Disjoint from [`FIRST_HALF`] on purpose** —
/// `css-fonts-4` §5.3 walks the family list per character and takes the first
/// family that covers it, so a letter in both would never reach the second
/// face and the line would be one segment again.
const SECOND_HALF: &str = "\u{645}\u{646}";

/// Four Arabic letters, the first two in one face and the last two in another.
const SPLIT_LINE: &str = "\u{628}\u{62D}\u{645}\u{646}";

/// The same four letters' Latin stand-in, for the control: one run, two faces,
/// and a direction that must **not** reorder anything.
const LTR_LINE: &str = "abcd";

/// A book set in two faces, listed in that order.
fn two_face_book(first: &str, second: &str, body: &str) -> Vec<u8> {
    let alpha = Face::new("Fixture Alpha", first)
        .with_joining(Joining { script: *b"arab" })
        .build();
    let beta = Face::new("Fixture Beta", second)
        .with_joining(Joining { script: *b"arab" })
        .build();
    faces_book(
        &[("Fixture Alpha", &alpha), ("Fixture Beta", &beta)],
        24,
        body,
    )
}

/// **A right-to-left line needing two faces is drawn right to left.**
///
/// The limit this file used to name, closed. Font fallback is resolved before
/// shaping — a glyph index means nothing outside its own face — so a line whose
/// characters need two faces is two segments, and UAX #9's rule L2 had been
/// applied *inside* each of them and to neither of them. Each piece was
/// internally correct and the line read backwards, which is the shape of defect
/// that survives inspection: every glyph is the right glyph.
///
/// The Arabic fixture above hid it, and said so: its face covers its own space
/// **precisely so that the line stays one segment**. That is a test arranged
/// around a limit rather than one that tests past it, and this is the pair that
/// tests past it.
///
/// The claim is on the **order of the objects and their origins**, not on the
/// glyphs: each face numbers its own from 1, so the two segments draw the same
/// indices and only where they sit says which came first.
#[test]
fn a_right_to_left_line_in_two_faces_is_drawn_right_to_left() {
    let doc = Document::open(two_face_book(FIRST_HALF, SECOND_HALF, SPLIT_LINE)).expect("a book");
    let content = page_content(&doc);
    let objects = text_objects(&content);
    assert_eq!(
        objects.len(),
        2,
        "two faces are two text objects: {content}"
    );
    assert_eq!(
        (objects[0].0.as_str(), objects[1].0.as_str()),
        ("Bf1", "Bf0"),
        "the two faces were drawn in logical order, so the line reads \
         backwards: {content}"
    );
    let (first, _) = origin_of(&objects[0].1);
    let (second, _) = origin_of(&objects[1].1);
    assert!(
        first < second,
        "the second face's letters are not to the left of the first's: {content}"
    );

    // **And the line reads forwards.** Ruling 14 (`docs/rulings.md`): the
    // page is drawn in visual order — that is what makes it right — and text
    // is extracted in logical order, by UAX #9's rule L2 applied to the line
    // as drawn. This assertion used to pin the reverse, with the decision it
    // needed named and not taken; the ruling is the decision.
    let page = doc.page(0).expect("a page");
    let extracted = page.text().plain_text();
    assert_eq!(
        extracted.trim_end(),
        SPLIT_LINE,
        "a right-to-left line drawn in visual order does not extract in \
         reading order"
    );
    // And the opt-out still says what the content stream says: the line as
    // drawn, last letter first.
    let drawn = page
        .text_with(&TextOptions {
            content_order: true,
        })
        .plain_text();
    let reversed: String = SPLIT_LINE.chars().rev().collect();
    assert_eq!(
        drawn.trim_end(),
        reversed,
        "content order is no longer the order the line was drawn in"
    );
}

/// **The one-face line reads forwards too**, with a line that is not a
/// palindrome.
///
/// [`LINE`] is the same three letters in both orders, so it reads the same
/// either way and could never have shown which order extraction used. This
/// one can: beh, hah, meem, noon — four letters in one face, drawn joined and
/// last letter first.
#[test]
fn a_one_face_arabic_line_extracts_in_reading_order() {
    let face = Face::new("Fixture Arabic", SPLIT_LINE).with_joining(Joining { script: *b"arab" });
    let doc = Document::open(one_face_book(
        "Fixture Arabic",
        &face.build(),
        24,
        SPLIT_LINE,
    ))
    .expect("a book");
    let content = page_content(&doc);
    let objects = text_objects(&content);
    assert_eq!(objects.len(), 1, "one face, one text object: {content}");
    // Drawn last letter first, which is what makes the page right.
    let drawn: String = SPLIT_LINE
        .chars()
        .rev()
        .map(|ch| {
            let form = match ch {
                '\u{646}' => Form::Final,
                '\u{628}' => Form::Initial,
                _ => Form::Medial,
            };
            format!(
                "{:04X}",
                face.form_glyph(ch, form)
                    .unwrap_or_else(|| panic!("{ch:?} has no {form:?} form"))
            )
        })
        .collect();
    assert_eq!(shown_glyphs(&objects[0].1), drawn, "{content}");
    let extracted = doc.page(0).expect("a page").text().plain_text();
    assert_eq!(
        extracted.trim_end(),
        SPLIT_LINE,
        "a joined Arabic line drawn right to left does not extract in reading order"
    );
}

/// **And a left-to-right line in two faces is not.**
///
/// The control, and the pair proves nothing without it: a build that reversed
/// every multi-face run would pass the test above and set every English
/// sentence whose accented letter fell to a second face backwards. The
/// direction has to be read from the text.
#[test]
fn a_left_to_right_line_in_two_faces_keeps_its_order() {
    let doc = Document::open(two_face_book("ab", "cd", LTR_LINE)).expect("a book");
    let content = page_content(&doc);
    let objects = text_objects(&content);
    assert_eq!(
        objects.len(),
        2,
        "two faces are two text objects: {content}"
    );
    assert_eq!(
        (objects[0].0.as_str(), objects[1].0.as_str()),
        ("Bf0", "Bf1"),
        "a left-to-right line was reordered: {content}"
    );
}

// ---- GPOS reaches the page --------------------------------------------------

/// The three Latin letters the positioning fixture is written with.
const MARKED_COVERS: &str = "ABC";

/// The text it draws: an ordinary letter, a displaced one, an ordinary one.
///
/// Three and not two. The letter **before** the displaced one is what says the
/// offset moved that glyph and not the whole run, and the letter **after** it
/// is what says the pen came back — an offset that shifted everything from
/// there on would satisfy a two-letter test perfectly.
const MARKED_LINE: &str = "ABC";

/// How far `B` is displaced along the baseline, in font units at 1000 to the
/// em.
const X_PLACEMENT: i16 = 250;

/// And off it. Chosen **above half the font size** on purpose: text extraction
/// treats a glyph half an em off the line's baseline as a new line, so a rise
/// smaller than that would make
/// [`the_positioned_page_still_extracts_as_one_line`] pass without saying
/// anything. At 24px — 18pt — half an em is 9pt and this is 10.8.
const Y_PLACEMENT: i16 = 600;

/// A face covering [`MARKED_COVERS`] whose `B` is displaced by a `GPOS`
/// `SinglePos`.
///
/// `DFLT` and `kern`: the script tag every run falls back to, and a feature
/// the default shaper turns on. A `SinglePos` under `kern` is unusual and
/// perfectly legal — a feature tag names an intention and a lookup type names
/// a mechanism, and nothing binds one to the other.
fn placed_face(x: i16, y: i16) -> Face {
    Face::new("Fixture Marks", MARKED_COVERS).with_placement(Placement {
        ch: 'B',
        script: *b"DFLT",
        feature: *b"kern",
        x,
        y,
    })
}

fn placed_book(x: i16, y: i16) -> Vec<u8> {
    one_face_book("Fixture Marks", &placed_face(x, y).build(), 24, MARKED_LINE)
}

/// **A `GPOS` offset reaches the page**, as 9.4.3's own arithmetic.
///
/// This is the demonstration the rest of this file could not give. The face
/// above has no marks — nor has any other fixture in this repository — so a
/// build that dropped every positioning offset on the floor drew exactly the
/// bytes a build that carried them did, and
/// [`SHAPED_PAGE`] did not move when the defect was fixed. That is what a
/// silent correctness defect looks like from inside a green suite.
///
/// What this proves is the **transport**: that a non-zero `x_offset` a shaper
/// produced becomes a number in the content stream that moves that glyph and
/// leaves the pen where it was. It proves nothing about anchor arithmetic and
/// does not try to — the aots corpus adjudicates `GPOS` lookup types 1 to 9
/// case by case, and `text-rendering-tests`' GPOS-3 and GPOS-4 sections
/// adjudicate mark-to-base and mark-to-mark against real faces carrying their
/// own expected positions.
///
/// The numbers are exact and worked out here rather than read back:
/// `X_PLACEMENT` is 250 units at 1000 to the em, and 9.4.3 measures a `TJ`
/// adjustment in thousandths of the em, so the displacement is **−250 whatever
/// the font size is** — the size cancels. The `+250` after it is the same
/// number undone, which is what makes the pen the reader's pen.
#[test]
fn a_positioned_glyph_is_drawn_where_its_anchor_puts_it() {
    let face = placed_face(X_PLACEMENT, 0);
    let doc = Document::open(placed_book(X_PLACEMENT, 0)).expect("a book");
    let content = page_content(&doc);
    let objects = text_objects(&content);
    assert_eq!(objects.len(), 1, "one face is one text object: {content}");

    let glyph = |ch: char| face.glyph_of(ch).expect("the face covers the letter");
    assert_eq!(
        shown_glyphs(&objects[0].1),
        format!("{:04X}{:04X}{:04X}", glyph('A'), glyph('B'), glyph('C')),
        "the three letters are not drawn in order: {content}"
    );
    // One `TJ` array, because nothing asked to leave the baseline: the offset
    // glyph is displaced by a number inside it and the one after it puts the
    // pen back.
    assert!(
        objects[0].1.contains(&format!(
            "[<{:04X}> -{X_PLACEMENT} <{:04X}> {X_PLACEMENT} <{:04X}>] TJ",
            glyph('A'),
            glyph('B'),
            glyph('C')
        )),
        "the offset did not reach the page as a TJ adjustment: {content}"
    );
}

/// **A vertical offset becomes a rise, and the rise is put back.**
///
/// `Ts` is text *state* and cannot go inside a `TJ` array, so a run that
/// leaves the baseline is more than one array — and `Ts` outlives `ET`, so a
/// run that set one and did not clear it would tilt whatever the page drew
/// next. Both halves are asserted, because a build that wrote the rise and
/// forgot the `0 Ts` produces a page whose *first* paragraph is right.
#[test]
fn a_vertical_offset_becomes_a_rise_and_is_put_back() {
    let doc = Document::open(placed_book(X_PLACEMENT, Y_PLACEMENT)).expect("a book");
    let content = page_content(&doc);
    let objects = text_objects(&content);
    assert_eq!(objects.len(), 1, "one face is one text object: {content}");

    // 600 units at 1000 to the em, at 24px, which is 18pt: 600 × 18 / 1000.
    let rise = f64::from(Y_PLACEMENT) * 18.0 / 1000.0;
    assert!(
        objects[0].1.contains(&format!("{rise} Ts")),
        "the y offset did not become a rise: {content}"
    );
    assert!(
        objects[0].1.contains("0 Ts"),
        "the rise was never cleared, so it outlives this text object: {content}"
    );
    let last_set = objects[0]
        .1
        .rfind(&format!("{rise} Ts"))
        .expect("a rise was set");
    let cleared = objects[0].1.rfind("0 Ts").expect("a rise was cleared");
    assert!(
        cleared > last_set,
        "the rise is cleared before it is set, so it survives the object: {content}"
    );
}

/// **And the page is still one line.**
///
/// The rise is 10.8 points against a font size of 18, which is more than half
/// an em — the distance text extraction reads as a different baseline. So a
/// build that decided line membership from where the ink is would report three
/// lines here: `A`, then `B` on a line of its own, then `C` on a third,
/// because a line remembers the previous glyph's origin and the rise both
/// starts and stops.
///
/// This is the other half of the same defect and it is why the extraction
/// change came first: carrying the offsets onto the page without it would have
/// traded a book that renders wrong for a book that renders right and extracts
/// in pieces.
#[test]
fn the_positioned_page_still_extracts_as_one_line() {
    let doc = Document::open(placed_book(X_PLACEMENT, Y_PLACEMENT)).expect("a book");
    let text = doc.page(0).expect("a page").text();
    assert_eq!(
        text.lines().len(),
        1,
        "the rise split the line: {:?}",
        text.plain_text()
    );
    // And the round trip is intact: `/ToUnicode` gives back what the book
    // said, which a page of correctly placed glyphs that could not be read
    // would not.
    assert_eq!(text.plain_text(), format!("{MARKED_LINE}\n"));
}

// ---- letter-spacing is in the positions, not in `Tc` ------------------------

/// A book of [`MARKED_LINE`] in a face with no `GPOS`, at `spacing` CSS pixels
/// of `letter-spacing`.
///
/// The spacing arrives through a `style=""` attribute rather than the sheet
/// [`epub_support::book`] writes, because that is the one place a test can put
/// a declaration on the run without the builder growing a parameter every
/// property would need.
fn spaced_book(spacing: u32) -> Vec<u8> {
    let face = Face::new("Fixture Marks", MARKED_COVERS).build();
    one_face_book(
        "Fixture Marks",
        &face,
        24,
        &format!(r#"<span style="letter-spacing: {spacing}px">{MARKED_LINE}</span>"#),
    )
}

/// **`letter-spacing` is folded into the glyph positions and `Tc` is zero.**
///
/// Not a preference, and two independent reasons say so.
///
/// `DocumentBuilder::glyph_run` works out each `TJ` adjustment from a pen it
/// advances by the font's own `/W`, and that pen knows nothing about `Tc` — so
/// a non-zero one would push glyph *k* by *k* × `Tc` past where the caller put
/// it, and every offset this file is about would be wrong by a growing amount.
///
/// And `Tc` is applied by a reader per **glyph** while `tinker-pdf-layout`
/// measures `letter_spacing × chars().count()` per **character**, so a
/// ligature or a joined Arabic word — fewer glyphs than characters — was drawn
/// narrower than the line box it was measured into. Folding at cluster
/// boundaries makes the drawn width the measured width by construction, and
/// keeps a mark on its base rather than spacing it away.
///
/// Six CSS pixels is 4.5 points, and 9.4.3 measures a `TJ` adjustment in
/// thousandths of the em: at 18 points that is **250**, once per character
/// that has gone by. The control below draws the same three letters with no
/// spacing at all and must have no adjustment in it, because a test that only
/// looked for `0 Tc` would pass on a build that dropped the spacing entirely.
#[test]
fn letter_spacing_is_folded_into_the_positions() {
    let doc = Document::open(spaced_book(6)).expect("a book");
    let content = page_content(&doc);
    let objects = text_objects(&content);
    assert_eq!(objects.len(), 1, "one face is one text object: {content}");
    assert!(
        content.contains("0 Tc"),
        "a Tc was written, so the spacing is charged twice: {content}"
    );
    assert!(
        objects[0].1.contains("[<0001> -250 <0002> -250 <0003>] TJ"),
        "the spacing did not reach the glyph positions: {content}"
    );

    let plain = Document::open(spaced_book(0)).expect("a book");
    let plain = page_content(&plain);
    let plain = text_objects(&plain);
    assert!(
        plain[0].1.contains("[<0001><0002><0003>] TJ"),
        "an unspaced run carries an adjustment, so the pair proves nothing: {}",
        plain[0].1
    );
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
/// *Moved once, 15 September 2026.* `fill` stopped truncating an edge's slope
/// to a whole 1/256 pixel per sub-scanline, which had been flattening every
/// shallow diagonal to vertical; the Arabic page is drawn from an embedded
/// outline face and every curve on it moved with it. Eight pages in
/// `determinism.rs` moved in the same commit and eleven did not, which is the
/// shape that says it was the scanline filler and nothing above it.
const SHAPED_PAGE: &str = "0d66ea167e51a1a0145cefcaeae2c4aea32c4ea14daffc0e686b70b31ac4cabe";

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

// ---- a right-to-left line of two styled spans --------------------------------

/// The first word of [`LINE`], with its space: what the first span holds.
const FIRST_WORD: &str = "\u{628}\u{62D}\u{645} ";

/// The second word, which a span colours: a different `TextRun`.
const SECOND_WORD: &str = "\u{645}\u{62D}\u{628}";

/// The glyphs one word of the Arabic face draws joined, last letter first —
/// what a right-to-left word's text object shows.
fn drawn_word(face: &Face, word: &str) -> String {
    let letters: Vec<char> = word.chars().filter(|c| *c != ' ').collect();
    let forms: Vec<u16> = letters
        .iter()
        .enumerate()
        .map(|(at, ch)| {
            let form = if at == 0 {
                Form::Initial
            } else if at + 1 == letters.len() {
                Form::Final
            } else {
                Form::Medial
            };
            face.form_glyph(*ch, form)
                .unwrap_or_else(|| panic!("{ch:?} has no {form:?} form"))
        })
        .collect();
    forms
        .iter()
        .rev()
        .map(|glyph| format!("{glyph:04X}"))
        .collect()
}

/// **A right-to-left line of two styled spans is drawn right to left.**
///
/// The limit this file named last, closed. A span is a `TextRun` of its own,
/// and layout places a line's runs left to right in the order they were
/// written, so the coloured second word used to be drawn to the **right** of
/// the first: every glyph right and the line backwards. `paint::visual_lines`
/// resolves UAX #9 over the line's whole text and lays its runs out again in
/// rule L2's order, so the second word is now the leftmost text object on the
/// line, and the line reads forwards when extracted.
///
/// The two words draw the same three letters in opposite orders, so each text
/// object is told apart by its glyphs rather than by its font, which both
/// share.
#[test]
fn a_right_to_left_line_of_two_styled_spans_is_drawn_right_to_left() {
    let face = arabic_face();
    let body = format!(r#"{FIRST_WORD}<span style="color: #c00000">{SECOND_WORD}</span>"#);
    let doc = Document::open(arabic_book(&body)).expect("a book");
    let content = page_content(&doc);
    let objects = text_objects(&content);
    let first = drawn_word(&face, FIRST_WORD);
    let second = drawn_word(&face, SECOND_WORD);
    let find = |word: &str| {
        objects
            .iter()
            .find(|(_, object)| shown_glyphs(object).contains(word))
            .map(|(_, object)| origin_of(object).0)
            .unwrap_or_else(|| panic!("no text object draws {word}: {content}"))
    };
    let (first_x, second_x) = (find(&first), find(&second));
    assert!(
        second_x < first_x,
        "the second span is not to the left of the first, so the line reads \
         backwards ({second_x} against {first_x}): {content}"
    );
    let extracted = doc.page(0).expect("a page").text().plain_text();
    assert_eq!(
        extracted.trim_end(),
        format!("{FIRST_WORD}{SECOND_WORD}"),
        "the line does not extract in reading order"
    );
}

/// **And a left-to-right line of two styled spans is not reordered.**
///
/// The control: a build that reversed every multi-span line would pass the
/// test above and set every English sentence with a bold word in it
/// backwards. The face covers Latin letters for it, and the spans are told
/// apart by their glyphs.
#[test]
fn a_left_to_right_line_of_two_styled_spans_keeps_its_order() {
    let face = Face::new("Fixture Arabic", "ab cd");
    let doc = Document::open(one_face_book(
        "Fixture Arabic",
        &face.build(),
        24,
        r#"ab <span style="color: #c00000">cd</span>"#,
    ))
    .expect("a book");
    let content = page_content(&doc);
    let objects = text_objects(&content);
    let glyph = |ch: char| format!("{:04X}", face.glyph_of(ch).expect("covered"));
    let find = |run: &str| {
        objects
            .iter()
            .find(|(_, object)| shown_glyphs(object).contains(run))
            .map(|(_, object)| origin_of(object).0)
            .unwrap_or_else(|| panic!("no text object draws {run}: {content}"))
    };
    let ab = find(&format!("{}{}", glyph('a'), glyph('b')));
    let cd = find(&format!("{}{}", glyph('c'), glyph('d')));
    assert!(ab < cd, "a left-to-right line was reordered: {content}");
}

/// **An English line holding an Arabic word reads back as it was written.**
///
/// The painter resolves the line by P2, so `a`, then the word, then `b` is a
/// left-to-right paragraph and the word alone is reversed on the page. The
/// reader used to take the line's direction from a count of its letters —
/// three Arabic against two Latin — and read the whole line as right to left:
/// `b بحم a` (review of lane 6C). It reads the direction off the drawn line
/// now, and both ends are Latin.
#[test]
fn an_english_line_holding_an_arabic_word_reads_back_as_written() {
    let face = Face::new("Fixture Arabic", " ab\u{628}\u{62D}\u{645}")
        .with_joining(Joining { script: *b"arab" });
    let written = "a \u{628}\u{62D}\u{645} b";
    let doc = Document::open(one_face_book("Fixture Arabic", &face.build(), 24, written))
        .expect("a book");
    let extracted = doc.page(0).expect("a page").text().plain_text();
    assert_eq!(extracted.trim_end(), written);
}

// ---- what a run's context may and may not reach -----------------------------

/// Every page's content stream, in page order.
fn every_page_content(doc: &Document) -> Vec<String> {
    let cos = doc.cos();
    tinker_pdf_cos::pages::collect(cos)
        .iter()
        .map(|page| {
            String::from_utf8_lossy(&tinker_pdf_cos::pages::content_bytes(cos, page)).into_owned()
        })
        .collect()
}

/// How many times `glyph` is shown, over every text object of every page.
fn times_shown(doc: &Document, glyph: u16) -> usize {
    let wanted = format!("{glyph:04X}");
    every_page_content(doc)
        .iter()
        .flat_map(|content| text_objects(content))
        .map(|(_, object)| {
            let shown = shown_glyphs(&object);
            shown
                .as_bytes()
                .chunks(4)
                .filter(|chunk| *chunk == wanted.as_bytes())
                .count()
        })
        .sum()
}

/// **A word that ends a line is not joined to the word that starts the
/// next** (review of lane 6C).
///
/// Sixty copies of `بحم`, each a word of its own, so meem is word-final
/// sixty times. A run's shaping context used to be any neighbour whose
/// baseline was within a font size of its own, and at a `line-height` of 1 or
/// less the next line's baseline is: the last run of one line was shaped with
/// the first of the next as its context, and the meem ending the line came out
/// **medial**, joined across the break. Context is now a neighbour on the same
/// baseline that the run touches on the page.
#[test]
fn a_word_ending_a_line_is_not_joined_to_the_next_line() {
    let face = arabic_face();
    let words = vec!["\u{628}\u{62D}\u{645}"; 60].join(" ");
    let medial = face
        .form_glyph('\u{645}', Form::Medial)
        .expect("meem has a medial form");
    let fin = face
        .form_glyph('\u{645}', Form::Final)
        .expect("meem has a final form");
    for line_height in ["1", "0.8", "0", "1.5"] {
        let body = format!(r#"x</p><p style="line-height: {line_height}">{words}"#);
        let doc = Document::open(arabic_book(&body)).expect("a book");
        assert_eq!(
            (times_shown(&doc, medial), times_shown(&doc, fin)),
            (0, 60),
            "line-height {line_height}: (medial, final) meems; a medial one is a \
             line's last letter joined to the next line"
        );
    }
}

/// **A styled letter in an Arabic word inside an English line overprints
/// nothing** (review of lane 6C).
///
/// The first run, `a ب`, was shaped with `حم b` after it, and in the visual
/// order of that whole text the context's `م` and `ح` sit between the run's
/// own space and its `ب`. Its glyphs were placed from the pen at the first of
/// them, so the `ب` was pushed past the run's box onto the `ح` the next run
/// draws: two glyphs in one box. A run whose own glyphs a context would split
/// is shaped alone now.
#[test]
fn a_styled_letter_in_a_mixed_line_overprints_nothing() {
    let face = Face::new("Fixture Arabic", " ab\u{628}\u{62D}\u{645}")
        .with_joining(Joining { script: *b"arab" });
    let body = "a \u{628}<span style=\"color: #c00000\">\u{62D}</span>\u{645} b";
    let doc =
        Document::open(one_face_book("Fixture Arabic", &face.build(), 24, body)).expect("a book");
    let page = doc.page(0).expect("a page");
    let text = page.text_with(&TextOptions {
        content_order: true,
    });
    let boxes: Vec<(String, f64, f64)> = text
        .lines()
        .iter()
        .flat_map(|line| line.chars.iter())
        .filter(|c| c.text != " ")
        .map(|c| {
            let (x0, _, x1, _) = c.quad.bounds();
            (c.text.clone(), x0, x1)
        })
        .collect();
    assert_eq!(boxes.len(), 5, "{boxes:?}");
    for (at, a) in boxes.iter().enumerate() {
        for b in &boxes[at + 1..] {
            let overlap = a.2.min(b.2) - a.1.max(b.1);
            assert!(
                overlap < 0.01,
                "{a:?} and {b:?} are drawn over each other: {boxes:?}"
            );
        }
    }
}

// ---- word spacing inside a right-to-left run ----------------------------------

/// **A right-to-left run with `word-spacing` draws its words right to left**
/// (review of lane 6C).
///
/// `word-spacing` splits a shaped run at its spaces so each space can be paid
/// for, and the pieces were drawn left to right in the order they were
/// written: the second word to the right of the first, and the line reading
/// backwards. The pieces are laid out in UAX #9's order now.
#[test]
fn a_word_spaced_right_to_left_run_draws_its_words_right_to_left() {
    let face = arabic_face();
    let body = format!(r#"<span style="word-spacing: 4px">{}</span>"#, LINE);
    let doc = Document::open(arabic_book(&body)).expect("a book");
    let content = page_content(&doc);
    let objects = text_objects(&content);
    let first = drawn_word(&face, FIRST_WORD);
    let second = drawn_word(&face, SECOND_WORD);
    let find = |word: &str| {
        objects
            .iter()
            .find(|(_, object)| shown_glyphs(object).contains(word))
            .map(|(_, object)| origin_of(object).0)
            .unwrap_or_else(|| panic!("no text object draws {word}: {content}"))
    };
    let (first_x, second_x) = (find(&first), find(&second));
    assert!(
        second_x < first_x,
        "the second word is not to the left of the first ({second_x} against \
         {first_x}): {content}"
    );
    let extracted = doc.page(0).expect("a page").text().plain_text();
    assert_eq!(extracted.trim_end(), LINE);

    // And the extra space is **between** the words, where the space is: the
    // gap from the left word's last glyph to the right word's first is the
    // space's own box and 4 px (3 pt) more. Paid after the right word
    // instead, the gap would be the space alone and the right word 3 pt short
    // of its box.
    let page = doc.page(0).expect("a page");
    let text = page.text_with(&TextOptions {
        content_order: true,
    });
    let chars: Vec<(String, f64, f64)> = text
        .lines()
        .iter()
        .flat_map(|line| line.chars.iter())
        .map(|c| {
            let (x0, _, x1, _) = c.quad.bounds();
            (c.text.clone(), x0, x1)
        })
        .collect();
    let space = chars
        .iter()
        .find(|(t, _, _)| t == " ")
        .unwrap_or_else(|| panic!("no space drawn: {chars:?}"));
    let left_end = chars
        .iter()
        .filter(|(t, x0, _)| t != " " && *x0 < space.1)
        .map(|(_, _, x1)| *x1)
        .fold(f64::NEG_INFINITY, f64::max);
    let right_start = chars
        .iter()
        .filter(|(t, x0, _)| t != " " && *x0 > space.1)
        .map(|(_, x0, _)| *x0)
        .fold(f64::INFINITY, f64::min);
    let gap = right_start - left_end;
    let space_width = space.2 - space.1;
    assert!(
        (gap - space_width - 3.0).abs() < 0.01,
        "the gap between the words is {gap}, the space {space_width}: {chars:?}"
    );
}

/// **A justified Arabic paragraph reads in order, line by line.**
///
/// Justification is `word-spacing` on every line but the last, so every such
/// line went through the pieces drawn in written order and read backwards.
/// Three words of the same three letters in different orders, so a word read
/// from its last letter, or two words swapped, is another word in the list.
#[test]
fn a_justified_arabic_paragraph_reads_in_order() {
    let words = [
        "\u{628}\u{62D}\u{645}",
        "\u{645}\u{62D}\u{628}",
        "\u{62D}\u{645}\u{628}",
    ];
    let text: Vec<&str> = (0..30).map(|at| words[at % 3]).collect();
    let body = format!(r#"x</p><p style="text-align: justify">{}"#, text.join(" "));
    let doc = Document::open(arabic_book(&body)).expect("a book");
    let mut read: Vec<String> = Vec::new();
    for page in doc.pages() {
        let plain = page.text().plain_text();
        read.extend(plain.split_whitespace().map(str::to_owned));
    }
    read.retain(|word| word != "x");
    assert_eq!(read, text, "the paragraph does not read in order");
}

// ---- shaping across a span boundary ------------------------------------------

/// **A word whose middle letter is in a span of its own is drawn joined.**
///
/// The span is a `TextRun` of its own, and a run shaped alone sees nothing
/// either side of it: hah was drawn in its isolated form between an isolated
/// beh and an isolated meem, three letters where the word is one. The painter
/// now shapes each run against its neighbours' text in the same face
/// (`Fonts::set_contexts`) and draws only its own glyphs, so each letter takes
/// the form its place in the **word** gives it — computed here from the face,
/// not read back: beh initial, hah medial, meem final.
#[test]
fn a_word_split_by_a_span_is_drawn_joined() {
    let face = arabic_face();
    let body = "\u{628}<span style=\"color: #c00000\">\u{62D}</span>\u{645}";
    let doc = Document::open(arabic_book(body)).expect("a book");
    let content = page_content(&doc);
    let objects = text_objects(&content);
    let drawn: Vec<String> = objects
        .iter()
        .map(|(_, object)| shown_glyphs(object))
        .collect();
    for (ch, form) in [
        ('\u{628}', Form::Initial),
        ('\u{62D}', Form::Medial),
        ('\u{645}', Form::Final),
    ] {
        let glyph = format!(
            "{:04X}",
            face.form_glyph(ch, form)
                .unwrap_or_else(|| panic!("{ch:?} has no {form:?} form"))
        );
        assert!(
            drawn.iter().any(|shown| shown == &glyph),
            "{ch:?} is not drawn in its {form:?} form on its own: {drawn:?}\n{content}"
        );
    }
}

/// **A text shadow of a span inside a word is the word's joined glyphs again.**
///
/// `css-text-decor-3` §4 makes a shadow the run drawn again, offset and in the
/// shadow's colour, so a letter its word joins is joined in its shadow too.
/// The painter draws a shadow as a copy of the run moved by the offset, and
/// finds the text a run is shaped against (`Fonts::set_contexts`) by where the
/// run is drawn — so the copy, drawn somewhere else, found none and was shaped
/// alone: under a joined beh, hah and meem the shadow was three isolated
/// letters. Each letter is asserted twice in its joined form, once for the
/// text and once for its shadow, and no isolated form is drawn at all.
#[test]
fn a_text_shadow_is_shaped_in_the_word_its_run_is() {
    let face = arabic_face();
    let body = "<span style=\"text-shadow: 2px 2px #0000c0\">\u{628}\
                <span style=\"color: #c00000\">\u{62D}</span>\u{645}</span>";
    let doc = Document::open(arabic_book(body)).expect("a book");
    let content = page_content(&doc);
    let drawn: Vec<String> = text_objects(&content)
        .iter()
        .map(|(_, object)| shown_glyphs(object))
        .collect();
    for (ch, form) in [
        ('\u{628}', Form::Initial),
        ('\u{62D}', Form::Medial),
        ('\u{645}', Form::Final),
    ] {
        let joined = format!(
            "{:04X}",
            face.form_glyph(ch, form)
                .unwrap_or_else(|| panic!("{ch:?} has no {form:?} form"))
        );
        assert_eq!(
            drawn.iter().filter(|shown| **shown == joined).count(),
            2,
            "{ch:?} is not drawn in its {form:?} form both as text and as its \
             shadow: {drawn:?}\n{content}"
        );
        let isolated = format!("{:04X}", face.glyph_of(ch).expect("covered"));
        assert!(
            !drawn.contains(&isolated),
            "{ch:?} is drawn isolated somewhere on the page: {drawn:?}\n{content}"
        );
    }
}

/// The pair the context fixture positions: `A` then `B`.
const PAIR_COVERS: &str = "ABC";

/// How far the pair moves `B`, in font units at 1000 to the em.
const PAIR_X: i16 = 250;

/// **A position a neighbour gives reaches a glyph in a span of its own.**
///
/// The face's `GPOS` `PairPos` displaces `B` by 250 units when it follows `A`
/// — a position that exists only when the two are in one buffer. With `B` in
/// a coloured span, `B` is a run of its own; shaped alone it was drawn where
/// its advance put it. Shaped against `A`, it is displaced, and the number is
/// 9.4.3's own: 250 units of a 1000-unit em is a `TJ` adjustment of −250
/// whatever the size, worked out here as in
/// [`a_positioned_glyph_is_drawn_where_its_anchor_puts_it`].
#[test]
fn a_pair_across_a_span_boundary_is_positioned() {
    let face = Face::new("Fixture Pairs", PAIR_COVERS).with_pair(Pair {
        first: 'A',
        second: 'B',
        script: *b"DFLT",
        feature: *b"kern",
        x: PAIR_X,
        y: 0,
    });
    let doc = Document::open(one_face_book(
        "Fixture Pairs",
        &face.build(),
        24,
        "A<span style=\"color: #c00000\">B</span>C",
    ))
    .expect("a book");
    let content = page_content(&doc);
    let objects = text_objects(&content);
    let b = format!("{:04X}", face.glyph_of('B').expect("covered"));
    let object = objects
        .iter()
        .map(|(_, object)| object)
        .find(|object| shown_glyphs(object) == b)
        .unwrap_or_else(|| panic!("no text object draws B alone: {content}"));
    assert!(
        object.contains(&format!("-{PAIR_X} <{b}>")),
        "the pair's offset did not reach B in its own span: {object}\n{content}"
    );
}

// ---- a run that mixes directions -----------------------------------------------

/// Every glyph on the first page but the spaces, as (text, left edge), sorted
/// left to right by where it is drawn.
///
/// Read back through this repository's own extraction in **content order**,
/// so the edges are the text positions the content stream states and not a
/// reading order put back afterwards.
fn drawn_left_to_right(doc: &Document) -> Vec<(String, f64)> {
    let page = doc.page(0).expect("a page");
    let text = page.text_with(&TextOptions {
        content_order: true,
    });
    let mut glyphs: Vec<(String, f64)> = text
        .lines()
        .iter()
        .flat_map(|line| line.chars.iter())
        .filter(|c| !c.text.trim().is_empty())
        .map(|c| (c.text.clone(), c.quad.bounds().0))
        .collect();
    glyphs.sort_by(|a, b| a.1.total_cmp(&b.1));
    glyphs
}

/// **A span boundary inside a word of the other direction leaves the word in
/// its order.**
///
/// `a ب<span>ح</span>م b` is three runs, and two of them mix directions: `a ب`
/// and `م b`. Each run used to be one unit of the line's reordering, placed at
/// the level of its lowest strong character — level 0 for both, since each
/// holds a Latin letter — so only the middle run was reversed, which reverses
/// nothing, and the Arabic word was drawn `ب ح م` from the left: in the order
/// it was typed, reading backwards. The runs are cut at the line's level
/// boundaries now (`paint::split_at_levels`), and every piece is placed by L2.
///
/// The expected positions are worked out here, not read back:
///
/// - **the levels** are UAX #9's. P2 finds `a` first, so the paragraph is
///   level 0; `ب`, `ح` and `م` are `AL`, raised to 1 by I1; each space sits
///   between an `L` and an `AL` and takes the embedding level, 0, by N2. L2
///   then reverses the one stretch at level 1, so the line is drawn
///   `a`, space, `م`, `ح`, `ب`, space, `b`.
/// - **the widths** are the face's `hmtx`: every glyph, the three joined forms
///   included, is 500 units of a 1000-unit em, and the paragraph is set at
///   24 px, which is 18 pt — so each glyph is 9 pt and the *k*-th one drawn
///   starts 9*k* pt right of the first.
/// - **the forms** are the joining rules': `ب` starts the word (initial), `ح`
///   joins on both sides (medial), `م` ends it (final), each across a span
///   boundary.
#[test]
fn a_span_inside_a_word_of_the_other_direction_keeps_the_word_in_order() {
    let face = Face::new("Fixture Arabic", " ab\u{628}\u{62D}\u{645}")
        .with_joining(Joining { script: *b"arab" });
    let body = "a \u{628}<span style=\"color: #c00000\">\u{62D}</span>\u{645} b";
    let doc =
        Document::open(one_face_book("Fixture Arabic", &face.build(), 24, body)).expect("a book");
    let drawn = drawn_left_to_right(&doc);
    let order: Vec<&str> = drawn.iter().map(|(text, _)| text.as_str()).collect();
    assert_eq!(
        order,
        ["a", "\u{645}", "\u{62D}", "\u{628}", "b"],
        "the Arabic word is not drawn last letter first: {drawn:?}"
    );
    // Where each one sits, in 9 pt advances from the first: the space after
    // `a` is advance 1, and the one before `b` advance 5.
    let advance = 500.0 / 1000.0 * 18.0;
    let origin = drawn[0].1;
    for ((text, x), slot) in drawn.iter().zip([0.0, 2.0, 3.0, 4.0, 6.0]) {
        let expected = origin + slot * advance;
        assert!(
            (x - expected).abs() < 0.01,
            "{text:?} is drawn at {x}, not {expected}: {drawn:?}"
        );
    }

    let content = page_content(&doc);
    let shown: String = text_objects(&content)
        .iter()
        .map(|(_, object)| shown_glyphs(object))
        .collect();
    for (ch, form) in [
        ('\u{628}', Form::Initial),
        ('\u{62D}', Form::Medial),
        ('\u{645}', Form::Final),
    ] {
        let glyph = face
            .form_glyph(ch, form)
            .unwrap_or_else(|| panic!("{ch:?} has no {form:?} form"));
        assert!(
            shown.contains(&format!("{glyph:04X}")),
            "{ch:?} is not drawn in its {form:?} form: {content}"
        );
    }
    // And it reads back as it was written (ruling 14).
    let extracted = doc.page(0).expect("a page").text().plain_text();
    assert_eq!(
        extracted.trim_end(),
        "a \u{628}\u{62D}\u{645} b",
        "the line does not read back in logical order"
    );
}

/// **A right-to-left run with a number in it is cut where the number is, and
/// the number still reads left to right.**
///
/// The control for the cut: a run whose own levels are 1 and 2 — Arabic and
/// European digits — is two pieces, and L2 puts the number on the word's left
/// with its digits in their written order. A build that cut the run and then
/// drew every piece of an odd paragraph right to left would draw `21`.
#[test]
fn a_number_in_a_right_to_left_run_keeps_its_digits_in_order() {
    let face = Face::new("Fixture Arabic", " 12\u{628}\u{62D}\u{645}")
        .with_joining(Joining { script: *b"arab" });
    let body = "\u{628}\u{62D}\u{645} 12";
    let doc =
        Document::open(one_face_book("Fixture Arabic", &face.build(), 24, body)).expect("a book");
    let drawn = drawn_left_to_right(&doc);
    let order: Vec<&str> = drawn.iter().map(|(text, _)| text.as_str()).collect();
    // P2 finds `ب`: a level-1 paragraph, the digits at level 2 by I2, and the
    // space between word and number at 1 by N1, an `EN` counting as `R`
    // there. L2 draws the number first, in its own order, then the space, then
    // the word last letter first.
    assert_eq!(
        order,
        ["1", "2", "\u{645}", "\u{62D}", "\u{628}"],
        "the number or the word is out of order: {drawn:?}"
    );
    let advance = 500.0 / 1000.0 * 18.0;
    let origin = drawn[0].1;
    for ((text, x), slot) in drawn.iter().zip([0.0, 1.0, 3.0, 4.0, 5.0]) {
        let expected = origin + slot * advance;
        assert!(
            (x - expected).abs() < 0.01,
            "{text:?} is drawn at {x}, not {expected}: {drawn:?}"
        );
    }
}

/// **A joiner inside a word does not cut the word.**
///
/// `ZWJ` is `Bidi_Class` `BN`, which UAX #9's X9 removes: it has no level of
/// its own, and after L1 it carries the paragraph's, 0 here. Cut at that
/// level, `ب‍حم` inside an English line would be three pieces — `ب` at 1, the
/// joiner at 0, `حم` at 1 — and L2 reverses each Arabic piece alone, drawing
/// `ب` on the left of the other two: the word read backwards again, by the
/// cut meant to fix it. The joiner keeps the level of the letter before it,
/// so the word is one piece and is drawn `م ح ب` from the left, exactly as
/// [`a_span_inside_a_word_of_the_other_direction_keeps_the_word_in_order`]
/// draws it, the joiner itself drawing nothing.
#[test]
fn a_joiner_inside_a_word_does_not_cut_it() {
    let face = Face::new("Fixture Arabic", " ab\u{628}\u{62D}\u{645}\u{200D}")
        .with_joining(Joining { script: *b"arab" });
    let body = "a \u{628}\u{200D}\u{62D}\u{645} b";
    let doc =
        Document::open(one_face_book("Fixture Arabic", &face.build(), 24, body)).expect("a book");
    let drawn = drawn_left_to_right(&doc);
    // The joiner is in the text of the glyph it rides on, `ب`'s, and is
    // drawn as nothing of its own.
    let order: Vec<String> = drawn
        .iter()
        .map(|(text, _)| text.replace('\u{200D}', ""))
        .collect();
    assert_eq!(
        order,
        ["a", "\u{645}", "\u{62D}", "\u{628}", "b"],
        "the word was cut at its joiner: {drawn:?}"
    );
    let advance = 500.0 / 1000.0 * 18.0;
    let origin = drawn[0].1;
    for ((text, x), slot) in drawn.iter().zip([0.0, 2.0, 3.0, 4.0, 6.0]) {
        let expected = origin + slot * advance;
        assert!(
            (x - expected).abs() < 0.01,
            "{text:?} is drawn at {x}, not {expected}: {drawn:?}"
        );
    }
}

// ---- a run measured in its context --------------------------------------------

/// The kerning fixture: `A`, `V`, `C` and the space, every glyph 500 units of
/// a 1000-unit em, and `A` 200 units narrower when `V` follows it — a `GPOS`
/// `kern` pair whose `XAdvance` is −200.
fn kerned_face() -> Face {
    Face::new("Fixture Kern", "ACV ").with_kern(Kern {
        first: 'A',
        second: 'V',
        script: *b"DFLT",
        feature: *b"kern",
        x_advance: -200,
    })
}

/// **A kerned pair across a span boundary is measured kerned.**
///
/// `A<span>V</span>C` is three runs. The painter shapes `A` against the `V`
/// beside it and draws it 300 units wide; layout measured `A` alone, at 500,
/// and put the `V` run where the unkerned pair would have left it — a gap of
/// the kern's width between two letters the face says belong together. The
/// `Shaper` seam takes the run's context now (`Shaper::shape_in`), so the
/// `V` run starts where the kerned `A` ends.
///
/// Worked from the face, not read back: at 24 px, which is 18 pt, `A`'s
/// kerned advance is (500 − 200)/1000 × 18 = 5.4 pt and `V`'s is 9 pt, so `V`
/// starts 5.4 pt after `A` and `C` 14.4 pt after it.
#[test]
fn a_kerned_pair_across_a_span_is_measured_kerned() {
    let face = kerned_face();
    let doc = Document::open(one_face_book(
        "Fixture Kern",
        &face.build(),
        24,
        "A<span style=\"color: #c00000\">V</span>C",
    ))
    .expect("a book");
    let drawn = drawn_left_to_right(&doc);
    let order: Vec<&str> = drawn.iter().map(|(text, _)| text.as_str()).collect();
    assert_eq!(order, ["A", "V", "C"], "{drawn:?}");
    let origin = drawn[0].1;
    for ((text, x), offset) in drawn.iter().zip([0.0, 5.4, 14.4]) {
        let expected = origin + offset;
        assert!(
            (x - expected).abs() < 0.01,
            "{text:?} is drawn at {x}, not {expected}: the pair was measured \
             without its kerning: {drawn:?}"
        );
    }
}

/// **A joined form wider than the isolated one is measured joined.**
///
/// The Arabic face with its joined forms 700 units wide against 500 for the
/// isolated glyphs: `ب<span>ح</span>م` is drawn initial, medial and final, each
/// 700/1000 × 18 = 12.6 pt, and each run was measured alone, isolated, at
/// 9 pt — so the three overlapped by 3.6 pt at each boundary. Measured in
/// its context each run is the width of the form it is drawn in.
///
/// The paragraph reads right to left (P2: its first strong character is
/// Arabic) and is set from the left edge, so the line is drawn `م ح ب` from
/// there, each 12.6 pt after the last.
#[test]
fn a_joined_form_across_a_span_is_measured_joined() {
    let face = arabic_face().with_joined_advance(700);
    let doc = Document::open(one_face_book(
        "Fixture Arabic",
        &face.build(),
        24,
        "\u{628}<span style=\"color: #c00000\">\u{62D}</span>\u{645}",
    ))
    .expect("a book");
    let drawn = drawn_left_to_right(&doc);
    let order: Vec<&str> = drawn.iter().map(|(text, _)| text.as_str()).collect();
    assert_eq!(order, ["\u{645}", "\u{62D}", "\u{628}"], "{drawn:?}");
    let origin = drawn[0].1;
    for ((text, x), slot) in drawn.iter().zip([0.0, 1.0, 2.0]) {
        let expected = origin + slot * 12.6;
        assert!(
            (x - expected).abs() < 0.01,
            "{text:?} is drawn at {x}, not {expected}: a joined form was \
             measured isolated: {drawn:?}"
        );
    }
}

/// **The line breaker sees the kerning.**
///
/// Four words `AV`, each `V` in a span of its own, on a measure of 90 pt.
/// Kerned, each word is 5.4 + 9 = 14.4 pt and the line is 4 × 14.4 + 3 × 9 =
/// 84.6 pt, which fits; measured run by run without context it is 4 × 18 +
/// 27 = 99 pt, which does not, and the last word went to a second line. The
/// page is 162 pt wide, the margins 36 pt each side.
#[test]
fn the_line_breaker_measures_a_kerned_pair_across_a_span() {
    let face = kerned_face();
    let word = "A<span style=\"color: #c00000\">V</span>";
    let body = [word; 4].join(" ");
    let doc = Document::open_with(
        one_face_book("Fixture Kern", &face.build(), 24, &body),
        &OpenOptions::at_page(162.0, 400.0),
    )
    .expect("a book");
    let page = doc.page(0).expect("a page");
    let text = page.text_with(&TextOptions {
        content_order: true,
    });
    let baselines: Vec<f64> = text
        .lines()
        .iter()
        .flat_map(|line| line.chars.iter())
        .map(|c| c.origin.1)
        .collect();
    assert_eq!(baselines.len(), 11, "every glyph is drawn: {baselines:?}");
    assert!(
        baselines.iter().all(|y| (y - baselines[0]).abs() < 0.01),
        "the four words were broken onto two lines: {baselines:?}"
    );
}

/// **Both halves at once: a word of the other direction split by a span, its
/// joined forms wider than its isolated ones.**
///
/// `a ب<span>ح</span>م b` again, with the joined forms 700 units wide. Layout
/// measures `a ب` with `ح` after it, so `ب` is initial and 12.6 pt; the cut
/// at the level boundary (`paint::split_at_levels`) has to share the run's
/// width out in that same context, or the `م` of `م b` — final, 12.6 pt —
/// gets its isolated 9 pt and the word overlaps itself. Worked from the face:
/// `a` and the space 9 pt each, the three letters 12.6 pt each, drawn
/// `م ح ب` from 18 pt, the space after them, then `b` at 18 + 3 × 12.6 + 9.
#[test]
fn a_mixed_line_is_cut_and_measured_in_one_context() {
    let face = Face::new("Fixture Arabic", " ab\u{628}\u{62D}\u{645}")
        .with_joining(Joining { script: *b"arab" })
        .with_joined_advance(700);
    let body = "a \u{628}<span style=\"color: #c00000\">\u{62D}</span>\u{645} b";
    let doc =
        Document::open(one_face_book("Fixture Arabic", &face.build(), 24, body)).expect("a book");
    let drawn = drawn_left_to_right(&doc);
    let order: Vec<&str> = drawn.iter().map(|(text, _)| text.as_str()).collect();
    assert_eq!(
        order,
        ["a", "\u{645}", "\u{62D}", "\u{628}", "b"],
        "{drawn:?}"
    );
    let origin = drawn[0].1;
    let expected = [0.0, 18.0, 30.6, 43.2, 64.8];
    for ((text, x), offset) in drawn.iter().zip(expected) {
        assert!(
            (x - (origin + offset)).abs() < 0.01,
            "{text:?} is drawn at {x}, not {}: {drawn:?}",
            origin + offset
        );
    }
}

/// **A paragraph set on one line opens, as one line** (review of lane 8C).
///
/// The input the review found quadratic: 20 000 two-letter words at 0.001
/// px in the kerning fixture's face, 60 000 characters on one line. A
/// slice of it was measured with the whole line before it as its context,
/// and the provider counted that to find its last eight characters —
/// `O(characters²)`, 25.8 s to open in a debug build against 2.6 s for the
/// same words at 16 px.
///
/// **This test does not hold the time, and its name does not say it
/// does.** No clock is read, for `5adf502`'s reason, and `cargo test` has
/// no timeout, so a quadratic build passes it slowly. The bound is held by
/// two tests that fail: layout's `shaper.rs`
/// `a_shaper_is_handed_the_near_end_of_a_neighbour_and_never_all_of_it`,
/// by the length of what a shaper is handed — at most `CONTEXT_BYTES`,
/// whatever the provider does with it — and this crate's
/// `a_run_cut_in_a_piece_per_character_finds_each_glyphs_piece_by_search`,
/// by count, for the painter's cut. What this one holds is that the input
/// is the shape those two are about — the paragraph is set on one line —
/// and that it opens.
#[test]
fn a_paragraph_set_on_one_line_opens_as_one_line() {
    let face = Face::new("Fixture Kern", "ab ");
    let words = "ab ".repeat(20_000);
    let body = format!("<span style=\"font-size: 0.001px\">{words}</span>");
    let doc =
        Document::open(one_face_book("Fixture Kern", &face.build(), 24, &body)).expect("a book");
    assert_eq!(doc.page_count(), 1);
    // The letters are a few ten-thousandths of a point apart, which the
    // extractor reads as one letter drawn over another; what it can say is
    // that they share one baseline.
    let text = doc.page(0).expect("a page").text_with(&TextOptions {
        content_order: true,
    });
    assert_eq!(
        text.lines().len(),
        1,
        "the paragraph was not set on one line, so it is not the input the bound is about"
    );
}

// ---- font-kerning and font-feature-settings ---------------------------------------

/// Where `A`, `V` and `C` land, in points from `A`, in a book whose paragraph
/// says `style` and is set in the kerning fixture.
fn kerned_positions(style: &str) -> Vec<(String, f64)> {
    let face = kerned_face();
    // `C` outside the span, so where it lands is the span's width as layout
    // measured it, not only where the shaper drew the span's own glyphs.
    let body = format!("<span style='{style}'>AV</span>C");
    let doc =
        Document::open(one_face_book("Fixture Kern", &face.build(), 24, &body)).expect("a book");
    let drawn = drawn_left_to_right(&doc);
    let origin = drawn[0].1;
    drawn
        .into_iter()
        .map(|(text, x)| (text, x - origin))
        .collect()
}

/// Whether two position lists agree to a hundredth of a point.
fn at(actual: &[(String, f64)], expected: &[(&str, f64)]) -> bool {
    actual.len() == expected.len()
        && actual
            .iter()
            .zip(expected)
            .all(|((t, x), (u, y))| t == u && (x - y).abs() < 0.01)
}

/// **`font-kerning: none` switches the face's kerning off, and
/// `font-feature-settings` can switch it back on** (`css-fonts-4` §6.4,
/// §6.12, and §7.2's precedence: the low-level property last).
///
/// The kerning fixture's `kern` pair draws `V` 5.4 pt after `A` — (500 − 200)
/// / 1000 × 18 — and without it 9 pt after: 500 / 1000 × 18. Measured and
/// drawn alike, so `C` moves with it.
#[test]
fn font_kerning_none_and_a_kern_setting_turn_the_pair_off_and_on() {
    let kerned = [("A", 0.0), ("V", 5.4), ("C", 14.4)];
    let unkerned = [("A", 0.0), ("V", 9.0), ("C", 18.0)];
    for (style, expected) in [
        ("", &kerned),
        ("font-kerning: auto", &kerned),
        ("font-kerning: normal", &kerned),
        ("font-kerning: none", &unkerned),
        ("font-feature-settings: \"kern\" 0", &unkerned),
        ("font-feature-settings: \"kern\" off", &unkerned),
        (
            "font-kerning: none; font-feature-settings: \"kern\"",
            &kerned,
        ),
    ] {
        let positions = kerned_positions(style);
        assert!(
            at(&positions, expected),
            "`{style}` drew {positions:?}, not {expected:?}"
        );
    }
}

/// A face that ligates `f` and `i` under `feature`, and covers `x` and the
/// space beside them, every glyph 500 units wide — the ligature too.
fn ligating_face(feature: &[u8; 4]) -> Face {
    Face::new("Fixture Liga", "fix ").with_ligature(Ligature {
        first: 'f',
        second: 'i',
        script: *b"DFLT",
        feature: *feature,
    })
}

/// The glyphs a book whose paragraph says `style` draws for `fix`, and where
/// its `x` lands in points from the first glyph.
fn ligated(face: &Face, style: &str) -> (String, f64) {
    let body = format!("<span style='{style}'>fi</span>x");
    let doc =
        Document::open(one_face_book("Fixture Liga", &face.build(), 24, &body)).expect("a book");
    let content = page_content(&doc);
    let shown: String = text_objects(&content)
        .iter()
        .map(|(_, object)| shown_glyphs(object))
        .collect();
    let drawn = drawn_left_to_right(&doc);
    let origin = drawn[0].1;
    let x = drawn
        .iter()
        .find(|(text, _)| text == "x")
        .map_or(f64::NAN, |(_, x)| x - origin);
    (shown, x)
}

/// **`font-feature-settings` switches a default feature off and a
/// discretionary one on, through the shaper** (`css-fonts-4` §6.12).
///
/// Worked from the face: under `liga`, which the shaper applies by default,
/// `fi` is the ligature glyph — one 500-unit glyph, so `x` starts 9 pt after
/// the first glyph — and with `"liga" 0` it is `f` and `i`, and `x` is 18 pt
/// along. Under `dlig`, which is off by default, the two are the other way
/// round. The glyph indices are the face builder's own: the covered
/// characters from 1 in sorted order, the ligature after them.
#[test]
fn font_feature_settings_switch_a_ligature_off_and_a_discretionary_one_on() {
    let liga = ligating_face(b"liga");
    let dlig = ligating_face(b"dlig");
    let glyph = |face: &Face, ch: char| format!("{:04X}", face.glyph_of(ch).expect("covered"));
    let ligature = |face: &Face| format!("{:04X}", face.ligature_glyph().expect("a ligature"));
    let joined = |face: &Face| format!("{}{}", ligature(face), glyph(face, 'x'));
    let apart = |face: &Face| {
        format!(
            "{}{}{}",
            glyph(face, 'f'),
            glyph(face, 'i'),
            glyph(face, 'x')
        )
    };
    for (face, style, glyphs, x) in [
        (&liga, "", joined(&liga), 9.0),
        (
            &liga,
            "font-feature-settings: \"liga\" 0",
            apart(&liga),
            18.0,
        ),
        (&dlig, "", apart(&dlig), 18.0),
        (&dlig, "font-feature-settings: \"dlig\"", joined(&dlig), 9.0),
        (
            &dlig,
            "font-feature-settings: \"dlig\" 1, \"dlig\" 0",
            apart(&dlig),
            18.0,
        ),
    ] {
        let (shown, at) = ligated(face, style);
        assert_eq!(shown, glyphs, "`{style}` drew the wrong glyphs");
        assert!(
            (at - x).abs() < 0.01,
            "`{style}`: `x` is {at} pt along, not {x}"
        );
    }
}

// ---- direction and unicode-bidi -------------------------------------------------

/// The bidi fixture: Latin `a`, `b` and `!`, the three Arabic letters, and the
/// space, every glyph 500 units of a 1000-unit em — 9 pt at the 24 px every
/// book here is set at — and joining under `arab`.
fn bidi_face() -> Face {
    Face::new("Fixture Bidi", " !ab\u{628}\u{62D}\u{645}")
        .with_joining(Joining { script: *b"arab" })
}

/// Each line's glyphs, top line first, each line's from the left, with the
/// left edge of each in points from the page's left edge.
fn drawn_lines(attributes: &str, body: &str) -> Vec<Vec<(String, f64)>> {
    let program = bidi_face().build();
    let book = faces_book_with(&[("Fixture Bidi", &program)], 24, attributes, body);
    let doc = Document::open(book).expect("a book");
    page_lines(&doc, 0)
}

/// [`drawn_lines`] of page `index` of a document already open.
fn page_lines(doc: &Document, index: u32) -> Vec<Vec<(String, f64)>> {
    let page = doc.page(index).expect("a page");
    let text = page.text_with(&TextOptions {
        content_order: true,
    });
    let mut lines: Vec<(f64, Vec<(String, f64)>)> = Vec::new();
    for c in text
        .lines()
        .iter()
        .flat_map(|line| line.chars.iter())
        .filter(|c| !c.text.trim().is_empty())
    {
        let (left, bottom, _, _) = c.quad.bounds();
        match lines.iter_mut().find(|(y, _)| (y - bottom).abs() < 1.0) {
            Some((_, glyphs)) => glyphs.push((c.text.clone(), left)),
            None => lines.push((bottom, vec![(c.text.clone(), left)])),
        }
    }
    lines.sort_by(|a, b| b.0.total_cmp(&a.0));
    lines
        .into_iter()
        .map(|(_, mut glyphs)| {
            glyphs.sort_by(|a, b| a.1.total_cmp(&b.1));
            glyphs
        })
        .collect()
}

/// The page's one line, as [`drawn_lines`] reads it.
fn drawn_line(attributes: &str, body: &str) -> Vec<(String, f64)> {
    let lines = drawn_lines(attributes, body);
    assert_eq!(lines.len(), 1, "not one line: {lines:?}");
    lines.into_iter().next().unwrap_or_default()
}

/// The content box's left and right edges on the default page: 432 pt wide,
/// with a 36 pt page margin either side and the fixture's `body { margin: 0
/// }`.
const LEFT: f64 = 36.0;
const RIGHT: f64 = 396.0;

/// One glyph's advance, in points.
const GLYPH: f64 = 9.0;

/// **A paragraph's direction is its block's, and it decides both where the
/// line starts and how the line is ordered** (`css-writing-modes-3` §2.1,
/// `css-text-3` §7.1).
///
/// `ab !` is laid out at the content box's left edge when nothing is said:
/// `a`, `b`, a space, `!`, nine points apart. Under `dir="rtl"` the same four
/// characters are worked out from UAX #9 with the paragraph at level 1, not
/// read back: `a` and `b` are `L`, raised to 2 by I2; the space and `!` lie
/// between `b` and the paragraph's end, whose `eos` is `R`, so N2 gives them
/// the embedding level, 1. L2 reverses the level-2 pair and then the whole
/// line: `!`, space, `a`, `b`. And `start`, the initial `text-align`, is the
/// right edge now, so the line's 36 pt end at the content box's right edge.
///
/// Before `direction` was read the second page drew the first page's line.
#[test]
fn a_right_to_left_paragraph_starts_at_the_right_and_is_ordered_at_level_one() {
    let ltr = drawn_line("", "ab !");
    assert!(
        at(
            &ltr,
            &[("a", LEFT), ("b", LEFT + GLYPH), ("!", LEFT + 3.0 * GLYPH)]
        ),
        "the left-to-right line moved: {ltr:?}"
    );
    let rtl = drawn_line(" dir=\"rtl\"", "ab !");
    let start = RIGHT - 4.0 * GLYPH;
    assert!(
        at(
            &rtl,
            &[
                ("!", start),
                ("a", start + 2.0 * GLYPH),
                ("b", start + 3.0 * GLYPH)
            ]
        ),
        "the right-to-left line is not `! ab` against the right edge: {rtl:?}"
    );
}

/// **A left-to-right paragraph that begins with an Arabic word stays left to
/// right**, and the same text in a right-to-left one does not.
///
/// `بحم ab`: the paragraph used to be each line's own P2 and P3, which find
/// `ب` first and make the line right to left whatever the block said. At
/// level 0 the three letters are an `AL` run raised to 1, the space between
/// `م` and `a` takes the embedding level, 0, by N2, and L2 reverses only the
/// word: `م`, `ح`, `ب`, space, `a`, `b` from the left edge. At level 1 `ab`
/// is raised to 2 and the space between `م` and `a` is 1, so L2 draws `a`,
/// `b`, space, `م`, `ح`, `ب`, ending at the right edge.
#[test]
fn the_block_and_not_the_first_letter_decides_a_paragraphs_direction() {
    let word = "\u{628}\u{62D}\u{645} ab";
    let ltr = drawn_line("", word);
    assert!(
        at(
            &ltr,
            &[
                ("\u{645}", LEFT),
                ("\u{62D}", LEFT + GLYPH),
                ("\u{628}", LEFT + 2.0 * GLYPH),
                ("a", LEFT + 4.0 * GLYPH),
                ("b", LEFT + 5.0 * GLYPH),
            ]
        ),
        "a left-to-right paragraph was laid out by its first letter: {ltr:?}"
    );
    let start = RIGHT - 6.0 * GLYPH;
    let rtl = drawn_line(" dir=\"rtl\"", word);
    assert!(
        at(
            &rtl,
            &[
                ("a", start),
                ("b", start + GLYPH),
                ("\u{645}", start + 3.0 * GLYPH),
                ("\u{62D}", start + 4.0 * GLYPH),
                ("\u{628}", start + 5.0 * GLYPH),
            ]
        ),
        "the right-to-left paragraph is not `ab محب` against the right edge: {rtl:?}"
    );
}

/// **`unicode-bidi: plaintext` on a block: each paragraph takes its
/// direction from its own first strong character, and its start side with
/// it** (`css-writing-modes-3` §2.2).
///
/// The block says so in its own `style`: HTML gives `plaintext` to `<pre
/// dir="auto">` and `<textarea dir="auto">` and to nothing else, so a `<p
/// dir="auto">` — what this test once wrote — is one direction for the whole
/// element ([`dir_auto_is_one_direction_from_the_content_and_is_inherited`]).
///
/// Under `white-space: pre` the newline is a forced break, and a forced
/// break ends a bidi paragraph (§2.4.1), so the two lines here are two
/// paragraphs: the first finds `ب` and is the right-to-left line of
/// [`the_block_and_not_the_first_letter_decides_a_paragraphs_direction`],
/// against the right edge; the second finds `a`, so it is left to right and
/// starts at the left edge, its Arabic word reversed alone.
#[test]
fn plaintext_gives_each_paragraph_its_own_first_strong_direction() {
    let lines = drawn_lines(
        " style=\"unicode-bidi: plaintext; white-space: pre\"",
        "\u{628}\u{62D}\u{645} ab\nab \u{628}\u{62D}\u{645}",
    );
    assert_eq!(lines.len(), 2, "not two lines: {lines:?}");
    let start = RIGHT - 6.0 * GLYPH;
    assert!(
        at(
            &lines[0],
            &[
                ("a", start),
                ("b", start + GLYPH),
                ("\u{645}", start + 3.0 * GLYPH),
                ("\u{62D}", start + 4.0 * GLYPH),
                ("\u{628}", start + 5.0 * GLYPH),
            ]
        ),
        "the first paragraph is not right to left: {lines:?}"
    );
    assert!(
        at(
            &lines[1],
            &[
                ("a", LEFT),
                ("b", LEFT + GLYPH),
                ("\u{645}", LEFT + 3.0 * GLYPH),
                ("\u{62D}", LEFT + 4.0 * GLYPH),
                ("\u{628}", LEFT + 5.0 * GLYPH),
            ]
        ),
        "the second paragraph is not left to right: {lines:?}"
    );
}

/// **An inline box's `unicode-bidi` opens a level round its content, and
/// `direction` alone does not** (`css-writing-modes-3` §2.2, §2.4.2).
///
/// `a b! a` with `b!` in a box. A box that only says `direction: rtl` opens
/// nothing — its text is the paragraph's, all at level 0 — so the line is
/// drawn as written. A box with `dir="rtl"` is an isolate (`RLI` … `PDI`),
/// and one with `unicode-bidi: embed; direction: rtl` an embedding (`RLE` …
/// `PDF`); in both `b!` is at level 1, `b` raised to 2 by I2, and `!` — between
/// `b` and the level run's end, whose `eos` is `R` — at 1 by N2. L2 then
/// draws `!` before `b`, in the slot `b` had: `a`, space, `!`, `b`, space,
/// `a`.
#[test]
fn an_isolate_or_an_embedding_reorders_its_content_and_direction_alone_does_not() {
    let written = [
        ("a", LEFT),
        ("b", LEFT + 2.0 * GLYPH),
        ("!", LEFT + 3.0 * GLYPH),
        ("a", LEFT + 5.0 * GLYPH),
    ];
    let turned = [
        ("a", LEFT),
        ("!", LEFT + 2.0 * GLYPH),
        ("b", LEFT + 3.0 * GLYPH),
        ("a", LEFT + 5.0 * GLYPH),
    ];
    for (open, close, expected) in [
        ("<span style=\"direction: rtl\">", "</span>", &written),
        ("<span dir=\"rtl\">", "</span>", &turned),
        (
            "<span style=\"unicode-bidi: embed; direction: rtl\">",
            "</span>",
            &turned,
        ),
        ("<bdi dir=\"rtl\">", "</bdi>", &turned),
    ] {
        let line = drawn_line("", &format!("a {open}b!{close} a"));
        assert!(at(&line, expected), "`{open}` drew {line:?}");
    }
}

/// **`text-align: start` and `end` are the paragraph's sides, not the
/// page's** (`css-text-3` §7.1).
///
/// `end` in a left-to-right paragraph is the right edge and in a
/// right-to-left one the left; `left` and `right` stay where they are
/// whatever the direction. The right-to-left line is `! ab`, as in
/// [`a_right_to_left_paragraph_starts_at_the_right_and_is_ordered_at_level_one`].
#[test]
fn start_and_end_are_the_paragraphs_sides() {
    let flush_right = RIGHT - 4.0 * GLYPH;
    for (attributes, expected) in [
        (
            " style=\"text-align: end\"",
            [
                ("a", flush_right),
                ("b", flush_right + GLYPH),
                ("!", flush_right + 3.0 * GLYPH),
            ],
        ),
        (
            " style=\"text-align: start\"",
            [("a", LEFT), ("b", LEFT + GLYPH), ("!", LEFT + 3.0 * GLYPH)],
        ),
        (
            " dir=\"rtl\" style=\"text-align: end\"",
            [
                ("!", LEFT),
                ("a", LEFT + 2.0 * GLYPH),
                ("b", LEFT + 3.0 * GLYPH),
            ],
        ),
        (
            " dir=\"rtl\" style=\"text-align: left\"",
            [
                ("!", LEFT),
                ("a", LEFT + 2.0 * GLYPH),
                ("b", LEFT + 3.0 * GLYPH),
            ],
        ),
        (
            " dir=\"rtl\" style=\"text-align: right\"",
            [
                ("!", flush_right),
                ("a", flush_right + 2.0 * GLYPH),
                ("b", flush_right + 3.0 * GLYPH),
            ],
        ),
    ] {
        let line = drawn_line(attributes, "ab !");
        assert!(at(&line, &expected), "`{attributes}` drew {line:?}");
    }
}

/// **`text-indent` is taken from the paragraph's start side** (`css-text-3`
/// §8.1: a margin on the line box's start edge), and under `plaintext` that
/// is each paragraph's own (review of lane 8C).
///
/// 36 px is 27 pt. Left to right `ab` starts 27 pt in from the left edge.
/// Right to left the same two letters are at level 2, drawn `ab`, and the
/// line ends 27 pt in from the right edge, where it used to end at the edge
/// with the indent spent on the left, which a flush-right line never
/// reaches. Under `unicode-bidi: plaintext` an Arabic paragraph in a
/// left-to-right block is right to left, so its indent is at the right too.
#[test]
fn text_indent_is_on_the_paragraphs_start_side() {
    const INDENT: f64 = 27.0;
    let ltr = drawn_line(" style=\"text-indent: 36px\"", "ab");
    assert!(
        at(&ltr, &[("a", LEFT + INDENT), ("b", LEFT + INDENT + GLYPH)]),
        "the left-to-right indent moved: {ltr:?}"
    );
    let end = RIGHT - INDENT;
    let rtl = drawn_line(" dir=\"rtl\" style=\"text-indent: 36px\"", "ab");
    assert!(
        at(&rtl, &[("a", end - 2.0 * GLYPH), ("b", end - GLYPH)]),
        "the right-to-left line does not end at the indent: {rtl:?}"
    );
    let plain = drawn_line(
        " style=\"unicode-bidi: plaintext; text-indent: 36px\"",
        "\u{628}\u{62D}\u{645}",
    );
    assert!(
        at(
            &plain,
            &[
                ("\u{645}", end - 3.0 * GLYPH),
                ("\u{62D}", end - 2.0 * GLYPH),
                ("\u{628}", end - GLYPH),
            ]
        ),
        "the right-to-left plaintext paragraph does not end at the indent: {plain:?}"
    );
}

/// **`dir="auto"` is one direction for the whole element, from its content,
/// and it is inherited** (HTML §3.2.6.4 and §15.3.5; review of lane 8C).
///
/// HTML gives an element whose `dir` is `auto` the direction of the first
/// strong character of its text and `unicode-bidi: isolate`, and everything
/// inside it inherits that direction. It was mapped to `plaintext` instead,
/// which is HTML's rule for `<pre dir="auto">` and `<textarea dir="auto">`
/// alone: the paragraph re-decided after every forced break, and a block
/// inside it inherited `ltr` from the parent.
///
/// - `ab` then `بحم` on a second preformatted line: the `p`'s first strong
///   character is `a`, so it is left to right throughout and the second line
///   starts at the left edge, its word reversed alone;
/// - `بحم ab` in a block `span`: the `p` is right to left and so is the span,
///   which inherits it — the same line as under `dir="rtl"`, ending at the
///   right edge;
/// - and `<pre dir="auto">` with the same two lines as the first case is
///   `plaintext`, so its second paragraph is right to left on its own and
///   ends at the right edge.
#[test]
fn dir_auto_is_one_direction_from_the_content_and_is_inherited() {
    let lines = drawn_lines(
        " dir=\"auto\" style=\"white-space: pre\"",
        "ab\n\u{628}\u{62D}\u{645}",
    );
    assert_eq!(lines.len(), 2, "not two lines: {lines:?}");
    assert!(
        at(
            &lines[1],
            &[
                ("\u{645}", LEFT),
                ("\u{62D}", LEFT + GLYPH),
                ("\u{628}", LEFT + 2.0 * GLYPH),
            ]
        ),
        "the second line re-decided the paragraph's direction: {lines:?}"
    );

    let block = "<span style=\"display: block\">\u{628}\u{62D}\u{645} ab</span>";
    let rtl = drawn_line(" dir=\"rtl\"", block);
    let start = RIGHT - 6.0 * GLYPH;
    let expected = [
        ("a", start),
        ("b", start + GLYPH),
        ("\u{645}", start + 3.0 * GLYPH),
        ("\u{62D}", start + 4.0 * GLYPH),
        ("\u{628}", start + 5.0 * GLYPH),
    ];
    assert!(
        at(&rtl, &expected),
        "the `dir=\"rtl\"` control moved: {rtl:?}"
    );
    let auto = drawn_line(" dir=\"auto\"", block);
    assert!(
        at(&auto, &expected),
        "the block inside `dir=\"auto\"` did not inherit its direction: {auto:?}"
    );

    let pre = drawn_lines(
        "",
        "<pre dir=\"auto\" style=\"margin: 0\">ab\n\u{628}\u{62D}\u{645}</pre>",
    );
    assert_eq!(pre.len(), 2, "not two lines: {pre:?}");
    assert!(
        at(
            &pre[1],
            &[
                ("\u{645}", RIGHT - 3.0 * GLYPH),
                ("\u{62D}", RIGHT - 2.0 * GLYPH),
                ("\u{628}", RIGHT - GLYPH),
            ]
        ),
        "the `pre`'s second paragraph is not its own, right to left: {pre:?}"
    );
}

/// **A line's levels are its paragraph's, so where a line wraps does not
/// change the order inside it** (UAX #9: X1 to I2 over the paragraph, L1 and
/// L2 per line; review of lane 8C).
///
/// `abc (de` in a right-to-left paragraph, in a face covering ` ()abcde`.
/// Every letter is `L`, raised to 2; the space and `(` lie between `c` and
/// `d`, both `L`, so N1 makes them `L` too, and the whole text is one
/// level-2 run drawn as written. Unwrapped it ends at the right edge with
/// `(` just before `d`. At 48 px wide — four glyphs — it breaks before `(`,
/// and the second line is `(de`: drawn `(`, `d`, `e` from 27 pt before the
/// box's right edge at 72. Each line used to be resolved alone, `(`
/// between the line's start (`sos`, R) and `d`, which N2 puts at level 1,
/// and the line was drawn `de(`.
///
/// And the same across a page: a page one line high puts `(de` at the top
/// of the second page, ordered by the `c` that ended the first.
#[test]
fn a_wrapped_line_is_ordered_by_its_paragraphs_levels() {
    let program = Face::new("Fixture Bidi", " ()abcde").build();
    let open = |attributes: &str, options: &OpenOptions| {
        let book = faces_book_with(&[("Fixture Bidi", &program)], 24, attributes, "abc (de");
        Document::open_with(book, options).expect("a book")
    };
    let whole = page_lines(&open(" dir=\"rtl\"", &OpenOptions::default()), 0);
    assert_eq!(whole.len(), 1, "{whole:?}");
    assert!(
        at(
            &whole[0],
            &[
                ("a", RIGHT - 7.0 * GLYPH),
                ("b", RIGHT - 6.0 * GLYPH),
                ("c", RIGHT - 5.0 * GLYPH),
                ("(", RIGHT - 3.0 * GLYPH),
                ("d", RIGHT - 2.0 * GLYPH),
                ("e", RIGHT - GLYPH)
            ]
        ),
        "the unwrapped line is not `abc (de` against the right edge: {whole:?}"
    );

    let narrow = " dir=\"rtl\" style=\"width: 48px\"";
    let edge = LEFT + 36.0;
    let expected = [
        ("(", edge - 3.0 * GLYPH),
        ("d", edge - 2.0 * GLYPH),
        ("e", edge - GLYPH),
    ];
    let wrapped = page_lines(&open(narrow, &OpenOptions::default()), 0);
    assert_eq!(wrapped.len(), 2, "{wrapped:?}");
    assert!(
        at(&wrapped[1], &expected),
        "the wrapped second line is not `(de`: {wrapped:?}"
    );

    let paged = open(narrow, &OpenOptions::at_page(432.0, 100.0));
    assert!(paged.page_count() >= 2, "the paragraph fitted one page");
    let second = page_lines(&paged, 1);
    assert!(
        second.first().is_some_and(|line| at(line, &expected)),
        "the line at the top of the next page is not `(de`: {second:?}"
    );
}
