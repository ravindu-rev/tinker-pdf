//! Window decode (P-10.3): a decode asked for a rectangle of the image is
//! that rectangle of the whole decode, sample for sample, at every discard,
//! and decodes only the code-blocks whose synthesis reaches it.
//!
//! **What this holds is two paths of one decoder to each other**, as
//! `reduced.rs` does: the whole decode is the reference here, and that the
//! whole decode is right is `jpx_reference.rs`'s and the annexes' to say.
//! The windows are chosen for their edges — corners, single pixels, a row
//! and a column, rectangles past the image's far edges and sizes that
//! overflow `u32` beside their corner — and seeded ones besides, over every
//! committed codestream and over writer-made codestreams of many tiles and
//! many precincts, both filters, G.2's transform and a subsampled
//! component, all at an odd origin.
//!
//! The code-block count is held to a bound this file derives **forward**,
//! from each block to the samples its coefficients can reach — a sample
//! reaches the interleaved samples within the filter's reach of it at its
//! own level, and each level's samples are the next one's even positions —
//! rather than through the decoder's own backward reckoning (`Needs::of`),
//! so the bound is not the decoder's arithmetic restated.

use super::writer::{coded, At, Spec};
use crate::jpx::codestream::parse;
use crate::jpx::tier2::{layout_for_test, Orientation};
use crate::jpx::{jpx_decode_request, JpxCounts, JpxImage, JpxRequest, JpxWindow};
use crate::{Capability, FilterError, Limits};

fn limits() -> Limits {
    Limits::new(1 << 24)
}

/// `bytes` decoded dropping `discard` levels, over `window`.
fn decode(bytes: &[u8], discard: u8, window: Option<JpxWindow>) -> (JpxImage, JpxCounts) {
    let mut warnings = Vec::new();
    jpx_decode_request(
        bytes,
        &limits(),
        &JpxRequest { discard, window },
        &mut warnings,
    )
    .unwrap_or_else(|e| panic!("dropping {discard}, window {window:?}: {e:?} {warnings:?}"))
}

/// One window's side clamped to an image's: its start and its end.
fn side(at: u32, size: u32, extent: u32) -> (u32, u32) {
    let start = u64::from(at).min(u64::from(extent));
    let end = (u64::from(at) + u64::from(size)).min(u64::from(extent));
    (start as u32, end as u32)
}

/// What a decode over `window` must return of `whole`: its size, its samples
/// and its opacity channel's, the rectangle clamped to the image, and nothing
/// for a rectangle that holds no pixel of it.
fn crop(whole: &JpxImage, window: JpxWindow) -> (u32, u32, Vec<u8>, Option<Vec<u8>>) {
    let (x0, x1) = side(window.x, window.width, whole.width);
    let (y0, y1) = side(window.y, window.height, whole.height);
    if x0 >= x1 || y0 >= y1 {
        return (0, 0, Vec::new(), whole.opacity.as_ref().map(|_| Vec::new()));
    }
    let per = usize::from(whole.precision > 8) + 1;
    let rows = |samples: &[u8], pixel: usize| -> Vec<u8> {
        let mut out = Vec::new();
        for y in y0..y1 {
            let start = (y as usize * whole.width as usize + x0 as usize) * pixel;
            out.extend_from_slice(&samples[start..start + (x1 - x0) as usize * pixel]);
        }
        out
    };
    (
        x1 - x0,
        y1 - y0,
        rows(&whole.samples, usize::from(whole.components) * per),
        whole
            .opacity
            .as_ref()
            .map(|opacity| rows(&opacity.samples, per)),
    )
}

/// SplitMix64, so the seeded windows are the same on every target.
fn next(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// The windows an image of `width` by `height` is held at: the whole, its
/// first, last and middle pixels, a row and a column, an interior rectangle,
/// one past the far corner, one whose size overflows beside its corner, and
/// five seeded ones.
fn windows(width: u32, height: u32, seed: u64) -> Vec<JpxWindow> {
    let at = |x: u32, y: u32, width: u32, height: u32| JpxWindow {
        x,
        y,
        width,
        height,
    };
    let mut out = vec![
        at(0, 0, width, height),
        at(0, 0, 1, 1),
        at(width.saturating_sub(1), height.saturating_sub(1), 1, 1),
        at(width / 2, height / 2, 1, 1),
        at(0, height / 3, width, 1),
        at(width / 3, 0, 1, height),
        at(width / 4, height / 4, width / 2 + 1, height / 2 + 1),
        at(width.saturating_sub(3), height.saturating_sub(2), 10, 10),
        at(1, 1, u32::MAX, u32::MAX),
    ];
    let mut state = seed;
    let (w, h) = (u64::from(width.max(1)), u64::from(height.max(1)));
    for _ in 0..5 {
        let x = (next(&mut state) % w) as u32;
        let y = (next(&mut state) % h) as u32;
        let ww = 1 + (next(&mut state) % w) as u32;
        let hh = 1 + (next(&mut state) % h) as u32;
        out.push(at(x, y, ww, hh));
    }
    out
}

/// A window decode of `bytes` against the crop of its whole decode, at
/// `discard`, for every window of [`windows`].
fn holds_every_window(bytes: &[u8], discard: u8, case: &str, seed: u64) -> usize {
    let (whole, _) = decode(bytes, discard, None);
    let mut checked = 0;
    for window in windows(whole.width, whole.height, seed) {
        let (got, _) = decode(bytes, discard, Some(window));
        let (width, height, samples, opacity) = crop(&whole, window);
        let case = format!("{case}, dropping {discard}, {window:?}");
        assert_eq!((got.width, got.height), (width, height), "{case}");
        assert!(
            got.samples == samples,
            "{case}: the samples are not the crop"
        );
        assert_eq!(
            got.opacity.map(|opacity| opacity.samples),
            opacity,
            "{case}: the opacity is not the crop"
        );
        assert_eq!(
            (
                got.components,
                got.precision,
                got.colour,
                got.truncated,
                got.discard
            ),
            (
                whole.components,
                whole.precision,
                whole.colour,
                whole.truncated,
                whole.discard
            ),
            "{case}"
        );
        checked += 1;
    }
    checked
}

/// A coefficient for every position, from a seeded hash of where it sits,
/// as `reduced.rs` keys its own: image-scale in the lowest band, small
/// elsewhere, and whole blocks of zeros now and then.
fn noise(at: At) -> i32 {
    let band = match at.orientation {
        Orientation::Ll => 0u64,
        Orientation::Hl => 1,
        Orientation::Lh => 2,
        Orientation::Hh => 3,
    };
    let mut state = (at.component as u64) << 48
        ^ (at.resolution as u64) << 40
        ^ band << 36
        ^ u64::from(at.x) << 18
        ^ u64::from(at.y);
    let h = next(&mut state);
    if band == 0 {
        return (h % 201) as i32 - 100;
    }
    if (at.x / 4 + at.y / 4 + at.resolution as u32) % 4 == 0 {
        return 0;
    }
    (h % 61) as i32 - 30
}

/// A 37 x 29 image at the odd origin `(3, 5)`, in 16 x 16 tiles from the
/// grid's origin, three levels, 8 x 8 code-blocks and precincts of 8 and 16
/// samples (so each band holds several), over `components`.
fn tiled(reversible: bool, components: &[(u8, bool, u8, u8)], mct: bool) -> Spec {
    Spec {
        xsiz: 40,
        ysiz: 34,
        xosiz: 3,
        yosiz: 5,
        xtsiz: 16,
        ytsiz: 16,
        xtosiz: 0,
        ytosiz: 0,
        components: components.to_vec(),
        levels: 3,
        xcb: 1,
        ycb: 1,
        scod: 1,
        precincts: vec![0x33, 0x33, 0x44, 0x44],
        mct: u8::from(mct),
        transform: u8::from(reversible),
        quant_style: if reversible { 0 } else { 2 },
        ..Spec::default()
    }
}

#[test]
fn a_window_decode_is_the_crop_of_the_full_decode() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/jpx");
    let mut names: Vec<String> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("{}: {e}", dir.display()))
        .map(|entry| entry.expect("an entry").file_name())
        .filter_map(|name| name.into_string().ok())
        .filter(|name| name.ends_with(".jp2"))
        .collect();
    names.sort();
    assert!(names.len() > 30, "the committed codestreams: {names:?}");
    let mut checked = 0;
    for (k, name) in names.iter().enumerate() {
        let bytes = std::fs::read(dir.join(name)).unwrap_or_else(|e| panic!("{name}: {e}"));
        let levels = decode(&bytes, u8::MAX, None).0.discard;
        for discard in 0..=levels {
            checked += holds_every_window(&bytes, discard, name, k as u64);
        }
    }

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
    for reversible in [true, false] {
        for (shape, components, mct) in shapes {
            let bytes = coded(&tiled(reversible, components, mct), &noise);
            let filter = if reversible { "5/3" } else { "9/7" };
            for discard in 0..=3 {
                checked += holds_every_window(
                    &bytes,
                    discard,
                    &format!("{filter} in tiles and precincts, {shape}"),
                    u64::from(discard) + 100,
                );
            }
        }
    }
    assert!(checked > 1000, "{checked} windows");
}

/// The blocks whose coefficients can reach `window` — a rectangle of the
/// reduced image, in its own pixels — of a one-component codestream whose
/// component is not subsampled, dropping `discard` of its `levels`, counted
/// forward from each block.
fn reachable(bytes: &[u8], levels: u8, discard: u8, reach: i64, window: JpxWindow) -> u64 {
    let stream = parse(bytes).expect("it parses");
    let siz = &stream.siz;
    let shrink = |v: u32| i64::from(v.div_ceil(1 << discard));
    let (gx0, gy0) = (shrink(siz.xosiz), shrink(siz.yosiz));
    let (gx1, gy1) = (shrink(siz.xsiz), shrink(siz.ysiz));
    let wx0 = (gx0 + i64::from(window.x)).min(gx1);
    let wy0 = (gy0 + i64::from(window.y)).min(gy1);
    let wx1 = (gx0 + i64::from(window.x) + i64::from(window.width)).min(gx1);
    let wy1 = (gy0 + i64::from(window.y) + i64::from(window.height)).min(gy1);
    let top = usize::from(levels - discard);
    let tiles = u64::from(siz.tiles_x) * u64::from(siz.tiles_y);
    let mut count = 0;
    for t in 0..tiles {
        let (tile, _) = layout_for_test(&stream, t as u32).expect("it lays out");
        let tc = &tile.components[0];
        let res = &tc.resolutions[top];
        // The tile's own part of the top resolution: no sample outside it is
        // the tile's to write.
        let (tx0, ty0, tx1, ty1) = (
            i64::from(res.x0).max(wx0),
            i64::from(res.y0).max(wy0),
            i64::from(res.x1).min(wx1),
            i64::from(res.y1).min(wy1),
        );
        if tx0 >= tx1 || ty0 >= ty1 {
            continue;
        }
        for (r, resolution) in tc.resolutions.iter().enumerate().take(top + 1) {
            for band in &resolution.bands {
                let (hx, hy) = match band.orientation {
                    Orientation::Ll => (0, 0),
                    Orientation::Hl => (1, 0),
                    Orientation::Lh => (0, 1),
                    Orientation::Hh => (1, 1),
                };
                for precinct in &band.precincts {
                    for block in &precinct.blocks {
                        let span = |lo: u32, hi: u32, high: i64| -> (i64, i64) {
                            let (lo, hi) = (i64::from(lo), i64::from(hi) - 1);
                            let (mut lo, mut hi) = if r == 0 {
                                (lo, hi)
                            } else {
                                (2 * lo + high - reach, 2 * hi + high + reach)
                            };
                            for _ in r + 1..=top {
                                lo = 2 * lo - reach;
                                hi = 2 * hi + reach;
                            }
                            (lo, hi)
                        };
                        let (x0, x1) = span(block.x0, block.x1, hx);
                        let (y0, y1) = span(block.y0, block.y1, hy);
                        if x0 < tx1 && tx0 <= x1 && y0 < ty1 && ty0 <= y1 {
                            count += 1;
                        }
                    }
                }
            }
        }
    }
    count
}

/// A window decodes no code-block whose coefficients cannot reach it —
/// counted forward by [`reachable`] — and so none of a tile it misses, and
/// strictly fewer than the whole decode where it misses something; the
/// whole image as a window decodes what no window does.
#[test]
fn a_window_decodes_only_the_code_blocks_it_reaches() {
    for reversible in [true, false] {
        let bytes = coded(&tiled(reversible, &[(8, false, 1, 1)], false), &noise);
        let reach = if reversible { 2 } else { 4 };
        for discard in 0..=3u8 {
            let (whole, all) = decode(&bytes, discard, None);
            let every = windows(whole.width, whole.height, u64::from(discard));
            for window in &every {
                let (_, counts) = decode(&bytes, discard, Some(*window));
                let bound = reachable(&bytes, 3, discard, reach, *window);
                assert!(
                    counts.code_blocks_decoded <= bound,
                    "dropping {discard}, {window:?}: {} decoded, {bound} reachable",
                    counts.code_blocks_decoded
                );
                assert!(counts.code_blocks_decoded <= all.code_blocks_decoded);
            }
            let (_, full) = decode(&bytes, discard, Some(every[0]));
            assert_eq!(
                full, all,
                "dropping {discard}: the whole image as a window is no window"
            );
            if whole.width > 4 && whole.height > 4 {
                let (_, pixel) = decode(&bytes, discard, Some(every[1]));
                assert!(
                    pixel.code_blocks_decoded < all.code_blocks_decoded,
                    "dropping {discard}: one corner pixel decodes {} of {} blocks",
                    pixel.code_blocks_decoded,
                    all.code_blocks_decoded
                );
                assert!(pixel.levels_synthesised < all.levels_synthesised);
            }
        }
    }
}

/// A window that holds no pixel of the image — past its right or bottom
/// edge, of no width or no height, or past both with sizes that overflow
/// beside it — is an empty image, decodes no code-block and synthesises no
/// level; tier-2 still reads every packet header.
#[test]
fn a_window_off_the_image_is_empty() {
    let bytes = coded(&tiled(false, &[(8, false, 1, 1); 3], true), &noise);
    for discard in 0..=3u8 {
        let (whole, _) = decode(&bytes, discard, None);
        for window in [
            JpxWindow {
                x: whole.width,
                y: 0,
                width: 5,
                height: 5,
            },
            JpxWindow {
                x: 0,
                y: whole.height,
                width: 5,
                height: 5,
            },
            JpxWindow {
                x: 0,
                y: 0,
                width: 0,
                height: 5,
            },
            JpxWindow {
                x: 0,
                y: 0,
                width: 5,
                height: 0,
            },
            JpxWindow {
                x: u32::MAX,
                y: u32::MAX,
                width: u32::MAX,
                height: u32::MAX,
            },
        ] {
            let (image, counts) = decode(&bytes, discard, Some(window));
            assert_eq!((image.width, image.height), (0, 0), "{window:?}");
            assert!(image.samples.is_empty(), "{window:?}");
            assert_eq!(counts, JpxCounts::default(), "{window:?}");
            assert_eq!(image.discard, whole.discard);
        }
    }
}

/// Damaged codestreams — cut short at every length and with bytes
/// overwritten — decoded over windows at every discard: each decodes into an
/// image its own geometry describes or refuses with the one named capability
/// and a warning, none panics (ruling 1), and a window refuses only a
/// codestream the whole decode refuses too, since it decodes a part of what
/// the whole decode does.
#[test]
fn a_window_refuses_only_what_the_whole_decode_refuses() {
    let bytes = coded(&tiled(false, &[(8, false, 1, 1); 3], true), &noise);
    let mut damaged: Vec<Vec<u8>> = (1..bytes.len())
        .step_by(7)
        .map(|n| bytes[..n].to_vec())
        .collect();
    for at in (0..bytes.len()).step_by(13) {
        let mut flipped = bytes.clone();
        flipped[at] ^= 0xA5;
        damaged.push(flipped);
    }
    let picks = [
        JpxWindow {
            x: 0,
            y: 0,
            width: 1,
            height: 1,
        },
        JpxWindow {
            x: 5,
            y: 3,
            width: 9,
            height: 7,
        },
        JpxWindow {
            x: 20,
            y: 10,
            width: u32::MAX,
            height: 2,
        },
    ];
    for input in &damaged {
        for discard in [0u8, 1, 3] {
            let mut warnings = Vec::new();
            let whole = jpx_decode_request(
                input,
                &limits(),
                &JpxRequest {
                    discard,
                    window: None,
                },
                &mut warnings,
            );
            for window in picks {
                let mut warnings = Vec::new();
                let got = jpx_decode_request(
                    input,
                    &limits(),
                    &JpxRequest {
                        discard,
                        window: Some(window),
                    },
                    &mut warnings,
                );
                match got {
                    Ok((image, _)) => {
                        let per = usize::from(image.precision > 8) + 1;
                        assert_eq!(
                            image.samples.len(),
                            image.width as usize
                                * image.height as usize
                                * usize::from(image.components)
                                * per
                        );
                    }
                    Err(error) => {
                        assert_eq!(error, FilterError::Unsupported(Capability::Jpx));
                        assert!(!warnings.is_empty(), "a refusal names its reason");
                        assert!(
                            whole.is_err(),
                            "dropping {discard}, {window:?}: a window refused what the \
                             whole decode decodes"
                        );
                    }
                }
            }
        }
    }
}

/// What a decode came to, for holding a window's to the whole image's: the
/// image, or the warnings its refusal left — E.1's clamp left out, since a
/// window clamps only the coefficients it reads.
fn outcome(
    input: &[u8],
    discard: u8,
    window: Option<JpxWindow>,
) -> Result<JpxImage, Vec<crate::Warning>> {
    let mut warnings = Vec::new();
    jpx_decode_request(
        input,
        &limits(),
        &JpxRequest { discard, window },
        &mut warnings,
    )
    .map(|(image, _)| image)
    .map_err(|error| {
        assert_eq!(error, FilterError::Unsupported(Capability::Jpx));
        warnings.retain(|warning| *warning != crate::Warning::JpxCoefficientClamped);
        warnings
    })
}

/// **A window refuses whatever the whole decode refuses, and for the same
/// reason**: the converse of the test above. Damaged codestreams of many
/// tiles — cut short and overwritten, one of them with D.5's segmentation
/// symbols at the end of every cleanup pass — decoded over windows at every
/// discard: wherever the whole image's decode refuses, every window's does,
/// with the same warning, though the damage lies in a code-block or a tile
/// the window does not read — its pass counts and its cost are accounted
/// for, a block under segmentation symbols is decoded, and a tile's
/// structure checked, all as the whole decode does them — and wherever the
/// whole decode draws, the window is its crop.
#[test]
fn a_window_refuses_whatever_the_whole_decode_refuses() {
    let codestreams = [
        coded(&tiled(false, &[(8, false, 1, 1); 3], true), &noise),
        coded(
            &Spec {
                cb_style: 0x20,
                ..tiled(true, &[(8, false, 1, 1)], false)
            },
            &noise,
        ),
    ];
    let picks = [
        JpxWindow {
            x: 0,
            y: 0,
            width: 1,
            height: 1,
        },
        JpxWindow {
            x: 5,
            y: 3,
            width: 9,
            height: 7,
        },
        JpxWindow {
            x: 20,
            y: 10,
            width: u32::MAX,
            height: 2,
        },
    ];
    let (mut refused, mut drawn) = (0, 0);
    for bytes in &codestreams {
        let mut damaged: Vec<Vec<u8>> = (1..bytes.len())
            .step_by(11)
            .map(|n| bytes[..n].to_vec())
            .collect();
        for at in (0..bytes.len()).step_by(7) {
            let mut flipped = bytes.clone();
            flipped[at] ^= 0x5A;
            damaged.push(flipped);
        }
        for input in &damaged {
            for discard in [0u8, 1, 3] {
                let whole = outcome(input, discard, None);
                for window in picks {
                    let case = format!("dropping {discard}, {window:?}");
                    match (&whole, outcome(input, discard, Some(window))) {
                        (Err(want), Err(got)) => {
                            assert_eq!(&got, want, "{case}: refused for another reason");
                            refused += 1;
                        }
                        (Ok(whole), Ok(got)) => {
                            let (width, height, samples, opacity) = crop(whole, window);
                            assert_eq!((got.width, got.height), (width, height), "{case}");
                            assert!(got.samples == samples, "{case}: the samples");
                            assert_eq!(
                                got.opacity.map(|opacity| opacity.samples),
                                opacity,
                                "{case}"
                            );
                            drawn += 1;
                        }
                        (Err(want), Ok(_)) => {
                            panic!("{case}: a window drew what the whole decode refuses: {want:?}")
                        }
                        (Ok(_), Err(got)) => {
                            panic!("{case}: a window refused what the whole decode draws: {got:?}")
                        }
                    }
                }
            }
        }
    }
    assert!(refused > 0 && drawn > 0, "{refused} refused, {drawn} drawn");
}

/// **A tile a window misses is still the codestream's**: four tiles, the
/// last of which a tile-part COD gives the 9/7 under the main header's
/// quantisation for the 5/3, which Table A.28 does not pair. The whole
/// decode refuses it as a structure, and so does a window over the first
/// tile alone, which reconstructs nothing of the last; with the last tile
/// left as the main header has it, both draw.
#[test]
fn a_window_refuses_a_tile_it_misses_for_its_structure() {
    use super::writer::tile_part;
    use crate::jpx::codestream::marker::EOC;

    let spec = Spec {
        xsiz: 32,
        ysiz: 32,
        xosiz: 0,
        yosiz: 0,
        ..tiled(true, &[(8, false, 1, 1)], false)
    };
    let bytes = coded(&spec, &noise);
    // The tile-parts `coded` wrote, each SOT, no header, SOD and its data.
    let main = spec.main_header();
    let mut parts = Vec::new();
    let mut at = main.len();
    while bytes.get(at..at + 2) == Some(&[0xFF, 0x90][..]) {
        let psot = u32::from_be_bytes([bytes[at + 6], bytes[at + 7], bytes[at + 8], bytes[at + 9]]);
        let end = at + psot as usize;
        parts.push(bytes[at + 14..end].to_vec());
        at = end;
    }
    assert_eq!(parts.len(), 4, "four tiles of sixteen");
    let rewritten = |last: &[u8]| {
        let mut out = main.clone();
        for (t, data) in parts.iter().enumerate() {
            let header: &[u8] = if t == 3 { last } else { &[] };
            out.extend(tile_part(t as u16, 0, 1, header, data));
        }
        out.extend_from_slice(&EOC.to_be_bytes());
        out
    };
    let first = Some(JpxWindow {
        x: 0,
        y: 0,
        width: 4,
        height: 4,
    });
    let plain = rewritten(&[]);
    assert!(
        outcome(&plain, 0, None).is_ok(),
        "the tiles as written draw"
    );
    assert!(outcome(&plain, 0, first).is_ok());
    let unpaired = rewritten(
        &Spec {
            transform: 0,
            ..spec.clone()
        }
        .cod(),
    );
    let whole = outcome(&unpaired, 0, None).expect_err("the whole decode refuses");
    assert_eq!(whole, vec![crate::Warning::JpxStructureInvalid]);
    assert_eq!(
        outcome(&unpaired, 0, first).expect_err("so does the first tile's window"),
        whole
    );
}
