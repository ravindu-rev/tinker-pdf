//! CFF subsetting: a *writer* whose only input is an attacker's font program.
//!
//! [`cff.rs`](cff.rs) fuzzes the reader, and a reader that survives hostile
//! input is not the same claim. The subsetter reads the same INDEXes, DICTs,
//! charsets and FDSelect the reader does, and then **writes them back out** —
//! new INDEXes, new offsets, renumbered subroutine calls around the 107 /
//! 1131 / 32768 bias thresholds. Every one of those is arithmetic on numbers
//! the file supplied, in a code path the reader never enters.
//!
//! Three failures are specific to this direction:
//!
//! - **Offset arithmetic that only the writer performs.** An INDEX writer
//!   chooses its own `offSize` from the total data length; a program that
//!   drives that length to a boundary is a program the reader never had to
//!   think about.
//! - **Renumbering across a bias threshold.** A subroutine INDEX of 1 239
//!   entries and one of 1 240 are biased differently, so a subset that moves
//!   a font across the boundary must rewrite every call. Getting it wrong
//!   produces a *valid* font that draws the wrong glyphs — which is why the
//!   check below re-parses the output rather than only asking that it exists.
//! - **The output must be readable.** A subsetter that emits bytes this
//!   repository's own `Cff::parse` refuses has produced a document no
//!   consumer can use, and nothing else in the pipeline would notice: the
//!   embed path takes the bytes and writes them into a `/FontFile3`.
//!
//! So the assertion is not "it did not panic". It is that whatever comes out
//! parses, and that every glyph asked for is still there — because a subset
//! that silently dropped the glyphs a page draws is the defect that reaches a
//! reader as a blank page.

//! # What this target cannot find, and what covers it instead
//!
//! Every assertion above is **structural**: whatever the subsetter emits
//! parses again, every glyph asked for is still at the id it was asked for,
//! and subsetting twice gives the same bytes. None of them asks whether the
//! result is the one the input describes, so an answer that is well-formed
//! and *wrong* passes exactly as a correct one does.
//!
//! Those are unusually strong for a fuzz target — this is the one target here
//! that fuzzes a *writer*, and a round trip through a parser is real
//! evidence. The residual is that the parser is **this repository's own**: a
//! misreading of the CFF specification shared by the writer and the reader
//! would round-trip perfectly and produce a font that draws the wrong glyphs
//! everywhere else. `crates/tinker-pdf/tests/cff_subset_census.rs` is what
//! covers that, by checking a rebuilt face against what a conformant consumer
//! accepts.
//!
#![no_main]
use libfuzzer_sys::fuzz_target;

use std::collections::BTreeSet;

use tinker_pdf_font::Cff;

fuzz_target!(|data: &[u8]| {
    let Some(original) = Cff::parse(data) else {
        return;
    };
    let count = original.glyph_count();
    if count == 0 {
        return;
    }

    // Four selections, because which glyphs are kept decides which code paths
    // run: the first alone exercises the notdef-only edge, a sparse spread
    // exercises charset and FDSelect rewriting across Font DICTs, and the
    // whole set is the case where the subsetter may legitimately decline to
    // shrink anything.
    let last = (count - 1).min(usize::from(u16::MAX)) as u16;
    let selections: [BTreeSet<u16>; 4] = [
        BTreeSet::from([0]),
        BTreeSet::from([0, last]),
        (0..count.min(64) as u16).filter(|g| g % 3 == 0).collect(),
        (0..count.min(256) as u16).collect(),
    ];

    for glyphs in selections {
        let Some(out) = tinker_pdf_font::subset(data, &glyphs) else {
            // Declining is always allowed (ruling 2) — the caller embeds the
            // whole face and says so. Producing something unreadable is not.
            continue;
        };

        let Some(subset) = Cff::parse(&out) else {
            panic!("the subsetter wrote {} bytes its own reader refuses", out.len());
        };

        // Glyph ids never move: that invariant is why `/Widths`, `/W`,
        // `/CIDToGIDMap` and `/ToUnicode` need no rewriting, and it is worth
        // more than the size reduction. A subset whose ids shifted would draw
        // the wrong glyphs everywhere with no error anywhere.
        for glyph in &glyphs {
            if usize::from(*glyph) >= count {
                continue;
            }
            assert!(
                usize::from(*glyph) < subset.glyph_count(),
                "glyph {glyph} was asked for and is past the end of the subset"
            );
            // Reading it is the point: a charstring whose subroutine calls
            // were renumbered wrongly parses as a charstring and executes as
            // nonsense, so the outline has to be asked for rather than
            // assumed.
            let _ = subset.outline(*glyph);
            let _ = subset.advance(*glyph);
            let _ = subset.font_matrix_for(*glyph);
        }

        // Determinism (ruling 4): the same program and the same selection
        // produce the same bytes. A subsetter whose output depended on hash
        // iteration order would make every document containing it
        // irreproducible, and the fingerprints would catch it only for the
        // faces this repository happens to test.
        let again = tinker_pdf_font::subset(data, &glyphs).expect("it succeeded once");
        assert!(again == out, "the same subset twice produced different bytes");
    }
});
