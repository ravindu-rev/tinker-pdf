//! Rendering intents (8.6.5.8) reach the colours a page paints
//! (`docs/design/icc.md`, "Rendering intents").
//!
//! A flat colour in an `ICCBased` space — a fill, a stroke, a glyph — converts
//! through the profile's transform for the intent in force at its painting
//! operator, `ri`'s or a graphics state's `/RI`; so does a shading, painted by
//! `sh` or as a shading pattern's fill, a mesh's among them; and an image
//! through the transform for its own `/Intent` (Table 89), or the intent in
//! force at `Do` or `EI` where it names none. Every expected colour here is
//! recomputed from ICC.1's own arithmetic: a table's multilinear
//! interpolation and v2's legacy Lab encoding, a matrix/TRC profile's columns
//! over its curves, and ICC-absolute colorimetry's scaling by the media white
//! (6.2.3). The connection space reaches sRGB through
//! `tinker_pdf_color::lab_to_srgb`, the engine's public conversion of it, so
//! what is under test is which transform the intent selects and what it makes
//! of the components.
//!
//! The flat-colour pages, and the page painting a fill, an image and a shading
//! under each intent, are `render_support`'s; the last is the page
//! `determinism.rs` fingerprints as `intents`, held here to the transforms it
//! paints. Nothing here depends on a font this build may or may not bundle:
//! the glyph is the repository's own embedded face.

mod render_support;

use render_support::{
    intent_lut_encoded, intent_lut_profile, intent_page, intent_spot, intent_states_page,
    intents_page, pixel, raw_pdf, raw_stream, render, INTENTS_AXIAL, INTENTS_CELL, INTENTS_SAMPLES,
    INTENT_NAMES, INTENT_PAINTS, INTENT_PAPER, INTENT_SIZE, SRGB_COLUMNS,
};
use tinker_pdf::{Bitmap, CacheLimits, Document, RenderOptions, Stats, SvgOptions};
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
    within_levels(got, want, 1, what);
}

/// `got` is `want` within `levels` of 255 in every channel.
fn within_levels(got: (u8, u8, u8), want: (u8, u8, u8), levels: u8, what: &str) {
    for (channel, (g, w)) in [(got.0, want.0), (got.1, want.1), (got.2, want.2)]
        .into_iter()
        .enumerate()
    {
        assert!(
            g.abs_diff(w) <= levels,
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

// ---- images ----------------------------------------------------------------
//
// An image converts in its own `/Intent` where that is a name, relative
// colorimetric for a name 8.6.5.8 does not list, and in the intent in force at
// `Do` or `EI` otherwise; an image mask's `/Intent` is never read, its paint
// being the fill colour (`docs/design/icc.md`, "Where each conversion takes its
// intent").

/// The samples every image here draws, the fingerprinted page's, row-major
/// from the top (8.9.5.2): three corners of the table profile's cube — nodes
/// of its grid, so their colour is the table's own — and a colour between
/// nodes.
const SAMPLES: [[u8; 3]; 4] = INTENTS_SAMPLES;

/// [`SAMPLES`] as an image's bytes.
fn flat_samples() -> Vec<u8> {
    SAMPLES.iter().flatten().copied().collect()
}

/// [`SAMPLES`] as `ASCIIHexDecode` data, its end-of-data marker included.
fn hex_samples() -> String {
    let mut out: String = flat_samples()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    out.push('>');
    out
}

/// The components of [`SAMPLES`]'s sample `index`.
fn sample_components(index: usize) -> [f64; 3] {
    SAMPLES[index].map(|value| f64::from(value) / 255.0)
}

/// The page-space centre of sample `index` of a [`SAMPLES`] image drawn on
/// the 40-point square whose lower left is `corner`: its first row at the
/// top.
fn sample_spot(index: usize, corner: (f64, f64)) -> (f64, f64) {
    let (row, column) = ((index / 2) as f64, (index % 2) as f64);
    (
        corner.0 + 20.0 * column + 10.0,
        corner.1 + 30.0 - 20.0 * row,
    )
}

/// Every sample of a [`SAMPLES`] image drawn at `corner` on a page `height`
/// points tall is the colour of its components through the table `intent`
/// selects: a node of the table's grid within a level, the colour between
/// nodes within the two levels a grid's read keeps to (`docs/design/icc.md`,
/// P-22). The colours painted, in sample order.
fn holds_samples(
    bitmap: &Bitmap,
    height: f64,
    corner: (f64, f64),
    intent: Intent,
    what: &str,
) -> Vec<(u8, u8, u8)> {
    (0..SAMPLES.len())
        .map(|index| {
            let got = at(bitmap, sample_spot(index, corner), height);
            let levels = if index == 3 { 2 } else { 1 };
            let want = table_colour(intent, sample_components(index));
            within_levels(got, want, levels, &format!("{what}, sample {index}"));
            got
        })
        .collect()
}

/// One image XObject of a page: its resource name, its dictionary's entries
/// but its type and length, and its samples.
struct Picture {
    name: &'static str,
    entries: String,
    samples: Vec<u8>,
}

/// A [`SAMPLES`] image's entries in the table profile's space, object 5,
/// and `extra`.
fn image_entries(extra: &str) -> String {
    format!("/Width 2 /Height 2 /ColorSpace [/ICCBased 5 0 R] /BitsPerComponent 8 {extra}")
}

/// A one-page document `width` by `height` points painting `content`: its
/// resources the table profile's space as `/L` — object 5 — beside the
/// `/ColorSpace` entries `spaces` adds, and each of `pictures` an image
/// XObject.
fn picture_document(
    width: f64,
    height: f64,
    spaces: &str,
    content: &str,
    pictures: &[Picture],
) -> Vec<u8> {
    let xobjects: String = pictures
        .iter()
        .enumerate()
        .map(|(index, picture)| format!("/{} {} 0 R ", picture.name, 6 + index))
        .collect();
    let mut objects = vec![
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Count 1 /Kids [3 0 R] >>".to_vec(),
        format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {width} {height}]\n   \
             /Resources << /ColorSpace << /L [/ICCBased 5 0 R] {spaces} >>\n     \
             /XObject << {xobjects}>> >>\n   /Contents 4 0 R >>"
        )
        .into_bytes(),
        raw_stream("", content.as_bytes()),
        raw_stream("/N 3", &intent_lut_profile()),
    ];
    for picture in pictures {
        objects.push(raw_stream(
            &format!("/Type /XObject /Subtype /Image {}", picture.entries),
            &picture.samples,
        ));
    }
    raw_pdf(&objects)
}

/// **An image's `/Intent` selects its transform** (Table 89). An image
/// XObject and an inline image in the table profile's space, each naming its
/// own intent while `ri` names another, paint each sample as the table that
/// intent selects makes it; a name 8.6.5.8 does not list is relative
/// colorimetric's. An `/Intent` that is not a name, and none at all, leave
/// the intent in force deciding, and an image mask's is never read: an
/// XObject mask and an inline one paint the fill colour in the intent in
/// force.
#[test]
fn an_images_intent_selects_its_transform() {
    let named = Intent::ALL.map(|intent| (intent.name(), intent));
    for (name, own) in named.into_iter().chain([("Ultraviolet", Intent::Relative)]) {
        // The intent in force is another, so the image's own is seen to
        // decide.
        let state = if own == Intent::Saturation {
            Intent::Perceptual
        } else {
            Intent::Saturation
        };
        let content = format!(
            "/{} ri\n\
             q 40 0 0 40 0 0 cm /Im0 Do Q\n\
             q 40 0 0 40 40 0 cm BI /W 2 /H 2 /CS /L /BPC 8 /F /AHx /Intent /{name} ID {} EI Q\n\
             q 40 0 0 40 0 40 cm /Im1 Do Q\n\
             q 40 0 0 40 40 40 cm /Im2 Do Q\n\
             /L cs 0.25 0.5 0.75 scn q 40 0 0 40 0 80 cm /Im3 Do Q\n\
             q 40 0 0 40 40 80 cm BI /W 1 /H 1 /IM true /F /AHx /Intent /{name} ID 00> EI Q",
            state.name(),
            hex_samples(),
        );
        let pictures = [
            Picture {
                name: "Im0",
                entries: image_entries(&format!("/Intent /{name}")),
                samples: flat_samples(),
            },
            // An entry that is not a name leaves the intent in force.
            Picture {
                name: "Im1",
                entries: image_entries("/Intent 5"),
                samples: flat_samples(),
            },
            Picture {
                name: "Im2",
                entries: image_entries(""),
                samples: flat_samples(),
            },
            Picture {
                name: "Im3",
                entries: format!(
                    "/Width 1 /Height 1 /ImageMask true /BitsPerComponent 1 /Intent /{name}"
                ),
                samples: vec![0x00],
            },
        ];
        let bitmap = render(picture_document(80.0, 120.0, "", &content, &pictures));
        let what = |part: &str| format!("/Intent /{name} under {state:?}: {part}");
        let xobject = holds_samples(&bitmap, 120.0, (0.0, 0.0), own, &what("the XObject"));
        let inline = holds_samples(&bitmap, 120.0, (40.0, 0.0), own, &what("the inline image"));
        assert_eq!(xobject, inline, "{}", what("the two images part"));
        let not_a_name = holds_samples(
            &bitmap,
            120.0,
            (0.0, 40.0),
            state,
            &what("an /Intent that is not a name"),
        );
        holds_samples(&bitmap, 120.0, (40.0, 40.0), state, &what("no /Intent"));
        assert_ne!(
            xobject,
            not_a_name,
            "{}",
            what("the image's own intent and the one in force paint alike")
        );
        for (x, part) in [(0.0, "the mask XObject"), (40.0, "the inline mask")] {
            within_a_level(
                at(&bitmap, (x + 20.0, 100.0), 120.0),
                table_colour(state, [0.25, 0.5, 0.75]),
                &what(part),
            );
        }
    }
}

/// **An image's `/Intent` and `ri` are twins.** A page drawing an image
/// XObject and an inline image with no `/Intent` under each intent `ri`
/// names, and its twin drawing them naming that intent each in its own
/// `/Intent` with no `ri`, render byte for byte alike — directly, and from
/// the page's display list — and write the same SVG. The four intents paint
/// four pictures, and a name 8.6.5.8 does not list paints relative
/// colorimetric's.
#[test]
fn an_image_intent_and_ri_are_twins() {
    let options = RenderOptions::default();
    let mut pictures_painted = Vec::new();
    for name in Intent::ALL
        .map(Intent::name)
        .into_iter()
        .chain(["Ultraviolet"])
    {
        let document = |ri: &str, intent: &str| {
            let content = format!(
                "{ri}q 40 0 0 40 0 0 cm /Im0 Do Q\n\
                 q 40 0 0 40 40 0 cm BI /W 2 /H 2 /CS /L /BPC 8 /F /AHx {intent} ID {} EI Q",
                hex_samples()
            );
            let pictures = [Picture {
                name: "Im0",
                entries: image_entries(intent),
                samples: flat_samples(),
            }];
            Document::open(picture_document(80.0, 40.0, "", &content, &pictures)).expect("it opens")
        };
        let by_ri = document(&format!("/{name} ri\n"), "");
        let by_intent = document("", &format!("/Intent /{name}"));
        let (ri_page, intent_page) = (
            by_ri.page(0).expect("a page"),
            by_intent.page(0).expect("a page"),
        );
        let rendered = intent_page.render(&options);
        assert!(
            ri_page.render(&options).data == rendered.data,
            "{name}: ri and /Intent rendered apart"
        );
        assert!(
            ri_page.display_list().render(&options).data == rendered.data,
            "{name}: ri's display list and /Intent rendered apart"
        );
        assert!(
            intent_page.display_list().render(&options).data == rendered.data,
            "{name}: /Intent's display list and its page rendered apart"
        );
        let svg = |page: &tinker_pdf::Page| page.to_svg(&SvgOptions::default()).markup;
        assert!(
            svg(&ri_page) == svg(&intent_page),
            "{name}: ri and /Intent wrote different SVG"
        );
        pictures_painted.push(rendered.data);
    }
    for (first, second) in [(0, 1), (0, 2), (0, 3), (1, 2), (1, 3), (2, 3)] {
        assert!(
            pictures_painted[first] != pictures_painted[second],
            "{} and {} painted one picture",
            Intent::ALL[first].name(),
            Intent::ALL[second].name()
        );
    }
    assert!(
        pictures_painted[4] == pictures_painted[1],
        "an unrecognised name paints otherwise than relative colorimetric"
    );
}

/// What rendering `bytes`' first page counted.
fn counted(bytes: Vec<u8>) -> Stats {
    Document::open(bytes)
        .expect("it opens")
        .page(0)
        .expect("a page")
        .render(&RenderOptions {
            stats: true,
            ..RenderOptions::default()
        })
        .stats
        .expect("a render asked for its stats hands them back")
}

/// **One image drawn under two intents decodes once per intent.** The
/// decoded-image cache keys an image on the conversion its colours take, so
/// an image in an `ICCBased` space drawn under four intents decodes four
/// times and meets the decode of an intent it is drawn in again — a name
/// 8.6.5.8 does not list meeting relative colorimetric's. An image whose
/// colours no intent changes is one decode under every intent: one in a
/// device space, an image mask, and one whose own `/Intent` decides for every
/// draw. A palette over an ICC base and a device space an ICC default remaps
/// are ICC colours, one decode an intent.
#[test]
fn an_image_decodes_once_in_each_intent_it_is_drawn_in() {
    let draws = |intents: &[&str]| -> String {
        intents
            .iter()
            .enumerate()
            .map(|(index, intent)| {
                format!("/{intent} ri q 10 0 0 10 {} 0 cm /Im0 Do Q\n", 10 * index)
            })
            .collect()
    };
    let decodes = |spaces: &str, entries: String, samples: Vec<u8>, intents: &[&str]| {
        let picture = Picture {
            name: "Im0",
            entries,
            samples,
        };
        let stats = counted(picture_document(
            80.0,
            10.0,
            spaces,
            &draws(intents),
            &[picture],
        ));
        (stats.images_decoded, stats.image_misses, stats.image_hits)
    };
    let two = ["Perceptual", "Saturation"];

    assert_eq!(
        decodes(
            "",
            image_entries(""),
            flat_samples(),
            &[
                "Perceptual",
                "RelativeColorimetric",
                "Perceptual",
                "Saturation",
                "AbsoluteColorimetric",
                "Ultraviolet",
            ],
        ),
        (4, 4, 2),
        "an ICC image: once in each of four intents, met twice"
    );
    assert_eq!(
        decodes(
            "",
            "/Width 2 /Height 2 /ColorSpace /DeviceRGB /BitsPerComponent 8".to_string(),
            flat_samples(),
            &two,
        ),
        (1, 1, 1),
        "a device image: one decode under both intents"
    );
    assert_eq!(
        decodes(
            "",
            "/Width 1 /Height 1 /ImageMask true /BitsPerComponent 1 /Intent /Perceptual"
                .to_string(),
            vec![0x00],
            &two,
        ),
        (1, 1, 1),
        "an image mask: one decode under both intents"
    );
    assert_eq!(
        decodes(
            "",
            image_entries("/Intent /Perceptual"),
            flat_samples(),
            &["Perceptual", "Saturation", "AbsoluteColorimetric"],
        ),
        (1, 1, 2),
        "an image naming its own intent: one decode under every intent"
    );
    assert_eq!(
        decodes(
            "",
            "/Width 2 /Height 2 /ColorSpace [/Indexed [/ICCBased 5 0 R] 1 <00ff00ff00ff>] \
             /BitsPerComponent 8"
                .to_string(),
            vec![0, 1, 1, 0],
            &two,
        ),
        (2, 2, 0),
        "a palette over an ICC base: once an intent"
    );
    assert_eq!(
        decodes(
            "/DefaultRGB [/ICCBased 5 0 R]",
            "/Width 2 /Height 2 /ColorSpace /DeviceRGB /BitsPerComponent 8".to_string(),
            flat_samples(),
            &two,
        ),
        (2, 2, 0),
        "a device image under an ICC default: once an intent"
    );
}

/// **A document's kept image is kept under its conversion.** With a cache
/// budget, an image in an `ICCBased` space drawn under two intents paints
/// what a render with no budget paints, decoding once in each intent and
/// meeting both decodes in the next render; a device image drawn under both
/// is one decode, met by the second draw and by both of the next render's.
#[test]
fn a_kept_image_is_kept_under_its_conversion() {
    let content = "/Perceptual ri q 10 0 0 10 0 0 cm /Im0 Do Q\n\
                   /Saturation ri q 10 0 0 10 10 0 cm /Im0 Do Q\n";
    let device = "/Width 2 /Height 2 /ColorSpace /DeviceRGB /BitsPerComponent 8".to_string();
    for (what, entries, first, again) in [
        ("an ICC image", image_entries(""), (2, 2, 0), (0, 0, 2)),
        ("a device image", device, (1, 1, 1), (0, 0, 2)),
    ] {
        let picture = Picture {
            name: "Im0",
            entries,
            samples: flat_samples(),
        };
        let bytes = picture_document(20.0, 10.0, "", content, &[picture]);
        let options = RenderOptions {
            stats: true,
            ..RenderOptions::default()
        };
        let plain = Document::open(bytes.clone())
            .expect("it opens")
            .page(0)
            .expect("a page")
            .render(&options);
        let kept = Document::open(bytes)
            .expect("it opens")
            .with_cache_limits(CacheLimits::new(1 << 24));
        for (render, want) in [("the first render", first), ("the next", again)] {
            let rendered = kept.page(0).expect("a page").render(&options);
            assert!(
                rendered.data == plain.data,
                "{what}, {render}: a kept image paints otherwise than a decode"
            );
            let stats = rendered
                .stats
                .expect("a render asked for its stats hands them back");
            assert_eq!(
                (stats.images_decoded, stats.image_misses, stats.image_hits),
                want,
                "{what}, {render}: decoded, missed and met"
            );
        }
    }
}

// ---- shadings ----------------------------------------------------------------
//
// A shading's colours convert in the intent in force at its painting operator,
// `sh`'s or the one at the fill that paints a shading pattern (11.7.5.3), a
// mesh's vertices among them (`docs/design/icc.md`, "Where each conversion
// takes its intent").

/// The shaded page's width and height, in points and pixels alike.
const SHADED: (f64, f64) = (120.0, 80.0);

/// The colour of a linear axial function (`/N 1`) from `start` to `end` at
/// `t`.
fn between(start: [f64; 3], end: [f64; 3], t: f64) -> [f64; 3] {
    [0, 1, 2].map(|channel| start[channel] + t * (end[channel] - start[channel]))
}

/// The colour of [`INTENTS_AXIAL`] at `t`.
fn axial_at(t: f64) -> [f64; 3] {
    between(INTENTS_AXIAL.0, INTENTS_AXIAL.1, t)
}

/// A free-form triangle mesh's vertex stream (8.7.4.5.5, type 4): each vertex
/// a flag of 0 in 8 bits, so that every three make a triangle of their own,
/// its coordinates in 16 bits over the shaded page, and its colour's 8-bit
/// inputs after them.
fn triangles(vertices: &[((f64, f64), &[u8])]) -> Vec<u8> {
    let mut out = Vec::new();
    for ((x, y), values) in vertices {
        out.push(0);
        for (value, extent) in [(*x, SHADED.0), (*y, SHADED.1)] {
            let raw = (value / extent * 65535.0).round() as u16;
            out.extend_from_slice(&raw.to_be_bytes());
        }
        out.extend_from_slice(values);
    }
    out
}

/// The mesh triangles' corners: two along the left of the top row, which
/// `sh` paints, and two along its right, which a shading pattern's fill
/// paints, each a third of a column wide from 62 to 78 points up.
const TRIANGLES: [[(f64, f64); 3]; 4] = [
    [(2.0, 62.0), (38.0, 62.0), (20.0, 78.0)],
    [(42.0, 62.0), (78.0, 62.0), (60.0, 78.0)],
    [(82.0, 62.0), (98.0, 62.0), (90.0, 78.0)],
    [(102.0, 62.0), (118.0, 62.0), (110.0, 78.0)],
];

/// The page-space point well inside triangle `index` of [`TRIANGLES`] the
/// test samples at.
fn inside_triangle(index: usize) -> (f64, f64) {
    let [(x0, _), (x1, _), _] = TRIANGLES[index];
    ((x0 + x1) / 2.0, 66.0)
}

/// What the shaded page paints its four triangles in: the first two by
/// their components, the last two by a function of `t` 0 and 1.
const TRIANGLE_COLOURS: [[f64; 3]; 4] = [
    [0.0, 1.0, 0.0],
    [1.0, 0.0, 1.0],
    [1.0, 1.0, 0.0],
    [0.0, 0.0, 1.0],
];

/// The shaded page, the intent in force set by `intent` — a `ri` or a `gs`
/// operation — in the table profile's space: along the bottom
/// [`INTENTS_AXIAL`] by `sh` across the first column and as a shading
/// pattern's fill across the second, each column's pixel `p` at `t = p / 32`;
/// in the middle a function-based shading of `(x, y)` to `[x, y, 1 − x]` by
/// `sh` over a 32-point square, its pixels at multiples of 1/32; along the top
/// [`TRIANGLES`] in [`TRIANGLE_COLOURS`], the first two a mesh carrying its
/// components that `sh` paints, the last two one carrying a function that a
/// shading pattern's fill paints. Every colour is a node of the 33-point grid
/// the table converts through, so a conversion is the table's pipeline
/// exactly (`docs/design/icc.md`, P-22).
fn shaded_page(intent: &str) -> Vec<u8> {
    let content = format!(
        "{intent}\n\
         q 0 0 40 20 re W n /Sh0 sh Q\n\
         /Pattern cs /P0 scn 40 0 40 20 re f\n\
         q 0 20 40 40 re W n 32 0 0 32 0.5 20.5 cm /Sh1 sh Q\n\
         q 0 60 120 20 re W n /Sh2 sh Q\n\
         /Pattern cs /P1 scn 80 60 40 20 re f"
    );
    let byte = |value: f64| (value * 255.0).round() as u8;
    let components: Vec<[u8; 3]> = TRIANGLE_COLOURS.iter().map(|c| c.map(byte)).collect();
    let mut direct = Vec::new();
    let mut parametric = Vec::new();
    for (index, corners) in TRIANGLES.iter().enumerate() {
        for corner in corners {
            match index {
                0 | 1 => direct.push((*corner, components[index].to_vec())),
                // `t` of 0 for the third triangle and 1 for the fourth.
                _ => parametric.push((*corner, vec![if index == 2 { 0 } else { 255 }])),
            }
        }
    }
    let borrowed = |vertices: &[((f64, f64), Vec<u8>)]| {
        triangles(
            &vertices
                .iter()
                .map(|(at, values)| (*at, values.as_slice()))
                .collect::<Vec<_>>(),
        )
    };
    let (start, end) = INTENTS_AXIAL;
    let mesh = "/ShadingType 4 /ColorSpace [/ICCBased 5 0 R] /BitsPerCoordinate 16 \
                /BitsPerComponent 8 /BitsPerFlag 8";
    let objects = vec![
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Count 1 /Kids [3 0 R] >>".to_vec(),
        format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {} {}]\n   \
             /Resources << /ColorSpace << /L [/ICCBased 5 0 R] >>\n     \
             /Shading << /Sh0 6 0 R /Sh1 7 0 R /Sh2 9 0 R >>\n     \
             /Pattern << /P0 10 0 R /P1 11 0 R >>\n     \
             /ExtGState << /GSp << /RI /Perceptual >> /GSr << /RI /RelativeColorimetric >>\n       \
             /GSs << /RI /Saturation >> /GSa << /RI /AbsoluteColorimetric >>\n       \
             /GSx << /RI /Ultraviolet >> >> >>\n   \
             /Contents 4 0 R >>",
            SHADED.0, SHADED.1
        )
        .into_bytes(),
        raw_stream("", content.as_bytes()),
        raw_stream("/N 3", &intent_lut_profile()),
        format!(
            "<< /ShadingType 2 /ColorSpace [/ICCBased 5 0 R] /Coords [0.5 0 32.5 0]\n   \
             /Function << /FunctionType 2 /Domain [0 1] /C0 [{} {} {}] /C1 [{} {} {}] /N 1 >>\n   \
             /Extend [true true] >>",
            start[0], start[1], start[2], end[0], end[1], end[2]
        )
        .into_bytes(),
        b"<< /ShadingType 1 /ColorSpace [/ICCBased 5 0 R] /Domain [0 1 0 1] /Function 8 0 R >>"
            .to_vec(),
        raw_stream(
            "/FunctionType 4 /Domain [0 1 0 1] /Range [0 1 0 1 0 1]",
            b"{ 1 index 1 exch sub }",
        ),
        raw_stream(
            &format!("{mesh} /Decode [0 {} 0 {} 0 1 0 1 0 1]", SHADED.0, SHADED.1),
            &borrowed(&direct),
        ),
        b"<< /PatternType 2 /Shading 6 0 R /Matrix [1 0 0 1 40 0] >>".to_vec(),
        b"<< /PatternType 2 /Shading 12 0 R >>".to_vec(),
        raw_stream(
            &format!(
                "{mesh} /Decode [0 {} 0 {} 0 1]\n   \
                 /Function << /FunctionType 2 /Domain [0 1] /C0 [1 1 0] /C1 [0 0 1] /N 1 >>",
                SHADED.0, SHADED.1
            ),
            &borrowed(&parametric),
        ),
    ];
    raw_pdf(&objects)
}

/// The function-based shading's pixels the test samples, as device
/// `(column, row)`: its corners and inside, each at whole 32nds of its
/// domain.
const FUNCTION_PIXELS: [(u32, u32); 5] = [(0, 59), (16, 43), (32, 27), (8, 51), (24, 35)];

/// **Each intent shades its recomputed transform.** Under each of Table 69's
/// intents, set by `ri`, an axial shading painted by `sh` and as a shading
/// pattern's fill, a function-based shading, a mesh carrying its components
/// that `sh` paints and one carrying a function that a pattern's fill paints,
/// all in the table profile's space, paint the sRGB the table 8.10.2
/// designates for the intent makes of their components, absolute colorimetric
/// relative colorimetric's scaled by the media white. The intent set by a
/// graphics state's `/RI` paints byte for byte as `ri`'s, the intents part
/// where their tables do, and a name 8.6.5.8 does not list paints as relative
/// colorimetric.
#[test]
fn each_intent_shades_its_recomputed_transform() {
    let height = SHADED.1;
    let mut axial = Vec::new();
    for (intent, state) in Intent::ALL.into_iter().zip(["GSp", "GSr", "GSs", "GSa"]) {
        let bitmap = render(shaded_page(&format!("/{} ri", intent.name())));
        let by_state = render(shaded_page(&format!("/{state} gs")));
        assert!(
            bitmap.data == by_state.data,
            "{intent:?}: ri and /RI shaded different pages"
        );
        for (base, how) in [(0.0, "sh"), (40.0, "a shading pattern's fill")] {
            for p in [0.0, 8.0, 16.0, 24.0, 32.0] {
                within_a_level(
                    at(&bitmap, (base + p + 0.5, 10.0), height),
                    table_colour(intent, axial_at(p / 32.0)),
                    &format!("{intent:?}: the axial shading by {how}, t = {p}/32"),
                );
            }
        }
        for (column, row) in FUNCTION_PIXELS {
            let (x, y) = (f64::from(column) / 32.0, f64::from(59 - row) / 32.0);
            within_a_level(
                pixel(&bitmap, column, row),
                table_colour(intent, [x, y, 1.0 - x]),
                &format!("{intent:?}: the function-based shading at ({x}, {y})"),
            );
        }
        for (index, colour) in TRIANGLE_COLOURS.iter().enumerate() {
            let how = if index < 2 { "sh" } else { "a pattern's fill" };
            within_a_level(
                at(&bitmap, inside_triangle(index), height),
                table_colour(intent, *colour),
                &format!("{intent:?}: mesh triangle {index}, by {how}"),
            );
        }
        axial.push(at(&bitmap, (16.5, 10.0), height));
    }
    let (perceptual, relative, saturation, absolute) = (axial[0], axial[1], axial[2], axial[3]);
    assert_ne!(perceptual, relative, "perceptual shades as relative");
    assert_ne!(saturation, relative, "saturation shades as relative");
    assert_ne!(absolute, relative, "absolute shades as relative");

    // 8.6.5.8: a name the reader does not recognise is relative colorimetric,
    // set by `ri` or by a graphics state.
    let relative_page = render(shaded_page("/RelativeColorimetric ri"));
    for unknown in ["/Ultraviolet ri", "/GSx gs"] {
        assert!(
            render(shaded_page(unknown)).data == relative_page.data,
            "{unknown}: an unrecognised intent shades otherwise than relative colorimetric"
        );
    }
}

/// A page shading [`INTENT_PAINTS`]'s first colour as a constant axial
/// shading by `sh` across its left half, and filling its right half with the
/// same colour, under `intent`.
fn constant_page(intent: &str) -> Vec<u8> {
    let [c0, c1, c2] = INTENT_PAINTS[0];
    let content = format!(
        "/{intent} ri\n\
         q 0 0 40 20 re W n /Sh0 sh Q\n\
         /L cs {c0} {c1} {c2} scn 40 0 40 20 re f"
    );
    let objects = vec![
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Count 1 /Kids [3 0 R] >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 80 20]\n   \
          /Resources << /ColorSpace << /L [/ICCBased 5 0 R] >> /Shading << /Sh0 6 0 R >> >>\n   \
          /Contents 4 0 R >>"
            .to_vec(),
        raw_stream("", content.as_bytes()),
        raw_stream("/N 3", &intent_lut_profile()),
        format!(
            "<< /ShadingType 2 /ColorSpace [/ICCBased 5 0 R] /Coords [0 0 40 0]\n   \
             /Function << /FunctionType 2 /Domain [0 1] /C0 [{c0} {c1} {c2}] /C1 [{c0} {c1} {c2}] \
             /N 1 >>\n   /Extend [true true] >>"
        )
        .into_bytes(),
    ];
    raw_pdf(&objects)
}

/// **A constant axial shading paints its interior as the flat fill** in the
/// same intent, under each of Table 69's: the shading's colour converts as
/// the fill's does, byte for byte, and both are the table the intent
/// designates.
#[test]
fn a_constant_axial_shading_paints_as_the_flat_fill_in_each_intent() {
    let mut painted = Vec::new();
    for intent in Intent::ALL {
        let bitmap = render(constant_page(intent.name()));
        let fill = pixel(&bitmap, 60, 10);
        within_a_level(
            fill,
            table_colour(intent, INTENT_PAINTS[0]),
            &format!("{intent:?}: the fill"),
        );
        for row in 3..17 {
            for column in 3..37 {
                assert_eq!(
                    pixel(&bitmap, column, row),
                    pixel(&bitmap, column + 40, row),
                    "{intent:?}: the shading at ({column}, {row}) and the fill beside it"
                );
            }
        }
        painted.push(fill);
    }
    assert_ne!(painted[0], painted[1], "perceptual paints as relative");
    assert_ne!(painted[2], painted[1], "saturation paints as relative");
    assert_ne!(painted[3], painted[1], "absolute paints as relative");
}

/// **The fingerprinted page paints each intent's transform.** `intents`, the
/// page `determinism.rs` fingerprints, paints in each column, under the
/// intent `ri` names there, its fill, its image's samples and its axial
/// shading's pixels in the colours the table that intent designates makes of
/// their components: a node of the table's grid within a level, the image's
/// colour between nodes within the two levels a grid's read keeps to.
#[test]
fn the_fingerprinted_intents_page_paints_each_intents_transform() {
    let bitmap = render(intents_page());
    let (width, height) = INTENTS_CELL;
    let page_height = 3.0 * height;
    for (column, (intent, name)) in Intent::ALL.into_iter().zip(INTENT_NAMES).enumerate() {
        assert_eq!(intent.name(), name, "the page's column {column}");
        let x = width * column as f64;
        within_a_level(
            at(&bitmap, (x + width / 2.0, 2.5 * height), page_height),
            table_colour(intent, INTENT_PAINTS[0]),
            &format!("{name}: the fill"),
        );
        for (index, sample) in INTENTS_SAMPLES.iter().enumerate() {
            let (row, across) = ((index / 2) as f64, (index % 2) as f64);
            let spot = (
                x + width / 2.0 * across + width / 4.0,
                2.0 * height - height / 2.0 * row - height / 4.0,
            );
            let levels = if index == 3 { 2 } else { 1 };
            within_levels(
                at(&bitmap, spot, page_height),
                table_colour(intent, sample.map(|value| f64::from(value) / 255.0)),
                levels,
                &format!("{name}: the image's sample {index}"),
            );
        }
        for p in [0.0, 8.0, 16.0, 24.0, 32.0] {
            within_a_level(
                at(&bitmap, (x + p + 0.5, height / 2.0), page_height),
                table_colour(intent, axial_at(p / 32.0)),
                &format!("{name}: the shading at t = {p}/32"),
            );
        }
    }
}
