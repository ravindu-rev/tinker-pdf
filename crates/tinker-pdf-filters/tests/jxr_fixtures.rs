//! The JPEG XR fixture set: what it is, who authored it, and what it proves.
//!
//! ITU-T T.832's conformance bitstreams are not freely licensed, and ruling 13
//! forbids asking another decoder what a picture should look like. So the
//! primary gate here is a **lossless identity**, and its whole point is that
//! nothing third-party adjudicates it:
//!
//! 1. [`RASTERS`] below authors a raster in this repository, from integer
//!    arithmetic in this file.
//! 2. `tests/jxr/make-fixtures.ps1` hands those bytes to Windows Imaging
//!    Component's JPEG XR encoder with `Lossless = true`, and commits what
//!    comes back.
//! 3. The decoder must return the raster from step 1, **bit for bit**.
//!
//! The pixels going in are ours, so the comparison is against a value this
//! repository chose rather than against another program's opinion. WIC is
//! *supplying* bytes, which ruling 13 admits explicitly; it never says
//! whether the output is right.
//!
//! Regenerating the fixtures is two commands, in this order:
//!
//! ```text
//! cargo test -p tinker-pdf-filters --test jxr_fixtures -- --ignored write_source_rasters
//! pwsh -File crates/tinker-pdf-filters/tests/jxr/make-fixtures.ps1
//! ```
//!
//! The `.raw` files the first writes are build intermediates and are not
//! committed: the raster's one definition is [`raster`] in this file, so
//! there is nothing for a committed copy to drift from. `manifest.txt` *is*
//! committed, because a reviewer should see the fixture list without running
//! anything — and [`the_committed_manifest_matches_the_table`] pins it to
//! [`RASTERS`] so it cannot drift either.

use std::path::{Path, PathBuf};

use tinker_pdf_filters::{jxr_decode, JxrChannels, Limits};

/// How a raster's samples are generated. Integer arithmetic only, so the
/// fixture set is reproducible on any target (ruling 4).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pattern {
    /// Value depends on `x` alone, linearly. **The seam fixture**: a ramp is
    /// smooth across every macroblock boundary by construction, so a wrong
    /// overlap filter shows up as a step at `x % 16 == 0` and nowhere else.
    Ramp,
    /// A different linear combination of `x` and `y` per channel — enough
    /// low-frequency content to exercise the DC and LP bands, and enough
    /// per-channel difference to catch a colour-transform defect.
    Gradient,
    /// 4x4 checkerboard over a gradient. The 4x4 period is the block size of
    /// 9.9.2's first-level transform, so this puts energy in the HP band that
    /// a DC-and-LP-only decode cannot fake.
    Checker,
    /// A gradient with a filled rectangle and a diagonal — sharp edges at
    /// positions that are not multiples of 16, which is where an off-by-one
    /// in the adaptive scan shows up.
    Mixed,
}

/// One fixture: a raster this repository authors, and the encoder settings
/// WIC is asked for.
pub struct Fixture {
    pub name: &'static str,
    pub width: u32,
    pub height: u32,
    /// The `System.Windows.Media.PixelFormats` member the script asks for.
    pub wpf_format: &'static str,
    /// Channels in the canonical raster: 1, 3 or 4.
    pub channels: u8,
    /// 8 or 16.
    pub depth: u8,
    pub pattern: Pattern,
    /// `WmpBitmapEncoder.Lossless`.
    pub lossless: bool,
    /// `WmpBitmapEncoder.OverlapLevel`: 8.3.10's `OVERLAP_MODE`.
    pub overlap: u8,
    /// `HorizontalTileSlices` and `VerticalTileSlices`.
    ///
    /// WIC counts *slices*, not extra slices: 1 is one slice and leaves
    /// 8.3.6's `TILING_FLAG` clear, and 2 is the smallest value that sets
    /// it. Measured rather than assumed — the first fixture set asked for 1
    /// and produced fifteen single-tile files whose tile tests all passed
    /// vacuously.
    pub tiles: (u8, u8),
    /// `WmpBitmapEncoder.FrequencyOrder`: 8.3.7.
    pub frequency: bool,
    /// `WmpBitmapEncoder.QualityLevel`, which is the codec's own
    /// quantization parameter rather than a percentage: 1 is lossless and
    /// larger is coarser. Ignored when `lossless`.
    ///
    /// It is this knob and not `ImageQualityLevel` because the latter has no
    /// effect once `UseCodecOptions` is set — measured, not assumed: three
    /// fixtures asked for 30 %, 60 % and 90 % came back byte-identical.
    pub quant: u8,
}

const fn f(
    name: &'static str,
    width: u32,
    height: u32,
    wpf_format: &'static str,
    channels: u8,
    depth: u8,
    pattern: Pattern,
) -> Fixture {
    Fixture {
        name,
        width,
        height,
        wpf_format,
        channels,
        depth,
        pattern,
        lossless: true,
        overlap: 1,
        tiles: (1, 1),
        frequency: false,
        quant: 1,
    }
}

/// The fixture set.
///
/// **Sizes are not arbitrary.** 48 x 32 is three macroblocks across and two
/// down, so a single-fixture set cannot pass while getting macroblock
/// ordering wrong. 96 x 64 with a 2 x 2 tile grid is six by four macroblocks
/// in four tiles, which is the smallest shape that can see a tile-ordering
/// defect at all — the reason the brief for this work insisted the fixtures
/// be larger than one tile and one macroblock row.
pub const RASTERS: &[Fixture] = &[
    // --- the eight pixel formats, single tile, second-level overlap ------
    f("gray8", 48, 32, "Gray8", 1, 8, Pattern::Mixed),
    f("gray16", 48, 32, "Gray16", 1, 16, Pattern::Mixed),
    f("rgb24", 48, 32, "Rgb24", 3, 8, Pattern::Mixed),
    f("bgr24", 48, 32, "Bgr24", 3, 8, Pattern::Mixed),
    f("bgr32", 48, 32, "Bgr32", 3, 8, Pattern::Mixed),
    f("bgra32", 48, 32, "Bgra32", 4, 8, Pattern::Mixed),
    f("rgb48", 48, 32, "Rgb48", 3, 16, Pattern::Mixed),
    f("rgba64", 48, 32, "Rgba64", 4, 16, Pattern::Mixed),
    // --- the three overlap modes, on content with block-scale energy -----
    Fixture {
        overlap: 0,
        ..f("overlap0", 64, 48, "Rgb24", 3, 8, Pattern::Checker)
    },
    Fixture {
        overlap: 1,
        ..f("overlap1", 64, 48, "Rgb24", 3, 8, Pattern::Checker)
    },
    Fixture {
        overlap: 2,
        ..f("overlap2", 64, 48, "Rgb24", 3, 8, Pattern::Checker)
    },
    // --- tiles: four tiles over six by four macroblocks ------------------
    Fixture {
        tiles: (2, 2),
        ..f("tiled", 96, 64, "Rgb24", 3, 8, Pattern::Mixed)
    },
    Fixture {
        tiles: (2, 2),
        ..f("tiled_gray", 96, 64, "Gray8", 1, 8, Pattern::Checker)
    },
    // --- frequency mode --------------------------------------------------
    Fixture {
        frequency: true,
        ..f("frequency", 48, 32, "Rgb24", 3, 8, Pattern::Mixed)
    },
    Fixture {
        frequency: true,
        tiles: (2, 2),
        ..f("frequency_tiled", 96, 64, "Rgb24", 3, 8, Pattern::Mixed)
    },
    // --- the seam fixtures: a ramp at each overlap mode, lossy -----------
    //
    // Lossy on purpose. A lossless ramp reconstructs exactly whatever the
    // overlap filter does, so the seam property would be measuring the
    // identity again; it is the *quantized* path where a wrong filter shows
    // as a step the identity cannot see.
    Fixture {
        lossless: false,
        quant: 8,
        overlap: 0,
        ..f("seam0", 96, 32, "Rgb24", 3, 8, Pattern::Ramp)
    },
    Fixture {
        lossless: false,
        quant: 8,
        overlap: 1,
        ..f("seam1", 96, 32, "Rgb24", 3, 8, Pattern::Ramp)
    },
    Fixture {
        lossless: false,
        quant: 8,
        overlap: 2,
        ..f("seam2", 96, 32, "Rgb24", 3, 8, Pattern::Ramp)
    },
    // --- monotonicity: the same source at falling quantization ----------
    Fixture {
        lossless: false,
        quant: 48,
        ..f("quant48", 64, 48, "Rgb24", 3, 8, Pattern::Mixed)
    },
    Fixture {
        lossless: false,
        quant: 16,
        ..f("quant16", 64, 48, "Rgb24", 3, 8, Pattern::Mixed)
    },
    Fixture {
        lossless: false,
        quant: 4,
        ..f("quant4", 64, 48, "Rgb24", 3, 8, Pattern::Mixed)
    },
];

/// The canonical raster: `channels` interleaved samples per pixel, row-major,
/// each in `0 ..= (1 << depth) - 1`, held as `u16` whatever the depth.
///
/// Every expression is integer. There is no float in this file, which is what
/// keeps the fixture set identical on every target that regenerates it.
#[must_use]
pub fn raster(spec: &Fixture) -> Vec<u16> {
    let max: u32 = if spec.depth == 16 { 65_535 } else { 255 };
    let w = spec.width;
    let h = spec.height;
    let ch = u32::from(spec.channels);
    let mut out = Vec::with_capacity((w * h * ch) as usize);
    for y in 0..h {
        for x in 0..w {
            for c in 0..ch {
                let v = match spec.pattern {
                    // A pure function of x: the second difference along a row
                    // is zero everywhere except for the rounding of the
                    // division, which is at most one unit.
                    Pattern::Ramp => x * max / (w - 1),
                    Pattern::Gradient => {
                        let a = x * max / (w - 1);
                        let b = y * max / (h - 1);
                        match c {
                            0 => a,
                            1 => b,
                            2 => (a + b) / 2,
                            _ => max - (a + b) / 2,
                        }
                    }
                    Pattern::Checker => {
                        // 4x4 is 9.9.2's first-level block size, so the
                        // energy lands squarely in the HP band.
                        let on = ((x / 4) + (y / 4)) % 2 == 0;
                        let base = (x * max / (w - 1) + y * max / (h - 1)) / 2;
                        if on {
                            base / 4 + max * 3 / 4
                        } else {
                            base / 4
                        }
                    }
                    Pattern::Mixed => {
                        let base = (x * 2 + y * 3 + c * 37) % (max + 1);
                        // A filled rectangle whose edges are at 13 and 29 —
                        // deliberately not multiples of 16, so an off-by-one
                        // in the adaptive scan cannot hide on a block edge.
                        let in_rect = (13..29).contains(&x) && (5..21).contains(&y);
                        // A diagonal, which no separable transform represents
                        // compactly.
                        let on_diagonal = (x + y) % 17 == 0;
                        if in_rect {
                            (base + max / 2) % (max + 1)
                        } else if on_diagonal {
                            max - base
                        } else {
                            base
                        }
                    }
                };
                // Every branch above is bounded by `max`, which is at most
                // 65 535, so the narrowing is exact.
                out.push(v.min(max) as u16);
            }
        }
    }
    out
}

/// The canonical raster packed the way the WPF pixel format lays it out,
/// which is what the encoder is handed.
#[must_use]
pub fn packed_for_wpf(spec: &Fixture) -> Vec<u8> {
    let samples = raster(spec);
    let ch = usize::from(spec.channels);
    let mut out = Vec::new();
    for px in samples.chunks_exact(ch) {
        match spec.wpf_format {
            "Gray8" => out.push(low(px[0])),
            "Gray16" => out.extend_from_slice(&px[0].to_le_bytes()),
            "Rgb24" => out.extend_from_slice(&[low(px[0]), low(px[1]), low(px[2])]),
            "Bgr24" => out.extend_from_slice(&[low(px[2]), low(px[1]), low(px[0])]),
            // 32bppBGR's fourth byte is padding no decoder reconstructs.
            "Bgr32" => out.extend_from_slice(&[low(px[2]), low(px[1]), low(px[0]), 0]),
            "Bgra32" => {
                out.extend_from_slice(&[low(px[2]), low(px[1]), low(px[0]), low(px[3])]);
            }
            "Rgb48" => {
                for s in &px[..3] {
                    out.extend_from_slice(&s.to_le_bytes());
                }
            }
            "Rgba64" => {
                for s in &px[..4] {
                    out.extend_from_slice(&s.to_le_bytes());
                }
            }
            other => panic!("no packing rule for WPF format {other}"),
        }
    }
    out
}

fn low(v: u16) -> u8 {
    // The 8-bit patterns generate values in 0..=255, so the low byte is the
    // whole sample.
    (v & 0xFF) as u8
}

/// The bytes the decoder must return, derived from the same canonical raster
/// and the channel order the *decoder* reports.
///
/// Deriving the expectation from the decoder's reported format rather than
/// from the WPF one is deliberate: it means the test pins the decode against
/// this repository's raster in whatever layout Table A.6's row calls for, and
/// a decoder that reported the wrong row would fail on the permutation rather
/// than pass by construction.
#[must_use]
pub fn expected_output(spec: &Fixture, channels: JxrChannels, bits: u8) -> Vec<u8> {
    let samples = raster(spec);
    let ch = usize::from(spec.channels);
    let mut out = Vec::new();
    for px in samples.chunks_exact(ch) {
        // The canonical raster is R, G, B, A; every output order is a
        // permutation of a prefix of it.
        let order: &[usize] = match channels {
            JxrChannels::Gray => &[0],
            JxrChannels::Rgb => &[0, 1, 2],
            JxrChannels::Bgr => &[2, 1, 0],
            JxrChannels::Bgra => &[2, 1, 0, 3],
            JxrChannels::Rgba => &[0, 1, 2, 3],
        };
        for &i in order {
            let s = px.get(i).copied().unwrap_or(0);
            if bits == 16 {
                out.extend_from_slice(&s.to_le_bytes());
            } else {
                out.push(low(s));
            }
        }
    }
    out
}

fn fixture_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/jxr")
}

/// The manifest text, derived from [`RASTERS`]. One line a fixture, so a
/// reviewer reading the diff sees the set change.
fn manifest_text() -> String {
    let mut s = String::from(
        "# Generated by `cargo test -p tinker-pdf-filters --test jxr_fixtures -- --ignored\n\
         # write_source_rasters`. Do not edit: the source of truth is RASTERS in\n\
         # crates/tinker-pdf-filters/tests/jxr_fixtures.rs.\n\
         # name width height wpf_format lossless overlap htiles vtiles frequency quant\n",
    );
    for spec in RASTERS {
        s.push_str(&format!(
            "{} {} {} {} {} {} {} {} {} {}\n",
            spec.name,
            spec.width,
            spec.height,
            spec.wpf_format,
            u8::from(spec.lossless),
            spec.overlap,
            spec.tiles.0,
            spec.tiles.1,
            u8::from(spec.frequency),
            spec.quant,
        ));
    }
    s
}

/// Writes the `.raw` sources and `manifest.txt` for `make-fixtures.ps1`.
///
/// `#[ignore]`d because it writes into the source tree, which an ordinary
/// `cargo test` must not do. It is the *only* definition of what the encoder
/// is handed, which is what keeps the fixtures and the expectations from
/// drifting apart — the discipline `docs/verification.md` records for the six
/// seed corpora that are written the same way.
#[test]
#[ignore = "writes fixture sources into the source tree; see the module docs"]
fn write_source_rasters() {
    let dir = fixture_dir();
    std::fs::create_dir_all(&dir).expect("the fixture directory");
    for spec in RASTERS {
        let path = dir.join(format!("{}.raw", spec.name));
        std::fs::write(&path, packed_for_wpf(spec)).expect("writing a source raster");
    }
    std::fs::write(dir.join("manifest.txt"), manifest_text()).expect("writing the manifest");
    println!(
        "RAN write_source_rasters: {} rasters and a manifest in {}",
        RASTERS.len(),
        dir.display()
    );
}

/// Copies the committed fixtures into the fuzz seed corpus.
///
/// `docs/verification.md` records why this exists and why it is a test rather
/// than a script: a target with no seeds spends its whole budget on random
/// bytes, and for a format gated on a four-byte magic that means it reaches
/// the parser essentially never.
#[test]
#[ignore = "writes into fuzz/corpus/jxr; see docs/verification.md"]
fn write_fuzz_seeds() {
    let corpus = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fuzz/corpus/jxr")
        .canonicalize()
        .unwrap_or_else(|_| Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fuzz/corpus/jxr"));
    std::fs::create_dir_all(&corpus).expect("the seed corpus directory");
    let dir = fixture_dir();
    let mut written = 0;
    for spec in RASTERS {
        let from = dir.join(format!("{}.jxr", spec.name));
        if let Ok(bytes) = std::fs::read(&from) {
            std::fs::write(corpus.join(format!("{}.jxr", spec.name)), &bytes)
                .expect("writing a seed");
            written += 1;
            // The bare codestream is a second entry point (`jxr_decode`
            // accepts one without a container), and a fuzzer reaching it only
            // through the container would never explore clause 8 directly.
            if let Some(cs) = codestream_of(&bytes) {
                std::fs::write(corpus.join(format!("{}.codestream", spec.name)), cs)
                    .expect("writing a codestream seed");
                written += 1;
            }
        }
    }
    println!(
        "RAN write_fuzz_seeds: {written} seeds in {}",
        corpus.display()
    );
    assert!(
        written > 0,
        "no fixtures to seed from: run make-fixtures.ps1 first"
    );
}

/// Pulls the `CODED_IMAGE( )` out of an Annex A file by finding 8.3.2's
/// signature. Good enough for a seed writer; the decoder does it properly.
fn codestream_of(file: &[u8]) -> Option<&[u8]> {
    file.windows(8)
        .position(|w| w == b"WMPHOTO\0")
        .and_then(|at| file.get(at..))
}

// --- what runs on an ordinary `cargo test` ------------------------------

#[test]
fn the_committed_manifest_matches_the_table() {
    let path = fixture_dir().join("manifest.txt");
    let Ok(committed) = std::fs::read_to_string(&path) else {
        panic!("SKIPPED is not available here: manifest.txt is committed and must exist");
    };
    assert_eq!(
        committed.replace("\r\n", "\n"),
        manifest_text(),
        "manifest.txt has drifted from RASTERS; rerun write_source_rasters"
    );
}

#[test]
fn every_committed_fixture_has_a_row_in_the_table() {
    let dir = fixture_dir();
    let entries = std::fs::read_dir(&dir).expect("the fixture directory is committed");
    let mut found = 0;
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        let Some(stem) = name.strip_suffix(".jxr") else {
            continue;
        };
        found += 1;
        assert!(
            RASTERS.iter().any(|f| f.name == stem),
            "committed fixture {stem}.jxr has no row in RASTERS"
        );
    }
    // A suite whose case count can shrink silently is not a suite.
    assert_eq!(
        found,
        RASTERS.len(),
        "the committed fixture count does not match RASTERS"
    );
}

/// **The lossless identity: the primary gate, and the only check in this
/// repository that compares JPEG XR pixels.**
///
/// [`RASTERS`] authors a raster in this file; `make-fixtures.ps1` hands those
/// bytes to Windows Imaging Component's JPEG XR encoder with `Lossless =
/// true`; this requires the decoder to return that raster, **bit for bit**.
///
/// Nothing third-party adjudicates it. The pixels going in are ours, so the
/// comparison is against a value this repository chose rather than against
/// another program's opinion of the picture. WIC is *supplying* bytes, which
/// ruling 13 admits in as many words — "a third-party program may host this
/// code, execute it, fetch bytes for it, or generate inputs for it" — and it
/// never says whether the output is right.
///
/// It is a **total** check: one wrong bit anywhere in the entropy decoder,
/// the prediction, the dequantization, either transform, the overlap filter
/// or the colour pipeline fails it. That is what a format whose failure mode
/// is a *plausible photograph* needs.
#[test]
fn every_lossless_fixture_decodes_to_the_raster_this_repository_authored() {
    let dir = fixture_dir();
    let limits = Limits::new(1 << 24);
    let mut checked = 0;
    for spec in RASTERS {
        if !spec.lossless {
            continue;
        }
        let path = dir.join(format!("{}.jxr", spec.name));
        let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        let image = jxr_decode(&bytes, &limits).unwrap_or_else(|e| panic!("{}: {e}", spec.name));
        assert_eq!(image.width, spec.width, "{} width", spec.name);
        assert_eq!(image.height, spec.height, "{} height", spec.name);
        assert!(image.complete, "{} was not complete", spec.name);
        let want = expected_output(spec, image.format.channels, image.bits_per_component());
        assert_eq!(
            image.data.len(),
            want.len(),
            "{}: {} bytes against {}",
            spec.name,
            image.data.len(),
            want.len()
        );
        if image.data != want {
            let differing = image.data.iter().zip(&want).filter(|(a, b)| a != b).count();
            let (at, got, expected) = first_difference(&image.data, &want);
            panic!(
                "{}: {differing} of {} bytes differ; the first is byte {at}, \
                 {got} against {expected}",
                spec.name,
                want.len()
            );
        }
        checked += 1;
    }
    // A suite whose case count can shrink silently is not a suite.
    assert_eq!(
        checked,
        RASTERS.iter().filter(|f| f.lossless).count(),
        "not every lossless fixture was checked"
    );
    assert!(checked >= 15, "only {checked} lossless fixtures");
}

/// Where two rasters first differ, so a failure names a position instead of
/// dumping two megabytes.
fn first_difference(got: &[u8], want: &[u8]) -> (usize, u8, u8) {
    for (i, (a, b)) in got.iter().zip(want).enumerate() {
        if a != b {
            return (i, *a, *b);
        }
    }
    (0, 0, 0)
}

/// The **monotonicity property**, and the weakest of the three evidence legs.
///
/// One source encoded at rising quantization must decode monotonically
/// further from it. It is here for exactly one reason: it is the only thing
/// that reaches 9.8's `QuantMap( )` at quantizers the lossless identity never
/// exercises, where QP is 1 and the map returns 1.
///
/// It catches a gross error — an inverted `QuantMap( )`, a QP index read from
/// the wrong band, a shift in the wrong direction. It would not notice an
/// error of a few least significant bits, and it is **not** evidence of
/// correctness at any particular quality. `docs/features/xps.md` records the
/// quantised path as unadjudicated for that reason.
#[test]
fn a_coarser_quantizer_decodes_further_from_the_source() {
    let dir = fixture_dir();
    let limits = Limits::new(1 << 24);
    let mut errors = Vec::new();
    for name in ["quant4", "quant16", "quant48"] {
        let spec = RASTERS
            .iter()
            .find(|f| f.name == name)
            .unwrap_or_else(|| panic!("{name} has no row in RASTERS"));
        let bytes = std::fs::read(dir.join(format!("{name}.jxr"))).expect("a committed fixture");
        let image = jxr_decode(&bytes, &limits).unwrap_or_else(|e| panic!("{name}: {e}"));
        let want = expected_output(spec, image.format.channels, image.bits_per_component());
        // Total absolute error, which needs no floating point (ruling 4).
        let total: u64 = image
            .data
            .iter()
            .zip(&want)
            .map(|(a, b)| u64::from(a.abs_diff(*b)))
            .sum();
        errors.push((name, total));
    }
    // `QualityLevel` is the codec's own QP: 1 is lossless and larger is
    // coarser, so the error must rise across the three.
    for pair in errors.windows(2) {
        let [(finer_name, finer), (coarser_name, coarser)] = pair else {
            continue;
        };
        assert!(
            finer < coarser,
            "{finer_name} ({finer}) should decode closer than {coarser_name} ({coarser})"
        );
    }
    // And the finest of the three is still visibly lossy, which is what makes
    // the comparison meaningful rather than three readings of zero.
    assert!(errors[0].1 > 0, "quant4 decoded losslessly");
}

/// Every fixture decodes, including the lossy ones, at the geometry it was
/// authored with.
///
/// Separate from the identity because it covers what the identity cannot: a
/// lossy decode has no exact answer, but "it decoded at all, at the right
/// size, without dropping a tile" is still a claim worth pinning.
#[test]
fn every_fixture_decodes_to_the_geometry_it_was_authored_with() {
    let dir = fixture_dir();
    let limits = Limits::new(1 << 24);
    let mut checked = 0;
    for spec in RASTERS {
        let path = dir.join(format!("{}.jxr", spec.name));
        let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        let image = jxr_decode(&bytes, &limits).unwrap_or_else(|e| panic!("{}: {e}", spec.name));
        assert_eq!(image.width, spec.width, "{}", spec.name);
        assert_eq!(image.height, spec.height, "{}", spec.name);
        assert!(image.complete, "{} dropped a tile", spec.name);
        let expected_len = spec.width as usize
            * spec.height as usize
            * usize::from(image.channels())
            * usize::from(image.bits_per_component() / 8);
        assert_eq!(image.data.len(), expected_len, "{}", spec.name);
        checked += 1;
    }
    assert_eq!(checked, RASTERS.len());
}

/// The alpha fixtures really carry A.3.2's separate alpha plane, and it
/// really reaches the output.
///
/// Worth its own test because an opaque alpha channel is also what an
/// *absent* alpha plane produces: if the second `CODED_IMAGE( )` were being
/// skipped, `bgra32` would still decode, still be the right size, and be
/// wrong only where the source raster is transparent.
#[test]
fn the_alpha_fixtures_carry_a_separate_alpha_plane_that_is_not_all_opaque() {
    let dir = fixture_dir();
    let limits = Limits::new(1 << 24);
    for name in ["bgra32", "rgba64"] {
        let bytes = std::fs::read(dir.join(format!("{name}.jxr"))).expect("a committed fixture");
        let image = jxr_decode(&bytes, &limits).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert!(image.format.channels.has_alpha(), "{name} has no alpha");
        let channels = usize::from(image.channels());
        let per = usize::from(image.bits_per_component() / 8);
        let opaque = if per == 2 { 0xFFFFu16 } else { 0xFFu16 };
        let mut transparent = 0;
        let mut samples = 0;
        for px in image.data.chunks_exact(channels * per) {
            let at = (channels - 1) * per;
            let a = if per == 2 {
                u16::from_le_bytes([px[at], px[at + 1]])
            } else {
                u16::from(px[at])
            };
            samples += 1;
            if a != opaque {
                transparent += 1;
            }
        }
        assert!(
            transparent > 0,
            "{name}: all {samples} alpha samples are opaque, so the separate \
             alpha plane is not reaching the output"
        );
    }
}
