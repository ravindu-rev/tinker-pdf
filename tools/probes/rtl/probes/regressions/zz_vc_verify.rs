//! Scratch verification probe (never committed).
#![allow(missing_docs, clippy::all)]

mod epub_support;

use epub_support::book::styled_book;
use epub_support::conservation::{conservation, conservation_in_logical_order};
use tinker_pdf::Document;

fn run(cases: &[(&str, &str, &str)]) -> usize {
    let mut failing = 0;
    for (language, style, body) in cases {
        let bytes = styled_book(language, style, body);
        let doc = Document::open(bytes.clone()).expect("the book opens");
        let content = conservation(&bytes, &doc);
        let logical = conservation_in_logical_order(&bytes, &doc);
        let text: String = (0..doc.page_count())
            .filter_map(|p| doc.page(p))
            .map(|p| p.text().plain_text())
            .collect::<Vec<_>>()
            .join("|")
            .replace('\n', "|");
        let ok = content.holds() && logical.holds();
        failing += usize::from(!ok);
        println!(
            "CASE\t{}\tcontent={}\tlogical={}\t{style:?}\t{body}\t{text}\t{:?}",
            if ok { "ok" } else { "FAIL" },
            content.holds(),
            logical.holds(),
            logical.divergences.first().or(content.divergences.first())
        );
    }
    failing
}

const SPACED: &str = "p, li, td { letter-spacing: 0.55em; text-align: justify; margin: 0 20% }";
const WORDED: &str = "p, li { word-spacing: 1em; margin: 0 18% }";

#[test]
fn f1_an_ornate_parenthesis_cut_from_its_line_carries_no_pen() {
    let failing = run(&[
        ("ar", SPACED, "<p><i>\u{FD3F}</i>عوامل عدّة<i>\u{FD3E}</i></p>"),
        ("he", SPACED, "<p><i>\u{FD3F}</i>שָׁלוֹם עוֹלָם<i>\u{FD3E}</i></p>"),
        ("en", SPACED, "<p>The verse \u{FD3F}שָׁלוֹם עוֹלָם\u{FD3E} and the words \u{FD3F}<i>מִלְחָמָה</i>\u{FD3E} are quoted in this English sentence here.</p>"),
        ("ar", WORDED, "<p>عوامل \u{FD3F}HDAC\u{FD3E} عدّة وقد.</p>"),
        ("ar", SPACED, "<table style=\"border-spacing: 0\"><tr><td>عوامل</td><td>\u{FD3F}عدّة\u{FD3E}</td><td>وقد</td></tr><tr><td>\u{FD3F}أصبحنا\u{FD3E}</td><td>نعرف</td><td>x</td></tr></table>"),
    ]);
    assert_eq!(failing, 0);
}

#[test]
fn f1_control_guillemets() {
    let failing = run(&[
        ("ar", SPACED, "<p><i>«</i>عوامل عدّة<i>»</i></p>"),
        ("he", SPACED, "<p><i>«</i>שָׁלוֹם עוֹלָם<i>»</i></p>"),
    ]);
    assert_eq!(failing, 0);
}

#[test]
fn f2_a_line_opening_with_a_bracketed_arabic_number_reads_its_stop_last() {
    let failing = run(&[(
        "ar",
        "p, li, td { letter-spacing: 0.5em }",
        "<p>عوامل عدّة وقد (١٬٢٣٤), أصبحنا نعرف الآن الموادّ. عدّة وقد أصبحنا (١٬٢٣٤), نعرف الآن الموادّ تكبح. وقد أصبحنا نعرف (١٬٢٣٤), الآن الموادّ تكبح عوامل. </p>",
    )]);
    assert_eq!(failing, 0);
}

#[test]
fn f2_control_plain_number() {
    let failing = run(&[(
        "ar",
        "p, li, td { letter-spacing: 0.5em }",
        "<p>عوامل عدّة وقد (١٢٣), أصبحنا نعرف الآن الموادّ. عدّة وقد أصبحنا (١٢٣), نعرف الآن الموادّ تكبح. وقد أصبحنا نعرف (١٢٣), الآن الموادّ تكبح عوامل. </p>",
    )]);
    assert_eq!(failing, 0);
}
