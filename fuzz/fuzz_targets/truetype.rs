//! TrueType: the table directory, `cmap` in every format read, and `glyf`
//! outlines and points including composites, whose recursion is the
//! interesting part — and a variable face's: its design space, its outlines
//! and phantom points as `gvar` moves them, and its `HVAR`, `VVAR`, `MVAR`
//! and `cvt ` values, at its default, at each named instance and at its
//! design space's corners.
//!
//! # What this target checks, and what it does not
//!
//! **Only that the code did not panic, hang, or exhaust memory.** Every call
//! is discarded. A composite glyph assembled with the wrong component
//! transform, a `cmap` subtable read in the wrong format, or a delta scaled by
//! the wrong region produces wrong outlines, points and metrics rather than a
//! crash. So a run that returned the *wrong* answer passes this target
//! exactly as a correct one does, and a green `cargo fuzz` here is evidence
//! about ruling 1 and about nothing else.
//!
//! That is worth writing down rather than leaving implied. Correctness lives
//! in `crates/tinker-pdf-font`'s own tests — `variations.rs` for the variable
//! half — and in the rendered glyph comparisons.
//!
//! Recorded because the same shape has already cost this repository once: the
//! `brotli` target asserts only self-consistency and could not have found the
//! ring-buffer defect that a decoded-bytes comparison found immediately.
#![no_main]
use libfuzzer_sys::fuzz_target;

use tinker_pdf_font::variation::cvar::cvt_at;
use tinker_pdf_font::variation::gvar::Gvar;
use tinker_pdf_font::variation::metrics::{Hvar, Mvar, Vvar, METRIC_FIELDS};
use tinker_pdf_font::variation::DesignSpace;
use tinker_pdf_font::{glyf, Sfnt};
use tinker_pdf_fuzz::variation::locations;

/// The glyphs a variable face is outlined at every location: fewer than the
/// sixty-four read once, because each outline may spend
/// `MAX_GLYPH_DELTA_WORK` point deltas and a face is read at up to nineteen
/// locations.
const VARIED_GLYPHS: u16 = 16;

fuzz_target!(|data: &[u8]| {
    let Some(font) = Sfnt::parse(data) else {
        return;
    };

    for tag in [b"glyf", b"cmap", b"loca", b"head"] {
        let _ = font.table(u32::from_be_bytes(*tag));
    }
    for c in ['\0', ' ', 'A', '\u{20AC}', '\u{1F600}'] {
        let _ = font.glyph_for_char(c);
    }
    let _ = font.glyph_for_name("A");

    // Bounded rather than driven by the font's own glyph count, which an
    // input is free to claim is enormous.
    for glyph in 0..64u16 {
        let _ = font.advance(glyph);
        let _ = glyf::outline(&font, glyph);
        let _ = glyf::points(&font, glyph);
    }

    // A refused design space, or a face that does not vary, is drawn at its
    // default, which the loop above already drew.
    let Ok(space) = DesignSpace::read(&font) else {
        return;
    };
    if !space.is_variable() {
        return;
    }
    // A table refused here is drawn at the default by every caller, so the
    // refusal is the answer and the rest of the face is still read.
    let gvar = Gvar::read(&font, &space).ok().flatten();
    let hvar = Hvar::read(&font, &space).ok().flatten();
    let vvar = Vvar::read(&font, &space).ok().flatten();
    let mvar = Mvar::read(&font, &space).ok().flatten();
    for location in locations(&space, space.axis_count()) {
        let _ = cvt_at(&font, &space, &location);
        if let Some(mvar) = &mvar {
            for field in METRIC_FIELDS {
                let _ = mvar.value_at(&font, field.tag, &location);
            }
        }
        let horizontal = hvar.as_ref().map(|hvar| hvar.scalars(&location));
        let vertical = vvar.as_ref().map(|vvar| vvar.scalars(&location));
        for glyph in 0..VARIED_GLYPHS {
            let _ = glyf::outline_at(&font, glyph, gvar.as_ref(), &location);
            let _ = glyf::phantom_points_at(&font, glyph, gvar.as_ref(), &location);
            if let (Some(hvar), Some(scalars)) = (&hvar, &horizontal) {
                let _ = hvar.advance_at(&font, glyph, &location);
                let _ = hvar.left_side_bearing_delta(glyph, scalars);
                let _ = hvar.right_side_bearing_delta(glyph, scalars);
            }
            if let (Some(vvar), Some(scalars)) = (&vvar, &vertical) {
                let _ = vvar.advance_at(&font, glyph, &location);
                let _ = vvar.origin_at(&font, glyph, &location);
                let _ = vvar.top_side_bearing_delta(glyph, scalars);
                let _ = vvar.bottom_side_bearing_delta(glyph, scalars);
            }
        }
    }
});
