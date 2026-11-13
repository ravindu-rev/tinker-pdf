//! The page walks: the pages of one document rendered or read on the
//! engine's pool and handed back in the order they were asked for
//! ([`Document::walk_pages`], [`Document::render_pages`],
//! [`Document::text_pages`]).
//!
//! Feature documentation: `docs/features/rendering.md` ("Page walks") and
//! `docs/features/content-and-text.md`; the design is
//! `docs/design/parallelism.md`.
//!
//! **A page is one operation, as a direct render is.** It builds its
//! resources fresh on the thread it runs on, counts into a tally of its own
//! and reports what it met, so a page's bitmap, warnings and text are the ones
//! [`Page::render`] and [`Page::text_with`] give it serially. The walk shares
//! the one [`Document`] by reference — `Sync`, not a clone a worker — and each
//! worker takes its pages through [`Document::page`], which hands back an
//! owned [`Page`]. Pages come back in the order the caller named them, never
//! in the order they finished, and at most one wave of them — one a worker —
//! is alive at once (`tinker_pdf_pool::waves`), so twenty pages on four
//! threads hold four bitmaps, not twenty.
//!
//! **Where a page ran is counted on its own stats.** A page of a walk is a
//! part of a pooled operation, and the first counted operation it makes — its
//! render when [`RenderOptions::stats`] asked, its counted text — counts that
//! part into its own tally: `pool_items_on_workers` when a worker ran it,
//! `pool_items_on_caller` when the caller did ([`crate::Stats`]). Neither is
//! a step, so the page's `Stats::work` is the one the same page rendered
//! alone totals; and a page rendered outside a walk counts neither.

use std::cell::Cell;

pub use tinker_pdf_pool::Parallelism;
use tinker_pdf_pool::{on_worker, waves};

use crate::stats::{Counter, Work};
use crate::{Bitmap, Document, Page, RenderOptions, TextOptions, TextPage};

thread_local! {
    /// The walk's part this thread is running, until the part's first counted
    /// operation takes it: the counter of where it runs.
    static PART: Cell<Option<Counter>> = const { Cell::new(None) };
}

/// Runs `body` as one part of a page walk, so that the first counted
/// operation it makes counts the part — on a worker or on the caller — into
/// its own tally ([`count_part`]).
fn as_part<T>(body: impl FnOnce() -> T) -> T {
    let here = if on_worker() {
        Counter::PoolItemsOnWorkers
    } else {
        Counter::PoolItemsOnCaller
    };
    let outer = PART.with(|part| part.replace(Some(here)));
    // A walk inside a part runs on its thread, part after part; whatever the
    // inner walk took, the outer part's own mark is as it was.
    let _restore = Restore(outer);
    body()
}

/// Puts the outer part's mark back when an inner part ends, however it ends.
struct Restore(Option<Counter>);

impl Drop for Restore {
    fn drop(&mut self) {
        PART.with(|part| part.set(self.0));
    }
}

/// Counts the page walk's part this thread is running into `work`, the tally
/// of the part's first counted operation, and nothing into any operation after
/// it or outside a walk.
pub(crate) fn count_part(work: &Work) {
    if let Some(counter) = PART.with(Cell::take) {
        work.add(counter, 1);
    }
}

impl Document {
    /// Each of `pages` handed to `work` on the threads `parallelism` names,
    /// and what `work` returned handed to `each` in the order `pages` names
    /// them, on this thread. A page the tree will not hand back reaches
    /// `work` as `None`; an index may be named twice.
    ///
    /// The walk both [`Document::render_pages`] and [`Document::text_pages`]
    /// are, for a caller whose page is more than one door: a page rendered
    /// and written out as a PNG in `work`, on its thread, and its report
    /// printed in `each`, in page order.
    ///
    /// With [`Parallelism::Serial`] — the default everywhere — every page runs
    /// here, in order, and `each` takes it before the next starts. With
    /// `Threads(n)` at most `n` workers start, never more than the pages, and
    /// worker `k` runs the `k`th page of every wave of `n`; each wave is
    /// handed to `each` before the next starts, so at most `n` of `work`'s
    /// results are alive at once. Where a worker will not start — wasm32, a
    /// host at its thread limit — its pages run here, and the output is the
    /// serial output. Called on a worker, the walk runs there.
    pub fn walk_pages<T: Send>(
        &self,
        pages: impl IntoIterator<Item = u32>,
        parallelism: Parallelism,
        work: impl Fn(u32, Option<Page>) -> T + Sync,
        mut each: impl FnMut(u32, T),
    ) {
        let pages: Vec<u32> = pages.into_iter().collect();
        // Every slot `waves` hands back is below `pages.len()`; past it would
        // name no page at all.
        let index = |slot: usize| pages.get(slot).copied().unwrap_or(u32::MAX);
        waves(
            parallelism,
            pages.len(),
            |slot| {
                let page = index(slot);
                as_part(|| work(page, self.page(page)))
            },
            |slot, value| each(index(slot), value),
        );
    }

    /// Each of `pages` rendered as `options` asks, on the threads
    /// [`RenderOptions::parallelism`] names, handed to `each` in the order
    /// `pages` names them, on this thread; a page the tree will not hand back
    /// is `None`.
    ///
    /// Each bitmap is the one [`Page::render`] returns for the page with the
    /// same options, its warnings included: the walk spends its threads on
    /// pages, and each page renders on its thread serially, whatever the
    /// options say. With [`RenderOptions::stats`] each bitmap's stats say
    /// where its page ran ([`crate::Stats::pool_items_on_workers`]). The
    /// bitmaps are handed over as their wave is done
    /// ([`Document::walk_pages`]), so a caller that writes each one out and
    /// drops it holds one wave of them.
    pub fn render_pages(
        &self,
        pages: impl IntoIterator<Item = u32>,
        options: &RenderOptions,
        each: impl FnMut(u32, Option<Bitmap>),
    ) {
        let page_options = RenderOptions {
            parallelism: Parallelism::Serial,
            ..options.clone()
        };
        self.walk_pages(
            pages,
            options.parallelism,
            |_, page| page.map(|page| page.render(&page_options)),
            each,
        );
    }

    /// Each of `pages`' text, as [`Page::text_with`] reads it with `options`,
    /// on the threads [`TextOptions::parallelism`] names, handed to `each` in
    /// the order `pages` names them, on this thread; a page the tree will not
    /// hand back is `None`.
    ///
    /// Each page's text is the one [`Page::text_with`] returns for it, its
    /// warnings included. A caller who wants each page's stats as well walks
    /// [`Page::text_counted`] through [`Document::walk_pages`].
    pub fn text_pages(
        &self,
        pages: impl IntoIterator<Item = u32>,
        options: &TextOptions,
        each: impl FnMut(u32, Option<TextPage>),
    ) {
        let page_options = TextOptions {
            parallelism: Parallelism::Serial,
            ..*options
        };
        self.walk_pages(
            pages,
            options.parallelism,
            |_, page| page.map(|page| page.text_with(&page_options)),
            each,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A part's mark is taken by the first counted operation inside it and by
    /// no other, is the caller's on this thread, and is restored to whatever
    /// the outer part had left when an inner part ends.
    #[test]
    fn a_part_counts_into_its_first_counted_operation_only() {
        let outside = Work::new();
        count_part(&outside);
        assert_eq!(outside.stats().pool_items_on_caller, 0, "no walk, no part");

        as_part(|| {
            let first = Work::new();
            count_part(&first);
            let second = Work::new();
            count_part(&second);
            assert_eq!(
                (
                    first.stats().pool_items_on_caller,
                    second.stats().pool_items_on_caller
                ),
                (1, 0),
                "the first counted operation takes the part"
            );
            // An inner part has a mark of its own, and leaves the outer one
            // as it found it: taken.
            as_part(|| {
                let inner = Work::new();
                count_part(&inner);
                assert_eq!(inner.stats().pool_items_on_caller, 1);
            });
            let after = Work::new();
            count_part(&after);
            assert_eq!(after.stats().pool_items_on_caller, 0);
        });

        as_part(|| {
            as_part(|| {});
            let untouched = Work::new();
            count_part(&untouched);
            assert_eq!(
                untouched.stats().pool_items_on_caller,
                1,
                "an inner part that counted nothing leaves the outer mark in place"
            );
        });
        let after = Work::new();
        count_part(&after);
        assert_eq!(after.stats().pool_items_on_caller, 0, "and none outside");
    }
}
