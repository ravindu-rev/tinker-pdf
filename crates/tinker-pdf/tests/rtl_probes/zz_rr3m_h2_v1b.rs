//! `zz_rr3m_h2_v1b` (h2, CD-19 fix review (hunt)): the books of
//! `tools/probes/rtl/probes/pdf/zz_rr3m_h2_v1b.rs`, which is the probe as it ran.
//!
//! Hunt probe: CD-19 round-5 hunt 2, family v1b.

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

// Family v1b: V1, ruling 14's line-before exception, in shapes the earlier
// probes did not build: nested pairs, a pair holding a number or a
// left-to-right word, angle brackets, the ending mid-line, a soft wrap, and
// stops that are strong letters (ARABIC QUESTION MARK, ARABIC FULL STOP,
// HEBREW SOF PASUQ), with closing quotes ” » ’.

fn cases() -> Vec<Case> {
    let mut out: Vec<Case> = Vec::new();
    let stops: [(&str, &str); 9] = [
        ("dot", "."),
        ("dots", "..."),
        ("ell", "\u{2026}"),
        ("qe", "?!"),
        ("exex", "!!"),
        ("qq", "??"),
        ("arq", "\u{61F}"),
        ("urstop", "\u{6D4}"),
        ("sof", "\u{5C3}"),
    ];
    let closers: [(&str, &str); 4] = [
        ("none", ""),
        ("rdq", "\u{201D}"),
        ("raq", "\u{BB}"),
        ("rsq", "\u{2019}"),
    ];
    for (sc, lang, w) in [("ar", "ar", AR), ("he", "he", HE)] {
        let s = |i: usize| w[i % w.len()];
        let num = if sc == "ar" { "\u{661}\u{662}" } else { "12" };
        for (sn, st) in stops {
            for (cn, cl) in closers {
                let end = format!("{st}{cl}");
                let shapes: Vec<(&str, String)> = vec![
                    (
                        "nest/none",
                        format!("<p>{} {}<br/>(({})) {}{end}</p>", s(0), s(1), s(2), s(3)),
                    ),
                    (
                        "num/none",
                        format!("<p>{} {}<br/>({num}) {}{end}</p>", s(4), s(5), s(6)),
                    ),
                    (
                        "ltrin/none",
                        format!("<p>{} {}<br/>(abc {}) {}{end}</p>", s(7), s(8), s(9), s(10)),
                    ),
                    (
                        "angle/none",
                        format!(
                            "<p>{} {}<br/>\u{27E8}{}\u{27E9} {}{end}</p>",
                            s(11),
                            s(0),
                            s(1),
                            s(2)
                        ),
                    ),
                    (
                        "mid/none",
                        format!(
                            "<p>{} {}<br/>({}){end} {} {}</p>",
                            s(3),
                            s(4),
                            s(5),
                            s(6),
                            s(7)
                        ),
                    ),
                    (
                        "soft/none",
                        format!(
                            "<p>{} {} {} {} {} ({}) {}{end}</p>",
                            s(8),
                            s(9),
                            s(10),
                            s(11),
                            s(0),
                            s(1),
                            s(2)
                        ),
                    ),
                    (
                        "open/rtl",
                        format!(
                            "<p dir=\"rtl\">{} {}<br/>{end}<b>(</b>{}<b>)</b> {} {}</p>",
                            s(3),
                            s(4),
                            s(5),
                            s(6),
                            s(7)
                        ),
                    ),
                    (
                        "ltr/ltr",
                        format!(
                            "<p dir=\"ltr\">{} {}<br/>({}) [{}] {}{end}</p>",
                            s(8),
                            s(9),
                            s(10),
                            s(11),
                            s(0)
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
    ["", "p { word-spacing: 0.6em; margin: 0 25% }"]
        .iter()
        .map(|s| s.to_string())
        .collect()
}

/// Every book of the family, as its probe's tests built them, in order.
pub(crate) fn run() {
    probe();
}
