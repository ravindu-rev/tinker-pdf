//! What `tpdf_document_open` holds of the bytes it is handed: one copy, the
//! buffer the document keeps, and the parse's own (P-05.6).
//!
//! The C ABI copies a caller's bytes so the caller may free theirs at once,
//! and one copy is the contract. A second — the slice into a `Vec`, the `Vec`
//! into the `Arc` the facade keeps — would hold the file twice for the length
//! of the copy, which for a large file is the open's peak.
//!
//! # Why this file counts allocations, and why it is a file of its own
//!
//! No tally sees an allocation, so the property is asserted where it lives, in
//! the allocator's peak, which needs a `#[global_allocator]` that counts, as
//! `crates/tinker-pdf/tests/svg_dash_memory.rs` does it. The one `unsafe
//! impl` below forwards every call to `std`'s own `System` allocator
//! unchanged and reads no document byte. One test, so nothing else in the
//! process allocates while it measures.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};

use tinker_pdf::DocumentBuilder;
use tinker_pdf_ffi::{tpdf_document_free, tpdf_document_open, TpdfDocument, TpdfStatus};

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

/// A page drawing at least a mebibyte of content: one document, few objects,
/// and almost every byte its content's.
fn a_mebibyte_document() -> Vec<u8> {
    let mut content = Vec::new();
    while content.len() < 1 << 20 {
        content.extend_from_slice(b"0 0 1 1 re f\n");
    }
    let mut builder = DocumentBuilder::new();
    builder.add_page(200.0, 100.0, |page| {
        page.raw(&content);
    });
    builder.finish()
}

/// **Opening bytes copies them once.** Over a mebibyte document,
/// `tpdf_document_open` keeps a copy of the input, and the most it holds at
/// once while it opens is that copy and the parse's own, short of a second
/// copy.
#[test]
fn opening_bytes_copies_them_once() {
    let bytes = a_mebibyte_document();
    let len = bytes.len();
    let before = LIVE.load(Ordering::SeqCst);
    PEAK.store(before, Ordering::SeqCst);
    let mut document: *mut TpdfDocument = std::ptr::null_mut();
    // SAFETY: `bytes` holds `len` readable bytes, and `document` is a valid
    // place to write the handle.
    let status = unsafe { tpdf_document_open(bytes.as_ptr(), len, &mut document) };
    let peak = PEAK.load(Ordering::SeqCst).saturating_sub(before);
    let held = LIVE.load(Ordering::SeqCst).saturating_sub(before);
    assert_eq!(status, TpdfStatus::Ok);
    assert!(
        held >= len,
        "the document holds its copy: {held} of {len} bytes"
    );
    assert!(
        peak < 2 * len,
        "opening {len} bytes held {peak} at once, a second copy's worth or more"
    );
    println!("RAN opening_bytes_copies_them_once: {len} bytes, {peak} at most at once");
    // SAFETY: the handle came from `tpdf_document_open` and is freed once.
    unsafe { tpdf_document_free(document) };
}
