//! `zz_rr3m_h2_gaps2` (h2, CD-19 fix review (hunt)): the books of
//! `tools/probes/rtl/probes/pdf/zz_rr3m_h2_gaps2.rs`, which is the probe as it ran.
//!
//! Hunt probe: CD-19 round-5 hunt 2, family gaps2.

use super::common::{halves, rep, run_book, Case, AR, GR, HE, HEP};

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

// Family gaps2: gaps inside a line that no word or letter spacing explains,
// of kinds earlier probes did not build: an empty inline-block, an EM SPACE,
// a THIN SPACE, a no-break space, a zero-width space, a ZWNJ, runs of spaces
// under pre-wrap, a hanging text-indent, padding on a bold word, a border on
// an italic one, letter-spacing or font-size changing inside a word, and a
// soft hyphen.

fn cases() -> Vec<Case> {
    let mut out: Vec<Case> = Vec::new();
    for (sc, lang, w) in [
        ("ar", "ar", AR),
        ("he", "he", HE),
        ("hep", "he", HEP),
        ("gr", "el", GR),
    ] {
        let s = |i: usize| w[i % w.len()];
        let (h0, h1) = halves(s(4));
        let shapes: Vec<(&str, String)> = vec![
            ("ibempty", format!("{} <span style=\"display: inline-block; width: 1.5em; height: 1em\"></span> {} {}.", s(0), s(1), s(2))),
            ("ibglued", format!("{}<span style=\"display: inline-block; width: 1em\"></span>{} {}.", s(0), s(1), s(2))),
            ("emsp", format!("{}\u{2003}{} {} {}.", s(0), s(1), s(2), s(3))),
            ("thinsp", format!("{}\u{2009}{} {}\u{2009}({}).", s(0), s(1), s(2), s(3))),
            ("nbsp", format!("{}\u{A0}{} {}\u{A0}{}.", s(0), s(1), s(2), s(3))),
            ("zwsp", format!("{}\u{200B}{} {} {}.", s(0), s(1), s(2), s(3))),
            ("zwnj", format!("{h0}\u{200C}{h1} {} {}.", s(1), s(2))),
            ("prewrap", format!("<span style=\"white-space: pre-wrap\">{}    {}  ({}) {}.</span>", s(0), s(1), s(2), s(3))),
            ("hang", rep(3, |i| format!("{} {} {}, {}.", s(i), s(i + 1), s(i + 2), s(i + 3)))),
            ("padb", format!("{} <b style=\"padding: 0 0.7em\">{}</b> {} {}.", s(0), s(1), s(2), s(3))),
            ("bordi", format!("{} <i style=\"border-left: 6px solid black\">{} {}</i> {}.", s(0), s(1), s(2), s(3))),
            ("lsword", format!("{} {h0}<span style=\"letter-spacing: 0.6em\">{h1}</span> {} {}.", s(0), s(1), s(2))),
            ("fsword", format!("{} {h0}<span style=\"font-size: 160%\">{h1}</span> {} {}.", s(0), s(1), s(2))),
            ("shy", format!("{} {h0}\u{AD}{h1} {} {}.", s(0), s(1), s(2))),
        ];
        for (name, text) in shapes {
            for (d, attr) in [("rtl", " dir=\"rtl\""), ("none", "")] {
                out.push((
                    format!("{sc}/{name}/{d}"),
                    lang,
                    format!("<p{attr}>{text}</p>"),
                ));
            }
        }
    }
    out
}

fn styles() -> Vec<String> {
    [
        "p { word-spacing: 0.6em; text-indent: -2em; padding-left: 2em }",
        "p { word-spacing: 11px; letter-spacing: 0.5pt }",
        "p { text-align: justify; margin: 0 37%; text-indent: 1em }",
        "p { letter-spacing: 0.3em; word-spacing: 0.4em }",
        "body { font-family: monospace } p { word-spacing: 1em; text-align: justify; margin: 0 20% }",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

/// Every book of the family, as its probe's tests built them, in order.
pub(crate) fn run() {
    probe();
}
