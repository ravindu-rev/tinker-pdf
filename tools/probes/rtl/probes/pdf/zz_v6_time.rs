//! Lane V timing probe (round 6 of ROADMAP CD-19): the cost of reading each
//! right-to-left line back (935b077's `Fonts::set_readings`), against a tree
//! without it. Each shape is opened at doubling sizes; a line is printed per
//! size with the wall-clock milliseconds of `Document::open` (which lays out
//! and writes the PDF) and of reading every page's text. A time is not a
//! measurement another machine repeats: read the ratio between sizes, and
//! between trees on one machine.
#![allow(missing_docs, clippy::all, unused)]

mod epub_support;

use epub_support::book::styled_book;
use std::time::Instant;
use tinker_pdf::Document;

fn shape(name: &str, n: usize) -> (&'static str, &'static str, String) {
    let he = ["בראשית", "ברא", "אלהים", "את", "השמים", "ואת", "הארץ", "אור"];
    let w = |i: usize| he[i % he.len()];
    match name {
        // A dir-less Hebrew line ending in a number and a stop under word
        // spacing (ROADMAP CD-19's H1, which no drawing reads right), its
        // words parted by styled neutrals: every run is at a right-to-left
        // level and spaced, so each offers seven drawings.
        "h1-nowrap" => {
            let body: String = (0..n)
                .map(|i| format!("{} <b>!</b> {} <i>,</i> ", w(i), w(i + 1)))
                .collect();
            ("he", "p { word-spacing: 0.5em; white-space: nowrap }", format!("<p>{body}אור \u{666}\u{660}.</p>"))
        }
        // The same, wrapped: n lines' worth, each line ending in the shape.
        "h1-wrapped" => {
            let body: String = (0..n)
                .map(|i| format!("<p>{} <b>!</b> {} <i>,</i> {} <b>?</b> {} \u{666}\u{660}.</p>", w(i), w(i + 1), w(i + 2), w(i + 3)))
                .collect();
            ("he", "p { word-spacing: 0.5em }", body)
        }
        // Control: the same text with no word spacing (the rules read it, or
        // the read-back gives up after one reading).
        "h1-nowrap-unspaced" => {
            let body: String = (0..n)
                .map(|i| format!("{} <b>!</b> {} <i>,</i> ", w(i), w(i + 1)))
                .collect();
            ("he", "p { white-space: nowrap }", format!("<p>{body}אור \u{666}\u{660}.</p>"))
        }
        // A right-to-left paragraph of styled neutrals under letter spacing
        // and justification, wrapped.
        "rtl-justified" => {
            let body: String = (0..n)
                .map(|i| format!("{} <b>?!</b> {} <i>(</i>{}<i>)</i> {}، ", w(i), w(i + 1), w(i + 2), w(i + 3)))
                .collect();
            ("he", "p { letter-spacing: 0.1em; word-spacing: 0.5em; text-align: justify }", format!("<p dir=\"rtl\">{body}.</p>"))
        }
        // Plain right-to-left prose, justified: what most books are.
        "rtl-prose-justified" => {
            let body: String = (0..n)
                .map(|i| format!("{} {} {} {}، {} {}. ", w(i), w(i + 1), w(i + 2), w(i + 3), w(i + 4), w(i + 5)))
                .collect();
            ("he", "p { text-align: justify }", format!("<p dir=\"rtl\">{body}</p>"))
        }
        // The same with a bold and an italic word a sentence.
        "rtl-styled-justified" => {
            let body: String = (0..n)
                .map(|i| format!("{} <b>{}</b> {} <i>{}</i>، {} {}. ", w(i), w(i + 1), w(i + 2), w(i + 3), w(i + 4), w(i + 5)))
                .collect();
            ("he", "p { text-align: justify }", format!("<p dir=\"rtl\">{body}</p>"))
        }
        // Dir-less prose, no spacing at all.
        "dirless-prose" => {
            let body: String = (0..n)
                .map(|i| format!("{} {} <b>{}</b> {}, ({}). ", w(i), w(i + 1), w(i + 2), w(i + 3), w(i + 4)))
                .collect();
            ("he", "p { margin: 0 }", format!("<p>{body}</p>"))
        }
        // One line letter-spaced past half an em, every word styled apart.
        "ls-nowrap" => {
            let body: String = (0..n)
                .map(|i| format!("<b>{}</b> <i>!</i> {} ", w(i), w(i + 1)))
                .collect();
            ("he", "p { letter-spacing: 0.6em; white-space: nowrap }", format!("<p>{body}\u{666}\u{660}.</p>"))
        }
        // The same under a scale of one axis: the line is read back on the
        // page, and a reading that cuts a word there is searched again in
        // the runs' own space.
        "ls-scaled" => {
            let body: String = (0..n)
                .map(|i| format!("<b>{}</b> <i>!</i> {} ", w(i), w(i + 1)))
                .collect();
            ("he", "p { letter-spacing: 0.6em; white-space: nowrap; transform: scaleX(2) }", format!("<p>{body}\u{666}\u{660}.</p>"))
        }
        // rtl-justified under a scale of one axis.
        "rtl-justified-scaled" => {
            let body: String = (0..n)
                .map(|i| format!("{} <b>?!</b> {} <i>(</i>{}<i>)</i> {}، ", w(i), w(i + 1), w(i + 2), w(i + 3)))
                .collect();
            ("he", "p { letter-spacing: 0.1em; word-spacing: 0.5em; text-align: justify; transform: scaleX(2) }", format!("<p dir=\"rtl\">{body}.</p>"))
        }
        _ => ("en", "", String::new()),
    }
}

#[test]
fn probe() {
    let only = std::env::var("V6T_ONLY").ok();
    let max: usize = std::env::var("V6T_MAX").ok().and_then(|s| s.parse().ok()).unwrap_or(256);
    for name in ["h1-nowrap", "h1-nowrap-unspaced", "h1-wrapped", "rtl-justified", "rtl-prose-justified", "rtl-styled-justified", "dirless-prose", "ls-nowrap", "ls-scaled", "rtl-justified-scaled"] {
        if only.as_deref().is_some_and(|o| o != name) {
            continue;
        }
        let mut n = 4usize;
        while n <= max {
            let (lang, style, body) = shape(name, n);
            let bytes = styled_book(lang, style, &body);
            let t0 = Instant::now();
            let doc = Document::open(bytes).expect("opens");
            let t1 = t0.elapsed();
            let mut chars = 0usize;
            let t2 = Instant::now();
            for at in 0..doc.page_count() {
                chars += doc.page(at).expect("a page").text().plain_text().chars().count();
            }
            let t3 = t2.elapsed();
            println!(
                "SCALE\t{name}\tn={n}\tbody={}\tpages={}\tchars={chars}\topen_ms={:.1}\ttext_ms={:.1}",
                body.chars().count(),
                doc.page_count(),
                t1.as_secs_f64() * 1000.0,
                t3.as_secs_f64() * 1000.0
            );
            n *= 2;
        }
    }
}
