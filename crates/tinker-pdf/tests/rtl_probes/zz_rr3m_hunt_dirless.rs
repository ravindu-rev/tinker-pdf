//! `zz_rr3m_hunt_dirless` (hunt, K-11 round-4 review (hunt)): the books of
//! `tools/probes/rtl/probes/pdf/zz_rr3m_hunt_dirless.rs`, which is the probe as it ran.
//!
//! Hunt probe: K-11 round-4 review, family dirless.

use super::common::{rep, run_book, Case, Shape, AR, HE, HEP};

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

fn cases() -> Vec<Case> {
    // Dir-less paragraphs of right-to-left words whose wrapped lines end a
    // sentence after a bracket pair, styled apart or not.
    let mut out: Vec<Case> = Vec::new();
    for (sc, lang, w) in [("ar", "ar", AR), ("he", "he", HE), ("hep", "he", HEP)] {
        let s = |i: usize| w[i % w.len()];
        let shapes: Vec<Shape<'_>> = vec![
            (
                "b-paren",
                Box::new(move |i| {
                    format!(
                        "{} {} {} <b>(</b>{}<b>)</b> {}.",
                        s(i),
                        s(i + 1),
                        s(i + 2),
                        s(i + 3),
                        s(i + 4)
                    )
                }),
            ),
            (
                "paren",
                Box::new(move |i| {
                    format!(
                        "{} {} {} ({}) {}.",
                        s(i),
                        s(i + 1),
                        s(i + 2),
                        s(i + 3),
                        s(i + 4)
                    )
                }),
            ),
            (
                "i-guil",
                Box::new(move |i| {
                    format!(
                        "{} {} {} <i>«</i>{}<i>»</i> {}.",
                        s(i),
                        s(i + 1),
                        s(i + 2),
                        s(i + 3),
                        s(i + 4)
                    )
                }),
            ),
            (
                "b-sq-q",
                Box::new(move |i| {
                    format!(
                        "{} {} {} <b>[</b>{}<b>]</b> {}?",
                        s(i),
                        s(i + 1),
                        s(i + 2),
                        s(i + 3),
                        s(i + 4)
                    )
                }),
            ),
            (
                "b-paren-bang",
                Box::new(move |i| {
                    format!(
                        "{} {} <b>(</b>{} {}<b>)</b> {}!",
                        s(i),
                        s(i + 1),
                        s(i + 2),
                        s(i + 3),
                        s(i + 4)
                    )
                }),
            ),
            (
                "whole-b",
                Box::new(move |i| {
                    format!(
                        "{} {} {} <b>({})</b> {}.",
                        s(i),
                        s(i + 1),
                        s(i + 2),
                        s(i + 3),
                        s(i + 4)
                    )
                }),
            ),
        ];
        for (name, shape) in &shapes {
            out.push((
                format!("{sc}/{name}/one"),
                lang,
                format!("<p>{}</p>", shape(0)),
            ));
            out.push((
                format!("{sc}/{name}/three"),
                lang,
                format!("<p>{}</p>", rep(3, shape)),
            ));
        }
    }
    out
}

fn styles() -> Vec<String> {
    let mut out = Vec::new();
    for m in ["30%", "34%", "38%", "40%", "42%", "44%", "46%"] {
        out.push(format!("p {{ margin: 0 {m} }}"));
    }
    out.push("p { text-align: justify; margin: 0 38% }".to_owned());
    out.push("body { font-family: sans-serif } p { margin: 0 40% }".to_owned());
    out
}

/// Every book of the family, as its probe's tests built them, in order.
pub(crate) fn run() {
    probe();
}
