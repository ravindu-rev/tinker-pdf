//! `zz_fv_shape` (lines, 56dfd93 verification): the books of
//! `tools/probes/rtl/probes/shape/zz_fv_shape.rs`, which is the probe as it ran.
//!
//! bidi.rs's new unit test
//! `a_pair_before_a_full_stop_the_line_above_resolved_turns_no_line`, through
//! the crate's public API.

use tinker_pdf_shape::bidi::{drawn_direction, drawn_reading, logical_order, BaseDirection};

fn read_in(units: &[&str], order: &[usize]) -> String {
    order
        .iter()
        .filter_map(|at| units.get(*at).copied())
        .collect()
}

fn chars(text: &str) -> Vec<String> {
    text.chars().map(String::from).collect()
}

fn a_pair_before_a_full_stop_the_line_above_resolved_turns_no_line() {
    let ltr = BaseDirection::LeftToRight;
    let rtl = BaseDirection::RightToLeft;
    let line = "\u{645}\u{648}\u{64A} )\u{631}\u{648}\u{646}(.";
    let units = chars(line);
    let units: Vec<&str> = units.iter().map(String::as_str).collect();
    let theirs = read_in(&units, &logical_order(&units, rtl));
    assert!(
        theirs.contains("(\u{646}\u{648}\u{631})"),
        "read from the right, the line pairs the pair: {theirs:?}"
    );
    assert_eq!(drawn_direction(&units), ltr, "{line:?}");
    let read = drawn_reading(&units);
    assert!(!read.turned_by_brackets);
    let symbol = chars("\u{5D1})\u{5D0}(\u{2680}");
    let symbol: Vec<&str> = symbol.iter().map(String::as_str).collect();
    assert_eq!(drawn_direction(&symbol), rtl);
}

/// Probes for the before_its_punctuation shape: print the direction and the
/// reading of lines a hunter might worry about.
fn probe_lines() {
    for line in [
        // the fix's own line
        "\u{645}\u{648}\u{64A} )\u{631}\u{648}\u{646}(.",
        // with a trailing Arabic comma
        "\u{645}\u{648}\u{64A} )\u{631}\u{648}\u{646}(\u{60C}",
        // ARABIC FULL STOP U+06D4 is AL: not the tie-break's shape
        "\u{645}\u{648}\u{64A} )\u{631}\u{648}\u{646}(\u{6D4}",
        // a pair not at the edge
        "\u{645}\u{648}\u{64A} )\u{631}\u{648}\u{646}( \u{645}\u{648}\u{64A}.",
        // Hebrew
        "\u{5D9}\u{5D5}\u{5DD} )\u{5D1}\u{5D9}\u{5EA}(.",
        // with two marks at the end
        "\u{645}\u{648}\u{64A} )\u{631}\u{648}\u{646}(.\u{201D}",
        "\u{645}\u{648}\u{64A} )\u{631}\u{648}\u{646}(?!",
        // a quote ending
        "\u{645}\u{648}\u{64A} )\u{631}\u{648}\u{646}(\u{2026}",
    ] {
        let units = chars(line);
        let units: Vec<&str> = units.iter().map(String::as_str).collect();
        let read = drawn_reading(&units);
        let dir = drawn_direction(&units);
        let order = logical_order(&units, dir);
        emit!(
            "LINE\t{line:?}\t{dir:?}\tturned={}\t{:?}",
            read.turned_by_brackets,
            read_in(&units, &order)
        );
    }
}

/// Every book of the family, as its probe's tests built them, in order.
pub(crate) fn run() {
    a_pair_before_a_full_stop_the_line_above_resolved_turns_no_line();
    probe_lines();
}
