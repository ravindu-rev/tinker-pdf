//! Scratch re-review probe (never committed): runs of neutrals at a
//! right-to-left level, which `coded_order` now draws in level order on a line
//! that holds a right-to-left character (or an RLE/RLO/RLI). One ROW per book.
#![allow(missing_docs, clippy::all)]

mod epub_support;


mod zz_vov_shim {
    use tinker_pdf::{Document, TextOptions};
    use tinker_pdf_shape::unicode::{bidi_class, BidiClass};
    fn h(s: &str) -> u64 {
        let mut x: u64 = 0xcbf29ce484222325;
        for b in s.bytes() {
            x ^= u64::from(b);
            x = x.wrapping_mul(0x100000001b3);
        }
        x
    }
    pub fn styled_book(lang: &str, style: &str, body: &str) -> Vec<u8> {
        let bytes = super::epub_support::book::styled_book(lang, style, body);
        let mut vov = 0usize;
        let mut at = String::new();
        if let Ok(doc) = Document::open(bytes.clone()) {
            for p in 0..doc.page_count() {
                let Some(page) = doc.page(p) else { continue };
                let drawn = page.text_with(&TextOptions { content_order: true });
                let mut boxes: Vec<(i64, f64, f64, f64, String)> = Vec::new();
                for line in drawn.lines() {
                    for c in &line.chars {
                        let (x0, _, x1, _) = c.quad.bounds();
                        if c.text.chars().next().is_some_and(|ch| bidi_class(ch) == BidiClass::NSM) {
                            continue;
                        }
                        if !c.text.trim().is_empty() {
                            boxes.push(((c.origin.1 * 10.0).round() as i64, x0, x1, c.size, c.text.clone()));
                        }
                    }
                }
                boxes.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.total_cmp(&b.1)));
                for pair in boxes.windows(2) {
                    let (a, b) = (&pair[0], &pair[1]);
                    if a.0 == b.0 && b.1 < a.2 - 0.02 * b.3 {
                        vov += 1;
                        if at.is_empty() {
                            at = format!("{:?}@{:.2}..{:.2}/{:?}@{:.2}", a.4, a.1, a.2, b.4, b.1);
                        }
                    }
                }
            }
        }
        println!("VOVX\t{style}{}\t{lang}\t{:016x}\t{vov}\t{at}", if body.contains("word-spacing") || body.contains("justify") { " INLINE-WS" } else { "" }, h(&format!("{lang}|{body}")));
        bytes
    }
}
use zz_vov_shim::styled_book;
use epub_support::conservation::{conservation, conservation_in_logical_order};
use tinker_pdf::Document;

#[test]
fn probe() {
    let bodies: Vec<(&str, &str, String)> = vec![
        ("rli-chars", "en", "<p>x \u{2067}?!\u{2069} y</p>".to_owned()),
        ("rle-chars", "en", "<p>x \u{202B}?!\u{202C} y</p>".to_owned()),
        ("rlo-chars", "en", "<p>x \u{202E}?!\u{202C} y</p>".to_owned()),
        ("rli-chars-he", "he", "<p>x \u{2067}?!\u{2069} y שלום</p>".to_owned()),
        ("span-rtl-latin", "en", "<p>abc <span dir=\"rtl\">?!</span> def</p>".to_owned()),
        ("span-rtl-he", "en", "<p>abc <span dir=\"rtl\">?!</span> שלום</p>".to_owned()),
        ("rtl-span-ltr", "he", "<p dir=\"rtl\">שלום <span dir=\"ltr\">?!</span> עולם</p>".to_owned()),
        ("rtl-mid", "he", "<p dir=\"rtl\">שלום ?! עולם</p>".to_owned()),
        ("rtl-italic-mid", "he", "<p dir=\"rtl\">שלום <i>?!</i> עולם</p>".to_owned()),
        ("rtl-opens", "he", "<p dir=\"rtl\">?! שלום</p>".to_owned()),
        ("rtl-italic-opens", "he", "<p dir=\"rtl\"><i>?!</i> שלום</p>".to_owned()),
        ("rtl-dots-opens", "he", "<p dir=\"rtl\">(...) שלום</p>".to_owned()),
        ("rtl-bold-brackets", "he", "<p dir=\"rtl\">שלום <b>(</b>abc<b>)</b> עולם.</p>".to_owned()),
        ("rtl-bold-brackets-ar", "ar", "<p dir=\"rtl\">مرحبا <b>(</b>abc<b>)</b> عالم.</p>".to_owned()),
        ("rtl-italic-ellipsis", "he", "<p dir=\"rtl\">שלום <i>...</i> עולם</p>".to_owned()),
        ("rtl-italic-quotes", "he", "<p dir=\"rtl\">שלום <i>\"</i>abc<i>\"</i></p>".to_owned()),
        ("rtl-italic-number-dot", "he", "<p dir=\"rtl\"><i>1.</i> שלום</p>".to_owned()),
        ("rtl-italic-time", "he", "<p dir=\"rtl\">שלום <i>12:30</i> עולם</p>".to_owned()),
        ("rtl-italic-dashes", "he", "<p dir=\"rtl\">שלום <i>- -</i> עולם</p>".to_owned()),
        ("rtl-italic-trailing", "he", "<p dir=\"rtl\">שלום<i>!?</i></p>".to_owned()),
        ("ltr-italic-before-he", "he", "<p>Hello <i>?!</i> שלום</p>".to_owned()),
        ("rtl-heading", "he", "<h1 dir=\"rtl\">?!</h1><p dir=\"rtl\">שלום עולם</p>".to_owned()),
        ("rtl-br", "he", "<p dir=\"rtl\">?!<br/>שלום</p>".to_owned()),
        ("rtl-br-after", "he", "<p dir=\"rtl\">שלום<br/>?!</p>".to_owned()),
        ("rtl-pct", "ar", "<p dir=\"rtl\">نسبة <i>%</i>50 عالية</p>".to_owned()),
        ("rtl-italic-paren-num", "he", "<p dir=\"rtl\"><i>(1)</i> פריט</p>".to_owned()),
        ("rtl-li", "he", "<ol dir=\"rtl\" style=\"list-style-position: inside\"><li>שלום ?!</li></ol>".to_owned()),
        ("rtl-italic-colon", "ar", "<p dir=\"rtl\">قال<i>: «</i>مرحبا<i>»</i></p>".to_owned()),
        ("rtl-italic-plus", "he", "<p dir=\"rtl\">שלום <i>+ -</i> 5 עולם</p>".to_owned()),
        ("rtl-italic-slash", "ar", "<p dir=\"rtl\">مرحبا <i>/</i> عالم <i>//</i> هنا</p>".to_owned()),
    ];
    let styles = ["", "body { font-family: sans-serif }", "p { text-align: justify; margin: 0 40% }"];
    for (si, style) in styles.iter().enumerate() {
        for (name, lang, body) in &bodies {
            let bytes = styled_book(lang, style, body);
            let doc = match Document::open(bytes.clone()) {
                Ok(d) => d,
                Err(e) => {
                    println!("ROW\ts{si}\t{name}\tOPEN-ERR {e:?}");
                    continue;
                }
            };
            let c = conservation(&bytes, &doc);
            let l = conservation_in_logical_order(&bytes, &doc);
            let text: Vec<String> = (0..doc.page_count())
                .filter_map(|p| doc.page(p))
                .map(|p| p.text().plain_text())
                .collect();
            println!(
                "ROW\ts{si}\t{name}\tcontent={}\tlogical={}\t{:?}",
                c.holds(),
                l.holds(),
                text.join("|")
            );
        }
    }
}
