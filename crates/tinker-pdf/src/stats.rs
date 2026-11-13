//! Counted work on the public surface: the [`Stats`] a door hands back when
//! it was asked to count, and the glue every door shares.
//!
//! Feature documentation: `docs/features/rendering.md` ("Counted work") for
//! the render door and the counters; `content-and-text.md` for text and image
//! extraction, `writing.md` for a save, a merge and an append and its reopen,
//! and `pdfa.md`, `pdfua.md` and `pdfx.md` for the validators. Design:
//! `docs/design/performance.md`.
//!
//! The tally itself is `tinker-pdf-cos`'s, because the page walk, the filter
//! chain and the editor it rides are there; this crate re-exports the value it
//! reads out as `tinker_pdf::Stats` (ruling 11) and keeps the tally to itself.
//! An operation nobody asked about carries none, and every counting site tests
//! that `Option` and does nothing else.

use std::sync::Arc;

pub use tinker_pdf_cos::Stats;
use tinker_pdf_cos::{pages as cos_pages, CosDocument, CosError, ObjRef, OpenError, Reopened};
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

/// `base` reopened over `update`, an incremental section written after its
/// bytes ([`CosDocument::reopen`]), what the reopen read counted into `work`
/// when the operation is counting: the section's tokens, its rows and the
/// objects it parsed, and everything a full open reads when the reopen opens
/// whole.
pub(crate) fn reopen(
    base: &Arc<CosDocument>,
    update: Arc<[u8]>,
    work: Option<&Arc<Work>>,
) -> Result<Reopened, OpenError> {
    match work {
        Some(work) => CosDocument::reopen_counted(base, update, work),
        None => CosDocument::reopen(base, update),
    }
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

/// Records the bytes the document's cache held at one of the operation's
/// lookups, when the operation is counting: the most it held is
/// [`Stats::cache_bytes_held`].
pub(crate) fn cache_held(work: Option<&Work>, bytes: u64) {
    if let Some(work) = work {
        work.cache_held(bytes);
    }
}

/// Whether a renderer's draws are the render's to count: the per-render
/// statistics of what it drew (`docs/design/page-analysis.md`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Draws {
    /// They are: the page's renderer, or the first band's of a page drawn in
    /// bands.
    Counted,
    /// Another renderer counts them, or none does: a band after the first,
    /// which draws the calls the first band counted, since every band replays
    /// the same calls and answers them alike; or a tiling pattern's cell,
    /// which is a paint of the fill that names it, drawn once for the page to
    /// repeat and only where a canvas needs it.
    Elsewhere,
}

/// Adds what a JPEG 2000 decode did — the inverse-wavelet levels it
/// synthesised, the code-blocks its tier-1 decoded and the levels it dropped
/// — when the operation is counting. The decoder keeps no tally of its own
/// (ruling 8) and hands the first two back as a value (`JpxCounts`), and the
/// third with its image (`JpxImage::discard`).
pub(crate) fn jpx_decoded(
    work: Option<&Work>,
    levels_synthesised: u64,
    code_blocks: u64,
    levels_discarded: u8,
) {
    if let Some(work) = work {
        work.jpx_decoded(levels_synthesised, code_blocks, u64::from(levels_discarded));
    }
}

/// Adds what a renderer counted — its glyphs shown, its outlines painted,
/// its glyph masks rasterised and found in its cache, its shading functions
/// evaluated, the pixels its group buffers and soft masks held beside the
/// regions they were held to, the pixels of its canvas, and, where `draws`
/// says they are the render's, the per-render statistics of what it drew —
/// to the operation's tally, when it is counting. The renderer has no edge
/// to the tally's crate and hands its counts back as a value instead.
pub(crate) fn add_render(work: Option<&Work>, counts: RenderCounts, draws: Draws) {
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
        work.add(Counter::PixelsRendered, counts.pixels_rendered);
        if draws == Draws::Elsewhere {
            return;
        }
        for (counter, n) in [
            (Counter::DrawnGlyphs, counts.drawn_glyphs),
            (Counter::DrawnFills, counts.drawn_fills),
            (Counter::DrawnStrokes, counts.drawn_strokes),
            (Counter::DrawnClips, counts.drawn_clips),
            (Counter::DrawnImages, counts.drawn_images),
            (Counter::DrawnShadings, counts.drawn_shadings),
            (Counter::DrawnGroups, counts.drawn_groups),
            (Counter::DrawnSoftMasks, counts.drawn_soft_masks),
            (Counter::HiddenCalls, counts.hidden_calls),
            (Counter::DecodedImagesDrawn, counts.decoded_images_drawn),
            (Counter::DecodedImageSamples, counts.decoded_image_samples),
        ] {
            work.add(counter, n);
        }
    }
}
