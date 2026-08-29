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

use std::path::PathBuf;
use std::sync::Arc;

use tinker_pdf::{
    CountingSource, Document, DocumentBuilder, Name, Object, RenderOptions, SliceSource,
    WarningKind, WriteMode, WriteOptions, CHUNK_SIZE,
};

/// How many bytes opening the 120-page fixture may read.
///
/// A ratchet in the sense `corpus/ratchet.json` is: measured, committed, and
/// moved only by a reviewed change. 13,753 bytes of a 4,891,065-byte document
/// is what tail-first discovery costs -- the head window, the two `startxref`
/// probes, the section that answers them, and the catalog and page tree the
/// open path resolves.
const OPEN_BUDGET: u64 = 13_753;

/// And how many it may read to open *and* pull one object out of the middle.
const OPEN_AND_ONE_OBJECT_BUDGET: u64 = 67_001;

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

// ---- Annex F: the head-only open ---------------------------------------

/// The same pages, saved linearized (Annex F).
///
/// Object streams are off for the same reason `tests/linearized.rs` turns them
/// off: a container would put the first page's objects in the same blob as
/// everything else and defeat the layout entirely.
fn a_linearized_document(pages: usize) -> Vec<u8> {
    let mut builder = DocumentBuilder::new();
    for index in 0..pages {
        builder.add_page(200.0, 100.0, |page| {
            // Tens of kilobytes a page, so `/E` lands far past the head window
            // and "no read reaches the tail" is a claim about the open path
            // rather than about a document that fits inside one chunk.
            let mut ops = Vec::with_capacity(24 * 1024);
            let shade = (index % 7) as f64 / 7.0;
            for step in 0..400 {
                let x = (step % 40) as f64 * 4.0;
                let y = (step / 40) as f64 * 6.0;
                ops.extend_from_slice(
                    format!(
                        "{shade:.3} 0.400 0.600 rg {x:.2} {y:.2} m {:.2} {:.2} l {:.2} {:.2} l h f ",
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
    let base = Document::open(builder.finish()).expect("it opens");
    base.editor().save(&WriteOptions {
        mode: WriteMode::Rewrite,
        linearize: true,
        object_streams: false,
        ..WriteOptions::default()
    })
}

/// Pixels that are not the white the page started as.
fn ink(bitmap: &tinker_pdf::Bitmap) -> usize {
    bitmap
        .data
        .chunks_exact(bitmap.components())
        .filter(|pixel| pixel.iter().any(|value| *value != 255))
        .count()
}

/// How many files in the pinned qpdf corpus open through Annex F's head-only
/// path. Committed, so that a set which shrank cannot read as a pass.
///
/// Measured against the `qpdf` entry of `corpus/corpora.lock`, commit
/// e8adee32, whose pinned subdirectory holds 626 PDFs, 45 of them already
/// linearized. The two that do not open from their heads state parameters
/// this reader will not act on, and fall back rather than guess.
const HEAD_OPENED_CORPUS_FILES: usize = 43;

/// The linearized corpus files whose page-one render reaches past `/E`, by
/// name and with the reason.
///
/// `badlin1.pdf` is qpdf's deliberately damaged linearization fixture: its
/// declared `/E` is not where the first page ends, so the objects page one
/// really needs are past it. Reading them is the correct answer -- hints
/// accelerate, they never decide, and the head ceiling is a guess the object
/// is allowed to overrule. Named rather than counted, so that a file which
/// stopped trespassing, or a second which started, both fail here.
const REACHES_PAST_E: &[&str] = &["badlin1.pdf"];

/// How many bytes a page-one render of the 60-page linearized fixture may
/// read. A ratchet, measured and committed.
///
/// 29,696 of 1,631,095 -- 1.8% -- and the shape of the number is the point:
/// `/E` is 28,224, so page one costs the head up to `/E` rounded up to the
/// chunk that contains it, and nothing else. A budget much below that would
/// mean the render was not drawing the page; anything above it would mean
/// something reached into the tail.
const LINEARIZED_PAGE_ONE_BUDGET: u64 = 29_696;

/// Page one of a linearized file costs head reads and nothing else.
#[test]
fn a_linearized_file_renders_page_one_without_touching_its_tail() {
    let bytes = a_linearized_document(60);
    let source = Arc::new(CountingSource::new(SliceSource::new(bytes.clone())));
    let document = Document::open_streaming(source.clone()).expect("it opens");

    let end_of_first_page = document
        .first_page_end()
        .expect("the linearized fast path engaged");
    assert!(
        end_of_first_page < bytes.len() as u64,
        "/E is inside the file"
    );

    let bitmap = document
        .page(0)
        .expect("page one")
        .render(&RenderOptions::default());
    let drawn = ink(&bitmap);
    assert!(drawn > 1000, "page one painted {drawn} pixels");

    // The tail begins at the first chunk boundary at or after `/E`. The chunk
    // that *contains* `/E` holds page one's last bytes as well as the first of
    // the tail, so a fixed aligned granularity cannot avoid it -- and a test
    // that pretended otherwise would be measuring the granularity rather than
    // the open path. Everything beyond that chunk is the tail proper, and
    // nothing reads it.
    let tail = end_of_first_page.div_ceil(CHUNK_SIZE) * CHUNK_SIZE..bytes.len() as u64;
    assert!(
        !source.touched(&tail),
        "reads past /E ({end_of_first_page}) in a {} byte file: {:?}",
        bytes.len(),
        source.touching(&tail)
    );
    println!(
        "RAN linearized page one: fixture {} bytes, /E {end_of_first_page}, read {}",
        bytes.len(),
        source.bytes_read()
    );
    assert!(
        source.bytes_read() <= LINEARIZED_PAGE_ONE_BUDGET,
        "page one read {} bytes, over the committed budget of {LINEARIZED_PAGE_ONE_BUDGET}",
        source.bytes_read()
    );
    assert!(!document.whole_file_fetched());
}

/// Annex F's own rule for a linearized file that was updated afterwards: `/L`
/// is the length the file had when it was linearized, so a file that has grown
/// is read as an ordinary one.
#[test]
fn a_length_that_is_not_the_file_falls_back_to_the_generic_path() {
    let mut bytes = a_linearized_document(6);
    let clean = Document::open_streaming(Arc::new(SliceSource::new(bytes.clone())))
        .expect("the untouched file opens");
    assert!(clean.first_page_end().is_some(), "the fast path engaged");

    // One byte of trailing junk, which is exactly what an incremental update
    // looks like to `/L` and nothing like it to any other structure.
    bytes.push(b'\n');
    let updated =
        Document::open_streaming(Arc::new(SliceSource::new(bytes.clone()))).expect("it opens");
    assert!(
        updated.first_page_end().is_none(),
        "the fast path must stand down when /L is not the file's length"
    );
    let kinds: Vec<WarningKind> = updated.warnings().iter().map(|w| w.kind).collect();
    assert!(
        kinds.contains(&WarningKind::LinearizedLengthMismatch),
        "and it says so: {kinds:?}"
    );

    // And it is the same page, drawn the same way, off the generic path.
    let one = clean
        .page(0)
        .expect("page one")
        .render(&RenderOptions::default());
    let other = updated
        .page(0)
        .expect("page one")
        .render(&RenderOptions::default());
    assert_eq!(one.data, other.data, "the fallback draws the same page");
}

/// Leaving page one is what fetches the main table, and it costs the tail.
#[test]
fn reading_past_page_one_is_what_pays_for_the_tail() {
    let bytes = a_linearized_document(60);
    let source = Arc::new(CountingSource::new(SliceSource::new(bytes.clone())));
    let document = Document::open_streaming(source.clone()).expect("it opens");
    let end_of_first_page = document.first_page_end().expect("the fast path engaged");
    let tail = end_of_first_page.div_ceil(CHUNK_SIZE) * CHUNK_SIZE..bytes.len() as u64;

    let _ = document
        .page(0)
        .expect("page one")
        .render(&RenderOptions::default());
    assert!(!source.touched(&tail), "page one stays in the head");

    let last = document.page_count() - 1;
    let bitmap = document
        .page(last)
        .expect("the last page")
        .render(&RenderOptions::default());
    assert!(ink(&bitmap) > 1000, "the last page really drew something");
    assert!(
        source.touched(&tail),
        "the main table at /T is in the tail, and reading past page one needs it"
    );
}

// ---- Files this project did not write ----------------------------------

/// Where `cargo xtask corpus-fetch` puts the qpdf corpus, and the override for
/// a checkout that shares one fetch between worktrees.
fn qpdf_corpus() -> Option<PathBuf> {
    let named = std::env::var_os("TINKER_QPDF_CORPUS").map(PathBuf::from);
    let default =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../corpus/files/qpdf/qpdf/qtest/qpdf");
    named
        .into_iter()
        .chain(std::iter::once(default))
        .find(|dir| dir.is_dir())
}

/// Every linearized file in the corpus that this reader can open from its head
/// renders page one without reading its tail.
///
/// The writer-made fixture above proves the path works on a file laid out by
/// the same code that reads it, which is the agreement that proves the least.
/// These were linearized by somebody else.
#[test]
fn linearized_files_from_the_qpdf_corpus_render_page_one_from_their_heads() {
    let Some(dir) = qpdf_corpus() else {
        println!("SKIPPED linearized corpus render: the qpdf corpus is not fetched");
        return;
    };
    let mut files: Vec<PathBuf> = std::fs::read_dir(&dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|e| e == "pdf"))
        .collect();
    files.sort();

    let mut engaged = 0usize;
    let mut rendered = 0usize;
    let mut trespassed: Vec<String> = Vec::new();
    for path in &files {
        let Ok(bytes) = std::fs::read(path) else {
            continue;
        };
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let source = Arc::new(CountingSource::new(SliceSource::new(bytes.clone())));
        let Ok(document) = Document::open_streaming(source.clone()) else {
            continue;
        };
        let Some(end_of_first_page) = document.first_page_end() else {
            continue;
        };
        engaged += 1;
        let Some(page) = document.page(0) else {
            continue;
        };
        let _ = page.render(&RenderOptions::default());
        rendered += 1;
        let tail = end_of_first_page.div_ceil(CHUNK_SIZE) * CHUNK_SIZE..bytes.len() as u64;
        if source.touched(&tail) {
            trespassed.push(name);
        }
    }

    println!(
        "RAN linearized corpus render: {engaged} files opened from their heads, \
         {rendered} rendered page one, over {} PDFs",
        files.len()
    );
    assert_eq!(
        trespassed, REACHES_PAST_E,
        "these are the linearized corpus files whose page one reaches past /E, and no others"
    );
    assert!(
        engaged >= HEAD_OPENED_CORPUS_FILES,
        "only {engaged} corpus files opened from their heads, and {HEAD_OPENED_CORPUS_FILES} did \
         when this was measured: a shrinking set reads as a pass"
    );
    assert_eq!(rendered, engaged, "every one of them drew its first page");
}
