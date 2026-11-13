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

use std::sync::Arc;

pub use tinker_pdf_cos::Stats;
use tinker_pdf_cos::{pages as cos_pages, CosDocument, CosError, ObjRef};
pub(crate) use tinker_pdf_cos::{Counter, Work};
use tinker_pdf_render::RenderCounts;

/// The tally for one operation: a fresh one when the caller asked, none when
/// not.
pub(crate) fn tally(asked: bool) -> Option<Arc<Work>> {
    asked.then(|| Arc::new(begin()))
}

/// A fresh tally for one counted operation — every door's, whether it was
/// asked for by a switch or is a counted sibling. Made inside a page walk's
/// part, the first one counts the part into itself: where the page ran
/// (`crate::parallel`).
pub(crate) fn begin() -> Work {
    let work = Work::new();
    crate::parallel::count_part(&work);
    work
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

/// Records what a PNG write held, when the operation is counting:
/// `image_scratch` bytes of buffers sized from the image, and `peak` bytes in
/// all, the file included.
pub(crate) fn png_held(work: Option<&Work>, image_scratch: usize, peak: usize) {
    if let Some(work) = work {
        let bytes = |n: usize| u64::try_from(n).unwrap_or(u64::MAX);
        work.png_held(bytes(image_scratch), bytes(peak));
    }
}

/// Records what a JPEG decode held beside the image it decoded into, when
/// the operation is counting.
pub(crate) fn jpeg_held(work: Option<&Work>, bytes: usize) {
    if let Some(work) = work {
        work.jpeg_held(u64::try_from(bytes).unwrap_or(u64::MAX));
    }
}

/// Adds the samples a JPEG decode's inverse transforms output, when the
/// operation is counting.
pub(crate) fn jpeg_transformed(work: Option<&Work>, samples: u64) {
    if let Some(work) = work {
        work.jpeg_transformed(samples);
    }
}

/// Adds what a renderer counted — its glyphs shown, its outlines painted,
/// its glyph masks rasterised and found in its cache, its shading functions
/// evaluated, and the pixels its group buffers and soft masks held beside the
/// regions they were held to — to the operation's tally, when it is
/// counting. The renderer has no edge to the tally's crate and hands its
/// counts back as a value instead.
pub(crate) fn add_render(work: Option<&Work>, counts: RenderCounts) {
    if let Some(work) = work {
        work.add(Counter::GlyphsShown, counts.glyphs_shown);
        work.add(Counter::OutlinesFilled, counts.outlines_filled);
        work.add(Counter::GlyphMasksRasterised, counts.glyph_masks_rasterised);
        work.add(Counter::GlyphMaskHits, counts.glyph_mask_hits);
        work.add(
            Counter::ShadingFunctionsEvaluated,
            counts.shading_functions_evaluated,
        );
        work.add(Counter::GroupBufferPixels, counts.group_buffer_pixels);
        work.add(Counter::GroupBoundPixels, counts.group_bound_pixels);
        work.add(Counter::SoftMaskPixels, counts.soft_mask_pixels);
        work.add(Counter::SoftMaskBoundPixels, counts.soft_mask_bound_pixels);
    }
}
