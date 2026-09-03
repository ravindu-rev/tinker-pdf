//! The shaper's own entry points: arbitrary text, resolved and shaped against
//! an arbitrary face.
//!
//! The repository's **twenty-sixth** target, and the second on
//! `tinker-pdf-shape`. `shape.rs` covers milestone 1 of
//! `docs/design/shaping.md` — `Layout`, `Buffer`, and a lookup list executed
//! against glyph indices a caller supplied. It does not reach milestones 2 to
//! 4, which is the larger untrusted surface of the two and the one a consumer
//! actually calls: `Paragraph::new` runs UAX #9 over text this engine did not
//! write, `itemize` cuts it into runs, and `Shaper::shape_text` drives `GSUB`
//! and `GPOS` from a face this engine did not write either.
//!
//! It is a second target rather than more of the first because the *input*
//! wants a different shape. `shape.rs` wants glyph indices and table bytes and
//! nothing else, and a mutator that found a good table directory should keep
//! exploring behind it; this one wants a string, and a corpus that mixed the
//! two would spend half its budget feeding text to a target that has no text
//! in it.
//!
//! ## What the input is
//!
//! One control byte, a text length, that many bytes of text, and the rest as a
//! face. The text is taken lossily as UTF-8 rather than rejected, so no input
//! is wasted: an invalid sequence becomes `U+FFFD`, which is a character a
//! reader can type and a perfectly good thing to shape.
//!
//! ## What is asserted beyond "it did not panic"
//!
//! - **Determinism**, which is ruling 4 asserted on an algorithm rather than
//!   on a bitmap. The same text and the same face resolve to the same levels
//!   and shape to the same glyphs, twice, every time. `shape.rs` asserts this
//!   for the `Layout` path; this asserts it for the path that has bidi,
//!   itemization, joining forms and feature masks in front of it.
//! - **Reordering is a permutation.** `reorder` returning anything that is not
//!   each of `0..n` exactly once is a defect that silently drops or duplicates
//!   a glyph on the page, and it is two lines to rule out. The same is
//!   asserted of a line's visual order, which is the L rules rather than L2
//!   alone and can legitimately be *shorter* — X9 removes characters — but
//!   still may not repeat one or invent one.
//! - **Levels are in range.** UAX #9's `max_depth` is 125 and rule X1 says an
//!   embedding past it overflows rather than nesting, so a level above 126 is
//!   a resolver that stopped counting.
//! - **Itemization tiles the text exactly**: the runs are in order, they do
//!   not overlap, they leave no gap, they start and end on character
//!   boundaries, and together they are the whole paragraph. A run boundary
//!   inside a code point would panic a consumer that sliced on it, and a gap
//!   would be text that never reached a glyph.
//! - **Every cluster points into its own run**, and clusters never go
//!   backwards. This is what milestone 7's `/ToUnicode` is built on: after a
//!   ligature there is nothing left but the cluster to say which characters
//!   went into it.

//! # What this target cannot find, and what covers it instead
//!
//! Every assertion above is **structural**: a paragraph resolves as many
//! characters as it was given, no embedding level passes X1's maximum,
//! reordering is a permutation, and a visual order names only characters the
//! paragraph has. None of them asks whether the numbers are the ones the
//! input actually describes, so a decode that is well-formed and *wrong*
//! passes this target exactly as a correct one does.
//!
//! Those are strong invariants and still not correctness: a bidi
//! implementation that resolved every level to the wrong *value* satisfies
//! all four. Correctness lives in `crates/tinker-pdf-
//! shape/tests/bidi_conformance.rs`, which runs the Unicode conformance data.
//!
//! This is the shape `brotli` records at length, and the difference is worth
//! keeping in view: there, nothing in the tree can supply the missing check
//! at all — rule 1 leaves no encoder to round-trip against and ruling 13 bars
//! a second decoder. Here the check exists, and it is somewhere else.
//!
#![no_main]
use libfuzzer_sys::fuzz_target;

use tinker_pdf_font::Sfnt;
use tinker_pdf_shape::bidi::{self, BaseDirection, Paragraph};
use tinker_pdf_shape::shape::{itemize, Shaper};
use tinker_pdf_shape::{Limits, ShapedGlyph};

/// The three base directions, so P2/P3's own resolution and the two
/// overrides are all on the path an ordinary input takes.
const DIRECTIONS: [BaseDirection; 3] = [
    BaseDirection::Auto,
    BaseDirection::LeftToRight,
    BaseDirection::RightToLeft,
];

/// Rule X1's `max_depth` is 125, and a level of 126 is reachable because an
/// isolate initiator is counted at the level *outside* the one it opens.
const DEEPEST: u8 = 126;

/// Whether `order` uses each of `0..len` exactly once.
fn is_permutation(order: &[usize], len: usize) -> bool {
    if order.len() != len {
        return false;
    }
    let mut seen = vec![false; len];
    for at in order {
        match seen.get_mut(*at) {
            Some(slot) if !*slot => *slot = true,
            _ => return false,
        }
    }
    true
}

/// The properties a caller of `bidi` is entitled to rely on.
fn check_paragraph(text: &str, paragraph: &Paragraph) {
    let count = text.chars().count();
    assert_eq!(
        paragraph.levels().len(),
        count,
        "a paragraph resolved a different number of characters than it was given"
    );
    for level in paragraph.levels() {
        assert!(
            level.number() <= DEEPEST,
            "an embedding level past X1's max_depth: {}",
            level.number()
        );
    }

    let order = bidi::reorder(paragraph.levels());
    assert!(
        is_permutation(&order, count),
        "reorder did not return a permutation: a glyph would be dropped or drawn twice"
    );

    let line = paragraph.line(0..paragraph.len());
    assert_eq!(line.levels().len(), count, "a whole-paragraph line lost characters");
    // The visual order of a *line* may be shorter than the line, because X9
    // removes the embedding initiators and `PDF` — but every index in it is
    // still a distinct character of the paragraph.
    let mut seen = vec![false; count];
    for at in line.visual_order() {
        let slot = seen
            .get_mut(*at)
            .expect("a visual order named a character the paragraph does not have");
        assert!(!*slot, "a visual order named one character twice");
        *slot = true;
    }
}

/// The properties a caller of `itemize` is entitled to rely on.
fn check_runs(text: &str, runs: &[tinker_pdf_shape::shape::Run]) {
    if text.is_empty() {
        assert!(runs.is_empty(), "empty text produced runs");
        return;
    }
    assert!(!runs.is_empty(), "text produced no runs at all");
    let mut expected = 0usize;
    for run in runs {
        assert_eq!(
            run.text.start, expected,
            "itemization left a gap or overlapped: {:?}",
            run.text
        );
        assert!(run.text.start < run.text.end, "an empty run");
        assert!(
            text.is_char_boundary(run.text.start) && text.is_char_boundary(run.text.end),
            "a run boundary inside a code point: {:?}",
            run.text
        );
        expected = run.text.end;
    }
    assert_eq!(expected, text.len(), "itemization did not reach the end");
}

/// The properties a caller of `Shaper` is entitled to rely on.
fn check_glyphs(run: &tinker_pdf_shape::shape::Run, glyphs: &[ShapedGlyph], limits: Limits) {
    assert!(
        glyphs.len() <= limits.max_glyphs.max(1),
        "the glyph ceiling was exceeded rather than refused"
    );
    let mut previous = None::<u32>;
    for glyph in glyphs {
        if let Some(previous) = previous {
            assert!(
                glyph.cluster >= previous,
                "clusters went backwards; nothing in this crate reorders a run"
            );
        }
        previous = Some(glyph.cluster);
        let at = usize::try_from(glyph.cluster).expect("a cluster is a byte offset");
        assert!(
            run.text.contains(&at),
            "a cluster outside the run it came from: {at} not in {:?}",
            run.text
        );
    }
}

/// Shapes one paragraph and returns everything that came out, so the whole of
/// it can be compared against a second run.
fn shape_all(
    face: &Sfnt<'_>,
    text: &str,
    direction: BaseDirection,
    limits: Limits,
) -> Vec<(u8, Vec<ShapedGlyph>)> {
    let shaper = Shaper::new(face).with_limits(limits);
    let paragraph = Paragraph::new(text, direction);
    check_paragraph(text, &paragraph);

    let runs = itemize(text, &paragraph);
    check_runs(text, &runs);

    let mut out = Vec::new();
    for run in &runs {
        let shaped = shaper.shape(text, run);
        check_glyphs(run, shaped.glyphs(), limits);
        assert_eq!(
            shaped.text(),
            run.text,
            "a shaped run reported a different range than it was asked for"
        );
        out.push((run.level.number(), shaped.glyphs().to_vec()));
    }
    out
}

fuzz_target!(|data: &[u8]| {
    let (control, rest) = data.split_at(data.len().min(2));
    let knobs = control.first().copied().unwrap_or(0);
    let length = usize::from(control.get(1).copied().unwrap_or(0));

    let (text, face) = rest.split_at(length.min(rest.len()));
    let text = String::from_utf8_lossy(text);

    // Small ceilings, for the reason `shape.rs` records at length: a target
    // whose budgets are all at their shipped defaults never explores a
    // refusal, because nothing a mutator builds crosses a 500 000-operation
    // budget inside one iteration.
    let limits = Limits {
        max_nesting_depth: u32::from(knobs & 3),
        max_extension_depth: u32::from((knobs >> 2) & 3),
        max_context_length: match (knobs >> 4) & 3 {
            0 => 1,
            1 => 4,
            2 => 64,
            _ => 4_096,
        },
        max_operations: match (knobs >> 6) & 1 {
            0 => 64,
            _ => 500_000,
        },
        max_glyphs: match (knobs >> 7) & 1 {
            0 => 8,
            _ => 65_536,
        },
    };

    // Bidi and itemization do not need a face at all, and most inputs will
    // not carry a readable one, so they are driven whether or not one parses.
    for direction in DIRECTIONS {
        let paragraph = Paragraph::new(&text, direction);
        check_paragraph(&text, &paragraph);
        check_runs(&text, &itemize(&text, &paragraph));
        let again = Paragraph::new(&text, direction);
        assert!(
            paragraph.levels() == again.levels(),
            "level resolution is not deterministic"
        );
    }

    let Some(sfnt) = Sfnt::parse(face) else {
        return;
    };
    for direction in DIRECTIONS {
        let once = shape_all(&sfnt, &text, direction, limits);
        let twice = shape_all(&sfnt, &text, direction, limits);
        assert!(once == twice, "shaping is not deterministic");
    }
});
