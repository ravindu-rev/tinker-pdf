//! `zz_rr3m_h1_v1` (h1, round-5 review (hunt)): the books of
//! `tools/probes/rtl/probes/pdf/zz_rr3m_h1_v1.rs`, which is the probe as it ran.
//!
//! Hunt probe: K-11/CD-19 round-5 hunt, family v1.

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

// Family 5: V1, a line ending a sentence with stops and closing quotation
// marks after a bracket pair, two pairs, and the <br/> openings of the price.

fn cases() -> Vec<Case> {
    let mut out: Vec<Case> = Vec::new();
    let stops: [(&str, &str); 6] = [
        ("dot", "."),
        ("q", "?"),
        ("ell", "\u{2026}"),
        ("qe", "?!"),
        ("dotex", ".!"),
        ("exell", "!\u{2026}"),
    ];
    let closers: [(&str, &str); 5] = [
        ("none", ""),
        ("dq", "\""),
        ("rdq", "\u{201D}"),
        ("two", "\"\u{201D}"),
        ("paren", ")"),
    ];
    for (sc, lang, w) in [("ar", "ar", AR), ("he", "he", HE)] {
        let s = |i: usize| w[i % w.len()];
        for (sn, st) in stops {
            for (cn, cl) in closers {
                let end = format!("{st}{cl}");
                let shapes: Vec<(&str, String)> = vec![
                    (
                        "brk/none",
                        format!(
                            "<p>{} {} {}<br/><b>(</b>{}<b>)</b> {}{end}</p>",
                            s(0),
                            s(1),
                            s(2),
                            s(3),
                            s(4)
                        ),
                    ),
                    (
                        "two/none",
                        format!(
                            "<p>{} {}<br/>({}) {} [{}] {}{end}</p>",
                            s(5),
                            s(6),
                            s(7),
                            s(8),
                            s(9),
                            s(10)
                        ),
                    ),
                    (
                        "opens/rtl",
                        format!(
                            "<p dir=\"rtl\">{} {}<br/>{end}<b>(</b>{}<b>)</b> {} {}</p>",
                            s(0),
                            s(8),
                            s(3),
                            s(4),
                            s(11)
                        ),
                    ),
                    (
                        "ltr/ltr",
                        format!(
                            "<p dir=\"ltr\">{} {}<br/>({}) {}{end}</p>",
                            s(2),
                            s(3),
                            s(6),
                            s(7)
                        ),
                    ),
                ];
                for (name, body) in shapes {
                    out.push((format!("{sc}/{sn}-{cn}/{name}"), lang, body));
                }
            }
        }
    }
    out
}

fn styles() -> Vec<String> {
    [
        "",
        "p { word-spacing: 0.6em }",
        "p { letter-spacing: 0.5em; text-align: justify; margin: 0 25% }",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

/// Every book of the family, as its probe's tests built them, in order.
pub(crate) fn run() {
    probe();
}
