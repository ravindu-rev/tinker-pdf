//! The text options, exercised the way a caller reaches them (ruling 11):
//! through `Page::text()` on a document the writer produced, and with every
//! type named from `tinker_pdf` rather than from the crate underneath.
//!
//! The rules themselves are unit-tested beside the code that applies them —
//! `crates/tinker-pdf-content/src/plain.rs` for hyphen rejoining. What this
//! file adds is that the lines extraction really makes, from a real content
//! stream, are the lines those rules see, and that the default is the
//! unoptioned answer to the byte.

use tinker_pdf::{Document, PlainTextOptions};
use tinker_pdf_cos::DocumentBuilder;

/// A one-page document whose content stream is `content`, with Helvetica
/// as `/F0`.
fn document(content: &str) -> Document {
    let mut builder = DocumentBuilder::new();
    builder.add_base_font(b"F0", b"Helvetica");
    builder.add_page(300.0, 200.0, |page| {
        page.raw(content.as_bytes());
    });
    Document::open(builder.finish()).expect("it opens")
}

/// Four lines: a word broken at a hard hyphen before a lower-case start,
/// which joins; a compound broken before a capital, which does not; and a
/// line with a hyphen in the middle, which nothing touches.
const HYPHENATED: &str = concat!(
    "BT /F0 10 Tf 10 150 Td (The hyphen-) Tj 0 -12 Td (ation of a Franco-) Tj ",
    "0 -12 Td (Prussian well-known war) Tj 0 -12 Td (ends here) Tj ET",
);

#[test]
fn a_hyphenated_line_end_rejoins_only_when_asked() {
    let doc = document(HYPHENATED);
    let text = doc.page(0).expect("a page").text();
    assert_eq!(text.lines().len(), 4, "{:?}", text.plain_text());

    let default = text.plain_text_with(&PlainTextOptions::default());
    assert_eq!(
        default.text,
        text.plain_text(),
        "the default is plain_text to the byte"
    );
    assert_eq!(default.hyphens.joins(), 0);

    let joined = text.plain_text_with(&PlainTextOptions {
        rejoin_hyphens: true,
    });
    assert_eq!(
        joined.text,
        "The hyphenation of a Franco-\nPrussian well-known war\nends here\n"
    );
    assert_eq!(joined.hyphens.hard_joins, 1, "one inferred join, counted");
    assert_eq!(joined.hyphens.soft_joins, 0);
}
