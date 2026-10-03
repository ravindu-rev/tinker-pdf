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
//! # What remains
//!
//! Layout still **measures** each run alone — its `Shaper` seam takes no
//! context — so a context that changes an *advance* (a joined form wider than
//! the isolated one, a pair that kerns) leaves that difference between the
//! run and the next; an offset moves no pen and costs nothing. And a run that
//! mixes directions inside a right-to-left line is ordered inside itself by
//! its own P2 and P3 rather than by the line's levels.

mod epub_support;

use epub_support::book::{faces_book, one_face_book};
use epub_support::typeface::{
    origin_of, shown_glyphs, text_objects, Face, Form, Joining, Pair, Placement,
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
