//! `zz_v6_auto` (v6, round 6 lane V (hunt)): the books of
//! `tools/probes/rtl/probes/pdf/zz_v6_auto.rs`, which is the probe as it ran.

use super::common::{run_book, Case, AR, HE, HEP};

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

/// Every book of the family, as its probe built them, in order.
pub(crate) fn run() {
    probe();
}

// Family auto: dir="auto" (a paragraph whose direction is its first strong
// character's) and unicode-bidi: plaintext, on paragraphs that open with a
// Latin word, a number, a bracket or a quotation before right-to-left text,
// and on a span inside a paragraph of the other direction.

fn cases() -> Vec<Case> {
    let mut out: Vec<Case> = Vec::new();
    for (sc, lang, w) in [("ar", "ar", AR), ("he", "he", HE), ("hep", "he", HEP)] {
        let s = |i: usize| w[i % w.len()];
        let openings: Vec<(&str, String)> = vec![
            (
                "rtl-first",
                format!("{} {} abc {} {}.", s(0), s(1), s(2), s(3)),
            ),
            (
                "latin-first",
                format!("abc {} {} <b>{}</b> {}، {}.", s(0), s(1), s(2), s(3), s(4)),
            ),
            (
                "num-first",
                format!("2010 {} {} {}؟ {}.", s(0), s(1), s(2), s(3)),
            ),
            (
                "an-first",
                format!("\u{661}\u{662} {} <i>{}</i> {}.", s(0), s(1), s(2)),
            ),
            (
                "bracket-first",
                format!("({}) {} {} {}!", s(0), s(1), s(2), s(3)),
            ),
            (
                "quote-first",
                format!("\u{ab}{} {}\u{bb} {} {}.", s(0), s(1), s(2), s(3)),
            ),
            ("punct-first", format!("?! {} {} {}.", s(0), s(1), s(2))),
            (
                "latin-mostly",
                format!("The word {} means {} in <i>{}</i>.", s(0), s(1), s(2)),
            ),
        ];
        for (name, text) in &openings {
            let text = format!("{text} {text}");
            out.push((
                format!("{sc}/{name}/auto"),
                lang,
                format!("<p dir=\"auto\">{text}</p>"),
            ));
            out.push((
                format!("{sc}/{name}/plaintext"),
                lang,
                format!("<p style=\"unicode-bidi: plaintext\">{text}</p>"),
            ));
            out.push((
                format!("{sc}/{name}/span-auto-in-ltr"),
                lang,
                format!("<p dir=\"ltr\">abc <span dir=\"auto\">{text}</span> def.</p>"),
            ));
            out.push((
                format!("{sc}/{name}/span-auto-in-rtl"),
                lang,
                format!(
                    "<p dir=\"rtl\">{} <span dir=\"auto\">{text}</span> {}.</p>",
                    s(5),
                    s(6)
                ),
            ));
            out.push((
                format!("{sc}/{name}/bdi-in-rtl"),
                lang,
                format!("<p dir=\"rtl\">{} <bdi>{text}</bdi> {}.</p>", s(7), s(8)),
            ));
        }
    }
    out
}

fn styles() -> Vec<String> {
    [
        "p { margin: 0 20% }",
        "p { word-spacing: 0.6em }",
        "p { word-spacing: 1em; margin: 0 20% }",
        "p { text-align: justify; margin: 0 25% }",
        "p { letter-spacing: 0.1em; word-spacing: 0.5em; text-align: justify }",
        "p { letter-spacing: 0.5em }",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}
