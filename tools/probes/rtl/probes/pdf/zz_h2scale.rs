//! Scale probe (scratch, never committed): time to lay out, write and read
//! back books whose spaced slices are long, at growing sizes.
#![allow(missing_docs, clippy::all, unused)]

mod epub_support;

use epub_support::book::styled_book;
use tinker_pdf::Document;

fn body(kind: &str, n: usize) -> String {
    let words = ["αλφα", "abc", "שלום", "βήτα", "def", "עולם", "кни", "١٢٣", "(x)", "?!"];
    let text: Vec<String> = (0..n).map(|i| words[i % words.len()].to_owned()).collect();
    match kind {
        // One unbroken line: a single slice per level run, one text object each.
        "pre" => format!("<pre style=\"letter-spacing: 0.5em; word-spacing: 1em\">{}</pre>", text.join(" ")),
        // Alternating scripts without spaces: a resource change at every word.
        "glued" => format!("<pre style=\"letter-spacing: 0.5em\">{}</pre>", text.join("")),
        _ => format!("<p dir=\"rtl\" style=\"word-spacing: 1em; text-align: justify\">{}</p>", text.join(" ")),
    }
}

#[test]
fn scale() {
    for kind in ["pre", "glued", "para"] {
        for n in [250usize, 500, 1000, 2000, 4000] {
            let bytes = styled_book("he", "", &body(kind, n));
            let t0 = std::time::Instant::now();
            let doc = Document::open(bytes).expect("opens");
            let t1 = t0.elapsed();
            let mut chars = 0usize;
            for at in 0..doc.page_count() {
                chars += doc.page(at).expect("page").text().plain_text().chars().count();
            }
            let t2 = t0.elapsed();
            println!("SCALE\t{kind}\t{n}\topen_ms={}\ttotal_ms={}\tpages={}\tchars={chars}", t1.as_millis(), t2.as_millis(), doc.page_count());
        }
    }
}
