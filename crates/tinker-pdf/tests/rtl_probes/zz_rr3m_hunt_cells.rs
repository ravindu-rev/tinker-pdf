//! `zz_rr3m_hunt_cells` (hunt, K-11 round-4 review (hunt)): the books of
//! `tools/probes/rtl/probes/pdf/zz_rr3m_hunt_cells.rs`, which is the probe as it ran.
//!
//! Hunt probe: K-11 round-4 review, family cells.

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

fn cases() -> Vec<Case> {
    // Cells and inline blocks whose gap is about the letter-spacing.
    let mut out: Vec<Case> = Vec::new();
    for (sc, lang, w) in [("he", "he", HE), ("hep", "he", HEP), ("ar", "ar", AR)] {
        let s = |i: usize| w[i % w.len()];
        let rows = [
            format!(
                "<tr><td>{}</td><td>{}</td><td>{}</td></tr>",
                s(0),
                s(1),
                s(2)
            ),
            format!(
                "<tr><td>{}.</td><td>({})</td><td>{}</td></tr>",
                s(3),
                s(4),
                s(5)
            ),
            format!(
                "<tr><td>{} {}</td><td>«{}»</td><td>12</td></tr>",
                s(6),
                s(7),
                s(8)
            ),
            format!("<tr><td>{}</td><td>HDAC</td><td>{}</td></tr>", s(9), s(10)),
        ]
        .concat();
        for (d, attr) in [("rtl", " dir=\"rtl\""), ("none", "")] {
            out.push((
                format!("{sc}/table/{d}"),
                lang,
                format!("<table{attr}>{rows}</table>"),
            ));
            out.push((
                format!("{sc}/iblock/{d}"),
                lang,
                format!(
                    "<p{attr}><span class=\"ib\">{}</span><span class=\"ib\">{}</span><span class=\"ib\">({})</span><span class=\"ib\">{}</span></p>",
                    s(0), s(1), s(2), s(3)
                ),
            ));
        }
    }
    out
}

fn styles() -> Vec<String> {
    let mut out = Vec::new();
    for (ls, bs) in [
        ("2px", "2px"),
        ("3px", "3px"),
        ("4px", "4px"),
        ("5px", "5px"),
        ("8px", "8px"),
        ("0.3em", "0.3em"),
        ("0.5em", "0.5em"),
        ("0.6em", "0.6em"),
        ("2px", "0"),
        ("4px", "0"),
        ("8px", "0"),
        ("4px", "3.5px"),
        ("4px", "4.5px"),
        ("0.25em", "0.2em"),
        ("-1px", "0"),
    ] {
        out.push(format!(
            "td {{ padding: 0; letter-spacing: {ls} }} table {{ border-spacing: {bs} }} .ib {{ display: inline-block; letter-spacing: {ls}; margin-left: {bs} }}"
        ));
    }
    out.push("td { padding: 0; word-spacing: 1em } table { border-spacing: 0 } .ib { display: inline-block; word-spacing: 1em }".to_owned());
    out.push("body { font-family: monospace } td { padding: 0; letter-spacing: 0.5em } table { border-spacing: 0.5em } .ib { display: inline-block; letter-spacing: 0.5em; margin-left: 0.5em }".to_owned());
    out
}

/// Every book of the family, as its probe's tests built them, in order.
pub(crate) fn run() {
    probe();
}
