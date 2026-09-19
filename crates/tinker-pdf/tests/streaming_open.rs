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
    AuthLevel, CountingSource, Document, DocumentBuilder, Name, Object, RenderOptions, SliceSource,
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

/// A linearized document whose pages are a few hundred bytes each, so a byte
/// offset wrong by a kilobyte lands in some other page's run rather than a few
/// bytes short of the right one.
fn a_small_linearized_document(pages: usize) -> Vec<u8> {
    let mut builder = DocumentBuilder::new();
    for index in 0..pages {
        builder.add_page(200.0, 100.0, |page| {
            let shade = (index % 7) as f64 / 7.0;
            page.raw(
                format!("{shade:.3} 0.400 0.600 rg 20 20 m 180 40 l 100 90 l h f\n").as_bytes(),
            );
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

/// The same document with every page's `/MediaBox` moved up to the page tree
/// root, which 7.7.3.4 makes an inherited attribute and plenty of producers
/// write that way.
///
/// The one shape that separates "build this page from its own object" from
/// "walk the tree to it": a page carrying its own box is the same page either
/// way, and a page inheriting one is 200 by 100 through the walk and US Letter
/// alone. Made by patching the *unlinearized* bytes and re-saving, because the
/// linearizer computes every offset and both hint tables from the object graph
/// -- so an intermediate whose cross-reference table the patch invalidated
/// still produces a clean linearized file, and nothing here has to hand-pack a
/// hint table.
fn a_linearized_document_with_inherited_boxes(pages: usize) -> Vec<u8> {
    let mut builder = DocumentBuilder::new();
    for index in 0..pages {
        builder.add_page(200.0, 100.0, |page| {
            let shade = (index % 7) as f64 / 7.0;
            page.raw(
                format!("{shade:.3} 0.400 0.600 rg 20 20 m 180 40 l 100 90 l h f\n").as_bytes(),
            );
        });
    }
    let mut raw = builder.finish();

    // Every `/MediaBox [...]` blanked where it stands, so the patch changes no
    // length and no other object moves.
    let box_at = |bytes: &[u8]| -> Option<(usize, usize)> {
        let at = bytes.windows(9).position(|w| w == b"/MediaBox")?;
        let end = bytes[at..].iter().position(|b| *b == b']')? + at + 1;
        Some((at, end))
    };
    let (first, last) = box_at(&raw).expect("the builder writes a media box");
    let media = raw[first..last].to_vec();
    while let Some((at, end)) = box_at(&raw) {
        for byte in &mut raw[at..end] {
            *byte = b' ';
        }
    }
    // And one copy put on the tree root, which `/Kids` identifies: the builder
    // writes exactly one of those.
    let kids = raw
        .windows(5)
        .position(|w| w == b"/Kids")
        .expect("the builder writes one page tree node");
    let mut patched = raw[..kids].to_vec();
    patched.extend_from_slice(&media);
    patched.push(b' ');
    patched.extend_from_slice(&raw[kids..]);

    let base = Document::open(patched).expect("the patched document still parses");
    base.editor().save(&WriteOptions {
        mode: WriteMode::Rewrite,
        linearize: true,
        object_streams: false,
        ..WriteOptions::default()
    })
}

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

/// The linearized corpus files whose page one the head-only path produces and
/// the page tree walk does not, by name and with the reason.
///
/// Both are sealed under a password this sweep does not have, and the
/// difference is which route survives that. `/O` names the first page's object
/// outright, and a page dictionary is structure rather than content — 7.6.2
/// encrypts its strings and its streams, not its keys — so the head-only path
/// builds a page and draws whatever the content stream decrypts to, which is
/// nothing. The walk cannot start: the catalogue is inside an object stream,
/// and an object stream is a stream.
///
/// Named rather than excluded, because it is a real difference between two
/// routes to the same page and the measurement below would otherwise report
/// it as an agreement. A file that stopped needing the exception, or one that
/// started, both fail here.
const PAGE_ONE_ONLY_FROM_THE_HEAD: &[&str] = &[
    "enc-XI-R6,V5,U=view,O=master.pdf",
    "enc-XI-R6,V5,U=view,attachments,cleartext-metadata.pdf",
];

/// How many bytes a page-one render of the 60-page linearized fixture may
/// read. A ratchet, measured and committed.
///
/// 29,696 of 1,631,075 -- 1.8% -- and the shape of the number is the point:
/// `/E` is 28,222, so page one costs the head up to `/E` rounded up to the
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

/// How many bytes rendering page 31 of the 60-page linearized fixture may
/// read, on a document nothing has opened past its head. A ratchet, measured
/// and committed.
///
/// 37,888 of 1,631,075 -- 2.3% -- and the shape of the number is the point, as
/// it is for page one. Page 31 costs the head that carries the linearization
/// dictionary and the first-page cross-reference section, the primary hint
/// stream at `/H`, and its own run of objects. Nothing else: not the main
/// cross-reference table at `/T`, which is the last thing in the file, and not
/// one of the thirty pages in between. A budget far above this would mean
/// something walked the page tree to get there; one far below would mean the
/// page was not drawn.
const LINEARIZED_PAGE_N_BUDGET: u64 = 37_888;

/// And how many the same render may read on a document that has already drawn
/// page one, which is the order a reader actually pages in.
///
/// 32,768 -- eight chunks -- because the chunk cache keeps what page one paid
/// for and this is the marginal cost of one more page: the hint stream, and
/// the page's own twenty-odd kilobytes. Measured separately from the figure
/// above because the two answer different questions, and one number for both
/// would be the larger of them pretending to be the smaller.
const LINEARIZED_PAGE_N_AFTER_ONE_BUDGET: u64 = 32_768;

/// Page 31 of a linearized file costs its own bytes, not the tail.
///
/// This is the row the roadmap carried: the hint tables were read by
/// `validate::hints` and were not on the open path, so every page but the
/// first needed the main cross-reference table. Annex F's Table F.4 item 2
/// places a page by accumulating the lengths of the pages before it, which is
/// what makes a page in the middle of the file reachable from the head alone.
#[test]
fn a_linearized_file_renders_a_middle_page_without_its_main_table() {
    let bytes = a_linearized_document(60);
    let source = Arc::new(CountingSource::new(SliceSource::new(bytes.clone())));
    let document = Document::open_streaming(source.clone()).expect("it opens");
    let end_of_first_page = document
        .first_page_end()
        .expect("the linearized fast path engaged");

    let bitmap = document
        .page(30)
        .expect("page 31")
        .render(&RenderOptions::default());
    let drawn = ink(&bitmap);
    assert!(drawn > 1000, "page 31 painted {drawn} pixels");

    // And it is the same page the generic path draws. A cheap render of the
    // wrong page would satisfy every byte budget here.
    let buffered = Document::open(bytes.clone()).expect("it opens from a buffer");
    let want = buffered
        .page(30)
        .expect("page 31")
        .render(&RenderOptions::default());
    assert_eq!(bitmap.data, want.data, "the hinted page is the same page");
    assert_eq!(bitmap.warnings, want.warnings);

    println!(
        "RAN linearized middle page: fixture {} bytes, /E {end_of_first_page}, read {}",
        bytes.len(),
        source.bytes_read()
    );
    assert!(
        source.bytes_read() <= LINEARIZED_PAGE_N_BUDGET,
        "page 31 read {} bytes, over the committed budget of {LINEARIZED_PAGE_N_BUDGET}",
        source.bytes_read()
    );
    assert!(
        !document.main_table_fetched(),
        "page 31 was reached through the main cross-reference table at /T"
    );
    // And the table really is in the tail, so the line above is a claim about
    // bytes and not only about a flag: nothing read the last chunk of the file.
    let table = bytes.len() as u64 / CHUNK_SIZE * CHUNK_SIZE..bytes.len() as u64;
    assert!(
        !source.touched(&table),
        "the end of the file was read: {:?}",
        source.touching(&table)
    );
    assert!(!document.whole_file_fetched());
}

/// Paging on from page one is the marginal cost of one more page.
#[test]
fn paging_on_from_page_one_costs_one_page_and_the_hint_tables() {
    let bytes = a_linearized_document(60);
    let source = Arc::new(CountingSource::new(SliceSource::new(bytes.clone())));
    let document = Document::open_streaming(source.clone()).expect("it opens");
    let _ = document
        .page(0)
        .expect("page one")
        .render(&RenderOptions::default());
    let after_one = source.bytes_read();

    let bitmap = document
        .page(30)
        .expect("page 31")
        .render(&RenderOptions::default());
    assert!(ink(&bitmap) > 1000);
    let marginal = source.bytes_read() - after_one;
    println!("RAN paging on: page one {after_one}, page 31 +{marginal}");
    assert!(
        marginal <= LINEARIZED_PAGE_N_AFTER_ONE_BUDGET,
        "paging on read {marginal} more bytes, over the committed budget of \
         {LINEARIZED_PAGE_N_AFTER_ONE_BUDGET}"
    );
    assert!(!document.main_table_fetched());
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

/// Asking how many pages there are is what fetches the main table now.
///
/// This test used to say that *leaving page one* did, because until the hint
/// tables reached the open path nothing else could find another page. They
/// can, so the line moved: a page named by number costs its own bytes, and a
/// question about every page costs the table that indexes every object.
///
/// `/N` in the linearization parameter dictionary is the file's own claim
/// about its page count and it is not answered from here. 7.7.3.2's `/Count`
/// is a claim like any other and `pages::count` walks the tree when it
/// disagrees, so a page count taken from `/N` would be a number this reader
/// had checked nothing about -- which is the opposite of what the rest of this
/// path does with a hint.
#[test]
fn asking_for_the_page_count_is_what_pays_for_the_tail() {
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

    // The last page by number, which the hint tables place like any other.
    let bitmap = document
        .page(59)
        .expect("the last page")
        .render(&RenderOptions::default());
    assert!(ink(&bitmap) > 1000, "the last page really drew something");
    assert!(
        !document.main_table_fetched(),
        "the last page is a page like any other to a hint table"
    );

    assert_eq!(document.page_count(), 60);
    assert!(
        document.main_table_fetched(),
        "and counting them is a question about every object, which is the table"
    );
    assert!(
        source.touched(&tail),
        "the main table at /T is in the tail, and the count needs it"
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
    let mut matched = 0usize;
    let mut head_only: Vec<String> = Vec::new();
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
        let got = page.render(&RenderOptions::default());
        rendered += 1;
        // `/O` is a hint like any other: it names an object, and the page
        // built from it must be the page the tree walk builds. A cheap render
        // of the wrong page passes every byte budget there is, so the two are
        // compared rather than counted.
        if let Ok(buffered) = Document::open(bytes.clone()) {
            if document.is_encrypted() {
                let _ = document.authenticate("");
                let _ = buffered.authenticate("");
            }
            match buffered.page(0) {
                Some(reference) => {
                    let want = reference.render(&RenderOptions::default());
                    if got.data == want.data {
                        matched += 1;
                    }
                }
                None => head_only.push(name.clone()),
            }
        }
        let tail = end_of_first_page.div_ceil(CHUNK_SIZE) * CHUNK_SIZE..bytes.len() as u64;
        if source.touched(&tail) {
            trespassed.push(name);
        }
    }

    println!(
        "RAN linearized corpus render: {engaged} files opened from their heads, \
         {rendered} rendered page one, {matched} drew the buffered page, over {} PDFs",
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
    assert_eq!(
        head_only, PAGE_ONE_ONLY_FROM_THE_HEAD,
        "these are the files the page tree walk cannot produce a page one for at all"
    );
    assert_eq!(
        matched + head_only.len(),
        engaged,
        "and every file the walk *can* answer for drew the page the walk draws"
    );
}

/// How many of the linearized corpus files that open from their heads have a
/// second page to ask for. Committed, so a shrinking set cannot read as a pass.
///
/// 43 open from their heads and 14 of those are one page long, which is the
/// case Annex F's own note calls out: "in a document consisting of only one
/// page, all of that page's objects shall be treated as if they were shared".
const LINEARIZED_MULTIPAGE_CORPUS_FILES: usize = 29;

/// The multipage linearized corpus files whose page two still needs the main
/// cross-reference table, by name and with the reason.
///
/// Two families and nothing else, which is the point of naming them rather
/// than counting them. Five are encrypted under a password this sweep does not
/// have -- the primary hint stream is an ordinary stream object and 7.6.1 does
/// not exempt it, so its bytes are ciphertext and refusing to read them is the
/// right answer. Four are qpdf's own damaged linearization fixtures, three of
/// which `validate::hints` already refuses by name. A file that stopped
/// falling back, or one that started, both fail here.
const PAGE_TWO_NEEDS_THE_MAIN_TABLE: &[&str] = &[
    "badlin1.pdf: a deliberately damaged linearization",
    "enc-R2,V1,U=view,O=master.pdf: the hint stream is ciphertext",
    "enc-R2,V1,U=view,O=view.pdf: the hint stream is ciphertext",
    "enc-R3,V2,U=view,O=master.pdf: the hint stream is ciphertext",
    "enc-R3,V2,U=view,O=view.pdf: the hint stream is ciphertext",
    "enc-long-password.pdf: the hint stream is ciphertext",
    "linearization-bounds-1.pdf: a deliberately damaged linearization",
    "linearization-bounds-2.pdf: a deliberately damaged linearization",
    "linearization-large-vector-alloc.pdf: a deliberately damaged linearization",
];

/// Page two of a linearized file this project did not write is the same page
/// the main table gives, and mostly costs no part of it.
///
/// Two claims, and the first one carries the policy. *Hints accelerate, they
/// never decide*: every page here is rendered twice, once through the hint
/// tables over a streamed source and once through the page tree out of a
/// buffer, and the two bitmaps are compared pixel for pixel. A hint table that
/// misdirected a read would draw a different page, and a cheap render of the
/// wrong page would pass every byte budget in this file.
///
/// The second is the roadmap's exit criterion measured over files somebody
/// else linearized: [`Document::main_table_fetched`] stays false, so page two
/// was reached without the table at `/T`.
#[test]
fn linearized_corpus_files_reach_page_two_without_their_main_tables() {
    let Some(dir) = qpdf_corpus() else {
        println!("SKIPPED linearized corpus page two: the qpdf corpus is not fetched");
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

    let mut multipage = 0usize;
    let mut matched = 0usize;
    let mut hinted = 0usize;
    let mut fell_back: Vec<String> = Vec::new();
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
        if document.first_page_end().is_none() {
            continue;
        }
        let Ok(buffered) = Document::open(bytes.clone()) else {
            continue;
        };
        // The empty user password and nothing else, which is what a reader
        // tries before it asks anyone: a document encrypted only to restrict
        // permissions opens with it, and one that wants a real password stays
        // sealed and is named below.
        if document.is_encrypted() {
            let _ = document.authenticate("");
            let _ = buffered.authenticate("");
        }
        if buffered.page_count() < 2 {
            continue;
        }
        multipage += 1;

        let Some(page) = document.page(1) else {
            fell_back.push(format!("{name}: no page two at all"));
            continue;
        };
        let got = page.render(&RenderOptions::default());
        let want = buffered
            .page(1)
            .expect("page two")
            .render(&RenderOptions::default());
        if got.data == want.data && got.warnings == want.warnings {
            matched += 1;
        }
        if document.main_table_fetched() {
            let sealed = document.is_encrypted() && document.auth_level() == AuthLevel::None;
            fell_back.push(format!(
                "{name}: {}",
                if sealed {
                    "the hint stream is ciphertext"
                } else {
                    "a deliberately damaged linearization"
                }
            ));
        } else {
            hinted += 1;
        }
    }

    println!(
        "RAN linearized corpus page two: {multipage} multipage files, {hinted} reached page two \
         from their heads, {matched} drew the buffered page"
    );
    assert_eq!(
        fell_back, PAGE_TWO_NEEDS_THE_MAIN_TABLE,
        "these are the files whose page two still needs the main table, and no others"
    );
    assert_eq!(
        multipage, LINEARIZED_MULTIPAGE_CORPUS_FILES,
        "this many linearized corpus files have a page two to ask for"
    );
    assert_eq!(
        matched, multipage,
        "every one of them must draw the page the main table draws: hints accelerate, they \
         never decide"
    );
}

/// A sealed document's hint tables are read the moment it is unsealed.
///
/// The hint stream is an ordinary stream object and 7.6.1 exempts only three
/// things from encryption, none of them this one -- so on an encrypted
/// document the tables decode to nothing until a password arrives, and a
/// refusal cached before that would seal the accelerator shut for the life of
/// the document. `install_security` drops it with everything else that was
/// read as plaintext out of ciphertext, and this is what says so.
#[test]
fn authenticating_reopens_the_hint_tables_a_sealed_document_refused() {
    let Some(dir) = qpdf_corpus() else {
        println!("SKIPPED sealed hint tables: the qpdf corpus is not fetched");
        return;
    };
    // Encrypted with an owner password only, so the empty user password opens
    // it -- and thirty pages, so page two is a long way from the head.
    let path = dir.join("enc-R3,V2.pdf");
    let Ok(bytes) = std::fs::read(&path) else {
        println!("SKIPPED sealed hint tables: {} is absent", path.display());
        return;
    };
    let source = Arc::new(CountingSource::new(SliceSource::new(bytes.clone())));
    let document = Document::open_streaming(source).expect("it opens");
    assert!(document.is_encrypted(), "the fixture is encrypted");
    assert!(document.first_page_end().is_some(), "the fast path engaged");

    // Before the password: the tables are ciphertext, so page two comes off
    // the page tree and the main table is fetched.
    let _ = document
        .page(1)
        .expect("page two")
        .render(&RenderOptions::default());
    assert!(
        document.main_table_fetched(),
        "an unauthenticated document cannot read its own hint tables"
    );
    let kinds: Vec<WarningKind> = document.warnings().iter().map(|w| w.kind).collect();
    assert!(
        kinds.contains(&WarningKind::LinearizedHintsUnusable),
        "and it says so rather than failing: {kinds:?}"
    );

    assert!(
        !document.cos().hint_tables_read(),
        "nothing was read out of ciphertext"
    );

    // The password arrives on the *same* document, which is the case the
    // clearing exists for: a refusal cached over ciphertext would seal the
    // accelerator shut for the life of the document, and every later page
    // would pay for a table it did not need.
    document.authenticate("").expect("the empty user password");
    let after = document
        .page(2)
        .expect("page three")
        .render(&RenderOptions::default());
    assert!(
        document.cos().hint_tables_read(),
        "and the tables are read the moment they stop being ciphertext"
    );

    // And it is the same page the generic path draws.
    let buffered = Document::open(bytes).expect("it opens from a buffer");
    buffered.authenticate("").expect("the empty user password");
    let want = buffered
        .page(2)
        .expect("page three")
        .render(&RenderOptions::default());
    assert_eq!(after.data, want.data);
}

/// A page length of zero would hand back the page before it, and does not.
///
/// The one hint in these tables that can change an *answer* rather than a
/// cost: a page's location is the accumulated lengths of the pages before it
/// (Table F.4 item 2), so a length stated as zero puts the next page's run at
/// the previous page's page object -- which is a page leaf with its own
/// `/MediaBox` and `/Resources`, and passes every other test here. Annex F's
/// layout is what refuses it: part 7 follows part 6, so a page after the first
/// begins at or past `/E`, and each page occupies bytes.
#[test]
fn a_page_length_of_zero_does_not_hand_back_the_page_before_it() {
    let clean = a_small_linearized_document(12);
    let want = buffered_page(&clean, 1);
    let first = buffered_page(&clean, 0);
    assert_ne!(
        want.data, first.data,
        "the fixture's two pages differ, or this test could not fail"
    );

    let mut bytes = clean.clone();
    // Item 4 of Table F.3 is the least page length and every page's length is
    // measured from it, so zero here with this writer's equal-length pages is
    // every page stating zero. It lands at bytes 10 to 14 of the table.
    let table = hint_table_at(&bytes);
    let least = u32::from_be_bytes([
        bytes[table + 10],
        bytes[table + 11],
        bytes[table + 12],
        bytes[table + 13],
    ]);
    assert!(least > 0, "the writer states a least page length");
    bytes[table + 10..table + 14].copy_from_slice(&0u32.to_be_bytes());

    let (document, got) = page_two_streamed(&bytes);
    assert_eq!(got.data, want.data, "page two is page two");
    assert_ne!(got.data, first.data, "and is not page one");
    assert!(
        document.main_table_fetched(),
        "the tables were refused and the page tree answered"
    );
    let kinds: Vec<WarningKind> = document.warnings().iter().map(|w| w.kind).collect();
    assert!(
        kinds.contains(&WarningKind::LinearizedHintsUnusable),
        "and the refusal is named (ruling 10): {kinds:?}"
    );
}

// ---- when the tables lie -----------------------------------------------
//
// Hints accelerate, they never decide. Everything below takes a file this
// writer produced, makes one statement in it false, and asserts three things:
// the page still comes out, it is the *same* page, and the leniency is named
// (ruling 10). The corpus cannot produce these -- qpdf's damaged linearization
// fixtures are damaged in their own ways, not in these -- so they are made
// here, one byte at a time, out of a file that was correct before.

/// The primary hint stream's offset, out of the linearization parameter
/// dictionary in the file's own head (F.3.3).
///
/// Read out of the text rather than through the reader, because these tests
/// are about to make the file disagree with itself and the reader is the thing
/// under test.
fn hint_stream_at(bytes: &[u8]) -> usize {
    let at = bytes
        .windows(4)
        .position(|w| w == b"/H [")
        .expect("a /H entry in the parameter dictionary");
    let digits: Vec<u8> = bytes[at + 4..]
        .iter()
        .copied()
        .skip_while(|b| b.is_ascii_whitespace())
        .take_while(u8::is_ascii_digit)
        .collect();
    std::str::from_utf8(&digits)
        .expect("ascii digits")
        .parse()
        .expect("an offset")
}

/// Reads an `N 0 obj` header beginning exactly at `at`, returning the object
/// number and how many digits it is spelled with. `None` when there is no
/// header there.
///
/// Generic in the number, because nothing reserves one. F.3.6 gives the
/// primary hint stream *the last object number in the file*, so which number
/// that is depends on how many objects the document has — a test that looked
/// for `2 0 obj` would be pinning one fixture's arithmetic.
fn object_header_at(bytes: &[u8], at: usize) -> Option<(u32, usize)> {
    let digits: Vec<u8> = bytes
        .get(at..)?
        .iter()
        .copied()
        .take_while(u8::is_ascii_digit)
        .collect();
    if digits.is_empty() || !bytes.get(at + digits.len()..)?.starts_with(b" 0 obj") {
        return None;
    }
    let number = std::str::from_utf8(&digits).ok()?.parse().ok()?;
    Some((number, digits.len()))
}

fn object_number_at(bytes: &[u8], at: usize) -> (u32, usize) {
    object_header_at(bytes, at).unwrap_or_else(|| panic!("an `N 0 obj` header begins at {at}"))
}

/// Where the first indirect object's header begins. F.3.3 puts the
/// linearization parameter dictionary there, whatever number it carries.
fn first_object_at(bytes: &[u8]) -> usize {
    (0..bytes.len())
        .find(|at| {
            (*at == 0 || matches!(bytes[at - 1], b'\n' | b'\r'))
                && object_header_at(bytes, *at).is_some()
        })
        .expect("an indirect object in the file")
}

/// Where the page offset hint table's own bytes begin (F.4: it is the first
/// table in the stream and starts at offset 0).
///
/// The writer never compresses the hint stream -- `/H` measures the object's
/// length and shrinking it afterwards would move every offset computed from
/// it -- so the stream data *is* the table and a byte patched here is a field
/// patched in Table F.3.
fn hint_table_at(bytes: &[u8]) -> usize {
    let object = hint_stream_at(bytes);
    let (_, width) = object_number_at(bytes, object);
    let from = object + width;
    let keyword = bytes[from..]
        .windows(6)
        .position(|w| w == b"stream")
        .expect("the stream keyword")
        + from
        + 6;
    // 7.3.8.1: the keyword is followed by CRLF or LF, and the data follows.
    let mut data = keyword;
    if bytes.get(data) == Some(&b'\r') {
        data += 1;
    }
    if bytes.get(data) == Some(&b'\n') {
        data += 1;
    }
    data
}

/// The same page, drawn off the generic path, for comparing a fallback
/// against.
fn buffered_page(bytes: &[u8], index: u32) -> tinker_pdf::Bitmap {
    Document::open(bytes.to_vec())
        .expect("it opens from a buffer")
        .page(index)
        .expect("the page")
        .render(&RenderOptions::default())
}

/// Renders page two of `bytes` over a counting source and reports what the
/// document had to say about it.
fn page_two_streamed(bytes: &[u8]) -> (Document, tinker_pdf::Bitmap) {
    let source = Arc::new(CountingSource::new(SliceSource::new(bytes.to_vec())));
    let document = Document::open_streaming(source).expect("it opens");
    assert!(
        document.first_page_end().is_some(),
        "the head-only path still engages: only a hint table was made false"
    );
    let bitmap = document
        .page(1)
        .expect("page two")
        .render(&RenderOptions::default());
    (document, bitmap)
}

/// Table F.3 item 2 against the first-page cross-reference section's own entry
/// for the object `/O` names.
///
/// Table F.4 item 2 gives a reader both routes to the first page's location,
/// which makes them the one pair of statements about the same byte that a
/// reader holding only the head can check against each other. A file whose
/// tables disagree with the table page one was opened from is describing some
/// other layout, and nothing else in them is believed.
#[test]
fn a_hint_table_that_disagrees_with_the_first_page_section_is_refused() {
    let clean = a_linearized_document(4);
    let want = buffered_page(&clean, 1);

    let mut bytes = clean.clone();
    // Item 2 is the second 32-bit field of Table F.3, so it lands at bytes 4
    // to 8 of the stream data. One higher is a first page that begins one byte
    // into its own object header.
    let table = hint_table_at(&bytes);
    bytes[table + 7] = bytes[table + 7].wrapping_add(1);
    assert_ne!(bytes, clean, "the patch landed");

    let (document, got) = page_two_streamed(&bytes);
    assert_eq!(got.data, want.data, "the page tree draws the same page two");
    assert!(
        document.main_table_fetched(),
        "and it cost the main table, which is what the fallback is"
    );
    let kinds: Vec<WarningKind> = document.warnings().iter().map(|w| w.kind).collect();
    assert!(
        kinds.contains(&WarningKind::LinearizedHintsUnusable),
        "the leniency is named (ruling 10): {kinds:?}"
    );
}

/// A page run whose leading object is not a page leaf.
///
/// Table F.4 item 1 makes a page's own page object the first object of its
/// run, so what sits at the front of the range the tables name is checkable
/// against what the file's own object header says is there. Item 4 of Table
/// F.3 is the least page length, which every page's length is measured from,
/// so one higher moves every page after the first a byte past its own header
/// -- and the object that answers there is the page's content stream, which is
/// not a page.
#[test]
fn a_hint_run_whose_leading_object_is_not_a_page_is_refused() {
    let clean = a_linearized_document(4);
    let want = buffered_page(&clean, 1);

    let mut bytes = clean.clone();
    // Item 4 is the third field and the first two are 32 bits and 32 bits with
    // a 16-bit item 3 between, so it lands at bytes 10 to 14.
    let table = hint_table_at(&bytes);
    bytes[table + 13] = bytes[table + 13].wrapping_add(1);
    assert_ne!(bytes, clean, "the patch landed");

    let (document, got) = page_two_streamed(&bytes);
    assert_eq!(got.data, want.data, "the page tree draws the same page two");
    assert!(document.main_table_fetched());
    let kinds: Vec<WarningKind> = document.warnings().iter().map(|w| w.kind).collect();
    assert!(
        kinds.contains(&WarningKind::LinearizedPageHintRejected),
        "the leniency names the object it found instead (ruling 10): {kinds:?}"
    );
    let named = document
        .warnings()
        .iter()
        .find(|w| w.kind == WarningKind::LinearizedPageHintRejected)
        .and_then(|w| w.object);
    assert!(named.is_some(), "and it says which object that was");
}

/// A hint stream the first-page cross-reference section places somewhere else.
///
/// The stream is read through the ordinary object path, so its offset has to
/// be recorded before it can be -- and a file whose own first-page table puts
/// that object number at a different byte is a file making two statements
/// about one object. Reading it anyway would decode whichever the table won,
/// which is some other object's bytes read as Table F.3.
#[test]
fn a_hint_stream_the_first_page_table_places_elsewhere_is_refused() {
    let clean = a_linearized_document(4);
    let want = buffered_page(&clean, 1);

    let mut bytes = clean.clone();
    // `/H` points at the hint stream's own `N 0 obj` header. Overwriting that
    // number with the linearization parameter dictionary's -- the first object
    // in the file, which F.3.3 requires the first-page section to carry an
    // entry for, at its own offset near byte zero -- makes the two statements
    // disagree and changes nothing else: every read of that object goes to the
    // section's offset and finds it there.
    //
    // Zero-padded to the header's own width, which 7.3.3 permits, so no offset
    // moves. Overwriting one digit is what this did first, and the day the
    // hint stream's number grew past nine that was a patch that changed
    // nothing and a test that asserted nothing.
    let object = hint_stream_at(&bytes);
    let (hint, width) = object_number_at(&bytes, object);
    let parameters = object_number_at(&bytes, first_object_at(&bytes)).0;
    assert_ne!(parameters, hint, "two different objects");
    let spelled = format!("{parameters:0width$}");
    assert_eq!(spelled.len(), width, "the patch keeps the header's width");
    bytes[object..object + width].copy_from_slice(spelled.as_bytes());
    assert_ne!(bytes, clean, "the patch landed");

    let (document, got) = page_two_streamed(&bytes);
    assert_eq!(got.data, want.data, "the page tree draws the same page two");
    assert!(document.main_table_fetched());
    let kinds: Vec<WarningKind> = document.warnings().iter().map(|w| w.kind).collect();
    assert!(
        kinds.contains(&WarningKind::LinearizedHintsUnusable),
        "the leniency is named (ruling 10): {kinds:?}"
    );
}

/// A page that inherits its `/MediaBox` is built by the walk, not from `/O`
/// or from a hint table's run.
///
/// Both routes into `page_alone` skip the tree, which is only sound when the
/// object carries the attributes a page is laid out from -- 7.7.3.4 lets
/// `/MediaBox` and `/Resources` come from an ancestor, and an ancestor is
/// exactly what neither route fetched. A page laid out at US Letter because
/// its parent was not read is the wrong page rather than a cheaper one, so
/// both decline and the walk answers.
///
/// Asserted on the page's *size*, which is the observable the mistake would
/// change: 200 by 100 through the tree, 612 by 792 from the object alone.
#[test]
fn a_page_that_inherits_its_media_box_is_not_built_from_its_own_object() {
    let bytes = a_linearized_document_with_inherited_boxes(12);

    let buffered = Document::open(bytes.clone()).expect("it opens from a buffer");
    let want = buffered
        .page(1)
        .expect("page two")
        .render(&RenderOptions::default());
    assert_eq!(
        (want.width, want.height),
        (200, 100),
        "the fixture inherits a 200 by 100 box, or this test could not fail"
    );

    let source = Arc::new(CountingSource::new(SliceSource::new(bytes.clone())));
    let document = Document::open_streaming(source).expect("it opens");
    assert!(
        document.first_page_end().is_some(),
        "the head-only path still engages"
    );

    // Page one, which `/O` names, and page two, which the hint tables place.
    for index in [0u32, 1] {
        let got = document
            .page(index)
            .expect("the page")
            .render(&RenderOptions::default());
        let reference = buffered
            .page(index)
            .expect("the page")
            .render(&RenderOptions::default());
        assert_eq!(
            (got.width, got.height),
            (reference.width, reference.height),
            "page {index} is laid out from the box it inherits"
        );
        assert_eq!(got.data, reference.data, "and drawn the same");
    }
    // Declining is not expensive here, and that is worth recording rather
    // than assuming either way: this writer keeps the catalogue and the page
    // tree root in part 4, which the first-page cross-reference section
    // indexes, so the walk runs entirely inside the head. A layout that left
    // the root in the tail -- qpdf's does -- would pay for it.
    assert!(
        !document.whole_file_fetched(),
        "and the walk that answered stayed inside the head"
    );
}

/// Bytes before `%PDF-` shift every offset the file states, Annex F's among
/// them.
///
/// 7.5.2 allows a file to begin with something else and says the header may be
/// anywhere in the first 1 024 bytes, with every stored offset measured from
/// it. Table F.1 says `/L` is "the length of the entire file", so a linearized
/// file carrying a prefix states a length that *includes* it while `/H`, `/O`,
/// `/E`, `/T` and every position in the hint tables do not -- and the head-only
/// open has to hold both readings at once or it reads the wrong bytes for the
/// right reasons.
///
/// The fixture is made rather than found: the one file in the fetched qpdf
/// corpus with leading junk states `/L` as the length *without* it, which
/// Table F.1 makes a mismatch and therefore an ordinary PDF -- so the corpus
/// says nothing about this case and the byte patch below is what does.
#[test]
fn a_linearized_file_behind_leading_junk_still_opens_from_its_head() {
    let clean = a_small_linearized_document(12);
    // Long enough to reach past a whole page's run, so a reader that dropped
    // the shift would not merely ask for a window a few bytes early -- it
    // would ask for some other page's.
    let mut bytes = vec![b'%'; 2048];
    let shift = bytes.len() as u64;
    bytes.extend_from_slice(&clean);
    // `/L` is written to a fixed ten-digit width, which is what lets it be
    // restated without moving anything.
    let at = bytes
        .windows(3)
        .position(|w| w == b"/L ")
        .expect("an /L entry")
        + 3;
    let total = format!("{:010}", bytes.len());
    bytes[at..at + 10].copy_from_slice(total.as_bytes());

    let source = Arc::new(CountingSource::new(SliceSource::new(bytes.clone())));
    let document = Document::open_streaming(source.clone()).expect("it opens");
    let end_of_first_page = document
        .first_page_end()
        .expect("the head-only path engages: /L is the whole file");
    assert!(
        end_of_first_page > shift,
        "/E is a document offset, not a header-relative one"
    );

    let got = document
        .page(8)
        .expect("page nine")
        .render(&RenderOptions::default());
    let want = buffered_page(&bytes, 8);
    assert_eq!(got.data, want.data, "the same page nine");
    assert!(ink(&got) > 500, "and it really drew it");
    assert!(
        !document.main_table_fetched(),
        "and it came out of the hint tables, shift and all"
    );
    // What is *not* fixed here, pinned rather than left to be discovered.
    // The first-page cross-reference section states its entries from the
    // header too, and the pass that reconciles those with the file's own
    // `N G obj` headers is the eager one a streamed open defers -- so the
    // objects that section places are found by the repair scanner instead,
    // which fetches everything and says so. Page two does not need them and
    // is drawn from its head regardless, which is why the assertions above
    // hold; a change that closes this has to come here and delete it.
    let kinds: Vec<WarningKind> = document.warnings().iter().map(|w| w.kind).collect();
    assert!(
        kinds.contains(&WarningKind::WholeFileFetched),
        "the first-page section's own entries are not shifted, and that is warned: {kinds:?}"
    );
}
