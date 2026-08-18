//! The writer's missing half, drawn by this repository's own renderer.
//!
//! `crates/tinker-pdf-cos/src/build.rs`'s own tests assert what is *in* the
//! file: that `/ca` and `/CA` are two entries, that a stitching function keeps
//! its bounds, that `/W` is the font's own `hmtx`. None of them can say the
//! file draws the picture it describes, because that crate has no renderer.
//!
//! This one can, and the comparison it makes is the one gaps 09, 10 and 11
//! bought: **every construct this writer now emits already had a reader here**,
//! exercised by hand-written fixtures. So the shape of most tests below is
//! *write it, open it, draw it, and compare against the same picture built by
//! hand* — two documents that share no code above the parser, rendered to the
//! same bytes.
//!
//! # Why a whole-bitmap comparison rather than a sampled pixel
//!
//! A sampled pixel says the middle of a gradient is the colour it should be. A
//! whole bitmap says the gradient's *ends*, its extension past the axis, its
//! interpolation and the area it did not paint are all the same too. Where the
//! two documents are meant to be identical, nothing is gained by looking at
//! less of them — and gap 07's headline defect, a gradient-stroked rule
//! painting solid black, is exactly the kind a sampled midpoint can miss.
//!
//! Where the two are meant to *differ* — isolation against knockout, a bound
//! moved along an axis — the assertion is a number at a named point instead,
//! because "these differ" is satisfied by a build that draws neither.

use std::collections::BTreeMap;

use tinker_pdf::{
    Bitmap, BlendMode, DeviceSpace, Document, DocumentBuilder, ExtGState, FormXObject, Function,
    Glyph, MaskKind, RenderOptions, Shading, StateMask, TilingPattern, TilingType,
    TransparencyGroup,
};

// ---- the two ways a picture gets made -----------------------------------

fn render(bytes: Vec<u8>) -> Bitmap {
    Document::open(bytes)
        .expect("it opens")
        .page(0)
        .expect("a page")
        .render(&RenderOptions::default())
}

/// A 60 x 60 page written by hand, in the shape `tests/transparency_groups.rs`
/// established: object 5 upward is whatever the fixture needs.
fn hand(resources: &str, content: &str, extra: &str) -> Vec<u8> {
    format!(
        "%PDF-1.7\n\
1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
2 0 obj\n<< /Type /Pages /Count 1 /Kids [3 0 R] >>\nendobj\n\
3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 60 60]\n\
   /Resources << {resources} >>\n\
   /Contents 4 0 R >>\nendobj\n\
4 0 obj\n<< /Length {} >>\nstream\n{content}\nendstream\nendobj\n\
{extra}\
trailer\n<< /Size 20 /Root 1 0 R >>\n%%EOF\n",
        content.len()
    )
    .into_bytes()
}

/// A pixel, by its position in *user* space on a 60 x 60 page.
fn at(bitmap: &Bitmap, ux: f64, uy: f64) -> (u8, u8, u8) {
    let x = ((ux / 60.0) * f64::from(bitmap.width)) as u32;
    let y = (((60.0 - uy) / 60.0) * f64::from(bitmap.height)) as u32;
    let x = x.min(bitmap.width - 1);
    let y = y.min(bitmap.height - 1);
    let base = (y as usize) * bitmap.stride + (x as usize) * bitmap.components();
    let p = bitmap.data.get(base..base + 3).unwrap_or(&[255, 255, 255]);
    (p[0], p[1], p[2])
}

/// Asserts two documents draw the same picture, pixel for pixel.
fn same_picture(built: Bitmap, written: Bitmap, what: &str) {
    assert_eq!(
        (built.width, built.height),
        (written.width, written.height),
        "{what}: different sizes"
    );
    if built.data == written.data {
        return;
    }
    let differing = built
        .data
        .iter()
        .zip(written.data.iter())
        .filter(|(a, b)| a != b)
        .count();
    let first = built
        .data
        .iter()
        .zip(written.data.iter())
        .position(|(a, b)| a != b)
        .unwrap_or(0);
    panic!(
        "{what}: the built document and the hand-written one drew different \
         pictures -- {differing} of {} bytes differ, first at {first} \
         ({} against {})",
        built.data.len(),
        built.data.get(first).copied().unwrap_or(0),
        written.data.get(first).copied().unwrap_or(0),
    );
}

// ---- /ExtGState ----------------------------------------------------------

/// An alpha set through the builder draws what the same alpha written by hand
/// draws.
#[test]
fn an_alpha_from_the_builder_draws_what_a_hand_written_one_draws() {
    let mut builder = DocumentBuilder::new();
    assert!(builder.add_ext_gstate(
        b"GS0",
        &ExtGState {
            fill_alpha: Some(0.25),
            ..ExtGState::default()
        }
    ));
    builder.add_page(60.0, 60.0, |page| {
        page.set_fill_rgb(0.0, 0.0, 0.0);
        page.raw(b"q");
        assert!(page.set_ext_gstate(b"GS0"));
        page.raw(b"10 10 40 40 re f");
        page.raw(b"Q");
    });

    let written = hand(
        "/ExtGState << /GS0 << /Type /ExtGState /ca 0.25 >> >>",
        "0 0 0 rg q /GS0 gs 10 10 40 40 re f Q",
        "",
    );
    same_picture(render(builder.finish()), render(written), "a fill alpha");
}

/// `/ca` and `/CA` reach **different pixels**, and swapping them swaps the
/// picture.
///
/// The dictionary test in `build.rs` says the two keys are written from two
/// fields. This says the two keys mean two things once a renderer has them —
/// which is the half that a writer with one alpha spelled into both keys would
/// pass on every document that never strokes and fills at different
/// opacities, and fail on the first one that does.
#[test]
fn fill_alpha_and_stroke_alpha_reach_different_pixels() {
    let draw = |fill: f64, stroke: f64| {
        let mut builder = DocumentBuilder::new();
        assert!(builder.add_ext_gstate(
            b"GS0",
            &ExtGState {
                fill_alpha: Some(fill),
                stroke_alpha: Some(stroke),
                ..ExtGState::default()
            }
        ));
        builder.add_page(60.0, 60.0, |page| {
            page.set_fill_rgb(0.0, 0.0, 0.0);
            page.set_stroke_rgb(0.0, 0.0, 0.0);
            assert!(page.set_ext_gstate(b"GS0"));
            // A filled square in the bottom half and a thick stroked rule
            // across the top, so one pixel of each is reachable.
            page.raw(b"5 5 25 25 re f");
            page.raw(b"10 w 5 45 m 55 45 l S");
        });
        render(builder.finish())
    };

    let faded_fill = draw(0.25, 1.0);
    let faded_stroke = draw(1.0, 0.25);

    let (fill_a, stroke_a) = (at(&faded_fill, 15.0, 15.0), at(&faded_fill, 30.0, 45.0));
    let (fill_b, stroke_b) = (at(&faded_stroke, 15.0, 15.0), at(&faded_stroke, 30.0, 45.0));

    assert!(
        fill_a.0 > 180 && fill_a.0 < 210,
        "a quarter-opaque black fill is three quarters white: {fill_a:?}"
    );
    assert!(
        stroke_a.0 < 20,
        "and the stroke beside it is fully opaque: {stroke_a:?}"
    );
    assert!(
        fill_b.0 < 20,
        "with the numbers exchanged the fill is opaque: {fill_b:?}"
    );
    assert!(
        stroke_b.0 > 180 && stroke_b.0 < 210,
        "and the stroke is the faded one: {stroke_b:?}"
    );
}

/// A blend mode written by the builder blends.
#[test]
fn a_blend_mode_from_the_builder_draws_what_a_hand_written_one_draws() {
    let mut builder = DocumentBuilder::new();
    assert!(builder.add_ext_gstate(
        b"GS0",
        &ExtGState {
            blend_mode: Some(BlendMode::Multiply),
            ..ExtGState::default()
        }
    ));
    builder.add_page(60.0, 60.0, |page| {
        page.set_fill_rgb(0.4, 0.8, 0.4);
        page.raw(b"0 0 60 60 re f");
        page.raw(b"q");
        assert!(page.set_ext_gstate(b"GS0"));
        page.set_fill_rgb(0.8, 0.4, 0.8);
        page.raw(b"10 10 40 40 re f");
        page.raw(b"Q");
    });

    let written = hand(
        "/ExtGState << /GS0 << /Type /ExtGState /BM /Multiply >> >>",
        "0.4 0.8 0.4 rg 0 0 60 60 re f \
         q /GS0 gs 0.8 0.4 0.8 rg 10 10 40 40 re f Q",
        "",
    );
    same_picture(render(builder.finish()), render(written), "a blend mode");
}

/// A luminosity soft mask from the builder fades what a hand-written one
/// fades.
#[test]
fn a_luminosity_soft_mask_from_the_builder_draws_what_a_hand_written_one_draws() {
    let mut builder = DocumentBuilder::new();
    // The mask paints white over the left half only; outside its own box the
    // luminosity is the backdrop's, which 11.6.5.2 defaults to black.
    assert!(builder.add_form(
        b"Mask",
        &FormXObject {
            bbox: [0.0, 0.0, 30.0, 60.0],
            matrix: None,
            group: Some(TransparencyGroup {
                color_space: DeviceSpace::Gray,
                isolated: true,
                knockout: false,
            }),
            content: b"1 g 0 0 30 60 re f",
        }
    ));
    assert!(builder.add_ext_gstate(
        b"GS0",
        &ExtGState {
            soft_mask: Some(StateMask::Group {
                kind: MaskKind::Luminosity,
                form: b"Mask",
                backdrop: None,
            }),
            ..ExtGState::default()
        }
    ));
    builder.add_page(60.0, 60.0, |page| {
        page.set_fill_rgb(0.0, 0.0, 0.0);
        page.raw(b"q");
        assert!(page.set_ext_gstate(b"GS0"));
        page.raw(b"0 0 60 60 re f");
        page.raw(b"Q");
    });

    let written = hand(
        "/ExtGState << /GS0 << /Type /ExtGState /SMask \
           << /Type /Mask /S /Luminosity /G 5 0 R >> >> >>",
        "0 0 0 rg q /GS0 gs 0 0 60 60 re f Q",
        "5 0 obj\n<< /Type /XObject /Subtype /Form /BBox [0 0 30 60]\n\
           /Group << /S /Transparency /CS /DeviceGray /I true >>\n\
           /Resources << >> /Length 18 >>\nstream\n1 g 0 0 30 60 re f\nendstream\nendobj\n",
    );

    let built = render(builder.finish());
    // Before comparing, the picture has to be one: a mask that did nothing
    // would make both halves black and pass a comparison against a
    // hand-written twin that did nothing either.
    let (left, right) = (at(&built, 15.0, 30.0), at(&built, 45.0, 30.0));
    assert!(left.0 < 20, "the masked-in half is painted: {left:?}");
    assert!(
        right.0 > 235,
        "and the half the mask left black is not: {right:?}"
    );

    same_picture(built, render(written), "a luminosity soft mask");
}

/// `/SMask /None` turns a mask off, which is what an absent entry cannot do.
#[test]
fn a_soft_mask_of_none_turns_an_inherited_mask_off() {
    let build = |stop: bool| {
        let mut builder = DocumentBuilder::new();
        assert!(builder.add_form(
            b"Mask",
            &FormXObject {
                bbox: [0.0, 0.0, 30.0, 60.0],
                matrix: None,
                group: Some(TransparencyGroup {
                    color_space: DeviceSpace::Gray,
                    isolated: true,
                    knockout: false,
                }),
                content: b"1 g 0 0 30 60 re f",
            }
        ));
        assert!(builder.add_ext_gstate(
            b"GOn",
            &ExtGState {
                soft_mask: Some(StateMask::Group {
                    kind: MaskKind::Luminosity,
                    form: b"Mask",
                    backdrop: None,
                }),
                ..ExtGState::default()
            }
        ));
        assert!(builder.add_ext_gstate(
            b"GOff",
            &ExtGState {
                soft_mask: Some(StateMask::None),
                ..ExtGState::default()
            }
        ));
        assert!(builder.add_ext_gstate(
            b"GQuiet",
            &ExtGState {
                fill_alpha: Some(1.0),
                ..ExtGState::default()
            }
        ));
        builder.add_page(60.0, 60.0, |page| {
            page.set_fill_rgb(0.0, 0.0, 0.0);
            assert!(page.set_ext_gstate(b"GOn"));
            assert!(page.set_ext_gstate(if stop { b"GOff" } else { b"GQuiet" }));
            page.raw(b"0 0 60 60 re f");
        });
        render(builder.finish())
    };

    let stopped = at(&build(true), 45.0, 30.0);
    let kept = at(&build(false), 45.0, 30.0);
    assert!(
        stopped.0 < 20,
        "`/SMask /None` let the right half be painted: {stopped:?}"
    );
    assert!(
        kept.0 > 235,
        "and a state that said nothing about the mask left it in force: {kept:?}"
    );
}

// ---- transparency groups -------------------------------------------------

/// Isolation is a real flag: a non-isolated group's contents see the page
/// underneath and an isolated one's do not.
///
/// 11.4.4 makes the distinction observable only when something inside the
/// group blends with a mode other than Normal, so that is what the fixture
/// does — the same shape `tests/transparency_groups.rs` uses, arriving through
/// the builder instead of through a string.
#[test]
fn an_isolated_group_from_the_builder_ignores_the_page_and_a_joined_one_does_not() {
    let build = |isolated: bool| {
        let mut builder = DocumentBuilder::new();
        assert!(builder.add_ext_gstate(
            b"Mul",
            &ExtGState {
                blend_mode: Some(BlendMode::Multiply),
                ..ExtGState::default()
            }
        ));
        assert!(builder.add_form(
            b"Fm0",
            &FormXObject {
                bbox: [0.0, 0.0, 60.0, 60.0],
                matrix: None,
                group: Some(TransparencyGroup {
                    color_space: DeviceSpace::Rgb,
                    isolated,
                    knockout: false,
                }),
                content: b"/Mul gs 0.8 0.4 0.8 rg 10 10 40 40 re f",
            }
        ));
        builder.add_page(60.0, 60.0, |page| {
            page.set_fill_rgb(0.4, 0.8, 0.4);
            page.raw(b"0 0 60 60 re f");
            assert!(page.form(b"Fm0"));
        });
        render(builder.finish())
    };

    let isolated = build(true);
    let joined = build(false);

    let iso = at(&isolated, 30.0, 30.0);
    assert!(
        iso.0 > 195 && iso.1 < 115 && iso.2 > 195,
        "an isolated group multiplies against nothing, so the source survives: {iso:?}"
    );
    let non = at(&joined, 30.0, 30.0);
    assert!(
        non.0 < 100 && non.1 < 100 && non.2 < 100,
        "a non-isolated one multiplies against the page underneath: {non:?}"
    );
    assert_eq!(
        at(&isolated, 55.0, 55.0),
        at(&joined, 55.0, 55.0),
        "outside the square both are the untouched backdrop"
    );
}

/// Knockout is a **different** flag, and it changes what the elements inside
/// one group do to each other.
///
/// 11.6.6: in a knockout group each element composites against the group's
/// *initial* backdrop rather than against the result so far, so the second of
/// two overlapping half-opaque shapes replaces the first instead of stacking
/// on it. Isolation cannot produce that difference and knockout cannot produce
/// the one above, which is why both are here rather than one standing for the
/// pair.
#[test]
fn a_knockout_group_replaces_what_it_overlaps_and_a_normal_one_stacks_on_it() {
    let build = |knockout: bool| {
        let mut builder = DocumentBuilder::new();
        assert!(builder.add_ext_gstate(
            b"Half",
            &ExtGState {
                fill_alpha: Some(0.5),
                ..ExtGState::default()
            }
        ));
        assert!(builder.add_form(
            b"Fm0",
            &FormXObject {
                bbox: [0.0, 0.0, 60.0, 60.0],
                matrix: None,
                group: Some(TransparencyGroup {
                    color_space: DeviceSpace::Rgb,
                    isolated: true,
                    knockout,
                }),
                content: b"/Half gs 1 0 0 rg 5 5 35 35 re f 0 0 1 rg 20 20 35 35 re f",
            }
        ));
        builder.add_page(60.0, 60.0, |page| {
            assert!(page.form(b"Fm0"));
        });
        render(builder.finish())
    };

    let knocked = at(&build(true), 30.0, 30.0);
    let stacked = at(&build(false), 30.0, 30.0);
    assert_ne!(
        knocked, stacked,
        "knockout changed nothing where two half-opaque shapes overlap"
    );
    // Half-opaque blue over the group's own white backdrop is (127, 127, 255).
    // Over half-opaque red on white it is (127, 63, 191): the red survives
    // underneath and pulls the green down and the blue with it.
    assert!(
        knocked.2 > 240 && knocked.0 == knocked.1,
        "the knocked-out red is gone from the overlap: {knocked:?}"
    );
    assert!(
        stacked.2 < 220 && stacked.1 < stacked.0,
        "and without it the red is still under the blue: {stacked:?} against {knocked:?}"
    );
}

// ---- shadings ------------------------------------------------------------

/// A type 2 shading from the builder draws what a hand-written one draws.
#[test]
fn an_axial_shading_from_the_builder_draws_what_a_hand_written_one_draws() {
    let mut builder = DocumentBuilder::new();
    assert!(builder.add_shading(
        b"Sh0",
        &Shading::Axial {
            color_space: DeviceSpace::Rgb,
            coords: [10.0, 0.0, 50.0, 0.0],
            function: Function::Exponential {
                domain: [0.0, 1.0],
                c0: vec![1.0, 0.0, 0.0],
                c1: vec![0.0, 0.0, 1.0],
                n: 1.0,
            },
            extend: (true, true),
        }
    ));
    builder.add_page(60.0, 60.0, |page| {
        page.raw(b"q 0 0 60 60 re W n");
        assert!(page.shading(b"Sh0"));
        page.raw(b"Q");
    });

    let written = hand(
        "/Shading << /Sh0 << /ShadingType 2 /ColorSpace /DeviceRGB \
           /Coords [10 0 50 0] /Extend [true true] \
           /Function << /FunctionType 2 /Domain [0 1] /C0 [1 0 0] /C1 [0 0 1] /N 1 >> \
         >> >>",
        "q 0 0 60 60 re W n /Sh0 sh Q",
        "",
    );

    let built = render(builder.finish());
    let middle = at(&built, 30.0, 30.0);
    assert!(
        middle.0 > 90 && middle.0 < 170 && middle.2 > 90 && middle.2 < 170,
        "the middle of a red-to-blue ramp is halfway: {middle:?}"
    );
    same_picture(built, render(written), "an axial shading");
}

/// A type 3 shading from the builder draws what a hand-written one draws.
///
/// Written separately from the axial one on purpose: a writer that dropped the
/// two radii and reused the axial path would produce a picture, and it would
/// be a picture of the wrong kind of gradient.
#[test]
fn a_radial_shading_from_the_builder_draws_what_a_hand_written_one_draws() {
    let mut builder = DocumentBuilder::new();
    assert!(builder.add_shading(
        b"Sh0",
        &Shading::Radial {
            color_space: DeviceSpace::Rgb,
            coords: [30.0, 30.0, 0.0, 30.0, 30.0, 25.0],
            function: Function::Exponential {
                domain: [0.0, 1.0],
                c0: vec![1.0, 1.0, 0.0],
                c1: vec![0.0, 0.0, 1.0],
                n: 1.0,
            },
            extend: (false, true),
        }
    ));
    builder.add_page(60.0, 60.0, |page| {
        page.raw(b"q 0 0 60 60 re W n");
        assert!(page.shading(b"Sh0"));
        page.raw(b"Q");
    });

    let written = hand(
        "/Shading << /Sh0 << /ShadingType 3 /ColorSpace /DeviceRGB \
           /Coords [30 30 0 30 30 25] /Extend [false true] \
           /Function << /FunctionType 2 /Domain [0 1] /C0 [1 1 0] /C1 [0 0 1] /N 1 >> \
         >> >>",
        "q 0 0 60 60 re W n /Sh0 sh Q",
        "",
    );

    let built = render(builder.finish());
    let centre = at(&built, 30.0, 30.0);
    let edge = at(&built, 30.0, 57.0);
    assert!(
        centre.0 > 200 && centre.1 > 200 && centre.2 < 60,
        "the centre circle is the first colour: {centre:?}"
    );
    assert!(
        edge.2 > 200 && edge.0 < 60,
        "and past the second circle the extension is the last: {edge:?}"
    );
    same_picture(built, render(written), "a radial shading");
}

/// A type 3 function's **bounds** decide where each sub-function starts, and
/// moving one moves the colour.
///
/// The stitch and the sub-functions are two independent things and this is the
/// stitch's half: both documents carry the same two sub-functions and differ
/// only in `/Bounds`, so a build that ignored `/Bounds` — or read it as
/// `/Encode` — draws the same picture twice and fails here while passing every
/// test that only looks at the ends of the ramp.
#[test]
fn a_stitching_functions_bounds_decide_where_its_middle_colour_lands() {
    let draw = |bound: f64| {
        let mut builder = DocumentBuilder::new();
        assert!(builder.add_shading(
            b"Sh0",
            &Shading::Axial {
                color_space: DeviceSpace::Rgb,
                coords: [0.0, 0.0, 60.0, 0.0],
                function: Function::Stitching {
                    domain: [0.0, 1.0],
                    functions: vec![
                        Function::Exponential {
                            domain: [0.0, 1.0],
                            c0: vec![1.0, 0.0, 0.0],
                            c1: vec![0.0, 1.0, 0.0],
                            n: 1.0,
                        },
                        Function::Exponential {
                            domain: [0.0, 1.0],
                            c0: vec![0.0, 1.0, 0.0],
                            c1: vec![0.0, 0.0, 1.0],
                            n: 1.0,
                        },
                    ],
                    bounds: vec![bound],
                    encode: vec![[0.0, 1.0], [0.0, 1.0]],
                },
                extend: (true, true),
            }
        ));
        builder.add_page(60.0, 60.0, |page| {
            page.raw(b"q 0 0 60 60 re W n");
            assert!(page.shading(b"Sh0"));
            page.raw(b"Q");
        });
        render(builder.finish())
    };

    let early = draw(0.25);
    let late = draw(0.75);

    // Green is the join between the two sub-functions, so it sits wherever the
    // bound puts it.
    let greenest = |bitmap: &Bitmap, x: f64| -> i32 {
        let (r, g, b) = at(bitmap, x, 30.0);
        i32::from(g) - i32::from(r).max(i32::from(b))
    };
    assert!(
        greenest(&early, 15.0) > greenest(&early, 45.0),
        "a bound at a quarter puts the join a quarter along"
    );
    assert!(
        greenest(&late, 45.0) > greenest(&late, 15.0),
        "and a bound at three quarters puts it three quarters along"
    );
    assert!(
        greenest(&early, 15.0) > 180 && greenest(&late, 45.0) > 180,
        "and the join really is the sub-functions' shared colour"
    );
}

// ---- tiling patterns -----------------------------------------------------

/// A tiling pattern from the builder tiles what a hand-written one tiles.
#[test]
fn a_tiling_pattern_from_the_builder_draws_what_a_hand_written_one_draws() {
    let cell = "0 0 1 rg 0 0 5 5 re f";
    let mut builder = DocumentBuilder::new();
    assert!(builder.add_tiling_pattern(
        b"P0",
        &TilingPattern {
            bbox: [0.0, 0.0, 5.0, 5.0],
            x_step: 10.0,
            y_step: 10.0,
            matrix: None,
            tiling_type: TilingType::ConstantSpacing,
            content: cell.as_bytes(),
        }
    ));
    builder.add_page(60.0, 60.0, |page| {
        assert!(page.set_fill_pattern(b"P0"));
        page.raw(b"0 0 60 60 re f");
    });

    let written = hand(
        "/Pattern << /P0 5 0 R >>",
        "/Pattern cs /P0 scn 0 0 60 60 re f",
        &format!(
            "5 0 obj\n<< /Type /Pattern /PatternType 1 /PaintType 1 /TilingType 1\n\
               /BBox [0 0 5 5] /XStep 10 /YStep 10 /Resources << >> /Length {} >>\n\
             stream\n{cell}\nendstream\nendobj\n",
            cell.len()
        ),
    );

    let built = render(builder.finish());
    // The steps are twice the cell, so the tiling leaves gaps -- which is what
    // says the steps were read rather than derived from the box.
    let inked = at(&built, 2.0, 2.0);
    let gap = at(&built, 7.0, 7.0);
    assert!(
        inked.2 > 200 && inked.0 < 60,
        "a cell is painted: {inked:?}"
    );
    assert!(gap.0 > 235 && gap.2 > 235, "and the gap is not: {gap:?}");
    same_picture(built, render(written), "a tiling pattern");
}

// ---- composite fonts -----------------------------------------------------

/// A TrueType program whose `cmap` disagrees with the glyph a caller wants.
///
/// Four glyphs. Glyph 0 is empty; glyph 1 is a square in the **bottom-left**
/// corner of the em; glyph 2 is a square in the **top-right**; glyph 3 fills
/// the em. The `cmap` maps `A` to glyph 1 and nothing else at all — so a build
/// that reached a glyph through a character draws in one corner and a build
/// that addressed the index draws in the other, and the two are told apart by
/// looking at two pixels rather than by looking for ink.
fn disagreeing_font() -> Vec<u8> {
    let square = |low: i16, high: i16| -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&1i16.to_be_bytes()); // one contour
        for value in [low, low, high, high] {
            out.extend_from_slice(&value.to_be_bytes()); // bounding box
        }
        out.extend_from_slice(&3u16.to_be_bytes()); // last point index
        out.extend_from_slice(&0u16.to_be_bytes()); // no instructions
        out.extend_from_slice(&[0x01, 0x01, 0x01, 0x01]); // on curve, i16 deltas
        let side = high - low;
        for dx in [low, side, 0, -side] {
            out.extend_from_slice(&dx.to_be_bytes());
        }
        for dy in [low, 0, side, 0] {
            out.extend_from_slice(&dy.to_be_bytes());
        }
        out
    };

    let mut glyf = Vec::new();
    let mut loca: Vec<u32> = vec![0, 0]; // glyph 0 is empty
    for (low, high) in [(0i16, 300i16), (400, 700), (0, 700)] {
        glyf.extend_from_slice(&square(low, high));
        loca.push(glyf.len() as u32);
    }
    let mut loca_bytes = Vec::new();
    for offset in &loca {
        loca_bytes.extend_from_slice(&offset.to_be_bytes());
    }

    let mut head = vec![0u8; 54];
    head[18..20].copy_from_slice(&1000u16.to_be_bytes());
    head[50..52].copy_from_slice(&1i16.to_be_bytes()); // long loca

    let mut maxp = vec![0u8; 32];
    maxp[0..4].copy_from_slice(&0x0001_0000u32.to_be_bytes());
    maxp[4..6].copy_from_slice(&4u16.to_be_bytes());

    let mut hhea = vec![0u8; 36];
    hhea[34..36].copy_from_slice(&4u16.to_be_bytes());

    let mut hmtx = Vec::new();
    for _ in 0..4u16 {
        hmtx.extend_from_slice(&1000u16.to_be_bytes());
        hmtx.extend_from_slice(&0i16.to_be_bytes());
    }

    // cmap format 4: `A` alone, mapping to glyph 1.
    let first = u16::from(b'A');
    let mut sub = Vec::new();
    for value in [4u16, 32, 0, 4, 4, 1, 0] {
        sub.extend_from_slice(&value.to_be_bytes());
    }
    for value in [
        first,
        0xFFFF,
        0,
        first,
        0xFFFF,
        1u16.wrapping_sub(first),
        1,
        0,
        0,
    ] {
        sub.extend_from_slice(&value.to_be_bytes());
    }
    let mut cmap = Vec::new();
    for value in [0u16, 1, 3, 1] {
        cmap.extend_from_slice(&value.to_be_bytes());
    }
    cmap.extend_from_slice(&12u32.to_be_bytes());
    cmap.extend_from_slice(&sub);

    let tables: [(&[u8; 4], &[u8]); 7] = [
        (b"cmap", &cmap),
        (b"glyf", &glyf),
        (b"head", &head),
        (b"hhea", &hhea),
        (b"hmtx", &hmtx),
        (b"loca", &loca_bytes),
        (b"maxp", &maxp),
    ];
    let mut out = Vec::new();
    out.extend_from_slice(&0x0001_0000u32.to_be_bytes());
    out.extend_from_slice(&(tables.len() as u16).to_be_bytes());
    out.extend_from_slice(&[0; 6]);
    let mut offset = 12 + tables.len() * 16;
    let mut body = Vec::new();
    for (tag, data) in tables {
        out.extend_from_slice(tag);
        out.extend_from_slice(&0u32.to_be_bytes());
        out.extend_from_slice(&(offset as u32).to_be_bytes());
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        offset += data.len();
        body.extend_from_slice(data);
    }
    out.extend_from_slice(&body);
    out
}

/// A glyph is addressed by **index**, and the font's `cmap` has no say in it.
///
/// This is the whole reason milestone 5 comes before the milestone that reads
/// glyph runs. The font's `cmap` maps `A` to glyph 1, which paints the
/// bottom-left corner of the em; glyph 2 paints the top-right. A composite
/// font asked for glyph 2 paints the top-right. A simple font asked for the
/// character `A` paints the bottom-left — correct, plausible, and not what a
/// caller holding a glyph index asked for.
///
/// The two are told apart at **both** corners in **both** documents, because a
/// build that drew nothing at all would satisfy either half on its own.
#[test]
fn a_composite_font_addresses_a_glyph_by_index_and_a_simple_font_cannot() {
    let program = disagreeing_font();

    let mut by_index = DocumentBuilder::new();
    assert!(by_index.add_cid_font(b"C0", b"Disagree", &program));
    by_index.add_page(60.0, 60.0, |page| {
        page.set_fill_rgb(0.0, 0.0, 0.0);
        assert!(page.glyphs(b"C0", 40.0, 10.0, 10.0, &[Glyph { id: 2, text: "A" }]));
    });

    let mut by_character = DocumentBuilder::new();
    assert!(by_character.add_embedded_font(b"F0", b"Disagree", &program));
    by_character.add_page(60.0, 60.0, |page| {
        page.set_fill_rgb(0.0, 0.0, 0.0);
        page.text(b"F0", 40.0, 10.0, 10.0, "A");
    });

    let indexed = render(by_index.finish());
    let charwise = render(by_character.finish());

    // Glyph 1 occupies 0..300 units of the em and glyph 2 occupies 400..700,
    // so at forty points from (10, 10) they land at 10..22 and 26..38.
    let low = (16.0, 16.0);
    let high = (32.0, 32.0);

    assert!(
        at(&indexed, high.0, high.1).0 < 40,
        "the composite font drew glyph 2, in the top-right of the em: {:?}",
        at(&indexed, high.0, high.1)
    );
    assert!(
        at(&indexed, low.0, low.1).0 > 235,
        "and not glyph 1, which is what its cmap would have given: {:?}",
        at(&indexed, low.0, low.1)
    );

    assert!(
        at(&charwise, low.0, low.1).0 < 40,
        "the simple font drew what the cmap says `A` is: {:?}",
        at(&charwise, low.0, low.1)
    );
    assert!(
        at(&charwise, high.0, high.1).0 > 235,
        "and could not have reached glyph 2 at all: {:?}",
        at(&charwise, high.0, high.1)
    );
}

/// `Page::text()` comes back through the `/ToUnicode` the writer wrote —
/// several glyphs standing for one character, and one standing for several.
///
/// This is the test that says synthesis paid for text extraction. Extraction
/// drives a **second** device through the same content stream, so a writer
/// half that the text extractor disagrees with is the two-paths-two-pictures
/// failure gap 29's milestone 4 found for `/Mask`, in the one place this
/// engine can see it from both sides.
#[test]
fn page_text_comes_back_through_the_to_unicode_the_writer_wrote() {
    let mut builder = DocumentBuilder::new();
    assert!(builder.add_cid_font(b"C0", b"Disagree", &disagreeing_font()));
    builder.add_page(200.0, 60.0, |page| {
        assert!(page.glyphs(
            b"C0",
            12.0,
            5.0,
            20.0,
            &[
                // Two different glyphs standing for one character.
                Glyph { id: 1, text: "f" },
                Glyph { id: 2, text: "f" },
                // One glyph standing for three characters.
                Glyph { id: 3, text: "ffi" },
            ]
        ));
    });

    let document = Document::open(builder.finish()).expect("it opens");
    let page = document.page(0).expect("a page");
    let text = page.text().plain_text();
    assert!(
        text.contains("ffffi"),
        "the run reads back as the text its glyphs stood for, got {text:?}"
    );

    // And the character counts are the mapping's, not the glyph count's: three
    // glyphs produced five characters, which is what a many-to-one and a
    // one-to-many mapping in one run means.
    let letters: BTreeMap<char, usize> =
        text.chars()
            .filter(|c| !c.is_whitespace())
            .fold(BTreeMap::new(), |mut counts, c| {
                *counts.entry(c).or_default() += 1;
                counts
            });
    assert_eq!(
        letters.get(&'f').copied(),
        Some(4),
        "four f's from {text:?}"
    );
    assert_eq!(letters.get(&'i').copied(), Some(1), "one i from {text:?}");
}

/// A glyph the document never drew is not in `/W`, and `/DW` is what a reader
/// falls back to — so text extraction stays right when the widths run out.
#[test]
fn a_composite_run_positions_its_glyphs_from_the_widths_the_font_states() {
    let mut builder = DocumentBuilder::new();
    assert!(builder.add_cid_font(b"C0", b"Disagree", &disagreeing_font()));
    builder.add_page(200.0, 60.0, |page| {
        assert!(page.glyphs(
            b"C0",
            10.0,
            0.0,
            20.0,
            &[Glyph { id: 1, text: "a" }, Glyph { id: 2, text: "b" }]
        ));
    });

    let document = Document::open(builder.finish()).expect("it opens");
    let page = document.page(0).expect("a page");
    let extracted = page.text();
    let quads: Vec<(f64, f64)> = extracted
        .blocks
        .iter()
        .flat_map(|block| block.lines.iter())
        .flat_map(|line| line.chars.iter())
        .map(|c| (c.quad.ll.0, c.quad.lr.0))
        .collect();

    assert_eq!(quads.len(), 2, "two characters: {quads:?}");
    // Every advance in this font is 1000 units at 1000 per em, so a ten-point
    // glyph is ten points wide and the second starts where the first ended.
    assert!(
        (quads[0].0 - 0.0).abs() < 0.5 && (quads[1].0 - 10.0).abs() < 0.5,
        "the second glyph is one advance along: {quads:?}"
    );
}
