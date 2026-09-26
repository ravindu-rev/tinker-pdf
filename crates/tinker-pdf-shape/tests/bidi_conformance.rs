//! Unicode's own conformance files for the bidirectional algorithm.
//!
//! `BidiTest.txt` and `BidiCharacterTest.txt`, vendored beside the property
//! files they test (THIRDPARTY.md), and run through
//! [`tinker_pdf_shape::bidi::Paragraph`] — **the same entry point a consumer
//! calls**, which is the whole point. `crates/tinker-pdf-layout/tests/
//! uax14_conformance.rs` says it in one line about UAX #14 and it is just as
//! true here: *a conformance run against a private code path proves that the
//! private code path is conformant.*
//!
//! # Why both files, and what each one can say that the other cannot
//!
//! `BidiCharacterTest.txt` states its cases as **characters** — real code
//! points, a paragraph direction, and the resolved levels and visual order.
//! It is the file that covers bracket pairs, because bracket pairing is a
//! property of characters and not of classes.
//!
//! `BidiTest.txt` states its cases as **`Bidi_Class` sequences**, which is why
//! it can be exhaustive: every combination of classes up to length four, which
//! is 490 846 data lines and three paragraph directions apiece. Its own usage
//! note tells an implementation that takes characters what to do — *"randomly
//! pick characters from those with the same Bidi_Class values"* — so this file
//! picks one representative per class and asserts, against the same vendored
//! property table the crate compiled, that the representative really has that
//! class. A representative that drifted would otherwise turn the whole
//! exhaustive half into a test of something else.
//!
//! The file also says in as many words that it *"is assumed that no bidi
//! paired brackets exist in the input"*, so the `ON` representative is chosen
//! to not be one, and that is asserted rather than assumed.
//!
//! # What is asserted beyond "the cases pass"
//!
//! A suite whose case count can shrink silently is not a suite
//! (`docs/verification.md`). Both files pin:
//!
//! - the version in the header, so the data and the tables cannot drift apart;
//! - how many data lines were parsed, against how many the file holds, so a
//!   parser that quietly skipped nine tenths of it cannot read as a pass;
//! - how many individual resolutions ran, which for `BidiTest.txt` is larger
//!   than the line count because each line names up to three paragraph
//!   directions.

use std::collections::BTreeMap;

use tinker_pdf_shape::bidi::{BaseDirection, Level, Paragraph};
use tinker_pdf_shape::unicode::{bidi_class, bracket, BidiClass};

const BIDI_TEST: &str = include_str!("../data/ucd/BidiTest.txt");
const CHARACTER_TEST: &str = include_str!("../data/ucd/BidiCharacterTest.txt");

/// One character standing for each `Bidi_Class`, for `BidiTest.txt`.
///
/// Every one of these is checked against [`bidi_class`] before a single case
/// runs — see [`the_representatives_really_have_the_classes_they_stand_for`] —
/// so this table cannot silently become a table of something else.
const REPRESENTATIVES: &[(BidiClass, char)] = &[
    (BidiClass::L, 'a'),
    (BidiClass::R, '\u{05D0}'),
    (BidiClass::AL, '\u{0627}'),
    (BidiClass::EN, '1'),
    (BidiClass::ES, '+'),
    (BidiClass::ET, '#'),
    (BidiClass::AN, '\u{0660}'),
    (BidiClass::CS, ','),
    (BidiClass::NSM, '\u{0300}'),
    (BidiClass::BN, '\u{00AD}'),
    (BidiClass::B, '\u{2029}'),
    (BidiClass::S, '\u{0009}'),
    (BidiClass::WS, ' '),
    // Not a bracket, deliberately: `BidiTest.txt` states that its expectations
    // assume no bracket pairs are present.
    (BidiClass::ON, '!'),
    (BidiClass::LRE, '\u{202A}'),
    (BidiClass::RLE, '\u{202B}'),
    (BidiClass::PDF, '\u{202C}'),
    (BidiClass::LRO, '\u{202D}'),
    (BidiClass::RLO, '\u{202E}'),
    (BidiClass::LRI, '\u{2066}'),
    (BidiClass::RLI, '\u{2067}'),
    (BidiClass::FSI, '\u{2068}'),
    (BidiClass::PDI, '\u{2069}'),
];

fn representative(name: &str) -> char {
    let class = class_named(name);
    REPRESENTATIVES
        .iter()
        .find(|(c, _)| *c == class)
        .map(|(_, ch)| *ch)
        .unwrap_or_else(|| panic!("no representative for {name}"))
}

/// The abbreviation `BidiTest.txt` writes, as a class.
///
/// Exhaustive rather than defaulting: a class name this crate has never heard
/// of stops the run, for the same reason `build.rs` emits enum variants.
fn class_named(name: &str) -> BidiClass {
    match name {
        "AL" => BidiClass::AL,
        "AN" => BidiClass::AN,
        "B" => BidiClass::B,
        "BN" => BidiClass::BN,
        "CS" => BidiClass::CS,
        "EN" => BidiClass::EN,
        "ES" => BidiClass::ES,
        "ET" => BidiClass::ET,
        "FSI" => BidiClass::FSI,
        "L" => BidiClass::L,
        "LRE" => BidiClass::LRE,
        "LRI" => BidiClass::LRI,
        "LRO" => BidiClass::LRO,
        "NSM" => BidiClass::NSM,
        "ON" => BidiClass::ON,
        "PDF" => BidiClass::PDF,
        "PDI" => BidiClass::PDI,
        "R" => BidiClass::R,
        "RLE" => BidiClass::RLE,
        "RLI" => BidiClass::RLI,
        "RLO" => BidiClass::RLO,
        "S" => BidiClass::S,
        "WS" => BidiClass::WS,
        other => panic!("BidiTest.txt names a class this crate does not know: {other}"),
    }
}

/// What one case expects: a level per character, `None` where UAX #9 assigns
/// none, and the visual order of the rest.
struct Expected {
    levels: Vec<Option<u8>>,
    order: Vec<usize>,
}

/// Whether a case would still pass against an implementation that has **no
/// bidirectional algorithm at all**.
///
/// `tests/aots.rs` set the discipline and `tests/text_rendering.rs` follows it:
/// report `(cases, discriminating cases)`, the second counting only the cases
/// whose expected output differs from what the naivest available
/// implementation produces. Here that implementation is the one every
/// single-script engine already is — every character at level 0, drawn in
/// logical order, nothing removed. A case the baseline already satisfies is
/// one that would stay green with `src/bidi.rs` deleted.
fn discriminating(expected: &Expected) -> bool {
    let flat = expected.levels.iter().all(|level| *level == Some(0));
    let identity: Vec<usize> = (0..expected.levels.len()).collect();
    !flat || expected.order != identity
}

/// The levels and order this crate produced for one paragraph, in the shape
/// the files state them.
fn run(text: &str, direction: BaseDirection) -> (Level, Vec<Option<u8>>, Vec<usize>) {
    let paragraph = Paragraph::new(text, direction);
    let line = paragraph.line(0..paragraph.len());
    let levels = (0..paragraph.len())
        .map(|at| {
            if paragraph.is_removed(at) {
                None
            } else {
                Some(line.levels()[at].number())
            }
        })
        .collect();
    (paragraph.base_level(), levels, line.visual_order().to_vec())
}

fn check(
    case: &str,
    text: &str,
    direction: BaseDirection,
    expected: &Expected,
    failures: &mut Vec<String>,
) {
    let (_, levels, order) = run(text, direction);
    if levels != expected.levels || order != expected.order {
        failures.push(format!(
            "{case}\n  expected levels {:?} order {:?}\n  ours     levels {levels:?} order {order:?}",
            expected.levels, expected.order
        ));
    }
}

#[test]
fn the_representatives_really_have_the_classes_they_stand_for() {
    for (class, c) in REPRESENTATIVES {
        assert_eq!(
            bidi_class(*c),
            *class,
            "U+{:04X} is not {class:?}",
            u32::from(*c)
        );
    }
    assert_eq!(REPRESENTATIVES.len(), 23, "a Bidi_Class went missing");
    let names: Vec<BidiClass> = REPRESENTATIVES.iter().map(|(c, _)| *c).collect();
    let mut sorted = names.clone();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(sorted.len(), names.len(), "two classes share a row");
    // `BidiTest.txt` assumes no bracket pairs, so the neutral representative
    // must not be one.
    for (_, c) in REPRESENTATIVES {
        assert!(
            bracket(*c).is_none(),
            "U+{:04X} is a paired bracket, which BidiTest.txt says its \
             expectations assume away",
            u32::from(*c)
        );
    }
}

/// `BidiTest.txt`: every combination of `Bidi_Class` values up to length four.
#[test]
fn the_whole_of_unicodes_own_bidi_test() {
    assert!(
        BIDI_TEST.starts_with("# BidiTest-17.0.0.txt"),
        "the vendored conformance file is not the 17.0.0 one the tables came from"
    );

    let mut levels: Vec<Option<u8>> = Vec::new();
    let mut order: Vec<usize> = Vec::new();
    let mut failures: Vec<String> = Vec::new();
    let mut lines = 0usize;
    let mut ran = 0usize;
    let mut discriminating_lines = 0usize;

    for (number, raw) in BIDI_TEST.lines().enumerate() {
        let raw = raw.trim_end_matches('\r');
        let body = match raw.find('#') {
            Some(0) => continue,
            Some(at) => &raw[..at],
            None => raw,
        };
        let body = body.trim();
        if body.is_empty() {
            continue;
        }
        if let Some(rest) = body.strip_prefix("@Levels:") {
            levels = rest
                .split_whitespace()
                .map(|token| {
                    if token == "x" {
                        None
                    } else {
                        Some(token.parse::<u8>().expect("a level"))
                    }
                })
                .collect();
            continue;
        }
        if let Some(rest) = body.strip_prefix("@Reorder:") {
            order = rest
                .split_whitespace()
                .map(|token| token.parse::<usize>().expect("an index"))
                .collect();
            continue;
        }
        if body.starts_with('@') {
            // The file reserves any other `@` line for forward compatibility
            // and says to ignore it.
            continue;
        }
        let Some((classes, bitset)) = body.split_once(';') else {
            continue;
        };
        lines += 1;
        let text: String = classes.split_whitespace().map(representative).collect();
        let bitset = u8::from_str_radix(bitset.trim(), 16).expect("a hex bitset");
        let expected = Expected {
            levels: levels.clone(),
            order: order.clone(),
        };
        if discriminating(&expected) {
            discriminating_lines += 1;
        }
        assert_eq!(
            expected.levels.len(),
            classes.split_whitespace().count(),
            "line {}: the @Levels line does not describe this case",
            number + 1
        );
        for (bit, direction) in [
            (1u8, BaseDirection::Auto),
            (2, BaseDirection::LeftToRight),
            (4, BaseDirection::RightToLeft),
        ] {
            if bitset & bit == 0 {
                continue;
            }
            ran += 1;
            if failures.len() < 20 {
                check(
                    &format!("line {}: {body} [{direction:?}]", number + 1),
                    &text,
                    direction,
                    &expected,
                    &mut failures,
                );
            } else {
                let (_, ours, visual) = run(&text, direction);
                if ours != expected.levels || visual != expected.order {
                    failures.push(String::new());
                }
            }
        }
    }

    assert_eq!(
        lines, 490_846,
        "BidiTest.txt changed size; if that is intended, this number moves with it"
    );
    // Larger than the line count because a line's bitset names up to three
    // paragraph directions, and smaller than three times it because most name
    // fewer.
    assert_eq!(ran, 770_241, "the number of resolutions that ran moved");
    // (cases, discriminating cases), in `tests/aots.rs`'s discipline: the
    // second counts only the lines whose expected answer an implementation
    // with no bidi algorithm at all would get wrong. See [`discriminating`].
    assert_eq!(
        (lines, discriminating_lines),
        (490_846, 470_208),
        "the (cases, discriminating cases) pair for BidiTest.txt moved"
    );
    assert!(
        failures.is_empty(),
        "{} of {ran} BidiTest.txt resolutions disagree:\n{}",
        failures.len(),
        failures
            .iter()
            .filter(|f| !f.is_empty())
            .take(20)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n")
    );
}

/// `BidiCharacterTest.txt`: real code points, including bracket pairs.
#[test]
fn the_whole_of_unicodes_own_bidi_character_test() {
    assert!(
        CHARACTER_TEST.starts_with("# BidiCharacterTest-17.0.0.txt"),
        "the vendored conformance file is not the 17.0.0 one the tables came from"
    );

    let mut failures: Vec<String> = Vec::new();
    let mut ran = 0usize;
    let mut with_brackets = 0usize;
    let mut discriminating_cases = 0usize;
    for (number, raw) in CHARACTER_TEST.lines().enumerate() {
        let raw = raw.trim_end_matches('\r');
        let body = match raw.find('#') {
            Some(0) => continue,
            Some(at) => &raw[..at],
            None => raw,
        };
        let body = body.trim();
        if body.is_empty() {
            continue;
        }
        let fields: Vec<&str> = body.split(';').collect();
        assert_eq!(fields.len(), 5, "line {}: {body}", number + 1);
        let text: String = fields[0]
            .split_whitespace()
            .map(|hex| {
                let code = u32::from_str_radix(hex, 16).expect("a code point");
                char::from_u32(code).expect("BidiCharacterTest.txt holds no surrogates")
            })
            .collect();
        let direction = match fields[1].trim() {
            "0" => BaseDirection::LeftToRight,
            "1" => BaseDirection::RightToLeft,
            "2" => BaseDirection::Auto,
            other => panic!("line {}: paragraph direction {other}", number + 1),
        };
        let base: u8 = fields[2].trim().parse().expect("a paragraph level");
        let expected = Expected {
            levels: fields[3]
                .split_whitespace()
                .map(|token| {
                    if token == "x" {
                        None
                    } else {
                        Some(token.parse::<u8>().expect("a level"))
                    }
                })
                .collect(),
            order: fields[4]
                .split_whitespace()
                .map(|token| token.parse::<usize>().expect("an index"))
                .collect(),
        };
        if text.chars().any(|c| bracket(c).is_some()) {
            with_brackets += 1;
        }
        if discriminating(&expected) {
            discriminating_cases += 1;
        }
        ran += 1;
        let (ours_base, levels, order) = run(&text, direction);
        if ours_base.number() != base {
            failures.push(format!(
                "line {}: {body}\n  paragraph level {base}, ours {}",
                number + 1,
                ours_base.number()
            ));
        } else if levels != expected.levels || order != expected.order {
            failures.push(format!(
                "line {}: {body}\n  expected levels {:?} order {:?}\n  ours     levels \
                 {levels:?} order {order:?}",
                number + 1,
                expected.levels,
                expected.order
            ));
        }
    }

    assert_eq!(
        ran, 91_707,
        "BidiCharacterTest.txt changed size; if that is intended, this number moves with it"
    );
    assert_eq!(
        (ran, discriminating_cases),
        (91_707, 83_031),
        "the (cases, discriminating cases) pair for BidiCharacterTest.txt moved"
    );
    // The file is the only one of the two that reaches rule N0 at all, so a
    // version that stopped carrying brackets would silently delete this
    // crate's only evidence for it.
    assert!(
        with_brackets > 40_000,
        "only {with_brackets} of {ran} cases hold a paired bracket, so N0 is barely covered"
    );
    assert!(
        failures.is_empty(),
        "{} of {ran} BidiCharacterTest.txt cases disagree:\n{}",
        failures.len(),
        failures
            .iter()
            .take(20)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n")
    );
}

/// The two files were parsed rather than skipped.
///
/// Gap 20's finding, applied to a vendored oracle a third and fourth time: a
/// harness that quietly parsed nothing passes both tests above with an empty
/// loop, and reads exactly like a run in which everything agreed.
#[test]
fn the_conformance_files_were_read_in_full() {
    let data = |text: &str| {
        text.lines()
            .filter(|l| {
                let l = l.trim();
                !l.starts_with('#') && !l.starts_with('@') && !l.is_empty()
            })
            .count()
    };
    assert_eq!(data(BIDI_TEST), 490_846);
    assert_eq!(data(CHARACTER_TEST), 91_707);

    // And the class names the file uses are the twenty-three this crate has,
    // with none left over on either side.
    let mut seen: BTreeMap<BidiClass, usize> = BTreeMap::new();
    for line in BIDI_TEST.lines() {
        let line = line.trim();
        if line.starts_with('#') || line.starts_with('@') || line.is_empty() {
            continue;
        }
        let Some((classes, _)) = line.split_once(';') else {
            continue;
        };
        for name in classes.split_whitespace() {
            *seen.entry(class_named(name)).or_default() += 1;
        }
    }
    assert_eq!(
        seen.len(),
        23,
        "BidiTest.txt exercises only {} of the twenty-three classes",
        seen.len()
    );
}
