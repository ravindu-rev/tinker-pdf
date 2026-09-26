//! Word boundaries (UAX #29, *Unicode Text Segmentation*, §4) and the word
//! boxes [`crate::TextLine::words`] builds from them.
//!
//! # Why the Unicode algorithm and not a space-scanner
//!
//! Splitting at whitespace is right for the text this engine's own fixtures
//! carry and wrong for most of the world's: Thai and Japanese write no spaces
//! between words, `can't` and `3.14` are one word each while `a.b` in running
//! text is not, and a combining accent belongs to the letter before it
//! whatever the whitespace says. UAX #29's default rules answer all of those,
//! and the Unicode Consortium publishes `WordBreakTest.txt` to prove an
//! implementation does — 1 944 cases, run whole by
//! `tests/uax29_conformance.rs` against [`word_boundaries`] itself.
//!
//! The rules are the **default** ones, untailored. UAX #29 notes that Thai,
//! Lao, Khmer, Myanmar and the ideographic scripts need a dictionary to find
//! word boundaries inside a run, and this crate carries none: an unspaced run
//! of Han is a boundary after every ideograph (WB999), and an unspaced run of
//! Thai is one segment. That is the algorithm's own stated limit rather than
//! a departure from it.
//!
//! # What counts as a word
//!
//! UAX #29 segments *everything*: the spaces and the punctuation between words
//! are segments too. [`crate::TextLine::words`] keeps a segment when it holds a
//! letter or a digit — a character with `Word_Break` `ALetter`,
//! `Hebrew_Letter`, `Numeric` or `Katakana`, or one the Unicode `Alphabetic`
//! or `Numeric` properties cover, which is how an ideograph (`Word_Break`
//! `Other`) is a word of its own — and drops runs of spaces, punctuation and
//! symbols, which are what separates words rather than words.

use crate::text::{Quad, TextLine};

/// UAX #29's `Word_Break` property values (Table 3).
#[allow(clippy::upper_case_acronyms)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum WordBreak {
    /// Letters of most alphabetic scripts.
    ALetter,
    /// U+000D.
    CR,
    /// `"`.
    DoubleQuote,
    /// Combining marks and other extenders, absorbed by WB4.
    Extend,
    /// Connector punctuation — `_` and its relatives.
    ExtendNumLet,
    /// Format characters, absorbed by WB4.
    Format,
    /// Hebrew letters, which WB7a–WB7c give quotation rules of their own.
    HebrewLetter,
    /// Katakana, which WB13 keeps together.
    Katakana,
    /// U+000A.
    LF,
    /// `:` and its relatives.
    MidLetter,
    /// `,` `;` and their relatives.
    MidNum,
    /// `.` and its relatives.
    MidNumLet,
    /// The other line and paragraph separators.
    Newline,
    /// Decimal digits.
    Numeric,
    /// U+1F1E6..U+1F1FF, which WB15 and WB16 pair into flags.
    RegionalIndicator,
    /// `'`.
    SingleQuote,
    /// Space separators, which WB3d keeps together.
    WSegSpace,
    /// U+200D ZERO WIDTH JOINER.
    ZWJ,
    /// Everything else.
    Other,
}

include!(concat!(env!("OUT_DIR"), "/ucd.rs"));

/// A code point's value in a sorted `(first, last, value)` table.
fn lookup<T: Copy>(table: &[(u32, u32, T)], code: u32, default: T) -> T {
    match table.binary_search_by(|&(first, last, _)| {
        if code < first {
            core::cmp::Ordering::Greater
        } else if code > last {
            core::cmp::Ordering::Less
        } else {
            core::cmp::Ordering::Equal
        }
    }) {
        Ok(at) => table.get(at).map_or(default, |&(_, _, value)| value),
        Err(_) => default,
    }
}

/// Whether a code point is in a sorted `(first, last)` table.
pub(crate) fn member(table: &[(u32, u32)], code: u32) -> bool {
    table
        .binary_search_by(|&(first, last)| {
            if code < first {
                core::cmp::Ordering::Greater
            } else if code > last {
                core::cmp::Ordering::Less
            } else {
                core::cmp::Ordering::Equal
            }
        })
        .is_ok()
}

/// A character's `Word_Break` property. Unlisted code points are `Other`,
/// which is the file's own `@missing` line.
pub(crate) fn word_break(c: char) -> WordBreak {
    lookup(WORD_BREAK, u32::from(c), WordBreak::Other)
}

/// Whether a character is `Extended_Pictographic` (UTS #51), for WB3c.
fn extended_pictographic(c: char) -> bool {
    member(EXTENDED_PICTOGRAPHIC, u32::from(c))
}

/// `AHLetter`: `ALetter | Hebrew_Letter`.
fn ah_letter(wb: Option<WordBreak>) -> bool {
    matches!(wb, Some(WordBreak::ALetter | WordBreak::HebrewLetter))
}

/// `MidNumLetQ`: `MidNumLet | Single_Quote`.
fn mid_num_let_q(wb: Option<WordBreak>) -> bool {
    matches!(wb, Some(WordBreak::MidNumLet | WordBreak::SingleQuote))
}

fn is(wb: Option<WordBreak>, want: WordBreak) -> bool {
    wb == Some(want)
}

/// The byte offsets of every word boundary in `text` (UAX #29 §4.1).
///
/// Includes `0` and `text.len()` for any non-empty text, which are WB1 and
/// WB2; an empty text has no boundaries at all. Consecutive offsets delimit
/// one segment each, words and the space between them alike.
///
/// Linear in the length of the text, including on hostile input: the one
/// rule that looks arbitrarily far back — WB15/WB16's count of regional
/// indicators — is carried forward as a running count rather than re-counted
/// at every position.
#[must_use]
pub fn word_boundaries(text: &str) -> Vec<usize> {
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    if chars.is_empty() {
        return Vec::new();
    }
    let segmenter = Segmenter::new(&chars);
    let mut out = Vec::with_capacity(chars.len() + 1);
    out.push(0);
    for (i, &(at, _)) in chars.iter().enumerate().skip(1) {
        if !segmenter.joined(i) {
            out.push(at);
        }
    }
    out.push(text.len());
    out
}

/// What the rules need to know about every position of one text.
struct Segmenter<'a> {
    chars: &'a [(usize, char)],
    props: Vec<WordBreak>,
    /// WB4: `X (Extend | Format | ZWJ)* → X`, for any X but sot, CR, LF and
    /// Newline. Whether character i is one of those extenders, folded into
    /// what precedes it.
    absorbed: Vec<bool>,
    /// The character that stands for position i in every rule after WB4:
    /// itself, unless absorbed, in which case the base it was folded into.
    base: Vec<usize>,
    /// The first unabsorbed character at or after each position, for the
    /// rules that look one character ahead past extenders (WB6, WB7b, WB12).
    next_base: Vec<usize>,
    /// How many regional indicators, counted in bases, end at each base: the
    /// parity WB15 and WB16 turn on.
    ri_run: Vec<usize>,
}

impl<'a> Segmenter<'a> {
    fn new(chars: &'a [(usize, char)]) -> Segmenter<'a> {
        let props: Vec<WordBreak> = chars.iter().map(|&(_, c)| word_break(c)).collect();
        let mut absorbed = Vec::with_capacity(props.len());
        let mut base: Vec<usize> = Vec::with_capacity(props.len());
        let mut previous: Option<WordBreak> = None;
        for (i, &p) in props.iter().enumerate() {
            let ignorable = matches!(p, WordBreak::Extend | WordBreak::Format | WordBreak::ZWJ);
            let after_break = previous
                .is_none_or(|q| matches!(q, WordBreak::CR | WordBreak::LF | WordBreak::Newline));
            let folded = ignorable && !after_break;
            absorbed.push(folded);
            let governing = if folded { base.last().copied() } else { None };
            base.push(governing.unwrap_or(i));
            previous = Some(p);
        }
        let mut next_base = vec![props.len(); props.len() + 1];
        let mut upcoming = props.len();
        for (i, &folded) in absorbed.iter().enumerate().rev() {
            if !folded {
                upcoming = i;
            }
            if let Some(slot) = next_base.get_mut(i) {
                *slot = upcoming;
            }
        }
        let mut ri_run = Vec::with_capacity(props.len());
        let mut run = 0usize;
        for (&p, &folded) in props.iter().zip(&absorbed) {
            if !folded {
                run = if p == WordBreak::RegionalIndicator {
                    run + 1
                } else {
                    0
                };
            }
            ri_run.push(run);
        }
        Segmenter {
            chars,
            props,
            absorbed,
            base,
            next_base,
            ri_run,
        }
    }

    fn prop(&self, i: usize) -> Option<WordBreak> {
        self.props.get(i).copied()
    }

    /// Whether there is **no** boundary before character `i`, for `0 < i <
    /// n`: the rules of §4.1.1 in their order, the first that applies
    /// deciding.
    fn joined(&self, i: usize) -> bool {
        let before = i.checked_sub(1).and_then(|p| self.prop(p));
        let here = self.prop(i);
        let newline = |p: Option<WordBreak>| {
            matches!(p, Some(WordBreak::CR | WordBreak::LF | WordBreak::Newline))
        };
        // WB3.
        if is(before, WordBreak::CR) && is(here, WordBreak::LF) {
            return true;
        }
        // WB3a, WB3b.
        if newline(before) || newline(here) {
            return false;
        }
        // WB3c.
        let pictograph = self
            .chars
            .get(i)
            .is_some_and(|&(_, c)| extended_pictographic(c));
        if is(before, WordBreak::ZWJ) && pictograph {
            return true;
        }
        // WB3d.
        if is(before, WordBreak::WSegSpace) && is(here, WordBreak::WSegSpace) {
            return true;
        }
        // WB4.
        if self.absorbed.get(i).copied().unwrap_or(false) {
            return true;
        }

        // Every later rule sees bases only.
        let left_at = i
            .checked_sub(1)
            .and_then(|p| self.base.get(p).copied())
            .unwrap_or(i);
        let l = self.prop(left_at);
        let ll = left_at
            .checked_sub(1)
            .and_then(|p| self.base.get(p).copied())
            .and_then(|p| self.prop(p));
        let r = here;
        let rr = self
            .next_base
            .get(i + 1)
            .copied()
            .and_then(|p| self.prop(p));
        let mid_letter = |p| is(p, WordBreak::MidLetter) || mid_num_let_q(p);
        let mid_num = |p| is(p, WordBreak::MidNum) || mid_num_let_q(p);
        let numeric = |p| is(p, WordBreak::Numeric);
        let katakana = |p| is(p, WordBreak::Katakana);
        let extend_num_let = |p| is(p, WordBreak::ExtendNumLet);
        let hebrew = |p| is(p, WordBreak::HebrewLetter);

        (ah_letter(l) && ah_letter(r)) // WB5
            || (ah_letter(l) && mid_letter(r) && ah_letter(rr)) // WB6
            || (ah_letter(ll) && mid_letter(l) && ah_letter(r)) // WB7
            || (hebrew(l) && is(r, WordBreak::SingleQuote)) // WB7a
            || (hebrew(l) && is(r, WordBreak::DoubleQuote) && hebrew(rr)) // WB7b
            || (hebrew(ll) && is(l, WordBreak::DoubleQuote) && hebrew(r)) // WB7c
            || (numeric(l) && numeric(r)) // WB8
            || (ah_letter(l) && numeric(r)) // WB9
            || (numeric(l) && ah_letter(r)) // WB10
            || (numeric(ll) && mid_num(l) && numeric(r)) // WB11
            || (numeric(l) && mid_num(r) && numeric(rr)) // WB12
            || (katakana(l) && katakana(r)) // WB13
            || ((ah_letter(l) || numeric(l) || katakana(l) || extend_num_let(l))
                && extend_num_let(r)) // WB13a
            || (extend_num_let(l) && (ah_letter(r) || numeric(r) || katakana(r))) // WB13b
            || (is(l, WordBreak::RegionalIndicator)
                && is(r, WordBreak::RegionalIndicator)
                && self.ri_run.get(left_at).is_some_and(|run| run % 2 == 1)) // WB15, WB16
    }
}

/// Whether a segment is a word rather than the space or punctuation between
/// words — see the module documentation.
fn is_word(segment: &str) -> bool {
    segment.chars().any(|c| {
        c.is_alphanumeric()
            || matches!(
                word_break(c),
                WordBreak::ALetter
                    | WordBreak::HebrewLetter
                    | WordBreak::Numeric
                    | WordBreak::Katakana
            )
    })
}

/// One word of a line, with the box that covers it.
#[derive(Clone, Debug, PartialEq)]
pub struct TextWord {
    /// The word's text: exactly the slice of [`TextLine::text`] between two
    /// UAX #29 boundaries.
    pub text: String,
    /// The union of the quads of the characters it covers, in the frame of
    /// the line's own baseline — so a word on a rotated line gets a rotated
    /// box that fits it, and a word on an upright line the plain enclosing
    /// rectangle.
    pub quad: Quad,
    /// The characters it covers, as indices into [`TextLine::chars`].
    ///
    /// A boundary inside one character's text — a ligature whose two halves
    /// UAX #29 separates, which is rare and possible — puts that character in
    /// both words, because its quad is the only box there is for either half.
    pub chars: core::ops::Range<usize>,
}

impl TextLine {
    /// The line's words, on UAX #29 default word boundaries, each with its
    /// box.
    ///
    /// Segments that hold no letter or digit — the spaces and punctuation
    /// between words — are not returned; [`word_boundaries`] is the
    /// unfiltered segmentation for a caller that wants them.
    #[must_use]
    pub fn words(&self) -> Vec<TextWord> {
        // Where each character's text starts and ends in the line's text,
        // which is their concatenation (`TextDevice::finish`).
        let mut spans = Vec::with_capacity(self.chars.len());
        let mut at = 0usize;
        for c in &self.chars {
            spans.push((at, at + c.text.len()));
            at += c.text.len();
        }

        let boundaries = word_boundaries(&self.text);
        let mut words = Vec::new();
        for pair in boundaries.windows(2) {
            let &[start, end] = pair else {
                continue;
            };
            let Some(text) = self.text.get(start..end) else {
                continue;
            };
            if !is_word(text) {
                continue;
            }
            let first = spans.iter().position(|&(s, e)| e > start && s < end);
            let last = spans.iter().rposition(|&(s, e)| e > start && s < end);
            let (Some(first), Some(last)) = (first, last) else {
                continue;
            };
            let Some(covered) = self.chars.get(first..=last) else {
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
}

/// The smallest quad enclosing `quads` whose sides run along and across the
/// first quad's baseline.
///
/// For upright text that is the axis-aligned enclosing rectangle. For a
/// rotated line it is the rotated rectangle a selection would draw; an
/// axis-aligned one would cover the text's neighbours as well as the text.
/// Non-finite quads are skipped, and `None` means nothing finite was left.
fn oriented_union(quads: impl Iterator<Item = Quad>) -> Option<Quad> {
    let quads: Vec<Quad> = quads.filter(Quad::is_finite).collect();
    let first = quads.first()?;
    let (dx, dy) = (first.lr.0 - first.ll.0, first.lr.1 - first.ll.1);
    let length = (dx * dx + dy * dy).sqrt();
    // A degenerate baseline has no direction to align to; the page's own axes
    // are then the only frame there is.
    let (ux, uy) = if length.is_finite() && length > 1e-9 {
        (dx / length, dy / length)
    } else {
        (1.0, 0.0)
    };
    // The across-baseline axis, turned a quarter to the left of the baseline,
    // which is "up" for text running left to right in a y-up space.
    let (vx, vy) = (-uy, ux);

    let (mut s0, mut s1, mut t0, mut t1) = (
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::INFINITY,
        f64::NEG_INFINITY,
    );
    for quad in &quads {
        for (x, y) in [quad.ll, quad.lr, quad.ul, quad.ur] {
            let s = x * ux + y * uy;
            let t = x * vx + y * vy;
            s0 = s0.min(s);
            s1 = s1.max(s);
            t0 = t0.min(t);
            t1 = t1.max(t);
        }
    }
    let point = |s: f64, t: f64| (s * ux + t * vx, s * uy + t * vy);
    let quad = Quad {
        ll: point(s0, t0),
        lr: point(s1, t0),
        ul: point(s0, t1),
        ur: point(s1, t1),
    };
    quad.is_finite().then_some(quad)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn segments(text: &str) -> Vec<&str> {
        word_boundaries(text)
            .windows(2)
            .filter_map(|w| text.get(w[0]..w[1]))
            .collect()
    }

    #[test]
    fn the_tables_were_compiled_from_the_file() {
        assert_eq!(word_break('a'), WordBreak::ALetter);
        assert_eq!(word_break('\u{05D0}'), WordBreak::HebrewLetter);
        assert_eq!(word_break('7'), WordBreak::Numeric);
        assert_eq!(word_break('\u{30A2}'), WordBreak::Katakana);
        assert_eq!(word_break('\u{0301}'), WordBreak::Extend);
        assert_eq!(word_break('\u{200D}'), WordBreak::ZWJ);
        assert_eq!(word_break(' '), WordBreak::WSegSpace);
        assert_eq!(word_break('\u{4E00}'), WordBreak::Other);
        assert!(extended_pictographic('\u{1F600}'));
        assert!(!extended_pictographic('a'));
    }

    #[test]
    fn words_and_what_separates_them() {
        assert_eq!(
            segments("can't stop 3.14, e.g. a_b"),
            vec!["can't", " ", "stop", " ", "3.14", ",", " ", "e.g", ".", " ", "a_b"]
        );
        assert!(word_boundaries("").is_empty());
    }

    use crate::interpret::{interpret, FontSource};
    use crate::state::Matrix;
    use crate::text::{TextChar, TextDevice, WritingMode};

    /// Every byte is one code, half an em wide, standing for itself.
    struct Simple;

    impl FontSource for Simple {
        fn decode(&self, _font: &[u8], bytes: &[u8]) -> Vec<(u32, String, f64)> {
            bytes
                .iter()
                .map(|&b| (u32::from(b), char::from(b).to_string(), 500.0))
                .collect()
        }
        fn vertical_metrics(&self, _font: &[u8], _code: u32) -> (f64, f64, f64) {
            (0.0, 880.0, -1000.0)
        }
    }

    fn close(a: (f64, f64), b: (f64, f64)) -> bool {
        (a.0 - b.0).abs() < 1e-9 && (a.1 - b.1).abs() < 1e-9
    }

    fn length(a: (f64, f64), b: (f64, f64)) -> f64 {
        ((b.0 - a.0).powi(2) + (b.1 - a.1).powi(2)).sqrt()
    }

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

    fn ch(text: &str, quad: Quad) -> TextChar {
        TextChar {
            text: text.to_string(),
            quad,
            size: 10.0,
            origin: quad.ll,
            mcid: None,
            stream: 0,
        }
    }

    /// A glyph box on a baseline through `origin` whose direction is the unit
    /// vector `(cos, sin)`, starting `along` units along it: `width` wide and
    /// ten tall, two of them below the baseline — the shape `TextDevice`
    /// gives a glyph at size ten.
    fn turned(origin: (f64, f64), (cos, sin): (f64, f64), along: f64, width: f64) -> Quad {
        let at = |s: f64, t: f64| (origin.0 + s * cos - t * sin, origin.1 + s * sin + t * cos);
        Quad {
            ll: at(along, -2.0),
            lr: at(along + width, -2.0),
            ul: at(along, 8.0),
            ur: at(along + width, 8.0),
        }
    }

    const UPRIGHT: (f64, f64) = (1.0, 0.0);

    /// Words from a real content stream, through the interpreter and the text
    /// device, with the boxes those glyphs were given.
    #[test]
    fn a_line_extracted_from_a_stream_has_its_words_boxed() {
        let mut device = TextDevice::new();
        interpret(
            b"BT /F0 10 Tf 100 700 Td (Hello, big world.) Tj ET",
            Matrix::IDENTITY,
            &mut device,
            &Simple,
        );
        let page = device.finish();
        let line = page.lines().first().copied().expect("a line");
        let words = line.words();
        let texts: Vec<&str> = words.iter().map(|w| w.text.as_str()).collect();
        assert_eq!(texts, vec!["Hello", "big", "world"]);

        // "Hello" is characters 0..5, each half an em (five units) wide, so
        // its box runs from the pen at 100 to 125, and from the glyph box's
        // bottom to its top.
        let hello = words.first().expect("a first word");
        assert_eq!(hello.chars, 0..5);
        let (x0, y0, x1, y1) = hello.quad.bounds();
        let first = line.chars.first().expect("a char").quad.bounds();
        assert!(
            (x0 - 100.0).abs() < 1e-9 && (x1 - 125.0).abs() < 1e-9,
            "{x0}..{x1}"
        );
        assert!((y0 - first.1).abs() < 1e-9 && (y1 - first.3).abs() < 1e-9);

        // "big" skips the comma and the space: characters 7..10.
        let big = words.get(1).expect("a second word");
        assert_eq!(big.chars, 7..10);
        let (bx0, _, bx1, _) = big.quad.bounds();
        assert!(
            (bx0 - 135.0).abs() < 1e-9 && (bx1 - 150.0).abs() < 1e-9,
            "{bx0}..{bx1}"
        );
    }

    /// The box is a union: a raised character inside a word lifts the top of
    /// the word's box, and nothing else moves.
    #[test]
    fn a_raised_character_widens_its_words_box() {
        let upright = |along: f64| turned((0.0, 0.0), UPRIGHT, along, 5.0);
        let mut raised = upright(5.0);
        for corner in [
            &mut raised.ll,
            &mut raised.lr,
            &mut raised.ul,
            &mut raised.ur,
        ] {
            corner.1 += 6.0;
        }
        let l = line(vec![
            ch("x", upright(0.0)),
            ch("2", raised),
            ch("y", upright(10.0)),
        ]);
        let words = l.words();
        assert_eq!(words.len(), 1, "x2y is one word (WB9, WB10)");
        let word = words.first().expect("a word");
        assert!(close(word.quad.ll, (0.0, -2.0)), "{:?}", word.quad);
        assert!(close(word.quad.ur, (15.0, 14.0)), "{:?}", word.quad);
    }

    /// On a turned line the box is turned with it: along the baseline it is as
    /// long as the word's advance, across it as tall as a glyph, and its
    /// corners are the first and last glyphs' — where an axis-aligned union
    /// would be a larger box covering the word's neighbours too.
    #[test]
    fn a_word_on_a_turned_line_gets_a_turned_box() {
        // Thirty degrees, spelled with the one irrational it needs.
        let direction = (3.0f64.sqrt() / 2.0, 0.5);
        let origin = (200.0, 300.0);
        let chars = "ab cd"
            .chars()
            .enumerate()
            .map(|(i, c)| {
                ch(
                    &c.to_string(),
                    turned(origin, direction, i as f64 * 5.0, 5.0),
                )
            })
            .collect();
        let l = line(chars);
        let words = l.words();
        let texts: Vec<&str> = words.iter().map(|w| w.text.as_str()).collect();
        assert_eq!(texts, vec!["ab", "cd"]);

        let ab = words.first().expect("a word").quad;
        let a = l.chars.first().expect("a").quad;
        let b = l.chars.get(1).expect("b").quad;
        assert!(close(ab.ll, a.ll) && close(ab.ul, a.ul), "{ab:?}");
        assert!(close(ab.lr, b.lr) && close(ab.ur, b.ur), "{ab:?}");
        assert!(
            (length(ab.ll, ab.lr) - 10.0).abs() < 1e-9,
            "along the baseline"
        );
        assert!((length(ab.ll, ab.ul) - 10.0).abs() < 1e-9, "across it");
    }

    /// A ligature is one character standing for several letters; its quad is
    /// the only box for any of them, and it belongs to the word it spells.
    #[test]
    fn a_ligature_is_inside_its_word() {
        let upright = |along: f64, width: f64| turned((0.0, 0.0), UPRIGHT, along, width);
        let l = line(vec![
            ch("o", upright(0.0, 5.0)),
            ch("ffi", upright(5.0, 12.0)),
            ch("ce", upright(17.0, 10.0)),
            ch(" ", upright(27.0, 3.0)),
            ch("x", upright(30.0, 5.0)),
        ]);
        let words = l.words();
        let office = words.first().expect("a word");
        assert_eq!(office.text, "office");
        assert_eq!(office.chars, 0..3);
        assert!(close(office.quad.lr, (27.0, -2.0)), "{:?}", office.quad);
        assert_eq!(words.get(1).map(|w| w.chars.clone()), Some(4..5));
    }

    #[test]
    fn a_combining_mark_stays_with_its_letter() {
        assert_eq!(segments("cafe\u{0301} ok"), vec!["cafe\u{0301}", " ", "ok"]);
    }

    #[test]
    fn regional_indicators_pair_into_flags() {
        let three = "\u{1F1E6}\u{1F1E7}\u{1F1E8}";
        assert_eq!(segments(three), vec!["\u{1F1E6}\u{1F1E7}", "\u{1F1E8}"]);
    }

    /// Ruling 1's posture for the one rule that looks back without limit: a
    /// line of nothing but regional indicators is linear, not quadratic.
    #[test]
    fn a_hostile_run_of_regional_indicators_is_linear() {
        let text: String = core::iter::repeat_n('\u{1F1E6}', 200_000).collect();
        let boundaries = word_boundaries(&text);
        assert_eq!(boundaries.len(), 100_001, "one flag per pair, plus sot");
    }
}
