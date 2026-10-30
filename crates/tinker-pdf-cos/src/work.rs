//! The counted-work tally: plain counters one operation carries from the door
//! that was asked for them to the door that hands them back.
//!
//! Design: `docs/design/performance.md`, "The tally, and the value a door
//! hands out". Feature documentation: `docs/features/rendering.md`, whose
//! render door is the first to hand a [`Stats`] out.
//!
//! A count is a number of steps of one named kind that one operation took — a
//! font dictionary read, a cache asked — and it depends on nothing but the
//! binary and its input: no clock, no thread and no hash order. That is what
//! lets a guard over one hold on a shared runner and under every target, where
//! a bound on wall-clock time cannot.
//!
//! [`Work`] is the tally and [`Stats`] the value read out of it. **Nothing
//! reads a `Work` but a door turning it into `Stats`**, which is the whole of
//! why a counted operation's output cannot differ from an uncounted one's. An
//! operation nobody asked about carries no `Work` at all: nothing counts,
//! nothing is locked or allocated, and the only cost is the `Option` each
//! counting site tests.
//!
//! It lives here, in the lowest crate with an operation context, although what
//! is counted so far is counted above it — glyphs by the renderer, font
//! dictionaries and cache lookups by the facade's resource scopes, page walks
//! where the facade makes them. One ledger an operation carries through every
//! context is the reason, and each counter says which layer counts it. The
//! editor is the context this crate gives it: a [`crate::DocumentEditor`]
//! asked to count holds one tally, which the passes over it count into.

use std::ops::{Add, AddAssign};
use std::sync::Mutex;

/// What one operation counted: the value a door hands back when asked.
///
/// Every field is a count of steps of one kind, a public `u64`. **It is
/// `#[non_exhaustive]`** so that the counters later rows add — pages walked,
/// streams decoded, masks rasterised — are additions rather than breaks: read
/// a field by name, total them with [`Stats::work`], and sum two with `+`.
///
/// A count is what the operation did, not what it was worth: a cache lookup
/// counts whether or not the entry it found held anything, and a glyph shown
/// counts whether or not anything could be drawn for it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct Stats {
    /// Glyphs the interpreter showed the renderer, one for each
    /// `Device::show_glyph` call — a glyph in a hidden layer or an invisible
    /// mode included, since it was shown. A Type 3 glyph runs its procedure
    /// instead and is not one.
    ///
    /// Counted by `tinker-pdf-render`'s renderer, a tiling pattern's cell
    /// included, and added to the operation's tally by the facade.
    pub glyphs_shown: u64,
    /// Glyph outlines the renderer built into a path and painted — filled,
    /// stroked or both, once a glyph. A glyph with no outline, an empty one
    /// (a space) and one only clipped with are not.
    ///
    /// Counted by the renderer, as `glyphs_shown` is.
    pub outlines_filled: u64,
    /// Font dictionaries read: each font a resource scope resolves when it is
    /// built, used or not, and each lookup of one font dictionary by its
    /// resource name — which today is made for every glyph the interpreter
    /// meets, to ask whether its font is a Type 3 font, and once a font when a
    /// substitute is requested for it.
    ///
    /// Counted by the facade's resource scopes.
    pub font_dictionaries_resolved: u64,
    /// Image decoders run. In a render, an image XObject's on a cache miss,
    /// and every inline image's, which is never cached; in an image
    /// extraction, one for each entry it lists — an image's masks part of
    /// its decode — and every inline image's.
    ///
    /// Counted by the facade's resource scopes.
    pub images_decoded: u64,
    /// Lookups of a font's program that found it decoded already, or found it
    /// recorded as absent.
    ///
    /// This and every hit and miss below are counted by the facade's resource
    /// scopes, against the five caches a scope keeps.
    pub font_program_hits: u64,
    /// Lookups of a font's program that had to read it.
    pub font_program_misses: u64,
    /// Lookups of an image XObject that found it decoded already, or found
    /// its decoder's refusal.
    pub image_hits: u64,
    /// Lookups of an image XObject that had to decode it.
    pub image_misses: u64,
    /// Lookups of an ICC profile that found its transform compiled already,
    /// or found it refused.
    pub icc_transform_hits: u64,
    /// Lookups of an ICC profile that had to read and compile it.
    pub icc_transform_misses: u64,
    /// Lookups of a glyph's outline that found it extracted already, or found
    /// it recorded as absent.
    pub glyph_outline_hits: u64,
    /// Lookups of a glyph's outline that had to extract it.
    pub glyph_outline_misses: u64,
    /// Lookups of a form XObject's resource scope that found it built already,
    /// or found that the form brings none.
    pub form_scope_hits: u64,
    /// Lookups of a form XObject's resource scope that had to build it.
    pub form_scope_misses: u64,
    /// Walks of the page tree from its root: each listing of a document's
    /// pages, whatever it found.
    ///
    /// Counted by the facade where it walks: each validator rule that reads
    /// every page, the structure tree's binding, and a save's image and font
    /// passes. The walks an editor makes of its own pages are not counted.
    pub page_walks: u64,
}

/// One kind of step a [`Work`] counts: a [`Stats`] field, by name.
///
/// `#[non_exhaustive]` for the reason `Stats` is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Counter {
    /// [`Stats::glyphs_shown`].
    GlyphsShown,
    /// [`Stats::outlines_filled`].
    OutlinesFilled,
    /// [`Stats::font_dictionaries_resolved`].
    FontDictionariesResolved,
    /// [`Stats::images_decoded`].
    ImagesDecoded,
    /// [`Stats::font_program_hits`].
    FontProgramHits,
    /// [`Stats::font_program_misses`].
    FontProgramMisses,
    /// [`Stats::image_hits`].
    ImageHits,
    /// [`Stats::image_misses`].
    ImageMisses,
    /// [`Stats::icc_transform_hits`].
    IccTransformHits,
    /// [`Stats::icc_transform_misses`].
    IccTransformMisses,
    /// [`Stats::glyph_outline_hits`].
    GlyphOutlineHits,
    /// [`Stats::glyph_outline_misses`].
    GlyphOutlineMisses,
    /// [`Stats::form_scope_hits`].
    FormScopeHits,
    /// [`Stats::form_scope_misses`].
    FormScopeMisses,
    /// [`Stats::page_walks`].
    PageWalks,
}

impl Stats {
    /// The total of every counter that counts a step taken — the figure a
    /// guard compares at two sizes of one input.
    ///
    /// Every counter `Stats` has today is one. A maximum or a quantity a later
    /// row adds — the most decodes any one stream had, the bytes a filter
    /// chain produced — is left out, because a byte total grows with the page
    /// and would hide a step count that grows with the document.
    #[must_use]
    pub fn work(&self) -> u64 {
        [
            self.glyphs_shown,
            self.outlines_filled,
            self.font_dictionaries_resolved,
            self.images_decoded,
            self.font_program_hits,
            self.font_program_misses,
            self.image_hits,
            self.image_misses,
            self.icc_transform_hits,
            self.icc_transform_misses,
            self.glyph_outline_hits,
            self.glyph_outline_misses,
            self.form_scope_hits,
            self.form_scope_misses,
            self.page_walks,
        ]
        .into_iter()
        .fold(0u64, u64::saturating_add)
    }

    /// The field `counter` names.
    fn slot(&mut self, counter: Counter) -> &mut u64 {
        match counter {
            Counter::GlyphsShown => &mut self.glyphs_shown,
            Counter::OutlinesFilled => &mut self.outlines_filled,
            Counter::FontDictionariesResolved => &mut self.font_dictionaries_resolved,
            Counter::ImagesDecoded => &mut self.images_decoded,
            Counter::FontProgramHits => &mut self.font_program_hits,
            Counter::FontProgramMisses => &mut self.font_program_misses,
            Counter::ImageHits => &mut self.image_hits,
            Counter::ImageMisses => &mut self.image_misses,
            Counter::IccTransformHits => &mut self.icc_transform_hits,
            Counter::IccTransformMisses => &mut self.icc_transform_misses,
            Counter::GlyphOutlineHits => &mut self.glyph_outline_hits,
            Counter::GlyphOutlineMisses => &mut self.glyph_outline_misses,
            Counter::FormScopeHits => &mut self.form_scope_hits,
            Counter::FormScopeMisses => &mut self.form_scope_misses,
            Counter::PageWalks => &mut self.page_walks,
        }
    }
}

/// Counts sum, saturating: a caller's loop of doors adds up to the loop's
/// totals.
impl Add for Stats {
    type Output = Stats;

    fn add(self, other: Stats) -> Stats {
        Stats {
            glyphs_shown: self.glyphs_shown.saturating_add(other.glyphs_shown),
            outlines_filled: self.outlines_filled.saturating_add(other.outlines_filled),
            font_dictionaries_resolved: self
                .font_dictionaries_resolved
                .saturating_add(other.font_dictionaries_resolved),
            images_decoded: self.images_decoded.saturating_add(other.images_decoded),
            font_program_hits: self
                .font_program_hits
                .saturating_add(other.font_program_hits),
            font_program_misses: self
                .font_program_misses
                .saturating_add(other.font_program_misses),
            image_hits: self.image_hits.saturating_add(other.image_hits),
            image_misses: self.image_misses.saturating_add(other.image_misses),
            icc_transform_hits: self
                .icc_transform_hits
                .saturating_add(other.icc_transform_hits),
            icc_transform_misses: self
                .icc_transform_misses
                .saturating_add(other.icc_transform_misses),
            glyph_outline_hits: self
                .glyph_outline_hits
                .saturating_add(other.glyph_outline_hits),
            glyph_outline_misses: self
                .glyph_outline_misses
                .saturating_add(other.glyph_outline_misses),
            form_scope_hits: self.form_scope_hits.saturating_add(other.form_scope_hits),
            form_scope_misses: self
                .form_scope_misses
                .saturating_add(other.form_scope_misses),
            page_walks: self.page_walks.saturating_add(other.page_walks),
        }
    }
}

impl AddAssign for Stats {
    fn add_assign(&mut self, other: Stats) {
        *self = *self + other;
    }
}

/// The tally one operation carries, from the door that creates it to the door
/// that reads it out as [`Stats`].
///
/// Counted through `&self`, because the contexts it rides are reached through
/// shared references — the interpreter and the renderer hold a page's
/// resources behind `&` — and behind one lock, because those contexts may be
/// shared between threads: a retained page is `Send + Sync` and keeps its
/// resources. The lock is held for one addition; one that will not open drops
/// the count rather than panicking, as the caches beside it drop an entry.
///
/// One `Work` an operation, never one a document: a document is read by many
/// threads at once, and a count kept on it would mix their operations.
#[derive(Debug, Default)]
pub struct Work {
    counts: Mutex<Stats>,
}

impl Work {
    /// A tally that has counted nothing.
    #[must_use]
    pub fn new() -> Work {
        Work::default()
    }

    /// Adds `n` steps of one kind, saturating.
    pub fn add(&self, counter: Counter, n: u64) {
        if let Ok(mut counts) = self.counts.lock() {
            let slot = counts.slot(counter);
            *slot = slot.saturating_add(n);
        }
    }

    /// What has been counted so far.
    #[must_use]
    pub fn stats(&self) -> Stats {
        self.counts.lock().map(|counts| *counts).unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every counter, so a test of one property covers them all.
    const ALL: [Counter; 15] = [
        Counter::GlyphsShown,
        Counter::OutlinesFilled,
        Counter::FontDictionariesResolved,
        Counter::ImagesDecoded,
        Counter::FontProgramHits,
        Counter::FontProgramMisses,
        Counter::ImageHits,
        Counter::ImageMisses,
        Counter::IccTransformHits,
        Counter::IccTransformMisses,
        Counter::GlyphOutlineHits,
        Counter::GlyphOutlineMisses,
        Counter::FormScopeHits,
        Counter::FormScopeMisses,
        Counter::PageWalks,
    ];

    #[test]
    fn each_counter_counts_into_its_own_field_and_into_the_total() {
        for (index, counter) in ALL.into_iter().enumerate() {
            let work = Work::new();
            let n = index as u64 + 1;
            work.add(counter, n);
            let mut expected = Stats::default();
            *expected.slot(counter) = n;
            let stats = work.stats();
            assert_eq!(stats, expected, "{counter:?} counted somewhere else");
            assert_eq!(stats.work(), n, "{counter:?} missing from the total");
        }
    }

    #[test]
    fn counts_saturate_rather_than_wrap() {
        let work = Work::new();
        work.add(Counter::GlyphsShown, u64::MAX);
        work.add(Counter::GlyphsShown, 2);
        work.add(Counter::ImageHits, 3);
        let stats = work.stats();
        assert_eq!(stats.glyphs_shown, u64::MAX);
        assert_eq!(stats.work(), u64::MAX);
        assert_eq!((stats + stats).image_hits, 6);
        assert_eq!((stats + stats).glyphs_shown, u64::MAX);
    }

    #[test]
    fn stats_sum_field_by_field() {
        let one = Work::new();
        let two = Work::new();
        for (index, counter) in ALL.into_iter().enumerate() {
            one.add(counter, index as u64);
            two.add(counter, 100);
        }
        let mut sum = one.stats();
        sum += two.stats();
        for (index, counter) in ALL.into_iter().enumerate() {
            let mut copy = sum;
            assert_eq!(*copy.slot(counter), index as u64 + 100, "{counter:?}");
        }
        assert_eq!(sum.work(), one.stats().work() + two.stats().work());
    }

    #[test]
    fn a_tally_is_shared_between_threads() {
        fn shared<T: Send + Sync>() {}
        shared::<Work>();
        let work = Work::new();
        std::thread::scope(|scope| {
            for _ in 0..4 {
                scope.spawn(|| {
                    for _ in 0..1_000 {
                        work.add(Counter::GlyphOutlineHits, 1);
                    }
                });
            }
        });
        assert_eq!(work.stats().glyph_outline_hits, 4_000);
    }
}
