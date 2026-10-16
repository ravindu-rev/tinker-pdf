//! `zz_rr3m_h1_gaps` (h1, round-5 review (hunt)): the books of
//! `tools/probes/rtl/probes/pdf/zz_rr3m_h1_gaps.rs`, which is the probe as it ran.
//!
//! Hunt probe: K-11/CD-19 round-5 hunt, family gaps.

use super::common::{halves, run_book, Case, AR, GR, HE};

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

// Family 6: gaps no spacing explains inside a line: padding, margin, borders
// and inline blocks, between words and inside them.

fn cases() -> Vec<Case> {
    let mut out: Vec<Case> = Vec::new();
    for (sc, lang, w) in [("ar", "ar", AR), ("he", "he", HE), ("gr", "el", GR)] {
        let s = |i: usize| w[i % w.len()];
        let (h0, h1) = halves(s(4));
        let shapes: Vec<(&str, String)> = vec![
            (
                "pad",
                format!(
                    "{} <span style=\"padding: 0 1em\">{}</span> {} {}.",
                    s(0),
                    s(1),
                    s(2),
                    s(3)
                ),
            ),
            (
                "padglue",
                format!(
                    "{}<span style=\"padding-left: 1em\">{}</span>{} {}.",
                    s(0),
                    s(1),
                    s(2),
                    s(3)
                ),
            ),
            (
                "marg",
                format!(
                    "{} <span style=\"margin: 0 0.75em\">{}</span> {} {}.",
                    s(0),
                    s(1),
                    s(2),
                    s(3)
                ),
            ),
            (
                "marg-in-word",
                format!(
                    "{} {h0}<span style=\"margin-left: 0.8em\">{h1}</span> {} {}.",
                    s(0),
                    s(2),
                    s(3)
                ),
            ),
            (
                "ib",
                format!(
                    "{} <span style=\"display: inline-block\">{} {}</span> {}.",
                    s(0),
                    s(1),
                    s(2),
                    s(3)
                ),
            ),
            (
                "ibpad",
                format!(
                    "{}<span style=\"display: inline-block; padding: 0 0.6em\">({})</span>{} {}.",
                    s(0),
                    s(1),
                    s(2),
                    s(3)
                ),
            ),
            (
                "border",
                format!(
                    "{} <span style=\"border: 2px solid black; padding: 0 4px\">{}</span> {} {}.",
                    s(0),
                    s(1),
                    s(2),
                    s(3)
                ),
            ),
            (
                "latin-ib",
                format!(
                    "{} <span style=\"display: inline-block; margin-left: 1em\">abc</span> {} {}.",
                    s(0),
                    s(1),
                    s(2)
                ),
            ),
            (
                "neutral-pad",
                format!(
                    "{} <span style=\"padding: 0 0.6em\">?!</span> {} {}.",
                    s(0),
                    s(1),
                    s(2)
                ),
            ),
            (
                "in-en",
                format!(
                    "abc <span style=\"padding-left: 1em\">{}</span> {} def.",
                    s(0),
                    s(1)
                ),
            ),
        ];
        for (name, text) in shapes {
            for (d, attr) in [("rtl", " dir=\"rtl\""), ("none", "")] {
                out.push((
                    format!("{sc}/{name}/{d}"),
                    lang,
                    format!("<p{attr}>{text}</p>"),
                ));
            }
        }
    }
    out
}

fn styles() -> Vec<String> {
    [
        "",
        "p { word-spacing: 0.25em }",
        "p { word-spacing: 0.5em }",
        "p { word-spacing: 1em }",
        "p { word-spacing: -2px }",
        "p { text-align: justify; margin: 0 38% }",
        "p { letter-spacing: 0.1em; word-spacing: 0.5em }",
        "p { letter-spacing: 0.5em; text-align: justify; margin: 0 25% }",
        "body { font-family: monospace } p { word-spacing: 0.5em }",
        "body { font-family: sans-serif } p { letter-spacing: 0.3em; word-spacing: 1em }",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

/// Every book of the family, as its probe's tests built them, in order.
pub(crate) fn run() {
    probe();
}
