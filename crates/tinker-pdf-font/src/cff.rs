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
//!
//! # Which charstring runs
//!
//! A CFF font does not number its glyphs the way a document addresses them.
//! The charset says which *name* — or, in a CID-keyed font, which CID — each
//! glyph index carries, and everything a caller can ask for goes through it:
//! [`Cff::gid_for_name`] for a simple font, [`Cff::gid_for_cid`] for a
//! CID-keyed one, and [`Cff::gid_for_code`] for the encoding the font program
//! carries itself. A caller that used the character code as a glyph index
//! would draw whatever glyph happened to be at that position.

use crate::outline::{Outline, Segment};

/// A parsed CFF font.
#[derive(Clone, Debug)]
pub struct Cff<'a> {
    charstrings: Index<'a>,
    global_subrs: Index<'a>,
    strings: Index<'a>,
    /// The top-level Private DICT: local subroutines and the two widths.
    private: Private<'a>,
    /// Which SID — or, in a CID-keyed font, which CID — each glyph carries.
    charset: Vec<u16>,
    /// The charset inverted and sorted, so a name or a CID reaches a glyph
    /// without scanning. Deduplicated on the first glyph that claims a value,
    /// which is what a reader has to pick when a font names two.
    by_sid: Vec<(u16, u16)>,
    /// The font's own encoding: which glyph each of the 256 codes selects,
    /// zero where the encoding defines none.
    encoding: Vec<u16>,
    /// Whether the Top DICT carries `ROS`, which is what makes a font
    /// CID-keyed and its charset a CID map rather than a name map.
    is_cid: bool,
    /// CID-keyed fonts: which Font DICT each glyph draws its Private DICT and
    /// local subroutines from. Empty when the font is not CID-keyed.
    fd_select: Vec<u8>,
    /// The FDArray's Private DICTs, and each one's own font matrix.
    fds: Vec<(Private<'a>, Option<[f64; 6]>)>,
    /// The font matrix, which is usually 1/1000 but need not be.
    pub font_matrix: [f64; 6],
    /// Whether the Top DICT stated the matrix above, rather than it being the
    /// default. A CID-keyed font may put the real one in its Font DICTs.
    top_has_matrix: bool,
}

/// A Private DICT's contribution to running a charstring.
#[derive(Clone, Debug, Default)]
struct Private<'a> {
    local_subrs: Index<'a>,
    default_width: f64,
    nominal_width: f64,
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

        // Offsets are 1-based from the first byte of the data, so item `i`
        // starts at `base + offsets[i] - 1` — and `get` is where that one is
        // subtracted. Taking it off here as well pointed `base` at the last
        // byte of the offset array, which shifted every item in every INDEX
        // one byte earlier and one byte shorter than it is.
        let base = offsets_at + (count + 1) * off_size;
        let end = base + offsets.last().copied().unwrap_or(1).saturating_sub(1);

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
        let (strings, at) = Index::parse(data, at)?;
        let (global_subrs, _) = Index::parse(data, at)?;

        let top = parse_dict(top_dicts.get(0)?);

        let charstrings_at = dict_get(&top, 17).and_then(<[f64]>::first).copied()? as usize;
        let (charstrings, _) = Index::parse(data, charstrings_at)?;
        let glyphs = charstrings.len();

        // The Private DICT gives the local subroutines and the widths.
        let private = match dict_get(&top, 18) {
            Some(operands) => read_private(data, operands)?,
            None => Private::default(),
        };

        // 12 30 is ROS: its presence, and nothing else, is what makes a font
        // CID-keyed, which changes what the charset means.
        let is_cid = dict_get(&top, 0x0C1E).is_some();

        // 15 is charset. Absent means the ISOAdobe predefined one, whose
        // offset is also zero.
        let charset_at = dict_get(&top, 15)
            .and_then(<[f64]>::first)
            .copied()
            .unwrap_or(0.0);
        let charset = read_charset(data, charset_at, glyphs, is_cid);
        let by_sid = invert_charset(&charset);

        // 16 is Encoding, which a CID-keyed font does not have: its glyphs are
        // reached by CID, and a one-byte code cannot address them.
        let encoding = if is_cid {
            Vec::new()
        } else {
            let at = dict_get(&top, 16)
                .and_then(<[f64]>::first)
                .copied()
                .unwrap_or(0.0);
            read_encoding(data, at, &by_sid)
        };

        // 12 36 FDArray and 12 37 FDSelect: a CID-keyed font's glyphs are
        // partitioned between Font DICTs, each with its own Private DICT and
        // its own local subroutines.
        let fds = match dict_get(&top, 0x0C24).and_then(<[f64]>::first) {
            Some(&at) => read_fd_array(data, at),
            None => Vec::new(),
        };
        let fd_select = match dict_get(&top, 0x0C25).and_then(<[f64]>::first) {
            Some(&at) if !fds.is_empty() => read_fd_select(data, at, glyphs),
            _ => Vec::new(),
        };

        // 12 7 is FontMatrix; the default is 1/1000 in both axes.
        let (font_matrix, top_has_matrix) = match dict_get(&top, 0x0C07) {
            Some(m) if m.len() >= 6 => ([m[0], m[1], m[2], m[3], m[4], m[5]], true),
            _ => ([0.001, 0.0, 0.0, 0.001, 0.0, 0.0], false),
        };

        Some(Cff {
            charstrings,
            global_subrs,
            strings,
            private,
            charset,
            by_sid,
            encoding,
            is_cid,
            fd_select,
            fds,
            font_matrix,
            top_has_matrix,
        })
    }

    /// How many glyphs the font holds.
    #[must_use]
    pub fn glyph_count(&self) -> usize {
        self.charstrings.len()
    }

    /// Whether the font is CID-keyed, which is what `ROS` declares.
    ///
    /// A CID-keyed font's charset maps glyphs to CIDs rather than to names, so
    /// [`Cff::gid_for_name`] answers nothing for one and [`Cff::gid_for_cid`]
    /// is the only way in.
    #[must_use]
    pub fn is_cid(&self) -> bool {
        self.is_cid
    }

    /// The name a glyph carries, through the charset and the string INDEX.
    ///
    /// `None` for a CID-keyed font, whose glyphs have CIDs and no names.
    #[must_use]
    pub fn glyph_name(&self, glyph: u16) -> Option<&'a str> {
        if self.is_cid {
            return None;
        }
        self.sid_name(*self.charset.get(usize::from(glyph))?)
    }

    /// The glyph a name selects.
    #[must_use]
    pub fn gid_for_name(&self, name: &str) -> Option<u16> {
        if self.is_cid {
            return None;
        }
        self.gid_for_sid(self.sid_for_name(name)?)
    }

    /// The glyph a CID selects, through the inverted charset.
    #[must_use]
    pub fn gid_for_cid(&self, cid: u32) -> Option<u16> {
        // A CID wider than the charset can express names no glyph, rather
        // than truncating into one that exists.
        let cid = u16::try_from(cid).ok()?;
        if !self.is_cid {
            return None;
        }
        self.gid_for_sid(cid)
    }

    /// The glyph the font's *own* encoding gives a code.
    ///
    /// This is the built-in encoding of the font program, which 9.6.6 makes
    /// the fallback: a PDF font dictionary's `/Encoding` wins where it names
    /// the glyph, and the caller is the one that knows whether it did.
    #[must_use]
    pub fn gid_for_code(&self, code: u8) -> Option<u16> {
        let glyph = *self.encoding.get(usize::from(code))?;
        (glyph != 0).then_some(glyph)
    }

    /// The matrix that maps one glyph's space into text space.
    ///
    /// Almost always the font's own, but a CID-keyed font may leave the Top
    /// DICT without one and put a different matrix in each Font DICT — an
    /// Adobe-Japan1 face built from sources drawn at different sizes does
    /// exactly that, and reading only the top-level default draws those
    /// glyphs at the wrong scale.
    #[must_use]
    pub fn font_matrix_for(&self, glyph: u16) -> [f64; 6] {
        let Some((_, Some(fd))) = self.fd_for(glyph) else {
            return self.font_matrix;
        };
        if self.top_has_matrix {
            // Both present: the glyph's space passes through the Font DICT's
            // matrix and then the Top DICT's.
            concat(*fd, self.font_matrix)
        } else {
            *fd
        }
    }

    /// One glyph's outline, in the font's own units.
    #[must_use]
    pub fn outline(&self, glyph: u16) -> Option<Outline> {
        let charstring = self.charstrings.get(usize::from(glyph))?;
        let mut ctx = self.charstring_context(glyph);
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
        let mut ctx = self.charstring_context(glyph);
        ctx.run(charstring);
        Some(ctx.width.unwrap_or(ctx.private.default_width))
    }

    fn charstring_context(&self, glyph: u16) -> Charstring<'a, '_> {
        Charstring {
            cff: self,
            private: match self.fd_for(glyph) {
                Some((private, _)) => private,
                None => &self.private,
            },
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
        }
    }

    /// The Font DICT a glyph draws from, in a CID-keyed font.
    fn fd_for(&self, glyph: u16) -> Option<(&Private<'a>, &Option<[f64; 6]>)> {
        let fd = *self.fd_select.get(usize::from(glyph))?;
        let (private, matrix) = self.fds.get(usize::from(fd))?;
        Some((private, matrix))
    }

    /// The lowest glyph carrying a SID, through the inverted charset.
    fn gid_for_sid(&self, sid: u16) -> Option<u16> {
        let at = self.by_sid.binary_search_by_key(&sid, |(s, _)| *s).ok()?;
        self.by_sid.get(at).map(|(_, gid)| *gid)
    }

    /// The SID a name has, from the standard strings or the string INDEX.
    fn sid_for_name(&self, name: &str) -> Option<u16> {
        if let Some(sid) = STANDARD_STRINGS.iter().position(|s| *s == name) {
            return u16::try_from(sid).ok();
        }
        // Anything else is a custom string, numbered from 391 upward.
        let at = (0..self.strings.len()).find(|i| self.strings.get(*i) == Some(name.as_bytes()))?;
        u16::try_from(at + STANDARD_STRINGS.len()).ok()
    }

    /// The string a SID names.
    fn sid_name(&self, sid: u16) -> Option<&'a str> {
        let sid = usize::from(sid);
        if let Some(name) = STANDARD_STRINGS.get(sid) {
            return Some(name);
        }
        let bytes = self.strings.get(sid - STANDARD_STRINGS.len())?;
        core::str::from_utf8(bytes).ok()
    }
}

/// Reads a Private DICT from its `(size, offset)` operand pair.
fn read_private<'a>(data: &'a [u8], operands: &[f64]) -> Option<Private<'a>> {
    let (Some(&size), Some(&offset)) = (operands.first(), operands.get(1)) else {
        return None;
    };
    if size < 0.0 || offset < 0.0 {
        return Some(Private::default());
    }
    let (size, offset) = (size as usize, offset as usize);
    let mut private = Private::default();
    if let Some(bytes) = data.get(offset..offset.saturating_add(size).min(data.len())) {
        let dict = parse_dict(bytes);
        private.default_width = dict_get(&dict, 20)
            .and_then(<[f64]>::first)
            .copied()
            .unwrap_or(0.0);
        private.nominal_width = dict_get(&dict, 21)
            .and_then(<[f64]>::first)
            .copied()
            .unwrap_or(0.0);
        // 19 is Subrs, whose offset is measured from the Private DICT rather
        // than from the start of the font.
        if let Some(&subrs) = dict_get(&dict, 19).and_then(<[f64]>::first) {
            if subrs >= 0.0 {
                if let Some((index, _)) = Index::parse(data, offset.saturating_add(subrs as usize))
                {
                    private.local_subrs = index;
                }
            }
        }
    }
    Some(private)
}

/// Reads the charset: the SID, or the CID, that each glyph carries.
///
/// Offsets 0, 1 and 2 name the predefined charsets rather than a position in
/// the file — a font whose charset is ISOAdobe writes no charset at all.
fn read_charset(data: &[u8], offset: f64, glyphs: usize, is_cid: bool) -> Vec<u16> {
    // Every entry is bounded by the glyph count, which the CharStrings INDEX
    // already capped at 65535: a run length in the file cannot make this
    // longer than the font has glyphs.
    let mut out = Vec::with_capacity(glyphs.min(1 << 16));
    if glyphs == 0 {
        return out;
    }
    // Glyph 0 is `.notdef` in every font and is not written down.
    out.push(0u16);

    if !(0.0..=(usize::MAX as f64)).contains(&offset) {
        return out;
    }
    let offset = offset as usize;

    match offset {
        // A CID-keyed font with a predefined charset offset has no name
        // ordering to borrow, so the identity is the only reading of it.
        0 if is_cid => {
            for gid in 1..glyphs {
                out.push(u16::try_from(gid).unwrap_or(0));
            }
        }
        // ISOAdobe: the first 229 standard strings, in order. It stops at 228
        // rather than running on, so a font with more glyphs than the charset
        // covers leaves the rest unnamed instead of naming them wrongly.
        0 => {
            for gid in 1..glyphs {
                out.push(u16::try_from(gid).ok().filter(|g| *g <= 228).unwrap_or(0));
            }
        }
        1 | 2 => {
            let table: &[u16] = if offset == 1 {
                EXPERT_CHARSET
            } else {
                EXPERT_SUBSET_CHARSET
            };
            for gid in 1..glyphs {
                out.push(table.get(gid - 1).copied().unwrap_or(0));
            }
        }
        _ => read_charset_table(data, offset, glyphs, &mut out),
    }
    out
}

/// Reads a charset written into the font, formats 0, 1 and 2.
///
/// Format 1 counts a run in one byte and format 2 in two. Confusing them does
/// not fail: it reads a plausible number and shifts every glyph after the
/// first run, so the font draws the wrong letters and nothing says why.
fn read_charset_table(data: &[u8], offset: usize, glyphs: usize, out: &mut Vec<u16>) {
    let Some(&format) = data.get(offset) else {
        return;
    };
    let mut at = offset + 1;
    match format {
        0 => {
            while out.len() < glyphs {
                let Some(sid) = be16(data, at) else { return };
                out.push(sid);
                at += 2;
            }
        }
        1 | 2 => {
            let wide = format == 2;
            while out.len() < glyphs {
                let Some(first) = be16(data, at) else { return };
                let left = if wide {
                    let Some(n) = be16(data, at + 2) else { return };
                    at += 4;
                    u32::from(n)
                } else {
                    let Some(&n) = data.get(at + 2) else { return };
                    at += 3;
                    u32::from(n)
                };
                // The run covers `left` glyphs *after* the first.
                for step in 0..=left {
                    if out.len() >= glyphs {
                        return;
                    }
                    out.push(first.saturating_add(step.min(0xFFFF) as u16));
                }
            }
        }
        _ => {}
    }
}

/// Inverts the charset, so a SID or a CID reaches a glyph by binary search.
///
/// Sorted by SID and then by glyph, deduplicated on the SID: a font that
/// names two glyphs the same resolves to the first, which is the only choice
/// that does not depend on how the table was built.
fn invert_charset(charset: &[u16]) -> Vec<(u16, u16)> {
    let mut out: Vec<(u16, u16)> = charset
        .iter()
        .enumerate()
        .filter_map(|(gid, sid)| Some((*sid, u16::try_from(gid).ok()?)))
        .collect();
    out.sort_unstable();
    out.dedup_by_key(|(sid, _)| *sid);
    out
}

/// Reads the font's built-in encoding into a glyph per code.
///
/// Offsets 0 and 1 name the two predefined encodings; anything else is a
/// table in the file. Both predefined encodings resolve through the charset,
/// because the encoding names a glyph and only the charset knows where it is.
fn read_encoding(data: &[u8], offset: f64, by_sid: &[(u16, u16)]) -> Vec<u16> {
    let mut out = vec![0u16; 256];
    let lookup = |sid: u16| -> u16 {
        by_sid
            .binary_search_by_key(&sid, |(s, _)| *s)
            .ok()
            .and_then(|at| by_sid.get(at).map(|(_, gid)| *gid))
            .unwrap_or(0)
    };

    if !(0.0..=(usize::MAX as f64)).contains(&offset) {
        return out;
    }
    let offset = offset as usize;

    match offset {
        0 | 1 => {
            let table: &[(u8, u16)] = if offset == 0 {
                STANDARD_ENCODING
            } else {
                EXPERT_ENCODING
            };
            for (code, sid) in table {
                if let Some(slot) = out.get_mut(usize::from(*code)) {
                    *slot = lookup(*sid);
                }
            }
            return out;
        }
        _ => {}
    }

    let Some(&format) = data.get(offset) else {
        return out;
    };
    let mut at = offset + 1;
    // The high bit says supplements follow the table itself.
    match format & 0x7F {
        0 => {
            let Some(&count) = data.get(at) else {
                return out;
            };
            at += 1;
            for i in 0..usize::from(count) {
                let Some(&code) = data.get(at + i) else { break };
                // Codes are listed for glyph 1 upward, in order.
                if let Some(slot) = out.get_mut(usize::from(code)) {
                    *slot = u16::try_from(i + 1).unwrap_or(0);
                }
            }
            at += usize::from(count);
        }
        1 => {
            let Some(&ranges) = data.get(at) else {
                return out;
            };
            at += 1;
            let mut glyph = 1u16;
            for i in 0..usize::from(ranges) {
                let (Some(&first), Some(&left)) = (data.get(at + i * 2), data.get(at + i * 2 + 1))
                else {
                    break;
                };
                for step in 0..=u16::from(left) {
                    let code = u16::from(first).saturating_add(step);
                    if let Some(slot) = out.get_mut(usize::from(code)) {
                        *slot = glyph;
                    }
                    glyph = glyph.saturating_add(1);
                }
            }
            at += usize::from(ranges) * 2;
        }
        _ => return out,
    }

    if format & 0x80 != 0 {
        // A supplement maps a further code to the glyph *named* by a SID,
        // which is how one glyph reaches two codes without being listed twice.
        let count = data.get(at).copied().unwrap_or(0);
        at += 1;
        for i in 0..usize::from(count) {
            let (Some(&code), Some(sid)) = (data.get(at + i * 3), be16(data, at + i * 3 + 1))
            else {
                break;
            };
            let glyph = lookup(sid);
            if glyph != 0 {
                if let Some(slot) = out.get_mut(usize::from(code)) {
                    *slot = glyph;
                }
            }
        }
    }

    out
}

/// Reads the FDArray: one Font DICT per set of glyphs, each with its own
/// Private DICT and possibly its own matrix.
fn read_fd_array(data: &[u8], offset: f64) -> Vec<(Private<'_>, Option<[f64; 6]>)> {
    if !(0.0..=(usize::MAX as f64)).contains(&offset) {
        return Vec::new();
    }
    let Some((index, _)) = Index::parse(data, offset as usize) else {
        return Vec::new();
    };
    let mut out = Vec::with_capacity(index.len().min(1 << 16));
    for i in 0..index.len() {
        let Some(bytes) = index.get(i) else { break };
        let dict = parse_dict(bytes);
        let private = dict_get(&dict, 18)
            .and_then(|operands| read_private(data, operands))
            .unwrap_or_default();
        let matrix = match dict_get(&dict, 0x0C07) {
            Some(m) if m.len() >= 6 => Some([m[0], m[1], m[2], m[3], m[4], m[5]]),
            _ => None,
        };
        out.push((private, matrix));
    }
    out
}

/// Reads FDSelect: which Font DICT each glyph belongs to, formats 0 and 3.
fn read_fd_select(data: &[u8], offset: f64, glyphs: usize) -> Vec<u8> {
    let mut out = vec![0u8; glyphs.min(1 << 16)];
    if !(0.0..=(usize::MAX as f64)).contains(&offset) || glyphs == 0 {
        return out;
    }
    let offset = offset as usize;
    match data.get(offset) {
        Some(0) => {
            for (gid, slot) in out.iter_mut().enumerate() {
                let Some(&fd) = data.get(offset + 1 + gid) else {
                    break;
                };
                *slot = fd;
            }
        }
        Some(3) => {
            let Some(ranges) = be16(data, offset + 1) else {
                return out;
            };
            let at = offset + 3;
            // Each range runs to the first glyph of the next one, and the
            // sentinel two bytes after the last range ends it.
            for i in 0..usize::from(ranges) {
                let (Some(first), Some(&fd)) = (be16(data, at + i * 3), data.get(at + i * 3 + 2))
                else {
                    break;
                };
                let Some(next) = be16(data, at + (i + 1) * 3) else {
                    break;
                };
                for gid in usize::from(first)..usize::from(next).min(out.len()) {
                    if let Some(slot) = out.get_mut(gid) {
                        *slot = fd;
                    }
                }
            }
        }
        _ => {}
    }
    out
}

/// `a` applied first, then `b`.
fn concat(a: [f64; 6], b: [f64; 6]) -> [f64; 6] {
    [
        a[0] * b[0] + a[1] * b[2],
        a[0] * b[1] + a[1] * b[3],
        a[2] * b[0] + a[3] * b[2],
        a[2] * b[1] + a[3] * b[3],
        a[4] * b[0] + a[5] * b[2] + b[4],
        a[4] * b[1] + a[5] * b[3] + b[5],
    ]
}

fn be16(data: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_be_bytes([*data.get(at)?, *data.get(at + 1)?]))
}

/// The 391 standard strings every CFF font shares (CFF specification,
/// Appendix A).
///
/// A SID below 391 names one of these; anything higher indexes the font's own
/// string INDEX, which is what makes `sid - 391` the only arithmetic between a
/// charset entry and a glyph name. Compiled in rather than generated: 391
/// fixed strings are not worth a build step, and four kilobytes is not a
/// number the wasm budget notices.
const STANDARD_STRINGS: &[&str; 391] = &[
    ".notdef",
    "space",
    "exclam",
    "quotedbl",
    "numbersign",
    "dollar",
    "percent",
    "ampersand",
    "quoteright",
    "parenleft",
    "parenright",
    "asterisk",
    "plus",
    "comma",
    "hyphen",
    "period",
    "slash",
    "zero",
    "one",
    "two",
    "three",
    "four",
    "five",
    "six",
    "seven",
    "eight",
    "nine",
    "colon",
    "semicolon",
    "less",
    "equal",
    "greater",
    "question",
    "at",
    "A",
    "B",
    "C",
    "D",
    "E",
    "F",
    "G",
    "H",
    "I",
    "J",
    "K",
    "L",
    "M",
    "N",
    "O",
    "P",
    "Q",
    "R",
    "S",
    "T",
    "U",
    "V",
    "W",
    "X",
    "Y",
    "Z",
    "bracketleft",
    "backslash",
    "bracketright",
    "asciicircum",
    "underscore",
    "quoteleft",
    "a",
    "b",
    "c",
    "d",
    "e",
    "f",
    "g",
    "h",
    "i",
    "j",
    "k",
    "l",
    "m",
    "n",
    "o",
    "p",
    "q",
    "r",
    "s",
    "t",
    "u",
    "v",
    "w",
    "x",
    "y",
    "z",
    "braceleft",
    "bar",
    "braceright",
    "asciitilde",
    "exclamdown",
    "cent",
    "sterling",
    "fraction",
    "yen",
    "florin",
    "section",
    "currency",
    "quotesingle",
    "quotedblleft",
    "guillemotleft",
    "guilsinglleft",
    "guilsinglright",
    "fi",
    "fl",
    "endash",
    "dagger",
    "daggerdbl",
    "periodcentered",
    "paragraph",
    "bullet",
    "quotesinglbase",
    "quotedblbase",
    "quotedblright",
    "guillemotright",
    "ellipsis",
    "perthousand",
    "questiondown",
    "grave",
    "acute",
    "circumflex",
    "tilde",
    "macron",
    "breve",
    "dotaccent",
    "dieresis",
    "ring",
    "cedilla",
    "hungarumlaut",
    "ogonek",
    "caron",
    "emdash",
    "AE",
    "ordfeminine",
    "Lslash",
    "Oslash",
    "OE",
    "ordmasculine",
    "ae",
    "dotlessi",
    "lslash",
    "oslash",
    "oe",
    "germandbls",
    "onesuperior",
    "logicalnot",
    "mu",
    "trademark",
    "Eth",
    "onehalf",
    "plusminus",
    "Thorn",
    "onequarter",
    "divide",
    "brokenbar",
    "degree",
    "thorn",
    "threequarters",
    "twosuperior",
    "registered",
    "minus",
    "eth",
    "multiply",
    "threesuperior",
    "copyright",
    "Aacute",
    "Acircumflex",
    "Adieresis",
    "Agrave",
    "Aring",
    "Atilde",
    "Ccedilla",
    "Eacute",
    "Ecircumflex",
    "Edieresis",
    "Egrave",
    "Iacute",
    "Icircumflex",
    "Idieresis",
    "Igrave",
    "Ntilde",
    "Oacute",
    "Ocircumflex",
    "Odieresis",
    "Ograve",
    "Otilde",
    "Scaron",
    "Uacute",
    "Ucircumflex",
    "Udieresis",
    "Ugrave",
    "Yacute",
    "Ydieresis",
    "Zcaron",
    "aacute",
    "acircumflex",
    "adieresis",
    "agrave",
    "aring",
    "atilde",
    "ccedilla",
    "eacute",
    "ecircumflex",
    "edieresis",
    "egrave",
    "iacute",
    "icircumflex",
    "idieresis",
    "igrave",
    "ntilde",
    "oacute",
    "ocircumflex",
    "odieresis",
    "ograve",
    "otilde",
    "scaron",
    "uacute",
    "ucircumflex",
    "udieresis",
    "ugrave",
    "yacute",
    "ydieresis",
    "zcaron",
    "exclamsmall",
    "Hungarumlautsmall",
    "dollaroldstyle",
    "dollarsuperior",
    "ampersandsmall",
    "Acutesmall",
    "parenleftsuperior",
    "parenrightsuperior",
    "twodotenleader",
    "onedotenleader",
    "zerooldstyle",
    "oneoldstyle",
    "twooldstyle",
    "threeoldstyle",
    "fouroldstyle",
    "fiveoldstyle",
    "sixoldstyle",
    "sevenoldstyle",
    "eightoldstyle",
    "nineoldstyle",
    "commasuperior",
    "threequartersemdash",
    "periodsuperior",
    "questionsmall",
    "asuperior",
    "bsuperior",
    "centsuperior",
    "dsuperior",
    "esuperior",
    "isuperior",
    "lsuperior",
    "msuperior",
    "nsuperior",
    "osuperior",
    "rsuperior",
    "ssuperior",
    "tsuperior",
    "ff",
    "ffi",
    "ffl",
    "parenleftinferior",
    "parenrightinferior",
    "Circumflexsmall",
    "hyphensuperior",
    "Gravesmall",
    "Asmall",
    "Bsmall",
    "Csmall",
    "Dsmall",
    "Esmall",
    "Fsmall",
    "Gsmall",
    "Hsmall",
    "Ismall",
    "Jsmall",
    "Ksmall",
    "Lsmall",
    "Msmall",
    "Nsmall",
    "Osmall",
    "Psmall",
    "Qsmall",
    "Rsmall",
    "Ssmall",
    "Tsmall",
    "Usmall",
    "Vsmall",
    "Wsmall",
    "Xsmall",
    "Ysmall",
    "Zsmall",
    "colonmonetary",
    "onefitted",
    "rupiah",
    "Tildesmall",
    "exclamdownsmall",
    "centoldstyle",
    "Lslashsmall",
    "Scaronsmall",
    "Zcaronsmall",
    "Dieresissmall",
    "Brevesmall",
    "Caronsmall",
    "Dotaccentsmall",
    "Macronsmall",
    "figuredash",
    "hypheninferior",
    "Ogoneksmall",
    "Ringsmall",
    "Cedillasmall",
    "questiondownsmall",
    "oneeighth",
    "threeeighths",
    "fiveeighths",
    "seveneighths",
    "onethird",
    "twothirds",
    "zerosuperior",
    "foursuperior",
    "fivesuperior",
    "sixsuperior",
    "sevensuperior",
    "eightsuperior",
    "ninesuperior",
    "zeroinferior",
    "oneinferior",
    "twoinferior",
    "threeinferior",
    "fourinferior",
    "fiveinferior",
    "sixinferior",
    "seveninferior",
    "eightinferior",
    "nineinferior",
    "centinferior",
    "dollarinferior",
    "periodinferior",
    "commainferior",
    "Agravesmall",
    "Aacutesmall",
    "Acircumflexsmall",
    "Atildesmall",
    "Adieresissmall",
    "Aringsmall",
    "AEsmall",
    "Ccedillasmall",
    "Egravesmall",
    "Eacutesmall",
    "Ecircumflexsmall",
    "Edieresissmall",
    "Igravesmall",
    "Iacutesmall",
    "Icircumflexsmall",
    "Idieresissmall",
    "Ethsmall",
    "Ntildesmall",
    "Ogravesmall",
    "Oacutesmall",
    "Ocircumflexsmall",
    "Otildesmall",
    "Odieresissmall",
    "OEsmall",
    "Oslashsmall",
    "Ugravesmall",
    "Uacutesmall",
    "Ucircumflexsmall",
    "Udieresissmall",
    "Yacutesmall",
    "Thornsmall",
    "Ydieresissmall",
    "001.000",
    "001.001",
    "001.002",
    "001.003",
    "Black",
    "Bold",
    "Book",
    "Light",
    "Medium",
    "Regular",
    "Roman",
    "Semibold",
];

/// The Standard encoding, as `(code, SID)` pairs (CFF specification,
/// Appendix B).
///
/// Codes 32 to 126 are the printable ASCII names in order, which is why that
/// half is arithmetic; the high range is contiguous in SIDs and full of holes
/// in codes, so it is written out.
const STANDARD_ENCODING: &[(u8, u16)] = &[
    (32, 1),
    (33, 2),
    (34, 3),
    (35, 4),
    (36, 5),
    (37, 6),
    (38, 7),
    (39, 8),
    (40, 9),
    (41, 10),
    (42, 11),
    (43, 12),
    (44, 13),
    (45, 14),
    (46, 15),
    (47, 16),
    (48, 17),
    (49, 18),
    (50, 19),
    (51, 20),
    (52, 21),
    (53, 22),
    (54, 23),
    (55, 24),
    (56, 25),
    (57, 26),
    (58, 27),
    (59, 28),
    (60, 29),
    (61, 30),
    (62, 31),
    (63, 32),
    (64, 33),
    (65, 34),
    (66, 35),
    (67, 36),
    (68, 37),
    (69, 38),
    (70, 39),
    (71, 40),
    (72, 41),
    (73, 42),
    (74, 43),
    (75, 44),
    (76, 45),
    (77, 46),
    (78, 47),
    (79, 48),
    (80, 49),
    (81, 50),
    (82, 51),
    (83, 52),
    (84, 53),
    (85, 54),
    (86, 55),
    (87, 56),
    (88, 57),
    (89, 58),
    (90, 59),
    (91, 60),
    (92, 61),
    (93, 62),
    (94, 63),
    (95, 64),
    (96, 65),
    (97, 66),
    (98, 67),
    (99, 68),
    (100, 69),
    (101, 70),
    (102, 71),
    (103, 72),
    (104, 73),
    (105, 74),
    (106, 75),
    (107, 76),
    (108, 77),
    (109, 78),
    (110, 79),
    (111, 80),
    (112, 81),
    (113, 82),
    (114, 83),
    (115, 84),
    (116, 85),
    (117, 86),
    (118, 87),
    (119, 88),
    (120, 89),
    (121, 90),
    (122, 91),
    (123, 92),
    (124, 93),
    (125, 94),
    (126, 95),
    (161, 96),
    (162, 97),
    (163, 98),
    (164, 99),
    (165, 100),
    (166, 101),
    (167, 102),
    (168, 103),
    (169, 104),
    (170, 105),
    (171, 106),
    (172, 107),
    (173, 108),
    (174, 109),
    (175, 110),
    (177, 111),
    (178, 112),
    (179, 113),
    (180, 114),
    (182, 115),
    (183, 116),
    (184, 117),
    (185, 118),
    (186, 119),
    (187, 120),
    (188, 121),
    (189, 122),
    (191, 123),
    (193, 124),
    (194, 125),
    (195, 126),
    (196, 127),
    (197, 128),
    (198, 129),
    (199, 130),
    (200, 131),
    (202, 132),
    (203, 133),
    (205, 134),
    (206, 135),
    (207, 136),
    (208, 137),
    (225, 138),
    (227, 139),
    (232, 140),
    (233, 141),
    (234, 142),
    (235, 143),
    (241, 144),
    (245, 145),
    (248, 146),
    (249, 147),
    (250, 148),
    (251, 149),
];

/// The Expert encoding, as `(code, SID)` pairs (CFF specification,
/// Appendix B).
///
/// Its SID column, in this order, is also the Expert charset: the expert
/// character set is laid out in code order. `the_expert_tables_agree` checks
/// that, which is the only cross-check available for data no font in this
/// repository uses.
const EXPERT_ENCODING: &[(u8, u16)] = &[
    (32, 1),
    (33, 229),
    (34, 230),
    (36, 231),
    (37, 232),
    (38, 233),
    (39, 234),
    (40, 235),
    (41, 236),
    (42, 237),
    (43, 238),
    (44, 13),
    (45, 14),
    (46, 15),
    (47, 99),
    (48, 239),
    (49, 240),
    (50, 241),
    (51, 242),
    (52, 243),
    (53, 244),
    (54, 245),
    (55, 246),
    (56, 247),
    (57, 248),
    (58, 27),
    (59, 28),
    (60, 249),
    (61, 250),
    (62, 251),
    (63, 252),
    (65, 253),
    (66, 254),
    (67, 255),
    (68, 256),
    (69, 257),
    (73, 258),
    (76, 259),
    (77, 260),
    (78, 261),
    (79, 262),
    (82, 263),
    (83, 264),
    (84, 265),
    (86, 266),
    (87, 109),
    (88, 110),
    (89, 267),
    (90, 268),
    (91, 269),
    (93, 270),
    (94, 271),
    (95, 272),
    (96, 273),
    (97, 274),
    (98, 275),
    (99, 276),
    (100, 277),
    (101, 278),
    (102, 279),
    (103, 280),
    (104, 281),
    (105, 282),
    (106, 283),
    (107, 284),
    (108, 285),
    (109, 286),
    (110, 287),
    (111, 288),
    (112, 289),
    (113, 290),
    (114, 291),
    (115, 292),
    (116, 293),
    (117, 294),
    (118, 295),
    (119, 296),
    (120, 297),
    (121, 298),
    (122, 299),
    (123, 300),
    (124, 301),
    (125, 302),
    (126, 303),
    (161, 304),
    (162, 305),
    (163, 306),
    (166, 307),
    (167, 308),
    (168, 309),
    (169, 310),
    (170, 311),
    (172, 312),
    (175, 313),
    (178, 314),
    (179, 315),
    (182, 316),
    (183, 317),
    (184, 318),
    (188, 158),
    (189, 155),
    (190, 163),
    (191, 319),
    (192, 320),
    (193, 321),
    (194, 322),
    (195, 323),
    (196, 324),
    (197, 325),
    (200, 326),
    (201, 150),
    (202, 164),
    (203, 169),
    (204, 327),
    (205, 328),
    (206, 329),
    (207, 330),
    (208, 331),
    (209, 332),
    (210, 333),
    (211, 334),
    (212, 335),
    (213, 336),
    (214, 337),
    (215, 338),
    (216, 339),
    (217, 340),
    (218, 341),
    (219, 342),
    (220, 343),
    (221, 344),
    (222, 345),
    (223, 346),
    (224, 347),
    (225, 348),
    (226, 349),
    (227, 350),
    (228, 351),
    (229, 352),
    (230, 353),
    (231, 354),
    (232, 355),
    (233, 356),
    (234, 357),
    (235, 358),
    (236, 359),
    (237, 360),
    (238, 361),
    (239, 362),
    (240, 363),
    (241, 364),
    (242, 365),
    (243, 366),
    (244, 367),
    (245, 368),
    (246, 369),
    (247, 370),
    (248, 371),
    (249, 372),
    (250, 373),
    (251, 374),
    (252, 375),
    (253, 376),
    (254, 377),
    (255, 378),
];

/// The Expert charset: the SID each glyph carries, from glyph 1 upward (CFF
/// specification, Appendix C).
const EXPERT_CHARSET: &[u16] = &[
    1, 229, 230, 231, 232, 233, 234, 235, 236, 237, 238, 13, 14, 15, 99, 239, 240, 241, 242, 243,
    244, 245, 246, 247, 248, 27, 28, 249, 250, 251, 252, 253, 254, 255, 256, 257, 258, 259, 260,
    261, 262, 263, 264, 265, 266, 109, 110, 267, 268, 269, 270, 271, 272, 273, 274, 275, 276, 277,
    278, 279, 280, 281, 282, 283, 284, 285, 286, 287, 288, 289, 290, 291, 292, 293, 294, 295, 296,
    297, 298, 299, 300, 301, 302, 303, 304, 305, 306, 307, 308, 309, 310, 311, 312, 313, 314, 315,
    316, 317, 318, 158, 155, 163, 319, 320, 321, 322, 323, 324, 325, 326, 150, 164, 169, 327, 328,
    329, 330, 331, 332, 333, 334, 335, 336, 337, 338, 339, 340, 341, 342, 343, 344, 345, 346, 347,
    348, 349, 350, 351, 352, 353, 354, 355, 356, 357, 358, 359, 360, 361, 362, 363, 364, 365, 366,
    367, 368, 369, 370, 371, 372, 373, 374, 375, 376, 377, 378,
];

/// The ExpertSubset charset, which is the Expert set with the glyphs an
/// expert *subset* face does not carry removed (CFF specification, Appendix
/// C).
const EXPERT_SUBSET_CHARSET: &[u16] = &[
    1, 231, 232, 235, 236, 237, 238, 13, 14, 15, 99, 239, 240, 241, 242, 243, 244, 245, 246, 247,
    248, 27, 28, 249, 250, 251, 253, 254, 255, 256, 257, 258, 259, 260, 261, 262, 263, 264, 265,
    266, 109, 110, 267, 268, 269, 270, 272, 300, 301, 302, 305, 314, 315, 158, 155, 163, 320, 321,
    322, 323, 324, 325, 326, 150, 164, 169, 327, 328, 329, 330, 331, 332, 333, 334, 335, 336, 337,
    338, 339, 340, 341, 342, 343, 344, 345, 346,
];

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
    /// The Private DICT this glyph runs under, which in a CID-keyed font is
    /// its Font DICT's rather than the top-level one.
    private: &'b Private<'a>,
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
            self.width = Some(self.private.nominal_width + extra);
        } else {
            self.width = Some(self.private.default_width);
        }
    }

    fn take_width_for(&mut self, expected: usize) {
        if self.width.is_some() {
            return;
        }
        if self.stack.len() > expected && !self.stack.is_empty() {
            let extra = self.stack.remove(0);
            self.width = Some(self.private.nominal_width + extra);
        } else {
            self.width = Some(self.private.default_width);
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
                    let biased = index as i32 + bias(self.private.local_subrs.len());
                    if let Ok(index) = usize::try_from(biased) {
                        if let Some(code) = self.private.local_subrs.get(index) {
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

    /// Wraps `items` as a CFF INDEX with four-byte offsets.
    ///
    /// Wide offsets throughout, so a fixture that grows past 255 bytes of
    /// charstrings does not silently change format halfway through a test.
    fn wide_index(items: &[Vec<u8>]) -> Vec<u8> {
        if items.is_empty() {
            return vec![0, 0];
        }
        let mut out = (items.len() as u16).to_be_bytes().to_vec();
        out.push(4);
        let mut offset = 1u32;
        out.extend_from_slice(&offset.to_be_bytes());
        for item in items {
            offset += item.len() as u32;
            out.extend_from_slice(&offset.to_be_bytes());
        }
        for item in items {
            out.extend_from_slice(item);
        }
        out
    }

    /// One DICT entry: operands in the fixed five-byte integer form, then the
    /// operator. Fixed width is what lets a fixture compute its own offsets.
    fn entry(op: u16, operands: &[i32]) -> Vec<u8> {
        let mut out = Vec::new();
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
        out
    }

    /// A charstring drawing a box `size` units on a side, at the origin.
    ///
    /// Every operand takes the three-byte 16-bit form, so the shape can be
    /// read off the bytes and the width prefix is unambiguous — a box is
    /// enough to tell one glyph from another by the ink it puts down.
    fn box_glyph(size: i16) -> Vec<u8> {
        let n = |v: i16| {
            let mut out = vec![28u8];
            out.extend_from_slice(&v.to_be_bytes());
            out
        };
        let mut out = Vec::new();
        out.extend(n(0));
        out.extend(n(0));
        out.push(21); // rmoveto
        for (dx, dy) in [(size, 0), (0, size), (-size, 0)] {
            out.extend(n(dx));
            out.extend(n(dy));
            out.push(5); // rlineto
        }
        out.push(14); // endchar
        out
    }

    /// A charstring that draws nothing itself and calls local subroutine 0.
    ///
    /// Subroutine numbers are biased, and a font with fewer than 1240 of them
    /// biases by 107 — so subroutine 0 is called with the operand -107, which
    /// is the single byte 32.
    fn subr_glyph() -> Vec<u8> {
        vec![32, 10, 14]
    }

    /// Where each table landed, so the Top DICT can point at it.
    #[derive(Default)]
    struct Offsets {
        charset: i32,
        encoding: i32,
        charstrings: i32,
        private: (i32, i32),
        fd_array: i32,
        fd_select: i32,
    }

    /// A charset or encoding: a predefined number, or a table in the file.
    enum Table {
        Predefined(i32),
        Bytes(Vec<u8>),
    }

    impl Default for Table {
        fn default() -> Table {
            Table::Predefined(0)
        }
    }

    /// A CFF font program assembled from its parts.
    ///
    /// Hand-built rather than borrowed from a real face: `testdata/` carries
    /// four PDFs and none embeds a CFF, and a fixture whose every byte is
    /// known is what makes "the right glyph" checkable by arithmetic rather
    /// than by eye.
    #[derive(Default)]
    struct Fixture {
        /// Strings beyond the standard 391, numbered from SID 391 upward.
        strings: Vec<Vec<u8>>,
        charstrings: Vec<Vec<u8>>,
        charset: Table,
        encoding: Table,
        private: Vec<u8>,
        subrs: Vec<Vec<u8>>,
        /// `ROS`, which is what makes the font CID-keyed.
        cid: bool,
        /// The FDArray, one entry per Font DICT.
        fds: Vec<FontDict>,
        /// The FDSelect table, already encoded.
        fd_select: Vec<u8>,
        /// A Top DICT `FontMatrix`, for the CID-keyed matrix rules.
        font_matrix: Option<[i32; 6]>,
    }

    impl Fixture {
        fn top_dict(&self, at: &Offsets) -> Vec<u8> {
            let mut out = Vec::new();
            if self.cid {
                // The registry and ordering SIDs are the first two custom
                // strings; the supplement is zero.
                out.extend(entry(0x0C1E, &[391, 392, 0]));
            }
            if let Some(m) = self.font_matrix {
                out.extend(entry(0x0C07, &m));
            }
            out.extend(entry(15, &[at.charset]));
            if !self.cid {
                out.extend(entry(16, &[at.encoding]));
            }
            out.extend(entry(17, &[at.charstrings]));
            out.extend(entry(18, &[at.private.0, at.private.1]));
            if !self.fds.is_empty() {
                out.extend(entry(0x0C24, &[at.fd_array]));
                out.extend(entry(0x0C25, &[at.fd_select]));
            }
            out
        }

        fn build(&self) -> Vec<u8> {
            let header = [1u8, 0, 4, 4];
            let names = wide_index(&[b"Fixture".to_vec()]);
            let strings = wide_index(&self.strings);
            let gsubrs = wide_index(&[]);
            let charstrings = wide_index(&self.charstrings);

            let table = |t: &Table| match t {
                Table::Predefined(_) => Vec::new(),
                Table::Bytes(bytes) => bytes.clone(),
            };
            let charset = table(&self.charset);
            let encoding = table(&self.encoding);

            // The Top DICT's length cannot change when the offsets in it do,
            // so one pass with zeroes measures it and the second fills it in.
            let top_len = self.top_dict(&Offsets::default()).len();
            let mut at = Offsets {
                charset: 0,
                encoding: 0,
                charstrings: 0,
                private: (self.private.len() as i32, 0),
                fd_array: 0,
                fd_select: 0,
            };
            let mut cursor =
                header.len() + names.len() + (2 + 1 + 8 + top_len) + strings.len() + gsubrs.len();

            let charset_at = cursor;
            at.charset = match self.charset {
                Table::Predefined(n) => n,
                Table::Bytes(_) => cursor as i32,
            };
            cursor += charset.len();

            let encoding_at = cursor;
            at.encoding = match self.encoding {
                Table::Predefined(n) => n,
                Table::Bytes(_) => cursor as i32,
            };
            cursor += encoding.len();

            let fd_select_at = cursor;
            at.fd_select = cursor as i32;
            cursor += self.fd_select.len();

            let charstrings_at = cursor;
            at.charstrings = cursor as i32;
            cursor += charstrings.len();

            let private_at = cursor;
            at.private.1 = cursor as i32;
            cursor += self.private.len();

            let subrs = wide_index(&self.subrs);
            let subrs_at = cursor;
            cursor += subrs.len();

            // Each Font DICT's Private DICT is laid down before the FDArray
            // that points at it, and its Subrs offset is measured from the
            // Private DICT rather than from the file.
            let mut fd_dicts: Vec<Vec<u8>> = Vec::new();
            let mut fd_bodies: Vec<(usize, Vec<u8>)> = Vec::new();
            for fd in &self.fds {
                let (private, fd_subrs, matrix) = (&fd.private, &fd.subrs, &fd.matrix);
                let mut body = private.clone();
                if !fd_subrs.is_empty() {
                    body.extend(entry(19, &[private.len() as i32 + 6]));
                    // The Subrs operand above is five bytes plus its operator,
                    // so the INDEX begins six bytes past the DICT it is in.
                    assert_eq!(body.len(), private.len() + 6);
                    body.extend(wide_index(fd_subrs));
                }
                // The DICT's declared size covers the Subrs entry too, or the
                // subroutines are simply not read.
                let dict_len = if fd_subrs.is_empty() {
                    private.len()
                } else {
                    private.len() + 6
                };
                let mut dict = Vec::new();
                if let Some(m) = matrix {
                    dict.extend(entry(0x0C07, m));
                }
                dict.extend(entry(18, &[dict_len as i32, cursor as i32]));
                fd_dicts.push(dict);
                fd_bodies.push((cursor, body.clone()));
                cursor += body.len();
            }
            at.fd_array = cursor as i32;

            let mut out = header.to_vec();
            out.extend_from_slice(&names);
            out.extend_from_slice(&wide_index(&[self.top_dict(&at)]));
            out.extend_from_slice(&strings);
            out.extend_from_slice(&gsubrs);
            assert_eq!(out.len(), charset_at, "the charset lands where it was put");
            out.extend_from_slice(&charset);
            assert_eq!(out.len(), encoding_at);
            out.extend_from_slice(&encoding);
            assert_eq!(out.len(), fd_select_at);
            out.extend_from_slice(&self.fd_select);
            assert_eq!(out.len(), charstrings_at);
            out.extend_from_slice(&charstrings);
            assert_eq!(out.len(), private_at);
            out.extend_from_slice(&self.private);
            assert_eq!(out.len(), subrs_at);
            out.extend_from_slice(&subrs);
            for (start, body) in &fd_bodies {
                assert_eq!(out.len(), *start);
                out.extend_from_slice(body);
            }
            if !self.fds.is_empty() {
                assert_eq!(out.len(), at.fd_array as usize);
                out.extend_from_slice(&wide_index(&fd_dicts));
            }
            out
        }
    }

    /// One member of a CID-keyed font's FDArray.
    struct FontDict {
        /// The Private DICT this Font DICT points at.
        private: Vec<u8>,
        /// Its local subroutines, which are what make the choice of Font DICT
        /// visible in the outline a glyph draws.
        subrs: Vec<Vec<u8>>,
        /// A font matrix of its own, which a CID-keyed font may carry here
        /// instead of in the Top DICT.
        matrix: Option<[i32; 6]>,
    }

    /// A Private DICT declaring both widths, and Subrs when `subrs` is set.
    fn private_dict(default_width: i32, nominal_width: i32, subrs: bool) -> Vec<u8> {
        let mut out = entry(20, &[default_width]);
        out.extend(entry(21, &[nominal_width]));
        if subrs {
            // The INDEX follows the DICT immediately, six bytes on.
            out.extend(entry(19, &[out.len() as i32 + 6]));
        }
        out
    }

    /// A charset in format 0: one SID per glyph after `.notdef`.
    fn charset_0(sids: &[u16]) -> Vec<u8> {
        let mut out = vec![0u8];
        for sid in sids {
            out.extend_from_slice(&sid.to_be_bytes());
        }
        out
    }

    /// A charset in format 1 or 2: runs of consecutive SIDs, counted in one
    /// byte or in two.
    fn charset_runs(format: u8, runs: &[(u16, u16)]) -> Vec<u8> {
        let mut out = vec![format];
        for (first, left) in runs {
            out.extend_from_slice(&first.to_be_bytes());
            if format == 2 {
                out.extend_from_slice(&left.to_be_bytes());
            } else {
                out.push(*left as u8);
            }
        }
        out
    }

    /// Wraps `items` as a CFF INDEX with one-byte offsets.
    fn index(items: &[&[u8]]) -> Vec<u8> {
        if items.is_empty() {
            return vec![0, 0];
        }
        let mut out = (items.len() as u16).to_be_bytes().to_vec();
        out.push(1);
        let mut offset = 1u8;
        out.push(offset);
        for item in items {
            offset += item.len() as u8;
            out.push(offset);
        }
        for item in items {
            out.extend_from_slice(item);
        }
        out
    }

    /// A whole CFF font program: three glyphs, a local subroutine, and a
    /// Private DICT carrying both widths.
    ///
    /// This is `fuzz/corpus/cff/three_glyphs.cff`, built here so the seed and
    /// the test cannot drift apart.
    fn three_glyph_program() -> Vec<u8> {
        // Glyph 0 is .notdef; glyph 1 carries a width prefix and two lines;
        // glyph 2 declares a stem, masks it, and calls the local subroutine.
        let charstrings = index(&[
            &[14][..],
            &[250, 100, 189, 189, 21, 239, 139, 5, 139, 239, 5, 14][..],
            &[189, 217, 1, 189, 189, 21, 19, 0x80, 32, 10, 14][..],
        ]);
        let subrs = index(&[&[239, 139, 5, 11][..]]);
        // defaultWidthX, nominalWidthX, then Subrs at the byte after this
        // DICT — fourteen bytes on, so the operand is 14 + 139.
        let private = vec![29, 0, 0, 1, 244, 20, 29, 0, 0, 1, 144, 21, 153, 19];

        let header = [1u8, 0, 4, 1];
        let names = index(&[b"TinkerSeed".as_slice()]);
        // Every Top DICT operand takes the fixed five-byte form, so the
        // offsets it holds do not depend on their own encoded width.
        let top_len = 6 + 11;
        let private_at = header.len() + names.len() + (2 + 1 + 2 + top_len) + 2 + 2;
        let charstrings_at = private_at + private.len() + subrs.len();

        let mut top = Vec::new();
        top.push(29);
        top.extend_from_slice(&(charstrings_at as u32).to_be_bytes());
        top.push(17);
        top.push(29);
        top.extend_from_slice(&(private.len() as u32).to_be_bytes());
        top.push(29);
        top.extend_from_slice(&(private_at as u32).to_be_bytes());
        top.push(18);
        assert_eq!(top.len(), top_len);

        let mut out = header.to_vec();
        out.extend_from_slice(&names);
        out.extend_from_slice(&index(&[top.as_slice()]));
        out.extend_from_slice(&index(&[])); // strings
        out.extend_from_slice(&index(&[])); // global subrs
        assert_eq!(out.len(), private_at);
        out.extend_from_slice(&private);
        out.extend_from_slice(&subrs);
        assert_eq!(out.len(), charstrings_at);
        out.extend_from_slice(&charstrings);
        out
    }

    /// The minimised reproducer for the INDEX shift: one item, `abc`, whose
    /// offsets are the 1 and 4 every CFF writer emits. Reading it as
    /// `[4, b'a', b'b']` is the whole bug in eight bytes.
    #[test]
    fn an_index_item_starts_after_the_offset_array() {
        let raw = [0u8, 1, 1, 1, 4, b'a', b'b', b'c'];
        let (index, next) = Index::parse(&raw, 0).expect("an INDEX");
        assert_eq!(index.len(), 1);
        assert_eq!(index.get(0), Some(b"abc".as_slice()));
        assert_eq!(next, raw.len(), "and the next structure begins after it");
    }

    /// Nothing in this crate had ever parsed a whole CFF program — the tests
    /// covered DICT operands, subroutine bias and refusal of garbage, all of
    /// which pass with the INDEX reader off by a byte. So embedded CFF and
    /// OpenType/CFF faces resolved no glyph at all, and only a font that got
    /// as far as a charstring could show it.
    #[test]
    fn a_whole_font_program_parses_and_outlines_its_glyphs() {
        let program = three_glyph_program();
        let cff = Cff::parse(&program).expect("a CFF font");
        assert_eq!(cff.glyph_count(), 3);

        assert!(
            cff.outline(0).expect("notdef answers").is_empty(),
            "glyph 0 is .notdef and draws nothing"
        );
        assert!(
            !cff.outline(1).expect("glyph 1").is_empty(),
            "glyph 1 draws two lines from a width-prefixed rmoveto"
        );
        assert!(
            !cff.outline(2).expect("glyph 2").is_empty(),
            "glyph 2 reaches its line through hintmask and callsubr"
        );

        // The width prefix on glyph 1 is nominalWidthX plus the operand.
        assert_eq!(cff.advance(1), Some(1376.0));
        // Glyph 2 clears its stack without one, so it takes defaultWidthX.
        assert_eq!(cff.advance(2), Some(500.0));
    }

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

    /// The SIDs of `A`, `B` and `C`, which are the standard strings 34 to 36.
    const ABC: [u16; 3] = [34, 35, 36];

    /// Writes the three seeds `fuzz/corpus/cff/` carries for the glyph
    /// selection tables, so the seeds and the fixtures cannot drift apart.
    ///
    /// Run with `--ignored` when a fixture changes; the corpus is committed,
    /// and a run that rewrites it is a diff to look at rather than to apply
    /// blindly.
    #[test]
    #[ignore = "writes into fuzz/corpus/cff, which is committed"]
    fn write_the_fuzz_seeds() {
        let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fuzz/corpus/cff");
        for (name, bytes) in [
            ("charset_and_encoding.cff", named_glyphs().build()),
            ("charset_runs.cff", run_charset_glyphs().build()),
            (
                "cid_keyed.cff",
                cid_fixture(vec![3, 0, 2, 0, 0, 0, 0, 2, 1, 0, 4]).build(),
            ),
        ] {
            std::fs::write(base.join(name), bytes).expect("the corpus directory is there");
        }
    }

    /// `fuzz/corpus/cff/charset_and_encoding.cff`: a format 0 charset, a
    /// custom encoding with a supplement, and a name from the string INDEX.
    fn named_glyphs() -> Fixture {
        let mut encoding = vec![0x80, 3, 0x41, 0x42, 0x43, 1, 0x61];
        encoding.extend_from_slice(&34u16.to_be_bytes());
        Fixture {
            strings: vec![b"uniE000".to_vec()],
            charset: Table::Bytes(charset_0(&[34, 35, 391])),
            encoding: Table::Bytes(encoding),
            ..abc(Table::default())
        }
    }

    /// `fuzz/corpus/cff/charset_runs.cff`: the same font with a format 2
    /// charset, which is the format whose run lengths are two bytes wide.
    fn run_charset_glyphs() -> Fixture {
        Fixture {
            charset: Table::Bytes(charset_runs(2, &[(34, 2)])),
            ..abc(Table::default())
        }
    }

    /// A three-letter font: `.notdef`, then `A`, `B` and `C` as boxes of
    /// three different sizes, so which glyph ran can be read off the outline.
    fn abc(charset: Table) -> Fixture {
        Fixture {
            charstrings: vec![vec![14], box_glyph(600), box_glyph(500), box_glyph(400)],
            charset,
            private: private_dict(600, 600, false),
            ..Fixture::default()
        }
    }

    /// The width of the box a glyph draws, which names it as surely as its
    /// SID does and is what a renderer would actually put on the page.
    fn box_width(cff: &Cff<'_>, glyph: u16) -> Option<f64> {
        let outline = cff.outline(glyph)?;
        let (x0, _, x1, _) = outline.bounds()?;
        Some(x1 - x0)
    }

    #[test]
    fn a_charset_gives_every_glyph_its_name() {
        let font = abc(Table::Bytes(charset_0(&ABC))).build();
        let cff = Cff::parse(&font).expect("a CFF font");

        assert_eq!(cff.glyph_name(0), Some(".notdef"));
        assert_eq!(cff.glyph_name(1), Some("A"));
        assert_eq!(cff.glyph_name(2), Some("B"));
        assert_eq!(cff.glyph_name(3), Some("C"));

        assert_eq!(cff.gid_for_name("A"), Some(1));
        assert_eq!(cff.gid_for_name("B"), Some(2));
        assert_eq!(cff.gid_for_name("C"), Some(3));
        assert_eq!(cff.gid_for_name("D"), None, "a name the font does not have");

        // And the glyph that comes back is the one that draws that letter.
        assert_eq!(box_width(&cff, 1), Some(600.0));
        assert_eq!(box_width(&cff, 2), Some(500.0));
    }

    /// Format 1 counts a run in one byte and format 2 in two. Reading one as
    /// the other does not fail — it reads a plausible length and shifts every
    /// glyph after the first run, so the font draws the wrong letters and
    /// nothing anywhere says so. The three formats describing one font is the
    /// only test that separates them.
    #[test]
    fn the_three_charset_formats_describe_the_same_font() {
        let formats = [
            Table::Bytes(charset_0(&ABC)),
            // One run: `A`, and two more after it.
            Table::Bytes(charset_runs(1, &[(34, 2)])),
            Table::Bytes(charset_runs(2, &[(34, 2)])),
        ];

        for (which, charset) in formats.into_iter().enumerate() {
            let font = abc(charset).build();
            let cff = Cff::parse(&font).unwrap_or_else(|| panic!("format {which} parses"));
            assert_eq!(cff.gid_for_name("A"), Some(1), "format {which}");
            assert_eq!(cff.gid_for_name("B"), Some(2), "format {which}");
            assert_eq!(cff.gid_for_name("C"), Some(3), "format {which}");
            assert_eq!(box_width(&cff, 3), Some(400.0), "format {which}");
        }

        // A format 2 table read as format 1 would take the high byte of the
        // run length as the length itself, which is zero here.
        let confused = Fixture {
            charset: Table::Bytes(charset_runs(1, &[(34, 0), (35, 0), (36, 0)])),
            ..abc(Table::default())
        }
        .build();
        let cff = Cff::parse(&confused).expect("three one-glyph runs parse");
        assert_eq!(
            cff.gid_for_name("C"),
            Some(3),
            "runs of one are the same map"
        );
    }

    #[test]
    fn a_name_outside_the_standard_strings_comes_from_the_string_index() {
        let font = Fixture {
            strings: vec![b"uniE000".to_vec(), b"threedotleader".to_vec()],
            // SID 391 is the first custom string.
            charset: Table::Bytes(charset_0(&[391, 34, 392])),
            ..abc(Table::default())
        }
        .build();
        let cff = Cff::parse(&font).expect("a CFF font");

        assert_eq!(cff.glyph_name(1), Some("uniE000"));
        assert_eq!(cff.gid_for_name("uniE000"), Some(1));
        assert_eq!(
            cff.gid_for_name("A"),
            Some(2),
            "a standard string beside it"
        );
        assert_eq!(cff.gid_for_name("threedotleader"), Some(3));
    }

    #[test]
    fn the_predefined_charsets_resolve() {
        // ISOAdobe is the standard strings in order, so glyph 34 is `A`.
        let mut charstrings = vec![vec![14u8]];
        charstrings.resize(40, box_glyph(300));
        let font = Fixture {
            charstrings: charstrings.clone(),
            charset: Table::Predefined(0),
            private: private_dict(600, 600, false),
            ..Fixture::default()
        }
        .build();
        let cff = Cff::parse(&font).expect("an ISOAdobe font");
        assert_eq!(cff.glyph_name(34), Some("A"));
        assert_eq!(cff.gid_for_name("A"), Some(34));
        assert_eq!(cff.gid_for_name("space"), Some(1));

        // Expert and ExpertSubset are their own orders, and both begin with
        // the space.
        let expert = Fixture {
            charstrings: charstrings.clone(),
            charset: Table::Predefined(1),
            private: private_dict(600, 600, false),
            ..Fixture::default()
        }
        .build();
        let cff = Cff::parse(&expert).expect("an Expert font");
        assert_eq!(cff.glyph_name(1), Some("space"));
        assert_eq!(cff.glyph_name(2), Some("exclamsmall"));
        assert_eq!(cff.gid_for_name("A"), None, "the expert set has no `A`");

        let subset = Fixture {
            charstrings,
            charset: Table::Predefined(2),
            private: private_dict(600, 600, false),
            ..Fixture::default()
        }
        .build();
        let cff = Cff::parse(&subset).expect("an ExpertSubset font");
        assert_eq!(cff.glyph_name(1), Some("space"));
        assert_eq!(cff.glyph_name(2), Some("dollaroldstyle"));
    }

    #[test]
    fn the_standard_strings_are_the_specification_s() {
        assert_eq!(STANDARD_STRINGS.len(), 391);
        assert_eq!(STANDARD_STRINGS[0], ".notdef");
        assert_eq!(STANDARD_STRINGS[1], "space");
        // 1 to 95 are the printable ASCII names in code order, which is what
        // makes the Standard encoding's first ninety-five entries arithmetic.
        assert_eq!(STANDARD_STRINGS[34], "A");
        assert_eq!(STANDARD_STRINGS[95], "asciitilde");
        assert_eq!(STANDARD_STRINGS[96], "exclamdown");
        assert_eq!(STANDARD_STRINGS[390], "Semibold");
    }

    #[test]
    fn the_expert_tables_agree() {
        // The expert character set is laid out in code order, so the encoding
        // and the charset are one table read two ways. Nothing in this
        // repository uses either, and this is the cross-check that stands in
        // for a font that would.
        assert_eq!(EXPERT_ENCODING.len(), EXPERT_CHARSET.len());
        for (i, (_, sid)) in EXPERT_ENCODING.iter().enumerate() {
            assert_eq!(Some(sid), EXPERT_CHARSET.get(i), "entry {i}");
        }
        let mut codes = EXPERT_ENCODING.iter().map(|(c, _)| *c);
        let mut previous = codes.next().expect("a first code");
        for code in codes {
            assert!(code > previous, "codes ascend: {code} after {previous}");
            previous = code;
        }
        // The subset is a subset.
        for sid in EXPERT_SUBSET_CHARSET {
            assert!(
                EXPERT_CHARSET.contains(sid),
                "SID {sid} is in the expert set"
            );
        }
        // Every SID in both tables names a standard string.
        for sid in EXPERT_CHARSET.iter().chain(EXPERT_SUBSET_CHARSET) {
            assert!(usize::from(*sid) < STANDARD_STRINGS.len());
        }
        // The Standard encoding's ASCII half is the arithmetic it claims.
        assert_eq!(
            STANDARD_ENCODING.iter().find(|(c, _)| *c == 65),
            Some(&(65, 34))
        );
        assert_eq!(STANDARD_ENCODING.len(), 95 + 54);
    }

    #[test]
    fn a_built_in_encoding_maps_codes_to_glyphs() {
        // Format 0: one code per glyph, glyph 1 upward.
        let font = Fixture {
            encoding: Table::Bytes(vec![0, 3, 0x41, 0x42, 0x43]),
            ..abc(Table::Bytes(charset_0(&ABC)))
        }
        .build();
        let cff = Cff::parse(&font).expect("a CFF font");
        assert_eq!(cff.gid_for_code(0x41), Some(1));
        assert_eq!(cff.gid_for_code(0x43), Some(3));
        assert_eq!(cff.gid_for_code(0x44), None);

        // Format 1: one run of three codes, glyphs assigned in order.
        let font = Fixture {
            encoding: Table::Bytes(vec![1, 1, 0x61, 2]),
            ..abc(Table::Bytes(charset_0(&ABC)))
        }
        .build();
        let cff = Cff::parse(&font).expect("a CFF font");
        assert_eq!(cff.gid_for_code(0x61), Some(1));
        assert_eq!(cff.gid_for_code(0x62), Some(2));
        assert_eq!(cff.gid_for_code(0x63), Some(3));
        assert_eq!(cff.gid_for_code(0x64), None);
    }

    #[test]
    fn a_supplement_gives_a_second_code_to_one_glyph() {
        // The high bit of the format byte says supplements follow, and a
        // supplement names its glyph by SID rather than by position.
        let mut encoding = vec![0x80, 1, 0x41];
        encoding.extend_from_slice(&[1, 0x61]);
        encoding.extend_from_slice(&34u16.to_be_bytes());
        let font = Fixture {
            encoding: Table::Bytes(encoding),
            ..abc(Table::Bytes(charset_0(&ABC)))
        }
        .build();
        let cff = Cff::parse(&font).expect("a CFF font");
        assert_eq!(cff.gid_for_code(0x41), Some(1), "the table itself");
        assert_eq!(cff.gid_for_code(0x61), Some(1), "and the supplement");
    }

    #[test]
    fn the_standard_encoding_is_what_a_font_without_one_uses() {
        let font = abc(Table::Bytes(charset_0(&ABC))).build();
        let cff = Cff::parse(&font).expect("a CFF font");
        // Encoding offset 0 is the Standard encoding, which the fixture
        // writes because a font that omits the operator means the same thing.
        assert_eq!(cff.gid_for_code(b'A'), Some(1));
        assert_eq!(cff.gid_for_code(b'C'), Some(3));
        assert_eq!(cff.gid_for_code(b'D'), None, "the font has no `D`");
        assert_eq!(cff.gid_for_code(0), None);
    }

    /// A CID-keyed font: `ROS`, a charset of CIDs, an FDArray and an
    /// FDSelect, in the two formats FDSelect has.
    fn cid_fixture(fd_select: Vec<u8>) -> Fixture {
        Fixture {
            strings: vec![b"Adobe".to_vec(), b"Identity".to_vec()],
            charstrings: vec![vec![14], subr_glyph(), subr_glyph(), subr_glyph()],
            // CIDs 10, 11 and 12, as one run.
            charset: Table::Bytes(charset_runs(2, &[(10, 2)])),
            private: private_dict(600, 600, false),
            cid: true,
            fds: vec![
                FontDict {
                    private: private_dict(600, 600, false),
                    subrs: vec![box_glyph(600)],
                    matrix: None,
                },
                FontDict {
                    private: private_dict(600, 600, false),
                    subrs: vec![box_glyph(200)],
                    matrix: None,
                },
            ],
            fd_select,
            ..Fixture::default()
        }
    }

    #[test]
    fn a_cid_keyed_font_reaches_its_glyphs_by_cid() {
        // FDSelect format 0: one byte per glyph.
        let font = cid_fixture(vec![0, 0, 0, 1, 1]).build();
        let cff = Cff::parse(&font).expect("a CID-keyed font");

        assert!(cff.is_cid());
        assert_eq!(cff.gid_for_cid(10), Some(1));
        assert_eq!(cff.gid_for_cid(12), Some(3));
        assert_eq!(cff.gid_for_cid(13), None, "a CID the font does not carry");
        assert_eq!(
            cff.gid_for_cid(0x1_0000),
            None,
            "a CID wider than the charset can express names no glyph"
        );
        assert_eq!(
            cff.gid_for_name("A"),
            None,
            "a CID-keyed font has CIDs, not names"
        );
        assert_eq!(cff.glyph_name(1), None);
    }

    /// Every glyph in a CID-keyed font calls local subroutine 0, and the two
    /// Font DICTs define a different subroutine for it. A parser that used
    /// the top-level Private DICT for all of them would draw one shape.
    #[test]
    fn fdselect_picks_the_local_subroutines() {
        for (which, fd_select) in [
            // Format 0: glyphs 0 and 1 on Font DICT 0, 2 and 3 on 1.
            vec![0u8, 0, 0, 1, 1],
            // Format 3: two ranges and the sentinel, saying the same thing.
            vec![3, 0, 2, 0, 0, 0, 0, 2, 1, 0, 4],
        ]
        .into_iter()
        .enumerate()
        {
            let font = cid_fixture(fd_select).build();
            let cff = Cff::parse(&font).unwrap_or_else(|| panic!("format {which}"));
            assert_eq!(box_width(&cff, 1), Some(600.0), "format {which}, glyph 1");
            assert_eq!(box_width(&cff, 2), Some(200.0), "format {which}, glyph 2");
            assert_eq!(box_width(&cff, 3), Some(200.0), "format {which}, glyph 3");
        }
    }

    #[test]
    fn a_font_dict_carries_its_own_matrix() {
        let mut fixture = cid_fixture(vec![0, 0, 0, 1, 1]);
        fixture.fds[1].matrix = Some([2, 0, 0, 2, 0, 0]);
        let font = fixture.build();
        let cff = Cff::parse(&font).expect("a CID-keyed font");

        // The Top DICT has no matrix, so the Font DICT's is the whole of it.
        assert_eq!(cff.font_matrix_for(1), [0.001, 0.0, 0.0, 0.001, 0.0, 0.0]);
        assert_eq!(cff.font_matrix_for(2), [2.0, 0.0, 0.0, 2.0, 0.0, 0.0]);

        // With both, the glyph's space passes through the Font DICT's matrix
        // and then the Top DICT's.
        let mut fixture = cid_fixture(vec![0, 0, 0, 1, 1]);
        fixture.fds[1].matrix = Some([2, 0, 0, 2, 0, 0]);
        fixture.font_matrix = Some([3, 0, 0, 3, 0, 0]);
        let font = fixture.build();
        let cff = Cff::parse(&font).expect("a CID-keyed font");
        assert_eq!(cff.font_matrix_for(1), [3.0, 0.0, 0.0, 3.0, 0.0, 0.0]);
        assert_eq!(cff.font_matrix_for(2), [6.0, 0.0, 0.0, 6.0, 0.0, 0.0]);
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

    /// The tables added for glyph selection are all offsets and counts the
    /// file supplies, which is the shape of every parser bug this repository
    /// has found. None of them may read past the end, allocate on a count it
    /// was handed, or take longer than the glyphs it has.
    #[test]
    fn hostile_charsets_encodings_and_fdselects_are_survivable() {
        // A charset run claiming 65535 glyphs in a font that has four.
        let font = Fixture {
            charset: Table::Bytes(charset_runs(2, &[(34, 0xFFFF)])),
            ..abc(Table::default())
        }
        .build();
        let cff = Cff::parse(&font).expect("it still parses");
        assert_eq!(cff.glyph_count(), 4, "the run cannot invent glyphs");
        assert_eq!(cff.gid_for_name("A"), Some(1));

        // A run starting at the top of the SID space, which would overflow.
        let font = Fixture {
            charset: Table::Bytes(charset_runs(1, &[(0xFFFF, 255)])),
            ..abc(Table::default())
        }
        .build();
        assert!(Cff::parse(&font).is_some());

        // A charset, an encoding and an FDSelect pointing past the end of the
        // file, and at each other.
        let base = abc(Table::Bytes(charset_0(&ABC))).build();
        for offset in [0i32, 1, 2, 3, 7, 0x7FFF_FFFF, -1] {
            for op in [15u16, 16] {
                let mut font = base.clone();
                // Rewrite the operand of the first matching operator, which
                // is at a known place because every operand is five bytes.
                if let Some(at) = find_operator(&font, op) {
                    font[at - 4..at].copy_from_slice(&offset.to_be_bytes());
                }
                let Some(cff) = Cff::parse(&font) else {
                    continue;
                };
                for glyph in 0..8u16 {
                    let _ = cff.outline(glyph);
                    let _ = cff.glyph_name(glyph);
                }
                let _ = cff.gid_for_name("A");
                let _ = cff.gid_for_code(65);
            }
        }

        // An FDSelect format 3 whose ranges run backwards, overlap, and end
        // past the glyph count.
        for table in [
            vec![3u8, 0, 1, 0, 9, 0, 0, 0],
            vec![3, 0, 2, 0, 3, 0, 0, 0, 1, 0, 0],
            vec![3, 0xFF, 0xFF, 0, 0, 0],
            vec![0, 9, 9, 9],
            vec![7],
            vec![],
        ] {
            let font = cid_fixture(table).build();
            let Some(cff) = Cff::parse(&font) else {
                continue;
            };
            for glyph in 0..8u16 {
                let _ = cff.outline(glyph);
                let _ = cff.advance(glyph);
                let _ = cff.font_matrix_for(glyph);
            }
        }
    }

    /// Where the operand of a one-operand Top DICT operator ends, in a
    /// fixture whose operands are all the fixed five-byte form.
    fn find_operator(font: &[u8], op: u16) -> Option<usize> {
        let wanted = entry(op, &[0]);
        let tail = *wanted.last()?;
        font.windows(6)
            .position(|w| w[0] == 29 && w[5] == tail)
            .map(|at| at + 5)
    }
}
