//! Variable faces built byte by byte, and the OpenType specification's
//! arithmetic written out a second time, so that a test reads its expected
//! value from the same statement the face was built from.
//!
//! Every table here is assembled from its chapter's field list in OpenType
//! 1.9.1 — `fvar`, `avar`, the tuple and item variation stores — and nothing
//! is borrowed from a face another producer wrote. The second half, [`spec`],
//! is the overview chapter's normalisation and interpolation transcribed
//! independently of `tinker_pdf_font::variation`: exact rationals, rounded
//! where the design's numeric model says a value is rounded and nowhere else
//! (`docs/design/variable-fonts.md`, 'The numeric model').
//!
//! # Why the allow
//!
//! Each test binary that names this module compiles its own copy, and each
//! uses the builders its tables need, so whichever does not use a builder
//! reports it dead.

#![allow(
    dead_code,
    reason = "shared by the variation test binaries; each uses a different subset"
)]

/// A four-byte tag.
pub type Tag = [u8; 4];

/// Appends a big-endian `u16`.
pub fn push16(out: &mut Vec<u8>, value: u16) {
    out.extend_from_slice(&value.to_be_bytes());
}

/// Appends a big-endian `i16`.
pub fn push_i16(out: &mut Vec<u8>, value: i16) {
    out.extend_from_slice(&value.to_be_bytes());
}

/// Appends a big-endian `u32`.
pub fn push32(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_be_bytes());
}

/// Appends a big-endian `i32`.
pub fn push_i32(out: &mut Vec<u8>, value: i32) {
    out.extend_from_slice(&value.to_be_bytes());
}

/// A user value as the 32-bit `Fixed` a table stores, for the values a test
/// writes — each a multiple of 1/65 536, so the conversion is exact.
pub fn fixed(value: f64) -> i32 {
    let scaled = value * 65_536.0;
    assert_eq!(scaled, scaled.trunc(), "{value} is not a 16.16 value");
    scaled as i32
}

/// A normalised value as 2.14, for the values a test writes — each a
/// multiple of 1/16 384.
pub fn f2dot14(value: f64) -> i16 {
    let scaled = value * 16_384.0;
    assert_eq!(scaled, scaled.trunc(), "{value} is not a 2.14 value");
    scaled as i16
}

/// A face: tables by tag, written as an sfnt whose directory is sorted, whose
/// tables are four-byte aligned and whose checksums are the specification's.
#[derive(Clone, Debug)]
pub struct Face {
    /// `0x00010000` for TrueType outlines, `OTTO` for CFF and CFF2.
    pub version: u32,
    /// The tables, in any order.
    pub tables: Vec<(Tag, Vec<u8>)>,
}

impl Face {
    /// A face with TrueType outlines and no tables yet.
    pub fn truetype() -> Face {
        Face {
            version: 0x0001_0000,
            tables: Vec::new(),
        }
    }

    /// A face with CFF-family outlines and no tables yet.
    pub fn opentype() -> Face {
        Face {
            version: u32::from_be_bytes(*b"OTTO"),
            tables: Vec::new(),
        }
    }

    /// This face with one more table, replacing one of the same tag.
    pub fn with(mut self, tag: Tag, bytes: Vec<u8>) -> Face {
        self.tables.retain(|(t, _)| *t != tag);
        self.tables.push((tag, bytes));
        self
    }

    /// This face without a table.
    pub fn without(mut self, tag: Tag) -> Face {
        self.tables.retain(|(t, _)| *t != tag);
        self
    }

    /// The face's bytes.
    pub fn build(&self) -> Vec<u8> {
        let mut tables = self.tables.clone();
        tables.sort_by(|a, b| a.0.cmp(&b.0));
        let count = u16::try_from(tables.len()).expect("a test face has few tables");
        let mut power = 1u16;
        let mut selector = 0u16;
        while power * 2 <= count {
            power *= 2;
            selector += 1;
        }
        let mut out = Vec::new();
        push32(&mut out, self.version);
        push16(&mut out, count);
        push16(&mut out, power * 16);
        push16(&mut out, selector);
        push16(&mut out, count * 16 - power * 16);
        let mut offset = 12 + tables.len() * 16;
        let mut body = Vec::new();
        for (tag, bytes) in &tables {
            out.extend_from_slice(tag);
            push32(&mut out, checksum(bytes));
            push32(&mut out, u32::try_from(offset).expect("small"));
            push32(&mut out, u32::try_from(bytes.len()).expect("small"));
            body.extend_from_slice(bytes);
            while body.len() % 4 != 0 {
                body.push(0);
            }
            offset = 12 + tables.len() * 16 + body.len();
        }
        out.extend_from_slice(&body);
        out
    }
}

/// The sum of a table's big-endian words, the last padded with zeros.
pub fn checksum(bytes: &[u8]) -> u32 {
    bytes.chunks(4).fold(0u32, |sum, chunk| {
        let mut word = [0u8; 4];
        word[..chunk.len()].copy_from_slice(chunk);
        sum.wrapping_add(u32::from_be_bytes(word))
    })
}

// ---- fvar and avar ---------------------------------------------------------

/// One axis as a test states it, in user units.
#[derive(Clone, Copy, Debug)]
pub struct AxisSpec {
    pub tag: Tag,
    pub min: f64,
    pub default: f64,
    pub max: f64,
    pub flags: u16,
    pub name_id: u16,
}

impl AxisSpec {
    /// An axis with no flags.
    pub fn new(tag: Tag, min: f64, default: f64, max: f64, name_id: u16) -> AxisSpec {
        AxisSpec {
            tag,
            min,
            default,
            max,
            flags: 0,
            name_id,
        }
    }
}

/// One named instance as a test states it.
#[derive(Clone, Debug)]
pub struct InstanceSpec {
    pub subfamily_name_id: u16,
    pub coordinates: Vec<f64>,
    pub postscript_name_id: Option<u16>,
}

impl InstanceSpec {
    /// An instance with a PostScript name ID.
    pub fn new(subfamily_name_id: u16, coordinates: &[f64], postscript: u16) -> InstanceSpec {
        InstanceSpec {
            subfamily_name_id,
            coordinates: coordinates.to_vec(),
            postscript_name_id: Some(postscript),
        }
    }
}

/// An `fvar` table (`fvar`, 'Table Formats'): the header, the axis records
/// twenty bytes apart, and the instance records after them, carrying a
/// PostScript name ID when the first instance has one.
pub fn fvar(axes: &[AxisSpec], instances: &[InstanceSpec]) -> Vec<u8> {
    let with_postscript = instances
        .first()
        .is_some_and(|i| i.postscript_name_id.is_some());
    let instance_size = axes.len() * 4 + if with_postscript { 6 } else { 4 };
    fvar_with_sizes(axes, instances, 20, instance_size)
}

/// An `fvar` table with the record sizes stated, so that a test can write
/// the extensions and the refusals the sizes allow. Records shorter than
/// their content are written whole all the same, and records longer are
/// padded with zeros.
pub fn fvar_with_sizes(
    axes: &[AxisSpec],
    instances: &[InstanceSpec],
    axis_size: usize,
    instance_size: usize,
) -> Vec<u8> {
    let mut out = Vec::new();
    push16(&mut out, 1); // majorVersion
    push16(&mut out, 0); // minorVersion
    push16(&mut out, 16); // axesArrayOffset
    push16(&mut out, 2); // reserved
    push16(&mut out, u16::try_from(axes.len()).expect("few axes"));
    push16(&mut out, u16::try_from(axis_size).expect("small"));
    push16(
        &mut out,
        u16::try_from(instances.len()).expect("few instances"),
    );
    push16(&mut out, u16::try_from(instance_size).expect("small"));
    for axis in axes {
        let start = out.len();
        out.extend_from_slice(&axis.tag);
        push_i32(&mut out, fixed(axis.min));
        push_i32(&mut out, fixed(axis.default));
        push_i32(&mut out, fixed(axis.max));
        push16(&mut out, axis.flags);
        push16(&mut out, axis.name_id);
        while out.len() < start + axis_size {
            out.push(0);
        }
    }
    for instance in instances {
        let start = out.len();
        push16(&mut out, instance.subfamily_name_id);
        push16(&mut out, 0); // flags
        for value in &instance.coordinates {
            push_i32(&mut out, fixed(*value));
        }
        if let Some(id) = instance.postscript_name_id {
            push16(&mut out, id);
        }
        while out.len() < start + instance_size {
            out.push(0);
        }
    }
    out
}

/// An `avar` table (`avar`, 'Table formats'): one segment map an axis, each a
/// list of `(fromCoordinate, toCoordinate)` in 2.14.
pub fn avar(maps: &[Vec<(i16, i16)>]) -> Vec<u8> {
    let mut out = Vec::new();
    push16(&mut out, 1); // majorVersion
    push16(&mut out, 0); // minorVersion
    push16(&mut out, 0); // reserved
    push16(&mut out, u16::try_from(maps.len()).expect("few axes"));
    for map in maps {
        push16(&mut out, u16::try_from(map.len()).expect("few maps"));
        for (from, to) in map {
            push_i16(&mut out, *from);
            push_i16(&mut out, *to);
        }
    }
    out
}

/// The three correspondences every modifying segment map holds.
pub fn identity_map() -> Vec<(i16, i16)> {
    vec![(-16_384, -16_384), (0, 0), (16_384, 16_384)]
}

// ---- The specification's arithmetic, a second time -------------------------

/// The overview chapter's normalisation and interpolation, transcribed from
/// its text without reference to the crate's code: each value an exact
/// rational until the numeric model rounds it, every rounding to the nearest
/// 1/65 536 with a half rounding up.
pub mod spec {
    /// `n / d` rounded to the nearest integer, a half rounding up.
    pub fn nearest(n: i128, d: i128) -> i128 {
        assert!(d != 0, "no zero denominators in the model");
        let (n, d) = if d < 0 { (-n, -d) } else { (n, d) };
        // floor(n/d + 1/2) = floor((2n + d) / 2d), floored toward −∞.
        let numerator = 2 * n + d;
        let denominator = 2 * d;
        let quotient = numerator / denominator;
        if numerator % denominator != 0 && numerator < 0 {
            quotient - 1
        } else {
            quotient
        }
    }

    /// An `f64` as 16.16 by the overview's conversion: the fraction times
    /// 65 536 rounded half up into the low word, the integer part's two's
    /// complement above it.
    pub fn user_fixed(value: f64) -> i64 {
        let whole = value.floor();
        let fraction = value - whole;
        let low = (fraction * 65_536.0 + 0.5).floor() as i64;
        (whole as i64) * 65_536 + low
    }

    /// The default normalisation of a 16.16 user value against an axis's
    /// 16.16 minimum, default and maximum: clamped to the axis, the segment's
    /// fraction rounded to 16.16, and clamped to [−1, 1].
    pub fn default_normalised(min: i64, default: i64, max: i64, user: i64) -> i64 {
        let user = user.clamp(min, max);
        let value = if user < default {
            nearest(
                i128::from(user - default) * 65_536,
                i128::from(default - min),
            ) as i64
        } else if user > default {
            nearest(
                i128::from(user - default) * 65_536,
                i128::from(max - default),
            ) as i64
        } else {
            0
        };
        value.clamp(-65_536, 65_536)
    }

    /// A default normalised 16.16 value through one well-formed `avar`
    /// segment map of 2.14 pairs: the first map at or past the value, and an
    /// interpolation from the one before it, ratio and product each rounded.
    pub fn through_avar(map: &[(i16, i16)], value: i64) -> i64 {
        let wide = |v: i16| i64::from(v) * 4;
        let end = map
            .iter()
            .position(|(from, _)| wide(*from) >= value)
            .expect("the map ends at one");
        let (end_from, end_to) = (wide(map[end].0), wide(map[end].1));
        if end_from == value {
            return end_to.clamp(-65_536, 65_536);
        }
        let (start_from, start_to) = (wide(map[end - 1].0), wide(map[end - 1].1));
        let ratio = nearest(
            i128::from(value - start_from) * 65_536,
            i128::from(end_from - start_from),
        );
        let product = nearest(ratio * i128::from(end_to - start_to), 65_536);
        (start_to + product as i64).clamp(-65_536, 65_536)
    }

    /// The records of a segment map the `avar` chapter keeps: each whose
    /// `fromCoordinate` passes the last kept one's and whose `toCoordinate`
    /// does not fall below it.
    pub fn avar_kept(map: &[(i16, i16)]) -> Vec<(i16, i16)> {
        let mut kept: Vec<(i16, i16)> = Vec::new();
        for &(from, to) in map {
            match kept.last() {
                Some(&(last_from, last_to)) if from <= last_from || to < last_to => {}
                _ => kept.push((from, to)),
            }
        }
        kept
    }

    /// Whether a segment map modifies its axis: −1→−1, 0→0 and 1→1 among
    /// the records it keeps.
    pub fn avar_applies(map: &[(i16, i16)]) -> bool {
        let kept = avar_kept(map);
        [-16_384, 0, 16_384]
            .iter()
            .all(|v| kept.iter().any(|(from, to)| from == v && to == v))
    }

    /// A default normalised value through an `avar` segment map as the
    /// chapter reads it: the kept records, or the value unchanged where they
    /// do not modify the axis.
    pub fn avar(map: &[(i16, i16)], value: i64) -> i64 {
        if avar_applies(map) {
            through_avar(&avar_kept(map), value)
        } else {
            value.clamp(-65_536, 65_536)
        }
    }

    /// The overview's 2.14 conversion: add 2, shift right by 2 (toward −∞).
    pub fn to_f2dot14(value: i64) -> i16 {
        let shifted = (value + 2).div_euclid(4);
        i16::try_from(shifted).expect("a normalised value")
    }

    /// One axis's scalar at a 2.14 coordinate, as a 16.16 value, from the
    /// overview's pseudo-code.
    pub fn axis_scalar(start: i16, peak: i16, end: i16, coordinate: i16) -> i64 {
        let (s, p, e, c) = (
            i128::from(start),
            i128::from(peak),
            i128::from(end),
            i128::from(coordinate),
        );
        if s > p || p > e || (s < 0 && e > 0 && p != 0) || p == 0 {
            return 65_536;
        }
        if c < s || c > e {
            return 0;
        }
        if c == p {
            return 65_536;
        }
        if c < p {
            nearest((c - s) * 65_536, p - s) as i64
        } else {
            nearest((e - c) * 65_536, e - p) as i64
        }
    }

    /// A region's scalar: the axes' scalars multiplied in order, each product
    /// rounded to 16.16.
    pub fn region_scalar(axes: &[(i16, i16, i16)], coordinates: &[i16]) -> i64 {
        let mut scalar: i128 = 65_536;
        for (index, (start, peak, end)) in axes.iter().enumerate() {
            let c = coordinates.get(index).copied().unwrap_or(0);
            let factor = i128::from(axis_scalar(*start, *peak, *end, c));
            scalar = nearest(scalar * factor, 65_536);
        }
        scalar as i64
    }

    /// The start, peak and end a peak alone implies.
    pub fn implied(peak: i16) -> (i16, i16, i16) {
        if peak > 0 {
            (0, peak, peak)
        } else if peak < 0 {
            (peak, peak, 0)
        } else {
            (0, 0, 0)
        }
    }

    /// A 16.16 value rounded to whole units, a half rounding up.
    pub fn round_units(value: i64) -> i64 {
        nearest(i128::from(value), 65_536) as i64
    }
}
