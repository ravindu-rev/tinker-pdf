//! UAX #14 and `css-text-3` §5, against fixtures a space-scanner fails.
//!
//! The conformance file is `tests/uax14_conformance.rs` and it is the
//! authority; these are about the **tailorings**, which the conformance file
//! says nothing about because they are CSS's and not Unicode's.

use tinker_pdf_css::property::{LineBreakStrictness, WordBreak};

use super::{opportunities, Opportunity, Tailoring};
use crate::unicode::{is_east_asian, line_break_class, Class};

fn breaks(text: &str, tailoring: Tailoring) -> Vec<usize> {
    opportunities(text, tailoring)
        .into_iter()
        .map(|o| o.at)
        .collect()
}

fn tailoring(strictness: LineBreakStrictness, word_break: WordBreak) -> Tailoring {
    Tailoring {
        strictness,
        word_break,
    }
}

/// Every combination `css-text-3` §5.1 and §5.2 offer, minus `anywhere` —
/// which is the one value that is *allowed* to break the required classes and
/// is therefore tested on its own.
fn every_tailoring_but_anywhere() -> Vec<Tailoring> {
    let mut out = Vec::new();
    for strictness in [
        LineBreakStrictness::Auto,
        LineBreakStrictness::Loose,
        LineBreakStrictness::Normal,
        LineBreakStrictness::Strict,
    ] {
        for word_break in [WordBreak::Normal, WordBreak::BreakAll, WordBreak::KeepAll] {
            out.push(tailoring(strictness, word_break));
        }
    }
    out
}

/// **The fixture the whole vendored UCD is for.**
///
/// There are no spaces in Japanese. A breaker that splits at U+0020 answers
/// *"one line"* here and passes every English test ever written, which is the
/// failure gap 31's risk table names by name and refuses to stage.
#[test]
fn a_cjk_run_breaks_between_every_ideograph() {
    // 東京都 — three ideographs, three bytes each, and a line may end after
    // any of them.
    let text = "\u{6771}\u{4eac}\u{90fd}";
    assert_eq!(breaks(text, Tailoring::default()), vec![3, 6, 9]);
    // The sentence a space-scanner would produce, written down so the
    // difference is a number rather than a claim.
    let spaces: Vec<usize> = text
        .char_indices()
        .filter(|(_, c)| *c == ' ')
        .map(|(at, _)| at + 1)
        .chain(std::iter::once(text.len()))
        .collect();
    assert_eq!(spaces, vec![9]);
}

/// Mixed Japanese: kana, kanji and the punctuation that must not start a line.
#[test]
fn japanese_punctuation_does_not_start_a_line() {
    // 「東京、京都」 — an opening bracket, two ideographs, an ideographic
    // comma, two more, a closing bracket.
    let text = "\u{300c}\u{6771}\u{4eac}\u{3001}\u{4eac}\u{90fd}\u{300d}";
    let at = breaks(text, Tailoring::default());
    // Never after the opening bracket (LB14 `OP SP* ×`), never before the
    // comma (LB13 `× CL`), never before the closing bracket, and never after
    // the comma's own position onto the closing quote.
    assert!(!at.contains(&3), "a line ended after an opening bracket");
    assert!(!at.contains(&9), "a line ended before an ideographic comma");
    assert!(!at.contains(&18), "a line ended before a closing bracket");
    // But between two ideographs, yes.
    assert!(at.contains(&6));
    assert!(at.contains(&15));
}

/// `css-text-3` §5.5: the behaviour of `WJ`, `ZW`, `GL` and `ZWJ` *"must be
/// honored"* — under **every** value of `line-break` and `word-break`.
///
/// Four classes and four separate assertions, because a build that honoured
/// three of them passes any test that only looks at one.
#[test]
fn the_four_required_classes_hold_under_every_tailoring() {
    for tailoring in every_tailoring_but_anywhere() {
        // WJ, U+2060 WORD JOINER: no break either side.
        let at = breaks("a\u{2060}b", tailoring);
        assert_eq!(
            at,
            vec![5],
            "a word joiner was broken around: {tailoring:?}"
        );

        // ZW, U+200B ZERO WIDTH SPACE: a break **after** it and not before.
        let at = breaks("ab\u{200b}cd", tailoring);
        assert!(at.contains(&5), "no break after a zero-width space");
        assert!(!at.contains(&2), "a break before a zero-width space");

        // GL, U+00A0 NO-BREAK SPACE: no break either side.
        let at = breaks("a\u{a0}b", tailoring);
        assert_eq!(at, vec![4], "a no-break space was broken at: {tailoring:?}");

        // ZWJ, U+200D: no break after it, which is what holds an emoji
        // sequence together.
        let at = breaks("\u{1f468}\u{200d}\u{1f469}", tailoring);
        assert_eq!(at, vec![11], "an emoji ZWJ sequence was broken");
    }
}

/// `line-break: anywhere` disregards even those four, which css-text-3 says in
/// as many words — and it is the only value that may.
#[test]
fn anywhere_disregards_even_the_required_classes() {
    let anywhere = tailoring(LineBreakStrictness::Anywhere, WordBreak::Normal);
    assert_eq!(breaks("a\u{2060}b", anywhere), vec![1, 4, 5]);
    assert_eq!(breaks("a\u{a0}b", anywhere), vec![1, 3, 4]);
    // The joiner is inside the first unit by then -- LB9 attached it -- so
    // the boundary `anywhere` opens is the one the joiner was protecting,
    // between the two pictographs. That is the difference this value is for,
    // and the required-class run above asserts the other answer for every
    // other value.
    assert_eq!(breaks("\u{1f468}\u{200d}\u{1f469}", anywhere), vec![7, 11]);
    // And around preserved white space, which the specification also names.
    assert_eq!(breaks("a b", anywhere), vec![1, 2, 3]);
    // A hard newline is still a hard newline: `anywhere` is about soft wrap
    // opportunities and does not make a mandatory break optional.
    let at = opportunities("a\nb", anywhere);
    assert!(at.iter().any(|o| o.at == 2 && o.mandatory));
}

/// §6.1's `CJ` resolution, which is the whole of what `strict` means: a small
/// kana may not start a line under `strict` and may under the others.
#[test]
fn strict_and_normal_disagree_about_a_small_kana() {
    // あぁ — HIRAGANA LETTER A, then the small one, which is `CJ`.
    let text = "\u{3042}\u{3041}";
    assert_eq!(
        breaks(
            text,
            tailoring(LineBreakStrictness::Strict, WordBreak::Normal)
        ),
        vec![6],
        "strict let a line start with a small kana"
    );
    for value in [
        LineBreakStrictness::Auto,
        LineBreakStrictness::Normal,
        LineBreakStrictness::Loose,
    ] {
        assert_eq!(
            breaks(text, tailoring(value, WordBreak::Normal)),
            vec![3, 6],
            "{value:?} did not allow a break before a small kana"
        );
    }
}

/// §5.1's `loose` adds its own list on top of that, and the iteration mark is
/// on it: 日々 may break under `loose` and may not under `normal`.
#[test]
fn loose_breaks_before_an_iteration_mark_and_normal_does_not() {
    // 日々 — an ideograph and U+3005, whose class is `NS`.
    let text = "\u{65e5}\u{3005}";
    assert_eq!(line_break_class('\u{3005}'), Class::NS);
    assert_eq!(
        breaks(
            text,
            tailoring(LineBreakStrictness::Normal, WordBreak::Normal)
        ),
        vec![6]
    );
    assert_eq!(
        breaks(
            text,
            tailoring(LineBreakStrictness::Loose, WordBreak::Normal)
        ),
        vec![3, 6]
    );
}

/// §5.2's `keep-all`: the implicit opportunities between letters go away, and
/// a run of ideographs becomes one word.
#[test]
fn keep_all_holds_a_cjk_run_together() {
    let text = "\u{6771}\u{4eac}\u{90fd}";
    assert_eq!(
        breaks(
            text,
            tailoring(LineBreakStrictness::Auto, WordBreak::KeepAll)
        ),
        vec![9]
    );
    // And an explicit opportunity still applies: a space is not an implicit
    // one, so `keep-all` does not weld two words together.
    assert_eq!(
        breaks(
            "ab cd",
            tailoring(LineBreakStrictness::Auto, WordBreak::KeepAll)
        ),
        vec![3, 5]
    );
}

/// §5.2's `break-all`: a Latin word may break between any two letters.
#[test]
fn break_all_breaks_inside_a_latin_word() {
    assert_eq!(
        breaks(
            "abc",
            tailoring(LineBreakStrictness::Auto, WordBreak::BreakAll)
        ),
        vec![1, 2, 3]
    );
    // And it still may not break an emoji sequence, because §5.5's required
    // classes stand above it — which is the difference between `break-all` and
    // `line-break: anywhere`.
    assert_eq!(
        breaks(
            "\u{1f468}\u{200d}\u{1f469}",
            tailoring(LineBreakStrictness::Auto, WordBreak::BreakAll)
        ),
        vec![11]
    );
}

/// LB4 and LB5: a newline is a **mandatory** break and says so, which is what
/// `white-space: pre` needs.
#[test]
fn a_newline_is_a_mandatory_break_and_a_space_is_not() {
    let at = opportunities("a\nb", Tailoring::default());
    assert_eq!(
        at,
        vec![
            Opportunity {
                at: 2,
                mandatory: true
            },
            Opportunity {
                at: 3,
                mandatory: true
            }
        ]
    );
    let at = opportunities("a b", Tailoring::default());
    assert!(!at[0].mandatory, "a space is a mandatory break");
}

/// LB9: a combining mark belongs to the character in front of it, so a line
/// never starts with an accent.
#[test]
fn a_combining_mark_stays_with_its_base() {
    // e + COMBINING ACUTE + a, with `break-all` so every other boundary is
    // open — which is what makes this about LB9 rather than about LB28.
    let at = breaks(
        "e\u{301}a",
        tailoring(LineBreakStrictness::Auto, WordBreak::BreakAll),
    );
    assert!(!at.contains(&1), "a line started with a combining mark");
    assert!(at.contains(&3));
}

/// LB30 excludes the East Asian forms, which is why `EastAsianWidth.txt` is
/// vendored: `a(` is one word and `a（` is two.
#[test]
fn east_asian_width_decides_whether_a_bracket_glues() {
    assert!(!is_east_asian('('));
    assert!(is_east_asian('\u{ff08}'));
    assert_eq!(breaks("a(b", Tailoring::default()), vec![3]);
    assert_eq!(
        breaks("a\u{ff08}b", Tailoring::default()),
        vec![1, 5],
        "a full-width bracket glued to the letter before it"
    );
}

/// The tables are the vendored ones and not a hand-written approximation.
#[test]
fn the_classes_come_from_the_vendored_ucd() {
    assert_eq!(line_break_class('a'), Class::AL);
    assert_eq!(line_break_class(' '), Class::SP);
    assert_eq!(line_break_class('\n'), Class::LF);
    assert_eq!(line_break_class('\u{6771}'), Class::ID);
    assert_eq!(line_break_class('\u{2060}'), Class::WJ);
    assert_eq!(line_break_class('\u{200b}'), Class::ZW);
    assert_eq!(line_break_class('\u{200d}'), Class::ZWJ);
    assert_eq!(line_break_class('\u{a0}'), Class::GL);
    assert_eq!(line_break_class('\u{3041}'), Class::CJ);
    // `HH`, new in Unicode 16.0 and the reason the generated table names enum
    // variants rather than indices: a build that mapped an unknown class onto
    // a default would read the whole family as `AL`.
    assert_eq!(line_break_class('\u{2010}'), Class::HH);
    assert_eq!(line_break_class('\u{05be}'), Class::HH);
}

/// An empty string has no opportunities at all, not even the end-of-text one.
#[test]
fn an_empty_string_has_no_opportunities() {
    assert!(opportunities("", Tailoring::default()).is_empty());
}
