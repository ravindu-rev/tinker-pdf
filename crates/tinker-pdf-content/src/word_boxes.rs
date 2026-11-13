//! A page's words with their boxes, and the formats that write them.
//!
//! The model, the frame and every format's mapping are
//! `docs/design/word-boxes.md`'s. Born-digital text fills the model through
//! the facade, which alone can reach a page's view transform; a recogniser
//! fills it with no page at all, from the public fields.
//!
//! # The model
//!
//! ```text
//! WordBoxes                     created, modified
//! └ WordPage                    index, width, height, resolution, image
//!   └ WordBlock                 quad
//!     └ WordParagraph           quad
//!       └ WordLine              quad, direction
//!         └ WordBox             text, quad, confidence
//! ```
//!
//! # The frame
//!
//! Every quad is in the page **as displayed** — crop box and `/Rotate`
//! applied — with the origin at its top-left corner, `x` running right and `y`
//! running down, in pixels of `1 / resolution` inch: the frame of a render at
//! that resolution. [`crate::Quad`]'s corners keep their names as the text
//! reads, so [`Quad::bounds`] in this frame is `(left, top, right, bottom)`,
//! and [`upright`] is the quad of a recogniser's rectangle.
//!
//! # The words
//!
//! [`box_words`] makes a line's words: each of [`TextLine::words`] with the
//! punctuation and symbols it touches, as the published TSV's words carry
//! theirs (`(quick)`, `jumps!`), and a run of them between white space a word
//! of its own — so every character but white space is in a word.
//!
//! # TSV
//!
//! [`WordBoxes::tsv`] writes the OCR TSV the design transcribes: [`TSV_HEADER`],
//! then one row a page, block, paragraph, line and word, depth first, each
//! numbered from 1 within its parent and 0 at the levels above it, each box in
//! whole pixels, `conf` `-1` on every row but a word's and on a word with no
//! confidence, and `text` empty on every row but a word's. Every record has
//! twelve fields, so a row that is not a word's ends in a tab. A word's
//! characters that would end a field or a record are written as spaces: a
//! `text/tab-separated-values` field holds no tab, and a record is a line.
//!
//! # A box in whole pixels
//!
//! The quad's enclosing rectangle, clipped to the page, rounded outward — left
//! and top down, right and bottom up — so the box written holds the word and,
//! the page's edges being whole numbers, stays on the page.

use core::fmt::Write as _;

use crate::text::{Quad, TextLine};
use crate::words::{is_word, oriented_union, word_boundaries, TextWord};

/// A document's word boxes: its pages, and the two dates PAGE XML's metadata
/// requires.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct WordBoxes {
    /// The pages, in document order.
    pub pages: Vec<WordPage>,
    /// When the document was made, as an XML Schema `dateTime`, or `None`
    /// where nothing says.
    pub created: Option<String>,
    /// When it last changed, likewise.
    pub modified: Option<String>,
}

/// One page's word boxes, in the page's frame (see the module documentation).
#[derive(Clone, Debug, PartialEq)]
pub struct WordPage {
    /// The page's zero-based index in its document.
    pub index: u32,
    /// The page's width in the frame's pixels.
    pub width: u32,
    /// The page's height in the frame's pixels.
    pub height: u32,
    /// The frame's pixels per inch.
    pub resolution: f64,
    /// The name of the image the boxes stand on, or `None` for a page that is
    /// not an image.
    pub image: Option<String>,
    /// The blocks, in reading order.
    pub blocks: Vec<WordBlock>,
}

/// A block of paragraphs.
#[derive(Clone, Debug, PartialEq)]
pub struct WordBlock {
    /// The block's box, in the page's frame.
    pub quad: Quad,
    /// Its paragraphs, in reading order. A born-digital block holds one, whose
    /// box is the block's.
    pub paragraphs: Vec<WordParagraph>,
}

/// A paragraph of lines.
#[derive(Clone, Debug, PartialEq)]
pub struct WordParagraph {
    /// The paragraph's box, in the page's frame.
    pub quad: Quad,
    /// Its lines, in reading order.
    pub lines: Vec<WordLine>,
}

/// A line of words.
#[derive(Clone, Debug, PartialEq)]
pub struct WordLine {
    /// The line's box, in the page's frame, turned with a turned line.
    pub quad: Quad,
    /// Which way the line reads.
    pub direction: LineDirection,
    /// Its words, in reading order.
    pub words: Vec<WordBox>,
}

/// Which way a line reads.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LineDirection {
    /// Left to right.
    #[default]
    LeftToRight,
    /// Right to left.
    RightToLeft,
    /// Top to bottom: a vertical line.
    TopToBottom,
}

/// One word and its box.
#[derive(Clone, Debug, PartialEq)]
pub struct WordBox {
    /// What the word says.
    pub text: String,
    /// Its box, in the page's frame, turned with a turned word.
    pub quad: Quad,
    /// How sure a recogniser is of the text, a percentage from 0 to 100, or
    /// `None` for text that is no recognition result — every word of a
    /// born-digital page. A writer clamps a value outside 0 to 100 into it and
    /// writes one that is not a number as `None`.
    pub confidence: Option<f32>,
}

/// The quad of an upright rectangle in the word-box frame, where `y` runs
/// down: `top` is the smaller `y`.
#[must_use]
pub fn upright(left: f64, top: f64, right: f64, bottom: f64) -> Quad {
    Quad {
        ul: (left, top),
        ur: (right, top),
        ll: (left, bottom),
        lr: (right, bottom),
    }
}

/// What a UAX #29 segment of a line is to [`box_words`].
#[derive(Clone, Copy, PartialEq, Eq)]
enum Segment {
    /// A word of [`TextLine::words`].
    Word,
    /// White space, which separates words.
    Space,
    /// Punctuation or a symbol, which joins the word it touches.
    Mark,
}

/// The words of `line` as the word-box formats write them, each with the
/// union of its characters' quads in the frame of the line's own baseline —
/// the box [`TextLine::words`] gives a word — in the line's space.
///
/// Each of [`TextLine::words`] is a word, and the characters between them
/// that are not white space — punctuation and symbols — join the word before
/// them where no white space stands between, and otherwise the word after
/// them where none does; a run of them with white space or a line's end on
/// both sides is a word of its own. So `(quick)` and `jumps!` are one word
/// each, `Hello, world.` is `Hello,` and `world.`, and a run of ideographs is
/// a word an ideograph, as UAX #29 breaks it.
#[must_use]
pub fn box_words(line: &TextLine) -> Vec<TextWord> {
    let boundaries = word_boundaries(&line.text);
    let mut runs: Vec<(usize, usize)> = Vec::new();
    // The word being built, open to the marks after it until white space.
    let mut current: Option<(usize, usize)> = None;
    // Marks no word has taken: after white space, waiting for a word.
    let mut pending: Option<(usize, usize)> = None;
    for pair in boundaries.windows(2) {
        let &[start, end] = pair else {
            continue;
        };
        let Some(text) = line.text.get(start..end) else {
            continue;
        };
        let segment = if text.chars().all(char::is_whitespace) {
            Segment::Space
        } else if is_word(text) {
            Segment::Word
        } else {
            Segment::Mark
        };
        match segment {
            Segment::Word => {
                runs.extend(current.take());
                let from = pending.take().map_or(start, |(at, _)| at);
                current = Some((from, end));
            }
            Segment::Mark => {
                if let Some(run) = current.as_mut() {
                    run.1 = end;
                } else if let Some(run) = pending.as_mut() {
                    run.1 = end;
                } else {
                    pending = Some((start, end));
                }
            }
            Segment::Space => {
                runs.extend(current.take());
                runs.extend(pending.take());
            }
        }
    }
    runs.extend(current);
    runs.extend(pending);

    // Where each character's text starts and ends in the line's text, which
    // is their concatenation (`TextDevice::finish`).
    let mut spans = Vec::with_capacity(line.chars.len());
    let mut at = 0usize;
    for c in &line.chars {
        spans.push((at, at + c.text.len()));
        at += c.text.len();
    }

    let mut words = Vec::with_capacity(runs.len());
    // The runs are in order and apart, so the characters are walked once. A
    // character whose text two runs share — a ligature UAX #29 parts — is in
    // both, as `TextLine::words` puts it.
    let mut next = 0usize;
    for (start, end) in runs {
        while spans.get(next).is_some_and(|&(_, e)| e <= start) {
            next += 1;
        }
        let first = next;
        if !spans.get(first).is_some_and(|&(s, _)| s < end) {
            continue;
        }
        let mut last = first;
        while spans.get(last + 1).is_some_and(|&(s, _)| s < end) {
            last += 1;
        }
        let (Some(text), Some(covered)) = (line.text.get(start..end), line.chars.get(first..=last))
        else {
            continue;
        };
        let Some(quad) = oriented_union(covered.iter().map(|c| c.quad)) else {
            continue;
        };
        words.push(TextWord {
            text: text.to_string(),
            quad,
            chars: first..last + 1,
        });
    }
    words
}

/// TSV's header line: its twelve field names, a tab between each two.
pub const TSV_HEADER: &str =
    "level\tpage_num\tblock_num\tpar_num\tline_num\tword_num\tleft\ttop\twidth\theight\tconf\ttext";

impl WordBoxes {
    /// The document as TSV: [`TSV_HEADER`], then a row a page, block,
    /// paragraph, line and word, as the module documentation says.
    #[must_use]
    pub fn tsv(&self) -> String {
        let mut out = String::new();
        out.push_str(TSV_HEADER);
        out.push('\n');
        for page in &self.pages {
            let (width, height) = (page.width, page.height);
            let p = u64::from(page.index) + 1;
            tsv_row(&mut out, 1, [p, 0, 0, 0, 0], (0, 0, width, height), None);
            for (b, block) in (1u64..).zip(&page.blocks) {
                let edges = whole_box(&block.quad, width, height);
                tsv_row(&mut out, 2, [p, b, 0, 0, 0], edges, None);
                for (par, paragraph) in (1u64..).zip(&block.paragraphs) {
                    let edges = whole_box(&paragraph.quad, width, height);
                    tsv_row(&mut out, 3, [p, b, par, 0, 0], edges, None);
                    for (l, line) in (1u64..).zip(&paragraph.lines) {
                        let edges = whole_box(&line.quad, width, height);
                        tsv_row(&mut out, 4, [p, b, par, l, 0], edges, None);
                        for (w, word) in (1u64..).zip(&line.words) {
                            let edges = whole_box(&word.quad, width, height);
                            tsv_row(&mut out, 5, [p, b, par, l, w], edges, Some(word));
                        }
                    }
                }
            }
        }
        out
    }
}

/// One TSV record: the level, its five numbers, its box as whole-pixel edges
/// `(left, top, right, bottom)`, and the word on a word's row.
fn tsv_row(
    out: &mut String,
    level: u8,
    numbers: [u64; 5],
    (left, top, right, bottom): (u32, u32, u32, u32),
    word: Option<&WordBox>,
) {
    let [p, b, par, l, w] = numbers;
    let _ = write!(
        out,
        "{level}\t{p}\t{b}\t{par}\t{l}\t{w}\t{left}\t{top}\t{}\t{}\t",
        right.saturating_sub(left),
        bottom.saturating_sub(top),
    );
    match word.and_then(|word| confidence(word.confidence)) {
        Some(conf) => {
            let _ = write!(out, "{conf:.6}");
        }
        None => out.push_str("-1"),
    }
    out.push('\t');
    if let Some(word) = word {
        for c in word.text.chars() {
            out.push(if ends_a_field_or_record(c) { ' ' } else { c });
        }
    }
    out.push('\n');
}

/// The characters a TSV field cannot hold: the tab that separates fields, and
/// every character that ends a line — line feed, vertical tab, form feed,
/// carriage return, U+0085, U+2028 and U+2029.
fn ends_a_field_or_record(c: char) -> bool {
    matches!(
        c,
        '\t' | '\n' | '\u{b}' | '\u{c}' | '\r' | '\u{85}' | '\u{2028}' | '\u{2029}'
    )
}

/// A confidence as a writer takes it: clamped into 0 to 100, and `None` for
/// one that is not a number.
fn confidence(value: Option<f32>) -> Option<f64> {
    let value = f64::from(value?);
    (!value.is_nan()).then(|| value.clamp(0.0, 100.0))
}

/// A box in whole pixels: `quad`'s enclosing rectangle, clipped to a page of
/// `width` by `height` and rounded outward, as `(left, top, right, bottom)`.
fn whole_box(quad: &Quad, width: u32, height: u32) -> (u32, u32, u32, u32) {
    let (x0, y0, x1, y1) = quad.bounds();
    // `Quad::bounds` passes over a corner that is not a number, so a quad with
    // no corner left comes back with its edges crossed: a box with no extent,
    // at the page's corner.
    if !(x0 <= x1 && y0 <= y1) {
        return (0, 0, 0, 0);
    }
    let (w, h) = (f64::from(width), f64::from(height));
    let down = |v: f64, max: f64| v.clamp(0.0, max).floor() as u32;
    let up = |v: f64, max: f64| v.clamp(0.0, max).ceil() as u32;
    (down(x0, w), down(y0, h), up(x1, w), up(y1, h))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::text::{TextChar, WritingMode};

    /// A line built by hand, so the geometry is exactly what the test says.
    fn line(chars: Vec<TextChar>) -> TextLine {
        let text = chars.iter().map(|c| c.text.as_str()).collect();
        let quad = chars.first().map(|c| c.quad).expect("a character");
        TextLine {
            chars,
            text,
            quad,
            wmode: WritingMode::Horizontal,
            rtl: false,
            size: 10.0,
        }
    }

    /// `text`, one character a glyph five units wide and ten tall, from 0 along
    /// an upright baseline.
    fn upright_line(text: &str) -> TextLine {
        line(
            text.chars()
                .enumerate()
                .map(|(i, c)| {
                    let x = i as f64 * 5.0;
                    TextChar {
                        text: c.to_string(),
                        quad: Quad {
                            ll: (x, -2.0),
                            lr: (x + 5.0, -2.0),
                            ul: (x, 8.0),
                            ur: (x + 5.0, 8.0),
                        },
                        size: 10.0,
                        origin: (x, 0.0),
                        mcid: None,
                        stream: 0,
                        font: None,
                        stated: None,
                        artifact: false,
                        glyph: None,
                    }
                })
                .collect(),
        )
    }

    fn texts(line: &TextLine) -> Vec<String> {
        box_words(line).into_iter().map(|w| w.text).collect()
    }

    /// Punctuation joins the word it touches, the word before it first.
    #[test]
    fn punctuation_joins_the_word_it_touches() {
        assert_eq!(texts(&upright_line("Hello, world.")), ["Hello,", "world."]);
        assert_eq!(
            texts(&upright_line("The (quick) [brown] {fox} jumps!")),
            ["The", "(quick)", "[brown]", "{fox}", "jumps!"]
        );
        assert_eq!(
            texts(&upright_line("$43,456.78 #90")),
            ["$43,456.78", "#90"]
        );
        // A mark between two words with no white space joins the first.
        assert_eq!(texts(&upright_line("a/b")), ["a/", "b"]);
    }

    /// A run of marks with white space on both sides is a word of its own, so
    /// nothing but white space goes missing.
    #[test]
    fn marks_alone_between_spaces_are_a_word() {
        assert_eq!(
            texts(&upright_line("one \u{2014} two \u{2022}")),
            ["one", "\u{2014}", "two", "\u{2022}"]
        );
        assert_eq!(texts(&upright_line("  ")), Vec::<String>::new());
        assert_eq!(texts(&upright_line("?!")), ["?!"]);
    }

    /// Ideographs stay a word each, as `TextLine::words` breaks them, with
    /// the marks around them joined.
    #[test]
    fn ideographs_stay_a_word_each() {
        assert_eq!(
            texts(&upright_line("(6)\u{7DA0}\u{7DDA}")),
            ["(6)", "\u{7DA0}", "\u{7DDA}"]
        );
    }

    /// A word's box is the union of its characters' quads, its characters the
    /// range it covers.
    #[test]
    fn a_word_is_boxed_by_its_characters() {
        let l = upright_line("ab, cd");
        let words = box_words(&l);
        let first = words.first().expect("a first word");
        assert_eq!(first.chars, 0..3);
        assert_eq!(first.quad.bounds(), (0.0, -2.0, 15.0, 8.0));
        let second = words.get(1).expect("a second word");
        assert_eq!(second.chars, 4..6);
        assert_eq!(second.quad.bounds(), (20.0, -2.0, 30.0, 8.0));
    }

    /// The whole-pixel box is the enclosing rectangle clipped to the page and
    /// rounded outward, and a box that is not a number stays on the page.
    #[test]
    fn a_whole_box_is_clipped_and_rounded_outward() {
        let quad = upright(1.2, 3.7, 10.1, 12.0);
        assert_eq!(whole_box(&quad, 100, 100), (1, 3, 11, 12));
        assert_eq!(
            whole_box(&upright(-5.5, -1.0, 120.5, 50.5), 100, 40),
            (0, 0, 100, 40)
        );
        let lost = upright(f64::NAN, f64::NAN, f64::NAN, f64::NAN);
        assert_eq!(whole_box(&lost, 100, 100), (0, 0, 0, 0));
    }

    /// A confidence is clamped into 0 to 100 and one that is not a number is
    /// none.
    #[test]
    fn a_confidence_is_clamped_and_a_nan_is_none() {
        assert_eq!(confidence(Some(96.5)), Some(96.5));
        assert_eq!(confidence(Some(140.0)), Some(100.0));
        assert_eq!(confidence(Some(-3.0)), Some(0.0));
        assert_eq!(confidence(Some(f32::NAN)), None);
        assert_eq!(confidence(None), None);
    }

    /// Every TSV record has twelve fields, whatever its level.
    #[test]
    fn every_tsv_record_has_twelve_fields() {
        let word = WordBox {
            text: "a\tb\nc".to_string(),
            quad: upright(1.0, 1.0, 9.0, 9.0),
            confidence: Some(50.0),
        };
        let boxes = WordBoxes {
            pages: vec![WordPage {
                index: 0,
                width: 20,
                height: 20,
                resolution: 72.0,
                image: None,
                blocks: vec![WordBlock {
                    quad: upright(1.0, 1.0, 9.0, 9.0),
                    paragraphs: vec![WordParagraph {
                        quad: upright(1.0, 1.0, 9.0, 9.0),
                        lines: vec![WordLine {
                            quad: upright(1.0, 1.0, 9.0, 9.0),
                            direction: LineDirection::LeftToRight,
                            words: vec![word],
                        }],
                    }],
                }],
            }],
            ..WordBoxes::default()
        };
        let tsv = boxes.tsv();
        let records: Vec<&str> = tsv.lines().collect();
        assert_eq!(records.len(), 6, "{tsv}");
        for record in &records {
            assert_eq!(record.split('\t').count(), 12, "{record:?}");
        }
        assert_eq!(
            records.last(),
            Some(&"5\t1\t1\t1\t1\t1\t1\t1\t8\t8\t50.000000\ta b c")
        );
    }
}
