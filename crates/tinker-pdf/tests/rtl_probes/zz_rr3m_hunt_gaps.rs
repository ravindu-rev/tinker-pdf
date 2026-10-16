//! `zz_rr3m_hunt_gaps` (hunt, K-11 round-4 review (hunt)): the books of
//! `tools/probes/rtl/probes/pdf/zz_rr3m_hunt_gaps.rs`, which is the probe as it ran.
//!
//! Hunt probe: K-11 round-4 review, family gaps.

use super::common::{rep, run_book, Case, AR, HEP};

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
    // Styled runs of neutrals between right-to-left words, at spacings about
    // the half-em a reader resumes a line within.
    let mut out: Vec<Case> = Vec::new();
    let pieces = [
        "?!", "!!", "...", "--", "*", "#", "%", "/", ";", ":", "«»", "()", "\"\"", "- -", "?! ?!",
        "١٢", "12",
    ];
    for (sc, lang, w) in [("hep", "he", HEP), ("ar", "ar", AR)] {
        let s = |i: usize| w[i % w.len()];
        for (pi, p) in pieces.iter().enumerate() {
            let text = rep(3, |i| {
                format!(
                    "{} <i>{p}</i> {} <b>{p}</b>{} {}.",
                    s(i + pi),
                    s(i + pi + 1),
                    s(i + pi + 2),
                    s(i + pi + 3)
                )
            });
            out.push((
                format!("{sc}/p{pi}/rtl"),
                lang,
                format!("<p dir=\"rtl\">{text}</p>"),
            ));
        }
    }
    out
}

fn styles() -> Vec<String> {
    let mut out = Vec::new();
    for ws in [
        "0.3em", "0.45em", "0.49em", "0.5em", "0.51em", "0.6em", "1em",
    ] {
        out.push(format!("p {{ word-spacing: {ws}; margin: 0 20% }}"));
    }
    for ls in ["0.2em", "0.25em", "0.3em", "0.49em", "0.5em"] {
        out.push(format!(
            "p {{ letter-spacing: {ls}; text-align: justify; margin: 0 25% }}"
        ));
    }
    out.push("p { letter-spacing: 0.2em; word-spacing: 0.3em; margin: 0 20% }".to_owned());
    out.push(
        "p { letter-spacing: 0.25em; word-spacing: 0.25em; text-align: justify; margin: 0 30% }"
            .to_owned(),
    );
    out.push("body { font-family: monospace } p { word-spacing: 0.5em; text-align: justify; margin: 0 25% }".to_owned());
    out
}

/// Every book of the family, as its probe's tests built them, in order.
pub(crate) fn run() {
    probe();
}
