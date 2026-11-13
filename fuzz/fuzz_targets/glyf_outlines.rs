//! TrueType outlines, generated: a minimal `sfnt` of simple and composite
//! glyphs, every glyph's outline read and then read again from a subset.
//!
//! `truetype` and `sfnt` hand mutated bytes to the reader and the subsetter,
//! and a mutation lands in the table directory, `loca` or a glyph's header far
//! more often than in its points, where it stops at a bounds check. This
//! target reads its input as choices (`tinker_pdf_fuzz::Choices`) and writes a
//! well-formed face with `tinker_pdf_fuzz::glyf`: the table directory and
//! `head`, `hhea`, `hmtx`, `maxp`, a short or long `loca`, and `glyf`. A
//! simple glyph is contours of on- and off-curve points whose flags take every
//! form the `glyf` table defines — a run compressed with `REPEAT_FLAG`, a short
//! coordinate with its sign in the other bit, a coordinate the same as the
//! last, and a long one — with or without instructions, and `OVERLAP_SIMPLE`.
//! A composite takes every component form: arguments as words or bytes, as
//! offsets (rounded to the grid or not) or as points to match, a scale, an x
//! and y scale or a two-by-two, its offset scaled or not, `USE_MY_METRICS`,
//! `OVERLAP_COMPOUND`, instructions after the last component, and components
//! that are composites themselves. Some glyphs have no outline at all.
//!
//! The caps — sixteen glyphs, four contours of eight points, three components
//! a composite and four levels of nesting, inside `glyf::outline`'s eight
//! levels and 256 components — are `tinker_pdf_fuzz::glyf`'s constants.
//!
//! What is asserted beyond "it did not panic":
//!
//! - **Every glyph is read whole.** `Sfnt::parse` reads the face, and every
//!   glyph's outline holds no more segments than `Outline`'s cap, with every
//!   coordinate finite, and draws one closed contour for each contour it holds,
//!   its components' included: a flag run read one point short, or a
//!   coordinate read in the wrong width, ends or starts a contour in the wrong
//!   place.
//! - **Each glyph's advance is the one `hmtx` states**: its own, or the last
//!   one past `numberOfHMetrics`.
//! - **A subset draws what the face drew.** For every glyph, and for a
//!   selection the input chooses, `tinker_pdf_font::subset` declines (ruling
//!   2) or writes a face `Sfnt::parse` reads, and each glyph asked for draws
//!   the same outline, segment for segment, with the same advance — glyph id
//!   for glyph id, since the subset keeps every id, and with every component
//!   of a kept composite kept too.
//!
//! # What this target cannot find, and what covers it instead
//!
//! Every assertion holds the reader to a generator and a subsetter written in
//! this repository, so a misreading of the `glyf` table all three share
//! round-trips perfectly here. Point-matched components are placed with no
//! offset by the reader (`glyf.rs`), which this target cannot tell from the
//! right placement either. The rendered glyph comparisons and the corpus's
//! embedded faces hold the reader to fonts other producers wrote
//! (`docs/features/fonts.md`, *Verified*).
#![no_main]
use libfuzzer_sys::fuzz_target;

use std::collections::BTreeSet;

use tinker_pdf_font::{glyf, Segment, Sfnt};
use tinker_pdf_fuzz::glyf::generate;
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
    let face = generate(&mut c);
    let glyphs = face.advances.len();
    let sfnt = Sfnt::parse(&face.bytes).unwrap_or_else(|| panic!("a generated face did not parse"));

    let mut drawn = Vec::with_capacity(glyphs);
    for (g, &advance) in face.advances.iter().enumerate() {
        let id = g as u16;
        let outline =
            glyf::outline(&sfnt, id).unwrap_or_else(|| panic!("glyph {g} has no outline"));
        assert!(
            outline.segments.len() <= MAX_SEGMENTS,
            "glyph {g} passed the segment cap"
        );
        assert!(
            outline.segments.iter().all(finite),
            "glyph {g} has a coordinate that is not finite"
        );
        let moves = outline
            .segments
            .iter()
            .filter(|s| matches!(s, Segment::MoveTo { .. }))
            .count();
        let closes = outline
            .segments
            .iter()
            .filter(|s| matches!(s, Segment::Close))
            .count();
        assert_eq!(
            (moves, closes),
            (face.contours[g], face.contours[g]),
            "glyph {g} draws another number of contours than it holds"
        );
        assert_eq!(
            sfnt.advance(id),
            Some(advance),
            "glyph {g} reads an advance hmtx does not state"
        );
        drawn.push(outline.segments);
    }

    // Every glyph, and a selection the input chooses.
    let chosen: BTreeSet<u16> = (0..glyphs as u16).filter(|_| c.flag()).collect();
    let selections: [BTreeSet<u16>; 2] = [(0..glyphs as u16).collect(), chosen];
    for selection in &selections {
        let Some(out) = tinker_pdf_font::subset(&face.bytes, selection) else {
            continue;
        };
        let subset = Sfnt::parse(&out)
            .unwrap_or_else(|| panic!("the subset of {selection:?} does not parse"));
        for &g in selection {
            let outline = glyf::outline(&subset, g)
                .unwrap_or_else(|| panic!("glyph {g} lost its outline in a subset"));
            assert!(
                outline.segments == drawn[usize::from(g)],
                "glyph {g} draws otherwise after a subset of {selection:?}"
            );
            assert_eq!(
                subset.advance(g),
                Some(face.advances[usize::from(g)]),
                "glyph {g} reads another advance after a subset of {selection:?}"
            );
        }
    }
});
