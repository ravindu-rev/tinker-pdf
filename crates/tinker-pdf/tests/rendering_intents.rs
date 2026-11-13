//! Rendering intents (8.6.5.8) reach the colours a page paints
//! (`docs/design/icc.md`, "Rendering intents").
//!
//! A flat colour in an `ICCBased` space — a fill, a stroke, a glyph — converts
//! through the profile's transform for the intent in force at its painting
//! operator, `ri`'s or a graphics state's `/RI`. Every expected colour here
//! is recomputed from ICC.1's own arithmetic: a table's multilinear
//! interpolation and v2's legacy Lab encoding, a matrix/TRC profile's
//! columns over its curves, and ICC-absolute colorimetry's scaling by the
//! media white (6.2.3). The connection space reaches sRGB through
//! `tinker_pdf_color::lab_to_srgb`, the engine's public conversion of it, so
//! what is under test is which transform the intent selects and what it
//! makes of the components.
//!
//! The pages are `render_support`'s, which `determinism.rs` fingerprints.
//! Nothing here depends on a font this build may or may not bundle: the glyph
//! is the repository's own embedded face.

mod render_support;

use render_support::{
    intent_lut_encoded, intent_page, intent_spot, intent_states_page, pixel, render, INTENT_PAINTS,
    INTENT_PAPER, INTENT_SIZE, SRGB_COLUMNS,
};
use tinker_pdf::Bitmap;
use tinker_pdf_color::lab_to_srgb;

/// The four intents Table 69 names, by the name `ri` gives them.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Intent {
    Perceptual,
    Relative,
    Saturation,
    Absolute,
}

impl Intent {
    const ALL: [Intent; 4] = [
        Intent::Perceptual,
        Intent::Relative,
        Intent::Saturation,
        Intent::Absolute,
    ];

    fn name(self) -> &'static str {
        match self {
            Intent::Perceptual => "Perceptual",
            Intent::Relative => "RelativeColorimetric",
            Intent::Saturation => "Saturation",
            Intent::Absolute => "AbsoluteColorimetric",
        }
    }

    /// The table ICC.1 8.10.2 has the intent convert through: `A2B0`
    /// perceptual, `A2B1` both colorimetric intents, `A2B2` saturation.
    fn table(self) -> usize {
        match self {
            Intent::Perceptual => 0,
            Intent::Relative | Intent::Absolute => 1,
            Intent::Saturation => 2,
        }
    }
}

/// What an `XYZType` tag holding `value` reads back as.
fn stored(value: f64) -> f64 {
    (value * 65536.0).round() / 65536.0
}

/// ICC-absolute colorimetry's scale (ICC.1 6.2.3, equations 4 to 6): the
/// media white over the PCS white, 6.3.4.3's D50 as an `XYZType` tag stores
/// it. The destination, sRGB, has the PCS white for its own (9.2.34), so its
/// ratio is one.
fn media_scale() -> [f64; 3] {
    let pcs = [
        f64::from(0x0000_F6D6u32) / 65536.0,
        1.0,
        f64::from(0x0000_D32Du32) / 65536.0,
    ];
    [0, 1, 2].map(|channel| stored(INTENT_PAPER[channel]) / pcs[channel])
}

/// The Lab reference white `lab_to_srgb` reads `L*a*b*` against: D50.
const D50: [f64; 3] = [0.964_212, 1.0, 0.825_188];

/// CIE L*a*b* as D50 XYZ, 8.6.5.4's relation run backwards.
fn xyz_of_lab([l, a, b]: [f64; 3]) -> [f64; 3] {
    const DELTA: f64 = 6.0 / 29.0;
    let finv = |t: f64| {
        if t > DELTA {
            t * t * t
        } else {
            3.0 * DELTA * DELTA * (t - 4.0 / 29.0)
        }
    };
    let fy = (l + 16.0) / 116.0;
    [
        D50[0] * finv(fy + a / 500.0),
        D50[1] * finv(fy),
        D50[2] * finv(fy - b / 200.0),
    ]
}

/// The real cube root of a positive `t`, by Newton's method: arithmetic
/// alone, so the expectation is the same number on every target.
fn cube_root(t: f64) -> f64 {
    let mut y = if t > 1.0 { t } else { 1.0 };
    for _ in 0..200 {
        let next = (2.0 * y + t / (y * y)) / 3.0;
        if next == y {
            break;
        }
        y = next;
    }
    y
}

/// D50 XYZ as CIE L*a*b*, 8.6.5.4's relation run forwards.
fn lab_of_xyz(xyz: [f64; 3]) -> [f64; 3] {
    const DELTA: f64 = 6.0 / 29.0;
    let f = |t: f64| {
        if t > DELTA * DELTA * DELTA {
            cube_root(t)
        } else {
            t / (3.0 * DELTA * DELTA) + 4.0 / 29.0
        }
    };
    let (fx, fy, fz) = (f(xyz[0] / D50[0]), f(xyz[1] / D50[1]), f(xyz[2] / D50[2]));
    [116.0 * fy - 16.0, 500.0 * (fx - fy), 200.0 * (fy - fz)]
}

/// D50 XYZ as sRGB, through its Lab.
fn srgb_of_xyz(xyz: [f64; 3]) -> (u8, u8, u8) {
    let [l, a, b] = lab_of_xyz(xyz);
    lab_to_srgb(l, a, b)
}

/// `xyz` scaled channel by channel by absolute colorimetry's media white.
fn absolute(xyz: [f64; 3]) -> [f64; 3] {
    let scale = media_scale();
    [xyz[0] * scale[0], xyz[1] * scale[1], xyz[2] * scale[2]]
}

/// What the table profile paints `components` as in `intent`: its table's
/// two-point grid interpolated multilinearly at them, the first input's axis
/// slowest, decoded from v2's legacy 16-bit Lab (full scale `0xFF00`), and,
/// for absolute colorimetric, scaled in XYZ by the media white.
fn table_colour(intent: Intent, components: [f64; 3]) -> (u8, u8, u8) {
    let mut encoded = [0.0f64; 3];
    for node in 0..8usize {
        let bits = [(node >> 2) & 1, (node >> 1) & 1, node & 1];
        let weight: f64 = (0..3)
            .map(|axis| {
                if bits[axis] == 1 {
                    components[axis]
                } else {
                    1.0 - components[axis]
                }
            })
            .product();
        for (slot, value) in encoded
            .iter_mut()
            .zip(intent_lut_encoded(intent.table(), node))
        {
            *slot += weight * f64::from(value);
        }
    }
    let lab = [
        encoded[0] / 65280.0 * 100.0,
        encoded[1] / 65280.0 * 255.0 - 128.0,
        encoded[2] / 65280.0 * 255.0 - 128.0,
    ];
    if intent == Intent::Absolute {
        srgb_of_xyz(absolute(xyz_of_lab(lab)))
    } else {
        lab_to_srgb(lab[0], lab[1], lab[2])
    }
}

/// What the matrix/TRC profile paints `components` as in `intent`: its
/// columns, as stored, over the components through its gamma 2 curves, and,
/// for absolute colorimetric, scaled by the media white. Table 25 calls the
/// model colorimetric and 8.10.2 d) has every intent reach it.
fn matrix_colour(intent: Intent, components: [f64; 3]) -> (u8, u8, u8) {
    let mut xyz = [0.0f64; 3];
    for (column, value) in SRGB_COLUMNS.iter().zip(components) {
        for (slot, entry) in xyz.iter_mut().zip(column) {
            *slot += stored(*entry) * value * value;
        }
    }
    if intent == Intent::Absolute {
        xyz = absolute(xyz);
    }
    srgb_of_xyz(xyz)
}

/// The pixel holding the page-space point `(x, y)` of a page `height` tall.
fn at(bitmap: &Bitmap, (x, y): (f64, f64), height: f64) -> (u8, u8, u8) {
    pixel(bitmap, x.floor() as u32, (height - y).floor() as u32)
}

/// `got` is `want` within the level of 255 the integer paths round by.
fn within_a_level(got: (u8, u8, u8), want: (u8, u8, u8), what: &str) {
    for (channel, (g, w)) in [(got.0, want.0), (got.1, want.1), (got.2, want.2)]
        .into_iter()
        .enumerate()
    {
        assert!(
            g.abs_diff(w) <= 1,
            "{what}: channel {channel} is {g}, not {w} ({got:?} against {want:?})"
        );
    }
}

/// **Each intent paints its recomputed transform.** Under each of Table 69's
/// intents, set by `ri`, a fill, a stroke and a glyph of the embedded face
/// in an `ICCBased` space over a profile carrying a different table for each
/// intent, and over a matrix/TRC profile, each with a media white other than
/// D50, paint the sRGB recomputed from ICC.1: the table 8.10.2 designates
/// for the intent, absolute colorimetric relative colorimetric's scaled by
/// the media white, and the matrix alike under every intent but absolute.
/// The intents part where their transforms do; a name 8.6.5.8 does not list
/// paints as relative colorimetric, byte for byte.
#[test]
fn each_intent_paints_its_recomputed_transform() {
    let mut painted = Vec::new();
    for intent in Intent::ALL {
        let bitmap = render(intent_page(intent.name()));
        let mut colours = Vec::new();
        for (row, colour) in [
            (0, table_colour as fn(Intent, [f64; 3]) -> (u8, u8, u8)),
            (1, matrix_colour),
        ] {
            for (paint, components) in INTENT_PAINTS.into_iter().enumerate() {
                let got = at(&bitmap, intent_spot(row, paint), INTENT_SIZE);
                let what = format!(
                    "{intent:?}, {} profile, {}",
                    ["table", "matrix"][row],
                    ["fill", "stroke", "glyph"][paint]
                );
                within_a_level(got, colour(intent, components), &what);
                colours.push(got);
            }
        }
        painted.push(colours);
    }

    // The table row's three tables are three colours, absolute colorimetric
    // is not relative's, and the matrix row is one colour for the three
    // intents that reach the model unscaled.
    let (perceptual, relative, saturation, absolute) =
        (&painted[0], &painted[1], &painted[2], &painted[3]);
    for paint in 0..3 {
        assert_ne!(perceptual[paint], relative[paint], "table: perceptual");
        assert_ne!(saturation[paint], relative[paint], "table: saturation");
        assert_ne!(absolute[paint], relative[paint], "table: absolute");
        let matrix = 3 + paint;
        assert_eq!(perceptual[matrix], relative[matrix], "matrix: perceptual");
        assert_eq!(saturation[matrix], relative[matrix], "matrix: saturation");
        assert_ne!(absolute[matrix], relative[matrix], "matrix: absolute");
    }

    // 8.6.5.8: a name the reader does not recognise is relative colorimetric.
    let unknown = render(intent_page("Ultraviolet"));
    let relative_page = render(intent_page("RelativeColorimetric"));
    assert!(
        unknown.data == relative_page.data,
        "an unrecognised intent paints otherwise than relative colorimetric"
    );
}

/// The point a 20-point square at `x` along the bottom of an
/// [`intent_states_page`] is sampled at.
fn patch(bitmap: &Bitmap, x: f64) -> (u8, u8, u8) {
    at(bitmap, (x + 10.5, 10.5), 40.0)
}

/// **`ri` and a graphics state's `/RI` are twins**, through `Q` and an
/// intent changed after `scn`. A page naming its intents by `ri` and its twin
/// naming the same by `gs` render byte for byte alike: a colour painted in
/// perceptual, the same colour in saturation inside `q`/`Q`, again after `Q`
/// in the perceptual `Q` restored, a colour set and then painted after the
/// intent changed to absolute colorimetric, and after a name 8.6.5.8 does not
/// list. Each patch is the recomputed colour of its intent, and the colour
/// set before absolute colorimetric was named paints as the one set after.
#[test]
fn ri_and_extgstate_ri_are_twins() {
    let content = |perceptual: &str, saturation: &str, absolute: &str, unknown: &str| {
        format!(
            "{perceptual} /L cs 0.25 0.5 0.75 scn 0 0 20 20 re f\n\
             q {saturation} 20 0 20 20 re f Q 40 0 20 20 re f\n\
             /L cs 0.75 0.25 0.5 scn {absolute} 60 0 20 20 re f\n\
             {unknown} 80 0 20 20 re f"
        )
    };
    let by_ri = render(intent_states_page(&content(
        "/Perceptual ri",
        "/Saturation ri",
        "/AbsoluteColorimetric ri",
        "/Ultraviolet ri",
    )));
    let by_gs = render(intent_states_page(&content(
        "/GSp gs", "/GSs gs", "/GSa gs", "/GSx gs",
    )));
    assert!(by_ri.data == by_gs.data, "ri and /RI drew different pages");

    let first = [0.25, 0.5, 0.75];
    let second = [0.75, 0.25, 0.5];
    for (x, intent, components, what) in [
        (0.0, Intent::Perceptual, first, "perceptual"),
        (20.0, Intent::Saturation, first, "saturation inside q"),
        (40.0, Intent::Perceptual, first, "perceptual after Q"),
        (60.0, Intent::Absolute, second, "absolute, named after scn"),
        (80.0, Intent::Relative, second, "an unrecognised name"),
    ] {
        within_a_level(patch(&by_ri, x), table_colour(intent, components), what);
    }
    assert_eq!(
        patch(&by_ri, 40.0),
        patch(&by_ri, 0.0),
        "Q restored the intent"
    );

    // The colour set before the intent changed paints as one set after it.
    let after = render(intent_states_page(
        "/AbsoluteColorimetric ri /L cs 0.75 0.25 0.5 scn 60 0 20 20 re f",
    ));
    assert_eq!(
        patch(&by_ri, 60.0),
        patch(&after, 60.0),
        "set before, painted after"
    );
}
