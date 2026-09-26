//! Cursive joining: which of the four forms each letter of a joining script
//! is in.
//!
//! Milestone 4 of `docs/design/shaping.md`. The output is one [`Form`] per
//! character, which [`crate::shape`] turns into the feature mask that decides
//! whether `isol`, `fina`, `medi` or `init` may touch that glyph.
//!
//! # Derived from the rule, not from a state table
//!
//! The Unicode Standard states cursive joining declaratively, in §9.2 and the
//! `Joining_Type` property: a character joins to the character *on its right*
//! if its own type is `R`, `D` or `C` and the type of the character on its
//! right is `L`, `D` or `C`; and symmetrically on its left. In a right-to-left
//! script the character on the right is the **preceding** one in logical
//! order, which is the one place the terminology has to be read carefully and
//! the reason [`Joins`] names the two sides after buffer positions rather than
//! after compass directions.
//!
//! Both sides gives the medial form, the previous side alone the final form,
//! the next side alone the initial form, and neither the isolated form. That
//! is the whole algorithm, and writing it this way rather than as the
//! equivalent finite-state table means there is no transcription to get wrong:
//! every letter of the rule is in [`form`] and can be read against the
//! Standard's own sentence.
//!
//! # Transparent characters, and why they are invisible rather than skipped
//!
//! A combining mark has `Joining_Type=T` and does not take part: it neither
//! has a form of its own nor breaks the join across it, so `beh + fatha + yeh`
//! joins exactly as `beh + yeh` does. That is done by looking past every `T`
//! when finding the neighbours, and by giving a `T` character [`Form::None`],
//! which sets no form bit at all — so a mark reaches the global features and
//! none of the four joining ones.
//!
//! # What is not here, named rather than absent
//!
//! **Syriac's Alaph.** Syriac selects `fin2`, `fin3` and `med2` in place of
//! `fina` for one letter, chosen by its `Joining_Group` and by what precedes
//! it. That property is not vendored and those three features are not
//! requested; the reason is ruling 13 and it is written up in `build.rs` and
//! in `docs/features/fonts.md` — no fixture in either vendored corpus has a
//! Syriac face, so implementing it would be adding behavior that nothing in
//! this repository could show was right.
//!
//! **Fallback joining.** A face with no `isol`/`fina`/`medi`/`init` features
//! gets no joining, rather than this crate substituting from the Arabic
//! Presentation Forms blocks on the face's behalf. Those blocks are
//! compatibility characters, the Standard says so, and a shaper that reached
//! for them would be inventing glyphs the designer did not draw.

use crate::unicode::{joining_type, JoiningType};

/// Which of the four joining forms a character is in.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Form {
    /// Joined on neither side. The `isol` feature.
    Isolated,
    /// Joined to the character before it only. The `fina` feature.
    Final,
    /// Joined on both sides. The `medi` feature.
    Medial,
    /// Joined to the character after it only. The `init` feature.
    Initial,
    /// Not a character that takes a form: a combining mark, or anything in a
    /// script that does not join. No feature.
    #[default]
    None,
}

/// Which sides of a character its neighbours join to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Joins {
    /// Whether this character joins to the one **before** it in the buffer —
    /// the one on its right, in a right-to-left script.
    previous: bool,
    /// Whether it joins to the one **after** it.
    next: bool,
}

/// Whether a type joins on the side that faces the character before it.
///
/// `R` — Right_Joining — is named for the character on its right, which in
/// logical order is the preceding one.
const fn joins_backward(kind: JoiningType) -> bool {
    matches!(kind, JoiningType::R | JoiningType::D | JoiningType::C)
}

/// Whether a type joins on the side that faces the character after it.
const fn joins_forward(kind: JoiningType) -> bool {
    matches!(kind, JoiningType::L | JoiningType::D | JoiningType::C)
}

/// Whether a run has anything in it that joins at all.
///
/// This is what decides that a run gets the joining shaper, and it is asked of
/// the **text** rather than of the script, deliberately. A list of joining
/// scripts written out here would be a transcription of somebody else's list,
/// and it would be wrong the day Unicode gives an existing script a joining
/// letter or adds a new one; the property itself cannot be. The consequence in
/// the other direction is bounded and correct: a run of Arabic-Indic digits
/// has no joining character in it, so it takes the default shaper, and the
/// default shaper gives it exactly the same glyphs because none of its
/// characters has a form to be in.
pub(crate) fn joins(text: &str) -> bool {
    text.chars().any(|c| {
        matches!(
            joining_type(c),
            JoiningType::R | JoiningType::D | JoiningType::L | JoiningType::C
        )
    })
}

/// The form each character of `text` is in, one per character, in order.
///
/// The two passes are what make a `T` invisible: the first records every
/// character's type, the second walks the non-transparent ones and asks each
/// about the non-transparent characters either side of it.
pub(crate) fn forms(text: &str) -> Vec<Form> {
    let types: Vec<JoiningType> = text.chars().map(joining_type).collect();
    let mut out = vec![Form::None; types.len()];
    // The indices of the characters that take part, in order. Everything not
    // in here is Transparent and keeps `Form::None`.
    let opaque: Vec<usize> = (0..types.len())
        .filter(|at| types[*at] != JoiningType::T)
        .collect();
    for (n, at) in opaque.iter().enumerate() {
        let kind = types[*at];
        // A Non_Joining character takes part in the rule — it is what stops
        // its neighbours joining across it — but has no form of its own. Only
        // the four joining types do, and giving `U` the isolated form would
        // hand a space and a digit the `isol` feature.
        if kind == JoiningType::U {
            continue;
        }
        let previous = n
            .checked_sub(1)
            .and_then(|before| opaque.get(before))
            .map(|before| types[*before]);
        let next = opaque.get(n.saturating_add(1)).map(|after| types[*after]);
        let joined = Joins {
            previous: joins_backward(kind) && previous.is_some_and(joins_forward),
            next: joins_forward(kind) && next.is_some_and(joins_backward),
        };
        out[*at] = form(joined);
    }
    out
}

/// The Standard's four cases, in one place.
const fn form(joined: Joins) -> Form {
    match (joined.previous, joined.next) {
        (true, true) => Form::Medial,
        (true, false) => Form::Final,
        (false, true) => Form::Initial,
        (false, false) => Form::Isolated,
    }
}

#[cfg(test)]
mod tests {
    use super::{forms, joins, Form};

    /// `لسان`, which is text-rendering-tests SHARAN-1/1, and whose expected
    /// glyph names state the answer outright: `LamIni`, `SeenMed`, `AlefFin`,
    /// `NoonxSep`. Four characters, four different forms, in one word.
    #[test]
    fn one_word_of_urdu_takes_all_four_forms() {
        assert_eq!(
            forms("\u{644}\u{633}\u{627}\u{646}"),
            vec![Form::Initial, Form::Medial, Form::Final, Form::Isolated]
        );
    }

    /// The rule is about the *neighbours*, so alef — which joins only to what
    /// precedes it — leaves the letter after it starting a new word.
    #[test]
    fn a_right_joining_letter_does_not_join_forwards() {
        // beh alef beh: beh-alef join, then alef cannot join to the second
        // beh, which is therefore isolated.
        let out = forms("\u{628}\u{627}\u{628}");
        assert_eq!(out, vec![Form::Initial, Form::Final, Form::Isolated]);
    }

    /// A combining mark is invisible to the rule: `beh + fatha + yeh` joins
    /// exactly as `beh + yeh` does, and the mark itself has no form.
    #[test]
    fn a_transparent_character_does_not_break_a_join() {
        let without = forms("\u{628}\u{64A}");
        let with = forms("\u{628}\u{64E}\u{64A}");
        assert_eq!(without, vec![Form::Initial, Form::Final]);
        assert_eq!(with, vec![Form::Initial, Form::None, Form::Final]);
    }

    /// `ZWNJ` is Non_Joining and `ZWJ` is Join_Causing, and the difference is
    /// the whole reason a reader can ask for either.
    #[test]
    fn the_two_zero_width_characters_pull_in_opposite_directions() {
        assert_eq!(
            forms("\u{628}\u{200C}\u{628}"),
            vec![Form::Isolated, Form::None, Form::Isolated],
            "ZWNJ should have broken the join"
        );
        assert_eq!(
            forms("\u{628}\u{200D}"),
            vec![Form::Initial, Form::Final],
            "ZWJ should have caused one"
        );
    }

    /// Tatweel is Join_Causing and takes forms of its own, which is what makes
    /// it stretch a word rather than end one.
    #[test]
    fn tatweel_joins_on_both_sides() {
        assert_eq!(
            forms("\u{628}\u{640}\u{628}"),
            vec![Form::Initial, Form::Medial, Form::Final]
        );
    }

    #[test]
    fn a_script_with_no_joining_letters_is_not_offered_the_shaper() {
        assert!(!joins("hello"));
        assert!(!joins("\u{5D0}\u{5D1}"), "Hebrew does not join");
        // Arabic-Indic digits are Non_Joining, so a run of them is not a
        // joining run even though its script is Arabic.
        assert!(!joins("\u{661}\u{662}\u{663}"));
        assert!(joins("\u{628}"));
        assert!(joins("\u{1820}"), "Mongolian joins");
    }

    #[test]
    fn text_with_nothing_in_it_has_no_forms() {
        assert!(forms("").is_empty());
        assert!(!joins(""));
    }
}
