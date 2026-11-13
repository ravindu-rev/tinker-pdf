//! Computes the engine's own sRGB ICC profile into `OUT_DIR/srgb.icc`
//! (`docs/design/icc.md`, "A first-party sRGB profile (RC-19)").
//!
//! The profile is computed, not vendored: every number below is IEC
//! 61966-2-1's, as W3C's CSS Color 4 publishes the final standard's values
//! (section 10.2, the primaries and the transfer function; section 2, D65),
//! or ICC.1:2001-04's (A.1, the D50 PCS white; E.1.2, the linearized
//! Bradford matrix), and the clauses cited are ICC.1:2001-04's, the edition
//! of profile version 2.4.0. `src/icc.rs` takes the bytes as
//! `icc::SRGB_PROFILE`.
//!
//! # One byte string on every build host
//!
//! The arithmetic is IEEE 754's four operations, which every host rounds
//! alike, and the curve's one transcendental is `tinker_pdf_math::pow`, the
//! deterministic power the pixel path uses, so the bytes do not depend on the
//! host's libm or on the target. `crates/tinker-pdf/tests/determinism.rs`
//! pins their SHA-256 on every target, and a host that writes others is a
//! defect here.
//!
//! # Panicking is correct here
//!
//! Ruling 1 binds code that reads documents. This reads nothing: a failure
//! is a defect in this file or an unwritable `OUT_DIR`, and failing the
//! build is the only useful answer to either.

use std::path::PathBuf;

/// IEC 61966-2-1's red, green and blue chromaticities `(x, y)` (CSS Color 4,
/// 10.2).
const PRIMARIES: [(f64, f64); 3] = [(0.640, 0.330), (0.300, 0.600), (0.150, 0.060)];

/// The white point, D65, `(x, y)` (CSS Color 4, 10.2 and 2).
const D65: (f64, f64) = (0.3127, 0.3290);

/// The PCS white, D50 (ICC.1:2001-04 A.1): what the colorants are adapted to.
const PCS_WHITE: [f64; 3] = [0.9642, 1.0, 0.8249];

/// The PCS white as the header's illuminant field and `wtpt` carry it, three
/// `s15Fixed16Number`s: the encoding `icc::PCS_WHITE` reads back as exactly
/// the PCS white, so ICC-absolute colorimetry through this profile scales by
/// exactly one.
const PCS_WHITE_ENCODED: [u32; 3] = [0x0000_F6D6, 0x0001_0000, 0x0000_D32D];

/// The linearized Bradford cone matrix, by rows (ICC.1:2001-04 E.1.2, E.6).
const BRADFORD: [[f64; 3]; 3] = [
    [0.8951, 0.2664, -0.1614],
    [-0.7502, 1.7135, 0.0367],
    [0.0389, -0.0685, 1.0296],
];

/// IEC 61966-2-1's transfer function, encoded to linear (CSS Color 4, 10.2):
/// the linear segment's end, its slope's divisor, and the power law's
/// offset, divisor and exponent.
const THRESHOLD: f64 = 0.04045;
const SLOPE_DIVISOR: f64 = 12.92;
const OFFSET: f64 = 0.055;
const SCALE: f64 = 1.055;
const EXPONENT: f64 = 2.4;

/// How many entries the sampled tone curve carries (6.5.3).
const CURVE_ENTRIES: usize = 1024;

/// The invariant ASCII description (6.4.32, 6.5.17).
const DESCRIPTION: &str = "sRGB IEC 61966-2-1 (tinker-pdf)";

/// The copyright text (6.4.13, 6.5.18).
const COPYRIGHT: &str = "Copyright tinker-pdf contributors, MIT OR Apache-2.0";

/// The header's creation date and time (Table 9, a `dateTimeNumber`, 5.3.1):
/// year, month, day, hours, minutes, seconds. Fixed, so the bytes are.
const CREATED: [u16; 6] = [2026, 11, 13, 0, 0, 0];

/// Profile version 2.4.0 (6.1.3, Table 10).
const VERSION: u32 = 0x0240_0000;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    let out = PathBuf::from(std::env::var("OUT_DIR").expect("cargo sets OUT_DIR"));
    std::fs::write(out.join("srgb.icc"), profile()).expect("OUT_DIR is writable");
}

/// The profile: the header, the tag table and the tags' data.
fn profile() -> Vec<u8> {
    let (columns, adaptation) = colorants();
    let curve = curve_tag();
    // The tag table's entries in order, each naming its data by index into
    // `elements`; the three tone curves name one element (6.3: "tags may
    // reference the same tag data").
    let elements: Vec<Vec<u8>> = vec![
        description_tag(DESCRIPTION),
        text_tag(COPYRIGHT),
        xyz_tag_encoded(PCS_WHITE_ENCODED),
        sf32_tag(&adaptation),
        xyz_tag(columns[0]),
        xyz_tag(columns[1]),
        xyz_tag(columns[2]),
        curve,
    ];
    let table: [(&[u8; 4], usize); 10] = [
        (b"desc", 0),
        (b"cprt", 1),
        (b"wtpt", 2),
        (b"chad", 3),
        (b"rXYZ", 4),
        (b"gXYZ", 5),
        (b"bXYZ", 6),
        (b"rTRC", 7),
        (b"gTRC", 7),
        (b"bTRC", 7),
    ];

    // The data follow the table, each element on a four-byte boundary
    // (6.2.2) and each padded with zero bytes to the next one; an element's
    // size is its own, never the padding's (6.2.3).
    let data_start = 128 + 4 + 12 * table.len();
    let mut data = Vec::new();
    let mut placed = Vec::with_capacity(elements.len());
    for element in &elements {
        assert_eq!((data_start + data.len()) % 4, 0, "an element is aligned");
        placed.push((data_start + data.len(), element.len()));
        data.extend_from_slice(element);
        while (data_start + data.len()) % 4 != 0 {
            data.push(0);
        }
    }

    let mut out = header();
    out.extend_from_slice(&u32_of(table.len()).to_be_bytes());
    for (signature, element) in table {
        let (offset, size) = placed[element];
        out.extend_from_slice(signature);
        out.extend_from_slice(&u32_of(offset).to_be_bytes());
        out.extend_from_slice(&u32_of(size).to_be_bytes());
    }
    assert_eq!(out.len(), data_start, "the table ends where the data begin");
    out.extend_from_slice(&data);
    let size = u32_of(out.len());
    out[0..4].copy_from_slice(&size.to_be_bytes());
    out
}

/// The 128-byte header (6.1, Table 9), its size field filled in last.
fn header() -> Vec<u8> {
    let mut header = vec![0u8; 128];
    // 0..3, the profile size, is written once the profile is whole; 4..7, the
    // preferred CMM, stays zero for none (6.1.2).
    header[8..12].copy_from_slice(&VERSION.to_be_bytes());
    header[12..16].copy_from_slice(b"mntr"); // 6.1.4, a display device
    header[16..20].copy_from_slice(b"RGB "); // 6.1.5
    header[20..24].copy_from_slice(b"XYZ "); // 6.1.6
    for (field, value) in CREATED.iter().enumerate() {
        let at = 24 + 2 * field;
        header[at..at + 2].copy_from_slice(&value.to_be_bytes());
    }
    header[36..40].copy_from_slice(b"acsp");
    // 40..43 the primary platform (6.1.7), 44..47 the flags (6.1.8), 48..55
    // the device manufacturer and model (6.1.9), 56..63 the attributes
    // (6.1.10) and 64..67 the rendering intent (6.1.11, perceptual) stay
    // zero.
    for (axis, value) in PCS_WHITE_ENCODED.iter().enumerate() {
        let at = 68 + 4 * axis;
        header[at..at + 4].copy_from_slice(&value.to_be_bytes());
    }
    // 80..83, the profile creator (6.1.12), and 84..127, reserved, stay zero.
    header
}

/// The `XYZ` value of the chromaticity `(x, y)` at a luminance of one.
fn xyz((x, y): (f64, f64)) -> [f64; 3] {
    [x / y, 1.0, (1.0 - x - y) / y]
}

/// `m` times `v`.
fn apply(m: &[[f64; 3]; 3], v: [f64; 3]) -> [f64; 3] {
    let row = |r: usize| m[r][0] * v[0] + m[r][1] * v[1] + m[r][2] * v[2];
    [row(0), row(1), row(2)]
}

/// `a` times `b`.
fn product(a: &[[f64; 3]; 3], b: &[[f64; 3]; 3]) -> [[f64; 3]; 3] {
    let mut out = [[0.0; 3]; 3];
    for (r, row) in out.iter_mut().enumerate() {
        for (c, slot) in row.iter_mut().enumerate() {
            *slot = a[r][0] * b[0][c] + a[r][1] * b[1][c] + a[r][2] * b[2][c];
        }
    }
    out
}

/// The inverse of `m`, by its cofactors over its determinant.
fn inverse(m: &[[f64; 3]; 3]) -> [[f64; 3]; 3] {
    let [[a, b, c], [d, e, f], [g, h, i]] = *m;
    let det = a * (e * i - f * h) - b * (d * i - f * g) + c * (d * h - e * g);
    assert!(det != 0.0, "the matrix is invertible");
    [
        [
            (e * i - f * h) / det,
            (c * h - b * i) / det,
            (b * f - c * e) / det,
        ],
        [
            (f * g - d * i) / det,
            (a * i - c * g) / det,
            (c * d - a * f) / det,
        ],
        [
            (d * h - e * g) / det,
            (b * g - a * h) / det,
            (a * e - b * d) / det,
        ],
    ]
}

/// The colorant columns, `rXYZ`, `gXYZ` and `bXYZ`, each `[X, Y, Z]`, and
/// the adaptation matrix `chad` holds, by rows.
///
/// With the primaries as the columns of `P`, `S = P⁻¹ W` scales them so that
/// `M = P · diag(S)` takes linear RGB to `XYZ` relative to D65, white to `W`.
/// The adaptation is E.1.2's (E.7): the cone responses of D65 and of the PCS
/// white through the Bradford matrix, and
/// `A = BFD⁻¹ · diag(pcs / src) · BFD`. The columns are those of `A · M`.
fn colorants() -> ([[f64; 3]; 3], [[f64; 3]; 3]) {
    let white = xyz(D65);
    let [red, green, blue] = PRIMARIES.map(xyz);
    let primaries = [
        [red[0], green[0], blue[0]],
        [red[1], green[1], blue[1]],
        [red[2], green[2], blue[2]],
    ];
    let scale = apply(&inverse(&primaries), white);
    let mut to_xyz = primaries;
    for row in &mut to_xyz {
        for (column, slot) in row.iter_mut().enumerate() {
            *slot *= scale[column];
        }
    }

    let source = apply(&BRADFORD, white);
    let target = apply(&BRADFORD, PCS_WHITE);
    let mut ratio = [[0.0; 3]; 3];
    for (axis, row) in ratio.iter_mut().enumerate() {
        row[axis] = target[axis] / source[axis];
    }
    let adaptation = product(&inverse(&BRADFORD), &product(&ratio, &BRADFORD));
    let adapted = product(&adaptation, &to_xyz);
    let columns = [0, 1, 2].map(|column| [0, 1, 2].map(|row| adapted[row][column]));
    (columns, adaptation)
}

/// `value` as an `s15Fixed16Number` (5.3.3), rounded to the nearest.
fn s15_fixed16(value: f64) -> u32 {
    let scaled = (value * 65536.0).round();
    assert!(
        (f64::from(i32::MIN)..=f64::from(i32::MAX)).contains(&scaled),
        "{value} is inside s15Fixed16's range"
    );
    // The two's complement bits of the signed number, as the field holds
    // them.
    (scaled as i32) as u32
}

/// A length or an offset as the `uInt32Number` the profile holds it in.
fn u32_of(value: usize) -> u32 {
    u32::try_from(value).expect("a profile of a few kilobytes")
}

/// An `XYZType` element of one `XYZNumber` (6.5.26, Table 78).
fn xyz_tag(value: [f64; 3]) -> Vec<u8> {
    xyz_tag_encoded(value.map(s15_fixed16))
}

/// An `XYZType` element of one `XYZNumber` already encoded.
fn xyz_tag_encoded(value: [u32; 3]) -> Vec<u8> {
    let mut out = b"XYZ ".to_vec();
    out.extend_from_slice(&[0; 4]);
    for component in value {
        out.extend_from_slice(&component.to_be_bytes());
    }
    out
}

/// An `s15Fixed16ArrayType` element of a 3 by 3 matrix, by rows (6.5.14,
/// Table 63; 6.4.11's order).
fn sf32_tag(matrix: &[[f64; 3]; 3]) -> Vec<u8> {
    let mut out = b"sf32".to_vec();
    out.extend_from_slice(&[0; 4]);
    for row in matrix {
        for value in row {
            out.extend_from_slice(&s15_fixed16(*value).to_be_bytes());
        }
    }
    out
}

/// A `textDescriptionType` element holding `text` as the invariant ASCII
/// description and nothing localised (6.5.17, Table 68): the Unicode
/// language code and count zero, the ScriptCode code and count zero and the
/// 67-byte Macintosh description zero, as 6.5.17 asks where neither is.
fn description_tag(text: &str) -> Vec<u8> {
    assert!(text.is_ascii(), "the description is 7-bit ASCII");
    let mut out = b"desc".to_vec();
    out.extend_from_slice(&[0; 4]);
    out.extend_from_slice(&u32_of(text.len() + 1).to_be_bytes());
    out.extend_from_slice(text.as_bytes());
    out.push(0);
    out.extend_from_slice(&0u32.to_be_bytes()); // Unicode language code
    out.extend_from_slice(&0u32.to_be_bytes()); // Unicode count
    out.extend_from_slice(&0u16.to_be_bytes()); // ScriptCode code
    out.push(0); // ScriptCode count
    out.extend_from_slice(&[0; 67]);
    out
}

/// A `textType` element holding `text` and its terminating null (6.5.18,
/// Table 70).
fn text_tag(text: &str) -> Vec<u8> {
    assert!(text.is_ascii(), "the text is 7-bit ASCII");
    let mut out = b"text".to_vec();
    out.extend_from_slice(&[0; 4]);
    out.extend_from_slice(text.as_bytes());
    out.push(0);
    out
}

/// The transfer function at the encoded value `c`, as linear light.
fn linear(c: f64) -> f64 {
    if c <= THRESHOLD {
        c / SLOPE_DIVISOR
    } else {
        tinker_pdf_math::pow((c + OFFSET) / SCALE, EXPONENT)
    }
}

/// The `curveType` element every tone curve names (6.5.3, Table 37):
/// [`CURVE_ENTRIES`] entries of the transfer function at `i / 1023`, each a
/// `uInt16Number` with 65 535 standing for 1.0, rounded to the nearest.
fn curve_tag() -> Vec<u8> {
    let mut out = b"curv".to_vec();
    out.extend_from_slice(&[0; 4]);
    out.extend_from_slice(&u32_of(CURVE_ENTRIES).to_be_bytes());
    let last = (CURVE_ENTRIES - 1) as f64;
    for i in 0..CURVE_ENTRIES {
        let value = (linear(i as f64 / last).clamp(0.0, 1.0) * 65535.0).round();
        out.extend_from_slice(&(value as u16).to_be_bytes());
    }
    out
}
