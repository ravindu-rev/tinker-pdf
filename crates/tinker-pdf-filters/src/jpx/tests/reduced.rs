//! Resolution-level discard (T.800 B.5): a decode that drops the top `d`
//! resolutions of an `N`-level codestream is the full decode of a codestream
//! of `N - d` levels carrying the same lower bands.
//!
//! **What this holds is two paths of one decoder to each other.** Both
//! codestreams are written by this repository's own test writer
//! (`writer::coded`) from the same coefficients, keyed by where each sits —
//! its component, its resolution counted from the bottom, its subband and its
//! position — so the bands the reduced codestream carries are the full one's
//! lower bands sample for sample. B.5 is what makes them line up: resolution
//! `r` of a tile-component of `N` levels is the tile-component divided by
//! `2^(N - r)` rounded up (B-14), and dividing the reference grid by `2^d`
//! first, rounded up, then by `2^(N - d - r)` is the same division. That the
//! full decode is *right* is `jpx_reference.rs`'s and the annexes' to say;
//! this says the reduced path computes what the full path would.
//!
//! The 9/7 codestreams carry expounded step sizes, one a subband, so a band
//! dequantises to the same coefficients whether it sits under `N` levels or
//! `N - d`: a derived step depends on the level count.

use super::writer::{boxed, coded, At, Spec, SIGNATURE};
use crate::jpx::codestream::parse;
use crate::jpx::tier2::{layout_for_test, Orientation};
use crate::jpx::{jpx_decode, jpx_decode_request, jpx_header, JpxImage, JpxRequest};
use crate::{Capability, FilterError, Limits};

/// Which of Annex F's filters a codestream is coded with.
#[derive(Clone, Copy, Debug)]
enum Filter {
    Reversible,
    Irreversible,
}

/// A coefficient for every position, from a seeded hash of where it sits:
/// image-scale in the lowest band and small elsewhere, and zero over whole
/// code-blocks of the highest bands now and then, so a packet leaves blocks
/// out. Every magnitude fits the writer's `Mb` of nine planes.
fn noise(at: At) -> i32 {
    let band = match at.orientation {
        Orientation::Ll => 0u64,
        Orientation::Hl => 1,
        Orientation::Lh => 2,
        Orientation::Hh => 3,
    };
    let mut h = (at.component as u64)
        .wrapping_mul(0x9E37_79B9_7F4A_7C15)
        .wrapping_add((at.resolution as u64).wrapping_mul(0xBF58_476D_1CE4_E5B9))
        .wrapping_add(band.wrapping_mul(0x94D0_49BB_1331_11EB))
        .wrapping_add(u64::from(at.x).wrapping_mul(0x2545_F491_4F6C_DD1D))
        .wrapping_add(u64::from(at.y).wrapping_mul(0x6A09_E667_F3BC_C909));
    h ^= h >> 30;
    h = h.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    h ^= h >> 27;
    h = h.wrapping_mul(0x94D0_49BB_1331_11EB);
    h ^= h >> 31;
    if band == 0 {
        return (h % 201) as i32 - 100;
    }
    if (at.x / 8 + at.y / 8 + at.resolution as u32) % 3 == 0 {
        return 0;
    }
    (h % 61) as i32 - 30
}

/// A 37 x 29 image at an odd origin, one tile, 8 x 8 code-blocks, `levels`
/// levels of `filter`, over `components`.
fn spec(filter: Filter, levels: u8, components: &[(u8, bool, u8, u8)], mct: bool) -> Spec {
    Spec {
        xsiz: 40,
        ysiz: 34,
        xosiz: 3,
        yosiz: 5,
        xtsiz: 40,
        ytsiz: 34,
        xtosiz: 0,
        ytosiz: 0,
        components: components.to_vec(),
        levels,
        xcb: 1,
        ycb: 1,
        mct: u8::from(mct),
        transform: match filter {
            Filter::Reversible => 1,
            Filter::Irreversible => 0,
        },
        quant_style: match filter {
            Filter::Reversible => 0,
            Filter::Irreversible => 2,
        },
        ..Spec::default()
    }
}

/// `spec` with its top `d` levels gone: its grid divided by `2^d`, rounded
/// up (B.5), one tile over it, and `levels - d` levels.
fn reduced(spec: &Spec, d: u8) -> Spec {
    let r = |v: u32| v.div_ceil(1 << d);
    Spec {
        xsiz: r(spec.xsiz),
        ysiz: r(spec.ysiz),
        xosiz: r(spec.xosiz),
        yosiz: r(spec.yosiz),
        xtsiz: r(spec.xsiz),
        ytsiz: r(spec.ysiz),
        xtosiz: 0,
        ytosiz: 0,
        levels: spec.levels - d,
        ..spec.clone()
    }
}

fn limits() -> Limits {
    Limits::new(1 << 24)
}

/// `bytes` decoded with `discard` levels dropped.
fn decode_dropping(bytes: &[u8], discard: u8) -> JpxImage {
    let mut warnings = Vec::new();
    let (image, _) = jpx_decode_request(
        bytes,
        &limits(),
        &JpxRequest {
            discard,
            window: None,
        },
        &mut warnings,
    )
    .unwrap_or_else(|e| panic!("a reduced decode: {e:?} {warnings:?}"));
    image
}

/// `bytes` decoded whole.
fn decode_whole(bytes: &[u8]) -> JpxImage {
    let mut warnings = Vec::new();
    jpx_decode(bytes, &limits(), &mut warnings)
        .unwrap_or_else(|e| panic!("a full decode: {e:?} {warnings:?}"))
}

/// Every field of `got` but the discard it reports, against `want`'s.
fn same_picture(got: &JpxImage, want: &JpxImage, case: &str) {
    assert_eq!((got.width, got.height), (want.width, want.height), "{case}");
    assert_eq!(got.components, want.components, "{case}");
    assert_eq!(got.precision, want.precision, "{case}");
    assert_eq!(got.colour, want.colour, "{case}");
    assert_eq!(got.truncated, want.truncated, "{case}");
    assert_eq!(got.opacity, want.opacity, "{case}");
    assert!(got.samples == want.samples, "{case}: the samples differ");
}

#[test]
fn a_reduced_decode_is_the_full_decode_of_its_own_low_bands() {
    let shapes: [(&str, &[(u8, bool, u8, u8)], bool); 3] = [
        ("grey", &[(8, false, 1, 1)], false),
        (
            "three components and G.2's transform",
            &[(8, false, 1, 1); 3],
            true,
        ),
        (
            "a component subsampled 2 x 2 beside a whole one",
            &[(8, false, 1, 1), (8, false, 2, 2)],
            false,
        ),
    ];
    let mut checked = 0;
    for filter in [Filter::Reversible, Filter::Irreversible] {
        for (shape, components, mct) in shapes {
            for levels in 1..=4u8 {
                let full = spec(filter, levels, components, mct);
                let bytes = coded(&full, &noise);
                let whole = decode_whole(&bytes);
                assert_eq!(whole.discard, 0);
                assert!(
                    whole.samples.iter().any(|&s| s != whole.samples[0]),
                    "{filter:?}, {shape}, {levels} levels: the picture is not flat"
                );
                assert_eq!(whole.levels, levels, "every level can be dropped");
                for d in 0..=levels {
                    let case = format!("{filter:?}, {shape}, {levels} levels, {d} dropped");
                    let got = decode_dropping(&bytes, d);
                    assert_eq!(got.discard, d, "{case}");
                    assert_eq!(got.levels, levels, "{case}: what can be dropped");
                    let want = decode_whole(&coded(&reduced(&full, d), &noise));
                    same_picture(&got, &want, &case);
                    if d == 0 {
                        assert_eq!(got, whole, "{case}: no level dropped is the full decode");
                    }
                    checked += 1;
                }
            }
        }
    }
    assert_eq!(checked, 2 * 3 * (2 + 3 + 4 + 5));
}

/// `spec` tiled 16 x 16 from the grid's origin with its top `d` levels gone:
/// the grid and the tiles both divided by `2^d` (B.5), which keeps every tile
/// edge where the full codestream's reduced tiles put it while 16 is a
/// multiple of `2^d`.
fn reduced_tiled(spec: &Spec, d: u8) -> Spec {
    let r = |v: u32| v.div_ceil(1 << d);
    Spec {
        xsiz: r(spec.xsiz),
        ysiz: r(spec.ysiz),
        xosiz: r(spec.xosiz),
        yosiz: r(spec.yosiz),
        xtsiz: spec.xtsiz >> d,
        ytsiz: spec.ytsiz >> d,
        xtosiz: spec.xtosiz >> d,
        ytosiz: spec.ytosiz >> d,
        levels: spec.levels - d,
        ..spec.clone()
    }
}

/// The same over a grid of tiles: each tile's rectangle reduces as the image
/// does, edge tiles that the image cuts short included. Three levels at most,
/// so the first tile still holds an image sample once its 16 columns are
/// divided by eight and the origin of 3 by eight, rounded up (A.5.1 requires
/// it to).
#[test]
fn a_reduced_decode_of_many_tiles_is_the_full_decode_of_its_own_low_bands() {
    let mut checked = 0;
    for filter in [Filter::Reversible, Filter::Irreversible] {
        for (components, mct) in [
            (&[(8, false, 1, 1)][..], false),
            (&[(8, false, 1, 1); 3][..], true),
        ] {
            for levels in 1..=3u8 {
                let full = Spec {
                    xtsiz: 16,
                    ytsiz: 16,
                    ..spec(filter, levels, components, mct)
                };
                let bytes = coded(&full, &noise);
                for d in 0..=levels {
                    let case = format!(
                        "{filter:?}, {} components, {levels} levels in tiles, {d} dropped",
                        components.len()
                    );
                    let got = decode_dropping(&bytes, d);
                    assert_eq!(got.discard, d, "{case}");
                    let want = decode_whole(&coded(&reduced_tiled(&full, d), &noise));
                    same_picture(&got, &want, &case);
                    checked += 1;
                }
            }
        }
    }
    assert_eq!(checked, 2 * 2 * (2 + 3 + 4));
}

/// Hostile input at every discard: a writer-made codestream cut short at
/// every length and with bytes overwritten, each decoded dropping 0 to 4
/// levels. Every one either decodes into an image its own geometry describes,
/// with no more levels dropped than asked, or refuses with the one named
/// capability and a warning, and none panics (ruling 1).
#[test]
fn a_damaged_codestream_decodes_or_refuses_by_name_at_every_discard() {
    let full = Spec {
        xtsiz: 16,
        ytsiz: 16,
        ..spec(Filter::Irreversible, 3, &[(8, false, 1, 1); 3], true)
    };
    let bytes = coded(&full, &noise);
    let mut damaged: Vec<Vec<u8>> = (1..bytes.len())
        .step_by(5)
        .map(|n| bytes[..n].to_vec())
        .collect();
    for at in (0..bytes.len()).step_by(11) {
        let mut flipped = bytes.clone();
        flipped[at] ^= 0x5A;
        damaged.push(flipped);
    }
    let (mut decoded, mut refused) = (0, 0);
    for input in &damaged {
        for discard in 0..=4u8 {
            let mut warnings = Vec::new();
            match jpx_decode_request(
                input,
                &limits(),
                &JpxRequest {
                    discard,
                    window: None,
                },
                &mut warnings,
            ) {
                Ok((image, _)) => {
                    assert!(image.discard <= discard);
                    let per = usize::from(image.precision > 8) + 1;
                    assert_eq!(
                        image.samples.len(),
                        image.width as usize
                            * image.height as usize
                            * usize::from(image.components)
                            * per,
                        "an image its own geometry describes"
                    );
                    decoded += 1;
                }
                Err(error) => {
                    assert_eq!(error, FilterError::Unsupported(Capability::Jpx));
                    assert!(
                        !warnings.is_empty(),
                        "a refusal names its reason (ruling 10)"
                    );
                    refused += 1;
                }
            }
            let mut seen: Vec<&str> = warnings.iter().map(|w| w.as_str()).collect();
            seen.sort_unstable();
            seen.dedup();
            assert_eq!(seen.len(), warnings.len(), "a warning recorded twice");
        }
    }
    assert!(
        decoded > 0 && refused > 0,
        "{decoded} decoded, {refused} refused"
    );
}

/// B.5's grid, on its own: the reduced image is the full one's area divided
/// by `2^d` and rounded up at both ends, so an odd origin can make it a
/// sample wider or narrower than half the full width. The header read says
/// where that origin is, which is what a caller placing the reduced image
/// over the full one reads its first cell's start from.
#[test]
fn a_reduced_image_lies_on_the_reference_grid_divided_by_two_to_the_d() {
    let full = spec(Filter::Reversible, 3, &[(8, false, 1, 1)], false);
    let bytes = coded(&full, &noise);
    let header = jpx_header(&bytes, &limits()).expect("the header reads");
    assert_eq!(header.origin, (3, 5), "XOsiz and YOsiz");
    assert_eq!((header.width, header.height), (37, 29), "the image area");
    for d in 0..=3u8 {
        let image = decode_dropping(&bytes, d);
        let r = |v: u32| v.div_ceil(1 << d);
        assert_eq!(
            (image.width, image.height),
            (r(40) - r(3), r(34) - r(5)),
            "{d} dropped"
        );
        assert_eq!(image.samples.len(), (image.width * image.height) as usize);
    }
}

/// A discard past the levels a codestream has is clamped to them, never a
/// panic, and the image says how many it dropped.
#[test]
fn a_discard_past_the_levels_is_clamped_to_them() {
    let full = spec(Filter::Irreversible, 2, &[(8, false, 1, 1)], false);
    let bytes = coded(&full, &noise);
    let deepest = decode_dropping(&bytes, 2);
    assert_eq!(deepest.levels, 2, "the most it can drop");
    for asked in [3u8, 7, 32, u8::MAX] {
        let image = decode_dropping(&bytes, asked);
        assert_eq!(image.discard, 2, "asked for {asked}");
        assert_eq!(image, deepest, "asked for {asked}");
    }
}

/// `codestream`, one component of `width` by `height` eight-bit samples,
/// wrapped in JP2 boxes whose `pclr` maps the component through a palette of
/// 256 entries in three columns (T.800 I.5.3.4, I.5.3.5): entry `i` is
/// `(i, 255 - i, i / 2)`, so no two entries share a colour.
fn through_a_palette(codestream: &[u8], width: u32, height: u32) -> Vec<u8> {
    let mut pclr = vec![0x01, 0x00, 3, 7, 7, 7]; // NE = 256, NPC = 3, 8-bit unsigned
    for i in 0..=255u8 {
        pclr.extend_from_slice(&[i, 255 - i, i / 2]);
    }
    let cmap: Vec<u8> = (0..3u8).flat_map(|column| [0, 0, 1, column]).collect();
    let mut ihdr = height.to_be_bytes().to_vec();
    ihdr.extend_from_slice(&width.to_be_bytes());
    ihdr.extend_from_slice(&[0, 1, 7, 7, 0, 0]);
    let mut header = boxed(*b"ihdr", &ihdr);
    header.extend(boxed(*b"colr", &[1, 0, 0, 0, 0, 0, 16])); // EnumCS 16, sRGB
    header.extend(boxed(*b"pclr", &pclr));
    header.extend(boxed(*b"cmap", &cmap));
    let mut file = SIGNATURE.to_vec();
    file.extend(boxed(*b"ftyp", b"jp2 \0\0\0\0jp2 "));
    file.extend(boxed(*b"jp2h", &header));
    file.extend(boxed(*b"jp2c", codestream));
    file
}

/// **A codestream a palette reads drops no level** (`JpxImage::levels`): its
/// component's samples are indices, and a lower resolution averages two
/// indices into a third that selects a colour neither sample has. So asked
/// for any discard it decodes whole and says it dropped none and can drop
/// none, sample for sample the decode that asked for none — while the same
/// codestream bare, its samples grey levels, drops what it is asked to. A
/// window of it is still a window: a crop averages nothing.
#[test]
fn a_codestream_a_palette_reads_is_decoded_whole_at_every_discard() {
    let full = Spec {
        xosiz: 0,
        yosiz: 0,
        ..spec(Filter::Reversible, 3, &[(8, false, 1, 1)], false)
    };
    let bare = coded(&full, &noise);
    let indexed = through_a_palette(&bare, 40, 34);
    let whole = decode_whole(&indexed);
    assert_eq!((whole.width, whole.height, whole.components), (40, 34, 3));
    assert_eq!(
        (whole.discard, whole.levels),
        (0, 0),
        "nothing can be dropped"
    );
    for asked in [1u8, 2, 3, 9, u8::MAX] {
        let image = decode_dropping(&indexed, asked);
        assert_eq!(image.discard, 0, "asked for {asked}");
        assert_eq!(image, whole, "asked for {asked}: the whole decode");
        let grey = decode_dropping(&bare, asked);
        assert_eq!(grey.levels, 3, "asked for {asked}");
        assert_eq!(
            grey.discard,
            asked.min(3),
            "asked for {asked}: the bare one drops"
        );
    }
    let mut warnings = Vec::new();
    let (cropped, _) = jpx_decode_request(
        &indexed,
        &limits(),
        &JpxRequest {
            discard: 2,
            window: Some(crate::jpx::JpxWindow {
                x: 5,
                y: 7,
                width: 11,
                height: 9,
            }),
        },
        &mut warnings,
    )
    .expect("a window of it decodes");
    assert_eq!((cropped.width, cropped.height, cropped.discard), (11, 9, 0));
    let crop: Vec<u8> = (7..16usize)
        .flat_map(|y| {
            let start = (y * 40 + 5) * 3;
            whole.samples[start..start + 11 * 3].iter().copied()
        })
        .collect();
    assert!(
        cropped.samples == crop,
        "the window is the whole decode's crop"
    );
}

/// Guard A7's leaf half (`docs/design/jpx-speed.md`, "Guard A7's JPX
/// half"): a decode dropping `d` of `N` levels synthesises `N - d` a
/// tile-component and decodes the code-blocks of the kept resolutions and no
/// others, over two tiles of three components.
#[test]
fn guard_a7_levels_decoded_are_the_levels_needed() {
    let levels = 3u8;
    let full = Spec {
        xtsiz: 20,
        ..spec(Filter::Irreversible, levels, &[(8, false, 1, 1); 3], true)
    };
    let bytes = coded(&full, &noise);
    let parsed = parse(&bytes).expect("it parses");
    let tiles: Vec<_> = (0..2)
        .map(|t| layout_for_test(&parsed, t).expect("it lays out").0)
        .collect();
    let blocks_up_to = |kept: usize| -> u64 {
        tiles
            .iter()
            .flat_map(|tile| tile.components.iter())
            .flat_map(|component| component.resolutions.iter().take(kept))
            .flat_map(|resolution| resolution.bands.iter())
            .flat_map(|band| band.precincts.iter())
            .map(|precinct| precinct.blocks.len() as u64)
            .sum()
    };
    let every = blocks_up_to(usize::from(levels) + 1);
    let mut previous = u64::MAX;
    for d in 0..=levels {
        let mut warnings = Vec::new();
        let (image, counts) = jpx_decode_request(
            &bytes,
            &limits(),
            &JpxRequest {
                discard: d,
                window: None,
            },
            &mut warnings,
        )
        .expect("it decodes");
        assert_eq!(image.discard, d);
        assert_eq!(
            counts.levels_synthesised,
            2 * 3 * u64::from(levels - d),
            "{d} dropped: two tiles of three components, {} levels each",
            levels - d
        );
        let kept = blocks_up_to(usize::from(levels - d) + 1);
        assert_eq!(counts.code_blocks_decoded, kept, "{d} dropped");
        assert!(
            kept < previous,
            "{d} dropped decodes fewer blocks than one fewer"
        );
        assert!(d == 0 || kept < every, "{d} dropped");
        previous = kept;
    }
}
