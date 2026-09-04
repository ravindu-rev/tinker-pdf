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

use tinker_pdf::{BlendMode, DocumentBuilder, ExtGState};

mod render_support;
use render_support::{
    axial_page, blend_cell_origin, blend_grid_page, byte, centre, image_page, ink, pixel,
    radial_page, render, AXIAL_AXIS, AXIAL_STOPS, BLEND_BACKDROPS, BLEND_CELL, BLEND_SOURCE,
    CHECKER, MODES, RADIAL_CENTRE, RADIAL_R0, RADIAL_R1, RADIAL_STOPS, SIZE,
};

/// Asserts a fixture drew enough to be evidence about anything.
///
/// The floor is roughly half of what the page paints today, which is
/// `determinism.rs`'s convention and is set so an ordinary rendering change
/// leaves it alone while a fixture that has stopped drawing trips it. These
/// four pages are enrolled as fingerprints, so the floor guards the hash as
/// well as the formula: a blank page hashes perfectly stably.
#[track_caller]
fn drew(bitmap: &tinker_pdf::Bitmap, least: usize, what: &str) {
    let drawn = ink(bitmap);
    assert!(
        drawn >= least,
        "the {what} fixture painted {drawn} pixels, fewer than the {least} it          is supposed to: it is measuring less than it claims"
    );
}

// ---- shadings ---------------------------------------------------------------

/// **Every pixel of an axial shading is the parametric equation** (8.7.4.5.3).
///
/// The axis is diagonal on purpose. An axis along `x` makes `t` a function of
/// one coordinate, so a build that projected onto the wrong axis, or that used
/// distance instead of projection, draws the same picture — every one of those
/// mistakes needs a slope to show.
#[test]
fn an_axial_shading_is_the_parametric_equation_at_every_pixel() {
    let (c0, c1) = AXIAL_STOPS;
    const AXIS: [f64; 4] = AXIAL_AXIS;

    let bitmap = render(axial_page());
    drew(&bitmap, 128, "axial shading");

    // 8.7.4.5.3: `t` is the projection of the point onto the axis, as a
    // fraction of the axis's own length, clamped by `/Extend`.
    let (dx, dy) = (AXIS[2] - AXIS[0], AXIS[3] - AXIS[1]);
    let length_squared = dx * dx + dy * dy;
    for y in 0..bitmap.height {
        for x in 0..bitmap.width {
            let (px, py) = centre(x, y);
            let t = (((px - AXIS[0]) * dx + (py - AXIS[1]) * dy) / length_squared).clamp(0.0, 1.0);
            let wanted = (
                byte(c0[0] + t * (c1[0] - c0[0])),
                byte(c0[1] + t * (c1[1] - c0[1])),
                byte(c0[2] + t * (c1[2] - c0[2])),
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
    let (c0, c1) = RADIAL_STOPS;
    const CENTRE: (f64, f64) = RADIAL_CENTRE;
    const R0: f64 = RADIAL_R0;
    const R1: f64 = RADIAL_R1;

    let bitmap = render(radial_page());
    drew(&bitmap, 100, "radial shading");

    for y in 0..bitmap.height {
        for x in 0..bitmap.width {
            let (px, py) = centre(x, y);
            let distance = ((px - CENTRE.0).powi(2) + (py - CENTRE.1).powi(2)).sqrt();
            let t = ((distance - R0) / (R1 - R0)).clamp(0.0, 1.0);
            let wanted = (
                byte(c0[0] + t * (c1[0] - c0[0])),
                byte(c0[1] + t * (c1[1] - c0[1])),
                byte(c0[2] + t * (c1[2] - c0[2])),
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

/// The same twelve expressions, laid out as one page rather than as a hundred
/// and eight documents.
///
/// The fixture above is the right shape for an expression and the wrong shape
/// for a fingerprint: a hash per document would be a hundred and eight of them,
/// and a hash of any one would cover a twelfth of one mode. This page carries
/// every mode over both backdrops, so `determinism.rs` can enrol the lot with
/// one hash — and this test is what keeps that hash a measurement rather than a
/// baseline, by holding every cell to the clause the same way.
#[test]
fn the_blend_grid_is_the_same_twelve_expressions_laid_out_as_a_picture() {
    let bitmap = render(blend_grid_page());
    drew(&bitmap, 800, "blend grid");

    let half = BLEND_CELL / 2.0;
    for (index, mode) in MODES.iter().enumerate() {
        let (x, y) = blend_cell_origin(index);
        for (which, backdrop) in BLEND_BACKDROPS.iter().enumerate() {
            // The centre of this half, converted to the row a bitmap counts
            // downward from the top.
            let px = x + which as f64 * half + half / 2.0;
            let py = y + BLEND_CELL / 2.0;
            let column = px as u32;
            let row = (f64::from(bitmap.height) - py) as u32;

            let painted = f64::from(byte(*backdrop)) / 255.0;
            let laid = f64::from(byte(BLEND_SOURCE)) / 255.0;
            let wanted = byte(blend(*mode, painted, laid));
            let (got, green, blue) = pixel(&bitmap, column, row);
            assert_eq!(
                (got, green, blue),
                (got, got, got),
                "{mode:?}: a grey backdrop and a grey source are grey"
            );
            let off = i32::from(got) - i32::from(wanted);
            assert!(
                off.abs() <= 1,
                "{mode:?} of {BLEND_SOURCE} over {backdrop} at ({column}, {row}):                  the page says {got} and the clause says {wanted}"
            );
        }
    }
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
    const SAMPLES: [[u8; 3]; 4] = CHECKER;

    let bitmap = render(image_page());
    drew(&bitmap, 64, "image");

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
