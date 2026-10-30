//! Ruling 14 (`docs/rulings.md`): extracted text is in logical order.
//!
//! Every page here is built with `DocumentBuilder::glyph_run`, which takes a
//! position for every glyph and the text each glyph stands for, so a test can
//! write the same Hebrew line the two ways producers write one — **drawn in
//! visual order** from left to right, and **drawn in reading order** with the
//! pen moving left — and assert both extract the same, in the order the line
//! is read. The face is synthesised (`epub_support::typeface`) and covers
//! exactly what each page draws.
//!
//! What the ruling promises beyond `plain_text` is asserted where it is
//! promised: search finds the word as it is read and boxes it the right way
//! round, the structured formats carry the same order, the opt-out returns the
//! stream's, and a page with no right-to-left character extracts exactly as it
//! did before the ruling.

mod epub_support;

use epub_support::typeface::Face;
use tinker_pdf::{Document, TextFormat, TextOptions, TextWriter};
use tinker_pdf_cos::build::{DocumentBuilder, FormXObject, Glyph, PlacedGlyph};
use tinker_pdf_cos::{Dict, DocumentEditor, Name, ObjRef, Object, StreamData, WriteOptions};

/// Shin, lamed, vav, final mem: `shalom`, as it is read.
const SHALOM: &str = "\u{5E9}\u{5DC}\u{5D5}\u{5DD}";

/// Hebrew point qamats, a nonspacing mark.
const QAMATS: char = '\u{5B8}';

/// Arabic noon, seen, beh, teh marbuta: `nisba`, a proportion, as it is read.
const NISBA: &str = "\u{646}\u{633}\u{628}\u{629}";

/// N'Ko a, ee, i: three letters whose `Bidi_Class` is `R` and which sit in
/// none of the Hebrew or Arabic blocks.
const NKO: &str = "\u{7CA}\u{7CB} \u{7CC}.";

/// Het, vav: the Hebrew of Moby-Dick's etymology, as it is read.
const HET_VAV: &str = "\u{5D7}\u{5D5}";

/// Arabic hah, waw, teh: `hut`, a whale, as it is read.
const HUT: &str = "\u{62D}\u{648}\u{62A}";

/// Pe, resh, yod, tet: `prit`, an item, as it is read.
const PRIT: &str = "\u{5E4}\u{5E8}\u{5D9}\u{5D8}";

/// Tav, pe, vav, het, yod, final mem: `tapuchim`, apples, as it is read.
const TAPUCHIM: &str = "\u{5EA}\u{5E4}\u{5D5}\u{5D7}\u{5D9}\u{5DD}";

/// Arabic beh, noon, dal: `band`, a clause, as it is read.
const BAND: &str = "\u{628}\u{646}\u{62F}";

/// Arabic meem, reh, hah, beh, alef: `marhaba`, hello, as it is read.
const MARHABA: &str = "\u{645}\u{631}\u{62D}\u{628}\u{627}";

/// Everything any page here draws.
const COVERS: &str = "\u{5E9}\u{5DC}\u{5D5}\u{5DD}\u{5B8}\u{646}\u{633}\u{628}\u{629}\u{7CA}\u{7CB}\u{7CC} 0123456789seonwab.%\u{5D7}\u{62D}\u{648}\u{62A},\u{2014}\u{5E4}\u{5E8}\u{5D9}\u{5D8}\u{5EA}()\u{2022}\u{62F}\u{661}\u{645}\u{631}\u{627}\u{201C}\u{AB}";

/// The font size, and so the advance of every glyph: the face's 500 units of
/// a 1000-unit em at twenty points is ten.
const SIZE: f64 = 20.0;
const ADVANCE: f64 = 10.0;

/// One glyph to draw: the character and where its origin goes along the
/// baseline, from the run's origin.
type Stroke = (char, f64);

/// A one-page document drawing `strokes`, in the order given, as one run.
fn page(strokes: &[Stroke]) -> Vec<u8> {
    page_of(&[strokes.to_vec()])
}

/// The distance between the baselines of [`page_of`]'s lines.
const LEADING: f64 = 30.0;

/// A one-page document drawing each of `lines` as a run of its own, the
/// first at the top and each a line under the last. The last line's
/// baseline is where [`page`]'s one line's is.
fn page_of(lines: &[Vec<Stroke>]) -> Vec<u8> {
    let face = Face::new("Fixture Hebrew", COVERS);
    let program = face.build();
    let mut builder = DocumentBuilder::new();
    assert!(builder.add_cid_font(b"F0", b"FixtureHebrew", &program));
    let below = LEADING * lines.len().saturating_sub(1) as f64;
    let mut content = Vec::new();
    for (row, strokes) in lines.iter().enumerate() {
        let texts: Vec<String> = strokes.iter().map(|(ch, _)| ch.to_string()).collect();
        let glyphs: Vec<PlacedGlyph<'_>> = strokes
            .iter()
            .zip(texts.iter())
            .map(|((ch, x), text)| PlacedGlyph {
                glyph: Glyph {
                    id: face
                        .glyph_of(*ch)
                        .expect("the face covers what the page draws"),
                    text,
                },
                x: *x,
                rise: 0.0,
            })
            .collect();
        let baseline = 100.0 + below - LEADING * row as f64;
        assert!(builder.glyph_run(
            &mut content,
            b"F0",
            SIZE,
            [1.0, 0.0, 0.0, 1.0, 40.0, baseline],
            &glyphs
        ));
    }
    builder.add_page(300.0, 200.0 + below, |page| page.raw(&content));
    builder.finish()
}

/// Every line of the page's text through [`tinker_pdf::Page::text`], with
/// the direction ruling 14 read it in (`true` for right to left).
fn extract_lines(bytes: Vec<u8>) -> Vec<(String, bool)> {
    let doc = Document::open(bytes).expect("the page opens");
    let text = doc.page(0).expect("a page").text();
    text.lines()
        .into_iter()
        .map(|line| (line.text.clone(), line.rtl))
        .collect()
}

/// `text` drawn in visual order: its characters laid left to right in the
/// order given, each one advance on from the last.
fn visual(text: &str) -> Vec<Stroke> {
    text.chars()
        .enumerate()
        .map(|(i, ch)| (ch, i as f64 * ADVANCE))
        .collect()
}

/// The page's text through [`tinker_pdf::Page::text`], without the trailing
/// newline of its one line.
fn extract(bytes: Vec<u8>) -> String {
    let doc = Document::open(bytes).expect("the page opens");
    let text = doc.page(0).expect("a page").text().plain_text();
    text.trim_end_matches('\n').to_owned()
}

fn reversed(text: &str) -> String {
    text.chars().rev().collect()
}

/// **The headline.** A Hebrew word drawn as the eye sees it — final mem at
/// the left, shin at the right — extracts as it is read.
#[test]
fn a_hebrew_word_drawn_in_visual_order_extracts_in_reading_order() {
    let bytes = page(&visual(&reversed(SHALOM)));
    assert_eq!(extract(bytes), SHALOM);
}

/// **And the other producer's habit gives the same answer.** The same word
/// drawn in reading order, shin first at the right and the pen moving left,
/// is the same page to a reader — and it extracted right before the ruling,
/// so a rule that reversed the content stream rather than the picture would
/// have broken it.
#[test]
fn the_same_word_drawn_in_reading_order_extracts_the_same() {
    let strokes: Vec<Stroke> = SHALOM
        .chars()
        .enumerate()
        .map(|(i, ch)| (ch, (3 - i) as f64 * ADVANCE))
        .collect();
    assert_eq!(extract(page(&strokes)), SHALOM);
}

/// **The opt-out is the content stream's order**, which for a visual
/// producer is the word backwards.
#[test]
fn content_order_is_the_order_the_stream_drew() {
    let doc = Document::open(page(&visual(&reversed(SHALOM)))).expect("opens");
    let page = doc.page(0).expect("a page");
    let drawn = page
        .text_with(&TextOptions {
            content_order: true,
        })
        .plain_text();
    assert_eq!(drawn.trim_end(), reversed(SHALOM));
    // And the default is the same call with the default options.
    assert_eq!(
        page.text_with(&TextOptions::default()).plain_text(),
        page.text().plain_text()
    );
}

/// **A number inside a right-to-left line keeps its own order**, which is
/// what resolving levels rather than reversing the line buys: L2 reverses the
/// digits twice.
#[test]
fn digits_in_a_right_to_left_line_read_left_to_right() {
    // As read: "shalom 2026". As drawn: "2026 molahs".
    let read = format!("{SHALOM} 2026");
    let drawn = format!("2026 {}", reversed(SHALOM));
    assert_eq!(extract(page(&visual(&drawn))), read);
}

/// **A percentage in an Arabic line reads as typed** (review of lane 6C).
///
/// W2 makes digits after an Arabic letter `AN`, so the `%` is a neutral and
/// is drawn on the number's left: `%50`. Levels resolved over the line as
/// drawn see an `ET` touching an `EN` and join it to the number, which read
/// the line back as `nisba %50`; the forward check is what refuses that.
#[test]
fn a_percentage_in_an_arabic_line_reads_as_typed() {
    let read = format!("{NISBA} 50%");
    let drawn = format!("%50 {}", reversed(NISBA));
    assert_eq!(extract(page(&visual(&drawn))), read);
}

/// **A line in a right-to-left script outside the Hebrew and Arabic blocks
/// reads right to left.** The paragraph direction is read from the line's
/// characters' own `Bidi_Class` (review of lane 6C); a table of blocks counted
/// N'Ko's letters as left to right, because they are alphabetic.
#[test]
fn an_nko_line_reads_right_to_left() {
    let drawn = reversed(NKO);
    assert_eq!(extract(page(&visual(&drawn))), NKO);
}

/// **An English line holding a Hebrew word longer than its English stays an
/// English line.** Its leftmost and rightmost strong characters are Latin, so
/// no left-to-right paragraph and no right-to-left one would draw it any
/// other way than a left-to-right paragraph does; a count of strong
/// characters took it as right to left and reversed the English around the
/// word (review of lane 6C).
#[test]
fn an_english_line_holding_a_longer_hebrew_word_stays_left_to_right() {
    let read = format!("a {SHALOM} b");
    let drawn = format!("a {} b", reversed(SHALOM));
    assert_eq!(extract(page(&visual(&drawn))), read);
}

/// **Two texts UAX #9 draws alike read as the stated one.**
///
/// In a right-to-left paragraph `shalom 2026 now` and `shalom now 2026` are
/// the same picture: W7 makes digits after a Latin word left to right, so
/// `now 2026` is one run either way. No reader can tell them apart, and
/// `tinker_pdf_shape::bidi::logical_order` says which it returns: the one
/// that keeps a Latin word and its number together. What it returns always
/// draws the line as drawn.
#[test]
fn two_texts_drawn_alike_read_as_the_stated_one() {
    use tinker_pdf_shape::bidi::{order_units, BaseDirection};
    let number_first = format!("{SHALOM} 2026 now");
    let word_first = format!("{SHALOM} now 2026");
    let draw = |text: &str| -> String {
        let units: Vec<String> = text.chars().map(String::from).collect();
        let borrowed: Vec<&str> = units.iter().map(String::as_str).collect();
        order_units(&borrowed, BaseDirection::RightToLeft)
            .into_iter()
            .map(|at| borrowed[at])
            .collect()
    };
    let drawn = format!("now 2026 {}", reversed(SHALOM));
    assert_eq!(draw(&number_first), drawn);
    assert_eq!(draw(&word_first), drawn);
    assert_eq!(extract(page(&visual(&drawn))), word_first);
}

/// **A Hebrew word inside an English line** is reversed back and the English
/// around it is not.
#[test]
fn a_right_to_left_word_in_a_left_to_right_line() {
    let read = format!("see {SHALOM} now");
    let drawn = format!("see {} now", reversed(SHALOM));
    assert_eq!(extract(page(&visual(&drawn))), read);
}

/// **A right-to-left word and its comma alone on a line of a left-to-right
/// page read with the comma after the word**: ruling 14's comma tie-break,
/// amended 10 October 2026.
///
/// Moby-Dick's etymology sets `חו,` alone in a table cell. A left-to-right
/// paragraph draws the comma right of the word, `וח,`. The line holds no
/// left-to-right character, so both its ends are right to left, and the rule
/// before the amendment read it as a right-to-left paragraph, comma first:
/// `,חו`, which CI's conservation sweep of the book counted as a
/// transposition. A strong right-to-left character leftmost and punctuation
/// rightmost now read as a left-to-right paragraph, and say so. The Arabic
/// `حوت,` is the same shape; the Latin lines around them are untouched.
#[test]
fn a_right_to_left_word_and_its_comma_alone_on_a_line_read_as_written() {
    let lines = [
        visual("see"),
        visual(&format!("{},", reversed(HET_VAV))),
        visual(&format!("{},", reversed(HUT))),
        visual("now"),
    ];
    assert_eq!(
        extract_lines(page_of(&lines)),
        [
            ("see".to_owned(), false),
            (format!("{HET_VAV},"), false),
            (format!("{HUT},"), false),
            ("now".to_owned(), false),
        ]
    );
}

/// **A right-to-left line ending in a full stop still reads right to left.**
/// Its paragraph draws the stop leftmost, `.םולש`, so the line's rightmost
/// unit is a letter and the tie-break does not reach it.
#[test]
fn a_right_to_left_line_ending_in_a_full_stop_still_reads_right_to_left() {
    let drawn = format!(".{}", reversed(SHALOM));
    assert_eq!(
        extract_lines(page(&visual(&drawn))),
        [(format!("{SHALOM}."), true)]
    );
}

/// **A line holding a left-to-right character is out of the tie-break's
/// reach**, whatever its ends. Two lines a right-to-left paragraph typed, each
/// drawn with a right-to-left letter leftmost and a dash rightmost, the
/// tie-break's shape. In `— a שלום`, drawn `םולש a —`, the `a` makes the ends
/// of the strong characters disagree, and the majority reads it right to
/// left. In `— שלום a חו`, drawn `וח a םולש —`, the strong characters are
/// Hebrew at both ends and the `a` lies between them: the ends agree, and
/// only the `a` itself keeps the tie-break off the line. Both read right to
/// left, as before the amendment.
#[test]
fn a_line_holding_a_left_to_right_character_keeps_its_rule() {
    let lines = [
        visual(&format!("{} a \u{2014}", reversed(SHALOM))),
        visual(&format!(
            "{} a {} \u{2014}",
            reversed(HET_VAV),
            reversed(SHALOM)
        )),
    ];
    assert_eq!(
        extract_lines(page_of(&lines)),
        [
            (format!("\u{2014} a {SHALOM}"), true),
            (format!("\u{2014} {SHALOM} a {HET_VAV}"), true),
        ]
    );
}

/// **The amendment's price, pinned so that changing it is a decision.** A
/// right-to-left paragraph's line that opens with punctuation and holds
/// nothing left to right — a dialogue dash, `— שלום`, alone on its line —
/// is drawn `םולש —`, the tie-break's shape, and reads with the dash at its
/// end: `שלום —`. Both paragraphs draw that line, and nothing on it says
/// which one did; ruling 14, amended 10 October 2026, takes the commoner, a
/// right-to-left word quoted in left-to-right text with its punctuation
/// after it. The owner was told this is the cost.
#[test]
fn a_right_to_left_line_opening_with_a_dash_reads_the_dash_last() {
    let drawn = format!("{} \u{2014}", reversed(SHALOM));
    assert_eq!(
        extract_lines(page(&visual(&drawn))),
        [(format!("{SHALOM} \u{2014}"), false)]
    );
}

/// **A right-to-left line that opens with a mark and a number reads as
/// typed** (review of the comma tie-break). `(1) פריט`, `— 2026 שלום` and
/// `• 5 תפוחים`, each a right-to-left paragraph's line alone, are drawn with
/// the number next to the mark, `טירפ )1(`: a letter leftmost, punctuation
/// rightmost and nothing left to right, the tie-break's shape. But no
/// left-to-right paragraph draws a European number there, so a left-to-right
/// reading has no order that checks, and the tie-break taken regardless
/// returned the search's first candidate, which draws another line:
/// `פריט )1(`. The tie-break now takes left to right only where a
/// left-to-right reading draws the line as it stands.
#[test]
fn a_right_to_left_line_opening_with_a_mark_and_a_number_reads_as_typed() {
    let lines = [
        visual(&format!("{} )1(", reversed(PRIT))),
        visual(&format!("{} 2026 \u{2014}", reversed(SHALOM))),
        visual(&format!("{} 5 \u{2022}", reversed(TAPUCHIM))),
    ];
    assert_eq!(
        extract_lines(page_of(&lines)),
        [
            (format!("(1) {PRIT}"), true),
            (format!("\u{2014} 2026 {SHALOM}"), true),
            (format!("\u{2022} 5 {TAPUCHIM}"), true),
        ]
    );
}

/// **A right-to-left line that opens with a bracket or a quotation mark
/// reads as typed** (review of the comma tie-break). In a left-to-right
/// reading a mark after the word closes, so an opening one drawn at the right
/// of a line is a right-to-left line's first character. `(١) بند` is drawn
/// `دنب )١(`, which a left-to-right paragraph draws too, from `١) بند(`, and
/// the tie-break read it so; `“שלום`, drawn `םולש“`, read `שלום“`, and
/// `«مرحبا` read `مرحبا«`. An opening bracket or quotation mark,
/// `General_Category` `Ps` or `Pi`, is not trailing punctuation.
#[test]
fn a_right_to_left_line_opening_with_a_bracket_or_quotation_mark_reads_as_typed() {
    let lines = [
        visual(&format!("{} )\u{661}(", reversed(BAND))),
        visual(&format!("{}\u{201C}", reversed(SHALOM))),
        visual(&format!("{}\u{AB}", reversed(MARHABA))),
    ];
    assert_eq!(
        extract_lines(page_of(&lines)),
        [
            (format!("(\u{661}) {BAND}"), true),
            (format!("\u{201C}{SHALOM}"), true),
            (format!("\u{AB}{MARHABA}"), true),
        ]
    );
}

/// A one-page document drawing each of `pieces` as a run of its own — a
/// text object each — all on one baseline, in the order given. Each
/// stroke's place is from the line's start, as [`visual`] gives it.
fn pieces_page(pieces: &[Vec<Stroke>]) -> Vec<u8> {
    let face = Face::new("Fixture Hebrew", COVERS);
    let program = face.build();
    let mut builder = DocumentBuilder::new();
    assert!(builder.add_cid_font(b"F0", b"FixtureHebrew", &program));
    let mut content = Vec::new();
    for strokes in pieces {
        let texts: Vec<String> = strokes.iter().map(|(ch, _)| ch.to_string()).collect();
        let glyphs: Vec<PlacedGlyph<'_>> = strokes
            .iter()
            .zip(texts.iter())
            .map(|((ch, x), text)| PlacedGlyph {
                glyph: Glyph {
                    id: face
                        .glyph_of(*ch)
                        .expect("the face covers what the page draws"),
                    text,
                },
                x: *x,
                rise: 0.0,
            })
            .collect();
        assert!(builder.glyph_run(
            &mut content,
            b"F0",
            SIZE,
            [1.0, 0.0, 0.0, 1.0, 40.0, 100.0],
            &glyphs
        ));
    }
    builder.add_page(300.0, 200.0, |page| page.raw(&content));
    builder.finish()
}

/// **A right-to-left line that opens with a bracketed Latin run reads as
/// written** — ROADMAP K-11, and ruling 14's bracket tie-break, amended 10
/// October 2026.
///
/// `(seen wan). חו`, a right-to-left paragraph's line, is drawn
/// `וח .)seen wan(`: the pair at the paragraph's level, typed characters
/// named, each bracket facing away from the words it encloses. An EPUB
/// writes it as three runs at three levels, each a text object, in reading
/// order: the `(` at the right end, the Latin words, then `וח .)` at the
/// left — three pieces on one baseline.
///
/// Two things read it backwards. The `(` and the Latin words hold no
/// right-to-left character, and the join asked that of each pair of
/// pieces, so the `(` was read as a line of its own; the whole chain is
/// joined now. And the line holds seven Latin letters to two Hebrew ones,
/// its ends disagree, and the majority read it left to right,
/// `חו .)seen wan(`, which a left-to-right paragraph draws as the line
/// stands; only a pair at a right-to-left level is drawn facing outward, and
/// the right-to-left reading pairs it.
#[test]
fn a_right_to_left_line_opening_with_a_bracketed_latin_run_reads_as_written() {
    let drawn = format!("{} .)seen wan(", reversed(HET_VAV));
    let strokes = visual(&drawn);
    let (left, rest) = strokes.split_at(5);
    let (latin, open) = rest.split_at(rest.len() - 1);
    let pieces = [open.to_vec(), latin.to_vec(), left.to_vec()];
    assert_eq!(
        extract_lines(pieces_page(&pieces)),
        [(format!("(seen wan). {HET_VAV}"), true)]
    );
}

/// **Brackets that are no pair as typed keep a left-to-right line's
/// direction** — ruling 14's bracket tie-break as the review of the K-11 fix
/// revised it.
///
/// A list marker before a parenthesis, `1) see (שלום) as 2) one.`, and a
/// parenthesis opened at a line's end, `a) שלום b) חו (see`, each draw a
/// closing bracket left of an opening one, and a right-to-left paragraph
/// draws each line the same for another text, in which two of them pair
/// facing outward. As first proposed, the tie-break took that reading and
/// read both lines backwards: `.as 2) one )שלום( see )1` and
/// `see( חו )b שלום )a`. The first pairs no
/// more brackets read from the right than as typed, `(שלום)` against
/// `) see (`; the second pairs one read from the right, `see( חו )b`, but
/// with the spaces that stood outside its two brackets inside it, and a
/// bracket is set against its own text. Both read as typed.
#[test]
fn a_list_marker_and_a_wrapped_parenthesis_in_left_to_right_text_read_as_typed() {
    let first = format!("1) see ({SHALOM}) as 2) one.");
    let second = format!("a) {SHALOM} b) {HET_VAV} (see");
    let drawn = |typed: &str| {
        typed
            .replace(SHALOM, &reversed(SHALOM))
            .replace(HET_VAV, &reversed(HET_VAV))
    };
    assert_eq!(
        extract_lines(page_of(&[visual(&drawn(&first)), visual(&drawn(&second))])),
        [(first, false), (second, false)]
    );
}

/// **A mark stays with its base, whichever side of it the producer wrote it.**
///
/// Qamats on lamed. A producer that shapes and then reverses whole clusters
/// writes the mark after its base; one that reverses glyph by glyph writes it
/// before. The mark sits over the middle of lamed either way, and that is what
/// pairs them.
#[test]
fn a_mark_follows_its_base_whichever_side_the_producer_wrote_it() {
    let read: String = SHALOM
        .chars()
        .flat_map(|ch| {
            if ch == '\u{5DC}' {
                vec![ch, QAMATS]
            } else {
                vec![ch]
            }
        })
        .collect();
    // Visual positions: final mem 0, vav 10, lamed 20, shin 30; the mark at
    // the middle of lamed.
    let mark = (QAMATS, 2.0 * ADVANCE + ADVANCE / 2.0);
    let mut after = visual(&reversed(SHALOM));
    after.insert(3, mark);
    let mut before = visual(&reversed(SHALOM));
    before.insert(2, mark);
    for strokes in [after, before] {
        assert_eq!(extract(page(&strokes)), read, "{strokes:?}");
    }
}

/// **Search finds the word as it is read, and boxes it the right way round.**
///
/// The box's left edge is the word's left edge on the page — final mem's —
/// and its right edge shin's. A box taken from the logically first glyph as
/// though it were the leftmost would come back inside out.
#[test]
fn search_finds_the_word_as_it_is_read() {
    let doc =
        Document::open(page(&visual(&format!("see {} now", reversed(SHALOM))))).expect("opens");
    let text = doc.page(0).expect("a page").text();
    let hits = text.search(SHALOM);
    assert_eq!(hits.len(), 1, "the word as read is found once");
    let (x0, _, x1, _) = hits[0].bounds();
    // "see " is four advances from the run's origin at 40.
    let left = 40.0 + 4.0 * ADVANCE;
    let right = left + 4.0 * ADVANCE;
    assert!(
        (x0 - left).abs() < 0.01 && (x1 - right).abs() < 0.01,
        "the box is {x0}..{x1}, not the word's {left}..{right}"
    );
    assert!(
        hits[0].ul.0 < hits[0].ur.0,
        "the box is inside out: {:?}",
        hits[0]
    );
    // And the word as drawn is not the word.
    assert!(text.search(&reversed(SHALOM)).is_empty());
}

/// **The structured formats read the same extraction.**
#[test]
fn the_structured_formats_carry_reading_order() {
    let doc = Document::open(page(&visual(&reversed(SHALOM)))).expect("opens");
    let page = doc.page(0).expect("a page");
    for format in [TextFormat::Json, TextFormat::Xml, TextFormat::Html] {
        let mut writer = TextWriter::new(format);
        writer.page(&page.text_frame(), &page.text());
        let out = writer.finish();
        assert!(
            out.contains(SHALOM),
            "{format:?} does not carry the word as read: {out}"
        );
    }
    // And the words of the line are the word as read.
    let text = page.text();
    let words: Vec<String> = text.lines()[0]
        .words()
        .into_iter()
        .map(|w| w.text)
        .collect();
    assert_eq!(words, vec![SHALOM.to_owned()]);
}

/// **A page with no right-to-left character is exactly what it was.**
///
/// Asserted over every committed PDF in `testdata/` and every page of the
/// committed EPUB books, line by line and character by character, against the
/// opt-out: the ruling may not move a byte of left-to-right text, and the
/// claim that it does not is checked rather than reasoned.
#[test]
fn no_left_to_right_page_moves() {
    let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../../testdata");
    let books = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/epub");
    let mut documents: Vec<(String, Vec<u8>)> = Vec::new();
    for dir in [root, books] {
        let mut entries: Vec<_> = std::fs::read_dir(dir)
            .expect("the committed fixtures are present")
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| matches!(p.extension().and_then(|e| e.to_str()), Some("pdf" | "epub")))
            .collect();
        entries.sort();
        for path in entries {
            let bytes = std::fs::read(&path).expect("readable");
            documents.push((path.display().to_string(), bytes));
        }
    }
    let mut pages = 0usize;
    let mut lines = 0usize;
    for (name, bytes) in documents {
        let Ok(doc) = Document::open(bytes) else {
            // An encrypted fixture opens with a password, which is not the
            // question here.
            continue;
        };
        for page in doc.pages() {
            let logical = page.text();
            let stream = page.text_with(&TextOptions {
                content_order: true,
            });
            let ours = logical.lines();
            let theirs = stream.lines();
            assert_eq!(ours.len(), theirs.len(), "{name}: the lines changed");
            for (a, b) in ours.iter().zip(theirs.iter()) {
                if a.text.chars().any(|c| {
                    matches!(
                        u32::from(c),
                        0x0590..=0x08FF | 0xFB1D..=0xFDFF | 0xFE70..=0xFEFF
                    )
                }) {
                    continue;
                }
                assert_eq!(a.text, b.text, "{name}: a left-to-right line moved");
                assert_eq!(a.chars.len(), b.chars.len());
                for (x, y) in a.chars.iter().zip(b.chars.iter()) {
                    assert_eq!(x.text, y.text, "{name}");
                    assert_eq!(x.quad, y.quad, "{name}");
                }
                lines += 1;
            }
            pages += 1;
        }
    }
    // A loop that found nothing would pass, so the counts are pinned: every
    // `testdata` document that opens without a password and the nine
    // committed books, measured when the ruling was made. A fixture added to
    // either directory moves them, which is the point.
    assert_eq!(
        (pages, lines),
        (58, 454),
        "the pages and lines compared moved"
    );
}

/// What [`stated_page`] writes into the content stream, in order.
enum Item {
    /// `/Span << … >> BDC`, the dictionary's entries as given.
    Open(String),
    /// `EMC`.
    Close,
    /// A run of strokes on a line, `0` the top one.
    Run(usize, Vec<Stroke>),
}

/// `text` as a PDF text string: UTF-16BE behind its byte-order mark
/// (7.9.2.2), in hexadecimal.
fn text_string(text: &str) -> String {
    let mut hex = String::from("<FEFF");
    for unit in text.encode_utf16() {
        hex.push_str(&format!("{unit:04X}"));
    }
    hex.push('>');
    hex
}

/// A one-page document of `rows` lines, drawn as `items` say, with
/// [`page_of`]'s face, size and leading.
fn stated_page(rows: usize, items: &[Item]) -> Vec<u8> {
    let face = Face::new("Fixture Hebrew", COVERS);
    let program = face.build();
    let mut builder = DocumentBuilder::new();
    assert!(builder.add_cid_font(b"F0", b"FixtureHebrew", &program));
    let content = stated_content(&mut builder, &face, rows, items);
    let below = LEADING * rows.saturating_sub(1) as f64;
    builder.add_page(300.0, 200.0 + below, |page| page.raw(&content));
    builder.finish()
}

/// [`stated_page`]'s page, `items` drawn in a form XObject, `/Fm0`, which
/// the page's own content invokes.
fn formed_page(rows: usize, items: &[Item]) -> Vec<u8> {
    let face = Face::new("Fixture Hebrew", COVERS);
    let program = face.build();
    let mut builder = DocumentBuilder::new();
    assert!(builder.add_cid_font(b"F0", b"FixtureHebrew", &program));
    let content = stated_content(&mut builder, &face, rows, items);
    let below = LEADING * rows.saturating_sub(1) as f64;
    assert!(builder.add_form(
        b"Fm0",
        &FormXObject {
            bbox: [0.0, 0.0, 300.0, 200.0 + below],
            matrix: None,
            group: None,
            content: &content,
        }
    ));
    builder.add_page(300.0, 200.0 + below, |page| page.raw(b"/Fm0 Do"));
    builder.finish()
}

/// The content stream [`stated_page`] draws `items` in, with `face` as `F0`.
fn stated_content(
    builder: &mut DocumentBuilder,
    face: &Face,
    rows: usize,
    items: &[Item],
) -> Vec<u8> {
    let below = LEADING * rows.saturating_sub(1) as f64;
    let mut content = Vec::new();
    for item in items {
        match item {
            Item::Open(dict) => {
                content.extend_from_slice(format!("/Span <<{dict}>> BDC\n").as_bytes())
            }
            Item::Close => content.extend_from_slice(b"EMC\n"),
            Item::Run(row, strokes) => {
                let texts: Vec<String> = strokes.iter().map(|(ch, _)| ch.to_string()).collect();
                let glyphs: Vec<PlacedGlyph<'_>> = strokes
                    .iter()
                    .zip(texts.iter())
                    .map(|((ch, x), text)| PlacedGlyph {
                        glyph: Glyph {
                            id: face
                                .glyph_of(*ch)
                                .expect("the face covers what the page draws"),
                            text,
                        },
                        x: *x,
                        rise: 0.0,
                    })
                    .collect();
                let baseline = 100.0 + below - LEADING * *row as f64;
                assert!(builder.glyph_run(
                    &mut content,
                    b"F0",
                    SIZE,
                    [1.0, 0.0, 0.0, 1.0, 40.0, baseline],
                    &glyphs
                ));
            }
        }
    }
    content
}

/// `/ActualText` stating `text`, the entries of a span's dictionary.
fn stating(text: &str) -> String {
    format!("/ActualText {}", text_string(text))
}

/// **A line a span states whole reads as its text** (14.9.4; ruling 14,
/// H-152). Left (1)'s shape, typed in a left-to-right paragraph,
/// `שלום 60.`, is drawn `60 םולש.`, which a right-to-left paragraph draws
/// for `.שלום 60`: read off the line, the stop comes first. Stated, the line
/// reads as typed, in `Page::text` and in the stream's order alike, each
/// character on the glyph that draws it.
#[test]
fn a_line_a_span_states_whole_reads_as_its_text() {
    let typed = format!("{SHALOM} 60.");
    let drawn = format!("60 {}.", reversed(SHALOM));
    assert_ne!(
        extract(page(&visual(&drawn))),
        typed,
        "read off the line alone"
    );
    let bytes = stated_page(
        1,
        &[
            Item::Open(stating(&typed)),
            Item::Run(0, visual(&drawn)),
            Item::Close,
        ],
    );
    let doc = Document::open(bytes).expect("opens");
    let page = doc.page(0).expect("a page");
    let text = page.text();
    assert_eq!(text.plain_text().trim_end(), typed);
    let line = text.lines()[0].clone();
    assert!(line.rtl, "the stated text opens with a Hebrew letter");
    // Shin is drawn seventh from the left: its character stands on it.
    let shin = line
        .chars
        .iter()
        .find(|c| c.text == "\u{5E9}")
        .expect("shin");
    assert!(
        (shin.quad.ll.0 - (40.0 + 6.0 * ADVANCE)).abs() < 0.01,
        "{:?}",
        shin.quad
    );
    assert_eq!(text.search(SHALOM).len(), 1, "the stated word is found");
    let stream = page.text_with(&TextOptions {
        content_order: true,
    });
    assert_eq!(
        stream.plain_text().trim_end(),
        typed,
        "in the stream's order too"
    );
}

/// **A span holding part of a left-to-right line puts its text in its
/// glyphs' place**, the rest of the line as drawn.
#[test]
fn a_span_inside_a_left_to_right_line_stands_where_its_glyphs_do() {
    let bytes = stated_page(
        1,
        &[
            Item::Run(0, vec![('s', 0.0)]),
            Item::Open(stating("wa")),
            Item::Run(0, vec![('e', ADVANCE)]),
            Item::Close,
            Item::Run(0, vec![('o', 2.0 * ADVANCE)]),
        ],
    );
    let doc = Document::open(bytes).expect("opens");
    assert_eq!(
        doc.page(0).expect("a page").text().plain_text().trim_end(),
        "swao"
    );
}

/// **A span holding part of a right-to-left line is one unit of its order**,
/// read as U+FFFC, a neutral that takes the level of the letters either
/// side: drawn left of `שלום`, it is read after it.
#[test]
fn a_span_inside_a_right_to_left_line_is_one_unit_of_its_order() {
    let drawn: Vec<Stroke> = visual(&format!("\u{5D5}\u{5D7} {}", reversed(SHALOM)));
    let bytes = stated_page(
        1,
        &[
            Item::Open(stating(PRIT)),
            Item::Run(0, drawn[..2].to_vec()),
            Item::Close,
            Item::Run(0, drawn[2..].to_vec()),
        ],
    );
    assert_eq!(extract(bytes), format!("{SHALOM} {PRIT}"));
}

/// **A span across two lines puts its text on the first**, and the glyphs
/// it holds on the second read nothing there.
#[test]
fn a_span_across_two_lines_states_its_text_on_the_first() {
    let bytes = stated_page(
        2,
        &[
            Item::Run(0, visual("wa")),
            Item::Open(stating("seon")),
            Item::Run(0, vec![('s', 2.0 * ADVANCE), ('e', 3.0 * ADVANCE)]),
            Item::Run(1, visual("on")),
            Item::Close,
            Item::Run(1, vec![('b', 2.0 * ADVANCE)]),
        ],
    );
    let lines: Vec<String> = extract_lines(bytes)
        .into_iter()
        .map(|(text, _)| text)
        .collect();
    assert_eq!(lines, ["waseon", "b"]);
}

/// **A span's text stands on its glyphs on every line they are on**: read on
/// the first line, each of its characters keeps the sequence, the font and
/// the place of the glyph that draws it, on the second line as on the
/// first, so that a structure tree reads each element's own text and search
/// boxes each character where it is drawn.
#[test]
fn a_span_across_two_lines_keeps_each_characters_own_glyph() {
    let bytes = stated_page(
        2,
        &[
            Item::Open(stating("seon")),
            sequence(0),
            Item::Run(0, vec![('s', 0.0), ('e', ADVANCE)]),
            Item::Close,
            sequence(1),
            Item::Run(1, visual("on")),
            Item::Close,
            Item::Close,
        ],
    );
    let doc = Document::open(bytes).expect("opens");
    let text = doc.page(0).expect("a page").text();
    let lines = text.lines();
    assert_eq!(lines.len(), 1, "the second line reads nothing");
    let placed: Vec<(String, Option<u32>, bool)> = lines[0]
        .chars
        .iter()
        .map(|c| (c.text.clone(), c.mcid, c.quad.ll.1 < 120.0))
        .collect();
    let expect = |text: &str, mcid: u32, below: bool| (text.to_owned(), Some(mcid), below);
    assert_eq!(
        placed,
        [
            expect("s", 0, false),
            expect("e", 0, false),
            expect("o", 1, true),
            expect("n", 1, true),
        ]
    );
}

/// **A span whose list carries an `/MCID` states its text too**, and each
/// character keeps the sequence it was drawn in.
#[test]
fn a_span_with_an_mcid_states_its_text_in_its_sequence() {
    let typed = format!("{SHALOM} 60.");
    let drawn = format!("60 {}.", reversed(SHALOM));
    let bytes = stated_page(
        1,
        &[
            Item::Open(format!("/MCID 0 {}", stating(&typed))),
            Item::Run(0, visual(&drawn)),
            Item::Close,
        ],
    );
    let doc = Document::open(bytes).expect("opens");
    let text = doc.page(0).expect("a page").text();
    assert_eq!(text.plain_text().trim_end(), typed);
    assert!(text.lines()[0].chars.iter().all(|c| c.mcid == Some(0)));
}

/// [`stated_page`]'s `/Span` holding the marked-content sequence `/MCID
/// mcid` of the stream it is drawn in (14.7.4.2).
fn sequence(mcid: u32) -> Item {
    Item::Open(format!("/MCID {mcid}"))
}

/// A structure element [`declaring`] writes, under the element before it
/// that `parent` names or else under the tree's root: the attribute objects
/// its `/A` holds, each an owner, a key and the name it is given, and the
/// classes its `/C` names.
#[derive(Default)]
struct Element {
    parent: Option<usize>,
    attributes: Vec<(&'static str, &'static str, &'static str)>,
    classes: Vec<&'static str>,
}

impl Element {
    /// One under the tree's root whose Layout attributes state the writing
    /// mode `mode` (ISO 32000-2 §14.8.5.4.2).
    fn writing(mode: &'static str) -> Element {
        Element {
            attributes: vec![("Layout", "WritingMode", mode)],
            ..Element::default()
        }
    }
}

/// `bytes`, a one-page document, given a structure tree (14.7): `elements`;
/// its `/ClassMap`, `classes`, each a class's name and the one Layout
/// attribute it states, a key and a name; and `claims`, the page's
/// `/ParentTree` entry and then its form `/Fm0`'s, under the stream's
/// `/StructParents`, `/MCID n` of the stream belonging to the element the
/// entry's `n`th names (14.7.4.4).
fn declaring(
    bytes: Vec<u8>,
    elements: &[Element],
    classes: &[(&str, &str, &str)],
    claims: &[&[usize]],
) -> Vec<u8> {
    fn name(editor: &DocumentEditor, text: &str) -> Object {
        Object::Name(editor.intern(text.as_bytes()))
    }
    fn attribute(editor: &DocumentEditor, owner: &str, key: &str, value: &str) -> Object {
        let mut attributes = Dict::new();
        attributes.insert(editor.intern(b"O"), name(editor, owner));
        attributes.insert(editor.intern(key.as_bytes()), name(editor, value));
        Object::Dict(attributes)
    }
    let doc = Document::open(bytes).expect("opens");
    let cos = doc.cos();
    let page = tinker_pdf_cos::pages::collect(cos)[0].reference;
    let catalog = cos.trailer().get_ref(Name::ROOT).expect("a catalog");
    let mut editor = doc.editor();
    let page_dict = editor
        .get(page)
        .and_then(|page| page.as_dict().cloned())
        .expect("a page");
    let form = (claims.len() > 1).then(|| {
        let resources = cos.resolve_key(&page_dict, Name::RESOURCES);
        let forms = cos.resolve_key(
            resources.as_dict().expect("resources"),
            cos.intern(b"XObject"),
        );
        let form = forms.as_dict().expect("forms").get_ref(cos.intern(b"Fm0"));
        form.expect("the form /Fm0")
    });

    let root = editor.allocate();
    let parent_tree = editor.allocate();
    let refs: Vec<ObjRef> = elements.iter().map(|_| editor.allocate()).collect();
    for (at, element) in elements.iter().enumerate() {
        let mut dict = Dict::new();
        dict.insert(Name::TYPE, name(&editor, "StructElem"));
        dict.insert(editor.intern(b"S"), name(&editor, "P"));
        let parent = element.parent.map_or(root, |parent| refs[parent]);
        dict.insert(editor.intern(b"P"), Object::Ref(parent));
        dict.insert(editor.intern(b"Pg"), Object::Ref(page));
        let mut kids: Vec<Object> = elements
            .iter()
            .zip(&refs)
            .filter(|(kid, _)| kid.parent == Some(at))
            .map(|(_, kid)| Object::Ref(*kid))
            .collect();
        for (key, entry) in claims.iter().enumerate() {
            for (mcid, _) in entry.iter().enumerate().filter(|(_, e)| **e == at) {
                let mcid = Object::Int(mcid as i64);
                kids.push(match (key, form) {
                    (0, _) | (_, None) => mcid,
                    (_, Some(form)) => {
                        let mut reference = Dict::new();
                        reference.insert(Name::TYPE, name(&editor, "MCR"));
                        reference.insert(editor.intern(b"Stm"), Object::Ref(form));
                        reference.insert(editor.intern(b"MCID"), mcid);
                        Object::Dict(reference)
                    }
                });
            }
        }
        dict.insert(editor.intern(b"K"), Object::Array(kids));
        let attributes: Vec<Object> = element
            .attributes
            .iter()
            .map(|(owner, key, value)| attribute(&editor, owner, key, value))
            .collect();
        match <[Object; 1]>::try_from(attributes) {
            Ok([one]) => {
                dict.insert(editor.intern(b"A"), one);
            }
            // An array of them, each followed by its revision number.
            Err(many) if !many.is_empty() => {
                let revised = many.into_iter().flat_map(|a| [a, Object::Int(0)]);
                dict.insert(editor.intern(b"A"), Object::Array(revised.collect()));
            }
            Err(_) => {}
        }
        match element.classes.as_slice() {
            [] => {}
            [one] => {
                dict.insert(editor.intern(b"C"), name(&editor, one));
            }
            many => {
                let many = many.iter().map(|class| name(&editor, class)).collect();
                dict.insert(editor.intern(b"C"), Object::Array(many));
            }
        }
        editor.put(refs[at], Object::Dict(dict));
    }

    let mut tree = Dict::new();
    tree.insert(Name::TYPE, name(&editor, "StructTreeRoot"));
    let top: Vec<Object> = elements
        .iter()
        .zip(&refs)
        .filter(|(element, _)| element.parent.is_none())
        .map(|(_, element)| Object::Ref(*element))
        .collect();
    tree.insert(editor.intern(b"K"), Object::Array(top));
    tree.insert(editor.intern(b"ParentTree"), Object::Ref(parent_tree));
    if !classes.is_empty() {
        let mut map = Dict::new();
        for (class, key, value) in classes {
            let layout = attribute(&editor, "Layout", key, value);
            map.insert(editor.intern(class.as_bytes()), layout);
        }
        tree.insert(editor.intern(b"ClassMap"), Object::Dict(map));
    }
    editor.put(root, Object::Dict(tree));

    let mut nums = Vec::new();
    for (key, entry) in claims.iter().enumerate() {
        nums.push(Object::Int(key as i64));
        let entry = entry.iter().map(|element| Object::Ref(refs[*element]));
        nums.push(Object::Array(entry.collect()));
    }
    let mut numbers = Dict::new();
    numbers.insert(editor.intern(b"Nums"), Object::Array(nums));
    editor.put(parent_tree, Object::Dict(numbers));

    let mut catalog_dict = editor
        .get(catalog)
        .and_then(|catalog| catalog.as_dict().cloned())
        .expect("a catalog");
    catalog_dict.insert(editor.intern(b"StructTreeRoot"), Object::Ref(root));
    editor.put(catalog, Object::Dict(catalog_dict));
    let mut page_dict = page_dict;
    page_dict.insert(editor.intern(b"StructParents"), Object::Int(0));
    editor.put(page, Object::Dict(page_dict));
    if let Some(form) = form {
        let stored = cos.get(form).expect("the form");
        let stored = stored.as_stream().expect("a stream");
        let mut dict = Dict::new();
        for (key, value) in stored.dict.iter() {
            if ![Name::FILTER, Name::DECODE_PARMS, Name::LENGTH].contains(key) {
                dict.insert(*key, value.clone());
            }
        }
        dict.insert(editor.intern(b"StructParents"), Object::Int(1));
        let data = editor.stream_bytes(form).expect("the form's content");
        editor.put_stream(form, StreamData { dict, data });
    }
    editor.save(&WriteOptions::default())
}

/// Left (1)'s line, `שלום 60.` typed in a left-to-right paragraph, as it is
/// drawn: `60 םולש.`, which a right-to-left paragraph draws for `.שלום 60`.
fn left_one() -> (String, String) {
    (format!("{SHALOM} 60."), format!("60 {}.", reversed(SHALOM)))
}

/// A right-to-left paragraph's line opening with a dash, `— שלום`, as it is
/// drawn: `םולש —`, which a left-to-right paragraph draws for `שלום —`.
fn opening_dash() -> (String, String) {
    (
        format!("\u{2014} {SHALOM}"),
        format!("{} \u{2014}", reversed(SHALOM)),
    )
}

/// **A line its structure element declares left to right reads so**
/// (ISO 32000-2 §14.8.5.4.2; ruling 14, H-152). Left (1)'s line is drawn as
/// a right-to-left paragraph draws another text, and read off the line
/// alone, its stop comes first. Its element's Layout attributes state
/// `/WritingMode /LrTb`, and the line reads as typed, the stop last.
#[test]
fn a_line_its_element_declares_left_to_right_reads_its_stop_last() {
    let (typed, drawn) = left_one();
    let items = [sequence(0), Item::Run(0, visual(&drawn)), Item::Close];
    let undeclared = extract_lines(declaring(stated_page(1, &items), &[], &[], &[]));
    assert_ne!(undeclared[0].0, typed, "read off the line alone");
    let bytes = declaring(
        stated_page(1, &items),
        &[Element::writing("LrTb")],
        &[],
        &[&[0]],
    );
    assert_eq!(extract_lines(bytes), [(typed, false)]);
}

/// **A right-to-left line opening with a dash, declared right to left,
/// reads as typed**: the price of ruling 14's tie-break
/// (`a_right_to_left_line_opening_with_a_dash_reads_the_dash_last`), which
/// a page that says how it reads does not pay.
#[test]
fn a_right_to_left_line_opening_with_a_dash_declared_right_to_left_reads_as_typed() {
    let (typed, drawn) = opening_dash();
    let items = [sequence(0), Item::Run(0, visual(&drawn)), Item::Close];
    let bytes = declaring(
        stated_page(1, &items),
        &[Element::writing("RlTb")],
        &[],
        &[&[0]],
    );
    assert_eq!(extract_lines(bytes), [(typed, true)]);
}

/// **A writing mode holds where an element inherits it or a class states
/// it** (14.7.6.2, 14.8.5.4.2): a line whose element states nothing under
/// one declaring `/RlTb`; one whose `/C` names a class that states no
/// writing mode and then one that states `/RlTb`; one whose `/A` holds a
/// Table attribute object and a Layout one, each with its revision number;
/// and Left (1)'s line, whose `/A` states `/LrTb` and whose class states
/// `/RlTb`, `/A` first.
#[test]
fn a_writing_mode_inherited_or_stated_by_a_class_holds() {
    let (dash, dash_drawn) = opening_dash();
    let (left, left_drawn) = left_one();
    let mut items = Vec::new();
    for (row, drawn) in [&dash_drawn, &dash_drawn, &dash_drawn, &left_drawn]
        .into_iter()
        .enumerate()
    {
        items.extend([
            sequence(row as u32),
            Item::Run(row, visual(drawn)),
            Item::Close,
        ]);
    }
    let elements = [
        Element::writing("RlTb"),
        Element {
            parent: Some(0),
            ..Element::default()
        },
        Element {
            classes: vec!["Plain", "Verse"],
            ..Element::default()
        },
        Element {
            attributes: vec![("Table", "Scope", "Row"), ("Layout", "WritingMode", "RlTb")],
            ..Element::default()
        },
        Element {
            classes: vec!["Verse"],
            ..Element::writing("LrTb")
        },
    ];
    let classes = [
        ("Plain", "Placement", "Block"),
        ("Verse", "WritingMode", "RlTb"),
    ];
    let bytes = declaring(
        stated_page(4, &items),
        &elements,
        &classes,
        &[&[1, 2, 3, 4]],
    );
    assert_eq!(
        extract_lines(bytes),
        [
            (dash.clone(), true),
            (dash.clone(), true),
            (dash, true),
            (left, false),
        ]
    );
}

/// **A line the page does not declare whole reads as before**: one with a
/// character drawn outside any sequence, one with a character whose
/// sequence no element claims, two whose characters' elements declare two
/// directions, each way round, and one whose element declares a vertical
/// mode, which says nothing of the direction a line is read in.
#[test]
fn a_line_declared_in_part_or_two_ways_or_vertically_reads_as_before() {
    let (_, drawn) = opening_dash();
    let strokes = visual(&drawn);
    let (word, dash) = strokes.split_at(5);
    let items = [
        sequence(0),
        Item::Run(0, word.to_vec()),
        Item::Close,
        Item::Run(0, dash.to_vec()),
        sequence(1),
        Item::Run(1, word.to_vec()),
        Item::Close,
        sequence(9),
        Item::Run(1, dash.to_vec()),
        Item::Close,
        sequence(2),
        Item::Run(2, word.to_vec()),
        Item::Close,
        sequence(3),
        Item::Run(2, dash.to_vec()),
        Item::Close,
        sequence(5),
        Item::Run(3, word.to_vec()),
        Item::Close,
        sequence(6),
        Item::Run(3, dash.to_vec()),
        Item::Close,
        sequence(4),
        Item::Run(4, strokes.clone()),
        Item::Close,
    ];
    let before = extract_lines(stated_page(5, &items));
    let elements = [
        Element::writing("RlTb"),
        Element::writing("LrTb"),
        Element::writing("TbRl"),
    ];
    let bytes = declaring(
        stated_page(5, &items),
        &elements,
        &[],
        &[&[0, 0, 0, 1, 2, 1, 0]],
    );
    let read = extract_lines(bytes);
    assert_eq!(read, before);
    assert!(read
        .iter()
        .all(|(text, rtl)| !rtl && text.ends_with('\u{2014}')));
}

/// **A line the stream split reads in its declared direction whole**: the
/// dash of `— תפוחים` drawn first, at the right, and the word after it,
/// more than three ems to its left, so the device cuts the line there, each
/// in a sequence of its own. The pieces are one line once joined
/// (`text_order`'s `rejoin_split_lines`), and the dash's piece, which holds
/// no right-to-left character, is declared too.
#[test]
fn a_line_the_stream_split_reads_in_its_declared_direction() {
    let typed = format!("\u{2014} {TAPUCHIM}");
    let strokes = visual(&format!("{} \u{2014}", reversed(TAPUCHIM)));
    let (word, dash) = strokes.split_at(7);
    let items = [
        sequence(0),
        Item::Run(0, dash.to_vec()),
        Item::Close,
        sequence(1),
        Item::Run(0, word.to_vec()),
        Item::Close,
    ];
    let bytes = declaring(
        stated_page(1, &items),
        &[Element::writing("RlTb")],
        &[],
        &[&[0, 0]],
    );
    assert_eq!(extract_lines(bytes), [(typed, true)]);
}

/// **A sequence a form draws finds its element through the form's own
/// `/StructParents`** (14.7.4.4): the form's `/MCID 0` belongs to an element
/// declaring `/RlTb`, and the page's own `/MCID 0`, which it never draws, to
/// one declaring `/LrTb`.
#[test]
fn a_sequence_a_form_draws_finds_its_element_through_the_forms_entry() {
    let (typed, drawn) = opening_dash();
    let items = [sequence(0), Item::Run(0, visual(&drawn)), Item::Close];
    let bytes = declaring(
        formed_page(1, &items),
        &[Element::writing("LrTb"), Element::writing("RlTb")],
        &[],
        &[&[0], &[1]],
    );
    assert_eq!(extract_lines(bytes), [(typed, true)]);
}
