//! The committed fixtures, read through the header layers.
//!
//! `tests/jxr_fixtures.rs` owns the fixture *set* and the pixel comparison;
//! this file owns the part that needs `pub(crate)` visibility — proving that
//! the encoder settings the script asked for actually reached the codestream.
//! That is not a formality: `ImageQualityLevel` was asked for and silently
//! ignored, and a fixture set whose overlap modes were all secretly the same
//! would pass a seam test that tested nothing.

use std::path::{Path, PathBuf};

use crate::jxr::bitstream::BitReader;
use crate::jxr::container;
use crate::jxr::headers::CodedImageHeaders;
use crate::jxr::JxrWarning;

fn fixture_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/jxr")
}

/// Reads one fixture's headers, or explains which step failed.
fn headers_of(name: &str) -> (CodedImageHeaders, Vec<JxrWarning>) {
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
    (h, warnings)
}

#[test]
fn the_overlap_fixtures_really_carry_three_different_overlap_modes() {
    // 8.3.10's OVERLAP_MODE. If WIC had ignored `OverlapLevel` the way it
    // ignores `ImageQualityLevel`, every seam fixture would be the same file
    // and the seam property would be three copies of one measurement.
    for (name, want) in [("overlap0", 0u8), ("overlap1", 1), ("overlap2", 2)] {
        let (h, _) = headers_of(name);
        assert_eq!(h.image.overlap_mode, want, "{name}");
    }
    for (name, want) in [("seam0", 0u8), ("seam1", 1), ("seam2", 2)] {
        let (h, _) = headers_of(name);
        assert_eq!(h.image.overlap_mode, want, "{name}");
    }
}

#[test]
fn the_tiled_fixtures_really_carry_more_than_one_tile() {
    for name in ["tiled", "tiled_gray", "frequency_tiled"] {
        let (h, _) = headers_of(name);
        assert!(h.image.tiling, "{name}: TILING_FLAG is not set");
        let tiles = h.image.num_ver_tiles * h.image.num_hor_tiles;
        assert!(tiles > 1, "{name}: {tiles} tiles");
        // Six by four macroblocks in four tiles: every tile is more than one
        // macroblock in both directions, which is what makes a tile-ordering
        // defect visible.
        assert_eq!(h.image.mb_width, 6, "{name}");
        assert_eq!(h.image.mb_height, 4, "{name}");
        assert_eq!(h.tile_mb_counts.len(), tiles as usize, "{name}");
        let total: u64 = h.tile_mb_counts.iter().sum();
        assert_eq!(total, 24, "{name}: tiles do not cover the macroblock grid");
    }
    // And the untiled ones really are untiled, so the pair is a contrast.
    let (h, _) = headers_of("rgb24");
    assert_eq!(h.image.num_ver_tiles * h.image.num_hor_tiles, 1);
}

#[test]
fn the_frequency_fixtures_really_are_frequency_mode() {
    for name in ["frequency", "frequency_tiled"] {
        let (h, _) = headers_of(name);
        assert!(h.image.frequency_mode, "{name}");
        // 8.5.1: a frequency-mode index table has one entry per tile per
        // band, so its length is the second thing that would have to be
        // wrong for the flag to be lying.
        let tiles = u64::from(h.image.num_ver_tiles * h.image.num_hor_tiles);
        let bands = u64::from(h.primary.bands_present.num_bands());
        assert_eq!(h.index_offsets.len() as u64, tiles * bands, "{name}");
    }
    let (h, _) = headers_of("rgb24");
    assert!(!h.image.frequency_mode);
}

#[test]
fn every_fixture_is_the_geometry_and_colour_layout_it_was_authored_with() {
    use crate::jxr::headers::{InternalClrFmt, OutputBitdepth, OutputClrFmt};

    // (name, width, height, components, OUTPUT_CLR_FMT, OUTPUT_BITDEPTH)
    let want: &[(&str, u32, u32, u32, OutputClrFmt, OutputBitdepth)] = &[
        ("gray8", 48, 32, 1, OutputClrFmt::YOnly, OutputBitdepth::Bd8),
        (
            "gray16",
            48,
            32,
            1,
            OutputClrFmt::YOnly,
            OutputBitdepth::Bd16,
        ),
        ("rgb24", 48, 32, 3, OutputClrFmt::Rgb, OutputBitdepth::Bd8),
        ("bgr24", 48, 32, 3, OutputClrFmt::Rgb, OutputBitdepth::Bd8),
        ("bgr32", 48, 32, 3, OutputClrFmt::Rgb, OutputBitdepth::Bd8),
        ("bgra32", 48, 32, 3, OutputClrFmt::Rgb, OutputBitdepth::Bd8),
        ("rgb48", 48, 32, 3, OutputClrFmt::Rgb, OutputBitdepth::Bd16),
        ("rgba64", 48, 32, 3, OutputClrFmt::Rgb, OutputBitdepth::Bd16),
        ("tiled", 96, 64, 3, OutputClrFmt::Rgb, OutputBitdepth::Bd8),
        (
            "tiled_gray",
            96,
            64,
            1,
            OutputClrFmt::YOnly,
            OutputBitdepth::Bd8,
        ),
    ];
    for &(name, w, h_, comps, clr, depth) in want {
        let (h, _) = headers_of(name);
        assert_eq!(h.image.width, w, "{name} width");
        assert_eq!(h.image.height, h_, "{name} height");
        assert_eq!(h.image.output_clr_fmt, clr, "{name} OUTPUT_CLR_FMT");
        assert_eq!(h.image.output_bitdepth, depth, "{name} OUTPUT_BITDEPTH");
        assert_eq!(h.primary.num_components, comps, "{name} NumComponents");
        // The script asks for SubsamplingLevel 3, so every fixture's internal
        // layout is one this build implements rather than one it refuses.
        assert!(
            matches!(
                h.primary.internal_clr_fmt,
                InternalClrFmt::YOnly | InternalClrFmt::Yuv444
            ),
            "{name} INTERNAL_CLR_FMT is {:?}",
            h.primary.internal_clr_fmt
        );
    }
}

/// Prints what every fixture's headers say. Not an assertion — a way to see
/// the set, which is what made the `ImageQualityLevel` finding visible.
#[test]
#[ignore = "prints the fixture header table; run with --nocapture"]
fn dump_fixture_headers() {
    let dir = fixture_dir();
    let mut names: Vec<String> = std::fs::read_dir(&dir)
        .expect("the fixture directory")
        .flatten()
        .filter_map(|e| {
            e.file_name()
                .to_string_lossy()
                .strip_suffix(".jxr")
                .map(str::to_owned)
        })
        .collect();
    names.sort();
    println!(
        "{:<18} {:>4}x{:<4} {:>3} {:>4} {:>7} {:>5} {:>5} {:>6}",
        "name", "w", "h", "nc", "ovl", "bands", "tiles", "freq", "alpha"
    );
    for name in &names {
        let (h, _) = headers_of(name);
        println!(
            "{:<18} {:>4}x{:<4} {:>3} {:>4} {:>7?} {:>5} {:>5} {:>6}",
            name,
            h.image.width,
            h.image.height,
            h.primary.num_components,
            h.image.overlap_mode,
            h.primary.bands_present,
            h.image.num_ver_tiles * h.image.num_hor_tiles,
            h.image.frequency_mode,
            h.image.alpha_image_plane,
        );
    }
    println!("RAN dump_fixture_headers: {} fixtures", names.len());
}
