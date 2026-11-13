//! Liang's hyphenation: a dictionary's patterns and exceptions in one trie,
//! and the points it gives a word (`docs/design/hyphenation.md`).
//!
//! # The algorithm, from its publication
//!
//! F. M. Liang, *Word Hy-phen-a-tion by Com-put-er*, Stanford University
//! report STAN-CS-83-977 (1983). A **pattern** is a string of letters with a
//! level, zero where none is written, before, between and after them, and
//! `.` for a word's beginning or end. A word is padded as `.word.`, every
//! pattern is matched at every position, and each gap between two letters
//! takes the **highest** level any matching pattern gives it: *"if the final
//! value is odd, the position is an allowable hyphen point"*. Odd levels
//! allow and even ones inhibit, each an exception to the level below it. An
//! **exception** is a word written whole with its points, and a word in the
//! list takes its points from there instead.
//!
//! A dictionary's **minimums** keep a point from a word's edges: after `i` of
//! `n` letters a point needs `i ≥ left` and `n − i ≥ right`, for an
//! exception's points as for a pattern's, since a minimum is a rule about
//! every word.
//!
//! # What a word is, here
//!
//! The caller hands [`Dictionary::points`] one word, and every character of it
//! must be one of the dictionary's letters **after folding**: as itself, as
//! its lowercase where the vendored UCD maps it to one character, or, for
//! U+2019, as U+0027 where the dictionary writes the apostrophe that way. A
//! word holding anything else — a soft hyphen, a letter of another alphabet, a
//! letter written decomposed where the dictionary has it composed — gets no
//! points rather than guessed ones, and so does a word longer than
//! [`MAX_HYPHENATED_WORD`]. No point is put before a combining mark, so none
//! parts a letter from its mark.
//!
//! # The table, and its walk
//!
//! `hyphenate/compile.rs` holds the format, the one `build.rs` writes and
//! [`Dictionary::parse`] builds at run time. Looking a word up walks the
//! exception path once and otherwise the patterns from each of `.word.`'s
//! positions, and no walk from one position follows more edges than the
//! longest pattern has symbols, so a word of `n` letters follows at most
//! `(n + 2) × longest + n + 1` edges — which `cfg(test)`'s step count holds.

use std::borrow::Cow;
use std::cmp::Ordering;

use crate::limits::MAX_HYPHENATED_WORD;
use crate::unicode;

mod compile;

pub use compile::PatternError;
use compile::{BOUNDARY, EXCEPTION, FIRST_LETTER, NODE_BYTES};

#[cfg(test)]
thread_local! {
    /// Trie edges [`Dictionary::points`] followed on this thread.
    pub(crate) static STEPS: core::cell::Cell<u64> = const { core::cell::Cell::new(0) };
    /// Words [`Dictionary::points`] was asked about on this thread.
    pub(crate) static LOOKUPS: core::cell::Cell<u64> = const { core::cell::Cell::new(0) };
}

/// One edge followed, for the `cfg(test)` count.
fn step() {
    #[cfg(test)]
    STEPS.with(|n| n.set(n.get() + 1));
}

/// One word looked up, for the `cfg(test)` count.
fn looked_up() {
    #[cfg(test)]
    LOOKUPS.with(|n| n.set(n.get() + 1));
}

/// One table this build carries: the vendored dictionary it was compiled
/// from, and the language tag it is found by.
#[derive(Debug)]
pub struct Table {
    /// The tag the dictionary's tree and file are named by: `en-gb` for
    /// `data/hyphenation-en-gb/hyph-en-gb.tex`.
    pub name: &'static str,
    /// The language tag a content language is looked up against: `en-GB`.
    pub language: &'static str,
    /// The compiled dictionary.
    pub dictionary: &'static Dictionary,
}

// The registry `build.rs` writes under the feature: one `static` for each
// vendored dictionary over its four arrays, and `TABLES`.
#[cfg(feature = "hyphenation")]
include!(concat!(env!("OUT_DIR"), "/hyphenation.rs"));

/// A build without the `hyphenation` feature carries no table.
#[cfg(not(feature = "hyphenation"))]
static TABLES: &[Table] = &[];

/// Every table this build carries, in the order `docs/design/hyphenation.md`
/// lists them: the eight vendored dictionaries under the `hyphenation`
/// feature, and none without it.
#[must_use]
pub fn tables() -> &'static [Table] {
    TABLES
}

/// The dictionary for a content language, or `None` where this build has no
/// table for it.
///
/// RFC 4647 §3.4's lookup: the tag is compared with each table's language,
/// ASCII-case-insensitively (RFC 5646 §2.1.1), and truncated from the end —
/// a single-letter subtag going with the subtag after it — until one matches
/// or nothing is left. `en-GB-oxendict` finds `en-GB`'s table and `pt-BR`
/// `pt`'s; `en` and `en-US` find none.
#[must_use]
pub fn dictionary(language: &str) -> Option<&'static Dictionary> {
    lookup(language, TABLES).map(|table| table.dictionary)
}

/// [`dictionary`]'s lookup, over any list of tables.
fn lookup<'t>(language: &str, tables: &'t [Table]) -> Option<&'t Table> {
    let mut range = language;
    while !range.is_empty() {
        if let Some(table) = tables
            .iter()
            .find(|table| table.language.eq_ignore_ascii_case(range))
        {
            return Some(table);
        }
        let cut = range.rfind('-')?;
        range = range.get(..cut).unwrap_or("");
        // RFC 4647 §3.4: a singleton is removed with the subtag after it, so
        // a range is never left ending in one.
        while let Some(cut) = range.rfind('-') {
            if range.len() - cut - 1 != 1 {
                break;
            }
            range = range.get(..cut).unwrap_or("");
        }
    }
    None
}

/// A compiled hyphenation dictionary: its patterns and exceptions as one trie
/// in `hyphenate/compile.rs`'s four arrays, and its minimums.
///
/// The arrays are borrowed where the build compiled them into the binary and
/// owned where [`Dictionary::parse`] built them, one type either way, so a
/// dictionary is looked up by one path whichever made it.
#[derive(Clone)]
pub struct Dictionary {
    alphabet: Cow<'static, [u8]>,
    nodes: Cow<'static, [u8]>,
    offsets: Cow<'static, [u8]>,
    values: Cow<'static, [u8]>,
    longest: u16,
    left: u8,
    right: u8,
}

impl std::fmt::Debug for Dictionary {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Dictionary")
            .field("letters", &self.letters())
            .field("nodes", &self.nodes())
            .field("vectors", &self.vectors())
            .field("longest", &self.longest)
            .field("minimums", &(self.left, self.right))
            .finish()
    }
}

impl Dictionary {
    /// Reads a pattern file and compiles it, with the reader `build.rs`
    /// compiles the vendored dictionaries with.
    ///
    /// The minimums are `minimums` where the caller gives them, and otherwise
    /// the header's `hyphenmins` block's `typesetting` entry.
    ///
    /// # Errors
    /// The [`PatternError`] that refused the file, or
    /// [`PatternError::NoMinimums`] where neither the caller nor the file
    /// states the minimums.
    pub fn parse(text: &str, minimums: Option<(u8, u8)>) -> Result<Dictionary, PatternError> {
        let source = compile::read(text)?;
        let minimums = minimums
            .or(source.minimums)
            .ok_or(PatternError::NoMinimums)?;
        let compiled = compile::compile(&source, minimums)?;
        Ok(Dictionary {
            alphabet: Cow::Owned(compiled.alphabet),
            nodes: Cow::Owned(compiled.nodes),
            offsets: Cow::Owned(compiled.offsets),
            values: Cow::Owned(compiled.values),
            longest: compiled.longest,
            left: compiled.left,
            right: compiled.right,
        })
    }

    /// A dictionary over arrays compiled into the binary: what the registry
    /// `build.rs` writes is made of.
    #[cfg(any(test, feature = "hyphenation"))]
    pub(crate) const fn from_static(
        (alphabet, nodes, offsets, values): (
            &'static [u8],
            &'static [u8],
            &'static [u8],
            &'static [u8],
        ),
        longest: u16,
        (left, right): (u8, u8),
    ) -> Dictionary {
        Dictionary {
            alphabet: Cow::Borrowed(alphabet),
            nodes: Cow::Borrowed(nodes),
            offsets: Cow::Borrowed(offsets),
            values: Cow::Borrowed(values),
            longest,
            left,
            right,
        }
    }

    /// The fewest letters a point may follow and the fewest it may precede.
    #[must_use]
    pub fn minimums(&self) -> (u8, u8) {
        (self.left, self.right)
    }

    /// The longest pattern, in symbols, a word's edge counted.
    #[must_use]
    pub fn longest(&self) -> usize {
        usize::from(self.longest)
    }

    /// How many letters the dictionary's patterns and exceptions use.
    #[must_use]
    pub fn letters(&self) -> usize {
        self.alphabet.len() / 4
    }

    /// The trie's nodes, the root included and the sentinel not.
    #[must_use]
    pub fn nodes(&self) -> usize {
        (self.nodes.len() / NODE_BYTES).saturating_sub(1)
    }

    /// The distinct vectors the trie's nodes hold.
    #[must_use]
    pub fn vectors(&self) -> usize {
        (self.offsets.len() / 4).saturating_sub(1)
    }

    /// What the table takes: its four arrays together, in bytes.
    #[must_use]
    pub fn bytes(&self) -> usize {
        self.alphabet.len() + self.nodes.len() + self.offsets.len() + self.values.len()
    }

    /// Whether `c` is one of the dictionary's letters, after folding.
    #[must_use]
    pub fn knows(&self, c: char) -> bool {
        self.symbol(c).is_some()
    }

    /// Appends to `out` the byte offset in `word` of each of its points, in
    /// order: where a line may break, a hyphen drawn before the break.
    ///
    /// Nothing is appended for a word with a character the dictionary does
    /// not know after folding, a word longer than [`MAX_HYPHENATED_WORD`],
    /// or a word too short for the minimums to leave a point in.
    pub fn points(&self, word: &str, out: &mut Vec<usize>) {
        looked_up();
        // `.word.`, as symbols, and where each letter starts in `word`.
        let mut padded = [BOUNDARY; MAX_HYPHENATED_WORD + 2];
        let mut starts = [0usize; MAX_HYPHENATED_WORD];
        let mut n = 0usize;
        for (at, c) in word.char_indices() {
            let Some(symbol) = self.symbol(c) else {
                return;
            };
            let (Some(slot), Some(start)) = (padded.get_mut(n + 1), starts.get_mut(n)) else {
                // Past the cap: no points.
                return;
            };
            *slot = symbol;
            *start = at;
            n += 1;
        }
        // A point needs a letter on each side whatever the minimums say.
        let left = usize::from(self.left.max(1));
        let right = usize::from(self.right.max(1));
        if n < left + right {
            return;
        }
        let Some(padded) = padded.get(..n + 2) else {
            return;
        };
        let mut levels = [0u8; MAX_HYPHENATED_WORD + 3];
        match padded
            .get(1..=n)
            .and_then(|letters| self.exception(letters))
        {
            // An exception's levels are its word's gaps; a gap of the word is
            // the gap of `.word.` one along.
            Some(vector) => {
                for (gap, &level) in vector.iter().enumerate() {
                    if let Some(slot) = levels.get_mut(gap + 1) {
                        *slot = level;
                    }
                }
            }
            None => self.walk(padded, &mut levels),
        }
        for (letters_before, &start) in starts.iter().enumerate().take(n + 1 - right).skip(left) {
            let odd = levels
                .get(letters_before + 1)
                .is_some_and(|level| level % 2 == 1);
            let before_a_mark = word
                .get(start..)
                .and_then(|rest| rest.chars().next())
                .is_some_and(unicode::is_combining);
            if odd && !before_a_mark {
                out.push(start);
            }
        }
    }

    /// Every pattern at every position of `padded`, each gap raised to the
    /// highest level a matching pattern gives it.
    fn walk(&self, padded: &[u8], levels: &mut [u8]) {
        for start in 0..padded.len() {
            let mut node = 0usize;
            for &symbol in padded.iter().skip(start) {
                let Some(child) = self.child(node, symbol) else {
                    break;
                };
                step();
                node = child;
                if let Some(vector) = self.vector(node) {
                    for (offset, &level) in vector.iter().enumerate() {
                        if let Some(slot) = levels.get_mut(start + offset) {
                            *slot = (*slot).max(level);
                        }
                    }
                }
            }
        }
    }

    /// The exception's levels for a word of these letters, if it is one.
    fn exception(&self, letters: &[u8]) -> Option<&[u8]> {
        let mut node = self.child(0, EXCEPTION)?;
        step();
        for &symbol in letters {
            node = self.child(node, symbol)?;
            step();
        }
        self.vector(node)
    }

    /// A letter's symbol, after folding.
    fn symbol(&self, c: char) -> Option<u8> {
        if let Some(symbol) = self.letter(c) {
            return Some(symbol);
        }
        if let Some(lower) = unicode::to_lower(c) {
            let mut chars = lower.chars();
            if let (Some(one), None) = (chars.next(), chars.next()) {
                if let Some(symbol) = self.letter(one) {
                    return Some(symbol);
                }
            }
        }
        if c == '\u{2019}' {
            return self.letter('\'');
        }
        None
    }

    /// The symbol of `c` as the alphabet writes it, by a binary search.
    fn letter(&self, c: char) -> Option<u8> {
        let wanted = u32::from(c);
        let (mut lo, mut hi) = (0usize, self.letters());
        while lo < hi {
            let mid = lo + (hi - lo) / 2;
            let at = mid.checked_mul(4)?;
            let bytes: [u8; 4] = self.alphabet.get(at..at + 4)?.try_into().ok()?;
            match u32::from_le_bytes(bytes).cmp(&wanted) {
                Ordering::Less => lo = mid + 1,
                Ordering::Greater => hi = mid,
                Ordering::Equal => return u8::try_from(mid).ok()?.checked_add(FIRST_LETTER),
            }
        }
        None
    }

    /// One node's record: its symbol, its first child, its vector.
    fn record(&self, index: usize) -> Option<(u8, usize, u16)> {
        let start = index.checked_mul(NODE_BYTES)?;
        let bytes: [u8; NODE_BYTES] = self
            .nodes
            .get(start..start.checked_add(NODE_BYTES)?)?
            .try_into()
            .ok()?;
        let [label, a, b, c, d, low, high] = bytes;
        let first = usize::try_from(u32::from_le_bytes([a, b, c, d])).ok()?;
        Some((label, first, u16::from_le_bytes([low, high])))
    }

    /// A node's child along `symbol`: a binary search of its children, which
    /// run from its first child to the next node's.
    fn child(&self, node: usize, symbol: u8) -> Option<usize> {
        let (_, first, _) = self.record(node)?;
        let (_, end, _) = self.record(node.checked_add(1)?)?;
        let (mut lo, mut hi) = (first, end);
        while lo < hi {
            let mid = lo + (hi - lo) / 2;
            let (label, _, _) = self.record(mid)?;
            match label.cmp(&symbol) {
                Ordering::Less => lo = mid + 1,
                Ordering::Greater => hi = mid,
                Ordering::Equal => return Some(mid),
            }
        }
        None
    }

    /// The levels a node's path ends with, if it ends a pattern or a word.
    fn vector(&self, node: usize) -> Option<&[u8]> {
        let (_, _, vector) = self.record(node)?;
        let index = usize::from(vector).checked_sub(1)?;
        let start = self.offset(index)?;
        let end = self.offset(index.checked_add(1)?)?;
        self.values.get(start..end)
    }

    /// A vector's start in `values`.
    fn offset(&self, index: usize) -> Option<usize> {
        let at = index.checked_mul(4)?;
        let bytes: [u8; 4] = self.offsets.get(at..at.checked_add(4)?)?.try_into().ok()?;
        usize::try_from(u32::from_le_bytes(bytes)).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    /// A dictionary from patterns and exceptions written in the test, at the
    /// minimums given.
    fn dictionary(patterns: &str, exceptions: &str, minimums: (u8, u8)) -> Dictionary {
        let text = format!("\\patterns{{{patterns}}}\n\\hyphenation{{{exceptions}}}\n");
        Dictionary::parse(&text, Some(minimums)).expect("the test's patterns read")
    }

    /// `word` with a hyphen at each point.
    fn hyphenated(dictionary: &Dictionary, word: &str) -> String {
        let mut points = Vec::new();
        dictionary.points(word, &mut points);
        let mut out = String::new();
        let mut from = 0;
        for point in points {
            out.push_str(&word[from..point]);
            out.push('-');
            from = point;
        }
        out.push_str(&word[from..]);
        out
    }

    /// The level of each gap between two letters of `word`, the first gap
    /// first, as the patterns alone leave it — the arithmetic the test works.
    fn levels(dictionary: &Dictionary, word: &str) -> Vec<u8> {
        let mut padded = vec![BOUNDARY];
        padded.extend(
            word.chars()
                .map(|c| dictionary.symbol(c).expect("a letter")),
        );
        padded.push(BOUNDARY);
        let mut levels = vec![0u8; padded.len() + 1];
        dictionary.walk(&padded, &mut levels);
        // Gap `g` of `.word.` is before its `g`th symbol, so the gap between
        // the word's letters `i - 1` and `i` is `i + 1`.
        (1..word.chars().count()).map(|i| levels[i + 1]).collect()
    }

    /// **Each gap takes the highest level every matching pattern gives it,
    /// and its odd levels are the word's points** — Liang's own three
    /// examples, worked here.
    ///
    /// `hyphenation` under `hy3ph he2n hena4 hen5at 1na n2at 1tio 2io`:
    /// `hy3ph` puts 3 between `y` and `p`; `he2n` 2 and `1na` 1 between `e`
    /// and `n`, so 2; `hen5at` 5 and `n2at` 2 between `n` and `a`, so 5;
    /// `hena4` 4 and `1tio` 1 between `a` and `t`, so 4; `2io` 2 between `t`
    /// and `i`. That is `hy3phe2n5a4t2ion`, the publication's, and its odd
    /// levels make `hy-phen-ation`.
    ///
    /// `computer` under `4m1p pu2t 5pute put3er`: `4m1p` gives 4 between `o`
    /// and `m` and 1 between `m` and `p`, which `5pute` raises to 5; `pu2t`
    /// 2 between `u` and `t`; `put3er` 3 between `t` and `e` —
    /// `co4m5pu2t3er`, `com-put-er`.
    ///
    /// `algorithm` under `l1g4 lgo3 1go 2ith 4hm`: `l1g4` and `1go` give 1
    /// between `l` and `g`, `l1g4` 4 between `g` and `o`, `lgo3` 3 between
    /// `o` and `r`, `2ith` 2 between `r` and `i`, `4hm` 4 between `t` and
    /// `h` — `al1g4o3r2it4hm`, `al-go-rithm`.
    #[test]
    fn points_are_the_odd_maxima_of_every_matching_pattern() {
        let liang = dictionary("hy3ph he2n hena4 hen5at 1na n2at 1tio 2io", "", (1, 1));
        assert_eq!(
            levels(&liang, "hyphenation"),
            vec![0, 3, 0, 0, 2, 5, 4, 2, 0, 0]
        );
        assert_eq!(hyphenated(&liang, "hyphenation"), "hy-phen-ation");

        let computer = dictionary("4m1p pu2t 5pute put3er", "", (1, 1));
        assert_eq!(levels(&computer, "computer"), vec![0, 4, 5, 0, 2, 3, 0]);
        assert_eq!(hyphenated(&computer, "computer"), "com-put-er");

        let algorithm = dictionary("l1g4 lgo3 1go 2ith 4hm", "", (1, 1));
        assert_eq!(
            levels(&algorithm, "algorithm"),
            vec![0, 1, 4, 3, 2, 0, 4, 0]
        );
        assert_eq!(hyphenated(&algorithm, "algorithm"), "al-go-rithm");

        // A word's edges, as `.` anchors them: `.ab1` only where the word
        // starts `ab`, `1cd.` only where it ends `cd`, and an even level
        // inhibiting the odd one a shorter pattern gives the same gap —
        // `b1c` would break `ab-cd` and `ab2c` forbids it. The level-free
        // `x` makes `x` a letter and changes no gap.
        let edges = dictionary(".ab1 1cd. b1c ab2c x", "", (1, 1));
        assert_eq!(levels(&edges, "abcd"), vec![0, 2, 0]);
        assert_eq!(hyphenated(&edges, "abcd"), "abcd");
        assert_eq!(levels(&edges, "xabx"), vec![0, 0, 0], "not at the start");
        assert_eq!(hyphenated(&edges, "abxcd"), "ab-x-cd");
    }

    /// **An exception replaces the patterns for its word**, and for that word
    /// alone: a longer word that starts with it is the patterns', a capital
    /// one is the exception's once folded, and an exception written with no
    /// hyphen takes every point away.
    #[test]
    fn an_exception_replaces_the_patterns_for_its_word() {
        let listed = dictionary("1ba", "abb-a abab", (1, 1));
        assert_eq!(
            hyphenated(&listed, "abba"),
            "abb-a",
            "the exception's point"
        );
        assert_eq!(hyphenated(&listed, "abbab"), "ab-bab", "the patterns'");
        assert_eq!(
            hyphenated(&listed, "ABBA"),
            "ABB-A",
            "folded to the exception"
        );
        assert_eq!(hyphenated(&listed, "abab"), "abab", "listed with no point");
        let unlisted = dictionary("1ba", "", (1, 1));
        assert_eq!(hyphenated(&unlisted, "abba"), "ab-ba");
        assert_eq!(hyphenated(&unlisted, "abab"), "a-bab");
    }

    /// **No point falls inside the minimums**, a pattern's or an
    /// exception's: after `i` of `n` letters a point needs `i ≥ left` and
    /// `n − i ≥ right`, and a minimum of zero is taken as one, since a point
    /// needs a letter on each side.
    #[test]
    fn no_point_falls_inside_the_minimums() {
        let everywhere = |minimums| dictionary("1a1", "aa-aa-aa", minimums);
        let points = |dictionary: &Dictionary, word: &str| {
            let mut out = Vec::new();
            dictionary.points(word, &mut out);
            out
        };
        assert_eq!(points(&everywhere((1, 1)), "aaaaa"), vec![1, 2, 3, 4]);
        assert_eq!(points(&everywhere((2, 3)), "aaaaa"), vec![2]);
        assert_eq!(points(&everywhere((0, 0)), "aaaaa"), vec![1, 2, 3, 4]);
        assert_eq!(points(&everywhere((3, 3)), "aaaaa"), Vec::<usize>::new());
        // The exception's points, two and four, under each.
        assert_eq!(points(&everywhere((1, 1)), "aaaaaa"), vec![2, 4]);
        assert_eq!(points(&everywhere((2, 2)), "aaaaaa"), vec![2, 4]);
        assert_eq!(points(&everywhere((3, 2)), "aaaaaa"), vec![4]);
        assert_eq!(points(&everywhere((3, 3)), "aaaaaa"), Vec::<usize>::new());
        // A byte offset, not a letter count: each `é` is two bytes.
        let accented = dictionary("1é1", "", (2, 2));
        assert_eq!(points(&accented, "éééé"), vec![4]);
    }

    /// **A word past the cap has no points**, and one at the cap has all of
    /// them.
    #[test]
    fn a_word_past_the_cap_has_no_points() {
        let everywhere = dictionary("1a1", "", (1, 1));
        let mut points = Vec::new();
        everywhere.points(&"a".repeat(MAX_HYPHENATED_WORD), &mut points);
        assert_eq!(
            points.len(),
            MAX_HYPHENATED_WORD - 1,
            "a point in every gap"
        );
        points.clear();
        everywhere.points(&"a".repeat(MAX_HYPHENATED_WORD + 1), &mut points);
        assert!(points.is_empty(), "past the cap");
    }

    /// **A word with a character the dictionary does not know gets no
    /// points**, and folding is what it knows: a capital as its lowercase,
    /// U+2019 as the U+0027 the dictionary writes, and nothing else — not a
    /// soft hyphen, not a letter of another alphabet, not a letter written
    /// decomposed where the dictionary has it composed.
    #[test]
    fn a_word_is_the_dictionarys_letters_after_folding() {
        let apostrophe = dictionary("1ba n1'", "", (1, 1));
        assert_eq!(hyphenated(&apostrophe, "abab"), "a-bab");
        assert_eq!(hyphenated(&apostrophe, "ABAB"), "A-BAB");
        assert_eq!(hyphenated(&apostrophe, "abn\u{2019}a"), "abn-\u{2019}a");
        assert!(apostrophe.knows('\u{2019}') && apostrophe.knows('B'));
        assert_eq!(hyphenated(&apostrophe, "ab\u{AD}ab"), "ab\u{AD}ab");
        assert_eq!(hyphenated(&apostrophe, "abxab"), "abxab");
        assert_eq!(hyphenated(&apostrophe, "abαab"), "abαab");
        let composed = dictionary("1é1", "", (1, 1));
        assert_eq!(hyphenated(&composed, "éé"), "é-é");
        assert_eq!(
            hyphenated(&composed, "e\u{301}e\u{301}"),
            "e\u{301}e\u{301}"
        );
        assert!(!composed.knows('e'));
    }

    /// **No point parts a letter from the combining mark after it**, even
    /// where the dictionary's levels put one there.
    #[test]
    fn no_point_falls_before_a_combining_mark() {
        let marks = dictionary("a1\u{301} \u{301}1b", "", (1, 1));
        assert_eq!(hyphenated(&marks, "a\u{301}b"), "a\u{301}-b");
    }

    /// **The pattern file is read as the collection writes it**: comments,
    /// several blocks of each kind, and the minimums from the header's
    /// `typesetting` entry and not its `generation` one, a comment after a
    /// value allowed.
    #[test]
    fn a_pattern_file_is_read_with_its_header() {
        let text = "% title: a test\n\
                    % hyphenmins:\n\
                    %     generation:\n\
                    %         left: 1\n\
                    %         right: 1\n\
                    %     typesetting:\n\
                    %         left: 2 # a remark\n\
                    %         right: 3\n\
                    %\n\
                    % texlive:\n\
                    %     encoding: ascii\n\
                    \\patterns{ % the first\n\
                    1ba\n\
                    }\n\
                    \\patterns{.a2b}\n\
                    \\hyphenation{abb-a}\n";
        let source = compile::read(text).expect("the file reads");
        assert_eq!(source.minimums, Some((2, 3)));
        assert_eq!(source.patterns.len(), 2);
        assert_eq!(source.patterns[1].chars, vec!['.', 'a', 'b']);
        assert_eq!(source.patterns[1].levels, vec![0, 0, 2, 0]);
        assert_eq!(source.exceptions.len(), 1);
        assert_eq!(source.exceptions[0].points, vec![3]);
        let read = Dictionary::parse(text, None).expect("the file compiles");
        assert_eq!(read.minimums(), (2, 3));
        let given = Dictionary::parse(text, Some((1, 1))).expect("the file compiles");
        assert_eq!(given.minimums(), (1, 1), "the caller's minimums win");
    }

    /// **A file this reader does not understand is refused, by what is wrong
    /// with it and where** — never compiled to a table that lost part of it.
    #[test]
    fn a_malformed_pattern_file_is_refused_by_name() {
        let refused = |text: &str| compile::read(text).expect_err(text);
        assert_eq!(refused("% nothing"), PatternError::NoPatterns);
        assert_eq!(refused("\\hyphenation{ab-c}"), PatternError::NoPatterns);
        assert_eq!(
            refused("\\patterns{\n1ab"),
            PatternError::Unterminated { line: 1 }
        );
        assert_eq!(
            refused("\\patterns{1ab}\n\\relax"),
            PatternError::Stray { line: 2 }
        );
        assert_eq!(refused("words"), PatternError::Stray { line: 1 });
        assert_eq!(refused("\\patterns 1ab}"), PatternError::Stray { line: 1 });
        assert_eq!(refused("\\patterns{a{b}"), PatternError::Stray { line: 1 });
        assert_eq!(
            refused("\\patterns{a12b}"),
            PatternError::TwoDigits { line: 1 }
        );
        assert_eq!(
            refused("\\patterns{a.b}"),
            PatternError::InnerBoundary { line: 1 }
        );
        assert_eq!(
            refused("\\patterns{.1.}"),
            PatternError::NoLetter { line: 1 }
        );
        assert_eq!(refused("\\patterns{1}"), PatternError::NoLetter { line: 1 });
        assert_eq!(
            refused("\\patterns{a1b\nab2}"),
            PatternError::RepeatedPattern { line: 2 }
        );
        for exception in ["-ab", "ab-", "a--b", "a1b", "a.b", "-"] {
            assert_eq!(
                refused(&format!("\\patterns{{}}\\hyphenation{{{exception}}}")),
                PatternError::MalformedException { line: 1 },
                "{exception}"
            );
        }
        assert_eq!(
            refused("\\patterns{}\\hyphenation{ab-c a-bc}"),
            PatternError::RepeatedException { line: 1 }
        );
        assert_eq!(
            refused("% hyphenmins:\n%  typesetting:\n%    left: two\n\\patterns{}"),
            PatternError::Minimums { line: 3 }
        );
        assert_eq!(
            refused("% hyphenmins:\n%  typesetting:\n%    left: 2\n\\patterns{}"),
            PatternError::Minimums { line: 3 }
        );
        assert_eq!(
            Dictionary::parse("\\patterns{1ba}", None).expect_err("no minimums"),
            PatternError::NoMinimums
        );
        let many: String = (0..=compile::MAX_LETTERS)
            .filter_map(|at| char::from_u32(0x4e00 + at as u32))
            .map(|c| format!("1{c} "))
            .collect();
        assert_eq!(
            Dictionary::parse(&format!("\\patterns{{{many}}}"), Some((1, 1)))
                .expect_err("too many letters"),
            PatternError::TooManyLetters {
                letters: compile::MAX_LETTERS + 1
            }
        );
    }

    /// **A dictionary compiled into the binary answers as one read at run
    /// time**: the registry `build.rs` writes is the four arrays
    /// [`compile::compile`] makes, borrowed, and this is that shape.
    #[test]
    fn a_static_dictionary_is_the_parsed_one_borrowed() {
        let text = "\\patterns{hy3ph he2n hena4 hen5at 1na n2at 1tio 2io}\\hyphenation{ta-ble}";
        let source = compile::read(text).expect("reads");
        let compiled = compile::compile(&source, (2, 3)).expect("compiles");
        let leak = |bytes: Vec<u8>| -> &'static [u8] { Box::leak(bytes.into_boxed_slice()) };
        let fixed = Dictionary::from_static(
            (
                leak(compiled.alphabet.clone()),
                leak(compiled.nodes.clone()),
                leak(compiled.offsets.clone()),
                leak(compiled.values.clone()),
            ),
            compiled.longest,
            (compiled.left, compiled.right),
        );
        let parsed = Dictionary::parse(text, Some((2, 3))).expect("compiles");
        for word in ["hyphenation", "table", "Hyphenation", "tables", "ta"] {
            assert_eq!(
                hyphenated(&fixed, word),
                hyphenated(&parsed, word),
                "{word}"
            );
        }
        assert_eq!(hyphenated(&fixed, "hyphenation"), "hy-phen-ation");
        assert_eq!(hyphenated(&fixed, "table"), "ta-ble", "the exception's");
        assert_eq!(fixed.bytes(), parsed.bytes());
        assert_eq!(fixed.nodes(), parsed.nodes());
        assert_eq!(fixed.vectors(), parsed.vectors());
        assert_eq!(fixed.letters(), parsed.letters());
        assert_eq!(fixed.longest(), 5, "`hen5at`'s five symbols");
    }

    /// **What a table takes is its four arrays, as the format counts them**:
    /// four bytes a letter, seven a node and seven for the sentinel, four a
    /// vector and four more, and a byte a level.
    #[test]
    fn a_tables_bytes_are_the_formats_count() {
        // `ab`, `.a1` and the exception `a-b`: letters a and b; nodes the
        // root, `a`, `a b`, `.`, `. a`, the exception mark, its `a` and its
        // `a b`; vectors `[0, 0, 0]` for `ab`, `[0, 0, 1]` for `.a1` and
        // `[0, 1, 0]` for `a-b`, nine levels.
        let small = dictionary("ab .a1", "a-b", (1, 1));
        assert_eq!(small.letters(), 2);
        assert_eq!(small.nodes(), 8);
        assert_eq!(small.vectors(), 3);
        assert_eq!(small.bytes(), 2 * 4 + (8 + 1) * 7 + (3 + 1) * 4 + 9);
        assert_eq!(small.longest(), 2);
    }

    /// **A word's walk follows at most `(n + 2) × longest + n + 1` edges** —
    /// the longest pattern from each of `.word.`'s positions and the
    /// exception path once — over a dense set of patterns and every word of
    /// two letters up to ten long, and long ones.
    #[test]
    fn a_walk_is_held_to_the_words_length_times_the_longest_pattern() {
        let mut patterns = String::new();
        for length in 1..=4u32 {
            for bits in 0..(1u32 << length) {
                let word: String = (0..length)
                    .map(|at| if (bits >> at) & 1 == 1 { 'b' } else { 'a' })
                    .collect();
                let (head, tail) = word.split_at(word.len() - 1);
                patterns.push_str(&format!("{head}{}{tail} ", length % 9 + 1));
            }
        }
        patterns.push_str(".a3b .b2a b4a. a5.");
        let dense = dictionary(&patterns, "ab-ab ba-ba-ba abba-abba", (1, 1));
        assert_eq!(dense.longest(), 4);
        let check = |word: &str| {
            STEPS.with(|n| n.set(0));
            let mut points = Vec::new();
            dense.points(word, &mut points);
            let n = word.chars().count() as u64;
            let bound = (n + 2) * dense.longest() as u64 + n + 1;
            let steps = STEPS.with(core::cell::Cell::get);
            assert!(steps <= bound, "{word}: {steps} edges past {bound}");
        };
        for length in 1..=10u32 {
            for bits in 0..(1u32 << length) {
                let word: String = (0..length)
                    .map(|at| if (bits >> at) & 1 == 1 { 'b' } else { 'a' })
                    .collect();
                check(&word);
            }
        }
        check(&"ab".repeat(MAX_HYPHENATED_WORD / 2));
        check(&"ABBA".repeat(MAX_HYPHENATED_WORD / 4));
    }

    /// **A content language finds its table by RFC 4647 §3.4's lookup**:
    /// case-insensitively, truncated from the end, a singleton going with the
    /// subtag after it — and only by truncation, so a language never finds a
    /// table for a longer tag, a region its tag does not name, or a
    /// separator BCP 47 does not use.
    #[test]
    fn a_language_finds_its_table_by_truncation() {
        let leak = |patterns: &str| -> &'static Dictionary {
            Box::leak(Box::new(dictionary(patterns, "", (1, 1))))
        };
        let tables = [
            Table {
                name: "en-gb",
                language: "en-GB",
                dictionary: leak("1ba"),
            },
            Table {
                name: "pt",
                language: "pt",
                dictionary: leak("1ab"),
            },
        ];
        let found = |tag: &str| lookup(tag, &tables).map(|table| table.name);
        assert_eq!(found("en-GB"), Some("en-gb"));
        assert_eq!(found("EN-gb"), Some("en-gb"), "case-insensitively");
        assert_eq!(found("en-GB-oxendict"), Some("en-gb"));
        assert_eq!(found("en-GB-x-house"), Some("en-gb"), "the singleton too");
        assert_eq!(found("pt-BR"), Some("pt"));
        assert_eq!(found("pt-Latn-BR"), Some("pt"));
        assert_eq!(found("en"), None, "never a longer tag");
        assert_eq!(found("en-US"), None, "nor another region");
        assert_eq!(found("en_GB"), None, "an underscore is not a separator");
        assert_eq!(found("x-pt"), None, "private use is not Portuguese");
        assert_eq!(found(""), None);
        assert_eq!(found("-"), None);
    }

    /// **The registry is the feature's**: the eight tables under
    /// `hyphenation`, each found by its own language, and none without it.
    #[test]
    fn the_registry_is_the_features() {
        if cfg!(feature = "hyphenation") {
            let names: Vec<(&str, &str)> = tables()
                .iter()
                .map(|table| (table.name, table.language))
                .collect();
            assert_eq!(
                names,
                [
                    ("af", "af"),
                    ("cu", "cu"),
                    ("en-gb", "en-GB"),
                    ("ga", "ga"),
                    ("nl", "nl"),
                    ("pl", "pl"),
                    ("pt", "pt"),
                    ("sk", "sk"),
                ]
            );
            for table in tables() {
                let found = super::dictionary(table.language).expect("its own language");
                assert!(std::ptr::eq(found, table.dictionary), "{}", table.name);
            }
        } else {
            assert!(tables().is_empty());
        }
        assert!(super::dictionary("en-US").is_none());
        assert!(super::dictionary("de").is_none());
    }

    /// One look-up counted for each word asked about, whatever it answers.
    #[test]
    fn each_word_asked_about_is_one_lookup() {
        let liang = dictionary("hy3ph he2n", "", (1, 1));
        LOOKUPS.with(|n| n.set(0));
        let mut points = Vec::new();
        for word in ["hyphen", "x", "", "hyphenhyphen"] {
            liang.points(word, &mut points);
        }
        assert_eq!(LOOKUPS.with(core::cell::Cell::get), 4);
    }

    proptest! {
        /// **Any pattern file and any word are read without a panic, and
        /// every point lies inside its word**: strictly after its first
        /// character and before its last, on a character boundary, in order.
        #[test]
        fn any_pattern_file_and_any_word_are_read_without_a_panic(
            text in prop_oneof![
                any::<String>(),
                r"(% hyphenmins:\n%  typesetting:\n%    left: [0-3]\n%    right: [0-3]\n)?\\patterns\{([abcé.\u{301}1-9]{1,6}[ \n]){0,40}\}(\\hyphenation\{([abcé-]{1,8} ){0,6}\})?",
            ],
            words in proptest::collection::vec(
                prop_oneof![
                    any::<String>(),
                    "[abcABCé.\u{301}\u{AD}\u{2019}'-]{0,70}",
                ],
                0..8,
            ),
        ) {
            let parsed = Dictionary::parse(&text, None)
                .or_else(|_| Dictionary::parse(&text, Some((1, 1))));
            if let Ok(dictionary) = parsed {
                for word in &words {
                    let mut points = Vec::new();
                    dictionary.points(word, &mut points);
                    let mut last = 0;
                    for &point in &points {
                        prop_assert!(point > last, "{point} after {last} in {word:?}");
                        prop_assert!(point < word.len(), "{point} at the end of {word:?}");
                        prop_assert!(word.is_char_boundary(point), "{point} inside a character of {word:?}");
                        last = point;
                    }
                }
            }
        }
    }
}
