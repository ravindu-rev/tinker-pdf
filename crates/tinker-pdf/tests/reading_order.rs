//! Reading order, named (`docs/design/reading-order.md`), and the scores that
//! design holds an inference to, measured before there is an inference.
//!
//! # The holdout set, and what it can stand for
//!
//! The design's adjudicator is the 1 078 tagged files of the fetched corpora,
//! scored with their trees hidden; that census is `reading_order_census.rs`,
//! and it runs where the corpora are. What runs everywhere is this:
//!
//! - **Pages built here with the tagging API**, each drawn in one order and
//!   tagged in another, so the content stream says one thing and the
//!   structure tree — the answer key — another. Every multi-column fixture
//!   asserts that the stream's own order **falls short** of the tree, so a
//!   later pass at the floor is evidence the inference added something rather
//!   than that the fixture was easy; and every one-column fixture asserts that
//!   the stream already reads it right, which is the design's set where
//!   nothing may move.
//! - **The committed EPUB books**, converted by this engine into tagged PDF
//!   whose tree `epub_structure.rs` holds to the XHTML source — the author's
//!   statement of the order, which this engine did not choose.
//!
//! What it cannot show is stated rather than absorbed: the layout engine that
//! placed an EPUB's columns and the inference that finds them share an author,
//! and a builder fixture's answer key is this repository's. These are
//! arithmetic fixtures with known answers — the design's "every threshold has
//! an arithmetic fixture beside it" — not a measurement of the world.

mod reading_order_support;

use reading_order_support::{pair_agreement, Agreement, Key, Scored};
use tinker_pdf::{
    DeclineReason, Document, InferenceOptions, InferenceWarning, OrderedText, ReadingOrder, Role,
};
use tinker_pdf_cos::build::DocumentBuilder;

// ---- fixtures ---------------------------------------------------------------

/// One `text` call: where, how big, what.
#[derive(Clone, Debug)]
struct Run {
    x: f64,
    y: f64,
    size: f64,
    text: String,
}

fn run(x: f64, y: f64, size: f64, text: impl Into<String>) -> Run {
    Run {
        x,
        y,
        size,
        text: text.into(),
    }
}

const WORDS: &[&str] = &[
    "alpha", "bravo", "charlie", "delta", "echo", "foxtrot", "golf", "hotel", "india", "juliett",
    "kilo", "lima", "mike", "november", "oscar", "papa", "quebec", "romeo", "sierra", "tango",
    "uniform", "victor", "whiskey", "xray", "yankee", "zulu",
];

/// A line of words, deterministic in `seed`, at least `length` characters.
fn prose(seed: usize, length: usize) -> String {
    let mut out = String::new();
    let mut at = seed.wrapping_mul(7);
    while out.len() < length {
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(WORDS[at % WORDS.len()]);
        at = at.wrapping_mul(31).wrapping_add(17);
    }
    out
}

/// A one-page document, `width` by `height`, drawing `runs` in the order
/// `draw` names them. With `read`, each run is its own `/P`, read at the
/// position `read` gives it.
fn page(width: f64, height: f64, runs: &[Run], draw: &[usize], read: Option<&[u64]>) -> Vec<u8> {
    let mut builder = DocumentBuilder::new();
    builder.add_base_font(b"F1", b"Helvetica");
    builder.add_page(width, height, |page| {
        for &at in draw {
            let r = &runs[at];
            match read {
                Some(read) => page.tagged_keyed(b"P", at as u64 + 1, read[at], |p| {
                    p.text(b"F1", r.size, r.x, r.y, &r.text);
                }),
                None => page.text(b"F1", r.size, r.x, r.y, &r.text),
            }
        }
    });
    builder.finish()
}

/// A US letter page with two columns of `rows` lines each, at 10 points:
/// column one at x = 72, column two at x = 324, paragraphs of five lines
/// whose last line is short.
fn two_columns(rows: usize) -> Vec<Run> {
    let mut runs = Vec::new();
    for column in 0..2 {
        let x = if column == 0 { 72.0 } else { 324.0 };
        let mut y = 720.0;
        for row in 0..rows {
            let last = row % 5 == 4;
            let length = if last { 18 } else { 40 };
            runs.push(run(x, y, 10.0, prose(column * 1000 + row, length)));
            y -= if last { 22.0 } else { 12.0 };
        }
    }
    runs
}

/// Every index of `runs`, row by row across the columns — the order a
/// producer that writes line by line across the page draws in.
fn across(rows: usize, columns: usize) -> Vec<usize> {
    let mut out = Vec::new();
    for row in 0..rows {
        for column in 0..columns {
            out.push(column * rows + row);
        }
    }
    out
}

fn identity(n: usize) -> Vec<u64> {
    (0..n as u64).collect()
}

fn hidden() -> InferenceOptions {
    InferenceOptions {
        hide_structure: true,
    }
}

fn open(bytes: Vec<u8>) -> Document {
    Document::open(bytes).expect("the fixture opens")
}

// ---- the label, and the default that does not move ---------------------------

/// **The default is untouched**: `Page::text`, and `text_in(Stream)`, are the
/// stream's order to the byte on every committed document, and `Stated` is
/// the structured view's.
#[test]
fn the_stream_and_stated_orders_are_what_already_existed() {
    for (name, doc) in committed() {
        for page in doc.pages() {
            let text = page.text().plain_text();
            let stream = page.text_in(ReadingOrder::Stream).expect("always");
            assert_eq!(stream.order(), ReadingOrder::Stream);
            assert_eq!(stream.plain_text(), text, "{name}");
            match (page.text_in(ReadingOrder::Stated), page.structured_text()) {
                (Some(stated), Some(view)) => {
                    assert_eq!(stated.order(), ReadingOrder::Stated);
                    assert_eq!(stated.plain_text(), view.plain_text(), "{name}");
                }
                (None, None) => {}
                _ => panic!("{name}: Stated and structured_text disagree about the tree"),
            }
        }
    }
}

/// **The inference reads the page `Page::text` reads**, character for
/// character: the observer is a tee into the same text device, and this is
/// the assertion that it is.
///
/// For every page of every committed untagged document, every character of
/// the inferred order is the character of `Page::text` its permutation
/// names — same text, same quad, same origin — and the permutation is one.
#[test]
fn the_inference_reads_the_same_page_text_reads() {
    let mut pages = 0usize;
    for (name, doc) in committed() {
        if doc.structure().is_some() {
            continue;
        }
        for page in doc.pages() {
            let text = page.text();
            let stream: Vec<_> = text
                .lines()
                .into_iter()
                .flat_map(|l| l.chars.iter())
                .collect();
            let order = page.inferred_order(&InferenceOptions::default());
            assert_permutation(&order.permutation, stream.len(), &name);
            for (c, from) in order.chars().into_iter().zip(&order.permutation) {
                let s = stream[*from];
                assert_eq!(
                    (&c.text, c.quad, c.origin),
                    (&s.text, s.quad, s.origin),
                    "{name}"
                );
            }
            pages += 1;
        }
    }
    assert!(pages > 0, "no untagged page was compared");
}

fn assert_permutation(permutation: &[usize], n: usize, name: &str) {
    assert_eq!(permutation.len(), n, "{name}: the permutation is not total");
    let mut seen = vec![false; n];
    for at in permutation {
        assert!(!seen[*at], "{name}: {at} appears twice");
        seen[*at] = true;
    }
}

/// **A guess is never preferred to a statement.** On a tagged page,
/// `text_in(Inferred)` is the tree's order, labelled `Stated`, and
/// `inferred_order` declines with the stream's blocks unmoved; only
/// `hide_structure` reads past the tree, and says the tree was there.
#[test]
fn a_tagged_page_answers_with_its_tree_unless_the_tree_is_hidden() {
    let runs = two_columns(10);
    let read = identity(runs.len());
    let doc = open(page(612.0, 792.0, &runs, &across(10, 2), Some(&read)));
    let page = doc.page(0).expect("a page");

    let asked = page.text_in(ReadingOrder::Inferred).expect("an answer");
    assert_eq!(
        asked.order(),
        ReadingOrder::Stated,
        "a guess replaced a statement"
    );
    assert!(matches!(asked, OrderedText::Stated(_)));

    let declined = page.inferred_order(&InferenceOptions::default());
    assert_eq!(declined.declined(), Some(DeclineReason::TreePresent));
    assert_eq!(declined.moved(), 0, "a declined inference moved something");
    assert!(declined.blocks.iter().all(|b| b.role == Role::Unplaced));
    assert_eq!(declined.plain_text(), page.text().plain_text());

    let measured = page.inferred_order(&hidden());
    assert!(measured.warnings.contains(&InferenceWarning::TreePresent));
    assert_eq!(measured.declined(), Some(DeclineReason::NotImplemented));
}

/// **Until the inference lands it declines by name**, on an untagged page,
/// with the stream's blocks unmoved and every one `Unplaced` — a placeholder
/// that cannot be mistaken for an answer.
#[test]
fn an_untagged_page_declines_until_there_is_an_inference() {
    let runs = two_columns(10);
    let doc = open(page(612.0, 792.0, &runs, &across(10, 2), None));
    let page = doc.page(0).expect("a page");
    let order = page.inferred_order(&InferenceOptions::default());
    assert_eq!(order.declined(), Some(DeclineReason::NotImplemented));
    assert_eq!(order.moved(), 0);
    assert!(order.blocks.iter().all(|b| b.role == Role::Unplaced));
    assert_eq!(order.plain_text(), page.text().plain_text());
    let asked = page.text_in(ReadingOrder::Inferred).expect("an answer");
    assert_eq!(asked.order(), ReadingOrder::Inferred);
    assert!(
        page.text_in(ReadingOrder::Stated).is_none(),
        "an untagged page has no tree"
    );
}

#[test]
fn a_page_with_no_text_says_so() {
    let doc = open(page(300.0, 300.0, &[], &[], None));
    let order = doc
        .page(0)
        .expect("a page")
        .inferred_order(&InferenceOptions::default());
    assert!(order.blocks.is_empty());
    assert_eq!(order.warnings, vec![InferenceWarning::NoBodyText]);
}

// ---- the instrument -----------------------------------------------------------

/// **The pair score counts inversions**, checked against arithmetic: the
/// identity agrees on all `n(n-1)/2` pairs, the reversal on none, one swap of
/// neighbours costs one pair, and a character the answer does not hold is not
/// a pair at all.
#[test]
fn the_pair_score_counts_inversions() {
    let keys: Vec<Key> = (0..40u64).map(|i| (i, 0, "x".to_string())).collect();
    let n = keys.len() as u64;
    let all = n * (n - 1) / 2;
    assert_eq!(
        pair_agreement(&keys, &keys),
        Agreement {
            pairs: all,
            agreeing: all
        }
    );
    let reversed: Vec<Key> = keys.iter().rev().cloned().collect();
    assert_eq!(pair_agreement(&keys, &reversed).agreeing, 0);
    let mut swapped = keys.clone();
    swapped.swap(10, 11);
    assert_eq!(pair_agreement(&keys, &swapped).agreeing, all - 1);
    let mut extra = keys.clone();
    extra.push((999, 0, "y".to_string()));
    assert_eq!(pair_agreement(&keys, &extra).pairs, all);
    // A repeated key is told apart by occurrence, so a fake bold is two
    // characters and not one.
    let doubled = vec![keys[0].clone(), keys[0].clone(), keys[1].clone()];
    assert_eq!(pair_agreement(&doubled, &doubled).pairs, 3);
}

/// **The baseline, before the inference**: the content stream's order scored
/// against the tree on the headline builder fixture. A multi-column fixture
/// is fit for purpose only if the stream falls short of the tree, and a
/// one-column one only if it does not, so both are asserted here, where
/// nothing else is being measured.
#[test]
fn the_streams_baseline_on_the_fixtures_is_measured_first() {
    let rows = 30;
    let runs = two_columns(rows);
    let read = identity(runs.len());
    let interleaved = open(page(612.0, 792.0, &runs, &across(rows, 2), Some(&read)));
    let scored = Scored::read(&interleaved, 0).expect("a tagged page");
    let stream = scored.stream_agreement();
    println!(
        "two interleaved columns: stream {:.4} ({} of {} pairs)",
        stream.score(),
        stream.agreeing,
        stream.pairs
    );
    assert!(stream.pairs > 0);
    assert!(
        !stream.at_least(9, 10),
        "the stream scores {}: too near agreement for the fixture to test anything",
        stream.score()
    );
    // Declined, the inference is the stream exactly.
    assert_eq!(scored.inferred_agreement(), stream);

    let draw: Vec<usize> = (0..runs.len()).collect();
    let in_order = open(page(612.0, 792.0, &runs, &draw, Some(&read)));
    let scored = Scored::read(&in_order, 0).expect("a tagged page");
    assert!(scored.stream_agreement().at_least(1, 1));
}

/// **The score sees a page drawn out of order.** One column, its lines drawn
/// in a seeded shuffle and tagged in reading order: the stream's agreement
/// falls from 1 to well below it, which is the measurement the census rests
/// on — an instrument that read a shuffled page as agreeing would make every
/// later number meaningless.
#[test]
fn the_score_sees_a_shuffled_page() {
    let runs: Vec<Run> = (0..40)
        .map(|row| run(72.0, 740.0 - row as f64 * 12.0, 10.0, prose(row, 70)))
        .collect();
    let read = identity(runs.len());
    let mut draw: Vec<usize> = (0..runs.len()).collect();
    // A fixed linear-congruential shuffle: the same order on every run.
    let mut state = 0x2545_f491_u64;
    for i in (1..draw.len()).rev() {
        state = state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1);
        let j = (state >> 33) as usize % (i + 1);
        draw.swap(i, j);
    }
    let shuffled = open(page(612.0, 792.0, &runs, &draw, Some(&read)));
    let stream = Scored::read(&shuffled, 0)
        .expect("tagged")
        .stream_agreement();
    println!("shuffled column: stream {:.4}", stream.score());
    assert!(!stream.at_least(3, 4), "stream {}", stream.score());
    let in_order: Vec<usize> = (0..runs.len()).collect();
    let ordered = open(page(612.0, 792.0, &runs, &in_order, Some(&read)));
    assert!(Scored::read(&ordered, 0)
        .expect("tagged")
        .stream_agreement()
        .at_least(1, 1));
}

// ---- the EPUB books ------------------------------------------------------------

/// **Every committed book, its stream scored against its own tree.** The tree
/// is the book's order, written by this engine from the XHTML and held to it
/// by `epub_structure.rs`; the stream is the order this engine's layout drew.
///
/// Measured when this landed: every book agrees exactly, because the EPUB
/// writer draws a page in the order it tags it. The books are therefore in
/// the set where nothing may move, and that is asserted as the floor the
/// inference will be held to.
#[test]
fn the_committed_books_stream_in_their_own_order() {
    let mut total = Agreement::default();
    for (name, doc) in committed() {
        if !name.ends_with(".epub") || doc.structure().is_none() {
            continue;
        }
        let mut stream = Agreement::default();
        for index in 0..doc.page_count() {
            let Some(scored) = Scored::read(&doc, index) else {
                continue;
            };
            stream = stream.plus(scored.stream_agreement());
        }
        println!(
            "{name:<28} stream {:.6} ({} pairs)",
            stream.score(),
            stream.pairs
        );
        assert!(
            stream.at_least(1, 1),
            "{name}: the stream disagrees with the tree"
        );
        total = total.plus(stream);
    }
    println!(
        "all books: stream {:.6} ({} pairs)",
        total.score(),
        total.pairs
    );
    assert!(total.pairs > 0, "no book was scored");
}

// ---- helpers ----------------------------------------------------------------

/// Every committed document: `testdata/`'s PDFs that open without a password,
/// and the nine EPUB books.
fn committed() -> Vec<(String, Document)> {
    let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../../testdata");
    let books = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/epub");
    let mut out = Vec::new();
    for dir in [root, books] {
        let mut entries: Vec<_> = std::fs::read_dir(dir)
            .expect("the committed fixtures are present")
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| matches!(p.extension().and_then(|e| e.to_str()), Some("pdf" | "epub")))
            .collect();
        entries.sort();
        for path in entries {
            let bytes = std::fs::read(&path).expect("readable");
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            if let Ok(doc) = Document::open(bytes) {
                out.push((name, doc));
            }
        }
    }
    out
}
