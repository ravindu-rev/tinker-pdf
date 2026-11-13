//! Render against rewrite: a generated document drawn, saved, reopened and
//! drawn again, every byte the same.
//!
//! This target reads its input as choices (`tinker_pdf_fuzz::Choices`) and
//! writes a document with `tinker_pdf_fuzz::document`: the page
//! `tinker_pdf_fuzz::content` generates — every operator of Annex A over
//! fonts, images, forms, patterns, shadings, colour spaces and annotations —
//! laid out in one to four sections, each a cross-reference table or stream,
//! objects packed into object streams or not, updates writing some of the
//! page's objects again as they were and adding and freeing objects nothing
//! refers to, and every token spelled one of the ways 7.3 allows. The render's
//! options are generated with the page.
//!
//! The relation is asked of a document read cleanly — on
//! `LadderLevel::Trust`, with no warning, not encrypted — as the corpus
//! relations ask theirs, since a rewrite of a document the reader repaired
//! compares two repairs rather than two renders. Such a document is rewritten
//! as they rewrite one, `WriteMode::Rewrite`, and reopened.
//!
//! The caps are `tinker_pdf_fuzz::content`'s — 192 operators in all, images of
//! eight samples a side, a page of 72 points a side — and four sections.
//!
//! What is asserted beyond "it did not panic":
//!
//! - **A generated document opens without repair.** Every length is honest
//!   and every offset true, so a document that needed the leniency ladder is
//!   the reader's defect or the writer's.
//! - **The rewrite reopens, with the same pages.** A save the reader cannot
//!   open, or one holding another number of pages, fails.
//! - **Every page draws the same before and after.** For the generated
//!   options, the reopened page's bitmap has the original's width, height,
//!   stride, format and alpha convention, every byte, and the same warnings:
//!   a rewrite that dropped an object a page names, renumbered a resource
//!   onto another, or rewrote a value a draw reads differs here.
//!
//! # What this target cannot find, and what covers it instead
//!
//! Both renders are this engine's, so a page drawn wrongly the same way
//! twice passes, and so does a rewrite that is not valid PDF the engine reads
//! anyway. The strict validator (`docs/verification.md`, the written-files
//! helper) holds what the writer writes to the standard, and the analytic
//! tier and the goldens what a page draws.
#![no_main]
use libfuzzer_sys::fuzz_target;

use tinker_pdf::{Bitmap, Document, LadderLevel, WriteMode, WriteOptions};
use tinker_pdf_fuzz::{document, Choices};

fuzz_target!(|data: &[u8]| {
    let generated = document::document(&mut Choices::new(data));
    let doc = Document::open(generated.bytes)
        .unwrap_or_else(|error| panic!("a generated document did not open: {error}"));
    assert_eq!(
        doc.ladder_level(),
        LadderLevel::Trust,
        "a document written clean needed the leniency ladder"
    );
    let pages = doc.page_count();
    let before: Vec<Bitmap> = (0..pages)
        .map(|at| {
            doc.page(at)
                .unwrap_or_else(|| panic!("page {at} of {pages} did not load"))
                .render(&generated.options)
        })
        .collect();
    // Read cleanly, objects loaded by the draw included.
    if !doc.warnings().is_empty() || doc.is_encrypted() {
        return;
    }

    let saved = doc.editor().save(&WriteOptions {
        mode: WriteMode::Rewrite,
        ..WriteOptions::default()
    });
    let again =
        Document::open(saved).unwrap_or_else(|error| panic!("the rewrite did not reopen: {error}"));
    assert_eq!(
        again.page_count(),
        pages,
        "the rewrite holds another number of pages"
    );
    for (at, before) in before.iter().enumerate() {
        let page = again
            .page(at as u32)
            .unwrap_or_else(|| panic!("page {at} of the rewrite did not load"));
        let after = page.render(&generated.options);
        same(at, before, &after);
    }
});

/// The rewrite's page draws the original's: its shape, every byte, and its
/// warnings.
fn same(page: usize, before: &Bitmap, after: &Bitmap) {
    assert_eq!(
        (
            before.width,
            before.height,
            before.stride,
            before.format,
            before.premultiplied
        ),
        (
            after.width,
            after.height,
            after.stride,
            after.format,
            after.premultiplied
        ),
        "page {page} of the rewrite draws a bitmap of another shape"
    );
    if before.data != after.data {
        let at = before
            .data
            .iter()
            .zip(&after.data)
            .position(|(a, b)| a != b)
            .unwrap_or(before.data.len().min(after.data.len()));
        let stride = before.stride.max(1);
        panic!(
            "page {page} of the rewrite draws byte {at} (row {}, column byte {}) as {:?}, \
             the original as {:?}",
            at / stride,
            at % stride,
            after.data.get(at),
            before.data.get(at)
        );
    }
    assert_eq!(
        before.warnings, after.warnings,
        "page {page} of the rewrite warns otherwise"
    );
}
