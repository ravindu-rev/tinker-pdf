//! `zz_rr3m_hunt_spacing` (hunt, K-11 round-4 review (hunt)): the books of
//! `tools/probes/rtl/probes/pdf/zz_rr3m_hunt_spacing.rs`, which is the probe as it ran.
//!
//! Hunt probe: K-11 round-4 review, family spacing.

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
    let mut out: Vec<Case> = Vec::new();
    for (sc, lang, w) in [("hep", "he", HEP), ("ar", "ar", AR)] {
        let s = |i: usize| w[i % w.len()];
        let shapes: Vec<(&str, String)> = vec![
            (
                "prose",
                rep(6, |i| {
                    format!(
                        "{} {} {}، {} {} {}.",
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
                "mixed",
                rep(6, |i| {
                    format!(
                        "{} {} (HDAC) {} 2010 {} «{}» {}.",
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
                "styled",
                rep(6, |i| {
                    format!(
                        "{} <b>{}</b> <i>?!</i> {} <b>(</b>{}<b>)</b> {}.",
                        s(i),
                        s(i + 1),
                        s(i + 2),
                        s(i + 3),
                        s(i + 4)
                    )
                }),
            ),
            (
                "neutrals",
                rep(6, |i| {
                    format!(
                        "{} - {} -- {} * {} # {} / {}.",
                        s(i),
                        s(i + 1),
                        s(i + 2),
                        s(i + 3),
                        s(i + 4),
                        s(i + 5)
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
    }
    out
}

fn styles() -> Vec<String> {
    let mut out = vec![String::new()];
    for ta in ["justify", "start", "end", "center"] {
        out.push(format!("p {{ text-align: {ta}; margin: 0 28% }}"));
    }
    for ti in ["2em", "-2em", "10%"] {
        out.push(format!(
            "p {{ text-indent: {ti}; text-align: justify; margin: 0 28% }}"
        ));
    }
    for ws in ["-0.2em", "-2px", "0.5em", "2em"] {
        out.push(format!("p {{ word-spacing: {ws}; margin: 0 24% }}"));
        out.push(format!(
            "p {{ word-spacing: {ws}; text-align: justify; margin: 0 32% }}"
        ));
    }
    for ls in ["-0.1em", "-1px", "0.05em", "0.2em", "0.45em", "0.7em"] {
        out.push(format!("p {{ letter-spacing: {ls}; margin: 0 24% }}"));
        out.push(format!(
            "p {{ letter-spacing: {ls}; text-align: justify; margin: 0 32% }}"
        ));
    }
    for m in ["0 44%", "0 46%"] {
        out.push(format!("p {{ text-align: justify; margin: {m} }}"));
    }
    out.push(
        "p { letter-spacing: 0.2em; word-spacing: -0.2em; text-align: justify; margin: 0 30% }"
            .to_owned(),
    );
    out.push("body { font-family: monospace } p { letter-spacing: 0.1em; text-align: justify; margin: 0 30% }".to_owned());
    out.push("body { font-family: sans-serif } p { word-spacing: 0.5em; text-align: end; margin: 0 30% }".to_owned());
    out
}

/// Every book of the family, as its probe's tests built them, in order.
pub(crate) fn run() {
    probe();
}
