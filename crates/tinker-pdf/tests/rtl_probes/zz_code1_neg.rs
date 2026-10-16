//! `zz_code1_neg` (neg, round-5 review (code)): the books of
//! `tools/probes/rtl/probes/pdf/zz_code1_neg.rs`, which is the probe as it ran.
//!
//! Review probe (round 5, code1): the overflow font under a negative word
//! spacing — does a glyph after its code-32 letter land on another?

use crate::epub_support::book::styled_book;
use tinker_pdf::{Document, TextOptions};
use tinker_pdf_shape::unicode::{bidi_class, BidiClass};

fn overprinted(doc: &Document) -> Vec<String> {
    let page = doc.page(0).expect("a page");
    let drawn = page.text_with(&TextOptions {
        content_order: true,
    });
    let mut boxes: Vec<(i64, f64, f64, f64, &str)> = drawn
        .lines()
        .iter()
        .flat_map(|line| line.chars.iter())
        .filter(|c| {
            !c.text.trim().is_empty()
                && c.text
                    .chars()
                    .next()
                    .is_none_or(|first| bidi_class(first) != BidiClass::NSM)
        })
        .map(|c| {
            let (x0, _, x1, _) = c.quad.bounds();
            let baseline = (c.origin.1 * 100.0).round() as i64;
            (baseline, x0, x1, c.size, c.text.as_str())
        })
        .collect();
    boxes.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.total_cmp(&b.1)));
    boxes
        .windows(2)
        .filter_map(|pair| match pair {
            [before, after] if before.0 == after.0 && after.1 < before.2 - 0.01 * after.3 => {
                Some(format!(
                    "{:?} {:.3}-{:.3} under {:?} at {:.3}",
                    before.4, before.1, before.2, after.4, after.1
                ))
            }
            _ => None,
        })
        .collect()
}

fn zz_code1_neg() {
    for ws in ["-4px", "-2px", "0px", "4px"] {
        for (lang, body) in [
            ("en", "<p>x <b>\u{3B1}\u{3BB}\u{3C6}\u{3B1}</b> y.</p>"),
            (
                "ar",
                "<p dir=\"rtl\">\u{645}\u{62F}\u{629} \u{645}\u{639}\u{644}\u{645}.</p>",
            ),
        ] {
            let style = format!("p {{ word-spacing: {ws} }}");
            let bytes = styled_book(lang, &style, body);
            let doc = Document::open(bytes).expect("opens");
            let text = doc.page(0).expect("page").text().plain_text();
            let over = overprinted(&doc);
            emit!(
                "ZZN {ws}\t{lang}\t{text:?}\toverprinted {}\t{:?}",
                over.len(),
                over
            );
        }
    }
}

/// Every book of the family, as its probe's tests built them, in order.
pub(crate) fn run() {
    zz_code1_neg();
}
