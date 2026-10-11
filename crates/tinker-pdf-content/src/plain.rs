//! Assembling lines into plain text, and the one thing assembly may change:
//! a word hyphenated across a line end.
//!
//! [`crate::TextPage::plain_text`] writes one line per line and changes
//! nothing, and it stays that way — the parity suite pins it, and a caller
//! comparing two extractions needs one that reports what the page drew. What
//! lives here is the **opt-in** sibling, [`crate::TextPage::plain_text_with`],
//! and the structured view's twin of it in the facade, which both hand their
//! lines to [`assemble`] so the two cannot come to disagree about what a
//! hyphen is.
//!
//! # What is joined
//!
//! Two cases, and they are different in kind, which is why they are counted
//! apart:
//!
//! - **A soft hyphen, always.** U+00AD is a *discretionary* hyphen: Unicode
//!   defines it as invisible except where a line breaks at it, so a line
//!   ending in one is by definition a word the producer broke, and one inside
//!   a line is a hyphenation point the producer recorded and did not use.
//!   Both are removed; a line ending in one is joined to the next whatever
//!   the next begins with. This is certain.
//! - **A hard hyphen at a line end, followed by a lower-case start.** U+002D
//!   HYPHEN-MINUS and U+2010 HYPHEN, when the character before it is a letter
//!   and the next line's first character is lower-case (the Unicode
//!   `Lowercase` property, as `char::is_lowercase` reads it). This is an
//!   **inference**: a compound broken at its own hyphen — `well-` / `known` —
//!   is joined to `wellknown`, which is wrong, and nothing on the page says
//!   which of the two it was. That is why it is opt-in, and why the count of
//!   hard joins is reported separately from the certain ones: a caller who
//!   cannot afford the guess can see how many were made.
//!
//! Not joined, deliberately: U+2011 NON-BREAKING HYPHEN (a line should never
//! end at one, so one that does is not a hyphenation point), the dashes U+2012
//! to U+2015 (punctuation, not word-breaking), U+FE63 and U+FF0D (the small
//! and full-width forms, which belong to CJK setting), a hyphen after a space
//! or standing alone (a dash written with the hyphen key), and a hard hyphen
//! before an upper-case letter, a digit or anything without case — `Jean-` /
//! `Paul` and `pages 10-` / `12` keep theirs.

/// How [`crate::TextPage::plain_text_with`] assembles a page's lines.
///
/// `Default` is [`crate::TextPage::plain_text`]'s behaviour, to the byte.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PlainTextOptions {
    /// Remove soft hyphens and rejoin words hyphenated across a line end —
    /// see the module documentation for exactly which.
    pub rejoin_hyphens: bool,
}

/// A page's plain text, and what assembling it changed.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PlainText {
    /// The text: one line per output line, each ending in `\n`.
    pub text: String,
    /// What [`PlainTextOptions::rejoin_hyphens`] did. All zero when it is
    /// off.
    pub hyphens: HyphenCounts,
}

/// How many hyphens assembly removed, and how many line ends it joined.
///
/// Counted, because a join is a change to what the page says: a caller that
/// compares extractions, or indexes them, needs to know that the text is not
/// the page's own and by how much. The certain case and the inferred one are
/// kept apart for the reason the module documentation gives.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct HyphenCounts {
    /// Line ends joined at a soft hyphen (U+00AD). Certain.
    pub soft_joins: usize,
    /// Line ends joined at a hard hyphen (U+002D, U+2010) before a lower-case
    /// start. Inferred, and sometimes wrong — a compound broken at its own
    /// hyphen is joined too.
    pub hard_joins: usize,
    /// Every soft hyphen removed, wherever it stood — the ones that ended a
    /// joined line included, so this is never less than
    /// [`HyphenCounts::soft_joins`].
    pub soft_removed: usize,
}

impl HyphenCounts {
    /// Every line end joined, certain and inferred together.
    #[must_use]
    pub fn joins(&self) -> usize {
        self.soft_joins + self.hard_joins
    }
}

/// U+00AD SOFT HYPHEN.
const SOFT_HYPHEN: char = '\u{AD}';

/// Whether a character is a hard hyphen this module will join at.
fn is_hard_hyphen(c: char) -> bool {
    // U+002D HYPHEN-MINUS, which is what almost every producer writes, and
    // U+2010 HYPHEN, which is unambiguously a hyphen and never a minus.
    matches!(c, '-' | '\u{2010}')
}

/// Which kind of join ends a line.
#[derive(Clone, Copy)]
enum Join {
    Soft,
    Hard,
}

/// Assembles lines into plain text: each line followed by `\n`, with hyphens
/// rejoined when `options` asks.
///
/// The lines are what the caller's view calls lines — a page's text lines in
/// content-stream order for [`crate::TextPage::plain_text_with`], a structure
/// run each for the facade's structured view. A join joins **consecutive
/// lines of that sequence**, whichever block they sit in: a word hyphenated
/// at the foot of one column continues at the head of the next, and the two
/// halves are two blocks.
#[must_use]
pub fn assemble<'a, I>(lines: I, options: &PlainTextOptions) -> PlainText
where
    I: IntoIterator<Item = &'a str>,
{
    let mut out = PlainText::default();
    if !options.rejoin_hyphens {
        for line in lines {
            out.text.push_str(line);
            out.text.push('\n');
        }
        return out;
    }

    let lines: Vec<&str> = lines.into_iter().collect();
    let mut continuing = false;
    for (index, line) in lines.iter().enumerate() {
        // A line the previous one joined into loses its indentation, which was
        // the gap at the start of a line and not a space inside the word.
        let line: &str = if continuing { line.trim_start() } else { line };
        let next = lines.get(index + 1).copied();
        let end = line.trim_end();
        let last = end.chars().next_back();
        let join = match last {
            Some(SOFT_HYPHEN) if next.is_some() => Some(Join::Soft),
            Some(c) if is_hard_hyphen(c) => {
                let before = end
                    .get(..end.len() - c.len_utf8())
                    .and_then(|head| head.chars().rev().find(|&b| b != SOFT_HYPHEN));
                let lower_start = next
                    .and_then(|n| n.trim_start().chars().next())
                    .is_some_and(char::is_lowercase);
                (before.is_some_and(char::is_alphabetic) && lower_start).then_some(Join::Hard)
            }
            _ => None,
        };

        // What is kept of this line: all of it, or everything before the
        // hyphen that joins it — trailing space included in what goes, since
        // it sat between the hyphen and the line end.
        let kept = match (join, last) {
            (Some(_), Some(c)) => end.get(..end.len() - c.len_utf8()).unwrap_or(end),
            _ => line,
        };
        for c in kept.chars() {
            if c == SOFT_HYPHEN {
                out.hyphens.soft_removed += 1;
            } else {
                out.text.push(c);
            }
        }
        match join {
            Some(Join::Soft) => {
                out.hyphens.soft_removed += 1;
                out.hyphens.soft_joins += 1;
            }
            Some(Join::Hard) => out.hyphens.hard_joins += 1,
            None => out.text.push('\n'),
        }
        continuing = join.is_some();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rejoin(lines: &[&str]) -> PlainText {
        assemble(
            lines.iter().copied(),
            &PlainTextOptions {
                rejoin_hyphens: true,
            },
        )
    }

    #[test]
    fn off_is_one_line_per_line_and_changes_nothing() {
        let lines = ["hyphen-", "ation", "soft\u{AD}", "ware"];
        let out = assemble(lines.iter().copied(), &PlainTextOptions::default());
        assert_eq!(out.text, "hyphen-\nation\nsoft\u{AD}\nware\n");
        assert_eq!(out.hyphens, HyphenCounts::default());
    }

    /// A soft hyphen at a line end joins, whatever the next line begins with:
    /// it is a break the producer made, by definition.
    #[test]
    fn a_soft_hyphen_at_a_line_end_always_joins() {
        let out = rejoin(&["Donau\u{AD}", "Dampfschiff"]);
        assert_eq!(out.text, "DonauDampfschiff\n");
        assert_eq!(out.hyphens.soft_joins, 1);
        assert_eq!(out.hyphens.soft_removed, 1);
        assert_eq!(out.hyphens.hard_joins, 0);

        // Before a digit, too, and with the line's trailing space and the next
        // line's indentation gone.
        let out = rejoin(&["ISBN\u{AD}  ", "   978"]);
        assert_eq!(out.text, "ISBN978\n");
        assert_eq!(out.hyphens.soft_joins, 1);
    }

    /// And inside a line it is removed with nothing joined: it is a
    /// hyphenation point the producer recorded and did not use.
    #[test]
    fn a_soft_hyphen_inside_a_line_is_removed() {
        let out = rejoin(&["hy\u{AD}phen\u{AD}ation is", "here"]);
        assert_eq!(out.text, "hyphenation is\nhere\n");
        assert_eq!(out.hyphens.soft_removed, 2);
        assert_eq!(out.hyphens.joins(), 0);
    }

    /// On the last line there is nothing to join to, and it is still removed.
    #[test]
    fn a_soft_hyphen_ending_the_last_line_is_removed_and_joins_nothing() {
        let out = rejoin(&["first", "continued\u{AD}"]);
        assert_eq!(out.text, "first\ncontinued\n");
        assert_eq!(out.hyphens.soft_removed, 1);
        assert_eq!(out.hyphens.soft_joins, 0);
    }

    #[test]
    fn a_hard_hyphen_before_a_lower_case_start_joins() {
        let out = rejoin(&["the hyphen-", "ation of words"]);
        assert_eq!(out.text, "the hyphenation of words\n");
        assert_eq!(out.hyphens.hard_joins, 1);
        assert_eq!(out.hyphens.soft_joins, 0);

        // U+2010 HYPHEN is the same hyphen, spelled unambiguously.
        let out = rejoin(&["hyphen\u{2010}", "ation"]);
        assert_eq!(out.text, "hyphenation\n");
        assert_eq!(out.hyphens.hard_joins, 1);

        // Lower-case beyond ASCII is lower-case.
        let out = rejoin(&["Straßen-", "übergang"]);
        assert_eq!(out.text, "Straßenübergang\n");
    }

    #[test]
    fn a_hard_hyphen_before_an_upper_case_start_or_a_digit_stays() {
        let out = rejoin(&["Jean-", "Paul"]);
        assert_eq!(out.text, "Jean-\nPaul\n");
        let out = rejoin(&["pages 10-", "12"]);
        assert_eq!(out.text, "pages 10-\n12\n");
        let out = rejoin(&["Mid-", "1990s"]);
        assert_eq!(out.text, "Mid-\n1990s\n");
        assert_eq!(out.hyphens, HyphenCounts::default());
    }

    #[test]
    fn a_hyphen_inside_a_line_is_untouched() {
        let out = rejoin(&["a well-known mid-line hyphen", "and more"]);
        assert_eq!(out.text, "a well-known mid-line hyphen\nand more\n");
        assert_eq!(out.hyphens, HyphenCounts::default());
    }

    /// A hyphen with no letter before it is a dash typed with the hyphen key,
    /// and the lines around it are two thoughts, not one word.
    #[test]
    fn a_standalone_hyphen_is_a_dash_and_stays() {
        let out = rejoin(&["the result -", "surprisingly"]);
        assert_eq!(out.text, "the result -\nsurprisingly\n");
        let out = rejoin(&["-", "item"]);
        assert_eq!(out.text, "-\nitem\n");
    }

    #[test]
    fn dashes_and_the_non_breaking_hyphen_are_not_joined() {
        for dash in [
            '\u{2011}', '\u{2012}', '\u{2013}', '\u{2014}', '\u{FE63}', '\u{FF0D}',
        ] {
            let first = format!("word{dash}");
            let out = rejoin(&[&first, "next"]);
            assert_eq!(out.text, format!("{first}\nnext\n"), "{dash:?} joined");
        }
    }

    /// Joins chain: a word broken across three lines comes back whole.
    #[test]
    fn joins_chain_across_several_lines() {
        let out = rejoin(&["in-", "compre\u{AD}", "hensible", "next"]);
        assert_eq!(out.text, "incomprehensible\nnext\n");
        assert_eq!(out.hyphens.hard_joins, 1);
        assert_eq!(out.hyphens.soft_joins, 1);
        assert_eq!(out.hyphens.joins(), 2);
    }

    #[test]
    fn nothing_in_nothing_out() {
        assert_eq!(rejoin(&[]), PlainText::default());
    }
}
