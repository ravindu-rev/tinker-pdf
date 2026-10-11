//! What `column-span: all` costs to lay out, in bytes.
//!
//! `css-multicol-1` §6 cuts a multi-column container's children at each
//! spanner into column sets of their own. The first way this build said that
//! laid each run out in a **copy** of the container holding a **copy** of the
//! run — a deep copy of the subtree, alive while the subtree was laid out. One
//! container is one copy, which is nothing; a multi-column container inside
//! another, each with a spanner, is a copy per level alive at once, and a
//! review measured nested containers round twenty thousand inline boxes
//! (180 KB of XHTML) at 332 MB for one level, 999 MB for ten and 1.7 GB for
//! twenty, against a flat 260 MB with the spanners' class removed — so the
//! 256 levels `MAX_BOX_DEPTH` allows were tens of gigabytes from a few
//! hundred kilobytes of markup. A run is now borrowed in place.
//!
//! # Why this file counts allocations, and why it is a file of its own
//!
//! A copy has no other observable: the layout it produces is the same layout,
//! which is the whole reason it went unnoticed. So the property is asserted
//! where it lives, in the allocator's peak — **the most bytes live at once**
//! while one tree is laid out — and that needs a `#[global_allocator]` that
//! counts, which is a test binary's concern only. `#![forbid(unsafe_code)]`
//! binds the library this tests and is untouched; the one `unsafe impl` below
//! forwards every call to `std`'s own `System` allocator unchanged and reads
//! no document byte. One test, so nothing else in the process allocates while
//! it measures.
//!
//! The comparison is against the same tree with nothing spanning, which is
//! laid out as one column set per container and never copied, so the
//! assertion is a ratio of two runs of this crate and not a number that moves
//! with the size of a computed style.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};

use tinker_pdf_css::cascade::ComputedStyle;
use tinker_pdf_css::property::{ColumnCount, ColumnSpan, Display};
use tinker_pdf_layout::metrics::FixedPitch;
use tinker_pdf_layout::{layout, BoxNode, Limits, Options};

/// `System`, keeping a count of the bytes live and the most there have been.
struct Counting;

static LIVE: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);

fn grew(by: usize) {
    let now = LIVE.fetch_add(by, Ordering::SeqCst) + by;
    PEAK.fetch_max(now, Ordering::SeqCst);
}

fn shrank(by: usize) {
    LIVE.fetch_sub(by, Ordering::SeqCst);
}

// SAFETY: every method forwards its arguments unchanged to `System`, whose
// contract is the trait's; the counters are plain atomics and allocate nothing.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        // SAFETY: the caller's contract for `alloc` is `System::alloc`'s.
        let at = unsafe { System.alloc(layout) };
        if !at.is_null() {
            grew(layout.size());
        }
        at
    }

    unsafe fn dealloc(&self, at: *mut u8, layout: Layout) {
        // SAFETY: `at` came from this allocator, which is `System`.
        unsafe { System.dealloc(at, layout) };
        shrank(layout.size());
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        // SAFETY: as `alloc`.
        let at = unsafe { System.alloc_zeroed(layout) };
        if !at.is_null() {
            grew(layout.size());
        }
        at
    }

    unsafe fn realloc(&self, at: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        // SAFETY: `at` and `layout` came from this allocator, which is
        // `System`, and `size` is the caller's under the same contract.
        let moved = unsafe { System.realloc(at, layout, size) };
        if !moved.is_null() {
            shrank(layout.size());
            grew(size);
        }
        moved
    }
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

/// The most bytes live at once while `tree` is laid out, above what was live
/// before it began.
fn peak_of(tree: &BoxNode) -> usize {
    let before = LIVE.load(Ordering::SeqCst);
    PEAK.store(before, Ordering::SeqCst);
    let laid = layout(
        tree,
        &FixedPitch::COURIER,
        &Options::new(600.0, 100_000.0),
        &Limits::DEFAULT,
    )
    .expect("the fixture is under every cap");
    let peak = PEAK.load(Ordering::SeqCst);
    drop(laid);
    peak.saturating_sub(before)
}

fn block() -> ComputedStyle {
    let mut style = ComputedStyle::initial();
    style.display = Display::Block;
    style
}

/// `levels` two-column containers, one inside the next, each starting with a
/// paragraph that spans when `spanning` says so, round a paragraph of `leaves`
/// inline boxes — the review's `<div class=mc><p class=s>s</p>` nest.
fn nest(levels: usize, leaves: usize, spanning: bool) -> BoxNode {
    let mut inline = ComputedStyle::initial();
    inline.display = Display::Inline;
    let words: Vec<BoxNode> = (0..leaves)
        .map(|_| BoxNode::element(inline.clone(), vec![BoxNode::text(inline.clone(), "x ")]))
        .collect();
    let mut inside = BoxNode::element(block(), words);
    let mut columns = block();
    columns.column_count = ColumnCount::Count(2);
    let mut spanner = block();
    if spanning {
        spanner.column_span = ColumnSpan::All;
    }
    for _ in 0..levels {
        let head = BoxNode::element(spanner.clone(), vec![BoxNode::text(block(), "s")]);
        inside = BoxNode::element(columns.clone(), vec![head, inside]);
    }
    BoxNode::element(block(), vec![inside])
}

/// **Nested spanning containers cost what the same nest without spanners
/// does.** Eight levels round two thousand inline boxes: the copy made each
/// level hold the whole subtree again, which is several times the control's
/// peak; borrowed, the two are the same to within the spanner's own boxes.
#[test]
fn nested_spanners_lay_out_without_copying_their_subtrees() {
    const LEVELS: usize = 8;
    const LEAVES: usize = 2000;
    let control = nest(LEVELS, LEAVES, false);
    let spanning = nest(LEVELS, LEAVES, true);
    let control_peak = peak_of(&control);
    let spanning_peak = peak_of(&spanning);
    assert!(control_peak > 0, "the control allocated nothing");
    assert!(
        spanning_peak * 4 < control_peak * 5,
        "spanners cost {spanning_peak} bytes at peak against {control_peak} without: \
         a copy of the subtree per level"
    );
}
