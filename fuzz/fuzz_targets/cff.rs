//! CFF: the tables that decide *which* charstring runs, and then the
//! charstring itself.
//!
//! Two halves, and they fail differently. The charset, the encoding, the
//! string INDEX and FDSelect are offsets and counts the file supplies, read
//! before a single outline operator executes -- a run that only asked for
//! outlines would never touch them, which is why this target asks a font what
//! its glyphs are called and which glyph a code, a name and a CID select. The
//! charstrings underneath are subroutine recursion, `seac`, `flex`, and the
//! hintmask whose length depends on how many stems were declared before it:
//! the operator most often counted wrongly.
//!
//! # What this target checks, and what it does not
//!
//! **That the code did not panic, hang, or exhaust memory, and that a reused
//! machine draws what a fresh one does.** Every glyph is read twice, through
//! `Cff::outline`'s fresh stack and through one `Machine` kept for the whole
//! font, and the two outlines must be equal: that holds the reused path to
//! the fresh one on whatever a mutator writes, a charstring that leaves its
//! operands on the stack included. Every other table walk and charstring run
//! below is discarded with `let _ =`, so an outline built from the wrong
//! offsets, a charset read one glyph out, or an FDSelect that picks the wrong
//! private dictionary all pass. So a run that returned the *wrong* answer
//! passes this target exactly as a correct one does, and a green `cargo fuzz`
//! here is evidence about ruling 1 and the reused machine, and about nothing
//! else.
//!
//! That is worth writing down rather than leaving implied. Correctness lives
//! in `crates/tinker-pdf-font`'s own tests and in the `cff_fonts.rs` suite
//! under `crates/tinker-pdf/tests/`, which compare rendered glyphs rather
//! than merely running the interpreter.
//!
//! Recorded because the same shape has already cost this repository once: the
//! `brotli` target asserts only self-consistency and could not have found the
//! ring-buffer defect that a decoded-bytes comparison found immediately.
#![no_main]
use libfuzzer_sys::fuzz_target;

use tinker_pdf_font::cff::Machine;
use tinker_pdf_font::{Cff, Outline};

fuzz_target!(|data: &[u8]| {
    let Some(cff) = Cff::parse(data) else {
        return;
    };
    let count = cff.glyph_count().min(64) as u16;

    // One machine and one outline for every glyph, as a caller reading a
    // whole face keeps them: the one assertion here beyond not panicking.
    // Each glyph through them draws what a fresh pair draws, so nothing a
    // hostile charstring leaves on the stack reaches the glyph after it.
    let mut machine = Machine::new();
    let mut reused = Outline::default();
    for glyph in 0..count {
        let fresh = cff.outline(glyph);
        let drawn = cff.outline_into(glyph, &mut reused, &mut machine);
        assert_eq!(drawn, fresh.is_some(), "glyph {glyph}: one path refused it");
        if let Some(fresh) = fresh {
            assert!(
                fresh.segments == reused.segments,
                "glyph {glyph}: the reused machine drew otherwise"
            );
        }
        let _ = cff.advance(glyph);
        // A CID-keyed font may carry a matrix per Font DICT, so this reads
        // FDSelect and the FDArray for every glyph.
        let _ = cff.font_matrix_for(glyph);

        // The charset, and the string INDEX behind it. Feeding the name back
        // in is what reaches the *resolving* half with something the font
        // actually holds: arbitrary bytes almost never name a real glyph, so
        // a target that only looked names up from the input would exercise
        // the miss path and call it coverage.
        if let Some(name) = cff.glyph_name(glyph) {
            let _ = cff.gid_for_name(name);
        }
    }

    // Every code, because the encoding is a 256-entry table however short the
    // one in the file is, and a supplement can put a glyph at any of them.
    for code in 0..=u8::MAX {
        let _ = cff.gid_for_code(code);
    }

    // The reverse charset. The CIDs tried are the ones the font declares --
    // taken from its own glyphs -- plus the edges, where `u16` runs out and
    // where a truncating cast would have found a glyph that is not there.
    for glyph in 0..count {
        let _ = cff.gid_for_cid(u32::from(glyph));
    }
    for cid in [0, 1, 0xFFFF, 0x1_0000, u32::MAX] {
        let _ = cff.gid_for_cid(cid);
    }

    // A name from the input as well, for the paths a hit never reaches: the
    // string INDEX walked to its end, and the standard strings searched in
    // full.
    if let Ok(name) = core::str::from_utf8(&data[..data.len().min(64)]) {
        let _ = cff.gid_for_name(name);
    }
    let _ = cff.is_cid();

    // The Top DICT's and the Private DICT's style entries the font
    // measurement API reads, `Weight` through the string INDEX and a
    // CID-keyed font's first Font DICT's Private DICT.
    let _ = cff.style();
});
