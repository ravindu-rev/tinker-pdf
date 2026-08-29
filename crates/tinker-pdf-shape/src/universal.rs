//! The Universal Shaping Engine's cluster model: what a syllable is, and which
//! of its characters have to move.
//!
//! Milestone 5 of `docs/design/shaping.md`, which calls this the roadmap's
//! largest single item and USE correctness *"effectively unbounded"*. What is
//! here is the part the vendored fixtures adjudicate, and
//! `docs/features/fonts.md` says by name which scripts that leaves shaped but
//! unverified. Nothing here is claimed beyond what SHBALI, SHKNDA and SHLANA
//! check.
//!
//! # The one thing a Brahmic shaper must do that a Latin one need not
//!
//! A dependent vowel is **typed after the consonant it belongs to and drawn on
//! whichever side the script puts it** — and for a good many of them that side
//! is the left. `Indic_Positional_Category` says which, and the value that
//! costs work is `Left`: those characters are stored after their base and have
//! to be moved before it before any lookup that positions them can match.
//!
//! Tai Tham's `U+1A6E TAI THAM VOWEL SIGN E` is the plain case, and
//! text-rendering-tests SHLANA-1/21 states it outright: two characters typed
//! consonant-then-vowel, and an expected rendering of vowel-then-consonant.
//! Balinese `SHBALI-2/1` is the case that says **how far** it moves: past a
//! whole conjunct, to the front of the syllable, and not merely past the
//! consonant it is attached to.
//!
//! `Visual_Order_Left` is the opposite value and the reason the property has
//! two: those characters are *already* stored where they are drawn, and moving
//! one would break a script that was fine.
//!
//! # What a syllable is here
//!
//! A base, and everything that hangs off it. A new one starts at a base
//! character unless a halant came immediately before — because a halant is
//! precisely the statement that the next consonant belongs to this cluster
//! rather than starting a new one. Anything that is not Brahmic at all ends
//! the syllable it interrupts and stands alone.
//!
//! That is a **simplification** of USE's published grammar, which is a regular
//! expression over a dozen categories with named cluster types, and it is
//! stated as one rather than presented as the whole thing. What it captures is
//! the base, the halant chain and the trailing marks, which is what the
//! reordering below and the per-syllable feature application need. What it
//! does not capture is the distinction between the cluster *types* — a numeral
//! cluster from a symbol cluster from a broken one — which USE uses to decide
//! whether to insert a dotted circle. This crate never inserts one; see
//! `docs/features/fonts.md`.
//!
//! # The three rules, reintroduced as defects and counted
//!
//! Each was put back and `cargo test -p tinker-pdf-shape --no-fail-fast` run
//! against it, so these are measured and not expected:
//!
//! | Defect reintroduced | Tests that caught it |
//! | --- | --- |
//! | The halant moves the reordering insertion point again | 3 |
//! | The reordering pause is skipped altogether | 2 |
//! | Canonical decomposition is switched off | 2 |
//!
//! The first is caught by this module's own unit test as well as by
//! `tests/text_rendering.rs` and `tests/fingerprints.rs`; the other two are
//! caught only by those, because both defects are in `crate::shape`'s *use* of
//! this module and [`reorder`] answers correctly for the categories it is
//! handed either way. That asymmetry is the reason the conformance suite is
//! the guard and the unit tests are the explanation.
//!
//! # Why the syllable outlives this module
//!
//! Every USE feature is applied **per syllable**: a rule may not match across a
//! cluster boundary, because two adjacent syllables are two words as far as a
//! conjunct-forming lookup is concerned. That is carried on the buffer rather
//! than done here, so that the whole of `GSUB` obeys it; see
//! [`crate::buffer::Buffer::set_syllable`].

use crate::unicode::{indic_positional, indic_syllabic, IndicPositional, IndicSyllabic};

/// What a character is, as far as the cluster model is concerned.
///
/// Five values where USE has around twenty. The ones that are collapsed are
/// collapsed because nothing this crate does distinguishes them: a nukta, a
/// tone mark and an above-base vowel are all "a mark that stays where it is",
/// and USE tells them apart in order to name cluster types and to order the
/// basic features, neither of which this milestone does per-category.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Category {
    /// A consonant, an independent vowel, a number, a placeholder: the thing a
    /// syllable is built around.
    Base,
    /// A virama or an invisible stacker. Ends the run of marks and says the
    /// next base continues this syllable rather than starting one.
    Halant,
    /// A dependent mark drawn to the **left** of its base, and therefore one
    /// that has to be moved there.
    PreBase,
    /// Any other dependent mark: above, below, after, or overstruck.
    Mark,
    /// Not a Brahmic character. Latin, a space, a digit, punctuation.
    #[default]
    Other,
}

/// A character's category, from the two Unicode properties and nothing else.
///
/// The mapping is stated exhaustively rather than with a catch-all arm, so
/// that a value Unicode adds to either property fails to compile here instead
/// of quietly becoming [`Category::Other`] and losing a script's reordering.
pub(crate) fn category(c: char) -> Category {
    use IndicSyllabic as S;
    match indic_syllabic(c) {
        // The two that stack what follows onto what precedes.
        S::Virama | S::InvisibleStacker => Category::Halant,

        // The bases. A placeholder is one because that is what it is for: it
        // stands in where a cluster would otherwise have none.
        S::Consonant
        | S::ConsonantDead
        | S::ConsonantHeadLetter
        | S::ConsonantPlaceholder
        | S::ConsonantWithStacker
        | S::ConsonantPrefixed
        | S::ConsonantInitialPostfixed
        | S::VowelIndependent
        | S::Vowel
        | S::ModifyingLetter
        | S::ToneLetter
        | S::Number
        | S::BrahmiJoiningNumber => Category::Base,

        // Everything that hangs off a base. Whether it hangs off the *left* is
        // the positional property's business and not this one's.
        S::Avagraha
        | S::Bindu
        | S::CantillationMark
        | S::ConsonantFinal
        | S::ConsonantKiller
        | S::ConsonantMedial
        | S::ConsonantPrecedingRepha
        | S::ConsonantSubjoined
        | S::ConsonantSucceedingRepha
        | S::GeminationMark
        | S::Joiner
        | S::NonJoiner
        | S::NumberJoiner
        | S::Nukta
        | S::PureKiller
        | S::RegisterShifter
        | S::ReorderingKiller
        | S::SyllableModifier
        | S::ToneMark
        | S::Visarga
        | S::VowelDependent => {
            if indic_positional(c) == IndicPositional::Left {
                Category::PreBase
            } else {
                Category::Mark
            }
        }

        S::Other => Category::Other,
    }
}

/// Which syllable each character of `text` belongs to, numbered from one.
///
/// Zero is never returned, because zero is the buffer's "this glyph is in no
/// syllable" and a run that is not Brahmic must not be restricted by a
/// boundary it does not have.
///
/// The numbering saturates at [`u16::MAX`]: a paragraph with sixty-five
/// thousand syllables in it would start sharing numbers between the last of
/// them, which merges two clusters that should have been separate rather than
/// splitting one that should not — the safe direction, and unreachable in any
/// text a reader would look at.
pub(crate) fn syllables(text: &[char]) -> Vec<u16> {
    let mut out = Vec::new();
    let mut current = 0u16;
    let mut previous = Category::Other;
    for c in text.iter().copied() {
        let category = category(c);
        let starts = match category {
            // A base starts a syllable unless a halant just said it does not.
            Category::Base => previous != Category::Halant,
            // A mark with no base before it is a syllable of its own, which is
            // the "broken cluster" USE names. It still gets a number, so the
            // features that would fix it are applied to it and to nothing
            // else.
            Category::Halant | Category::PreBase | Category::Mark => {
                matches!(previous, Category::Other)
            }
            Category::Other => false,
        };
        if starts {
            current = current.saturating_add(1);
        }
        out.push(if category == Category::Other {
            0
        } else {
            current
        });
        previous = category;
    }
    out
}

/// Where each character of one syllable moves to, as a permutation.
///
/// Returns `None` when nothing moves, which is the overwhelmingly common case
/// and worth not allocating for.
///
/// # The rule
///
/// Every pre-base member of the syllable is moved to the **front of the
/// syllable**, and everything it passes shuffles up one. The insertion point
/// advances after each move, so two pre-base characters in one syllable keep
/// the order they were typed in.
///
/// # The halant used to move the insertion point, and the corpus said no
///
/// This walked the syllable keeping an insertion point just after the last
/// halant, on the reasoning that *"a pre-base vowel belongs in front of the
/// consonant it attaches to and not in front of the whole conjunct"*. That is
/// a plausible sentence and it is not what the fixtures say. Balinese
/// `SHBALI-2/1` is `KA ADEG-ADEG PA TALING` — a conjunct of two consonants
/// followed by a pre-base vowel — and the expected rendering puts the taling
/// **first**, in front of the whole conjunct rather than in front of the
/// second consonant. Dropping the halant rule moved thirty cases across six
/// sections and cost none, which is as close to adjudication as ruling 13
/// permits.
///
/// What survives of the old reasoning is a real question this corpus does not
/// answer: a `pref` consonant — one a face reorders to *before the base* — is
/// a different move from a pre-base vowel's, and USE has a separate step for
/// it. Nothing here implements that step, and no fixture reaches one.
///
/// # Two pre-base characters come out reversed, and that is adjudicated now
///
/// The insertion point does **not** advance after a move, so the second
/// pre-base member of a syllable ends up in front of the first. That stood
/// here as a guess, on the grounds that USE says where a pre-base character
/// goes and does not say what two of them do relative to each other, and that
/// no case in the vendored corpus had two.
///
/// The second half of that was wrong. Tai Tham `SHLANA-6/2` and `SHLANA-6/4`
/// each have a syllable holding both `U+1A55 CONSONANT SIGN MEDIAL RA` and a
/// pre-base vowel — two `Left` characters in one cluster — and both expect the
/// vowel first, which is the reverse of the order they are typed in. Advancing
/// the insertion point was tried and costs those two cases and one of
/// `SHLANA-10`'s; not advancing it is what the fixtures say.
pub(crate) fn reorder(categories: &[Category]) -> Option<Vec<usize>> {
    if !categories.contains(&Category::PreBase) {
        return None;
    }
    let mut order: Vec<usize> = (0..categories.len()).collect();
    let mut moved = false;
    for (at, category) in categories.iter().enumerate() {
        // `at` indexes the *original* sequence, and `order` says where each
        // original character now sits, so the position to lift from is the
        // one holding `at`.
        let Some(from) = order.iter().position(|which| *which == at) else {
            continue;
        };
        // The insertion point is the front of the syllable and stays there,
        // which is what reverses a pair of pre-base characters.
        if *category == Category::PreBase && from > 0 {
            let lifted = order.remove(from);
            order.insert(0, lifted);
            moved = true;
        }
    }
    moved.then_some(order)
}

#[cfg(test)]
mod tests {
    use super::{category, reorder, syllables, Category};

    /// Tai Tham, and the case text-rendering-tests SHLANA-1/21 states: a
    /// consonant and a vowel typed in that order and drawn in the other.
    #[test]
    fn a_pre_base_vowel_is_a_pre_base_vowel() {
        assert_eq!(category('\u{1A3D}'), Category::Base, "TAI THAM LETTER BHA");
        assert_eq!(
            category('\u{1A6E}'),
            Category::PreBase,
            "TAI THAM VOWEL SIGN E is Vowel_Dependent and Left"
        );
        // And one that is drawn above rather than before, so the positional
        // property is doing the work rather than the syllabic one alone.
        assert_eq!(category('\u{1A65}'), Category::Mark, "VOWEL SIGN I is Top");
        assert_eq!(category('\u{1A60}'), Category::Halant, "SAKOT");
        assert_eq!(category('a'), Category::Other);
    }

    /// `Visual_Order_Left` is already where it is drawn. Thai's `SARA E` is
    /// the classic one, and moving it would break a script that was correct.
    #[test]
    fn a_visual_order_left_vowel_does_not_move() {
        // `U+0E40 THAI CHARACTER SARA E` is a dependent vowel drawn before its
        // consonant and **typed** there too, which is exactly what
        // `Visual_Order_Left` means. So it is a mark like any other as far as
        // this module is concerned: the one thing that must not happen to it
        // is being moved somewhere it already is.
        assert_eq!(category('\u{0E40}'), Category::Mark);
        assert_eq!(
            category('\u{0E01}'),
            Category::Base,
            "THAI CHARACTER KO KAI"
        );
        assert!(reorder(&[category('\u{0E40}'), category('\u{0E01}')]).is_none());
    }

    #[test]
    fn a_pre_base_vowel_moves_in_front_of_its_base() {
        let cats = [Category::Base, Category::PreBase];
        assert_eq!(reorder(&cats), Some(vec![1, 0]));
    }

    /// A halant does **not** move the insertion point: the vowel goes in front
    /// of the whole conjunct.
    ///
    /// text-rendering-tests `SHBALI-2/1` is this shape — `KA ADEG-ADEG PA
    /// TALING` — and its expected rendering draws the taling first. The
    /// opposite rule stood here until the fixtures were run against it, and
    /// dropping it moved thirty cases across six sections.
    #[test]
    fn a_halant_does_not_move_the_insertion_point() {
        let cats = [
            Category::Base,
            Category::Halant,
            Category::Base,
            Category::PreBase,
        ];
        assert_eq!(reorder(&cats), Some(vec![3, 0, 1, 2]));
    }

    /// Two pre-base characters in one syllable come out in the **reverse** of
    /// the order they were typed in.
    ///
    /// Tai Tham `SHLANA-6/2` and `SHLANA-6/4` are where that is adjudicated:
    /// each holds `U+1A55 CONSONANT SIGN MEDIAL RA` and a pre-base vowel in
    /// one syllable, and each expects the vowel drawn first. The opposite rule
    /// was tried and costs three cases across two sections.
    #[test]
    fn two_pre_base_characters_come_out_reversed() {
        let cats = [Category::Base, Category::PreBase, Category::PreBase];
        assert_eq!(reorder(&cats), Some(vec![2, 1, 0]));
    }

    #[test]
    fn a_syllable_with_nothing_to_move_moves_nothing() {
        assert!(reorder(&[Category::Base, Category::Mark]).is_none());
        assert!(reorder(&[]).is_none());
        // A pre-base character already at the front is already right.
        assert!(reorder(&[Category::PreBase, Category::Base]).is_none());
    }

    #[test]
    fn a_halant_keeps_two_consonants_in_one_syllable() {
        // Devanagari ka, virama, ka: one syllable, not two.
        assert_eq!(
            syllables(&"\u{915}\u{94D}\u{915}".chars().collect::<Vec<char>>()),
            vec![1, 1, 1]
        );
        // Without the virama they are two.
        assert_eq!(
            syllables(&"\u{915}\u{915}".chars().collect::<Vec<char>>()),
            vec![1, 2]
        );
    }

    #[test]
    fn what_is_not_brahmic_is_in_no_syllable() {
        assert_eq!(
            syllables(&"a\u{915}b".chars().collect::<Vec<char>>()),
            vec![0, 1, 0]
        );
        assert!(syllables(&"hello".chars().collect::<Vec<char>>())
            .iter()
            .all(|id| *id == 0));
        assert!(syllables(&"".chars().collect::<Vec<char>>()).is_empty());
    }

    /// A mark with no base in front of it is USE's "broken cluster". It is
    /// still numbered, so the features that would repair it reach it and
    /// nothing else.
    #[test]
    fn a_mark_with_no_base_is_still_a_syllable() {
        assert_eq!(
            syllables(&"\u{94D}\u{915}".chars().collect::<Vec<char>>()),
            vec![1, 1]
        );
        assert_eq!(
            syllables(&"a\u{94D}".chars().collect::<Vec<char>>()),
            vec![0, 1]
        );
    }
}
