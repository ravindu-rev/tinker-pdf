//! `zz_rr3m_h2_nested` (h2, CD-19 fix review (hunt)): the books of
//! `tools/probes/rtl/probes/pdf/zz_rr3m_h2_nested.rs`, which is the probe as it ran.
//!
//! Hunt probe: CD-19 round-5 hunt 2, family nested.

use super::common::{run_book, Case, AR, ARP, HE};

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

// Family nested: nested inline styles and the block kinds earlier probes
// did not build: spacing set on a span differently from its paragraph (word
// spacing on, off, negative; letter-spacing inside a word-spaced line),
// three levels of styles, <q> (generated quotes), <code> in right-to-left
// text, lists with arabic-indic and hebrew markers, inside markers,
// description lists, figures with captions, block quotes, headings.

fn cases() -> Vec<Case> {
    let mut out: Vec<Case> = Vec::new();
    for (sc, lang, w) in [("ar", "ar", AR), ("he", "he", HE), ("arp", "ar", ARP)] {
        let s = |i: usize| w[i % w.len()];
        // (name, body, wrap in <p>)
        let shapes: Vec<(&str, String, bool)> = vec![
            ("spanws", format!("{} <span style=\"word-spacing: 2em\">{} {} ({})</span> {} {}.", s(0), s(1), s(2), s(3), s(4), s(5)), true),
            ("spanws0", format!("{} <span style=\"word-spacing: 0\">{} {}, {}</span> {} {}.", s(0), s(1), s(2), s(3), s(4), s(5)), true),
            ("spanwsneg", format!("{} <span style=\"word-spacing: -3px\">{} {} {}</span> {}.", s(0), s(1), s(2), s(3), s(4)), true),
            ("spanls", format!("{} <span style=\"letter-spacing: 0.4em\">{} {}</span> {} {}.", s(0), s(1), s(2), s(3), s(4)), true),
            ("deep", format!("{} <b>{} <i>{} <u>{} ({})</u></i>.</b> {}.", s(0), s(1), s(2), s(3), s(4), s(5)), true),
            ("q", format!("{} <q>{} {}</q> {} <q><b>{}</b></q>.", s(0), s(1), s(2), s(3), s(4)), true),
            ("code", format!("{} <code>{} x = {}</code> {} {}.", s(0), s(1), s(2), s(3), s(4)), true),
            ("olai", format!("<ol style=\"list-style-type: arabic-indic\"><li>{} {} {}.</li><li>{} ({}) {}.</li></ol>", s(0), s(1), s(2), s(3), s(4), s(5)), false),
            ("olhe", format!("<ol style=\"list-style-type: hebrew\"><li>{} {} {}.</li><li>{} ({}) {}.</li></ol>", s(0), s(1), s(2), s(3), s(4), s(5)), false),
            ("ulin", format!("<ul style=\"list-style-position: inside\"><li>{} {} {}.</li><li><b>{}</b> {}.</li></ul>", s(0), s(1), s(2), s(3), s(4)), false),
            ("dl", format!("<dl><dt>{} {}</dt><dd>{} ({}) {}.</dd></dl>", s(0), s(1), s(2), s(3), s(4)), false),
            ("fig", format!("<figure><p>{} {} {}.</p><figcaption>{} <b>{}</b> {}.</figcaption></figure>", s(0), s(1), s(2), s(3), s(4), s(5)), false),
            ("bq", format!("<blockquote><p>{} {}, {} {}.</p></blockquote>", s(0), s(1), s(2), s(3)), false),
            ("h2", format!("<h2>{} {} ({})</h2><p>{} {}.</p>", s(0), s(1), s(2), s(3), s(4)), false),
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
        "p, li, dd, dt, figcaption, h2 { word-spacing: 0.6em }",
        "p, li, dd, dt, figcaption, h2 { word-spacing: 1em; letter-spacing: 0.05em }",
        "p, li, dd, dt, figcaption, h2 { text-align: justify; margin-left: 30%; margin-right: 30% }",
        "body { font-family: sans-serif } p, li, dd, dt, figcaption, h2 { word-spacing: 0.5em; text-align: justify; margin-left: 25%; margin-right: 25% }",
        "p, li, dd, dt, figcaption, h2 { word-spacing: 8pt }",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

/// Every book of the family, as its probe's tests built them, in order.
pub(crate) fn run() {
    probe();
}
