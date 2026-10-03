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

/// Everything any page here draws.
const COVERS: &str = "\u{5E9}\u{5DC}\u{5D5}\u{5DD}\u{5B8} 0123456789seonw";

/// The font size, and so the advance of every glyph: the face's 500 units of
/// a 1000-unit em at twenty points is ten.
const SIZE: f64 = 20.0;
const ADVANCE: f64 = 10.0;

/// One glyph to draw: the character and where its origin goes along the
/// baseline, from the run's origin.
type Stroke = (char, f64);

/// A one-page document drawing `strokes`, in the order given, as one run.
fn page(strokes: &[Stroke]) -> Vec<u8> {
    let face = Face::new("Fixture Hebrew", COVERS);
    let program = face.build();
    let mut builder = DocumentBuilder::new();
    assert!(builder.add_cid_font(b"F0", b"FixtureHebrew", &program));
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
    let mut content = Vec::new();
    assert!(builder.glyph_run(
        &mut content,
        b"F0",
        SIZE,
        [1.0, 0.0, 0.0, 1.0, 40.0, 100.0],
        &glyphs
    ));
    builder.add_page(300.0, 200.0, |page| page.raw(&content));
    builder.finish()
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

/// **A Hebrew word inside an English line** is reversed back and the English
/// around it is not.
#[test]
fn a_right_to_left_word_in_a_left_to_right_line() {
    let read = format!("see {SHALOM} now");
    let drawn = format!("see {} now", reversed(SHALOM));
    assert_eq!(extract(page(&visual(&drawn))), read);
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
