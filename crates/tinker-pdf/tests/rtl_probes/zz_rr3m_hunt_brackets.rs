//! `zz_rr3m_hunt_brackets` (hunt, K-11 round-4 review (hunt)): the books of
//! `tools/probes/rtl/probes/pdf/zz_rr3m_hunt_brackets.rs`, which is the probe as it ran.
//!
//! Hunt probe: K-11 round-4 review, family brackets.

use super::common::{rep, run_book, Case, Shape, AR, HEP};

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
        let shapes: Vec<Shape<'_>> = vec![
            (
                "nest-sq-pa-cu",
                Box::new(move |i| {
                    format!(
                        "{} [{} ({} {{{}}}) {}] {}.",
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
                "nest-tight",
                Box::new(move |i| {
                    format!("{} ({{[{}]}}) {} {}.", s(i), s(i + 1), s(i + 2), s(i + 3))
                }),
            ),
            (
                "guil-low9",
                Box::new(move |i| {
                    format!(
                        "{} «{} „{}“ {}» {}.",
                        s(i),
                        s(i + 1),
                        s(i + 2),
                        s(i + 3),
                        s(i + 4)
                    )
                }),
            ),
            (
                "single-guil",
                Box::new(move |i| {
                    format!(
                        "{} ‹{}› {} «{} {}» {}.",
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
                "cjk",
                Box::new(move |i| {
                    format!(
                        "{} 「{}」 {} 『{} {}』 {}.",
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
                "dq-sq",
                Box::new(move |i| {
                    format!(
                        "{} \"{} '{}' {}\" {}.",
                        s(i),
                        s(i + 1),
                        s(i + 2),
                        s(i + 3),
                        s(i + 4)
                    )
                }),
            ),
            (
                "curly-nest",
                Box::new(move |i| {
                    format!(
                        "{} “{} ‘{}’ {}” {}.",
                        s(i),
                        s(i + 1),
                        s(i + 2),
                        s(i + 3),
                        s(i + 4)
                    )
                }),
            ),
            (
                "rev-guil",
                Box::new(move |i| format!("{} »{} {}« {}.", s(i), s(i + 1), s(i + 2), s(i + 3))),
            ),
            (
                "ornate",
                Box::new(move |i| {
                    format!(
                        "{} \u{FD3F}{} {}\u{FD3E} {}.",
                        s(i),
                        s(i + 1),
                        s(i + 2),
                        s(i + 3)
                    )
                }),
            ),
            (
                "ornate-rev",
                Box::new(move |i| {
                    format!(
                        "{} \u{FD3E}{} {}\u{FD3F} {}.",
                        s(i),
                        s(i + 1),
                        s(i + 2),
                        s(i + 3)
                    )
                }),
            ),
            (
                "latin-nest",
                Box::new(move |i| format!("{} (HDAC [class I]) {} {}.", s(i), s(i + 1), s(i + 2))),
            ),
            (
                "num-nest",
                Box::new(move |i| format!("{} [(١٢٣)] {} ((2010)) {}.", s(i), s(i + 1), s(i + 2))),
            ),
            (
                "unbal",
                Box::new(move |i| {
                    format!(
                        "{} ({} {}. {} {}) {} ] {}.",
                        s(i),
                        s(i + 1),
                        s(i + 2),
                        s(i + 3),
                        s(i + 4),
                        s(i + 5),
                        s(i + 6)
                    )
                }),
            ),
            (
                "punct-after",
                Box::new(move |i| {
                    format!(
                        "{} ({}). {} ({})، {} «{}»؟ {} [{}]: {}",
                        s(i),
                        s(i + 1),
                        s(i + 2),
                        s(i + 3),
                        s(i + 4),
                        s(i + 5),
                        s(i + 6),
                        s(i + 7),
                        s(i + 8)
                    )
                }),
            ),
            (
                "empty",
                Box::new(move |i| {
                    format!("{} () {} [] {} «» {}.", s(i), s(i + 1), s(i + 2), s(i + 3))
                }),
            ),
            (
                "styled-nest",
                Box::new(move |i| {
                    format!(
                        "{} <b>(</b>{} <i>[{}]</i> {}<b>)</b> {}.",
                        s(i),
                        s(i + 1),
                        s(i + 2),
                        s(i + 3),
                        s(i + 4)
                    )
                }),
            ),
            (
                "cross-script",
                Box::new(move |i| format!("{} (שלום «سلام» {}) {}.", s(i), s(i + 1), s(i + 2))),
            ),
            (
                "q-latin-q",
                Box::new(move |i| {
                    format!(
                        "{} «HDAC» {} „SIRT“ {} 「x」 {}.",
                        s(i),
                        s(i + 1),
                        s(i + 2),
                        s(i + 3)
                    )
                }),
            ),
        ];
        for (name, shape) in &shapes {
            let text = rep(5, shape);
            out.push((
                format!("{sc}/{name}/rtl"),
                lang,
                format!("<p dir=\"rtl\">{text}</p>"),
            ));
            out.push((format!("{sc}/{name}/none"), lang, format!("<p>{text}</p>")));
            if sc == "hep" {
                out.push((
                    format!("{sc}/{name}/css"),
                    lang,
                    format!("<div style=\"direction: rtl\"><p>{text}</p></div>"),
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
