//! UAX #14, the Unicode line breaking algorithm, with `css-text-3` §5's
//! tailorings.
//!
//! # Why this is not a space-scanner, in one sentence
//!
//! There are no spaces in Japanese. A breaker that splits on U+0020 and after
//! hyphens passes every English fixture anybody has ever written, passes the
//! whole of Project Gutenberg, and lays a Japanese book out as one line per
//! paragraph — and gap 31's plan names that as its own likeliest failure and
//! refuses the heuristic rather than staging it. So the rules below are the
//! specification's, by number, over the vendored UCD, and they are checked
//! against **Unicode's own conformance file**: `LineBreakTest.txt`, 19 338
//! cases, run in `tests/uax14_conformance.rs` against the same entry point a
//! book goes through.
//!
//! # The shape, and where it differs from a pair table
//!
//! UAX #14 is often implemented as a two-dimensional table indexed by the
//! classes on either side of a boundary. That works for about two thirds of
//! the rules and quietly fails the rest, because eleven of them are not about
//! a pair at all:
//!
//! - LB8, LB14, LB15a, LB16 and LB17 look **back across a run of spaces**;
//! - LB25 looks back across a run of `SY`/`IS` and forward past an `OP`;
//! - LB15b, LB15c and LB19a look **forward** one unit past the boundary;
//! - LB28a looks two units in each direction;
//! - LB30a counts the **parity** of a run of regional indicators.
//!
//! So the text is turned into [`Unit`]s first — LB9's combining marks already
//! attached to what they combine with — and every rule is then written as a
//! predicate over indices into that vector, in the specification's own order,
//! with its number beside it. The first rule that fires decides, which is
//! UAX #14's own *"a rule is invoked only when no lower-numbered rules have
//! applied"*.
//!
//! # `css-text-3` §5.5's required classes
//!
//! §5.5 says the behaviour defined for `WJ`, `ZW`, `GL` and `ZWJ` *"must be
//! honored"* whatever the tailoring. That is LB7's second half, LB8, LB8a,
//! LB11 and LB12/LB12a, and the only value that overrides any of it is
//! `line-break: anywhere`, which css-text-3 says in as many words disregards
//! even those classes. Every other tailoring here goes through
//! [`Tailoring::required_only`], so a `word-break: break-all` cannot open a
//! break inside an emoji ZWJ sequence.

use tinker_pdf_css::property::{LineBreakStrictness, WordBreak};

use crate::unicode::{
    is_combining, is_east_asian, is_extended_pictographic, is_final_punctuation,
    is_initial_punctuation, is_unassigned, line_break_class, Class,
};

/// U+25CC DOTTED CIRCLE, which LB28a names as a class of its own.
const DOTTED_CIRCLE: char = '\u{25cc}';

/// Where a line may be broken, and whether it must be.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Opportunity {
    /// The byte offset in the text at which the next line starts.
    pub at: usize,
    /// A mandatory break — LB4 and LB5's `!`, which is a newline in the
    /// source rather than a place a line happens to be full.
    pub mandatory: bool,
}

/// How `css-text-3` §5 tailors the algorithm.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Tailoring {
    /// `line-break`, §5.1.
    pub strictness: LineBreakStrictness,
    /// `word-break`, §5.2.
    pub word_break: WordBreak,
}

impl Default for Tailoring {
    fn default() -> Self {
        Self {
            strictness: LineBreakStrictness::Auto,
            word_break: WordBreak::Normal,
        }
    }
}

impl Tailoring {
    /// UAX #14 with no CSS tailoring at all, which is what the conformance
    /// file is written against.
    ///
    /// It is `strict` rather than a mode of its own, and that is the point:
    /// `line-break: strict` **is** the unmodified algorithm — §6.1's own
    /// tailoring note resolves `CJ` to `NS` for the strict style and to `ID`
    /// for the others — so the conformance run drives the same code path a
    /// book does rather than a special one written to be conformant.
    pub const UAX14: Self = Self {
        strictness: LineBreakStrictness::Strict,
        word_break: WordBreak::Normal,
    };

    /// Whether only §5.5's required classes may prohibit a break.
    ///
    /// True for `line-break: anywhere` alone. Every other value leaves the
    /// full rule set standing and only adds or removes opportunities on top of
    /// it.
    #[must_use]
    pub fn required_only(&self) -> bool {
        self.strictness == LineBreakStrictness::Anywhere
    }
}

/// One typographic character unit: a base character and the combining marks
/// LB9 attaches to it.
#[derive(Clone, Copy, Debug)]
struct Unit {
    /// The class after LB1, LB9 and LB10.
    class: Class,
    /// The base character, kept because five rules ask about the *character*
    /// rather than about its class — LB15a and LB15b about `Pi`/`Pf`, LB19a
    /// and LB30 about East Asian width, LB28a about U+25CC, and LB30b about
    /// an unassigned pictograph.
    ch: char,
    /// The byte offset of the base character.
    at: usize,
    /// Whether the unit's **last** character is a ZWJ, which is LB8a: a break
    /// is prohibited after a zero-width joiner, and LB9 has by then hidden the
    /// ZWJ inside this unit.
    ends_zwj: bool,
}

/// A decision and whether `css-text-3` §5.5 makes it untouchable.
///
/// The flag is not bookkeeping. §5.5 says the behaviour of `WJ`, `ZW`, `GL`
/// and `ZWJ` *"must be honored"* whatever the tailoring, and `word-break:
/// break-all` adds an opportunity before every letter — so without it, a
/// `break-all` paragraph breaks an emoji ZWJ sequence in half and a
/// `<span style="word-break: break-all">` around a word joiner does exactly
/// what the word joiner is there to prevent. It covers LB4 to LB12a, which is
/// the required classes plus the mandatory breaks: a tailoring may not make a
/// hard newline optional either.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Verdict {
    decision: Decision,
    required: bool,
}

impl Verdict {
    /// A decision no tailoring may change.
    const fn required(decision: Decision) -> Self {
        Self {
            decision,
            required: true,
        }
    }

    /// A decision the tailorings may add to or take away from.
    const fn tailorable(decision: Decision) -> Self {
        Self {
            decision,
            required: false,
        }
    }
}

/// What one boundary is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Decision {
    /// LB4 and LB5's `!`.
    Mandatory,
    /// A `×`.
    Prohibited,
    /// A `÷`.
    Allowed,
}

/// LB1: resolve the five classes that have no behaviour of their own.
///
/// `AI`, `SG` and `XX` become `AL`; `SA` becomes `CM` when the character is a
/// combining mark and `AL` otherwise; and `CJ` — the conditional Japanese
/// starter — is the one whose answer the **tailoring** decides. §6.1: `NS` for
/// the strict style, `ID` for the others, which is precisely the difference
/// between a Japanese line that may break before a small kana and one that may
/// not.
fn resolve(class: Class, ch: char, tailoring: Tailoring) -> Class {
    match class {
        Class::AI | Class::SG | Class::XX => Class::AL,
        Class::SA => {
            if is_combining(ch) {
                Class::CM
            } else {
                Class::AL
            }
        }
        Class::CJ => match tailoring.strictness {
            LineBreakStrictness::Strict => Class::NS,
            _ => Class::ID,
        },
        other => other,
    }
}

/// LB9 and LB10: attach combining marks to what they combine with.
fn units(text: &str, tailoring: Tailoring) -> Vec<Unit> {
    let mut out: Vec<Unit> = Vec::new();
    for (at, ch) in text.char_indices() {
        let raw = line_break_class(ch);
        let resolved = resolve(raw, ch, tailoring);
        if matches!(resolved, Class::CM | Class::ZWJ) {
            // LB9: `X (CM | ZWJ)* -> X`, where X is anything but a break or a
            // space. A mark after one of those is not attached to anything,
            // and LB10 makes it an `AL` of its own — which is why a combining
            // mark at the start of a line is a letter rather than a hole.
            if let Some(last) = out.last_mut() {
                if !matches!(
                    last.class,
                    Class::BK | Class::CR | Class::LF | Class::NL | Class::SP | Class::ZW
                ) {
                    last.ends_zwj = resolved == Class::ZWJ;
                    continue;
                }
            }
            out.push(Unit {
                class: Class::AL,
                ch,
                at,
                ends_zwj: resolved == Class::ZWJ,
            });
            continue;
        }
        out.push(Unit {
            class: resolved,
            ch,
            at,
            ends_zwj: false,
        });
    }
    out
}

/// Every break opportunity in a string, in order, with the end of the string
/// always among them.
///
/// The final entry is LB3 — *"always break at the end of text"* — and it is
/// there rather than left implicit because a caller that filled a line and
/// then asked whether the remainder had an opportunity would otherwise have to
/// special-case the tail.
#[must_use]
pub fn opportunities(text: &str, tailoring: Tailoring) -> Vec<Opportunity> {
    let mut out = Vec::new();
    for_each_opportunity(text, tailoring, |opportunity| out.push(opportunity));
    out
}

/// The same walk, without the allocation, for a caller that measures as it
/// goes.
pub fn for_each_opportunity(text: &str, tailoring: Tailoring, mut sink: impl FnMut(Opportunity)) {
    if text.is_empty() {
        return;
    }
    let units = units(text, tailoring);
    for index in 0..units.len().saturating_sub(1) {
        let decision = tailor(&units, index, tailoring, decide(&units, index, tailoring));
        if decision != Decision::Prohibited {
            sink(Opportunity {
                at: units[index + 1].at,
                mandatory: decision == Decision::Mandatory,
            });
        }
    }
    // LB3.
    sink(Opportunity {
        at: text.len(),
        mandatory: true,
    });
}

/// The class of the unit `steps` before `index`, if there is one.
fn class_at(units: &[Unit], index: usize, steps: usize) -> Option<Class> {
    index.checked_sub(steps).map(|at| units[at].class)
}

/// Walks back from `index` over units of the given classes and answers where
/// it stopped.
fn skip_back(units: &[Unit], index: usize, over: &[Class]) -> Option<usize> {
    let mut at = index;
    while over.contains(&units[at].class) {
        at = at.checked_sub(1)?;
    }
    Some(at)
}

/// Whether the run ending at `index` is `NU (SY | IS)*`, LB25's left context.
fn numeric_run(units: &[Unit], index: usize) -> bool {
    skip_back(units, index, &[Class::SY, Class::IS]).is_some_and(|at| units[at].class == Class::NU)
}

/// The set §5.2's `word-break` calls a *typographic letter unit*.
///
/// `keep-all` suppresses the implicit opportunities between two of these and
/// `break-all` adds one before each, so the set is written once and used in
/// both directions rather than twice with a chance of disagreeing.
fn is_letter(class: Class) -> bool {
    matches!(
        class,
        Class::AL
            | Class::HL
            | Class::NU
            | Class::ID
            | Class::H2
            | Class::H3
            | Class::JL
            | Class::JT
            | Class::JV
            | Class::EB
            | Class::EM
            | Class::AK
            | Class::AP
            | Class::AS
    )
}

/// `line-break: loose`'s own list, `css-text-3` §5.1.
///
/// Two of the four groups the specification names are here as characters and
/// the third is not a list at all: *"breaks are allowed before Japanese small
/// kana"* is `CJ` resolving to `ID`, which [`resolve`] already does for every
/// value but `strict`. The fourth group — breaks before centred punctuation —
/// is **not implemented** and is recorded in the crate's `Still owed` rather
/// than approximated, because a list that is nearly right is what device 2 of
/// gap 31's honesty machinery exists to prevent.
const LOOSE_BREAK_BEFORE: &[char] = &[
    // Hyphens.
    '\u{2010}', '\u{2013}', '\u{301c}', '\u{30a0}', // Iteration marks.
    '\u{3005}', '\u{303b}', '\u{309d}', '\u{309e}', '\u{30fd}', '\u{30fe}',
];

/// One boundary, by the specification's rules in the specification's order.
fn decide(units: &[Unit], b: usize, tailoring: Tailoring) -> Verdict {
    let a = b + 1;
    let before = units[b].class;
    let after = units[a].class;

    // LB4, LB5, LB6, LB7: the mandatory breaks and the two classes nothing may
    // be separated from. These stand under every tailoring, `anywhere`
    // included: a hard newline is a hard newline.
    if before == Class::BK {
        return Verdict::required(Decision::Mandatory);
    }
    if before == Class::CR && after == Class::LF {
        return Verdict::required(Decision::Prohibited);
    }
    if matches!(before, Class::CR | Class::LF | Class::NL) {
        return Verdict::required(Decision::Mandatory);
    }
    if matches!(after, Class::BK | Class::CR | Class::LF | Class::NL) {
        return Verdict::required(Decision::Prohibited);
    }
    // LB7 is `× SP` and `× ZW`. `× ZW` is one of §5.5's required rules;
    // `× SP` is not, and `line-break: anywhere` explicitly breaks around
    // preserved white space.
    if after == Class::ZW {
        return Verdict::required(Decision::Prohibited);
    }
    if after == Class::SP && !tailoring.required_only() {
        return Verdict::required(Decision::Prohibited);
    }
    // LB8: `ZW SP* ÷`.
    if skip_back(units, b, &[Class::SP]).is_some_and(|at| units[at].class == Class::ZW) {
        return Verdict::required(Decision::Allowed);
    }
    // LB8a: `ZWJ ×`. §5.5 makes this required, so `anywhere` is the only
    // value that may open it — which css-text-3 says in as many words.
    if units[b].ends_zwj && !tailoring.required_only() {
        return Verdict::required(Decision::Prohibited);
    }
    // LB11 and LB12/LB12a: `WJ` and `GL`, §5.5's other two required classes.
    if after == Class::WJ || before == Class::WJ {
        if tailoring.required_only() {
            return Verdict::required(Decision::Allowed);
        }
        return Verdict::required(Decision::Prohibited);
    }
    if before == Class::GL
        || (after == Class::GL && !matches!(before, Class::SP | Class::BA | Class::HY | Class::HH))
    {
        if tailoring.required_only() {
            return Verdict::required(Decision::Allowed);
        }
        return Verdict::required(Decision::Prohibited);
    }
    // Past §5.5's required classes, `line-break: anywhere` allows everything.
    if tailoring.required_only() {
        return Verdict::tailorable(Decision::Allowed);
    }

    // LB13.
    if matches!(after, Class::CL | Class::CP | Class::EX | Class::SY) {
        return Verdict::tailorable(Decision::Prohibited);
    }
    // LB14: `OP SP* ×`.
    if skip_back(units, b, &[Class::SP]).is_some_and(|at| units[at].class == Class::OP) {
        return Verdict::tailorable(Decision::Prohibited);
    }
    // LB15a: an opening quotation mark, possibly with spaces after it, glued
    // to whatever follows — but only where the quotation itself opens a
    // phrase. `“ x` does not break; `a “ x` does.
    if let Some(quote) = skip_back(units, b, &[Class::SP]) {
        if units[quote].class == Class::QU && is_initial_punctuation(units[quote].ch) {
            let opens = match quote.checked_sub(1) {
                None => true,
                Some(previous) => matches!(
                    units[previous].class,
                    Class::BK
                        | Class::CR
                        | Class::LF
                        | Class::NL
                        | Class::OP
                        | Class::QU
                        | Class::GL
                        | Class::SP
                        | Class::ZW
                ),
            };
            if opens {
                return Verdict::tailorable(Decision::Prohibited);
            }
        }
    }
    // LB15b: a closing quotation mark, where what follows it closes a phrase
    // too.
    if after == Class::QU && is_final_punctuation(units[a].ch) {
        let closes = match units.get(a + 1) {
            None => true,
            Some(next) => matches!(
                next.class,
                Class::SP
                    | Class::GL
                    | Class::WJ
                    | Class::CL
                    | Class::QU
                    | Class::CP
                    | Class::EX
                    | Class::IS
                    | Class::SY
                    | Class::BK
                    | Class::CR
                    | Class::LF
                    | Class::NL
                    | Class::ZW
            ),
        };
        if closes {
            return Verdict::tailorable(Decision::Prohibited);
        }
    }
    // LB15c: `SP ÷ IS NU`. It comes **before** LB15d and that order is the
    // whole rule: `. 5` breaks and `.5` does not.
    if before == Class::SP
        && after == Class::IS
        && units.get(a + 1).is_some_and(|u| u.class == Class::NU)
    {
        return Verdict::tailorable(Decision::Allowed);
    }
    // LB15d.
    if after == Class::IS {
        return Verdict::tailorable(Decision::Prohibited);
    }
    // LB16 and LB17: the two other space-crossing rules.
    if after == Class::NS
        && skip_back(units, b, &[Class::SP])
            .is_some_and(|at| matches!(units[at].class, Class::CL | Class::CP))
    {
        return Verdict::tailorable(Decision::Prohibited);
    }
    if after == Class::B2
        && skip_back(units, b, &[Class::SP]).is_some_and(|at| units[at].class == Class::B2)
    {
        return Verdict::tailorable(Decision::Prohibited);
    }
    // LB18: everything else after a space breaks. Every rule that had to look
    // across a run of spaces has now had its turn, which is why this sits
    // here and not with LB7.
    if before == Class::SP {
        return Verdict::tailorable(Decision::Allowed);
    }
    // LB19 and LB19a: quotation marks. The four LB19a clauses are why the
    // East Asian width table is vendored: a Western quote glues to its
    // neighbour and a full-width one does not.
    if after == Class::QU && !is_initial_punctuation(units[a].ch) {
        return Verdict::tailorable(Decision::Prohibited);
    }
    if before == Class::QU && !is_final_punctuation(units[b].ch) {
        return Verdict::tailorable(Decision::Prohibited);
    }
    if after == Class::QU && !is_east_asian(units[b].ch) {
        return Verdict::tailorable(Decision::Prohibited);
    }
    if after == Class::QU && units.get(a + 1).is_none_or(|u| !is_east_asian(u.ch)) {
        return Verdict::tailorable(Decision::Prohibited);
    }
    if before == Class::QU && !is_east_asian(units[a].ch) {
        return Verdict::tailorable(Decision::Prohibited);
    }
    if before == Class::QU
        && b.checked_sub(1)
            .is_none_or(|previous| !is_east_asian(units[previous].ch))
    {
        return Verdict::tailorable(Decision::Prohibited);
    }
    // LB20.
    if after == Class::CB || before == Class::CB {
        return Verdict::tailorable(Decision::Allowed);
    }
    // LB20a: a hyphen at the start of a word is part of the word — `-3` and
    // ` -abc` do not break after the hyphen, where `a-b` does.
    if matches!(after, Class::AL | Class::HL)
        && matches!(before, Class::HY | Class::HH)
        && b.checked_sub(1).is_none_or(|previous| {
            matches!(
                units[previous].class,
                Class::BK
                    | Class::CR
                    | Class::LF
                    | Class::NL
                    | Class::SP
                    | Class::ZW
                    | Class::CB
                    | Class::GL
            )
        })
    {
        return Verdict::tailorable(Decision::Prohibited);
    }
    // LB21, LB21a, LB21b.
    if matches!(after, Class::BA | Class::HY | Class::HH | Class::NS) || before == Class::BB {
        return Verdict::tailorable(Decision::Prohibited);
    }
    if matches!(before, Class::HY | Class::HH)
        && class_at(units, b, 1) == Some(Class::HL)
        && after != Class::HL
    {
        return Verdict::tailorable(Decision::Prohibited);
    }
    if before == Class::SY && after == Class::HL {
        return Verdict::tailorable(Decision::Prohibited);
    }
    // LB22.
    if after == Class::IN {
        return Verdict::tailorable(Decision::Prohibited);
    }
    // LB23 and LB23a.
    if (matches!(before, Class::AL | Class::HL) && after == Class::NU)
        || (before == Class::NU && matches!(after, Class::AL | Class::HL))
    {
        return Verdict::tailorable(Decision::Prohibited);
    }
    if (before == Class::PR && matches!(after, Class::ID | Class::EB | Class::EM))
        || (matches!(before, Class::ID | Class::EB | Class::EM) && after == Class::PO)
    {
        return Verdict::tailorable(Decision::Prohibited);
    }
    // LB24.
    if (matches!(before, Class::PR | Class::PO) && matches!(after, Class::AL | Class::HL))
        || (matches!(before, Class::AL | Class::HL) && matches!(after, Class::PR | Class::PO))
    {
        return Verdict::tailorable(Decision::Prohibited);
    }
    // LB25, all fifteen of it. Unicode 15.1 rewrote what used to be a regular
    // expression over a whole numeric expression into these pairwise clauses,
    // which is the only reason an implementation of this shape can be
    // conformant at all.
    if matches!(before, Class::CL | Class::CP)
        && matches!(after, Class::PO | Class::PR)
        && b.checked_sub(1)
            .is_some_and(|left| numeric_run(units, left))
    {
        return Verdict::tailorable(Decision::Prohibited);
    }
    if matches!(after, Class::PO | Class::PR | Class::NU) && numeric_run(units, b) {
        return Verdict::tailorable(Decision::Prohibited);
    }
    if matches!(before, Class::PO | Class::PR) {
        if after == Class::NU {
            return Verdict::tailorable(Decision::Prohibited);
        }
        if after == Class::OP {
            let one = units.get(a + 1).map(|u| u.class);
            let two = units.get(a + 2).map(|u| u.class);
            if one == Some(Class::NU) || (one == Some(Class::IS) && two == Some(Class::NU)) {
                return Verdict::tailorable(Decision::Prohibited);
            }
        }
    }
    if matches!(before, Class::HY | Class::IS) && after == Class::NU {
        return Verdict::tailorable(Decision::Prohibited);
    }
    // LB26 and LB27: Hangul.
    if (before == Class::JL && matches!(after, Class::JL | Class::JV | Class::H2 | Class::H3))
        || (matches!(before, Class::JV | Class::H2) && matches!(after, Class::JV | Class::JT))
        || (matches!(before, Class::JT | Class::H3) && after == Class::JT)
    {
        return Verdict::tailorable(Decision::Prohibited);
    }
    let hangul = |class: Class| {
        matches!(
            class,
            Class::JL | Class::JV | Class::JT | Class::H2 | Class::H3
        )
    };
    if (hangul(before) && after == Class::PO) || (before == Class::PR && hangul(after)) {
        return Verdict::tailorable(Decision::Prohibited);
    }
    // LB28.
    if matches!(before, Class::AL | Class::HL) && matches!(after, Class::AL | Class::HL) {
        return Verdict::tailorable(Decision::Prohibited);
    }
    // LB28a: the Brahmi-family aksara rules, added in Unicode 15.1.
    let aksara =
        |unit: &Unit| matches!(unit.class, Class::AK | Class::AS) || unit.ch == DOTTED_CIRCLE;
    let aksara_or_ak = |unit: &Unit| unit.class == Class::AK || unit.ch == DOTTED_CIRCLE;
    if before == Class::AP && aksara(&units[a]) {
        return Verdict::tailorable(Decision::Prohibited);
    }
    if aksara(&units[b]) && matches!(after, Class::VF | Class::VI) {
        return Verdict::tailorable(Decision::Prohibited);
    }
    if before == Class::VI
        && b.checked_sub(1).is_some_and(|left| aksara(&units[left]))
        && aksara_or_ak(&units[a])
    {
        return Verdict::tailorable(Decision::Prohibited);
    }
    if aksara(&units[b])
        && aksara(&units[a])
        && units.get(a + 1).is_some_and(|u| u.class == Class::VF)
    {
        return Verdict::tailorable(Decision::Prohibited);
    }
    // LB29.
    if before == Class::IS && matches!(after, Class::AL | Class::HL) {
        return Verdict::tailorable(Decision::Prohibited);
    }
    // LB30: and this is the other place the East Asian table earns itself.
    if matches!(before, Class::AL | Class::HL | Class::NU)
        && after == Class::OP
        && !is_east_asian(units[a].ch)
    {
        return Verdict::tailorable(Decision::Prohibited);
    }
    if before == Class::CP
        && !is_east_asian(units[b].ch)
        && matches!(after, Class::AL | Class::HL | Class::NU)
    {
        return Verdict::tailorable(Decision::Prohibited);
    }
    // LB30a: a flag is two regional indicators, and two flags in a row may be
    // separated between them and nowhere else. Parity, not a pair.
    if before == Class::RI && after == Class::RI {
        let mut run = 0usize;
        let mut at = Some(b);
        while let Some(index) = at {
            if units[index].class != Class::RI {
                break;
            }
            run += 1;
            at = index.checked_sub(1);
        }
        if run % 2 == 1 {
            return Verdict::tailorable(Decision::Prohibited);
        }
        return Verdict::tailorable(Decision::Allowed);
    }
    // LB30b: an emoji modifier stays with what it modifies, including where
    // Unicode has not assigned the base yet.
    if before == Class::EB && after == Class::EM {
        return Verdict::tailorable(Decision::Prohibited);
    }
    if after == Class::EM && is_extended_pictographic(units[b].ch) && is_unassigned(units[b].ch) {
        return Verdict::tailorable(Decision::Prohibited);
    }
    // LB31.
    Verdict::tailorable(Decision::Allowed)
}

/// The CSS tailorings that add or remove opportunities on top of the rules.
///
/// Kept separate from [`decide`] so that the conformance run can drive the
/// rules with nothing on top of them, and so that a tailoring that changed an
/// answer the specification fixes would be visible as an edit **here** rather
/// than as a changed rule.
fn tailor(units: &[Unit], b: usize, tailoring: Tailoring, verdict: Verdict) -> Decision {
    let decision = verdict.decision;
    if verdict.required || tailoring.required_only() {
        return decision;
    }
    let before = units[b].class;
    let after = units[b + 1].class;
    match tailoring.word_break {
        // §5.2: *"only ... explicit opportunities apply"*, so an implicit one
        // between two letters is suppressed. This is the value a CJK book sets
        // to keep a proper noun together, and without it 東京都 may break
        // after any of the three.
        WordBreak::KeepAll if is_letter(before) && is_letter(after) => Decision::Prohibited,
        // §5.2: a break before any letter, which is what a CJK-style
        // justification of Latin text asks for. §5.5's required classes have
        // already had their say above, so this cannot open an emoji sequence.
        WordBreak::BreakAll if is_letter(after) => Decision::Allowed,
        _ => match tailoring.strictness {
            LineBreakStrictness::Loose if LOOSE_BREAK_BEFORE.contains(&units[b + 1].ch) => {
                Decision::Allowed
            }
            _ => decision,
        },
    }
}

#[cfg(test)]
mod tests;
