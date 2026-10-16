//! `zz_rr3m_hunt_urls` (hunt, K-11 round-4 review (hunt)): the books of
//! `tools/probes/rtl/probes/pdf/zz_rr3m_hunt_urls.rs`, which is the probe as it ran.
//!
//! Hunt probe: K-11 round-4 review, family urls.

use super::common::{rep, run_book, Case, Shape, AR, HEP};

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
    for (sc, lang, w) in [("hep", "he", HEP), ("ar", "ar", AR)] {
        let s = |i: usize| w[i % w.len()];
        let shapes: Vec<Shape<'_>> = vec![
            (
                "url-query",
                Box::new(move |i| {
                    format!(
                        "{} https://example.com/path/to?q=1&amp;x=2 {} {}.",
                        s(i),
                        s(i + 1),
                        s(i + 2)
                    )
                }),
            ),
            (
                "www",
                Box::new(move |i| format!("{} www.example.org {} {}.", s(i), s(i + 1), s(i + 2))),
            ),
            (
                "email",
                Box::new(move |i| {
                    format!(
                        "{} user.name@example.co.il {} {}.",
                        s(i),
                        s(i + 1),
                        s(i + 2)
                    )
                }),
            ),
            (
                "url-paren",
                Box::new(move |i| {
                    format!("{} (http://a.example/b-c) {} {}.", s(i), s(i + 1), s(i + 2))
                }),
            ),
            (
                "url-link",
                Box::new(move |i| {
                    format!(
                        "{} <a href=\"https://ex.com/a\">https://ex.com/a</a> {} {}.",
                        s(i),
                        s(i + 1),
                        s(i + 2)
                    )
                }),
            ),
            (
                "url-end-dot",
                Box::new(move |i| format!("{} {} https://ex.com/x. {}", s(i), s(i + 1), s(i + 2))),
            ),
            (
                "email-guil",
                Box::new(move |i| format!("{} «info@ex.co.il» {}, {}.", s(i), s(i + 1), s(i + 2))),
            ),
            (
                "paths",
                Box::new(move |i| {
                    format!(
                        "{} C:\\Users\\dan {} /usr/local/bin {}.",
                        s(i),
                        s(i + 1),
                        s(i + 2)
                    )
                }),
            ),
            (
                "handles",
                Box::new(move |i| {
                    format!(
                        "{} @user_1 {} #tag {} #{} {}.",
                        s(i),
                        s(i + 1),
                        s(i + 2),
                        s(i + 3),
                        s(i + 4)
                    )
                }),
            ),
            (
                "filename",
                Box::new(move |i| {
                    format!("{} report_2010-v2.pdf {} {}.", s(i), s(i + 1), s(i + 2))
                }),
            ),
            (
                "url-rtl-path",
                Box::new(move |i| {
                    format!(
                        "{} https://he.wikipedia.org/wiki/{} {}.",
                        s(i),
                        s(i + 1),
                        s(i + 2)
                    )
                }),
            ),
            (
                "url-port",
                Box::new(move |i| {
                    format!(
                        "{} http://10.0.0.1:8080/ {} ftp://x.y/z.txt {}.",
                        s(i),
                        s(i + 1),
                        s(i + 2)
                    )
                }),
            ),
        ];
        for (name, shape) in &shapes {
            let text = rep(5, shape);
            out.push((
                format!("{sc}/{name}/rtl"),
                lang,
                format!("<p dir=\"rtl\">{text}</p>"),
            ));
            out.push((format!("{sc}/{name}/none"), lang, format!("<p>{text}</p>")));
        }
    }
    out
}

fn styles() -> Vec<String> {
    [
        "",
        "p { text-align: justify; margin: 0 30% }",
        "p { letter-spacing: 0.3em; text-align: justify; margin: 0 25% }",
        "p { word-spacing: 1em; margin: 0 20% }",
        "body { font-family: sans-serif } p { text-align: justify; margin: 0 33% }",
        "body { font-family: monospace } p { margin: 0 26%; text-align: justify }",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

/// Every book of the family, as its probe's tests built them, in order.
pub(crate) fn run() {
    probe();
}
