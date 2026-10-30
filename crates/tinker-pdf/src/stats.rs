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

/// Counts one walk of the page tree from its root, when the operation is
/// counting. Called where the facade walks, before the walk: the walk is the
/// step, whatever it finds.
pub(crate) fn walked(work: Option<&Arc<Work>>) {
    count(work.map(|work| &**work), Counter::PageWalks);
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
