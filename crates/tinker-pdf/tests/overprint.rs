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
             /ColorSpace << {spaces} >> /ExtGState << {states} >> /Font << /F0 5 0 R >> \
             /XObject << /M0 8 0 R >> /Pattern << /P0 9 0 R >> >> /Contents 4 0 R >>"
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
    let document = case_page(content, false);
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
