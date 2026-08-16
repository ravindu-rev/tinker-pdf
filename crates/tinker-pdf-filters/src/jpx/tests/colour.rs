//! Milestone 6: T.800 Annex G's component transforms and Annex I's channels.
//!
//! # What is worth testing here and what is not
//!
//! Almost nothing in this file is a picture. Milestones 4 and 5 established
//! why: a unit test asserting that a colour decode "looks like an image" is
//! worth close to nothing, because the inverse wavelet is a smoothing
//! operator and a wrong colour pipeline downstream of it produces a
//! photograph with a cast rather than an error. Milestone 4's halved
//! dequantisation passed every test in the repository and only the oracle
//! caught it; replacing symmetric extension with zero padding left all 225
//! unit tests green and failed only the oracle.
//!
//! So the evidence is split the way the plan splits it:
//!
//! - **The inverse RCT is exact integer arithmetic**, so it does not need a
//!   gate of its own: it extends gate 1. A losslessly coded RGB stream comes
//!   back byte-identical to `opj_decompress` in `tests/jpx_oracle.rs`, and no
//!   decoder with the two chrominance terms transposed can pass that.
//! - **The inverse ICT is the 9/7's partner and a float transform in the
//!   standard**, so it gets the 9/7's gate 2: the same ladder walked in `f64`
//!   over the same coefficients, with the *only* difference being the
//!   arithmetic.
//! - **The four Q24 constants are recomputed from G.2.2's decimals**, because
//!   milestone 5 injected one wavelet constant at Q16 and both comparison
//!   gates passed it. That is a relative error of one part in a million and
//!   only the recomputation sees it.
//! - **Annex I's boxes are arithmetic, not pictures.** A palette is a lookup
//!   table; its expected output is derived by hand rather than compared, and
//!   `opj_compress` cannot write a `pclr` box in any case, so a hand-built
//!   file is not a second-best here — it is the only material there is.

use super::wavelet::Reference;
use super::writer::{boxed, Spec, SIGNATURE};
use crate::jpx::boxes::{self, ChannelDef, ChannelMap, Jp2Header, Palette};
use crate::jpx::codestream::{self, Component};
use crate::jpx::colour::{self, ICT_B_CB, ICT_G_CB, ICT_G_CR, ICT_R_CR};
use crate::jpx::wavelet::{into_samples, ladder, level_shift, Fixed, QC};
use crate::jpx::{jpx_decode, tier1, tier2, JpxColour, Refusal};
use crate::Limits;

// --- T.800 G.2.2, as decimals ---------------------------------------------
//
// Transcribed from the standard and used two ways, exactly as Table F.4's six
// are: `tests::wavelet`'s `f64` reference arithmetic multiplies with them, and
// the recomputation below derives the shipped Q24 integers from them.
// Restating the integers here would let a transcription slip in the constant
// agree with a transcription slip in the test.
pub(super) const G2_R_CR: f64 = 1.402;
pub(super) const G2_G_CB: f64 = 0.344136;
pub(super) const G2_G_CR: f64 = 0.714136;
pub(super) const G2_B_CB: f64 = 1.772;

/// Every shipped ICT constant equals `round(c * 2^24)` recomputed from
/// G.2.2's decimals, at the **same Q24 the wavelet uses**.
///
/// The second half of that sentence is the point. Milestone 5's plan settled
/// one fixed-point format for the whole pixel path and proved its error
/// budget once; a colour transform carried at some other width would put half
/// the path outside that proof while looking perfectly reasonable in
/// isolation. The assertion on `QC` is what makes this test fail if somebody
/// gives the ICT a format of its own.
#[test]
fn g2_ict_constants_are_recomputed_not_restated() {
    assert_eq!(
        QC, 24,
        "the ICT shares the wavelet's constant format by decision; a second \
         fixed-point width is the failure the plan was written to prevent"
    );
    let scale = 2.0f64.powi(QC as i32);
    let cases: &[(&str, f64, i64)] = &[
        ("1.402 (red from Cr)", G2_R_CR, ICT_R_CR),
        ("0.344136 (green from Cb)", G2_G_CB, ICT_G_CB),
        ("0.714136 (green from Cr)", G2_G_CR, ICT_G_CR),
        ("1.772 (blue from Cb)", G2_B_CB, ICT_B_CB),
    ];
    for &(name, decimal, shipped) in cases {
        let recomputed = (decimal * scale).round() as i64;
        assert_eq!(
            recomputed, shipped,
            "{name}: G.2.2's decimal rounds to {recomputed} at Q{QC}, not {shipped}"
        );
        let residual = (shipped as f64 / scale - decimal).abs();
        assert!(
            residual < 2.0f64.powi(-25),
            "{name}: residual {residual:e} is past the 2^-25 the plan's error \
             analysis assumes"
        );
    }
}

/// The two green terms are not interchangeable, and the red and blue ones are
/// not either.
///
/// A transposition here is invisible to inspection — swapping the two green
/// coefficients tints a picture and leaves everything else about it intact —
/// so the relationships are pinned by arithmetic. All four come from the same
/// forward matrix, and the identity that holds them together is that a
/// greyscale pixel (`Cb = Cr = 0`) must come back grey.
#[test]
fn the_ict_constants_are_the_ones_g22_assigns_to_each_channel() {
    // Red takes 1.402 and blue takes 1.772, so blue's is the larger; and green
    // subtracts more of Cr than of Cb. Both orderings survive a transposition
    // that no picture would show, which is why they are stated as numbers.
    let ordered: &[(&str, i64, i64)] = &[
        (
            "blue's Cb term is the larger of the two chrominance multipliers",
            ICT_B_CB,
            ICT_R_CR,
        ),
        ("green subtracts more of Cr than of Cb", ICT_G_CR, ICT_G_CB),
    ];
    for &(what, larger, smaller) in ordered {
        assert!(larger > smaller, "{what}: {larger} is not above {smaller}");
    }
    // A pixel with no chrominance is grey in all three channels, which is
    // what says the luma coefficient is 1 in each row rather than folded into
    // the constants.
    for y in [-2048i64, 0, 1 << 20] {
        assert_eq!(
            colour::inverse_ict(y, 0, 0),
            (y, y, y),
            "a chrominance-free pixel must come back grey"
        );
    }
}

/// G.2.1's inverse undoes G.2.1's forward, exactly, over the whole 8-bit
/// range and past it.
///
/// The RCT is reversible and that is the whole of its contract: there is no
/// tolerance to negotiate and no reconstruction point to choose. This is the
/// property the plan leans on when it says the RCT extends gate 1 rather than
/// needing a gate of its own.
#[test]
fn the_inverse_rct_is_exact_over_the_forward_one() {
    // G.2.1's forward transform (the encoder's, written here from the
    // equations so the round trip is not the shipped code arguing with
    // itself).
    let forward = |r: i64, g: i64, b: i64| -> (i64, i64, i64) {
        ((r + 2 * g + b).div_euclid(4), b - g, r - g)
    };
    let mut checked = 0;
    for r in (-128..=127).step_by(7) {
        for g in (-128..=127).step_by(5) {
            for b in (-128..=127).step_by(11) {
                let (y0, y1, y2) = forward(r, g, b);
                assert_eq!(
                    colour::inverse_rct(y0, y1, y2),
                    (r, g, b),
                    "the RCT is not reversible at ({r}, {g}, {b})"
                );
                checked += 1;
            }
        }
    }
    assert!(checked > 5_000, "only {checked} triples were checked");
}

/// The RCT's two chrominance terms carry red and blue *specifically*, and
/// swapping them is not caught by the round trip above.
///
/// That is the point of writing it separately: a forward transform with the
/// same two terms transposed round-trips just as exactly, because the error
/// cancels. Only a comparison against a decoder sharing no code can see it —
/// gate 1, on the RGB fixtures — and this test states the relationship so
/// that the gate's failure has a name.
#[test]
fn the_rct_puts_red_on_the_second_chrominance_and_blue_on_the_first() {
    // Forward: Y1 = B - G and Y2 = R - G. For (200, 100, 50) that is
    // Y0 = floor((200 + 200 + 50) / 4) = 112, Y1 = -50, Y2 = 100.
    assert_eq!(colour::inverse_rct(112, -50, 100), (200, 100, 50));
    // With the two swapped the same input decodes to (50, 100, 200): red and
    // blue exchanged, everything else identical, and a green-and-grey test
    // image would show nothing at all.
    assert_ne!(colour::inverse_rct(112, 100, -50), (200, 100, 50));
}

/// G.1's level shift runs **after** G.2's transform, and this is what it
/// costs to get the order wrong.
///
/// The shipped order is pinned by gate 1 rather than here — a reversible RGB
/// stream cannot be byte-identical to `opj_decompress` with the two swapped —
/// but the size of the error is worth writing down, because "a colour cast"
/// is not a number and a number is what makes a reviewer look.
///
/// Shifting first adds `2^(R-1)` to all three of Y0, Y1 and Y2. The RCT's
/// green is `Y0 - floor((Y1 + Y2) / 4)`, so the shift on Y0 is half cancelled
/// by the shift on the two chrominances and green comes out **64 low**; red
/// and blue are green plus a chrominance that is itself 128 high, so they
/// come out 64 high. Every pixel of an 8-bit image lands 64 levels out in
/// opposite directions — a uniform magenta cast on a picture that is
/// otherwise exactly right, which is the failure this codec is dangerous for
/// wearing yet another hat.
#[test]
fn the_dc_level_shift_belongs_after_the_component_transform() {
    let (y0, y1, y2) = (112i64, -50, 100);
    let shift = 128;
    let after = colour::inverse_rct(y0, y1, y2);
    let after = (after.0 + shift, after.1 + shift, after.2 + shift);
    let before = colour::inverse_rct(y0 + shift, y1 + shift, y2 + shift);
    assert_eq!(after, (328, 228, 178), "the standard's order");
    assert_eq!(before, (392, 164, 242), "shifted first");
    assert_eq!(
        (before.0 - after.0, before.1 - after.1, before.2 - after.2),
        (64, -64, 64),
        "shifting first is a magenta cast of half the shift, not a failure"
    );
}

// --- gate 2, extended to the ICT ------------------------------------------

fn fixture(name: &str) -> Vec<u8> {
    let path =
        std::path::PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/jpx")).join(name);
    std::fs::read(&path).unwrap_or_else(|e| panic!("{path:?}: {e}"))
}

/// Gate 2 over the whole irreversible path, wavelet **and** ICT: no sample
/// differs from the `f64` reference by more than one level of 255, and at
/// most 1 per cent differ at all.
///
/// Both sides walk the same ladder over the same dequantised coefficients and
/// then run G.2.2 — one in Q12 through `mul_q24`, one in `f64` from the
/// standard's decimals — so the only difference between them is the
/// arithmetic. That is what makes a one-level budget a gate rather than a
/// negotiation, and it is why the ICT does not get a looser test than the
/// wavelet it partners.
#[test]
fn the_fixed_point_ict_tracks_an_f64_reference_of_g22() {
    let cases: &[&str] = &["c1-i2", "c2-i2", "c3-i2", "c1-q20", "c2-q20", "c3-q20"];

    let mut compared = 0;
    for &name in cases {
        let bytes = fixture(&format!("{name}.jp2"));
        let container = boxes::parse(&bytes).expect("the fixture is a JP2");
        let stream = codestream::parse(container.codestream).expect("the codestream parses");
        assert!(
            stream.cod_for(0).mct && !stream.cod_for(0).style.reversible,
            "{name} is supposed to be an irreversible three-component fixture"
        );
        let mut tiles = tier2::decode_tiles(&stream).expect("tier-2 runs");
        tier1::decode_tiles(&stream, &mut tiles).expect("tier-1 runs");

        let (mut total, mut differing, mut worst) = (0usize, 0usize, 0i32);
        for tile in &tiles {
            let mut clamped = false;
            let mut fixed = Vec::new();
            let mut reference = Vec::new();
            for c in 0..3 {
                fixed.push(ladder::<Fixed>(&stream, tile, c, &mut clamped).expect("fixed"));
                reference
                    .push(ladder::<Reference>(&stream, tile, c, &mut clamped).expect("reference"));
            }
            assert!(!clamped, "{name} should not reach E.1's clamp");
            colour::inverse_mct(&mut fixed).expect("the fixed MCT runs");
            colour::inverse_mct(&mut reference).expect("the reference MCT runs");

            for (fixed, reference) in fixed.into_iter().zip(reference) {
                let mut fixed = into_samples(fixed);
                let mut reference = into_samples(reference);
                level_shift(&mut fixed, 8, false);
                level_shift(&mut reference, 8, false);
                for (&got, &want) in fixed.samples.iter().zip(&reference.samples) {
                    total += 1;
                    let moved = (got - want).abs();
                    worst = worst.max(moved);
                    if moved != 0 {
                        differing += 1;
                    }
                }
            }
        }

        assert!(
            worst <= 1,
            "{name}: a sample moved {worst} levels from the f64 reference of \
             G.2.2, where the fixed-point analysis bounds the whole path at an \
             eleventh of one"
        );
        let fraction = differing as f64 / total as f64;
        assert!(
            fraction <= 0.01,
            "{name}: {differing} of {total} samples differ from the f64 \
             reference ({fraction:.4}), past the 1 per cent budget"
        );
        compared += 1;
    }
    assert!(
        compared >= 6,
        "only {compared} fixtures were compared; a gate that silently stops \
         comparing is worse than no gate"
    );
}

// --- Annex I's channels ---------------------------------------------------

fn components(precisions: &[u8]) -> Vec<Component> {
    precisions
        .iter()
        .map(|&precision| Component {
            precision,
            signed: false,
            dx: 1,
            dy: 1,
        })
        .collect()
}

fn header(f: impl FnOnce(&mut Jp2Header)) -> Jp2Header {
    let mut h = Jp2Header {
        width: 4,
        height: 4,
        components: 1,
        bpc: Some((8, false)),
        bpcc: Vec::new(),
        colour: JpxColour::Srgb,
        palette: None,
        component_map: Vec::new(),
        channel_definition: Vec::new(),
    };
    f(&mut h);
    h
}

/// With no `cmap` the channels are the components, in order. That is the
/// common case and every oracle fixture takes it.
#[test]
fn without_a_cmap_the_channels_are_the_components() {
    let plan = colour::plan(None, &components(&[8, 8, 8])).expect("a plan");
    assert_eq!(plan.colour.len(), 3);
    assert_eq!(plan.precision, 8);
    assert!(plan.opacity.is_none());
    for (i, channel) in plan.colour.iter().enumerate() {
        assert_eq!(channel.component, i);
        assert_eq!(channel.column, None);
    }
}

/// I.5.3.4 and I.5.3.5: one component of indices becomes three channels.
#[test]
fn a_palette_expands_one_component_into_its_columns() {
    let h = header(|h| {
        h.palette = Some(Palette {
            channels: vec![(8, false); 3],
            columns: vec![vec![10, 20], vec![30, 40], vec![50, 60]],
        });
        h.component_map = vec![
            ChannelMap {
                component: 0,
                column: Some(0),
            },
            ChannelMap {
                component: 0,
                column: Some(1),
            },
            ChannelMap {
                component: 0,
                column: Some(2),
            },
        ];
    });
    let plan = colour::plan(Some(&h), &components(&[8])).expect("a plan");
    assert_eq!(plan.colour.len(), 3);
    for (i, channel) in plan.colour.iter().enumerate() {
        assert_eq!(channel.component, 0);
        assert_eq!(channel.column, Some(i));
        assert_eq!(
            channel.lookup(h.palette.as_ref(), 1),
            [20, 40, 60][i],
            "column {i} entry 1"
        );
        // Out of range clamps rather than refuses: a palette index is a
        // decoded sample, and a lossy stream can land one outside the table
        // through no fault of the file's.
        assert_eq!(channel.lookup(h.palette.as_ref(), 900), [20, 40, 60][i]);
        assert_eq!(channel.lookup(h.palette.as_ref(), -3), [10, 30, 50][i]);
    }
}

/// I.5.3.6: the opacity channel comes out of the colour set, carrying which
/// of the two types named it.
#[test]
fn cdef_lifts_the_opacity_channel_out_of_the_colours() {
    for (kind, premultiplied) in [(1u16, false), (2, true)] {
        let h = header(|h| {
            h.channel_definition = vec![
                ChannelDef {
                    channel: 0,
                    kind: 0,
                    association: 1,
                },
                ChannelDef {
                    channel: 1,
                    kind: 0,
                    association: 2,
                },
                ChannelDef {
                    channel: 2,
                    kind: 0,
                    association: 3,
                },
                ChannelDef {
                    channel: 3,
                    kind,
                    association: 0,
                },
            ];
        });
        let plan = colour::plan(Some(&h), &components(&[8; 4])).expect("a plan");
        assert_eq!(plan.colour.len(), 3, "type {kind}");
        assert_eq!(plan.opacity.map(|c| c.component), Some(3), "type {kind}");
        assert_eq!(plan.premultiplied, premultiplied, "type {kind}");
    }
}

/// `Asoc` is 1-based and says which colour a channel *is*, so a file may
/// declare them out of order — and this build reorders only when the whole
/// set is a permutation of 1..=n, because a partial set means the declared
/// order is the only order there is.
#[test]
fn cdef_associations_reorder_the_colours_when_they_are_a_permutation() {
    let h = header(|h| {
        h.channel_definition = vec![
            ChannelDef {
                channel: 0,
                kind: 0,
                association: 3,
            },
            ChannelDef {
                channel: 1,
                kind: 0,
                association: 1,
            },
            ChannelDef {
                channel: 2,
                kind: 0,
                association: 2,
            },
        ];
    });
    let plan = colour::plan(Some(&h), &components(&[8; 3])).expect("a plan");
    assert_eq!(
        plan.colour.iter().map(|c| c.component).collect::<Vec<_>>(),
        vec![1, 2, 0]
    );

    // A partial set is left alone: reordering against a association nobody
    // filled in would be inventing an order.
    let h = header(|h| {
        h.channel_definition = vec![ChannelDef {
            channel: 0,
            kind: 0,
            association: 3,
        }];
    });
    let plan = colour::plan(Some(&h), &components(&[8; 3])).expect("a plan");
    assert_eq!(
        plan.colour.iter().map(|c| c.component).collect::<Vec<_>>(),
        vec![0, 1, 2]
    );
}

/// Every combination of Annex I's boxes this build cannot interpret is
/// refused **by name** rather than approximated. Ruling 2, and the reason it
/// matters more here than anywhere else in the engine: a palette applied
/// wrongly is a picture.
#[test]
fn a_channel_arrangement_this_build_cannot_map_is_refused_by_name() {
    let palette = Palette {
        channels: vec![(8, false); 3],
        columns: vec![vec![1, 2], vec![3, 4], vec![5, 6]],
    };
    let cases: &[(&str, Jp2Header, &[u8])] = &[
        (
            "a pclr with no cmap to apply it",
            header(|h| h.palette = Some(palette.clone())),
            &[8],
        ),
        (
            "a cmap naming no such component",
            header(|h| {
                h.component_map = vec![ChannelMap {
                    component: 7,
                    column: None,
                }];
            }),
            &[8],
        ),
        (
            "a cmap PCOL past the palette's channels",
            header(|h| {
                h.palette = Some(palette.clone());
                h.component_map = vec![ChannelMap {
                    component: 0,
                    column: Some(9),
                }];
            }),
            &[8],
        ),
        (
            "a cdef naming no such channel",
            header(|h| {
                h.channel_definition = vec![ChannelDef {
                    channel: 9,
                    kind: 0,
                    association: 1,
                }];
            }),
            &[8],
        ),
        (
            "a cdef channel type this build cannot map",
            header(|h| {
                h.channel_definition = vec![ChannelDef {
                    channel: 0,
                    kind: 65535,
                    association: 0,
                }];
            }),
            &[8],
        ),
        (
            "more than one cdef opacity channel",
            header(|h| {
                h.channel_definition = vec![
                    ChannelDef {
                        channel: 0,
                        kind: 1,
                        association: 0,
                    },
                    ChannelDef {
                        channel: 1,
                        kind: 1,
                        association: 0,
                    },
                ];
            }),
            &[8, 8, 8],
        ),
        (
            "an image whose every channel is opacity",
            header(|h| {
                h.channel_definition = vec![ChannelDef {
                    channel: 0,
                    kind: 1,
                    association: 0,
                }];
            }),
            &[8],
        ),
        ("channels of differing depth", header(|_| {}), &[8, 8, 12]),
    ];
    for (what, h, precisions) in cases {
        let got = colour::plan(Some(h), &components(precisions));
        assert!(
            matches!(
                got,
                Err(Refusal::Structure(_) | Refusal::Feature(_) | Refusal::NotBuilt(_))
            ),
            "{what} was not refused: {got:?}"
        );
    }
}

/// A `cmap` MTYP outside I.5.3.5's 0 and 1 is refused where the box is read,
/// before anything downstream can act on it.
#[test]
fn a_cmap_mtyp_i535_does_not_define_is_refused() {
    let mut jp2h = boxed(*b"ihdr", &[0, 0, 0, 4, 0, 0, 0, 4, 0, 1, 7, 7, 0, 0]);
    jp2h.extend_from_slice(&boxed(*b"cmap", &[0, 0, 2, 0]));
    let mut file = SIGNATURE.to_vec();
    file.extend_from_slice(&boxed(*b"jp2h", &jp2h));
    file.extend_from_slice(&boxed(
        *b"jp2c",
        &Spec::default().codestream(&[(0, &[0, 0])]),
    ));
    assert!(matches!(
        boxes::parse(&file),
        Err(Refusal::Feature("a cmap MTYP I.5.3.5 does not define"))
    ));
}

/// End to end: a hand-built palette image decodes to the palette's colours,
/// not to its indices.
///
/// This is the test that says the palette is *applied*. The codestream is one
/// 8-bit component with an empty packet, so every coefficient is zero and
/// G.1's level shift puts every sample at 128 — an index whose three palette
/// columns are chosen to be distinguishable from each other and from 128, so
/// that a build ignoring the `pclr` (which would give 128, 128, 128) and one
/// reading the wrong column both fail.
///
/// `opj_compress` cannot write a `pclr` box, so there is no oracle to compare
/// against and this is not a second-best: a palette is a lookup table, its
/// answer is derived by arithmetic, and that is exactly the material source
/// (a) of the plan exists to supply.
#[test]
fn a_palette_image_decodes_to_colours_rather_than_indices() {
    // 256 entries, three channels: red counts up, green counts down, blue is
    // a quarter of the index. At index 128 that is (128, 127, 32).
    let mut pclr = vec![0x01, 0x00, 0x03, 0x07, 0x07, 0x07];
    for i in 0..256u32 {
        pclr.push(i as u8);
        pclr.push((255 - i) as u8);
        pclr.push((i / 4) as u8);
    }
    let mut jp2h = boxed(*b"ihdr", &[0, 0, 0, 4, 0, 0, 0, 4, 0, 1, 7, 7, 0, 0]);
    jp2h.extend_from_slice(&boxed(*b"colr", &[1, 0, 0, 0, 0, 0, 16]));
    jp2h.extend_from_slice(&boxed(*b"pclr", &pclr));
    // Three channels, all from component 0, through palette columns 0, 1, 2.
    jp2h.extend_from_slice(&boxed(*b"cmap", &[0, 0, 1, 0, 0, 0, 1, 1, 0, 0, 1, 2]));
    let mut file = SIGNATURE.to_vec();
    file.extend_from_slice(&boxed(*b"jp2h", &jp2h));
    file.extend_from_slice(&boxed(
        *b"jp2c",
        &Spec::default().codestream(&[(0, &[0, 0])]),
    ));

    let mut warnings = Vec::new();
    let image = jpx_decode(&file, &Limits::new(1 << 20), &mut warnings)
        .unwrap_or_else(|e| panic!("refused: {e:?}, warnings {warnings:?}"));
    assert_eq!((image.width, image.height), (4, 4));
    assert_eq!(image.components, 3, "the palette generates three channels");
    assert_eq!(image.colour, JpxColour::Srgb);
    assert_eq!(image.samples.len(), 4 * 4 * 3);
    for pixel in image.samples.chunks_exact(3) {
        assert_eq!(
            pixel,
            [128, 127, 32],
            "a palette image decoded to its indices rather than its colours"
        );
    }
}

/// `cdef` on a real decode: the opacity channel comes back beside the colour
/// ones rather than interleaved with them.
#[test]
fn a_cdef_opacity_channel_is_carried_out_of_the_samples() {
    let spec = Spec {
        components: vec![(8, false, 1, 1); 4],
        ..Spec::default()
    };
    let mut jp2h = boxed(*b"ihdr", &[0, 0, 0, 4, 0, 0, 0, 4, 0, 4, 7, 7, 0, 0]);
    jp2h.extend_from_slice(&boxed(*b"colr", &[1, 0, 0, 0, 0, 0, 16]));
    jp2h.extend_from_slice(&boxed(
        *b"cdef",
        &[
            0, 4, // four definitions
            0, 0, 0, 0, 0, 1, // channel 0, colour, association 1
            0, 1, 0, 0, 0, 2, // channel 1, colour, association 2
            0, 2, 0, 0, 0, 3, // channel 2, colour, association 3
            0, 3, 0, 2, 0, 0, // channel 3, premultiplied opacity
        ],
    ));
    let mut file = SIGNATURE.to_vec();
    file.extend_from_slice(&boxed(*b"jp2h", &jp2h));
    file.extend_from_slice(&boxed(*b"jp2c", &spec.codestream(&[(0, &[0; 8])])));

    let mut warnings = Vec::new();
    let image = jpx_decode(&file, &Limits::new(1 << 20), &mut warnings)
        .unwrap_or_else(|e| panic!("refused: {e:?}, warnings {warnings:?}"));
    assert_eq!(image.components, 3);
    assert_eq!(image.samples.len(), 4 * 4 * 3);
    let opacity = image.opacity.expect("cdef named an opacity channel");
    assert!(
        opacity.premultiplied,
        "cdef type 2 is premultiplied, and un-multiplying it is 8.9.5.4's job"
    );
    assert_eq!(opacity.samples.len(), 4 * 4);
}
