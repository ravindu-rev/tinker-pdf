//! `zz_rr3m_h2_tables` (h2, CD-19 fix review (hunt)): the books of
//! `tools/probes/rtl/probes/pdf/zz_rr3m_h2_tables.rs`, which is the probe as it ran.
//!
//! Hunt probe: CD-19 round-5 hunt 2, family tables.

use super::common::{run_book, Case};

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

// Family tables: structures earlier table probes did not build (three-cell
// rows, header cells, captions, colspan, nested tables, display: table-cell
// divs), the cell before ending in a styled Latin word, a styled number, a
// dash, CJK, U+2011 or nothing, a cell after in Syriac, mixed Hebrew and
// Latin, Arabic with a number, or a bracketed Hebrew word, under spacing in
// px, pt, ex and em, collapsed borders, a full-width table, monospace cells.

fn cases() -> Vec<Case> {
    let mut out: Vec<Case> = Vec::new();
    let prev: [(&str, &str); 7] = [
        ("latsty", "abc <b>def</b>"),
        ("numsty", "x <i>12</i>"),
        ("dash", "abc \u{2014}"),
        ("cjk", "\u{4E2D}\u{6587} \u{5B57}\u{7B26}"),
        ("empty", ""),
        (
            "grk-b",
            "\u{3B1}\u{3BB}\u{3C6}\u{3B1} <b>\u{3B2}\u{3AE}\u{3C4}\u{3B1}</b>",
        ),
        ("he-neu", "\u{5E1}\u{5E4}\u{5E8} (x)"),
    ];
    let next: [(&str, &'static str, &str); 4] = [
        (
            "syr",
            "syr",
            "\u{71F}\u{72C}\u{712}\u{710} \u{712}\u{71D}\u{72C}\u{710}",
        ),
        (
            "hemix",
            "he",
            "\u{5E1}\u{5E4}\u{5E8} abc \u{5D1}\u{5D9}\u{5EA}",
        ),
        (
            "arnum",
            "ar",
            "\u{643}\u{62A}\u{627}\u{628} 12 \u{646}\u{648}\u{631}.",
        ),
        (
            "hebr",
            "he",
            "(\u{5E1}\u{5E4}\u{5E8}) \u{5D1}\u{5D9}\u{5EA}",
        ),
    ];
    for (pn, p) in prev {
        for (nn, lang, n) in next {
            let structures: Vec<(&str, String)> = vec![
                ("three", format!("<table{{d}}><tr><td>{p}</td><td>{n}</td><td>{p}</td></tr></table>")),
                ("th", format!("<table{{d}}><tr><th>{p}</th><th>{n}</th></tr><tr><td>{n}</td><td>{p}</td></tr></table>")),
                ("cap", format!("<table{{d}}><caption>{n} {p}</caption><tr><td>{p}</td><td>{n}</td></tr></table>")),
                ("span", format!("<table{{d}}><tr><td colspan=\"2\">{p} {n}</td></tr><tr><td>{p}</td><td>{n}</td></tr></table>")),
                ("nest", format!("<table{{d}}><tr><td><table><tr><td>{p}</td></tr></table></td><td>{n}</td></tr></table>")),
                ("divs", format!("<div{{d}} style=\"display: table\"><div style=\"display: table-row\"><div style=\"display: table-cell\">{p}</div><div style=\"display: table-cell\">{n}</div></div></div>")),
            ];
            for (sn, s) in &structures {
                for (d, attr) in [("rtl", " dir=\"rtl\""), ("none", "")] {
                    out.push((format!("{pn}-{nn}/{sn}/{d}"), lang, s.replace("{d}", attr)));
                }
            }
        }
    }
    out
}

fn styles() -> Vec<String> {
    [
        "td, th, caption, div { word-spacing: 12px } table { border-spacing: 2px }",
        "table { border-spacing: 0 } td, th, caption, div { padding: 0; word-spacing: 0.75em }",
        "table { border-collapse: collapse } td, th, div { padding: 0 1pt; letter-spacing: 0.2em; word-spacing: 1.5ex }",
        "table { width: 100%; border-spacing: 0 } td, th, div { padding: 0; text-align: justify; word-spacing: 0.5em }",
        "body { font-family: monospace } table { border-spacing: 0 } td, th, div { padding: 0; word-spacing: 1.2em }",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

/// Every book of the family, as its probe's tests built them, in order.
pub(crate) fn run() {
    probe();
}
