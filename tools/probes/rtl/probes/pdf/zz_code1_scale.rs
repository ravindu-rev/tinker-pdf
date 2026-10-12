//! Review probe (round 5, code1): time the writer and reader on lines that
//! stress the round's changed paths, doubling the line, at any tree.
//! - `neutrals`: a right-to-left nowrap line whose middle is one slice of
//!   neutrals alternating a WinAnsi space and an overflow-font sign (C3's one
//!   text object of 2N pieces), under word-spacing.
//! - `code32`: a nowrap line of N short Arabic words under word-spacing (the
//!   overflow font at 0 Tw, C2).
//! - `stops`: a dir-less line opening with a styled bracket pair and ending
//!   with N/2 `?!` (V1's sentence end read leftward).
mod epub_support;
use epub_support::book::styled_book;
use std::time::Instant;
use tinker_pdf::Document;

fn time(body: &str, style: &str) -> (f64, f64, usize) {
    let bytes = styled_book("ar", style, body);
    let t = Instant::now();
    let doc = Document::open(bytes).expect("opens");
    let pages = doc.page_count();
    let open = t.elapsed().as_secs_f64();
    let t = Instant::now();
    let mut n = 0;
    for p in 0..pages {
        n += doc.page(p).expect("page").text().plain_text().chars().count();
    }
    (open, t.elapsed().as_secs_f64(), n)
}

#[test]
fn zz_code1_scale() {
    for n in [500usize, 1_000, 2_000, 4_000] {
        let neutrals = format!(
            "<p dir=\"rtl\">بيت <i>{}</i> بيت</p>",
            "\u{66A} ".repeat(n).trim_end()
        );
        let code32 = format!("<p dir=\"rtl\">{}</p>", "عدن ".repeat(n));
        let stops = format!(
            "<p>جميل بيت كبير <b>(</b>نور<b>)</b> يوم{}</p>",
            "?!".repeat(n / 2)
        );
        for (name, body, style) in [
            ("neutrals", neutrals, "p { white-space: nowrap; word-spacing: 0.5em }"),
            ("code32", code32, "p { white-space: nowrap; word-spacing: 0.5em }"),
            ("stops", stops, "p { white-space: nowrap; letter-spacing: 0.1em }"),
        ] {
            let (open, text, chars) = time(&body, style);
            eprintln!("ZZS {name}\tn={n}\topen={:.3}s\ttext={:.3}s\tchars={chars}", open, text);
        }
    }
}
