//! `zz_rr3m_hunt_rtlpunct` (hunt, K-11 round-4 review (hunt)): the books of
//! `tools/probes/rtl/probes/pdf/zz_rr3m_hunt_rtlpunct.rs`, which is the probe as it ran.
//!
//! Hunt probe: K-11 round-4 review, family rtlpunct.

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
    // The right-to-left scripts' own punctuation (General_Category P* in
    // their blocks), which the device now takes to read right to left.
    let mut out: Vec<Case> = Vec::new();
    for (sc, lang, w) in [("he", "he", HE), ("ar", "ar", AR)] {
        let s = |i: usize| w[i % w.len()];
        let shapes: Vec<(&str, String)> = vec![
            (
                "ar-pct",
                rep(5, |i| {
                    format!(
                        "{} ٪٥٠ {} ٥٠٪ {} (٪١٢) {}.",
                        s(i),
                        s(i + 1),
                        s(i + 2),
                        s(i + 3)
                    )
                }),
            ),
            (
                "ar-dec",
                rep(5, |i| {
                    format!(
                        "{} ٣٫١٤ {} ١٬٠٠٠ {} ٫٥ {}.",
                        s(i),
                        s(i + 1),
                        s(i + 2),
                        s(i + 3)
                    )
                }),
            ),
            (
                "ar-date",
                rep(5, |i| {
                    format!("{} ٢٠١٠؍٥؍٣ {} {}.", s(i), s(i + 1), s(i + 2))
                }),
            ),
            (
                "ar-stops",
                rep(5, |i| {
                    format!("{}۔ {}؞ {}؛ {}؟", s(i), s(i + 1), s(i + 2), s(i + 3))
                }),
            ),
            (
                "quran",
                rep(4, |i| {
                    format!(
                        "{} ﴿{} {}﴾ [{}: ٢٥٥] {} \u{06DD}١٢ {}.",
                        s(i),
                        s(i + 1),
                        s(i + 2),
                        s(i + 3),
                        s(i + 4),
                        s(i + 5)
                    )
                }),
            ),
            (
                "he-gersh",
                rep(5, |i| {
                    format!(
                        "{} צה״ל {} ג׳ {} {}׳ {}.",
                        s(i),
                        s(i + 1),
                        s(i + 2),
                        s(i + 3),
                        s(i + 4)
                    )
                }),
            ),
            (
                "he-maqaf",
                rep(5, |i| {
                    format!(
                        "{}־{} {} {}׃ {}׀ {}.",
                        s(i),
                        s(i + 1),
                        s(i + 2),
                        s(i + 3),
                        s(i + 4),
                        s(i + 5)
                    )
                }),
            ),
            (
                "he-ascii-gersh",
                rep(5, |i| {
                    format!(
                        "{} צה\"ל {} ג' {} {}' {}.",
                        s(i),
                        s(i + 1),
                        s(i + 2),
                        s(i + 3),
                        s(i + 4)
                    )
                }),
            ),
            (
                "syr-punct",
                rep(5, |i| {
                    format!(
                        "{}܀ {}܁ {} ܂{} {}.",
                        s(i),
                        s(i + 1),
                        s(i + 2),
                        s(i + 3),
                        s(i + 4)
                    )
                }),
            ),
            (
                "nko-punct",
                rep(5, |i| {
                    format!("{}߸ {}߹ {} ߷ {}.", s(i), s(i + 1), s(i + 2), s(i + 3))
                }),
            ),
            (
                "pct-latin",
                rep(5, |i| {
                    format!(
                        "{} ٪HDAC {} 5٪ {} ٪ {}.",
                        s(i),
                        s(i + 1),
                        s(i + 2),
                        s(i + 3)
                    )
                }),
            ),
            (
                "styled-pct",
                rep(5, |i| {
                    format!(
                        "{} <b>٪</b>٥٠ {} <i>٫</i> {} <b>﴿</b>{}<b>﴾</b>.",
                        s(i),
                        s(i + 1),
                        s(i + 2),
                        s(i + 3)
                    )
                }),
            ),
        ];
        for (name, text) in shapes {
            out.push((
                format!("{sc}/{name}/rtl"),
                lang,
                format!("<p dir=\"rtl\">{text}</p>"),
            ));
            out.push((format!("{sc}/{name}/none"), lang, format!("<p>{text}</p>")));
        }
        out.push((
            format!("{sc}/en-pct"),
            "en",
            format!(
                "<p>{}</p>",
                rep(5, |i| format!(
                    "The rate {} ٪٥٠ and ({} ٥٠٪) here,",
                    s(i),
                    s(i + 1)
                ))
            ),
        ));
        out.push((
            format!("{sc}/en-quran"),
            "en",
            format!(
                "<p>{}</p>",
                rep(5, |i| format!(
                    "The verse ﴿{} {}﴾ is cited,",
                    s(i),
                    s(i + 1)
                ))
            ),
        ));
    }
    out
}

fn styles() -> Vec<String> {
    [
        "",
        "p { text-align: justify; margin: 0 30% }",
        "p { text-align: justify; margin: 0 41% }",
        "p { letter-spacing: 0.3em; text-align: justify; margin: 0 25% }",
        "p { letter-spacing: 0.5em; margin: 0 20% }",
        "p { word-spacing: 1em; margin: 0 20% }",
        "body { font-family: sans-serif } p { text-align: justify; margin: 0 33% }",
        "body { font-family: monospace } p { margin: 0 26%; text-align: justify }",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

/// Every book of the family, as its probe's tests built them, in order.
pub(crate) fn run() {
    probe();
}
