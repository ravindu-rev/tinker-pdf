//! What a zero-width dashed line under a stretch costs to write as SVG, in
//! bytes.
//!
//! Under a map that is not a similarity no one dash array states a
//! zero-width line's dashes, so `Page::to_svg` cuts them in user space and
//! writes the pieces. The first way it did that cut **every** piece into
//! memory, and wrote every piece into the path data, before the markup budget
//! was asked whether the element fitted: the dash bound is 100 000 steps a
//! segment, so the pieces grow with the segment count, and a review measured
//! ten segments (121 bytes of content) at 10 770 294 bytes of markup and forty
//! (346 bytes) at 43 080 339 — about a megabyte per nine-byte segment, so ten
//! thousand segments asked for gigabytes before `MAX_SVG_BYTES` was consulted.
//! The pieces are now written as they are cut, and the cutting stops where the
//! budget would.
//!
//! # Why this file counts allocations, and why it is a file of its own
//!
//! The markup a budgeted write hands back is the same either way — the element
//! did not fit, nothing after it is written, and `SvgWarning::Truncated` says
//! so — which is why the cost went unnoticed. So the property is asserted
//! where it lives, in the allocator's peak, and that needs a
//! `#[global_allocator]` that counts, which is a test binary's concern only,
//! as `tinker-pdf-layout`'s `column_span_memory.rs` does it.
//! `#![forbid(unsafe_code)]` binds the library this tests and is untouched; the
//! one `unsafe impl` below forwards every call to `std`'s own `System`
//! allocator unchanged and reads no document byte. One test, so nothing else
//! in the process allocates while it measures.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};

use tinker_pdf::{Document, SvgOptions, SvgWarning};

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

/// A one-page document of 100 x 100 points around `content`.
fn pdf(content: &str) -> Vec<u8> {
    format!(
        "%PDF-1.7\n\
1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
2 0 obj\n<< /Type /Pages /Count 1 /Kids [3 0 R] >>\nendobj\n\
3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100]\n\
   /Resources << >> /Contents 4 0 R >>\nendobj\n\
4 0 obj\n<< /Length {} >>\nstream\n{content}\nendstream\nendobj\n\
trailer\n<< /Size 5 /Root 1 0 R >>\n%%EOF\n",
        content.len()
    )
    .into_bytes()
}

/// **A zero-width dashed line under a stretch costs what the budget allows,
/// not what its pieces would.** The review's page: under `scale(1, 3)` a
/// hairline dashed `[0.01 0.01]` runs forty times out to user `x` 1000 and
/// back, which cut whole is two million pieces and 43 MB of path data. Written
/// under a 1 MiB budget, the write stops where the budget does — the markup
/// is under it and `Truncated` names it — and the most bytes live at once
/// while it runs are a small multiple of the budget rather than of the pieces.
#[test]
fn a_dashed_hairline_under_a_stretch_stops_at_the_markup_budget() {
    const BUDGET: usize = 1 << 20;
    let mut content = String::from("q 1 0 0 3 0 0 cm 0 w [0.01 0.01] 0 d 0 0 m");
    for _ in 0..40 {
        content.push_str(" 1000 0 l 0 0 l");
    }
    content.push_str(" S Q");
    let document = Document::open(pdf(&content)).expect("it opens");
    let page = document.page(0).expect("a page");
    let mut options = SvgOptions::default();
    options.max_bytes = BUDGET;

    let before = LIVE.load(Ordering::SeqCst);
    PEAK.store(before, Ordering::SeqCst);
    let svg = page.to_svg(&options);
    let peak = PEAK.load(Ordering::SeqCst).saturating_sub(before);

    assert!(
        svg.warnings
            .contains(&SvgWarning::Truncated { limit: BUDGET }),
        "the pieces pass the budget and the writer says so: {:?}",
        svg.warnings
    );
    assert!(svg.markup.len() < BUDGET, "{} bytes", svg.markup.len());
    assert!(
        peak < 8 * BUDGET,
        "writing the page held {peak} bytes at once under a {BUDGET}-byte budget: \
         the pieces were cut whole before the budget was asked"
    );
}
