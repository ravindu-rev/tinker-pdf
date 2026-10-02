//! One-file documents that are not PDFs (tier 5's formats row): the sniff that
//! decides a standalone SVG, a loose XHTML file or a bare image from its first
//! bytes, the routes behind it into the EPUB reader and the comic path's image
//! embedders, and the RFC 2397 `data:` URL and RFC 4648 base64 readers a loose
//! file's references go through.
//!
//! The control byte picks the **route** rather than the input. Bytes arrive as
//! they are on `0`, so the sniff itself is under test — the XML prolog walk,
//! the doctype's internal subset, the PDF-header window — and are wrapped in an
//! `<svg>` root on `1` and an XHTML `<html><body>` on `2`, because a mutator
//! that has to rediscover a root element before every iteration spends its
//! time on the sniff and never reaches the cascade or the scene. `3` hands the
//! body to the two URL readers alone.
//!
//! # What this target checks
//!
//! **That nothing panics, hangs or exhausts memory** (ruling 1) over every
//! route, and four properties beyond that:
//!
//! - **A sniffed document opens.** Nothing on these routes refuses a document
//!   the sniff recognised except a synthesised file past
//!   `MAX_SYNTHESISED_PDF`, which these inputs cannot reach: an unreadable
//!   SVG, tag soup and an undecodable picture are each a page saying so. An
//!   `Err` here is a refusal the module comment says does not exist.
//! - **A PDF header in the first kilobyte wins**: the sniff answers `None`,
//!   so a polyglot stays a PDF.
//! - **Opening is deterministic** — the same page count and the same warnings
//!   from the same bytes — which is ruling 4 over the reader rather than over
//!   a rendered page.
//! - **base64 never grows**: what comes out is at most three bytes for every
//!   four characters that went in, and a `data:` URL's bytes are never more
//!   than the URL's own, so a reference cannot choose how much memory it costs.
//!
//! # What this target cannot find, and what covers it instead
//!
//! Whether the page is the picture the file describes. That is
//! `crates/tinker-pdf/tests/standalone.rs`, which holds a loose XHTML file to
//! the pixels of the same file as an EPUB chapter, a bare PNG to the pixels of
//! the same file as a comic page, and the decoders to RFC 4648's and RFC
//! 2397's own examples.
#![no_main]
use libfuzzer_sys::fuzz_target;

use tinker_pdf::standalone::{base64_decode, data_url, sniff};
use tinker_pdf::{Document, RenderOptions};

fuzz_target!(|data: &[u8]| {
    let (control, body) = data.split_at(data.len().min(1));
    let knob = control.first().copied().unwrap_or(0);

    let input: Vec<u8> = match knob & 3 {
        0 => body.to_vec(),
        1 => [
            b"<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"40\" height=\"40\">".as_slice(),
            body,
            b"</svg>",
        ]
        .concat(),
        2 => [
            b"<html xmlns=\"http://www.w3.org/1999/xhtml\"><body>".as_slice(),
            body,
            b"</body></html>",
        ]
        .concat(),
        _ => {
            if let Some(out) = base64_decode(body) {
                assert!(
                    out.len() <= body.len() / 4 * 3 + 2,
                    "{} bytes of base64 decoded to {}",
                    body.len(),
                    out.len()
                );
            }
            if let Ok(text) = core::str::from_utf8(body) {
                let url = format!("data:{text}");
                if let Some(out) = data_url(&url) {
                    assert!(out.len() <= url.len(), "a data: URL grew");
                }
            }
            return;
        }
    };

    let head = input.get(..input.len().min(1024)).unwrap_or_default();
    let kind = sniff(&input);
    if head.windows(5).any(|w| w == b"%PDF-") {
        assert!(kind.is_none(), "a PDF header was sniffed as {kind:?}");
    }
    assert_eq!(kind, sniff(&input), "the sniff is not deterministic");
    let Some(_) = kind else {
        return;
    };

    let opened = Document::open(input.clone());
    let doc = match opened {
        Ok(doc) => doc,
        Err(error) => panic!("a sniffed document was refused: {error:?}"),
    };
    let again = Document::open(input).expect("the same bytes refused on a second run");
    assert_eq!(doc.page_count(), again.page_count(), "the page count moved");
    let warnings = doc.archive().map(|report| report.warnings().to_vec());
    let warnings_again = again.archive().map(|report| report.warnings().to_vec());
    assert!(warnings == warnings_again, "the warnings moved");

    if let Some(page) = doc.page(0) {
        let _ = page.text();
        let _ = page.links();
        let _ = page.render(&RenderOptions {
            scale: 0.1,
            ..RenderOptions::default()
        });
    }
});
