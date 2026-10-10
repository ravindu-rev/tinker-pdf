//! FictionBook 2 (tier 5's FB2 row): the translation of an FB2 document into
//! the XHTML the EPUB reader lays out, and the document that comes of it.
//!
//! The control byte picks the input's **frame** rather than the input: on `0`
//! the body is the whole document, so the XML prolog, the root, the
//! `<description>` and the `<binary>` elements are all the mutator's; on `1`
//! it is wrapped in a `FictionBook` root and a `<body>`, so a mutator spends
//! its time on sections, poems, tables and notes rather than on
//! rediscovering a root element.
//!
//! # What this target checks
//!
//! **That nothing panics, hangs or exhausts memory** (ruling 1) — the element
//! stack that pairs every end tag with what its start wrote, the dropped-
//! element depth the description and the binaries are read under, base64 over
//! a binary's text — and two properties beyond that:
//!
//! - **The translation is well-formed XML**, every time it translates at all.
//!   Every element it writes is closed, every attribute is quoted and escaped,
//!   and text is escaped, so the XHTML reader stops only where the FB2 did —
//!   at the depth cap, which a deep FB2 reaches by design. Anything else is a
//!   tag the translation wrote wrong.
//! - **A sniffed FB2 opens.** Nothing on the path refuses one but the
//!   synthesised-document ceiling, which these inputs cannot reach.
//!
//! # What this target cannot find, and what covers it instead
//!
//! Whether a section is set as a section. That is
//! `crates/tinker-pdf/tests/fb2.rs`, which holds an FB2 to the pixels of the
//! XHTML it translates to and its words, title, author, pictures and notes to
//! the book it was written as.
#![no_main]
use libfuzzer_sys::fuzz_target;

use tinker_pdf::epub::read::markup;
use tinker_pdf::{Document, Standalone};

fuzz_target!(|data: &[u8]| {
    let (control, body) = data.split_at(data.len().min(1));
    let knob = control.first().copied().unwrap_or(0);
    let input: Vec<u8> = if knob & 1 == 0 {
        body.to_vec()
    } else {
        [
            b"<FictionBook xmlns=\"http://www.gribuser.ru/xml/fictionbook/2.0\" \
              xmlns:l=\"http://www.w3.org/1999/xlink\"><body>"
                .as_slice(),
            body,
            b"</body></FictionBook>",
        ]
        .concat()
    };

    if let Some(xhtml) = tinker_pdf::fb2::to_xhtml(&input) {
        let dom = markup(xhtml.as_bytes(), &tinker_pdf_xml::Limits::DEFAULT);
        if !dom.defects.is_empty() {
            let deep = dom.nodes.iter().any(|node| {
                let mut depth = 0;
                let mut at = node.parent;
                while let Some(parent) = at {
                    depth += 1;
                    at = dom.nodes.get(parent).and_then(|n| n.parent);
                }
                depth + 2 >= tinker_pdf_xml::limits::MAX_XML_DEPTH
            });
            assert!(deep, "the translation is not XML: {:?}", dom.defects);
        }
    }

    if tinker_pdf::standalone::sniff(&input) == Some(Standalone::Fb2) {
        let doc = match Document::open(input) {
            Ok(doc) => doc,
            Err(error) => panic!("a sniffed FB2 was refused: {error:?}"),
        };
        let _ = doc.page_count();
        if let Some(page) = doc.page(0) {
            let _ = page.text();
        }
    }
});
