//! CFF2: the table's structures — its header, the Top DICT outside any
//! INDEX, the 32-bit INDEXes, the Font DICTs and their Private DICTs,
//! FDSelect in its three formats and the variation store behind them — and
//! then every glyph's charstring through the one Type 2 machine, with
//! `blend` and `vsindex`, at the default, at each named instance and at the
//! design space's corners, and every Private DICT's blended values there.
//!
//! The input is an sfnt carrying a `CFF2` table, whose `fvar` names the
//! instances and the axes, or a bare `CFF2` table, whose variation store's
//! axis count gives the corners. A mutation that breaks the directory still
//! reaches the table that way.
//!
//! # What this target checks, and what it does not
//!
//! **That the code did not panic, hang, or exhaust memory, and that a reused
//! machine draws what a fresh one does.** Every glyph is drawn twice at each
//! location — through `Cff2::outline_at`, which computes the blends and the
//! stack afresh, and through `Cff2::outline_with` over blends computed once
//! for the location and one `Machine` kept for the whole table — and the two
//! must refuse together or draw the same segments. Everything else is
//! discarded with `let _ =`, so a blend scaled by the wrong region, a Private
//! DICT read under the wrong `vsindex` or an FDSelect that picks the wrong
//! Font DICT all pass. So a run that returned the *wrong* answer passes this
//! target exactly as a correct one does, and a green `cargo fuzz` here is
//! evidence about ruling 1 and the reused machine, and about nothing else.
//!
//! Correctness lives in `crates/tinker-pdf-font/tests/variations.rs`, which
//! holds every blend to the deltas the specification's arithmetic gives in
//! the test.
#![no_main]
use libfuzzer_sys::fuzz_target;

use tinker_pdf_font::cff::Machine;
use tinker_pdf_font::cff2::Cff2;
use tinker_pdf_font::variation::{tag_value, DesignSpace};
use tinker_pdf_font::{Outline, Sfnt};
use tinker_pdf_fuzz::variation::locations;

fuzz_target!(|data: &[u8]| {
    let sfnt = Sfnt::parse(data);
    let table = sfnt
        .as_ref()
        .and_then(|font| font.table(tag_value(*b"CFF2")));
    let (bytes, space) = match (&sfnt, table) {
        // A design space this reader refuses draws the default, as every
        // caller draws it; the corners still come from the store.
        (Some(font), Some(table)) => (table, DesignSpace::read(font).unwrap_or_default()),
        _ => (data, DesignSpace::default()),
    };
    let Ok(cff2) = Cff2::parse(bytes) else {
        return;
    };
    let axes = if space.is_variable() {
        space.axis_count()
    } else {
        cff2.variation_store()
            .map_or(0, |store| usize::from(store.axis_count()))
    };

    // Bounded rather than driven by the table's own counts, which an input
    // is free to claim are enormous.
    let count = cff2.glyph_count().min(64) as u16;
    let font_dicts = cff2.font_dict_count().min(16) as u16;

    // The default reading, whose blends keep their defaults and read no
    // scalars at all.
    for glyph in 0..count {
        let _ = cff2.outline(glyph);
        let _ = cff2.font_dict_for(glyph);
        let _ = cff2.private_vsindex(glyph);
    }
    for font_dict in 0..font_dicts {
        let _ = cff2.private_dict(font_dict);
    }

    let mut machine = Machine::new();
    let mut reused = Outline::default();
    for location in locations(&space, axes) {
        if space.is_variable() {
            let _ = cff2.blends_in(&space, &location);
        }
        let blends = cff2.blends_at(&location);
        for glyph in 0..count {
            let fresh = cff2.outline_at(glyph, &location);
            let drawn = cff2.outline_with(glyph, &blends, &mut reused, &mut machine);
            assert_eq!(
                drawn.is_ok(),
                fresh.is_ok(),
                "glyph {glyph}: one path refused it"
            );
            if let Ok(fresh) = fresh {
                assert!(
                    fresh.segments == reused.segments,
                    "glyph {glyph}: the reused machine drew otherwise"
                );
            }
        }
        for font_dict in 0..font_dicts {
            let _ = cff2.private_dict_at(font_dict, &blends);
        }
    }
});
