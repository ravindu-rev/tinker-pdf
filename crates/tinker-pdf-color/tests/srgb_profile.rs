//! The engine's own sRGB profile, `tinker_pdf_color::icc::SRGB_PROFILE`
//! (`docs/design/icc.md`, "A first-party sRGB profile (RC-19)"), held to the
//! numbers it is computed from.
//!
//! Everything here is recomputed in this file from the published values and
//! read out of the profile's bytes through its own small reader, sharing no
//! code with `build.rs`: the matrix is solved by elimination rather than by
//! cofactors, and the curve's power is the platform's `powf` rather than
//! `tinker_pdf_math::pow`, so the two agree only where both are right. The
//! clauses are ICC.1:2001-04's; the values IEC 61966-2-1's as CSS Color 4
//! publishes them (10.2 and 2), and ICC.1:2001-04's A.1 and E.1.2.

use tinker_pdf_color::icc::{
    data_space, Curve, Intent, Model, Profile, Transform, PCS_WHITE, SRGB_PROFILE,
};

/// IEC 61966-2-1's red, green and blue chromaticities, and D65.
const RED: (f64, f64) = (0.640, 0.330);
const GREEN: (f64, f64) = (0.300, 0.600);
const BLUE: (f64, f64) = (0.150, 0.060);
const WHITE: (f64, f64) = (0.3127, 0.3290);

/// ICC.1:2001-04 A.1's D50 and E.1.2's linearized Bradford matrix.
const D50: [f64; 3] = [0.9642, 1.0, 0.8249];
const BRADFORD: [[f64; 3]; 3] = [
    [0.8951, 0.2664, -0.1614],
    [-0.7502, 1.7135, 0.0367],
    [0.0389, -0.0685, 1.0296],
];

/// The tags RC-19.1 lists, in the table's order.
const TAGS: [&[u8; 4]; 10] = [
    b"desc", b"cprt", b"wtpt", b"chad", b"rXYZ", b"gXYZ", b"bXYZ", b"rTRC", b"gTRC", b"bTRC",
];

/// The description and copyright texts RC-19.1 lists.
const DESCRIPTION: &str = "sRGB IEC 61966-2-1 (tinker-pdf)";
const COPYRIGHT: &str = "Copyright tinker-pdf contributors, MIT OR Apache-2.0";

/// How many entries the shared tone curve carries.
const ENTRIES: usize = 1024;

/// The big-endian `u32` at `at`.
fn u32_at(bytes: &[u8], at: usize) -> u32 {
    u32::from_be_bytes(bytes[at..at + 4].try_into().expect("four bytes"))
}

/// The big-endian `u16` at `at`.
fn u16_at(bytes: &[u8], at: usize) -> u16 {
    u16::from_be_bytes(bytes[at..at + 2].try_into().expect("two bytes"))
}

/// The tag table: each entry's signature, offset and size, in order.
fn table() -> Vec<([u8; 4], usize, usize)> {
    let count = u32_at(SRGB_PROFILE, 128) as usize;
    (0..count)
        .map(|index| {
            let at = 132 + 12 * index;
            let signature: [u8; 4] = SRGB_PROFILE[at..at + 4].try_into().expect("four bytes");
            (
                signature,
                u32_at(SRGB_PROFILE, at + 4) as usize,
                u32_at(SRGB_PROFILE, at + 8) as usize,
            )
        })
        .collect()
}

/// The data element the tag `signature` names.
fn element(signature: &[u8; 4]) -> &'static [u8] {
    let (_, offset, size) = table()
        .into_iter()
        .find(|(found, _, _)| found == signature)
        .unwrap_or_else(|| panic!("the profile carries {:?}", std::str::from_utf8(signature)));
    &SRGB_PROFILE[offset..offset + size]
}

/// An `XYZType` element's one `XYZNumber`, as the three raw
/// `s15Fixed16Number`s.
fn xyz_raw(signature: &[u8; 4]) -> [i32; 3] {
    let data = element(signature);
    assert_eq!(&data[..4], b"XYZ ", "{signature:?} is an XYZType");
    assert_eq!(data.len(), 20, "{signature:?} holds one XYZNumber");
    [8, 12, 16].map(|at| u32_at(data, at) as i32)
}

/// The `XYZ` value of the chromaticity `(x, y)` at a luminance of one.
fn xyz((x, y): (f64, f64)) -> [f64; 3] {
    [x / y, 1.0, (1.0 - x - y) / y]
}

/// The solution `v` of `a v = b`, by Gaussian elimination with partial
/// pivoting.
fn solve(a: [[f64; 3]; 3], b: [f64; 3]) -> [f64; 3] {
    let mut rows = [
        [a[0][0], a[0][1], a[0][2], b[0]],
        [a[1][0], a[1][1], a[1][2], b[1]],
        [a[2][0], a[2][1], a[2][2], b[2]],
    ];
    for column in 0..3 {
        let pivot = (column..3)
            .max_by(|&p, &q| rows[p][column].abs().total_cmp(&rows[q][column].abs()))
            .expect("a row");
        rows.swap(column, pivot);
        for row in 0..3 {
            if row != column {
                let factor = rows[row][column] / rows[column][column];
                for k in column..4 {
                    rows[row][k] -= factor * rows[column][k];
                }
            }
        }
    }
    [0, 1, 2].map(|row| rows[row][3] / rows[row][row])
}

/// `m` times `v`.
fn times(m: &[[f64; 3]; 3], v: [f64; 3]) -> [f64; 3] {
    [0, 1, 2].map(|row| (0..3).map(|k| m[row][k] * v[k]).sum())
}

/// The adaptation matrix ICC.1:2001-04 E.7 gives from D65 to D50, by rows:
/// column `j` is the adapted image of the unit vector `e_j`, found by taking
/// it into cone space, scaling it, and solving back out of cone space.
fn adaptation() -> [[f64; 3]; 3] {
    let source = times(&BRADFORD, xyz(WHITE));
    let target = times(&BRADFORD, D50);
    let mut by_columns = [[0.0; 3]; 3];
    for (column, out) in by_columns.iter_mut().enumerate() {
        let mut unit = [0.0; 3];
        unit[column] = 1.0;
        let cone = times(&BRADFORD, unit);
        let scaled = [0, 1, 2].map(|axis| cone[axis] * target[axis] / source[axis]);
        *out = solve(BRADFORD, scaled);
    }
    [0, 1, 2].map(|row| [0, 1, 2].map(|column| by_columns[column][row]))
}

/// The colorant columns, each `[X, Y, Z]`: each primary's `XYZ` scaled so
/// the three sum to D65, then adapted.
fn columns() -> [[f64; 3]; 3] {
    let primaries = [xyz(RED), xyz(GREEN), xyz(BLUE)];
    let matrix = [0, 1, 2].map(|row| [0, 1, 2].map(|column| primaries[column][row]));
    let scale = solve(matrix, xyz(WHITE));
    let adapt = adaptation();
    [0, 1, 2].map(|column| times(&adapt, primaries[column].map(|v| v * scale[column])))
}

/// Asserts `raw` is `value` as an `s15Fixed16Number` (5.3.3): the nearest
/// step, within what two computations in floating point can disagree by.
#[track_caller]
fn is_step_of(raw: i32, value: f64, what: &str) {
    let exact = value * 65536.0;
    assert!(
        (f64::from(raw) - exact).abs() <= 0.5 + 1e-6,
        "{what}: {raw} stored, {exact} computed"
    );
}

/// **The colorants are IEC 61966-2-1's primaries adapted to D50**: each of
/// `rXYZ`, `gXYZ` and `bXYZ` is the nearest `s15Fixed16Number` to the column
/// recomputed here from the chromaticities and D65 through E.1.2's Bradford
/// transform, and `chad` is the transform, by rows; each is what RC-19.1's
/// table gives, and the parsed model reads the columns back as stored.
#[test]
fn the_colorants_are_iec_61966_2_1s_primaries_adapted_to_d50() {
    let want = columns();
    let stored = [b"rXYZ", b"gXYZ", b"bXYZ"].map(xyz_raw);
    for (column, signature) in ["rXYZ", "gXYZ", "bXYZ"].iter().enumerate() {
        for axis in 0..3 {
            is_step_of(
                stored[column][axis],
                want[column][axis],
                &format!("{signature} component {axis}"),
            );
        }
    }
    assert_eq!(
        stored,
        [
            [0x6FA0, 0x38F5, 0x0390],
            [0x6297, 0xB787, 0x18D9],
            [0x249F, 0x0F84, 0xB6C3],
        ],
        "RC-19.1's table"
    );
    // Before rounding the columns sum to the PCS white, since the adaptation
    // takes D65 to it exactly; after it, each sum rounds to the white's own
    // step.
    for axis in 0..3 {
        let exact: f64 = want.iter().map(|column| column[axis]).sum();
        assert!((exact - D50[axis]).abs() < 1e-12, "axis {axis}: {exact}");
        let sum: i32 = stored.iter().map(|column| column[axis]).sum();
        is_step_of(sum, D50[axis], &format!("the sum on axis {axis}"));
    }

    let chad = element(b"chad");
    assert_eq!(&chad[..4], b"sf32");
    assert_eq!(chad.len(), 8 + 9 * 4, "nine s15Fixed16Numbers");
    let adapt = adaptation();
    let raw: Vec<i32> = (0..9).map(|k| u32_at(chad, 8 + 4 * k) as i32).collect();
    for row in 0..3 {
        for column in 0..3 {
            is_step_of(
                raw[3 * row + column],
                adapt[row][column],
                &format!("chad row {row} column {column}"),
            );
        }
    }
    assert_eq!(
        raw.iter().map(|&v| v as u32).collect::<Vec<u32>>(),
        [
            0x0001_0C42,
            0x0000_05DE,
            0xFFFF_F325,
            0x0000_0793,
            0x0000_FD90,
            0xFFFF_FBA1,
            0xFFFF_FDA2,
            0x0000_03DC,
            0x0000_C06E,
        ],
        "RC-19.1's chad"
    );
    // The adaptation takes D65 to D50, which is what makes the media white
    // the PCS white (A.3.1.1).
    let adapted_white = times(&adapt, xyz(WHITE));
    for axis in 0..3 {
        assert!((adapted_white[axis] - D50[axis]).abs() < 1e-12);
    }

    let profile = Profile::parse(SRGB_PROFILE).expect("the profile parses");
    let Some(Model::MatrixTrc { columns: read, .. }) = profile.model else {
        panic!("a matrix/TRC model: {:?}", profile.model);
    };
    for column in 0..3 {
        for axis in 0..3 {
            assert_eq!(
                read[column][axis],
                f64::from(stored[column][axis]) / 65536.0,
                "the parser reads column {column} as stored"
            );
        }
    }
}

/// IEC 61966-2-1's transfer function at the encoded value `c`, as linear
/// light: computed with the platform's `powf`, apart from `build.rs`.
fn transfer(c: f64) -> f64 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

/// **The tone curve is IEC 61966-2-1's transfer function**: the three TRC
/// tags name one `curveType` element of 1 024 entries, every entry the
/// nearest `uInt16Number` to the function at `i / 1023` — exactly so along
/// the linear segment, which needs no power — rising from 0 to 65 535; and the
/// parsed model holds it as a sampled curve.
#[test]
fn the_trc_is_iec_61966_2_1s_transfer_function() {
    let entries: Vec<_> = table()
        .into_iter()
        .filter(|(signature, _, _)| [b"rTRC", b"gTRC", b"bTRC"].contains(&signature))
        .collect();
    assert_eq!(entries.len(), 3);
    assert!(
        entries
            .iter()
            .all(|(_, offset, size)| (*offset, *size) == (entries[0].1, entries[0].2)),
        "the three curves are one element: {entries:?}"
    );
    let curve = element(b"rTRC");
    assert_eq!(&curve[..4], b"curv");
    assert_eq!(u32_at(curve, 4), 0, "reserved");
    assert_eq!(u32_at(curve, 8) as usize, ENTRIES);
    assert_eq!(curve.len(), 12 + 2 * ENTRIES, "the data's size, no padding");

    let stored: Vec<u16> = (0..ENTRIES).map(|i| u16_at(curve, 12 + 2 * i)).collect();
    let mut linear_segment = 0;
    for (i, &value) in stored.iter().enumerate() {
        let c = i as f64 / (ENTRIES - 1) as f64;
        let exact = transfer(c) * 65535.0;
        assert!(
            (f64::from(value) - exact).abs() <= 0.5 + 1e-6,
            "entry {i}: {value} stored, {exact} computed"
        );
        if c <= 0.04045 {
            linear_segment += 1;
            assert_eq!(
                value,
                (c / 12.92 * 65535.0).round() as u16,
                "entry {i}, on the linear segment"
            );
        }
    }
    assert_eq!(
        linear_segment, 42,
        "entries 0 to 41 are on the linear segment"
    );
    assert_eq!((stored[0], stored[ENTRIES - 1]), (0, 65535));
    assert!(
        stored.windows(2).all(|pair| pair[0] <= pair[1]),
        "the curve never falls"
    );

    let profile = Profile::parse(SRGB_PROFILE).expect("the profile parses");
    let Some(Model::MatrixTrc { curves, .. }) = profile.model else {
        panic!("a matrix/TRC model");
    };
    for read in curves {
        assert_eq!(read, Curve::Sampled(stored.clone()));
    }
}

/// **The profile parses, compiles and is sRGB**: the parser reads a
/// matrix/TRC model of an `RGB ` display profile into `XYZ` with the PCS
/// white as its media white, every intent compiles, and the engine's own
/// conversion takes every grey level and the three primaries back to
/// themselves within one level — ICC-absolute colorimetry the same bytes as
/// media-relative, since the media white is the PCS white.
#[test]
fn the_profile_parses_compiles_and_is_srgb() {
    let profile = Profile::parse(SRGB_PROFILE).expect("the profile parses");
    assert!(matches!(profile.model, Some(Model::MatrixTrc { .. })));
    assert_eq!(
        (profile.class, profile.space, profile.pcs, profile.version),
        (*b"mntr", *b"RGB ", *b"XYZ ", 2)
    );
    assert_eq!(profile.white, Some(PCS_WHITE), "the media white is D50");
    assert_eq!(profile.media_scale(), [1.0; 3]);
    assert_eq!(data_space(SRGB_PROFILE), Some(*b"RGB "));

    let relative =
        Transform::compile(&profile, Intent::RelativeColorimetric).expect("a relative transform");
    for intent in Intent::ALL {
        assert!(
            Transform::compile(&profile, intent).is_some(),
            "{intent:?} compiles"
        );
    }
    let absolute =
        Transform::compile(&profile, Intent::AbsoluteColorimetric).expect("an absolute transform");

    let within_one = |got: (u8, u8, u8), want: [u8; 3], what: &str| {
        for (channel, (g, w)) in [got.0, got.1, got.2].into_iter().zip(want).enumerate() {
            assert!(
                (i32::from(g) - i32::from(w)).abs() <= 1,
                "{what}, channel {channel}: {got:?} for {want:?}"
            );
        }
    };
    for level in 0..=255u8 {
        let value = f64::from(level) / 255.0;
        let got = relative.apply(&[value; 3]);
        within_one(got, [level; 3], &format!("grey {level}"));
        assert_eq!(absolute.apply(&[value; 3]), got, "grey {level}, absolute");
    }
    for (primary, want) in [
        ([1.0, 0.0, 0.0], [255, 0, 0]),
        ([0.0, 1.0, 0.0], [0, 255, 0]),
        ([0.0, 0.0, 1.0], [0, 0, 255]),
    ] {
        let got = relative.apply(&primary);
        within_one(got, want, &format!("primary {want:?}"));
        assert_eq!(absolute.apply(&primary), got, "{want:?}, absolute");
    }
}

/// **The header and the tags are what RC-19.1 lists**: every header field of
/// ICC.1:2001-04's Table 9, and a table of ten tags in RC-19.1's order, each
/// element on a four-byte boundary, sized as its type defines it, padded with
/// zeros to the next and none overlapping, the last ending where the profile
/// does; `desc`, `cprt` and `wtpt` holding RC-19.1's texts and the PCS
/// white.
#[test]
fn the_header_and_tags_are_what_rc_19_1_lists() {
    let p = SRGB_PROFILE;
    assert_eq!(u32_at(p, 0) as usize, p.len(), "6.1.1: the profile's size");
    assert_eq!(u32_at(p, 4), 0, "6.1.2: no preferred CMM");
    assert_eq!(u32_at(p, 8), 0x0240_0000, "6.1.3: version 2.4.0");
    assert_eq!(&p[12..16], b"mntr", "6.1.4: a display device");
    assert_eq!(&p[16..20], b"RGB ", "6.1.5");
    assert_eq!(&p[20..24], b"XYZ ", "6.1.6");
    let date: Vec<u16> = (0..6).map(|k| u16_at(p, 24 + 2 * k)).collect();
    assert!(
        date[0] >= 1994
            && (1..=12).contains(&date[1])
            && (1..=31).contains(&date[2])
            && date[3] < 24
            && date[4] < 60
            && date[5] < 60,
        "5.3.1: a dateTimeNumber {date:?}"
    );
    assert_eq!(&p[36..40], b"acsp");
    assert_eq!(u32_at(p, 40), 0, "6.1.7: no primary platform");
    assert_eq!(u32_at(p, 44), 0, "6.1.8: no flags");
    assert_eq!(&p[48..56], &[0; 8], "6.1.9: no manufacturer or model");
    assert_eq!(&p[56..64], &[0; 8], "6.1.10: no attributes");
    assert_eq!(u32_at(p, 64), 0, "6.1.11: perceptual");
    assert_eq!(
        [u32_at(p, 68), u32_at(p, 72), u32_at(p, 76)],
        [0x0000_F6D6, 0x0001_0000, 0x0000_D32D],
        "A.1: the PCS illuminant, D50"
    );
    assert_eq!(u32_at(p, 80), 0, "6.1.12: no creator");
    assert!(p[84..128].iter().all(|&byte| byte == 0), "reserved");

    let entries = table();
    assert_eq!(
        entries.iter().map(|(s, _, _)| *s).collect::<Vec<_>>(),
        TAGS.map(|tag| *tag).to_vec(),
        "RC-19.1's tags, in its order"
    );
    let data_start = 128 + 4 + 12 * entries.len();
    let mut elements: Vec<(usize, usize)> = entries
        .iter()
        .map(|(_, offset, size)| (*offset, *size))
        .collect();
    elements.dedup();
    assert_eq!(elements.len(), 8, "eight elements, the curves one");
    let mut expected_at = data_start;
    for &(offset, size) in &elements {
        assert_eq!(offset % 4, 0, "6.2.2: aligned");
        assert_eq!(offset, expected_at, "each element follows the last");
        let end = offset + size;
        assert!(
            p[end..end.next_multiple_of(4).min(p.len())]
                .iter()
                .all(|&byte| byte == 0),
            "padding is zero"
        );
        expected_at = end.next_multiple_of(4);
    }
    assert_eq!(expected_at, p.len(), "the last element ends the profile");

    let sizes: Vec<usize> = entries.iter().map(|(_, _, size)| *size).collect();
    assert_eq!(
        sizes,
        [
            90 + DESCRIPTION.len() + 1,
            8 + COPYRIGHT.len() + 1,
            20,
            44,
            20,
            20,
            20,
            12 + 2 * ENTRIES,
            12 + 2 * ENTRIES,
            12 + 2 * ENTRIES,
        ],
        "6.2.3: each element its type's own size"
    );
    for signature in TAGS {
        assert_eq!(u32_at(element(signature), 4), 0, "{signature:?}: reserved");
    }

    let desc = element(b"desc");
    assert_eq!(&desc[..4], b"desc", "6.5.17: textDescriptionType");
    let count = u32_at(desc, 8) as usize;
    assert_eq!(count, DESCRIPTION.len() + 1, "the count holds the null");
    assert_eq!(&desc[12..12 + count - 1], DESCRIPTION.as_bytes());
    assert_eq!(desc[12 + count - 1], 0);
    let after = 12 + count;
    assert_eq!(
        u32_at(desc, after),
        0,
        "no Unicode language code: not localised"
    );
    assert_eq!(u32_at(desc, after + 4), 0, "no Unicode description");
    assert_eq!(u16_at(desc, after + 8), 0, "no ScriptCode code");
    assert_eq!(desc[after + 10], 0, "no ScriptCode description");
    assert!(desc[after + 11..].iter().all(|&byte| byte == 0));
    assert_eq!(desc.len(), after + 11 + 67, "the 67-byte Macintosh field");

    let cprt = element(b"cprt");
    assert_eq!(&cprt[..4], b"text", "6.5.18: textType");
    assert_eq!(&cprt[8..cprt.len() - 1], COPYRIGHT.as_bytes());
    assert_eq!(cprt[cprt.len() - 1], 0, "terminated by a null");

    assert_eq!(
        xyz_raw(b"wtpt").map(|v| v as u32),
        [0x0000_F6D6, 0x0001_0000, 0x0000_D32D],
        "A.3.1.1: the media white is the PCS white"
    );
}

/// **A copy cut short is refused by name**: the profile's own size field says
/// what is missing, so a truncated copy is not read as a smaller profile, and
/// one cut inside its header is too short to be one.
#[test]
fn a_truncated_copy_is_refused_by_name() {
    use tinker_pdf_color::icc::IccError;
    assert_eq!(
        Profile::parse(&SRGB_PROFILE[..SRGB_PROFILE.len() - 1]).err(),
        Some(IccError::SizeMismatch)
    );
    assert_eq!(
        Profile::parse(&SRGB_PROFILE[..100]).err(),
        Some(IccError::TooShort)
    );
}
