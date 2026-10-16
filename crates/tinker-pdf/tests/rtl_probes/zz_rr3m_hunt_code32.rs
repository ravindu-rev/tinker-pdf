//! `zz_rr3m_hunt_code32` (hunt, K-11 round-4 review (hunt)): the books of
//! `tools/probes/rtl/probes/pdf/zz_rr3m_hunt_code32.rs`, which is the probe as it ran.
//!
//! Hunt probe: K-11 round-4 review, family code32.

use super::common::{rep, run_book, Case, AR, HE};

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

fn cases() -> Vec<Case> {
    // Right-to-left paragraphs whose first character past WinAnsiEncoding in
    // the face (the overflow font's code 32) is in a left-to-right slice:
    // Greek, Cyrillic, Latin Extended, Arabic-Indic and Extended
    // Arabic-Indic digits, U+2212, U+20AA, U+2030.
    let mut out: Vec<Case> = Vec::new();
    let foreign: [(&str, &str); 9] = [
        ("greek", "αβγ δεζ ηθ"),
        ("cyr", "абв где жз"),
        ("latext", "Łódź Kraków ő"),
        ("viet", "Nguyễn Thành"),
        ("an", "١٢٣ ٤٥٦ ٧٨"),
        ("ean", "۱۲۳ ۴۵۶ ۷۸"),
        ("minus", "−5 −7 −12"),
        ("shekel", "₪100 ₪20 ₪3"),
        ("arrows", "a → b ← c"),
    ];
    for (sc, lang, w) in [("he", "he", HE), ("ar", "ar", AR)] {
        let s = |i: usize| w[i % w.len()];
        for (fname, f) in foreign {
            let shapes: Vec<(&str, String)> = vec![
                (
                    "first",
                    format!(
                        "{f} {}",
                        rep(5, |i| format!("{} {} {f} {}.", s(i), s(i + 1), s(i + 2)))
                    ),
                ),
                (
                    "mid",
                    rep(5, |i| format!("{} {} {f} {}.", s(i), s(i + 1), s(i + 2))),
                ),
                (
                    "paren",
                    rep(5, |i| format!("{} ({f}), {} {}.", s(i), s(i + 1), s(i + 2))),
                ),
                (
                    "styled",
                    rep(5, |i| {
                        format!("{} <b>{f}</b> {} <i>{f}</i>.", s(i), s(i + 1))
                    }),
                ),
            ];
            for (name, text) in shapes {
                out.push((
                    format!("{sc}/{fname}/{name}/rtl"),
                    lang,
                    format!("<p dir=\"rtl\">{text}</p>"),
                ));
            }
        }
    }
    out
}

fn styles() -> Vec<String> {
    [
        "",
        "p { text-align: justify; margin: 0 30% }",
        "p { letter-spacing: 0.3em; text-align: justify; margin: 0 25% }",
        "p { word-spacing: 0.5em; margin: 0 20% }",
        "p { word-spacing: -2px; text-align: justify; margin: 0 30% }",
        "body { font-family: sans-serif } p { text-align: justify; margin: 0 33% }",
        "body { font-family: monospace } p { word-spacing: 4px; margin: 0 26% }",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

/// Every book of the family, as its probe's tests built them, in order.
pub(crate) fn run() {
    probe();
}
