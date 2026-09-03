//! The whole pipeline: open arbitrary bytes, extract text, and render.
//!
//! The slowest target and the broadest. It is the one that finds panics in
//! the parts no leaf target covers — colour spaces, shadings, resource
//! lookups — because those are only reachable through a document.
//!
//! # What this target checks, and what it does not
//!
//! **Only that the code did not panic, hang, or exhaust memory.** Six calls,
//! every one discarded. A page that rendered blank, upside down, in the wrong
//! colours, or with the wrong glyphs passes. So a run that returned the
//! *wrong* answer passes this target exactly as a correct one does, and a
//! green `cargo fuzz` here is evidence about ruling 1 and about nothing else.
//!
//! That is worth writing down rather than leaving implied. Correctness lives
//! in the rendered comparisons under `crates/tinker-pdf/tests/` and in the
//! corpus ratchet. This is the broadest target in the tree and the one whose
//! green run is most easily mistaken for more than it is.
//!
//! Recorded because the same shape has already cost this repository once: the
//! `brotli` target asserts only self-consistency and could not have found the
//! ring-buffer defect that a decoded-bytes comparison found immediately.
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
    // 14.7's `/K` graph and `/RoleMap` rewriting system, both attacker-shaped
    // rather than merely attacker-corrupted, and the join over the same
    // `TextPage` the line above already built.
    if let Some(tree) = doc.structure() {
        let _ = tree.element_count();
        let _ = tree.text_for_page(0, &page.text()).plain_text();
    }

    // A low resolution on purpose: a fuzzer's inputs claim enormous page
    // boxes, and the interesting failures are in the operators rather than in
    // how many pixels they cover.
    let _ = page.render(&RenderOptions::at_dpi(12.0));
});
