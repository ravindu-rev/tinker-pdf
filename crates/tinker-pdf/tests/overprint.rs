//! Overprint simulation (RC-03; ISO 32000-2 8.6.7, 11.7.4;
//! `docs/design/overprint-separations.md`): `RenderOptions::simulate_overprint`
//! draws a page on its plates — the canvas `Page::separations` draws — and
//! turns that composite into the format asked for by the design's closed form.
//!
//! Every expectation here is worked out from the fixture as it is written and
//! from the clauses — which space names which colorant, at which tint, the
//! tint transform's formula, 8.6.4.4's relation — and never read off the code
//! that draws it. Where a test needs the plates a page composites, it reads
//! them through `Page::separations`, the same pipeline drawing the same
//! canvas (ruling 5), and composites them itself.

mod render_support;

use render_support::{raw_pdf, raw_stream, spot_page, SpotPage, SpotSpaces};
use tinker_pdf::{
    Document, ForcedColours, Page, Parallelism, PixelFormat, RenderOptions, RenderWarning,
    SeparationOptions, Separations,
};

/// The test's own `a × b / 255`, rounded half up: never a tie, 255 being odd.
fn times(a: u8, b: u8) -> u8 {
    ((2 * u32::from(a) * u32::from(b) + 255) / 510) as u8
}

/// A value in `0..=1` as a byte, rounded half away from zero.
fn byte(value: f64) -> u8 {
    (value.clamp(0.0, 1.0) * 255.0).round() as u8
}

/// The colour alone, at tint `t`, of a spot whose tint transform puts
/// `inks` — each 0 or 1 — onto DeviceCMYK at a tint of 1, linearly from no
/// ink: its light, by 8.6.4.4's `(1 − c)(1 − k)`, and its ink. The tint
/// transform's output at `x = t/255` is `x` where an ink is 1 and 0 where it is
/// 0 — 7.10.3's `c0 + x·(c1 − c0)` from `c0 = 0`, and the `/DeviceN` pages'
/// type 4 functions, which hand each component to one ink. No tint is white
/// and no ink.
fn alone(inks: [u8; 4], t: u8) -> ([u8; 3], [u8; 4]) {
    if t == 0 {
        return ([255; 3], [0; 4]);
    }
    let x = f64::from(t) / 255.0;
    let ink = inks.map(|ink| if ink == 0 { 0.0 } else { x });
    let light = [ink[0], ink[1], ink[2]].map(|v| byte((1.0 - v) * (1.0 - ink[3])));
    (light, ink.map(byte))
}

/// The design's composite into light of one pixel of plates: the process
/// tints' light by 8.6.4.4's relation in bytes, each of red, green and blue
/// then multiplied by that channel of each spot's light alone at its tint.
fn composite_light(process: [u8; 4], spots: &[([u8; 4], u8)]) -> [u8; 3] {
    let [c, m, y, k] = process;
    let mut light = [c, m, y].map(|v| times(255 - v, 255 - k));
    for &(inks, t) in spots {
        let (alone, _) = alone(inks, t);
        for (slot, s) in light.iter_mut().zip(alone) {
            *slot = times(*slot, s);
        }
    }
    light
}

/// The design's composite into ink of one pixel of plates: the process
/// tints, each spot's ink alone folded into each as `255 − (255 − x)(255 −
/// s)/255`.
fn composite_ink(process: [u8; 4], spots: &[([u8; 4], u8)]) -> [u8; 4] {
    let mut ink = process;
    for &(inks, t) in spots {
        let (_, alone) = alone(inks, t);
        for (slot, s) in ink.iter_mut().zip(alone) {
            *slot = 255 - times(255 - *slot, 255 - s);
        }
    }
    ink
}

/// The grey a light colour reads as, by the weights a grey canvas stores it
/// with: 0.299, 0.587 and 0.114, rounded.
fn luma([r, g, b]: [u8; 3]) -> u8 {
    ((u32::from(r) * 299 + u32::from(g) * 587 + u32::from(b) * 114 + 500) / 1000) as u8
}

/// `page`'s plates at `render`, every colorant a plate of its own.
fn plates(page: &Page, render: &RenderOptions) -> Separations {
    page.separations(&SeparationOptions {
        render: render.clone(),
        ..SeparationOptions::default()
    })
}

/// What a pixel `at` of `plates` composites, as `format` stores it.
fn expected_pixel(
    plates: &Separations,
    spots: &[(String, [u8; 4])],
    at: usize,
    format: PixelFormat,
) -> Vec<u8> {
    let tint = |plate: usize| plates.plates[plate].tints[at];
    let process = [tint(0), tint(1), tint(2), tint(3)];
    let spot_tints: Vec<([u8; 4], u8)> = spots
        .iter()
        .enumerate()
        .map(|(i, (_, inks))| (*inks, tint(4 + i)))
        .collect();
    let alpha = plates.alpha.as_ref().map_or(255, |alpha| alpha[at]);
    match format {
        PixelFormat::CmykA8 => {
            let [c, m, y, k] = composite_ink(process, &spot_tints);
            vec![c, m, y, k, alpha]
        }
        PixelFormat::Gray8 => vec![luma(composite_light(process, &spot_tints))],
        PixelFormat::Rgba8 => {
            let [r, g, b] = composite_light(process, &spot_tints);
            vec![r, g, b, alpha]
        }
        PixelFormat::Bgra8 => {
            let [r, g, b] = composite_light(process, &spot_tints);
            vec![b, g, r, alpha]
        }
        _ => composite_light(process, &spot_tints).to_vec(),
    }
}

/// Holds every pixel of `bitmap`, rendered at `render`, to what the page's
/// plates at the same options composite.
#[track_caller]
fn holds_the_composite(
    page: &Page,
    spots: &[(String, [u8; 4])],
    render: &RenderOptions,
    format: PixelFormat,
    what: &str,
) {
    let bitmap = page.render(render);
    let plates = plates(page, render);
    assert_eq!(bitmap.format, format, "{what}: the format handed back");
    assert_eq!(
        (bitmap.width, bitmap.height),
        (plates.width, plates.height),
        "{what}: the plates' size"
    );
    let names: Vec<String> = plates
        .plates
        .iter()
        .skip(4)
        .map(|plate| String::from_utf8_lossy(&plate.name).into_owned())
        .collect();
    let named: Vec<String> = spots.iter().map(|(name, _)| name.clone()).collect();
    assert_eq!(names, named, "{what}: a plate for each spot, in order");
    let n = format.components();
    for at in 0..(plates.width * plates.height) as usize {
        let want = expected_pixel(&plates, spots, at, format);
        assert_eq!(
            &bitmap.data[at * n..(at + 1) * n],
            want.as_slice(),
            "{what}: pixel ({}, {})",
            at % plates.width as usize,
            at / plates.width as usize
        );
    }
}

/// [`RenderOptions`] simulating overprint, in `format`.
fn simulated(format: PixelFormat) -> RenderOptions {
    RenderOptions {
        format,
        allow_cmyk: format == PixelFormat::CmykA8,
        simulate_overprint: true,
        ..RenderOptions::default()
    }
}

/// **The switch converts the composite by the model** (RC-03.3): a page
/// painting two spots — in `/Separation` spaces, and in a `/DeviceN` — and
/// process inks without overprint, fills, bands over each other and text in
/// an embedded face, renders switched on as the composite of its plates the
/// test works out pixel by pixel: as `Rgb8`, as `CmykA8` with `allow_cmyk`,
/// as `Gray8` and blue-first `Bgra8`, and as a transparent `Rgba8` whose
/// alpha is the plates'; switched off, byte for byte the render without the
/// switch, warnings included, before and after a render with it on.
#[test]
fn the_switch_converts_the_composite_by_the_model() {
    for spaces in [SpotSpaces::Separation, SpotSpaces::DeviceN] {
        let SpotPage { bytes, spots } = spot_page(2, spaces);
        let what = format!("{spaces:?}");
        let document = Document::open(bytes.clone()).expect("it opens");
        let page = document.page(0).expect("a page");
        let plain = page.render(&RenderOptions::default());

        for format in [
            PixelFormat::Rgb8,
            PixelFormat::CmykA8,
            PixelFormat::Gray8,
            PixelFormat::Bgra8,
        ] {
            holds_the_composite(
                &page,
                &spots,
                &simulated(format),
                format,
                &format!("{what} as {format:?}"),
            );
        }
        holds_the_composite(
            &page,
            &spots,
            &RenderOptions {
                transparent: true,
                ..simulated(PixelFormat::Rgba8)
            },
            PixelFormat::Rgba8,
            &format!("{what}, transparent"),
        );

        let off = RenderOptions {
            simulate_overprint: false,
            ..RenderOptions::default()
        };
        let after = page.render(&off);
        assert_eq!(after.data, plain.data, "{what}: off, after a render on");
        assert_eq!(after.warnings, plain.warnings, "{what}: off, the warnings");
        let fresh = Document::open(bytes).expect("it opens");
        let fresh = fresh.page(0).expect("a page").render(&off);
        assert_eq!(fresh.data, plain.data, "{what}: off, on a fresh document");
    }
}

/// **A spot at a tint composites its colour alone at that tint**: a page
/// filling a quarter each with one spot at four tints, over paper and over a
/// process backdrop, composites into light as the spot's alternate at each
/// tint, the process ink under it knocked out since nothing overprints —
/// each pixel worked out here from the tint as written, with no plates read
/// — and its tint 0 leaves the paper white.
#[test]
fn a_spot_composites_its_colour_alone_at_its_tint() {
    let tints = [0.0, 0.2, 0.6, 1.0];
    let mut content = String::from("0 0 0.4 0 k 0 0 8 4 re f\n");
    for (i, tint) in tints.iter().enumerate() {
        content.push_str(&format!("/G cs {tint} scn {} 0 2 8 re f\n", 2 * i));
    }
    let bytes = raw_pdf(&[
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Count 1 /Kids [3 0 R] >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 8 8] /Resources << /ColorSpace << \
           /G [/Separation /Gold /DeviceCMYK << /FunctionType 2 /Domain [0 1] \
           /C0 [0 0 0 0] /C1 [0 0 1 0] /N 1 >>] >> >> /Contents 4 0 R >>"
            .to_vec(),
        raw_stream("", content.as_bytes()),
    ]);
    let document = Document::open(bytes).expect("it opens");
    let page = document.page(0).expect("a page");
    let bitmap = page.render(&simulated(PixelFormat::Rgb8));
    assert_eq!((bitmap.width, bitmap.height), (8, 8));
    for row in 0..8usize {
        for col in 0..8usize {
            let t = byte(tints[col / 2]);
            // The spot knocks out the process ink under it (no overprint),
            // so every pixel is the spot's colour alone over paper.
            let want = alone([0, 0, 1, 0], t).0;
            let at = (row * 8 + col) * 3;
            assert_eq!(
                &bitmap.data[at..at + 3],
                &want,
                "({col}, {row}) at tint {}",
                tints[col / 2]
            );
        }
    }
    assert_eq!(&bitmap.data[..3], &[255, 255, 255], "tint 0 is paper");
}

/// **A display list, a render in bands and the page's parts draw the
/// simulated page as the page does** (RC-03.3): a list a caller recorded,
/// rendered with the switch on, replays the page recorded again with its
/// colours' colorants — the same bitmap and warnings as `Page::render`, at
/// two scales, and again on a second render from the recording it kept; a
/// render in three bands is the page drawn whole; and the same list with
/// the switch off is the render without it.
#[test]
fn a_display_list_and_bands_draw_the_simulated_page_alike() {
    for spaces in [SpotSpaces::Separation, SpotSpaces::DeviceN] {
        let SpotPage { bytes, .. } = spot_page(8, spaces);
        let document = Document::open(bytes).expect("it opens");
        let page = document.page(0).expect("a page");
        let list = page.display_list();
        for scale in [1.0, 2.5] {
            for format in [PixelFormat::Rgb8, PixelFormat::CmykA8] {
                let options = RenderOptions {
                    scale,
                    ..simulated(format)
                };
                let what = format!("{spaces:?} at {scale}x as {format:?}");
                let direct = page.render(&options);
                for round in ["first", "second"] {
                    let replayed = list.render(&options);
                    assert_eq!(replayed.data, direct.data, "{what}: the {round} replay");
                    assert_eq!(
                        replayed.warnings, direct.warnings,
                        "{what}: the {round} replay's warnings"
                    );
                }
                let banded = page.render(&RenderOptions {
                    parallelism: Parallelism::Threads(3),
                    ..options.clone()
                });
                assert_eq!(banded.data, direct.data, "{what}: in bands");
                assert_eq!(banded.warnings, direct.warnings, "{what}: bands' warnings");
                let off = RenderOptions {
                    simulate_overprint: false,
                    ..options.clone()
                };
                assert_eq!(
                    list.render(&off).data,
                    page.render(&off).data,
                    "{what}: the list, switched off"
                );
            }
        }
    }
}

/// A page of `count` spots, each a `/Separation` onto cyan, filling a
/// column one point wide at a tint of 1, on a page `count` points wide.
fn many_spots(count: usize) -> Vec<u8> {
    let mut spaces = String::new();
    let mut content = String::new();
    for i in 0..count {
        spaces.push_str(&format!(
            "/S{i} [/Separation /Spot{i} /DeviceCMYK << /FunctionType 2 /Domain [0 1] \
             /C0 [0 0 0 0] /C1 [1 0 0 0] /N 1 >>] "
        ));
        content.push_str(&format!("/S{i} cs 1 scn {i} 0 1 2 re f\n"));
    }
    raw_pdf(&[
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Count 1 /Kids [3 0 R] >>".to_vec(),
        format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {count} 2] /Resources << \
             /ColorSpace << {spaces} >> >> /Contents 4 0 R >>"
        )
        .into_bytes(),
        raw_stream("", content.as_bytes()),
    ])
}

/// **Spots past the plate cap composite through their alternates, and the
/// render says so** (RC-03.3): a page of sixty-six spots keeps sixty-four
/// plates, the last two in inventory order drawn on the process plates
/// through their alternates — every column the cyan its alternate is,
/// whether its spot kept a plate or not — and the render carries
/// `PlatesCapped` naming both counts, once, as the page's separations do; a
/// page within the cap carries none.
#[test]
fn spots_past_the_cap_composite_through_their_alternates() {
    let count = tinker_pdf::MAX_SPOT_PLATES + 2;
    let document = Document::open(many_spots(count)).expect("it opens");
    let page = document.page(0).expect("a page");
    let bitmap = page.render(&simulated(PixelFormat::Rgb8));
    let capped = RenderWarning::PlatesCapped {
        named: count,
        kept: tinker_pdf::MAX_SPOT_PLATES,
    };
    assert_eq!(
        bitmap.warnings.iter().filter(|w| **w == capped).count(),
        1,
        "{:?}",
        bitmap.warnings
    );
    assert!(plates(&page, &RenderOptions::default())
        .warnings
        .contains(&capped));
    let cyan = alone([1, 0, 0, 0], 255).0;
    for col in 0..count {
        let at = col * 3;
        assert_eq!(&bitmap.data[at..at + 3], &cyan, "column {col}");
    }

    let document = Document::open(many_spots(4)).expect("it opens");
    let within = document
        .page(0)
        .expect("a page")
        .render(&simulated(PixelFormat::Rgb8));
    assert!(
        !within
            .warnings
            .iter()
            .any(|w| matches!(w, RenderWarning::PlatesCapped { .. })),
        "{:?}",
        within.warnings
    );
}

/// **Where the switch has nothing to draw by, it changes nothing**: a page
/// whose transparency group composites in DeviceRGB draws its spots through
/// their alternates in that space with the switch on as off (11.7.3); forced
/// colours draw coverage, switched on or not; and a command stream, which
/// holds no inventory of the page's spots, draws as it does with the switch
/// off.
#[test]
fn where_the_switch_has_nothing_to_draw_by_it_changes_nothing() {
    let grouped = raw_pdf(&[
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Count 1 /Kids [3 0 R] >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 20 10] \
           /Group << /S /Transparency /CS /DeviceRGB >> /Resources << /ColorSpace << \
           /G [/Separation /Gold /DeviceCMYK << /FunctionType 2 /Domain [0 1] \
           /C0 [0 0 0 0] /C1 [0 0.3 1 0] /N 1 >>] >> >> /Contents 4 0 R >>"
            .to_vec(),
        raw_stream(
            "",
            b"0.2 0 0 0.1 k 0 0 20 10 re f /G cs 0.7 scn 2.5 2.5 9 5 re f \
              0 0 1 rg 8 3 9 4 re f",
        ),
    ]);
    let document = Document::open(grouped).expect("it opens");
    let page = document.page(0).expect("a page");
    for format in [PixelFormat::Rgb8, PixelFormat::CmykA8] {
        let on = page.render(&simulated(format));
        let off = page.render(&RenderOptions {
            simulate_overprint: false,
            ..simulated(format)
        });
        assert_eq!(on.data, off.data, "a DeviceRGB page group as {format:?}");
        assert_eq!(on.warnings, off.warnings);
    }

    let SpotPage { bytes, .. } = spot_page(2, SpotSpaces::Separation);
    let document = Document::open(bytes).expect("it opens");
    let page = document.page(0).expect("a page");
    let forced = RenderOptions {
        forced_colours: Some(ForcedColours::default()),
        ..RenderOptions::default()
    };
    let forced_on = page.render(&RenderOptions {
        simulate_overprint: true,
        ..forced.clone()
    });
    assert_eq!(forced_on.data, page.render(&forced).data, "forced colours");

    let stream = tinker_pdf::CommandStream::parse(&page.command_stream()).expect("it reads");
    assert_eq!(
        stream.render(&simulated(PixelFormat::Rgb8)).data,
        stream.render(&RenderOptions::default()).data,
        "a command stream"
    );
}
