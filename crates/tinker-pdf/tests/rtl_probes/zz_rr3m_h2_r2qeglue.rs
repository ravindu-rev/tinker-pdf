//! `zz_rr3m_h2_r2qeglue` (h2, CD-19 fix review (hunt)): the books of
//! `tools/probes/rtl/probes/pdf/zz_rr3m_h2_r2qeglue.rs`, which is the probe as it ran.
//!
//! Hunt probe: CD-19 round-5 hunt 2, family r2qeglue.

use super::common::{run_book, Case};

fn probe() {
    let mut n = 0usize;
    for style in styles() {
        for (name, lang, body) in cases() {
            n += 1;
            let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                run_book(&style, &name, lang, &body)
            }));
            if r.is_err() {
                emit!("ROW\t{style}\t{name}\tm=0\ttext=0\tsearch=0/1\tltr=-\tcons=0\tlogical=0\tPANIC");
                emit!("PANIC\t{style}\t{name}");
            }
        }
    }
    emit!("books={n}");
}

// Family qeglue: breadth of N2, a styled run of neutrals glued to a
// right-to-left word, opening a right-to-left paragraph or in its middle,
// where the word's letters hold the overflow font's code-32 letter twice
// (first and last) or once, under word spacing.

fn cases() -> Vec<Case> {
    let mut out: Vec<Case> = Vec::new();
    let neutrals: [(&str, &str); 5] = [
        ("qe", "?!"),
        ("ex2", "!!"),
        ("dots", "..."),
        ("em", "\u{2014}"),
        ("raq", "\u{BB}"),
    ];
    // (script, lang, a word with its first letter again at its end, a word with it once, a second word)
    let words: [(&str, &'static str, &str, &str, &str); 3] = [
        (
            "ar",
            "ar",
            "\u{627}\u{628}\u{627}",
            "\u{627}\u{628}\u{648}",
            "\u{627}\u{645}\u{627}",
        ),
        (
            "he",
            "he",
            "\u{5D0}\u{5D1}\u{5D0}",
            "\u{5D0}\u{5D1}\u{5D9}",
            "\u{5D0}\u{5DE}\u{5D0}",
        ),
        (
            "fa",
            "fa",
            "\u{6A9}\u{6CC}\u{6A9}",
            "\u{6A9}\u{6CC}\u{645}",
            "\u{6A9}\u{648}\u{6A9}",
        ),
    ];
    for (sc, lang, twice, once, second) in words {
        for (nn, n) in neutrals {
            for (wn, w) in [("twice", twice), ("once", once)] {
                for (st, open, close) in
                    [("b", "<b>", "</b>"), ("i", "<i>", "</i>"), ("none", "", "")]
                {
                    let shapes: Vec<(&str, String)> = vec![
                        ("open", format!("{open}{n}{close}{w} {second}.")),
                        ("mid", format!("{second} {open}{n}{close}{w} {second}.")),
                    ];
                    for (name, text) in shapes {
                        for (d, attr) in [("rtl", " dir=\"rtl\""), ("none", "")] {
                            out.push((
                                format!("{sc}/{nn}/{wn}/{st}/{name}/{d}"),
                                lang,
                                format!("<p{attr}>{text}</p>"),
                            ));
                        }
                    }
                }
            }
        }
    }
    out
}

fn styles() -> Vec<String> {
    [
        "p { word-spacing: 0.3em }",
        "p { word-spacing: 0.6em }",
        "p { word-spacing: 1em }",
        "p { word-spacing: 2em }",
        "p { letter-spacing: 0.1em; word-spacing: 0.5em }",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

/// Every book of the family, as its probe's tests built them, in order.
pub(crate) fn run() {
    probe();
}
