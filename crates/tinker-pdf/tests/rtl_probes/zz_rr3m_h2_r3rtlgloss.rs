//! `zz_rr3m_h2_r3rtlgloss` (h2, CD-19 fix review (hunt)): the books of
//! `tools/probes/rtl/probes/pdf/zz_rr3m_h2_r3rtlgloss.rs`, which is the probe as it ran.
//!
//! Hunt probe: CD-19 round-5 hunt 2, family r2rtlgloss.

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

// Family rtlgloss: the mirror of N5, a right-to-left paragraph of some forty
// words (Hebrew, Arabic) with left-to-right glosses (Latin, Greek, Cyrillic)
// in brackets and other shapes, justified at three measures or word-spaced.

fn cases() -> Vec<Case> {
    let mut out: Vec<Case> = Vec::new();
    let glosses: [(&str, &str); 6] = [
        ("paren-lat", "(peace)"),
        ("paren2-lat", "(world peace)"),
        ("ital-grk", "<i>(ειρήνη)</i>"),
        ("sq-cyr", "[мир]"),
        ("bold-lat", "<b>peace</b> (world)"),
        ("comma-lat", "(peace, world)"),
    ];
    for (sc, lang, w) in [("he", "he", HE), ("ar", "ar", AR)] {
        let s = |i: usize| w[i % w.len()];
        for (gn, g) in glosses {
            let mut words: Vec<String> = Vec::new();
            for i in 0..40 {
                words.push(s(i).to_owned());
                if i == 3 || i == 17 || i == 31 {
                    words.push(g.to_owned());
                }
            }
            let text = format!("{}.", words.join(" "));
            for (d, attr) in [("rtl", " dir=\"rtl\""), ("none", "")] {
                out.push((
                    format!("{sc}/{gn}/{d}"),
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
        "p { text-align: justify; margin: 0 30% }",
        "p { text-align: justify; margin: 0 38% }",
        "p { text-align: justify; margin: 0 10% }",
        "body { font-family: sans-serif } p { text-align: justify; margin: 0 33% }",
        "p { word-spacing: 0.5em }",
        "p { word-spacing: 0.75em; margin: 0 20% }",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

/// Every book of the family, as its probe's tests built them, in order.
pub(crate) fn run() {
    probe();
}
