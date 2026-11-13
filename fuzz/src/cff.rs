//! A bare CFF program, generated (Adobe Technical Notes 5176 and 5177):
//! INDEXes and DICTs built from choices, and Type 2 charstrings drawn from
//! every operator, for `cff_charstrings` to read back and subset.
//!
//! Every program stays inside the reader's caps — ten levels of subroutine,
//! its operation budget, the outline's segment cap — so an outline is the
//! whole program's, and every glyph's advance is known from what was written.

use std::collections::{BTreeMap, BTreeSet};

use crate::Choices;

/// The most glyphs a font holds.
pub const MAX_GLYPHS: usize = 12;
/// The most operators a program holds before its end.
pub const MAX_OPERATORS: usize = 24;
/// The subroutines of an INDEX that hold content; the rest are `return`.
pub const MAX_REAL: usize = 6;
/// Subroutine counts: small ones, and either side of each bias edge, which
/// an INDEX takes one time in four.
const SMALL: &[usize] = &[0, 1, 2, 5];
/// The 107 and 1 131 edge seven times in eight: an INDEX past 33 899
/// subroutines costs a subset tens of times what a small one does.
const EDGES: &[usize] = &[
    1239, 1240, 1239, 1240, 1239, 1240, 1239, 1240, 1239, 1240, 1239, 1240, 1239, 1240, 33_899,
    33_900,
];

/// Type 2's subroutine bias (5177, 4.7).
fn bias(count: usize) -> i32 {
    if count < 1240 {
        107
    } else if count < 33_900 {
        1131
    } else {
        32_768
    }
}

/// An INDEX (5176, 5) with offsets of at least `width` bytes.
fn index(items: &[Vec<u8>], width: u8) -> Vec<u8> {
    if items.is_empty() {
        return vec![0, 0];
    }
    let total: usize = items.iter().map(Vec::len).sum();
    let last = total + 1;
    let narrowest = match last {
        0..=0xFF => 1,
        0x100..=0xFFFF => 2,
        0x1_0000..=0xFF_FFFF => 3,
        _ => 4,
    };
    let size = usize::from(width).clamp(narrowest, 4);
    let mut out = (items.len() as u16).to_be_bytes().to_vec();
    out.push(size as u8);
    let mut offset = 1usize;
    out.extend_from_slice(&(offset as u32).to_be_bytes()[4 - size..]);
    for item in items {
        offset += item.len();
        out.extend_from_slice(&(offset as u32).to_be_bytes()[4 - size..]);
    }
    for item in items {
        out.extend_from_slice(item);
    }
    out
}

/// A DICT entry, its operands in the five-byte integer form so that a DICT's
/// length does not depend on the offsets it holds.
fn entry(out: &mut Vec<u8>, op: u16, operands: &[i32]) {
    for value in operands {
        out.push(29);
        out.extend_from_slice(&value.to_be_bytes());
    }
    if op > 0xFF {
        out.push(12);
        out.push((op & 0xFF) as u8);
    } else {
        out.push(op as u8);
    }
}

/// A charstring operand (5177, 3.2), in an encoding chosen among those that
/// hold it, as the value it reads as.
fn number(c: &mut Choices, out: &mut Vec<u8>, value: i32) -> f64 {
    let mut forms = vec![28u8, 255];
    if (-107..=107).contains(&value) {
        forms.push(0);
    }
    if (108..=1131).contains(&value) || (-1131..=-108).contains(&value) {
        forms.push(1);
    }
    let form = forms[c.below(forms.len())];
    match form {
        0 => out.push((value + 139) as u8),
        1 if value > 0 => {
            let v = value - 108;
            out.push(247 + (v >> 8) as u8);
            out.push((v & 0xFF) as u8);
        }
        1 => {
            let v = -value - 108;
            out.push(251 + (v >> 8) as u8);
            out.push((v & 0xFF) as u8);
        }
        28 => {
            out.push(28);
            out.extend_from_slice(&(value.clamp(-32_768, 32_767) as i16).to_be_bytes());
            return f64::from(value.clamp(-32_768, 32_767));
        }
        _ => {
            out.push(255);
            out.extend_from_slice(&(value.clamp(-32_768, 32_767) << 16).to_be_bytes());
            return f64::from(value.clamp(-32_768, 32_767));
        }
    }
    f64::from(value)
}

/// A 16.16 operand whose value has a fraction.
fn fraction(c: &mut Choices, out: &mut Vec<u8>) {
    let raw = c.range(-0x00FF_FFFF, 0x00FF_FFFF) as i32;
    out.push(255);
    out.extend_from_slice(&raw.to_be_bytes());
}

/// An operand for a coordinate: small mostly, sometimes the 16-bit edges.
fn coordinate(c: &mut Choices, out: &mut Vec<u8>) {
    match c.below(8) {
        0 => fraction(c, out),
        1 => {
            let edge = *c.pick(&[32_767, -32_768, 1131, -1131, 108, -108]);
            number(c, out, edge);
        }
        _ => {
            let value = c.range(-200, 200) as i32;
            number(c, out, value);
        }
    }
}

/// One INDEX of subroutines: how many, which hold content, and the content.
struct Subrs {
    count: usize,
    real: Vec<usize>,
}

/// What a program may call: the local and the global subroutines.
struct Calls<'s> {
    local: &'s Subrs,
    global: &'s Subrs,
    /// Every program declares this many stems, so a subroutine's `hintmask`
    /// reads the same mask whoever calls it; `None` keeps masks out of
    /// subroutines.
    stems: Option<usize>,
}

/// The drawing between a program's start and its end: lines, curves, flex,
/// masks and calls.
fn drawing(
    c: &mut Choices,
    out: &mut Vec<u8>,
    calls: &Calls,
    stems: usize,
    below: Option<(bool, usize)>,
) {
    for _ in 0..c.below(MAX_OPERATORS) {
        match c.below(16) {
            0 => {
                for _ in 0..2 * (1 + c.below(4)) {
                    coordinate(c, out);
                }
                out.push(5); // rlineto
            }
            1 => {
                for _ in 0..1 + c.below(6) {
                    coordinate(c, out);
                }
                out.push(*c.pick(&[6, 7])); // hlineto, vlineto
            }
            2 => {
                for _ in 0..6 * (1 + c.below(3)) {
                    coordinate(c, out);
                }
                out.push(8); // rrcurveto
            }
            3 => {
                let args = 4 * (1 + c.below(3)) + c.below(2);
                for _ in 0..args {
                    coordinate(c, out);
                }
                out.push(*c.pick(&[27, 26, 31, 30])); // hh, vv, hv, vh
            }
            4 => {
                for _ in 0..6 * (1 + c.below(2)) + 2 {
                    coordinate(c, out);
                }
                out.push(24); // rcurveline
            }
            5 => {
                for _ in 0..2 * (1 + c.below(3)) + 6 {
                    coordinate(c, out);
                }
                out.push(25); // rlinecurve
            }
            6 => {
                // flex, hflex, hflex1, flex1 (5177, 4.1).
                let (op, args) = *c.pick(&[(35u8, 13usize), (34, 7), (36, 9), (37, 11)]);
                for _ in 0..args {
                    coordinate(c, out);
                }
                out.push(12);
                out.push(op);
            }
            7 | 8 => {
                for _ in 0..2 {
                    coordinate(c, out);
                }
                out.push(21); // rmoveto
            }
            9 => {
                coordinate(c, out);
                out.push(*c.pick(&[22, 4])); // hmoveto, vmoveto
            }
            10 if below.is_none() || calls.stems.is_some() => {
                // hintmask or cntrmask, and the mask its stems call for.
                out.push(*c.pick(&[19, 20]));
                for _ in 0..stems.div_ceil(8).max(1) {
                    out.push(c.byte());
                }
            }
            11..=13 => call(c, out, calls, below),
            _ => {
                for _ in 0..2 {
                    coordinate(c, out);
                }
                out.push(5);
            }
        }
    }
}

/// A call to a subroutine with content: from a glyph any, from a subroutine
/// only one after it in its own INDEX or any global one after it, so no
/// subroutine reaches itself.
fn call(c: &mut Choices, out: &mut Vec<u8>, calls: &Calls, below: Option<(bool, usize)>) {
    let global = c.flag();
    let subrs = if global { calls.global } else { calls.local };
    let candidates: Vec<usize> = subrs
        .real
        .iter()
        .copied()
        .filter(|at| match below {
            None => true,
            Some((from_global, from)) => {
                // A global subroutine may call a local one, and a local one a
                // global: what keeps the graph acyclic is that every call
                // goes to a subroutine of a higher rank, the rank being the
                // position among each INDEX's real ones, globals after locals.
                let rank = |is_global: bool, at: usize| -> usize {
                    let list = if is_global {
                        &calls.global.real
                    } else {
                        &calls.local.real
                    };
                    let position = list.iter().position(|r| *r == at).unwrap_or(0);
                    position + if is_global { MAX_REAL } else { 0 }
                };
                rank(global, *at) > rank(from_global, from)
            }
        })
        .collect();
    let Some(&at) = candidates.get(c.below(candidates.len().max(1))) else {
        return;
    };
    number(c, out, at as i32 - bias(subrs.count));
    out.push(if global { 29 } else { 10 });
}

/// A subroutine's body: drawing, then `return` or, now and then, `endchar`.
fn subroutine(c: &mut Choices, calls: &Calls, global: bool, at: usize) -> Vec<u8> {
    let mut out = Vec::new();
    let stems = calls.stems.unwrap_or(0);
    drawing(c, &mut out, calls, stems, Some((global, at)));
    out.push(if c.below(8) == 7 { 14 } else { 11 });
    out
}

/// A glyph's program, and the advance it must read with: its width operand
/// over the nominal width, or the default width where it has none.
fn glyph(
    c: &mut Choices,
    calls: &Calls,
    nominal: f64,
    default: f64,
    seac: bool,
    shape: &mut (usize, usize),
) -> (Vec<u8>, f64) {
    let mut out = Vec::new();
    let width = if c.flag() {
        let w = c.range(-500, 1500) as i32;
        Some(number(c, &mut out, w))
    } else {
        None
    };
    let advance = width.map_or(default, |w| nominal + w);

    // A glyph that is its `endchar` alone, as a space or an accented letter
    // made by `seac` is: there the width, or its absence, is read off
    // `endchar` itself.
    if c.below(4) == 3 {
        shape.0 += 1;
        if seac && c.flag() {
            seac_operands(c, &mut out);
            shape.1 += 1;
        }
        out.push(14);
        return (out, advance);
    }

    // Stem hints first (5177, 4.3), as pairs; the width before them makes
    // their count odd, which is how it is read.
    let mut stems = 0usize;
    let wanted = calls.stems.unwrap_or_else(|| c.below(12));
    while stems < wanted {
        let pairs = (1 + c.below(4)).min(wanted - stems);
        for _ in 0..2 * pairs {
            let value = c.range(-50, 400) as i32;
            number(c, &mut out, value);
        }
        out.push(*c.pick(&[1, 3, 18, 23]));
        stems += pairs;
    }
    if calls.stems.is_none() && c.below(3) == 2 {
        // Implicit vstems before the first mask.
        let pairs = 1 + c.below(3);
        for _ in 0..2 * pairs {
            let value = c.range(-50, 400) as i32;
            number(c, &mut out, value);
        }
        stems += pairs;
        out.push(*c.pick(&[19, 20]));
        for _ in 0..stems.div_ceil(8).max(1) {
            out.push(c.byte());
        }
    }
    // A move before any drawing: the width, if it is still on the stack,
    // goes to the first of these.
    for _ in 0..2 {
        coordinate(c, &mut out);
    }
    out.push(21);

    drawing(c, &mut out, calls, stems, None);

    if seac && c.flag() {
        seac_operands(c, &mut out);
        shape.1 += 1;
    }
    out.push(14);
    (out, advance)
}

/// `seac`'s four operands (5177, appendix C): `adx ady bchar achar`, the two
/// codes StandardEncoding's for letters and accents the charset names.
fn seac_operands(c: &mut Choices, out: &mut Vec<u8>) {
    for _ in 0..2 {
        coordinate(c, out);
    }
    let base = *c.pick(&[65, 97, 69]);
    let accent = *c.pick(&[193, 194]);
    number(c, out, base);
    number(c, out, accent);
}

/// A Private DICT (5176, 15) and its local Subrs right after it.
fn private(default: i32, nominal: i32, subrs: &Bodies, width: u8) -> Vec<u8> {
    let mut dict = Vec::new();
    entry(&mut dict, 20, &[default]);
    entry(&mut dict, 21, &[nominal]);
    if subrs.count > 0 {
        // Subrs is measured from the Private DICT itself; five-byte operands
        // make the DICT's length known before the offset is.
        let len = dict.len() + 6;
        entry(&mut dict, 19, &[len as i32]);
        dict.extend(subr_index(subrs, width));
    }
    dict
}

/// A subroutine INDEX's real subroutines, chosen among its count.
fn subrs(c: &mut Choices) -> Subrs {
    let count = if c.below(4) == 3 {
        *c.pick(EDGES)
    } else {
        *c.pick(SMALL)
    };
    let mut real = BTreeSet::new();
    if count > 0 {
        for _ in 0..c.below(MAX_REAL + 1) {
            let at = match c.below(4) {
                0 => 0,
                1 => count - 1,
                2 => count / 2,
                _ => c.below(count),
            };
            real.insert(at);
        }
    }
    Subrs {
        count,
        real: real.into_iter().collect(),
    }
}

/// A subroutine INDEX's bodies: content at its real subroutines, and a bare
/// `return` at every other, which is written without being held.
struct Bodies {
    count: usize,
    real: BTreeMap<usize, Vec<u8>>,
}

fn bodies(c: &mut Choices, subrs: &Subrs, calls: &Calls, global: bool) -> Bodies {
    let mut real = BTreeMap::new();
    for &at in &subrs.real {
        real.insert(at, subroutine(c, calls, global, at));
    }
    Bodies {
        count: subrs.count,
        real,
    }
}

/// [`index`] over `bodies`, the bare returns written in place: an INDEX of
/// 33 900 subroutines is one allocation rather than one a subroutine.
fn subr_index(bodies: &Bodies, width: u8) -> Vec<u8> {
    if bodies.count == 0 {
        return vec![0, 0];
    }
    let body = |at: usize| -> &[u8] { bodies.real.get(&at).map_or(&[11u8][..], Vec::as_slice) };
    let total: usize = (0..bodies.count).map(|at| body(at).len()).sum();
    let narrowest = match total + 1 {
        0..=0xFF => 1,
        0x100..=0xFFFF => 2,
        0x1_0000..=0xFF_FFFF => 3,
        _ => 4,
    };
    let size = usize::from(width).clamp(narrowest, 4);
    let mut out = Vec::with_capacity(3 + (bodies.count + 1) * size + total);
    out.extend_from_slice(&(bodies.count as u16).to_be_bytes());
    out.push(size as u8);
    let mut offset = 1usize;
    out.extend_from_slice(&(offset as u32).to_be_bytes()[4 - size..]);
    for at in 0..bodies.count {
        offset += body(at).len();
        out.extend_from_slice(&(offset as u32).to_be_bytes()[4 - size..]);
    }
    for at in 0..bodies.count {
        out.extend_from_slice(body(at));
    }
    out
}

/// A generated font: its bytes, its glyph count, each glyph's advance.
pub struct Font {
    /// The program.
    pub bytes: Vec<u8>,
    /// Each glyph's advance, as its program and Private DICT state it.
    pub advances: Vec<f64>,
    /// Whether it is CID-keyed.
    pub cid: bool,
    /// How many subroutines the global INDEX holds, then each local one.
    pub counts: Vec<usize>,
    /// Whether every glyph declares one stem count, so subroutines carry
    /// masks.
    pub uniform: bool,
    /// How many glyphs are their `endchar` alone.
    pub bare: usize,
    /// How many glyphs end with `seac`'s operands.
    pub seac: usize,
}

/// A font of one to [`MAX_GLYPHS`] glyphs, built from `c`.
pub fn generate(c: &mut Choices) -> Font {
    let glyphs = 1 + c.below(MAX_GLYPHS);
    let cid = c.below(4) == 3;
    let widths = [c.below(5) as u8, c.below(5) as u8];
    let global = subrs(c);
    let uniform = c.flag().then(|| c.below(12));

    // One Private DICT, or one per Font DICT.
    let fds = if cid { 1 + c.below(3) } else { 1 };
    let mut privates = Vec::new();
    let mut locals = Vec::new();
    let mut metrics = Vec::new();
    for _ in 0..fds {
        let local = subrs(c);
        let default = c.range(0, 1000) as i32;
        let nominal = c.range(-200, 800) as i32;
        metrics.push((f64::from(default), f64::from(nominal)));
        locals.push(local);
        privates.push((default, nominal));
    }
    let select: Vec<usize> = (0..glyphs).map(|_| c.below(fds)).collect();

    // Bodies: globals are shared, so their calls into local subroutines are
    // read against the first Font DICT's.
    let empty = Subrs {
        count: 0,
        real: Vec::new(),
    };
    let global_bodies = {
        let calls = Calls {
            local: &locals[0],
            global: &global,
            stems: uniform,
        };
        bodies(c, &global, &calls, true)
    };
    let mut local_bodies = Vec::new();
    for local in &locals {
        let calls = Calls {
            local,
            global: &global,
            stems: uniform,
        };
        local_bodies.push(bodies(c, local, &calls, false));
    }
    let mut charstrings = Vec::new();
    let mut advances = Vec::new();
    let mut shape = (0usize, 0usize);
    for g in 0..glyphs {
        let fd = select[g];
        let calls = Calls {
            local: &locals[fd],
            global: if cid && fd != 0 { &empty } else { &global },
            stems: uniform,
        };
        let (default, nominal) = metrics[fd];
        let (code, advance) = glyph(c, &calls, nominal, default, !cid, &mut shape);
        charstrings.push(code);
        advances.push(advance);
    }

    // The charset (5176, 13): SIDs naming the StandardEncoding letters and
    // accents `seac` asks for, or CIDs.
    let named: [u16; 5] = [34, 66, 38, 124, 125];
    let ids: Vec<u16> = (1..glyphs)
        .map(|g| {
            if cid {
                g as u16 * (1 + c.below(3) as u16)
            } else {
                named.get(g - 1).copied().unwrap_or(200 + g as u16)
            }
        })
        .collect();
    let charset = match c.below(3) {
        0 => {
            let mut out = vec![0u8];
            for id in &ids {
                out.extend_from_slice(&id.to_be_bytes());
            }
            out
        }
        format => {
            // Ranges: each id starts one of `nLeft + 1` consecutive ones.
            let mut out = vec![format as u8];
            for id in &ids {
                out.extend_from_slice(&id.to_be_bytes());
                if format == 1 {
                    out.push(0);
                } else {
                    out.extend_from_slice(&0u16.to_be_bytes());
                }
            }
            out
        }
    };

    let strings: Vec<Vec<u8>> = vec![b"Adobe".to_vec(), b"Identity".to_vec()];
    let name = index(&[b"Generated".to_vec()], widths[0]);
    let string_index = index(&strings, widths[1]);
    let global_index = subr_index(&global_bodies, widths[0]);
    let charstring_index = index(&charstrings, widths[1]);
    let private_dicts: Vec<Vec<u8>> = privates
        .iter()
        .zip(&local_bodies)
        .map(|((default, nominal), bodies)| private(*default, *nominal, bodies, widths[0]))
        .collect();

    // The Top DICT holds every offset in five bytes, so its length is known
    // before the offsets are, and everything after it can be placed.
    let top = |at: &[i32; 6]| -> Vec<u8> {
        let mut dict = Vec::new();
        if cid {
            entry(&mut dict, 0x0C1E, &[391, 392, 0]);
        }
        entry(&mut dict, 15, &[at[0]]);
        entry(&mut dict, 17, &[at[1]]);
        entry(&mut dict, 18, &[at[3], at[2]]);
        if cid {
            entry(&mut dict, 0x0C24, &[at[4]]);
            entry(&mut dict, 0x0C25, &[at[5]]);
        }
        dict
    };
    let top_len = index(&[top(&[0; 6])], widths[1]).len();
    let mut at = 4 + name.len() + top_len + string_index.len() + global_index.len();
    let charset_at = at;
    at += charset.len();
    let charstrings_at = at;
    at += charstring_index.len();
    let private_at = at;
    let private_len = private_dicts[0].len();
    at += private_len;

    // A CID-keyed font: the FDArray, each Font DICT naming its Private DICT,
    // and the FDSelect.
    let mut tail = Vec::new();
    let mut fd_array_at = 0;
    let mut fd_select_at = 0;
    if cid {
        let font_dict = |size: usize, offset: usize| {
            let mut dict = Vec::new();
            entry(&mut dict, 18, &[size as i32, offset as i32]);
            dict
        };
        let fd_index_len = index(
            &(0..fds).map(|_| font_dict(0, 0)).collect::<Vec<_>>(),
            widths[0],
        )
        .len();
        let fd_select = if c.flag() {
            let mut out = vec![0u8];
            out.extend(select.iter().map(|fd| *fd as u8));
            out
        } else {
            // Format 3: a range per glyph, then the sentinel.
            let mut out = vec![3u8];
            out.extend_from_slice(&(glyphs as u16).to_be_bytes());
            for (g, fd) in select.iter().enumerate() {
                out.extend_from_slice(&(g as u16).to_be_bytes());
                out.push(*fd as u8);
            }
            out.extend_from_slice(&(glyphs as u16).to_be_bytes());
            out
        };
        fd_array_at = at;
        fd_select_at = at + fd_index_len;
        let mut privates_at = fd_select_at + fd_select.len();
        let mut dicts = Vec::new();
        let mut placed = Vec::new();
        for dict in private_dicts.iter() {
            dicts.push(font_dict(dict.len(), privates_at));
            placed.extend_from_slice(dict);
            privates_at += dict.len();
        }
        tail.extend(index(&dicts, widths[0]));
        tail.extend(fd_select);
        tail.extend(placed);
    }

    let top_dict = top(&[
        charset_at as i32,
        charstrings_at as i32,
        private_at as i32,
        private_len as i32,
        fd_array_at as i32,
        fd_select_at as i32,
    ]);
    let mut bytes = vec![1, 0, 4, 1 + c.below(4) as u8];
    bytes.extend(name);
    bytes.extend(index(&[top_dict], widths[1]));
    bytes.extend(string_index);
    bytes.extend(global_index);
    bytes.extend(charset);
    bytes.extend(charstring_index);
    bytes.extend(&private_dicts[0]);
    bytes.extend(tail);
    let mut counts = vec![global.count];
    counts.extend(locals.iter().map(|local| local.count));
    Font {
        bytes,
        advances,
        cid,
        counts,
        uniform: uniform.is_some(),
        bare: shape.0,
        seac: shape.1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_input_makes_a_program_that_parses() {
        for input in [&b""[..], b"\x07\x01", &[0xff; 200], &[3; 900]] {
            let font = generate(&mut Choices::new(input));
            let cff = tinker_pdf_font::Cff::parse(&font.bytes).expect("a generated CFF parses");
            assert_eq!(cff.glyph_count(), font.advances.len());
        }
    }

    #[test]
    fn an_index_takes_the_offset_width_asked_for() {
        let items = vec![b"ab".to_vec(), b"c".to_vec()];
        assert_eq!(index(&items, 1), [0, 2, 1, 1, 3, 4, b'a', b'b', b'c']);
        assert_eq!(&index(&items, 3)[..3], [0, 2, 3]);
        assert_eq!(index(&[], 4), [0, 0], "an empty INDEX is its count alone");
    }

    type Feature = fn(&Font) -> bool;

    /// Rewrites `fuzz/corpus/cff_charstrings/`: one seed per feature, each
    /// the first input of `first_input`'s sequence whose font has it.
    #[test]
    #[ignore = "writes fuzz/corpus/cff_charstrings"]
    fn write_the_fuzz_seeds() {
        let features: &[(&str, Feature)] = &[
            ("local-1239", |f| {
                f.counts.iter().skip(1).any(|n| *n == 1239)
            }),
            ("local-1240", |f| {
                f.counts.iter().skip(1).any(|n| *n == 1240)
            }),
            ("local-33899", |f| {
                f.counts.iter().skip(1).any(|n| *n == 33_899)
            }),
            ("local-33900", |f| {
                f.counts.iter().skip(1).any(|n| *n == 33_900)
            }),
            ("global-1240", |f| f.counts[0] == 1240),
            ("global-33900", |f| f.counts[0] == 33_900),
            ("cid-keyed", |f| f.cid && f.counts.len() >= 3),
            ("seac", |f| f.seac >= 2),
            ("bare-endchar", |f| f.bare >= 2),
            ("masks-in-subroutines", |f| {
                f.uniform && f.counts.iter().any(|n| *n > 0)
            }),
            ("twelve-glyphs", |f| f.advances.len() == MAX_GLYPHS),
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
                2000,
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
        crate::write_seeds("cff_charstrings", &seeds);
    }
}
