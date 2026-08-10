//! The whole file parser: the target that matters most.
//!
//! Everything the ladder does — trusting an xref, patching it, rescanning the
//! file from scratch — is reachable from arbitrary bytes, and none of it may
//! panic. Walking the page tree afterwards is part of the target because a
//! document that opens and then panics on use is no better than one that
//! panics on open.
#![no_main]
use libfuzzer_sys::fuzz_target;

use tinker_pdf_cos::{pages, CosDocument};

fuzz_target!(|data: &[u8]| {
    let Ok(doc) = CosDocument::open(data) else {
        return;
    };

    let _ = doc.trailer();
    let _ = doc.catalog();
    let _ = doc.ladder_level();
    let _ = doc.warnings();
    let _ = doc.header_version();
    let _ = doc.permissions();

    // Bounded, because a fuzzer will happily produce a file claiming millions
    // of pages and the point is to find panics, not to time out.
    for page in pages::collect(&doc).iter().take(8) {
        let _ = pages::content_bytes(&doc, page);
        let _ = page.display_size();
    }

    for num in 1..=doc.max_object_number().min(64) {
        let r = tinker_pdf_cos::ObjRef::new(num, 0);
        let _ = doc.get(r);
        let _ = doc.stream_decoded(r);
    }
});
