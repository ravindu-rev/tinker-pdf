//! The vendored UCD, compiled into static ranges by `build.rs`.
//!
//! Nothing here decides anything: it answers *what property does this
//! character have*, and [`crate::uax14`] is where the answers turn into break
//! opportunities. The split is deliberate — the tables are somebody else's
//! published facts and the algorithm is ours, and the two are wrong in
//! completely different ways.
//!
//! # Why the generated table names variants rather than indices
//!
//! `build.rs` writes `Class::AL` into every row rather than a number, so a
//! Line_Break class this crate has never heard of **fails to build**. That is
//! not hypothetical: Unicode 16.0 added `HH` and Unicode 15.1 added `AK`,
//! `AP`, `AS`, `VF` and `VI`, and a build that mapped an unknown class name
//! onto a default would have laid out Brahmi-family scripts and unambiguous
//! hyphens as though the additions had never happened — silently, and
//! plausibly, which is this gap's whole subject.

/// UAX #14's Line_Break property, at Unicode 17.0's forty-eight values.
///
/// The names are the specification's two-letter abbreviations rather than
/// spelled-out ones, because every rule in [`crate::uax14`] is written in
/// them and a translation layer between the rule text and the code is a place
/// for a transcription error to hide.
#[allow(clippy::upper_case_acronyms)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Class {
    /// Ambiguous (alphabetic or ideograph).
    AI,
    /// Aksara.
    AK,
    /// Alphabetic.
    AL,
    /// Aksara pre-base.
    AP,
    /// Aksara start.
    AS,
    /// Break opportunity before and after.
    B2,
    /// Break after.
    BA,
    /// Break before.
    BB,
    /// Mandatory break.
    BK,
    /// Contingent break opportunity.
    CB,
    /// Conditional Japanese starter.
    CJ,
    /// Close punctuation.
    CL,
    /// Combining mark.
    CM,
    /// Close parenthesis.
    CP,
    /// Carriage return.
    CR,
    /// Emoji base.
    EB,
    /// Emoji modifier.
    EM,
    /// Exclamation/interrogation.
    EX,
    /// Non-breaking (glue).
    GL,
    /// Hangul LV syllable.
    H2,
    /// Hangul LVT syllable.
    H3,
    /// Unambiguous hyphen, new in Unicode 16.0.
    HH,
    /// Hebrew letter.
    HL,
    /// Hyphen.
    HY,
    /// Ideographic.
    ID,
    /// Inseparable.
    IN,
    /// Infix numeric separator.
    IS,
    /// Hangul L Jamo.
    JL,
    /// Hangul T Jamo.
    JT,
    /// Hangul V Jamo.
    JV,
    /// Line feed.
    LF,
    /// Next line.
    NL,
    /// Non-starter.
    NS,
    /// Numeric.
    NU,
    /// Open punctuation.
    OP,
    /// Postfix numeric.
    PO,
    /// Prefix numeric.
    PR,
    /// Quotation.
    QU,
    /// Regional indicator.
    RI,
    /// Complex-context dependent (South East Asian).
    SA,
    /// Surrogate.
    SG,
    /// Space.
    SP,
    /// Symbols allowing break after.
    SY,
    /// Virama final.
    VF,
    /// Virama.
    VI,
    /// Word joiner.
    WJ,
    /// Unknown.
    XX,
    /// Zero width space.
    ZW,
    /// Zero width joiner.
    ZWJ,
}

/// UAX #11's East_Asian_Width, which UAX #14 itself needs.
///
/// Not for measurement — advance widths come from [`crate::metrics::Metrics`]
/// and never from here. LB30's parenthesis rules exclude the Wide, Full-width
/// and Half-width forms, and LB19a's quotation rules turn on the same
/// distinction, so a breaker without this table gets `A(` and `Ａ（` the same
/// way and one of the two is wrong.
#[allow(clippy::upper_case_acronyms)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Eaw {
    /// Ambiguous.
    A,
    /// Fullwidth.
    F,
    /// Halfwidth.
    H,
    /// Neutral.
    N,
    /// Narrow.
    Na,
    /// Wide.
    W,
}

include!(concat!(env!("OUT_DIR"), "/ucd.rs"));

/// A code point's value in a sorted `(first, last, value)` table.
fn lookup<T: Copy>(table: &[(u32, u32, T)], code: u32, default: T) -> T {
    let mut low = 0usize;
    let mut high = table.len();
    while low < high {
        let middle = (low + high) / 2;
        let (first, last, value) = table[middle];
        if code < first {
            high = middle;
        } else if code > last {
            low = middle + 1;
        } else {
            return value;
        }
    }
    default
}

/// Whether a code point is in a sorted `(first, last)` table.
fn member(table: &[(u32, u32)], code: u32) -> bool {
    let mut low = 0usize;
    let mut high = table.len();
    while low < high {
        let middle = (low + high) / 2;
        let (first, last) = table[middle];
        if code < first {
            high = middle;
        } else if code > last {
            low = middle + 1;
        } else {
            return true;
        }
    }
    false
}

/// The character's Line_Break property, **unresolved**.
///
/// `AI`, `CJ`, `SA`, `SG` and `XX` come back as themselves rather than as
/// what LB1 turns them into, because LB1's answer depends on the tailoring —
/// `CJ` is `NS` under `line-break: strict` and `ID` under `normal` — and a
/// table that had already decided could not be tailored at all.
#[must_use]
pub fn line_break_class(c: char) -> Class {
    lookup(LINE_BREAK, c as u32, Class::XX)
}

/// The character's East_Asian_Width.
#[must_use]
pub fn east_asian_width(c: char) -> Eaw {
    lookup(EAST_ASIAN, c as u32, Eaw::N)
}

/// Wide, Full-width or Half-width — the set UAX #14's own rules call
/// `EastAsian`.
#[must_use]
pub fn is_east_asian(c: char) -> bool {
    matches!(east_asian_width(c), Eaw::F | Eaw::W | Eaw::H)
}

/// `Extended_Pictographic`, LB30b's half.
#[must_use]
pub fn is_extended_pictographic(c: char) -> bool {
    member(EXTENDED_PICTOGRAPHIC, c as u32)
}

/// General_Category `Cn` — a code point Unicode has not assigned.
#[must_use]
pub fn is_unassigned(c: char) -> bool {
    member(UNASSIGNED, c as u32)
}

/// General_Category `Mn` or `Mc`, which is LB1's test for an `SA` character.
#[must_use]
pub fn is_combining(c: char) -> bool {
    member(COMBINING, c as u32)
}

/// General_Category `Pi`, which LB15a's `QU ∩ Pi` is.
#[must_use]
pub fn is_initial_punctuation(c: char) -> bool {
    member(INITIAL_PUNCTUATION, c as u32)
}

/// General_Category `Pf`, which LB15b's `QU ∩ Pf` is.
#[must_use]
pub fn is_final_punctuation(c: char) -> bool {
    member(FINAL_PUNCTUATION, c as u32)
}
