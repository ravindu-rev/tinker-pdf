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
    Document, ForcedColours, Page, Parallelism, PixelFormat, PixelRegion, RenderOptions,
    RenderWarning, SeparationOptions, Separations,
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

// ---- flat colours overprint (RC-03.5) ----------------------------------------

/// The page the overprint cases draw on, in points and pixels alike.
const CASE_PAGE: u32 = 24;

/// The backdrop every overprint case paints over: a `/DeviceN` of the four
/// process colorants and both spots, each at a tint of its own, so every
/// channel holds ink before the paint.
const BACKDROP_TINTS: [f64; 6] = [0.2, 0.4, 0.6, 0.8, 0.5, 0.3];

/// The process inks each spot's tint transform paints at a tint of 1: Gold
/// onto yellow, Teal onto cyan.
const SPOT_INKS: [[u8; 4]; 2] = [[0, 0, 1, 0], [1, 0, 0, 0]];

/// The graphics states the cases are drawn under, by resource name: no
/// overprint; both flags under `/OPM 0`; both under `/OPM 1`; `/OP` alone,
/// which `/op` follows (Table 57: `/op` absent is `/OP`); and `/OP` on with
/// `/op` off. Each with its stroking flag, its non-stroking flag and
/// whether the non-zero mode is in force.
const STATES: [(&str, bool, bool, bool); 5] = [
    ("G0", false, false, false),
    ("G1", true, true, false),
    ("G2", true, true, true),
    ("G3", true, true, false),
    ("G4", true, false, false),
];

/// One colour an overprint case paints: how the fill slot and the stroke
/// slot set it, its tint on each of the six channels — cyan, magenta,
/// yellow, black, Gold, Teal — and which channels it marks with its flag
/// on, under the non-zero mode or not (Table 146, worked out here from the
/// clause: the design's "Overprint: which channels a paint marks").
struct Colour {
    name: &'static str,
    fill: String,
    stroke: String,
    tints: [u8; 6],
    marks: fn(bool) -> [bool; 6],
    /// Whether it paints at all: `/None` has no effect on the page.
    paints: bool,
}

/// Light turned into ink by 8.6.4.4's relation inverted with maximum
/// undercolour removal, worked out here: black takes the grey,
/// `K = 255 − max(r, g, b)`, and each ink what is left.
fn ink_of_light([r, g, b]: [u8; 3]) -> [u8; 4] {
    let white = u32::from(r.max(g).max(b));
    if white == 0 {
        return [0, 0, 0, 255];
    }
    let ink = |v: u8| (255 - (u32::from(v) * 255 + white / 2) / white) as u8;
    [ink(r), ink(g), ink(b), (255 - white) as u8]
}

/// DeviceCMYK's marks: its four process channels, or under the non-zero
/// mode those of `[0.6, 0, 0.3, 0]` and `[0.001, 0, 0.3, 0]` the file gives
/// as not zero, cyan and yellow.
fn cmyk_marks(nonzero: bool) -> [bool; 6] {
    if nonzero {
        [true, false, true, false, false, false]
    } else {
        [true, true, true, true, false, false]
    }
}

/// The colours the cases paint, one of each kind of the design's first
/// table and its edges.
fn colours() -> Vec<Colour> {
    let process = |_: bool| [true, true, true, true, false, false];
    let cmyk = |name: &'static str, c: [f64; 4]| Colour {
        name,
        fill: format!("{} {} {} {} k", c[0], c[1], c[2], c[3]),
        stroke: format!("{} {} {} {} K", c[0], c[1], c[2], c[3]),
        tints: [byte(c[0]), byte(c[1]), byte(c[2]), byte(c[3]), 0, 0],
        marks: cmyk_marks,
        paints: true,
    };
    let [c, m, y, k] = ink_of_light([0.2, 0.4, 0.9].map(byte));
    let [gc, gm, gy, gk] = ink_of_light([byte(0.25); 3]);
    let named = |name: &'static str,
                 fill: &str,
                 stroke: &str,
                 tints: [u8; 6],
                 marks: fn(bool) -> [bool; 6],
                 paints: bool| Colour {
        name,
        fill: fill.to_string(),
        stroke: stroke.to_string(),
        tints,
        marks,
        paints,
    };
    vec![
        // DeviceCMYK as the current colour: under `/OPM 1` only the
        // components the file gives as not zero.
        cmyk("k", [0.6, 0.0, 0.3, 0.0]),
        // 8.6.7: `0.001` is the byte 0 and not zero, so its cyan is marked,
        // at no ink, under `/OPM 1`.
        cmyk("k, a cyan of 0.001", [0.001, 0.0, 0.3, 0.0]),
        named(
            "DeviceCMYK by name",
            "/DeviceCMYK cs 0.6 0 0.3 0 sc",
            "/DeviceCMYK CS 0.6 0 0.3 0 SC",
            [byte(0.6), 0, byte(0.3), 0, 0, 0],
            cmyk_marks,
            true,
        ),
        named(
            "a /Separation of cyan",
            "/SC cs 0.6 scn",
            "/SC CS 0.6 SCN",
            [byte(0.6), 0, 0, 0, 0, 0],
            |_| [true, false, false, false, false, false],
            true,
        ),
        named(
            "a /Separation of a spot",
            "/SG cs 0.7 scn",
            "/SG CS 0.7 SCN",
            [0, 0, 0, 0, byte(0.7), 0],
            |_| [false, false, false, false, true, false],
            true,
        ),
        named(
            "/All",
            "/SA cs 0.4 scn",
            "/SA CS 0.4 SCN",
            [byte(0.4); 6],
            |_| [true; 6],
            true,
        ),
        named(
            "/None",
            "/SN cs 1 scn",
            "/SN CS 1 SCN",
            [0; 6],
            |_| [false; 6],
            false,
        ),
        named(
            "a /DeviceN of both spots",
            "/DN cs 0.2 0.9 scn",
            "/DN CS 0.2 0.9 SCN",
            [0, 0, 0, 0, byte(0.2), byte(0.9)],
            |_| [false, false, false, false, true, true],
            true,
        ),
        named(
            "a /DeviceN of a spot and /None",
            "/DG cs 0.6 0.3 scn",
            "/DG CS 0.6 0.3 SCN",
            [0, 0, 0, 0, byte(0.6), 0],
            |_| [false, false, false, false, true, false],
            true,
        ),
        named(
            "an NChannel of a spot and cyan",
            "/NC cs 0.5 0.8 scn",
            "/NC CS 0.5 0.8 SCN",
            [byte(0.8), 0, 0, 0, byte(0.5), 0],
            |_| [true, false, false, false, true, false],
            true,
        ),
        named(
            "DeviceRGB",
            "0.2 0.4 0.9 rg",
            "0.2 0.4 0.9 RG",
            [c, m, y, k, 0, 0],
            process,
            true,
        ),
        named(
            "DeviceGray",
            "0.25 g",
            "0.25 G",
            [gc, gm, gy, gk, 0, 0],
            process,
            true,
        ),
    ]
}

/// How a case paints, and whether it reads the stroking flag: a fill, a
/// stroke, a glyph filled — drawn from its mask — and one stroked, and a
/// stencil mask; each given the colour's operators for its slot, the
/// paint's content after them.
const PAINTS: [(&str, bool, &str); 5] = [
    ("a fill", false, "4 4 16 16 re f"),
    ("a stroke", true, "2 w 4 12 m 20 12 l S"),
    ("a glyph", false, "BT /F0 18 Tf 3 5 Td (B) Tj ET"),
    (
        "a stroked glyph",
        true,
        "BT /F0 18 Tf 1 Tr 2 w 3 5 Td (B) Tj ET",
    ),
    ("a stencil mask", false, "q 16 0 0 16 4 4 cm /M0 Do Q"),
];

/// A case page: `content` over the page's resources — the backdrop's and
/// the cases' spaces, the states, the embedded face, a checkered stencil
/// mask, an uncoloured pattern — and `/DefaultCMYK` among the spaces where
/// `default_cmyk` asks.
fn case_page(content: &str, default_cmyk: bool) -> Document {
    case_page_with(content, default_cmyk, &Extra::default())
}

/// What a case page holds beyond [`case_page`]'s: entries of its
/// `/ExtGState`, `/XObject`, `/Shading` and `/Pattern` dictionaries and the
/// objects they name, numbered from 13.
#[derive(Default)]
struct Extra {
    ext_g_states: String,
    xobjects: String,
    shadings: String,
    patterns: String,
    objects: Vec<Vec<u8>>,
}

/// [`case_page`] holding `extra` too.
fn case_page_with(content: &str, default_cmyk: bool, extra: &Extra) -> Document {
    let ramp =
        |c1: &str| format!("<< /FunctionType 2 /Domain [0 1] /C0 [0 0 0 0] /C1 [{c1}] /N 1 >>");
    let mut spaces = format!(
        "/BK [/DeviceN [/Cyan /Magenta /Yellow /Black /Gold /Teal] /DeviceCMYK 10 0 R] \
         /SG [/Separation /Gold /DeviceCMYK {}] /ST [/Separation /Teal /DeviceCMYK {}] \
         /SC [/Separation /Cyan /DeviceCMYK {}] /SA [/Separation /All /DeviceCMYK {}] \
         /SN [/Separation /None /DeviceCMYK {}] \
         /DN [/DeviceN [/Gold /Teal] /DeviceCMYK 11 0 R] \
         /DG [/DeviceN [/Gold /None] /DeviceCMYK 11 0 R] \
         /NC [/DeviceN [/Gold /Cyan] /DeviceCMYK 11 0 R << /Subtype /NChannel >>] \
         /PG [/Pattern [/Separation /Gold /DeviceCMYK {}]] /PK [/Pattern /DeviceCMYK] \
         /PR [/Pattern /DeviceRGB] /PY [/Pattern /DeviceGray]",
        ramp("0 0 1 0"),
        ramp("1 0 0 0"),
        ramp("1 0 0 0"),
        ramp("0 0 0 1"),
        ramp("1 1 1 1"),
        ramp("0 0 1 0"),
    );
    if default_cmyk {
        spaces.push_str(" /DefaultCMYK [/ICCBased 12 0 R]");
    }
    let states = STATES
        .iter()
        .map(|(name, stroke, fill, nonzero)| match *name {
            "G3" => format!("/{name} << /OP {stroke} >>"),
            _ => format!(
                "/{name} << /OP {stroke} /op {fill} /OPM {} >>",
                u8::from(*nonzero)
            ),
        })
        .collect::<Vec<_>>()
        .join(" ");
    let mut objects = vec![
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Count 1 /Kids [3 0 R] >>".to_vec(),
        format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {CASE_PAGE} {CASE_PAGE}] /Resources << \
             /ColorSpace << {spaces} >> /ExtGState << {states} {} >> /Font << /F0 5 0 R >> \
             /XObject << /M0 8 0 R {} >> /Pattern << /P0 9 0 R {} >> /Shading << {} >> >> \
             /Contents 4 0 R >>",
            extra.ext_g_states, extra.xobjects, extra.patterns, extra.shadings
        )
        .into_bytes(),
        raw_stream("", content.as_bytes()),
    ];
    objects.extend(render_support::curvy_font_objects());
    // Object 8: an 8 by 8 checkered stencil, two pixels a sample at 1x.
    let checker: Vec<u8> = (0..8)
        .map(|row| if row % 2 == 0 { 0xAA } else { 0x55 })
        .collect();
    objects.push(raw_stream(
        "/Type /XObject /Subtype /Image /Width 8 /Height 8 /ImageMask true /BitsPerComponent 1",
        &checker,
    ));
    // Object 9: an uncoloured tiling pattern whose cell fills itself.
    objects.push(raw_stream(
        "/Type /Pattern /PatternType 1 /PaintType 2 /TilingType 1 /BBox [0 0 4 4] \
         /XStep 4 /YStep 4 /Resources << >>",
        b"0 0 4 4 re f",
    ));
    // Objects 10 and 11: the `/DeviceN` spaces' tint transforms.
    objects.push(raw_stream(
        "/FunctionType 4 /Domain [0 1 0 1 0 1 0 1 0 1 0 1] /Range [0 1 0 1 0 1 0 1]",
        b"{ pop pop }",
    ));
    objects.push(raw_stream(
        "/FunctionType 4 /Domain [0 1 0 1] /Range [0 1 0 1 0 1 0 1]",
        b"{ pop pop 0 0 0 0 }",
    ));
    // Object 12: a profile no reader parses, so the default reads as a
    // four-component space of its own.
    objects.push(raw_stream("/N 4", b"not a profile"));
    objects.extend(extra.objects.iter().cloned());
    Document::open(raw_pdf(&objects)).expect("it opens")
}

/// The backdrop's operators: every channel inked over the whole page.
fn backdrop_operators() -> String {
    let tints: Vec<String> = BACKDROP_TINTS.iter().map(f64::to_string).collect();
    format!(
        "/BK cs {} scn 0 0 {CASE_PAGE} {CASE_PAGE} re f\n",
        tints.join(" ")
    )
}

/// The coverage `content` gives each pixel: what it takes off white, drawn
/// in black on a grey page with nothing else — `255 − grey`, since a black
/// paint over white composites to `255 − c` exactly.
fn coverage_of(content: &str) -> Vec<u8> {
    coverage_of_with(content, &Extra::default())
}

/// [`coverage_of`] on a case page holding `extra` too.
fn coverage_of_with(content: &str, extra: &Extra) -> Vec<u8> {
    let document = case_page_with(content, false, extra);
    let page = document.page(0).expect("a page");
    let grey = page.render(&RenderOptions {
        format: PixelFormat::Gray8,
        ..RenderOptions::default()
    });
    grey.data.iter().map(|v| 255 - v).collect()
}

/// What a case's plates hold at a pixel its paint covers by `c`: each
/// channel the paint marks 11.3.6's `Normal` composite of its tint over the
/// backdrop's, `cs·c + cb·(1 − c)` in 255ths; each it leaves, and every
/// channel where it does not paint, the backdrop's.
fn case_plates(tints: [u8; 6], paints: bool, marks: [bool; 6], c: u8) -> [u8; 6] {
    let backdrop = BACKDROP_TINTS.map(byte);
    let mut plates = backdrop;
    if !paints || c == 0 {
        return plates;
    }
    for channel in 0..6 {
        if marks[channel] {
            plates[channel] = times(tints[channel], c) + times(backdrop[channel], 255 - c);
        }
    }
    plates
}

/// The composite of one pixel of a case's plates, as `format` stores it.
fn case_composite(plates: [u8; 6], format: PixelFormat) -> Vec<u8> {
    let process = [plates[0], plates[1], plates[2], plates[3]];
    let spots = [(SPOT_INKS[0], plates[4]), (SPOT_INKS[1], plates[5])];
    match format {
        PixelFormat::CmykA8 => {
            let [c, m, y, k] = composite_ink(process, &spots);
            vec![c, m, y, k, 255]
        }
        _ => composite_light(process, &spots).to_vec(),
    }
}

/// Holds `page`, rendered simulating overprint in `format`, to the plates a
/// paint of `tints` covering each pixel by `coverage` and marking `marks`
/// composite to, pixel by pixel.
#[track_caller]
fn holds_the_case(
    page: &Page,
    (tints, paints, marks): ([u8; 6], bool, [bool; 6]),
    coverage: &[u8],
    format: PixelFormat,
    what: &str,
) {
    let bitmap = page.render(&simulated(format));
    assert_eq!(bitmap.format, format, "{what}");
    let n = format.components();
    let mut whole = 0;
    for (at, &c) in coverage.iter().enumerate() {
        let want = case_composite(case_plates(tints, paints, marks, c), format);
        assert_eq!(
            &bitmap.data[at * n..(at + 1) * n],
            want.as_slice(),
            "{what}: pixel ({}, {}) at coverage {c}",
            at % CASE_PAGE as usize,
            at / CASE_PAGE as usize
        );
        whole += usize::from(c == 255);
    }
    assert!(whole > 0, "{what}: the paint covers a whole pixel");
}

/// **Each overprint case paints its closed form** (RC-03.5): over a backdrop
/// with ink in every channel, a fill, a stroke, a glyph drawn from its mask,
/// a stroked glyph and a stencil mask, each in DeviceCMYK set by `k` and by
/// name, a cyan of `0.001`, a `/Separation` of a process colorant and of a
/// spot, `/All`, `/None`, a `/DeviceN` of two spots and of a spot and
/// `/None`, an `NChannel`, DeviceRGB and DeviceGray, under no overprint,
/// `/OPM 0`, `/OPM 1`, `/OP` alone and `/OP` without `/op`, render as `Rgb8`
/// and as `CmykA8` the composite of the plates the test works out from
/// Table 146: a paint whose flag is clear — `/op` for what fills, `/OP` for
/// what strokes — marks every channel, erasing what it does not name; one
/// whose flag is set marks the channels its colour names, DeviceCMYK under
/// `/OPM 1` those it gives as not zero, a process colour the four process
/// channels; and every channel it leaves keeps its backdrop. Each pixel's
/// coverage is read off the same paint in black on white, so an
/// anti-aliased glyph's edge is held as its middle is. The text is in an
/// embedded face, so both builds draw these pixels.
#[test]
fn each_overprint_case_paints_its_closed_form() {
    let backdrop = backdrop_operators();
    for (paint_name, strokes, paint) in PAINTS {
        let black = if strokes { "0 G" } else { "0 g" };
        let coverage = coverage_of(&format!("{black} {paint}"));
        for colour in colours() {
            let slot = if strokes {
                &colour.stroke
            } else {
                &colour.fill
            };
            for (state, stroke_flag, fill_flag, nonzero) in STATES {
                let on = if strokes { stroke_flag } else { fill_flag };
                let marks = if on {
                    (colour.marks)(nonzero)
                } else {
                    [true; 6]
                };
                let content = format!("{backdrop}/{state} gs {slot} {paint}");
                let document = case_page(&content, false);
                let page = document.page(0).expect("a page");
                for format in [PixelFormat::Rgb8, PixelFormat::CmykA8] {
                    holds_the_case(
                        &page,
                        (colour.tints, colour.paints, marks),
                        &coverage,
                        format,
                        &format!(
                            "{} by {paint_name} under {state} as {format:?}",
                            colour.name
                        ),
                    );
                }
            }
        }
    }
}

/// **An uncoloured pattern overprints as its colour does** (RC-03.5): a
/// `/PaintType 2` cell filled in a spot, in DeviceCMYK under `/OPM 1` and in
/// DeviceRGB marks the channels that colour marks, its cell tiling the
/// filled rectangle whole; with overprint off it marks every channel; each
/// pixel the composite worked out from the table.
#[test]
fn an_uncoloured_pattern_overprints_as_its_colour_does() {
    let backdrop = backdrop_operators();
    let fill = "4 4 16 16 re f";
    let coverage = coverage_of(&format!("/PY cs 0 /P0 scn {fill}"));
    let rgb = ink_of_light([0.2, 0.4, 0.9].map(byte));
    let cases: [(&str, &str, &str, [u8; 6], [bool; 6]); 4] = [
        (
            "a spot",
            "G2",
            "/PG cs 0.7 /P0 scn",
            [0, 0, 0, 0, byte(0.7), 0],
            [false, false, false, false, true, false],
        ),
        (
            "DeviceCMYK under /OPM 1",
            "G2",
            "/PK cs 0.6 0 0.3 0 /P0 scn",
            [byte(0.6), 0, byte(0.3), 0, 0, 0],
            [true, false, true, false, false, false],
        ),
        (
            "DeviceRGB",
            "G1",
            "/PR cs 0.2 0.4 0.9 /P0 scn",
            [rgb[0], rgb[1], rgb[2], rgb[3], 0, 0],
            [true, true, true, true, false, false],
        ),
        (
            "a spot with overprint off",
            "G0",
            "/PG cs 0.7 /P0 scn",
            [0, 0, 0, 0, byte(0.7), 0],
            [true; 6],
        ),
    ];
    for (name, state, colour, tints, marks) in cases {
        let document = case_page(&format!("{backdrop}/{state} gs {colour} {fill}"), false);
        let page = document.page(0).expect("a page");
        for format in [PixelFormat::Rgb8, PixelFormat::CmykA8] {
            holds_the_case(
                &page,
                (tints, true, marks),
                &coverage,
                format,
                &format!("an uncoloured pattern in {name} as {format:?}"),
            );
        }
    }
}

/// **The twins the clause makes equal render alike** (RC-03.5): a DeviceCMYK
/// colour of cyan alone under `/OPM 1` and a `/Separation` of cyan at the
/// same tint mark the one channel and render byte for byte alike; `/op`
/// absent and `/op` written equal to `/OP` render alike for every colour and
/// every paint that fills; a `k` colour under `/DefaultCMYK` renders as the
/// same colour without the default, the plates keeping its own components
/// (H-135); a `/Separation /None` paint leaves the page as the backdrop
/// alone draws it; and `0.001 k`'s cyan, marked under `/OPM 1`, erases the
/// backdrop's cyan where `0 k`'s leaves it.
#[test]
fn the_twins_the_clause_makes_equal_render_alike() {
    let backdrop = backdrop_operators();
    let render = |content: &str, default_cmyk: bool, format: PixelFormat| {
        case_page(&format!("{backdrop}{content}"), default_cmyk)
            .page(0)
            .expect("a page")
            .render(&simulated(format))
            .data
    };
    for format in [PixelFormat::Rgb8, PixelFormat::CmykA8] {
        for (_, strokes, paint) in PAINTS {
            let (k, separation) = if strokes {
                ("0.6 0 0 0 K", "/SC CS 0.6 SCN")
            } else {
                ("0.6 0 0 0 k", "/SC cs 0.6 scn")
            };
            assert_eq!(
                render(&format!("/G2 gs {k} {paint}"), false, format),
                render(&format!("/G2 gs {separation} {paint}"), false, format),
                "{paint}: cyan alone under /OPM 1 and a /Separation of cyan"
            );
            if !strokes {
                for colour in colours() {
                    assert_eq!(
                        render(&format!("/G3 gs {} {paint}", colour.fill), false, format),
                        render(&format!("/G1 gs {} {paint}", colour.fill), false, format),
                        "{}, {paint}: /op absent and /op equal to /OP",
                        colour.name
                    );
                }
            }
            for state in ["G1", "G2"] {
                let k = if strokes {
                    "0.6 0 0.3 0 K"
                } else {
                    "0.6 0 0.3 0 k"
                };
                assert_eq!(
                    render(&format!("/{state} gs {k} {paint}"), true, format),
                    render(&format!("/{state} gs {k} {paint}"), false, format),
                    "{paint} under {state}: a k colour under /DefaultCMYK"
                );
            }
            let none = if strokes {
                "/SN CS 1 SCN"
            } else {
                "/SN cs 1 scn"
            };
            assert_eq!(
                render(&format!("/G1 gs {none} {paint}"), false, format),
                render("", false, format),
                "{paint}: /None leaves every channel"
            );
        }
        assert_ne!(
            render("/G2 gs 0.001 0 0.3 0 k 4 4 16 16 re f", false, format),
            render("/G2 gs 0 0 0.3 0 k 4 4 16 16 re f", false, format),
            "0.001 k's cyan is marked and 0 k's is not"
        );
    }
}

/// **A separations render knocks out whatever the flags say**: the plates
/// of a page painting under `/OP` and `/OPM 1` are those of the same page
/// with no overprint at all, every paint erasing what it does not name, as
/// the design keeps separations (`overprint-separations.md`, the
/// separations' non-goals); and with the switch off a render draws the
/// overprinting page as the page without the state.
#[test]
fn separations_and_a_render_without_the_switch_knock_out() {
    let backdrop = backdrop_operators();
    for (_, strokes, paint) in PAINTS {
        let slot = if strokes {
            "/SG CS 0.7 SCN"
        } else {
            "/SG cs 0.7 scn"
        };
        let overprinting = case_page(&format!("{backdrop}/G2 gs {slot} {paint}"), false);
        let plain = case_page(&format!("{backdrop}/G0 gs {slot} {paint}"), false);
        let (overprinting, plain) = (
            overprinting.page(0).expect("a page"),
            plain.page(0).expect("a page"),
        );
        let render = RenderOptions::default();
        let tints = |page: &Page| -> Vec<Vec<u8>> {
            plates(page, &render)
                .plates
                .iter()
                .map(|plate| plate.tints.clone())
                .collect()
        };
        assert_eq!(tints(&overprinting), tints(&plain), "{paint}: separations");
        assert_eq!(
            overprinting.render(&render).data,
            plain.render(&render).data,
            "{paint}: the switch off"
        );
    }
}

// ---- sampled sources overprint (RC-03.6) -------------------------------------

/// A space a sampled case is drawn in: as an image XObject's and a shading's
/// `/ColorSpace`, as an inline image's `/CS` names it, its components, an
/// image's four samples, a ramp's two ends, a flat fill of the first
/// sample's colour, and the channels it marks — by its kind alone, since
/// 8.6.7 keeps the non-zero mode off images and shadings.
struct Sampled {
    name: &'static str,
    space: String,
    inline: &'static str,
    samples: Vec<u8>,
    ends: (&'static str, &'static str),
    flat: &'static str,
    marks: [bool; 6],
}

/// The spaces the sampled cases draw in: DeviceCMYK, a spot's
/// `/Separation`, a `/DeviceN` of both spots, and DeviceRGB.
fn sampled_spaces() -> Vec<Sampled> {
    let process = [true, true, true, true, false, false];
    vec![
        Sampled {
            name: "DeviceCMYK",
            space: "/DeviceCMYK".to_string(),
            inline: "/DeviceCMYK",
            samples: vec![51, 0, 153, 0, 0, 102, 0, 204, 204, 51, 0, 0, 0, 0, 0, 51],
            ends: ("0.2 0 0.6 0", "0 0.8 0.2 0.4"),
            flat: "0.2 0 0.6 0 k",
            marks: process,
        },
        Sampled {
            name: "a /Separation of a spot",
            space: "[/Separation /Gold /DeviceCMYK << /FunctionType 2 /Domain [0 1] \
                    /C0 [0 0 0 0] /C1 [0 0 1 0] /N 1 >>]"
                .to_string(),
            inline: "/SG",
            samples: vec![51, 102, 153, 204],
            ends: ("0.2", "0.8"),
            flat: "/SG cs 0.2 scn",
            marks: [false, false, false, false, true, false],
        },
        Sampled {
            name: "a /DeviceN of both spots",
            space: "[/DeviceN [/Gold /Teal] /DeviceCMYK 11 0 R]".to_string(),
            inline: "/DN",
            samples: vec![51, 204, 102, 153, 153, 102, 204, 51],
            ends: ("0.2 0.8", "0.8 0.2"),
            flat: "/DN cs 0.2 0.8 scn",
            marks: [false, false, false, false, true, true],
        },
        Sampled {
            name: "DeviceRGB",
            space: "/DeviceRGB".to_string(),
            inline: "/DeviceRGB",
            samples: vec![51, 102, 153, 204, 51, 0, 0, 0, 0, 255, 255, 255],
            ends: ("0.2 0.4 0.6", "0.8 0.2 0"),
            flat: "0.2 0.4 0.6 rg",
            marks: process,
        },
    ]
}

/// What a sampled case's page holds for `space`: a two-by-two image and a
/// one-sample one of its first colour (objects 13 and 14), an axial shading
/// and a type 4 mesh (object 15) over the square the paints cover, and a
/// shading pattern of the axial.
fn sampled_extra(space: &Sampled) -> Extra {
    let n = space.samples.len() / 4;
    let image = |width: u32, samples: &[u8]| {
        raw_stream(
            &format!(
                "/Type /XObject /Subtype /Image /Width {width} /Height {width} \
                 /ColorSpace {} /BitsPerComponent 8",
                space.space
            ),
            samples,
        )
    };
    let axial = format!(
        "<< /ShadingType 2 /ColorSpace {} /Coords [4 0 20 0] /Function << /FunctionType 2 \
         /Domain [0 1] /C0 [{}] /C1 [{}] /N 1 >> /Extend [true true] >>",
        space.space, space.ends.0, space.ends.1
    );
    // Three vertices, a flag, x, y and the components a byte each: the
    // first sample's colour, the last's, and the second's.
    let vertex = |x: u8, y: u8, sample: usize| {
        let mut bytes = vec![0, x, y];
        bytes.extend_from_slice(&space.samples[sample * n..(sample + 1) * n]);
        bytes
    };
    let mut mesh = vertex(4, 4, 0);
    mesh.extend(vertex(20, 4, 3));
    mesh.extend(vertex(12, 20, 1));
    let decode = vec!["0 1"; n].join(" ");
    Extra {
        xobjects: "/Im 13 0 R /I1 14 0 R".to_string(),
        shadings: format!("/Ax {axial} /Me 15 0 R"),
        patterns: format!("/SP << /PatternType 2 /Shading {axial} >>"),
        objects: vec![
            image(2, &space.samples),
            image(1, &space.samples[..n]),
            raw_stream(
                &format!(
                    "/ShadingType 4 /ColorSpace {} /BitsPerCoordinate 8 /BitsPerComponent 8 \
                     /BitsPerFlag 8 /Decode [0 255 0 255 {decode}]",
                    space.space
                ),
                &mesh,
            ),
        ],
        ..Extra::default()
    }
}

/// The paints a sampled case draws over the square from (4, 4) to (20, 20):
/// an image XObject, an inline image of the same samples, `sh` clipped to
/// the square, a shading pattern filling it, and a mesh by `sh`.
fn sampled_paints(space: &Sampled) -> Vec<(&'static str, String)> {
    let hex: String = space.samples.iter().map(|b| format!("{b:02X}")).collect();
    vec![
        ("an image", "q 16 0 0 16 4 4 cm /Im Do Q".to_string()),
        (
            "an inline image",
            format!(
                "q 16 0 0 16 4 4 cm BI /W 2 /H 2 /CS {} /BPC 8 /F /AHx ID {hex}> EI Q",
                space.inline
            ),
        ),
        ("sh", "q 4 4 16 16 re W n /Ax sh Q".to_string()),
        (
            "a shading pattern",
            "/Pattern cs /SP scn 4 4 16 16 re f".to_string(),
        ),
        ("a mesh", "/Me sh".to_string()),
    ]
}

/// **Images and shadings overprint by their space** (RC-03.6): an image
/// XObject, an inline image, an axial shading by `sh` and as a pattern, and
/// a type 4 mesh, each in DeviceCMYK, a spot's `/Separation`, a `/DeviceN`
/// of both spots and DeviceRGB, over a backdrop with ink in every channel,
/// under no overprint, `/OPM 0`, `/OPM 1` and `/OP` without `/op`, render as
/// `Rgb8` and `CmykA8` the composite of the plates the test works out: where
/// `/op` is set each channel the space names — DeviceCMYK's four process
/// channels under both modes, 8.6.7 keeping the non-zero mode off images and
/// shadings, and DeviceRGB's as light turned into ink — takes the paint's
/// own tint, read off the page's separations, which knock out, and every
/// other channel keeps its backdrop; where it is clear every channel takes
/// the paint's.
#[test]
fn images_and_shadings_overprint_by_their_space() {
    let backdrop = backdrop_operators();
    let backdrop_tints = BACKDROP_TINTS.map(byte);
    for space in sampled_spaces() {
        let extra = sampled_extra(&space);
        for (paint_name, paint) in sampled_paints(&space) {
            for (state, _, fill_flag, nonzero) in STATES {
                let page = case_page_with(&format!("{backdrop}/{state} gs {paint}"), false, &extra);
                let page = page.page(0).expect("a page");
                let knocked = plates(&page, &RenderOptions::default());
                let marks = if fill_flag { space.marks } else { [true; 6] };
                for format in [PixelFormat::Rgb8, PixelFormat::CmykA8] {
                    let what = format!(
                        "{paint_name} in {} under {state} (non-zero {nonzero}) as {format:?}",
                        space.name
                    );
                    let bitmap = page.render(&simulated(format));
                    let n = format.components();
                    for at in 0..(CASE_PAGE * CASE_PAGE) as usize {
                        let mut channels = [0u8; 6];
                        for (channel, slot) in channels.iter_mut().enumerate() {
                            *slot = if marks[channel] {
                                knocked.plates[channel].tints[at]
                            } else {
                                backdrop_tints[channel]
                            };
                        }
                        assert_eq!(
                            &bitmap.data[at * n..(at + 1) * n],
                            case_composite(channels, format).as_slice(),
                            "{what}: pixel ({}, {})",
                            at % CASE_PAGE as usize,
                            at / CASE_PAGE as usize
                        );
                    }
                }
            }
        }
    }
}

/// **A one-sample image paints as a flat fill of its colour** (RC-03.6): in
/// each space, under `/OPM 0`, an image of one sample over the square and a
/// fill of its colour over the same square render byte for byte alike; under
/// `/OPM 1` a DeviceCMYK image still marks all four process channels, so it
/// erases the backdrop's magenta and black where the fill, whose magenta and
/// black are zero, keeps them.
#[test]
fn a_one_sample_image_paints_as_a_flat_fill_of_its_colour() {
    let backdrop = backdrop_operators();
    for space in sampled_spaces() {
        let extra = sampled_extra(&space);
        let render = |state: &str, paint: &str, format: PixelFormat| {
            case_page_with(&format!("{backdrop}/{state} gs {paint}"), false, &extra)
                .page(0)
                .expect("a page")
                .render(&simulated(format))
                .data
        };
        for format in [PixelFormat::Rgb8, PixelFormat::CmykA8] {
            let image = "q 16 0 0 16 4 4 cm /I1 Do Q";
            let fill = format!("{} 4 4 16 16 re f", space.flat);
            for state in ["G0", "G1"] {
                assert_eq!(
                    render(state, image, format),
                    render(state, &fill, format),
                    "{} under {state} as {format:?}",
                    space.name
                );
            }
            if space.name == "DeviceCMYK" {
                assert_ne!(
                    render("G2", image, format),
                    render("G2", &fill, format),
                    "/OPM 1 reaches the fill and not the image"
                );
            }
        }
    }
}

// ---- overprint under transparency (RC-03.7) ----------------------------------

/// The Gold a transparency case fills with, at a tint of 0.7, and the
/// channel it marks.
fn gold() -> ([u8; 6], [bool; 6]) {
    (
        [0, 0, 0, 0, byte(0.7), 0],
        [false, false, false, false, true, false],
    )
}

/// The Teal a transparency case strokes or fills with, at a tint of 0.6,
/// and the channel it marks.
fn teal() -> ([u8; 6], [bool; 6]) {
    (
        [0, 0, 0, 0, 0, byte(0.6)],
        [false, false, false, false, false, true],
    )
}

/// What the isolated and the non-isolated groups paint: a cyan erasing every
/// channel over `R1` (4, 4 to 16, 16), then Gold overprinting over `R2` (8, 8
/// to 20, 20), which covers a quarter of it.
const GROUPED: &str = "/G0 gs 0.6 0 0 0 k 4 4 12 12 re f /G1 gs /SG cs 0.7 scn 8 8 12 12 re f";

/// [`GROUPED`], and Teal overprinting under `Multiply` over 6, 6 to 12, 12,
/// through the paint's implicit group (11.7.4.3).
const GROUPED_MULTIPLIED: &str = "/G0 gs 0.6 0 0 0 k 4 4 12 12 re f /G1 gs /SG cs 0.7 scn \
                                  8 8 12 12 re f /BM1 gs /ST cs 0.6 scn 6 6 6 6 re f";

/// What the knockout groups paint: Gold over `R1`, then Teal over `R2`, both
/// overprinting.
const KNOCKED: &str = "/G1 gs /SG cs 0.7 scn 4 4 12 12 re f /ST cs 0.6 scn 8 8 12 12 re f";

/// The resources a transparency case's forms paint with: the page's Gold
/// and Teal separations, no overprint and both flags under `/OPM 0`, and
/// `Multiply`.
fn form_resources() -> String {
    let ramp =
        |c1: &str| format!("<< /FunctionType 2 /Domain [0 1] /C0 [0 0 0 0] /C1 [{c1}] /N 1 >>");
    format!(
        "/Resources << /ColorSpace << /SG [/Separation /Gold /DeviceCMYK {}] \
         /ST [/Separation /Teal /DeviceCMYK {}] >> /ExtGState << \
         /G0 << /OP false /op false /OPM 0 >> /G1 << /OP true /op true /OPM 0 >> \
         /BM1 << /BM /Multiply >> >> >>",
        ramp("0 0 1 0"),
        ramp("1 0 0 0")
    )
}

/// What the transparency cases' pages hold beyond a case page's: `/H5`, both
/// alphas 0.5; `/HU`, a fill alpha of 1 and a stroke alpha of 0.5; `/BM1`
/// `Multiply` and `/BD` `Darken`; `/SM`, a soft mask reading the alpha of a
/// group (object 13) that fills the page's lower half at alpha 0.5; and
/// forms painting as groups — [`GROUPED`] isolated (`/GI`, 14) and not
/// (`/GN`, 15), [`KNOCKED`] in a non-isolated knockout group (`/GK`, 16)
/// and an isolated one (`/GJ`, 17), and [`GROUPED_MULTIPLIED`] not isolated
/// (`/GT`, 18).
fn transparency_extra() -> Extra {
    let form = |group: &str, content: &str| {
        raw_stream(
            &format!(
                "/Type /XObject /Subtype /Form /BBox [0 0 {CASE_PAGE} {CASE_PAGE}] \
                 /Group << /S /Transparency {group} >> {}",
                form_resources()
            ),
            content.as_bytes(),
        )
    };
    Extra {
        ext_g_states: "/H5 << /ca 0.5 /CA 0.5 >> /HU << /ca 1 /CA 0.5 >> \
                       /BM1 << /BM /Multiply >> /BD << /BM /Darken >> \
                       /SM << /SMask << /Type /Mask /S /Alpha /G 13 0 R >> >>"
            .to_string(),
        xobjects: "/GI 14 0 R /GN 15 0 R /GK 16 0 R /GJ 17 0 R /GT 18 0 R".to_string(),
        objects: vec![
            raw_stream(
                &format!(
                    "/Type /XObject /Subtype /Form /BBox [0 0 {CASE_PAGE} {CASE_PAGE}] \
                     /Group << /S /Transparency >> \
                     /Resources << /ExtGState << /H << /ca 0.5 >> >> >>"
                ),
                format!("/H gs 0 g 0 0 {CASE_PAGE} 12 re f").as_bytes(),
            ),
            form("/I true", GROUPED),
            form("", GROUPED),
            form("/K true", KNOCKED),
            form("/I true /K true", KNOCKED),
            form("", GROUPED_MULTIPLIED),
        ],
        ..Extra::default()
    }
}

/// Whether pixel `at` of a case page lies in the rectangle from user-space
/// `(x, y)`, `w` by `h`: one pixel a point, the rows counted down from the
/// top.
fn inside(at: usize, (x, y, w, h): (u32, u32, u32, u32)) -> bool {
    let column = (at % CASE_PAGE as usize) as u32;
    let up = CASE_PAGE - 1 - (at / CASE_PAGE as usize) as u32;
    (x..x + w).contains(&column) && (y..y + h).contains(&up)
}

/// `R1` and `R2`, where [`GROUPED`] and [`KNOCKED`] paint.
const R1: (u32, u32, u32, u32) = (4, 4, 12, 12);
const R2: (u32, u32, u32, u32) = (8, 8, 12, 12);

/// The plates [`GROUPED`] leaves at pixel `at`, drawn outside a group or in
/// a non-isolated one — Gold overprinting the cyan where it covers it and
/// the backdrop elsewhere — or in an isolated one, whose Gold over nothing
/// the group painted takes every channel's source, erasing the backdrop's
/// inks, since its first paints see no backdrop (11.4.4).
fn grouped_plates(at: usize, isolated: bool) -> [u8; 6] {
    let backdrop = BACKDROP_TINTS.map(byte);
    let cyan = [byte(0.6), 0, 0, 0, 0, 0];
    let (gold, _) = gold();
    match (inside(at, R1), inside(at, R2)) {
        (false, false) => backdrop,
        (true, false) => cyan,
        (true, true) => [cyan[0], 0, 0, 0, gold[4], 0],
        (false, true) if isolated => gold,
        (false, true) => {
            let mut plates = backdrop;
            plates[4] = gold[4];
            plates
        }
    }
}

/// The plates [`KNOCKED`] leaves at pixel `at` in a knockout group: Teal
/// over `R2` composited against the group's initial backdrop, so the Gold
/// under it is gone (11.4.5) — the page's own Gold in a non-isolated group,
/// and no ink at all, with every other channel, in an isolated one.
fn knocked_plates(at: usize, isolated: bool) -> [u8; 6] {
    let mut plates = if isolated {
        [0; 6]
    } else {
        BACKDROP_TINTS.map(byte)
    };
    if inside(at, R2) {
        plates[5] = teal().0[5];
    } else if inside(at, R1) {
        plates[4] = gold().0[4];
    } else {
        plates = BACKDROP_TINTS.map(byte);
    }
    plates
}

/// What an overprinting paint of `tints` marking `marks` under `Multiply`
/// leaves where it covers a pixel by `e`, through its implicit
/// non-isolated, non-knockout group (11.7.4.3): the group holds the paint in
/// each marked channel and the backdrop in every other, and composites
/// under `Multiply`, `B(Cb, Cs)` taken on the tints' complements (11.3.5) —
/// so every channel the paint leaves is multiplied by itself and comes out
/// darker, as the design reads the clause.
fn multiplied(tints: [u8; 6], marks: [bool; 6], e: u8) -> [u8; 6] {
    let backdrop = BACKDROP_TINTS.map(byte);
    let mut plates = backdrop;
    if e == 0 {
        return plates;
    }
    for channel in 0..6 {
        let cb = backdrop[channel];
        let source = if marks[channel] { tints[channel] } else { cb };
        let blended = 255 - times(255 - cb, 255 - source);
        plates[channel] = times(blended, e) + times(cb, 255 - e);
    }
    plates
}

/// One pixel of a non-isolated group's buffer over the opaque page, as
/// 11.4 composites it in 255ths, each product rounded: its six tints, over
/// the backdrop, and the group's own alpha (11.4.4).
#[derive(Clone, Copy)]
struct GroupPixel {
    tints: [u8; 6],
    alpha: u8,
}

impl GroupPixel {
    /// The buffer before anything is painted: the backdrop's tints, at no
    /// alpha of the group's own.
    fn start() -> GroupPixel {
        GroupPixel {
            tints: BACKDROP_TINTS.map(byte),
            alpha: 0,
        }
    }

    /// 11.3.6's `Normal` composite of `tints`, marking `marks`, at opacity
    /// `e`: the backdrop's union alpha is 1, so a marked channel is
    /// `cs·e + cb·(1 − e)` and every other keeps its value; the group's own
    /// alpha becomes `e + α·(1 − e)`.
    fn paint(self, tints: [u8; 6], marks: [bool; 6], e: u8) -> GroupPixel {
        if e == 0 {
            return self;
        }
        let mut next = self;
        for channel in 0..6 {
            if marks[channel] {
                next.tints[channel] =
                    times(tints[channel], e) + times(self.tints[channel], 255 - e);
            }
        }
        next.alpha = e + times(self.alpha, 255 - e);
        next
    }

    /// 11.4.5's knockout under coverage `c`: every channel and the alpha
    /// weighted back toward the group's initial state, the backdrop at no
    /// alpha.
    fn knock(self, c: u8) -> GroupPixel {
        if c == 0 {
            return self;
        }
        let backdrop = BACKDROP_TINTS.map(byte);
        let mut next = self;
        for channel in 0..6 {
            next.tints[channel] = times(backdrop[channel], c) + times(self.tints[channel], 255 - c);
        }
        next.alpha = times(self.alpha, 255 - c);
        next
    }

    /// The group's colour once 11.4.7.2 has taken its backdrop out,
    /// `C = Cn + (Cn − C0)·(α0/αg − α0)` with `α0` 1: the factor
    /// `⌊255·255/αg⌋ − 255` in 255ths, each channel's correction truncated
    /// toward zero and the result held to a byte; nothing where the group
    /// painted nothing.
    fn removed(self) -> [u8; 6] {
        if self.alpha == 0 {
            return self.tints;
        }
        let backdrop = BACKDROP_TINTS.map(byte);
        let factor = 255 * 255 / i64::from(self.alpha) - 255;
        let mut tints = self.tints;
        for (channel, slot) in tints.iter_mut().enumerate() {
            let cn = i64::from(*slot);
            let c0 = i64::from(backdrop[channel]);
            *slot = (cn + (cn - c0) * factor / 255).clamp(0, 255) as u8;
        }
        tints
    }

    /// The group composited onto the page's backdrop at the operation's
    /// alpha `alpha` under `Normal`: each channel `C·e + Cb·(1 − e)`, `e` the
    /// group's alpha at the operation's.
    fn composited(self, alpha: u8) -> [u8; 6] {
        let backdrop = BACKDROP_TINTS.map(byte);
        let e = times(self.alpha, alpha);
        if e == 0 {
            return backdrop;
        }
        let colour = self.removed();
        let mut plates = backdrop;
        for channel in 0..6 {
            plates[channel] = times(colour[channel], e) + times(backdrop[channel], 255 - e);
        }
        plates
    }
}

/// What a path's fill in Gold and its stroke in Teal, both overprinting,
/// leave as 11.7.4.4's one object, from each pixel's fill and stroke
/// coverage: with the two alphas equal at 0.5, a non-isolated,
/// non-knockout group in which the fill and then the stroke are painted at
/// alpha 1, the stroke overprinting the fill, composited at 0.5; with them
/// unequal, the fill's 1 and the stroke's 0.5, a non-isolated knockout
/// group in which each is painted at its own, the stroke knocking the fill
/// out under it and so overprinting the backdrop (the clause's third note),
/// composited at 1.
fn compound_plates(fill: &[u8], stroke: &[u8], equal: bool) -> Vec<[u8; 6]> {
    let (gold, gold_marks) = gold();
    let (teal, teal_marks) = teal();
    fill.iter()
        .zip(stroke)
        .map(|(&f, &s)| {
            if equal {
                GroupPixel::start()
                    .paint(gold, gold_marks, f)
                    .paint(teal, teal_marks, s)
                    .composited(byte(0.5))
            } else {
                GroupPixel::start()
                    .knock(f)
                    .paint(gold, gold_marks, f)
                    .knock(s)
                    .paint(teal, teal_marks, times(s, byte(0.5)))
                    .composited(255)
            }
        })
        .collect()
}

/// **Overprint under transparency composites as designed** (RC-03.7): over
/// a backdrop with ink in every channel, rendered as `Rgb8` and as
/// `CmykA8`, each pixel is the composite of the plates the test works out
/// from the design's *Overprint under transparency*, for a Gold
/// `/Separation` and a DeviceCMYK colour under `/OPM 1`:
///
/// - **at alpha 0.5 and under a soft mask**, the paint's alpha enters as its
///   coverage does, read off the same paint in black on white, and every
///   channel it leaves keeps its backdrop;
/// - **under `Multiply`**, through the paint's implicit group, every channel
///   it leaves is `B(Cb, Cb)`;
/// - **inside a non-isolated group** the overprinting Gold leaves the
///   backdrop's channels as it would outside, and **inside an isolated
///   one**, over nothing the group painted, erases them;
/// - **inside a knockout group**, non-isolated and isolated, the Teal over
///   the Gold composites against the group's initial backdrop, so the Gold
///   under it is the page's, or gone;
/// - **`B` and a mode 2 glyph** with both flags set, at equal and at unequal
///   alphas, paint their fill and stroke as 11.7.4.4's one object, each
///   coverage read off the same path or glyph filled, clipped by or stroked
///   in black on white.
///
/// The glyph is in an embedded face, so both builds draw these pixels.
#[test]
fn overprint_under_transparency_composites_as_designed() {
    let extra = transparency_extra();
    let backdrop = backdrop_operators();
    let square = "4 4 16 16 re f";
    let whole = coverage_of_with(&format!("0 g {square}"), &extra);
    let colours = [
        ("Gold", "G1", "/SG cs 0.7 scn", gold().0, gold().1),
        (
            "DeviceCMYK under /OPM 1",
            "G2",
            "0.6 0 0.3 0 k",
            [byte(0.6), 0, byte(0.3), 0, 0, 0],
            cmyk_marks(true),
        ),
    ];
    let mut cases: Vec<(String, String, Vec<[u8; 6]>)> = Vec::new();
    for (state, how) in [("H5", "at alpha 0.5"), ("SM", "under a soft mask")] {
        let opacity = coverage_of_with(&format!("/{state} gs 0 g {square}"), &extra);
        assert!(
            opacity.iter().any(|&e| e > 0 && e < 255),
            "{how}: the paint is partly opaque somewhere"
        );
        for (name, flags, colour, tints, marks) in colours {
            cases.push((
                format!("{name} {how}"),
                format!("/{flags} gs /{state} gs {colour} {square}"),
                opacity
                    .iter()
                    .map(|&e| case_plates(tints, true, marks, e))
                    .collect(),
            ));
        }
    }
    for (name, flags, colour, tints, marks) in colours {
        cases.push((
            format!("{name} under Multiply"),
            format!("/{flags} gs /BM1 gs {colour} {square}"),
            whole.iter().map(|&e| multiplied(tints, marks, e)).collect(),
        ));
    }
    let pixels = (CASE_PAGE * CASE_PAGE) as usize;
    for (name, form, plates) in [
        (
            "a non-isolated group",
            "/GN Do",
            (0..pixels)
                .map(|at| grouped_plates(at, false))
                .collect::<Vec<_>>(),
        ),
        (
            "an isolated group",
            "/GI Do",
            (0..pixels).map(|at| grouped_plates(at, true)).collect(),
        ),
        (
            "a non-isolated knockout group",
            "/GK Do",
            (0..pixels).map(|at| knocked_plates(at, false)).collect(),
        ),
        (
            "an isolated knockout group",
            "/GJ Do",
            (0..pixels).map(|at| knocked_plates(at, true)).collect(),
        ),
    ] {
        cases.push((name.to_string(), form.to_string(), plates));
    }
    let path_fill = coverage_of_with("0 g 4 4 16 16 re f", &extra);
    let path_stroke = coverage_of_with("0 G 2 w 4 4 16 16 re S", &extra);
    let glyph_fill = coverage_of_with(
        &format!("BT /F0 18 Tf 7 Tr 3 5 Td (B) Tj ET 0 g 0 0 {CASE_PAGE} {CASE_PAGE} re f"),
        &extra,
    );
    let glyph_stroke = coverage_of_with("0 G BT /F0 18 Tf 1 Tr 2 w 3 5 Td (B) Tj ET", &extra);
    let both = "/G1 gs /SG cs 0.7 scn /ST CS 0.6 SCN 2 w";
    for (alphas, equal) in [("H5", true), ("HU", false)] {
        cases.push((
            format!("B under {alphas}"),
            format!("{both} /{alphas} gs 4 4 16 16 re B"),
            compound_plates(&path_fill, &path_stroke, equal),
        ));
        cases.push((
            format!("a mode 2 glyph under {alphas}"),
            format!("{both} /{alphas} gs BT /F0 18 Tf 2 Tr 3 5 Td (B) Tj ET"),
            compound_plates(&glyph_fill, &glyph_stroke, equal),
        ));
    }
    for (name, content, plates) in cases {
        let document = case_page_with(&format!("{backdrop}{content}"), false, &extra);
        let page = document.page(0).expect("a page");
        for format in [PixelFormat::Rgb8, PixelFormat::CmykA8] {
            let bitmap = page.render(&simulated(format));
            assert_eq!(bitmap.format, format, "{name}");
            let n = format.components();
            for (at, &pixel) in plates.iter().enumerate() {
                assert_eq!(
                    &bitmap.data[at * n..(at + 1) * n],
                    case_composite(pixel, format).as_slice(),
                    "{name} as {format:?}: pixel ({}, {})",
                    at % CASE_PAGE as usize,
                    at / CASE_PAGE as usize
                );
            }
        }
    }
}

/// Holds every tile of `page` at `options`, 7 and 11 pixels a side, to the
/// page under it, and the page's display list replayed to the page.
#[track_caller]
fn holds_its_tiles_and_its_replay(page: &Page, options: &RenderOptions, what: &str) {
    let whole = page.render(options);
    assert_eq!(
        page.display_list().render(options).data,
        whole.data,
        "{what}: replayed from its display list"
    );
    let n = whole.format.components();
    for tile in [7u32, 11] {
        for top in (0..whole.height).step_by(tile as usize) {
            for left in (0..whole.width).step_by(tile as usize) {
                let asked = PixelRegion::new(left, top, tile, tile);
                let part = page.render(&RenderOptions {
                    region: Some(asked),
                    ..options.clone()
                });
                let held = asked.clamped_to(whole.width, whole.height);
                let span = held.width as usize * n;
                for row in 0..held.height as usize {
                    let from = ((top as usize + row) * whole.width as usize + left as usize) * n;
                    assert_eq!(
                        &part.data[row * span..(row + 1) * span],
                        &whole.data[from..from + span],
                        "{what}: a {tile}px tile at ({left}, {top}), row {row}"
                    );
                }
            }
        }
    }
}

/// **The twins the clause makes equal under transparency render alike**
/// (RC-03.7): a non-isolated group's contents — a cyan erasing every
/// channel, Gold overprinting it and the backdrop, and Teal overprinting
/// both under `Multiply` through its implicit group — render byte for byte
/// as the same contents drawn outside it; an overprinting `Darken` fill,
/// glyph and `/OPM 1` fill render as the same paints under `Normal`, every
/// channel each leaves being opaque and each it marks taking more ink than
/// the backdrop holds there, so `B(Cb, Cb)` is `Cb` and `B(Cb, Cs)` is
/// `Cs`; and each transparency case's page drawn in tiles is the page under
/// them, and replayed from its display list is the page.
#[test]
fn the_twins_the_clause_makes_equal_under_transparency_render_alike() {
    let extra = transparency_extra();
    let backdrop = backdrop_operators();
    let page_of = |content: &str| case_page_with(&format!("{backdrop}{content}"), false, &extra);
    let darkened = [
        "/G1 gs /SG cs 0.9 scn 4 4 16 16 re f",
        "/G1 gs /SG cs 0.9 scn BT /F0 18 Tf 3 5 Td (B) Tj ET",
        "/G2 gs 0.6 0 0.9 0 k 4 4 16 16 re f",
    ];
    for format in [PixelFormat::Rgb8, PixelFormat::CmykA8] {
        let options = simulated(format);
        let render = |content: &str| {
            page_of(content)
                .page(0)
                .expect("a page")
                .render(&options)
                .data
        };
        assert_eq!(
            render("/GT Do"),
            render(GROUPED_MULTIPLIED),
            "{format:?}: a non-isolated group's contents and the same outside it"
        );
        for paint in darkened {
            assert_eq!(
                render(&format!("/BD gs {paint}")),
                render(paint),
                "{format:?}: {paint} under Darken and under Normal"
            );
        }
        for content in [
            "/G1 gs /BM1 gs /SG cs 0.7 scn 4 4 16 16 re f",
            "/GN Do /GK Do",
            "/G1 gs /SG cs 0.7 scn /ST CS 0.6 SCN 2 w /H5 gs 4 4 16 16 re B",
            "/G1 gs /SG cs 0.7 scn /ST CS 0.6 SCN 2 w /HU gs 4 4 16 16 re B",
            "/G1 gs /SG cs 0.7 scn /ST CS 0.6 SCN 2 w /HU gs BT /F0 18 Tf 2 Tr 3 5 Td (B) Tj ET",
        ] {
            let document = page_of(content);
            let page = document.page(0).expect("a page");
            holds_its_tiles_and_its_replay(&page, &options, &format!("{content} as {format:?}"));
        }
    }
}
