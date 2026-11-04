//! Counted work on the public surface: the [`Stats`] a door hands back when
//! it was asked to count, and the glue every door shares.
//!
//! Feature documentation: `docs/features/rendering.md` ("Counted work") for
//! the render door and the counters; `content-and-text.md` for text and image
//! extraction, `writing.md` for a save and a merge, and `pdfa.md`, `pdfua.md`
//! and `pdfx.md` for the validators. Design: `docs/design/performance.md`.
//!
//! The tally itself is `tinker-pdf-cos`'s, because the page walk, the filter
//! chain and the editor it rides are there; this crate re-exports the value it
//! reads out as `tinker_pdf::Stats` (ruling 11) and keeps the tally to itself.
//! An operation nobody asked about carries none, and every counting site tests
//! that `Option` and does nothing else.

use std::collections::HashMap;
use std::sync::Arc;

use tinker_pdf_cos::font::Font;
pub use tinker_pdf_cos::Stats;
use tinker_pdf_cos::{
    font as cos_font, pages as cos_pages, CosDocument, CosError, Counting, Dict, Name, ObjRef,
};
pub(crate) use tinker_pdf_cos::{Counter, Work};
use tinker_pdf_render::RenderCounts;

/// The tally for one operation: a fresh one when the caller asked, none when
/// not.
pub(crate) fn tally(asked: bool) -> Option<Arc<Work>> {
    asked.then(|| Arc::new(Work::new()))
}

/// Counts one step of `counter`, when the operation is counting.
pub(crate) fn count(work: Option<&Work>, counter: Counter) {
    if let Some(work) = work {
        work.add(counter, 1);
    }
}

/// Every page of `doc`, by a walk counted into `work` when the operation is
/// counting: the walk, and each node of the tree it reads.
pub(crate) fn pages(doc: &CosDocument, work: Option<&Arc<Work>>) -> Vec<cos_pages::Page> {
    cos_pages::collect_counted(doc, work.map(|work| &**work))
}

/// [`pages`], stopped at `limit` pages.
pub(crate) fn pages_upto(
    doc: &CosDocument,
    limit: usize,
    work: Option<&Arc<Work>>,
) -> Vec<cos_pages::Page> {
    cos_pages::collect_upto_counted(doc, limit, work.map(|work| &**work))
}

/// `r`'s bytes through its filter chain, the run counted into `work` when the
/// operation is counting.
pub(crate) fn decoded(
    doc: &CosDocument,
    r: ObjRef,
    work: Option<&Arc<Work>>,
) -> Result<Vec<u8>, CosError> {
    doc.stream_decoded_counted(r, work.map(|work| &**work))
}

/// The document's XMP packet, its decode counted into `work` when the
/// operation is counting.
pub(crate) fn xmp(doc: &CosDocument, work: Option<&Arc<Work>>) -> Option<Vec<u8>> {
    tinker_pdf_cos::xmp_metadata_counted(doc, work.map(|work| &**work))
}

/// A page's content streams decoded and joined, each part's run counted into
/// `work` when the operation is counting.
pub(crate) fn content(
    doc: &CosDocument,
    page: &cos_pages::Page,
    work: Option<&Arc<Work>>,
) -> Vec<u8> {
    cos_pages::content_bytes_counted(doc, page, work.map(|work| &**work))
}

/// The fonts a resource dictionary names, each read as a scope reads it; when
/// the operation is counting, through a view that counts what reading a font
/// decodes — its CMaps, its `/ToUnicode` and its `/CIDToGIDMap`.
pub(crate) fn fonts(
    doc: &CosDocument,
    resources: &Dict,
    work: Option<&Work>,
) -> HashMap<Name, Arc<Font>> {
    match work {
        Some(work) => cos_font::from_resources_in(&Counting::new(doc, work), resources),
        None => cos_font::from_resources(doc, resources),
    }
}

/// Adds what a renderer counted — its glyphs shown and outlines painted — to
/// the operation's tally, when it is counting. The renderer has no edge to the
/// tally's crate and hands its counts back as a value instead.
pub(crate) fn add_render(work: Option<&Work>, counts: RenderCounts) {
    if let Some(work) = work {
        work.add(Counter::GlyphsShown, counts.glyphs_shown);
        work.add(Counter::OutlinesFilled, counts.outlines_filled);
    }
}
