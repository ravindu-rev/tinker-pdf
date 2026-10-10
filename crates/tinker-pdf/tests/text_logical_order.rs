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
use tinker_pdf_cos::build::{DocumentBuilder, Glyph, PlacedGlyph};

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

/// Everything any page here draws.
const COVERS: &str = "\u{5E9}\u{5DC}\u{5D5}\u{5DD}\u{5B8}\u{646}\u{633}\u{628}\u{629}\u{7CA}\u{7CB}\u{7CC} 0123456789seonwab.%\u{5D7}\u{62D}\u{648}\u{62A},\u{2014}";

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
