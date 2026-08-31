//! The photo overlap transform's properties.
//!
//! # The seam property, and what it measures
//!
//! A wrong overlap filter does not produce noise. The POT runs *across* block
//! boundaries, so getting it wrong yields a picture that is correct
//! everywhere except within two samples of a block edge — faint seams on an
//! otherwise plausible photograph. A whole-image error bound passes that; a
//! human skimming a page does not see it.
//!
//! So the probe is a **horizontal ramp**, whose discrete second difference
//! along a row is zero by construction, and the metric is the largest second
//! difference at block-edge columns against the largest inside blocks.
//! Quantization raises both together; a wrong filter raises only the first.
//!
//! The fixtures are **lossy on purpose**. A lossless ramp reconstructs
//! exactly whatever the filter does, so the property would be measuring the
//! identity again and would fire on nothing the identity had not caught.
//!
//! # Measured, with the second-level filter on and off
//!
//! | Fixture | `OVERLAP_MODE` | Filtered: edge / interior | Unfiltered: edge / interior |
//! | --- | ---: | ---: | ---: |
//! | `seam0` | 0 | 15 / 16 | 15 / 16 |
//! | `seam1` | 1 | 23 / 31 | **90** / 33 |
//! | `seam2` | 2 | 22 / 33 | **93** / 33 |
//!
//! Three things in that table are worth reading rather than skimming.
//!
//! With the filter correct, the edge figure is **lower** than the interior
//! one — 23 against 31, 22 against 33. The ramp really is smooth across the
//! boundaries, not merely no worse there.
//!
//! With it disabled, the edge figure roughly **quadruples** while the
//! interior figure does not move at all (33 either way). That is the
//! signature of a seam and nothing else: an error confined to block edges.
//! The injection is counted at **2 of the 2 filtered modes**.
//!
//! And `seam0` does not move, because it was encoded at `OVERLAP_MODE` 0 and
//! the decoder was already not filtering it. That zero is a check in its own
//! right — it says the mode is being honoured rather than the filter being
//! applied unconditionally, which is the other way to get this wrong.
//!
//! # What this property does not reach, by name
//!
//! - **The first-level filter across a soft tile boundary.** It needs a
//!   multi-tile image at `OVERLAP_MODE` 2, and both tiled fixtures were
//!   encoded at mode 1 instead. That is also the one path where 9.9.3.2's
//!   text disagrees with its own geometry — see `overlap.rs`'s module docs.
//! - **`HARD_TILING_FLAG`.** WIC does not expose it, so only the soft-tile
//!   path has a fixture at all.
//!
//! Both are recorded in `docs/features/xps.md` and
//! `docs/design/jpeg-xr.md` as decoded but unadjudicated.

use super::*;
use crate::jxr::bitstream::BitReader;
use crate::jxr::coefficients::{PlaneDecoder, PlaneGeometry};
use crate::jxr::container;
use crate::jxr::headers::CodedImageHeaders;
use std::path::{Path, PathBuf};

fn fixture_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/jxr")
}

/// Decodes one fixture as far as 9.9 goes and returns its luma plane and
/// dimensions. 9.10's output formatting is not applied, which does not
/// matter here: a second difference is unchanged by 9.10.5's bias and merely
/// scaled by 9.10.6's shift, so the seam metric below reads the same either
/// way.
fn luma_plane(name: &str, disable_second_level: bool) -> (Vec<i32>, usize, usize) {
    let path = fixture_dir().join(format!("{name}.jxr"));
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let c = container::read(&bytes).unwrap_or_else(|e| panic!("{name} container: {e}"));
    let codestream = bytes
        .get(c.image.clone())
        .unwrap_or_else(|| panic!("{name}: image range outside the file"));
    let mut warnings = Vec::new();
    let mut r = BitReader::new(codestream);
    let h = CodedImageHeaders::read(&mut r, &mut warnings)
        .unwrap_or_else(|e| panic!("{name} headers: {e}"));
    let g = PlaneGeometry::from_headers(&h, &h.primary)
        .unwrap_or_else(|e| panic!("{name} geometry: {e}"));
    let mut d = PlaneDecoder::new(&g).unwrap_or_else(|e| panic!("{name} allocation: {e}"));
    d.parse_tiles(&mut r, &h, &h.primary, &mut warnings)
        .unwrap_or_else(|e| panic!("{name} tiles: {e}"));
    d.reconstruct_for_test(disable_second_level);
    let w = g.ext_width;
    let plane = d.plane.first().cloned().unwrap_or_default();
    (plane, w, g.ext_height)
}

/// The seam metric: the largest second difference along a row, taken
/// separately over the columns that sit on a **block boundary** and over
/// those that do not.
///
/// A horizontal ramp has a second difference of zero everywhere by
/// construction — that is what makes it the right probe. Whatever the
/// quantizer does to it, it does uniformly; a wrong overlap filter instead
/// puts its error *only* where blocks meet, which is exactly the defect a
/// loosely set whole-image error bound would pass.
fn seam_metric(plane: &[i32], w: usize, h: usize) -> (i64, i64) {
    let mut on_edge = 0i64;
    let mut interior = 0i64;
    for y in 0..h {
        for x in 1..w.saturating_sub(1) {
            let a = i64::from(plane[y * w + x - 1]);
            let b = i64::from(plane[y * w + x]);
            let c = i64::from(plane[y * w + x + 1]);
            let d2 = (a - 2 * b + c).abs();
            // 9.9.5's transform works in 4x4 blocks, so a block edge is every
            // fourth column; the second difference centred on `x` reaches
            // `x - 1` and `x + 1`, so both columns either side of an edge see
            // it.
            if x % 4 == 0 || x % 4 == 3 {
                on_edge = on_edge.max(d2);
            } else {
                interior = interior.max(d2);
            }
        }
    }
    (on_edge, interior)
}

/// The three seam fixtures and the `OVERLAP_MODE` each was encoded at.
const SEAMS: [(&str, u8); 3] = [("seam0", 0), ("seam1", 1), ("seam2", 2)];

#[test]
fn a_ramp_gains_no_discontinuity_at_a_block_boundary_in_any_overlap_mode() {
    // **The seam property.** 8.3.10's three overlap modes are three different
    // agreements between encoder and decoder about how much filtering the
    // encoder already did. Getting one wrong does not produce noise: it
    // produces a picture that is right everywhere except within two samples
    // of a block edge, which is precisely what a human skimming a page does
    // not see and what a whole-image error bound does not catch.
    //
    // The fixtures are **lossy on purpose**. A lossless ramp reconstructs
    // exactly whatever the filter does, so the property would only be
    // measuring the identity again; it is on the quantized path that a wrong
    // filter shows as a step no other check here can see.
    for (name, mode) in SEAMS {
        let (plane, w, h) = luma_plane(name, false);
        let (on_edge, interior) = seam_metric(&plane, w, h);
        // "No worse at the boundary than in the interior", with the slack a
        // quantizer needs: the ramp's own rounding contributes a little
        // everywhere, and the comparison is between two maxima rather than
        // two averages, so exact equality is not the claim.
        assert!(
            on_edge <= interior * 2 + 8,
            "{name} (OVERLAP_MODE {mode}): second difference is {on_edge} at block \
             edges against {interior} inside blocks"
        );
    }
}

#[test]
fn disabling_the_second_level_filter_makes_the_seam_metric_fire() {
    // **The counted injection.** A property that fires on nothing is not a
    // property. Disabling 9.9.6's filter is not a subtle break — it is the
    // whole second-level POT — and it is the right injection precisely
    // because a decoder that simply forgot to call it would still return a
    // recognisable picture.
    //
    // `seam0` is excluded: it was encoded at OVERLAP_MODE 0, so the decoder
    // is *already* not filtering it and there is nothing to disable. That it
    // is unaffected is itself the check that the mode is being honoured.
    let mut fired = 0;
    for (name, mode) in SEAMS {
        let (plane, w, h) = luma_plane(name, true);
        let (on_edge, interior) = seam_metric(&plane, w, h);
        let seamy = on_edge > interior * 2 + 8;
        if mode == 0 {
            assert!(
                !seamy,
                "seam0 is encoded unfiltered, so disabling the filter must \
                 change nothing"
            );
        } else if seamy {
            fired += 1;
        }
    }
    assert_eq!(
        fired, 2,
        "the seam property must fire on both filtered modes when the \
         second-level filter is disabled"
    );
}

// --- the filter primitives ----------------------------------------------

#[test]
fn the_four_point_filter_is_not_the_identity_and_is_deterministic() {
    // Narrow, but it pins two things a stub would pass: that the filter does
    // something, and that it does the same thing twice. A filter that
    // silently became a no-op would leave the seam property to catch it end
    // to end, and this names it here instead.
    let mut a = [10i32, 20, 30, 40];
    let before = a;
    overlap_post_filter4(&mut a);
    assert_ne!(a, before, "the four-point filter did nothing");
    let mut b = before;
    overlap_post_filter4(&mut b);
    assert_eq!(a, b, "the four-point filter is not deterministic");
}

#[test]
fn the_interior_filter_touches_every_one_of_its_sixteen_positions() {
    // 9.9.8.1 gathers a 4x4 block that straddles four blocks' corners. If any
    // position were left out of the quadruples, that sample would keep its
    // pre-filter value and the seam would appear at one corner only — the
    // hardest kind of defect to see in a picture.
    let mut moved = [false; 16];
    for probe in 0..16usize {
        let mut c = [0i32; 16];
        c[probe] = 4096;
        let mut d = c;
        overlap_post_filter4x4(&mut d);
        for (i, m) in moved.iter_mut().enumerate() {
            if d[i] != c[i] {
                *m = true;
            }
        }
    }
    assert_eq!(
        moved.iter().filter(|m| **m).count(),
        16,
        "the interior filter leaves a position untouched: {moved:?}"
    );
}

#[test]
fn the_dc_plane_round_trips_through_the_gather_and_scatter() {
    // The first level filters a *materialised* DC plane, so the mapping
    // between `MbDCLP[x][y][i][j]` and `(4x + j % 4, 4y + j / 4)` is load
    // bearing in both directions. A transposed gather would filter the right
    // values in the wrong places and scatter them back looking plausible.
    let g = Geometry {
        components: 2,
        mb_width: 3,
        mb_height: 2,
        ext_width: 48,
        ext_height: 32,
        hard_tiling: false,
        left_mb_of_tile: vec![0, 3],
        top_mb_of_tile: vec![0, 2],
        num_tile_cols: 1,
        num_tile_rows: 1,
    };
    let n = g.mb_width * g.mb_height * g.components * 16;
    let original: Vec<i32> = (0..n).map(|i| i as i32 * 7 - 100).collect();
    let mut dclp = original.clone();
    let (w, h) = (g.mb_width * 4, g.mb_height * 4);
    let mut dc = vec![0i32; w * h];
    for i in 0..g.components {
        gather_dc_plane(&dclp, &g, i, &mut dc, w);
        scatter_dc_plane(&mut dclp, &g, i, &dc, w);
    }
    assert_eq!(dclp, original, "gather and scatter are not inverses");
}

#[test]
#[ignore = "prints the seam metric; run with --nocapture"]
fn dump_seam_metric() {
    for (name, mode) in SEAMS {
        let (real, w, h) = luma_plane(name, false);
        let (re, ri) = seam_metric(&real, w, h);
        let (broken, _, _) = luma_plane(name, true);
        let (be, bi) = seam_metric(&broken, w, h);
        println!(
            "{name} (mode {mode}): filtered edge={re} interior={ri} | \
             unfiltered edge={be} interior={bi}"
        );
    }
}
