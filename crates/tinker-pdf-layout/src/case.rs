//! `css-text-3` §2.1's `text-transform`, over Unicode's full case mappings.
//!
//! # What "full" buys, and why `char::to_uppercase` is not it
//!
//! §2.1 makes the mapping Unicode's *full* case mapping (§3.13), which is
//! longer than one character exactly where a naive one is wrong: `ß` uppercases
//! to `SS`, the `ﬁ` ligature to `FI`, `ŉ` to `ʼN`. The standard library has
//! the same algorithm, but over the table of whatever Unicode version the
//! compiler shipped, and this repository pins one version across three
//! vendored trees (`tinker-pdf-shape/tests/ucd_version.rs`) — a casing table
//! a version apart from the line breaker that measures its output is the skew
//! that test exists to stop. So the tables are `build.rs`'s, from this crate's
//! own `UnicodeData.txt`, `SpecialCasing.txt` and `DerivedCoreProperties.txt`.
//!
//! # The one context that is implemented, and the three that are not
//!
//! **Final_Sigma** is implemented: Σ lowercases to ς when a cased letter
//! precedes it (across any case-ignorable characters) and none follows, which
//! is §3.13 Table 3-17's definition. It is the one conditional mapping in
//! `SpecialCasing.txt` that names no language.
//!
//! The **language-conditional** mappings — Lithuanian's retained dot, and
//! Turkish and Azeri's dotted and dotless `i` — are not, because this crate is
//! never told a run's language: a box tree carries computed styles and the
//! language is the document's. §2.1 requires them *"if (and only if) the
//! content language of the element is ... known"*, so the cascade counts every
//! element in one of the three languages with a casing transform as
//! `text-transform` unimplemented, rather than letting a Turkish heading set
//! with an English `I` read as honoured.
//!
//! # What a word is
//!
//! §2.1 leaves *word* to the user agent. Here it is a maximal run of
//! characters that are not white space, and `capitalize` titlecases the first
//! *typographic letter unit* in it — the first letter or number, so the `(` of
//! `(hello` is passed over and the `1` of `1st` is the unit, which is not
//! lowercase and so is left alone. *"If lowercase"* is §2.1's own condition:
//! `ǆ` becomes `ǅ` and an already-capital `Ǆ` stays `Ǆ`, which is why
//! `capitalize` reads the titlecase mapping and is not `uppercase` on one
//! letter. A hyphen does not start a word here, so `well-known` is
//! `Well-known`; a user agent that split at the hyphen would set
//! `Well-Known`, and §2.1 permits either.

use tinker_pdf_css::property::TextTransform;

use crate::unicode;

/// What a transform needs to know about the text before the run in hand.
///
/// One per inline formatting context, carried run to run by
/// [`crate::text::Collapser`]. The two questions it answers are both about
/// characters already passed: whether a word is open, and whether a cased
/// letter precedes (across case-ignorable characters).
#[derive(Clone, Copy, Debug, Default)]
pub struct CaseContext {
    /// A word is open: the last character was not white space.
    in_word: bool,
    /// The open word has had its first letter unit.
    letter_seen: bool,
    /// Final_Sigma's *before* condition: the last character that was not
    /// case-ignorable was cased.
    after_cased: bool,
}

impl CaseContext {
    /// Moves the context past one character of the **source**, which is what
    /// both conditions are stated over.
    fn advance(&mut self, c: char) {
        if c.is_whitespace() {
            self.in_word = false;
            self.letter_seen = false;
        } else {
            self.in_word = true;
        }
        if unicode::is_cased(c) {
            self.after_cased = true;
        } else if !unicode::is_case_ignorable(c) {
            self.after_cased = false;
        }
    }
}

/// One run of already-collapsed text, transformed.
///
/// `none` returns the text unchanged and still advances the context, for the
/// reason [`crate::text::Collapser::push_transformed`] gives.
#[must_use]
pub fn transform(text: &str, how: TextTransform, context: &mut CaseContext) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    for (at, &c) in chars.iter().enumerate() {
        match how {
            TextTransform::None => out.push(c),
            TextTransform::Uppercase => push_mapped(&mut out, c, unicode::to_upper(c)),
            TextTransform::Lowercase => {
                if c == '\u{3a3}' && context.after_cased && !cased_follows(&chars[at + 1..]) {
                    // Unicode §3.13 Table 3-17, Final_Sigma: SpecialCasing.txt's
                    // one language-independent condition.
                    out.push('\u{3c2}');
                } else {
                    push_mapped(&mut out, c, unicode::to_lower(c));
                }
            }
            TextTransform::Capitalize => {
                let first_unit =
                    !c.is_whitespace() && !context.letter_seen && unicode::is_letter_or_number(c);
                if first_unit && unicode::is_lowercase(c) {
                    push_mapped(&mut out, c, unicode::to_title(c));
                } else {
                    out.push(c);
                }
            }
        }
        // The first-unit flag is set by the character whether or not it was
        // changed: a word whose first letter is already a capital has had its
        // first letter, and the next lowercase one in it is not a second.
        let starts = !context.letter_seen && unicode::is_letter_or_number(c);
        context.advance(c);
        if starts && context.in_word {
            context.letter_seen = true;
        }
    }
    out
}

/// Final_Sigma's *after* condition, negated: a cased letter follows across any
/// case-ignorable characters.
///
/// Only the rest of this run is visible, so a sigma that is the last letter of
/// a run whose word continues into the next element's run is taken as final.
/// That is a word split across two inline boxes mid-letter, which a book does
/// not write; recorded rather than carried, because carrying it would mean
/// transforming a run after the next one had been read.
fn cased_follows(rest: &[char]) -> bool {
    for &c in rest {
        if unicode::is_cased(c) {
            return true;
        }
        if !unicode::is_case_ignorable(c) {
            return false;
        }
    }
    false
}

fn push_mapped(out: &mut String, c: char, mapped: Option<&str>) {
    match mapped {
        Some(text) => out.push_str(text),
        None => out.push(c),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(text: &str, how: TextTransform) -> String {
        transform(text, how, &mut CaseContext::default())
    }

    /// **The mappings that are longer than the character**, which are the ones
    /// a one-to-one table gets wrong while passing every English test.
    #[test]
    fn uppercase_is_the_full_mapping_and_not_the_simple_one() {
        assert_eq!(run("straße", TextTransform::Uppercase), "STRASSE");
        assert_eq!(run("\u{fb01}ne", TextTransform::Uppercase), "FINE");
        assert_eq!(run("\u{149}", TextTransform::Uppercase), "\u{2bc}N");
        // U+0390, iota with dialytika and tonos: Greek has no precomposed
        // capital for it, so it is three characters.
        assert_eq!(
            run("\u{390}", TextTransform::Uppercase),
            "\u{399}\u{308}\u{301}"
        );
        assert_eq!(
            run("hello, world", TextTransform::Uppercase),
            "HELLO, WORLD"
        );
        // `İ` lowercases to `i` and a combining dot above: SpecialCasing.txt's
        // unconditional row, not the Turkish one.
        assert_eq!(run("\u{130}", TextTransform::Lowercase), "i\u{307}");
    }

    /// **Final_Sigma**: Σ is ς at the end of a word and σ inside one, and the
    /// condition is stated over cased and case-ignorable characters rather
    /// than over letters and spaces.
    #[test]
    fn a_capital_sigma_lowercases_to_the_final_form_only_at_the_end_of_a_word() {
        assert_eq!(run("ΟΔΟΣ", TextTransform::Lowercase), "οδος");
        assert_eq!(run("ΟΔΟΣ ΟΔΟΣ", TextTransform::Lowercase), "οδος οδος");
        assert_eq!(run("ΣΑΣ", TextTransform::Lowercase), "σας");
        // An apostrophe is case-ignorable, so a sigma before one with a letter
        // after is not final; a full stop after it is, and a sigma with no
        // cased letter before it is not final either.
        assert_eq!(run("ΟΣ'Α", TextTransform::Lowercase), "οσ'α");
        assert_eq!(run("ΟΣ.", TextTransform::Lowercase), "ος.");
        assert_eq!(run("Σ", TextTransform::Lowercase), "σ");
        // And the *before* half is carried from the previous run.
        let mut context = CaseContext::default();
        let first = transform("ΟΔΟ", TextTransform::Lowercase, &mut context);
        let second = transform("Σ", TextTransform::Lowercase, &mut context);
        assert_eq!(format!("{first}{second}"), "οδος");
    }

    /// **`capitalize` titlecases the first letter unit of each word, if it is
    /// lowercase**, and nothing else.
    #[test]
    fn capitalize_reads_the_titlecase_mapping_of_the_first_letter_unit() {
        assert_eq!(
            run("the sea, the sea", TextTransform::Capitalize),
            "The Sea, The Sea"
        );
        // Everything after the first unit is left as it is: `capitalize` is
        // not `lowercase` on the rest.
        assert_eq!(run("mcDONALD", TextTransform::Capitalize), "McDONALD");
        // The first *letter unit*, so the parenthesis is passed over and the
        // digit counts as the unit.
        assert_eq!(run("(hello) 1st", TextTransform::Capitalize), "(Hello) 1st");
        // `ǆ` is a digraph whose titlecase is not its uppercase.
        assert_eq!(run("\u{1c6}ak", TextTransform::Capitalize), "\u{1c5}ak");
        // `ß` titlecases to `Ss`, one of SpecialCasing.txt's rows.
        assert_eq!(run("ßa", TextTransform::Capitalize), "Ssa");
        assert_eq!(run("well-known", TextTransform::Capitalize), "Well-known");
    }

    /// **A word is the text's and not the element's.** The second half of a
    /// word set in its own element is still the middle of the word, and the
    /// first word of an element that follows a space is a new one.
    #[test]
    fn a_word_continues_across_runs_and_a_none_run_still_moves_the_context() {
        let mut context = CaseContext::default();
        let a = transform("he", TextTransform::None, &mut context);
        let b = transform("llo world", TextTransform::Capitalize, &mut context);
        assert_eq!(format!("{a}{b}"), "hello World");

        let mut context = CaseContext::default();
        let a = transform("one ", TextTransform::None, &mut context);
        let b = transform("two", TextTransform::Capitalize, &mut context);
        assert_eq!(format!("{a}{b}"), "one Two");
    }
}
