//! The text group: text read out of pages.
//!
//! The harness, the sampling and the rules every name here keeps are
//! `support/mod.rs`'s; `baseline.json`'s `text` list names these operations
//! in this order. Each input is asserted to be what its operation reads
//! before anything is timed: its pages, its face embedded, its text holding
//! what each search finds.

mod support;

use std::hint::black_box;
use std::sync::Arc;

use criterion::{criterion_group, criterion_main, Criterion, SamplingMode};
use support::cli::{self, Cli};
use support::{inputs, Per};
use tinker_pdf::{Document, SearchOptions, TextFormat, TextPage, TextWriter};

fn text(c: &mut Criterion) {
    let doc = Document::open(inputs::three_pages()).expect("it opens");
    let page = doc.page(0).expect("a page");

    let dense = pages(inputs::dense(100, 0), 100);
    let held = Document::open(Arc::clone(&dense)).expect("it opens");
    assert!(
        held.fonts()
            .iter()
            .any(|font| font.name == "LiberationSerif" && font.program.is_some()),
        "the dense pages embed their face"
    );
    let prebuilt: Vec<TextPage> = held.pages().iter().map(|page| page.text()).collect();
    let unaccented = SearchOptions {
        whole_word: true,
        diacritic_insensitive: true,
        ..SearchOptions::default()
    };
    for (index, text) in prebuilt.iter().enumerate() {
        assert!(
            text.plain_text().contains(inputs::PHRASE),
            "dense page {index} does not read its phrase"
        );
        assert!(
            !text.search(inputs::PHRASE).is_empty(),
            "the phrase is not found on dense page {index}"
        );
        assert!(
            !text.search_with(inputs::UNACCENTED, &unaccented).is_empty(),
            "`{}` is not found, whole and unaccented, on dense page {index}",
            inputs::UNACCENTED
        );
    }
    let producer = pages(inputs::producer(100), 100);
    let standard = Document::open(Arc::clone(&producer)).expect("it opens");
    assert!(
        standard
            .page(0)
            .expect("a page")
            .text()
            .plain_text()
            .contains(inputs::PHRASE),
        "the standard-14 pages do not read their phrase"
    );
    let small_1000 = pages(inputs::small(1000), 1000);
    let small_10000 = pages(inputs::small(10000), 10000);

    let mut group = c.benchmark_group("text");
    group.sampling_mode(SamplingMode::Flat);

    group.throughput(Per::Pages(1).throughput());
    group.bench_function("extract a page of text", |b| {
        b.iter(|| black_box(page.text().plain_text()));
    });

    group.throughput(Per::Pages(100).throughput());
    group.bench_function("text of 100 dense pages", |b| {
        b.iter(|| black_box(every_page_text(&dense)));
    });

    group.bench_function("text of 100 standard-14 pages", |b| {
        b.iter(|| black_box(every_page_text(&producer)));
    });

    group.throughput(Per::Pages(1000).throughput());
    group.bench_function("text of 1000 small pages", |b| {
        b.iter(|| black_box(every_page_text(&small_1000)));
    });

    group.throughput(Per::Pages(10000).throughput());
    group.bench_function("text of 10000 small pages", |b| {
        b.iter(|| black_box(every_page_text(&small_10000)));
    });

    group.throughput(Per::Pages(100).throughput());
    group.bench_function("structured JSON of 100 pages", |b| {
        b.iter(|| {
            let doc = Document::open(Arc::clone(&dense)).expect("it opens");
            let mut writer = TextWriter::new(TextFormat::Json);
            let mut written = 0usize;
            for index in 0..doc.page_count() {
                if let Some(page) = doc.page(index) {
                    writer.page(&page.text_frame(), &page.text());
                    written += writer.take().len();
                }
            }
            written += writer.finish().len();
            black_box(written)
        });
    });

    group.bench_function("search a phrase over 100 pages from scratch", |b| {
        b.iter(|| {
            let doc = Document::open(Arc::clone(&dense)).expect("it opens");
            let mut hits = 0usize;
            for index in 0..doc.page_count() {
                if let Some(page) = doc.page(index) {
                    hits += page.text().search(inputs::PHRASE).len();
                }
            }
            black_box(hits)
        });
    });

    group.throughput(Per::Pages(500).throughput());
    group.bench_function("search 100 pages' prebuilt text", |b| {
        b.iter(|| {
            let mut hits = 0usize;
            for _ in 0..5 {
                for text in &prebuilt {
                    hits += text.search(inputs::PHRASE).len();
                }
            }
            black_box(hits)
        });
    });

    group.throughput(Per::Pages(300).throughput());
    group.bench_function(
        "search 100 pages' prebuilt text, whole words, unaccented",
        |b| {
            b.iter(|| {
                let mut hits = 0usize;
                for _ in 0..3 {
                    for text in &prebuilt {
                        hits += text.search_with(inputs::UNACCENTED, &unaccented).len();
                    }
                }
                black_box(hits)
            });
        },
    );

    // The same text through the CLI, one child an iteration: plain text with
    // a form feed after every page, and the structured text as JSON.
    let dense_pdf = cli::fixture("dense-100.pdf", || dense.to_vec());
    let small_1000_pdf = cli::fixture("small-1000.pdf", || small_1000.to_vec());
    let small_10000_pdf = cli::fixture("small-10000.pdf", || small_10000.to_vec());
    let dense_text = Cli::new("text-dense-100");
    group
        .throughput(Per::Pages(100).throughput())
        .bench_function("text of 100 dense pages, CLI", |b| {
            b.iter(|| dense_text.run(&[&"text", &dense_pdf]));
        });
    assert_eq!(dense_text.stdout().matches('\u{c}').count(), 100);
    assert_eq!(
        dense_text.stdout().matches(inputs::PHRASE).count(),
        100,
        "every dense page's phrase"
    );
    let small_text = Cli::new("text-small-1000");
    group
        .throughput(Per::Pages(1000).throughput())
        .bench_function("text of 1000 small pages, CLI", |b| {
            b.iter(|| small_text.run(&[&"text", &small_1000_pdf]));
        });
    assert_eq!(small_text.stdout().matches('\u{c}').count(), 1000);
    let more_text = Cli::new("text-small-10000");
    group
        .throughput(Per::Pages(10000).throughput())
        .bench_function("text of 10000 small pages, CLI", |b| {
            b.iter(|| more_text.run(&[&"text", &small_10000_pdf]));
        });
    assert_eq!(more_text.stdout().matches('\u{c}').count(), 10000);
    let json = Cli::new("text-json-100");
    group
        .throughput(Per::Pages(100).throughput())
        .bench_function("structured JSON of 100 pages, CLI", |b| {
            b.iter(|| json.run(&[&"text", &dense_pdf, &"--json"]));
        });
    let written = json.stdout();
    assert!(written.starts_with("{\"format\":\"tinker-pdf/text\""));
    assert_eq!(
        written.matches("{\"index\":").count(),
        100,
        "the JSON's pages"
    );

    group.finish();
}

/// `bytes`, which open to `count` pages.
fn pages(bytes: Vec<u8>, count: u32) -> Arc<[u8]> {
    let bytes: Arc<[u8]> = bytes.into();
    let doc = Document::open(Arc::clone(&bytes)).expect("it opens");
    assert_eq!(doc.page_count(), count, "the input's pages");
    bytes
}

/// The document opened from `bytes`, and every page's text read, by index,
/// as `tpdf text` reads it.
fn every_page_text(bytes: &Arc<[u8]>) -> usize {
    let doc = Document::open(Arc::clone(bytes)).expect("it opens");
    let mut read = 0usize;
    for index in 0..doc.page_count() {
        if let Some(page) = doc.page(index) {
            read += page.text().plain_text().len();
        }
    }
    read
}

criterion_group! {
    name = benches;
    config = support::config();
    targets = text
}
criterion_main!(benches);
