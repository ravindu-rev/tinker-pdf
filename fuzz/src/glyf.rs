//! A minimal `sfnt` of TrueType outlines, generated: the `glyf`, `loca`,
//! `head`, `hhea`, `hmtx` and `maxp` tables of the OpenType specification,
//! simple glyphs in every flag and coordinate form, and composite glyphs in
//! every component form, nested, for `glyf_outlines` to read back and subset.
//!
//! Every glyph stays inside the reader's caps — composites nest at most
//! [`MAX_LEVEL`] deep with at most [`MAX_COMPONENTS`] components each, well
//! inside `glyf::outline`'s eight levels and 256 components — so an outline is
//! the whole glyph's, and each glyph's contour count and advance are known
//! from what was written.

use crate::Choices;

/// The most glyphs a face holds.
pub const MAX_GLYPHS: usize = 16;
/// The most contours a simple glyph holds.
pub const MAX_CONTOURS: usize = 4;
/// The most points a contour holds.
pub const MAX_POINTS: usize = 8;
/// The most components a composite holds.
pub const MAX_COMPONENTS: usize = 3;
/// How deep composites nest: a composite of composites of composites of
/// composites at most.
pub const MAX_LEVEL: u32 = 4;

/// Simple-glyph flags (`glyf`, table of simple glyph flags).
const ON_CURVE: u8 = 0x01;
const X_SHORT: u8 = 0x02;
const Y_SHORT: u8 = 0x04;
const REPEAT: u8 = 0x08;
const X_SAME_OR_POSITIVE: u8 = 0x10;
const Y_SAME_OR_POSITIVE: u8 = 0x20;
const OVERLAP_SIMPLE: u8 = 0x40;

/// Composite-glyph flags (`glyf`, table of component flags).
const ARG_1_AND_2_ARE_WORDS: u16 = 0x0001;
const ARGS_ARE_XY_VALUES: u16 = 0x0002;
const ROUND_XY_TO_GRID: u16 = 0x0004;
const WE_HAVE_A_SCALE: u16 = 0x0008;
const MORE_COMPONENTS: u16 = 0x0020;
const WE_HAVE_AN_X_AND_Y_SCALE: u16 = 0x0040;
const WE_HAVE_A_TWO_BY_TWO: u16 = 0x0080;
const WE_HAVE_INSTRUCTIONS: u16 = 0x0100;
const USE_MY_METRICS: u16 = 0x0200;
const OVERLAP_COMPOUND: u16 = 0x0400;
const SCALED_COMPONENT_OFFSET: u16 = 0x0800;
const UNSCALED_COMPONENT_OFFSET: u16 = 0x1000;

/// What one glyph is, as the generator wrote it.
#[derive(Clone, Copy, Debug, Default)]
struct Info {
    points: usize,
    contours: usize,
    level: u32,
}

/// A generated face and what the generator knows of it.
pub struct Face {
    /// The `sfnt`.
    pub bytes: Vec<u8>,
    /// Each glyph's advance, as `hmtx` states it.
    pub advances: Vec<u16>,
    /// The closed contours each glyph draws, its components' included.
    pub contours: Vec<usize>,
    /// Whether `loca` is the long form.
    pub long_loca: bool,
    /// How many glyphs are composites, how many of them have a composite
    /// component, and how many components are placed by matching points.
    pub composites: usize,
    /// Composites with a composite component.
    pub nested: usize,
    /// Components placed by point matching.
    pub matched: usize,
    /// Every component flag some component carries, or'd together.
    pub component_flags: u16,
    /// Every simple-glyph flag some point carries, or'd together, with
    /// `REPEAT` among them where a run was compressed.
    pub point_flags: u8,
    /// Glyphs with no outline at all: an empty `loca` range.
    pub empty: usize,
}

fn be16(out: &mut Vec<u8>, value: u16) {
    out.extend_from_slice(&value.to_be_bytes());
}

fn be16s(out: &mut Vec<u8>, value: i16) {
    out.extend_from_slice(&value.to_be_bytes());
}

fn be32(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_be_bytes());
}

/// A coordinate delta: small mostly, sometimes zero, sometimes past a byte.
fn delta(c: &mut Choices) -> i32 {
    match c.below(8) {
        0 => 0,
        1 => c.range(256, 2000) as i32 * if c.flag() { 1 } else { -1 },
        _ => c.range(-255, 255) as i32,
    }
}

/// One axis of a point: its flag bits and its bytes, in a form chosen among
/// those that hold the delta.
fn axis(c: &mut Choices, d: i32, short: u8, same: u8, bytes: &mut Vec<u8>) -> u8 {
    let mut forms: Vec<u8> = vec![0];
    if d == 0 {
        forms.push(1);
    }
    if d.unsigned_abs() <= 255 {
        forms.push(2);
    }
    match forms[c.below(forms.len())] {
        // A signed 16-bit delta.
        0 => {
            be16s(bytes, d as i16);
            0
        }
        // The same as the previous coordinate: no bytes.
        1 => same,
        // A byte of magnitude, its sign in the other bit.
        _ => {
            bytes.push(d.unsigned_abs() as u8);
            if d > 0 || (d == 0 && c.flag()) {
                short | same
            } else {
                short
            }
        }
    }
}

/// A simple glyph: its bytes and what it holds.
fn simple(c: &mut Choices, point_flags: &mut u8) -> (Vec<u8>, Info) {
    let contours = c.below(MAX_CONTOURS + 1);
    let mut ends = Vec::with_capacity(contours);
    let mut flags = Vec::new();
    let mut xs = Vec::new();
    let mut ys = Vec::new();
    let (mut x, mut y) = (0i32, 0i32);
    let (mut x_min, mut y_min, mut x_max, mut y_max) = (i32::MAX, i32::MAX, i32::MIN, i32::MIN);
    let mut total = 0usize;
    for _ in 0..contours {
        let points = 1 + c.below(MAX_POINTS);
        total += points;
        ends.push((total - 1) as u16);
        for _ in 0..points {
            // Turned back where it would leave the 16-bit box the header's
            // bounds are stated in.
            let mut dx = delta(c);
            if (x + dx).abs() > 16_000 {
                dx = -dx;
            }
            let mut dy = delta(c);
            if (y + dy).abs() > 16_000 {
                dy = -dy;
            }
            x += dx;
            y += dy;
            x_min = x_min.min(x);
            y_min = y_min.min(y);
            x_max = x_max.max(x);
            y_max = y_max.max(y);
            let mut flag = if c.flag() { ON_CURVE } else { 0 };
            flag |= axis(c, dx, X_SHORT, X_SAME_OR_POSITIVE, &mut xs);
            flag |= axis(c, dy, Y_SHORT, Y_SAME_OR_POSITIVE, &mut ys);
            flags.push(flag);
        }
    }
    if let Some(first) = flags.first_mut() {
        if c.below(4) == 3 {
            *first |= OVERLAP_SIMPLE;
        }
    }
    for flag in &flags {
        *point_flags |= *flag;
    }

    // The flags, a run of one flag compressed with REPEAT where chosen.
    let mut packed = Vec::with_capacity(flags.len());
    let mut at = 0usize;
    while at < flags.len() {
        let flag = flags[at];
        let run = flags[at..]
            .iter()
            .take_while(|f| **f == flag)
            .count()
            .min(256);
        if run > 1 && c.flag() {
            packed.push(flag | REPEAT);
            packed.push((run - 1) as u8);
            *point_flags |= REPEAT;
            at += run;
        } else {
            packed.push(flag);
            at += 1;
        }
    }

    let mut out = Vec::new();
    be16s(&mut out, contours as i16);
    if total == 0 {
        for _ in 0..4 {
            be16s(&mut out, 0);
        }
    } else {
        for bound in [x_min, y_min, x_max, y_max] {
            be16s(&mut out, bound as i16);
        }
    }
    for end in &ends {
        be16(&mut out, *end);
    }
    let instructions = c.bytes(6);
    be16(&mut out, instructions.len() as u16);
    out.extend_from_slice(&instructions);
    out.extend_from_slice(&packed);
    out.extend_from_slice(&xs);
    out.extend_from_slice(&ys);
    (
        out,
        Info {
            points: total,
            contours,
            level: 0,
        },
    )
}

/// An F2Dot14 (`glyf`'s component scales): the edges and a random one.
fn f2dot14(c: &mut Choices) -> i16 {
    match c.below(8) {
        0 => 0x4000,
        1 => 0x2000,
        2 => -0x4000,
        3 => 0x7FFF,
        4 => i16::MIN,
        5 => 0,
        _ => c.range(-0x8000, 0x7FFF) as i16,
    }
}

/// A composite glyph over the glyphs before it, or `None` where none is
/// shallow enough to be a component.
fn composite(c: &mut Choices, before: &[Info], face: &mut Face) -> Option<(Vec<u8>, Info)> {
    let candidates: Vec<usize> = (0..before.len())
        .filter(|g| before[*g].level < MAX_LEVEL)
        .collect();
    if candidates.is_empty() {
        return None;
    }
    let count = 1 + c.below(MAX_COMPONENTS);
    let mut out = Vec::new();
    be16s(&mut out, -1);
    for _ in 0..4 {
        be16s(&mut out, 0);
    }
    let mut points = 0usize;
    let mut contours = 0usize;
    let mut level = 0u32;
    let mut metrics_taken = false;
    let mut nested = false;
    let mut last_flags = 0u16;
    for index in 0..count {
        let glyph = candidates[c.below(candidates.len())];
        let info = before[glyph];
        let mut flags = 0u16;
        let words = c.flag();
        if words {
            flags |= ARG_1_AND_2_ARE_WORDS;
        }
        // Point matching needs a point of the composite so far and one of
        // the component, each a number its argument width holds.
        let fits = |n: usize| words || n <= 256;
        let matching =
            points > 0 && info.points > 0 && fits(points) && fits(info.points) && c.below(3) == 2;
        let mut args = Vec::new();
        if matching {
            face.matched += 1;
            let parent = c.below(points) as u16;
            let child = c.below(info.points) as u16;
            if words {
                be16(&mut args, parent);
                be16(&mut args, child);
            } else {
                args.push(parent as u8);
                args.push(child as u8);
            }
        } else {
            flags |= ARGS_ARE_XY_VALUES;
            if c.flag() {
                flags |= ROUND_XY_TO_GRID;
            }
            if words {
                be16s(&mut args, c.range(-2000, 2000) as i16);
                be16s(&mut args, c.range(-2000, 2000) as i16);
            } else {
                args.push(c.range(-128, 127) as i8 as u8);
                args.push(c.range(-128, 127) as i8 as u8);
            }
        }
        let mut scales = Vec::new();
        match c.below(4) {
            0 => {}
            1 => {
                flags |= WE_HAVE_A_SCALE;
                be16s(&mut scales, f2dot14(c));
            }
            2 => {
                flags |= WE_HAVE_AN_X_AND_Y_SCALE;
                be16s(&mut scales, f2dot14(c));
                be16s(&mut scales, f2dot14(c));
            }
            _ => {
                flags |= WE_HAVE_A_TWO_BY_TWO;
                for _ in 0..4 {
                    be16s(&mut scales, f2dot14(c));
                }
            }
        }
        match c.below(4) {
            1 => flags |= SCALED_COMPONENT_OFFSET,
            2 => flags |= UNSCALED_COMPONENT_OFFSET,
            _ => {}
        }
        if !metrics_taken && c.below(4) == 3 {
            flags |= USE_MY_METRICS;
            metrics_taken = true;
        }
        if index == 0 && c.below(4) == 3 {
            flags |= OVERLAP_COMPOUND;
        }
        if index + 1 < count {
            flags |= MORE_COMPONENTS;
        } else if c.below(3) == 2 {
            flags |= WE_HAVE_INSTRUCTIONS;
        }
        be16(&mut out, flags);
        be16(&mut out, glyph as u16);
        out.extend_from_slice(&args);
        out.extend_from_slice(&scales);
        face.component_flags |= flags;
        last_flags = flags;
        points += info.points;
        contours += info.contours;
        level = level.max(info.level + 1);
        nested |= info.level > 0;
    }
    if last_flags & WE_HAVE_INSTRUCTIONS != 0 {
        let instructions = c.bytes(6);
        be16(&mut out, instructions.len() as u16);
        out.extend_from_slice(&instructions);
    }
    face.composites += 1;
    if nested {
        face.nested += 1;
    }
    Some((
        out,
        Info {
            points,
            contours,
            level,
        },
    ))
}

/// The checksum of a table, its length padded to a whole number of words.
fn checksum(table: &[u8]) -> u32 {
    table.chunks(4).fold(0u32, |sum, word| {
        let mut bytes = [0u8; 4];
        bytes[..word.len()].copy_from_slice(word);
        sum.wrapping_add(u32::from_be_bytes(bytes))
    })
}

/// A face of one to [`MAX_GLYPHS`] glyphs, built from `c`.
pub fn generate(c: &mut Choices) -> Face {
    let count = 1 + c.below(MAX_GLYPHS);
    let long = c.flag();
    let mut face = Face {
        bytes: Vec::new(),
        advances: Vec::new(),
        contours: Vec::new(),
        long_loca: long,
        composites: 0,
        nested: 0,
        matched: 0,
        component_flags: 0,
        point_flags: 0,
        empty: 0,
    };

    let mut infos: Vec<Info> = Vec::with_capacity(count);
    let mut glyphs: Vec<Vec<u8>> = Vec::with_capacity(count);
    for g in 0..count {
        let made = match c.below(8) {
            // No outline: an empty `loca` range, as a space has.
            0 => {
                face.empty += 1;
                (Vec::new(), Info::default())
            }
            1..=3 if g > 0 => match composite(c, &infos, &mut face) {
                Some(made) => made,
                None => simple(c, &mut face.point_flags),
            },
            _ => simple(c, &mut face.point_flags),
        };
        let (mut data, info) = made;
        // The short `loca` form stores offsets halved, so a glyph is padded
        // to an even length; the long form pads to a word now and then.
        if !long || c.flag() {
            while data.len() % 2 != 0 {
                data.push(0);
            }
        }
        face.contours.push(info.contours);
        infos.push(info);
        glyphs.push(data);
    }

    let mut glyf = Vec::new();
    let mut offsets = Vec::with_capacity(count + 1);
    for data in &glyphs {
        offsets.push(glyf.len());
        glyf.extend_from_slice(data);
    }
    offsets.push(glyf.len());
    let mut loca = Vec::new();
    for offset in &offsets {
        if long {
            be32(&mut loca, *offset as u32);
        } else {
            be16(&mut loca, (*offset / 2) as u16);
        }
    }

    // `hmtx`: the first `numberOfHMetrics` glyphs have an advance of their
    // own, and the rest repeat the last.
    let metrics = 1 + c.below(count);
    let mut hmtx = Vec::new();
    let mut stated = Vec::with_capacity(metrics);
    for g in 0..count {
        if g < metrics {
            let advance = c.range(0, 2400) as u16;
            stated.push(advance);
            be16(&mut hmtx, advance);
        }
        be16s(&mut hmtx, c.range(-200, 200) as i16);
    }
    face.advances = (0..count).map(|g| stated[g.min(metrics - 1)]).collect();

    let units = *c.pick(&[1000u16, 1024, 2048, 16, 16_384]);
    let mut head = Vec::new();
    be32(&mut head, 0x0001_0000);
    be32(&mut head, 0x0001_0000);
    be32(&mut head, 0);
    be32(&mut head, 0x5F0F_3CF5);
    be16(&mut head, 0x000B);
    be16(&mut head, units);
    head.extend_from_slice(&[0u8; 16]);
    for _ in 0..4 {
        be16s(&mut head, 0);
    }
    be16(&mut head, 0);
    be16(&mut head, 8);
    be16s(&mut head, 2);
    be16s(&mut head, i16::from(long));
    be16s(&mut head, 0);

    let mut hhea = Vec::new();
    be32(&mut hhea, 0x0001_0000);
    be16s(&mut hhea, 800);
    be16s(&mut hhea, -200);
    be16s(&mut hhea, 0);
    be16(&mut hhea, stated.iter().copied().max().unwrap_or(0));
    for _ in 0..3 {
        be16s(&mut hhea, 0);
    }
    be16s(&mut hhea, 1);
    for _ in 0..7 {
        be16s(&mut hhea, 0);
    }
    be16(&mut hhea, metrics as u16);

    let mut maxp = Vec::new();
    be32(&mut maxp, 0x0001_0000);
    be16(&mut maxp, count as u16);
    let simple_points = infos
        .iter()
        .filter(|i| i.level == 0)
        .map(|i| i.points)
        .max()
        .unwrap_or(0);
    let simple_contours = infos
        .iter()
        .filter(|i| i.level == 0)
        .map(|i| i.contours)
        .max()
        .unwrap_or(0);
    be16(&mut maxp, simple_points as u16);
    be16(&mut maxp, simple_contours as u16);
    be16(
        &mut maxp,
        infos.iter().map(|i| i.points).max().unwrap_or(0) as u16,
    );
    be16(
        &mut maxp,
        infos.iter().map(|i| i.contours).max().unwrap_or(0) as u16,
    );
    be16(&mut maxp, 2);
    for _ in 0..6 {
        be16(&mut maxp, 0);
    }
    be16(&mut maxp, MAX_COMPONENTS as u16);
    be16(
        &mut maxp,
        infos.iter().map(|i| i.level).max().unwrap_or(0) as u16,
    );

    // The directory (`sfnt` version, the table records in tag order), the
    // tables each on a word boundary, and the whole file's checksum stated
    // in `head`.
    let mut tables: Vec<([u8; 4], Vec<u8>)> = vec![
        (*b"glyf", glyf),
        (*b"head", head),
        (*b"hhea", hhea),
        (*b"hmtx", hmtx),
        (*b"loca", loca),
        (*b"maxp", maxp),
    ];
    let version = if c.flag() { 0x0001_0000 } else { 0x7472_7565 };
    let n = tables.len() as u16;
    let power = 1u16 << (15 - n.leading_zeros());
    let mut out = Vec::new();
    be32(&mut out, version);
    be16(&mut out, n);
    be16(&mut out, power * 16);
    be16(&mut out, power.trailing_zeros() as u16);
    be16(&mut out, n * 16 - power * 16);
    let mut at = 12 + 16 * tables.len();
    let mut records = Vec::new();
    for (tag, data) in &tables {
        records.push((*tag, checksum(data), at, data.len()));
        at += data.len().div_ceil(4) * 4;
    }
    for (tag, sum, offset, len) in &records {
        out.extend_from_slice(tag);
        be32(&mut out, *sum);
        be32(&mut out, *offset as u32);
        be32(&mut out, *len as u32);
    }
    for (_, data) in &mut tables {
        out.extend_from_slice(data);
        while out.len() % 4 != 0 {
            out.push(0);
        }
    }
    let head_at = records[1].2;
    let adjustment = 0xB1B0_AFBAu32.wrapping_sub(checksum(&out));
    out[head_at + 8..head_at + 12].copy_from_slice(&adjustment.to_be_bytes());
    face.bytes = out;
    face
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_input_makes_a_face_that_parses() {
        for input in [&b""[..], b"\x05\x09", &[0xff; 300], &[2; 700]] {
            let face = generate(&mut Choices::new(input));
            let sfnt = tinker_pdf_font::Sfnt::parse(&face.bytes).expect("a generated face parses");
            for g in 0..face.advances.len() as u16 {
                assert_eq!(sfnt.advance(g), Some(face.advances[usize::from(g)]));
            }
        }
    }

    #[test]
    fn the_file_checksum_is_the_magic_number() {
        let face = generate(&mut Choices::new(b"a face"));
        assert_eq!(checksum(&face.bytes), 0xB1B0_AFBA);
    }

    type Feature = fn(&Face) -> bool;

    /// Rewrites `fuzz/corpus/glyf_outlines/`: one seed per feature, each the
    /// first input of `first_input`'s sequence whose face has it.
    #[test]
    #[ignore = "writes fuzz/corpus/glyf_outlines"]
    fn write_the_fuzz_seeds() {
        let features: &[(&str, Feature)] = &[
            ("short-loca", |f| {
                !f.long_loca && f.contours.iter().sum::<usize>() >= 6
            }),
            ("long-loca", |f| {
                f.long_loca && f.contours.iter().sum::<usize>() >= 6
            }),
            ("repeated-flags", |f| {
                f.point_flags & REPEAT != 0 && f.point_flags & OVERLAP_SIMPLE != 0
            }),
            ("every-coordinate-form", |f| {
                f.point_flags & (X_SHORT | Y_SHORT | X_SAME_OR_POSITIVE | Y_SAME_OR_POSITIVE)
                    == X_SHORT | Y_SHORT | X_SAME_OR_POSITIVE | Y_SAME_OR_POSITIVE
            }),
            ("nested-composites", |f| f.nested >= 2),
            ("point-matching", |f| f.matched >= 2),
            ("scales", |f| {
                let all = WE_HAVE_A_SCALE | WE_HAVE_AN_X_AND_Y_SCALE | WE_HAVE_A_TWO_BY_TWO;
                f.component_flags & all == all
            }),
            ("component-offsets", |f| {
                let all = SCALED_COMPONENT_OFFSET | UNSCALED_COMPONENT_OFFSET | ROUND_XY_TO_GRID;
                f.component_flags & all == all
            }),
            ("word-arguments-and-metrics", |f| {
                let all = ARG_1_AND_2_ARE_WORDS
                    | USE_MY_METRICS
                    | OVERLAP_COMPOUND
                    | WE_HAVE_INSTRUCTIONS;
                f.component_flags & all == all
            }),
            ("empty-glyphs", |f| f.empty >= 2 && f.composites >= 1),
            ("sixteen-glyphs", |f| f.advances.len() == MAX_GLYPHS),
        ];
        let read = |data: &[u8]| {
            let mut choices = Choices::new(data);
            let _ = generate(&mut choices);
            choices.consumed()
        };
        let mut seeds: Vec<(&str, Vec<u8>)> = vec![("plain", Vec::new())];
        for (name, has) in features {
            let taken: Vec<Vec<u8>> = seeds.iter().map(|(_, seed)| seed.clone()).collect();
            let seed = crate::first_input(
                1500,
                200_000,
                |data| has(&generate(&mut Choices::new(data))),
                read,
                &taken,
            )
            .unwrap_or_else(|| panic!("no input has {name}"));
            assert!(
                has(&generate(&mut Choices::new(&seed))),
                "{name} lost its feature when cut"
            );
            seeds.push((name, seed));
        }
        crate::write_seeds("glyf_outlines", &seeds);
    }
}
