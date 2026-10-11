//! Inferred reading order (`docs/design/reading-order.md`), held by the
//! scores that design defines, over a first-party holdout set.
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
//!   asserts a pair-agreement floor that the stream's own order **does not
//!   meet**, so a pass is evidence the inference added something rather than
//!   that the fixture was easy; and every one-column fixture asserts that
//!   nothing moved, which is the design's set where nothing may move.
//! - **The committed EPUB books**, converted by this engine into tagged PDF
//!   whose tree `epub_structure.rs` holds to the XHTML source — the author's
//!   statement of the order, which this engine did not choose.
//!
//! What it cannot show is stated rather than absorbed: the layout engine that
//! placed an EPUB's columns and the inference that finds them share an author,
//! and a builder fixture's answer key is this repository's. These are
//! arithmetic fixtures with known answers — the design's "every threshold has
//! an arithmetic fixture beside it" — not a measurement of the world.

mod epub_support;
mod reading_order_support;

use epub_support::book::styled_book;
use reading_order_support::{pair_agreement, Agreement, Key, Scored};
use tinker_pdf::reading_order::{COLUMN_GAP_EMS, MIN_COLUMN_LINES};
use tinker_pdf::{
    DeclineReason, Document, InferenceOptions, InferenceWarning, OrderedText, ReadingOrder, Role,
    TableEvidence, TableOptions,
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

/// The fixture's text in the order the answer reads it.
fn read_text(runs: &[Run], read: &[u64]) -> String {
    let mut order: Vec<usize> = (0..runs.len()).collect();
    order.sort_by_key(|at| read[*at]);
    order
        .iter()
        .map(|at| format!("{}\n", runs[*at].text))
        .collect()
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
/// `hide_structure` infers, and says the tree was there.
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
    assert_eq!(measured.declined(), None);
    assert!(measured.warnings.contains(&InferenceWarning::TreePresent));
    assert_eq!(measured.columns, 2);
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

// ---- columns ----------------------------------------------------------------

/// **The headline fixture.** Two columns drawn line by line across the page,
/// tagged column by column: the stream interleaves them and the tree does not.
///
/// The stream's pair agreement with the tree is printed and asserted to fall
/// short of the floor; the inference's, with the tree hidden, is asserted at
/// it — every pair the right way round — and walking the tree crosses no
/// column the inference found.
#[test]
fn two_interleaved_columns_read_down_one_and_then_the_other() {
    let rows = 30;
    let runs = two_columns(rows);
    let read = identity(runs.len());
    let doc = open(page(612.0, 792.0, &runs, &across(rows, 2), Some(&read)));
    let scored = Scored::read(&doc, 0).expect("a tagged page");

    let stream = scored.stream_agreement();
    let inferred = scored.inferred_agreement();
    println!(
        "two interleaved columns: stream {:.4} ({} of {} pairs), inferred {:.4}",
        stream.score(),
        stream.agreeing,
        stream.pairs,
        inferred.score()
    );
    assert!(stream.pairs > 0 && stream.pairs == inferred.pairs);
    assert!(
        !stream.at_least(1, 1),
        "the stream already reads the fixture right, so it proves nothing"
    );
    assert!(
        stream.score() < 0.9,
        "the stream scores {}, too near the floor to be a test",
        stream.score()
    );
    assert!(
        inferred.at_least(1, 1),
        "the inference scored {} against the tree",
        inferred.score()
    );
    assert_eq!(scored.inferred.columns, 2);
    assert_eq!(scored.crossings(), 0);
    // And the same page untagged reads the same, with no tree to hide.
    let untagged = open(page(612.0, 792.0, &runs, &across(rows, 2), None));
    let order = untagged
        .page(0)
        .expect("a page")
        .inferred_order(&InferenceOptions::default());
    assert_eq!(order.plain_text(), read_text(&runs, &read));
    assert!(order.warnings.is_empty(), "{:?}", order.warnings);
}

/// **Where the stream was already right, nothing moves**: two columns drawn
/// column by column.
#[test]
fn columns_drawn_in_order_are_left_in_order() {
    let runs = two_columns(30);
    let draw: Vec<usize> = (0..runs.len()).collect();
    let doc = open(page(612.0, 792.0, &runs, &draw, None));
    let order = doc
        .page(0)
        .expect("a page")
        .inferred_order(&InferenceOptions::default());
    assert_eq!(order.columns, 2);
    assert_eq!(order.moved(), 0, "{}", order.plain_text());
    assert!(order
        .blocks
        .iter()
        .all(|b| b.role == Role::Body && b.column.is_some()));
}

/// **A heading across both columns reads before them, and a paragraph across
/// both in the middle of the page between the columns above it and the
/// columns below** (`css-multicol-1` §6's `column-span: all`, drawn by hand).
/// The stream draws the lower set first, the spanning paragraph last and the
/// heading in the middle.
#[test]
fn a_spanning_block_is_read_between_the_column_sets_it_divides() {
    // At sixteen points the heading runs from 72 to about 420, well across
    // the gap at 280 to 324.
    let mut runs = vec![run(
        72.0,
        740.0,
        16.0,
        "A Heading Set Across Both The Columns",
    )];
    // Upper set: two columns of eight lines.
    for column in 0..2 {
        let x = if column == 0 { 72.0 } else { 324.0 };
        for row in 0..8 {
            runs.push(run(
                x,
                700.0 - row as f64 * 12.0,
                10.0,
                prose(column * 100 + row, 40),
            ));
        }
    }
    // The spanning paragraph, two lines across the whole measure.
    runs.push(run(72.0, 590.0, 10.0, prose(500, 96)));
    runs.push(run(72.0, 578.0, 10.0, prose(501, 60)));
    // Lower set: two columns of eight lines.
    for column in 0..2 {
        let x = if column == 0 { 72.0 } else { 324.0 };
        for row in 0..8 {
            runs.push(run(
                x,
                550.0 - row as f64 * 12.0,
                10.0,
                prose(column * 100 + 50 + row, 40),
            ));
        }
    }
    let read = identity(runs.len());
    // Lower set first, then the heading, then the upper set, then the span.
    let mut draw: Vec<usize> = (19..runs.len()).collect();
    draw.push(0);
    draw.extend(1..17);
    draw.extend([17, 18]);
    let doc = open(page(612.0, 792.0, &runs, &draw, Some(&read)));
    let scored = Scored::read(&doc, 0).expect("tagged");
    let stream = scored.stream_agreement();
    let inferred = scored.inferred_agreement();
    println!(
        "spanning blocks: stream {:.4}, inferred {:.4}",
        stream.score(),
        inferred.score()
    );
    assert!(!stream.at_least(1, 1));
    assert!(inferred.at_least(1, 1), "inferred {}", inferred.score());
    assert_eq!(scored.inferred.columns, 2);
    assert_eq!(scored.crossings(), 0);
    let spanners: Vec<_> = scored
        .inferred
        .blocks
        .iter()
        .filter(|b| b.column.is_none())
        .collect();
    assert_eq!(spanners.len(), 2, "the heading and the spanning paragraph");
}

/// Three columns are two gaps, found in one pass.
#[test]
fn three_columns_read_left_to_right() {
    let rows = 20;
    let mut runs = Vec::new();
    for column in 0..3 {
        let x = 54.0 + column as f64 * 176.0;
        for row in 0..rows {
            runs.push(run(
                x,
                720.0 - row as f64 * 12.0,
                10.0,
                prose(column * 1000 + row, 26),
            ));
        }
    }
    let read = identity(runs.len());
    let doc = open(page(612.0, 792.0, &runs, &across(rows, 3), Some(&read)));
    let scored = Scored::read(&doc, 0).expect("tagged");
    assert!(!scored.stream_agreement().at_least(1, 1));
    assert!(scored.inferred_agreement().at_least(1, 1));
    assert_eq!(scored.inferred.columns, 3);
    assert_eq!(scored.crossings(), 0);
}

/// The column rule is in ems: the same two-column page at 7 and at 14 points,
/// its gap a fixed one and a half ems, is two columns at both sizes; and a gap
/// of three quarters of an em — below [`COLUMN_GAP_EMS`] and within a factor
/// of two of it — is one column at both, with the near miss named.
#[test]
fn the_column_gap_is_measured_in_ems_of_the_page() {
    for size in [7.0, 14.0] {
        for (gap_ems, columns) in [(1.5, 2usize), (0.75, 1usize)] {
            assert!((gap_ems >= COLUMN_GAP_EMS) == (columns == 2));
            let rows = 20;
            // Each line is exactly `width` points of `x`: one character per
            // half em, so the gap is what the arithmetic says.
            let chars = 30usize;
            let width = chars as f64 * size * 0.5; // Helvetica's `x` is 500 units
            let mut runs = Vec::new();
            for column in 0..2 {
                let x = 40.0 + column as f64 * (width + gap_ems * size);
                for row in 0..rows {
                    runs.push(run(
                        x,
                        740.0 - row as f64 * size * 1.2,
                        size,
                        "x".repeat(chars),
                    ));
                }
            }
            let doc = open(page(1400.0, 792.0, &runs, &across(rows, 2), None));
            let order = doc
                .page(0)
                .expect("a page")
                .inferred_order(&InferenceOptions::default());
            assert_eq!(order.columns, columns, "{size} pt, {gap_ems} em");
            if columns == 1 {
                assert!(
                    order
                        .warnings
                        .iter()
                        .any(|w| matches!(w, InferenceWarning::ColumnsAmbiguous { .. })),
                    "{size} pt: the near miss was not named: {:?}",
                    order.warnings
                );
                assert_eq!(order.moved(), 0, "one column, drawn in order, moved");
            }
        }
    }
}

/// **Labels beside their values are not two columns.** Short labels at the
/// left, each value on the label's baseline: the whitespace between them is a
/// gap by width and height, and reading every label before any value is the
/// one order nobody wants. The labels are a column narrower than
/// [`tinker_pdf::reading_order::COLUMN_MIN_WIDTH_EMS`], so the page is one
/// column and the near miss is named.
#[test]
fn labels_beside_their_values_are_one_column() {
    let labels = [
        "Name",
        "Address",
        "City",
        "Telephone",
        "Account",
        "Reference",
        "Date",
        "Signed",
    ];
    let mut runs = Vec::new();
    let mut draw = Vec::new();
    for (row, label) in labels.iter().enumerate() {
        let y = 700.0 - row as f64 * 14.0;
        draw.push(runs.len());
        runs.push(run(72.0, y, 10.0, *label));
        draw.push(runs.len());
        runs.push(run(200.0, y, 10.0, prose(row, 50)));
    }
    let doc = open(page(612.0, 792.0, &runs, &draw, None));
    let order = doc
        .page(0)
        .expect("a page")
        .inferred_order(&InferenceOptions::default());
    assert_eq!(order.columns, 1, "{}", order.plain_text());
    assert_eq!(order.moved(), 0);
    assert!(order
        .warnings
        .iter()
        .any(|w| matches!(w, InferenceWarning::ColumnsAmbiguous { .. })));
    // The table inference finds an aligned table here; nothing the page drew
    // bounds it, so it is not handed off, and the lines are read as lines.
    let tables = doc
        .page(0)
        .expect("a page")
        .inferred_tables(&TableOptions::default());
    assert_eq!(tables.tables.len(), 1);
    assert_eq!(tables.tables[0].evidence, TableEvidence::Aligned);
    assert!(!order
        .warnings
        .iter()
        .any(|w| matches!(w, InferenceWarning::TableSuspected { .. })));
}

/// **Right-to-left columns read right to left.** Two columns of Hebrew,
/// drawn left column first; the tree reads the right column first, as a
/// Hebrew page is read, and so does the inference, because most of the
/// page's lines are right-to-left.
#[test]
fn right_to_left_columns_read_from_the_right() {
    use epub_support::typeface::Face;
    use tinker_pdf_cos::build::{Glyph, PlacedGlyph};
    const LETTERS: &str = "\u{5D0}\u{5D1}\u{5D2}\u{5D3}\u{5D4}\u{5D5}\u{5D6}\u{5D7}\u{5D8}\u{5D9}";
    let face = Face::new("Fixture Hebrew", &format!("{LETTERS} "));
    let program = face.build();
    let mut builder = DocumentBuilder::new();
    assert!(builder.add_cid_font(b"F0", b"FixtureHebrew", &program));
    let letters: Vec<char> = LETTERS.chars().collect();
    let rows = 12usize;
    // The face's 500-unit advance at ten points.
    let (size, advance) = (10.0, 5.0);
    // (x of the column's left edge, its position in reading order): the
    // right column is read first.
    let columns = [(60.0, 1u64), (330.0, 0u64)];
    let mut lines: Vec<(Vec<u8>, u64)> = Vec::new();
    for (left, reading) in columns {
        for row in 0..rows {
            // Forty glyphs in visual order, a space every fifth.
            let texts: Vec<String> = (0..40)
                .map(|i| {
                    if i % 5 == 4 {
                        " ".to_string()
                    } else {
                        letters[(row * 3 + i + reading as usize) % letters.len()].to_string()
                    }
                })
                .collect();
            let glyphs: Vec<PlacedGlyph<'_>> = texts
                .iter()
                .enumerate()
                .map(|(i, text)| PlacedGlyph {
                    glyph: Glyph {
                        id: face
                            .glyph_of(text.chars().next().expect("one character"))
                            .expect("the face covers the page"),
                        text,
                    },
                    x: i as f64 * advance,
                    rise: 0.0,
                })
                .collect();
            let mut content = Vec::new();
            let y = 720.0 - row as f64 * 12.0;
            assert!(builder.glyph_run(
                &mut content,
                b"F0",
                size,
                [1.0, 0.0, 0.0, 1.0, left, y],
                &glyphs
            ));
            lines.push((content, reading * rows as u64 + row as u64));
        }
    }
    builder.add_page(612.0, 792.0, |page| {
        for (content, order) in &lines {
            page.tagged_keyed(b"P", order + 1, *order, |p| p.raw(content));
        }
    });
    let doc = open(builder.finish());
    let scored = Scored::read(&doc, 0).expect("tagged");
    let stream = scored.stream_agreement();
    println!("right-to-left columns: stream {:.4}", stream.score());
    assert!(!stream.at_least(1, 1), "the stream already reads it right");
    assert!(
        scored.inferred_agreement().at_least(1, 1),
        "inferred {}",
        scored.inferred_agreement().score()
    );
    assert_eq!(scored.inferred.columns, 2);
    // Column 0 is the one read first, which is the right-hand one.
    let first = scored.inferred.blocks.first().expect("blocks");
    assert_eq!(first.column, Some(0));
    assert!(
        first.quad.ll.0 > 300.0,
        "the first block read is not the right column"
    );
}

// ---- running heads, running feet and page numbers ------------------------------

/// One page of a book with page furniture: a running head (`head`, at the
/// top, left or right), the page's number centred at the foot, and between
/// them two columns of body text drawn line by line across both, tagged
/// column by column. The furniture is drawn as `/Artifact /Pagination`
/// (14.8.2.2), which is what a producer that marks it writes; the stream
/// draws the number first, the body next and the head last.
fn furnished_page(
    builder: &mut DocumentBuilder,
    head: &str,
    head_right: bool,
    number: &str,
    seed: usize,
) {
    let rows = 20;
    let runs = two_columns(rows)
        .into_iter()
        .enumerate()
        .map(|(at, r)| Run {
            text: prose(seed * 1000 + at, r.text.len()),
            ..r
        })
        .collect::<Vec<_>>();
    let draw = across(rows, 2);
    builder.add_page(612.0, 792.0, |page| {
        let artifact = |page: &mut tinker_pdf_cos::build::PageBuilder,
                        subtype: &str,
                        x: f64,
                        y: f64,
                        text: &str| {
            page.raw(
                format!("/Artifact <</Type /Pagination /Subtype /{subtype}>> BDC\n").as_bytes(),
            );
            page.text(b"F1", 9.0, x, y, text);
            page.raw(b"EMC\n");
        };
        artifact(page, "Footer", 300.0, 40.0, number);
        for &at in &draw {
            let r = &runs[at];
            page.tagged_keyed(b"P", at as u64 + 1, at as u64, |p| {
                p.text(b"F1", r.size, r.x, r.y, &r.text);
            });
        }
        let x = if head_right { 420.0 } else { 72.0 };
        artifact(page, "Header", x, 760.0, head);
    });
}

/// A six-page book with a verso head, a recto head and page numbers.
fn furnished_book(pages: usize) -> Document {
    let mut builder = DocumentBuilder::new();
    builder.add_base_font(b"F1", b"Helvetica");
    for at in 0..pages {
        let (head, right) = if at % 2 == 0 {
            ("A Treatise On Columns", false)
        } else {
            ("Chapter Two: The Gap", true)
        };
        furnished_page(&mut builder, head, right, &(at + 1).to_string(), at);
    }
    open(builder.finish())
}

/// **Running heads and page numbers are found by their recurring, and set
/// aside first and last.** Six pages, a verso head and a recto head each
/// recurring on every other page, a page number at each foot counting up,
/// and two columns of body between them drawn across the page. The truth is
/// the producer's own `/Artifact /Pagination` marks, read with the tree
/// hidden; the stream's order calls nothing furniture, so its recall is zero
/// and the inference's is asserted at one, with precision one.
#[test]
fn running_heads_and_page_numbers_are_found_by_recurring_and_set_aside() {
    let doc = furnished_book(6);
    let furniture = [Role::RunningHead, Role::RunningFoot, Role::PageNumber];
    let mut total = reading_order_support::RoleScore::default();
    for index in 0..doc.page_count() {
        let scored = Scored::read(&doc, index).expect("tagged");
        let truth = reading_order_support::artifact_keys(&doc, index);
        assert!(
            !truth.is_empty(),
            "page {index}: the fixture marks its furniture"
        );
        let score = reading_order_support::role_score(&scored.inferred, &furniture, &truth);
        total = total.plus(score);

        let blocks = &scored.inferred.blocks;
        let first = blocks.first().expect("blocks");
        let last = blocks.last().expect("blocks");
        assert_eq!(first.role, Role::RunningHead, "page {index}");
        assert_eq!(last.role, Role::PageNumber, "page {index}");
        assert_eq!(last.lines[0].text, (index + 1).to_string());
        assert!(blocks[1..blocks.len() - 1]
            .iter()
            .all(|b| b.role == Role::Body));
        // The body between them reads down its columns.
        assert!(scored.inferred_agreement().at_least(1, 1), "page {index}");
        assert!(!scored.stream_agreement().at_least(9, 10), "page {index}");
        assert_eq!(scored.inferred.columns, 2);
    }
    println!(
        "furniture: called {} correct {}, truth {} chars found {}",
        total.called, total.correct, total.truth, total.found
    );
    assert!(total.precision_at_least(1, 1), "{total:?}");
    assert!(total.recall_at_least(1, 1), "{total:?}");
}

/// **One page has nothing to compare**: its margin blocks are `Unplaced`
/// where they stand, with the warning that says why, and no block is called a
/// running head or a page number on no evidence.
#[test]
fn a_single_page_has_no_running_heads_only_unplaced_margins() {
    let doc = furnished_book(1);
    let order = doc.inferred_order(0, &hidden()).expect("a page");
    assert!(order.blocks.iter().all(|b| !matches!(
        b.role,
        Role::RunningHead | Role::RunningFoot | Role::PageNumber
    )));
    let unplaced: Vec<&str> = order
        .blocks
        .iter()
        .filter(|b| b.role == Role::Unplaced)
        .flat_map(|b| b.lines.iter().map(|l| l.text.as_str()))
        .collect();
    assert_eq!(
        unplaced,
        ["A Treatise On Columns", "1"],
        "{}",
        order.plain_text()
    );
    assert!(order
        .warnings
        .contains(&InferenceWarning::NoCrossPageEvidence {
            pages: 0,
            blocks: 2
        }));
}

/// **A page number needs one neighbour, a running head two.** Two pages: each
/// number is confirmed by the other's, one more than or one less than its
/// own, but each head has one recurrence where two are asked, so it is left
/// `Unplaced` and named.
#[test]
fn two_pages_confirm_their_numbers_and_not_their_heads() {
    let mut builder = DocumentBuilder::new();
    builder.add_base_font(b"F1", b"Helvetica");
    for at in 0..2 {
        furnished_page(
            &mut builder,
            "A Treatise On Columns",
            false,
            &(at + 7).to_string(),
            at,
        );
    }
    let doc = open(builder.finish());
    for index in 0..2 {
        let order = doc.inferred_order(index, &hidden()).expect("a page");
        let last = order.blocks.last().expect("blocks");
        assert_eq!(last.role, Role::PageNumber, "page {index}");
        let head = order
            .blocks
            .iter()
            .find(|b| b.lines.iter().any(|l| l.text == "A Treatise On Columns"))
            .expect("the head");
        assert_eq!(head.role, Role::Unplaced, "page {index}");
        assert!(order
            .warnings
            .contains(&InferenceWarning::NoCrossPageEvidence {
                pages: 1,
                blocks: 1
            }));
    }
}

/// **A margin line that recurs nowhere is body**, on a document with pages
/// to compare: a chapter's title at the top of its first page is read where
/// it stands, and so is a numeral that does not count.
#[test]
fn a_margin_line_that_does_not_recur_is_body() {
    let mut builder = DocumentBuilder::new();
    builder.add_base_font(b"F1", b"Helvetica");
    for at in 0..5 {
        builder.add_page(612.0, 792.0, |page| {
            if at == 2 {
                page.text(b"F1", 16.0, 72.0, 750.0, "Chapter Three");
            }
            // The same numeral on every page is not a page number; it
            // recurs, so it is a running foot.
            page.text(b"F1", 9.0, 300.0, 30.0, "2026");
            for row in 0..20 {
                page.text(
                    b"F1",
                    10.0,
                    72.0,
                    700.0 - row as f64 * 12.0,
                    &prose(at * 50 + row, 80),
                );
            }
        });
    }
    let doc = open(builder.finish());
    let order = doc
        .inferred_order(2, &InferenceOptions::default())
        .expect("page 3");
    let title = order
        .blocks
        .iter()
        .find(|b| b.lines.iter().any(|l| l.text == "Chapter Three"))
        .expect("the title");
    assert_eq!(title.role, Role::Body);
    let foot = order.blocks.last().expect("blocks");
    assert_eq!(foot.lines[0].text, "2026");
    assert_eq!(foot.role, Role::RunningFoot);
    assert!(order.warnings.is_empty(), "{:?}", order.warnings);
}

/// **The same words elsewhere on the band are not a running head.** Five
/// pages each open with a one-word line, the same word, set at a different
/// place along the top band on every page: it recurs, but never where it
/// stood, so it is body every time.
#[test]
fn a_recurrence_elsewhere_on_the_band_is_not_a_running_head() {
    let mut builder = DocumentBuilder::new();
    builder.add_base_font(b"F1", b"Helvetica");
    for at in 0..5 {
        builder.add_page(612.0, 792.0, |page| {
            page.text(b"F1", 10.0, 72.0 + at as f64 * 90.0, 760.0, "Summary");
            for row in 0..20 {
                page.text(
                    b"F1",
                    10.0,
                    72.0,
                    700.0 - row as f64 * 12.0,
                    &prose(at * 50 + row, 80),
                );
            }
        });
    }
    let doc = open(builder.finish());
    for index in 0..5 {
        let order = doc
            .inferred_order(index, &InferenceOptions::default())
            .expect("a page");
        assert!(
            order.blocks.iter().all(|b| b.role == Role::Body),
            "page {index}: {:?}",
            order.blocks.iter().map(|b| b.role).collect::<Vec<_>>()
        );
    }
}

/// **"Page 3 of 9" recurs**, its digits masked; and a roman page number
/// counts like an arabic one.
#[test]
fn masked_and_roman_page_furniture_is_found() {
    let mut builder = DocumentBuilder::new();
    builder.add_base_font(b"F1", b"Helvetica");
    let romans = ["iii", "iv", "v", "vi", "vii"];
    for (at, roman) in romans.iter().enumerate() {
        builder.add_page(612.0, 792.0, |page| {
            page.text(b"F1", 9.0, 72.0, 760.0, &format!("Page {} of 9", at + 3));
            page.text(b"F1", 9.0, 300.0, 30.0, roman);
            for row in 0..20 {
                page.text(
                    b"F1",
                    10.0,
                    72.0,
                    700.0 - row as f64 * 12.0,
                    &prose(at * 50 + row, 80),
                );
            }
        });
    }
    let doc = open(builder.finish());
    for index in 0..5 {
        let order = doc
            .inferred_order(index, &InferenceOptions::default())
            .expect("a page");
        assert_eq!(
            order.blocks.first().map(|b| b.role),
            Some(Role::RunningHead),
            "page {index}"
        );
        assert_eq!(
            order.blocks.last().map(|b| b.role),
            Some(Role::PageNumber),
            "page {index}"
        );
    }
}

/// **The answer for a page does not depend on how it was asked for**: one page
/// at a time and the whole document at once give the same order, role for
/// role.
#[test]
fn every_way_of_asking_gives_the_same_order() {
    let doc = furnished_book(6);
    let all = doc.inferred_orders(0..doc.page_count(), &hidden());
    assert_eq!(all.len(), 6);
    for (index, together) in all.iter().enumerate() {
        let alone = doc.inferred_order(index as u32, &hidden()).expect("a page");
        let page = doc
            .page(index as u32)
            .expect("a page")
            .inferred_order(&hidden());
        for other in [&alone, &page] {
            assert_eq!(other.plain_text(), together.plain_text(), "page {index}");
            assert_eq!(other.permutation, together.permutation, "page {index}");
            let roles =
                |o: &tinker_pdf::InferredOrder| o.blocks.iter().map(|b| b.role).collect::<Vec<_>>();
            assert_eq!(roles(other), roles(together), "page {index}");
        }
    }
}

// ---- footnotes ------------------------------------------------------------------

/// How a footnote fixture marks and separates its notes.
#[derive(Clone, Copy)]
struct Notes {
    /// A half-point rule a third of the measure wide over the notes.
    rule: bool,
    /// Note markers raised with `Ts`; otherwise set on the baseline.
    raised: bool,
    /// The body's reference marks raised with `Ts`; otherwise none at all.
    references: bool,
    /// Note 2 set above note 1 on the page, the reverse of its marks.
    swapped: bool,
    /// The notes drawn one after the other after the body, so the text
    /// device makes one block of both.
    together: bool,
}

/// A page of body text with two reference marks and two footnotes, tagged in
/// reading order — every body line a `/P`, every note a `/Note` after the
/// body, in mark order — and drawn note 2 first, then the body, then note 1.
fn footnoted(notes: Notes) -> Document {
    let rows = 20usize;
    let mut body: Vec<String> = Vec::new();
    for row in 0..rows {
        let y = 720.0 - row as f64 * 12.0;
        let text = prose(row, 70);
        let mark = match row {
            3 => Some("1"),
            12 => Some("2"),
            _ => None,
        };
        body.push(match (mark, notes.references) {
            (Some(m), true) => format!(
                "BT /F1 10 Tf 72 {y} Td ({text}) Tj /F1 6 Tf 4 Ts ({m}) Tj 0 Ts /F1 10 Tf ( more) Tj ET\n"
            ),
            _ => format!("BT /F1 10 Tf 72 {y} Td ({text}) Tj ET\n"),
        });
    }
    let note = |mark: &str, y: f64, first: &str, second: &str| -> String {
        let opener = if notes.raised {
            format!("/F1 5 Tf 3 Ts ({mark}) Tj 0 Ts /F1 8 Tf ( {first}) Tj")
        } else {
            format!("/F1 8 Tf ({mark} {first}) Tj")
        };
        format!(
            "BT 72 {y} Td {opener} ET BT /F1 8 Tf 72 {} Td ({second}) Tj ET\n",
            y - 10.0
        )
    };
    let (y1, y2) = if notes.swapped {
        (82.0, 105.0)
    } else {
        (105.0, 82.0)
    };
    let note1 = note(
        "1",
        y1,
        "The first note, at the foot",
        "of the page it annotates.",
    );
    let note2 = note(
        "2",
        y2,
        "The second note, after it.",
        "And its second line.",
    );
    let mut builder = DocumentBuilder::new();
    builder.add_base_font(b"F1", b"Helvetica");
    builder.add_page(612.0, 792.0, |page| {
        let key = rows as u64;
        if !notes.together {
            page.tagged_keyed(b"Note", key + 2, key + 1, |p| p.raw(note2.as_bytes()));
        }
        for (row, line) in body.iter().enumerate() {
            page.tagged_keyed(b"P", row as u64 + 1, row as u64, |p| p.raw(line.as_bytes()));
        }
        if notes.rule {
            page.raw(b"0.5 w 72 120 m 222 120 l S\n");
        }
        page.tagged_keyed(b"Note", key + 1, key, |p| p.raw(note1.as_bytes()));
        if notes.together {
            page.tagged_keyed(b"Note", key + 2, key + 1, |p| p.raw(note2.as_bytes()));
        }
    });
    open(builder.finish())
}

const ALL: Notes = Notes {
    rule: true,
    raised: true,
    references: true,
    swapped: false,
    together: false,
};

/// **Footnotes are read after the body, in the order of their marks.** Two
/// notes under a separator rule, each opening with a raised marker, set at
/// eight points under ten-point body text with two raised reference marks;
/// drawn note 2, body, note 1, and tagged body then the notes. With the tree
/// hidden the inference reads every pair the tree's way, where the stream
/// does not; it calls exactly the two notes footnotes — precision and recall
/// one against the producer's `/Note` elements, where the stream calls none.
#[test]
fn footnotes_are_read_after_the_body_in_mark_order() {
    let doc = footnoted(ALL);
    let scored = Scored::read(&doc, 0).expect("tagged");
    let (stream, inferred) = (scored.stream_agreement(), scored.inferred_agreement());
    println!(
        "footnotes: stream {:.4} inferred {:.4}",
        stream.score(),
        inferred.score()
    );
    assert!(!stream.at_least(1, 1), "the stream already reads it right");
    assert!(inferred.at_least(1, 1), "inferred {}", inferred.score());
    let truth = reading_order_support::note_keys(&doc, 0);
    let score = reading_order_support::role_score(&scored.inferred, &[Role::Footnote], &truth);
    println!("{score:?}");
    assert_eq!((score.called, score.correct), (2, 2));
    assert!(score.recall_at_least(1, 1));
    let notes: Vec<String> = scored
        .inferred
        .blocks
        .iter()
        .filter(|b| b.role == Role::Footnote)
        .map(|b| {
            b.lines
                .iter()
                .map(|l| l.text.as_str())
                .collect::<Vec<_>>()
                .join(" ")
        })
        .collect();
    assert!(notes[0].starts_with("1 The first note"), "{notes:?}");
    assert!(notes[1].starts_with("2 The second note"), "{notes:?}");
}

/// **Either sign is enough, and neither is not.** A rule over notes with
/// markers on the baseline: footnotes. Raised markers and no rule:
/// footnotes. Small text at the foot with neither: body, where it stands.
#[test]
fn a_rule_or_a_raised_marker_makes_a_note_and_neither_does_not() {
    for (notes, expect) in [
        (
            Notes {
                raised: false,
                ..ALL
            },
            2usize,
        ),
        (Notes { rule: false, ..ALL }, 2),
        (
            Notes {
                rule: false,
                raised: false,
                ..ALL
            },
            0,
        ),
    ] {
        let doc = footnoted(notes);
        let order = doc.inferred_order(0, &hidden()).expect("a page");
        let found = order
            .blocks
            .iter()
            .filter(|b| b.role == Role::Footnote)
            .count();
        assert_eq!(found, expect, "rule {} raised {}", notes.rule, notes.raised);
    }
}

/// **Mark order, not position order**: note 2 set above note 1 on the page
/// still reads after it, because the body marks 1 first; with no reference
/// marks in the body to go by, the notes read in the order they stand.
#[test]
fn notes_follow_their_marks_and_else_their_places() {
    let doc = footnoted(Notes {
        swapped: true,
        ..ALL
    });
    let order = doc.inferred_order(0, &hidden()).expect("a page");
    let notes: Vec<&str> = order
        .blocks
        .iter()
        .filter(|b| b.role == Role::Footnote)
        .filter_map(|b| b.lines.first().map(|l| l.text.as_str()))
        .collect();
    assert!(
        notes[0].starts_with('1') && notes[1].starts_with('2'),
        "{notes:?}"
    );

    let doc = footnoted(Notes {
        swapped: true,
        references: false,
        ..ALL
    });
    let order = doc.inferred_order(0, &hidden()).expect("a page");
    let notes: Vec<&str> = order
        .blocks
        .iter()
        .filter(|b| b.role == Role::Footnote)
        .filter_map(|b| b.lines.first().map(|l| l.text.as_str()))
        .collect();
    assert!(
        notes[0].starts_with('2') && notes[1].starts_with('1'),
        "{notes:?}"
    );
}

/// **Notes set one after the other are cut at their markers.** Drawn
/// together after the body, the two notes are one block of the text
/// device's; each line opening with a raised marker starts a note of its own.
#[test]
fn notes_set_together_are_cut_at_their_markers() {
    let doc = footnoted(Notes {
        together: true,
        ..ALL
    });
    let order = doc.inferred_order(0, &hidden()).expect("a page");
    let notes: Vec<Vec<&str>> = order
        .blocks
        .iter()
        .filter(|b| b.role == Role::Footnote)
        .map(|b| b.lines.iter().map(|l| l.text.as_str()).collect())
        .collect();
    assert_eq!(notes.len(), 2, "{notes:?}");
    assert_eq!(notes[0].len(), 2);
    assert_eq!(notes[1].len(), 2);
}

/// **A rise of zero everywhere changes nothing**: the same page with no
/// glyph raised and no rule drawn calls nothing a footnote, and reads its
/// foot where the columns put it — the monotone property `Glyph::baseline`'s
/// documentation states, asserted on the inference that reads it.
#[test]
fn a_rise_of_zero_everywhere_changes_nothing() {
    let flat = footnoted(Notes {
        rule: false,
        raised: false,
        references: false,
        swapped: false,
        together: false,
    });
    let order = flat.inferred_order(0, &hidden()).expect("a page");
    assert!(order.blocks.iter().all(|b| b.role != Role::Footnote));
    // Read top to bottom: the notes at the foot, in the order they stand.
    let text = order.plain_text();
    let first = text.find("1 The first note").expect("note 1");
    let second = text.find("2 The second note").expect("note 2");
    assert!(first < second);
    assert!(text.find("alpha").expect("body") < first);
}
// ---- one column: the set where nothing may move ------------------------------

/// **One column drawn top to bottom moves nothing**, whatever is in it: a
/// heading over paragraphs, a list, a short last line, a line set larger.
#[test]
fn a_one_column_page_drawn_in_order_moves_nothing() {
    let mut runs = vec![run(72.0, 740.0, 18.0, "One Column")];
    let mut y = 710.0;
    for paragraph in 0..4 {
        for line in 0..6 {
            let length = if line == 5 { 30 } else { 90 };
            runs.push(run(72.0, y, 10.0, prose(paragraph * 10 + line, length)));
            y -= 12.0;
        }
        y -= 10.0;
    }
    for item in 0..4 {
        runs.push(run(90.0, y, 10.0, format!("- {}", prose(90 + item, 30))));
        y -= 12.0;
    }
    let read = identity(runs.len());
    let draw: Vec<usize> = (0..runs.len()).collect();
    let doc = open(page(612.0, 792.0, &runs, &draw, Some(&read)));
    let scored = Scored::read(&doc, 0).expect("tagged");
    assert!(scored.stream_agreement().at_least(1, 1));
    assert!(scored.inferred_agreement().at_least(1, 1));
    assert_eq!(scored.inferred.moved(), 0);
    assert_eq!(scored.inferred.columns, 1);
}

/// **A ragged right edge is not a near miss.** One column of lines from three
/// to nine words long, in order: the whitespace past the short lines' ends is
/// as wide as a gap and as empty, but it has text on one side only, so it is
/// the column's margin and nothing is named.
#[test]
fn a_ragged_edge_is_a_margin_and_not_a_gap() {
    let runs: Vec<Run> = (0..30)
        .map(|row| {
            run(
                72.0,
                720.0 - row as f64 * 12.0,
                10.0,
                prose(row, 20 + (row * 17) % 60),
            )
        })
        .collect();
    let draw: Vec<usize> = (0..runs.len()).collect();
    let doc = open(page(612.0, 792.0, &runs, &draw, None));
    let order = doc
        .page(0)
        .expect("a page")
        .inferred_order(&InferenceOptions::default());
    assert_eq!(order.columns, 1);
    assert_eq!(order.moved(), 0);
    assert!(order.warnings.is_empty(), "{:?}", order.warnings);
}

/// **A page of ten thousand scattered one-glyph lines is inferred, not
/// squared.** Every line its own fragment at its own `x`, which is the input
/// that made each of the column finder's questions a walk over every fragment
/// before they became binary searches; the inference must finish, find no
/// column, and give back a permutation of the page.
#[test]
fn a_page_of_scattered_glyphs_is_inferred_in_bounded_work() {
    let mut content = String::new();
    let mut state = 0x9E37_79B9_u64;
    for _ in 0..10_000 {
        state = state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        let x = 20.0 + ((state >> 20) % 5_720) as f64 / 10.0;
        let y = 20.0 + ((state >> 40) % 7_520) as f64 / 10.0;
        content.push_str(&format!("BT /F1 4 Tf {x:.1} {y:.1} Td (o) Tj ET\n"));
    }
    let mut builder = DocumentBuilder::new();
    builder.add_base_font(b"F1", b"Helvetica");
    builder.add_page(612.0, 792.0, |page| page.raw(content.as_bytes()));
    let doc = open(builder.finish());
    let page = doc.page(0).expect("a page");
    let started = std::time::Instant::now();
    let order = page.inferred_order(&InferenceOptions::default());
    println!("10 000 scattered glyphs: {:?}", started.elapsed());
    assert_permutation(
        &order.permutation,
        page.text().lines().iter().map(|l| l.chars.len()).sum(),
        "scattered",
    );
}

/// **Nothing on a committed untagged page moves** that its stream drew top to
/// bottom in one column. `testdata/` is mutool's output and holds no
/// multi-column page; every page there is in the set where nothing may move,
/// and this asserts it is.
#[test]
fn no_committed_untagged_page_moves() {
    let mut pages = 0usize;
    for (name, doc) in committed() {
        if doc.structure().is_some() {
            continue;
        }
        for page in doc.pages() {
            let order = page.inferred_order(&InferenceOptions::default());
            assert_eq!(
                order.moved(),
                0,
                "{name} page {}: {:?}",
                page.index(),
                order.warnings
            );
            pages += 1;
        }
    }
    assert!(pages > 0);
}

// ---- what the inference will not guess -----------------------------------------

#[test]
fn a_sparse_page_looks_for_no_columns() {
    let runs = vec![
        run(72.0, 700.0, 10.0, "left"),
        run(400.0, 700.0, 10.0, "right"),
    ];
    let doc = open(page(612.0, 792.0, &runs, &[0, 1], None));
    let order = doc
        .page(0)
        .expect("a page")
        .inferred_order(&InferenceOptions::default());
    assert_eq!(order.columns, 1);
    assert_eq!(order.moved(), 0);
    assert!(order
        .warnings
        .contains(&InferenceWarning::PageTooSparse { lines: 2 }));
    const { assert!(MIN_COLUMN_LINES > 2) };
}

/// A line drawn at an angle is placed last, as `Unplaced`, and named; a page
/// that is mostly such lines is declined rather than guessed at.
#[test]
fn rotated_lines_are_unplaced_and_a_rotated_page_is_declined() {
    let rotated = |text: &str, y: f64| {
        format!("BT /F1 10 Tf 0.7071 0.7071 -0.7071 0.7071 300 {y} Tm ({text}) Tj ET")
    };
    let mut builder = DocumentBuilder::new();
    builder.add_base_font(b"F1", b"Helvetica");
    builder.add_page(612.0, 792.0, |page| {
        page.raw(rotated("DRAFT", 400.0).as_bytes());
        for row in 0..5 {
            page.text(
                b"F1",
                10.0,
                72.0,
                700.0 - row as f64 * 12.0,
                &prose(row, 60),
            );
        }
    });
    builder.add_page(612.0, 792.0, |page| {
        for row in 0..5 {
            page.raw(rotated(&prose(row, 20), 100.0 + row as f64 * 40.0).as_bytes());
        }
        page.text(b"F1", 10.0, 72.0, 700.0, "upright");
    });
    let doc = open(builder.finish());
    let order = doc
        .inferred_order(0, &InferenceOptions::default())
        .expect("page one");
    // The text device, not this inference, decides what a line is, and it
    // makes more than one of a word set at 45 degrees; every one of them is
    // counted and placed last, in the stream's order.
    let last = order.blocks.last().expect("blocks");
    assert_eq!(last.role, Role::Unplaced);
    let unplaced: String = last.lines.iter().map(|l| l.text.as_str()).collect();
    assert_eq!(unplaced, "DRAFT");
    assert!(order.warnings.contains(&InferenceWarning::RotatedText {
        lines: last.lines.len()
    }));
    assert!(
        order
            .blocks
            .iter()
            .rev()
            .skip(1)
            .all(|b| b.role == Role::Body),
        "the upright body is read as body"
    );

    let order = doc
        .inferred_order(1, &InferenceOptions::default())
        .expect("page two");
    assert_eq!(order.declined(), Some(DeclineReason::RotatedText));
    assert_eq!(order.moved(), 0);
}

/// Two pages drawn with a vertical composite font (`/Identity-V`, 9.7.4.3)
/// beside Helvetica: on the first, five lines across and one down; on the
/// second, four down and one across. Assembled by hand, because the
/// document builder writes no vertical font; text extraction needs no font
/// program, so none is embedded.
fn vertical_pages() -> Document {
    let down = |x: f64, text: &str| {
        let codes: String = text.bytes().map(|b| format!("{b:04X}")).collect();
        format!("BT /F0 12 Tf {x} 700 Td <{codes}> Tj ET\n")
    };
    let across = |y: f64, text: &str| format!("BT /F1 10 Tf 72 {y} Td ({text}) Tj ET\n");
    let mut first = down(540.0, "VERTICAL");
    for row in 0..5 {
        first.push_str(&across(600.0 - row as f64 * 12.0, &prose(row, 40)));
    }
    let mut second = across(100.0, "across");
    for column in 0..4 {
        second.push_str(&down(500.0 - column as f64 * 30.0, "COLUMNS"));
    }
    let to_unicode = "/CIDInit /ProcSet findresource begin 12 dict begin begincmap\n\
        /CMapName /Fixture-UCS def\n\
        1 begincodespacerange <0000> <FFFF> endcodespacerange\n\
        1 beginbfrange <0041> <005A> <0041> endbfrange\n\
        endcmap CMapName currentdict /CMap defineresource pop end end";
    let objects: Vec<String> = vec![
        "<< /Type /Catalog /Pages 2 0 R >>".into(),
        "<< /Type /Pages /Count 2 /Kids [3 0 R 4 0 R] >>".into(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] \
         /Resources << /Font << /F0 5 0 R /F1 6 0 R >> >> /Contents 9 0 R >>"
            .into(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] \
         /Resources << /Font << /F0 5 0 R /F1 6 0 R >> >> /Contents 10 0 R >>"
            .into(),
        "<< /Type /Font /Subtype /Type0 /BaseFont /Fixture /Encoding /Identity-V \
         /DescendantFonts [7 0 R] /ToUnicode 8 0 R >>"
            .into(),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".into(),
        "<< /Type /Font /Subtype /CIDFontType2 /BaseFont /Fixture \
         /CIDSystemInfo << /Registry (Adobe) /Ordering (Identity) /Supplement 0 >> \
         /DW 1000 /CIDToGIDMap /Identity >>"
            .into(),
        format!(
            "<< /Length {} >>\nstream\n{to_unicode}\nendstream",
            to_unicode.len()
        ),
        format!("<< /Length {} >>\nstream\n{first}\nendstream", first.len()),
        format!(
            "<< /Length {} >>\nstream\n{second}\nendstream",
            second.len()
        ),
    ];
    let mut out = String::from("%PDF-1.7\n");
    for (at, object) in objects.iter().enumerate() {
        out.push_str(&format!("{} 0 obj\n{object}\nendobj\n", at + 1));
    }
    out.push_str(&format!(
        "trailer\n<< /Size {} /Root 1 0 R >>\n%%EOF\n",
        objects.len() + 1
    ));
    open(out.into_bytes())
}

/// **A vertical line is placed last, as `Unplaced`, and named; a page that is
/// mostly vertical is declined** — the order of vertical lines is not a
/// question this inference answers.
#[test]
fn vertical_lines_are_unplaced_and_a_vertical_page_is_declined() {
    let doc = vertical_pages();
    let page = doc.page(0).expect("page one");
    let vertical: Vec<String> = page
        .text()
        .lines()
        .iter()
        .filter(|l| l.wmode == tinker_pdf::WritingMode::Vertical)
        .map(|l| l.text.clone())
        .collect();
    assert_eq!(
        vertical,
        ["VERTICAL"],
        "the fixture draws one vertical line"
    );
    let order = page.inferred_order(&InferenceOptions::default());
    assert!(
        order
            .warnings
            .contains(&InferenceWarning::VerticalWriting { lines: 1 }),
        "{:?}",
        order.warnings
    );
    let last = order.blocks.last().expect("blocks");
    assert_eq!(last.role, Role::Unplaced);
    assert_eq!(last.lines.len(), 1);
    assert_eq!(last.lines[0].text, "VERTICAL");
    assert!(order
        .blocks
        .iter()
        .rev()
        .skip(1)
        .all(|b| b.role == Role::Body));

    let order = doc
        .inferred_order(1, &InferenceOptions::default())
        .expect("page two");
    assert_eq!(order.declined(), Some(DeclineReason::VerticalWriting));
    assert_eq!(order.moved(), 0);
    assert!(order.blocks.iter().all(|b| b.role == Role::Unplaced));
}

// ---- tables, handed off ---------------------------------------------------------

/// A cell of [`table_page`]'s table: its row, its column, its lines.
const CELLS: [(usize, usize, &[&str]); 6] = [
    (0, 0, &["alpha first", "alpha second"]),
    (0, 1, &["bravo"]),
    (0, 2, &["charlie"]),
    (1, 0, &["delta first", "delta second"]),
    (1, 1, &["echo"]),
    (1, 2, &["foxtrot"]),
];

/// A page holding three lines of a paragraph, a ruled table of two rows by
/// three columns, 72 to 432 across (to 282 in a column) and 600 to 540 down, whose first column's
/// cells each hold two lines, and three lines of a second paragraph. Every
/// paragraph line and every cell is one element of the tree, read in that
/// order, a cell's two lines one element; the text is drawn **baseline by
/// baseline across the page**, as a producer writing lines draws it, so the
/// stream reads a first-column cell's second line after the rest of its row.
///
/// `close` sets the paragraphs one ordinary line from the table's rules,
/// where the text device may run one block through it; otherwise they stand
/// two lines off. With `columns`, the whole is set in the left column of a
/// two-column page, beside a right column of twenty lines drawn
/// interleaved with it.
fn table_page(close: bool, columns: bool, tag: bool) -> Vec<u8> {
    let (above, below) = if close {
        (606.0, 528.0)
    } else {
        (630.0, 510.0)
    };
    let (width, length) = if columns { (210.0, 30) } else { (360.0, 60) };
    let mut lines: Vec<(f64, f64, String, u64)> = Vec::new(); // (x, y, text, element)
    for row in 0..3 {
        lines.push((
            72.0,
            above + (2 - row) as f64 * 12.0,
            prose(row, length),
            row as u64,
        ));
    }
    for (at, (row, column, texts)) in CELLS.iter().enumerate() {
        for (line, text) in texts.iter().enumerate() {
            lines.push((
                76.0 + *column as f64 * width / 3.0,
                588.0 - *row as f64 * 30.0 - line as f64 * 12.0,
                (*text).to_string(),
                3 + at as u64,
            ));
        }
    }
    for row in 0..3 {
        lines.push((
            72.0,
            below - row as f64 * 12.0,
            prose(10 + row, length),
            9 + row as u64,
        ));
    }
    if columns {
        for row in 0..20 {
            lines.push((
                340.0,
                654.0 - row as f64 * 12.0,
                prose(20 + row, 30),
                12 + row as u64,
            ));
        }
    }
    // Baseline by baseline down the page, left to right along each.
    let mut draw: Vec<usize> = (0..lines.len()).collect();
    draw.sort_by(|a, b| {
        let (la, lb) = (&lines[*a], &lines[*b]);
        lb.1.total_cmp(&la.1).then(la.0.total_cmp(&lb.0))
    });
    let mut rules = String::from("0.5 w\n");
    for y in [600.0, 570.0, 540.0] {
        rules.push_str(&format!("72 {y} m {} {y} l S\n", 72.0 + width));
    }
    for column in 0..4 {
        let x = 72.0 + column as f64 * width / 3.0;
        rules.push_str(&format!("{x} 540 m {x} 600 l S\n"));
    }
    let mut builder = DocumentBuilder::new();
    builder.add_base_font(b"F1", b"Helvetica");
    builder.add_page(612.0, 792.0, |page| {
        page.raw(rules.as_bytes());
        for at in draw {
            let (x, y, text, element) = &lines[at];
            if tag {
                page.tagged_keyed(b"P", element + 1, *element, |p| {
                    p.text(b"F1", 10.0, *x, *y, text)
                });
            } else {
                page.text(b"F1", 10.0, *x, *y, text);
            }
        }
    });
    builder.finish()
}

/// The table's text in its own order: each cell's lines, cell by cell, row
/// by row.
fn table_text() -> Vec<String> {
    CELLS
        .iter()
        .flat_map(|(_, _, texts)| texts.iter().map(|t| (*t).to_string()))
        .collect()
}

/// **A ruled table is read as one block, in its own order, where it
/// stands** — the sibling design's `TableSuspected` handoff, on a ruled-table
/// page. The tree reads each cell whole; the stream reads a first-column
/// cell's second line after the rest of its row, and the inference, which
/// reads lines, would do the same or worse. Handed the table the table
/// inference found, the order agrees with the tree on every pair, the table
/// is one `Table` block whose lines are the table's permutation, and nothing
/// outside it moves — whether the paragraphs stand off the table or one line
/// from its rules.
#[test]
fn a_ruled_table_is_read_as_one_block_in_its_own_order() {
    for close in [false, true] {
        let doc = open(table_page(close, false, true));
        let scored = Scored::read(&doc, 0).expect("a tagged page");
        let stream = scored.stream_agreement();
        let inferred = scored.inferred_agreement();
        println!(
            "ruled table, close {close}: stream {:.4} ({} of {} pairs), inferred {:.4}",
            stream.score(),
            stream.agreeing,
            stream.pairs,
            inferred.score()
        );
        assert!(stream.pairs > 0 && !stream.at_least(1, 1));
        assert!(inferred.at_least(1, 1), "inferred {}", inferred.score());
        assert_eq!(
            scored.inferred.warnings,
            [
                InferenceWarning::TreePresent,
                InferenceWarning::TableSuspected { tables: 1 }
            ]
        );

        let untagged = open(table_page(close, false, false));
        let page = untagged.page(0).expect("a page");
        let order = page.inferred_order(&InferenceOptions::default());
        assert_eq!(
            order.warnings,
            [InferenceWarning::TableSuspected { tables: 1 }]
        );
        let tables: Vec<_> = order
            .blocks
            .iter()
            .filter(|b| b.role == Role::Table)
            .collect();
        assert_eq!(tables.len(), 1);
        let table = tables[0];
        let read: Vec<String> = table.lines.iter().map(|l| l.text.clone()).collect();
        assert_eq!(read, table_text());
        assert_eq!(table.column, Some(0));
        // Its characters are the table inference's, in its order.
        let found = page.inferred_tables(&TableOptions::default());
        assert_eq!(found.tables.len(), 1);
        let held = &found.tables[0].permutation;
        assert_eq!(
            order.permutation.get(table.start..table.start + held.len()),
            Some(held.as_slice())
        );
        // Nothing outside the table moved.
        let span = table.start..table.start + held.len();
        assert!(order
            .permutation
            .iter()
            .enumerate()
            .all(|(at, from)| span.contains(&at) || at == *from));
        assert!(order.moved() > 0);
    }
}

/// **A table in a column is read in that column.** The same page set in the
/// left column of two, its lines drawn interleaved with the right column's:
/// two columns found from the lines outside the table, the table read whole
/// between the left column's paragraphs, and every pair the tree's way —
/// where the stream reads across both columns.
#[test]
fn a_table_in_a_column_is_read_in_that_column() {
    let doc = open(table_page(false, true, true));
    let scored = Scored::read(&doc, 0).expect("a tagged page");
    let stream = scored.stream_agreement();
    let inferred = scored.inferred_agreement();
    println!(
        "ruled table in a column: stream {:.4} ({} of {} pairs), inferred {:.4}",
        stream.score(),
        stream.agreeing,
        stream.pairs,
        inferred.score()
    );
    assert!(stream.score() < 0.9, "stream {}", stream.score());
    assert!(inferred.at_least(1, 1), "inferred {}", inferred.score());
    assert_eq!(scored.inferred.columns, 2);
    let table = scored
        .inferred
        .blocks
        .iter()
        .find(|b| b.role == Role::Table)
        .expect("the table");
    assert_eq!(table.column, Some(0));
    assert_eq!(scored.crossings(), 0);
}

/// **A table across the columns is read between the column sets it
/// divides**, as a heading across them is: two columns of ten lines, a ruled
/// table the width of both, and two more columns of ten lines, drawn line by
/// line across the page and tagged column by column with the table between.
#[test]
fn a_table_across_the_columns_is_read_between_them() {
    // (x, y, text, element)
    let mut lines: Vec<(f64, f64, String, u64)> = Vec::new();
    let mut element = 0u64;
    let mut columns = |top: f64, lines: &mut Vec<(f64, f64, String, u64)>| {
        for x in [72.0, 324.0] {
            for row in 0..10 {
                lines.push((
                    x,
                    top - row as f64 * 12.0,
                    prose(element as usize, 40),
                    element,
                ));
                element += 1;
            }
        }
    };
    columns(690.0, &mut lines);
    let cells = ["alpha", "bravo", "charlie", "delta", "echo", "foxtrot"];
    let table_first = 20u64;
    for (at, text) in cells.iter().enumerate() {
        let (row, column) = (at / 3, at % 3);
        lines.push((
            76.0 + column as f64 * 156.0,
            548.0 - row as f64 * 30.0,
            (*text).to_string(),
            table_first + at as u64,
        ));
    }
    let mut element = 26u64;
    for x in [72.0, 324.0] {
        for row in 0..10 {
            lines.push((
                x,
                480.0 - row as f64 * 12.0,
                prose(element as usize, 40),
                element,
            ));
            element += 1;
        }
    }
    let mut draw: Vec<usize> = (0..lines.len()).collect();
    draw.sort_by(|a, b| {
        let (la, lb) = (&lines[*a], &lines[*b]);
        lb.1.total_cmp(&la.1).then(la.0.total_cmp(&lb.0))
    });
    let mut rules = String::from("0.5 w\n");
    for y in [560.0, 530.0, 500.0] {
        rules.push_str(&format!("72 {y} m 540 {y} l S\n"));
    }
    for x in [72.0, 228.0, 384.0, 540.0] {
        rules.push_str(&format!("{x} 500 m {x} 560 l S\n"));
    }
    let mut builder = DocumentBuilder::new();
    builder.add_base_font(b"F1", b"Helvetica");
    builder.add_page(612.0, 792.0, |page| {
        page.raw(rules.as_bytes());
        for at in draw {
            let (x, y, text, element) = &lines[at];
            page.tagged_keyed(b"P", element + 1, *element, |p| {
                p.text(b"F1", 10.0, *x, *y, text)
            });
        }
    });
    let doc = open(builder.finish());
    let scored = Scored::read(&doc, 0).expect("a tagged page");
    let stream = scored.stream_agreement();
    let inferred = scored.inferred_agreement();
    println!(
        "a table across the columns: stream {:.4}, inferred {:.4}",
        stream.score(),
        inferred.score()
    );
    assert!(stream.score() < 0.9, "stream {}", stream.score());
    assert!(inferred.at_least(1, 1), "inferred {}", inferred.score());
    assert_eq!(scored.inferred.columns, 2);
    let table = scored
        .inferred
        .blocks
        .iter()
        .find(|b| b.role == Role::Table)
        .expect("the table");
    assert_eq!(table.column, None);
    assert_eq!(table.section, 1);
    let read: Vec<&str> = table.lines.iter().map(|l| l.text.as_str()).collect();
    assert_eq!(read, cells);
}

/// **A page that is mostly a ruled table is declined**: its order is the
/// table's, which [`tinker_pdf::Page::inferred_tables`] gives, and not a
/// reading order's. The blocks are the stream's, unplaced, and nothing moves.
#[test]
fn a_page_that_is_mostly_a_table_is_declined() {
    let mut content = String::from("0.5 w\n");
    for row in 0..=6 {
        let y = 700.0 - row as f64 * 20.0;
        content.push_str(&format!("72 {y} m 472 {y} l S\n"));
    }
    for column in 0..=4 {
        let x = 72.0 + column as f64 * 100.0;
        content.push_str(&format!("{x} 580 m {x} 700 l S\n"));
    }
    for column in 0..4 {
        for row in 0..6 {
            let (x, y) = (76.0 + column as f64 * 100.0, 686.0 - row as f64 * 20.0);
            content.push_str(&format!(
                "BT /F1 10 Tf {x} {y} Td ({}) Tj ET\n",
                prose(row * 4 + column, 8)
            ));
        }
    }
    content.push_str("BT /F1 10 Tf 72 540 Td (Notes) Tj ET\n");
    let mut builder = DocumentBuilder::new();
    builder.add_base_font(b"F1", b"Helvetica");
    builder.add_page(612.0, 792.0, |page| page.raw(content.as_bytes()));
    let doc = open(builder.finish());
    let order = doc
        .inferred_order(0, &InferenceOptions::default())
        .expect("a page");
    assert_eq!(order.declined(), Some(DeclineReason::Table));
    assert_eq!(
        order.warnings,
        [
            InferenceWarning::TableSuspected { tables: 1 },
            InferenceWarning::Declined {
                reason: DeclineReason::Table
            }
        ]
    );
    assert_eq!(order.moved(), 0);
    assert!(order.blocks.iter().all(|b| b.role == Role::Unplaced));
}

/// **A held table's own rules are no footnote separator.** A paragraph, a
/// ruled table, and under the table's bottom rule a line set small — a
/// table's source line, the last thing in the column. Small text under a
/// rule is how a footnote looks; the rule is the table's, so the line is
/// body.
#[test]
fn a_tables_rules_do_not_make_a_footnote() {
    let mut content = String::from("0.5 w\n");
    for y in [600.0, 570.0, 540.0] {
        content.push_str(&format!("72 {y} m 432 {y} l S\n"));
    }
    for x in [72.0, 252.0, 432.0] {
        content.push_str(&format!("{x} 540 m {x} 600 l S\n"));
    }
    for (x, y, text) in [
        (76.0, 588.0, "alpha"),
        (256.0, 588.0, "bravo"),
        (76.0, 558.0, "charlie"),
        (256.0, 558.0, "delta"),
    ] {
        content.push_str(&format!("BT /F1 10 Tf {x} {y} Td ({text}) Tj ET\n"));
    }
    for row in 0..3 {
        content.push_str(&format!(
            "BT /F1 10 Tf 72 {} Td ({}) Tj ET\n",
            650.0 - row as f64 * 12.0,
            prose(row, 60)
        ));
    }
    content.push_str("BT /F1 7 Tf 72 528 Td (Source: the fixture itself) Tj ET\n");
    let mut builder = DocumentBuilder::new();
    builder.add_base_font(b"F1", b"Helvetica");
    builder.add_page(612.0, 792.0, |page| page.raw(content.as_bytes()));
    let doc = open(builder.finish());
    let order = doc
        .inferred_order(0, &InferenceOptions::default())
        .expect("a page");
    assert!(order
        .warnings
        .contains(&InferenceWarning::TableSuspected { tables: 1 }));
    let last = order.blocks.last().expect("blocks");
    assert_eq!(last.lines[0].text, "Source: the fixture itself");
    assert_eq!(
        last.role,
        Role::Body,
        "{:?}",
        order.blocks.iter().map(|b| b.role).collect::<Vec<_>>()
    );
    assert!(order.blocks.iter().all(|b| b.role != Role::Footnote));
}

/// **A book's bordered table is one block, and nothing moves.** The EPUB
/// writer draws a page in the order it tags it, so a table it borders is in
/// the set where nothing may move: with the tree hidden, every page reads
/// every pair the tree's way, no character moves, and the table is handed
/// off as one `Table` block.
#[test]
fn a_books_bordered_table_is_one_block_and_nothing_moves() {
    let mut body = String::from("<p>Before the table, a paragraph of its own.</p><table>");
    for (name, weight, price) in [
        ("Apple", "150", "1.20"),
        ("Banana", "120", "0.80"),
        ("Cherry", "5", "0.10"),
    ] {
        body.push_str(&format!(
            "<tr><td>{name}</td><td>{weight}</td><td>{price}</td></tr>"
        ));
    }
    body.push_str("</table><p>After the table, another.</p>");
    let doc = open(styled_book(
        "en",
        "table { border-collapse: collapse } td { border: 1px solid black; padding: 4px }",
        &body,
    ));
    let mut tables = 0;
    for index in 0..doc.page_count() {
        let Some(scored) = Scored::read(&doc, index) else {
            continue;
        };
        assert!(scored.inferred_agreement().at_least(1, 1));
        assert_eq!(scored.inferred.moved(), 0);
        tables += scored
            .inferred
            .blocks
            .iter()
            .filter(|b| b.role == Role::Table)
            .count();
    }
    assert_eq!(tables, 1);
}

// ---- the EPUB books ------------------------------------------------------------

/// **Every committed book, scored with its tree hidden.** The tree is the
/// book's own order, written by this engine from the XHTML and held to it by
/// `epub_structure.rs`; the stream is the order this engine's layout drew.
///
/// Measured when milestone 2 landed: every book's stream agrees exactly,
/// because the EPUB writer draws a page in the order it tags it. The books are
/// therefore in the set where nothing may move, and the inference is held to
/// exactly that: on every page of every book, no character moved and every
/// pair agrees.
#[test]
fn the_committed_books_read_in_their_own_order() {
    let mut stream_total = Agreement::default();
    let mut inferred_total = Agreement::default();
    for (name, doc) in committed() {
        if !name.ends_with(".epub") || doc.structure().is_none() {
            continue;
        }
        let mut stream = Agreement::default();
        let mut inferred = Agreement::default();
        let mut moved = 0usize;
        for index in 0..doc.page_count() {
            let Some(scored) = Scored::read(&doc, index) else {
                continue;
            };
            stream = stream.plus(scored.stream_agreement());
            inferred = inferred.plus(scored.inferred_agreement());
            moved += scored.inferred.moved();
        }
        println!(
            "{name:<28} stream {:.6} inferred {:.6} ({} pairs, {moved} moved)",
            stream.score(),
            inferred.score(),
            stream.pairs
        );
        assert!(stream.at_least(1, 1), "{name}: the stream disagrees");
        assert!(inferred.at_least(1, 1), "{name}: the inference disagrees");
        assert_eq!(moved, 0, "{name}: the inference moved a book's text");
        stream_total = stream_total.plus(stream);
        inferred_total = inferred_total.plus(inferred);
    }
    println!(
        "all books: stream {:.6} inferred {:.6} ({} pairs)",
        stream_total.score(),
        inferred_total.score(),
        stream_total.pairs
    );
    assert!(stream_total.pairs > 0, "no book was scored");
}

/// The body of a book laid out in two columns: twelve justified paragraphs of
/// prose in one `column-count: 2` container, at the CSS default gap of one
/// em — which, justified, is exactly the whitespace between the columns.
fn two_column_book() -> Document {
    let mut body = String::from(r#"<div class="mc">"#);
    for paragraph in 0..12 {
        body.push_str(&format!("<p>{}</p>", prose(paragraph * 37, 420)));
    }
    body.push_str("</div>");
    open(styled_book(
        "en",
        ".mc { column-count: 2 } p { margin: 0 0 1em 0; text-align: justify }",
        &body,
    ))
}

/// **A two-column book reads down its columns** (`css-multicol-1`, laid out
/// by this engine at the default `column-gap: normal`, which §4.1 makes one
/// em). The answer key is the XHTML's paragraph order, through the tree this
/// engine wrote for it; the stream already agrees, so what is asserted is
/// that the inference *finds* the columns the layout made — two on every page
/// with text on both sides of the gap, and walking the tree crosses none of
/// them — and moves nothing in doing so.
///
/// At the design's 1.5 em the gap is one column with a near miss, which is
/// why [`COLUMN_GAP_EMS`] is below one em.
#[test]
fn a_two_column_book_reads_down_its_columns() {
    let doc = two_column_book();
    let mut both = 0usize;
    for index in 0..doc.page_count() {
        let scored = Scored::read(&doc, index).expect("a tagged book");
        assert!(scored.inferred_agreement().at_least(1, 1), "page {index}");
        assert_eq!(scored.inferred.moved(), 0, "page {index}");
        assert_eq!(scored.crossings(), 0, "page {index}");
        // A page with lines starting on both halves is a page the layout
        // set in two columns; the book's last page, balanced short, may not
        // be one.
        let page = doc.page(index).expect("a page");
        let middle = (page.media_box().0 + page.media_box().2) / 2.0;
        let text = page.text();
        let starts: Vec<f64> = text.lines().iter().map(|l| l.quad.bounds().0).collect();
        let two_sided = starts.iter().any(|x| *x < middle) && starts.iter().any(|x| *x >= middle);
        println!(
            "page {index}: {} lines, {} columns{}",
            starts.len(),
            scored.inferred.columns,
            if two_sided { ", set in two" } else { "" }
        );
        assert_eq!(
            scored.inferred.columns,
            if two_sided { 2 } else { 1 },
            "page {index}"
        );
        if two_sided {
            both += 1;
        }
    }
    assert!(both >= 2, "the book is meant to fill pages in two columns");
}

/// **The same book drawn as a producer that writes across the page would draw
/// it**, and read back in the author's order.
///
/// Each page of the two-column book is drawn again — every line where this
/// engine's layout put it, word by word in the book's own standard face at
/// each word's own origin — but in the order a line-by-line producer emits: down the page by
/// baseline, left to right across the columns. Each line is tagged with its
/// position in the book's own tree, so the answer key is still the XHTML's.
/// The stream of the redrawn page interleaves the columns and falls well
/// short of the tree; the inference, tree hidden, is asserted at every pair.
#[test]
fn the_two_column_book_redrawn_across_the_page_reads_in_the_authors_order() {
    let book = two_column_book();
    let mut stream_total = Agreement::default();
    let mut inferred_total = Agreement::default();
    for index in 0..book.page_count() {
        if book.page(index).expect("a page").text().lines().is_empty() {
            continue;
        }
        let original = Scored::read(&book, index).expect("a tagged book");
        let page = book.page(index).expect("a page");
        let (width, height) = {
            let (x0, y0, x1, y1) = page.media_box();
            (x1 - x0, y1 - y0)
        };
        // Where each character stands in the book's own order.
        let mut position: std::collections::BTreeMap<Key, usize> = Default::default();
        for (at, key) in original.answer.iter().enumerate() {
            position.entry(key.clone()).or_insert(at);
        }
        let lines: Vec<tinker_pdf::TextLine> = page.text().lines().into_iter().cloned().collect();
        let mut keyed: Vec<(usize, &tinker_pdf::TextLine)> = lines
            .iter()
            .filter_map(|line| {
                let first = line.chars.first()?;
                Some((*position.get(&reading_order_support::key(first))?, line))
            })
            .collect();
        keyed.sort_by_key(|(at, _)| *at);
        // Drawn by baseline, top first, then left to right.
        let mut draw: Vec<usize> = (0..keyed.len()).collect();
        draw.sort_by(|a, b| {
            let (la, lb) = (keyed[*a].1, keyed[*b].1);
            let (ya, yb) = (la.chars[0].origin.1, lb.chars[0].origin.1);
            yb.total_cmp(&ya)
                .then(la.chars[0].origin.0.total_cmp(&lb.chars[0].origin.0))
        });
        // The book is set in a standard face, so the redrawn page sets every
        // word in the same face at the same size, and its geometry is the
        // book's to the advance.
        let face = keyed
            .first()
            .and_then(|(_, line)| line.chars.first())
            .and_then(|c| c.font.as_deref().map(str::to_owned))
            .unwrap_or_else(|| "Times-Roman".to_owned());
        let mut builder = DocumentBuilder::new();
        builder.add_base_font(b"F1", face.as_bytes());
        builder.add_page(width, height, |p| {
            for at in &draw {
                let line = keyed[*at].1;
                p.tagged_keyed(b"P", *at as u64 + 1, *at as u64, |p| {
                    for (x, y, size, word) in words(line) {
                        p.text(b"F1", size, x, y, &word);
                    }
                });
            }
        });
        let redrawn = open(builder.finish());
        let scored = Scored::read(&redrawn, 0).expect("tagged");
        let (stream, inferred) = (scored.stream_agreement(), scored.inferred_agreement());
        println!(
            "page {index}: stream {:.4} inferred {:.4}, {} columns",
            stream.score(),
            inferred.score(),
            scored.inferred.columns
        );
        assert!(
            inferred.at_least(1, 1),
            "page {index}: inferred {}",
            inferred.score()
        );
        if scored.inferred.columns == 2 {
            assert!(
                !stream.at_least(9, 10),
                "page {index}: the stream is already right"
            );
        }
        stream_total = stream_total.plus(stream);
        inferred_total = inferred_total.plus(inferred);
    }
    println!(
        "redrawn book: stream {:.4} inferred {:.4} ({} pairs)",
        stream_total.score(),
        inferred_total.score(),
        stream_total.pairs
    );
    assert!(!stream_total.at_least(9, 10));
}

/// A line's words, each with the origin and size of its first character.
fn words(line: &tinker_pdf::TextLine) -> Vec<(f64, f64, f64, String)> {
    let mut out: Vec<(f64, f64, f64, String)> = Vec::new();
    let mut current: Option<(f64, f64, f64, String)> = None;
    for c in &line.chars {
        if c.text.trim().is_empty() {
            out.extend(current.take());
            continue;
        }
        match current.as_mut() {
            Some(word) => word.3.push_str(&c.text),
            None => current = Some((c.origin.0, c.origin.1, c.size, c.text.clone())),
        }
    }
    out.extend(current);
    out
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
