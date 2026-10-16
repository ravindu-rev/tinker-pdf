//! `zz_rr3m_h1_inline` (h1, round-5 review (hunt)): the books of
//! `tools/probes/rtl/probes/pdf/zz_rr3m_h1_inline.rs`, which is the probe as it ran.
//!
//! Hunt probe: K-11/CD-19 round-5 hunt, family inline.

use super::common::{rep, run_book, Case, AR, CY, GR, HE};

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

// Family 8: inline formatting no probe has tried under word spacing, with
// overflow-font text: lists, headings, size changes, sub/sup, bdi/bdo,
// text-transform, small-caps, pre-wrap runs of spaces, text-indent.

fn cases() -> Vec<Case> {
    let mut out: Vec<Case> = Vec::new();
    for (sc, lang, w) in [
        ("ar", "ar", AR),
        ("he", "he", HE),
        ("gr", "el", GR),
        ("cy", "ru", CY),
    ] {
        let s = |i: usize| w[i % w.len()];
        let shapes: Vec<(&str, String, bool)> = vec![
            // (name, body, wrap the body in <p>)
            (
                "ol",
                format!(
                    "<ol><li>{} {} {}.</li><li>{} ({}) {}.</li></ol>",
                    s(0),
                    s(1),
                    s(2),
                    s(3),
                    s(4),
                    s(5)
                ),
                false,
            ),
            (
                "h2",
                format!(
                    "<h2>{} {} {}</h2><p>{} {}.</p>",
                    s(0),
                    s(1),
                    s(2),
                    s(3),
                    s(4)
                ),
                false,
            ),
            (
                "big",
                format!(
                    "{} <span style=\"font-size: 150%\">{} {}</span> {} {}.",
                    s(0),
                    s(1),
                    s(2),
                    s(3),
                    s(4)
                ),
                true,
            ),
            (
                "small",
                format!(
                    "{} <small>{}</small> {} <small>{} {}</small>.",
                    s(0),
                    s(1),
                    s(2),
                    s(3),
                    s(4)
                ),
                true,
            ),
            (
                "sup",
                format!(
                    "{} {}<sup>12</sup> {} {}<sub>{}</sub> {}.",
                    s(0),
                    s(1),
                    s(2),
                    s(3),
                    s(4),
                    s(5)
                ),
                true,
            ),
            (
                "bdi",
                format!("{} <bdi>{} {}</bdi> abc {}.", s(0), s(1), s(2), s(3)),
                true,
            ),
            (
                "bdo",
                format!(
                    "{} <bdo dir=\"ltr\">{} {}</bdo> {}.",
                    s(0),
                    s(1),
                    s(2),
                    s(3)
                ),
                true,
            ),
            (
                "upper",
                format!(
                    "{} <span style=\"text-transform: uppercase\">{} {}</span> {}.",
                    s(0),
                    s(1),
                    s(2),
                    s(3)
                ),
                true,
            ),
            (
                "smallcaps",
                format!(
                    "{} <span style=\"font-variant: small-caps\">{} {}</span> {}.",
                    s(0),
                    s(1),
                    s(2),
                    s(3)
                ),
                true,
            ),
            (
                "prewrap",
                format!(
                    "<span style=\"white-space: pre-wrap\">{}   {}  {} {}.</span>",
                    s(0),
                    s(1),
                    s(2),
                    s(3)
                ),
                true,
            ),
            (
                "link",
                format!("{} <a href=\"#x\">{} {}</a> {}.", s(0), s(1), s(2), s(3)),
                true,
            ),
            (
                "indent",
                rep(3, |i| {
                    format!("{} {} {}, {}.", s(i), s(i + 1), s(i + 2), s(i + 3))
                }),
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
                out.push((format!("{sc}/{name}/{d}"), lang, book));
            }
        }
    }
    out
}

fn styles() -> Vec<String> {
    [
        "",
        "p, li, h2 { word-spacing: 0.5em }",
        "p, li, h2 { word-spacing: 1em }",
        "p, li, h2 { word-spacing: 2em }",
        "p, li, h2 { text-align: justify; text-indent: 2em; margin: 0 30% }",
        "p, li, h2 { letter-spacing: 0.1em; word-spacing: 0.6em }",
        "body { font-family: monospace } p, li, h2 { word-spacing: 0.5em; text-align: justify; margin: 0 25% }",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

/// Every book of the family, as its probe's tests built them, in order.
pub(crate) fn run() {
    probe();
}
