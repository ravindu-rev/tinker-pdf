//! Search with options: case-sensitive, whole word, diacritic-insensitive.
//!
//! [`TextPage::search`] is literal and case-insensitive with one argument,
//! and it stays exactly that — the parity suite pins it. This is the sibling
//! that takes a [`SearchOptions`], and with `SearchOptions::default()` it
//! finds what `search` finds, quad for quad.
//!
//! # What each option means
//!
//! - **Case.** Case-insensitive, the default, is `str::to_lowercase` on both
//!   sides, character by character — what `search` has always done and what
//!   callers have calibrated against. It is *lower-casing*, not Unicode case
//!   folding: `ß` does not match `SS`.
//! - **Whole word.** A match counts only when it begins and ends on a
//!   **UAX #29 word boundary** of the text searched ([`crate::words`]), which
//!   is the same segmentation [`crate::TextLine::words`] reports. So `cat` is
//!   not found in `concat` or in `scat`, and — because UAX #29 keeps an
//!   apostrophe between letters inside the word — not in `cat's` either. A
//!   needle of several words is held to boundaries at its two ends only.
//! - **Diacritic-insensitive.** Both sides are **canonically decomposed**
//!   (`UnicodeData.txt`'s untagged `Decomposition_Mapping`, fully expanded),
//!   and every nonspacing mark (`General_Category` `Mn`) that Unicode also
//!   classes `Diacritic` (`PropList.txt`) is removed. So `resume` finds
//!   `résumé` whether its accents are precomposed or combining, Arabic
//!   harakat and Hebrew points are ignored, and a Devanagari vowel sign —
//!   `Mn` but not `Diacritic`, because it is a vowel rather than an accent —
//!   still has to match. Compatibility mappings (`ﬁ`, `①`) are not applied,
//!   Hangul syllables are left composed, and canonical reordering is not
//!   performed; the module documentation of `build.rs` says why for each.
//!
//! Regular expressions are **not** an option: the tree has no regex engine
//! and links none, and whether it should grow one is a decision the roadmap
//! keeps open rather than one this module makes.

use crate::text::{span_quad, Quad, TextPage};
use crate::words::{member, word_boundaries};

/// How [`TextPage::search_with`] matches.
///
/// `Default` is [`TextPage::search`]'s behaviour: literal and
/// case-insensitive, anywhere in a line, accents significant.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SearchOptions {
    /// Match case exactly, rather than lower-casing both sides.
    pub case_sensitive: bool,
    /// Match only where the match begins and ends on a UAX #29 word boundary.
    pub whole_word: bool,
    /// Ignore diacritics: canonical decomposition, then every nonspacing
    /// `Diacritic` mark removed, on both sides.
    pub diacritic_insensitive: bool,
}

include!(concat!(env!("OUT_DIR"), "/fold.rs"));

/// `text` with its diacritics removed, the way
/// [`SearchOptions::diacritic_insensitive`] removes them: each character
/// canonically decomposed and every nonspacing `Diacritic` mark dropped.
#[must_use]
pub fn fold_diacritics(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        if member(DIACRITIC_MARKS, u32::from(c)) {
            continue;
        }
        if let Ok(at) = FOLD_ONE.binary_search_by_key(&c, |&(from, _)| from) {
            if let Some(&(_, to)) = FOLD_ONE.get(at) {
                out.push(to);
                continue;
            }
        }
        if let Ok(at) = FOLD_MANY.binary_search_by_key(&c, |&(from, _)| from) {
            if let Some(&(_, parts)) = FOLD_MANY.get(at) {
                out.extend(parts.iter());
                continue;
            }
        }
        out.push(c);
    }
    out
}

/// One side of a comparison, folded as `options` asks.
fn fold(text: &str, options: &SearchOptions) -> String {
    let cased = if options.case_sensitive {
        text.to_string()
    } else {
        text.to_lowercase()
    };
    if options.diacritic_insensitive {
        fold_diacritics(&cased)
    } else {
        cased
    }
}

impl TextPage {
    /// Finds `needle` as `options` asks, returning one quad per match.
    ///
    /// Matches within a line, like [`TextPage::search`], and maps each back to
    /// the glyphs it covers; with `SearchOptions::default()` the answer is
    /// `search`'s, quad for quad. What each option means — and what
    /// "whole word" and "diacritic" are defined as — is in
    /// [`crate::search`]'s documentation.
    ///
    /// An empty needle finds nothing, and so does one that folding empties
    /// (a needle of nothing but accents, searched without them).
    #[must_use]
    pub fn search_with(&self, needle: &str, options: &SearchOptions) -> Vec<Quad> {
        if needle.is_empty() {
            return Vec::new();
        }
        let needle = fold(needle, options);
        if needle.is_empty() {
            return Vec::new();
        }
        let mut hits = Vec::new();

        for line in self.lines() {
            // Folded per character, so a match maps back to the glyphs it
            // covers even where folding changed a character's length.
            let pieces: Vec<String> = line.chars.iter().map(|c| fold(&c.text, options)).collect();
            let joined: String = pieces.concat();
            let boundaries = if options.whole_word {
                word_boundaries(&joined)
            } else {
                Vec::new()
            };

            let mut from = 0usize;
            while let Some(found) = joined.get(from..).and_then(|s| s.find(&needle)) {
                let start = from + found;
                let end = start + needle.len();
                let whole = !options.whole_word
                    || (boundaries.binary_search(&start).is_ok()
                        && boundaries.binary_search(&end).is_ok());

                if whole {
                    // Byte offsets back to glyph indices.
                    let mut at = 0usize;
                    let mut first = None;
                    let mut last = None;
                    for (i, piece) in pieces.iter().enumerate() {
                        let next = at + piece.len();
                        if first.is_none() && next > start {
                            first = Some(i);
                        }
                        if at < end {
                            last = Some(i);
                        }
                        at = next;
                    }
                    if let (Some(a), Some(b)) = (first, last) {
                        if let Some(quad) = span_quad(line, a, b) {
                            hits.push(quad);
                        }
                    }
                    from = end;
                } else {
                    // A candidate that is not a whole word may overlap one
                    // that is — `x x` in `xx x x` — so the search resumes one
                    // character on rather than past the candidate.
                    from = start
                        + joined
                            .get(start..)
                            .and_then(|s| s.chars().next())
                            .map_or(1, char::len_utf8);
                }
                if from >= joined.len() {
                    break;
                }
            }
        }

        hits
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::device::{Device, Glyph};
    use crate::state::{GraphicsState, Matrix};
    use crate::text::TextDevice;

    /// A page of one line per string, each character one glyph half an em
    /// wide — or, where a string holds a `|`, the pieces between bars one
    /// glyph each, so a test can put a ligature or a lone combining mark in a
    /// glyph of its own.
    fn page(lines: &[&str]) -> TextPage {
        let mut device = TextDevice::new();
        let state = GraphicsState::new(Matrix::IDENTITY);
        for (row, line) in lines.iter().enumerate() {
            let pieces: Vec<String> = if line.contains('|') {
                line.split('|').map(str::to_string).collect()
            } else {
                line.chars().map(|c| c.to_string()).collect()
            };
            for (i, piece) in pieces.iter().enumerate() {
                device.show_glyph(
                    &Glyph {
                        code: 0,
                        text: piece.clone(),
                        transform: Matrix {
                            a: 10.0,
                            b: 0.0,
                            c: 0.0,
                            d: 10.0,
                            e: i as f64 * 5.0,
                            f: 700.0 - row as f64 * 20.0,
                        },
                        baseline: None,
                        advance: 5.0,
                        size: 10.0,
                        vertical: false,
                        font_id: 1,
                        font_name: None,
                    },
                    &state,
                );
            }
        }
        device.finish()
    }

    fn count(p: &TextPage, needle: &str, options: SearchOptions) -> usize {
        p.search_with(needle, &options).len()
    }

    const CASE: SearchOptions = SearchOptions {
        case_sensitive: true,
        whole_word: false,
        diacritic_insensitive: false,
    };
    const WORD: SearchOptions = SearchOptions {
        case_sensitive: false,
        whole_word: true,
        diacritic_insensitive: false,
    };
    const ACCENTS: SearchOptions = SearchOptions {
        case_sensitive: false,
        whole_word: false,
        diacritic_insensitive: true,
    };

    /// The default is `search`, quad for quad, over the shapes that could
    /// tell two implementations apart: overlapping repeats, a capital whose
    /// lower case is two characters, a multi-letter glyph, a lone combining
    /// mark, a needle straddling glyphs, and misses.
    #[test]
    fn the_default_is_search_quad_for_quad() {
        let p = page(&[
            "Tinker tinker TINKER",
            "aaaa aa",
            "\u{130}stanbul i\u{307}stanbul",
            "o|ffi|ce of|fi|ce",
            "cafe|\u{301}| caf\u{e9}",
        ]);
        for needle in [
            "tink",
            "Tinker",
            "aa",
            "a",
            "\u{130}",
            "i\u{307}",
            "ffi",
            "office",
            "ce o",
            "caf\u{e9}",
            "e\u{301}",
            "zzz",
            "",
            " ",
        ] {
            assert_eq!(
                p.search_with(needle, &SearchOptions::default()),
                p.search(needle),
                "{needle:?}"
            );
        }
    }

    #[test]
    fn case_sensitive_matches_case_exactly() {
        let p = page(&["Tinker tinker TINKER"]);
        assert_eq!(count(&p, "tink", SearchOptions::default()), 3);
        assert_eq!(count(&p, "Tink", CASE), 1);
        assert_eq!(count(&p, "tink", CASE), 1);
        assert_eq!(count(&p, "TINK", CASE), 1);
        assert_eq!(count(&p, "tINK", CASE), 0);
    }

    #[test]
    fn whole_word_is_uax29_boundaries_at_both_ends() {
        let p = page(&["cat concat cat's scat cat."]);
        assert_eq!(count(&p, "cat", SearchOptions::default()), 5);
        let hits = p.search_with("cat", &WORD);
        assert_eq!(hits.len(), 2, "the first cat and the last, not cat's");
        let (x0, ..) = hits.first().expect("a hit").bounds();
        assert!(x0.abs() < 1e-9, "the first hit is the first word: {x0}");

        // A needle of several words is held at its ends.
        let p = page(&["page 3 of 3", "apage 3 of 3s"]);
        assert_eq!(count(&p, "page 3 of 3", WORD), 1);

        // And a rejected candidate does not hide an overlapping whole word:
        // `x x` first matches from inside `xx`, and the whole-word match
        // begins one character later, inside that candidate.
        let p = page(&["xx x x"]);
        assert_eq!(count(&p, "x x", SearchOptions::default()), 1);
        assert_eq!(count(&p, "x x", WORD), 1);
    }

    #[test]
    fn diacritic_insensitive_folds_precomposed_and_combining_alike() {
        let p = page(&[
            "r\u{e9}sum\u{e9}",
            "re\u{301}sume\u{301}",
            "resume",
            "M\u{fc}ller",
        ]);
        assert_eq!(count(&p, "resume", SearchOptions::default()), 1);
        assert_eq!(count(&p, "resume", ACCENTS), 3);
        assert_eq!(
            count(&p, "r\u{e9}sum\u{e9}", ACCENTS),
            3,
            "folds the needle too"
        );
        assert_eq!(count(&p, "muller", ACCENTS), 1, "and composes with case");
        assert_eq!(
            count(
                &p,
                "muller",
                SearchOptions {
                    case_sensitive: true,
                    ..ACCENTS
                }
            ),
            0
        );
    }

    /// `Diacritic` and not bare `Mn`: Arabic harakat and Hebrew points are
    /// accents and go; a Devanagari vowel sign is a vowel and stays.
    #[test]
    fn a_vowel_sign_is_not_a_diacritic() {
        let p = page(&[
            "\u{643}\u{64e}\u{62a}\u{64e}\u{628}\u{64e}",
            "\u{5e9}\u{5c1}\u{5b8}\u{5dc}\u{5d5}\u{5b9}\u{5dd}",
            "\u{915}\u{941}\u{932}",
        ]);
        assert_eq!(count(&p, "\u{643}\u{62a}\u{628}", ACCENTS), 1, "harakat");
        assert_eq!(
            count(&p, "\u{5e9}\u{5dc}\u{5d5}\u{5dd}", ACCENTS),
            1,
            "niqqud"
        );
        assert_eq!(
            count(&p, "\u{915}\u{932}", ACCENTS),
            0,
            "a vowel sign stays"
        );
        assert_eq!(
            fold_diacritics("\u{958}"),
            "\u{915}",
            "nukta decomposed off"
        );
    }

    /// A hit on a combining-mark glyph covers the glyphs it matched, and a
    /// whole-word match survives the mark: folding empties the mark's piece,
    /// and the boundary is the folded text's.
    #[test]
    fn options_compose_and_hits_cover_their_glyphs() {
        let p = page(&["a |caf|e|\u{301}| bar"]);
        let both = SearchOptions {
            whole_word: true,
            ..ACCENTS
        };
        let hits = p.search_with("cafe", &both);
        assert_eq!(hits.len(), 1);
        let (x0, _, x1, _) = hits.first().expect("a hit").bounds();
        assert!((x0 - 5.0).abs() < 1e-9, "starts at the glyph caf: {x0}");
        assert!((x1 - 15.0).abs() < 1e-9, "ends at the e: {x1}");
        assert_eq!(count(&p, "caf", both), 0, "caf is not a whole word");
    }

    #[test]
    fn a_needle_folding_to_nothing_finds_nothing() {
        let p = page(&["e\u{301}"]);
        assert!(p.search_with("\u{301}", &ACCENTS).is_empty());
        assert!(p.search_with("", &WORD).is_empty());
    }

    #[test]
    fn the_folding_tables_were_compiled_from_the_file() {
        assert_eq!(fold_diacritics("\u{e9}"), "e");
        assert_eq!(fold_diacritics("\u{1d6}"), "u", "fully expanded: ǖ");
        assert_eq!(fold_diacritics("\u{2126}"), "\u{3a9}", "a singleton");
        assert_eq!(fold_diacritics("\u{ac00}"), "\u{ac00}", "Hangul composed");
        assert_eq!(fold_diacritics("\u{fb01}"), "\u{fb01}", "no compatibility");
        assert_eq!(
            fold_diacritics("x^2 \u{b4}"),
            "x^2 \u{b4}",
            "spacing marks stay"
        );
    }
}
