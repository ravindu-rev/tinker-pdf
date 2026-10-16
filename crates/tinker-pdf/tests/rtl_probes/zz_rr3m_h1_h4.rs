//! `zz_rr3m_h1_h4` (h1, round-5 review (hunt)): the books of
//! `tools/probes/rtl/probes/pdf/zz_rr3m_h1_h4.rs`, which is the probe as it ran.
//!
//! Hunt probe: K-11/CD-19 round-5 hunt, family h4.

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

// Family 3: H4 variants, a styled neutral glued to a word on one side or
// both, or opening the paragraph, Hebrew and Arabic.

fn cases() -> Vec<Case> {
    let mut out: Vec<Case> = Vec::new();
    let neutrals: [(&str, &str); 8] = [
        ("dot", "."),
        ("qe", "?!"),
        ("gq", "\u{BB},"),
        ("open", "("),
        ("hash", "#"),
        ("pct", "%"),
        ("dash", "-"),
        ("acomma", "\u{60C}"),
    ];
    for (sc, lang, w) in [("he", "he", HE), ("ar", "ar", AR)] {
        let s = |i: usize| w[i % w.len()];
        for (nn, n) in neutrals {
            let shapes: Vec<(&str, String)> = vec![
                (
                    "right",
                    rep(2, |i| {
                        format!("{} <b>{n}</b>{} {} {}.", s(i), s(i + 1), s(i + 2), s(i + 3))
                    }),
                ),
                (
                    "left",
                    rep(2, |i| {
                        format!("{}<b>{n}</b> {} {} {}.", s(i), s(i + 1), s(i + 2), s(i + 3))
                    }),
                ),
                (
                    "both",
                    rep(2, |i| {
                        format!("{}<i>{n}</i>{} {} {}.", s(i), s(i + 1), s(i + 2), s(i + 3))
                    }),
                ),
                (
                    "opens",
                    format!("<b>{n}</b>{} {} {} {}.", s(8), s(9), s(10), s(11)),
                ),
            ];
            for (name, text) in shapes {
                for (d, attr) in [("rtl", " dir=\"rtl\""), ("none", "")] {
                    out.push((
                        format!("{sc}/{nn}/{name}/{d}"),
                        lang,
                        format!("<p{attr}>{text}</p>"),
                    ));
                }
            }
        }
    }
    out
}

fn styles() -> Vec<String> {
    [
        "p { word-spacing: 0.3em; margin: 0 20% }",
        "p { word-spacing: 0.5em; margin: 0 20% }",
        "p { word-spacing: 0.6em; margin: 0 20% }",
        "p { word-spacing: 1em; margin: 0 20% }",
        "p { text-align: justify; margin: 0 35% }",
        "p { letter-spacing: 0.25em; word-spacing: 0.5em; margin: 0 20% }",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

/// Every book of the family, as its probe's tests built them, in order.
pub(crate) fn run() {
    probe();
}
