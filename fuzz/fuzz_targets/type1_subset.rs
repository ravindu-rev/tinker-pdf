//! Type 1 subsetting: a *writer* whose only input is an attacker's font
//! program.
//!
//! [`type1.rs`](type1.rs) fuzzes the reader. The subsetter reads the same
//! eexec section and the same `/Subrs` and `/CharStrings` entries, and then
//! **writes a program back** — charstrings deleted, subroutines rewritten in
//! place as `return`, both cipher layers applied again, a trailer carried —
//! which is arithmetic on spans the file supplied, in a code path the reader
//! never enters.
//!
//! So the assertion is not "it did not panic". Whatever comes out has three
//! lengths that add up to it, parses again, holds every glyph that was asked
//! for under its own name, names as many glyphs as it holds, and comes out the
//! same twice (ruling 4).
//!
//! # What this target cannot find, and what covers it instead
//!
//! Every assertion above is **structural**, and the reader that re-parses the
//! output is this repository's own: a misreading of the Type 1 format the
//! writer and the reader share would round-trip and draw the wrong glyph
//! everywhere else. The subsetter's own check — every kept glyph outlined and
//! advanced in both programs — is the same reader again.
//! `crates/tinker-pdf-font/src/type1_subset/tests.rs` holds the cut against
//! programs whose every byte the tests wrote, and chapter 7's own worked
//! cipher example is in `type1.rs`'s tests.
#![no_main]
use libfuzzer_sys::fuzz_target;

use std::collections::BTreeSet;

use tinker_pdf_font::{subset_type1, Type1};

fuzz_target!(|data: &[u8]| {
    let Some(original) = Type1::parse(data) else {
        return;
    };
    let count = original.glyph_count();
    if count == 0 {
        return;
    }

    // Four selections, as `cff_subset`'s: the first glyph alone, the first and
    // the last, a sparse spread, and every glyph up to 256 — the case where
    // nothing is left to remove.
    let last = (count - 1).min(usize::from(u16::MAX)) as u16;
    let selections: [BTreeSet<u16>; 4] = [
        BTreeSet::from([0]),
        BTreeSet::from([0, last]),
        (0..count.min(64) as u16).filter(|g| g % 3 == 0).collect(),
        (0..count.min(256) as u16).collect(),
    ];

    for glyphs in selections {
        let Ok(out) = subset_type1(data, &glyphs) else {
            // Declining is always allowed (ruling 2): the caller embeds the
            // whole program and says so. Producing something unreadable is
            // not.
            continue;
        };
        assert_eq!(
            out.lengths.iter().sum::<usize>(),
            out.program.len(),
            "Table 125's three lengths are the program"
        );
        let Some(subset) = Type1::parse(&out.program) else {
            panic!(
                "the subsetter wrote {} bytes its own reader refuses",
                out.program.len()
            );
        };
        assert_eq!(
            out.names.len(),
            subset.glyph_count(),
            "the names it reports are the glyphs it holds"
        );
        for glyph in &glyphs {
            if usize::from(*glyph) >= count {
                continue;
            }
            let Some(name) = original.glyph_name(usize::from(*glyph)) else {
                continue;
            };
            let Some(found) = subset.glyph_for_name(name) else {
                panic!("glyph {glyph} was asked for and is not in the subset");
            };
            let _ = subset.outline(found);
            let _ = subset.advance(found);
        }

        let again = subset_type1(data, &glyphs).expect("it succeeded once");
        assert!(
            again == out,
            "the same subset twice produced different bytes"
        );
    }
});
