//! What a streamed open actually costs, in bytes (`docs/design/streaming-open.md`).
//!
//! The claim streaming makes is a claim about *bytes read*, so it is measured
//! rather than asserted. `CountingSource` sits between the engine and the
//! document and charges for what arrived; the budgets below are the numbers
//! that came out, and they are committed so that an open path which quietly
//! started reading everything fails here instead of passing.
//!
//! Every fixture is generated rather than committed. A multi-megabyte binary
//! in the repository is a thing no reviewer can read and a diff nobody can
//! assess, and the generator is deterministic -- the same builder calls
//! produce the same bytes on every target, which is what `determinism.rs`
//! pins for the writer already.

use std::sync::Arc;

use tinker_pdf::{CountingSource, Document, DocumentBuilder, Name, Object, SliceSource};

/// How many bytes opening the 120-page fixture may read.
///
/// A ratchet in the sense `corpus/ratchet.json` is: measured, committed, and
/// moved only by a reviewed change. 13,753 bytes of a 4,891,065-byte document
/// is what tail-first discovery costs -- the head window, the two `startxref`
/// probes, the section that answers them, and the catalog and page tree the
/// open path resolves.
const OPEN_BUDGET: u64 = 13_753;

/// And how many it may read to open *and* pull one object out of the middle.
const OPEN_AND_ONE_OBJECT_BUDGET: u64 = 71_097;

/// A document of `pages` pages, each carrying a content stream of a few tens
/// of kilobytes, so the whole thing runs to megabytes and one page is a small
/// fraction of it.
fn a_large_document(pages: usize) -> Vec<u8> {
    let mut builder = DocumentBuilder::new();
    for index in 0..pages {
        builder.add_page(200.0, 100.0, |page| {
            // Distinct per page, so no two pages could be confused for each
            // other and a reader that fetched the wrong one would draw the
            // wrong thing rather than the same thing.
            let mut ops = Vec::with_capacity(24 * 1024);
            let shade = (index % 7) as f64 / 7.0;
            for step in 0..600 {
                let x = (step % 40) as f64 * 4.0;
                let y = (step / 40) as f64 * 6.0;
                ops.extend_from_slice(
                    format!(
                        "{shade:.3} 0.400 0.600 rg {x:.2} {y:.2} m {:.2} {:.2} l {:.2} {:.2} l h f\n",
                        x + 3.5,
                        y + 5.5,
                        x + 1.25,
                        y + 2.75
                    )
                    .as_bytes(),
                );
            }
            page.raw(&ops);
        });
    }
    builder.finish()
}

/// The content stream of a page in the middle of the document, reached
/// through the page tree rather than guessed at.
///
/// The builder writes one `/Pages` node with every page as a kid, which this
/// asserts rather than assumes: a nested tree would make `kids[at]` the wrong
/// object and the budget would be measuring the wrong read.
fn mid_page_content(document: &Document, at: usize) -> Object {
    let cos = document.cos();
    let root = cos.resolve_key(cos.trailer(), Name::ROOT);
    let catalog = root.as_dict().expect("a catalog").clone();
    let pages = cos.resolve_key(&catalog, Name::PAGES);
    let node = pages.as_dict().expect("a page tree root").clone();
    let kids = cos.resolve_key(&node, Name::KIDS);
    let kids = kids.as_array().expect("a page tree has kids");
    assert_eq!(
        node.get_int(Name::COUNT).map(|c| c as usize),
        Some(kids.len()),
        "the fixture tree is flat, so a kid is a page"
    );
    let page = cos.resolve(kids.get(at).expect("a kid at this index"));
    let page = page.as_dict().expect("a page object").clone();
    page.get(Name::CONTENTS)
        .cloned()
        .expect("a page carries contents")
}

/// Opening a multi-megabyte document and reading one object out of the middle
/// costs a bounded number of bytes, and the bound is committed.
///
/// The number is a ratchet in the sense `corpus/ratchet.json` is: it moves
/// only by a reviewed change, because an open path that started reading more
/// would otherwise be invisible.
#[test]
fn opening_a_large_document_and_reading_one_object_stays_under_budget() {
    let bytes = a_large_document(120);
    assert!(
        bytes.len() > 2_000_000,
        "the fixture is {} bytes, which is not multi-megabyte",
        bytes.len()
    );

    let source = Arc::new(CountingSource::new(SliceSource::new(bytes.clone())));
    let document = Document::open_streaming(source.clone()).expect("it opens");
    let after_open = source.bytes_read();

    let middle = document.page_count() as usize / 2;
    let contents = mid_page_content(&document, middle);
    let reference = contents.as_objref().expect("an indirect content stream");
    let stream = document
        .cos()
        .stream_decoded(reference)
        .expect("the content stream decodes");
    assert!(stream.len() > 10_000, "the middle page really is large");
    let after_read = source.bytes_read();

    println!(
        "RAN streaming open budget: fixture {} bytes, open {after_open}, \
         open plus one mid-file object {after_read}",
        bytes.len()
    );

    assert!(
        after_open <= OPEN_BUDGET,
        "opening read {after_open} bytes, over the committed budget of {OPEN_BUDGET}"
    );
    assert!(
        after_read <= OPEN_AND_ONE_OBJECT_BUDGET,
        "opening and reading one mid-file object read {after_read} bytes, over the
         committed budget of {OPEN_AND_ONE_OBJECT_BUDGET}"
    );
    // And the budgets are a small fraction of the document, which is the claim
    // the numbers are evidence for. Stated separately so that a budget nudged
    // upward by a reviewed change still has to stay a streaming figure.
    assert!(
        after_read * 20 < bytes.len() as u64,
        "reading one object out of the middle read {after_read} of {} bytes, which is
         not streaming",
        bytes.len()
    );
    assert!(
        !document.whole_file_fetched(),
        "nothing should have pulled the whole document"
    );
}
