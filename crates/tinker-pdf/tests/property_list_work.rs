//! What a named marked-content property list costs to read, in bytes
//! allocated.
//!
//! `/Tag /P0 BDC` names an entry of the scope's `/Properties` (14.6.2), and the
//! interpreter asks the facade for it at every such `BDC` on every reading of
//! the page: text, search and render alike. A review of the PDF 2.0 lane's
//! `/MCAF` reading found each of those asks **deep-copying** the whole resolved
//! list, so the work grew with the number of `BDC`s times the size of the
//! list. 2 000 `/X /P0 BDC EMC` pairs naming one indirect list holding a
//! 1 048 576-entry array — a 2.1 MB file — took 150 s to extract text from,
//! against 0.38 s with the list borrowed. The direct form,
//! `/Properties << /P0 << ... >> >>`, made two copies before that lane and
//! three after it (643 s). The list, the `/Properties` table it sits in and
//! the `/MCAF` array beside it are now read where they lie: the document's
//! cached object for an indirect one, the resource dictionary itself for a
//! direct one. So is every value read out of a list or an optional content
//! group: a verifier found the same copy, at the same cost, with the array
//! under `/MCID` or `/Alt` rather than beside them, or under an `/OC`
//! group's `/Name` — and on develop before the lane too.
//!
//! # Why this file counts allocations, and why it is a file of its own
//!
//! A copy has no other observable — the text, the marked content and the
//! listed files are the same either way, which is why it went unnoticed — and
//! a clock is not a test. So the work is counted where it lives, in the bytes
//! the allocator hands out while the page is read, and that needs a
//! `#[global_allocator]` that counts, which is a test binary's concern only, as
//! `svg_dash_memory.rs` does it. `#![forbid(unsafe_code)]` binds the library
//! this tests and is untouched; the one `unsafe impl` below forwards every call
//! to `std`'s own `System` allocator unchanged and reads no document byte. One
//! test, so nothing else in the process allocates while it measures.
//!
//! Each page is measured against the same page naming the list **once**: the
//! difference is what the other sequences cost, which is a ratio of two runs
//! of this crate and not a number that moves with the size of an `Object`.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};

use tinker_pdf::Document;

/// `System`, keeping a count of every byte it has handed out.
struct Counting;

static HANDED_OUT: AtomicUsize = AtomicUsize::new(0);

// SAFETY: every method forwards its arguments unchanged to `System`, whose
// contract is the trait's; the counter is a plain atomic and allocates nothing.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        HANDED_OUT.fetch_add(layout.size(), Ordering::SeqCst);
        // SAFETY: the caller's contract for `alloc` is `System::alloc`'s.
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, at: *mut u8, layout: Layout) {
        // SAFETY: `at` came from this allocator, which is `System`.
        unsafe { System.dealloc(at, layout) };
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        HANDED_OUT.fetch_add(layout.size(), Ordering::SeqCst);
        // SAFETY: as `alloc`.
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn realloc(&self, at: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        HANDED_OUT.fetch_add(size, Ordering::SeqCst);
        // SAFETY: `at` and `layout` came from this allocator, which is
        // `System`, and `size` is the caller's under the same contract.
        unsafe { System.realloc(at, layout, size) }
    }
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

/// The bytes handed out while `read` runs.
fn handed_out_by(read: impl FnOnce()) -> usize {
    let before = HANDED_OUT.load(Ordering::SeqCst);
    read();
    HANDED_OUT.load(Ordering::SeqCst).saturating_sub(before)
}

/// How many entries the large array in each list holds: a copy of it is
/// megabytes, against the few hundred bytes a sequence otherwise costs.
const LARGE: usize = 1 << 16;

/// How many sequences the measured page draws.
const SEQUENCES: usize = 64;

/// What one sequence past the first may cost at most: far above what reading
/// a sequence does, far below one copy of a `LARGE`-entry array.
const PER_SEQUENCE: usize = 16 * 1024;

/// A 2.0 page whose `/Properties` is `properties` and whose content is
/// `/{tag} /P0 BDC EMC` `count` times, with `extra` objects from 5 on.
///
/// The catalog's `/OCProperties` is `7 0 R`, for `extra` to write where a
/// page needs a configuration; on every other page it names nothing, which
/// reads as none.
fn pdf(tag: &str, count: usize, properties: &str, extra: &str) -> Vec<u8> {
    let content = format!("/{tag} /P0 BDC EMC\n").repeat(count);
    format!(
        "%PDF-2.0\n\
1 0 obj\n<< /Type /Catalog /Pages 2 0 R /OCProperties 7 0 R >>\nendobj\n\
2 0 obj\n<< /Type /Pages /Count 1 /Kids [3 0 R] >>\nendobj\n\
3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100]\n\
   /Resources << /Properties {properties} >> /Contents 4 0 R >>\nendobj\n\
4 0 obj\n<< /Length {} >>\nstream\n{content}endstream\nendobj\n\
{extra}\
trailer\n<< /Size 9 /Root 1 0 R >>\n%%EOF\n",
        content.len()
    )
    .into_bytes()
}

/// `LARGE` zeros, as a PDF array.
fn large_array() -> String {
    format!("[{}]", "0 ".repeat(LARGE))
}

/// What `read` costs on the page of `SEQUENCES` sequences beyond what it costs
/// on the same page with one.
fn cost_of_more_sequences(
    tag: &str,
    properties: &str,
    extra: &str,
    read: impl Fn(&tinker_pdf::Page),
) -> usize {
    let mut costs = [1, SEQUENCES].map(|count| {
        let document = Document::open(pdf(tag, count, properties, extra)).expect("it opens");
        let page = document.page(0).expect("a page");
        handed_out_by(|| read(&page))
    });
    costs.sort_unstable();
    costs[1] - costs[0]
}

/// **A sequence naming a property list costs what reading the sequence costs,
/// not what copying the list would.** Five pages, each read once naming the
/// list and once naming it `SEQUENCES` times:
///
/// - an indirect list beside a `LARGE`-entry array, read as text — the
///   review's file, and the indirect form real files write;
/// - the same list written directly in a direct `/Properties` table;
/// - a list whose `/MCAF` is itself a direct `LARGE`-entry array, which the
///   reading only asks the presence of;
/// - `/AF` sequences through an indirect list holding one file and the large
///   array, listed by `Page::marked_content_associated_files`, which reads the
///   list again for each;
/// - `/OC` sequences naming a direct optional content group beside the large
///   array, which the reading asks for its layer as well as its properties.
///
/// Then the array as each **value** the reading takes out of a list, one
/// page each, since a list read where it lies still copies a value read by
/// copy: the list's `/MCID`, `/ActualText`, `/Alt`, `/Lang` and `/E`; a
/// group's `/Name` and `/Type`; a membership dictionary's `/OCGs`, `/VE`, an
/// operand of its `/VE`, and its `/P`.
///
/// Each extra sequence is held to `PER_SEQUENCE` bytes. A copy per sequence
/// is `SEQUENCES - 1` copies of the array, megabytes each.
#[test]
fn a_named_property_list_is_read_where_it_lies() {
    let large = large_array();
    let budget = (SEQUENCES - 1) * PER_SEQUENCE;
    let text = |page: &tinker_pdf::Page| {
        let _ = page.text();
    };

    let indirect = cost_of_more_sequences(
        "X",
        "<< /P0 5 0 R >>",
        &format!("5 0 obj\n<< /Lang (en) /Big {large} >>\nendobj\n"),
        text,
    );
    assert!(
        indirect <= budget,
        "{SEQUENCES} sequences naming an indirect list cost {indirect} bytes more than \
         one did, over {budget}: the list is copied for each"
    );

    let direct = cost_of_more_sequences(
        "X",
        &format!("<< /P0 << /Lang (en) /Big {large} >> >>"),
        "",
        text,
    );
    assert!(
        direct <= budget,
        "{SEQUENCES} sequences naming a direct list cost {direct} bytes more than one \
         did, over {budget}: the list or its table is copied for each"
    );

    let mcaf = cost_of_more_sequences("X", &format!("<< /P0 << /MCAF {large} >> >>"), "", text);
    assert!(
        mcaf <= budget,
        "{SEQUENCES} sequences naming a list with a long /MCAF cost {mcaf} bytes more \
         than one did, over {budget}: the array is copied to ask whether it is one"
    );

    let listed = cost_of_more_sequences(
        "AF",
        "<< /P0 5 0 R >>",
        &format!(
            "5 0 obj\n<< /MCAF [6 0 R] /Big {large} >>\nendobj\n\
6 0 obj\n<< /Type /Filespec /F (a.txt) /UF (a.txt) /AFRelationship /Data >>\nendobj\n"
        ),
        |page| {
            let list = page.marked_content_associated_files();
            assert_eq!(list.dropped, 0);
            assert!(list
                .sequences
                .iter()
                .all(|sequence| sequence.files.len() == 1 && !sequence.incomplete));
        },
    );
    assert!(
        listed <= budget,
        "{SEQUENCES} /AF sequences cost {listed} bytes more to list than one did, over \
         {budget}: the property list is copied for each"
    );

    let layer = cost_of_more_sequences(
        "OC",
        &format!("<< /P0 << /Type /OCG /Name (Layer) /Big {large} >> >>"),
        "",
        text,
    );
    assert!(
        layer <= budget,
        "{SEQUENCES} /OC sequences naming a direct group cost {layer} bytes more than one \
         did, over {budget}: the group or its table is copied for each"
    );

    // Each value a list's properties are read from: the reading asks each
    // one's type, and a copy to ask is the array's size whatever the answer.
    for key in ["MCID", "ActualText", "Alt", "Lang", "E"] {
        let value =
            cost_of_more_sequences("X", &format!("<< /P0 << /{key} {large} >> >>"), "", text);
        assert!(
            value <= budget,
            "{SEQUENCES} sequences naming a list whose /{key} is a long array cost {value} \
             bytes more than one did, over {budget}: the value is copied for each"
        );
    }

    // What a group's layer is read from, and a membership dictionary's
    // visibility. `/P` is read only once every group in `/OCGs` is one the
    // configuration knows, so that page writes one.
    let configured = "7 0 obj\n<< /OCGs [8 0 R] >>\nendobj\n\
8 0 obj\n<< /Type /OCG /Name (Layer) >>\nendobj\n";
    for (what, group, extra) in [
        ("/Name", format!("<< /Type /OCG /Name {large} >>"), ""),
        ("/Type", format!("<< /Type {large} /Name (Layer) >>"), ""),
        ("/OCGs", format!("<< /Type /OCMD /OCGs {large} >>"), ""),
        ("/VE", format!("<< /Type /OCMD /VE {large} >>"), ""),
        (
            "/VE operand",
            format!("<< /Type /OCMD /VE [/Or {large}] >>"),
            "",
        ),
        (
            "/P",
            format!("<< /Type /OCMD /OCGs [8 0 R] /P {large} >>"),
            configured,
        ),
    ] {
        let value = cost_of_more_sequences("OC", &format!("<< /P0 {group} >>"), extra, text);
        assert!(
            value <= budget,
            "{SEQUENCES} /OC sequences naming a group whose {what} is a long array cost \
             {value} bytes more than one did, over {budget}: the value is copied for each"
        );
    }
}
