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
