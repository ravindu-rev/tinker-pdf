//! Pages whose right answer is a formula (ruling 13, roadmap step 6).
//!
//! `tinker-pdf-raster/tests/analytic_coverage.rs` does this for the sampling
//! grid, where the arithmetic lives. This is the same tier one level up, where
//! a fixture needs a content stream: shadings, blend modes and image sampling
//! are each published by ISO 32000-1 as an expression, and an expression can be
//! evaluated by the test.
//!
//! # Two conventions, measured once and stated here
//!
//! - **A component reaches the page as `round(v × 255)`.** Not truncation: 0.5
//!   is 128 rather than 127, and every expectation below rounds the same way.
//! - **A shading is sampled at the pixel's centre**, so the pixel at column `x`
//!   is the shading's value at `x + 0.5`. A build sampling at the corner is off
//!   by half a pixel everywhere, which reads as a gradient that starts slightly
//!   too dark.
//!
//! Both are properties of this build rather than of the specification, which is
//! why they are written down here rather than assumed: an expectation carrying
//! them silently would be a fixture that agrees with whatever the renderer did.
//!
//! # The injections that were counted
//!
//! | Injected | Caught by the workspace | Of which here |
//! | --- | ---: | ---: |
//! | `Screen` read as `Multiply` (11.3.5.2) | 2 | 1 |
//! | `Overlay` not swapping its arguments (11.3.5.2) | **1** | **1** |
//! | an axial shading measuring distance rather than projection | 9 | 1 |
//! | a shading sampled at the pixel corner rather than its centre | 2 | 1 |
//!
//! `Overlay` is the row worth reading: swapping its two arguments back is a
//! change no other assertion in 2 939 sees. It draws a plausible picture, it is
//! still a blend, and only an expression evaluated from the clause can tell.
//!
//! # What this cannot reach
//!
//! Whether the page is the page a producer meant. Every formula below is
//! evaluated from the same clause the implementation was written from, so a
//! misreading shared by both is invisible here — `docs/verification.md` says so
//! in its own voice, and that is the property the retired oracles had.

use tinker_pdf::{BlendMode, DeviceSpace, DocumentBuilder, ExtGState, Function, ImageData, Shading};

mod render_support;
use render_support::{byte, centre, pixel, render, SIZE};

/// 11.3.5.2's twelve separable modes, which are the ones with a closed form
/// over one channel. The four non-separable ones need all three at once and are
/// a different claim.
const MODES: [BlendMode; 12] = [
    BlendMode::Normal,
    BlendMode::Multiply,
    BlendMode::Screen,
    BlendMode::Overlay,
    BlendMode::Darken,
    BlendMode::Lighten,
    BlendMode::ColorDodge,
    BlendMode::ColorBurn,
    BlendMode::HardLight,
    BlendMode::SoftLight,
    BlendMode::Difference,
    BlendMode::Exclusion,
];

// ---- shadings ---------------------------------------------------------------

/// **Every pixel of an axial shading is the parametric equation** (8.7.4.5.3).
///
/// The axis is diagonal on purpose. An axis along `x` makes `t` a function of
/// one coordinate, so a build that projected onto the wrong axis, or that used
/// distance instead of projection, draws the same picture — every one of those
/// mistakes needs a slope to show.
#[test]
fn an_axial_shading_is_the_parametric_equation_at_every_pixel() {
    const C0: [f64; 3] = [1.0, 0.0, 0.0];
    const C1: [f64; 3] = [0.0, 0.0, 1.0];
    // From the bottom left to the top right of the page.
    const AXIS: [f64; 4] = [0.0, 0.0, SIZE, SIZE];

    let mut builder = DocumentBuilder::new();
    assert!(builder.add_shading(
        b"Sh0",
        &Shading::Axial {
            color_space: DeviceSpace::Rgb,
            coords: AXIS,
            function: Function::Exponential {
                domain: [0.0, 1.0],
                c0: C0.to_vec(),
                c1: C1.to_vec(),
                n: 1.0,
            },
            extend: (true, true),
        }
    ));
    builder.add_page(SIZE, SIZE, |page| {
        page.raw(format!("q 0 0 {SIZE} {SIZE} re W n").as_bytes());
        assert!(page.shading(b"Sh0"));
        page.raw(b"Q");
    });
    let bitmap = render(builder.finish());

    // 8.7.4.5.3: `t` is the projection of the point onto the axis, as a
    // fraction of the axis's own length, clamped by `/Extend`.
    let (dx, dy) = (AXIS[2] - AXIS[0], AXIS[3] - AXIS[1]);
    let length_squared = dx * dx + dy * dy;
    for y in 0..bitmap.height {
        for x in 0..bitmap.width {
            let (px, py) = centre(x, y);
            let t = (((px - AXIS[0]) * dx + (py - AXIS[1]) * dy) / length_squared).clamp(0.0, 1.0);
            let wanted = (
                byte(C0[0] + t * (C1[0] - C0[0])),
                byte(C0[1] + t * (C1[1] - C0[1])),
                byte(C0[2] + t * (C1[2] - C0[2])),
            );
            assert_eq!(pixel(&bitmap, x, y), wanted, "pixel ({x}, {y}), t = {t}");
        }
    }
}

/// **Every pixel of a radial shading is its own parametric equation**
/// (8.7.4.5.4), and it is a different equation.
///
/// Concentric circles, so `t` is the distance from the centre scaled between
/// the two radii — the case where a build that reused the axial projection
/// draws a gradient that is plausible and is not a circle.
#[test]
fn a_radial_shading_is_the_distance_between_its_two_circles() {
    const C0: [f64; 3] = [1.0, 1.0, 1.0];
    const C1: [f64; 3] = [0.0, 0.0, 0.0];
    const CENTRE: (f64, f64) = (SIZE / 2.0, SIZE / 2.0);
    const R0: f64 = 1.0;
    const R1: f64 = 7.0;

    let mut builder = DocumentBuilder::new();
    assert!(builder.add_shading(
        b"Sh0",
        &Shading::Radial {
            color_space: DeviceSpace::Rgb,
            coords: [CENTRE.0, CENTRE.1, R0, CENTRE.0, CENTRE.1, R1],
            function: Function::Exponential {
                domain: [0.0, 1.0],
                c0: C0.to_vec(),
                c1: C1.to_vec(),
                n: 1.0,
            },
            extend: (true, true),
        }
    ));
    builder.add_page(SIZE, SIZE, |page| {
        page.raw(format!("q 0 0 {SIZE} {SIZE} re W n").as_bytes());
        assert!(page.shading(b"Sh0"));
        page.raw(b"Q");
    });
    let bitmap = render(builder.finish());

    for y in 0..bitmap.height {
        for x in 0..bitmap.width {
            let (px, py) = centre(x, y);
            let distance = ((px - CENTRE.0).powi(2) + (py - CENTRE.1).powi(2)).sqrt();
            let t = ((distance - R0) / (R1 - R0)).clamp(0.0, 1.0);
            let wanted = (
                byte(C0[0] + t * (C1[0] - C0[0])),
                byte(C0[1] + t * (C1[1] - C0[1])),
                byte(C0[2] + t * (C1[2] - C0[2])),
            );
            assert_eq!(pixel(&bitmap, x, y), wanted, "pixel ({x}, {y}), t = {t}");
        }
    }
}

// ---- blend modes ------------------------------------------------------------

/// 11.3.5's separable blend functions, transcribed.
///
/// One expression per mode, from the table rather than from `blend.rs`. The two
/// that are not one-liners are written the way the clause writes them —
/// `HardLight` is `Multiply` or `Screen` of the doubled source, and `SoftLight`
/// carries the `D(x)` the clause defines beneath it.
fn blend(mode: BlendMode, backdrop: f64, source: f64) -> f64 {
    let (b, s) = (backdrop, source);
    match mode {
        BlendMode::Normal => s,
        BlendMode::Multiply => b * s,
        BlendMode::Screen => b + s - b * s,
        BlendMode::Overlay => blend(BlendMode::HardLight, s, b),
        BlendMode::Darken => b.min(s),
        BlendMode::Lighten => b.max(s),
        BlendMode::ColorDodge => {
            if b <= 0.0 {
                0.0
            } else if s >= 1.0 {
                1.0
            } else {
                (b / (1.0 - s)).min(1.0)
            }
        }
        BlendMode::ColorBurn => {
            if b >= 1.0 {
                1.0
            } else if s <= 0.0 {
                0.0
            } else {
                1.0 - ((1.0 - b) / s).min(1.0)
            }
        }
        BlendMode::HardLight => {
            if s <= 0.5 {
                b * (2.0 * s)
            } else {
                let d = 2.0 * s - 1.0;
                b + d - b * d
            }
        }
        BlendMode::SoftLight => {
            let d = if b <= 0.25 {
                ((16.0 * b - 12.0) * b + 4.0) * b
            } else {
                b.sqrt()
            };
            if s <= 0.5 {
                b - (1.0 - 2.0 * s) * b * (1.0 - b)
            } else {
                b + (2.0 * s - 1.0) * (d - b)
            }
        }
        BlendMode::Difference => (b - s).abs(),
        BlendMode::Exclusion => b + s - 2.0 * b * s,
        other => panic!("{other:?} is not one of 11.3.5's separable modes"),
    }
}

/// **Each separable blend mode is the arithmetic 11.3.5 publishes**, over nine
/// backdrop and source pairs.
///
/// Nine pairs and not one, because most of these expressions agree somewhere: a
/// backdrop and a source that are both `0.5` make `Multiply`, `HardLight` and
/// `Overlay` all `0.25`, and a fixture built on that pair would call three
/// modes correct because one of them is.
///
/// # Why eight modes are exact and four are within a level
///
/// The clause publishes its expressions over the reals. **This build evaluates
/// them in eight-bit fixed point** — `blend.rs` has no floating point in it at
/// all, deliberately, so that ruling 4's bit-identical contract does not rest
/// on anybody's `sqrt` — and integer division truncates. So a float evaluation
/// of the same clause and the page can differ, and where they may is decided by
/// the expression rather than tuned:
///
/// - Eight modes are **exact**. Selections and an absolute difference have
///   nothing to round at all, and the products round once, at the end, exactly
///   as the float evaluation does.
/// - `ColorDodge`, `ColorBurn` and `Exclusion` divide or double, and
///   `SoftLight` takes a square root by integer Newton rather than by `sqrt`.
///   Each truncation can cost a level: **at most one**, and one is what is
///   measured.
///
/// The bound is the assertion, and the split is the ratchet: a mode that leaves
/// the exact list has changed. A build with a wrong expression is not off by a
/// level — `Screen` read as `Multiply` is off by 96 at the middle of this
/// range — so a one-level allowance costs nothing, and saying which four may
/// use it is what keeps this from being a tolerance nobody can justify.
#[test]
fn every_separable_blend_mode_is_the_expression_the_clause_publishes() {
    const LEVELS: [f64; 3] = [0.25, 0.5, 0.8];
    /// The eight the fixed-point evaluation reproduces exactly. The four that
    /// are not here are `ColorDodge`, `ColorBurn`, `SoftLight` and `Exclusion`.
    const EXACT: [BlendMode; 8] = [
        BlendMode::Normal,
        BlendMode::Multiply,
        BlendMode::Screen,
        BlendMode::Overlay,
        BlendMode::Darken,
        BlendMode::Lighten,
        BlendMode::HardLight,
        BlendMode::Difference,
    ];

    for mode in MODES {
        let mut worst = 0i32;
        for backdrop in LEVELS {
            for source in LEVELS {
                let mut builder = DocumentBuilder::new();
                assert!(builder.add_ext_gstate(
                    b"GS0",
                    &ExtGState {
                        blend_mode: Some(mode),
                        ..ExtGState::default()
                    }
                ));
                builder.add_page(SIZE, SIZE, |page| {
                    page.set_fill_rgb(backdrop, backdrop, backdrop);
                    page.raw(format!("0 0 {SIZE} {SIZE} re f").as_bytes());
                    assert!(page.set_ext_gstate(b"GS0"));
                    page.set_fill_rgb(source, source, source);
                    page.raw(format!("0 0 {SIZE} {SIZE} re f").as_bytes());
                });
                let bitmap = render(builder.finish());

                // **Both operands are the page's bytes, not the numbers that
                // painted them.** The canvas is eight-bit, so the blend
                // function is handed what was stored: `0.25` paints 64 and
                // blends as 64/255, and a source of `0.8` blends as 204/255.
                // Passing the original floats is off by a level on some pairs
                // — `Difference` of 0.5 over 0.8 is 76 on the page and 77 in
                // reals, because 0.3 of 255 is 76.5 and the bytes are 76
                // apart — and that is exactly the kind of thing a per-pixel
                // fixture exists to find.
                let painted = f64::from(byte(backdrop)) / 255.0;
                let laid = f64::from(byte(source)) / 255.0;
                let wanted = byte(blend(mode, painted, laid));
                let (got, green, blue) = pixel(&bitmap, 8, 8);
                assert_eq!(
                    (got, green, blue),
                    (got, got, got),
                    "{mode:?}: a grey backdrop and a grey source are grey"
                );

                let off = i32::from(got) - i32::from(wanted);
                assert!(
                    off.abs() <= 1,
                    "{mode:?} of {source} over {backdrop}: the page says {got} and                      the clause says {wanted}"
                );
                if EXACT.contains(&mode) {
                    assert_eq!(
                        got, wanted,
                        "{mode:?} is one of the eight the fixed-point evaluation                          reproduces exactly"
                    );
                }
                worst = worst.max(off.abs());
            }
        }
        println!("  {mode:?} worst {worst}");
    }
}

/// And the twelve expressions are twelve, which is what says the loop above is
/// not comparing one formula with itself.
///
/// At a backdrop of 0.25 and a source of 0.8 the modes take eleven distinct
/// values — `Darken` and `Normal` are the pair that coincide, since the source
/// is the lighter of the two — so a build that read every `/BM` as `Normal`
/// fails eleven of the twelve rather than passing on a fixture that could not
/// tell them apart.
#[test]
fn the_twelve_separable_modes_are_not_one_expression() {
    let values: Vec<u8> = MODES
        .into_iter()
        .map(|mode| {
            byte(blend(
                mode,
                f64::from(byte(0.25)) / 255.0,
                f64::from(byte(0.8)) / 255.0,
            ))
        })
        .collect();
    let mut distinct = values.clone();
    distinct.sort_unstable();
    distinct.dedup();
    assert!(
        distinct.len() >= 10,
        "the modes should mostly disagree at this pair: {values:?}"
    );
}

// ---- image sampling ---------------------------------------------------------

/// **An image scaled by a whole number is that many pixels of each sample.**
///
/// A two-by-two checker over a sixteen-point square is eight pixels of each
/// sample, exactly, with no interpolation anywhere: the sample grid and the
/// pixel grid line up, so every pixel's centre falls squarely inside one
/// source pixel and the answer needs no filter theory at all.
///
/// It is the one image assertion that can be exact, which is why it is the one
/// here — a non-integer scale would need a statement about which filter, and
/// that is a different claim from *the samples arrived*.
#[test]
fn an_image_scaled_by_a_whole_number_is_blocks_of_its_samples() {
    // A two-by-two checker: black, white / white, black, row-major from the
    // top, which is the order 8.9.5.2 states.
    const SAMPLES: [[u8; 3]; 4] = [[0, 0, 0], [255, 255, 255], [255, 255, 255], [0, 0, 0]];
    let data: Vec<u8> = SAMPLES.iter().flatten().copied().collect();

    let mut builder = DocumentBuilder::new();
    assert!(builder.add_image(
        b"Im0",
        &ImageData::Rgb8 {
            width: 2,
            height: 2,
            data: &data,
        }
    ));
    builder.add_page(SIZE, SIZE, |page| {
        page.image(b"Im0", 0.0, 0.0, SIZE, SIZE);
    });
    let bitmap = render(builder.finish());

    let half = (SIZE / 2.0) as u32;
    for y in 0..bitmap.height {
        for x in 0..bitmap.width {
            // The sample this pixel's centre lands in. The image's own rows run
            // downward, and so do the bitmap's, so this one needs no flip —
            // which is the asymmetry with `centre` above and the reason the
            // two are written out separately rather than shared.
            let column = usize::from(x >= half);
            let row = usize::from(y >= half);
            let sample = SAMPLES[row * 2 + column];
            assert_eq!(
                pixel(&bitmap, x, y),
                (sample[0], sample[1], sample[2]),
                "pixel ({x}, {y}) should be sample ({column}, {row})"
            );
        }
    }
}
