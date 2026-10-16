//! `zz_rr3m_h2_r2ornate` (h2, CD-19 fix review (hunt)): the books of
//! `tools/probes/rtl/probes/pdf/zz_rr3m_h2_r2ornate.rs`, which is the probe as it ran.
//!
//! Hunt probe: CD-19 round-5 hunt 2, family r2ornate.

use super::common::{run_book, Case, AR, HE};

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

// Family ornate: breadth of N3, a bracket pair (ornate, mathematical angle,
// ASCII) round Arabic-Indic digits, European digits or a word, followed by a
// styled or plain word, opening a paragraph, after <br/>, or in a list item.

fn cases() -> Vec<Case> {
    let mut out: Vec<Case> = Vec::new();
    let pairs: [(&str, &str, &str); 3] = [
        ("ornate", "\u{FD3E}", "\u{FD3F}"),
        ("angle", "\u{27E8}", "\u{27E9}"),
        ("paren", "(", ")"),
    ];
    for (sc, lang, w) in [("ar", "ar", AR), ("he", "he", HE)] {
        let s = |i: usize| w[i % w.len()];
        let contents: [(&str, String); 3] = [
            ("an", "\u{662}\u{665}".to_owned()),
            ("en", "25".to_owned()),
            ("word", s(5).to_owned()),
        ];
        for (pn, o, c) in pairs {
            for (cn, inner) in &contents {
                for (an, after) in [
                    ("b", format!("<b>{}</b>", s(1))),
                    ("plain", s(1).to_owned()),
                ] {
                    let unit = format!("{o}{inner}{c} {after} {} {}.", s(2), s(3));
                    // (name, body, wrap in <p>)
                    let shapes: Vec<(&str, String, bool)> = vec![
                        ("open", unit.clone(), true),
                        ("br", format!("{} {}<br/>{unit}", s(0), s(8)), true),
                        ("li", format!("<ul><li>{unit}</li></ul>"), false),
                    ];
                    for (name, body, wrap) in shapes {
                        for (d, attr) in [("rtl", " dir=\"rtl\""), ("none", "")] {
                            let book = if wrap {
                                format!("<p{attr}>{body}</p>")
                            } else {
                                format!("<div{attr}>{body}</div>")
                            };
                            out.push((format!("{sc}/{pn}/{cn}/{an}/{name}/{d}"), lang, book));
                        }
                    }
                }
            }
        }
    }
    out
}

fn styles() -> Vec<String> {
    [
        "p, li { word-spacing: 0.5em }",
        "p, li { word-spacing: 0.75em }",
        "p, li { word-spacing: 1.5em }",
        "p, li { letter-spacing: 0.1em; word-spacing: 0.4em }",
        "p, li { text-align: justify; letter-spacing: 0.1em; word-spacing: 0.25em; margin: 0 30% }",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

/// Every book of the family, as its probe's tests built them, in order.
pub(crate) fn run() {
    probe();
}
