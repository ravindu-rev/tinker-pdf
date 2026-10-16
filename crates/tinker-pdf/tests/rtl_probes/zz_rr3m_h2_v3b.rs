//! `zz_rr3m_h2_v3b` (h2, CD-19 fix review (hunt)): the books of
//! `tools/probes/rtl/probes/pdf/zz_rr3m_h2_v3b.rs`, which is the probe as it ran.
//!
//! Hunt probe: CD-19 round-5 hunt 2, family v3b.

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

// Family v3b: V3's neutrals after numbers in contexts no earlier probe
// built: a list item, a table cell, a heading, a line after <br/>, before a
// nested bold-italic word, inside a superscript; Persian digits besides
// European and Arabic-Indic; signs ٪ ٫ ٬ ، ﴾﴿ ⟨⟩ − ₪ ‱ ₹.

fn cases() -> Vec<Case> {
    let mut out: Vec<Case> = Vec::new();
    let nums: [(&str, &str); 3] = [
        ("en", "25"),
        ("an", "\u{662}\u{665}"),
        ("fa", "\u{6F2}\u{6F5}"),
    ];
    let signs: [(&str, &str, &str); 9] = [
        ("pct", "", "\u{66A}"),
        ("dec", "", "\u{66B}5"),
        ("thou", "", "\u{66C}000"),
        ("acomma", "", "\u{60C}"),
        ("ornate", "\u{FD3E}", "\u{FD3F}"),
        ("angle", "\u{27E8}", "\u{27E9}"),
        ("minus", "\u{2212}", ""),
        ("shekel", "", "\u{20AA}"),
        ("rupee", "\u{20B9}", ""),
    ];
    for (sc, lang, w) in [("ar", "ar", AR), ("he", "he", HE)] {
        let s = |i: usize| w[i % w.len()];
        for (numname, n) in nums {
            for (en, pre, post) in signs {
                let e = format!("{pre}{n}{post}");
                // (name, body, wrap in <p>)
                let shapes: Vec<(&str, String, bool)> = vec![
                    (
                        "li",
                        format!(
                            "<ol><li>{} {} {e} <b>{}</b> {}.</li><li>{e} {} {}.</li></ol>",
                            s(0),
                            s(8),
                            s(1),
                            s(2),
                            s(3),
                            s(4)
                        ),
                        false,
                    ),
                    (
                        "td",
                        format!(
                            "<table><tr><td>{} {e} <b>{}</b></td><td>{e} {}</td></tr></table>",
                            s(0),
                            s(1),
                            s(2)
                        ),
                        false,
                    ),
                    (
                        "br",
                        format!(
                            "{} {}<br/>{e} <b>{}</b> {} {}.",
                            s(0),
                            s(8),
                            s(1),
                            s(2),
                            s(9)
                        ),
                        true,
                    ),
                    (
                        "bi",
                        format!(
                            "{} {} {e} <b><i>{}</i></b> {}, {} {e}.",
                            s(0),
                            s(8),
                            s(1),
                            s(2),
                            s(3)
                        ),
                        true,
                    ),
                ];
                for (name, body, wrap) in shapes {
                    for (d, attr) in [("rtl", " dir=\"rtl\""), ("none", "")] {
                        let book = if wrap {
                            format!("<p{attr}>{body}</p>")
                        } else {
                            format!("<div{attr}>{body}</div>")
                        };
                        out.push((format!("{sc}/{numname}/{en}/{name}/{d}"), lang, book));
                    }
                }
            }
        }
    }
    out
}

fn styles() -> Vec<String> {
    [
        "p, li, td { word-spacing: 0.75em }",
        "p, li, td { word-spacing: 16px; letter-spacing: 1px }",
        "p, li, td { text-align: justify; letter-spacing: 0.1em; word-spacing: 0.25em; margin: 0 30% }",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

/// Every book of the family, as its probe's tests built them, in order.
pub(crate) fn run() {
    probe();
}
