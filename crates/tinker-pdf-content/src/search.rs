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
//!
//! # What a page keeps
//!
//! A page is built once and searched many times, so a search does not fold
//! the page's text again: for each of the four ways a search folds — none,
//! lower-cased, diacritics removed, both — the page keeps a **form**, every
//! line's text folded a glyph at a time and joined, with the byte offset
//! where each glyph's folded piece starts. A form is built at the first
//! search that asks for its fold, never at extraction, and kept until the
//! blocks change ([`TextPage::blocks_mut`] drops it); every later search in
//! that fold folds its needle alone (`docs/design/performance.md`, P-24).
//! [`TextPage::search_counted`] says how many forms a search built.

use std::sync::OnceLock;

use tinker_pdf_cos::{Counter, Stats, Work};

use crate::text::{span_quad, Quad, TextLine, TextPage};
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

/// One of the four ways a search folds text (`docs/design/performance.md`,
/// P-24, "What a form is, and where a `TextPage` keeps it").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Fold {
    /// Case and accents significant.
    Exact,
    /// Lower-cased, accents significant: [`TextPage::search`]'s, and the
    /// default options'.
    Lower,
    /// Case significant, diacritics removed.
    Unaccented,
    /// Lower-cased, then diacritics removed.
    LowerUnaccented,
}

impl Fold {
    /// The fold `options` search in.
    const fn of(options: &SearchOptions) -> Fold {
        match (options.case_sensitive, options.diacritic_insensitive) {
            (true, false) => Fold::Exact,
            (false, false) => Fold::Lower,
            (true, true) => Fold::Unaccented,
            (false, true) => Fold::LowerUnaccented,
        }
    }

    /// Where a page keeps its form in this fold.
    const fn index(self) -> usize {
        match self {
            Fold::Exact => 0,
            Fold::Lower => 1,
            Fold::Unaccented => 2,
            Fold::LowerUnaccented => 3,
        }
    }

    /// `text` folded: lower-cased by `str::to_lowercase` unless case is
    /// significant, then its diacritics removed where they are not.
    fn text(self, text: &str) -> String {
        let cased = match self {
            Fold::Exact | Fold::Unaccented => text.to_string(),
            Fold::Lower | Fold::LowerUnaccented => text.to_lowercase(),
        };
        match self {
            Fold::Unaccented | Fold::LowerUnaccented => fold_diacritics(&cased),
            Fold::Exact | Fold::Lower => cased,
        }
    }
}

/// A page's text in one fold: for each line, in the order
/// [`TextPage::lines`] gives them, its folded text and where each glyph's
/// piece of it starts.
#[derive(Clone, Debug)]
pub(crate) struct Form {
    lines: Vec<FormLine>,
}

/// One line of a [`Form`].
#[derive(Clone, Debug)]
struct FormLine {
    /// Every glyph's text, folded a glyph at a time and joined, so a match
    /// maps back to the glyphs it covers even where folding changed a
    /// character's length.
    text: String,
    /// The byte offset in `text` where each glyph's folded piece starts, and
    /// `text`'s length after the last: one more entry than the line has
    /// glyphs. A glyph whose piece folding empties — a combining mark searched
    /// without diacritics — starts where the next one does.
    offsets: Vec<u32>,
}

impl Form {
    /// `page`'s text in `fold`.
    fn folded(page: &TextPage, fold: Fold) -> Form {
        Form {
            lines: page
                .blocks()
                .iter()
                .flat_map(|block| &block.lines)
                .map(|line| FormLine::folded(line, fold))
                .collect(),
        }
    }
}

impl FormLine {
    /// `line`'s text in `fold`, a glyph at a time.
    fn folded(line: &TextLine, fold: Fold) -> FormLine {
        let mut text = String::new();
        let mut offsets = Vec::with_capacity(line.chars.len() + 1);
        for c in &line.chars {
            offsets.push(offset(text.len()));
            text.push_str(&fold.text(&c.text));
        }
        offsets.push(offset(text.len()));
        FormLine { text, offsets }
    }

    /// The glyphs the bytes `start..end` of the line's text cover: the first
    /// whose piece ends past `start` and the last whose piece starts before
    /// `end`.
    fn glyphs(&self, start: usize, end: usize) -> Option<(usize, usize)> {
        let mut first = None;
        let mut last = None;
        for (index, pair) in self.offsets.windows(2).enumerate() {
            let (at, next) = (pair[0] as usize, pair[1] as usize);
            if first.is_none() && next > start {
                first = Some(index);
            }
            if at < end {
                last = Some(index);
            }
        }
        Some((first?, last?))
    }
}

/// A byte offset as a [`FormLine`] holds it: a line's text is far shorter
/// than four gigabytes, and one past it is held at the end.
fn offset(at: usize) -> u32 {
    u32::try_from(at).unwrap_or(u32::MAX)
}

/// The forms a [`TextPage`] keeps, one a [`Fold`], each built at the first
/// search that asks for its fold.
///
/// `OnceLock`s, because a search takes `&TextPage` and a page is `Send` and
/// `Sync`: two threads searching one page in one fold at once build its form
/// once, the second waiting for the first. A clone keeps the forms built,
/// which describe its blocks as they describe the original's.
#[derive(Clone, Debug, Default)]
pub(crate) struct Forms([OnceLock<Form>; 4]);

impl Forms {
    /// Drops every form, for blocks about to change.
    pub(crate) fn clear(&mut self) {
        for form in &mut self.0 {
            form.take();
        }
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
        self.find(needle, options, None)
    }

    /// [`TextPage::search_with`], with a count of what the search did: its
    /// quads are `search_with`'s for the same arguments, to the bit, and its
    /// [`Stats`] counts `text_forms_built`, the forms this search folded the
    /// page into, which is one for the first search of a page in a fold and
    /// none for every search after it.
    ///
    /// A counted sibling, as `Page::text_counted` is, because `search` takes
    /// no options to carry a switch and the answer is a list
    /// (`docs/design/performance.md`, P-24, "The counted door").
    #[must_use]
    pub fn search_counted(&self, needle: &str, options: &SearchOptions) -> (Vec<Quad>, Stats) {
        let work = Work::new();
        let quads = self.find(needle, options, Some(&work));
        (quads, work.stats())
    }

    /// The page's text in `fold`, folded now if no search has read it in
    /// that fold, the build counted into `work` where there is one.
    fn form(&self, fold: Fold, work: Option<&Work>) -> &Form {
        // `index` is below four, the array's length, for every fold.
        self.forms.0[fold.index()].get_or_init(|| {
            if let Some(work) = work {
                work.add(Counter::TextFormsBuilt, 1);
            }
            Form::folded(self, fold)
        })
    }

    /// What [`TextPage::search`], [`TextPage::search_with`] and
    /// [`TextPage::search_counted`] share: `needle` found as `options` asks
    /// in the page's kept form, counting into `work` where there is one.
    pub(crate) fn find(
        &self,
        needle: &str,
        options: &SearchOptions,
        work: Option<&Work>,
    ) -> Vec<Quad> {
        if needle.is_empty() {
            return Vec::new();
        }
        let fold = Fold::of(options);
        let needle = fold.text(needle);
        if needle.is_empty() {
            return Vec::new();
        }
        let form = self.form(fold, work);
        let mut hits = Vec::new();

        let lines = self.blocks().iter().flat_map(|block| &block.lines);
        for (line, kept) in lines.zip(&form.lines) {
            let joined = kept.text.as_str();
            let boundaries = if options.whole_word {
                word_boundaries(joined)
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
                    if let Some((a, b)) = kept.glyphs(start, end) {
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

    /// The four option sets that each fold the text their own way, and one
    /// whole-word set, which folds as its case and accents say.
    const FOLDS: [SearchOptions; 5] = [
        SearchOptions {
            case_sensitive: false,
            whole_word: false,
            diacritic_insensitive: false,
        },
        CASE,
        ACCENTS,
        SearchOptions {
            case_sensitive: true,
            whole_word: false,
            diacritic_insensitive: true,
        },
        WORD,
    ];

    /// Every quad's corners as bits, so two answers compare to the bit.
    fn bits(quads: &[Quad]) -> Vec<[u64; 8]> {
        quads
            .iter()
            .map(|q| {
                [
                    q.ul.0, q.ul.1, q.ur.0, q.ur.1, q.ll.0, q.ll.1, q.lr.0, q.lr.1,
                ]
                .map(f64::to_bits)
            })
            .collect()
    }

    /// A page of accents, capitals and repeats for the folds to part on.
    fn folding_page() -> TextPage {
        page(&[
            "R\u{e9}sum\u{e9} resume RESUME",
            "o|ffi|ce caf|e|\u{301} M\u{fc}ller",
            "aaaa aa \u{130}stanbul",
        ])
    }

    /// **A page is folded once a fold, whatever the searches.** Ten searches
    /// in each fold build its form at the first and read it after, each
    /// returning, to the bit, what the same search of a page that has kept
    /// nothing returns; a whole-word search reads the form of its case and
    /// accents, and `search` the default's.
    #[test]
    fn a_page_is_folded_once_a_fold_whatever_the_searches() {
        let needles = [
            "resume",
            "R\u{e9}sum\u{e9}",
            "aa",
            "a",
            "ffi",
            "e\u{301}",
            "m\u{fc}ller",
            "office",
            "zzz",
            "\u{130}",
        ];
        let kept = folding_page();
        let mut built = 0u64;
        for options in FOLDS {
            for (round, needle) in needles.iter().enumerate() {
                let (quads, stats) = kept.search_counted(needle, &options);
                built += stats.text_forms_built;
                if round > 0 {
                    assert_eq!(stats.text_forms_built, 0, "{needle:?}, {options:?}");
                }
                assert_eq!(
                    bits(&quads),
                    bits(&folding_page().search_with(needle, &options)),
                    "{needle:?}, {options:?}: a kept form finds what a fresh page finds"
                );
                assert_eq!(bits(&quads), bits(&kept.search_with(needle, &options)));
            }
        }
        assert_eq!(built, 4, "one form a fold, the whole-word set's among them");
        let (_, stats) = kept.search_counted("aa", &SearchOptions::default());
        assert_eq!(stats.text_forms_built, 0);

        let fresh = folding_page();
        let _ = fresh.search("resume");
        let (_, stats) = fresh.search_counted("aa", &SearchOptions::default());
        assert_eq!(stats.text_forms_built, 0, "search keeps the default's form");
        let (_, stats) = fresh.search_counted("", &CASE);
        assert_eq!(stats.text_forms_built, 0, "an empty needle folds nothing");
        let (_, stats) = fresh.search_counted("\u{301}", &ACCENTS);
        assert_eq!(stats.text_forms_built, 0, "nor one folding empties");
        let copy = kept.clone();
        let (_, stats) = copy.search_counted("resume", &CASE);
        assert_eq!(stats.text_forms_built, 0, "a clone keeps the forms built");
        let (_, stats) = TextPage::default().search_counted("a", &CASE);
        assert_eq!(stats.text_forms_built, 1, "an empty page has a form too");
    }

    /// **A form does not outlive a change to its blocks.** A search after
    /// `blocks_mut` has reordered the lines, dropped a glyph or edited one
    /// finds what a page made of those blocks afresh finds, and folds again.
    #[test]
    fn a_form_does_not_outlive_a_change_to_its_blocks() {
        type Change = fn(&mut Vec<crate::text::TextBlock>);
        let changes: [(&str, Change); 4] = [
            ("lines reversed", |blocks| {
                for block in blocks.iter_mut() {
                    block.lines.reverse();
                }
                blocks.reverse();
            }),
            ("a glyph dropped", |blocks| {
                if let Some(line) = blocks.first_mut().and_then(|b| b.lines.first_mut()) {
                    line.chars.remove(1);
                }
            }),
            ("a glyph edited", |blocks| {
                if let Some(c) = blocks
                    .first_mut()
                    .and_then(|b| b.lines.first_mut())
                    .and_then(|l| l.chars.get_mut(0))
                {
                    c.text = "Z".to_string();
                }
            }),
            ("everything dropped", |blocks| blocks.clear()),
        ];
        for (what, change) in changes {
            for options in FOLDS {
                let mut kept = folding_page();
                for needle in ["resume", "Zsum", "aa", "ffi"] {
                    let _ = kept.search_with(needle, &options);
                }
                change(kept.blocks_mut());
                let afresh = TextPage::new(
                    kept.blocks().to_vec(),
                    kept.mcid_props.clone(),
                    kept.stated.clone(),
                    kept.warnings.clone(),
                );
                for (index, needle) in ["resume", "Zsum", "aa", "ffi", "R"].iter().enumerate() {
                    let (quads, stats) = kept.search_counted(needle, &options);
                    assert_eq!(
                        bits(&quads),
                        bits(&afresh.search_with(needle, &options)),
                        "{what}, {needle:?}, {options:?}"
                    );
                    assert_eq!(
                        stats.text_forms_built,
                        u64::from(index == 0),
                        "{what}, {needle:?}, {options:?}: folded again once"
                    );
                }
            }
        }
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
