//! The whole file parser: the target that matters most.
//!
//! Everything the ladder does — trusting an xref, patching it, rescanning the
//! file from scratch — is reachable from arbitrary bytes, and none of it may
//! panic. Walking the page tree afterwards is part of the target because a
//! document that opens and then panics on use is no better than one that
//! panics on open.
//!
//! # What this target checks, and what it does not
//!
//! **Only that the code did not panic, hang, or exhaust memory.** Eleven
//! calls, every one discarded. A document whose xref was repaired *wrongly* —
//! pointing at the wrong objects, losing a page, resolving a reference to the
//! wrong generation — opens without a panic and passes. So a run that
//! returned the *wrong* answer passes this target exactly as a correct one
//! does, and a green `cargo fuzz` here is evidence about ruling 1 and about
//! nothing else.
//!
//! That is worth writing down rather than leaving implied. Correctness lives
//! in the corpus ratchet (`corpus/ratchet*.json`, which pins per-document
//! outcomes across hundreds of real files) and in `crates/tinker-pdf`'s own
//! document tests. This target is the broadest *crash* net in the tree and
//! makes no claim beyond that.
//!
//! Recorded because the same shape has already cost this repository once: the
//! `brotli` target asserts only self-consistency and could not have found the
//! ring-buffer defect that a decoded-bytes comparison found immediately.
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
    // The strict validator walks the sections itself rather than reading the
    // merged table, so it is a second cross-reference parser over the same
    // arbitrary bytes and needs the same guarantee.
    let _ = tinker_pdf_cos::validate(&doc);

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
