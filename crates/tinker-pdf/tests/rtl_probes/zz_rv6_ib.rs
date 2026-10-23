//! `zz_rv6_ib` (rv6, round 6 review (hunt)): the books of
//! `tools/probes/rtl/probes/pdf/zz_rv6_ib.rs`, which is the probe as it ran.

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

/// Every book of the family, as its probe built them, in order.
pub(crate) fn run() {
    probe();
}

// Family rv6_ib (round 6's independent review of ROADMAP CD-19's fix, its
// second hunt): the class of the first hunt's ar/mixib/none. Hebrew and Arabic
// paragraphs, right-to-left and with no dir: an inline block widened by
// scaleX(2) that holds a word and a styled `?!` (`wide`); a styled `?!` and, a
// word on, an inline block narrowed by scaleX(0.6), 0.7 or 0.8 that holds a
// word (`narrow`); the narrowed block holding a word and the `?!`
// (`narrowmark`); and the widened and the narrowed block in one line (`both`).
// Under no transform, a quarter turn and scaleX(2) on the paragraph, at word
// spacing 0.4em, 0.5em and 0.55em and letter spacing 0.4em.

fn cases() -> Vec<Case> {
    let mut out: Vec<Case> = Vec::new();
    for (sc, lang, w) in [("ar", "ar", AR), ("he", "he", HE)] {
        let s = |i: usize| w[i % w.len()];
        for narrow in ["0.6", "0.7", "0.8"] {
            let ib = |sx: &str, inner: String| {
                format!(
                    "<span style=\"display: inline-block; transform: scaleX({sx})\">{inner}</span>"
                )
            };
            let shapes: Vec<(&str, String)> = vec![
                (
                    "both",
                    rep(2, |i| {
                        format!(
                            "{} {} {} {} {}.",
                            s(i),
                            ib("2", format!("{} <b>?!</b>", s(i + 1))),
                            s(i + 2),
                            ib(narrow, s(i + 3).to_string()),
                            s(i + 4)
                        )
                    }),
                ),
                (
                    "narrow",
                    rep(2, |i| {
                        format!(
                            "{} {} <b>?!</b> {} {} {}.",
                            s(i),
                            s(i + 1),
                            s(i + 2),
                            ib(narrow, s(i + 3).to_string()),
                            s(i + 4)
                        )
                    }),
                ),
                (
                    "narrowmark",
                    rep(2, |i| {
                        format!(
                            "{} {} {} {}.",
                            s(i),
                            ib(narrow, format!("{} <b>?!</b>", s(i + 1))),
                            s(i + 2),
                            s(i + 3)
                        )
                    }),
                ),
                (
                    "wide",
                    rep(2, |i| {
                        format!(
                            "{} {} {} {} {}.",
                            s(i),
                            ib("2", format!("{} <b>?!</b>", s(i + 1))),
                            s(i + 2),
                            s(i + 3),
                            s(i + 4)
                        )
                    }),
                ),
            ];
            for (name, text) in &shapes {
                for (d, attr) in [("rtl", " dir=\"rtl\""), ("none", "")] {
                    out.push((
                        format!("{sc}/{name}{narrow}/{d}"),
                        lang,
                        format!("<div class=\"w\"><p{attr}>{text}</p></div>"),
                    ));
                }
            }
        }
    }
    out
}

fn styles() -> Vec<String> {
    let mut out = Vec::new();
    for p in ["none", "rotate(90deg)", "scaleX(2)"] {
        for s in [
            "word-spacing: 0.4em",
            "word-spacing: 0.5em",
            "word-spacing: 0.55em",
            "letter-spacing: 0.4em",
        ] {
            out.push(format!(
                "p {{ margin: 0 30%; transform: {p}; {s}; }} .w {{ transform: none }}"
            ));
        }
    }
    out
}
