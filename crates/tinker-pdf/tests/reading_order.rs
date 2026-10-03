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
