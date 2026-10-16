//! `zz_rr3m_hunt_gloss` (hunt, K-11 round-4 review (hunt)): the books of
//! `tools/probes/rtl/probes/pdf/zz_rr3m_hunt_gloss.rs`, which is the probe as it ran.
//!
//! Hunt probe: K-11 round-4 review, family gloss.

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
    let mut out: Vec<Case> = Vec::new();
    for (sc, w) in [("he", HE), ("hep", HEP), ("ar", AR)] {
        let s = |i: usize| w[i % w.len()];
        let shapes: Vec<Shape<'_>> = vec![
            (
                "paren1",
                Box::new(move |i| format!("The word for peace is ({})", s(i))),
            ),
            (
                "paren2",
                Box::new(move |i| format!("and the phrase ({} {}) is common", s(i), s(i + 1))),
            ),
            (
                "paren3-dot",
                Box::new(move |i| format!("written as ({} {} {}).", s(i), s(i + 1), s(i + 2))),
            ),
            ("sq", Box::new(move |i| format!("see [{}] here", s(i)))),
            (
                "guil",
                Box::new(move |i| format!("he said «{} {}» today", s(i), s(i + 1))),
            ),
            (
                "curly",
                Box::new(move |i| format!("quoted “{}”, then", s(i))),
            ),
            (
                "paren-num",
                Box::new(move |i| format!("item ({} 12) and", s(i))),
            ),
            (
                "paren-an",
                Box::new(move |i| format!("item ({} ١٢) and", s(i))),
            ),
            (
                "paren-comma",
                Box::new(move |i| format!("words ({}, {}) follow", s(i), s(i + 1))),
            ),
            (
                "nested",
                Box::new(move |i| format!("note ({} [{}]) ends", s(i), s(i + 1))),
            ),
            (
                "styled",
                Box::new(move |i| format!("a <i>({})</i> and <b>[{}]</b> b", s(i), s(i + 1))),
            ),
            ("dash", Box::new(move |i| format!("text — {} — more", s(i)))),
            ("colon", Box::new(move |i| format!("term: {}; next", s(i)))),
            (
                "paren-latin-in",
                Box::new(move |i| format!("name ({} HDAC) here", s(i))),
            ),
        ];
        for (name, shape) in &shapes {
            let text: String = (0..7)
                .map(|k| {
                    format!(
                        "{} {}",
                        [
                            "Some English words",
                            "More text here",
                            "Then a gloss",
                            "Another one comes",
                            "We read on",
                            "Lines wrap now",
                            "Last of them"
                        ][k],
                        shape(k * 2)
                    )
                })
                .collect::<Vec<_>>()
                .join(" ");
            out.push((format!("{sc}/{name}"), "en", format!("<p>{text}</p>")));
        }
        // The mirror: right-to-left paragraphs quoting Latin in brackets.
        let mirror: Vec<Shape<'_>> = vec![
            (
                "m-paren1",
                Box::new(move |i| format!("{} {} HDAC SIRT (class) {}", s(i), s(i + 1), s(i + 2))),
            ),
            (
                "m-paren2",
                Box::new(move |i| {
                    format!(
                        "{} with the enzyme (histone deacetylase) {}",
                        s(i),
                        s(i + 1)
                    )
                }),
            ),
            (
                "m-sq",
                Box::new(move |i| format!("{} see [HDAC] {}", s(i), s(i + 1))),
            ),
            (
                "m-num",
                Box::new(move |i| format!("{} {} (2010) {}.", s(i), s(i + 1), s(i + 2))),
            ),
            (
                "m-mixed",
                Box::new(move |i| format!("{} HDAC ({} x) {}", s(i), s(i + 1), s(i + 2))),
            ),
        ];
        for (name, shape) in &mirror {
            let text = rep(7, shape);
            out.push((
                format!("{sc}/{name}/rtl"),
                if sc == "ar" { "ar" } else { "he" },
                format!("<p dir=\"rtl\">{text}</p>"),
            ));
        }
    }
    out
}

fn styles() -> Vec<String> {
    let mut out = vec![String::new()];
    for m in ["22%", "31%", "38%", "43%"] {
        out.push(format!("p {{ margin: 0 {m} }}"));
        out.push(format!("p {{ text-align: justify; margin: 0 {m} }}"));
    }
    out.push("p { letter-spacing: 0.3em; text-align: justify; margin: 0 25% }".to_owned());
    out.push(
        "body { font-family: sans-serif } p { text-align: justify; margin: 0 33% }".to_owned(),
    );
    out.push("body { font-family: monospace } p { margin: 0 26%; text-align: justify }".to_owned());
    out
}

/// Every book of the family, as its probe's tests built them, in order.
pub(crate) fn run() {
    probe();
}
