//! `zz_rr3m_h2_h4b` (h2, CD-19 fix review (hunt)): the books of
//! `tools/probes/rtl/probes/pdf/zz_rr3m_h2_h4b.rs`, which is the probe as it ran.
//!
//! Hunt probe: CD-19 round-5 hunt 2, family h4b.

use super::common::{nsm, rep, run_book, Case, AR, HE};

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

// Family h4b: H4's styled neutrals glued to a word, in shapes no earlier
// probe built: nested styles, a superscript, a link, a coloured span,
// opening a list item or a figure caption; Hebrew, Arabic and Syriac; with
// the letter beside the neutral first met in a preface (so it is the
// overflow font's code 32) or not.

const SY: [&str; 12] = [
    "ܫܠܡܐ",
    "ܥܠܡܐ",
    "ܟܬܒܐ",
    "ܒܝܬܐ",
    "ܪܒܐ",
    "ܢܘܗܪܐ",
    "ܝܘܡܐ",
    "ܡܕܝܢܬܐ",
    "ܐܠܗܐ",
    "ܐܪܥܐ",
    "ܫܡܝܐ",
    "ܡܠܬܐ",
];

fn first_letter(w: &str) -> char {
    w.chars().next().unwrap_or('x')
}

fn last_letter(w: &str) -> char {
    w.chars().rfind(|c| !nsm(*c)).unwrap_or('x')
}

fn cases() -> Vec<Case> {
    let mut out: Vec<Case> = Vec::new();
    let neutrals: [(&str, &str); 5] = [
        ("dot", "."),
        ("qe", "?!"),
        ("guil", "\u{AB}"),
        ("emdash", "\u{2014}"),
        ("slash", "/"),
    ];
    for (sc, lang, w) in [("he", "he", HE), ("ar", "ar", AR), ("syr", "syr", SY)] {
        let s = |i: usize| w[i % w.len()];
        for (nn, n) in neutrals {
            // (name, body, wrap in <p>, the letter beside the neutral)
            let shapes: Vec<(&str, String, bool, String)> = vec![
                (
                    "nestR",
                    rep(2, |i| {
                        format!(
                            "{} <b><i>{n}</i></b>{} {} {}.",
                            s(i),
                            s(i + 1),
                            s(i + 2),
                            s(i + 3)
                        )
                    }),
                    true,
                    first_letter(s(1)).to_string(),
                ),
                (
                    "supR",
                    rep(2, |i| {
                        format!(
                            "{} <sup>{n}</sup>{} {} {}.",
                            s(i),
                            s(i + 1),
                            s(i + 2),
                            s(i + 3)
                        )
                    }),
                    true,
                    first_letter(s(1)).to_string(),
                ),
                (
                    "linkL",
                    rep(2, |i| {
                        format!(
                            "{}<a href=\"#x\">{n}</a> {} {} {}.",
                            s(i),
                            s(i + 1),
                            s(i + 2),
                            s(i + 3)
                        )
                    }),
                    true,
                    last_letter(s(0)).to_string(),
                ),
                (
                    "colorB",
                    rep(2, |i| {
                        format!(
                            "{}<span style=\"color: red\">{n}</span>{} {} {}.",
                            s(i),
                            s(i + 1),
                            s(i + 2),
                            s(i + 3)
                        )
                    }),
                    true,
                    format!("{}{}", last_letter(s(0)), first_letter(s(1))),
                ),
                (
                    "li",
                    format!(
                        "<ul><li><b>{n}</b>{} {} {} {}.</li><li>{} <b>{n}</b>{} {}.</li></ul>",
                        s(8),
                        s(9),
                        s(10),
                        s(11),
                        s(0),
                        s(1),
                        s(2)
                    ),
                    false,
                    first_letter(s(8)).to_string(),
                ),
                (
                    "figcap",
                    format!(
                        "<figure><figcaption><i>{n}</i>{} {} {} {} {}.</figcaption></figure>",
                        s(8),
                        s(9),
                        s(10),
                        s(11),
                        s(4)
                    ),
                    false,
                    first_letter(s(8)).to_string(),
                ),
            ];
            for (name, body, wrap, adj) in shapes {
                for (pname, preface) in [("nat", String::new()), ("adj", format!("<p>{adj}</p>"))] {
                    for (d, attr) in [("rtl", " dir=\"rtl\""), ("none", "")] {
                        let book = if wrap {
                            format!("{preface}<p{attr}>{body}</p>")
                        } else {
                            format!("{preface}<div{attr}>{body}</div>")
                        };
                        out.push((format!("{sc}/{nn}/{name}/{pname}/{d}"), lang, book));
                    }
                }
            }
        }
    }
    out
}

fn styles() -> Vec<String> {
    [
        "p, li, figcaption { word-spacing: 0.6em }",
        "p, li, figcaption { word-spacing: 14px; margin: 0 15% }",
        "p, li, figcaption { text-align: justify; letter-spacing: 0.02em; margin: 0 33% }",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

/// Every book of the family, as its probe's tests built them, in order.
pub(crate) fn run() {
    probe();
}
