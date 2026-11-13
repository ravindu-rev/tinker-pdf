//! The text group: text read out of pages.
//!
//! The harness, the sampling and the rules every name here keeps are
//! `support/mod.rs`'s; `baseline.json`'s `text` list names these operations
//! in this order. Each input is asserted to be what its operation reads
//! before that operation is timed: its pages, its face embedded, its text
//! holding what each search finds.
//!
//! Each operation is sized into the set's 20-300 ms window on the hosted
//! runner (`docs/design/performance.md`): five dense pages where a whole
//! read of the dense text is longer, twenty standard-14 pages, 4 000 small
//! pages beside 1 000, and the shorter ones repeated inside an iteration,
//! their unit counting every call.
//!
//! Beside the two searches of the 100 pages' prebuilt text, an operation for
//! each phase of each: the work `TextPage::search` and `TextPage::search_with`
//! do on every call, split where its cost could be — folding and joining each
//! line, the whole-word search's UAX #29 boundaries, `str::find` with its
//! restarts, and mapping each hit back to glyphs — each run from the content
//! crate's public pieces as the search runs them, over what the phases before
//! it produce, made once beforehand and checked to find the search's own
//! quads. `cargo xtask search-split` reads a run of this group and prints
//! each phase's share of its search, failing where the phases together are
//! not within a tenth of it (`docs/verification.md`, "Where prebuilt-text
//! search spends its time").

mod support;

use std::cell::LazyCell;
use std::hint::black_box;
use std::path::Path;
use std::sync::Arc;

use criterion::{criterion_group, criterion_main, Criterion};
use support::cli::Cli;
use support::{inputs, Group, Per};
use tinker_pdf::{Document, Quad, SearchOptions, TextFormat, TextLine, TextPage, TextWriter};
use tinker_pdf_content::{fold_diacritics, word_boundaries};

fn text(c: &mut Criterion) {
    let dense = LazyCell::new(|| dense_pages(100));
    let dense_5 = LazyCell::new(|| dense_pages(5));
    let unaccented = SearchOptions {
        whole_word: true,
        diacritic_insensitive: true,
        ..SearchOptions::default()
    };
    let prebuilt = LazyCell::new(|| {
        let held = Document::open(Arc::clone(&dense)).expect("it opens");
        let prebuilt: Vec<TextPage> = held.pages().iter().map(|page| page.text()).collect();
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
        prebuilt
    });
    // The two searches' lines as each reads them, every phase's output made
    // once, so that a phase is timed over what the phases before it produce.
    let plain = LazyCell::new(|| {
        Prepared::of(&prebuilt, inputs::PHRASE, &lowercase, false, |text| {
            text.search(inputs::PHRASE)
        })
    });
    let unaccented_lines = LazyCell::new(|| {
        Prepared::of(&prebuilt, inputs::UNACCENTED, &unaccent, true, |text| {
            text.search_with(inputs::UNACCENTED, &unaccented)
        })
    });
    let small_1000 = LazyCell::new(|| small_pages(1000));
    let small_4000 = LazyCell::new(|| small_pages(4000));
    let dense_pdf = LazyCell::new(|| inputs::store().file("dense-5.pdf", || dense_5.to_vec()));

    let mut group = Group::new(c, "text");

    group.operation("extract a page of text", |op| {
        let doc = Document::open(inputs::store().bytes("three-pages.pdf", inputs::three_pages))
            .expect("it opens");
        let page = doc.page(0).expect("a page");
        op.per(Per::Pages(1));
        op.iter(move || black_box(page.text().plain_text()));
    });

    group.operation("text of 5 dense pages", |op| {
        op.per(Per::Pages(5));
        op.iter(|| black_box(every_page_text(&dense_5)));
    });

    group.operation("text of 20 standard-14 pages", |op| {
        let producer = pages(
            inputs::store().bytes("producer-20.pdf", || inputs::producer(20)),
            20,
        );
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
        op.per(Per::Pages(20));
        op.iter(move || black_box(every_page_text(&producer)));
    });

    group.operation("text of 1000 small pages", |op| {
        op.per(Per::Pages(2 * 1000));
        op.iter(|| {
            for _ in 0..2 {
                black_box(every_page_text(&small_1000));
            }
        });
    });

    group.operation("text of 4000 small pages", |op| {
        op.per(Per::Pages(4000));
        op.iter(|| black_box(every_page_text(&small_4000)));
    });

    group.operation("structured JSON of 5 pages", |op| {
        op.per(Per::Pages(5));
        op.iter(|| {
            let doc = Document::open(Arc::clone(&dense_5)).expect("it opens");
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

    group.operation("search a phrase over 5 pages from scratch", |op| {
        op.per(Per::Pages(5));
        op.iter(|| {
            let doc = Document::open(Arc::clone(&dense_5)).expect("it opens");
            let mut hits = 0usize;
            for index in 0..doc.page_count() {
                if let Some(page) = doc.page(index) {
                    hits += page.text().search(inputs::PHRASE).len();
                }
            }
            black_box(hits)
        });
    });

    group.operation("search 100 pages' prebuilt text", |op| {
        op.per(Per::Pages(500));
        op.iter(|| {
            let mut hits = 0usize;
            for _ in 0..5 {
                for text in prebuilt.iter() {
                    hits += text.search(inputs::PHRASE).len();
                }
            }
            black_box(hits)
        });
    });

    group.operation(
        "search 100 pages' prebuilt text, whole words, unaccented",
        |op| {
            op.per(Per::Pages(200));
            op.iter(|| {
                let mut hits = 0usize;
                for _ in 0..2 {
                    for text in prebuilt.iter() {
                        hits += text.search_with(inputs::UNACCENTED, &unaccented).len();
                    }
                }
                black_box(hits)
            });
        },
    );

    // The two searches phase by phase, each phase a call at a time as its
    // search's operation makes them: five over the 100 pages for the plain
    // search, two for the whole-word one.
    group.operation("plain prebuilt search, folding and joining", |op| {
        op.per(Per::Pages(500));
        op.iter(|| {
            let mut folded = 0usize;
            for _ in 0..5 {
                for text in prebuilt.iter() {
                    folded += fold_and_join(text, inputs::PHRASE, &lowercase);
                }
            }
            black_box(folded)
        });
    });
    group.operation("plain prebuilt search, find and its restarts", |op| {
        op.per(Per::Pages(500));
        op.iter(|| {
            let mut found = 0usize;
            for _ in 0..5 {
                for line in plain.lines() {
                    found += find_hits(&line.joined, &plain.needle, None).len();
                }
            }
            black_box(found)
        });
    });
    group.operation("plain prebuilt search, mapping hits to glyphs", |op| {
        op.per(Per::Pages(500));
        op.iter(|| {
            let mut mapped = 0usize;
            for _ in 0..5 {
                for line in plain.lines() {
                    mapped += map_hits(line).len();
                }
            }
            black_box(mapped)
        });
    });
    group.operation("unaccented whole-word search, folding and joining", |op| {
        op.per(Per::Pages(200));
        op.iter(|| {
            let mut folded = 0usize;
            for _ in 0..2 {
                for text in prebuilt.iter() {
                    folded += fold_and_join(text, inputs::UNACCENTED, &unaccent);
                }
            }
            black_box(folded)
        });
    });
    group.operation(
        "unaccented whole-word search, UAX 29 word boundaries",
        |op| {
            op.per(Per::Pages(200));
            op.iter(|| {
                let mut bounded = 0usize;
                for _ in 0..2 {
                    for line in unaccented_lines.lines() {
                        bounded += black_box(word_boundaries(&line.joined)).len();
                    }
                }
                black_box(bounded)
            });
        },
    );
    group.operation(
        "unaccented whole-word search, find and its restarts",
        |op| {
            op.per(Per::Pages(200));
            op.iter(|| {
                let mut found = 0usize;
                for _ in 0..2 {
                    for line in unaccented_lines.lines() {
                        let boundaries = Some(line.boundaries.as_slice());
                        found +=
                            find_hits(&line.joined, &unaccented_lines.needle, boundaries).len();
                    }
                }
                black_box(found)
            });
        },
    );
    group.operation(
        "unaccented whole-word search, mapping hits to glyphs",
        |op| {
            op.per(Per::Pages(200));
            op.iter(|| {
                let mut mapped = 0usize;
                for _ in 0..2 {
                    for line in unaccented_lines.lines() {
                        mapped += map_hits(line).len();
                    }
                }
                black_box(mapped)
            });
        },
    );

    // The same text through the CLI, one child a call: plain text with a
    // form feed after every page, and the structured text as JSON.
    group.operation("text of 5 dense pages, CLI", |op| {
        let dense_pdf: &Path = &dense_pdf;
        let dense_text = Cli::new("text-dense-5");
        let ran = dense_text.clone();
        op.per(Per::Pages(5));
        op.iter(move || dense_text.run(&[&"text", &dense_pdf]));
        op.after(move || {
            assert_eq!(ran.stdout().matches('\u{c}').count(), 5);
            assert_eq!(
                ran.stdout().matches(inputs::PHRASE).count(),
                5,
                "every dense page's phrase"
            );
        });
    });
    group.operation("text of 1000 small pages, CLI", |op| {
        let small_1000_pdf = inputs::store().file("small-1000.pdf", || small_1000.to_vec());
        let small_text = Cli::new("text-small-1000");
        let ran = small_text.clone();
        op.per(Per::Pages(2 * 1000));
        op.iter(move || {
            for _ in 0..2 {
                small_text.run(&[&"text", &small_1000_pdf]);
            }
        });
        op.after(move || assert_eq!(ran.stdout().matches('\u{c}').count(), 1000));
    });
    group.operation("text of 4000 small pages, CLI", |op| {
        let small_4000_pdf = inputs::store().file("small-4000.pdf", || small_4000.to_vec());
        let more_text = Cli::new("text-small-4000");
        let ran = more_text.clone();
        op.per(Per::Pages(4000));
        op.iter(move || more_text.run(&[&"text", &small_4000_pdf]));
        op.after(move || assert_eq!(ran.stdout().matches('\u{c}').count(), 4000));
    });
    group.operation("structured JSON of 5 pages, CLI", |op| {
        let dense_pdf: &Path = &dense_pdf;
        let json = Cli::new("text-json-5");
        let ran = json.clone();
        op.per(Per::Pages(5));
        op.iter(move || json.run(&[&"text", &dense_pdf, &"--json"]));
        op.after(move || {
            let written = ran.stdout();
            assert!(written.starts_with("{\"format\":\"tinker-pdf/text\""));
            assert_eq!(
                written.matches("{\"index\":").count(),
                5,
                "the JSON's pages"
            );
        });
    });

    group.finish();
}

/// `count` dense pages, their face asserted embedded.
fn dense_pages(count: u32) -> Arc<[u8]> {
    let dense = pages(
        inputs::store().bytes(&format!("dense-{count}.pdf"), || {
            inputs::dense(count as usize, 0)
        }),
        count,
    );
    let held = Document::open(Arc::clone(&dense)).expect("it opens");
    assert!(
        held.fonts()
            .iter()
            .any(|font| font.name == "LiberationSerif" && font.program.is_some()),
        "the dense pages embed their face"
    );
    dense
}

/// `small(count)`, which opens to its pages.
fn small_pages(count: u32) -> Arc<[u8]> {
    pages(
        inputs::store().bytes(&format!("small-{count}.pdf"), || {
            inputs::small(count as usize)
        }),
        count,
    )
}

/// `bytes`, which open to `count` pages.
fn pages(bytes: Vec<u8>, count: u32) -> Arc<[u8]> {
    let bytes: Arc<[u8]> = bytes.into();
    let doc = Document::open(Arc::clone(&bytes)).expect("it opens");
    assert_eq!(doc.page_count(), count, "the input's pages");
    bytes
}

/// `TextPage::search`'s fold: `str::to_lowercase`, which is lower-casing and
/// not Unicode case folding.
fn lowercase(text: &str) -> String {
    text.to_lowercase()
}

/// `TextPage::search_with`'s fold for a case- and diacritic-insensitive
/// search: lower-cased, then `fold_diacritics`.
fn unaccent(text: &str) -> String {
    fold_diacritics(&text.to_lowercase())
}

/// The first phase of a search of one page, as every call makes it: the
/// needle folded, then each line's glyph texts folded one by one, so that a
/// hit maps back to the glyphs it covers, and joined.
fn fold_and_join(text: &TextPage, needle: &str, fold: &impl Fn(&str) -> String) -> usize {
    let mut folded = black_box(fold(needle)).len();
    for line in text.lines() {
        let pieces: Vec<String> = line.chars.iter().map(|c| fold(&c.text)).collect();
        let joined: String = pieces.concat();
        folded += black_box(&pieces).len() + black_box(&joined).len();
    }
    folded
}

/// The third phase on one line: `str::find` from each restart. A hit
/// resumes the search past itself; where `boundaries` holds the line's UAX
/// #29 word boundaries, a candidate that does not begin and end on two of
/// them is no hit and resumes the search one character on, since it may
/// overlap one that is. The byte range of each hit.
fn find_hits(joined: &str, needle: &str, boundaries: Option<&[usize]>) -> Vec<(usize, usize)> {
    let mut hits = Vec::new();
    let mut from = 0usize;
    while let Some(found) = joined.get(from..).and_then(|s| s.find(needle)) {
        let start = from + found;
        let end = start + needle.len();
        let whole = boundaries.is_none_or(|boundaries| {
            boundaries.binary_search(&start).is_ok() && boundaries.binary_search(&end).is_ok()
        });
        if whole {
            hits.push((start, end));
            from = end;
        } else {
            from = start
                + joined
                    .get(start..)
                    .and_then(|s| s.chars().next())
                    .map_or(1, char::len_utf8);
        }
        if from >= joined.len() {
            break;
        }
    }
    hits
}

/// The last phase on one line: each hit's byte range walked back to the
/// glyphs it covers from the line's start, and the quad over them.
fn map_hits(line: &Searched<'_>) -> Vec<Quad> {
    let mut quads = Vec::new();
    for &(start, end) in &line.hits {
        let mut at = 0usize;
        let mut first = None;
        let mut last = None;
        for (i, piece) in line.pieces.iter().enumerate() {
            let next = at + piece.len();
            if first.is_none() && next > start {
                first = Some(i);
            }
            if at < end {
                last = Some(i);
            }
            at = next;
        }
        if let (Some(a), Some(b)) = (first, last) {
            if let Some(quad) = span_quad(line.line, a, b) {
                quads.push(quad);
            }
        }
    }
    quads
}

/// The quad over glyphs `a..=b` of `line`, as the content crate makes one
/// for a hit: its left edge from whichever of the two ends starts first along
/// the first's baseline, so a right-to-left line in logical order gives a
/// quad the right way out.
fn span_quad(line: &TextLine, a: usize, b: usize) -> Option<Quad> {
    let mut first = line.chars.get(a)?;
    let mut last = line.chars.get(b)?;
    let (ux, uy) = (
        first.quad.lr.0 - first.quad.ll.0,
        first.quad.lr.1 - first.quad.ll.1,
    );
    let along = |p: (f64, f64)| p.0 * ux + p.1 * uy;
    if along(last.quad.lr) < along(first.quad.ll) {
        std::mem::swap(&mut first, &mut last);
    }
    Some(Quad {
        ul: first.quad.ul,
        ll: first.quad.ll,
        ur: last.quad.ur,
        lr: last.quad.lr,
    })
}

/// One line of a prebuilt page as a search reads it: its glyph texts
/// folded, joined, the joined text's UAX #29 boundaries where the search is
/// whole-word, and the byte range of each hit.
struct Searched<'p> {
    line: &'p TextLine,
    pieces: Vec<String>,
    joined: String,
    boundaries: Vec<usize>,
    hits: Vec<(usize, usize)>,
}

/// Every line of the 100 pages as one search reads it, and its needle
/// folded.
struct Prepared<'p> {
    needle: String,
    pages: Vec<Vec<Searched<'p>>>,
}

impl<'p> Prepared<'p> {
    /// `pages` read for `needle` by the phases in turn, folding as `fold`
    /// does and holding hits to word boundaries where `whole_word` says, and
    /// asserted to find on every page exactly the quads `search` finds there:
    /// the phases are the search, split.
    fn of(
        pages: &'p [TextPage],
        needle: &str,
        fold: &impl Fn(&str) -> String,
        whole_word: bool,
        search: impl Fn(&TextPage) -> Vec<Quad>,
    ) -> Prepared<'p> {
        let needle = fold(needle);
        let prepared: Vec<Vec<Searched<'p>>> = pages
            .iter()
            .map(|text| {
                text.lines()
                    .into_iter()
                    .map(|line| {
                        let pieces: Vec<String> =
                            line.chars.iter().map(|c| fold(&c.text)).collect();
                        let joined = pieces.concat();
                        let boundaries = if whole_word {
                            word_boundaries(&joined)
                        } else {
                            Vec::new()
                        };
                        let hits =
                            find_hits(&joined, &needle, whole_word.then_some(&boundaries[..]));
                        Searched {
                            line,
                            pieces,
                            joined,
                            boundaries,
                            hits,
                        }
                    })
                    .collect()
            })
            .collect();
        for (index, (text, lines)) in pages.iter().zip(&prepared).enumerate() {
            let quads: Vec<Quad> = lines.iter().flat_map(map_hits).collect();
            assert!(!quads.is_empty(), "page {index} holds `{needle}`");
            assert_eq!(
                quads,
                search(text),
                "page {index}: the phases find the quads the search finds"
            );
        }
        Prepared {
            needle,
            pages: prepared,
        }
    }

    /// Every line of every page, in order.
    fn lines(&self) -> impl Iterator<Item = &Searched<'p>> {
        self.pages.iter().flatten()
    }
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
