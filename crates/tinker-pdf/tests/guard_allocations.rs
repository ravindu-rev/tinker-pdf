//! Allocations per page, by operation: the measurement A10 is settled by
//! (`docs/design/performance.md`, "The guard suite").
//!
//! A10 holds an operation over every page of a document to a flat number of
//! allocations a page. No tally sees an allocation, so this binary installs
//! `guard_support::Counting` as its global allocator, which counts each
//! allocation against the thread that made it: a test reads its own thread's
//! count, and every operation measured here runs on its caller's thread.
//! `#![forbid(unsafe_code)]` binds the library this tests and is untouched;
//! the allocator forwards every call to `System` and reads no document byte.
//!
//! Each operation runs as the CLI runs it over a whole document — every page
//! looked up by its index, then read — over a document of its own, written and
//! opened outside the count, at both of A2's sizes. A count at 4 000 pages
//! within 1.1 times the count at 1 000, a page each, is flat.

mod guard_support;
mod render_support;

use guard_support::{allocations, allocations_of, Counting, A2_PAGES};
use tinker_pdf::{Document, RenderOptions};

#[global_allocator]
static COUNTING: Counting = Counting;

/// A page looked up by its index.
fn lookup(document: &Document, index: u32) {
    drop(document.page(index));
}

/// A page looked up by its index and its text read.
fn text(document: &Document, index: u32) {
    if let Some(page) = document.page(index) {
        drop(page.text());
    }
}

/// A page looked up by its index and rendered at 36 dpi.
fn render(document: &Document, index: u32) {
    if let Some(page) = document.page(index) {
        drop(page.render(&RenderOptions {
            scale: 0.5,
            ..RenderOptions::default()
        }));
    }
}

/// One operation on one page of a document, by its index.
type Operation = fn(&Document, u32);

/// The operations A10 names, as the table prints them.
const OPERATIONS: [(&str, Operation); 3] = [
    ("page lookup", lookup),
    ("text", text),
    ("render-all at 36 dpi", render),
];

/// Prints each operation's allocations per page at 1 000 and 4 000 pages,
/// their ratio, and what text and the render allocate a page beyond their
/// lookup. `docs/design/performance.md` commits the table, and A10's roster
/// entries follow it.
#[test]
fn allocations_per_page_by_operation() {
    // The count is this thread's: one allocation made here counts once, and
    // a thousand made on another thread count nothing here.
    let one = allocations_of(|| drop(std::hint::black_box(Box::new(7_u8))));
    assert_eq!(one, 1, "an allocation counts once");
    let before = allocations();
    let spawned = std::thread::Builder::new().spawn(|| {
        for _ in 0..1_000 {
            drop(std::hint::black_box(Box::new(7_u8)));
        }
    });
    // A target with no threads, `wasm32-wasip1`, has no other thread to count.
    if let Ok(thread) = spawned {
        thread.join().expect("the thread ends");
        assert!(
            allocations() - before < 1_000,
            "another thread's allocations counted here"
        );
    }

    let mut rows = Vec::new();
    for (name, operation) in OPERATIONS {
        let per_page: Vec<f64> = A2_PAGES
            .iter()
            .map(|&pages| {
                let document =
                    Document::open(guard_support::document(0, pages)).expect("the document opens");
                let count = u32::try_from(pages).expect("a page count");
                let allocated = allocations_of(|| {
                    for index in 0..count {
                        operation(&document, index);
                    }
                });
                assert!(allocated > 0, "{name} at {pages} pages allocated nothing");
                allocated as f64 / pages as f64
            })
            .collect();
        rows.push((name, per_page));
    }

    println!(
        "{:<36} {:>12} {:>12} {:>7}",
        "allocations a page", "1 000 pages", "4 000 pages", "ratio"
    );
    for (name, per_page) in &rows {
        println!(
            "{name:<36} {:>12.1} {:>12.1} {:>7.2}",
            per_page[0],
            per_page[1],
            per_page[1] / per_page[0]
        );
    }
    let lookup = &rows[0].1;
    for (name, per_page) in &rows[1..] {
        println!(
            "{:<36} {:>12.1} {:>12.1} {:>7.2}",
            format!("{name}, beyond its lookup"),
            per_page[0] - lookup[0],
            per_page[1] - lookup[1],
            (per_page[1] - lookup[1]) / (per_page[0] - lookup[0])
        );
    }
}
