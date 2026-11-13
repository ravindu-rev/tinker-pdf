//! Type 2 charstrings, generated: a bare CFF built from generated INDEXes and
//! DICTs, every glyph's outline read and then read again from a subset.
//!
//! `cff` and `cff_subset` hand mutated bytes to the reader and the subsetter,
//! and a mutation lands in an INDEX's offsets or a DICT's operands far more
//! often than in a charstring, where it stops at a refusal. This target reads
//! its input as choices (`tinker_pdf_fuzz::Choices`) and writes a well-formed
//! program (Adobe Technical Notes 5176 and 5177): the header, the Name, Top
//! DICT, String and Global Subr INDEXes, a charset in format 0, 1 or 2, the
//! CharStrings INDEX, and a Private DICT with its local Subrs — or a CID-keyed
//! font, its `ROS`, an FDArray of Font DICTs each with its own Private DICT
//! and Subrs, and an FDSelect in format 0 or 3. Each subroutine INDEX holds a
//! count either side of a bias edge — 1 239 or 1 240, 33 899 or 33 900 — or a
//! small one, so a call is biased by 107, 1 131 or 32 768, and every INDEX's
//! offsets take a width the generator chooses.
//!
//! Each charstring is drawn from every Type 2 operator: a width or none; the
//! stem hints, with `hintmask` and `cntrmask` followed by the mask bytes the
//! stems so far call for, and implicit `vstem`s before the first mask; the
//! moves, lines and curves in each argument count the operators take; the
//! four flex operators; `callsubr` and `callgsubr` whose biased operand names
//! a subroutine the INDEX holds, the subroutines calling each other without
//! a cycle; `return`; and `endchar`, alone or with the four operands of
//! `seac`. Operands take each of 5177's encodings: one byte, two bytes, the
//! 16-bit `28` and the 16.16 `255`. Every program stays inside the
//! interpreter's caps — ten levels of subroutine, its operation budget, and
//! the outline's segment cap — so each outline is the whole program's.
//!
//! A glyph is sometimes its `endchar` alone — a space, or an accented letter
//! `seac` makes — so the width is read off `endchar` as well as off the
//! operators before it. The generator is `tinker_pdf_fuzz::cff`, and its
//! caps — twelve glyphs, twenty-four operators a program, six subroutines
//! with content in an INDEX, the rest a bare `return`, an edge count one
//! INDEX in four — are its constants.
//!
//! What is asserted beyond "it did not panic":
//!
//! - **The program parses and every glyph is read.** `Cff::parse` reads what
//!   was written, with the generated glyph count, and every glyph's outline
//!   holds no more segments than `Outline`'s cap with every coordinate finite.
//! - **Each glyph's advance is the width it was written with**: the Private
//!   DICT's nominal width plus the glyph's width operand, or its default
//!   width where the glyph has none (5177, 4.4) — `endchar`'s four `seac`
//!   operands included, which carry no width.
//! - **A subset draws what the font drew.** For every glyph, and for a
//!   selection the input chooses, `tinker_pdf_font::subset` declines (ruling
//!   2) or writes a program `Cff::parse` reads with every glyph at its own id,
//!   and each glyph asked for draws the same outline, segment for segment,
//!   with the same advance. Subroutines are renumbered under the subset's own
//!   bias, so a rewritten call that names the wrong subroutine fails here.
//!
//! # What this target cannot find, and what covers it instead
//!
//! Every assertion holds the reader to a generator and a subsetter written in
//! this repository: an encoding of 5176 or 5177 that all three misread
//! round-trips perfectly here. The rendered glyph comparisons
//! (`crates/tinker-pdf/tests/cff_fonts.rs`) and the subset census over real
//! faces (`crates/tinker-pdf/tests/cff_subset_census.rs`) hold them to fonts
//! other producers wrote. `seac`'s components are kept by the subset and are
//! not drawn by the Type 2 reader, so their composition is outside this
//! target too.
#![no_main]
use libfuzzer_sys::fuzz_target;

use std::collections::BTreeSet;

use tinker_pdf_font::{Cff, Segment};
use tinker_pdf_fuzz::cff::generate;
use tinker_pdf_fuzz::Choices;

/// `Outline`'s segment cap (`crates/tinker-pdf-font/src/outline.rs`).
const MAX_SEGMENTS: usize = 1 << 16;

fn finite(segment: &Segment) -> bool {
    match *segment {
        Segment::MoveTo { x, y } | Segment::LineTo { x, y } => x.is_finite() && y.is_finite(),
        Segment::QuadTo { cx, cy, x, y } => [cx, cy, x, y].iter().all(|v| v.is_finite()),
        Segment::CurveTo {
            c1x,
            c1y,
            c2x,
            c2y,
            x,
            y,
        } => [c1x, c1y, c2x, c2y, x, y].iter().all(|v| v.is_finite()),
        Segment::Close => true,
    }
}

fuzz_target!(|data: &[u8]| {
    let mut c = Choices::new(data);
    let font = generate(&mut c);
    let glyphs = font.advances.len();
    let cff = Cff::parse(&font.bytes).unwrap_or_else(|| panic!("a generated CFF did not parse"));
    assert_eq!(
        cff.glyph_count(),
        glyphs,
        "the CharStrings INDEX read otherwise"
    );

    let mut drawn = Vec::with_capacity(glyphs);
    for (g, &advance) in font.advances.iter().enumerate() {
        let id = g as u16;
        let outline = cff
            .outline(id)
            .unwrap_or_else(|| panic!("glyph {g} has no outline"));
        assert!(
            outline.segments.len() <= MAX_SEGMENTS,
            "glyph {g} passed the segment cap"
        );
        assert!(
            outline.segments.iter().all(finite),
            "glyph {g} has a coordinate that is not finite"
        );
        let read = cff
            .advance(id)
            .unwrap_or_else(|| panic!("glyph {g} has no advance"));
        assert_eq!(
            read, advance,
            "glyph {g} reads an advance it was not written with"
        );
        drawn.push((outline.segments, read));
    }

    // Every glyph, and a selection the input chooses: the subsetter is most
    // of an iteration's work, so two subsets an input rather than more, and
    // mutation of the choice reaches the sparse and the `.notdef`-only ones.
    let chosen: BTreeSet<u16> = (0..glyphs as u16).filter(|_| c.flag()).collect();
    let selections: [BTreeSet<u16>; 2] = [(0..glyphs as u16).collect(), chosen];
    for selection in &selections {
        let Some(out) = tinker_pdf_font::subset(&font.bytes, selection) else {
            continue;
        };
        let subset = Cff::parse(&out)
            .unwrap_or_else(|| panic!("the subset of {selection:?} does not parse"));
        assert_eq!(subset.glyph_count(), glyphs, "the subset moved glyph ids");
        for &g in selection {
            let outline = subset
                .outline(g)
                .unwrap_or_else(|| panic!("glyph {g} lost its outline in a subset"));
            let (segments, advance) = &drawn[usize::from(g)];
            assert!(
                &outline.segments == segments,
                "glyph {g} draws otherwise after a subset of {selection:?}"
            );
            assert_eq!(
                subset.advance(g),
                Some(*advance),
                "glyph {g} reads another advance after a subset of {selection:?}"
            );
        }
    }
});
