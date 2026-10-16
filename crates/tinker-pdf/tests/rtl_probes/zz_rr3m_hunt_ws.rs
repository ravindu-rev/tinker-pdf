//! `zz_rr3m_hunt_ws` (hunt, K-11 round-4 review (hunt)): the books of
//! `tools/probes/rtl/probes/pdf/zz_rr3m_hunt_ws.rs`, which is the probe as it ran.
//!
//! Hunt probe: K-11 round-4 review, family ws.

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
        let mut paras: Vec<(&str, String, String)> = Vec::new();
        // (name, paragraph style, text)
        for (ws_name, ws) in [
            ("pre", "white-space: pre"),
            ("prewrap", "white-space: pre-wrap"),
            ("nowrap", "white-space: nowrap"),
            ("preline", "white-space: pre-line"),
        ] {
            paras.push((
                ws_name,
                ws.to_owned(),
                rep(3, |i| {
                    format!(
                        "{} {}  ({})\n{}\t{}.",
                        s(i),
                        s(i + 1),
                        s(i + 2),
                        s(i + 3),
                        s(i + 4)
                    )
                }),
            ));
        }
        let shapes: Vec<(&str, String)> = vec![
            (
                "tabs",
                rep(5, |i| {
                    format!("{}\t{}\t(2010)\t{}.", s(i), s(i + 1), s(i + 2))
                }),
            ),
            (
                "shy",
                rep(5, |i| {
                    format!(
                        "{}\u{AD}{} {}\u{AD}{}\u{AD}{} ({}).",
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
                "nbsp",
                rep(5, |i| {
                    format!(
                        "{}\u{A0}{} ({}\u{A0}12) {}\u{A0}:",
                        s(i),
                        s(i + 1),
                        s(i + 2),
                        s(i + 3)
                    )
                }),
            ),
            (
                "nnbsp",
                rep(5, |i| {
                    format!(
                        "«\u{202F}{} {}\u{202F}» {}\u{202F}? {}.",
                        s(i),
                        s(i + 1),
                        s(i + 2),
                        s(i + 3)
                    )
                }),
            ),
            (
                "zwnj",
                rep(5, |i| {
                    format!(
                        "{}\u{200C}{} می\u{200C}خواهم {} ({}\u{200C}).",
                        s(i),
                        s(i + 1),
                        s(i + 2),
                        s(i + 3)
                    )
                }),
            ),
            (
                "zwj",
                rep(5, |i| {
                    format!(
                        "{}\u{200D} \u{200D}{} {}\u{200D}{} {}.",
                        s(i),
                        s(i + 1),
                        s(i + 2),
                        s(i + 3),
                        s(i + 4)
                    )
                }),
            ),
            (
                "zwsp",
                rep(5, |i| {
                    format!(
                        "{}\u{200B}{} ({})\u{200B}{}.",
                        s(i),
                        s(i + 1),
                        s(i + 2),
                        s(i + 3)
                    )
                }),
            ),
            (
                "wj",
                rep(5, |i| {
                    format!(
                        "{}\u{2060}{} {}\u{2060}. {}",
                        s(i),
                        s(i + 1),
                        s(i + 2),
                        s(i + 3)
                    )
                }),
            ),
            (
                "thin",
                rep(5, |i| {
                    format!(
                        "{}\u{2009}{} {}\u{2002}{} {}\u{3000}{}.",
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
                "br",
                rep(4, |i| {
                    format!(
                        "{} ({})<br/>{} {}.<br/>",
                        s(i),
                        s(i + 1),
                        s(i + 2),
                        s(i + 3)
                    )
                }),
            ),
        ];
        for (name, text) in shapes {
            paras.push((name, String::new(), text));
        }
        for (name, st, text) in paras {
            out.push((
                format!("{sc}/{name}/rtl"),
                lang,
                format!("<p dir=\"rtl\" style=\"{st}\">{text}</p>"),
            ));
            out.push((
                format!("{sc}/{name}/none"),
                lang,
                format!("<p style=\"{st}\">{text}</p>"),
            ));
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
