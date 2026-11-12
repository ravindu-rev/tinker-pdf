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
//! It lives here, in the lowest crate with an operation context, although much
//! of what is counted is counted above it — glyphs by the renderer, font
//! dictionaries and cache lookups by the facade's resource scopes. The page
//! walk and the filter chain are counted here. One ledger an operation carries
//! through every context is the reason, and each counter says which layer
//! counts it. The editor is the context this crate gives it: a
//! [`crate::DocumentEditor`] asked to count holds one tally, which the passes
//! over it count into.

use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::ops::{Add, AddAssign};
use std::sync::Mutex;

use crate::object::ObjRef;

/// The lower of two references, either absent: how a tie between two streams
/// decoded equally often is settled, so that no order of the runs decides it.
fn lowest(one: Option<ObjRef>, other: Option<ObjRef>) -> Option<ObjRef> {
    match (one, other) {
        (Some(one), Some(other)) => Some(one.min(other)),
        (one, other) => one.or(other),
    }
}

/// What one operation counted: the value a door hands back when asked.
///
/// Every field is public, and each but [`Stats::most_decoded_stream`] a `u64`:
/// a count of steps of one kind, but for one maximum, the stream it was reached
/// on, and one quantity, which say so. **It is `#[non_exhaustive]`** so
/// that a counter a later row adds — masks rasterised, threads run — is an
/// addition rather than a break: read a field by name, total the steps with
/// [`Stats::work`], and sum two with `+`.
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
    /// Font dictionaries read: one for each name a resource scope's content
    /// selects a font by — its `/Font` entry, or the object a graphics
    /// state's `/Font` names — read the first time the scope is asked about
    /// the name, or about the id another scope over the same dictionary
    /// handed out, and kept for every render over the scope. A font a scope
    /// names and never selects is not read, and no glyph reads one.
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
    /// Counted by this crate's walk itself, for whichever operation makes it:
    /// a page lookup the hints cannot answer, each validator rule that reads
    /// every page, the structure tree's binding, a save's image and font
    /// passes, and an editor's own page operations — its page order, read
    /// until it is first changed, and an import's walk of its source.
    pub page_walks: u64,
    /// Nodes of the page tree read: each `/Pages` node and each leaf a walk
    /// reads, and each object a lookup of one page reads through a linearized
    /// file's first-page entry or its hint tables — the page, and each
    /// ancestor read with it.
    ///
    /// Counted by this crate's page walk and lookup.
    pub page_nodes_visited: u64,
    /// Pages handed out of a document's page index (`crate::pages`): one for
    /// each lookup the index answers, and each page a whole-tree read the
    /// index answers returns. Counted beside `page_nodes_visited`, since a page
    /// read out of the index stands in for the nodes a walk would read to it.
    pub page_entries_copied: u64,
    /// Lookups of a pre-interned name by its bytes: a key the document's name
    /// table holds at a fixed id ([`crate::NAMES`]), looked up where its
    /// [`crate::Name`] constant answers without the table — or a resource
    /// name that happens to spell one, such as a font named `/Font`. None
    /// while every reader on a counted path takes the constants, which is
    /// what makes a key spelled out at every glyph or operator show.
    ///
    /// Counted by the facade's resource scopes, for every name they look up
    /// by its bytes, and by every reader handed the counting view
    /// ([`crate::resolve::Counting`]) — a font read while a scope resolves
    /// it among them.
    pub keys_looked_up: u64,
    /// Runs of a stream's filter chain: each time an operation read a
    /// stream's bytes decoded, whatever the chain returned — a refusal, and
    /// output stopped at the decode ceiling, included. A stream read twice is
    /// two runs; a reference that names no stream is none.
    ///
    /// Counted by this crate where the chain is applied: the two decoding
    /// entry points' counted siblings,
    /// [`crate::CosDocument::stream_decoded_counted`] and
    /// [`crate::CosDocument::stream_image_input_counted`], every
    /// [`crate::resolve::Counting`] view, and a counting editor's own decodes.
    /// What a document decodes for itself — its object streams,
    /// cross-reference streams and hint tables, read once and kept — belongs
    /// to no operation and is not counted.
    pub streams_decoded: u64,
    /// The most runs of its filter chain any one stream had, the stream named
    /// by its reference. A maximum rather than a count of steps: [`Stats::work`]
    /// leaves it out, and `+` keeps the larger of two. One means no stream was
    /// decoded twice.
    ///
    /// Counted with `streams_decoded`.
    pub most_decodes_of_one_stream: u64,
    /// The stream `most_decodes_of_one_stream` was reached on, or `None` where
    /// nothing was decoded: of the streams decoded that many times, the one
    /// with the lowest reference, so that it depends on no order the runs
    /// came in. It names the object a count that grows with a page keeps
    /// decoding, which the count alone cannot.
    ///
    /// Counted with `streams_decoded`.
    pub most_decoded_stream: Option<ObjRef>,
    /// Bytes the filter chains returned, every run's summed. A quantity rather
    /// than a count of steps, so [`Stats::work`] leaves it out.
    ///
    /// Counted with `streams_decoded`.
    pub bytes_decoded: u64,
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
    /// [`Stats::page_nodes_visited`].
    PageNodesVisited,
    /// [`Stats::page_entries_copied`].
    PageEntriesCopied,
    /// [`Stats::keys_looked_up`].
    KeysLookedUp,
}

impl Stats {
    /// The total of every counter that counts a step taken — the figure a
    /// guard compares at two sizes of one input.
    ///
    /// A maximum and a quantity are left out — `most_decodes_of_one_stream`
    /// and `bytes_decoded` — because a byte total grows with the page and
    /// would hide a step count that grows with the document.
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
            self.page_nodes_visited,
            self.page_entries_copied,
            self.keys_looked_up,
            self.streams_decoded,
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
            Counter::PageNodesVisited => &mut self.page_nodes_visited,
            Counter::PageEntriesCopied => &mut self.page_entries_copied,
            Counter::KeysLookedUp => &mut self.keys_looked_up,
        }
    }
}

/// Counts sum, saturating: a caller's loop of doors adds up to the loop's
/// totals. A maximum stays the largest either side saw.
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
            page_nodes_visited: self
                .page_nodes_visited
                .saturating_add(other.page_nodes_visited),
            page_entries_copied: self
                .page_entries_copied
                .saturating_add(other.page_entries_copied),
            keys_looked_up: self.keys_looked_up.saturating_add(other.keys_looked_up),
            streams_decoded: self.streams_decoded.saturating_add(other.streams_decoded),
            most_decodes_of_one_stream: self
                .most_decodes_of_one_stream
                .max(other.most_decodes_of_one_stream),
            most_decoded_stream: match self
                .most_decodes_of_one_stream
                .cmp(&other.most_decodes_of_one_stream)
            {
                Ordering::Greater => self.most_decoded_stream,
                Ordering::Less => other.most_decoded_stream,
                Ordering::Equal => lowest(self.most_decoded_stream, other.most_decoded_stream),
            },
            bytes_decoded: self.bytes_decoded.saturating_add(other.bytes_decoded),
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
/// resources. The lock is held for one addition or one insertion; one that
/// will not open drops the count rather than panicking, as the caches beside
/// it drop an entry.
///
/// One `Work` an operation, never one a document: a document is read by many
/// threads at once, and a count kept on it would mix their operations.
#[derive(Debug, Default)]
pub struct Work {
    counts: Mutex<Tally>,
}

/// What a [`Work`] keeps behind its lock.
#[derive(Debug, Default)]
struct Tally {
    stats: Stats,
    /// Each stream's runs of its filter chain so far, by its reference: what
    /// [`Stats::most_decodes_of_one_stream`] is the largest of. A `BTreeMap`,
    /// so that no count depends on hash order.
    runs: BTreeMap<ObjRef, u64>,
}

impl Work {
    /// A tally that has counted nothing.
    #[must_use]
    pub fn new() -> Work {
        Work::default()
    }

    /// Adds `n` steps of one kind, saturating.
    pub fn add(&self, counter: Counter, n: u64) {
        if let Ok(mut tally) = self.counts.lock() {
            let slot = tally.stats.slot(counter);
            *slot = slot.saturating_add(n);
        }
    }

    /// Counts one run of `stream`'s filter chain that returned `bytes`
    /// bytes: the run, the bytes, and the stream's own runs, the largest of
    /// which is [`Stats::most_decodes_of_one_stream`].
    pub fn decoded(&self, stream: ObjRef, bytes: usize) {
        if let Ok(mut tally) = self.counts.lock() {
            let runs = tally.runs.entry(stream).or_insert(0);
            *runs = runs.saturating_add(1);
            let runs = *runs;
            let stats = &mut tally.stats;
            stats.streams_decoded = stats.streams_decoded.saturating_add(1);
            match runs.cmp(&stats.most_decodes_of_one_stream) {
                Ordering::Greater => {
                    stats.most_decodes_of_one_stream = runs;
                    stats.most_decoded_stream = Some(stream);
                }
                Ordering::Equal => {
                    stats.most_decoded_stream = lowest(stats.most_decoded_stream, Some(stream));
                }
                Ordering::Less => {}
            }
            let bytes = u64::try_from(bytes).unwrap_or(u64::MAX);
            stats.bytes_decoded = stats.bytes_decoded.saturating_add(bytes);
        }
    }

    /// What has been counted so far.
    #[must_use]
    pub fn stats(&self) -> Stats {
        self.counts
            .lock()
            .map(|tally| tally.stats)
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every counter, so a test of one property covers them all.
    const ALL: [Counter; 18] = [
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
        Counter::PageNodesVisited,
        Counter::PageEntriesCopied,
        Counter::KeysLookedUp,
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
    fn a_decode_counts_its_run_its_bytes_and_its_stream() {
        let work = Work::new();
        let (one, two) = (ObjRef::new(4, 0), ObjRef::new(7, 0));
        work.decoded(one, 10);
        work.decoded(two, 3);
        work.decoded(one, 10);
        // Another generation of one number is another stream.
        work.decoded(ObjRef::new(4, 1), 0);
        let stats = work.stats();
        assert_eq!(stats.streams_decoded, 4);
        assert_eq!(stats.most_decodes_of_one_stream, 2, "object 4 twice");
        assert_eq!(stats.most_decoded_stream, Some(one), "and named");
        assert_eq!(stats.bytes_decoded, 23);
        assert_eq!(stats.work(), 4, "the runs alone are steps");
        assert_eq!(Work::new().stats().most_decoded_stream, None);
    }

    /// Two streams decoded equally often are settled by the lower reference,
    /// whichever reached the count first, and so is a sum of two tallies.
    #[test]
    fn a_tie_names_the_lower_reference_in_any_order() {
        let (low, high) = (ObjRef::new(3, 0), ObjRef::new(9, 0));
        let first = Work::new();
        for stream in [high, low] {
            first.decoded(stream, 1);
            first.decoded(stream, 1);
        }
        let second = Work::new();
        for stream in [low, high] {
            second.decoded(stream, 1);
            second.decoded(stream, 1);
        }
        assert_eq!(first.stats().most_decoded_stream, Some(low));
        assert_eq!(second.stats().most_decoded_stream, Some(low));
        let only_high = Work::new();
        only_high.decoded(high, 1);
        let only_low = Work::new();
        only_low.decoded(low, 1);
        assert_eq!(
            (only_high.stats() + only_low.stats()).most_decoded_stream,
            Some(low)
        );
        assert_eq!(
            (only_low.stats() + only_high.stats()).most_decoded_stream,
            Some(low)
        );
        // A later run that passes the tie names its own stream.
        first.decoded(high, 1);
        assert_eq!(first.stats().most_decoded_stream, Some(high));
    }

    #[test]
    fn a_sum_keeps_the_larger_maximum() {
        let once = Work::new();
        once.decoded(ObjRef::new(1, 0), 5);
        let thrice = Work::new();
        for _ in 0..3 {
            thrice.decoded(ObjRef::new(1, 0), 2);
        }
        let sum = once.stats() + thrice.stats();
        assert_eq!(sum.streams_decoded, 4);
        assert_eq!(sum.bytes_decoded, 11);
        assert_eq!(sum.most_decodes_of_one_stream, 3, "a maximum, not a sum");
        assert_eq!(sum.most_decoded_stream, Some(ObjRef::new(1, 0)));
        assert_eq!(thrice.stats() + once.stats(), sum, "in either order");
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
