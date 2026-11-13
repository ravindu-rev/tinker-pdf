//! Known answers for the ICC engine's transforms out of the connection space
//! (`docs/design/icc.md`, "ICC engine completion"): each held to its
//! arithmetic recomputed here from ICC.1's own equations — a colorant
//! matrix's inverse by its cofactors, a tone curve's inverse from its form
//! (10.5's `curv` gamma and sampled curves, 10.18's `para`), ICC-absolute
//! colorimetry's media white (6.2.3) and Lab's relation to XYZ (6.4) — and
//! never from the engine's code. The profiles are built here byte by byte, so
//! each answer is a statement about the format.
//!
//! A transform out of the connection space reads its device value from
//! tables: the inverted curve is held at 4 096 evenly spaced values of light
//! and read between them, and the matrix is held in s15.16. So a channel is
//! held to **the table step** around its light: the span of the curve's exact
//! inverse over the interval of those values the light falls in and the one
//! either side of it — the matrix's rounding can move a light across one of
//! them — and one device unit for the rounding of the entries
//! (`docs/design/icc.md`, "The output transform").

use tinker_pdf_color::icc::{Intent, Model, Output, Profile, PCS_WHITE};

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
    let mut out = vec![0u8; 128];
    out[8..12].copy_from_slice(&0x0430_0000u32.to_be_bytes());
    out[12..16].copy_from_slice(b"mntr");
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
