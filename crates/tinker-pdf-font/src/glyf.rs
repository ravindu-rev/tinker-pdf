//! TrueType glyph outlines, from the `glyf` table.
//!
//! A TrueType contour is a ring of points, each on or off the curve. Two
//! consecutive off-curve points imply an on-curve midpoint between them —
//! the compression trick that makes the format compact and its parsers
//! fiddly.
//!
//! Hinting is deliberately not implemented. The bytecode interpreter is a
//! large surface with a patent history and a habit of making small text
//! *differently* wrong rather than better; every modern renderer at these
//! resolutions runs unhinted with good anti-aliasing instead.

use crate::outline::{Outline, Segment};
use crate::sfnt::Sfnt;

/// How deep a composite glyph may nest before recursion is refused.
const MAX_COMPONENT_DEPTH: u32 = 8;

/// How many component glyphs one outline may draw in total.
///
/// A depth cap alone is not enough: a composite that lists sixty-four
/// components, each referring back to itself, stays within eight levels while
/// asking for 64^8 glyphs. The budget bounds the *work*, which is the thing
/// that actually has to be finite.
const MAX_COMPONENTS: u32 = 256;

fn be16(data: &[u8], at: usize) -> Option<u16> {
    let b = data.get(at..at + 2)?;
    Some(u16::from_be_bytes([b[0], b[1]]))
}

fn be32(data: &[u8], at: usize) -> Option<u32> {
    let b = data.get(at..at + 4)?;
    Some(u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
}

fn i16_at(data: &[u8], at: usize) -> Option<i16> {
    be16(data, at).map(|v| v as i16)
}

/// The `loca` table: where each glyph's data begins.
fn glyph_range(sfnt: &Sfnt, glyph: u16) -> Option<(usize, usize)> {
    let head = sfnt.table(0x6865_6164)?;
    // head.indexToLocFormat: 0 means 16-bit offsets stored halved.
    let long_format = i16_at(head, 50)? != 0;
    let loca = sfnt.table(0x6C6F_6361)?;

    let index = usize::from(glyph);
    let (start, end) = if long_format {
        (
            be32(loca, index * 4)? as usize,
            be32(loca, index * 4 + 4)? as usize,
        )
    } else {
        (
            usize::from(be16(loca, index * 2)?) * 2,
            usize::from(be16(loca, index * 2 + 2)?) * 2,
        )
    };

    // An empty range is a glyph with no outline, such as a space.
    (end > start).then_some((start, end))
}

/// Reads one glyph's outline in font units.
#[must_use]
pub fn outline(sfnt: &Sfnt, glyph: u16) -> Option<Outline> {
    let mut out = Outline::default();
    let mut budget = MAX_COMPONENTS;
    append(
        sfnt,
        glyph,
        0.0,
        0.0,
        1.0,
        0.0,
        0.0,
        1.0,
        0,
        &mut budget,
        &mut out,
    );
    Some(out)
}

/// Appends a glyph's contours, transformed by the given 2×3 matrix.
#[allow(clippy::too_many_arguments)]
fn append(
    sfnt: &Sfnt,
    glyph: u16,
    dx: f64,
    dy: f64,
    a: f64,
    b: f64,
    c: f64,
    d: f64,
    depth: u32,
    budget: &mut u32,
    out: &mut Outline,
) {
    if depth > MAX_COMPONENT_DEPTH || *budget == 0 {
        return;
    }
    *budget -= 1;
    let Some(glyf) = sfnt.table(0x676C_7966) else {
        return;
    };
    let Some((start, end)) = glyph_range(sfnt, glyph) else {
        return; // No outline: a space, or a glyph past the end.
    };
    let Some(data) = glyf.get(start..end) else {
        return;
    };

    let Some(contours) = i16_at(data, 0) else {
        return;
    };

    let transform = |x: f64, y: f64| (a * x + c * y + dx, b * x + d * y + dy);

    if contours >= 0 {
        simple(data, contours as usize, &transform, out);
    } else {
        composite(sfnt, data, dx, dy, a, b, c, d, depth, budget, out);
    }
}

/// A simple glyph: contours of on- and off-curve points.
fn simple(
    data: &[u8],
    contours: usize,
    transform: &dyn Fn(f64, f64) -> (f64, f64),
    out: &mut Outline,
) {
    // The header is 10 bytes, then one end-point index per contour.
    let mut ends = Vec::with_capacity(contours.min(256));
    for i in 0..contours {
        let Some(end) = be16(data, 10 + i * 2) else {
            return;
        };
        ends.push(usize::from(end));
    }
    let Some(&last) = ends.last() else { return };
    let count = last + 1;
    if count > 1 << 16 {
        return;
    }

    // Skip the instructions: hinting bytecode, which is not interpreted.
    let Some(instruction_len) = be16(data, 10 + contours * 2) else {
        return;
    };
    let mut at = 10 + contours * 2 + 2 + usize::from(instruction_len);

    // Flags, run-length encoded by the REPEAT bit.
    let mut flags = Vec::with_capacity(count);
    while flags.len() < count {
        let Some(&flag) = data.get(at) else { return };
        at += 1;
        flags.push(flag);
        if flag & 0x08 != 0 {
            let Some(&repeats) = data.get(at) else {
                return;
            };
            at += 1;
            for _ in 0..repeats {
                if flags.len() >= count {
                    break;
                }
                flags.push(flag);
            }
        }
    }

    // Coordinates are deltas, one or two bytes by the flag bits.
    let mut xs = Vec::with_capacity(count);
    let mut x = 0i32;
    for &flag in &flags {
        if flag & 0x02 != 0 {
            let Some(&delta) = data.get(at) else { return };
            at += 1;
            x += if flag & 0x10 != 0 {
                i32::from(delta)
            } else {
                -i32::from(delta)
            };
        } else if flag & 0x10 == 0 {
            let Some(delta) = i16_at(data, at) else {
                return;
            };
            at += 2;
            x += i32::from(delta);
        }
        xs.push(x);
    }

    let mut ys = Vec::with_capacity(count);
    let mut y = 0i32;
    for &flag in &flags {
        if flag & 0x04 != 0 {
            let Some(&delta) = data.get(at) else { return };
            at += 1;
            y += if flag & 0x20 != 0 {
                i32::from(delta)
            } else {
                -i32::from(delta)
            };
        } else if flag & 0x20 == 0 {
            let Some(delta) = i16_at(data, at) else {
                return;
            };
            at += 2;
            y += i32::from(delta);
        }
        ys.push(y);
    }

    let mut first = 0usize;
    for &end in &ends {
        if end < first || end >= count {
            break;
        }
        emit_contour(&flags, &xs, &ys, first, end, transform, out);
        first = end + 1;
    }
}

/// Emits one closed contour, resolving implied on-curve midpoints.
fn emit_contour(
    flags: &[u8],
    xs: &[i32],
    ys: &[i32],
    first: usize,
    last: usize,
    transform: &dyn Fn(f64, f64) -> (f64, f64),
    out: &mut Outline,
) {
    let len = last + 1 - first;
    if len == 0 {
        return;
    }

    let point = |i: usize| -> Option<(f64, f64, bool)> {
        let index = first + (i % len);
        let on_curve = flags.get(index)? & 0x01 != 0;
        let x = f64::from(*xs.get(index)?);
        let y = f64::from(*ys.get(index)?);
        Some((x, y, on_curve))
    };

    // Find a starting point that is on the curve; if none is, the midpoint
    // between the last and first off-curve points serves, which is what the
    // implied-midpoint rule means at the seam.
    let mut start_index = None;
    for i in 0..len {
        if point(i).is_some_and(|(_, _, on)| on) {
            start_index = Some(i);
            break;
        }
    }

    let (start_x, start_y) = match start_index {
        Some(i) => match point(i) {
            Some((x, y, _)) => (x, y),
            None => return,
        },
        None => {
            let (Some((ax, ay, _)), Some((bx, by, _))) = (point(0), point(len - 1)) else {
                return;
            };
            ((ax + bx) / 2.0, (ay + by) / 2.0)
        }
    };

    let (tx, ty) = transform(start_x, start_y);
    out.push(Segment::MoveTo { x: tx, y: ty });

    let begin = start_index.unwrap_or(0);
    let mut pending: Option<(f64, f64)> = None;

    for step in 1..=len {
        let Some((px, py, on_curve)) = point(begin + step) else {
            break;
        };

        match (pending, on_curve) {
            (None, true) => {
                let (x, y) = transform(px, py);
                out.push(Segment::LineTo { x, y });
            }
            (None, false) => pending = Some((px, py)),
            (Some((cx, cy)), true) => {
                let (c, p) = (transform(cx, cy), transform(px, py));
                out.push(Segment::QuadTo {
                    cx: c.0,
                    cy: c.1,
                    x: p.0,
                    y: p.1,
                });
                pending = None;
            }
            (Some((cx, cy)), false) => {
                // Two off-curve points in a row: an on-curve point is implied
                // halfway between them.
                let (mx, my) = ((cx + px) / 2.0, (cy + py) / 2.0);
                let (c, m) = (transform(cx, cy), transform(mx, my));
                out.push(Segment::QuadTo {
                    cx: c.0,
                    cy: c.1,
                    x: m.0,
                    y: m.1,
                });
                pending = Some((px, py));
            }
        }
    }

    // Close back to the start, through any control point still pending.
    if let Some((cx, cy)) = pending {
        let (c, s) = (transform(cx, cy), transform(start_x, start_y));
        out.push(Segment::QuadTo {
            cx: c.0,
            cy: c.1,
            x: s.0,
            y: s.1,
        });
    }
    out.push(Segment::Close);
}

/// A composite glyph: other glyphs, each with a placement.
#[allow(clippy::too_many_arguments)]
fn composite(
    sfnt: &Sfnt,
    data: &[u8],
    dx: f64,
    dy: f64,
    a: f64,
    b: f64,
    c: f64,
    d: f64,
    depth: u32,
    budget: &mut u32,
    out: &mut Outline,
) {
    let mut at = 10usize;
    let mut guard = 0u32;

    loop {
        guard += 1;
        if guard > 64 {
            return;
        }
        let (Some(flags), Some(index)) = (be16(data, at), be16(data, at + 2)) else {
            return;
        };
        at += 4;

        // ARG_1_AND_2_ARE_WORDS decides the argument width; ARGS_ARE_XY_VALUES
        // decides whether they are offsets or point indices to match.
        let words = flags & 0x0001 != 0;
        let xy_values = flags & 0x0002 != 0;

        let (arg1, arg2) = if words {
            let (Some(a1), Some(a2)) = (i16_at(data, at), i16_at(data, at + 2)) else {
                return;
            };
            at += 4;
            (f64::from(a1), f64::from(a2))
        } else {
            let (Some(&a1), Some(&a2)) = (data.get(at), data.get(at + 1)) else {
                return;
            };
            at += 2;
            (f64::from(a1 as i8), f64::from(a2 as i8))
        };

        // A component's own 2×2, in F2Dot14.
        let f2dot14 = |at: usize| i16_at(data, at).map(|v| f64::from(v) / 16384.0);
        let (ca, cb, cc, cd) = if flags & 0x0008 != 0 {
            let Some(scale) = f2dot14(at) else { return };
            at += 2;
            (scale, 0.0, 0.0, scale)
        } else if flags & 0x0040 != 0 {
            let (Some(sx), Some(sy)) = (f2dot14(at), f2dot14(at + 2)) else {
                return;
            };
            at += 4;
            (sx, 0.0, 0.0, sy)
        } else if flags & 0x0080 != 0 {
            let (Some(m0), Some(m1), Some(m2), Some(m3)) = (
                f2dot14(at),
                f2dot14(at + 2),
                f2dot14(at + 4),
                f2dot14(at + 6),
            ) else {
                return;
            };
            at += 8;
            (m0, m1, m2, m3)
        } else {
            (1.0, 0.0, 0.0, 1.0)
        };

        // Point-matching placement is rare and needs both glyphs' points; an
        // offset of zero is a better answer than refusing the glyph.
        let (ox, oy) = if xy_values { (arg1, arg2) } else { (0.0, 0.0) };

        // Compose the component's transform with the one already in effect.
        let na = ca * a + cb * c;
        let nb = ca * b + cb * d;
        let nc = cc * a + cd * c;
        let nd = cc * b + cd * d;
        let ndx = ox * a + oy * c + dx;
        let ndy = ox * b + oy * d + dy;

        append(
            sfnt,
            index,
            ndx,
            ndy,
            na,
            nb,
            nc,
            nd,
            depth + 1,
            budget,
            out,
        );
        if *budget == 0 {
            return;
        }

        if flags & 0x0020 == 0 {
            return; // No MORE_COMPONENTS.
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds a font with `head`, `loca` and `glyf` around one simple glyph.
    fn font_with_glyph(glyph_data: &[u8]) -> Vec<u8> {
        font_with_glyphs(&[glyph_data])
    }

    /// The same, for a `glyf` holding several glyphs.
    ///
    /// A composite needs this: a component is a **glyph id into this same
    /// table**, so a one-glyph font can only build a composite that refers to
    /// itself, which is the recursion guard's case and not the format's.
    fn font_with_glyphs(glyphs: &[&[u8]]) -> Vec<u8> {
        let mut head = vec![0u8; 54];
        head[18..20].copy_from_slice(&1000u16.to_be_bytes());
        head[50..52].copy_from_slice(&1i16.to_be_bytes()); // long loca

        let mut loca = Vec::new();
        let mut glyf: Vec<u8> = Vec::new();
        loca.extend_from_slice(&0u32.to_be_bytes());
        for glyph in glyphs {
            glyf.extend_from_slice(glyph);
            // Two-align the next glyph's start, as a real producer does. The
            // pad byte lands at the tail of the range just recorded, where no
            // reader of a well-formed glyph ever gets to it.
            if glyf.len() % 2 != 0 {
                glyf.push(0);
            }
            loca.extend_from_slice(&(glyf.len() as u32).to_be_bytes());
        }

        let tables: [(&[u8; 4], &[u8]); 3] =
            [(b"head", &head), (b"loca", &loca), (b"glyf", &glyf)];

        let mut out = Vec::new();
        out.extend_from_slice(&0x0001_0000u32.to_be_bytes());
        out.extend_from_slice(&(tables.len() as u16).to_be_bytes());
        out.extend_from_slice(&[0; 6]);

        let mut offset = 12 + tables.len() * 16;
        let mut body = Vec::new();
        for (tag, data) in tables {
            out.extend_from_slice(tag);
            out.extend_from_slice(&0u32.to_be_bytes());
            out.extend_from_slice(&(offset as u32).to_be_bytes());
            out.extend_from_slice(&(data.len() as u32).to_be_bytes());
            offset += data.len();
            body.extend_from_slice(data);
        }
        out.extend_from_slice(&body);
        out
    }

    /// A triangle: three on-curve points in one contour.
    fn triangle() -> Vec<u8> {
        let mut g = Vec::new();
        g.extend_from_slice(&1i16.to_be_bytes()); // numberOfContours
        g.extend_from_slice(&0i16.to_be_bytes()); // xMin
        g.extend_from_slice(&0i16.to_be_bytes()); // yMin
        g.extend_from_slice(&100i16.to_be_bytes()); // xMax
        g.extend_from_slice(&100i16.to_be_bytes()); // yMax
        g.extend_from_slice(&2u16.to_be_bytes()); // endPtsOfContours[0]
        g.extend_from_slice(&0u16.to_be_bytes()); // instructionLength
                                                  // Flags: on-curve, both coordinates as signed words.
        g.extend_from_slice(&[0x01, 0x01, 0x01]);
        // x deltas: 0, +100, -100
        g.extend_from_slice(&0i16.to_be_bytes());
        g.extend_from_slice(&100i16.to_be_bytes());
        g.extend_from_slice(&(-100i16).to_be_bytes());
        // y deltas: 0, 0, +100
        g.extend_from_slice(&0i16.to_be_bytes());
        g.extend_from_slice(&0i16.to_be_bytes());
        g.extend_from_slice(&100i16.to_be_bytes());
        g
    }

    #[test]
    fn a_simple_contour_reads_as_lines() {
        let data = font_with_glyph(&triangle());
        let sfnt = Sfnt::parse(&data).expect("a font");
        let outline = outline(&sfnt, 0).expect("an outline");

        assert_eq!(
            outline.segments.first(),
            Some(&Segment::MoveTo { x: 0.0, y: 0.0 })
        );
        assert!(outline
            .segments
            .contains(&Segment::LineTo { x: 100.0, y: 0.0 }));
        assert!(outline
            .segments
            .contains(&Segment::LineTo { x: 0.0, y: 100.0 }));
        assert_eq!(outline.segments.last(), Some(&Segment::Close));
        assert_eq!(outline.bounds(), Some((0.0, 0.0, 100.0, 100.0)));
    }

    #[test]
    fn two_off_curve_points_imply_a_midpoint() {
        let mut g = Vec::new();
        g.extend_from_slice(&1i16.to_be_bytes());
        g.extend_from_slice(&[0; 8]); // bounding box, unread
        g.extend_from_slice(&2u16.to_be_bytes()); // three points
        g.extend_from_slice(&0u16.to_be_bytes());
        // On-curve, then two off-curve.
        g.extend_from_slice(&[0x01, 0x00, 0x00]);
        for delta in [0i16, 50, 50] {
            g.extend_from_slice(&delta.to_be_bytes());
        }
        for delta in [0i16, 100, -100] {
            g.extend_from_slice(&delta.to_be_bytes());
        }

        let data = font_with_glyph(&g);
        let sfnt = Sfnt::parse(&data).expect("a font");
        let outline = outline(&sfnt, 0).expect("an outline");

        // The implied midpoint between (50,100) and (100,0) is (75,50).
        assert!(
            outline.segments.iter().any(|s| matches!(
                s,
                Segment::QuadTo { x, y, .. } if (*x - 75.0).abs() < 0.01 && (*y - 50.0).abs() < 0.01
            )),
            "expected an implied midpoint, got {:?}",
            outline.segments
        );
    }

    /// An OpenType/CFF program: the `OTTO` tag, a `CFF ` table holding the
    /// outlines, and no `glyf` at all.
    fn opentype_cff() -> Vec<u8> {
        let mut head = vec![0u8; 54];
        head[18..20].copy_from_slice(&1000u16.to_be_bytes()); // unitsPerEm
        let mut hhea = vec![0u8; 36];
        hhea[34..36].copy_from_slice(&1u16.to_be_bytes()); // numberOfHMetrics

        let tables: [(&[u8; 4], &[u8]); 4] = [
            (b"CFF ", &[1, 0, 4, 1, 0, 0, 0, 0]),
            (b"cmap", &[0; 4]),
            (b"head", &head),
            (b"hhea", &hhea),
        ];

        let mut out = b"OTTO".to_vec();
        out.extend_from_slice(&(tables.len() as u16).to_be_bytes());
        out.extend_from_slice(&[0; 6]);

        let mut offset = 12 + tables.len() * 16;
        let mut body = Vec::new();
        for (tag, data) in tables {
            out.extend_from_slice(tag);
            out.extend_from_slice(&0u32.to_be_bytes());
            out.extend_from_slice(&(offset as u32).to_be_bytes());
            out.extend_from_slice(&(data.len() as u32).to_be_bytes());
            offset += data.len();
            body.extend_from_slice(data);
        }
        out.extend_from_slice(&body);
        out
    }

    /// The trap behind the OpenType/CFF fix: asked for a glyph from a face
    /// that keeps its outlines in `CFF `, this module answers `Some(empty)`
    /// rather than `None` — and an empty outline reads downstream as a
    /// legitimate space, like a word gap, so a whole face drew as blank and
    /// reported nothing. A caller must therefore look for `CFF ` itself
    /// before asking here; `None` would have let it fall through instead.
    #[test]
    fn an_opentype_cff_face_yields_an_empty_outline_rather_than_none() {
        let data = opentype_cff();
        let sfnt = Sfnt::parse(&data).expect("the sfnt parser accepts OTTO");
        assert!(sfnt.table(0x676C_7966).is_none(), "there is no glyf");
        assert!(sfnt.table(0x4346_4620).is_some(), "the outlines are in CFF");

        let outline = outline(&sfnt, 3).expect("an answer rather than a refusal");
        assert!(outline.is_empty(), "and the answer is an empty outline");
    }

    #[test]
    fn a_glyph_with_no_outline_is_empty_not_missing() {
        // loca giving an empty range: a space.
        let data = font_with_glyph(&[]);
        let sfnt = Sfnt::parse(&data).expect("a font");
        let outline = outline(&sfnt, 0).expect("a result");
        assert!(outline.is_empty());
    }

    #[test]
    fn truncated_and_hostile_glyphs_do_not_panic() {
        for cut in 0..triangle().len() {
            let truncated: Vec<u8> = triangle().into_iter().take(cut).collect();
            let data = font_with_glyph(&truncated);
            if let Some(sfnt) = Sfnt::parse(&data) {
                let _ = outline(&sfnt, 0);
            }
        }

        // A composite claiming components forever.
        let mut composite = Vec::new();
        composite.extend_from_slice(&(-1i16).to_be_bytes());
        composite.extend_from_slice(&[0; 8]);
        for _ in 0..100 {
            composite.extend_from_slice(&0x0022u16.to_be_bytes()); // MORE|XY
            composite.extend_from_slice(&0u16.to_be_bytes()); // itself
            composite.extend_from_slice(&[0, 0]);
        }
        let data = font_with_glyph(&composite);
        if let Some(sfnt) = Sfnt::parse(&data) {
            let _ = outline(&sfnt, 0);
        }
    }

    /// A quadratic bowl: two consecutive off-curve points, so the reader has
    /// to imply the on-curve midpoint between them. [`triangle`] never takes
    /// that branch — every one of its points is on the curve.
    fn curve() -> Vec<u8> {
        let mut g = Vec::new();
        g.extend_from_slice(&1i16.to_be_bytes()); // numberOfContours
        g.extend_from_slice(&0i16.to_be_bytes()); // xMin
        g.extend_from_slice(&0i16.to_be_bytes()); // yMin
        g.extend_from_slice(&200i16.to_be_bytes()); // xMax
        g.extend_from_slice(&150i16.to_be_bytes()); // yMax
        g.extend_from_slice(&3u16.to_be_bytes()); // endPtsOfContours[0]
        g.extend_from_slice(&0u16.to_be_bytes()); // instructionLength
                                                  // On, off, off, on: the middle pair implies a midpoint.
        g.extend_from_slice(&[0x01, 0x00, 0x00, 0x01]);
        for dx in [0i16, 50, 100, 50] {
            g.extend_from_slice(&dx.to_be_bytes());
        }
        for dy in [0i16, 150, 0, -150] {
            g.extend_from_slice(&dy.to_be_bytes());
        }
        g
    }

    /// Two contours in the compact encodings: the REPEAT run-length on the
    /// flags and one-byte coordinate deltas.
    ///
    /// That is how a real font stores most of its points, and it is a
    /// different path through [`simple`] from either glyph above — both of
    /// those spell every flag out and take every delta as a signed word.
    fn compact_contours() -> Vec<u8> {
        let mut g = Vec::new();
        g.extend_from_slice(&2i16.to_be_bytes()); // numberOfContours
        g.extend_from_slice(&[0; 8]); // bounding box, unread
        g.extend_from_slice(&2u16.to_be_bytes()); // endPtsOfContours[0]
        g.extend_from_slice(&5u16.to_be_bytes()); // endPtsOfContours[1]
        g.extend_from_slice(&0u16.to_be_bytes()); // instructionLength

        // ON|X_SHORT|Y_SHORT|REPEAT|X_POSITIVE|Y_POSITIVE, then the same with
        // Y_POSITIVE cleared so the second contour walks back down. Each
        // repeat byte of 2 stands for three points.
        g.extend_from_slice(&[0x3F, 2, 0x1F, 2]);
        g.extend_from_slice(&[30; 6]); // x deltas, all positive
        g.extend_from_slice(&[30; 6]); // y deltas: +30 then -30, by the flag
        g
    }

    /// One component record: the flags, the glyph it draws, its two arguments
    /// at the width the flags choose, and whatever transform follows them.
    fn component(flags: u16, index: u16, arg1: i16, arg2: i16, transform: &[i16]) -> Vec<u8> {
        let mut g = Vec::new();
        g.extend_from_slice(&flags.to_be_bytes());
        g.extend_from_slice(&index.to_be_bytes());
        if flags & 0x0001 != 0 {
            // ARG_1_AND_2_ARE_WORDS.
            g.extend_from_slice(&arg1.to_be_bytes());
            g.extend_from_slice(&arg2.to_be_bytes());
        } else {
            g.push(arg1 as i8 as u8);
            g.push(arg2 as i8 as u8);
        }
        for value in transform {
            g.extend_from_slice(&value.to_be_bytes());
        }
        g
    }

    /// A composite glyph: `numberOfContours` of -1, an unread bounding box,
    /// then the component records back to back.
    fn composite_glyph(components: &[Vec<u8>]) -> Vec<u8> {
        let mut g = Vec::new();
        g.extend_from_slice(&(-1i16).to_be_bytes());
        g.extend_from_slice(&[0; 8]);
        for record in components {
            g.extend_from_slice(record);
        }
        g
    }

    /// A quarter turn in F2Dot14, where 16384 is 1.0.
    const TURN: [i16; 4] = [0, 16384, -16384, 0];

    /// `fuzz/corpus/truetype/simple-contours.ttf`: three glyphs that between
    /// them take every branch of [`simple`] — spelled-out flags with word
    /// deltas, an implied midpoint, and the REPEAT and short-vector
    /// encodings across two contours.
    fn simple_contours_seed() -> Vec<u8> {
        font_with_glyphs(&[&triangle(), &curve(), &compact_contours()])
    }

    /// `fuzz/corpus/truetype/composite-transforms.ttf`: glyph 2 is a
    /// composite whose five components use every component encoding
    /// [`composite`] reads.
    ///
    /// The last one is the pair easiest to get wrong and rarest in the wild:
    /// byte-wide arguments, and `ARGS_ARE_XY_VALUES` **clear**, so the
    /// arguments are point indices to match rather than an offset.
    fn composite_transforms_seed() -> Vec<u8> {
        let glyph = composite_glyph(&[
            // WORDS|XY|MORE: a plain offset, no transform.
            component(0x0023, 0, 200, 0, &[]),
            // WORDS|XY|WE_HAVE_A_SCALE|MORE: one scale for both axes.
            component(0x002B, 1, 0, 400, &[8192]),
            // WORDS|XY|WE_HAVE_AN_X_AND_Y_SCALE|MORE.
            component(0x0063, 0, 500, 0, &[24576, 8192]),
            // WORDS|XY|WE_HAVE_A_TWO_BY_TWO|MORE.
            component(0x00A3, 1, 0, -300, &TURN),
            // Nothing set: byte arguments, point matching, and no
            // MORE_COMPONENTS, which is what ends the record list.
            component(0x0000, 0, 1, 2, &[]),
        ]);
        font_with_glyphs(&[&triangle(), &curve(), &glyph])
    }

    /// `fuzz/corpus/truetype/composite-nested.ttf`: a composite of a
    /// composite of a composite, which is the recursion the target's own
    /// header calls the interesting part.
    ///
    /// Glyph 3 draws glyph 2, which draws glyph 1 twice, which draws the
    /// triangle at glyph 0 — four levels of the eight [`MAX_COMPONENT_DEPTH`]
    /// allows, so the seed exercises the recursion rather than the guard that
    /// stops it.
    fn composite_nested_seed() -> Vec<u8> {
        // Glyph 1: the triangle at unit scale, stated as a scale so the
        // F2Dot14 path runs at every level.
        let one = composite_glyph(&[component(0x000B, 0, 0, 0, &[16384])]);
        // Glyph 2: glyph 1 twice, the second one turned.
        let two = composite_glyph(&[
            component(0x0023, 1, 0, 0, &[]),
            component(0x0083, 1, 300, 0, &TURN),
        ]);
        // Glyph 3: glyph 2, lifted.
        let three = composite_glyph(&[component(0x0003, 2, 0, 200, &[])]);
        font_with_glyphs(&[&triangle(), &one, &two, &three])
    }

    /// Writes the three `glyf`-bearing seeds in `fuzz/corpus/truetype/`, so
    /// the seeds and the fixtures cannot drift apart.
    ///
    /// Run with `--ignored` when a fixture changes; the corpus is committed,
    /// and a run that rewrites it is a diff to look at rather than to apply
    /// blindly.
    #[test]
    #[ignore = "writes into fuzz/corpus/truetype, which is committed"]
    fn write_the_fuzz_seeds() {
        let base =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fuzz/corpus/truetype");
        for (name, bytes) in [
            ("simple-contours.ttf", simple_contours_seed()),
            ("composite-transforms.ttf", composite_transforms_seed()),
            ("composite-nested.ttf", composite_nested_seed()),
        ] {
            std::fs::write(base.join(name), bytes).expect("the corpus directory is there");
        }
    }

    /// The fixtures draw, which is the half `fuzz_seeds.rs` cannot check.
    ///
    /// That test counts the seeds **on disk**; this holds the fixtures that
    /// write them to what they claim, so a fixture edited into a refusal
    /// fails here rather than being written out as a seed reaching nothing.
    #[test]
    fn the_written_seeds_draw_what_they_claim_to() {
        let data = simple_contours_seed();
        let sfnt = Sfnt::parse(&data).expect("a font");
        for glyph in 0..3 {
            let drawn = outline(&sfnt, glyph).expect("a result");
            assert!(
                !drawn.is_empty(),
                "simple-contours glyph {glyph} drew nothing"
            );
        }

        let data = composite_transforms_seed();
        let sfnt = Sfnt::parse(&data).expect("a font");
        let assembled = outline(&sfnt, 2).expect("a result");
        let one_component = outline(&sfnt, 0).expect("a result");
        assert!(
            assembled.segments.len() > one_component.segments.len(),
            "the composite drew no more than one of its five components"
        );

        let data = composite_nested_seed();
        let sfnt = Sfnt::parse(&data).expect("a font");
        let deep = outline(&sfnt, 3).expect("a result");
        let shallow = outline(&sfnt, 1).expect("a result");
        assert!(!shallow.is_empty(), "the innermost composite drew nothing");
        assert!(
            deep.segments.len() > shallow.segments.len(),
            "four levels of nesting drew no more than one"
        );
    }
}
