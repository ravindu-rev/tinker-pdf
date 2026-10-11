//! Unicode's own conformance file for word boundaries.
//!
//! `WordBreakTest.txt`, vendored beside the property files it tests
//! (THIRDPARTY.md): 1 944 cases, each of which names every position in a short
//! string as `×` (no boundary) or `÷` (a boundary).
//!
//! It is the assertion a space-scanner cannot satisfy, and it is vendored
//! rather than fetched for the reason `LineBreakTest.txt` is one crate over: a
//! skipped oracle exits 0 and reads exactly like a pass. It drives
//! [`word_boundaries`] — the function `TextLine::words` itself calls, so what
//! is proved conformant is the shipped path and not a copy of it.

use tinker_pdf_content::word_boundaries;

const CASES: &str = include_str!("../data/ucd/WordBreakTest.txt");

/// The number of data lines in the vendored 17.0.0 file, counted once and
/// pinned, so that a truncated or swapped file fails rather than passing
/// on fewer cases.
const EXPECTED_CASES: usize = 1_944;

/// One line of the file: the string, and the byte offsets it says are
/// boundaries.
struct Case {
    line: usize,
    text: String,
    boundaries: Vec<usize>,
    source: String,
}

fn parse() -> (Vec<Case>, usize) {
    let mut out = Vec::new();
    let mut unusable = 0usize;
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
        let mut boundaries = Vec::new();
        let mut usable = true;
        for token in body.split_whitespace() {
            match token {
                "\u{00f7}" => boundaries.push(text.len()),
                "\u{00d7}" => {}
                hex => match u32::from_str_radix(hex, 16).ok().and_then(char::from_u32) {
                    Some(ch) => text.push(ch),
                    // A lone surrogate is not a `char` and cannot be in a
                    // `&str`. Counted, and the count is asserted.
                    None => usable = false,
                },
            }
        }
        if !usable {
            unusable += 1;
            continue;
        }
        out.push(Case {
            line: number + 1,
            text,
            boundaries,
            source: body.to_string(),
        });
    }
    (out, unusable)
}

/// Every case in Unicode's file, against the shipped segmenter.
#[test]
fn the_whole_of_unicodes_own_word_break_test() {
    let (cases, unusable) = parse();
    assert_eq!(
        unusable, 0,
        "{unusable} cases name a code point a Rust string cannot hold"
    );
    assert_eq!(
        cases.len(),
        EXPECTED_CASES,
        "the vendored file parsed to {} cases, not the {EXPECTED_CASES} it holds",
        cases.len()
    );

    let mut failures = Vec::new();
    for case in &cases {
        let ours = word_boundaries(&case.text);
        if ours != case.boundaries {
            failures.push(format!(
                "line {}: {}\n  expected {:?}\n  ours     {:?}",
                case.line, case.source, case.boundaries, ours
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
    println!(
        "uax29-conformance: {} of {} cases pass",
        cases.len(),
        cases.len()
    );
}

/// The file is the 17.0.0 one the table was compiled from, and every data
/// line in it was parsed rather than skipped.
#[test]
fn the_conformance_file_is_the_one_that_was_vendored() {
    assert!(
        CASES.starts_with("# WordBreakTest-17.0.0.txt"),
        "the vendored conformance file is not the 17.0.0 one the tables came from"
    );
    let total = CASES
        .lines()
        .filter(|l| !l.trim_start().starts_with('#') && !l.trim().is_empty())
        .count();
    assert_eq!(total, EXPECTED_CASES);
}
