//! `zz_v6_cells` (v6, round 6 lane V (hunt)): the books of
//! `tools/probes/rtl/probes/pdf/zz_v6_cells.rs`, which is the probe as it ran.

use super::common::{AR, Case, HE, run_book};

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

// Family cells: word and letter spacing combined around table cells, set
// apart per cell (the first cell word-spaced, the second letter-spaced),
// touching or a pixel apart, with cells that wrap onto two or three lines
// (so a cell's second line is drawn after the first cell's first, and the
// read-back of one line sees runs whose neighbours on the page it does not
// draw), header cells, a caption, a cell spanning two columns, and styled
// words and punctuation in cells.

fn cases() -> Vec<Case> {
    let mut out: Vec<Case> = Vec::new();
    for (sc, lang, w) in [("ar", "ar", AR), ("he", "he", HE)] {
        let s = |i: usize| w[i % w.len()];
        let tables: Vec<(&str, String)> = vec![
            ("pair", format!("<tr><td>{} {}</td><td>{} {}.</td></tr><tr><td>{}</td><td>{} {}</td></tr>", s(0), s(1), s(2), s(3), s(4), s(5), s(6))),
            ("gloss", format!("<tr><td>{} (abc) {}</td><td>xyz {}</td></tr><tr><td>abc def.</td><td>{} {}</td></tr>", s(0), s(1), s(2), s(3), s(4))),
            ("num", format!("<tr><td>{} \u{661}\u{662}\u{663}</td><td>25\u{66A} {}</td></tr><tr><td>2010</td><td>{} 3.5</td></tr>", s(0), s(1), s(2))),
            ("wrap", format!("<tr><td style=\"width: 6em\">{} {} {} {} {}، {}.</td><td style=\"width: 6em\">{} {} <b>{}</b> {} {}.</td></tr>", s(0), s(1), s(2), s(3), s(4), s(5), s(6), s(7), s(8), s(9), s(10))),
            ("th", format!("<tr><th>{} {}</th><th>abc</th></tr><tr><td>{}?!</td><td>12</td></tr>", s(0), s(1), s(2))),
            ("caption", format!("<caption>{} {} ({}).</caption><tr><td>{}</td><td>{} {}</td></tr>", s(0), s(1), s(2), s(3), s(4), s(5))),
            ("colspan", format!("<tr><td colspan=\"2\">{} {} {}.</td></tr><tr><td>{}</td><td>{} abc</td></tr>", s(0), s(1), s(2), s(3), s(4))),
            ("styled", format!("<tr><td><b>{}</b> <i>{}</i>\u{60C}</td><td>{} <i>(</i>{}<i>)</i>.</td></tr>", s(0), s(1), s(2), s(3))),
        ];
        for (name, rows) in &tables {
            for (d, attr) in [("rtl", " dir=\"rtl\""), ("none", "")] {
                out.push((format!("{sc}/{name}/{d}"), lang, format!("<table{attr}>{rows}</table>")));
            }
            // A left-to-right cell beside a right-to-left one, each with its own dir.
            if *name == "pair" || *name == "gloss" {
                out.push((format!("{sc}/{name}/cells"), lang, format!("<table>{}</table>", rows.replacen("<td>", "<td dir=\"rtl\">", 1).replacen("<td>", "<td dir=\"ltr\">", 1))));
            }
        }
    }
    out
}

fn styles() -> Vec<String> {
    let borders = [
        ("touch", "table { border-spacing: 0 } td, th { padding: 0 }"),
        ("px", "table { border-spacing: 1px } td, th { padding: 0 }"),
        ("pad", "table { border-spacing: 0 } td, th { padding: 0 2px }"),
    ];
    let spacing = [
        "td, th { word-spacing: 1em }",
        "td, th { letter-spacing: 0.3em }",
        "td, th { word-spacing: 0.5em; letter-spacing: 0.1em }",
        "td:first-child { word-spacing: 1em } td + td { letter-spacing: 0.3em }",
        "td, th, caption { text-align: justify; word-spacing: 0.3em }",
        "td { word-spacing: 0 }",
    ];
    let mut out = Vec::new();
    for (_, b) in borders {
        for s in spacing {
            out.push(format!("{b} {s}"));
        }
    }
    out
}
