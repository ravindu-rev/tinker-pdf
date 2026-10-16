//! `zz_rr3m_h2_pages` (h2, CD-19 fix review (hunt)): the books of
//! `tools/probes/rtl/probes/pdf/zz_rr3m_h2_pages.rs`, which is the probe as it ran.
//!
//! Hunt probe: CD-19 round-5 hunt 2, family pages.

use super::common::{rep, run_book, Case, AR, GR, HE, HEP};

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

// Family pages: measures and breaks no earlier probe built: two and three
// columns (a column's gap is one no spacing explains), a paragraph a page
// break cuts, a very narrow measure (a word or two a line), a very wide one
// at a small size, and a float beside the text.

const ARN: [&str; 12] = [
    "كتاب",
    "١٢٣",
    "الطالب",
    "(٤٥)",
    "بيت",
    "٢٠١٠،",
    "نور",
    "<b>٧٨٩</b>",
    "عوامل",
    "٣٫٥",
    "أصبحنا",
    "٦٠.",
];

const MIX: [&str; 12] = [
    "שלום",
    "abc",
    "עולם",
    "(def)",
    "ספר",
    "12",
    "בית",
    "<i>ghi</i>",
    "אור",
    "?!",
    "יום",
    "x\u{2011}y",
];

fn cases() -> Vec<Case> {
    let mut out: Vec<Case> = Vec::new();
    for (sc, lang, w) in [
        ("ar", "ar", AR),
        ("he", "he", HE),
        ("hep", "he", HEP),
        ("arn", "ar", ARN),
        ("mix", "he", MIX),
        ("gr", "el", GR),
    ] {
        let s = |i: usize| w[i % w.len()];
        let para = |n: usize| {
            rep(n, |i| {
                format!(
                    "{} {} <b>{}</b> {}, {} ({}) {}.",
                    s(i),
                    s(i + 1),
                    s(i + 2),
                    s(i + 3),
                    s(i + 4),
                    s(i + 5),
                    s(i + 6)
                )
            })
        };
        let shapes: Vec<(&str, String)> = vec![
            ("cols2", format!("<div style=\"column-count: 2; column-gap: 1em\">{{P}}{}{{/P}}</div>", para(4))),
            ("cols3", format!("<div style=\"column-count: 3; column-gap: 0\">{{P}}{}{{/P}}</div>", para(4))),
            ("pagebreak", format!("<div style=\"height: 640pt\"></div>{{P}}{}{{/P}}", para(6))),
            ("narrow", format!("<div style=\"width: 5em\">{{P}}{}{{/P}}</div>", para(2))),
            ("wide", format!("<div style=\"font-size: 7px\">{{P}}{}{{/P}}</div>", para(3))),
            ("float", format!("<div style=\"float: right; width: 30%; height: 4em; border: 1px solid black\"></div>{{P}}{}{{/P}}", para(2))),
        ];
        for (name, text) in shapes {
            for (d, attr) in [("rtl", " dir=\"rtl\""), ("none", "")] {
                let body = text
                    .replace("{P}", &format!("<p{attr}>"))
                    .replace("{/P}", "</p>");
                out.push((format!("{sc}/{name}/{d}"), lang, body));
            }
        }
    }
    out
}

fn styles() -> Vec<String> {
    [
        "p { word-spacing: 0.6em }",
        "p { text-align: justify }",
        "p { letter-spacing: 0.1em; word-spacing: 0.5em; text-align: justify }",
        "body { font-family: sans-serif } p { word-spacing: 1em; text-align: justify }",
        "p { word-spacing: 0.25em; orphans: 1; widows: 1 }",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

/// Every book of the family, as its probe's tests built them, in order.
pub(crate) fn run() {
    probe();
}
