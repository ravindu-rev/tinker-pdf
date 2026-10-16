//! The overprint shim the `zz_vx_*` families build their books through: the
//! book, and a `VOVX` line saying how many of its visible glyphs are drawn over
//! another.

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
    let bytes = crate::epub_support::book::styled_book(lang, style, body);
    let mut vov = 0usize;
    let mut at = String::new();
    if let Ok(doc) = Document::open(bytes.clone()) {
        for p in 0..doc.page_count() {
            let Some(page) = doc.page(p) else { continue };
            let drawn = page.text_with(&TextOptions {
                content_order: true,
            });
            let mut boxes: Vec<(i64, f64, f64, f64, String)> = Vec::new();
            for line in drawn.lines() {
                for c in &line.chars {
                    let (x0, _, x1, _) = c.quad.bounds();
                    if c.text
                        .chars()
                        .next()
                        .is_some_and(|ch| bidi_class(ch) == BidiClass::NSM)
                    {
                        continue;
                    }
                    if !c.text.trim().is_empty() {
                        boxes.push((
                            (c.origin.1 * 10.0).round() as i64,
                            x0,
                            x1,
                            c.size,
                            c.text.clone(),
                        ));
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
    emit!(
        "VOVX\t{style}{}\t{lang}\t{:016x}\t{vov}\t{at}",
        if body.contains("word-spacing") || body.contains("justify") {
            " INLINE-WS"
        } else {
            ""
        },
        h(&format!("{lang}|{body}"))
    );
    bytes
}
