//! Known answers for the ICC engine's transforms out of the connection space
//! (`docs/design/icc.md`, "ICC engine completion"): each held to its
//! arithmetic recomputed here from ICC.1's own equations — a colorant
//! matrix's inverse by its cofactors, a tone curve's inverse from its form
//! (10.5's `curv` gamma and sampled curves, 10.18's `para`), ICC-absolute
//! colorimetry's media white (6.2.3), Lab's relation to XYZ (6.4), and a
//! lookup table's stages in its type's order, `mft1`'s and `mft2`'s (10.8,
//! 10.9) and `mBA `'s (10.11) — and never from the engine's code. The
//! profiles are built here byte by byte, so each answer is a statement about
//! the format.
//!
//! A transform out of the connection space reads its device value from
//! tables: the inverted curve is held at 4 096 evenly spaced values of light
//! and read between them, and the matrix is held in s15.16. So a channel is
//! held to **the table step** around its light: the span of the curve's exact
//! inverse over the interval of those values the light falls in and the one
//! either side of it — the matrix's rounding can move a light across one of
//! them — and one device unit for the rounding of the entries
//! (`docs/design/icc.md`, "The output transform").

use tinker_pdf_color::icc::{
    IccError, Intent, Link, Model, Output, Profile, Transform, Transforms, PCS_WHITE,
};

/// Entries in an inverted curve: the engine's compiled-curve resolution.
const ENTRIES: f64 = 4096.0;

/// sRGB's primaries adapted to D50, as an sRGB ICC profile stores them.
const SRGB_COLUMNS: [[f64; 3]; 3] = [
    [0.436_065_674, 0.222_488_403, 0.013_916_015],
    [0.385_147_095, 0.716_873_169, 0.097_076_416],
    [0.143_066_406, 0.060_607_910, 0.714_096_069],
];

/// A warm paper, darker than D50 and every channel its own ratio to it: the
/// media white the profiles here carry, so ICC-absolute colorimetry is a
/// transform of its own.
const PAPER: [f64; 3] = [0.88, 0.91, 0.72];

/// An `s15Fixed16Number`'s bytes.
fn s15(value: f64) -> [u8; 4] {
    (((value * 65536.0).round() as i32) as u32).to_be_bytes()
}

/// What an `s15Fixed16Number` holding `value` reads back as.
fn stored(value: f64) -> f64 {
    (value * 65536.0).round() / 65536.0
}

/// An `XYZType` tag.
fn xyz_tag(value: [f64; 3]) -> Vec<u8> {
    let mut out = b"XYZ ".to_vec();
    out.extend_from_slice(&[0; 4]);
    for component in value {
        out.extend_from_slice(&s15(component));
    }
    out
}

/// A `curv` tag holding one gamma, in `u8Fixed8Number` (10.5).
fn gamma_tag(gamma: f64) -> Vec<u8> {
    let mut out = b"curv".to_vec();
    out.extend_from_slice(&[0; 4]);
    out.extend_from_slice(&1u32.to_be_bytes());
    out.extend_from_slice(&((gamma * 256.0).round() as u16).to_be_bytes());
    out
}

/// A `curv` tag holding sampled points (10.5).
fn sampled_tag(points: &[u16]) -> Vec<u8> {
    let mut out = b"curv".to_vec();
    out.extend_from_slice(&[0; 4]);
    out.extend_from_slice(&(points.len() as u32).to_be_bytes());
    for point in points {
        out.extend_from_slice(&point.to_be_bytes());
    }
    out
}

/// A `para` tag of `function` with `params` (10.18).
fn para_tag(function: u16, params: &[f64]) -> Vec<u8> {
    let mut out = b"para".to_vec();
    out.extend_from_slice(&[0; 4]);
    out.extend_from_slice(&function.to_be_bytes());
    out.extend_from_slice(&[0; 2]);
    for param in params {
        out.extend_from_slice(&s15(*param));
    }
    out
}

/// A version 4 display profile in `space` and `pcs` carrying `tags`.
fn profile(space: &[u8; 4], pcs: &[u8; 4], tags: &[([u8; 4], Vec<u8>)]) -> Vec<u8> {
    profile_of(b"mntr", space, pcs, tags)
}

/// A version 4 profile of device class `class` in `space` and `pcs`
/// carrying `tags`.
fn profile_of(
    class: &[u8; 4],
    space: &[u8; 4],
    pcs: &[u8; 4],
    tags: &[([u8; 4], Vec<u8>)],
) -> Vec<u8> {
    let mut out = vec![0u8; 128];
    out[8..12].copy_from_slice(&0x0430_0000u32.to_be_bytes());
    out[12..16].copy_from_slice(class);
    out[16..20].copy_from_slice(space);
    out[20..24].copy_from_slice(pcs);
    out[36..40].copy_from_slice(b"acsp");
    out.extend_from_slice(&(tags.len() as u32).to_be_bytes());
    let start = 132 + 12 * tags.len();
    let mut body = Vec::new();
    for (signature, data) in tags {
        out.extend_from_slice(signature);
        out.extend_from_slice(&((start + body.len()) as u32).to_be_bytes());
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        body.extend_from_slice(data);
        while body.len() % 4 != 0 {
            body.push(0);
        }
    }
    out.extend_from_slice(&body);
    let size = out.len() as u32;
    out[0..4].copy_from_slice(&size.to_be_bytes());
    out
}

/// A tone curve as ICC.1 defines it, from the numbers its tag stores, with
/// its exact inverse.
#[derive(Clone, Debug)]
enum Tone {
    /// `y = x^γ` (10.5, one entry; 10.18, function type 0).
    Gamma(f64),
    /// 10.18's function type 3: `(a·x + b)^γ` from `d` up, `c·x` below.
    Linear {
        g: f64,
        a: f64,
        b: f64,
        c: f64,
        d: f64,
    },
    /// 10.5's sampled curve: its points evenly spaced over `0..=1`, read
    /// linearly between them.
    Sampled(Vec<f64>),
}

impl Tone {
    /// The curve at `x` in `0..=1`.
    fn forward(&self, x: f64) -> f64 {
        match self {
            Tone::Gamma(g) => x.powf(*g),
            Tone::Linear { g, a, b, c, d } => {
                if x >= *d {
                    (a * x + b).max(0.0).powf(*g)
                } else {
                    c * x
                }
            }
            Tone::Sampled(points) => {
                let last = (points.len() - 1) as f64;
                let position = x * last;
                let index = (position.floor() as usize).min(points.len() - 2);
                let fraction = position - index as f64;
                points[index] + (points[index + 1] - points[index]) * fraction
            }
        }
    }

    /// The `x` the curve takes to light `y` in `0..=1`: the lowest, where a
    /// flat stretch or the jump between a linear foot and its power law
    /// leaves more than one.
    fn inverse(&self, y: f64) -> f64 {
        let x = match self {
            Tone::Gamma(g) => y.powf(1.0 / g),
            Tone::Linear { g, a, b, c, d } => {
                let (foot, head) = (c * d, (a * d + b).max(0.0).powf(*g));
                if y < foot {
                    y / c
                } else if y <= head {
                    *d
                } else {
                    (y.powf(1.0 / g) - b) / a
                }
            }
            Tone::Sampled(points) => {
                let last = (points.len() - 1) as f64;
                if y <= points[0] {
                    0.0
                } else {
                    let segment = points.windows(2).position(|pair| y <= pair[1]);
                    match segment {
                        None => 1.0,
                        Some(index) => {
                            let (low, high) = (points[index], points[index + 1]);
                            (index as f64 + (y - low) / (high - low)) / last
                        }
                    }
                }
            }
        };
        x.clamp(0.0, 1.0)
    }

    /// The most the curve rises per unit of `x` anywhere on `0..=1`: a gamma
    /// of one or more at one, a linear foot's slope or its power law's at
    /// one, a sampled curve's steepest segment; a gamma below one has no
    /// bound at zero.
    fn steepest(&self) -> f64 {
        match self {
            Tone::Gamma(g) if *g >= 1.0 => *g,
            Tone::Gamma(_) => f64::INFINITY,
            Tone::Linear { g, a, b, c, .. } => c.max(g * a * (a + b).max(0.0).powf(g - 1.0)),
            Tone::Sampled(points) => {
                points
                    .windows(2)
                    .map(|pair| (pair[1] - pair[0]).abs())
                    .fold(0.0f64, f64::max)
                    * (points.len() - 1) as f64
            }
        }
    }
}

/// The table step a channel is held to around light `y`, in device units:
/// the span of `tone`'s exact inverse over the interval of the 4 096 values
/// `y` falls in and the one either side, and one unit for the rounding.
fn step(tone: &Tone, y: f64) -> f64 {
    let last = ENTRIES - 1.0;
    let node = (y.clamp(0.0, 1.0) * last).floor().min(last - 1.0);
    let low = (node - 1.0).max(0.0) / last;
    let high = (node + 2.0).min(last) / last;
    (tone.inverse(high) - tone.inverse(low)) * 65535.0 + 1.0
}

/// `got` is `tone`'s inverse at light `y` within the table step there.
fn holds(got: u16, tone: &Tone, y: f64, what: &str) {
    let light = y.clamp(0.0, 1.0);
    let want = tone.inverse(light) * 65535.0;
    let slack = step(tone, light);
    assert!(
        (f64::from(got) - want).abs() <= slack,
        "{what}: {got}, not {want:.1} within {slack:.1} (light {y})"
    );
}

/// ICC-absolute colorimetry's scale (6.2.3, equations 4 to 6): the media
/// white, as stored, over the PCS white, as stored.
fn media_scale() -> [f64; 3] {
    [0, 1, 2].map(|channel| stored(PAPER[channel]) / PCS_WHITE[channel])
}

/// What a profile's transform for `intent` divides a connection-space value
/// by before it enters: the media scale under absolute colorimetric, one
/// otherwise.
fn divisor(intent: Intent) -> [f64; 3] {
    if intent == Intent::AbsoluteColorimetric {
        media_scale()
    } else {
        [1.0; 3]
    }
}

/// The matrix/TRC profile: sRGB's columns, a gamma 2.2 `curv` for red,
/// sRGB's own `para` (function type 3) for green and a five-point sampled
/// `curv` for blue, on [`PAPER`]. Its tones, as stored, beside it.
fn matrix_profile() -> (Vec<u8>, [Tone; 3]) {
    let srgb = [2.4, 1.0 / 1.055, 0.055 / 1.055, 1.0 / 12.92, 0.040_45];
    let points: [u16; 5] = [0, 3_000, 12_000, 30_000, 65_535];
    let bytes = profile(
        b"RGB ",
        b"XYZ ",
        &[
            (*b"rXYZ", xyz_tag(SRGB_COLUMNS[0])),
            (*b"gXYZ", xyz_tag(SRGB_COLUMNS[1])),
            (*b"bXYZ", xyz_tag(SRGB_COLUMNS[2])),
            (*b"rTRC", gamma_tag(2.2)),
            (*b"gTRC", para_tag(3, &srgb)),
            (*b"bTRC", sampled_tag(&points)),
            (*b"wtpt", xyz_tag(PAPER)),
        ],
    );
    let [g, a, b, c, d] = srgb.map(stored);
    let tones = [
        Tone::Gamma((2.2f64 * 256.0).round() / 256.0),
        Tone::Linear { g, a, b, c, d },
        Tone::Sampled(points.iter().map(|p| f64::from(*p) / 65535.0).collect()),
    ];
    (bytes, tones)
}

/// The matrix taking linear device values to XYZ — the stored columns as its
/// columns — inverted by its cofactors over its determinant (Cramer's rule).
fn inverse_columns() -> [[f64; 3]; 3] {
    let m = |row: usize, column: usize| stored(SRGB_COLUMNS[column][row]);
    let minor = |r: [usize; 2], c: [usize; 2]| {
        m(r[0], c[0]) * m(r[1], c[1]) - m(r[0], c[1]) * m(r[1], c[0])
    };
    let others = |i: usize| match i {
        0 => [1, 2],
        1 => [0, 2],
        _ => [0, 1],
    };
    let sign = |i: usize, j: usize| if (i + j) % 2 == 0 { 1.0 } else { -1.0 };
    let determinant: f64 = (0..3)
        .map(|j| sign(0, j) * m(0, j) * minor(others(0), others(j)))
        .sum();
    let mut inverse = [[0.0; 3]; 3];
    for (i, row) in inverse.iter_mut().enumerate() {
        for (j, slot) in row.iter_mut().enumerate() {
            // The adjugate's (i, j) is the cofactor of (j, i).
            *slot = sign(j, i) * minor(others(j), others(i)) / determinant;
        }
    }
    inverse
}

/// The XYZ the profile's columns make of linear device values `linear`.
fn columns_times(linear: [f64; 3]) -> [f64; 3] {
    [0, 1, 2].map(|row| {
        (0..3)
            .map(|column| stored(SRGB_COLUMNS[column][row]) * linear[column])
            .sum()
    })
}

/// Device values, each channel at the foot, the knee and the top of its
/// curve and between them.
fn device_grid() -> Vec<[f64; 3]> {
    let levels = [0.0, 0.03, 0.2, 0.5, 0.8, 0.97, 1.0];
    let mut grid = Vec::new();
    for r in levels {
        for g in levels {
            for b in levels {
                grid.push([r, g, b]);
            }
        }
    }
    grid
}

/// **A matrix profile takes XYZ back to its device values.** Under each of
/// ICC.1's four intents, the profile's transform out of the connection space
/// takes XYZ — the images of a grid of device values through its columns and
/// curves, the PCS white, black, and values out of its gamut on both sides —
/// to the device values ICC.1's arithmetic gives: the columns' inverse
/// (recomputed by Cramer's rule from the stored columns) times the XYZ, each
/// channel through its curve's inverse — a gamma's root, sRGB's `para`
/// solved on both sides of its break, a sampled curve's segment read
/// backwards — within the table step. Under absolute colorimetric the XYZ is
/// first divided by the media white over the PCS white (6.2.3). Every intent
/// but absolute colorimetric is one transform, the model serving them all.
#[test]
fn a_matrix_profile_takes_xyz_back_to_its_device_values() {
    let (bytes, tones) = matrix_profile();
    let profile = Profile::parse(&bytes).expect("the matrix profile parses");
    let inverse = inverse_columns();
    let mut inputs: Vec<[f64; 3]> = device_grid()
        .into_iter()
        .map(|device| columns_times([0, 1, 2].map(|c| tones[c].forward(device[c]))))
        .collect();
    inputs.extend([
        [0.0; 3],
        [0.9642, 1.0, 0.8249],
        [0.2, 0.3, 0.1],
        [1.2, 0.8, 1.5],
        [-0.05, 0.4, 0.3],
        [0.05, 0.05, 0.4],
    ]);
    let relative = Output::compile(&profile, Intent::RelativeColorimetric)
        .expect("a matrix/TRC model inverts");
    for intent in Intent::ALL {
        let output = Output::compile(&profile, intent).expect("a matrix/TRC model inverts");
        assert_eq!(output.channels(), 3);
        if intent != Intent::AbsoluteColorimetric {
            assert_eq!(output, relative, "{intent:?} inverts the one model");
        }
        let scale = divisor(intent);
        for relative_xyz in &inputs {
            // What the page holds under this intent: under absolute
            // colorimetric, the relative value on the paper's white.
            let pcs = [0, 1, 2].map(|k| relative_xyz[k] * scale[k]);
            let got = output.apply(pcs);
            assert_eq!(got.channels().len(), 3);
            for (channel, tone) in tones.iter().enumerate() {
                let light: f64 = (0..3)
                    .map(|k| inverse[channel][k] * (pcs[k] / scale[k]))
                    .sum();
                holds(
                    got.channels()[channel],
                    tone,
                    light,
                    &format!("{intent:?}, {pcs:?}, channel {channel}"),
                );
            }
        }
    }
}

/// **Forward then output is the identity for a matrix profile.** Each device
/// value of the grid, taken into the connection space through the parsed
/// profile's own model — its curves evaluated by the engine's
/// `Curve::eval`, its stored columns — and on the media white under absolute
/// colorimetric, comes back through the transform out of it as itself, within
/// the table step of its curve there, under every intent.
#[test]
fn forward_then_output_is_the_identity_for_a_matrix_profile() {
    let (bytes, tones) = matrix_profile();
    let profile = Profile::parse(&bytes).expect("the matrix profile parses");
    let Some(Model::MatrixTrc { columns, curves }) = &profile.model else {
        panic!("a matrix/TRC model: {:?}", profile.model);
    };
    for intent in Intent::ALL {
        let output = Output::compile(&profile, intent).expect("a matrix/TRC model inverts");
        let scale = divisor(intent);
        for device in device_grid() {
            let linear = [0, 1, 2].map(|c| curves[c].eval(device[c]));
            let pcs = [0, 1, 2].map(|row| {
                scale[row]
                    * (0..3)
                        .map(|column| columns[column][row] * linear[column])
                        .sum::<f64>()
            });
            let got = output.apply(pcs);
            for (channel, tone) in tones.iter().enumerate() {
                let want = device[channel] * 65535.0;
                let slack = step(tone, linear[channel]);
                let value = f64::from(got.channels()[channel]);
                assert!(
                    (value - want).abs() <= slack,
                    "{intent:?}, {device:?}, channel {channel}: {value}, not {want} \
                     within {slack:.1}"
                );
            }
        }
    }
}

/// CIE L*a*b*'s lightness as luminance relative to the white (6.4), and
/// back.
fn luminance_of(lightness: f64) -> f64 {
    const DELTA: f64 = 6.0 / 29.0;
    let f = (lightness + 16.0) / 116.0;
    if f > DELTA {
        f * f * f
    } else {
        3.0 * DELTA * DELTA * (f - 4.0 / 29.0)
    }
}

/// [`luminance_of`] run backwards.
fn lightness_of(luminance: f64) -> f64 {
    const DELTA: f64 = 6.0 / 29.0;
    let f = if luminance > DELTA * DELTA * DELTA {
        luminance.cbrt()
    } else {
        luminance / (3.0 * DELTA * DELTA) + 4.0 / 29.0
    };
    116.0 * f - 16.0
}

/// **A grey profile takes luminance back through its curve.** Grey profiles
/// under an XYZ connection space — a gamma `curv`, a `para` of function type
/// 0 and a four-point sampled `curv` — take XYZ to the device grey the
/// curve's inverse gives its `Y`, whatever `X` and `Z` are; one under a Lab
/// connection space, `L*` over 100 through the curve's inverse, whatever `a*`
/// and `b*` are (F.2). Under absolute colorimetric `Y` is first divided by the
/// media white's over the PCS white's — under Lab, the lightness is found
/// again from that `Y` (6.2.3, 6.4). Each within the table step, under every
/// intent, one channel each.
#[test]
fn a_grey_profile_takes_luminance_back_through_its_curve() {
    let gamma = (1.8f64 * 256.0).round() / 256.0;
    let points: [u16; 4] = [0, 10_000, 30_000, 65_535];
    let greys: [(&[u8; 4], Vec<u8>, Tone); 4] = [
        (b"XYZ ", gamma_tag(1.8), Tone::Gamma(gamma)),
        (b"XYZ ", para_tag(0, &[2.2]), Tone::Gamma(stored(2.2))),
        (
            b"XYZ ",
            sampled_tag(&points),
            Tone::Sampled(points.iter().map(|p| f64::from(*p) / 65535.0).collect()),
        ),
        (b"Lab ", gamma_tag(2.0), Tone::Gamma(2.0)),
    ];
    let luminances = [0.0, 0.004, 0.05, 0.18, 0.5, 0.9, 1.0, 1.3];
    let lightnesses = [0.0, 4.0, 18.0, 50.0, 75.0, 100.0];
    for (pcs, curve, tone) in greys {
        let bytes = profile(
            b"GRAY",
            pcs,
            &[(*b"kTRC", curve), (*b"wtpt", xyz_tag(PAPER))],
        );
        let profile = Profile::parse(&bytes).expect("the grey profile parses");
        let lab = pcs == b"Lab ";
        for intent in Intent::ALL {
            let output = Output::compile(&profile, intent).expect("a grey model inverts");
            assert_eq!(output.channels(), 1);
            let scale = divisor(intent)[1];
            let what = |value: &str| format!("{tone:?} under {intent:?}: {value}");
            if lab {
                for lightness in lightnesses {
                    for (a, b) in [(0.0, 0.0), (20.0, -30.0)] {
                        let got = output.apply([lightness, a, b]);
                        let light = lightness_of(luminance_of(lightness) / scale) / 100.0;
                        holds(
                            got.channels()[0],
                            &tone,
                            light,
                            &what(&format!("L* {lightness}, a* {a}, b* {b}")),
                        );
                    }
                }
            } else {
                for y in luminances {
                    for (x, z) in [(0.9642 * y, 0.8249 * y), (0.1, 0.7)] {
                        let got = output.apply([x, y, z]);
                        holds(
                            got.channels()[0],
                            &tone,
                            y / scale,
                            &what(&format!("Y {y}, X {x}, Z {z}")),
                        );
                    }
                }
            }
        }
    }
}

// ---- B2A* tables: lut16Type and lut8Type out of the connection space -------
//
// A `B2A*` table's input is the connection space (ICC.1 9.2.1 to 9.2.3), so
// its value is encoded on the way in as the table's type defines it — XYZ in
// `u1Fixed15` through the table's matrix (10.8), Lab in `lut16Type`'s legacy
// scale or `lut8Type`'s own (6.3.4.2) — then runs through the input curves,
// the grid and the output curves to the data space's channels.

/// The D50 white ICC.1's connection space is relative to (6.3.4.3), which
/// its Lab is read against (6.4).
const D50: [f64; 3] = [0.9642, 1.0, 0.8249];

/// CIE L*a*b* as D50 XYZ (6.4).
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

/// D50 XYZ as CIE L*a*b* (6.4).
fn lab_of_xyz(xyz: [f64; 3]) -> [f64; 3] {
    const DELTA: f64 = 6.0 / 29.0;
    let f = |t: f64| {
        if t > DELTA * DELTA * DELTA {
            t.cbrt()
        } else {
            t / (3.0 * DELTA * DELTA) + 4.0 / 29.0
        }
    };
    let (fx, fy, fz) = (f(xyz[0] / D50[0]), f(xyz[1] / D50[1]), f(xyz[2] / D50[2]));
    [116.0 * fy - 16.0, 500.0 * (fx - fy), 200.0 * (fy - fz)]
}

/// A version 2 lookup table (10.8, 10.9): `lut16Type` (`mft2`) where `wide`,
/// `lut8Type` (`mft1`) otherwise, its every value in the type's own width —
/// an `mft1`'s a byte, from 0 to 255.
#[derive(Clone, Debug)]
struct V2Table {
    wide: bool,
    inputs: usize,
    outputs: usize,
    grid: usize,
    /// The 3×3 matrix, row by row, as real numbers before they are stored.
    matrix: [f64; 9],
    input_curves: Vec<Vec<u16>>,
    /// `grid.pow(inputs)` entries of `outputs` values, the last input's axis
    /// fastest.
    clut: Vec<u16>,
    output_curves: Vec<Vec<u16>>,
}

impl V2Table {
    /// The tag's bytes.
    fn tag(&self) -> Vec<u8> {
        let mut out = if self.wide {
            b"mft2".to_vec()
        } else {
            b"mft1".to_vec()
        };
        out.extend_from_slice(&[0; 4]);
        out.extend_from_slice(&[self.inputs as u8, self.outputs as u8, self.grid as u8, 0]);
        for value in self.matrix {
            out.extend_from_slice(&s15(value));
        }
        let value = |out: &mut Vec<u8>, v: u16| {
            if self.wide {
                out.extend_from_slice(&v.to_be_bytes());
            } else {
                out.push(v as u8);
            }
        };
        if self.wide {
            out.extend_from_slice(&(self.input_curves[0].len() as u16).to_be_bytes());
            out.extend_from_slice(&(self.output_curves[0].len() as u16).to_be_bytes());
        }
        for curve in &self.input_curves {
            for &v in curve {
                value(&mut out, v);
            }
        }
        for &v in &self.clut {
            value(&mut out, v);
        }
        for curve in &self.output_curves {
            for &v in curve {
                value(&mut out, v);
            }
        }
        out
    }

    /// A value as the 16-bit currency, over 0..=1: an `mft1`'s byte over 255.
    fn unit(&self, v: u16) -> f64 {
        if self.wide {
            f64::from(v) / 65535.0
        } else {
            f64::from(v) / 255.0
        }
    }

    /// A curve at `x`, read linearly between its entries.
    fn curve(&self, curve: &[u16], x: f64) -> f64 {
        let last = (curve.len() - 1) as f64;
        let position = x.clamp(0.0, 1.0) * last;
        let index = (position.floor() as usize).min(curve.len() - 2);
        let fraction = position - index as f64;
        let (low, high) = (self.unit(curve[index]), self.unit(curve[index + 1]));
        low + (high - low) * fraction
    }

    /// The grid's cell at `positions`, in nodes along each axis, and the
    /// fractions past it.
    fn cell(&self, positions: [f64; 3]) -> ([usize; 3], [f64; 3]) {
        let last = (self.grid - 1) as f64;
        let mut base = [0usize; 3];
        let mut fraction = [0.0f64; 3];
        for axis in 0..3 {
            let p = positions[axis].clamp(0.0, last);
            let index = (p.floor() as usize).min(self.grid - 2);
            base[axis] = index;
            fraction[axis] = p - index as f64;
        }
        (base, fraction)
    }

    /// Grid node `at`'s channel `c`.
    fn node(&self, at: [usize; 3], c: usize) -> f64 {
        let offset = (at[0] * self.grid + at[1]) * self.grid + at[2];
        self.unit(self.clut[offset * self.outputs + c])
    }

    /// Where `encoded`, three inputs over 0..=1, falls on the grid: each
    /// through its input curve, in nodes.
    fn positions(&self, encoded: [f64; 3]) -> [f64; 3] {
        [0, 1, 2].map(|axis| {
            self.curve(&self.input_curves[axis], encoded[axis]) * (self.grid - 1) as f64
        })
    }

    /// The table's channels for `encoded`, each over 0..=1, recomputed stage
    /// by stage: the input curves, the grid interpolated multilinearly, the
    /// output curves (10.8, 10.9).
    fn evaluate(&self, encoded: [f64; 3]) -> Vec<f64> {
        let (base, fraction) = self.cell(self.positions(encoded));
        (0..self.outputs)
            .map(|c| {
                let mut value = 0.0;
                for corner in 0..8usize {
                    let bits = [(corner >> 2) & 1, (corner >> 1) & 1, corner & 1];
                    let weight: f64 = (0..3)
                        .map(|axis| {
                            if bits[axis] == 1 {
                                fraction[axis]
                            } else {
                                1.0 - fraction[axis]
                            }
                        })
                        .product();
                    let at = [0, 1, 2].map(|axis| base[axis] + bits[axis]);
                    value += weight * self.node(at, c);
                }
                self.curve(&self.output_curves[c], value)
            })
            .collect()
    }

    /// One table step for channel `c` at `encoded`, in 16-bit units: the
    /// most a 1/256 step of the grid's cell along each axis moves the grid's
    /// value there — its interpolation is in 256ths of a cell — carried
    /// through the steepest stretch of the output curve, and four units for
    /// the roundings of the curves, the grid's mix and the result.
    fn step(&self, encoded: [f64; 3], c: usize) -> f64 {
        let (base, _) = self.cell(self.positions(encoded));
        let mut span = 0.0;
        for axis in 0..3 {
            let others: Vec<usize> = (0..3).filter(|&other| other != axis).collect();
            let mut widest = 0.0f64;
            for corner in 0..4usize {
                let mut low = base;
                low[others[0]] += (corner >> 1) & 1;
                low[others[1]] += corner & 1;
                let mut high = low;
                high[axis] += 1;
                widest = widest.max((self.node(high, c) - self.node(low, c)).abs());
            }
            span += widest / 256.0;
        }
        let curve = &self.output_curves[c];
        let slope = curve
            .windows(2)
            .map(|pair| (self.unit(pair[1]) - self.unit(pair[0])).abs())
            .fold(0.0f64, f64::max)
            * (curve.len() - 1) as f64;
        span * slope * 65535.0 + 4.0
    }
}

/// How a connection space is encoded into a table's inputs.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Encoding {
    /// XYZ in `u1Fixed15`, 1.0 at `8000h`, through the table's matrix.
    Xyz,
    /// `lut16Type`'s legacy Lab: `L*` 100 at `FF00h`, `a*` 0 at `8000h`.
    LegacyLab,
    /// `lut8Type`'s Lab, `L*` 100 at `FFh`, `a*` 0 at `80h`, as 0..=1.
    FullLab,
}

impl Encoding {
    /// `pcs`, in its own units, as a table's three inputs over 0..=1,
    /// `matrix` applied to XYZ.
    fn encode(self, pcs: [f64; 3], matrix: [f64; 9]) -> [f64; 3] {
        let pcs = pcs.map(|v| if v.is_nan() { 0.0 } else { v });
        let encoded = match self {
            Encoding::Xyz => {
                let e = matrix.map(stored);
                [0, 1, 2].map(|row| {
                    (e[3 * row] * pcs[0] + e[3 * row + 1] * pcs[1] + e[3 * row + 2] * pcs[2])
                        * 32768.0
                        / 65535.0
                })
            }
            Encoding::LegacyLab | Encoding::FullLab => {
                let reach = if self == Encoding::LegacyLab {
                    65280.0 / 65535.0
                } else {
                    1.0
                };
                [
                    pcs[0] / 100.0 * reach,
                    (pcs[1] + 128.0) / 255.0 * reach,
                    (pcs[2] + 128.0) / 255.0 * reach,
                ]
            }
        };
        encoded.map(|v| v.clamp(0.0, 1.0))
    }
}

/// `got`, a transform's device colour, is `table`'s recomputation for `pcs`
/// encoded as `encoding` says, within one table step in every channel.
fn holds_table(got: &[u16], table: &V2Table, encoding: Encoding, pcs: [f64; 3], what: &str) {
    let encoded = encoding.encode(pcs, table.matrix);
    let want = table.evaluate(encoded);
    assert_eq!(got.len(), table.outputs, "{what}: one value a channel");
    for (c, (&got, want)) in got.iter().zip(want).enumerate() {
        let want = want * 65535.0;
        let slack = table.step(encoded, c);
        assert!(
            (f64::from(got) - want).abs() <= slack,
            "{what}, channel {c}: {got}, not {want:.1} within {slack:.1}"
        );
    }
}

/// A grid of `grid` points an axis over three inputs, `outputs` channels
/// each, every node's value within `top`: channel `c` of node `(i, j, k)`
/// rises along each axis at its own rate.
fn clut(grid: usize, outputs: usize, top: u16) -> Vec<u16> {
    let mut values = Vec::new();
    let last = (grid - 1) as f64;
    for i in 0..grid {
        for j in 0..grid {
            for k in 0..grid {
                for c in 0..outputs {
                    let (x, y, z) = (i as f64 / last, j as f64 / last, k as f64 / last);
                    let weights = [0.5, 0.3, 0.2];
                    let turn = c % 3;
                    let mixed = weights[turn] * x
                        + weights[(turn + 1) % 3] * y * y
                        + weights[(turn + 2) % 3] * (1.0 - z)
                        + 0.05 * c as f64;
                    values.push((mixed.clamp(0.0, 1.0) * f64::from(top)).round() as u16);
                }
            }
        }
    }
    values
}

/// A matrix that mixes every channel, row by row: what 10.8 has a table
/// over XYZ apply to its input.
const MIXING: [f64; 9] = [0.9, 0.05, 0.05, 0.02, 0.95, 0.03, 0.0, 0.1, 0.9];

/// A 16-bit `B2A*` table over three inputs to `outputs` channels: a
/// three-point grid between input curves bent at their middle and output
/// curves of four entries, [`MIXING`] for its matrix.
fn lut16(outputs: usize) -> V2Table {
    V2Table {
        wide: true,
        inputs: 3,
        outputs,
        grid: 3,
        matrix: MIXING,
        input_curves: vec![
            vec![0, 30_000, 65_535],
            vec![0, 40_000, 65_535],
            vec![0, 20_000, 65_535],
        ],
        clut: clut(3, outputs, 65_535),
        output_curves: (0..outputs)
            .map(|c| vec![1_000 * c as u16, 20_000, 45_000, 64_000])
            .collect(),
    }
}

/// An 8-bit table's curve of 256 entries: `x^g` for each byte, as a byte.
fn byte_curve(g: f64) -> Vec<u16> {
    (0..=255u32)
        .map(|i| ((f64::from(i) / 255.0).powf(g) * 255.0).round() as u16)
        .collect()
}

/// An 8-bit `B2A*` table over three inputs to `outputs` channels: a
/// three-point grid between 256-entry curves, [`MIXING`] for its matrix.
fn lut8(outputs: usize) -> V2Table {
    V2Table {
        wide: false,
        inputs: 3,
        outputs,
        grid: 3,
        matrix: MIXING,
        input_curves: vec![byte_curve(0.8), byte_curve(1.0), byte_curve(1.25)],
        clut: clut(3, outputs, 255),
        output_curves: (0..outputs)
            .map(|c| byte_curve(1.0 + 0.1 * c as f64))
            .collect(),
    }
}

/// An `A2B0` of `inputs` channels to the connection space's three over a
/// two-point grid of `wide` width, each node the colour `node` gives its
/// index: enough of a forward table for a profile with no model to parse.
fn forward_table(wide: bool, inputs: usize, node: impl Fn(usize) -> [u16; 3]) -> V2Table {
    let entries = if wide { 2 } else { 256 };
    let top = if wide { 65_535.0 } else { 255.0 };
    let identity = |n: usize| -> Vec<u16> {
        (0..n)
            .map(|i| (i as f64 / (n - 1) as f64 * top).round() as u16)
            .collect()
    };
    V2Table {
        wide,
        inputs,
        outputs: 3,
        grid: 2,
        matrix: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
        input_curves: (0..inputs).map(|_| identity(entries)).collect(),
        clut: (0..1usize << inputs).flat_map(node).collect(),
        output_curves: (0..3).map(|_| identity(entries)).collect(),
    }
}

/// XYZ values a page might hold: black, the PCS white, greys and colours
/// inside the gamut, and values past it on both sides.
const XYZ_VALUES: [[f64; 3]; 8] = [
    [0.0, 0.0, 0.0],
    [0.9642, 1.0, 0.8249],
    [0.2, 0.3, 0.1],
    [0.5, 0.5, 0.5],
    [0.05, 0.6, 0.35],
    [0.4, 0.1, 0.7],
    [1.5, 0.2, 0.8],
    [-0.1, 0.4, 2.5],
];

/// Lab values: black, white, a grey, colours, and the corners of the
/// encodable range.
const LAB_VALUES: [[f64; 3]; 8] = [
    [0.0, 0.0, 0.0],
    [100.0, 0.0, 0.0],
    [50.0, 0.0, 0.0],
    [75.0, 40.0, -30.0],
    [25.0, -60.0, 70.0],
    [60.0, 127.0, -128.0],
    [90.0, -10.0, 95.0],
    [10.0, 30.0, 30.0],
];

/// The value a transform out of the connection space under `intent` hands
/// its table: under absolute colorimetric, `pcs` divided by the media white
/// over the PCS white in XYZ — under Lab, through XYZ and back.
fn relative_value(pcs: [f64; 3], lab: bool, intent: Intent) -> [f64; 3] {
    let scale = divisor(intent);
    if lab {
        let xyz = xyz_of_lab(pcs);
        lab_of_xyz([0, 1, 2].map(|k| xyz[k] / scale[k]))
    } else {
        [0, 1, 2].map(|k| pcs[k] / scale[k])
    }
}

/// The tags of an RGB matrix/TRC model: sRGB's columns over curves of
/// `gamma`.
fn rgb_model(gamma: f64) -> Vec<([u8; 4], Vec<u8>)> {
    vec![
        (*b"rXYZ", xyz_tag(SRGB_COLUMNS[0])),
        (*b"gXYZ", xyz_tag(SRGB_COLUMNS[1])),
        (*b"bXYZ", xyz_tag(SRGB_COLUMNS[2])),
        (*b"rTRC", gamma_tag(gamma)),
        (*b"gTRC", gamma_tag(gamma)),
        (*b"bTRC", gamma_tag(gamma)),
    ]
}

/// **A `B2A*` `lut16Type` takes the connection space through its matrix,
/// curves and grid.** Over XYZ, an RGB profile's `B2A0` — [`MIXING`] for its
/// matrix, input curves bent at their middle, a three-point grid and four
/// -entry output curves — takes each XYZ value to the device values each
/// stage recomputed here gives: the matrix applied to the XYZ, `u1Fixed15`
/// encoding, the input curves read linearly, the grid interpolated
/// multilinearly and the output curves read linearly (10.8). Over Lab, the
/// same table on a profile whose connection space is Lab takes `L*a*b*` in
/// the legacy encoding, its matrix not applied. Within one table step, under
/// perceptual — `B2A0` being its own table — and under relative and absolute
/// colorimetric, which reach it after the `B2A1` the profile does not carry,
/// absolute colorimetric's value divided by the media white first.
#[test]
fn a_b2a_lut16_takes_the_connection_space_through_its_matrix_curves_and_grid() {
    let table = lut16(3);
    let mut xyz_tags = rgb_model(2.2);
    xyz_tags.extend([(*b"B2A0", table.tag()), (*b"wtpt", xyz_tag(PAPER))]);
    let over_xyz =
        Profile::parse(&profile(b"RGB ", b"XYZ ", &xyz_tags)).expect("an RGB/XYZ profile");

    let forward = forward_table(true, 3, |node| {
        let bit = |shift: usize| ((node >> shift) & 1) as u16;
        [20_000 + 30_000 * bit(2), 32_768, 32_768 + 10_000 * bit(0)]
    });
    let lab_tags = [
        (*b"A2B0", forward.tag()),
        (*b"B2A0", table.tag()),
        (*b"wtpt", xyz_tag(PAPER)),
    ];
    let over_lab =
        Profile::parse(&profile(b"RGB ", b"Lab ", &lab_tags)).expect("an RGB/Lab profile");

    for intent in [
        Intent::Perceptual,
        Intent::RelativeColorimetric,
        Intent::AbsoluteColorimetric,
    ] {
        let output = Output::compile(&over_xyz, intent).expect("the B2A0 table reads");
        assert_eq!(output.channels(), 3);
        for pcs in XYZ_VALUES {
            holds_table(
                output.apply(pcs).channels(),
                &table,
                Encoding::Xyz,
                relative_value(pcs, false, intent),
                &format!("over XYZ, {intent:?}, {pcs:?}"),
            );
        }
        let output = Output::compile(&over_lab, intent).expect("the B2A0 table reads");
        for pcs in LAB_VALUES {
            holds_table(
                output.apply(pcs).channels(),
                &table,
                Encoding::LegacyLab,
                relative_value(pcs, true, intent),
                &format!("over Lab, {intent:?}, {pcs:?}"),
            );
        }
    }
}

/// **A `B2A*` `lut8Type` writes four channels of ink.** A CMYK profile's
/// `B2A0`, an 8-bit table over 256-entry curves and a three-point grid, takes
/// XYZ — through its matrix and `u1Fixed15`, which `lut8Type` leaves to the
/// implementation and this reads as its 16-bit widening — and `L*a*b*` in
/// its own 8-bit encoding, `L*` 100 at `FFh` (6.3.4.2), to the four values
/// each stage recomputed here gives, within one table step.
#[test]
fn a_b2a_lut8_writes_four_channels_of_ink() {
    let table = lut8(4);
    for (pcs_space, encoding, values) in [
        (b"XYZ ", Encoding::Xyz, &XYZ_VALUES),
        (b"Lab ", Encoding::FullLab, &LAB_VALUES),
    ] {
        let forward = forward_table(false, 4, |node| {
            let bit = |shift: usize| ((node >> shift) & 1) as u16;
            [255 - 60 * bit(0), 128, 128 + 20 * bit(3)]
        });
        let tags = [(*b"A2B0", forward.tag()), (*b"B2A0", table.tag())];
        let parsed = Profile::parse(&profile(b"CMYK", pcs_space, &tags)).expect("a CMYK profile");
        let output =
            Output::compile(&parsed, Intent::RelativeColorimetric).expect("the B2A0 table reads");
        assert_eq!(output.channels(), 4, "four channels of ink");
        for &pcs in values {
            let got = output.apply(pcs);
            assert_eq!(got.channels().len(), 4);
            holds_table(
                got.channels(),
                &table,
                encoding,
                pcs,
                &format!("{encoding:?}, {pcs:?}"),
            );
        }
    }
}

/// A `B2A*` `mft2` whose every grid node gives `value` in each of `outputs`
/// channels: a table whose output names it.
fn constant(outputs: usize, value: u16) -> Vec<u8> {
    V2Table {
        wide: true,
        inputs: 3,
        outputs,
        grid: 2,
        matrix: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
        input_curves: vec![vec![0, 65_535]; 3],
        clut: vec![value; 8 * outputs],
        output_curves: vec![vec![0, 65_535]; outputs],
    }
    .tag()
}

/// The value [`constant`] gives table `t`'s nodes.
fn constant_value(t: usize) -> u16 {
    [11_111, 33_333, 55_555][t]
}

/// What a profile's `B2A*` tag holds in
/// [`the_relative_colorimetric_b2a_is_chosen_first`].
#[derive(Clone, Copy)]
enum Slot {
    /// A table of three channels, every node its number's constant.
    Constant,
    /// A table whose counts say nothing.
    Broken,
    /// A table of four channels, for a space of three.
    Ink,
    /// No tag.
    Absent,
}

/// **The relative colorimetric `B2A*` table is chosen first**, by ICC.1
/// 8.10.2's rows with `B` for `A`: relative and absolute colorimetric through
/// `B2A1`, perceptual through `B2A0` and saturation through `B2A2`, each
/// before the model; an intent whose table the profile does not carry
/// through `B2A0`, then the model inverted, then the other tables, in that
/// order; a table that does not read — one whose counts say nothing, one of
/// another number of channels than the data space — passed over to the next;
/// and a profile carrying neither a table nor a model refused, as one whose
/// only sources do not read is, for the first reason.
#[test]
fn the_relative_colorimetric_b2a_is_chosen_first() {
    let broken = {
        let mut bytes = b"mft2".to_vec();
        bytes.extend_from_slice(&[0; 8]);
        bytes
    };
    let build = |slots: [Slot; 3], with_model: bool| {
        let mut tags = if with_model {
            rgb_model(1.0)
        } else {
            vec![(
                *b"A2B0",
                forward_table(true, 3, |_| [32_768, 32_768, 32_768]).tag(),
            )]
        };
        for (t, slot) in slots.into_iter().enumerate() {
            let signature = [*b"B2A0", *b"B2A1", *b"B2A2"][t];
            match slot {
                Slot::Constant => tags.push((signature, constant(3, constant_value(t)))),
                Slot::Broken => tags.push((signature, broken.clone())),
                Slot::Ink => tags.push((signature, constant(4, constant_value(t)))),
                Slot::Absent => {}
            }
        }
        Profile::parse(&profile(b"RGB ", b"XYZ ", &tags)).expect("the profile parses")
    };
    // Which table an intent converts through, by the constant it gives:
    // `Some(t)` for `B2A{t}`, `None` for the model.
    let through = |profile: &Profile, intent: Intent| -> Option<usize> {
        let got = Output::compile(profile, intent)
            .unwrap_or_else(|error| panic!("{intent:?}: {error:?}"))
            .apply([0.3, 0.4, 0.2]);
        (0..3).find(|&t| got.channels() == [constant_value(t); 3])
    };
    let intents = [
        Intent::Perceptual,
        Intent::RelativeColorimetric,
        Intent::Saturation,
        Intent::AbsoluteColorimetric,
    ];
    use Slot::{Absent, Broken, Constant, Ink};
    for (slots, with_model, expected, what) in [
        (
            [Constant, Constant, Constant],
            true,
            [Some(0), Some(1), Some(2), Some(1)],
            "every table: each intent its own, both colorimetric intents B2A1",
        ),
        (
            [Constant, Absent, Absent],
            true,
            [Some(0); 4],
            "B2A0 alone: every intent through it, before the model",
        ),
        (
            [Absent, Absent, Absent],
            true,
            [None; 4],
            "no table: the model inverted",
        ),
        (
            [Absent, Absent, Constant],
            false,
            [Some(2); 4],
            "B2A2 alone and no model: the other tables last",
        ),
        (
            [Constant, Broken, Constant],
            true,
            [Some(0), Some(0), Some(2), Some(0)],
            "a B2A1 that does not read: passed over to B2A0",
        ),
        (
            [Constant, Ink, Absent],
            true,
            [Some(0); 4],
            "a B2A1 of four channels for three: passed over",
        ),
        (
            [Absent, Broken, Absent],
            true,
            [None; 4],
            "every table broken: the model",
        ),
    ] {
        let profile = build(slots, with_model);
        for (intent, want) in intents.into_iter().zip(expected) {
            assert_eq!(through(&profile, intent), want, "{what}: {intent:?}");
        }
    }

    // Nothing to convert out through: a profile of forward tables alone, and
    // one whose only `B2A*` does not read.
    let none = build([Absent; 3], false);
    assert_eq!(
        Output::compile(&none, Intent::RelativeColorimetric),
        Err(IccError::MissingTags)
    );
    let unread = build([Absent, Ink, Absent], false);
    assert_eq!(
        Output::compile(&unread, Intent::RelativeColorimetric),
        Err(IccError::UnsupportedSpace)
    );
}

/// **An `mft1` Lab table reads in its own encoding** (6.3.4.2): `lut8Type`'s
/// is not `lut16Type`'s legacy one. Forward, an `mft1` `A2B0` whose node
/// holds `FFh` and `80h` gives `L*` 100 and `a*`, `b*` 0 — an `mft2` holding
/// `FF00h` and `8000h` the same — where the legacy reading of the `mft1`
/// would give 100.39 and 0.5. Out, an `mft1` `B2A0` whose three-point grid
/// holds 0, `80h` and `FFh` along each axis takes `L*` 100 to full scale, its
/// top node, and `a*` 0 to just past its middle node, where `80h` over `FFh`
/// lies, 33 024 of 65 535; the legacy encoding would take them to 65 280 and
/// 32 896.
#[test]
fn an_mft1_lab_table_reads_in_its_own_encoding() {
    let forward8 = forward_table(false, 3, |node| {
        if node == 7 {
            [0xFF, 0x80, 0x80]
        } else {
            [0x00, 0x80, 0x80]
        }
    });
    let forward16 = forward_table(true, 3, |node| {
        if node == 7 {
            [0xFF00, 0x8000, 0x8000]
        } else {
            [0x0000, 0x8000, 0x8000]
        }
    });
    let axes = V2Table {
        wide: false,
        inputs: 3,
        outputs: 3,
        grid: 3,
        matrix: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
        input_curves: vec![byte_curve(1.0); 3],
        clut: (0..27usize)
            .flat_map(|node| {
                let level = [0u16, 0x80, 0xFF];
                [level[node / 9], level[(node / 3) % 3], level[node % 3]]
            })
            .collect(),
        output_curves: vec![byte_curve(1.0); 3],
    };
    for forward in [&forward8, &forward16] {
        let parsed = Profile::parse(&profile(
            b"RGB ",
            b"Lab ",
            &[(*b"A2B0", forward.tag()), (*b"B2A0", axes.tag())],
        ))
        .expect("an RGB/Lab profile");
        let what = if forward.wide { "mft2" } else { "mft1" };
        let white = parsed
            .connection_value(Intent::RelativeColorimetric, &[1.0, 1.0, 1.0])
            .expect("a value");
        assert!(
            (white[0] - 100.0).abs() < 1e-9 && white[1].abs() < 1e-9 && white[2].abs() < 1e-9,
            "{what}: its full scale reads {white:?}, not L* 100 and a*, b* 0"
        );
        let black = parsed
            .connection_value(Intent::RelativeColorimetric, &[0.0, 0.0, 0.0])
            .expect("a value");
        assert!(
            black[0].abs() < 1e-9 && black[1].abs() < 1e-9,
            "{what}: {black:?}"
        );

        let output =
            Output::compile(&parsed, Intent::RelativeColorimetric).expect("the mft1 B2A0 reads");
        let got = output.apply([100.0, 0.0, 0.0]);
        let channels = got.channels();
        assert_eq!(channels[0], 65_535, "L* 100 is the top node: {channels:?}");
        for channel in &channels[1..] {
            assert!(
                channel.abs_diff(33_024) <= 1,
                "a*, b* 0 lie at 80h over FFh: {channels:?}"
            );
        }
    }
}

// ---- v4 tables out of the connection space ----------------------------------
//
// An `mBA ` (`lutBToAType`, ICC.1 10.11) takes the connection space through
// up to five stages in the order the type names them, the reverse of `mAB `'s:
// B curves, a matrix with offsets whose results are clipped to 0..=1, M
// curves, a grid with a point count of its own on each axis, and A curves, one
// a device channel. Its curves are `curv` and `para` tags. Both v4 types
// encode a Lab connection space by each component's range over 0..=1 (10.10.1,
// 10.11.1, 6.3.4.2): `L*` 100 at `FFFFh`, `a*` and `b*` 0 at `8080h`.

/// The identity, row by row.
const IDENTITY: [f64; 9] = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];

/// A stage before an `mBA `'s grid.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Stage {
    /// The B curves, one an input.
    B,
    /// The matrix and its offsets, its results clipped to 0..=1.
    Matrix,
    /// The M curves, one an input.
    M,
}

/// ICC.1's order for the stages before an `mBA `'s grid (10.11).
const ICC_ORDER: [Stage; 3] = [Stage::B, Stage::Matrix, Stage::M];

/// A sampled tone of `points` in the 16-bit currency.
fn sampled(points: &[u16]) -> Tone {
    Tone::Sampled(points.iter().map(|p| f64::from(*p) / 65535.0).collect())
}

/// A curve's tag: a `para` of function type 0 for a gamma or 3 for a linear
/// foot (10.18), a sampled `curv` for sampled points (10.5).
fn tone_tag(tone: &Tone) -> Vec<u8> {
    match tone {
        Tone::Gamma(g) => para_tag(0, &[*g]),
        Tone::Linear { g, a, b, c, d } => para_tag(3, &[*g, *a, *b, *c, *d]),
        Tone::Sampled(points) => sampled_tag(
            &points
                .iter()
                .map(|p| (p * 65535.0).round() as u16)
                .collect::<Vec<u16>>(),
        ),
    }
}

/// A version 4 lookup table out of the connection space, `mBA `: its curves
/// as [`Tone`]s, each stage empty where the tag does not carry it.
#[derive(Clone, Debug)]
struct V4Table {
    /// One an input; ICC.1 makes them required.
    b: Vec<Tone>,
    /// Nine entries row by row, then three offsets, each a whole number of
    /// s15.16 steps.
    matrix: Option<[f64; 12]>,
    /// One an input.
    m: Vec<Tone>,
    /// One point count an input; empty where there is no grid.
    grids: Vec<usize>,
    /// The grid's entries of `outputs` values, the last input's axis fastest.
    clut: Vec<u16>,
    /// One an output.
    a: Vec<Tone>,
    outputs: usize,
}

impl V4Table {
    /// The `mBA ` tag's bytes.
    fn tag(&self) -> Vec<u8> {
        self.tag_as(b"mBA ", 3)
    }

    /// The tag's bytes under `signature` over `inputs` channels: the header,
    /// then each stage present in the order the header lists their offsets —
    /// B, matrix, M, CLUT, A — each curve and stage on a four-byte boundary
    /// (10.10.1, 10.11.1). An `mAB ` written this way runs its A curves
    /// first and its B curves last.
    fn tag_as(&self, signature: &[u8; 4], inputs: u8) -> Vec<u8> {
        fn pad(out: &mut Vec<u8>) {
            while out.len() % 4 != 0 {
                out.push(0);
            }
        }
        fn run(out: &mut Vec<u8>, tones: &[Tone]) -> u32 {
            let at = out.len() as u32;
            for tone in tones {
                out.extend_from_slice(&tone_tag(tone));
                pad(out);
            }
            at
        }
        let mut out = signature.to_vec();
        out.extend_from_slice(&[0; 4]);
        out.extend_from_slice(&[inputs, self.outputs as u8, 0, 0]);
        out.extend_from_slice(&[0; 20]);
        let mut offsets = [0u32; 5];
        if !self.b.is_empty() {
            offsets[0] = run(&mut out, &self.b);
        }
        if let Some(matrix) = self.matrix {
            offsets[1] = out.len() as u32;
            for value in matrix {
                out.extend_from_slice(&s15(value));
            }
        }
        if !self.m.is_empty() {
            offsets[2] = run(&mut out, &self.m);
        }
        if !self.grids.is_empty() {
            offsets[3] = out.len() as u32;
            let mut counts = [0u8; 16];
            for (slot, points) in counts.iter_mut().zip(&self.grids) {
                *slot = *points as u8;
            }
            out.extend_from_slice(&counts);
            // Two bytes an entry, then three of padding.
            out.extend_from_slice(&[2, 0, 0, 0]);
            for value in &self.clut {
                out.extend_from_slice(&value.to_be_bytes());
            }
            pad(&mut out);
        }
        if !self.a.is_empty() {
            offsets[4] = run(&mut out, &self.a);
        }
        for (slot, offset) in offsets.iter().enumerate() {
            out[12 + 4 * slot..16 + 4 * slot].copy_from_slice(&offset.to_be_bytes());
        }
        out
    }

    /// The stages before the grid for `encoded`, three inputs over 0..=1, in
    /// `order`: each curve read exactly, the matrix as stored, its results
    /// clipped to 0..=1 (10.11.3). A stage the tag does not carry passes its
    /// channels through.
    fn before_grid(&self, encoded: [f64; 3], order: [Stage; 3]) -> [f64; 3] {
        let mut v = encoded.map(|x| x.clamp(0.0, 1.0));
        for stage in order {
            match stage {
                Stage::B => v = [0, 1, 2].map(|c| self.b[c].forward(v[c])),
                Stage::M if !self.m.is_empty() => {
                    v = [0, 1, 2].map(|c| self.m[c].forward(v[c]));
                }
                Stage::M => {}
                Stage::Matrix => {
                    if let Some(matrix) = self.matrix {
                        let e = matrix.map(stored);
                        v = [0, 1, 2].map(|row| {
                            (e[3 * row] * v[0]
                                + e[3 * row + 1] * v[1]
                                + e[3 * row + 2] * v[2]
                                + e[9 + row])
                                .clamp(0.0, 1.0)
                        });
                    }
                }
            }
        }
        v
    }

    /// The grid's cell at `v`, in nodes along each axis, and the fractions
    /// past it.
    fn cell(&self, v: [f64; 3]) -> ([usize; 3], [f64; 3]) {
        let mut base = [0usize; 3];
        let mut fraction = [0.0f64; 3];
        for axis in 0..3 {
            let last = (self.grids[axis] - 1) as f64;
            let p = v[axis].clamp(0.0, 1.0) * last;
            let index = (p.floor() as usize).min(self.grids[axis] - 2);
            base[axis] = index;
            fraction[axis] = p - index as f64;
        }
        (base, fraction)
    }

    /// Grid node `at`'s channel `o`, over 0..=1.
    fn node(&self, at: [usize; 3], o: usize) -> f64 {
        let offset = (at[0] * self.grids[1] + at[1]) * self.grids[2] + at[2];
        f64::from(self.clut[offset * self.outputs + o]) / 65535.0
    }

    /// The table's channels for `encoded`, each over 0..=1, recomputed stage
    /// by stage with the stages before the grid in `order`: the grid
    /// interpolated multilinearly — or, with none, the M stage's channels as
    /// they stand — then each output through its A curve.
    fn evaluate(&self, encoded: [f64; 3], order: [Stage; 3]) -> Vec<f64> {
        let v = self.before_grid(encoded, order);
        let grid: Vec<f64> = if self.grids.is_empty() {
            v.to_vec()
        } else {
            let (base, fraction) = self.cell(v);
            (0..self.outputs)
                .map(|o| {
                    let mut value = 0.0;
                    for corner in 0..8usize {
                        let bits = [(corner >> 2) & 1, (corner >> 1) & 1, corner & 1];
                        let weight: f64 = (0..3)
                            .map(|axis| {
                                if bits[axis] == 1 {
                                    fraction[axis]
                                } else {
                                    1.0 - fraction[axis]
                                }
                            })
                            .product();
                        let at = [0, 1, 2].map(|axis| base[axis] + bits[axis]);
                        value += weight * self.node(at, o);
                    }
                    value
                })
                .collect()
        };
        grid.iter()
            .enumerate()
            .map(|(o, &value)| match self.a.get(o) {
                Some(tone) => tone.forward(value.clamp(0.0, 1.0)),
                None => value,
            })
            .collect()
    }

    /// One table step for channel `o`, in 16-bit units: along each axis, the
    /// most two neighbouring nodes differ by anywhere on the grid, over 256 —
    /// the grid is interpolated in 256ths of a cell — and over the eight units
    /// a coordinate can be off by after the B curves', the matrix's and the M
    /// curves' roundings; that carried through the A curve's steepest
    /// stretch, and four units for the roundings after the grid.
    fn step(&self, o: usize) -> f64 {
        let coordinate = 8.0 / 65535.0;
        let span = if self.grids.is_empty() {
            coordinate
        } else {
            let mut span = 0.0;
            for axis in 0..3 {
                let mut widest = 0.0f64;
                for i in 0..self.grids[0] {
                    for j in 0..self.grids[1] {
                        for k in 0..self.grids[2] {
                            let low = [i, j, k];
                            if low[axis] + 1 < self.grids[axis] {
                                let mut high = low;
                                high[axis] += 1;
                                widest = widest.max((self.node(high, o) - self.node(low, o)).abs());
                            }
                        }
                    }
                }
                span += widest * (1.0 / 256.0 + coordinate * (self.grids[axis] - 1) as f64);
            }
            span
        };
        let slope = self.a.get(o).map_or(1.0, Tone::steepest);
        span * slope * 65535.0 + 4.0
    }
}

/// A grid of `grids` points along its three axes, `outputs` channels a node:
/// channel `c` of node `(i, j, k)` rises along each axis at its own rate, as
/// [`clut`]'s do.
fn grid_entries(grids: [usize; 3], outputs: usize) -> Vec<u16> {
    let mut values = Vec::new();
    let unit = |index: usize, points: usize| index as f64 / (points - 1) as f64;
    for i in 0..grids[0] {
        for j in 0..grids[1] {
            for k in 0..grids[2] {
                for c in 0..outputs {
                    let (x, y, z) = (unit(i, grids[0]), unit(j, grids[1]), unit(k, grids[2]));
                    let weights = [0.5, 0.3, 0.2];
                    let turn = c % 3;
                    let mixed = weights[turn] * x
                        + weights[(turn + 1) % 3] * y * y
                        + weights[(turn + 2) % 3] * (1.0 - z)
                        + 0.05 * c as f64;
                    values.push((mixed.clamp(0.0, 1.0) * 65535.0).round() as u16);
                }
            }
        }
    }
    values
}

/// An `mBA ` with every stage, to four channels: B curves of gamma 2, gamma
/// 1.5 and four sampled points; a matrix mixing every channel with an offset
/// on each; M curves of four points, each bent its own way; a grid of three,
/// four and three points; and A curves of four points, one a channel. A
/// curve before the matrix bends what it mixes and one after it what it made,
/// so no two of the stages before the grid commute. Every sampled curve's
/// points fall on every 341st of the 1 023 steps a v4 curve is sampled in.
fn every_stage() -> V4Table {
    V4Table {
        b: vec![
            Tone::Gamma(2.0),
            Tone::Gamma(1.5),
            sampled(&[0, 30_000, 52_000, 65_535]),
        ],
        matrix: Some([
            0.75, 0.25, 0.0, -0.25, 1.0, 0.25, 0.125, -0.125, 0.875, 0.0625, -0.0625, 0.125,
        ]),
        m: vec![
            sampled(&[0, 8_000, 30_000, 65_535]),
            sampled(&[0, 40_000, 60_000, 65_535]),
            sampled(&[10_000, 20_000, 45_000, 65_535]),
        ],
        grids: vec![3, 4, 3],
        clut: grid_entries([3, 4, 3], 4),
        a: (0..4u16)
            .map(|c| sampled(&[0, 18_000 + 4_000 * c, 47_000 - 3_000 * c, 65_535]))
            .collect(),
        outputs: 4,
    }
}

/// XYZ values over most of the cube `u1Fixed15` encodes, from black to 1.7
/// in each component, past the PCS white.
fn xyz_cube() -> Vec<[f64; 3]> {
    let levels = [0.0, 0.15, 0.4, 0.7, 1.0, 1.3, 1.7];
    let mut values = Vec::new();
    for x in levels {
        for y in levels {
            for z in levels {
                values.push([x, y, z]);
            }
        }
    }
    values
}

/// A profile over XYZ whose `B2A0` is `table`, on [`PAPER`]: CMYK with a
/// forward table for a table of four outputs, RGB on a matrix/TRC model for
/// one of three.
fn mba_profile(table: &V4Table) -> Profile {
    let (space, mut tags) = if table.outputs == 4 {
        let forward = forward_table(true, 4, |_| [20_000, 30_000, 25_000]);
        (b"CMYK", vec![(*b"A2B0", forward.tag())])
    } else {
        (b"RGB ", rgb_model(2.2))
    };
    tags.extend([(*b"B2A0", table.tag()), (*b"wtpt", xyz_tag(PAPER))]);
    Profile::parse(&profile(space, b"XYZ ", &tags)).expect("the profile parses")
}

/// **An `mBA ` tag runs its stages in ICC.1's order** (10.11): B curves, the
/// matrix and its offsets, its results clipped to 0..=1, M curves, the grid,
/// A curves. A CMYK profile's `B2A0` carrying all five, stages whose effects
/// do not commute, takes XYZ in `u1Fixed15` to the four values each stage
/// recomputed here in that order gives, within one table step, under
/// perceptual, relative and absolute colorimetric — the last's value divided
/// by the media white first — and every other order of the three stages
/// before the grid misses by more than a step somewhere. So do the tag's
/// shorter forms, each stage present as its offsets say: B curves alone, B
/// curves with the matrix and M curves, and B curves with the grid and A
/// curves. At an `A2B*` tag an `mBA ` stays refused by name.
#[test]
fn an_mba_tag_runs_its_stages_in_icc_1s_order() {
    let full = every_stage();
    let forms = [
        ("every stage", full.clone()),
        (
            "B curves alone",
            V4Table {
                matrix: None,
                m: Vec::new(),
                grids: Vec::new(),
                clut: Vec::new(),
                a: Vec::new(),
                outputs: 3,
                ..full.clone()
            },
        ),
        (
            "B curves, the matrix and M curves",
            V4Table {
                grids: Vec::new(),
                clut: Vec::new(),
                a: Vec::new(),
                outputs: 3,
                ..full.clone()
            },
        ),
        (
            "B curves, the grid and A curves",
            V4Table {
                matrix: None,
                m: Vec::new(),
                ..full.clone()
            },
        ),
    ];
    let values = xyz_cube();
    for (what, table) in &forms {
        let parsed = mba_profile(table);
        for intent in [
            Intent::Perceptual,
            Intent::RelativeColorimetric,
            Intent::AbsoluteColorimetric,
        ] {
            let output = Output::compile(&parsed, intent)
                .unwrap_or_else(|error| panic!("{what}, {intent:?}: {error:?}"));
            assert_eq!(output.channels(), table.outputs, "{what}");
            for &pcs in &values {
                let encoded = Encoding::Xyz.encode(relative_value(pcs, false, intent), IDENTITY);
                let want = table.evaluate(encoded, ICC_ORDER);
                let got = output.apply(pcs);
                assert_eq!(got.channels().len(), table.outputs, "{what}");
                for (c, (&got, want)) in got.channels().iter().zip(want).enumerate() {
                    let (want, slack) = (want * 65535.0, table.step(c));
                    assert!(
                        (f64::from(got) - want).abs() <= slack,
                        "{what}, {intent:?}, {pcs:?}, channel {c}: {got}, not {want:.1} within \
                         {slack:.1}"
                    );
                }
            }
        }
    }

    // Every other order of the three stages before the grid misses
    // somewhere, so the answers above are the order's and not the stages'.
    let output = Output::compile(&mba_profile(&full), Intent::RelativeColorimetric)
        .expect("the mBA B2A0 reads");
    let got: Vec<Vec<u16>> = values
        .iter()
        .map(|&pcs| output.apply(pcs).channels().to_vec())
        .collect();
    use Stage::{Matrix, B, M};
    for order in [
        [B, M, Matrix],
        [Matrix, B, M],
        [Matrix, M, B],
        [M, B, Matrix],
        [M, Matrix, B],
    ] {
        let missed = values.iter().zip(&got).any(|(&pcs, got)| {
            let want = full.evaluate(Encoding::Xyz.encode(pcs, IDENTITY), order);
            got.iter()
                .zip(want)
                .enumerate()
                .any(|(c, (&got, want))| (f64::from(got) - want * 65535.0).abs() > full.step(c))
        });
        assert!(missed, "{order:?} gives what ICC.1's order does");
    }

    let misplaced = profile(b"CMYK", b"XYZ ", &[(*b"A2B0", full.tag())]);
    assert_eq!(
        Profile::parse(&misplaced),
        Err(IccError::NeedsLut),
        "an mBA at an A2B tag"
    );
}

/// **A v4 table reads and writes Lab in its own encoding** (10.10.1,
/// 10.11.1, 6.3.4.2): each component's range over the whole currency, not
/// `lut16Type`'s legacy scale. Forward, an `mAB ` `A2B0` whose grid's top
/// node holds `FFFFh`, `8080h` and `8080h` gives `L*` 100 and `a*`, `b*` 0,
/// where the legacy reading would give 100.39 and 0.5. Out, an `mBA ` `B2A0`
/// whose three-point grid holds 0, `8000h` and `FFFFh` along each axis takes
/// `L*` 100 to its top node, 65 535, and `a*`, `b*` 0, which lie at `80h`
/// over `FFh` of their axes, just past the middle node, to `8080h`, 32 896;
/// the legacy encoding would take them to about 65 279 and 32 768.
#[test]
fn a_v4_table_reads_and_writes_lab_in_its_own_encoding() {
    let identity = || vec![Tone::Gamma(1.0); 3];
    let forward = V4Table {
        b: identity(),
        matrix: None,
        m: Vec::new(),
        grids: vec![2, 2, 2],
        clut: (0..8)
            .flat_map(|node| [if node == 7 { 0xFFFF } else { 0 }, 0x8080, 0x8080])
            .collect(),
        a: identity(),
        outputs: 3,
    };
    let level = [0u16, 0x8000, 0xFFFF];
    let out = V4Table {
        b: identity(),
        matrix: None,
        m: Vec::new(),
        grids: vec![3, 3, 3],
        clut: (0..27usize)
            .flat_map(|node| [level[node / 9], level[(node / 3) % 3], level[node % 3]])
            .collect(),
        a: identity(),
        outputs: 3,
    };
    let parsed = Profile::parse(&profile(
        b"RGB ",
        b"Lab ",
        &[
            (*b"A2B0", forward.tag_as(b"mAB ", 3)),
            (*b"B2A0", out.tag()),
        ],
    ))
    .expect("an RGB/Lab profile");

    let white = parsed
        .connection_value(Intent::RelativeColorimetric, &[1.0, 1.0, 1.0])
        .expect("a value");
    assert!(
        (white[0] - 100.0).abs() < 0.01 && white[1].abs() < 0.01 && white[2].abs() < 0.01,
        "the top node reads {white:?}, not L* 100 and a*, b* 0"
    );
    let black = parsed
        .connection_value(Intent::RelativeColorimetric, &[0.0, 0.0, 0.0])
        .expect("a value");
    assert!(
        black[0].abs() < 0.01 && black[1].abs() < 0.01 && black[2].abs() < 0.01,
        "the bottom node reads {black:?}"
    );

    let output =
        Output::compile(&parsed, Intent::RelativeColorimetric).expect("the mBA B2A0 reads");
    let got = output.apply([100.0, 0.0, 0.0]);
    let channels = got.channels();
    assert!(
        channels[0] >= 65_534,
        "L* 100 is the top node: {channels:?}"
    );
    for channel in &channels[1..] {
        assert!(
            channel.abs_diff(0x8080) <= 2,
            "a*, b* 0 lie at 80h over FFh: {channels:?}"
        );
    }
}

// ---- device links ------------------------------------------------------------
//
// A device link (ICC.1 8.6) takes one device's values to another's through its
// one `AToB0` — `lut8Type`, `lut16Type` or `lutAToBType` — with no connection
// space at either end: its data space is the space it takes, and its
// connection-space field names the space it gives (7.2.7). Each structure's
// stages run as they do at any `A2B*` tag: input or A curves, the grid, output
// or B curves (10.8, 10.9, 10.10).

/// Which of the three structures a link's `A2B0` is.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Form {
    /// `lut8Type`, `mft1`.
    Lut8,
    /// `lut16Type`, `mft2`.
    Lut16,
    /// `lutAToBType`, `mAB `: A curves, a grid, B curves.
    AToB,
}

/// A link's `A2B0` over `inputs` channels to `outputs`, laid out and
/// recomputed here.
#[derive(Clone, Debug)]
struct LinkTable {
    form: Form,
    inputs: usize,
    outputs: usize,
    /// Grid points along each input axis; `mft1` and `mft2` write one count
    /// for every axis.
    grids: Vec<usize>,
    /// `mft1`'s and `mft2`'s input curves, in the type's own width.
    input_curves: Vec<Vec<u16>>,
    /// `mAB `'s A curves, one a gamma an input, as `para` tags of type 0.
    a_gammas: Vec<f64>,
    /// The grid's entries, `outputs` a node, the last input's axis fastest,
    /// in the type's own width: an `mft1`'s bytes, an `mft2`'s and an
    /// `mAB `'s 16-bit values.
    clut: Vec<u16>,
    /// `mft1`'s and `mft2`'s output curves, in the type's own width.
    output_curves: Vec<Vec<u16>>,
    /// `mAB `'s B curves, one a gamma an output.
    b_gammas: Vec<f64>,
}

impl LinkTable {
    /// The `A2B0` tag's bytes.
    fn tag(&self) -> Vec<u8> {
        match self.form {
            Form::Lut8 | Form::Lut16 => self.v2_tag(),
            Form::AToB => self.mab_tag(),
        }
    }

    /// An `mft1` or `mft2`: the header, the identity matrix 10.8 has a
    /// device input ignore, then the curves and the grid in the type's width.
    fn v2_tag(&self) -> Vec<u8> {
        let wide = self.form == Form::Lut16;
        let mut out = if wide {
            b"mft2".to_vec()
        } else {
            b"mft1".to_vec()
        };
        out.extend_from_slice(&[0; 4]);
        out.extend_from_slice(&[
            self.inputs as u8,
            self.outputs as u8,
            self.grids[0] as u8,
            0,
        ]);
        for value in IDENTITY {
            out.extend_from_slice(&s15(value));
        }
        if wide {
            out.extend_from_slice(&(self.input_curves[0].len() as u16).to_be_bytes());
            out.extend_from_slice(&(self.output_curves[0].len() as u16).to_be_bytes());
        }
        let value = |out: &mut Vec<u8>, v: u16| {
            if wide {
                out.extend_from_slice(&v.to_be_bytes());
            } else {
                out.push(v as u8);
            }
        };
        for curve in &self.input_curves {
            for &v in curve {
                value(&mut out, v);
            }
        }
        for &v in &self.clut {
            value(&mut out, v);
        }
        for curve in &self.output_curves {
            for &v in curve {
                value(&mut out, v);
            }
        }
        out
    }

    /// An `mAB ` of B curves, a 16-bit grid and A curves, the header listing
    /// their offsets B, matrix, M, CLUT, A (10.10.1), each on a four-byte
    /// boundary.
    fn mab_tag(&self) -> Vec<u8> {
        fn pad(out: &mut Vec<u8>) {
            while out.len() % 4 != 0 {
                out.push(0);
            }
        }
        let mut out = b"mAB ".to_vec();
        out.extend_from_slice(&[0; 4]);
        out.extend_from_slice(&[self.inputs as u8, self.outputs as u8, 0, 0]);
        out.extend_from_slice(&[0; 20]);
        let b_at = out.len() as u32;
        for gamma in &self.b_gammas {
            out.extend_from_slice(&para_tag(0, &[*gamma]));
            pad(&mut out);
        }
        let clut_at = out.len() as u32;
        let mut counts = [0u8; 16];
        for (slot, points) in counts.iter_mut().zip(&self.grids) {
            *slot = *points as u8;
        }
        out.extend_from_slice(&counts);
        out.extend_from_slice(&[2, 0, 0, 0]);
        for value in &self.clut {
            out.extend_from_slice(&value.to_be_bytes());
        }
        pad(&mut out);
        let a_at = out.len() as u32;
        for gamma in &self.a_gammas {
            out.extend_from_slice(&para_tag(0, &[*gamma]));
            pad(&mut out);
        }
        for (slot, offset) in [(0, b_at), (3, clut_at), (4, a_at)] {
            out[12 + 4 * slot..16 + 4 * slot].copy_from_slice(&offset.to_be_bytes());
        }
        out
    }

    /// A value of the type's width as the 16-bit currency over 0..=1: an
    /// `mft1`'s byte over 255.
    fn unit(&self, v: u16) -> f64 {
        if self.form == Form::Lut8 {
            f64::from(v) / 255.0
        } else {
            f64::from(v) / 65535.0
        }
    }

    /// A sampled curve at `x`, read linearly between its entries.
    fn curve(&self, curve: &[u16], x: f64) -> f64 {
        let last = (curve.len() - 1) as f64;
        let position = x.clamp(0.0, 1.0) * last;
        let index = (position.floor() as usize).min(curve.len() - 2);
        let fraction = position - index as f64;
        let (low, high) = (self.unit(curve[index]), self.unit(curve[index + 1]));
        low + (high - low) * fraction
    }

    /// Grid node `at`'s channel `c`, over 0..=1.
    fn node(&self, at: &[usize], c: usize) -> f64 {
        let offset = at
            .iter()
            .zip(&self.grids)
            .fold(0usize, |offset, (&index, &points)| offset * points + index);
        self.unit(self.clut[offset * self.outputs + c])
    }

    /// The link's channels for device values `x`, each over 0..=1,
    /// recomputed stage by stage: each input through its curve — a table's
    /// read linearly, an A curve's gamma as stored — the grid interpolated
    /// multilinearly over every input's axis, each output through its curve.
    fn evaluate(&self, x: &[f64]) -> Vec<f64> {
        let mut base = vec![0usize; self.inputs];
        let mut fraction = vec![0.0f64; self.inputs];
        for axis in 0..self.inputs {
            let v = x[axis].clamp(0.0, 1.0);
            let coordinate = match self.form {
                Form::AToB => v.powf(stored(self.a_gammas[axis])),
                _ => self.curve(&self.input_curves[axis], v),
            };
            let last = (self.grids[axis] - 1) as f64;
            let p = coordinate * last;
            let index = (p.floor() as usize).min(self.grids[axis] - 2);
            base[axis] = index;
            fraction[axis] = p - index as f64;
        }
        (0..self.outputs)
            .map(|c| {
                let mut value = 0.0;
                for corner in 0..1usize << self.inputs {
                    let mut weight = 1.0;
                    let mut at = base.clone();
                    for axis in 0..self.inputs {
                        if (corner >> (self.inputs - 1 - axis)) & 1 == 1 {
                            weight *= fraction[axis];
                            at[axis] += 1;
                        } else {
                            weight *= 1.0 - fraction[axis];
                        }
                    }
                    value += weight * self.node(&at, c);
                }
                match self.form {
                    Form::AToB => value.clamp(0.0, 1.0).powf(stored(self.b_gammas[c])),
                    _ => self.curve(&self.output_curves[c], value),
                }
            })
            .collect()
    }

    /// One table step for channel `c`, in 16-bit units: along each axis, the
    /// most two neighbouring nodes differ by anywhere on the grid, over the
    /// 256ths of a cell the grid is interpolated in and the units a
    /// coordinate can be off by — one for a table curve's rounding, eight for
    /// an A curve sampled into 1 024 entries — carried through the output
    /// curve's steepest stretch, and four units for the roundings after the
    /// grid.
    fn step(&self, c: usize) -> f64 {
        let coordinate = if self.form == Form::AToB { 8.0 } else { 1.0 } / 65535.0;
        let nodes: usize = self.grids.iter().product();
        let mut span = 0.0;
        for axis in 0..self.inputs {
            let mut widest = 0.0f64;
            for index in 0..nodes {
                let mut at = vec![0usize; self.inputs];
                let mut rest = index;
                for a in (0..self.inputs).rev() {
                    at[a] = rest % self.grids[a];
                    rest /= self.grids[a];
                }
                if at[axis] + 1 < self.grids[axis] {
                    let mut high = at.clone();
                    high[axis] += 1;
                    widest = widest.max((self.node(&high, c) - self.node(&at, c)).abs());
                }
            }
            span += widest * (1.0 / 256.0 + coordinate * (self.grids[axis] - 1) as f64);
        }
        let slope = match self.form {
            Form::AToB => stored(self.b_gammas[c]).max(1.0),
            _ => {
                let curve = &self.output_curves[c];
                curve
                    .windows(2)
                    .map(|pair| (self.unit(pair[1]) - self.unit(pair[0])).abs())
                    .fold(0.0f64, f64::max)
                    * (curve.len() - 1) as f64
            }
        };
        span * slope * 65535.0 + 4.0
    }
}

/// A link's `A2B0` of `form` over `inputs` channels to `outputs`: a grid of
/// three points an axis — an `mAB `'s of two to four, its own on each axis —
/// whose channel `c` rises along each axis by a curve of its own, between
/// input curves bent their own way and output curves of their own.
fn link_table(form: Form, inputs: usize, outputs: usize) -> LinkTable {
    let grids: Vec<usize> = match form {
        Form::AToB => (0..inputs).map(|axis| [3, 2, 4, 3][axis % 4]).collect(),
        _ => vec![3; inputs],
    };
    let top = if form == Form::Lut8 { 255.0 } else { 65535.0 };
    let nodes: usize = grids.iter().product();
    let mut clut = Vec::with_capacity(nodes * outputs);
    for index in 0..nodes {
        let mut at = vec![0usize; inputs];
        let mut rest = index;
        for axis in (0..inputs).rev() {
            at[axis] = rest % grids[axis];
            rest /= grids[axis];
        }
        for c in 0..outputs {
            let mut mixed = 0.03 * c as f64;
            for axis in 0..inputs {
                let u = at[axis] as f64 / (grids[axis] - 1) as f64;
                let shaped = match (axis + c) % 3 {
                    0 => u,
                    1 => u * u,
                    _ => 1.0 - u,
                };
                mixed += shaped / inputs as f64 * (0.6 + 0.1 * ((axis + 2 * c) % 4) as f64);
            }
            clut.push((mixed.clamp(0.0, 1.0) * top).round() as u16);
        }
    }
    let (input_curves, output_curves) = match form {
        Form::Lut16 => (
            (0..inputs)
                .map(|axis| vec![0, 20_000 + 5_000 * axis as u16, 65_535])
                .collect(),
            (0..outputs)
                .map(|c| vec![1_000 * c as u16, 22_000, 47_000, 64_000 + 300 * c as u16])
                .collect(),
        ),
        Form::Lut8 => (
            (0..inputs)
                .map(|axis| byte_curve(0.8 + 0.2 * axis as f64))
                .collect(),
            (0..outputs)
                .map(|c| byte_curve(1.0 + 0.1 * c as f64))
                .collect(),
        ),
        Form::AToB => (Vec::new(), Vec::new()),
    };
    LinkTable {
        form,
        inputs,
        outputs,
        grids,
        input_curves,
        a_gammas: (0..inputs)
            .map(|axis| [1.0, 1.4, 1.8, 1.2][axis % 4])
            .collect(),
        clut,
        output_curves,
        b_gammas: (0..outputs).map(|c| [1.0, 1.3, 2.0, 1.6][c % 4]).collect(),
    }
}

/// Device values over `inputs` channels: each at both ends of its axis, on
/// grid nodes and between them.
fn device_lattice(inputs: usize) -> Vec<Vec<f64>> {
    let levels: &[f64] = if inputs > 3 {
        &[0.0, 0.2, 0.5, 0.65, 1.0]
    } else {
        &[0.0, 0.1, 0.35, 0.5, 0.72, 0.9, 1.0]
    };
    let mut out = vec![Vec::new()];
    for _ in 0..inputs {
        out = out
            .into_iter()
            .flat_map(|prefix: Vec<f64>| {
                levels.iter().map(move |&level| {
                    let mut next = prefix.clone();
                    next.push(level);
                    next
                })
            })
            .collect();
    }
    out
}

/// **A device link maps device values through its table.** A CMYK-to-CMYK
/// and an RGB-to-CMYK link, each with its `A2B0` in all three structures a
/// link's `AToB0` takes — `mft1`, `mft2` and `mAB ` (ICC.1 8.6) — takes each
/// device value of a lattice over its inputs to the four ink values each
/// stage recomputed here gives: input or A curves, the grid interpolated
/// multilinearly over every input's axis, output or B curves, with no
/// connection-space encoding at either end; within one table step. Values
/// past either end of the input device's axis are held there.
#[test]
fn a_device_link_maps_device_values_through_its_table() {
    for form in [Form::Lut8, Form::Lut16, Form::AToB] {
        for (space, inputs) in [(b"CMYK", 4usize), (b"RGB ", 3)] {
            let table = link_table(form, inputs, 4);
            let bytes = profile_of(b"link", space, b"CMYK", &[(*b"A2B0", table.tag())]);
            let what = format!("{form:?}, {inputs} inputs");
            let parsed = Profile::parse(&bytes).unwrap_or_else(|error| panic!("{what}: {error:?}"));
            let link = Link::compile(&parsed).unwrap_or_else(|error| panic!("{what}: {error:?}"));
            assert_eq!((link.inputs(), link.channels()), (inputs, 4), "{what}");
            for x in device_lattice(inputs) {
                let got = link.apply(&x);
                assert_eq!(got.channels().len(), 4, "{what}");
                for (c, want) in table.evaluate(&x).into_iter().enumerate() {
                    let (got, want, slack) =
                        (f64::from(got.channels()[c]), want * 65535.0, table.step(c));
                    assert!(
                        (got - want).abs() <= slack,
                        "{what}, {x:?}, channel {c}: {got}, not {want:.1} within {slack:.1}"
                    );
                }
            }
            assert_eq!(
                link.apply(&vec![-0.5; inputs]),
                link.apply(&vec![0.0; inputs]),
                "{what}: below the axis is held at its foot"
            );
            assert_eq!(
                link.apply(&vec![1.5; inputs]),
                link.apply(&vec![1.0; inputs]),
                "{what}: past it, at its top"
            );
        }
    }
}

/// **A link names its output space in the connection-space field** (ICC.1
/// 7.2.7, 8.6): a profile of class `link` whose field names `CMYK`, a
/// six-colour `6CLR` or `GRAY` parses, and its link gives that space's
/// channels — four, six, one — from its data space's; the same bytes in any
/// other class are refused, a device space being no connection space. A link
/// builds no transform to sRGB, under any intent, nor one out of the
/// connection space, and a profile of another class carries no link. A
/// table whose output count is not the field's space's is refused, as one
/// whose input count is not the data space's is.
#[test]
fn a_link_names_its_output_space_in_the_connection_space_field() {
    for (field, outputs) in [(b"CMYK", 4usize), (b"6CLR", 6), (b"GRAY", 1)] {
        let table = link_table(Form::Lut16, 3, outputs);
        let tags = [(*b"A2B0", table.tag())];
        let parsed = Profile::parse(&profile_of(b"link", b"RGB ", field, &tags))
            .unwrap_or_else(|error| panic!("{field:?}: {error:?}"));
        assert_eq!(parsed.pcs, *field);
        assert_eq!(
            parsed.channels(),
            Some(3),
            "the data space is the link's input"
        );
        let link = Link::compile(&parsed).expect("a link");
        assert_eq!(link.output_space(), *field);
        assert_eq!((link.inputs(), link.channels()), (3, outputs));
        assert_eq!(link.apply(&[0.3, 0.6, 0.9]).channels().len(), outputs);
        for intent in Intent::ALL {
            assert!(
                Transform::compile(&parsed, intent).is_none(),
                "{field:?}, {intent:?}: a link builds no transform to sRGB"
            );
            assert_eq!(
                Output::compile(&parsed, intent),
                Err(IccError::MissingTags),
                "{field:?}, {intent:?}: nor one out of the connection space"
            );
        }
        assert!(Transforms::compile(parsed).is_none());

        for class in [b"prtr", b"mntr", b"scnr", b"spac"] {
            assert_eq!(
                Profile::parse(&profile_of(class, b"RGB ", field, &tags)),
                Err(IccError::UnsupportedPcs),
                "{class:?} naming {field:?} in its connection-space field"
            );
        }
    }

    let (bytes, _) = matrix_profile();
    let display = Profile::parse(&bytes).expect("a display profile");
    assert_eq!(Link::compile(&display), Err(IccError::MissingTags));

    let four = link_table(Form::Lut16, 3, 4).tag();
    assert_eq!(
        Profile::parse(&profile_of(
            b"link",
            b"RGB ",
            b"6CLR",
            &[(*b"A2B0", four.clone())]
        )),
        Err(IccError::UnsupportedSpace),
        "four outputs for a six-colour space"
    );
    assert_eq!(
        Profile::parse(&profile_of(b"link", b"CMYK", b"CMYK", &[(*b"A2B0", four)])),
        Err(IccError::UnsupportedSpace),
        "three inputs for a four-channel data space"
    );
}

// ---- the gamut tag -------------------------------------------------------------
//
// `gamt` (ICC.1 9.2.28) is a table from the connection space to one channel,
// in `lut8Type`, `lut16Type` or `lutBToAType`: an output of zero is in gamut
// and any other value out. The table interpolates, so a value is out of gamut
// where its output, in the table's own precision, is half a step or more.

/// The structures a gamut tag takes, with the bits its grid holds a value in.
#[derive(Clone, Copy, Debug, PartialEq)]
enum GamutForm {
    /// `lut8Type`.
    Lut8,
    /// `lut16Type`.
    Lut16,
    /// `lutBToAType` whose grid holds a byte an entry.
    BToA8,
    /// `lutBToAType` whose grid holds two bytes an entry.
    BToA16,
}

impl GamutForm {
    /// The most the grid's entries hold: 255 for a byte, 65 535 for two.
    fn top(self) -> u16 {
        match self {
            GamutForm::Lut8 | GamutForm::BToA8 => 255,
            GamutForm::Lut16 | GamutForm::BToA16 => 65_535,
        }
    }
}

/// Grid points along each axis of the gamut tables here.
const GAMUT_GRID: usize = 5;

/// Whether node `(i, j, k)` of a [`GAMUT_GRID`]-point grid is in the gamut:
/// the cube of nodes `1..=3` on every axis, a quarter of the way in from each
/// end of the encoded connection space.
fn in_the_box(i: usize, j: usize, k: usize) -> bool {
    [i, j, k].iter().all(|index| (1..=3).contains(index))
}

/// A `gamt` tag of `form` over the connection space's three channels to one,
/// identity curves either side of a [`GAMUT_GRID`]-point grid whose nodes in
/// the box are zero and whose others are the grid's top.
fn gamut_tag(form: GamutForm) -> Vec<u8> {
    let top = form.top();
    let mut nodes = Vec::new();
    for i in 0..GAMUT_GRID {
        for j in 0..GAMUT_GRID {
            for k in 0..GAMUT_GRID {
                nodes.push(if in_the_box(i, j, k) { 0 } else { top });
            }
        }
    }
    match form {
        GamutForm::Lut8 | GamutForm::Lut16 => {
            let wide = form == GamutForm::Lut16;
            V2Table {
                wide,
                inputs: 3,
                outputs: 1,
                grid: GAMUT_GRID,
                matrix: IDENTITY,
                input_curves: vec![
                    if wide {
                        vec![0, 65_535]
                    } else {
                        byte_curve(1.0)
                    };
                    3
                ],
                clut: nodes,
                output_curves: vec![if wide {
                    vec![0, 65_535]
                } else {
                    byte_curve(1.0)
                }],
            }
            .tag()
        }
        GamutForm::BToA8 | GamutForm::BToA16 => {
            fn pad(out: &mut Vec<u8>) {
                while out.len() % 4 != 0 {
                    out.push(0);
                }
            }
            let identity = || {
                let mut curve = b"curv".to_vec();
                curve.extend_from_slice(&[0; 8]);
                curve
            };
            let mut out = b"mBA ".to_vec();
            out.extend_from_slice(&[0; 4]);
            out.extend_from_slice(&[3, 1, 0, 0]);
            out.extend_from_slice(&[0; 20]);
            let b_at = out.len() as u32;
            for _ in 0..3 {
                out.extend_from_slice(&identity());
            }
            let clut_at = out.len() as u32;
            let mut counts = [0u8; 16];
            counts[..3].fill(GAMUT_GRID as u8);
            out.extend_from_slice(&counts);
            let bytes = if form == GamutForm::BToA8 { 1 } else { 2 };
            out.extend_from_slice(&[bytes, 0, 0, 0]);
            for node in &nodes {
                if bytes == 1 {
                    out.push(*node as u8);
                } else {
                    out.extend_from_slice(&node.to_be_bytes());
                }
            }
            pad(&mut out);
            for (slot, offset) in [(0, b_at), (3, clut_at)] {
                out[12 + 4 * slot..16 + 4 * slot].copy_from_slice(&offset.to_be_bytes());
            }
            out
        }
    }
}

/// The table's output at `u`, a point of the encoded connection space's cube,
/// over 0..=1: the box's grid interpolated multilinearly, the identity curves
/// either side changing nothing.
fn gamut_value(form: GamutForm, u: [f64; 3]) -> f64 {
    let last = (GAMUT_GRID - 1) as f64;
    let mut base = [0usize; 3];
    let mut fraction = [0.0f64; 3];
    for axis in 0..3 {
        let p = u[axis].clamp(0.0, 1.0) * last;
        let index = (p.floor() as usize).min(GAMUT_GRID - 2);
        base[axis] = index;
        fraction[axis] = p - index as f64;
    }
    let mut value = 0.0;
    for corner in 0..8usize {
        let bits = [(corner >> 2) & 1, (corner >> 1) & 1, corner & 1];
        let weight: f64 = (0..3)
            .map(|axis| {
                if bits[axis] == 1 {
                    fraction[axis]
                } else {
                    1.0 - fraction[axis]
                }
            })
            .product();
        let [i, j, k] = [0, 1, 2].map(|axis| base[axis] + bits[axis]);
        if !in_the_box(i, j, k) {
            value += weight * f64::from(form.top());
        }
    }
    value / f64::from(form.top())
}

/// **A gamut tag answers in and out where its table says** (ICC.1 9.2.28).
/// An RGB profile over XYZ carries a `gamt` whose five-point grid is zero in
/// a box of nodes and its top elsewhere, in each structure the tag takes —
/// `mft1`, `mft2` and an `mBA ` whose grid holds one byte an entry or two.
/// At points inside the box, on its faces and corners, a quarter of a cell
/// past its faces, and far outside it, the transform out of the connection
/// space answers in gamut exactly where the table's output recomputed here is
/// below half a step of its own precision — 1/255 for the one-byte grids,
/// 1/65 535 for the others — the value encoded in `u1Fixed15` on the way in.
/// Under absolute colorimetric the value is divided by the media white over
/// the PCS white first, so a value on the paper's white answers as its
/// relative value does. A profile with no `gamt` answers none rather than in,
/// and so does one whose `gamt` does not read, its transform compiled all the
/// same.
#[test]
fn a_gamut_tag_answers_in_and_out_where_its_table_says() {
    let quarter = 0.25 / (GAMUT_GRID - 1) as f64;
    let points: Vec<([f64; 3], &str)> = vec![
        ([0.5, 0.5, 0.5], "inside the box"),
        ([0.375, 0.625, 0.4], "inside the box, between its nodes"),
        ([0.25, 0.5, 0.5], "on a face"),
        ([0.75, 0.75, 0.75], "on a corner"),
        ([0.25, 0.25, 0.25], "on the opposite corner"),
        ([0.25 - quarter, 0.5, 0.5], "a quarter cell past a face"),
        ([0.75, 0.75, 0.75 + quarter], "a quarter cell past a corner"),
        (
            [0.5, 0.75 + quarter, 0.5],
            "a quarter cell past another face",
        ),
        ([0.9, 0.1, 0.9], "far outside"),
        ([0.0, 0.0, 0.0], "black"),
        ([1.0, 1.0, 1.0], "the top of the cube"),
    ];
    let scale = media_scale();
    for form in [
        GamutForm::Lut8,
        GamutForm::Lut16,
        GamutForm::BToA8,
        GamutForm::BToA16,
    ] {
        let mut tags = rgb_model(2.2);
        tags.extend([(*b"gamt", gamut_tag(form)), (*b"wtpt", xyz_tag(PAPER))]);
        let parsed = Profile::parse(&profile(b"RGB ", b"XYZ ", &tags)).expect("the profile parses");
        let relative = Output::compile(&parsed, Intent::RelativeColorimetric).expect("the model");
        let absolute = Output::compile(&parsed, Intent::AbsoluteColorimetric).expect("the model");
        let step = 1.0 / f64::from(form.top());
        for (u, what) in &points {
            let want = gamut_value(form, *u) < step / 2.0;
            // `u1Fixed15`: 1.0 of the connection space at 8000h of 65 535.
            let xyz = u.map(|v| v * 65535.0 / 32768.0);
            assert_eq!(
                relative.in_gamut(xyz),
                Some(want),
                "{form:?}, {what} ({u:?}): the table's output is {}",
                gamut_value(form, *u)
            );
            let on_paper = [0, 1, 2].map(|k| xyz[k] * scale[k]);
            assert_eq!(
                absolute.in_gamut(on_paper),
                Some(want),
                "{form:?}, {what}, under absolute colorimetric"
            );
        }
        // The box and its outside are both reached, so the answers above
        // are the table's and not one value throughout.
        let some_in = points.iter().any(|(u, _)| gamut_value(form, *u) == 0.0);
        let some_out = points.iter().any(|(u, _)| gamut_value(form, *u) > 0.2);
        assert!(some_in && some_out, "{form:?}: the points reach both sides");
    }

    // No `gamt`: none, never in.
    let plain = Profile::parse(&profile(b"RGB ", b"XYZ ", &rgb_model(2.2))).expect("a profile");
    let output = Output::compile(&plain, Intent::RelativeColorimetric).expect("the model");
    assert_eq!(output.in_gamut([0.5, 0.5, 0.4]), None);

    // A `gamt` that does not read — three outputs where it gives one — is
    // none as well, and refuses nothing.
    let mut tags = rgb_model(2.2);
    tags.push((*b"gamt", lut16(3).tag()));
    let broken = Profile::parse(&profile(b"RGB ", b"XYZ ", &tags)).expect("a profile");
    let output = Output::compile(&broken, Intent::RelativeColorimetric).expect("the model");
    assert_eq!(output.in_gamut([0.5, 0.5, 0.4]), None);
}

// ---- named colours -------------------------------------------------------------
//
// `ncl2` (ICC.1 10.15, Table 63): a vendor flag, the colour count `n`, the
// device coordinate count `m`, a 32-byte prefix and suffix, then `n` entries
// of a 32-byte root name, three connection-space coordinates in `lut16Type`'s
// encoding and `m` device coordinates. A colour's name is prefix, root and
// suffix joined.

/// An `ncl2` tag naming `entries` — each a root name, its three
/// connection-space coordinates and its `device` device coordinates — with
/// `prefix` and `suffix`, its vendor flag zero.
fn ncl2(
    prefix: &str,
    suffix: &str,
    device: usize,
    entries: &[(&str, [u16; 3], Vec<u16>)],
) -> Vec<u8> {
    let field = |text: &str| {
        let mut out = text.as_bytes().to_vec();
        out.resize(32, 0);
        out
    };
    let mut out = b"ncl2".to_vec();
    out.extend_from_slice(&[0; 8]);
    out.extend_from_slice(&(entries.len() as u32).to_be_bytes());
    out.extend_from_slice(&(device as u32).to_be_bytes());
    out.extend_from_slice(&field(prefix));
    out.extend_from_slice(&field(suffix));
    for (root, pcs, coordinates) in entries {
        assert_eq!(coordinates.len(), device);
        out.extend_from_slice(&field(root));
        for value in pcs.iter().chain(coordinates) {
            out.extend_from_slice(&value.to_be_bytes());
        }
    }
    out
}

/// Three coordinates in `lut16Type`'s encoding as the value they encode: XYZ
/// in `u1Fixed15`, 1.0 at `8000h`; Lab in version 2's legacy encoding, `L*`
/// 100 at `FF00h`, `a*` and `b*` 0 at `8000h` (Table 63, 10.8).
fn legacy(encoded: [u16; 3], lab: bool) -> [f64; 3] {
    if lab {
        let unit = encoded.map(|v| f64::from(v) / 65280.0);
        [
            unit[0] * 100.0,
            unit[1] * 255.0 - 128.0,
            unit[2] * 255.0 - 128.0,
        ]
    } else {
        encoded.map(|v| f64::from(v) / 32768.0)
    }
}

/// The ink library the tests here name: three spot colours over CMYK, each
/// with its four device coordinates.
fn inks() -> Vec<(&'static str, [u16; 3], Vec<u16>)> {
    vec![
        ("185", [0x7A00, 0xD200, 0xA600], vec![0, 0xF5C2, 0xC28F, 0]),
        (
            "Process Blue",
            [0x5F00, 0x6E00, 0x2800],
            vec![0xFFFF, 0x2E14, 0, 0x051E],
        ),
        (
            "Cool Gray 9",
            [0x8A00, 0x7F80, 0x7C00],
            vec![0x3333, 0x2666, 0x199A, 0x570A],
        ),
    ]
}

/// **A named colour answers its coordinates by name** (ICC.1 10.15, 8.9). A
/// profile of class `nmcl` over CMYK and Lab, its `ncl2` naming three inks
/// with the prefix `PMS ` and the suffix ` C`, parses with its colours and no
/// transform: each colour is found by its whole name — prefix, root and
/// suffix — and answers the connection-space coordinates its entry encodes,
/// in the legacy Lab encoding, and its four device coordinates as stored;
/// a root alone, or a name missing its suffix, finds nothing. Under an XYZ
/// connection space the coordinates read in `u1Fixed15`. A printer profile
/// carrying the tag beside its tables keeps both. A named-colour profile
/// with no `ncl2`, one whose `ncl2` does not read, and one whose connection
/// space is neither `XYZ` nor `Lab` are refused.
#[test]
fn a_named_colour_answers_its_coordinates_by_name() {
    let library = inks();
    let tag = ncl2("PMS ", " C", 4, &library);
    for (pcs, lab) in [(b"Lab ", true), (b"XYZ ", false)] {
        let parsed = Profile::parse(&profile_of(
            b"nmcl",
            b"CMYK",
            pcs,
            &[(*b"ncl2", tag.clone())],
        ))
        .unwrap_or_else(|error| panic!("{pcs:?}: {error:?}"));
        let named = parsed.named.as_ref().expect("its colours");
        assert_eq!((named.len(), named.device_channels()), (3, 4));
        assert_eq!((named.prefix(), named.suffix()), ("PMS ", " C"));
        for (root, encoded, device) in &library {
            let name = format!("PMS {root} C");
            let colour = named
                .get(&name)
                .unwrap_or_else(|| panic!("{name} is named"));
            assert_eq!(colour.name(), name);
            assert_eq!(colour.lab(), lab);
            assert_eq!(colour.pcs(), legacy(*encoded, lab), "{name}");
            assert_eq!(colour.device(), device.as_slice(), "{name}");
            assert!(named.get(root).is_none(), "{root} alone");
            assert!(
                named.get(&format!("PMS {root}")).is_none(),
                "{root} with no suffix"
            );
        }
        for intent in Intent::ALL {
            assert!(Transform::compile(&parsed, intent).is_none(), "{intent:?}");
            assert_eq!(
                Output::compile(&parsed, intent),
                Err(IccError::MissingTags),
                "{intent:?}"
            );
        }
        assert!(Transforms::compile(parsed.clone()).is_none());
    }

    // Beside a printer profile's tables, both.
    let forward = forward_table(true, 4, |_| [32_768, 32_768, 32_768]);
    let printer = Profile::parse(&profile_of(
        b"prtr",
        b"CMYK",
        b"Lab ",
        &[(*b"A2B0", forward.tag()), (*b"ncl2", tag.clone())],
    ))
    .expect("a printer profile");
    assert!(Transform::compile(&printer, Intent::RelativeColorimetric).is_some());
    assert_eq!(printer.named.as_ref().map(|named| named.len()), Some(3));

    let mut broken = tag.clone();
    broken.truncate(tag.len() - 2);
    for (tags, pcs, expected, what) in [
        (Vec::new(), b"Lab ", IccError::MissingTags, "no ncl2"),
        (
            vec![(*b"ncl2", broken)],
            b"Lab ",
            IccError::MalformedTag,
            "entries cut short",
        ),
        (
            vec![(*b"ncl2", tag)],
            b"CMYK",
            IccError::UnsupportedPcs,
            "a device connection space",
        ),
    ] {
        assert_eq!(
            Profile::parse(&profile_of(b"nmcl", b"CMYK", pcs, &tags)),
            Err(expected),
            "{what}"
        );
    }
}

/// The D50 white the connection space's Lab is relative to (6.3.4.3, 6.4),
/// as the engine's Lab relation reads it.
const LAB_WHITE: [f64; 3] = [0.964_212, 1.0, 0.825_188];

/// D50 XYZ to linear sRGB: IEC 61966-2-1's primaries and D65 white, adapted
/// to D50 by the Bradford transform — the matrix every connection-space value
/// reaches sRGB through.
const D50_TO_SRGB: [[f64; 3]; 3] = [
    [3.134_136, -1.617_036, -0.490_662],
    [-0.978_755, 1.916_143, 0.033_454],
    [0.071_95, -0.228_988, 1.405_386],
];

/// IEC 61966-2-1's encoding of linear light, held to 0..=1, as a byte.
fn srgb_byte(linear: f64) -> i32 {
    let v = linear.clamp(0.0, 1.0);
    let encoded = if v <= 0.003_130_8 {
        12.92 * v
    } else {
        1.055 * v.powf(1.0 / 2.4) - 0.055
    };
    (encoded * 255.0).round() as i32
}

/// **A named colour converts through its connection space.** Each ink of
/// the library, under a Lab and an XYZ connection space, is the sRGB the test
/// recomputes from its stored coordinates: decoded from `lut16Type`'s
/// encoding, Lab taken to D50 XYZ by 6.4's relation against the white the
/// engine's Lab is relative to, XYZ through the D50-adapted sRGB matrix, and
/// IEC 61966-2-1's transfer; within one level in every channel, at eight bits
/// and at sixteen, whose top byte is the eight-bit answer.
#[test]
fn a_named_colour_converts_through_its_connection_space() {
    let library = inks();
    let tag = ncl2("", "", 4, &library);
    for (pcs, lab) in [(b"Lab ", true), (b"XYZ ", false)] {
        let parsed = Profile::parse(&profile_of(
            b"nmcl",
            b"CMYK",
            pcs,
            &[(*b"ncl2", tag.clone())],
        ))
        .expect("the library parses");
        let named = parsed.named.as_ref().expect("its colours");
        for (root, encoded, _) in &library {
            let value = legacy(*encoded, lab);
            let xyz = if lab {
                let fy = (value[0] + 16.0) / 116.0;
                let finv = |t: f64| {
                    const DELTA: f64 = 6.0 / 29.0;
                    if t > DELTA {
                        t * t * t
                    } else {
                        3.0 * DELTA * DELTA * (t - 4.0 / 29.0)
                    }
                };
                [
                    LAB_WHITE[0] * finv(fy + value[1] / 500.0),
                    LAB_WHITE[1] * finv(fy),
                    LAB_WHITE[2] * finv(fy - value[2] / 200.0),
                ]
            } else {
                value
            };
            let want: Vec<i32> = D50_TO_SRGB
                .iter()
                .map(|row| srgb_byte(row[0] * xyz[0] + row[1] * xyz[1] + row[2] * xyz[2]))
                .collect();
            let colour = named.get(root).expect("named by its root");
            let (r, g, b) = colour.to_rgb();
            for (got, want) in [r, g, b].iter().zip(&want) {
                assert!(
                    (i32::from(*got) - want).abs() <= 1,
                    "{pcs:?} {root}: {:?} against {want:?}",
                    (r, g, b)
                );
            }
            let wide = colour.to_rgb16();
            assert_eq!(
                wide.map(|channel| (channel >> 8) as u8),
                [r, g, b],
                "{pcs:?} {root}: sixteen bits top out at the bytes"
            );
        }
    }
}

// ---- multi-process element pipelines -------------------------------------------
//
// A `multiProcessElementsType` (ICC.1 10.14, ICC.2 10.2.17 and clause 11) is a
// chain of elements in floating point: curve sets of segmented curves — ICC.1's
// formula types 0 to 2, ICC.2's 3 to 7 (Table 111), sampled segments — and
// ICC.2's single sampled curves, matrices with offsets, and grids of float32
// values or, ICC.2's extended grid, of half floats or unsigned integers. Each
// element hands its channels to the next unclipped; a grid holds its inputs to
// 0..=1. Here each pipeline is laid out byte by byte and recomputed in `f64`
// from the stored numbers.

/// `value` as the float32 an element stores it in.
fn f32_bytes(value: f64) -> [u8; 4] {
    (value as f32).to_bits().to_be_bytes()
}

/// What a float32 field holding `value` reads back as.
fn as_f32(value: f64) -> f64 {
    f64::from(value as f32)
}

/// A curve, as a curve set stores it and as the test recomputes it.
#[derive(Clone, Debug)]
enum OneCurve {
    /// A segmented curve: break-points and a segment either side of each.
    Segments {
        breaks: Vec<f64>,
        segments: Vec<Piece>,
    },
    /// ICC.2's single sampled curve: `values` evenly spaced from `first` to
    /// `last`, clipped or extended beyond them, in `encoding` (Table 8).
    Single {
        first: f64,
        last: f64,
        extrapolate: bool,
        encoding: u16,
        values: Vec<f64>,
    },
}

/// One segment of a segmented curve.
#[derive(Clone, Debug)]
enum Piece {
    /// A formula of function type `function`, its parameters as the tables
    /// list them.
    Formula { function: u16, params: Vec<f64> },
    /// Samples evenly spaced over the segment, its start excluded.
    Samples(Vec<f64>),
}

/// An `f64` held to what a value of `encoding` (Table 8) can hold: a float32,
/// a half float — the values here are all exact in one — or a fraction in 16
/// or 8 bits.
fn encoded(value: f64, encoding: u16) -> f64 {
    match encoding {
        0 => as_f32(value),
        1 => value,
        2 => (value.clamp(0.0, 1.0) * 65535.0).round() / 65535.0,
        _ => (value.clamp(0.0, 1.0) * 255.0).round() / 255.0,
    }
}

/// The bytes of `value` in `encoding`: a half float only for the values the
/// tests here store, each exact in one.
fn encoding_bytes(value: f64, encoding: u16) -> Vec<u8> {
    match encoding {
        0 => f32_bytes(value).to_vec(),
        1 => {
            let half: u16 = match value {
                v if v == 0.0 => 0x0000,
                v if v == 0.125 => 0x3000,
                v if v == 0.25 => 0x3400,
                v if v == 0.375 => 0x3600,
                v if v == 0.5 => 0x3800,
                v if v == 0.625 => 0x3900,
                v if v == 0.75 => 0x3A00,
                v if v == 0.875 => 0x3B00,
                v if v == 1.0 => 0x3C00,
                v => panic!("{v} is not one of the half floats the tests here use"),
            };
            half.to_be_bytes().to_vec()
        }
        2 => ((value.clamp(0.0, 1.0) * 65535.0).round() as u16)
            .to_be_bytes()
            .to_vec(),
        _ => vec![(value.clamp(0.0, 1.0) * 255.0).round() as u8],
    }
}

/// `out` padded with zeros to a four-byte boundary.
fn pad4(mut out: Vec<u8>) -> Vec<u8> {
    while out.len() % 4 != 0 {
        out.push(0);
    }
    out
}

impl OneCurve {
    /// The curve's bytes.
    fn tag(&self) -> Vec<u8> {
        match self {
            OneCurve::Segments { breaks, segments } => {
                let mut out = b"curf".to_vec();
                out.extend_from_slice(&[0; 4]);
                out.extend_from_slice(&(segments.len() as u16).to_be_bytes());
                out.extend_from_slice(&[0; 2]);
                for &point in breaks {
                    out.extend_from_slice(&f32_bytes(point));
                }
                for segment in segments {
                    match segment {
                        Piece::Formula { function, params } => {
                            out.extend_from_slice(b"parf");
                            out.extend_from_slice(&[0; 4]);
                            out.extend_from_slice(&function.to_be_bytes());
                            out.extend_from_slice(&[0; 2]);
                            for &param in params {
                                out.extend_from_slice(&f32_bytes(param));
                            }
                        }
                        Piece::Samples(samples) => {
                            out.extend_from_slice(b"samf");
                            out.extend_from_slice(&[0; 4]);
                            out.extend_from_slice(&(samples.len() as u32).to_be_bytes());
                            for &sample in samples {
                                out.extend_from_slice(&f32_bytes(sample));
                            }
                        }
                    }
                }
                out
            }
            OneCurve::Single {
                first,
                last,
                extrapolate,
                encoding,
                values,
            } => {
                let mut out = b"sngf".to_vec();
                out.extend_from_slice(&[0; 4]);
                out.extend_from_slice(&(values.len() as u32).to_be_bytes());
                out.extend_from_slice(&f32_bytes(*first));
                out.extend_from_slice(&f32_bytes(*last));
                out.extend_from_slice(&u16::from(*extrapolate).to_be_bytes());
                out.extend_from_slice(&encoding.to_be_bytes());
                for &value in values {
                    out.extend(encoding_bytes(value, *encoding));
                }
                out
            }
        }
    }

    /// The curve at `x`, from the numbers its bytes store.
    fn at(&self, x: f64) -> f64 {
        match self {
            OneCurve::Segments { breaks, segments } => {
                let breaks: Vec<f64> = breaks.iter().map(|&b| as_f32(b)).collect();
                let index = breaks
                    .iter()
                    .position(|&point| x <= point)
                    .unwrap_or(breaks.len());
                self.segment(&breaks, segments, index, x)
            }
            OneCurve::Single {
                first,
                last,
                extrapolate,
                encoding,
                values,
            } => {
                let (first, last) = (as_f32(*first), as_f32(*last));
                let values: Vec<f64> = values.iter().map(|&v| encoded(v, *encoding)).collect();
                let n = values.len() - 1;
                let step = (last - first) / n as f64;
                let line = |from: usize| {
                    let at = first + step * from as f64;
                    values[from] + (values[from + 1] - values[from]) * (x - at) / step
                };
                if x < first {
                    if *extrapolate {
                        line(0)
                    } else {
                        values[0]
                    }
                } else if x > last {
                    if *extrapolate {
                        line(n - 1)
                    } else {
                        values[n]
                    }
                } else {
                    let position = (x - first) / step;
                    let below = (position.floor() as usize).min(n - 1);
                    values[below] + (values[below + 1] - values[below]) * (position - below as f64)
                }
            }
        }
    }

    /// Segment `index` of a segmented curve at `x`.
    fn segment(&self, breaks: &[f64], segments: &[Piece], index: usize, x: f64) -> f64 {
        match &segments[index] {
            Piece::Formula { function, params } => {
                let p: Vec<f64> = params.iter().map(|&v| as_f32(v)).collect();
                match function {
                    0 => (p[1] * x + p[2]).powf(p[0]) + p[3],
                    1 => p[1] * (p[2] * x.powf(p[0]) + p[3]).log10() + p[4],
                    2 => p[0] * p[1].powf(p[2] * x + p[3]) + p[4],
                    3 => p[1] * (p[2] * x + p[3]).powf(p[0]) + p[4],
                    4 => p[1] * (p[4] * x.powf(p[0]) - p[2]).ln() + p[3],
                    5 => p[5] * ((p[4] * x.powf(p[0]) - p[3]) / p[1]).exp() + p[2],
                    6 => {
                        let raised = x.powf(p[1]);
                        p[5] * ((p[6] * raised - p[2]).max(0.0) / (p[3] - p[4] * raised)).powf(p[0])
                    }
                    _ => {
                        let raised = x.powf(p[1]);
                        p[5] * ((p[2] + p[3] * raised) / (1.0 + p[4] * raised)).powf(p[0])
                    }
                }
            }
            Piece::Samples(samples) => {
                let samples: Vec<f64> = samples.iter().map(|&v| as_f32(v)).collect();
                let (from, to) = (breaks[index - 1], breaks[index]);
                let count = samples.len();
                let position = ((x - from) / (to - from) * count as f64).clamp(0.0, count as f64);
                let below = (position.floor() as usize).min(count - 1);
                let start = if below == 0 {
                    self.segment(breaks, segments, index - 1, from)
                } else {
                    samples[below - 1]
                };
                start + (samples[below] - start) * (position - below as f64)
            }
        }
    }
}

/// One element of a pipeline, as laid out and recomputed here.
#[derive(Clone, Debug)]
enum Element {
    /// A curve set, one curve a channel.
    Curves(Vec<OneCurve>),
    /// A matrix: rows of coefficients, one an output, then an offset a row.
    Matrix {
        rows: Vec<Vec<f64>>,
        offsets: Vec<f64>,
    },
    /// A grid of `grids` points an input axis, `outputs` values a node, the
    /// last axis fastest, in `encoding`: `None` for ICC.1's `clut` of float32
    /// values, `Some` for ICC.2's `xclt`.
    Grid {
        grids: Vec<usize>,
        outputs: usize,
        encoding: Option<u16>,
        values: Vec<f64>,
    },
}

impl Element {
    /// The element's bytes.
    fn tag(&self) -> Vec<u8> {
        let head = |signature: &[u8; 4], p: usize, q: usize| {
            let mut out = signature.to_vec();
            out.extend_from_slice(&[0; 4]);
            out.extend_from_slice(&(p as u16).to_be_bytes());
            out.extend_from_slice(&(q as u16).to_be_bytes());
            out
        };
        match self {
            Element::Curves(curves) => {
                let mut out = head(b"cvst", curves.len(), curves.len());
                let mut data = Vec::new();
                let start = 12 + 8 * curves.len();
                for curve in curves {
                    let bytes = curve.tag();
                    out.extend_from_slice(&((start + data.len()) as u32).to_be_bytes());
                    out.extend_from_slice(&(bytes.len() as u32).to_be_bytes());
                    data.extend(pad4(bytes));
                }
                out.extend(data);
                out
            }
            Element::Matrix { rows, offsets } => {
                let mut out = head(b"matf", rows[0].len(), rows.len());
                for row in rows {
                    for &value in row {
                        out.extend_from_slice(&f32_bytes(value));
                    }
                }
                for &offset in offsets {
                    out.extend_from_slice(&f32_bytes(offset));
                }
                out
            }
            Element::Grid {
                grids,
                outputs,
                encoding,
                values,
            } => {
                let mut counts = [0u8; 16];
                for (slot, &points) in counts.iter_mut().zip(grids) {
                    *slot = points as u8;
                }
                match encoding {
                    None => {
                        let mut out = head(b"clut", grids.len(), *outputs);
                        out.extend_from_slice(&counts);
                        for &value in values {
                            out.extend_from_slice(&f32_bytes(value));
                        }
                        out
                    }
                    Some(encoding) => {
                        let mut out = head(b"xclt", grids.len(), *outputs);
                        out.extend_from_slice(&u32::from(*encoding).to_be_bytes());
                        out.extend_from_slice(&counts);
                        for &value in values {
                            out.extend(encoding_bytes(value, *encoding));
                        }
                        out
                    }
                }
            }
        }
    }

    /// How many channels the element gives.
    fn outputs(&self) -> usize {
        match self {
            Element::Curves(curves) => curves.len(),
            Element::Matrix { rows, .. } => rows.len(),
            Element::Grid { outputs, .. } => *outputs,
        }
    }

    /// The element's outputs for `x`, recomputed from the stored numbers.
    fn apply(&self, x: &[f64]) -> Vec<f64> {
        match self {
            Element::Curves(curves) => curves.iter().zip(x).map(|(c, &v)| c.at(v)).collect(),
            Element::Matrix { rows, offsets } => rows
                .iter()
                .zip(offsets)
                .map(|(row, &offset)| {
                    row.iter().zip(x).map(|(&e, &v)| as_f32(e) * v).sum::<f64>() + as_f32(offset)
                })
                .collect(),
            Element::Grid {
                grids,
                outputs,
                encoding,
                values,
            } => {
                let stored = |value: f64| match encoding {
                    None => as_f32(value),
                    Some(encoding) => encoded(value, *encoding),
                };
                let inputs = grids.len();
                let mut base = vec![0usize; inputs];
                let mut fraction = vec![0.0f64; inputs];
                for axis in 0..inputs {
                    let p = x[axis].clamp(0.0, 1.0) * (grids[axis] - 1) as f64;
                    let index = (p.floor() as usize).min(grids[axis] - 2);
                    base[axis] = index;
                    fraction[axis] = p - index as f64;
                }
                (0..*outputs)
                    .map(|channel| {
                        let mut sum = 0.0;
                        for corner in 0..1usize << inputs {
                            let mut weight = 1.0;
                            let mut offset = 0usize;
                            for axis in 0..inputs {
                                let high = (corner >> (inputs - 1 - axis)) & 1;
                                weight *= if high == 1 {
                                    fraction[axis]
                                } else {
                                    1.0 - fraction[axis]
                                };
                                offset = offset * grids[axis] + base[axis] + high;
                            }
                            sum += weight * stored(values[offset * outputs + channel]);
                        }
                        sum
                    })
                    .collect()
            }
        }
    }
}

/// An `mpet` tag of `inputs` channels chaining `elements`.
fn mpet_tag(inputs: usize, elements: &[Element]) -> Vec<u8> {
    let outputs = elements.last().map_or(inputs, Element::outputs);
    let mut out = b"mpet".to_vec();
    out.extend_from_slice(&[0; 4]);
    out.extend_from_slice(&(inputs as u16).to_be_bytes());
    out.extend_from_slice(&(outputs as u16).to_be_bytes());
    out.extend_from_slice(&(elements.len() as u32).to_be_bytes());
    let start = 16 + 8 * elements.len();
    let mut data = Vec::new();
    for element in elements {
        let bytes = element.tag();
        out.extend_from_slice(&((start + data.len()) as u32).to_be_bytes());
        out.extend_from_slice(&(bytes.len() as u32).to_be_bytes());
        data.extend(pad4(bytes));
    }
    out.extend(data);
    out
}

/// The pipeline's outputs for `x`, held first to `domain`: each element in
/// turn, nothing clipped between them.
fn run(elements: &[Element], x: &[f64], domain: &[(f64, f64)]) -> Vec<f64> {
    let mut values: Vec<f64> = x
        .iter()
        .zip(domain)
        .map(|(&v, &(low, high))| v.clamp(low, high))
        .collect();
    for element in elements {
        values = element.apply(&values);
    }
    values
}

/// A formula segment of `function` over `params`.
fn formula(function: u16, params: &[f64]) -> Piece {
    Piece::Formula {
        function,
        params: params.to_vec(),
    }
}

/// A device-to-XYZ pipeline in ICC.1's elements: segmented curves either side
/// of a break-point — a square below 0.4 and a line above it, a logarithm, an
/// exponential below 0.5 and a sampled stretch to a line above it — a matrix
/// with offsets, and a grid of float32 values.
fn forward_elements() -> Vec<Element> {
    vec![
        Element::Curves(vec![
            OneCurve::Segments {
                breaks: vec![0.4],
                segments: vec![
                    formula(0, &[2.0, 1.0, 0.0, 0.0]),
                    formula(0, &[1.0, 1.2, -0.32, 0.0]),
                ],
            },
            OneCurve::Segments {
                breaks: Vec::new(),
                segments: vec![formula(1, &[1.0, 0.5, 9.0, 1.0, 0.0])],
            },
            OneCurve::Segments {
                breaks: vec![0.5, 0.75],
                segments: vec![
                    formula(2, &[0.25, 2.0, 1.0, -1.0, 0.0]),
                    Piece::Samples(vec![0.3, 0.45, 0.6]),
                    formula(0, &[1.0, 1.0, -0.15, 0.0]),
                ],
            },
        ]),
        Element::Matrix {
            rows: vec![
                vec![0.6, 0.3, 0.1],
                vec![0.2, 0.7, 0.1],
                vec![0.1, 0.1, 0.8],
            ],
            offsets: vec![0.05, 0.0, -0.02],
        },
        Element::Grid {
            grids: vec![2, 2, 2],
            outputs: 3,
            encoding: None,
            values: (0..8usize)
                .flat_map(|node| {
                    let (i, j, k) = ((node >> 2) & 1, (node >> 1) & 1, node & 1);
                    let (i, j, k) = (i as f64, j as f64, k as f64);
                    [0.2 + 0.6 * i, 0.15 + 0.5 * j + 0.1 * i, 0.1 + 0.6 * k]
                })
                .collect(),
        },
    ]
}

/// An XYZ-to-CMYK pipeline in ICC.1's elements: curves over the connection
/// space's range — a line below 0.5 and a square above it, an exponential, a
/// logarithm — a matrix with offsets, and a grid of float32 values giving
/// four inks.
fn output_elements() -> Vec<Element> {
    vec![
        Element::Curves(vec![
            OneCurve::Segments {
                breaks: vec![0.5],
                segments: vec![
                    formula(0, &[1.0, 0.5, 0.0, 0.0]),
                    formula(0, &[2.0, 0.632_455_5, -0.316_227_8, 0.25]),
                ],
            },
            OneCurve::Segments {
                breaks: Vec::new(),
                segments: vec![formula(2, &[0.3, 2.0, 0.5, 0.0, -0.3])],
            },
            OneCurve::Segments {
                breaks: Vec::new(),
                segments: vec![formula(1, &[1.0, 0.6, 4.0, 1.0, 0.0])],
            },
        ]),
        Element::Matrix {
            rows: vec![
                vec![0.9, -0.2, 0.1],
                vec![-0.1, 1.1, 0.0],
                vec![0.2, 0.1, 0.7],
            ],
            offsets: vec![0.02, -0.01, 0.0],
        },
        Element::Grid {
            grids: vec![2, 2, 2],
            outputs: 4,
            encoding: None,
            values: (0..8usize)
                .flat_map(|node| {
                    let (i, j, k) = ((node >> 2) & 1, (node >> 1) & 1, node & 1);
                    let (i, j, k) = (i as f64, j as f64, k as f64);
                    [
                        0.9 - 0.7 * i,
                        0.85 - 0.6 * j,
                        0.8 - 0.5 * k,
                        0.3 * (1.0 - i) * (1.0 - j),
                    ]
                })
                .collect(),
        },
    ]
}

/// The XYZ a transform out of the connection space takes: from 0 to what
/// `u1Fixed15` encodes.
const XYZ_RANGE: (f64, f64) = (0.0, 65535.0 / 32768.0);

/// Device values over three channels: each at seven levels across the range,
/// clear of every break-point.
fn three_channels() -> Vec<[f64; 3]> {
    let levels = [0.0, 0.1, 0.25, 0.55, 0.62, 0.85, 1.0];
    let mut out = Vec::new();
    for a in levels {
        for b in levels {
            for c in levels {
                out.push([a, b, c]);
            }
        }
    }
    out
}

/// XYZ values over the connection space's range, past the PCS white and
/// clear of every break-point.
fn xyz_lattice() -> Vec<[f64; 3]> {
    let levels = [0.0, 0.2, 0.45, 0.7, 0.96, 1.3, 1.8];
    let mut out = Vec::new();
    for x in levels {
        for y in levels {
            for z in levels {
                out.push([x, y, z]);
            }
        }
    }
    out
}

/// How far a pipeline's integer stages may stray from its closed forms: the
/// fixed point the stages compute in and the curves' sampling over their
/// intervals, as the `D2B*` tests in `icc.rs` hold it.
const PIPELINE_SLACK: f64 = 2e-3;

/// **A curve-set, matrix and CLUT pipeline is recomputed, in both
/// directions** (ICC.1 10.14, 8.10.2 a)). A version 4 printer profile's `D2B1`
/// chains segmented curves either side of their break-points — ICC.1's three
/// formula types and a sampled segment — a matrix with offsets and a grid of
/// float32 values; relative colorimetric converts through it before the
/// profile's `A2B0`, and the connection-space value it gives each device
/// value of a lattice is the chain recomputed from the stored numbers,
/// within the fixed point the stages compute in. Its `B2D1` chains the same
/// kinds of element from the connection space, each component held to the
/// space's range, to four inks; relative colorimetric's transform out of the
/// connection space converts through it before the profile's `B2A0`, and
/// gives each XYZ value of a lattice past the PCS white the four inks the
/// chain recomputed here gives.
#[test]
fn a_curve_set_matrix_clut_pipeline_is_recomputed() {
    let forward = forward_elements();
    let output = output_elements();
    let a2b0 = forward_table(true, 4, |_| [20_000, 30_000, 25_000]);
    let b2a0 = lut16(4);
    let parsed = Profile::parse(&profile_of(
        b"prtr",
        b"CMYK",
        b"XYZ ",
        &[
            (*b"A2B0", a2b0.tag()),
            (*b"B2A0", b2a0.tag()),
            (*b"D2B1", mpet_tag(4, &cmyk_forward(&forward))),
            (*b"B2D1", mpet_tag(3, &output)),
        ],
    ))
    .expect("the profile parses");
    let domain = [(0.0, 1.0); 4];
    for x in three_channels() {
        let cmyk = [x[0], x[1], x[2], 1.0 - x[0]];
        let got = parsed
            .connection_value(Intent::RelativeColorimetric, &cmyk)
            .expect("the D2B1 converts");
        let want = run(&cmyk_forward(&forward), &cmyk, &domain);
        for channel in 0..3 {
            assert!(
                (got[channel] - want[channel]).abs() < PIPELINE_SLACK,
                "{cmyk:?}, channel {channel}: {} against {}",
                got[channel],
                want[channel]
            );
        }
    }

    let out = Output::compile(&parsed, Intent::RelativeColorimetric).expect("the B2D1 reads");
    assert_eq!(out.channels(), 4);
    for pcs in xyz_lattice() {
        let got = out.apply(pcs);
        let want = run(&output, &pcs, &[XYZ_RANGE; 3]);
        for (channel, want) in want.into_iter().enumerate() {
            let want = want.clamp(0.0, 1.0) * 65535.0;
            assert!(
                (f64::from(got.channels()[channel]) - want).abs() <= PIPELINE_SLACK * 65535.0,
                "{pcs:?}, ink {channel}: {} against {want:.1}",
                got.channels()[channel]
            );
        }
    }
    // Past the connection space's range a component is held to it.
    assert_eq!(
        out.apply([3.0, -1.0, 9.0]),
        out.apply([XYZ_RANGE.1, 0.0, XYZ_RANGE.1])
    );
}

/// [`forward_elements`] taking four inks: a matrix folding the fourth into
/// the first three first, so the chain's grid sees three.
fn cmyk_forward(three: &[Element]) -> Vec<Element> {
    let mut out = vec![
        Element::Curves(vec![
            OneCurve::Segments {
                breaks: Vec::new(),
                segments: vec![formula(0, &[1.0, 1.0, 0.0, 0.0])],
            };
            4
        ]),
        Element::Matrix {
            rows: vec![
                vec![0.75, 0.0, 0.0, 0.25],
                vec![0.0, 0.75, 0.0, 0.25],
                vec![0.0, 0.0, 0.75, 0.25],
            ],
            offsets: vec![0.0, 0.0, 0.0],
        },
    ];
    out.extend_from_slice(three);
    out
}

/// A version 5 profile (ICC.2) of `tags`, its header naming `spectral` in
/// bytes 100 to 103 and the spectral range `range` in 104 to 109.
fn v5_profile(
    space: &[u8; 4],
    pcs: &[u8; 4],
    spectral: [u8; 4],
    range: [u16; 3],
    tags: &[([u8; 4], Vec<u8>)],
) -> Vec<u8> {
    let mut out = profile_of(b"mntr", space, pcs, tags);
    out[8..12].copy_from_slice(&0x0500_0000u32.to_be_bytes());
    out[100..104].copy_from_slice(&spectral);
    for (slot, value) in range.iter().enumerate() {
        out[104 + 2 * slot..106 + 2 * slot].copy_from_slice(&value.to_be_bytes());
    }
    out
}

/// A device-to-XYZ pipeline in ICC.2's elements: segmented curves of formula
/// types 3 to 7 (Table 111) either side of their break-points, a single
/// sampled curve clipped past its ends, a matrix with offsets and an
/// extended grid of half floats.
fn v5_forward() -> Vec<Element> {
    vec![
        Element::Curves(vec![
            OneCurve::Segments {
                breaks: vec![0.3],
                segments: vec![
                    formula(3, &[2.0, 1.0, 1.0, 0.1, -0.01]),
                    formula(4, &[1.0, 0.2, -1.0, 0.1, 1.0]),
                ],
            },
            OneCurve::Single {
                first: 0.0,
                last: 1.0,
                extrapolate: false,
                encoding: 0,
                values: vec![0.0, 0.1, 0.3, 0.55, 0.8],
            },
            OneCurve::Segments {
                breaks: vec![0.4, 0.8],
                segments: vec![
                    formula(5, &[1.0, 1.0, -0.1, 0.0, 1.0, 0.1]),
                    formula(7, &[1.0, 1.0, 0.0, 0.2, 0.5, 1.0]),
                    formula(6, &[0.5, 1.0, 0.2, 2.0, 1.0, 0.4, 1.0]),
                ],
            },
        ]),
        Element::Matrix {
            rows: vec![
                vec![0.5, 0.3, 0.2],
                vec![0.25, 0.6, 0.15],
                vec![0.1, 0.2, 0.7],
            ],
            offsets: vec![0.0, 0.05, 0.0],
        },
        Element::Grid {
            grids: vec![2, 2, 2],
            outputs: 3,
            encoding: Some(1),
            values: (0..8usize)
                .flat_map(|node| {
                    let (i, j, k) = ((node >> 2) & 1, (node >> 1) & 1, node & 1);
                    [
                        [0.125, 0.875][i],
                        [0.25, 0.75, 0.5, 1.0][2 * i + j],
                        [0.125, 0.625][k],
                    ]
                })
                .collect(),
        },
    ]
}

/// An XYZ-to-RGB pipeline in ICC.2's elements: a single sampled curve of
/// 16-bit entries extended along its line past its last input, a segmented
/// curve of type 3, one of type 0, a matrix and an extended grid of bytes.
fn v5_output() -> Vec<Element> {
    vec![
        Element::Curves(vec![
            OneCurve::Single {
                first: 0.0,
                last: 1.5,
                extrapolate: true,
                encoding: 2,
                values: vec![0.0, 0.3, 0.5, 0.65],
            },
            OneCurve::Segments {
                breaks: Vec::new(),
                segments: vec![formula(3, &[1.0, 0.5, 1.0, 0.0, 0.0])],
            },
            OneCurve::Segments {
                breaks: Vec::new(),
                segments: vec![formula(0, &[1.0, 0.45, 0.0, 0.0])],
            },
        ]),
        Element::Matrix {
            rows: vec![
                vec![1.1, -0.1, 0.0],
                vec![-0.05, 1.0, 0.05],
                vec![0.0, -0.1, 1.1],
            ],
            offsets: vec![0.0, 0.0, 0.0],
        },
        Element::Grid {
            grids: vec![2, 2, 2],
            outputs: 3,
            encoding: Some(3),
            values: (0..8usize)
                .flat_map(|node| {
                    let (i, j, k) = ((node >> 2) & 1, (node >> 1) & 1, node & 1);
                    let (i, j, k) = (i as f64, j as f64, k as f64);
                    [0.1 + 0.8 * i, 0.15 + 0.7 * j, 0.05 + 0.9 * k]
                })
                .collect(),
        },
    ]
}

/// **A version 5 profile with a multi-process transform converts, in both
/// directions** (ICC.2 9.2.1, 9.2.38, 10.2.17). A version 5 display profile
/// whose `A2B1` and `B2A1` are `multiProcessElementsType` — ICC.2's formula
/// segments of types 3 to 7, single sampled curves clipped and extended,
/// extended grids of half floats and of bytes, matrices with offsets —
/// parses with its header's version 5 fields read; converts each device
/// value through its `A2B1`'s chain to the XYZ recomputed here, and builds a
/// transform to sRGB through it under every intent; and takes each XYZ value
/// through its `B2A1`'s chain, the components held to the connection
/// space's range, to the device values recomputed here. Its `D2B*` tags are
/// a spectral connection space's (9.2.77), which these do not read.
#[test]
fn a_v5_profile_with_a_multi_process_transform_converts() {
    let forward = v5_forward();
    let output = v5_output();
    // `rs0024`: reflectance spectra of 36 channels, 380 to 730 nanometres.
    let bytes = v5_profile(
        b"RGB ",
        b"XYZ ",
        *b"rs\x00\x24",
        [0x5DF0, 0x61B4, 36],
        &[
            (*b"A2B1", mpet_tag(3, &forward)),
            (*b"B2A1", mpet_tag(3, &output)),
            (*b"D2B1", vec![0; 16]),
        ],
    );
    let parsed = Profile::parse(&bytes).expect("the version 5 profile parses");
    assert_eq!(parsed.version, 5);
    let header = parsed.version5.expect("a version 5 header");
    assert_eq!(header.spectral_pcs, *b"rs\x00\x24");
    assert_eq!(
        (
            header.spectral_range.start,
            header.spectral_range.end,
            header.spectral_range.steps
        ),
        (380.0, 730.0, 36)
    );
    assert_eq!(header.mcs, [0; 4]);

    let domain = [(0.0, 1.0); 3];
    for x in three_channels() {
        let got = parsed
            .connection_value(Intent::RelativeColorimetric, &x)
            .expect("the A2B1 converts");
        let want = run(&forward, &x, &domain);
        for channel in 0..3 {
            assert!(
                (got[channel] - want[channel]).abs() < PIPELINE_SLACK,
                "{x:?}, channel {channel}: {} against {}",
                got[channel],
                want[channel]
            );
        }
    }
    for intent in Intent::ALL {
        assert!(Transform::compile(&parsed, intent).is_some(), "{intent:?}");
    }
    assert!(Transforms::compile(parsed.clone()).is_some());

    let out = Output::compile(&parsed, Intent::RelativeColorimetric).expect("the B2A1 reads");
    assert_eq!(out.channels(), 3);
    for pcs in xyz_lattice() {
        let got = out.apply(pcs);
        let want = run(&output, &pcs, &[XYZ_RANGE; 3]);
        for (channel, want) in want.into_iter().enumerate() {
            let want = want.clamp(0.0, 1.0) * 65535.0;
            assert!(
                (f64::from(got.channels()[channel]) - want).abs() <= PIPELINE_SLACK * 65535.0,
                "{pcs:?}, channel {channel}: {} against {want:.1}",
                got.channels()[channel]
            );
        }
    }

    // The same tags in a version 4 profile are tables no `A2B*` tag may
    // hold there, and are refused for it.
    let mut v4 = bytes.clone();
    v4[8..12].copy_from_slice(&0x0430_0000u32.to_be_bytes());
    assert_eq!(Profile::parse(&v4), Err(IccError::NeedsLut));
}

// ---- the calculator element ------------------------------------------------------
//
// ICC.2's calculator (11.2.1): a main function of operations in reverse Polish
// notation — eight bytes each, a signature and four data bytes — over input,
// output and temporary channels. A pipeline runs it at every node of a grid
// over its inputs' intervals when the table is built: 33 nodes an axis over
// three inputs, so at a node the pipeline gives what the program gives there.

/// A calculator operation: a signature and four data bytes.
type Operation = [u8; 8];

/// An operation of a 16-bit `s` and `t`.
fn calc_op(signature: &[u8; 4], s: u16, t: u16) -> Operation {
    let mut out = [0u8; 8];
    out[..4].copy_from_slice(signature);
    out[4..6].copy_from_slice(&s.to_be_bytes());
    out[6..].copy_from_slice(&t.to_be_bytes());
    out
}

/// `data`: `value` pushed as a float32.
fn calc_push(value: f64) -> Operation {
    let mut out = [0u8; 8];
    out[..4].copy_from_slice(b"data");
    out[4..].copy_from_slice(&(value as f32).to_bits().to_be_bytes());
    out
}

/// An operation carrying a 32-bit count: `if`, `else`, `case`, `dflt`.
fn calc_counted(signature: &[u8; 4], count: usize) -> Operation {
    let mut out = [0u8; 8];
    out[..4].copy_from_slice(signature);
    out[4..].copy_from_slice(&(count as u32).to_be_bytes());
    out
}

/// A calculator element of three inputs and three outputs whose main
/// function is `ops`, with no sub-elements.
fn calc_element(ops: &[Operation]) -> Vec<u8> {
    let mut out = b"calc".to_vec();
    out.extend_from_slice(&[0; 4]);
    out.extend_from_slice(&3u16.to_be_bytes());
    out.extend_from_slice(&3u16.to_be_bytes());
    out.extend_from_slice(&[0; 4]);
    out.extend_from_slice(&24u32.to_be_bytes());
    out.extend_from_slice(&((12 + 8 * ops.len()) as u32).to_be_bytes());
    out.extend_from_slice(b"func");
    out.extend_from_slice(&[0; 4]);
    out.extend_from_slice(&(ops.len() as u32).to_be_bytes());
    for op in ops {
        out.extend_from_slice(op);
    }
    out
}

/// One program and the values it gives, recomputed here.
struct Program {
    what: &'static str,
    ops: Vec<Operation>,
    want: fn([f64; 3]) -> [f64; 3],
}

/// `in` of channel `s` alone.
fn input(s: u16) -> Operation {
    calc_op(b"in  ", s, 0)
}

/// `out` of channel `s` alone.
fn output(s: u16) -> Operation {
    calc_op(b"out ", s, 0)
}

/// A one-value vector function.
fn unary(signature: &[u8; 4]) -> Operation {
    calc_op(signature, 0, 0)
}

/// The programs [`the_calculator_computes_what_its_program_says`] runs, one
/// an operation family each.
fn programs() -> Vec<Program> {
    vec![
        Program {
            what: "arithmetic",
            ops: vec![
                input(0),
                input(1),
                input(2),
                unary(b"mul "),
                unary(b"add "),
                output(0),
                input(0),
                input(1),
                unary(b"sub "),
                input(2),
                calc_push(1.0),
                unary(b"add "),
                unary(b"div "),
                output(1),
                input(0),
                calc_push(0.5),
                unary(b"add "),
                calc_push(2.5),
                unary(b"pow "),
                calc_push(0.7),
                unary(b"mod "),
                output(2),
            ],
            want: |[x, y, z]| {
                let base = (x + 0.5).powf(2.5);
                let seven = f64::from(0.7f32);
                [
                    x + y * z,
                    (x - y) / (z + 1.0),
                    base - (base / seven).trunc() * seven,
                ]
            },
        },
        Program {
            what: "transcendentals",
            ops: vec![
                input(0),
                unary(b"sin "),
                input(1),
                unary(b"cos "),
                unary(b"add "),
                output(0),
                input(2),
                unary(b"sqrt"),
                input(0),
                unary(b"neg "),
                unary(b"exp "),
                unary(b"mul "),
                output(1),
                input(1),
                calc_push(1.0),
                unary(b"add "),
                unary(b"ln  "),
                input(2),
                calc_push(1.0),
                unary(b"add "),
                unary(b"log "),
                unary(b"add "),
                input(0),
                unary(b"atan"),
                unary(b"add "),
                output(2),
            ],
            want: |[x, y, z]| {
                [
                    x.sin() + y.cos(),
                    z.sqrt() * (-x).exp(),
                    (y + 1.0).ln() + (z + 1.0).log10() + x.atan(),
                ]
            },
        },
        Program {
            what: "comparisons and logic",
            ops: vec![
                input(0),
                input(1),
                unary(b"lt  "),
                input(1),
                input(2),
                unary(b"ge  "),
                calc_push(0.5),
                unary(b"mul "),
                unary(b"add "),
                output(0),
                calc_op(b"in  ", 0, 2),
                calc_op(b"max ", 1, 0),
                calc_op(b"in  ", 0, 2),
                calc_op(b"min ", 1, 0),
                unary(b"sub "),
                output(1),
                input(0),
                input(1),
                calc_op(b"and ", 0, 0),
                input(1),
                input(2),
                calc_op(b"or  ", 0, 0),
                unary(b"add "),
                input(2),
                unary(b"not "),
                unary(b"add "),
                output(2),
            ],
            want: |[x, y, z]| {
                let truth = |b: bool| f64::from(u8::from(b));
                [
                    truth(x < y) + 0.5 * truth(y >= z),
                    x.max(y).max(z) - x.min(y).min(z),
                    truth(x >= 0.5 && y >= 0.5) + truth(y >= 0.5 || z >= 0.5) + truth(z < 0.5),
                ]
            },
        },
        Program {
            what: "selection",
            ops: [
                vec![input(0), calc_counted(b"if  ", 1), calc_counted(b"else", 1)],
                vec![input(1), input(2), output(0)],
                vec![
                    input(2),
                    calc_push(3.0),
                    unary(b"mul "),
                    calc_op(b"sel ", 0, 0),
                    calc_counted(b"case", 1),
                    calc_counted(b"case", 1),
                    calc_counted(b"case", 1),
                    calc_counted(b"dflt", 1),
                    calc_push(0.125),
                    calc_push(0.25),
                    calc_push(0.375),
                    calc_push(0.5),
                    output(1),
                ],
                vec![
                    input(0),
                    calc_push(0.5),
                    unary(b"sub "),
                    unary(b"sign"),
                    input(1),
                    calc_push(4.0),
                    unary(b"mul "),
                    unary(b"rond"),
                    calc_push(4.0),
                    unary(b"div "),
                    unary(b"add "),
                    input(2),
                    calc_push(2.0),
                    unary(b"mul "),
                    unary(b"flor"),
                    unary(b"add "),
                    input(0),
                    unary(b"ceil"),
                    unary(b"sub "),
                    input(1),
                    calc_push(3.0),
                    unary(b"mul "),
                    unary(b"trnc"),
                    unary(b"add "),
                    output(2),
                ],
            ]
            .concat(),
            want: |[x, y, z]| {
                let sign = |v: f64| {
                    if v < 0.0 {
                        -1.0
                    } else if v > 0.0 {
                        1.0
                    } else {
                        0.0
                    }
                };
                let round = |v: f64| {
                    if v < 0.0 {
                        (v - 0.5).trunc()
                    } else {
                        (v + 0.5).trunc()
                    }
                };
                let selector = round(z * 3.0);
                [
                    if x >= 0.5 { y } else { z },
                    match selector as i64 {
                        0 => 0.125,
                        1 => 0.25,
                        2 => 0.375,
                        _ => 0.5,
                    },
                    sign(x - 0.5) + round(y * 4.0) / 4.0 + (z * 2.0).floor() - x.ceil()
                        + (y * 3.0).trunc(),
                ]
            },
        },
        Program {
            what: "the stack and temporary channels",
            ops: vec![
                calc_op(b"in  ", 0, 2),
                calc_op(b"rotl", 2, 0),
                calc_op(b"tput", 0, 2),
                calc_op(b"tget", 1, 0),
                calc_op(b"tget", 0, 0),
                unary(b"sub "),
                output(0),
                calc_op(b"tget", 2, 0),
                calc_op(b"copy", 0, 0),
                unary(b"mul "),
                output(1),
                calc_op(b"tget", 0, 2),
                calc_op(b"flip", 1, 0),
                calc_op(b"posd", 2, 0),
                calc_op(b"pop ", 0, 0),
                calc_op(b"rotr", 2, 0),
                calc_op(b"sum ", 1, 0),
                output(2),
            ],
            want: |[x, y, z]| [z - y, x * x, x + y + z],
        },
        Program {
            what: "XYZ to Lab",
            ops: vec![
                calc_op(b"in  ", 0, 2),
                calc_op(b"tLab", 0, 0),
                calc_op(b"tput", 0, 2),
                calc_op(b"tget", 0, 0),
                calc_push(100.0),
                unary(b"div "),
                output(0),
                calc_op(b"tget", 1, 0),
                calc_push(128.0),
                unary(b"add "),
                calc_push(255.0),
                unary(b"div "),
                output(1),
                calc_op(b"tget", 2, 0),
                calc_push(128.0),
                unary(b"add "),
                calc_push(255.0),
                unary(b"div "),
                output(2),
            ],
            want: |[x, y, z]| {
                let f = |t: f64| {
                    if t > (6.0f64 / 29.0).powi(3) {
                        t.cbrt()
                    } else {
                        841.0 / 108.0 * t + 4.0 / 29.0
                    }
                };
                let (fx, fy, fz) = (f(x), f(y), f(z));
                [
                    (116.0 * fy - 16.0) / 100.0,
                    (500.0 * (fx - fy) + 128.0) / 255.0,
                    (200.0 * (fy - fz) + 128.0) / 255.0,
                ]
            },
        },
    ]
}

/// **The calculator computes what its program says** (ICC.2 11.2.1). A
/// version 5 profile's `A2B1` is a multi-process tag of one calculator
/// element, and for each program — arithmetic, transcendentals, comparisons
/// with the sequence functions and logic, conditional and selected streams
/// with rounding, the stack operations and temporary channels, and XYZ to
/// Lab — the connection-space value at each node of the grid it is run over
/// is the program's answer recomputed here from Table 102's definitions,
/// within the fixed point the stage holds it in.
#[test]
fn the_calculator_computes_what_its_program_says() {
    let levels = [0usize, 5, 13, 16, 23, 32];
    for program in programs() {
        let tag = {
            let element = calc_element(&program.ops);
            let mut out = b"mpet".to_vec();
            out.extend_from_slice(&[0; 4]);
            out.extend_from_slice(&3u16.to_be_bytes());
            out.extend_from_slice(&3u16.to_be_bytes());
            out.extend_from_slice(&1u32.to_be_bytes());
            out.extend_from_slice(&24u32.to_be_bytes());
            out.extend_from_slice(&(element.len() as u32).to_be_bytes());
            out.extend(element);
            out
        };
        let bytes = v5_profile(b"RGB ", b"XYZ ", [0; 4], [0; 3], &[(*b"A2B1", tag)]);
        let parsed =
            Profile::parse(&bytes).unwrap_or_else(|error| panic!("{}: {error:?}", program.what));
        for &i in &levels {
            for &j in &levels {
                for &k in &levels {
                    let x = [i, j, k].map(|n| n as f64 / 32.0);
                    let got = parsed
                        .connection_value(Intent::RelativeColorimetric, &x)
                        .expect("a value");
                    let want = (program.want)(x);
                    for channel in 0..3 {
                        assert!(
                            (got[channel] - want[channel]).abs() < 2e-5,
                            "{}, {x:?}, output {channel}: {} against {}",
                            program.what,
                            got[channel],
                            want[channel]
                        );
                    }
                }
            }
        }
    }
}
