//! `zz_v6_units` (v6, round 6 lane V (hunt)): the books of
//! `tools/probes/rtl/probes/pdf/zz_v6_units.rs`, which is the probe as it ran.

use super::common::{AR, Case, HE, rep, run_book};

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

/// Every book of the family, as its probe built them, in order.
pub(crate) fn run() {
    probe();
}

// Family units: word and letter spacing in every unit a stylesheet writes
// (px, pt, rem, ex, ch, a percentage), at and around the two thresholds the
// writer and the reader share (PARTED_FROM 0.45em, RESUMED_WITHIN 0.5em), as
// sums of letter and word spacing, and under each alignment: centre, right,
// end, text-align-last, a positive and a hanging indent.

const MIXW: [&str; 12] = ["שלום", "abc", "עולם", "(def)", "ספר", "12", "בית", "<i>ghi</i>", "אור", "?!", "יום", "25%"];

fn cases() -> Vec<Case> {
    let mut out: Vec<Case> = Vec::new();
    for (sc, lang, w) in [("ar", "ar", AR), ("he", "he", HE), ("mix", "he", MIXW)] {
        let s = |i: usize| w[i % w.len()];
        let shapes: Vec<(&str, Box<dyn Fn(usize) -> String>)> = vec![
            ("prose", Box::new(move |i| format!("{} {} {}، {} {} {}.", s(i), s(i + 1), s(i + 2), s(i + 3), s(i + 4), s(i + 5)))),
            ("styled", Box::new(move |i| format!("{} <b>{}</b> {} <i>{}</i>, ({}).", s(i), s(i + 1), s(i + 2), s(i + 3), s(i + 4)))),
            ("punct", Box::new(move |i| format!("{} <b>?!</b> {} {}. <i>(</i>{}<i>)</i> {}!", s(i), s(i + 1), s(i + 2), s(i + 3), s(i + 4)))),
            ("num", Box::new(move |i| format!("{} 25\u{66A} <b>{}</b> \u{661}\u{662}\u{663} {} 2010، {}.", s(i), s(i + 1), s(i + 2), s(i + 3)))),
            ("gloss", Box::new(move |i| format!("{} {} (lingua franca) {} <i>sic</i> {}.", s(i), s(i + 1), s(i + 2), s(i + 3)))),
        ];
        for (name, shape) in &shapes {
            let text = rep(3, |k| shape(k));
            for (d, attr) in [("rtl", " dir=\"rtl\""), ("none", "")] {
                out.push((format!("{sc}/{name}/{d}"), lang, format!("<p{attr}>{text}</p>")));
            }
        }
    }
    out
}

fn styles() -> Vec<String> {
    [
        "p { word-spacing: 8px }",
        "p { word-spacing: 6pt }",
        "p { word-spacing: 0.5rem }",
        "p { word-spacing: 1ex }",
        "p { word-spacing: 1ch }",
        "p { word-spacing: 50% }",
        "p { word-spacing: 0.44em }",
        "p { word-spacing: 0.45em }",
        "p { word-spacing: 0.46em }",
        "p { word-spacing: 0.4999em }",
        "p { word-spacing: 0.5em }",
        "p { letter-spacing: 0.15em; word-spacing: 0.3em }",
        "p { letter-spacing: 0.25em; word-spacing: 0.25em }",
        "p { letter-spacing: 2px; word-spacing: 0.4em }",
        "p { letter-spacing: 0.45em }",
        "p { letter-spacing: 0.46em }",
        "p { letter-spacing: 1pt; word-spacing: 4pt; text-align: justify }",
        "p { letter-spacing: 0.5em; word-spacing: -0.5em }",
        "p { letter-spacing: 0.3em; word-spacing: -0.2em }",
        "p { word-spacing: 0.6em; text-align: center }",
        "p { word-spacing: 0.6em; text-align: right }",
        "p { word-spacing: 0.6em; text-align: end }",
        "p { text-align: justify; text-align-last: justify; margin: 0 20% }",
        "p { text-align: justify; text-indent: 3em; margin: 0 20% }",
        "p { text-align: justify; text-indent: -2em; padding: 0 2em; margin: 0 20% }",
        "p { word-spacing: 1em; text-indent: 1.5em }",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}
