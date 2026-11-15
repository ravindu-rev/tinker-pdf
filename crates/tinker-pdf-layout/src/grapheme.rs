//! UAX #29's extended grapheme clusters, and the typographic character units
//! no line breaks inside.
//!
//! `css-text-3` §1.4 makes *"the extended grapheme cluster (not legacy
//! grapheme cluster), as defined in UAX29"* the basis of a typographic
//! character unit, and §5.5 says *"CSS never allows soft wrap opportunities
//! within typographic character units"*. So every place a line may break —
//! UAX #14's opportunities under every `line-break` and `word-break`, a soft
//! hyphen's, a point `hyphens: auto` finds, and the break `overflow-wrap`
//! makes inside a word — is held to a boundary [`units`] gives
//! ([`crate::flow`]), and so is every point [`crate::hyphenate`] returns. A
//! letter is never parted from its marks, a Hangul syllable from its jamo, a
//! conjunct from its consonants, an emoji from its modifier or the rest of its
//! ZWJ sequence, or a flag from the other half of its pair.
//!
//! # The rules, from the specification
//!
//! UAX #29 revision 47, for Unicode 17.0, §3.1.1, over the vendored
//! `GraphemeBreakProperty.txt`, `emoji-data.txt`'s `Extended_Pictographic`
//! and `DerivedCoreProperties.txt`'s `Indic_Conjunct_Break`, each rule beside
//! its number in [`decide`]; the first that applies decides:
//!
//! - GB1, GB2: a boundary at the start and the end of text, unless it is empty;
//! - GB3: `CR × LF`;
//! - GB4, GB5: a boundary after and before every `Control`, `CR` and `LF`;
//! - GB6 to GB8: none inside a Hangul syllable sequence;
//! - GB9, GB9a, GB9b: none before `Extend`, `ZWJ` or `SpacingMark`, and none
//!   after `Prepend`;
//! - GB9c: none in `Consonant [Extend Linker]* Linker [Extend Linker]* ×
//!   Consonant`, over `Indic_Conjunct_Break`;
//! - GB11: none in `Extended_Pictographic Extend* ZWJ × Extended_Pictographic`;
//! - GB12, GB13: none between two regional indicators after an odd number of
//!   them;
//! - GB999: a boundary everywhere else.
//!
//! [`boundaries`] is that algorithm untailored, and `tests/grapheme_conformance.rs`
//! holds it to every case of Unicode's own `GraphemeBreakTest.txt`.
//!
//! # The one tailoring
//!
//! §1.4 goes on: the UA *"should tailor the definitions as required by
//! typographical tradition"*, and [`units`] does once. **A mark —
//! General_Category `M` — never starts a unit**, except after a control,
//! where GB4's boundary stands. UAX #29 Table 2 leaves a list of spacing marks
//! out of `SpacingMark` — Myanmar's vowel sign AA, U+102C, among them — so its
//! clusters break before each; UAX #14's LB9 attaches every one of them to the
//! letter before it, and so did this crate's hyphenator, so a line breaks
//! round a Myanmar syllable and not inside it. The tailoring changes no
//! boundary GB1 to GB13 decide: only GB999's, before a mark.

use crate::unicode::{
    grapheme_break, indic_conjunct_break, is_extended_pictographic, is_mark, Gcb, Incb,
};

/// Every extended grapheme cluster boundary of `text`, as byte offsets in
/// order: UAX #29's rules, untailored, the start and the end of the text
/// among them, and none at all for empty text.
#[must_use]
pub fn boundaries(text: &str) -> Vec<usize> {
    let mut out = Vec::new();
    walk(text, false, |at| out.push(at));
    out
}

/// Every boundary of `text`'s typographic character units, as byte offsets
/// in order: [`boundaries`] with [the one tailoring](self#the-one-tailoring),
/// the boundaries no line may break off them.
#[must_use]
pub fn units(text: &str) -> Vec<usize> {
    let mut out = Vec::new();
    walk(text, true, |at| out.push(at));
    out
}

/// What one boundary is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Decision {
    /// GB4 or GB5's boundary, round a control, which the tailoring keeps.
    Control,
    /// A rule's `×`.
    Joined,
    /// GB999's boundary, the one the tailoring may take away.
    Otherwise,
}

/// How much of GB9c's `Consonant [Extend Linker]* Linker [Extend Linker]*`
/// the text up to a position ends with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Conjunct {
    /// None of it.
    None,
    /// A consonant, and marks after it with no linker among them.
    Consonant,
    /// A consonant, and marks after it with a linker among them.
    Linked,
}

impl Conjunct {
    /// The state after one more character.
    fn after(self, c: char) -> Conjunct {
        match indic_conjunct_break(c) {
            Some(Incb::Consonant) => Conjunct::Consonant,
            Some(Incb::Linker) if self != Conjunct::None => Conjunct::Linked,
            Some(Incb::Extend) => self,
            _ => Conjunct::None,
        }
    }
}

/// How much of GB11's `Extended_Pictographic Extend* ZWJ` the text up to a
/// position ends with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Emoji {
    /// None of it.
    None,
    /// A pictograph, and `Extend` characters after it.
    Pictograph,
    /// A pictograph, its `Extend` characters, and a ZWJ.
    Joined,
}

impl Emoji {
    /// The state after one more character, of the given break property.
    fn after(self, c: char, property: Gcb) -> Emoji {
        if is_extended_pictographic(c) || (self == Emoji::Pictograph && property == Gcb::Extend) {
            Emoji::Pictograph
        } else if self == Emoji::Pictograph && property == Gcb::ZWJ {
            Emoji::Joined
        } else {
            Emoji::None
        }
    }
}

/// The boundary between a character of property `before` and the next, `c`
/// of property `after`, given what the text before it ends with: the
/// conjunct GB9c reads, the emoji sequence GB11 reads, and how many regional
/// indicators in a row end it, which GB12 and GB13 read.
fn decide(
    before: Gcb,
    (c, after): (char, Gcb),
    conjunct: Conjunct,
    emoji: Emoji,
    indicators: usize,
) -> Decision {
    use Gcb::{Control, Extend, Prepend, SpacingMark, CR, L, LF, LV, LVT, RI, T, V, ZWJ};
    // GB3.
    if before == CR && after == LF {
        return Decision::Joined;
    }
    // GB4 and GB5.
    if matches!(before, Control | CR | LF) || matches!(after, Control | CR | LF) {
        return Decision::Control;
    }
    // GB6, GB7 and GB8: a Hangul syllable sequence.
    if (before == L && matches!(after, L | V | LV | LVT))
        || (matches!(before, LV | V) && matches!(after, V | T))
        || (matches!(before, LVT | T) && after == T)
    {
        return Decision::Joined;
    }
    // GB9, GB9a and GB9b.
    if matches!(after, Extend | ZWJ | SpacingMark) || before == Prepend {
        return Decision::Joined;
    }
    // GB9c.
    if conjunct == Conjunct::Linked && indic_conjunct_break(c) == Some(Incb::Consonant) {
        return Decision::Joined;
    }
    // GB11.
    if emoji == Emoji::Joined && is_extended_pictographic(c) {
        return Decision::Joined;
    }
    // GB12 and GB13: the indicators before this one pair up only if there is
    // an odd number of them.
    if after == RI && indicators % 2 == 1 {
        return Decision::Joined;
    }
    // GB999.
    Decision::Otherwise
}

/// The walk both answers share, handing each boundary to `sink` in order.
fn walk(text: &str, tailored: bool, mut sink: impl FnMut(usize)) {
    let mut chars = text.char_indices();
    // GB1 and GB2 break at the ends of text, *"unless the text is empty"*.
    let Some((_, first)) = chars.next() else {
        return;
    };
    sink(0);
    let mut before = grapheme_break(first);
    let mut conjunct = Conjunct::None.after(first);
    let mut emoji = Emoji::None.after(first, before);
    let mut indicators = usize::from(before == Gcb::RI);
    for (at, c) in chars {
        let after = grapheme_break(c);
        let boundary = match decide(before, (c, after), conjunct, emoji, indicators) {
            Decision::Control => true,
            Decision::Joined => false,
            Decision::Otherwise => !(tailored && is_mark(c)),
        };
        if boundary {
            sink(at);
        }
        conjunct = conjunct.after(c);
        emoji = emoji.after(c, after);
        indicators = if after == Gcb::RI {
            indicators.saturating_add(1)
        } else {
            0
        };
        before = after;
    }
    sink(text.len());
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `text` cut at `boundaries`, for a test to compare with what it reads.
    fn cut(text: &str, at: &[usize]) -> Vec<String> {
        at.windows(2)
            .map(|pair| text[pair[0]..pair[1]].to_owned())
            .collect()
    }

    /// **Each kind of cluster is one unit**, as each rule makes it: a letter
    /// and its nonspacing, spacing and enclosing marks (GB9, GB9a), a Hangul
    /// syllable written as jamo (GB6 to GB8), a conjunct (GB9c), an emoji
    /// and its modifier (GB9, `Emoji_Modifier` being `Extend`), an emoji ZWJ
    /// sequence (GB11), a flag's pair of regional indicators (GB12, GB13),
    /// and a prepended concatenation mark with what it is prepended to
    /// (GB9b).
    #[test]
    fn each_kind_of_cluster_is_one_unit() {
        for (text, clusters) in [
            ("e\u{301}x", vec!["e\u{301}", "x"]),
            ("\u{915}\u{93f}\u{915}", vec!["\u{915}\u{93f}", "\u{915}"]),
            ("a\u{20dd}b", vec!["a\u{20dd}", "b"]),
            (
                "\u{1100}\u{1161}\u{11a8}\u{1100}",
                vec!["\u{1100}\u{1161}\u{11a8}", "\u{1100}"],
            ),
            ("\u{915}\u{94d}\u{924}a", vec!["\u{915}\u{94d}\u{924}", "a"]),
            ("\u{1f44d}\u{1f3fd}a", vec!["\u{1f44d}\u{1f3fd}", "a"]),
            (
                "\u{1f469}\u{200d}\u{1f4bb}a",
                vec!["\u{1f469}\u{200d}\u{1f4bb}", "a"],
            ),
            (
                "\u{1f1ec}\u{1f1e7}\u{1f1eb}\u{1f1f7}\u{1f1ee}",
                vec!["\u{1f1ec}\u{1f1e7}", "\u{1f1eb}\u{1f1f7}", "\u{1f1ee}"],
            ),
            ("\u{600}1a", vec!["\u{600}1", "a"]),
        ] {
            assert_eq!(cut(text, &boundaries(text)), clusters, "{text:?}");
            assert_eq!(cut(text, &units(text)), clusters, "{text:?}, tailored");
        }
        assert!(boundaries("").is_empty() && units("").is_empty());
        assert_eq!(boundaries("a"), [0, 1]);
    }

    /// **A mark never starts a unit, except after a control**: Myanmar's
    /// vowel sign AA, which UAX #29 leaves out of `SpacingMark`, stays with
    /// its consonant in a unit and not in a cluster, while a mark after a
    /// line feed or a soft hyphen — GB4's boundary — still starts one, and
    /// nothing else GB999 decides moves.
    #[test]
    fn the_tailoring_keeps_every_mark_with_its_letter_and_nothing_else() {
        let myanmar = "\u{1019}\u{102c}\u{1019}";
        assert_eq!(
            cut(myanmar, &boundaries(myanmar)),
            ["\u{1019}", "\u{102c}", "\u{1019}"]
        );
        assert_eq!(
            cut(myanmar, &units(myanmar)),
            ["\u{1019}\u{102c}", "\u{1019}"]
        );
        for after_a_control in ["a\n\u{301}", "a\u{ad}\u{301}", "\r\u{102c}"] {
            assert_eq!(
                units(after_a_control),
                boundaries(after_a_control),
                "{after_a_control:?}"
            );
        }
        let plain = "ab cd, \u{4e2d}\u{6587}";
        assert_eq!(units(plain), boundaries(plain));
    }
}
