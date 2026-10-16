//! `zz_rr3m_h1_tables` (h1, round-5 review (hunt)): the books of
//! `tools/probes/rtl/probes/pdf/zz_rr3m_h1_tables.rs`, which is the probe as it ran.
//!
//! Hunt probe: K-11/CD-19 round-5 hunt, family tables.

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

// Family 2: table rows, right to left and with no dir, the cell before
// ending in a Latin word, a number, a neutral or right-to-left text.

fn cases() -> Vec<Case> {
    let mut out: Vec<Case> = Vec::new();
    // The cell before.
    let prev: [(&str, &str); 7] = [
        ("lat", "abc def"),
        ("num", "12 2010"),
        ("an", "\u{661}\u{662}\u{663}"),
        ("neu", "abc (x)."),
        (
            "grk",
            "\u{3B1}\u{3BB}\u{3C6}\u{3B1} \u{3B2}\u{3AE}\u{3C4}\u{3B1}",
        ),
        (
            "rtl-ar",
            "\u{643}\u{62A}\u{627}\u{628} \u{62C}\u{645}\u{64A}\u{644}",
        ),
        (
            "rtl-he",
            "\u{5E9}\u{5DC}\u{5D5}\u{5DD} \u{5E2}\u{5D5}\u{5DC}\u{5DD}",
        ),
    ];
    // The cell after.
    let next: [(&str, &'static str, &str); 6] = [
        (
            "ar",
            "ar",
            "\u{623}\u{635}\u{628}\u{62D}\u{646}\u{627} \u{646}\u{639}\u{631}\u{641}",
        ),
        ("he", "he", "\u{5E1}\u{5E4}\u{5E8} \u{5D1}\u{5D9}\u{5EA}"),
        (
            "hep",
            "he",
            "\u{5E1}\u{5B5}\u{5E4}\u{5B6}\u{5E8} \u{5D1}\u{5BC}\u{5B7}\u{5D9}\u{5B4}\u{5EA}",
        ),
        (
            "arnum",
            "ar",
            "\u{643}\u{62A}\u{627}\u{628} \u{661}\u{662} \u{646}\u{648}\u{631}",
        ),
        (
            "arneu",
            "ar",
            "(\u{643}\u{62A}\u{627}\u{628}) \u{646}\u{648}\u{631}.",
        ),
        (
            "grk",
            "el",
            "\u{3B1}\u{3BB}\u{3C6}\u{3B1} \u{3B3}\u{3AC}\u{3BC}\u{3BC}\u{3B1}",
        ),
    ];
    for (pn, p) in prev {
        for (nn, lang, n) in next {
            let rows =
                format!("<tr><td>{p}</td><td>{n}</td></tr><tr><td>{n}</td><td>{p}</td></tr>");
            for (d, attr) in [("rtl", " dir=\"rtl\""), ("none", "")] {
                out.push((
                    format!("{pn}-{nn}/{d}"),
                    lang,
                    format!("<table{attr}>{rows}</table>"),
                ));
            }
        }
    }
    out
}

fn styles() -> Vec<String> {
    [
        "",
        "table { border-spacing: 0 } td { padding: 0; word-spacing: 1em }",
        "table { border-spacing: 0 } td { padding: 0; word-spacing: 0.5em }",
        "table { border-spacing: 0.5em } td { padding: 0.25em; word-spacing: 1em }",
        "table { border-spacing: 0 } td { padding: 0 1em; word-spacing: 2em }",
        "table { border-spacing: 0 } td { padding: 0; letter-spacing: 0.1em; word-spacing: 0.5em }",
        "table { border-spacing: 0 } td { padding: 0; letter-spacing: 0.5em }",
        "table { border-spacing: 0 } td { padding: 0; word-spacing: -2px }",
        "body { font-family: monospace } table { border-spacing: 0 } td { padding: 0; word-spacing: 1em }",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

/// Every book of the family, as its probe's tests built them, in order.
pub(crate) fn run() {
    probe();
}
