//! What reads a page's position reads the document's page index (P-01.4),
//! rather than a list of every page of its own.
//!
//! A destination names its page by reference, and a link is resolved by
//! finding that reference's position. Copying the page list for it is no step
//! any tally counts, so this binary installs `guard_support::Counting` as its
//! global allocator, which counts each allocation against the thread that made
//! it, and holds what a page's links allocate to a flat amount a page.

mod guard_support;
mod render_support;

use guard_support::{allocations_of, object, pdf, Counting, A2_PAGES};
use tinker_pdf::{Action, Destination, Document};

#[global_allocator]
static COUNTING: Counting = Counting;

/// `pages` pages under one `/Pages` node, each with one link to the page after
/// it and the last to the first: the catalog, the tree, then each page and its
/// link from 3.
fn linked(pages: usize) -> Vec<u8> {
    let page = |at: usize| 3 + 2 * at;
    let kids: Vec<String> = (0..pages).map(|at| format!("{} 0 R", page(at))).collect();
    let mut objects = vec![
        object("<< /Type /Catalog /Pages 2 0 R >>"),
        object(&format!(
            "<< /Type /Pages /Count {pages} /Kids [{}] >>",
            kids.join(" ")
        )),
    ];
    for at in 0..pages {
        objects.push(object(&format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 100] /Annots [{} 0 R] >>",
            page(at) + 1
        )));
        objects.push(object(&format!(
            "<< /Type /Annot /Subtype /Link /Rect [10 10 50 30] /Dest [{} 0 R /Fit] >>",
            page((at + 1) % pages)
        )));
    }
    pdf(&objects)
}

/// The position of the page a page's one link goes to.
fn target(links: &[tinker_pdf::Link]) -> Option<u32> {
    match links {
        [link] => match &link.target {
            Some(Action::GoTo(Destination::Explicit { page_index, .. })) => *page_index,
            _ => None,
        },
        _ => None,
    }
}

/// Every page's links, read as a page loop reads them — the pages counted,
/// then each looked up by its index — resolve to the page each names, and
/// allocate as much a page at 4 000 pages as at 1 000, within 1.1 times: no
/// page's links copy the page list.
#[test]
fn links_on_every_page_copy_no_page_list() {
    let mut per_page = [0.0; 2];
    for (slot, &pages) in per_page.iter_mut().zip(A2_PAGES.iter()) {
        let document = Document::open(linked(pages)).expect("the document opens");
        let count = document.page_count();
        assert_eq!(count as usize, pages, "the pages are counted");
        let mut targets = Vec::with_capacity(pages);
        let allocated = allocations_of(|| {
            for index in 0..count {
                let page = document.page(index).expect("a counted page is there");
                targets.push(target(&page.links()));
            }
        });
        for (index, found) in (0..count).zip(&targets) {
            assert_eq!(*found, Some((index + 1) % count), "page {index}'s link");
        }
        *slot = allocated as f64 / pages as f64;
    }
    let [small, large] = per_page;
    assert!(
        large <= 1.1 * small,
        "a page's links allocate {large:.1} at 4 000 pages against {small:.1} at 1 000"
    );
}
