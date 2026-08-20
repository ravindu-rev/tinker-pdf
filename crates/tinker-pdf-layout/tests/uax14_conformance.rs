//! Unicode's own conformance file for the line breaking algorithm.
//!
//! `LineBreakTest.txt`, vendored beside the property files it tests
//! (THIRDPARTY.md), 19 338 cases, each of which names every boundary in a
//! short string as `×` or `÷`.
//!
//! **This is the assertion that cannot be satisfied by a space-scanner**, and
//! it is why the file is vendored rather than fetched or sampled. Gap 31's
//! risk table says a breaker done at spaces *"passes every English fixture,
//! works on Project Gutenberg's entire catalogue, and is catastrophically
//! wrong on CJK"*; the crate's own tests are written by the same author as the
//! crate and can only ask questions that author thought of, and this file was
//! written by the people who wrote the algorithm.
//!
//! It drives [`opportunities`] — the entry point a book goes through — at
//! [`Tailoring::UAX14`], which is `line-break: strict` and not a mode of its
//! own. A conformance run against a private code path proves that the private
//! code path is conformant.

use tinker_pdf_layout::uax14::{opportunities, Tailoring};

const CASES: &str = include_str!("../data/ucd/LineBreakTest.txt");

/// One line of the file: the string, and the byte offsets at which it says a
/// line may start.
struct Case {
    line: usize,
    text: String,
    breaks: Vec<usize>,
    source: String,
}

fn parse() -> Vec<Case> {
    let mut out = Vec::new();
    for (number, raw) in CASES.lines().enumerate() {
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
        let mut text = String::new();
        let mut breaks = Vec::new();
        let mut usable = true;
        for token in body.split_whitespace() {
            match token {
                // A break at the current end of the string. Position zero is
                // always `×`, so nothing is ever pushed for it.
                "\u{00f7}" => breaks.push(text.len()),
                "\u{00d7}" => {}
                hex => match u32::from_str_radix(hex, 16).ok().and_then(char::from_u32) {
                    Some(ch) => text.push(ch),
                    // A lone surrogate is not a `char` and cannot be in a
                    // `&str`. Counted rather than silently dropped, and the
                    // count is asserted below.
                    None => usable = false,
                },
            }
        }
        if !usable {
            continue;
        }
        out.push(Case {
            line: number + 1,
            text,
            breaks,
            source: body.to_string(),
        });
    }
    out
}

/// Every case in Unicode's file, against the shipped breaker.
#[test]
fn the_whole_of_unicodes_own_line_break_test() {
    let cases = parse();
    assert!(
        cases.len() > 19_000,
        "only {} cases parsed out of the vendored file, so most of it was not run",
        cases.len()
    );

    let mut failures = Vec::new();
    for case in &cases {
        let ours: Vec<usize> = opportunities(&case.text, Tailoring::UAX14)
            .into_iter()
            .map(|o| o.at)
            .collect();
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
}

/// The file was parsed rather than skipped.
///
/// Gap 20's finding, applied to a vendored oracle: a harness that quietly
/// parsed nothing would pass the test above with an empty loop, and would read
/// exactly like a run in which everything agreed.
#[test]
fn the_conformance_file_is_the_one_that_was_vendored() {
    assert!(
        CASES.starts_with("# LineBreakTest-17.0.0.txt"),
        "the vendored conformance file is not the 17.0.0 one the tables came from"
    );
    let cases = parse();
    let total = CASES
        .lines()
        .filter(|l| !l.trim_start().starts_with('#') && !l.trim().is_empty())
        .count();
    assert_eq!(
        cases.len(),
        total,
        "{} of {total} data lines could not be parsed",
        total - cases.len()
    );
}
