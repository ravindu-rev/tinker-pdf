//! CFF fonts and the Type 2 charstring interpreter.
//!
//! A CFF font is INDEX structures and DICTs of operator-tagged numbers, with
//! the outlines themselves in a stack language. Type 2 charstrings are
//! compact by being implicit: the width is an optional extra argument on the
//! first stack-clearing operator, and `hintmask` consumes bytes whose count
//! depends on how many stem hints have been declared — so a parser that
//! ignores hints still has to count them.
//!
//! Hints are parsed only far enough to skip them correctly. No hinting is
//! applied; see `glyf.rs` for why.

use crate::outline::{Outline, Segment};

/// A parsed CFF font.
#[derive(Clone, Debug)]
pub struct Cff<'a> {
    charstrings: Index<'a>,
    global_subrs: Index<'a>,
    local_subrs: Index<'a>,
    default_width: f64,
    nominal_width: f64,
    /// The font matrix, which is usually 1/1000 but need not be.
    pub font_matrix: [f64; 6],
}

/// A CFF INDEX: a count, offsets, then the data.
#[derive(Clone, Debug, Default)]
struct Index<'a> {
    offsets: Vec<usize>,
    data: &'a [u8],
}

impl<'a> Index<'a> {
    fn parse(data: &'a [u8], at: usize) -> Option<(Index<'a>, usize)> {
        let count = usize::from(u16::from_be_bytes([*data.get(at)?, *data.get(at + 1)?]));
        if count == 0 {
            return Some((Index::default(), at + 2));
        }

        let off_size = usize::from(*data.get(at + 2)?);
        if !(1..=4).contains(&off_size) {
            return None;
        }

        let offsets_at = at + 3;
        let mut offsets = Vec::with_capacity((count + 1).min(1 << 16));
        for i in 0..=count {
            let mut value = 0usize;
            for byte in 0..off_size {
                value = (value << 8) | usize::from(*data.get(offsets_at + i * off_size + byte)?);
            }
            offsets.push(value);
        }

        // Offsets are 1-based from the byte after the offset array.
        let base = offsets_at + (count + 1) * off_size - 1;
        let end = base + offsets.last().copied().unwrap_or(1);

        Some((
            Index {
                offsets,
                data: data.get(base..end.min(data.len()))?,
            },
            end,
        ))
    }

    fn get(&self, index: usize) -> Option<&'a [u8]> {
        let start = self.offsets.get(index)?.checked_sub(1)?;
        let end = self.offsets.get(index + 1)?.checked_sub(1)?;
        (end >= start).then(|| self.data.get(start..end))?
    }

    fn len(&self) -> usize {
        self.offsets.len().saturating_sub(1)
    }
}

/// Reads a DICT into `(operator, operands)` pairs.
fn parse_dict(data: &[u8]) -> Vec<(u16, Vec<f64>)> {
    let mut out = Vec::new();
    let mut operands: Vec<f64> = Vec::new();
    let mut at = 0usize;

    while at < data.len() {
        let Some(&b) = data.get(at) else { break };
        match b {
            // Operators.
            0..=21 => {
                let op = if b == 12 {
                    at += 1;
                    0x0C00 | u16::from(data.get(at).copied().unwrap_or(0))
                } else {
                    u16::from(b)
                };
                at += 1;
                out.push((op, std::mem::take(&mut operands)));
            }
            28 => {
                let (Some(&a), Some(&c)) = (data.get(at + 1), data.get(at + 2)) else {
                    break;
                };
                operands.push(f64::from(i16::from_be_bytes([a, c])));
                at += 3;
            }
            29 => {
                let Some(bytes) = data.get(at + 1..at + 5) else {
                    break;
                };
                let value = i32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
                operands.push(f64::from(value));
                at += 5;
            }
            30 => {
                // A real number, packed as nibbles.
                let (value, next) = parse_real(data, at + 1);
                operands.push(value);
                at = next;
            }
            32..=246 => {
                operands.push(f64::from(b) - 139.0);
                at += 1;
            }
            247..=250 => {
                let Some(&next) = data.get(at + 1) else { break };
                operands.push((f64::from(b) - 247.0) * 256.0 + f64::from(next) + 108.0);
                at += 2;
            }
            251..=254 => {
                let Some(&next) = data.get(at + 1) else { break };
                operands.push(-(f64::from(b) - 251.0) * 256.0 - f64::from(next) - 108.0);
                at += 2;
            }
            _ => at += 1,
        }
        if operands.len() > 48 {
            operands.clear();
        }
    }

    out
}

fn parse_real(data: &[u8], mut at: usize) -> (f64, usize) {
    let mut text = String::new();
    'outer: while let Some(&byte) = data.get(at) {
        at += 1;
        for nibble in [byte >> 4, byte & 0x0F] {
            match nibble {
                0..=9 => text.push(char::from(b'0' + nibble)),
                0x0A => text.push('.'),
                0x0B => text.push('E'),
                0x0C => text.push_str("E-"),
                0x0E => text.push('-'),
                0x0F => break 'outer,
                _ => {}
            }
        }
        if text.len() > 64 {
            break;
        }
    }
    (text.parse::<f64>().unwrap_or(0.0), at)
}

fn dict_get(dict: &[(u16, Vec<f64>)], op: u16) -> Option<&[f64]> {
    dict.iter()
        .find(|(o, _)| *o == op)
        .map(|(_, v)| v.as_slice())
}

impl<'a> Cff<'a> {
    /// Parses a bare CFF font program.
    #[must_use]
    pub fn parse(data: &'a [u8]) -> Option<Cff<'a>> {
        // The header's fourth byte is its own size, so the name INDEX follows.
        let header_size = usize::from(*data.get(2)?);
        let (_names, at) = Index::parse(data, header_size)?;
        let (top_dicts, at) = Index::parse(data, at)?;
        let (_strings, at) = Index::parse(data, at)?;
        let (global_subrs, _) = Index::parse(data, at)?;

        let top = parse_dict(top_dicts.get(0)?);

        let charstrings_at = dict_get(&top, 17).and_then(<[f64]>::first).copied()? as usize;
        let (charstrings, _) = Index::parse(data, charstrings_at)?;

        // The Private DICT gives the local subroutines and the widths.
        let mut local_subrs = Index::default();
        let mut default_width = 0.0;
        let mut nominal_width = 0.0;
        if let Some(private) = dict_get(&top, 18) {
            let (Some(&size), Some(&offset)) = (private.first(), private.get(1)) else {
                return None;
            };
            let (size, offset) = (size as usize, offset as usize);
            if let Some(bytes) = data.get(offset..offset.saturating_add(size).min(data.len())) {
                let private_dict = parse_dict(bytes);
                default_width = dict_get(&private_dict, 20)
                    .and_then(<[f64]>::first)
                    .copied()
                    .unwrap_or(0.0);
                nominal_width = dict_get(&private_dict, 21)
                    .and_then(<[f64]>::first)
                    .copied()
                    .unwrap_or(0.0);
                if let Some(&subrs) = dict_get(&private_dict, 19).and_then(<[f64]>::first) {
                    if let Some((index, _)) = Index::parse(data, offset + subrs as usize) {
                        local_subrs = index;
                    }
                }
            }
        }

        // 12 7 is FontMatrix; the default is 1/1000 in both axes.
        let font_matrix = match dict_get(&top, 0x0C07) {
            Some(m) if m.len() >= 6 => [m[0], m[1], m[2], m[3], m[4], m[5]],
            _ => [0.001, 0.0, 0.0, 0.001, 0.0, 0.0],
        };

        Some(Cff {
            charstrings,
            global_subrs,
            local_subrs,
            default_width,
            nominal_width,
            font_matrix,
        })
    }

    /// How many glyphs the font holds.
    #[must_use]
    pub fn glyph_count(&self) -> usize {
        self.charstrings.len()
    }

    /// One glyph's outline, in the font's own units.
    #[must_use]
    pub fn outline(&self, glyph: u16) -> Option<Outline> {
        let charstring = self.charstrings.get(usize::from(glyph))?;
        let mut ctx = Charstring {
            cff: self,
            outline: Outline::default(),
            stack: Vec::new(),
            x: 0.0,
            y: 0.0,
            stems: 0,
            width: None,
            open: false,
            depth: 0,
            budget: 64_000,
            transient: [0.0; 32],
        };
        ctx.run(charstring);
        if ctx.open {
            ctx.outline.push(Segment::Close);
        }
        Some(ctx.outline)
    }

    /// A glyph's advance, when the charstring declares one.
    #[must_use]
    pub fn advance(&self, glyph: u16) -> Option<f64> {
        let charstring = self.charstrings.get(usize::from(glyph))?;
        let mut ctx = Charstring {
            cff: self,
            outline: Outline::default(),
            stack: Vec::new(),
            x: 0.0,
            y: 0.0,
            stems: 0,
            width: None,
            open: false,
            depth: 0,
            budget: 64_000,
            transient: [0.0; 32],
        };
        ctx.run(charstring);
        Some(ctx.width.unwrap_or(self.default_width))
    }
}

/// The bias applied to subroutine indices (Type 2 charstring specification).
fn bias(count: usize) -> i32 {
    if count < 1240 {
        107
    } else if count < 33900 {
        1131
    } else {
        32768
    }
}

struct Charstring<'a, 'b> {
    cff: &'b Cff<'a>,
    outline: Outline,
    stack: Vec<f64>,
    x: f64,
    y: f64,
    stems: usize,
    width: Option<f64>,
    open: bool,
    depth: u32,
    budget: u32,
    transient: [f64; 32],
}

impl Charstring<'_, '_> {
    /// Takes the width if this is the first stack-clearing operator and the
    /// stack holds one more operand than the operator needs.
    fn take_width(&mut self, even: bool) {
        if self.width.is_some() {
            return;
        }
        let odd_extra = if even {
            self.stack.len() % 2 == 1
        } else {
            // For rmoveto (2 args), hmoveto/vmoveto (1 arg), endchar (0).
            false
        };
        if odd_extra && !self.stack.is_empty() {
            let extra = self.stack.remove(0);
            self.width = Some(self.cff.nominal_width + extra);
        } else {
            self.width = Some(self.cff.default_width);
        }
    }

    fn take_width_for(&mut self, expected: usize) {
        if self.width.is_some() {
            return;
        }
        if self.stack.len() > expected && !self.stack.is_empty() {
            let extra = self.stack.remove(0);
            self.width = Some(self.cff.nominal_width + extra);
        } else {
            self.width = Some(self.cff.default_width);
        }
    }

    fn move_to(&mut self, x: f64, y: f64) {
        if self.open {
            self.outline.push(Segment::Close);
        }
        self.outline.push(Segment::MoveTo { x, y });
        self.open = true;
    }

    fn run(&mut self, code: &[u8]) {
        if self.depth > 10 {
            return;
        }
        let mut at = 0usize;

        while at < code.len() {
            self.budget = self.budget.saturating_sub(1);
            if self.budget == 0 {
                return;
            }

            let Some(&b) = code.get(at) else { return };
            at += 1;

            match b {
                // Operands.
                32..=246 => self.push(f64::from(b) - 139.0),
                247..=250 => {
                    let Some(&next) = code.get(at) else { return };
                    at += 1;
                    self.push((f64::from(b) - 247.0) * 256.0 + f64::from(next) + 108.0);
                }
                251..=254 => {
                    let Some(&next) = code.get(at) else { return };
                    at += 1;
                    self.push(-(f64::from(b) - 251.0) * 256.0 - f64::from(next) - 108.0);
                }
                28 => {
                    let (Some(&hi), Some(&lo)) = (code.get(at), code.get(at + 1)) else {
                        return;
                    };
                    at += 2;
                    self.push(f64::from(i16::from_be_bytes([hi, lo])));
                }
                255 => {
                    let Some(bytes) = code.get(at..at + 4) else {
                        return;
                    };
                    at += 4;
                    // A 16.16 fixed-point number.
                    let value = i32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
                    self.push(f64::from(value) / 65536.0);
                }

                // hstem, vstem, hstemhm, vstemhm: counted, then discarded.
                1 | 3 | 18 | 23 => {
                    self.take_width(true);
                    self.stems += self.stack.len() / 2;
                    self.stack.clear();
                }

                // hintmask, cntrmask: implicit vstem, then a bitmask whose
                // length depends on the stem count. Miscounting here
                // desynchronizes the whole charstring.
                19 | 20 => {
                    self.take_width(true);
                    self.stems += self.stack.len() / 2;
                    self.stack.clear();
                    at += self.stems.div_ceil(8).max(1);
                }

                // rmoveto
                21 => {
                    self.take_width_for(2);
                    let (dx, dy) = (self.arg(0), self.arg(1));
                    self.x += dx;
                    self.y += dy;
                    let (x, y) = (self.x, self.y);
                    self.move_to(x, y);
                    self.stack.clear();
                }
                // hmoveto
                22 => {
                    self.take_width_for(1);
                    self.x += self.arg(0);
                    let (x, y) = (self.x, self.y);
                    self.move_to(x, y);
                    self.stack.clear();
                }
                // vmoveto
                4 => {
                    self.take_width_for(1);
                    self.y += self.arg(0);
                    let (x, y) = (self.x, self.y);
                    self.move_to(x, y);
                    self.stack.clear();
                }

                // rlineto
                5 => {
                    let args = self.stack.clone();
                    for pair in args.chunks_exact(2) {
                        self.x += pair[0];
                        self.y += pair[1];
                        self.outline.push(Segment::LineTo {
                            x: self.x,
                            y: self.y,
                        });
                    }
                    self.stack.clear();
                }
                // hlineto and vlineto alternate axes.
                6 | 7 => {
                    let args = self.stack.clone();
                    let mut horizontal = b == 6;
                    for value in args {
                        if horizontal {
                            self.x += value;
                        } else {
                            self.y += value;
                        }
                        self.outline.push(Segment::LineTo {
                            x: self.x,
                            y: self.y,
                        });
                        horizontal = !horizontal;
                    }
                    self.stack.clear();
                }

                // rrcurveto
                8 => {
                    let args = self.stack.clone();
                    for six in args.chunks_exact(6) {
                        self.curve(six[0], six[1], six[2], six[3], six[4], six[5]);
                    }
                    self.stack.clear();
                }
                // hhcurveto
                27 => {
                    let mut args = self.stack.clone();
                    let mut dy1 = 0.0;
                    if args.len() % 4 == 1 && !args.is_empty() {
                        dy1 = args.remove(0);
                    }
                    for four in args.chunks_exact(4) {
                        self.curve(four[0], dy1, four[1], four[2], four[3], 0.0);
                        dy1 = 0.0;
                    }
                    self.stack.clear();
                }
                // vvcurveto
                26 => {
                    let mut args = self.stack.clone();
                    let mut dx1 = 0.0;
                    if args.len() % 4 == 1 && !args.is_empty() {
                        dx1 = args.remove(0);
                    }
                    for four in args.chunks_exact(4) {
                        self.curve(dx1, four[0], four[1], four[2], 0.0, four[3]);
                        dx1 = 0.0;
                    }
                    self.stack.clear();
                }
                // hvcurveto and vhcurveto alternate their starting axis.
                31 | 30 => {
                    let args = self.stack.clone();
                    let mut horizontal = b == 31;
                    let mut i = 0usize;
                    while i + 4 <= args.len() {
                        let last = i + 8 > args.len();
                        let extra = if last && args.len() - i == 5 {
                            args.get(i + 4).copied().unwrap_or(0.0)
                        } else {
                            0.0
                        };
                        let (a, bb, c, d) = (args[i], args[i + 1], args[i + 2], args[i + 3]);
                        if horizontal {
                            self.curve(a, 0.0, bb, c, extra, d);
                        } else {
                            self.curve(0.0, a, bb, c, d, extra);
                        }
                        horizontal = !horizontal;
                        i += 4;
                    }
                    self.stack.clear();
                }

                // rcurveline
                24 => {
                    let args = self.stack.clone();
                    let curves = (args.len().saturating_sub(2)) / 6;
                    for k in 0..curves {
                        let s = k * 6;
                        if let Some(six) = args.get(s..s + 6) {
                            self.curve(six[0], six[1], six[2], six[3], six[4], six[5]);
                        }
                    }
                    if let Some(tail) = args.get(curves * 6..curves * 6 + 2) {
                        self.x += tail[0];
                        self.y += tail[1];
                        self.outline.push(Segment::LineTo {
                            x: self.x,
                            y: self.y,
                        });
                    }
                    self.stack.clear();
                }
                // rlinecurve
                25 => {
                    let args = self.stack.clone();
                    let lines = (args.len().saturating_sub(6)) / 2;
                    for k in 0..lines {
                        if let Some(pair) = args.get(k * 2..k * 2 + 2) {
                            self.x += pair[0];
                            self.y += pair[1];
                            self.outline.push(Segment::LineTo {
                                x: self.x,
                                y: self.y,
                            });
                        }
                    }
                    if let Some(six) = args.get(lines * 2..lines * 2 + 6) {
                        self.curve(six[0], six[1], six[2], six[3], six[4], six[5]);
                    }
                    self.stack.clear();
                }

                // callsubr
                10 => {
                    let Some(index) = self.stack.pop() else {
                        return;
                    };
                    let biased = index as i32 + bias(self.cff.local_subrs.len());
                    if let Ok(index) = usize::try_from(biased) {
                        if let Some(code) = self.cff.local_subrs.get(index) {
                            self.depth += 1;
                            self.run(code);
                            self.depth -= 1;
                        }
                    }
                }
                // callgsubr
                29 => {
                    let Some(index) = self.stack.pop() else {
                        return;
                    };
                    let biased = index as i32 + bias(self.cff.global_subrs.len());
                    if let Ok(index) = usize::try_from(biased) {
                        if let Some(code) = self.cff.global_subrs.get(index) {
                            self.depth += 1;
                            self.run(code);
                            self.depth -= 1;
                        }
                    }
                }
                // return
                11 => return,
                // endchar
                14 => {
                    self.take_width_for(0);
                    if self.open {
                        self.outline.push(Segment::Close);
                        self.open = false;
                    }
                    return;
                }

                12 => {
                    let Some(&second) = code.get(at) else { return };
                    at += 1;
                    self.escaped(second);
                }

                _ => self.stack.clear(),
            }
        }
    }

    /// The two-byte operators, of which only flex and the arithmetic matter.
    fn escaped(&mut self, op: u8) {
        match op {
            // flex, hflex, hflex1, flex1: two curves in one operator.
            35 => {
                let a = self.stack.clone();
                if a.len() >= 12 {
                    self.curve(a[0], a[1], a[2], a[3], a[4], a[5]);
                    self.curve(a[6], a[7], a[8], a[9], a[10], a[11]);
                }
                self.stack.clear();
            }
            34 => {
                let a = self.stack.clone();
                if a.len() >= 7 {
                    let y = self.y;
                    self.curve(a[0], 0.0, a[1], a[2], a[3], 0.0);
                    self.curve(a[4], 0.0, a[5], y - self.y, a[6], 0.0);
                }
                self.stack.clear();
            }
            36 => {
                let a = self.stack.clone();
                if a.len() >= 9 {
                    let start_y = self.y;
                    self.curve(a[0], a[1], a[2], a[3], a[4], 0.0);
                    self.curve(a[5], 0.0, a[6], a[7], a[8], start_y - self.y - a[7]);
                }
                self.stack.clear();
            }
            37 => {
                let a = self.stack.clone();
                if a.len() >= 11 {
                    let (sx, sy) = (self.x, self.y);
                    self.curve(a[0], a[1], a[2], a[3], a[4], a[5]);
                    let dx: f64 = a[0] + a[2] + a[4] + a[6] + a[8];
                    let dy: f64 = a[1] + a[3] + a[5] + a[7] + a[9];
                    // The last coordinate is whichever axis moved less.
                    if dx.abs() > dy.abs() {
                        self.curve(a[6], a[7], a[8], a[9], a[10], sy - self.y - a[7] - a[9]);
                    } else {
                        self.curve(a[6], a[7], a[8], a[9], sx - self.x - a[6] - a[8], a[10]);
                    }
                }
                self.stack.clear();
            }
            // put/get, the transient array.
            20 => {
                let (Some(index), Some(value)) = (self.stack.pop(), self.stack.pop()) else {
                    return;
                };
                if let Some(slot) = self.transient.get_mut(index.max(0.0) as usize) {
                    *slot = value;
                }
            }
            21 => {
                let Some(index) = self.stack.pop() else {
                    return;
                };
                let value = self
                    .transient
                    .get(index.max(0.0) as usize)
                    .copied()
                    .unwrap_or(0.0);
                self.push(value);
            }
            _ => self.stack.clear(),
        }
    }

    fn curve(&mut self, dx1: f64, dy1: f64, dx2: f64, dy2: f64, dx3: f64, dy3: f64) {
        let c1x = self.x + dx1;
        let c1y = self.y + dy1;
        let c2x = c1x + dx2;
        let c2y = c1y + dy2;
        self.x = c2x + dx3;
        self.y = c2y + dy3;
        self.outline.push(Segment::CurveTo {
            c1x,
            c1y,
            c2x,
            c2y,
            x: self.x,
            y: self.y,
        });
    }

    fn push(&mut self, value: f64) {
        // The specification caps the stack at 48; a longer one is corruption.
        if self.stack.len() < 48 && value.is_finite() {
            self.stack.push(value);
        }
    }

    fn arg(&self, index: usize) -> f64 {
        self.stack.get(index).copied().unwrap_or(0.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dict_operands_decode_in_every_encoding() {
        // 139 is zero; 247/108 is the two-byte positive form; 28 is a short.
        let dict = parse_dict(&[139, 0]);
        assert_eq!(
            dict.first().map(|(op, v)| (*op, v.clone())),
            Some((0, vec![0.0]))
        );

        let dict = parse_dict(&[247, 0, 0]);
        assert_eq!(
            dict.first().and_then(|(_, v)| v.first().copied()),
            Some(108.0)
        );

        let dict = parse_dict(&[251, 0, 0]);
        assert_eq!(
            dict.first().and_then(|(_, v)| v.first().copied()),
            Some(-108.0)
        );

        let mut short = vec![28];
        short.extend_from_slice(&1000i16.to_be_bytes());
        short.push(0);
        let dict = parse_dict(&short);
        assert_eq!(
            dict.first().and_then(|(_, v)| v.first().copied()),
            Some(1000.0)
        );
    }

    #[test]
    fn real_numbers_decode_from_nibbles() {
        // -2.25 is: e (minus) 2 a (point) 2 5 f (end)
        let (value, _) = parse_real(&[0xE2, 0xA2, 0x5F], 0);
        assert!((value - -2.25).abs() < 1e-9, "got {value}");

        // 0.001 as 0 . 0 0 1
        let (value, _) = parse_real(&[0x0A, 0x00, 0x1F], 0);
        assert!((value - 0.001).abs() < 1e-9, "got {value}");
    }

    #[test]
    fn two_byte_operators_are_distinguished() {
        // 12 7 is FontMatrix, which must not collide with operator 7.
        let dict = parse_dict(&[139, 12, 7]);
        assert_eq!(dict.first().map(|(op, _)| *op), Some(0x0C07));
    }

    #[test]
    fn subroutine_bias_follows_the_specification() {
        assert_eq!(bias(0), 107);
        assert_eq!(bias(1239), 107);
        assert_eq!(bias(1240), 1131);
        assert_eq!(bias(33899), 1131);
        assert_eq!(bias(33900), 32768);
    }

    #[test]
    fn garbage_is_rejected_without_panicking() {
        assert!(Cff::parse(&[]).is_none());
        assert!(Cff::parse(&[1, 0, 4, 1]).is_none());
        for len in 0..64 {
            let data: Vec<u8> = (0..len).map(|i| (i * 7) as u8).collect();
            let _ = Cff::parse(&data);
        }
        // A DICT of arbitrary bytes must terminate.
        for len in 0..256 {
            let data: Vec<u8> = (0..len).map(|i| (i % 256) as u8).collect();
            let _ = parse_dict(&data);
        }
    }
}
