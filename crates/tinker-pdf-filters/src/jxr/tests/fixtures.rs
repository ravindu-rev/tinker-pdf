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

// --- milestone 1's evidence: the entropy decoder stays in step ----------

/// What one fixture's coefficient parse produced: where each tile packet
/// ended, where each FLEXBITS packet ended, the index table it should agree
/// with, and any leniency the decode had to perform.
struct Parsed {
    /// One entry per index-table entry, in the same order.
    packet_ends: Vec<Option<u64>>,
    offsets: Vec<u64>,
    base: u64,
    /// The length of the `CODED_IMAGE( )`, which is where the last packet
    /// must end — the index table says nothing about that one.
    len: u64,
    warnings: Vec<JxrWarning>,
}

fn parse_coefficients(name: &str) -> Parsed {
    use crate::jxr::coefficients::{PlaneDecoder, PlaneGeometry};

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
    Parsed {
        packet_ends: d.packet_ends.clone(),
        offsets: h.index_offsets.clone(),
        base: h.tile_base,
        len: codestream.len() as u64,
        warnings,
    }
}

/// Every fixture, because this build's coefficient decoder accepts every
/// internal layout the set carries — 4:4:4 or single-component, BD8 or BD16.
const PARSEABLE: &[&str] = &[
    "gray8",
    "gray16",
    "rgb24",
    "bgr24",
    "bgr32",
    "bgra32",
    "rgb48",
    "rgba64",
    "overlap0",
    "overlap1",
    "overlap2",
    "tiled",
    "tiled_gray",
    "frequency",
    "frequency_tiled",
    "seam0",
    "seam1",
    "seam2",
    "quant48",
    "quant16",
    "quant4",
];

impl Parsed {
    /// The byte at which the packet starting at `offset` must end: the next
    /// larger entry in the index table, or — for the last packet, which the
    /// table says nothing about — the end of the coded image.
    fn ceiling_after(&self, offset: u64) -> u64 {
        self.offsets
            .iter()
            .copied()
            .filter(|&o| o > offset)
            .min()
            .map_or(self.len, |o| self.base + o)
    }
}

/// **Milestone 1's evidence, and the strongest first-party check the entropy
/// layer can have.**
///
/// A JPEG XR tile packet is delimited by 8.5.3's index table: entry `n` says
/// where packet `n` begins, so the next larger entry says where it must end.
/// The entropy decoder is a long chain of stateful decisions — adaptive VLC
/// table selection, adaptive scan reordering, coefficient normalization,
/// CBPHP prediction — and **any** of them being wrong desynchronises the bit
/// reader. A desynchronised reader does not stop: it goes on decoding
/// plausible-looking symbols and finishes somewhere else.
///
/// So "the parse ended on exactly the byte the encoder said it would, for
/// every packet of every fixture" is a total check on that whole chain, and
/// it needs no oracle: the index table is the codestream's own statement
/// about itself, not another program's opinion about the picture.
///
/// **What it does not check** is the *values*: a decoder could read the right
/// number of bits and assign them to the wrong coefficients. That is what the
/// lossless identity is for, and the two are complementary rather than
/// overlapping.
///
/// It found one defect on its first run, which is recorded in `tables.rs`:
/// Table 52's code table 0 had its last two rows transposed.
#[test]
fn every_packet_is_consumed_to_the_byte_the_index_table_predicts() {
    let mut checked = 0;
    for name in PARSEABLE {
        let p = parse_coefficients(name);
        assert!(
            !p.warnings.contains(&JxrWarning::TileDroppedAsZero),
            "{name}: a tile was dropped, so its packet did not parse"
        );
        for (n, &offset) in p.offsets.iter().enumerate() {
            let end =
                p.packet_ends[n].unwrap_or_else(|| panic!("{name}: packet {n} was not parsed"));
            let ceiling = p.ceiling_after(offset);
            assert_eq!(
                end, ceiling,
                "{name}: packet {n} ended at {end}, not at {ceiling}"
            );
            checked += 1;
        }
    }
    // A suite whose case count can shrink silently is not a suite. Seventeen
    // single-tile spatial fixtures contribute one packet each, the two
    // 2x2-tiled spatial ones four each, and the two frequency-mode ones four
    // and sixteen — a tile per band.
    assert_eq!(checked, 45, "the packet-boundary check covered too little");
}

/// Every fixture's coefficient layers parse without a single leniency.
///
/// Separate from the boundary check because the two fail differently: a
/// dropped tile means the parse *raised* something, while a boundary mismatch
/// means it finished quietly in the wrong place. A decoder can do either.
#[test]
fn no_fixture_needs_a_tile_dropped_to_zero() {
    for name in PARSEABLE {
        let p = parse_coefficients(name);
        assert_eq!(
            p.warnings
                .iter()
                .filter(|w| **w == JxrWarning::TileDroppedAsZero)
                .count(),
            0,
            "{name} needed a tile dropped"
        );
        assert!(
            p.packet_ends.iter().all(Option::is_some),
            "{name}: not every packet was parsed"
        );
    }
}
