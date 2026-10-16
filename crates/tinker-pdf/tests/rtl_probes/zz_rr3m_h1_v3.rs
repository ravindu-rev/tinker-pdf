//! `zz_rr3m_h1_v3` (h1, round-5 review (hunt)): the books of
//! `tools/probes/rtl/probes/pdf/zz_rr3m_h1_v3.rs`, which is the probe as it ran.
//!
//! Hunt probe: K-11/CD-19 round-5 hunt, family v3.

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

// Family 4: V3, neutrals past WinAnsiEncoding (and controls inside it)
// with numbers under word spacing.

fn cases() -> Vec<Case> {
    let mut out: Vec<Case> = Vec::new();
    for (sc, lang, w) in [("ar", "ar", AR), ("he", "he", HE)] {
        let s = |i: usize| w[i % w.len()];
        for (numname, n) in [("an", "\u{665}"), ("en", "25")] {
            let exprs: Vec<(&str, String)> = vec![
                ("pct", format!("{n}\u{66A}")),
                ("pct-pre", format!("\u{66A}{n}")),
                ("dec", format!("{n}\u{66B}{n}")),
                ("thou", format!("{n}\u{66C}{n}{n}")),
                ("acomma", format!("{n}\u{60C}")),
                ("ornate", format!("\u{FD3E}{n}\u{FD3F}")),
                ("angle", format!("\u{27E8}{n}\u{27E9}")),
                ("minus", format!("\u{2212}{n}")),
                ("shekel", format!("{n}\u{20AA}")),
                ("shekel-pre", format!("\u{20AA}{n}")),
                ("ctl-pct", format!("{n}%")),
            ];
            for (en, e) in &exprs {
                let shapes: Vec<(&str, String)> = vec![
                    (
                        "bold-after",
                        format!("{} {} {e} <b>{}</b> {}.", s(0), s(8), s(1), s(2)),
                    ),
                    (
                        "mid",
                        rep(2, |i| {
                            format!("{} {} {e} {} {}.", s(i), s(i + 1), s(i + 2), s(i + 3))
                        }),
                    ),
                ];
                for (name, text) in shapes {
                    for (d, attr) in [("rtl", " dir=\"rtl\""), ("none", "")] {
                        out.push((
                            format!("{sc}/{numname}/{en}/{name}/{d}"),
                            lang,
                            format!("<p{attr}>{text}</p>"),
                        ));
                    }
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
        "p { word-spacing: 1em; margin: 0 20% }",
        "p { letter-spacing: 0.1em; word-spacing: 0.25em; text-align: justify; margin: 0 35% }",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

/// Every book of the family, as its probe's tests built them, in order.
pub(crate) fn run() {
    probe();
}
