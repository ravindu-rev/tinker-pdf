//! The vendored UCD, compiled by `build.rs` into runs and decoded into range
//! tables at first use.
//!
//! Nothing here decides anything: it answers *what property does this
//! character have*, and [`crate::uax14`] is where the answers turn into break
//! opportunities. The split is deliberate — the tables are somebody else's
//! published facts and the algorithm is ours, and the two are wrong in
//! completely different ways.
//!
//! # The form, and why it is runs
//!
//! Each property is written as the runs it makes over every code point in
//! order — an enumerated one's value index and length, a binary one's lengths
//! alone, alternately without and with — as bytes, two to a run for most
//! runs, and decoded once a process into each run's start and value, which
//! one bisection answers from (`Tables`). Of the forms measured — ranges of
//! twelve bytes a row, run starts packed with their values in four, and runs
//! — the runs are the smallest after gzip, and gzipped is how every wasm
//! build ships them. `tests/properties.rs` holds every table, at every code
//! point, to the file it came from.
//!
//! # Why the generated table names variants rather than indices
//!
//! `build.rs` writes each table's values as `Class::AL` and the rest, in the
//! list the runs index, rather than as numbers, so a
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
/// Not for measurement — advance widths come from `tinker-pdf-layout`'s
/// `Metrics` and never from here. LB30's parenthesis rules exclude the Wide,
/// Full-width and Half-width forms, and LB19a's quotation rules turn on the same
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

/// UAX #29's Grapheme_Cluster_Break, at Unicode 17.0's thirteen values and
/// `Other`, the value of every code point the file does not list.
///
/// The names are the file's own, `RI` for `Regional_Indicator` as the rules
/// write it, so [`crate::grapheme`]'s rules read as the specification's.
#[allow(clippy::upper_case_acronyms)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Gcb {
    /// U+000D.
    CR,
    /// U+000A.
    LF,
    /// A control, format or separator character, the line and paragraph
    /// separators among them.
    Control,
    /// `Grapheme_Extend` or `Emoji_Modifier`: the nonspacing and enclosing
    /// marks, ZWNJ, and the few spacing marks canonical equivalence needs.
    Extend,
    /// U+200D ZERO WIDTH JOINER.
    ZWJ,
    /// A regional indicator, half of a flag.
    RI,
    /// A prepended concatenation mark or a consonant written before its
    /// syllable's base.
    Prepend,
    /// A spacing mark that extends a cluster.
    SpacingMark,
    /// A Hangul leading consonant jamo.
    L,
    /// A Hangul vowel jamo.
    V,
    /// A Hangul trailing consonant jamo.
    T,
    /// A Hangul LV syllable.
    LV,
    /// A Hangul LVT syllable.
    LVT,
    /// Anything else (`XX`).
    Other,
}

/// UAX #29's `Indic_Conjunct_Break`, which GB9c is written in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Incb {
    /// A consonant a conjunct joins.
    Consonant,
    /// A mark that may sit inside a conjunct.
    Extend,
    /// A virama that links two consonants into one cluster.
    Linker,
}

include!(concat!(env!("OUT_DIR"), "/ucd.rs"));

/// An enumerated property, decoded at first use from the runs `build.rs`
/// wrote: each run's first code point, in order, and its value, so a code
/// point's value is the last run's that starts at or before it, found by one
/// bisection.
#[derive(Debug)]
struct Runs<T> {
    starts: Vec<u32>,
    values: Vec<T>,
    /// The property's value where no row names one: `{NAME}_VALUES`' first.
    default: T,
}

impl<T: Copy> Runs<T> {
    /// The runs of `bytes` — each a value's index into `values`, a byte, and
    /// its length in code points as LEB128 — every code point in order.
    fn decode(bytes: &[u8], values: &[T], default: T) -> Runs<T> {
        let mut starts = Vec::new();
        let mut out = Vec::new();
        let mut at = 0u32;
        let mut rest = bytes;
        while let Some((&index, tail)) = rest.split_first() {
            let (length, tail) = leb128(tail);
            starts.push(at);
            out.push(values.get(usize::from(index)).copied().unwrap_or(default));
            at = at.saturating_add(length);
            rest = tail;
        }
        Runs {
            starts,
            values: out,
            default,
        }
    }

    fn get(&self, code: u32) -> T {
        let run = self.starts.partition_point(|&start| start <= code);
        run.checked_sub(1)
            .and_then(|run| self.values.get(run))
            .copied()
            .unwrap_or(self.default)
    }
}

/// A binary property, decoded at first use from its runs' lengths: each
/// run's first code point, in order, the runs alternately without the
/// property and with it from U+0000.
#[derive(Debug)]
struct Set {
    starts: Vec<u32>,
}

impl Set {
    fn decode(bytes: &[u8]) -> Set {
        let mut starts = vec![0];
        let mut at = 0u32;
        let mut rest = bytes;
        while !rest.is_empty() {
            let (length, tail) = leb128(rest);
            at = at.saturating_add(length);
            starts.push(at);
            rest = tail;
        }
        Set { starts }
    }

    /// Whether `code` is in a run with the property: the run that holds it is
    /// the last of the runs starting at or before it, and those are counted
    /// from the first, without.
    fn contains(&self, code: u32) -> bool {
        self.starts.partition_point(|&start| start <= code) % 2 == 0
    }
}

/// The LEB128 number `bytes` starts with — seven bits a byte, the low ones
/// first, each byte but the last with its top bit set — and the bytes after.
fn leb128(bytes: &[u8]) -> (u32, &[u8]) {
    let mut value = 0u32;
    let mut shift = 0u32;
    for (at, &byte) in bytes.iter().enumerate() {
        value |= u32::from(byte & 0x7f).checked_shl(shift).unwrap_or(0);
        if byte & 0x80 == 0 {
            return (value, bytes.get(at + 1..).unwrap_or(&[]));
        }
        shift += 7;
    }
    (value, &[])
}

/// Every property this module answers from a table, decoded together at its
/// first question (P-19.1: a derived table is built at first use, never
/// evaluated at compile time).
///
/// `build.rs` writes each as runs, the smallest form after gzip of the ones
/// measured — a range's twelve bytes a row against a run's two — because the
/// wasm builds ship it gzipped, and each answers by one bisection of its
/// runs' starts, as the ranges did.
#[derive(Debug)]
struct Tables {
    line_break: Runs<Class>,
    east_asian: Runs<Eaw>,
    grapheme_break: Runs<Gcb>,
    indic_conjunct_break: Runs<Option<Incb>>,
    extended_pictographic: Set,
    unassigned: Set,
    combining: Set,
    mark: Set,
    initial_punctuation: Set,
    final_punctuation: Set,
    punctuation_or_symbol: Set,
    letter_or_number: Set,
    cased: Set,
    case_ignorable: Set,
    lowercase: Set,
}

static TABLES: std::sync::LazyLock<Tables> = std::sync::LazyLock::new(|| Tables {
    line_break: Runs::decode(LINE_BREAK_RUNS, LINE_BREAK_VALUES, Class::XX),
    east_asian: Runs::decode(EAST_ASIAN_RUNS, EAST_ASIAN_VALUES, Eaw::N),
    grapheme_break: Runs::decode(GRAPHEME_BREAK_RUNS, GRAPHEME_BREAK_VALUES, Gcb::Other),
    indic_conjunct_break: Runs::decode(
        INDIC_CONJUNCT_BREAK_RUNS,
        INDIC_CONJUNCT_BREAK_VALUES,
        None,
    ),
    extended_pictographic: Set::decode(EXTENDED_PICTOGRAPHIC_RUNS),
    unassigned: Set::decode(UNASSIGNED_RUNS),
    combining: Set::decode(COMBINING_RUNS),
    mark: Set::decode(MARK_RUNS),
    initial_punctuation: Set::decode(INITIAL_PUNCTUATION_RUNS),
    final_punctuation: Set::decode(FINAL_PUNCTUATION_RUNS),
    punctuation_or_symbol: Set::decode(PUNCTUATION_OR_SYMBOL_RUNS),
    letter_or_number: Set::decode(LETTER_OR_NUMBER_RUNS),
    cased: Set::decode(CASED_RUNS),
    case_ignorable: Set::decode(CASE_IGNORABLE_RUNS),
    lowercase: Set::decode(LOWERCASE_RUNS),
});

/// The character's Line_Break property, **unresolved**.
///
/// `AI`, `CJ`, `SA`, `SG` and `XX` come back as themselves rather than as
/// what LB1 turns them into, because LB1's answer depends on the tailoring —
/// `CJ` is `NS` under `line-break: strict` and `ID` under `normal` — and a
/// table that had already decided could not be tailored at all.
#[must_use]
pub fn line_break_class(c: char) -> Class {
    TABLES.line_break.get(c as u32)
}

/// The character's East_Asian_Width.
#[must_use]
pub fn east_asian_width(c: char) -> Eaw {
    TABLES.east_asian.get(c as u32)
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
    TABLES.extended_pictographic.contains(c as u32)
}

/// General_Category `Cn` — a code point Unicode has not assigned.
#[must_use]
pub fn is_unassigned(c: char) -> bool {
    TABLES.unassigned.contains(c as u32)
}

/// General_Category `Mn` or `Mc`, which is LB1's test for an `SA` character.
#[must_use]
pub fn is_combining(c: char) -> bool {
    TABLES.combining.contains(c as u32)
}

/// General_Category `Pi`, which LB15a's `QU ∩ Pi` is.
#[must_use]
pub fn is_initial_punctuation(c: char) -> bool {
    TABLES.initial_punctuation.contains(c as u32)
}

/// General_Category `Pf`, which LB15b's `QU ∩ Pf` is.
#[must_use]
pub fn is_final_punctuation(c: char) -> bool {
    TABLES.final_punctuation.contains(c as u32)
}

/// General_Category `P*` or `S*`: CommonMark 0.31 §2.1's *Unicode punctuation
/// character*, which the facade's Markdown reader asks of the character either
/// side of an emphasis delimiter run. From the same vendored
/// `DerivedGeneralCategory.txt` as every other category here.
#[must_use]
pub fn is_punctuation_or_symbol(c: char) -> bool {
    TABLES.punctuation_or_symbol.contains(c as u32)
}

/// A code point's entry in a sorted `(code, mapped)` case table.
fn mapped(table: &'static [(u32, &'static str)], code: u32) -> Option<&'static str> {
    table
        .binary_search_by_key(&code, |(at, _)| *at)
        .ok()
        .and_then(|index| table.get(index))
        .map(|(_, mapped)| *mapped)
}

/// The character's full uppercase mapping, Unicode §3.13, or `None` where it
/// maps to itself. `ß` is `SS`: the mapping is a string because it can be
/// longer than the character.
#[must_use]
pub fn to_upper(c: char) -> Option<&'static str> {
    mapped(UPPER_MAP, c as u32)
}

/// The full lowercase mapping, **without** Final_Sigma, which needs the
/// characters around this one and is `tinker-pdf-layout`'s `case` to apply.
#[must_use]
pub fn to_lower(c: char) -> Option<&'static str> {
    mapped(LOWER_MAP, c as u32)
}

/// The full titlecase mapping. `ǆ` is `ǅ` and not `Ǆ`: three mappings and not
/// two, which is why `capitalize` is not `uppercase` on one letter.
#[must_use]
pub fn to_title(c: char) -> Option<&'static str> {
    mapped(TITLE_MAP, c as u32)
}

/// `Cased`, Unicode §3.13 D135.
#[must_use]
pub fn is_cased(c: char) -> bool {
    TABLES.cased.contains(c as u32)
}

/// `Case_Ignorable`, §3.13 D136.
#[must_use]
pub fn is_case_ignorable(c: char) -> bool {
    TABLES.case_ignorable.contains(c as u32)
}

/// `Lowercase`, from `DerivedCoreProperties.txt` — `Ll` and
/// `Other_Lowercase`, so `ª` is lowercase and `ʰ` is too.
#[must_use]
pub fn is_lowercase(c: char) -> bool {
    TABLES.lowercase.contains(c as u32)
}

/// General_Category `L*` or `N*`.
#[must_use]
pub fn is_letter_or_number(c: char) -> bool {
    TABLES.letter_or_number.contains(c as u32)
}

/// The character's Grapheme_Cluster_Break, UAX #29 Table 2.
#[must_use]
pub fn grapheme_break(c: char) -> Gcb {
    TABLES.grapheme_break.get(c as u32)
}

/// The character's `Indic_Conjunct_Break`, or `None` where it has none.
#[must_use]
pub fn indic_conjunct_break(c: char) -> Option<Incb> {
    TABLES.indic_conjunct_break.get(c as u32)
}

/// General_Category `M`: `Mn`, `Mc` or `Me`.
#[must_use]
pub fn is_mark(c: char) -> bool {
    TABLES.mark.contains(c as u32)
}
