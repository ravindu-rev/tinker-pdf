//! The whole pipeline: open arbitrary bytes, extract text, and render.
//!
//! The slowest target and the broadest. It is the one that finds panics in
//! the parts no leaf target covers — colour spaces, shadings, resource
//! lookups — because those are only reachable through a document.
#![no_main]
use libfuzzer_sys::fuzz_target;

use tinker_pdf::{Document, RenderOptions};

fuzz_target!(|data: &[u8]| {
    let Ok(doc) = Document::open(data.to_vec()) else {
        return;
    };
    let Some(page) = doc.page(0) else {
        return;
    };

    let _ = page.text().plain_text();
    let _ = doc.form_fields();
    let _ = doc.outline();

    // A low resolution on purpose: a fuzzer's inputs claim enormous page
    // boxes, and the interesting failures are in the operators rather than in
    // how many pixels they cover.
    let _ = page.render(&RenderOptions::at_dpi(12.0));
});
