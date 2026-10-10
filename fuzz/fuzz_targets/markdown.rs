//! The Markdown reader (tier 5's formats row): CommonMark 0.31.2's block and
//! inline phases, over arbitrary text.
//!
//! The input is *text* — the bytes are read as UTF-8 with each malformed
//! sequence as U+FFFD, exactly as `Document::open_markdown` reads them — so
//! there is no offset to corrupt and no length to lie about. What a mutator
//! finds instead is the specification's seams: a tab half consumed by a list
//! marker, a delimiter run that both opens and closes, a link label a thousand
//! characters long, a fence inside a list inside a quote, an HTML block type 7
//! that a lazy line may not start. No control byte: every seed is a document.
//!
//! # What this target checks
//!
//! **That the reader never panics, hangs or exhausts memory** (ruling 1) — the
//! arena trees' unlink and re-parent, the delimiter stack's prev/next indices,
//! the bracket stack's watermark, every slice of a line taken at a byte
//! offset — and three properties beyond that:
//!
//! - **The document path's XHTML is well-formed XML**, always. Raw HTML is
//!   escaped, XML 1.0's forbidden characters are replaced, and an inline that
//!   would nest past `tinker_pdf_xml::limits::MAX_XML_DEPTH` is set without
//!   its element, so the reader never stops: a `Truncated` is a tag, a
//!   character or a depth the translation let through.
//! - **Rendering is deterministic**, ruling 4 over the reader.
//! - **The output is bounded by the input**: no construct may expand a byte
//!   into more than a fixed amount of HTML, so a megabyte cannot ask for a
//!   gigabyte. The bound asserted is generous — a `*` becomes `<em>`, a `<`
//!   becomes `&lt;`, a byte of a destination becomes `%XX` — and is there to
//!   catch a multiplication rather than to describe the encoder. The one
//!   construct that *does* multiply is a reference link, which copies its
//!   definition, and its copies are held to
//!   `MAX_MARKDOWN_REFERENCE_BYTES` or the input's length, so the bound is the
//!   input's multiple plus that budget's.
//!
//! # What this target cannot find, and what covers it instead
//!
//! Whether the HTML is the HTML CommonMark specifies. That is
//! `crates/tinker-pdf/tests/commonmark_spec.rs`, which holds the reader to
//! the specification's own 652 examples when the fetched `spec.txt` is there,
//! and `tests/markdown.rs`, which runs on every `cargo test`.
#![no_main]
use libfuzzer_sys::fuzz_target;

use tinker_pdf::epub::read::markup;
use tinker_pdf::markdown::{to_html, to_xhtml, MAX_MARKDOWN_REFERENCE_BYTES};

fuzz_target!(|data: &[u8]| {
    let text = String::from_utf8_lossy(data);

    let html = to_html(&text);
    assert!(html == to_html(&text), "rendering is not deterministic");
    assert!(
        html.len() <= 64 * text.len() + 8 * text.len().max(MAX_MARKDOWN_REFERENCE_BYTES) + 64,
        "{} bytes of Markdown became {} of HTML",
        text.len(),
        html.len()
    );

    let (xhtml, _) = to_xhtml(&text);
    let dom = markup(xhtml.as_bytes(), &tinker_pdf_xml::Limits::DEFAULT);
    assert!(
        dom.defects.is_empty(),
        "the translation's XHTML is not XML: {:?}",
        dom.defects
    );
});
