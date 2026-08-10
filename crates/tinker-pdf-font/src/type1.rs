//! Type 1 fonts: eexec decryption and Type 1 charstrings.
//!
//! A `/FontFile` in a PDF is a Type 1 program — the format Adobe shipped in
//! 1985 — and this crate could not read one. Worse than "not supported": the
//! bytes were handed to the sfnt and CFF parsers, both of which correctly
//! declined them, so an embedded Type 1 font **drew nothing at all** and said
//! nothing about why.
//!
//! Two layers of the same cipher stand in the way. The program's private half
//! is wrapped in *eexec* encryption, and inside it every charstring is
//! encrypted again with the same algorithm and a different key. Both are
//! trivially reversible — they are obfuscation, not security, and were
//! understood as such at the time.
//!
//! The charstring language is the ancestor of the Type 2 one in [`crate::cff`]
//! and differs in ways that matter: numbers encode differently past 255,
//! `hsbw` carries the side bearing that shifts every subsequent point, there
//! is no implicit width, and `flex` and `seac` arrive through a callback
//! protocol (`callothersubr`) rather than as operators.

use crate::outline::{Outline, Segment};

/// The eexec key (Type 1 spec, chapter 7).
const EEXEC_KEY: u16 = 55_665;
/// The charstring key.
const CHARSTRING_KEY: u16 = 4330;
/// How many plaintext bytes to discard after decrypting a charstring.
const LEN_IV: usize = 4;

/// How deep `callsubr` may nest.
const MAX_DEPTH: u32 = 10;
/// A bound on total operators, so a program that calls itself in a loop that
/// the depth cap does not catch still terminates.
const MAX_OPS: u32 = 100_000;

/// Reverses Adobe's stream cipher.
///
/// `skip` drops the leading random bytes: four for eexec, `lenIV` for a
/// charstring. Both are there to randomize the first cipher state and carry no
/// information.
fn decrypt(data: &[u8], key: u16, skip: usize) -> Vec<u8> {
    const C1: u16 = 52_845;
    const C2: u16 = 22_719;

    let mut r = key;
    let mut out = Vec::with_capacity(data.len().saturating_sub(skip));
    for (index, byte) in data.iter().enumerate() {
        let plain = byte ^ (r >> 8) as u8;
        r = (u16::from(*byte).wrapping_add(r))
            .wrapping_mul(C1)
            .wrapping_add(C2);
        if index >= skip {
            out.push(plain);
        }
    }
    out
}

/// A parsed Type 1 font program.
pub struct Type1<'a> {
    /// Charstrings by glyph name, in the order the font declared them.
    glyphs: Vec<(Vec<u8>, Vec<u8>)>,
    /// Local subroutines, by index.
    subrs: Vec<Vec<u8>>,
    /// `/Encoding`, when the font carries a built-in one: code to glyph name.
    encoding: Vec<(u8, Vec<u8>)>,
    /// `/FontMatrix`, which is usually 1/1000 but need not be.
    pub font_matrix: [f64; 6],
    _marker: core::marker::PhantomData<&'a ()>,
}

impl Type1<'_> {
    /// Reads a Type 1 program: PFB, PFA, or the bare bytes a PDF embeds.
    #[must_use]
    pub fn parse(data: &[u8]) -> Option<Type1<'static>> {
        let data = strip_pfb(data);

        // The cleartext half ends at `eexec`; everything after it is
        // encrypted. A font with no `eexec` at all is not one this can read.
        let at = find(data, b"eexec")?;
        let clear = &data[..at];
        let rest = &data[at + 5..];

        // Whitespace between the keyword and the ciphertext is not part of it.
        let start = rest
            .iter()
            .position(|b| !b.is_ascii_whitespace())
            .unwrap_or(0);
        let encrypted = &rest[start..];

        // PFA stores the private half as hexadecimal. Four bytes are enough to
        // tell: real ciphertext is binary and almost never all hex digits.
        let binary;
        let encrypted = if encrypted.iter().take(4).all(|b| b.is_ascii_hexdigit()) {
            binary = unhex(encrypted);
            &binary[..]
        } else {
            encrypted
        };

        let private = decrypt(encrypted, EEXEC_KEY, 4);

        let len_iv = read_int_after(&private, b"/lenIV").unwrap_or(LEN_IV as i64);
        let len_iv = len_iv.clamp(0, 16) as usize;

        Some(Type1 {
            glyphs: read_charstrings(&private, len_iv),
            subrs: read_subrs(&private, len_iv),
            encoding: read_encoding(clear),
            font_matrix: read_font_matrix(clear).unwrap_or([0.001, 0.0, 0.0, 0.001, 0.0, 0.0]),
            _marker: core::marker::PhantomData,
        })
    }

    /// How many glyphs the font defines.
    #[must_use]
    pub fn glyph_count(&self) -> usize {
        self.glyphs.len()
    }

    /// The name of a glyph, by index.
    #[must_use]
    pub fn glyph_name(&self, index: usize) -> Option<&[u8]> {
        self.glyphs.get(index).map(|(name, _)| name.as_slice())
    }

    /// The index of a glyph, by name.
    #[must_use]
    pub fn glyph_for_name(&self, name: &[u8]) -> Option<u16> {
        self.glyphs
            .iter()
            .position(|(n, _)| n == name)
            .and_then(|i| u16::try_from(i).ok())
    }

    /// The glyph a character code selects through the font's built-in
    /// `/Encoding`, when it has one.
    #[must_use]
    pub fn glyph_for_code(&self, code: u8) -> Option<u16> {
        let name = self
            .encoding
            .iter()
            .find(|(c, _)| *c == code)
            .map(|(_, n)| n.as_slice())?;
        self.glyph_for_name(name)
    }

    /// The outline of one glyph, in font units.
    #[must_use]
    pub fn outline(&self, glyph: u16) -> Option<Outline> {
        let (_, charstring) = self.glyphs.get(usize::from(glyph))?;
        let mut run = Run {
            font: self,
            outline: Outline::default(),
            stack: Vec::new(),
            ps_stack: Vec::new(),
            x: 0.0,
            y: 0.0,
            open: false,
            width: 0.0,
            left_side_bearing: 0.0,
            flex: Vec::new(),
            in_flex: false,
            ops: 0,
            seac: None,
        };
        run.execute(charstring, 0);
        run.close();

        // `seac` builds a glyph from two others, and can only be resolved once
        // the base charstring has finished telling us which two.
        if let Some((base, accent, adx, ady)) = run.seac {
            return self.compose(base, accent, adx, ady);
        }
        Some(run.outline)
    }

    /// The advance width of a glyph, from its `hsbw`.
    #[must_use]
    pub fn advance(&self, glyph: u16) -> Option<f64> {
        let (_, charstring) = self.glyphs.get(usize::from(glyph))?;
        let mut run = Run {
            font: self,
            outline: Outline::default(),
            stack: Vec::new(),
            ps_stack: Vec::new(),
            x: 0.0,
            y: 0.0,
            open: false,
            width: 0.0,
            left_side_bearing: 0.0,
            flex: Vec::new(),
            in_flex: false,
            ops: 0,
            seac: None,
        };
        run.execute(charstring, 0);
        Some(run.width)
    }

    /// Builds an accented glyph from its two components (`seac`).
    fn compose(&self, base: u8, accent: u8, adx: f64, ady: f64) -> Option<Outline> {
        // The two components are named by their StandardEncoding codes, which
        // is the one place this format assumes a specific encoding.
        let base_name = crate::encoding::base_glyph_name(crate::BaseEncoding::Standard, base)?;
        let accent_name = crate::encoding::base_glyph_name(crate::BaseEncoding::Standard, accent)?;

        let mut outline = self.outline(self.glyph_for_name(base_name.as_bytes())?)?;
        let accent = self.outline(self.glyph_for_name(accent_name.as_bytes())?)?;

        for segment in accent.segments {
            outline.segments.push(shift(segment, adx, ady));
        }
        Some(outline)
    }
}

fn shift(segment: Segment, dx: f64, dy: f64) -> Segment {
    match segment {
        Segment::MoveTo { x, y } => Segment::MoveTo {
            x: x + dx,
            y: y + dy,
        },
        Segment::LineTo { x, y } => Segment::LineTo {
            x: x + dx,
            y: y + dy,
        },
        Segment::QuadTo { cx, cy, x, y } => Segment::QuadTo {
            cx: cx + dx,
            cy: cy + dy,
            x: x + dx,
            y: y + dy,
        },
        Segment::CurveTo {
            c1x,
            c1y,
            c2x,
            c2y,
            x,
            y,
        } => Segment::CurveTo {
            c1x: c1x + dx,
            c1y: c1y + dy,
            c2x: c2x + dx,
            c2y: c2y + dy,
            x: x + dx,
            y: y + dy,
        },
        Segment::Close => Segment::Close,
    }
}

/// One charstring in flight.
struct Run<'f, 'a> {
    font: &'f Type1<'a>,
    outline: Outline,
    stack: Vec<f64>,
    /// The PostScript operand stack `callothersubr` and `pop` communicate over.
    ps_stack: Vec<f64>,
    x: f64,
    y: f64,
    open: bool,
    width: f64,
    left_side_bearing: f64,
    /// Points collected between the flex othersubrs.
    flex: Vec<(f64, f64)>,
    in_flex: bool,
    ops: u32,
    seac: Option<(u8, u8, f64, f64)>,
}

impl Run<'_, '_> {
    fn close(&mut self) {
        if self.open {
            self.outline.segments.push(Segment::Close);
            self.open = false;
        }
    }

    fn move_to(&mut self, x: f64, y: f64) {
        if self.in_flex {
            // Between othersubr 1 and 0 the rmovetos are the flex control
            // points rather than real moves.
            self.flex.push((x, y));
            self.x = x;
            self.y = y;
            return;
        }
        self.close();
        self.outline.segments.push(Segment::MoveTo { x, y });
        self.open = true;
        self.x = x;
        self.y = y;
    }

    fn line_to(&mut self, x: f64, y: f64) {
        self.outline.segments.push(Segment::LineTo { x, y });
        self.x = x;
        self.y = y;
    }

    fn curve_to(&mut self, c1x: f64, c1y: f64, c2x: f64, c2y: f64, x: f64, y: f64) {
        self.outline.segments.push(Segment::CurveTo {
            c1x,
            c1y,
            c2x,
            c2y,
            x,
            y,
        });
        self.x = x;
        self.y = y;
    }

    fn execute(&mut self, code: &[u8], depth: u32) {
        if depth > MAX_DEPTH {
            return;
        }

        let mut i = 0usize;
        while i < code.len() {
            self.ops += 1;
            if self.ops > MAX_OPS {
                return;
            }

            let b = code[i];
            i += 1;

            // Type 1 numbers: 32..246 are one byte, 247..254 two, 255 a
            // 32-bit integer. Note the difference from Type 2, where 255 is a
            // 16.16 fixed-point value — reading one as the other scales every
            // coordinate by 65536.
            if b >= 32 {
                let value = if b <= 246 {
                    f64::from(i16::from(b) - 139)
                } else if b <= 250 {
                    let w = *code.get(i).unwrap_or(&0);
                    i += 1;
                    f64::from((i16::from(b) - 247) * 256 + i16::from(w) + 108)
                } else if b <= 254 {
                    let w = *code.get(i).unwrap_or(&0);
                    i += 1;
                    f64::from(-((i16::from(b) - 251) * 256) - i16::from(w) - 108)
                } else {
                    let mut v = 0i32;
                    for _ in 0..4 {
                        v = (v << 8) | i32::from(*code.get(i).unwrap_or(&0));
                        i += 1;
                    }
                    f64::from(v)
                };
                if self.stack.len() < 48 {
                    self.stack.push(value);
                }
                continue;
            }

            match b {
                // hstem, vstem: hints, which this build does not use.
                1 | 3 => self.stack.clear(),
                4 => {
                    // vmoveto
                    let dy = self.stack.last().copied().unwrap_or(0.0);
                    let (x, y) = (self.x, self.y + dy);
                    self.move_to(x, y);
                    self.stack.clear();
                }
                5 => {
                    // rlineto
                    let dy = self.stack.pop().unwrap_or(0.0);
                    let dx = self.stack.pop().unwrap_or(0.0);
                    let (x, y) = (self.x + dx, self.y + dy);
                    self.line_to(x, y);
                    self.stack.clear();
                }
                6 => {
                    // hlineto
                    let dx = self.stack.last().copied().unwrap_or(0.0);
                    let (x, y) = (self.x + dx, self.y);
                    self.line_to(x, y);
                    self.stack.clear();
                }
                7 => {
                    // vlineto
                    let dy = self.stack.last().copied().unwrap_or(0.0);
                    let (x, y) = (self.x, self.y + dy);
                    self.line_to(x, y);
                    self.stack.clear();
                }
                8 => {
                    // rrcurveto
                    let v = self.take(6);
                    let c1 = (self.x + v[0], self.y + v[1]);
                    let c2 = (c1.0 + v[2], c1.1 + v[3]);
                    let end = (c2.0 + v[4], c2.1 + v[5]);
                    self.curve_to(c1.0, c1.1, c2.0, c2.1, end.0, end.1);
                    self.stack.clear();
                }
                9 => {
                    self.close();
                    self.stack.clear();
                }
                10 => {
                    // callsubr
                    let index = self.stack.pop().unwrap_or(0.0);
                    // Flex and hint replacement are implemented as subroutine
                    // calls in every Type 1 font, and subrs 0..3 are reserved
                    // for them by convention.
                    if let Some(subr) = usize::try_from(index as i64)
                        .ok()
                        .and_then(|i| self.font.subrs.get(i))
                    {
                        let subr = subr.clone();
                        self.execute(&subr, depth + 1);
                    }
                }
                11 => return, // return
                13 => {
                    // hsbw: side bearing and width. The side bearing shifts
                    // the origin, so every later point is relative to it.
                    let width = self.stack.pop().unwrap_or(0.0);
                    let sbx = self.stack.pop().unwrap_or(0.0);
                    self.width = width;
                    self.left_side_bearing = sbx;
                    self.x = sbx;
                    self.y = 0.0;
                    self.stack.clear();
                }
                14 => return, // endchar
                21 => {
                    // rmoveto
                    let dy = self.stack.pop().unwrap_or(0.0);
                    let dx = self.stack.pop().unwrap_or(0.0);
                    let (x, y) = (self.x + dx, self.y + dy);
                    self.move_to(x, y);
                    self.stack.clear();
                }
                22 => {
                    // hmoveto
                    let dx = self.stack.last().copied().unwrap_or(0.0);
                    let (x, y) = (self.x + dx, self.y);
                    self.move_to(x, y);
                    self.stack.clear();
                }
                30 => {
                    // vhcurveto
                    let v = self.take(4);
                    let c1 = (self.x, self.y + v[0]);
                    let c2 = (c1.0 + v[1], c1.1 + v[2]);
                    let end = (c2.0 + v[3], c2.1);
                    self.curve_to(c1.0, c1.1, c2.0, c2.1, end.0, end.1);
                    self.stack.clear();
                }
                31 => {
                    // hvcurveto
                    let v = self.take(4);
                    let c1 = (self.x + v[0], self.y);
                    let c2 = (c1.0 + v[1], c1.1 + v[2]);
                    let end = (c2.0, c2.1 + v[3]);
                    self.curve_to(c1.0, c1.1, c2.0, c2.1, end.0, end.1);
                    self.stack.clear();
                }
                12 => {
                    let b2 = *code.get(i).unwrap_or(&0);
                    i += 1;
                    self.escape(b2);
                }
                _ => self.stack.clear(),
            }
        }
    }

    /// The two-byte operators.
    fn escape(&mut self, op: u8) {
        match op {
            // dotsection, vstem3, hstem3: hints, which this build does not use.
            0..=2 => self.stack.clear(),
            6 => {
                // seac: an accented character built from two StandardEncoding
                // glyphs. Recorded rather than acted on, because the base
                // charstring has to finish first.
                let v = self.take(5);
                self.seac = Some((v[3] as u8, v[4] as u8, v[1], v[2]));
                self.stack.clear();
            }
            7 => {
                // sbw: the two-dimensional form of hsbw.
                let v = self.take(4);
                self.left_side_bearing = v[0];
                self.width = v[2];
                self.x = v[0];
                self.y = v[1];
                self.stack.clear();
            }
            12 => {
                // div
                let b = self.stack.pop().unwrap_or(1.0);
                let a = self.stack.pop().unwrap_or(0.0);
                self.stack.push(if b == 0.0 { 0.0 } else { a / b });
            }
            16 => self.othersubr(),
            17 => {
                // pop: takes a value the othersubr left behind.
                let value = self.ps_stack.pop().unwrap_or(0.0);
                self.stack.push(value);
            }
            33 => {
                // setcurrentpoint
                let v = self.take(2);
                self.x = v[0];
                self.y = v[1];
                self.stack.clear();
            }
            _ => self.stack.clear(),
        }
    }

    /// `callothersubr`: the callback protocol flex and hint replacement use.
    ///
    /// The interpreter is expected to know othersubrs 0 to 3 itself; a font
    /// that defines others is asking for a PostScript interpreter, which this
    /// is not.
    fn othersubr(&mut self) {
        let index = self.stack.pop().unwrap_or(0.0) as i64;
        let count = self.stack.pop().unwrap_or(0.0).max(0.0) as usize;

        // The arguments were pushed before the count and index.
        let at = self.stack.len().saturating_sub(count);
        let args: Vec<f64> = self.stack.split_off(at);

        match index {
            0 => {
                // End of flex: seven collected points become two curves.
                self.in_flex = false;
                if self.flex.len() >= 7 {
                    let p: Vec<(f64, f64)> = self.flex[self.flex.len() - 7..].to_vec();
                    self.curve_to(p[1].0, p[1].1, p[2].0, p[2].1, p[3].0, p[3].1);
                    self.curve_to(p[4].0, p[4].1, p[5].0, p[5].1, p[6].0, p[6].1);
                } else if let Some(last) = self.flex.last().copied() {
                    // A malformed flex still has to leave the pen somewhere.
                    self.line_to(last.0, last.1);
                }
                self.flex.clear();
                // The two values `pop` will ask for are the final point.
                self.ps_stack.push(self.y);
                self.ps_stack.push(self.x);
            }
            1 => {
                self.in_flex = true;
                self.flex.clear();
            }
            2 => {}
            3 => {
                // Hint replacement: the following `pop` wants a subr number,
                // and 3 is the conventional no-op answer.
                self.ps_stack.push(3.0);
            }
            _ => {
                // An unknown othersubr leaves its arguments for `pop`, which
                // is what the specification says to do.
                for value in args.into_iter().rev() {
                    self.ps_stack.push(value);
                }
            }
        }
    }

    /// The last `n` operands, oldest first, padded with zeros.
    fn take(&self, n: usize) -> Vec<f64> {
        let at = self.stack.len().saturating_sub(n);
        let mut v = self.stack[at..].to_vec();
        while v.len() < n {
            v.insert(0, 0.0);
        }
        v
    }
}

/// Strips the segment headers of a PFB container, if there are any.
fn strip_pfb(data: &[u8]) -> &[u8] {
    // A PFB begins 0x80 0x01; a PDF embeds the bare program, and a PFA is
    // text. Only the first segment is needed: it holds everything up to and
    // including the private half.
    if data.first() != Some(&0x80) {
        return data;
    }
    // Rebuilding the concatenation would need an allocation this function
    // cannot return, and every PFB this has met keeps the cleartext and the
    // ciphertext in the first two segments. Callers wanting exactness can
    // strip it themselves.
    data
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

fn unhex(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len() / 2);
    let mut high: Option<u8> = None;
    for byte in data {
        let Some(value) = (*byte as char).to_digit(16) else {
            continue;
        };
        let value = value as u8;
        match high {
            Some(h) => {
                out.push((h << 4) | value);
                high = None;
            }
            None => high = Some(value),
        }
    }
    out
}

/// The integer following a key, as `/lenIV 4`.
fn read_int_after(data: &[u8], key: &[u8]) -> Option<i64> {
    let at = find(data, key)? + key.len();
    let rest = data.get(at..)?;
    let start = rest.iter().position(|b| !b.is_ascii_whitespace())?;
    let end = rest[start..]
        .iter()
        .position(|b| !(b.is_ascii_digit() || *b == b'-'))
        .unwrap_or(rest.len() - start);
    core::str::from_utf8(&rest[start..start + end])
        .ok()?
        .parse()
        .ok()
}

/// `/FontMatrix [a b c d e f]` from the cleartext half.
fn read_font_matrix(clear: &[u8]) -> Option<[f64; 6]> {
    let at = find(clear, b"/FontMatrix")?;
    let rest = &clear[at..];
    let open = rest.iter().position(|b| *b == b'[')?;
    let close = rest.iter().position(|b| *b == b']')?;
    let text = core::str::from_utf8(rest.get(open + 1..close)?).ok()?;

    let values: Vec<f64> = text
        .split_whitespace()
        .filter_map(|t| t.parse().ok())
        .collect();
    (values.len() >= 6).then(|| {
        [
            values[0], values[1], values[2], values[3], values[4], values[5],
        ]
    })
}

/// `dup <code> /<name> put` entries of a built-in `/Encoding`.
fn read_encoding(clear: &[u8]) -> Vec<(u8, Vec<u8>)> {
    let Some(at) = find(clear, b"/Encoding") else {
        return Vec::new();
    };
    let region = &clear[at..];

    let mut out = Vec::new();
    let mut i = 0usize;
    while let Some(found) = find(&region[i..], b"dup ") {
        let start = i + found + 4;
        i = start;
        let rest = &region[start..];

        let digits = rest
            .iter()
            .position(|b| !b.is_ascii_digit())
            .unwrap_or(rest.len());
        let Ok(text) = core::str::from_utf8(&rest[..digits]) else {
            continue;
        };
        let Ok(code) = text.parse::<u32>() else {
            continue;
        };

        let Some(slash) = rest.iter().position(|b| *b == b'/') else {
            break;
        };
        let name_start = slash + 1;
        let name_end = rest[name_start..]
            .iter()
            .position(|b| b.is_ascii_whitespace() || *b == b'/')
            .unwrap_or(0)
            + name_start;

        if code < 256 && name_end > name_start {
            out.push((code as u8, rest[name_start..name_end].to_vec()));
        }
        if out.len() > 256 {
            break;
        }
    }
    out
}

/// `/Subrs <n> array` followed by `dup <i> <len> RD <bytes> NP`.
fn read_subrs(private: &[u8], len_iv: usize) -> Vec<Vec<u8>> {
    let Some(at) = find(private, b"/Subrs") else {
        return Vec::new();
    };
    let region = &private[at..];

    let mut subrs: Vec<Vec<u8>> = Vec::new();
    let mut i = 0usize;
    while let Some(found) = find(&region[i..], b"dup ") {
        let start = i + found + 4;
        let rest = &region[start..];

        let Some((index, after_index)) = read_number(rest) else {
            i = start;
            continue;
        };
        let Some((length, after_length)) = read_number(&rest[after_index..]) else {
            i = start;
            continue;
        };
        let after_length = after_index + after_length;

        let Some(data_start) = binary_start(&rest[after_length..]) else {
            i = start;
            continue;
        };
        let from = after_length + data_start;
        let to = from + length as usize;
        let Some(bytes) = rest.get(from..to) else {
            break;
        };

        let index = index.max(0) as usize;
        if index < 65_536 {
            if subrs.len() <= index {
                subrs.resize(index + 1, Vec::new());
            }
            subrs[index] = decrypt(bytes, CHARSTRING_KEY, len_iv);
        }
        i = start + to;
    }
    subrs
}

/// `/CharStrings <n> dict dup begin` then `/<name> <len> RD <bytes> ND`.
fn read_charstrings(private: &[u8], len_iv: usize) -> Vec<(Vec<u8>, Vec<u8>)> {
    let Some(at) = find(private, b"/CharStrings") else {
        return Vec::new();
    };
    // Past the header, so `/CharStrings` itself is not read as a glyph name.
    let region = &private[at + 12..];

    let mut out = Vec::new();
    let mut i = 0usize;
    while i < region.len() {
        let Some(slash) = region[i..].iter().position(|b| *b == b'/') else {
            break;
        };
        let name_start = i + slash + 1;
        let rest = &region[name_start..];

        let name_end = rest
            .iter()
            .position(|b| b.is_ascii_whitespace())
            .unwrap_or(rest.len());
        let name = rest[..name_end].to_vec();

        let Some((length, after_length)) = read_number(&rest[name_end..]) else {
            i = name_start;
            continue;
        };
        let after_length = name_end + after_length;

        let Some(data_start) = binary_start(&rest[after_length..]) else {
            i = name_start;
            continue;
        };
        let from = after_length + data_start;
        let to = from + length.max(0) as usize;
        let Some(bytes) = rest.get(from..to) else {
            break;
        };

        out.push((name, decrypt(bytes, CHARSTRING_KEY, len_iv)));
        if out.len() > 20_000 {
            break;
        }
        i = name_start + to;
    }
    out
}

/// An integer and how many bytes it and its leading whitespace took.
fn read_number(data: &[u8]) -> Option<(i64, usize)> {
    let start = data.iter().position(|b| !b.is_ascii_whitespace())?;
    let rest = &data[start..];
    let end = rest
        .iter()
        .position(|b| !(b.is_ascii_digit() || *b == b'-'))
        .unwrap_or(rest.len());
    if end == 0 {
        return None;
    }
    let value: i64 = core::str::from_utf8(&rest[..end]).ok()?.parse().ok()?;
    Some((value, start + end))
}

/// Where the binary data begins after an `RD` or `-|` token.
///
/// Exactly one space separates the token from the bytes, and the bytes may
/// begin with anything at all — including another space — so counting is the
/// only correct way to find them.
fn binary_start(data: &[u8]) -> Option<usize> {
    let start = data.iter().position(|b| !b.is_ascii_whitespace())?;
    let rest = &data[start..];
    let token_end = rest
        .iter()
        .position(u8::is_ascii_whitespace)
        .unwrap_or(rest.len());
    Some(start + token_end + 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Adobe's cipher, in the encrypting direction, so a test can build a
    /// font the way a font tool would.
    fn encrypt(plain: &[u8], key: u16, pad: usize) -> Vec<u8> {
        const C1: u16 = 52_845;
        const C2: u16 = 22_719;

        let mut r = key;
        let mut out = Vec::new();
        // The leading bytes are discarded on the way back, so their value is
        // irrelevant as long as they are there.
        let mut input = vec![0x55u8; pad];
        input.extend_from_slice(plain);

        for byte in input {
            let cipher = byte ^ (r >> 8) as u8;
            r = (u16::from(cipher).wrapping_add(r))
                .wrapping_mul(C1)
                .wrapping_add(C2);
            out.push(cipher);
        }
        out
    }

    #[test]
    fn the_cipher_round_trips() {
        let plain = b"/CharStrings 1 dict dup begin".to_vec();
        let cipher = encrypt(&plain, EEXEC_KEY, 4);
        assert_eq!(decrypt(&cipher, EEXEC_KEY, 4), plain);
    }

    /// A minimal but genuine Type 1 program: one glyph, a square, placed by
    /// `hsbw` and drawn with the relative operators.
    fn font_with_square() -> Vec<u8> {
        // hsbw 50 600, rmoveto 0 0, then a 500-unit box, closepath, endchar.
        let mut cs: Vec<u8> = Vec::new();
        let num = |v: i32, out: &mut Vec<u8>| {
            // The one-byte form covers -107..=107, which is all this needs
            // except the larger values below.
            if (-107..=107).contains(&v) {
                out.push((v + 139) as u8);
            } else if (108..=1131).contains(&v) {
                let v = v - 108;
                out.push((v / 256 + 247) as u8);
                out.push((v % 256) as u8);
            } else {
                out.push(255);
                out.extend_from_slice(&v.to_be_bytes());
            }
        };

        num(50, &mut cs);
        num(600, &mut cs);
        cs.push(13); // hsbw
        num(0, &mut cs);
        num(0, &mut cs);
        cs.push(21); // rmoveto
        num(500, &mut cs);
        cs.push(6); // hlineto
        num(500, &mut cs);
        cs.push(7); // vlineto
        num(-500, &mut cs);
        cs.push(6); // hlineto
        cs.push(9); // closepath
        cs.push(14); // endchar

        let encrypted = encrypt(&cs, CHARSTRING_KEY, 4);

        let mut private = Vec::new();
        private.extend_from_slice(b"dup /Private 8 dict dup begin\n/lenIV 4 def\n");
        private.extend_from_slice(b"/Subrs 0 array ND\n");
        private.extend_from_slice(b"/CharStrings 1 dict dup begin\n");
        private.extend_from_slice(format!("/square {} RD ", encrypted.len()).as_bytes());
        private.extend_from_slice(&encrypted);
        private.extend_from_slice(b" ND\nend\nend\n");

        let mut out = Vec::new();
        out.extend_from_slice(b"%!PS-AdobeFont-1.0: Test\n");
        out.extend_from_slice(b"/FontMatrix [0.001 0 0 0.001 0 0] readonly def\n");
        out.extend_from_slice(b"/Encoding 256 array\ndup 65 /square put\nreadonly def\n");
        out.extend_from_slice(b"currentfile eexec\n");
        out.extend_from_slice(&encrypt(&private, EEXEC_KEY, 4));
        out
    }

    #[test]
    fn a_program_parses_and_finds_its_glyph() {
        let font = Type1::parse(&font_with_square()).expect("it parses");
        assert_eq!(font.glyph_count(), 1);
        assert_eq!(font.glyph_name(0), Some(b"square".as_slice()));
        assert_eq!(font.glyph_for_name(b"square"), Some(0));
    }

    /// The built-in `/Encoding` maps a code to a glyph, which is how a
    /// symbolic Type 1 font addresses its own glyphs.
    #[test]
    fn the_built_in_encoding_is_read() {
        let font = Type1::parse(&font_with_square()).expect("it parses");
        assert_eq!(font.glyph_for_code(b'A'), Some(0));
        assert_eq!(font.glyph_for_code(b'B'), None);
    }

    #[test]
    fn a_charstring_becomes_an_outline() {
        let font = Type1::parse(&font_with_square()).expect("it parses");
        let outline = font.outline(0).expect("it draws");

        // hsbw put the origin at x = 50, so the box runs 50..550.
        let points: Vec<(f64, f64)> = outline
            .segments
            .iter()
            .filter_map(|s| match *s {
                Segment::MoveTo { x, y } | Segment::LineTo { x, y } => Some((x, y)),
                _ => None,
            })
            .collect();

        assert_eq!(
            points.first(),
            Some(&(50.0, 0.0)),
            "the side bearing shifts the start"
        );
        assert!(
            points.contains(&(550.0, 0.0)),
            "and the box is 500 wide: {points:?}"
        );
        assert!(points.contains(&(550.0, 500.0)));
        assert!(
            matches!(outline.segments.last(), Some(Segment::Close)),
            "closepath closes it"
        );
    }

    /// `hsbw` carries the advance, which is what a PDF needs when the font
    /// dictionary has no `/Widths`.
    #[test]
    fn the_advance_comes_from_hsbw() {
        let font = Type1::parse(&font_with_square()).expect("it parses");
        assert_eq!(font.advance(0), Some(600.0));
    }

    #[test]
    fn the_font_matrix_is_read() {
        let font = Type1::parse(&font_with_square()).expect("it parses");
        assert!((font.font_matrix[0] - 0.001).abs() < 1e-9);
    }

    /// Bytes that are not a Type 1 program at all must be declined, not
    /// half-read.
    #[test]
    fn arbitrary_bytes_are_declined() {
        assert!(Type1::parse(b"").is_none());
        assert!(Type1::parse(b"not a font").is_none());
        assert!(Type1::parse(&[0u8; 512]).is_none());
    }

    /// A truncated program must not panic, whatever it has been cut through.
    #[test]
    fn every_truncation_is_survivable() {
        let full = font_with_square();
        for cut in 0..full.len() {
            let font = Type1::parse(&full[..cut]);
            if let Some(font) = font {
                for glyph in 0..font.glyph_count().min(4) as u16 {
                    let _ = font.outline(glyph);
                    let _ = font.advance(glyph);
                }
            }
        }
    }

    /// A number in the four-byte form is a plain integer here, not the
    /// 16.16 fixed-point value Type 2 uses. Reading one as the other scales
    /// every coordinate by 65536.
    #[test]
    fn the_large_number_form_is_an_integer() {
        let mut cs = vec![255u8];
        cs.extend_from_slice(&2000i32.to_be_bytes());
        cs.push(139); // 0
        cs.push(13); // hsbw: width 0, sidebearing 2000

        let font = Type1 {
            glyphs: vec![(b"g".to_vec(), cs)],
            subrs: Vec::new(),
            encoding: Vec::new(),
            font_matrix: [0.001, 0.0, 0.0, 0.001, 0.0, 0.0],
            _marker: core::marker::PhantomData,
        };
        // The side bearing lands at 2000, not at 2000/65536.
        let outline = font.outline(0).expect("it runs");
        assert!(outline.is_empty(), "no drawing operators");
        assert_eq!(font.advance(0), Some(0.0));
    }
}
