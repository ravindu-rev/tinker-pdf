//! Unicode's own conformance file for extended grapheme clusters.
//!
//! `GraphemeBreakTest.txt`, vendored beside the property file it tests
//! (THIRDPARTY.md), 766 cases, each of which names every boundary in a short
//! string as `×` or `÷` — written by the people who wrote UAX #29, so it asks
//! the questions this crate's own tests would not think to.
//!
//! It drives [`boundaries`], the untailored algorithm that
//! [`tinker_pdf_layout::grapheme::units`] tailors once, and asserts that
//! tailoring touches no case but the ones it is for: a boundary before a
//! mark that GB999 decided.

use tinker_pdf_layout::grapheme::{boundaries, units};
use tinker_pdf_layout::unicode::{grapheme_break, is_mark, Gcb};

const CASES: &str = include_str!("../data/ucd/GraphemeBreakTest.txt");

/// One line of the file: the string, and the byte offsets of its boundaries.
struct Case {
    line: usize,
    text: String,
    breaks: Vec<usize>,
    source: String,
}

fn parse() -> Vec<Case> {
    let mut out = Vec::new();
    for (number, raw) in CASES.lines().enumerate() {
        let body = match raw.find('#') {
            Some(0) => continue,
            Some(at) => &raw[..at],
            None => raw,
        };
        let body = body.trim();
        if body.is_empty() {
            continue;
        }
        let mut text = String::new();
        let mut breaks = Vec::new();
        for token in body.split_whitespace() {
            match token {
                "\u{00f7}" => breaks.push(text.len()),
                "\u{00d7}" => {}
                hex => text.push(
                    u32::from_str_radix(hex, 16)
                        .ok()
                        .and_then(char::from_u32)
                        .unwrap_or_else(|| panic!("line {}: {hex} is not a character", number + 1)),
                ),
            }
        }
        out.push(Case {
            line: number + 1,
            text,
            breaks,
            source: body.to_owned(),
        });
    }
    out
}

/// Every case in Unicode's file, against the shipped algorithm.
#[test]
fn the_whole_of_unicodes_own_grapheme_break_test() {
    let cases = parse();
    let mut failures = Vec::new();
    for case in &cases {
        let ours = boundaries(&case.text);
        if ours != case.breaks {
            failures.push(format!(
                "line {}: {}\n  expected {:?}\n  ours     {:?}",
                case.line, case.source, case.breaks, ours
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "{} of {} conformance cases disagree:\n{}",
        failures.len(),
        cases.len(),
        failures
            .iter()
            .take(20)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n")
    );
    println!("RAN grapheme conformance: {} cases", cases.len());
}

/// **The tailoring takes away only boundaries before a mark**, and keeps
/// every one round a control: over every case of the file, a unit boundary
/// is a cluster boundary, and a cluster boundary the units lack is one
/// before a mark that follows no control.
#[test]
fn the_units_are_the_clusters_less_a_boundary_before_a_mark() {
    let mut taken = 0usize;
    for case in parse() {
        let clusters = boundaries(&case.text);
        let ours = units(&case.text);
        assert!(
            ours.iter().all(|at| clusters.contains(at)),
            "line {}: a unit boundary that is no cluster boundary",
            case.line
        );
        for at in clusters.iter().filter(|at| !ours.contains(at)) {
            let next = case.text[*at..].chars().next().expect("inside the text");
            let previous = case.text[..*at]
                .chars()
                .next_back()
                .expect("inside the text");
            assert!(
                is_mark(next)
                    && !matches!(grapheme_break(previous), Gcb::Control | Gcb::CR | Gcb::LF),
                "line {}: the units lost the boundary at {at}",
                case.line
            );
            taken += 1;
        }
    }
    assert!(taken > 0, "no case reaches the tailoring");
}

/// The file was parsed rather than skipped: a harness that parsed nothing
/// would pass the tests above with an empty loop.
#[test]
fn the_conformance_file_is_the_one_that_was_vendored() {
    assert!(
        CASES.starts_with("# GraphemeBreakTest-17.0.0.txt"),
        "the vendored conformance file is not the 17.0.0 one the table came from"
    );
    let total = CASES
        .lines()
        .filter(|l| !l.trim_start().starts_with('#') && !l.trim().is_empty())
        .count();
    assert_eq!(parse().len(), total);
    assert_eq!(total, 766, "the file's own count, its last lines say");
}
