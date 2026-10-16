//! `zz_rr3m_hunt_punct` (hunt, K-11 round-4 review (hunt)): the books of
//! `tools/probes/rtl/probes/pdf/zz_rr3m_hunt_punct.rs`, which is the probe as it ran.
//!
//! Hunt probe: K-11 round-4 review, family punct.

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
                "qx-end",
                rep(6, |i| format!("{} {} {}?!", s(i), s(i + 1), s(i + 2))),
            ),
            (
                "qx-start",
                rep(6, |i| format!("?! {} {} {}.", s(i), s(i + 1), s(i + 2))),
            ),
            (
                "dots",
                rep(6, |i| {
                    format!(
                        "{} {}... {} …{} {}.",
                        s(i),
                        s(i + 1),
                        s(i + 2),
                        s(i + 3),
                        s(i + 4)
                    )
                }),
            ),
            (
                "dashes",
                rep(6, |i| {
                    format!(
                        "{} – {} — {}—{} –{}.",
                        s(i),
                        s(i + 1),
                        s(i + 2),
                        s(i + 3),
                        s(i + 4)
                    )
                }),
            ),
            (
                "arabic-q",
                rep(6, |i| {
                    format!(
                        "{} {}؟! {}؛ {}،، {}.",
                        s(i),
                        s(i + 1),
                        s(i + 2),
                        s(i + 3),
                        s(i + 4)
                    )
                }),
            ),
            (
                "quote-dot",
                rep(6, |i| {
                    format!(
                        "{} «{} {}». {} «{}.» {}",
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
                "colon-list",
                rep(6, |i| {
                    format!(
                        "{}: {}, {}; {}!! {}.",
                        s(i),
                        s(i + 1),
                        s(i + 2),
                        s(i + 3),
                        s(i + 4)
                    )
                }),
            ),
            (
                "lead-dash",
                rep(6, |i| {
                    format!("— {} {}. – {} {}?", s(i), s(i + 1), s(i + 2), s(i + 3))
                }),
            ),
            // Inline elements cutting a word.
            (
                "mid-b",
                rep(6, |i| {
                    let x = s(i);
                    let c: Vec<char> = x.chars().collect();
                    let k = c.len() / 2;
                    format!(
                        "{} {}<b>{}</b> {}.",
                        s(i + 1),
                        c[..k].iter().collect::<String>(),
                        c[k..].iter().collect::<String>(),
                        s(i + 2)
                    )
                }),
            ),
            (
                "mid-i3",
                rep(6, |i| {
                    let x = s(i);
                    let c: Vec<char> = x.chars().collect();
                    let n = c.len();
                    let a = n / 3;
                    let b = 2 * n / 3;
                    format!(
                        "{} {}<i>{}</i>{} {}.",
                        s(i + 1),
                        c[..a].iter().collect::<String>(),
                        c[a..b].iter().collect::<String>(),
                        c[b..].iter().collect::<String>(),
                        s(i + 2)
                    )
                }),
            ),
            (
                "mid-ls",
                rep(6, |i| {
                    let x = s(i);
                    let c: Vec<char> = x.chars().collect();
                    let k = c.len() / 2;
                    format!(
                        "{} <span style=\"letter-spacing: 0.4em\">{}</span>{} {}.",
                        s(i + 1),
                        c[..k].iter().collect::<String>(),
                        c[k..].iter().collect::<String>(),
                        s(i + 2)
                    )
                }),
            ),
            (
                "mid-ws",
                rep(6, |i| {
                    let x = s(i);
                    let c: Vec<char> = x.chars().collect();
                    let k = c.len() / 2;
                    format!(
                        "{} <span style=\"word-spacing: 1em\">{} {}</span>{} ({}).",
                        s(i + 1),
                        s(i + 3),
                        c[..k].iter().collect::<String>(),
                        c[k..].iter().collect::<String>(),
                        s(i + 2)
                    )
                }),
            ),
            (
                "mid-punct",
                rep(6, |i| {
                    let x = s(i);
                    let c: Vec<char> = x.chars().collect();
                    let k = c.len() / 2;
                    format!(
                        "{} ({}<b>{}</b>) {}<i>{}.</i>",
                        s(i + 1),
                        c[..k].iter().collect::<String>(),
                        c[k..].iter().collect::<String>(),
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
    }
    out
}

fn styles() -> Vec<String> {
    [
        "",
        "p { text-align: justify; margin: 0 30% }",
        "p { text-align: justify; margin: 0 41% }",
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
