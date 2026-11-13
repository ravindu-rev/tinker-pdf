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
//!
//! # One machine for many glyphs
//!
//! [`Cff::outline_into`] runs a glyph on a caller's [`Machine`] and writes it
//! into a caller's [`Outline`], so a caller reading every glyph of a face
//! allocates the operand stack and the segment buffer once rather than once a
//! glyph; [`Cff::outline`] is that call with a fresh pair. Only the buffers
//! are carried: the stack is cleared before each glyph, and the operation
//! budget, the subroutine depth cap, the transient array and `random`'s
//! sequence are each glyph's own. The two subroutine biases (Type 2
//! charstring specification, 4.7) are a property of the INDEXes, so they are
//! resolved when the face is parsed — the global one and each Private DICT's
//! local one — and a `callsubr` reads the stored number. [`Cff::advance`]
//! runs a charstring only as far as the operator that settles its width, and
//! builds no outline.
//!
//! # Computed operands
//!
//! A CFF charstring runs 5177's arithmetic, storage and conditional
//! operators (§4.4 to §4.6) through [`compute`], so an operand it computes —
//! a `callsubr` number among them — is the one it draws with. `cff_subset.rs`
//! walks charstrings through the same function, so the two agree on every
//! stack depth a `hintmask` is measured from. Each result is the IEEE 754
//! operation, settled to 0 where it is not finite or not normal, and
//! `random` draws a fixed sequence that restarts at every glyph, so an
//! outline is the same bits on every target and in any order (ruling 4).
//! CFF2 has none of these operators and clears the stack at them. A run
//! records the first [`Bound`] it meets.

use crate::outline::{Outline, Segment};

/// A parsed CFF font.
#[derive(Clone, Debug)]
pub struct Cff<'a> {
    charstrings: Index<'a>,
    global_subrs: Index<'a>,
    /// The bias of [`Cff::global_subrs`], resolved when the face was parsed.
    global_bias: i32,
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
    /// `ROS`'s registry and ordering SIDs, where both are SIDs.
    ros: Option<(u16, u16)>,
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
    /// The Top DICT's style entries, each its default where absent.
    facts: TopFacts,
}

/// The Top DICT's entries [`Cff::style`] answers, read when the font is
/// parsed: Table 9 of TN #5176, each absent or unreadable entry its default.
#[derive(Clone, Copy, Debug, PartialEq)]
struct TopFacts {
    font_bbox: [f64; 4],
    is_fixed_pitch: bool,
    italic_angle: f64,
    underline_position: f64,
    underline_thickness: f64,
    /// `Weight`'s SID.
    weight: Option<u16>,
}

/// What a CFF font's Top DICT and Private DICT state of its style
/// ([`Cff::style`]): the entries the font flags and the font measurement API
/// read (`docs/design/font-metrics.md`), each absent entry its default as
/// TN #5176 states it — section 9's Table 9 for the Top DICT, section 15's
/// Table 23 for the Private DICT.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Style<'a> {
    /// `FontBBox` (5) as `[llx, lly, urx, ury]`, in glyph space: Table 9's
    /// `0 0 0 0` where it is absent or not four finite numbers.
    pub font_bbox: [f64; 4],
    /// `isFixedPitch` (12 1), a boolean written as a number, not zero: Table
    /// 9's `0 (false)` where absent.
    pub is_fixed_pitch: bool,
    /// `ItalicAngle` (12 2), degrees counterclockwise from the vertical:
    /// Table 9's `0` where absent.
    pub italic_angle: f64,
    /// `UnderlinePosition` (12 3): Table 9's `-100` where absent.
    pub underline_position: f64,
    /// `UnderlineThickness` (12 4): Table 9's `50` where absent.
    pub underline_thickness: f64,
    /// `Weight` (4), its SID read through the standard strings and the
    /// string INDEX: `None` where absent, which Table 9 gives no default.
    pub weight: Option<&'a str>,
    /// The Private DICT's `ForceBold` (12 14) not zero: Table 23's `false`
    /// where absent. A CID-keyed font, whose Top DICT names no Private DICT,
    /// reads its first Font DICT's.
    pub force_bold: bool,
    /// The Private DICT's `StdVW` (11), the dominant vertical stem's width
    /// in glyph space, where it is a finite number; Table 23 gives it no
    /// default. A CID-keyed font reads its first Font DICT's.
    pub std_vw: Option<f64>,
}

/// A Private DICT's contribution to running a charstring.
///
/// CFF2's Private DICTs build one too (`cff2.rs`): their local subroutines,
/// no widths, and the `vsindex` a glyph's blends default to.
#[derive(Clone, Debug)]
pub(crate) struct Private<'a> {
    pub(crate) local_subrs: Index<'a>,
    /// The bias of `local_subrs`, resolved when the DICT was read.
    pub(crate) local_bias: i32,
    pub(crate) default_width: f64,
    pub(crate) nominal_width: f64,
    /// CFF2's `vsindex` key: the item variation data a glyph's blends read
    /// unless the glyph names another. Zero, and unread, in CFF.
    pub(crate) vsindex: u16,
    /// `ForceBold` (12 14) not zero; Table 23's default is false.
    pub(crate) force_bold: bool,
    /// `StdVW` (11), where it is a finite number.
    pub(crate) std_vw: Option<f64>,
}

impl Default for Private<'_> {
    fn default() -> Self {
        Private {
            local_subrs: Index::default(),
            // An empty INDEX's bias, which a DICT with no Subrs keeps.
            local_bias: EMPTY_INDEX_BIAS,
            default_width: 0.0,
            nominal_width: 0.0,
            vsindex: 0,
            force_bold: false,
            std_vw: None,
        }
    }
}

/// A CFF INDEX: a count, offsets, then the data.
#[derive(Clone, Debug, Default)]
pub(crate) struct Index<'a> {
    offsets: Vec<usize>,
    data: &'a [u8],
}

/// Why a CFF2 INDEX was not read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum IndexRefusal {
    /// Its count, offsets or data run past the bytes.
    Truncated,
    /// It counts more entries than [`CFF2_INDEX_LIMIT`].
    Count(u32),
}

/// The most entries a CFF2 INDEX is read with. Its count is 32 bits, and no
/// glyph id, Font DICT number or biased subroutine number reaches past
/// 65 536, so a larger count is refused before its offsets are read.
pub(crate) const CFF2_INDEX_LIMIT: u32 = 65_536;

impl<'a> Index<'a> {
    pub(crate) fn parse(data: &'a [u8], at: usize) -> Option<(Index<'a>, usize)> {
        let count = usize::from(u16::from_be_bytes([*data.get(at)?, *data.get(at + 1)?]));
        Index::parse_after_count(data, count, at + 2)
    }

    /// A CFF2 INDEX (CFF2 chapter, 'INDEX data'): CFF's, with a 32-bit count,
    /// and an empty one four bytes long.
    pub(crate) fn parse_cff2(
        data: &'a [u8],
        at: usize,
    ) -> Result<(Index<'a>, usize), IndexRefusal> {
        let count = data
            .get(at..at.checked_add(4).ok_or(IndexRefusal::Truncated)?)
            .map(|b| u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
            .ok_or(IndexRefusal::Truncated)?;
        if count > CFF2_INDEX_LIMIT {
            return Err(IndexRefusal::Count(count));
        }
        Index::parse_after_count(data, count as usize, at + 4).ok_or(IndexRefusal::Truncated)
    }

    /// The rest of an INDEX once its count is read: `at` is its `offSize`.
    fn parse_after_count(data: &'a [u8], count: usize, at: usize) -> Option<(Index<'a>, usize)> {
        if count == 0 {
            return Some((Index::default(), at));
        }

        let off_size = usize::from(*data.get(at)?);
        if !(1..=4).contains(&off_size) {
            return None;
        }

        let offsets_at = at + 1;
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
        // Saturating: a 32-bit offset past the data reaches past `usize` on a
        // 32-bit target, and the data is cut at its end below either way.
        let end = base.saturating_add(offsets.last().copied().unwrap_or(1).saturating_sub(1));

        Some((
            Index {
                offsets,
                data: data.get(base..end.min(data.len()))?,
            },
            end,
        ))
    }

    pub(crate) fn get(&self, index: usize) -> Option<&'a [u8]> {
        let start = self.offsets.get(index)?.checked_sub(1)?;
        let end = self.offsets.get(index + 1)?.checked_sub(1)?;
        (end >= start).then(|| self.data.get(start..end))?
    }

    pub(crate) fn len(&self) -> usize {
        self.offsets.len().saturating_sub(1)
    }
}

/// One token of DICT data.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum DictToken {
    /// An operator; a two-byte one as `0x0C00` and its second byte.
    Operator(u16),
    /// A number.
    Operand(f64),
}

/// The DICT token at `at`, and where the next one starts: `None` at the end,
/// or where an operand is cut short, which ends the DICT; `Some((None, _))`
/// for a reserved byte, read past. A byte up to `max_operator` is a
/// one-byte operator — 21 in CFF, 24 in CFF2, whose DICTs add `vsindex`,
/// `blend` and `VariationStoreOffset` — and 12 escapes a two-byte one.
pub(crate) fn dict_token(
    data: &[u8],
    at: usize,
    max_operator: u8,
) -> Option<(Option<DictToken>, usize)> {
    let b = *data.get(at)?;
    let token = match b {
        12 => {
            let second = data.get(at + 1).copied().unwrap_or(0);
            return Some((
                Some(DictToken::Operator(0x0C00 | u16::from(second))),
                at + 2,
            ));
        }
        b if b <= max_operator => {
            return Some((Some(DictToken::Operator(u16::from(b))), at + 1));
        }
        28 => {
            let (Some(&a), Some(&c)) = (data.get(at + 1), data.get(at + 2)) else {
                return None;
            };
            (f64::from(i16::from_be_bytes([a, c])), at + 3)
        }
        29 => {
            let bytes = data.get(at + 1..at + 5)?;
            let value = i32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
            (f64::from(value), at + 5)
        }
        // A real number, packed as nibbles.
        30 => parse_real(data, at + 1),
        32..=246 => (f64::from(b) - 139.0, at + 1),
        247..=250 => {
            let next = *data.get(at + 1)?;
            (
                (f64::from(b) - 247.0) * 256.0 + f64::from(next) + 108.0,
                at + 2,
            )
        }
        251..=254 => {
            let next = *data.get(at + 1)?;
            (
                -(f64::from(b) - 251.0) * 256.0 - f64::from(next) - 108.0,
                at + 2,
            )
        }
        _ => return Some((None, at + 1)),
    };
    Some((Some(DictToken::Operand(token.0)), token.1))
}

/// Reads a DICT into `(operator, operands)` pairs.
pub(crate) fn parse_dict(data: &[u8]) -> Vec<(u16, Vec<f64>)> {
    let mut out = Vec::new();
    let mut operands: Vec<f64> = Vec::new();
    let mut at = 0usize;

    while at < data.len() {
        let Some((token, next)) = dict_token(data, at, 21) else {
            break;
        };
        at = next;
        match token {
            Some(DictToken::Operator(op)) => out.push((op, std::mem::take(&mut operands))),
            Some(DictToken::Operand(value)) => operands.push(value),
            None => {}
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

pub(crate) fn dict_get(dict: &[(u16, Vec<f64>)], op: u16) -> Option<&[f64]> {
    dict.iter()
        .find(|(o, _)| *o == op)
        .map(|(_, v)| v.as_slice())
}

impl<'a> Cff<'a> {
    /// Parses a bare CFF font program.
    #[must_use]
    pub fn parse(data: &'a [u8]) -> Option<Cff<'a>> {
        Cff::parse_with(data, &mut 0)
    }

    /// [`Cff::parse`], and the steps it took
    /// (`docs/design/resource-exhaustion.md`, "The counters"): an entry for
    /// each offset of each INDEX it reads, and for each glyph's charset,
    /// encoding and FDSelect entry and each Font DICT.
    #[must_use]
    pub fn parse_counted(data: &'a [u8]) -> (Option<Cff<'a>>, u64) {
        let mut steps = 0u64;
        let font = Cff::parse_with(data, &mut steps);
        (font, steps)
    }

    fn parse_with(data: &'a [u8], steps: &mut u64) -> Option<Cff<'a>> {
        let read = |steps: &mut u64, entries: usize| {
            *steps = steps.saturating_add(u64::try_from(entries).unwrap_or(u64::MAX));
        };
        // The header's fourth byte is its own size, so the name INDEX follows.
        let header_size = usize::from(*data.get(2)?);
        let (names, at) = Index::parse(data, header_size)?;
        read(steps, names.len() + 1);
        let (top_dicts, at) = Index::parse(data, at)?;
        read(steps, top_dicts.len() + 1);
        let (strings, at) = Index::parse(data, at)?;
        read(steps, strings.len() + 1);
        let (global_subrs, _) = Index::parse(data, at)?;
        read(steps, global_subrs.len() + 1);
        let global_bias = bias(global_subrs.len());

        let top = parse_dict(top_dicts.get(0)?);

        let charstrings_at = dict_get(&top, 17).and_then(<[f64]>::first).copied()? as usize;
        let (charstrings, _) = Index::parse(data, charstrings_at)?;
        let glyphs = charstrings.len();
        read(steps, glyphs + 1);

        // The Private DICT gives the local subroutines and the widths.
        let private = match dict_get(&top, 18) {
            Some(operands) => read_private(data, operands)?,
            None => Private::default(),
        };
        read(steps, private.local_subrs.len() + 1);

        // 12 30 is ROS: its presence, and nothing else, is what makes a font
        // CID-keyed, which changes what the charset means.
        let is_cid = dict_get(&top, 0x0C1E).is_some();
        // Its first two operands are the registry's and the ordering's SIDs,
        // the third the supplement (Adobe Technical Note 5176, Table 10).
        let ros = dict_get(&top, 0x0C1E).and_then(|operands| {
            let sid = |at: usize| {
                operands
                    .get(at)
                    .filter(|value| value.fract() == 0.0 && (0.0..=65_535.0).contains(*value))
                    .map(|value| *value as u16)
            };
            Some((sid(0)?, sid(1)?))
        });

        // 15 is charset. Absent means the ISOAdobe predefined one, whose
        // offset is also zero.
        let charset_at = dict_get(&top, 15)
            .and_then(<[f64]>::first)
            .copied()
            .unwrap_or(0.0);
        let charset = read_charset(data, charset_at, glyphs, is_cid);
        read(steps, charset.len());
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
        read(steps, encoding.len());

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
        read(steps, fds.len() + fd_select.len());

        // 12 7 is FontMatrix; the default is 1/1000 in both axes.
        let (font_matrix, top_has_matrix) = match dict_get(&top, 0x0C07) {
            Some(m) if m.len() >= 6 => ([m[0], m[1], m[2], m[3], m[4], m[5]], true),
            _ => ([0.001, 0.0, 0.0, 0.001, 0.0, 0.0], false),
        };
        let facts = top_facts(&top);

        Some(Cff {
            charstrings,
            global_subrs,
            global_bias,
            strings,
            private,
            charset,
            by_sid,
            encoding,
            is_cid,
            ros,
            fd_select,
            fds,
            font_matrix,
            top_has_matrix,
            facts,
        })
    }

    /// The font's style as its Top DICT and Private DICT state it, each
    /// absent entry its default (TN #5176 Tables 9 and 23): what the font
    /// flags and the font measurement API read (`docs/design/font-metrics.md`).
    #[must_use]
    pub fn style(&self) -> Style<'a> {
        // A CID-keyed font's Top DICT names no Private DICT of its own; its
        // first Font DICT's stands for the font's.
        let private = if self.is_cid {
            self.fds.first().map(|(private, _)| private)
        } else {
            Some(&self.private)
        };
        Style {
            font_bbox: self.facts.font_bbox,
            is_fixed_pitch: self.facts.is_fixed_pitch,
            italic_angle: self.facts.italic_angle,
            underline_position: self.facts.underline_position,
            underline_thickness: self.facts.underline_thickness,
            weight: self.facts.weight.and_then(|sid| self.sid_name(sid)),
            force_bold: private.is_some_and(|private| private.force_bold),
            std_vw: private.and_then(|private| private.std_vw),
        }
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

    /// The character collection a CID-keyed font declares in `ROS` (Top DICT
    /// `12 30`): its registry and its ordering, read through the string
    /// INDEX, as `("Adobe", "Japan1")`. `None` for a font that is not
    /// CID-keyed, and for one whose `ROS` names strings it does not hold.
    ///
    /// What says whether the font's CIDs are another document's: a CID of a
    /// collection reaches the same character in every face keyed by that
    /// collection (`docs/design/fallback-faces.md`).
    #[must_use]
    pub fn registry_ordering(&self) -> Option<(&'a str, &'a str)> {
        let (registry, ordering) = self.ros?;
        Some((self.sid_name(registry)?, self.sid_name(ordering)?))
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

    /// The CID a glyph carries: the charset read forwards, which is
    /// [`Cff::gid_for_cid`]'s inverse wherever the charset is one-to-one.
    ///
    /// A writer needs this direction. A composite font addresses a CID-keyed
    /// program by CID (ISO 32000-1 9.7.4.2), so a caller holding a glyph index
    /// — from a shaper, or a document that states glyphs — has to say which
    /// CID that glyph answers to before the code can be written at all.
    ///
    /// `None` when the font is not CID-keyed, whose charset holds names, or
    /// when the glyph is past the end of the charset.
    #[must_use]
    pub fn cid_for_gid(&self, glyph: u16) -> Option<u16> {
        if !self.is_cid {
            return None;
        }
        self.charset.get(usize::from(glyph)).copied()
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

    /// The glyph the **standard** encoding gives a code, whatever the font's
    /// own encoding says.
    ///
    /// This is not a second spelling of [`Cff::gid_for_code`]. It exists for
    /// one caller: `endchar` with four operands is the deprecated `seac`
    /// (Type 2 charstring specification, appendix C), whose two components are
    /// named by **StandardEncoding** codes and by nothing else — the font's
    /// own encoding does not enter into it. A subsetter that resolved those
    /// through `gid_for_code` would drop the wrong component and an accented
    /// letter would come out as bare space in every reader that draws `seac`.
    pub(crate) fn gid_for_standard_code(&self, code: u8) -> Option<u16> {
        let sid = STANDARD_ENCODING
            .iter()
            .find_map(|(c, sid)| (*c == code).then_some(*sid))?;
        self.gid_for_sid(sid)
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
    ///
    /// [`Cff::outline_into`] with a fresh outline and machine: a caller that
    /// reads many glyphs keeps one of each and calls that instead.
    #[must_use]
    pub fn outline(&self, glyph: u16) -> Option<Outline> {
        self.outline_counted(glyph).0
    }

    /// [`Cff::outline`], and the steps it took: each operand and operator the
    /// Type 2 machine read, in the glyph's charstring and in every subroutine
    /// it called — what the machine spends of its budget.
    #[must_use]
    pub fn outline_counted(&self, glyph: u16) -> (Option<Outline>, u64) {
        let mut outline = Outline::default();
        let mut machine = Machine::new();
        match self.interpret(glyph, &mut outline, &mut machine) {
            Some(steps) => (Some(outline), steps),
            None => (None, 0),
        }
    }

    /// One glyph's outline, in the font's own units, written into `outline`
    /// through `machine`'s operand stack.
    ///
    /// The segments are [`Cff::outline`]'s, segment for segment; what
    /// differs is where they are kept. `outline`'s segments are replaced and
    /// its allocation kept, and `machine` keeps its stack, so a caller that
    /// reads every glyph of a face through one of each allocates both once.
    /// Nothing else carries from one glyph to the next: the stack is cleared
    /// first, and the budget, the depth cap and the transient array start
    /// afresh, so the order glyphs are read in changes no outline.
    ///
    /// False, with `outline` emptied, for a glyph the font does not have.
    pub fn outline_into(&self, glyph: u16, outline: &mut Outline, machine: &mut Machine) -> bool {
        self.interpret(glyph, outline, machine).is_some()
    }

    /// Runs one glyph's charstring into `outline`, and says how many steps it
    /// took, or `None` for a glyph the font does not have.
    fn interpret(&self, glyph: u16, outline: &mut Outline, machine: &mut Machine) -> Option<u64> {
        outline.segments.clear();
        let charstring = self.charstrings.get(usize::from(glyph))?;
        let mut ctx = self.charstring_context(glyph, Some(outline), &mut machine.stack);
        ctx.run(charstring);
        if ctx.open {
            ctx.emit(Segment::Close);
        }
        Some(u64::from(CHARSTRING_BUDGET - ctx.budget))
    }

    /// A glyph's Type 2 charstring as the CharStrings INDEX holds it, before
    /// any subroutine it calls is followed.
    #[must_use]
    pub fn charstring(&self, glyph: u16) -> Option<&'a [u8]> {
        self.charstrings.get(usize::from(glyph))
    }

    /// A glyph's advance, when the charstring declares one.
    ///
    /// The width is settled by the first stack-clearing operator and nothing
    /// after it changes it, so the charstring is run only that far, and no
    /// outline is built.
    #[must_use]
    pub fn advance(&self, glyph: u16) -> Option<f64> {
        let charstring = self.charstrings.get(usize::from(glyph))?;
        let mut machine = Machine::new();
        let mut ctx = self.charstring_context(glyph, None, &mut machine.stack);
        ctx.run(charstring);
        Some(ctx.width.unwrap_or(ctx.private.default_width))
    }

    /// One glyph run as the reader runs it, with every token it reads written
    /// to `tape`, its calls inlined (`cff_subset.rs`'s flattening): the first
    /// bound the run met, `None` inside for none, and `None` for a glyph the
    /// font does not have.
    pub(crate) fn run_taped(&self, glyph: u16, tape: &mut Tape) -> Option<Option<Bound>> {
        let charstring = self.charstrings.get(usize::from(glyph))?;
        let mut outline = Outline::default();
        let mut stack = Vec::with_capacity(MAX_OPERANDS);
        let mut ctx = self
            .charstring_context(glyph, Some(&mut outline), &mut stack)
            .taped(tape);
        ctx.run(charstring);
        Some(ctx.met)
    }

    /// The first bound a glyph's run meets, or `None` for a run that meets
    /// none and for a glyph the font does not have.
    #[cfg(test)]
    pub(crate) fn first_bound(&self, glyph: u16) -> Option<Bound> {
        let charstring = self.charstrings.get(usize::from(glyph))?;
        let mut outline = Outline::default();
        let mut machine = Machine::new();
        let mut ctx = self.charstring_context(glyph, Some(&mut outline), &mut machine.stack);
        ctx.run(charstring);
        ctx.met
    }

    /// The machine one glyph runs on: its Private DICT, where its segments
    /// go — nowhere when only the width is wanted — and the stack, cleared.
    fn charstring_context<'c>(
        &'c self,
        glyph: u16,
        outline: Option<&'c mut Outline>,
        stack: &'c mut Vec<f64>,
    ) -> Charstring<'a, 'c> {
        let private = match self.fd_for(glyph) {
            Some((private, _)) => private,
            None => &self.private,
        };
        Charstring::new(
            Flavour::Cff,
            Subroutines {
                global: &self.global_subrs,
                global_bias: self.global_bias,
                private,
            },
            outline,
            stack,
            None,
        )
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

/// The Top DICT's style entries, each absent or unreadable entry the
/// default TN #5176's Table 9 gives it.
fn top_facts(top: &[(u16, Vec<f64>)]) -> TopFacts {
    let number = |op: u16, default: f64| {
        dict_get(top, op)
            .and_then(<[f64]>::first)
            .copied()
            .filter(|value| value.is_finite())
            .unwrap_or(default)
    };
    let font_bbox = match dict_get(top, 5) {
        Some(&[llx, lly, urx, ury]) if [llx, lly, urx, ury].iter().all(|v| v.is_finite()) => {
            [llx, lly, urx, ury]
        }
        _ => [0.0; 4],
    };
    TopFacts {
        font_bbox,
        is_fixed_pitch: number(0x0C01, 0.0) != 0.0,
        italic_angle: number(0x0C02, 0.0),
        underline_position: number(0x0C03, -100.0),
        underline_thickness: number(0x0C04, 50.0),
        weight: dict_get(top, 4)
            .and_then(<[f64]>::first)
            .filter(|sid| sid.fract() == 0.0 && (0.0..=65_535.0).contains(*sid))
            .map(|sid| *sid as u16),
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
        // 12 14 is ForceBold, a boolean written as a number (Table 23).
        private.force_bold = dict_get(&dict, 0x0C0E)
            .and_then(<[f64]>::first)
            .is_some_and(|value| *value != 0.0);
        private.std_vw = dict_get(&dict, 11)
            .and_then(<[f64]>::first)
            .copied()
            .filter(|value| value.is_finite());
        // 19 is Subrs, whose offset is measured from the Private DICT rather
        // than from the start of the font.
        if let Some(&subrs) = dict_get(&dict, 19).and_then(<[f64]>::first) {
            if subrs >= 0.0 {
                if let Some((index, _)) = Index::parse(data, offset.saturating_add(subrs as usize))
                {
                    private.local_bias = bias(index.len());
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
pub(crate) fn read_fd_select(data: &[u8], offset: f64, glyphs: usize) -> Vec<u8> {
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
pub(crate) const STANDARD_STRINGS: &[&str; 391] = &[
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
///
/// A property of the INDEX, not of the call: [`Cff::parse`] resolves it once
/// for the global subroutines and once for each Private DICT's local ones,
/// and the subsetter for each INDEX it rebuilds.
pub(crate) fn bias(count: usize) -> i32 {
    #[cfg(test)]
    counters::bias_resolved();
    if count < 1240 {
        EMPTY_INDEX_BIAS
    } else if count < 33900 {
        1131
    } else {
        32768
    }
}

/// The bias of an INDEX of fewer than 1 240 subroutines, an empty one
/// included.
const EMPTY_INDEX_BIAS: i32 = 107;

/// The operands and operators one glyph's Type 2 machine may read, its
/// subroutines' included, before it stops.
const CHARSTRING_BUDGET: u32 = 64_000;

/// The operands the Type 2 stack holds (Type 2 charstring specification,
/// appendix B); a longer one is corruption, and the excess is dropped.
const MAX_OPERANDS: usize = 48;

/// The operands a CFF2 stack holds (CFF2 chapter: "The maximum number of
/// operands on the CFF2 stack is 513"); a longer one refuses its glyph.
pub(crate) const CFF2_MAX_OPERANDS: usize = 513;

/// Which charstring language a machine runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Flavour {
    /// Type 2 charstrings in a CFF table.
    Cff,
    /// CFF2 charstrings (CFF2 chapter, 'CharString operators'): no width, no
    /// `endchar` and no `return` — a charstring and a subroutine end where
    /// their data does — no `put`, `get` or arithmetic, a stack of 513, and
    /// `vsindex` and `blend`.
    Cff2,
}

/// Why a CFF2 machine refused what it ran. CFF's machine stops silently
/// where it always has; every bound a CFF2 charstring or DICT passes is
/// refused by name instead.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Stop {
    /// The operation budget ran out.
    Budget,
    /// Subroutines nested past ten levels.
    SubroutineDepth,
    /// More operands than the stack holds.
    StackOverflow,
    /// A `blend` whose `n` is not a whole number or whose operands are
    /// fewer than `n + n·k + 1`.
    BlendOperands,
    /// `blend` or `vsindex` in a table with no variation store.
    NoVariationStore,
    /// `vsindex` naming no item variation data, or a non-integral one.
    VsindexRange,
    /// `vsindex` after a `blend`, or given twice.
    VsindexMisplaced,
    /// `vsindex` with no operand.
    VsindexOperand,
}

/// A bound one charstring's run meets, recorded in either flavour
/// (`docs/design/subsetting.md`, *Bounds*): CFF draws on past it, CFF2
/// refuses the glyph at the three it shares with [`Stop`], and
/// flattening refuses every one by name, since what a reader drew past it is
/// that reader's and not the program's.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Bound {
    /// A push past the 48 operands the stack holds, dropped.
    StackOverflow,
    /// A `put` or `get` outside the transient array's 32 elements.
    TransientIndex,
    /// A call past ten levels deep, which does not run.
    SubroutineDepth,
    /// The operation budget ran out.
    Budget,
    /// A `callsubr` or `callgsubr` naming no subroutine the INDEX holds.
    MissingSubroutine,
}

/// The transient array's elements (5177, appendix B).
pub(crate) const TRANSIENT_ELEMENTS: usize = 32;

/// `random`'s sequence (`docs/design/subsetting.md`, *The CFF reader's
/// arithmetic*): a 32-bit state, started afresh at every glyph, stepped as
/// xorshift with shifts 13, 17 and 5, each value `((s >> 16) + 1) / 65536`.
///
/// 5177 §4.4 asks only for a value in (0, 1]. This one reads no clock and no
/// operating system, holds a 16.16 number exactly, and restarts each glyph,
/// so the order glyphs are drawn in changes no outline (ruling 4).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Random(u32);

impl Random {
    /// The state every glyph's run starts at.
    pub(crate) const SEED: u32 = 2_463_534_242;

    pub(crate) fn new() -> Random {
        Random(Self::SEED)
    }

    /// The next value of the sequence, in (0, 1].
    pub(crate) fn next_value(&mut self) -> f64 {
        let mut s = self.0;
        s ^= s << 13;
        s ^= s >> 17;
        s ^= s << 5;
        self.0 = s;
        f64::from((s >> 16) + 1) / 65_536.0
    }
}

impl Default for Random {
    fn default() -> Random {
        Random::new()
    }
}

/// A result settled as `docs/design/subsetting.md` decides 5177's undefined
/// cases: one that is not finite, or smaller in magnitude than the least
/// normal double, is 0 — *zero for underflow*, and this reader's answer to
/// the overflow 5177 leaves undefined.
fn settle(value: f64) -> f64 {
    if !value.is_finite() || value.abs() < f64::MIN_POSITIVE {
        0.0
    } else {
        value
    }
}

/// Runs the two-byte operator `op` over `stack` where it is one of 5177's
/// arithmetic (§4.4), storage (§4.5) or conditional (§4.6) operators: `None`
/// where it is not one, and otherwise `Some` with the bound it met, if any.
///
/// The reader and `cff_subset.rs`'s token walk both run it, so the depth the
/// walk counts stem hints from is the reader's. An operator that finds fewer
/// operands than it takes leaves the stack as it is; `put` and `get` pop what
/// there is. Every depth this leaves is a function of the operator and the
/// depth it found, never of the values.
pub(crate) fn compute(
    op: u8,
    stack: &mut Vec<f64>,
    transient: &mut [f64; TRANSIENT_ELEMENTS],
    random: &mut Random,
    max: usize,
) -> Option<Option<Bound>> {
    let takes = match op {
        // put and get, over the transient array.
        20 | 21 => return Some(storage(op, stack, transient, max)),
        // random
        23 => 0,
        // not, abs, neg, drop, sqrt, dup
        5 | 9 | 14 | 18 | 26 | 27 => 1,
        // and, or, add, sub, div, eq, mul, exch; index and roll, which take
        // their count and at least what they count.
        3 | 4 | 10 | 11 | 12 | 15 | 24 | 28 | 29 | 30 => 2,
        // ifelse
        22 => 4,
        _ => return None,
    };
    if stack.len() < takes {
        return Some(None);
    }
    let unary = |stack: &mut Vec<f64>, f: fn(f64) -> f64| {
        if let Some(top) = stack.last_mut() {
            *top = settle(f(*top));
        }
    };
    let binary = |stack: &mut Vec<f64>, f: fn(f64, f64) -> f64| {
        if let (Some(b), Some(a)) = (stack.pop(), stack.pop()) {
            stack.push(settle(f(a, b)));
        }
    };
    fn flag(truth: bool) -> f64 {
        if truth {
            1.0
        } else {
            0.0
        }
    }
    let mut pushed: Option<f64> = None;
    match op {
        9 => unary(stack, f64::abs),
        14 => unary(stack, |v| -v),
        // A negative operand's root is undefined, and 0.
        26 => unary(stack, |v| if v < 0.0 { 0.0 } else { v.sqrt() }),
        5 => unary(stack, |v| if v == 0.0 { 1.0 } else { 0.0 }),
        10 => binary(stack, |a, b| a + b),
        11 => binary(stack, |a, b| a - b),
        24 => binary(stack, |a, b| a * b),
        // A zero divisor gives 0.
        12 => binary(stack, |a, b| if b == 0.0 { 0.0 } else { a / b }),
        3 => binary(stack, |a, b| flag(a != 0.0 && b != 0.0)),
        4 => binary(stack, |a, b| flag(a != 0.0 || b != 0.0)),
        15 => binary(stack, |a, b| flag(a == b)),
        18 => {
            stack.pop();
        }
        28 => {
            let n = stack.len();
            stack.swap(n - 1, n - 2);
        }
        27 => pushed = stack.last().copied(),
        23 => pushed = Some(random.next_value()),
        29 => {
            // index: a negative `i` copies the top (5177), one past the
            // bottom the bottom.
            let i = stack.pop().unwrap_or(0.0).trunc();
            let depth = stack.len();
            let from_top = if i < 0.0 {
                0
            } else {
                (i.min((depth - 1) as f64)) as usize
            };
            pushed = stack.get(depth - 1 - from_top).copied();
        }
        30 => {
            // roll: the top N elements turned by J, positive J upward — the
            // top moving down to the bottom of the N, 5177's picture.
            let j = stack.pop().unwrap_or(0.0).trunc();
            let n = stack.pop().unwrap_or(0.0).trunc();
            let depth = stack.len();
            if n > 0.0 && depth > 0 {
                let n = (n.min(depth as f64)) as usize;
                let turn = (j as i64).rem_euclid(n as i64) as usize;
                stack[depth - n..].rotate_right(turn);
            }
        }
        22 => {
            // ifelse: s1 s2 v1 v2, s1 where v1 <= v2.
            let v2 = stack.pop().unwrap_or(0.0);
            let v1 = stack.pop().unwrap_or(0.0);
            let s2 = stack.pop().unwrap_or(0.0);
            let s1 = stack.pop().unwrap_or(0.0);
            stack.push(if v1 <= v2 { s1 } else { s2 });
        }
        _ => {}
    }
    match pushed {
        Some(value) if stack.len() < max => {
            stack.push(value);
            Some(None)
        }
        Some(_) => Some(Some(Bound::StackOverflow)),
        None => Some(None),
    }
}

/// `put` and `get` over the transient array: each pops what there is, an
/// index is truncated toward zero and a negative one taken as element 0, and
/// one past the array is ignored by `put` and read as 0 by `get`. An index
/// outside 5177's 0 to 31 is a bound met.
fn storage(
    op: u8,
    stack: &mut Vec<f64>,
    transient: &mut [f64; TRANSIENT_ELEMENTS],
    max: usize,
) -> Option<Bound> {
    let element = |index: f64| -> (usize, Option<Bound>) {
        let inside = (0.0..TRANSIENT_ELEMENTS as f64).contains(&index.trunc());
        (
            index.max(0.0) as usize,
            (!inside).then_some(Bound::TransientIndex),
        )
    };
    if op == 20 {
        let (Some(index), Some(value)) = (stack.pop(), stack.pop()) else {
            return None;
        };
        let (at, met) = element(index);
        if let Some(slot) = transient.get_mut(at) {
            *slot = value;
        }
        return met;
    }
    let index = stack.pop()?;
    let (at, met) = element(index);
    // The index's own slot is free again, so the value always has room.
    if stack.len() < max {
        stack.push(transient.get(at).copied().unwrap_or(0.0));
    }
    met
}

/// What flattening (`cff_subset.rs`; `docs/design/subsetting.md`,
/// *Flattening*) reads of one glyph's run: every token the machine read, in
/// order, as one body. A call is written as nothing together with the number
/// written immediately before it, where that is the number it popped, and
/// otherwise as `drop`; `return` is written as nothing; `endchar` is written.
/// So the stack depths, stem counts, widths and `random`'s draws of the body
/// are the run's at every token.
#[derive(Debug, Default)]
pub(crate) struct Tape {
    /// The flattened charstring so far.
    pub(crate) bytes: Vec<u8>,
    /// Whether `endchar` ran.
    pub(crate) ended: bool,
    /// The StandardEncoding codes of `seac`'s two components, where
    /// `endchar` carried four or five operands.
    pub(crate) seac: Option<(u8, u8)>,
    /// The first thing that keeps the run from being written as one
    /// charstring.
    pub(crate) flaw: Option<Flaw>,
}

/// Why a run read onto a [`Tape`] is not one charstring.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Flaw {
    /// A token read after `endchar`.
    AfterEndchar,
    /// A token cut by the end of its body.
    Cut,
    /// More than the 65 535 bytes a charstring holds (5177, appendix B).
    TooLong,
}

impl Tape {
    /// The most bytes a charstring holds (5177, appendix B).
    pub(crate) const LIMIT: usize = 65_535;

    /// Writes one token, or notes one cut short by its body; answers where
    /// it starts when it is an operand the stack took.
    fn write(&mut self, token: Option<&[u8]>, operand: bool) -> Option<usize> {
        let Some(token) = token else {
            self.flaw.get_or_insert(Flaw::Cut);
            return None;
        };
        let at = self.bytes.len();
        if at.saturating_add(token.len()) > Self::LIMIT {
            self.flaw.get_or_insert(Flaw::TooLong);
            return None;
        }
        self.bytes.extend_from_slice(token);
        operand.then_some(at)
    }
}

/// What a CFF2 `blend` reads (CFF2 chapter, 'Variation operators'): each
/// item variation data subtable's region count — the `k` a `blend` pops —
/// and, at an instance, each subtable's region scalars in column order. At
/// the default instance there are no scalars, and a `blend` keeps its
/// defaults.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Blend<'b> {
    pub(crate) region_counts: &'b [u16],
    pub(crate) scalars: Option<&'b [Vec<f64>]>,
}

impl Blend<'_> {
    /// `blend` over `stack`, under the data subtable `vsindex` selects: pops
    /// `n`, then `n·k` deltas and `n` defaults, and pushes the `n` results —
    /// each default plus its deltas times the scalars, in region order,
    /// rounded to 16.16 (`docs/design/variable-fonts.md`, 'The numeric
    /// model'), or the default itself where there are no scalars.
    pub(crate) fn apply(&self, vsindex: u16, stack: &mut Vec<f64>) -> Result<(), Stop> {
        if self.region_counts.is_empty() {
            return Err(Stop::NoVariationStore);
        }
        let k = usize::from(
            *self
                .region_counts
                .get(usize::from(vsindex))
                .ok_or(Stop::VsindexRange)?,
        );
        let n_value = stack.pop().ok_or(Stop::BlendOperands)?;
        if !(n_value >= 0.0 && n_value.trunc() == n_value && n_value <= stack.len() as f64) {
            return Err(Stop::BlendOperands);
        }
        let n = n_value as usize;
        let needed = n.checked_mul(k + 1).ok_or(Stop::BlendOperands)?;
        let base = stack.len().checked_sub(needed).ok_or(Stop::BlendOperands)?;
        let scalars = self.scalars.and_then(|all| all.get(usize::from(vsindex)));
        for item in 0..n {
            let default = stack.get(base + item).copied().unwrap_or(0.0);
            let value = match scalars {
                None => default,
                Some(scalars) => {
                    let mut value = default;
                    for (region, scalar) in scalars.iter().enumerate().take(k) {
                        let delta = stack
                            .get(base + n + item * k + region)
                            .copied()
                            .unwrap_or(0.0);
                        value += scalar * delta;
                    }
                    round_to_fixed(value)
                }
            };
            if let Some(slot) = stack.get_mut(base + item) {
                *slot = value;
            }
        }
        stack.truncate(base + n);
        Ok(())
    }
}

/// An `f64` rounded to 16.16 by the overview's conversion: `floor(x · 65536
/// + ½) / 65536`. Every step is exact in IEEE 754 for a value a charstring
/// carries.
pub(crate) fn round_to_fixed(value: f64) -> f64 {
    (value * 65_536.0 + 0.5).floor() / 65_536.0
}

/// One operator a CFF2 charstring ran, as a static instance re-encodes it:
/// the operator — a two-byte one as `0x0C00` and its second byte — the
/// operands on the stack when it ran, every `blend` before it resolved, and
/// a mask operator's mask bytes. Calls, `vsindex`, `blend` and operators
/// that only clear the stack are not commands: they draw nothing, and the
/// stack the next command finds says what they left.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Command {
    pub(crate) op: u16,
    pub(crate) operands: Vec<f64>,
    pub(crate) mask: Vec<u8>,
}

/// The subroutines a charstring calls: the global INDEX and its bias, and
/// the Private DICT its local ones come from.
#[derive(Clone, Copy)]
pub(crate) struct Subroutines<'a, 'b> {
    pub(crate) global: &'b Index<'a>,
    pub(crate) global_bias: i32,
    pub(crate) private: &'b Private<'a>,
}

/// The Type 2 machine's operand stack, kept across the glyphs a caller reads.
///
/// [`Cff::outline_into`] runs each glyph it is handed on this one buffer, so
/// a caller reading a whole face allocates the stack once. It carries nothing
/// from one glyph to the next: each run clears it first, and a glyph that
/// ends with operands left on it hands none of them on.
#[derive(Debug)]
pub struct Machine {
    pub(crate) stack: Vec<f64>,
}

impl Machine {
    /// A machine whose stack has room for every operand a charstring may
    /// push, so running a glyph never grows it.
    #[must_use]
    pub fn new() -> Machine {
        #[cfg(test)]
        counters::machine_made();
        Machine {
            stack: Vec::with_capacity(MAX_OPERANDS),
        }
    }
}

impl Default for Machine {
    fn default() -> Machine {
        Machine::new()
    }
}

/// What the tests count of the interpreter's work: the machines made and the
/// subroutine biases resolved, per thread so that tests run side by side do
/// not see each other's.
#[cfg(test)]
mod counters {
    use std::cell::Cell;

    thread_local! {
        static MACHINES: Cell<u64> = const { Cell::new(0) };
        static BIASES: Cell<u64> = const { Cell::new(0) };
    }

    pub(super) fn machine_made() {
        MACHINES.with(|count| count.set(count.get() + 1));
    }

    pub(super) fn bias_resolved() {
        BIASES.with(|count| count.set(count.get() + 1));
    }

    /// How many machines this thread has made.
    pub(super) fn machines() -> u64 {
        MACHINES.with(Cell::get)
    }

    /// How many biases this thread has resolved.
    pub(super) fn biases() -> u64 {
        BIASES.with(Cell::get)
    }
}

/// The one Type 2 machine, for both flavours (`docs/design/
/// variable-fonts.md`, 'CFF2'): every CFF2 difference is gated on
/// [`Flavour`], so a CFF charstring runs exactly as it always has.
pub(crate) struct Charstring<'a, 'b> {
    flavour: Flavour,
    /// The global subroutines and their bias.
    global_subrs: &'b Index<'a>,
    global_bias: i32,
    /// The Private DICT this glyph runs under, which in a CID-keyed font is
    /// its Font DICT's rather than the top-level one.
    private: &'b Private<'a>,
    /// Where the segments go: the caller's outline, or nowhere, when only the
    /// width is wanted — and then the run stops once the width is known.
    outline: Option<&'b mut Outline>,
    /// The caller's operand stack, cleared before the glyph runs.
    stack: &'b mut Vec<f64>,
    x: f64,
    y: f64,
    stems: usize,
    width: Option<f64>,
    open: bool,
    depth: u32,
    budget: u32,
    transient: [f64; TRANSIENT_ELEMENTS],
    /// `random`'s sequence, started afresh for this glyph.
    random: Random,
    /// The first bound the run met, in either flavour.
    met: Option<Bound>,
    /// CFF2: what a `blend` reads. `None` in CFF.
    blend: Option<Blend<'b>>,
    /// CFF2: the item variation data the glyph's blends read.
    vsindex: u16,
    /// CFF2: whether the charstring has set `vsindex` itself.
    vsindex_set: bool,
    /// CFF2: whether a `blend` has run, after which `vsindex` may not.
    blended: bool,
    /// CFF2: the bound the run stopped at, which refuses the glyph. CFF never
    /// sets it.
    stop: Option<Stop>,
    /// Flattening (`cff_subset.rs`): where every token the run reads is
    /// written, its calls inlined. `None` for every other run.
    tape: Option<&'b mut Tape>,
    /// Where each operator the run executes is recorded, for a static
    /// instance to re-encode; `None` everywhere else, which records nothing
    /// and changes nothing the machine does.
    commands: Option<&'b mut Vec<Command>>,
}

impl<'a, 'b> Charstring<'a, 'b> {
    /// A machine for one glyph, its stack cleared.
    pub(crate) fn new(
        flavour: Flavour,
        subroutines: Subroutines<'a, 'b>,
        outline: Option<&'b mut Outline>,
        stack: &'b mut Vec<f64>,
        blend: Option<Blend<'b>>,
    ) -> Charstring<'a, 'b> {
        stack.clear();
        Charstring {
            flavour,
            global_subrs: subroutines.global,
            global_bias: subroutines.global_bias,
            private: subroutines.private,
            outline,
            stack,
            x: 0.0,
            y: 0.0,
            stems: 0,
            width: None,
            open: false,
            depth: 0,
            budget: CHARSTRING_BUDGET,
            transient: [0.0; TRANSIENT_ELEMENTS],
            random: Random::new(),
            met: None,
            blend,
            vsindex: subroutines.private.vsindex,
            vsindex_set: false,
            blended: false,
            stop: None,
            tape: None,
            commands: None,
        }
    }

    /// The same machine, writing every token it reads to `tape`.
    pub(crate) fn taped(mut self, tape: &'b mut Tape) -> Charstring<'a, 'b> {
        self.tape = Some(tape);
        self
    }

    /// This machine, recording every operator it runs into `commands`.
    pub(crate) fn recording(mut self, commands: &'b mut Vec<Command>) -> Charstring<'a, 'b> {
        self.commands = Some(commands);
        self
    }

    /// Runs a whole charstring, closing a contour left open, and says where
    /// a CFF2 run stopped short, if it did.
    pub(crate) fn run_glyph(&mut self, code: &[u8]) -> Option<Stop> {
        self.run(code);
        if self.open {
            self.emit(Segment::Close);
            self.open = false;
        }
        self.stop
    }
}

impl Charstring<'_, '_> {
    /// The stack's capacity in this flavour.
    fn max_operands(&self) -> usize {
        match self.flavour {
            Flavour::Cff => MAX_OPERANDS,
            Flavour::Cff2 => CFF2_MAX_OPERANDS,
        }
    }

    /// Records the bound a CFF2 run stopped at; CFF's silent stops stay
    /// silent, and are met.
    fn refuse(&mut self, stop: Stop) {
        match stop {
            Stop::Budget => self.meet(Bound::Budget),
            Stop::SubroutineDepth => self.meet(Bound::SubroutineDepth),
            Stop::StackOverflow => self.meet(Bound::StackOverflow),
            _ => {}
        }
        if self.flavour == Flavour::Cff2 && self.stop.is_none() {
            self.stop = Some(stop);
        }
    }

    /// Records the first bound the run meets, in either flavour.
    fn meet(&mut self, bound: Bound) {
        self.met.get_or_insert(bound);
    }

    /// Records the operator about to run, with the stack as it finds it.
    fn note(&mut self, op: u16, mask: &[u8]) {
        if let Some(commands) = self.commands.as_deref_mut() {
            commands.push(Command {
                op,
                operands: self.stack.to_vec(),
                mask: mask.to_vec(),
            });
        }
    }

    /// CFF2's `vsindex`: one operand, the item variation data a later
    /// `blend` reads, set once and before any `blend` (CFF2 chapter,
    /// 'Variation operators').
    fn select_variation_data(&mut self) {
        if self.vsindex_set || self.blended {
            self.refuse(Stop::VsindexMisplaced);
            return;
        }
        let Some(value) = self.stack.pop() else {
            self.refuse(Stop::VsindexOperand);
            return;
        };
        self.stack.clear();
        let count = self.blend.map_or(0, |blend| blend.region_counts.len());
        if count == 0 {
            self.refuse(Stop::NoVariationStore);
            return;
        }
        if !(value >= 0.0 && value.trunc() == value && value < count as f64) {
            self.refuse(Stop::VsindexRange);
            return;
        }
        self.vsindex = value as u16;
        self.vsindex_set = true;
    }

    /// CFF2's `blend`, over the stack.
    fn blend_operands(&mut self) {
        let Some(blend) = self.blend else {
            self.refuse(Stop::NoVariationStore);
            return;
        };
        match blend.apply(self.vsindex, self.stack) {
            Ok(()) => self.blended = true,
            Err(stop) => self.refuse(stop),
        }
    }

    /// Appends a segment to the outline, if there is one.
    fn emit(&mut self, segment: Segment) {
        if let Some(outline) = self.outline.as_deref_mut() {
            outline.push(segment);
        }
    }

    /// Whether a run that wants only the width has it, which nothing after
    /// the first stack-clearing operator changes.
    fn done(&self) -> bool {
        self.outline.is_none() && self.width.is_some()
    }

    /// Runs a drawing operator over the operands, then clears them.
    ///
    /// The stack is lent to `draw` rather than copied: a drawing operator
    /// reads its operands while it moves the pen and pushes none, so the
    /// buffer goes back, cleared, with its allocation intact.
    fn with_operands(&mut self, draw: impl FnOnce(&mut Self, &[f64])) {
        let mut operands = core::mem::take(self.stack);
        draw(self, &operands);
        operands.clear();
        *self.stack = operands;
    }

    /// Takes the width if this is the first stack-clearing operator and the
    /// stack holds one more operand than the operator needs. A CFF2
    /// charstring carries no width.
    fn take_width(&mut self, even: bool) {
        if self.width.is_some() || self.flavour == Flavour::Cff2 {
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
        if self.width.is_some() || self.flavour == Flavour::Cff2 {
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
            self.emit(Segment::Close);
        }
        self.emit(Segment::MoveTo { x, y });
        self.open = true;
    }

    fn line_to(&mut self) {
        let (x, y) = (self.x, self.y);
        self.emit(Segment::LineTo { x, y });
    }

    fn run(&mut self, code: &[u8]) {
        // Ten levels of subroutines, the cap both flavours' chapters state;
        // CFF2 refuses a glyph that passes it, CFF skips the call.
        if self.depth > 10 {
            self.refuse(Stop::SubroutineDepth);
            return;
        }
        let mut at = 0usize;
        // Flattening: where on the tape the token this body read last
        // starts, while that token is an operand the stack took, so a call
        // that pops it takes it back off.
        let mut operand: Option<usize> = None;

        while at < code.len() {
            // Checked before the next byte is read, so a width settled inside
            // a subroutine ends the caller's loop too.
            if self.done() {
                return;
            }
            // A CFF2 refusal ends every caller's loop as well. CFF never
            // sets one.
            if self.stop.is_some() {
                return;
            }
            self.budget = self.budget.saturating_sub(1);
            if self.budget == 0 {
                self.refuse(Stop::Budget);
                return;
            }

            let start = at;
            let Some(&b) = code.get(at) else { return };
            at += 1;
            let depth = self.stack.len();
            // Whether the tape takes the token as it was read: a call is
            // written otherwise, and `return` and `endchar` write themselves.
            let mut verbatim = true;
            if let Some(tape) = self.tape.as_deref_mut() {
                if tape.ended {
                    tape.flaw.get_or_insert(Flaw::AfterEndchar);
                }
            }

            // A recording run notes each stem and drawing operator before it
            // runs; the mask operators and the flex family note themselves.
            if self.commands.is_some() && matches!(b, 1 | 3..=8 | 18 | 21..=27 | 30 | 31) {
                self.note(u16::from(b), &[]);
            }

            match b {
                // Operands.
                32..=246 => self.push(f64::from(b) - 139.0),
                247..=250 => {
                    let Some(&next) = code.get(at) else {
                        self.cut();
                        return;
                    };
                    at += 1;
                    self.push((f64::from(b) - 247.0) * 256.0 + f64::from(next) + 108.0);
                }
                251..=254 => {
                    let Some(&next) = code.get(at) else {
                        self.cut();
                        return;
                    };
                    at += 1;
                    self.push(-(f64::from(b) - 251.0) * 256.0 - f64::from(next) - 108.0);
                }
                28 => {
                    let (Some(&hi), Some(&lo)) = (code.get(at), code.get(at + 1)) else {
                        self.cut();
                        return;
                    };
                    at += 2;
                    self.push(f64::from(i16::from_be_bytes([hi, lo])));
                }
                255 => {
                    let Some(bytes) = code.get(at..at + 4) else {
                        self.cut();
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
                    if self.commands.is_some() {
                        let length = (self.stems + self.stack.len() / 2).div_ceil(8).max(1);
                        let mask = code.get(at..at + length).unwrap_or(&[]).to_vec();
                        self.note(u16::from(b), &mask);
                    }
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
                5 => self.with_operands(|ctx, args| {
                    for pair in args.chunks_exact(2) {
                        ctx.x += pair[0];
                        ctx.y += pair[1];
                        ctx.line_to();
                    }
                }),
                // hlineto and vlineto alternate axes.
                6 | 7 => self.with_operands(|ctx, args| {
                    let mut horizontal = b == 6;
                    for &value in args {
                        if horizontal {
                            ctx.x += value;
                        } else {
                            ctx.y += value;
                        }
                        ctx.line_to();
                        horizontal = !horizontal;
                    }
                }),

                // rrcurveto
                8 => self.with_operands(|ctx, args| {
                    for six in args.chunks_exact(6) {
                        ctx.curve(six[0], six[1], six[2], six[3], six[4], six[5]);
                    }
                }),
                // hhcurveto: an odd operand out is the first curve's dy1.
                27 => self.with_operands(|ctx, args| {
                    let (mut dy1, rest) = match args.split_first() {
                        Some((&first, rest)) if args.len() % 4 == 1 => (first, rest),
                        _ => (0.0, args),
                    };
                    for four in rest.chunks_exact(4) {
                        ctx.curve(four[0], dy1, four[1], four[2], four[3], 0.0);
                        dy1 = 0.0;
                    }
                }),
                // vvcurveto: an odd operand out is the first curve's dx1.
                26 => self.with_operands(|ctx, args| {
                    let (mut dx1, rest) = match args.split_first() {
                        Some((&first, rest)) if args.len() % 4 == 1 => (first, rest),
                        _ => (0.0, args),
                    };
                    for four in rest.chunks_exact(4) {
                        ctx.curve(dx1, four[0], four[1], four[2], 0.0, four[3]);
                        dx1 = 0.0;
                    }
                }),
                // hvcurveto and vhcurveto alternate their starting axis.
                31 | 30 => self.with_operands(|ctx, args| {
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
                            ctx.curve(a, 0.0, bb, c, extra, d);
                        } else {
                            ctx.curve(0.0, a, bb, c, d, extra);
                        }
                        horizontal = !horizontal;
                        i += 4;
                    }
                }),

                // rcurveline
                24 => self.with_operands(|ctx, args| {
                    let curves = (args.len().saturating_sub(2)) / 6;
                    for k in 0..curves {
                        let s = k * 6;
                        if let Some(six) = args.get(s..s + 6) {
                            ctx.curve(six[0], six[1], six[2], six[3], six[4], six[5]);
                        }
                    }
                    if let Some(tail) = args.get(curves * 6..curves * 6 + 2) {
                        ctx.x += tail[0];
                        ctx.y += tail[1];
                        ctx.line_to();
                    }
                }),
                // rlinecurve
                25 => self.with_operands(|ctx, args| {
                    let lines = (args.len().saturating_sub(6)) / 2;
                    for k in 0..lines {
                        if let Some(pair) = args.get(k * 2..k * 2 + 2) {
                            ctx.x += pair[0];
                            ctx.y += pair[1];
                            ctx.line_to();
                        }
                    }
                    if let Some(six) = args.get(lines * 2..lines * 2 + 6) {
                        ctx.curve(six[0], six[1], six[2], six[3], six[4], six[5]);
                    }
                }),

                // callsubr, biased by the number the Private DICT's INDEX
                // was given when the face was parsed.
                10 => {
                    let Some(index) = self.stack.pop() else {
                        return;
                    };
                    verbatim = false;
                    self.call_taped(operand);
                    let biased = index as i32 + self.private.local_bias;
                    let subroutine = usize::try_from(biased)
                        .ok()
                        .and_then(|index| self.private.local_subrs.get(index));
                    match subroutine {
                        Some(code) => {
                            self.depth += 1;
                            self.run(code);
                            self.depth -= 1;
                        }
                        None => self.meet(Bound::MissingSubroutine),
                    }
                }
                // callgsubr, likewise for the global INDEX.
                29 => {
                    let Some(index) = self.stack.pop() else {
                        return;
                    };
                    verbatim = false;
                    self.call_taped(operand);
                    let biased = index as i32 + self.global_bias;
                    let subroutine = usize::try_from(biased)
                        .ok()
                        .and_then(|index| self.global_subrs.get(index));
                    match subroutine {
                        Some(code) => {
                            self.depth += 1;
                            self.run(code);
                            self.depth -= 1;
                        }
                        None => self.meet(Bound::MissingSubroutine),
                    }
                }
                // return. CFF2 has none: a subroutine ends where its data
                // does, and the byte is an unrecognised operator.
                11 if self.flavour == Flavour::Cff => return,
                // endchar: no operands, or `seac`'s four (5177, appendix
                // C), so a width makes one or five, and four are not one.
                // CFF2 has none either: a charstring ends where its data
                // does.
                14 if self.flavour == Flavour::Cff => {
                    self.end_taped();
                    let expected = if self.stack.len() >= 4 { 4 } else { 0 };
                    self.take_width_for(expected);
                    if self.open {
                        self.emit(Segment::Close);
                        self.open = false;
                    }
                    return;
                }
                // CFF2's vsindex and blend, which leave the stack to the
                // operator after them.
                15 if self.flavour == Flavour::Cff2 => self.select_variation_data(),
                16 if self.flavour == Flavour::Cff2 => self.blend_operands(),

                12 => {
                    let Some(&second) = code.get(at) else {
                        self.cut();
                        return;
                    };
                    at += 1;
                    if (34..=37).contains(&second) {
                        self.note(0x0C00 | u16::from(second), &[]);
                    }
                    self.escaped(second);
                }

                _ => self.stack.clear(),
            }

            operand = match self.tape.as_deref_mut() {
                Some(tape) if verbatim => {
                    let taken = (b == 28 || b >= 32) && self.stack.len() > depth;
                    tape.write(code.get(start..at), taken)
                }
                _ => None,
            };
        }
    }

    /// Flattening: a token cut by the end of its body.
    fn cut(&mut self) {
        if let Some(tape) = self.tape.as_deref_mut() {
            tape.flaw.get_or_insert(Flaw::Cut);
        }
    }

    /// Flattening: a call. Where the number it popped is the operand written
    /// immediately before it in the same body, both are written as nothing;
    /// otherwise the call is `drop` (12 18), which pops the number as the
    /// call did.
    fn call_taped(&mut self, operand: Option<usize>) {
        let Some(tape) = self.tape.as_deref_mut() else {
            return;
        };
        match operand {
            Some(at) => tape.bytes.truncate(at),
            None => {
                tape.write(Some(&[12, 18]), false);
            }
        }
    }

    /// Flattening: `endchar`, written, with `seac`'s two components where
    /// its operands are four or five: the last two, as StandardEncoding
    /// codes.
    fn end_taped(&mut self) {
        let Some(tape) = self.tape.as_deref_mut() else {
            return;
        };
        let n = self.stack.len();
        if n == 4 || n == 5 {
            let code = |at: usize| {
                self.stack
                    .get(at)
                    .and_then(|&value| u8::try_from(value as i64).ok())
            };
            if let (Some(bchar), Some(achar)) = (code(n - 2), code(n - 1)) {
                tape.seac = Some((bchar, achar));
            }
        }
        tape.write(Some(&[14]), false);
        tape.ended = true;
    }

    /// The two-byte operators: the flex operators, and in CFF 5177's
    /// arithmetic, storage and conditional operators (§4.4 to §4.6), which
    /// CFF2 does not have.
    fn escaped(&mut self, op: u8) {
        match op {
            // flex, hflex, hflex1, flex1: two curves in one operator.
            35 => self.with_operands(|ctx, a| {
                if a.len() >= 12 {
                    ctx.curve(a[0], a[1], a[2], a[3], a[4], a[5]);
                    ctx.curve(a[6], a[7], a[8], a[9], a[10], a[11]);
                }
            }),
            34 => self.with_operands(|ctx, a| {
                if a.len() >= 7 {
                    let y = ctx.y;
                    ctx.curve(a[0], 0.0, a[1], a[2], a[3], 0.0);
                    ctx.curve(a[4], 0.0, a[5], y - ctx.y, a[6], 0.0);
                }
            }),
            36 => self.with_operands(|ctx, a| {
                if a.len() >= 9 {
                    let start_y = ctx.y;
                    ctx.curve(a[0], a[1], a[2], a[3], a[4], 0.0);
                    ctx.curve(a[5], 0.0, a[6], a[7], a[8], start_y - ctx.y - a[7]);
                }
            }),
            37 => self.with_operands(|ctx, a| {
                if a.len() >= 11 {
                    let (sx, sy) = (ctx.x, ctx.y);
                    ctx.curve(a[0], a[1], a[2], a[3], a[4], a[5]);
                    let dx: f64 = a[0] + a[2] + a[4] + a[6] + a[8];
                    let dy: f64 = a[1] + a[3] + a[5] + a[7] + a[9];
                    // The last coordinate is whichever axis moved less.
                    if dx.abs() > dy.abs() {
                        ctx.curve(a[6], a[7], a[8], a[9], a[10], sy - ctx.y - a[7] - a[9]);
                    } else {
                        ctx.curve(a[6], a[7], a[8], a[9], sx - ctx.x - a[6] - a[8], a[10]);
                    }
                }
            }),
            // The arithmetic, `put` and `get` over the transient array, and the
            // conditionals, which CFF2 does not have.
            _ if self.flavour == Flavour::Cff => {
                let max = self.max_operands();
                match compute(op, self.stack, &mut self.transient, &mut self.random, max) {
                    Some(Some(bound)) => self.meet(bound),
                    Some(None) => {}
                    None => self.stack.clear(),
                }
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
        let (x, y) = (self.x, self.y);
        self.emit(Segment::CurveTo {
            c1x,
            c1y,
            c2x,
            c2y,
            x,
            y,
        });
    }

    fn push(&mut self, value: f64) {
        // The specification caps the stack at 48, and CFF2's at 513; a longer
        // one is corruption, whose excess CFF drops and CFF2 refuses.
        if !value.is_finite() {
            return;
        }
        if self.stack.len() < self.max_operands() {
            self.stack.push(value);
        } else {
            self.refuse(Stop::StackOverflow);
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
        /// The Global Subr INDEX.
        gsubrs: Vec<Vec<u8>>,
        /// `ROS`, which is what makes the font CID-keyed.
        cid: bool,
        /// The FDArray, one entry per Font DICT.
        fds: Vec<FontDict>,
        /// The FDSelect table, already encoded.
        fd_select: Vec<u8>,
        /// A Top DICT `FontMatrix`, for the CID-keyed matrix rules.
        font_matrix: Option<[i32; 6]>,
        /// Top DICT entries written first, as given: the style entries
        /// `Cff::style` reads.
        top_extra: Vec<u8>,
    }

    impl Fixture {
        fn top_dict(&self, at: &Offsets) -> Vec<u8> {
            let mut out = self.top_extra.clone();
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
            let gsubrs = wide_index(&self.gsubrs);
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
        program_with(&[
            &[14][..],
            &[250, 100, 189, 189, 21, 239, 139, 5, 139, 239, 5, 14][..],
            &[189, 217, 1, 189, 189, 21, 19, 0x80, 32, 10, 14][..],
        ])
    }

    /// A program of these charstrings, under a Private DICT whose
    /// defaultWidthX is 500 and nominalWidthX 400, with one local subroutine.
    fn program_with(glyphs: &[&[u8]]) -> Vec<u8> {
        let charstrings = index(glyphs);
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

    /// A last offset near 2^32 reaches past `usize` on a 32-bit target; the
    /// INDEX is read with its data cut at the end, in CFF's 16-bit count and
    /// CFF2's 32-bit one alike, and the item that would run past it is none.
    #[test]
    fn an_index_whose_last_offset_passes_the_data_is_cut_at_its_end() {
        let tail = [4u8, 0, 0, 0, 1, 0xFF, 0xFF, 0xFF, 0xFF, b'a', b'b'];
        let mut cff = vec![0u8, 1];
        cff.extend_from_slice(&tail);
        let (index, _) = Index::parse(&cff, 0).expect("an INDEX");
        assert_eq!(index.len(), 1);
        assert_eq!(index.get(0), None, "the item runs past the data");
        let mut cff2 = vec![0u8, 0, 0, 1];
        cff2.extend_from_slice(&tail);
        let (index, _) = Index::parse_cff2(&cff2, 0).expect("an INDEX");
        assert_eq!(index.len(), 1);
        assert_eq!(index.get(0), None);
        // And a CFF2 count past the limit is refused before any offset.
        let huge = [0u8, 1, 0, 1, 4];
        assert_eq!(
            Index::parse_cff2(&huge, 0).err(),
            Some(IndexRefusal::Count(65_537))
        );
        assert_eq!(
            Index::parse_cff2(&[0, 0, 0], 0).err(),
            Some(IndexRefusal::Truncated)
        );
        let (empty, next) = Index::parse_cff2(&[0, 0, 0, 0], 0).expect("an empty INDEX");
        assert_eq!((empty.len(), next), (0, 4), "four bytes long");
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

    /// 5177, appendix C: `endchar` may carry `seac`'s four operands, so a
    /// width before them makes five, and four carry none. Reading the first
    /// of four as a width gave an accented letter its accent's horizontal
    /// offset over nominalWidthX for an advance.
    #[test]
    fn endchar_with_seacs_four_operands_carries_no_width() {
        // `50 0 65 193 endchar`: adx ady bchar achar, the codes of A and grave.
        let seac: &[u8] = &[189, 139, 204, 247, 85, 14];
        let program = program_with(&[
            &[14][..],
            seac,
            &[&[215][..], seac].concat(),
            &[215, 14][..],
        ]);
        let cff = Cff::parse(&program).expect("a CFF font");
        assert_eq!(cff.advance(1), Some(500.0), "four operands: defaultWidthX");
        assert_eq!(
            cff.advance(2),
            Some(476.0),
            "five: nominalWidthX and the first"
        );
        assert_eq!(cff.advance(3), Some(476.0), "one: nominalWidthX and it");
        assert!(cff.outline(1).expect("glyph 1").is_empty());
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

    /// **A CID-keyed font names its collection**: `ROS`'s registry and
    /// ordering through the string INDEX, here `Adobe` and `Identity`; none
    /// where `ROS` names a string the font does not hold, and none for a
    /// font that is not CID-keyed.
    #[test]
    fn a_cid_keyed_font_names_its_collection() {
        let font = cid_fixture(vec![0, 0, 0, 1, 1]).build();
        let cff = Cff::parse(&font).expect("a CID-keyed font");
        assert_eq!(cff.registry_ordering(), Some(("Adobe", "Identity")));

        let short = Fixture {
            strings: vec![b"Adobe".to_vec()],
            ..cid_fixture(vec![0, 0, 0, 1, 1])
        }
        .build();
        let cff = Cff::parse(&short).expect("a CID-keyed font missing a string");
        assert!(cff.is_cid());
        assert_eq!(cff.registry_ordering(), None, "SID 392 names nothing");

        let named = abc(Table::Bytes(charset_0(&ABC))).build();
        let cff = Cff::parse(&named).expect("a CFF font");
        assert_eq!(cff.registry_ordering(), None, "not CID-keyed");
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

    /// Every CFF program the repository commits, by file name: the
    /// `fuzz/corpus/cff` seeds as they are, and the `CFF ` table of every
    /// OpenType face the shaping fixtures and the `sfnt` seeds carry — the set
    /// MS-04's fonts group outlines. `None` where this checkout has no such
    /// tree, as a published crate does not.
    fn committed_programs() -> Option<Vec<(String, Vec<u8>)>> {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let mut out = Vec::new();
        for (dir, extension, whole) in [
            ("../../fuzz/corpus/cff", "cff", true),
            ("../tinker-pdf-shape/data/aots/fonts", "otf", false),
            (
                "../tinker-pdf-shape/data/text-rendering-tests/fonts",
                "otf",
                false,
            ),
            ("../../fuzz/corpus/sfnt", "otf", false),
        ] {
            let mut paths: Vec<std::path::PathBuf> = std::fs::read_dir(root.join(dir))
                .ok()?
                .flatten()
                .map(|entry| entry.path())
                .filter(|path| path.extension().is_some_and(|e| e == extension))
                .collect();
            paths.sort();
            for path in paths {
                let bytes = std::fs::read(&path).ok()?;
                let program = if whole {
                    bytes
                } else {
                    let table = crate::sfnt::Sfnt::parse(&bytes).and_then(|face| {
                        face.table(u32::from_be_bytes(*b"CFF ")).map(<[u8]>::to_vec)
                    });
                    match table {
                        Some(table) => table,
                        None => continue,
                    }
                };
                out.push((path.file_name()?.to_string_lossy().into_owned(), program));
            }
        }
        Some(out)
    }

    /// The width a glyph's whole charstring settles, run to its end with its
    /// outline built: the answer [`Cff::advance`] has to give having stopped
    /// at the width.
    fn width_of_whole_run(cff: &Cff<'_>, glyph: u16) -> Option<f64> {
        let charstring = cff.charstrings.get(usize::from(glyph))?;
        let mut outline = Outline::default();
        let mut stack = Vec::new();
        let mut ctx = cff.charstring_context(glyph, Some(&mut outline), &mut stack);
        ctx.run(charstring);
        Some(ctx.width.unwrap_or(ctx.private.default_width))
    }

    /// Every glyph of `cff` read through one machine and one outline, forwards
    /// and then backwards, against [`Cff::outline`]'s fresh pair, segment for
    /// segment; and every advance against the whole run's width. How many
    /// glyphs that was.
    fn assert_reused_path_is_fresh_path(cff: &Cff<'_>, name: &str) -> usize {
        let count = u16::try_from(cff.glyph_count()).expect("an INDEX holds at most 65 535 items");
        let fresh: Vec<(Outline, u64)> = (0..count)
            .map(|glyph| {
                let (outline, steps) = cff.outline_counted(glyph);
                (outline.expect("every glyph below the count"), steps)
            })
            .collect();
        let made = counters::machines();
        let mut machine = Machine::new();
        let mut outline = Outline::default();
        // Forwards and then backwards: whatever one glyph leaves behind, the
        // next one in either order would read.
        for glyph in (0..count).chain((0..count).rev()) {
            let (expected, steps) = &fresh[usize::from(glyph)];
            assert_eq!(
                cff.interpret(glyph, &mut outline, &mut machine),
                Some(*steps),
                "{name}, glyph {glyph}: the budget it spent"
            );
            assert_eq!(
                outline.segments, expected.segments,
                "{name}, glyph {glyph}: the reused path drew otherwise"
            );
        }
        // A glyph past the last, through the same pair, is refused and leaves
        // nothing behind of the one before it.
        assert!(!cff.outline_into(count, &mut outline, &mut machine));
        assert!(outline.is_empty(), "{name}: the refused glyph's outline");
        assert_eq!(
            counters::machines() - made,
            1,
            "{name}: one machine read every glyph"
        );
        for glyph in 0..count {
            assert_eq!(
                cff.advance(glyph),
                width_of_whole_run(cff, glyph),
                "{name}, glyph {glyph}: the advance read as far as its width"
            );
        }
        usize::from(count)
    }

    /// P-26.1: the reused buffers draw what fresh ones draw, over every glyph
    /// of every committed CFF program, and one machine reads a whole face.
    #[test]
    fn every_glyph_interprets_into_one_reused_buffer() {
        let Some(programs) = committed_programs() else {
            println!("cff-reused-buffer: SKIPPED (no fuzz or shaping fixture tree)");
            return;
        };
        let (mut faces, mut glyphs) = (0usize, 0usize);
        for (name, program) in &programs {
            let Some(cff) = Cff::parse(program) else {
                continue;
            };
            faces += 1;
            glyphs += assert_reused_path_is_fresh_path(&cff, name);
        }
        assert!(
            programs.iter().any(|(name, _)| name.ends_with(".cff")),
            "the fuzz seeds were read"
        );
        assert!(faces > programs.len() / 2, "most programs parse");
        assert!(glyphs > 0, "no glyph was read");
        println!(
            "RAN cff-reused-buffer: {faces} of {} programs, {glyphs} glyphs",
            programs.len()
        );

        // The built fixtures too: local subroutines, a CID-keyed font whose
        // Font DICTs each have their own, and `seac`.
        for (name, program) in [
            ("three glyphs", three_glyph_program()),
            ("CID-keyed", cid_fixture(vec![0, 0, 0, 1, 1]).build()),
            ("named", named_glyphs().build()),
            (
                "seac",
                program_with(&[&[14][..], &[189, 139, 204, 247, 85, 14][..]]),
            ),
        ] {
            let cff = Cff::parse(&program).expect(name);
            assert_reused_path_is_fresh_path(&cff, name);
        }
    }

    /// P-26.1: the biases are resolved when the face is parsed, one per
    /// subroutine INDEX, and never again however many glyphs are read.
    #[test]
    fn the_bias_is_resolved_once_per_face() {
        // Read every glyph by every road there is, and say how many biases
        // that resolved.
        let read_everything = |cff: &Cff<'_>| {
            let before = counters::biases();
            let mut machine = Machine::new();
            let mut outline = Outline::default();
            for glyph in 0..u16::try_from(cff.glyph_count()).unwrap_or(u16::MAX) {
                let _ = cff.outline(glyph);
                let _ = cff.outline_counted(glyph);
                let _ = cff.outline_into(glyph, &mut outline, &mut machine);
                let _ = cff.advance(glyph);
            }
            counters::biases() - before
        };

        // Exact counts where the INDEXes are known: the empty global INDEX and
        // the one Private DICT's Subrs.
        let program = three_glyph_program();
        let before = counters::biases();
        let cff = Cff::parse(&program).expect("a CFF font");
        assert_eq!(counters::biases() - before, 2, "global and local");
        assert_eq!(read_everything(&cff), 0, "no glyph resolves a bias");

        // A CID-keyed font: the global INDEX and each Font DICT's local one;
        // the top-level Private DICT carries no Subrs, so it has none to
        // resolve.
        let program = cid_fixture(vec![0, 0, 0, 1, 1]).build();
        let before = counters::biases();
        let cff = Cff::parse(&program).expect("a CID-keyed font");
        assert_eq!(counters::biases() - before, 3, "global and two Font DICTs");
        assert_eq!(read_everything(&cff), 0, "no glyph resolves a bias");

        let Some(programs) = committed_programs() else {
            println!("cff-bias-once: SKIPPED (no fuzz or shaping fixture tree)");
            return;
        };
        let mut faces = 0usize;
        for (name, program) in &programs {
            let before = counters::biases();
            let Some(cff) = Cff::parse(program) else {
                continue;
            };
            faces += 1;
            // The global INDEX, the top-level Private DICT's and each Font
            // DICT's: at most one each.
            let indexes = 2 + cff.fds.len() as u64;
            let parsed = counters::biases() - before;
            assert!(
                parsed <= indexes,
                "{name}: {parsed} biases for {indexes} subroutine INDEXes"
            );
            assert_eq!(read_everything(&cff), 0, "{name}: a glyph resolved a bias");
        }
        println!("RAN cff-bias-once: {faces} programs");
    }

    /// A bias stored at parse is the one its own INDEX needs: past 1 239
    /// subroutines a call to subroutine 0 is the operand -1131, and only that
    /// bias reaches it. The global and the local INDEX are counted apart, so
    /// each call is biased by its own.
    #[test]
    fn each_index_is_biased_by_its_own_count() {
        // -1131, in the 16-bit operand form.
        let mut call = vec![28u8];
        call.extend_from_slice(&(-1131i16).to_be_bytes());
        let many = |first: Vec<u8>| {
            let mut subrs = vec![first];
            subrs.extend(std::iter::repeat_with(|| vec![11u8]).take(1239));
            subrs
        };
        for (op, local) in [(10u8, true), (29, false)] {
            let mut glyph = call.clone();
            glyph.extend_from_slice(&[op, 14]);
            let fixture = Fixture {
                charstrings: vec![vec![14], glyph],
                private: private_dict(500, 400, local),
                subrs: if local {
                    many(box_glyph(300))
                } else {
                    Vec::new()
                },
                gsubrs: if local {
                    Vec::new()
                } else {
                    many(box_glyph(300))
                },
                ..Fixture::default()
            };
            let program = fixture.build();
            let cff = Cff::parse(&program).expect("a CFF font");
            let outline = cff.outline(1).expect("glyph 1");
            assert_eq!(
                outline.bounds(),
                Some((0.0, 0.0, 300.0, 300.0)),
                "callsubr {op} reached subroutine 0 through a bias of 1131"
            );
        }
        // And the small INDEX beside a large one keeps 107: a local call of
        // -107 under 1 240 global subroutines reaches local subroutine 0.
        let fixture = Fixture {
            charstrings: vec![vec![14], subr_glyph()],
            private: private_dict(500, 400, true),
            subrs: vec![box_glyph(200)],
            gsubrs: many(vec![11]),
            ..Fixture::default()
        };
        let program = fixture.build();
        let cff = Cff::parse(&program).expect("a CFF font");
        assert_eq!(
            cff.outline(1).and_then(|outline| outline.bounds()),
            Some((0.0, 0.0, 200.0, 200.0))
        );
    }

    /// A reused machine hands nothing on: not the operands a glyph left on the
    /// stack, not the transient array it wrote, not the budget it spent or the
    /// depth it reached.
    #[test]
    fn a_glyph_hands_nothing_to_the_next() {
        let fixture = Fixture {
            charstrings: vec![
                vec![14],
                // Three operands pushed and never consumed.
                vec![139, 139, 139],
                // `rlineto` with nothing pushed: no segment, unless the
                // operands above were carried in.
                vec![5, 14],
                // `5 0 put`, writing the transient array.
                vec![144, 139, 12, 20, 14],
                // `0 get 0 rlineto`: a line to x = whatever slot 0 holds.
                vec![139, 12, 21, 139, 5, 14],
                // Subroutine 0 calls itself four times a level, so this runs
                // the budget dry at the depth cap.
                vec![32, 10, 14],
                box_glyph(100),
            ],
            private: private_dict(500, 400, true),
            subrs: vec![vec![32, 10, 32, 10, 32, 10, 32, 10, 11]],
            ..Fixture::default()
        };
        let program = fixture.build();
        let cff = Cff::parse(&program).expect("a CFF font");
        assert_reused_path_is_fresh_path(&cff, "leftovers");

        let mut machine = Machine::new();
        let mut outline = Outline::default();
        for glyph in [1u16, 2] {
            assert!(cff.outline_into(glyph, &mut outline, &mut machine));
        }
        assert!(outline.is_empty(), "the stack carried glyph 1's operands");
        for glyph in [3u16, 4] {
            assert!(cff.outline_into(glyph, &mut outline, &mut machine));
        }
        assert_eq!(
            outline.segments,
            [Segment::LineTo { x: 0.0, y: 0.0 }],
            "the transient array carried glyph 3's write"
        );
        let (_, spent) = cff.outline_counted(5);
        assert_eq!(
            spent,
            u64::from(CHARSTRING_BUDGET),
            "glyph 5 spends the whole budget"
        );
        for glyph in [5u16, 6] {
            assert!(cff.outline_into(glyph, &mut outline, &mut machine));
        }
        assert_eq!(
            outline.bounds(),
            Some((0.0, 0.0, 100.0, 100.0)),
            "the box after a spent budget is drawn whole"
        );
    }

    /// `advance` stops at the operator that settles the width, and the width
    /// it reads is the one the whole run settles, wherever the operator is:
    /// first, inside a subroutine, after drawing operators a hostile glyph put
    /// before it, at `endchar` with `seac`'s operands, or nowhere at all.
    #[test]
    fn advance_reads_as_far_as_the_width_and_no_further() {
        let fixture = Fixture {
            charstrings: vec![
                vec![14],
                // `11 0 0 rmoveto`: the width is the odd operand out.
                vec![150, 139, 139, 21, 239, 139, 5, 14],
                // The width set by `hstem` inside subroutine 0.
                vec![150, 32, 10, 239, 139, 5, 14],
                // A line first, then `11 0 0 rmoveto`.
                vec![239, 139, 5, 150, 139, 139, 21, 14],
                // No stack-clearing operator at all.
                vec![239, 139, 5],
                // `seac`'s four, carrying no width.
                vec![189, 139, 204, 247, 85, 14],
                // Nothing but `endchar`.
                vec![14],
            ],
            private: private_dict(500, 400, true),
            subrs: vec![vec![239, 239, 1, 11]],
            ..Fixture::default()
        };
        let program = fixture.build();
        let cff = Cff::parse(&program).expect("a CFF font");
        for (glyph, width) in [
            (1u16, 411.0),
            (2, 411.0),
            (3, 411.0),
            (4, 500.0),
            (5, 500.0),
            (6, 500.0),
        ] {
            assert_eq!(cff.advance(glyph), Some(width), "glyph {glyph}");
            assert_eq!(
                width_of_whole_run(&cff, glyph),
                Some(width),
                "glyph {glyph}"
            );
        }
        assert_eq!(cff.advance(7), None, "a glyph the font does not have");
    }

    /// Hostile charstrings, generated: bytes drawn at random, operators and
    /// operands alike, under subroutines that call each other. Whatever they
    /// do, the reused path draws what the fresh one draws and the advance is
    /// the whole run's.
    #[test]
    fn generated_charstrings_read_alike_through_either_path() {
        // A fixed linear congruential sequence, so a failure reproduces.
        let mut state = 0x2545_F491_4F6C_DD1Du64;
        let mut next = move || {
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            (state >> 33) as u8
        };
        let mut random = |len: usize| -> Vec<u8> { (0..len).map(|_| next()).collect() };
        let charstrings: Vec<Vec<u8>> = (0..300).map(|n| random(n % 61)).collect();
        // Subroutines a call of -107 to -104 reaches, each drawing or calling.
        let subrs: Vec<Vec<u8>> = (0..4).map(|n| random(8 + n * 5)).collect();
        let gsubrs: Vec<Vec<u8>> = (0..4).map(|n| random(6 + n * 7)).collect();
        let fixture = Fixture {
            charstrings,
            private: private_dict(500, 400, true),
            subrs,
            gsubrs,
            ..Fixture::default()
        };
        let program = fixture.build();
        let cff = Cff::parse(&program).expect("a CFF font of random charstrings");
        assert_eq!(assert_reused_path_is_fresh_path(&cff, "generated"), 300);
    }

    /// An `OTTO` face around a CFF program, with `vhea`, `vmtx` and, where
    /// given, a `VORG`.
    fn vertical_cff_face(program: &[u8], vorg: Option<&[u8]>) -> Vec<u8> {
        let mut head = vec![0u8; 54];
        head[18..20].copy_from_slice(&1000u16.to_be_bytes());
        let mut maxp = vec![0u8; 6];
        maxp[0..4].copy_from_slice(&0x0000_5000u32.to_be_bytes());
        maxp[4..6].copy_from_slice(&3u16.to_be_bytes());
        let mut vhea = vec![0u8; 36];
        vhea[0..4].copy_from_slice(&0x0001_1000u32.to_be_bytes());
        vhea[34..36].copy_from_slice(&3u16.to_be_bytes());
        let mut vmtx = Vec::new();
        for bearing in [30i16, 40, 50] {
            vmtx.extend_from_slice(&1000u16.to_be_bytes());
            vmtx.extend_from_slice(&bearing.to_be_bytes());
        }
        let mut tables: Vec<(&[u8; 4], &[u8])> = vec![(b"CFF ", program)];
        if let Some(vorg) = vorg {
            tables.push((b"VORG", vorg));
        }
        tables.extend([
            (b"head", head.as_slice()),
            (b"maxp", maxp.as_slice()),
            (b"vhea", vhea.as_slice()),
            (b"vmtx", vmtx.as_slice()),
        ]);
        let mut out = Vec::new();
        out.extend_from_slice(&0x4F54_544Fu32.to_be_bytes()); // OTTO
        out.extend_from_slice(&(tables.len() as u16).to_be_bytes());
        out.extend_from_slice(&[0; 6]);
        let mut offset = 12 + tables.len() * 16;
        let mut body = Vec::new();
        for (tag, data) in &tables {
            out.extend_from_slice(*tag);
            out.extend_from_slice(&0u32.to_be_bytes());
            out.extend_from_slice(&(offset as u32).to_be_bytes());
            out.extend_from_slice(&(data.len() as u32).to_be_bytes());
            offset += data.len();
            body.extend_from_slice(data);
        }
        out.extend_from_slice(&body);
        out
    }

    /// **A CFF face's vertical origin**: `VORG`'s where it has one, and the
    /// `vmtx` clause's top side bearing plus the top of the box where it does
    /// not — the box being what the charstring draws.
    ///
    /// [`three_glyph_program`]'s glyph 1 moves to (50, 50) and draws up to
    /// (150, 150), so its top is 150 and its origin 150 plus its bearing of
    /// 40; glyph 0 draws nothing, so its box is its origin and its origin its
    /// bearing of 30. Glyph 2 ends in a subroutine call and is measured the
    /// way it is drawn. With a `VORG`, every glyph takes the table's word.
    #[test]
    fn a_cff_face_takes_its_vertical_origin_from_vorg_or_its_outline() {
        let program = three_glyph_program();
        let data = vertical_cff_face(&program, None);
        let face = crate::Sfnt::parse(&data).expect("an OTTO face");
        assert_eq!(
            face.vertical_origin(0),
            Some(30),
            "an empty glyph: its bearing"
        );
        assert_eq!(
            face.vertical_origin(1),
            Some(190),
            "a top of 150 and a bearing of 40"
        );
        let drawn = Cff::parse(&program)
            .and_then(|cff| cff.outline(2))
            .and_then(|outline| outline.bounds())
            .map(|(_, _, _, top)| top.round() as i32)
            .expect("glyph 2 draws");
        assert_eq!(face.vertical_origin(2), Some(drawn + 50));
        assert_eq!(
            face.vertical_origin(3),
            None,
            "no side bearing past maxp's count"
        );

        let mut vorg = Vec::new();
        for value in [1u16, 0, 880, 1, 1] {
            vorg.extend_from_slice(&value.to_be_bytes());
        }
        vorg.extend_from_slice(&861i16.to_be_bytes());
        let data = vertical_cff_face(&program, Some(&vorg));
        let face = crate::Sfnt::parse(&data).expect("an OTTO face");
        let origins: Vec<Option<i32>> = (0..4).map(|g| face.vertical_origin(g)).collect();
        assert_eq!(
            origins,
            [Some(880), Some(861), Some(880), Some(880)],
            "VORG's record for glyph 1 and its default for the rest"
        );
    }

    /// A Type 2 operand in the three-byte form, which holds every value these
    /// tests write.
    fn short(value: i16) -> Vec<u8> {
        let mut out = vec![28u8];
        out.extend_from_slice(&value.to_be_bytes());
        out
    }

    /// `12 op` run over `stack` with a fresh transient array and sequence:
    /// the stack it leaves, having met no bound.
    #[track_caller]
    fn computed(op: u8, stack: &[f64]) -> Vec<f64> {
        let mut stack = stack.to_vec();
        let mut transient = [0.0; TRANSIENT_ELEMENTS];
        let mut random = Random::new();
        let ran = compute(op, &mut stack, &mut transient, &mut random, MAX_OPERANDS);
        assert_eq!(
            ran,
            Some(None),
            "12 {op} is one of 5177's and meets no bound"
        );
        stack
    }

    /// 5177 §4.4 to §4.6: every operator against results worked here, the
    /// cases 5177 leaves undefined as `docs/design/subsetting.md` settles
    /// them, and the operands below the ones an operator takes left alone.
    #[test]
    fn each_arithmetic_operator_computes_what_the_specification_says() {
        // abs and neg.
        assert_eq!(computed(9, &[7.0, -3.5]), [7.0, 3.5]);
        assert_eq!(computed(9, &[2.0]), [2.0]);
        assert_eq!(computed(14, &[7.0, 3.5]), [7.0, -3.5]);
        assert_eq!(computed(14, &[-2.0]), [2.0]);
        // add, sub, mul and div, num1 the deeper of the two.
        assert_eq!(computed(10, &[1.0, 7.0, 3.5]), [1.0, 10.5]);
        assert_eq!(computed(11, &[7.0, 3.5]), [3.5]);
        assert_eq!(computed(11, &[3.5, 7.0]), [-3.5]);
        assert_eq!(computed(24, &[-6.0, 2.5]), [-15.0]);
        assert_eq!(computed(12, &[7.0, 2.0]), [3.5]);
        assert_eq!(computed(12, &[-1.0, 4.0]), [-0.25]);
        // A zero divisor gives 0; an overflow is 0, and so is a result
        // below the least normal double.
        assert_eq!(computed(12, &[7.0, 0.0]), [0.0]);
        assert_eq!(computed(24, &[1e200, 1e200]), [0.0]);
        assert_eq!(computed(10, &[f64::MAX, f64::MAX]), [0.0]);
        assert_eq!(computed(12, &[1e-300, 1e10]), [0.0]);
        // sqrt: the root, and 0 for a negative operand.
        assert_eq!(computed(26, &[6.25]), [2.5]);
        assert_eq!(computed(26, &[0.0]), [0.0]);
        assert_eq!(computed(26, &[-4.0]), [0.0]);
        // drop, exch and dup.
        assert_eq!(computed(18, &[1.0, 2.0]), [1.0]);
        assert_eq!(computed(28, &[1.0, 2.0, 3.0]), [1.0, 3.0, 2.0]);
        assert_eq!(computed(27, &[1.0, 2.0]), [1.0, 2.0, 2.0]);
        // index: element i counted from the top below i; a negative i copies
        // the top (5177), a fraction is truncated toward zero, and an i past
        // the bottom copies the bottom.
        let three = [10.0, 20.0, 30.0];
        for (i, copied) in [
            (0.0, 30.0),
            (1.0, 20.0),
            (2.0, 10.0),
            (1.9, 20.0),
            (-1.0, 30.0),
            (-7.5, 30.0),
            (9.0, 10.0),
        ] {
            assert_eq!(
                computed(29, &[&three[..], &[i]].concat()),
                [&three[..], &[copied]].concat(),
                "{i} index"
            );
        }
        // roll: `a b c 3 1 roll` is `c a b`, 5177's picture; J is taken
        // modulo N, N and J are truncated toward zero, an N past the stack is
        // the stack, and an N of zero or less turns nothing.
        for (n, j, rolled) in [
            (3.0, 1.0, [30.0, 10.0, 20.0]),
            (3.0, -1.0, [20.0, 30.0, 10.0]),
            (3.0, 4.0, [30.0, 10.0, 20.0]),
            (3.0, 3.0, [10.0, 20.0, 30.0]),
            (2.0, 1.0, [10.0, 30.0, 20.0]),
            (7.0, 1.0, [30.0, 10.0, 20.0]),
            (2.9, 1.5, [10.0, 30.0, 20.0]),
            (0.0, 1.0, [10.0, 20.0, 30.0]),
            (-2.0, 1.0, [10.0, 20.0, 30.0]),
        ] {
            assert_eq!(
                computed(30, &[&three[..], &[n, j]].concat()),
                rolled,
                "{n} {j} roll"
            );
        }
        // and, or, not and eq answer 1 or 0.
        assert_eq!(computed(3, &[2.0, -1.0]), [1.0]);
        assert_eq!(computed(3, &[2.0, 0.0]), [0.0]);
        assert_eq!(computed(4, &[0.0, 0.0]), [0.0]);
        assert_eq!(computed(4, &[0.0, 0.5]), [1.0]);
        assert_eq!(computed(5, &[0.0]), [1.0]);
        assert_eq!(computed(5, &[3.0]), [0.0]);
        assert_eq!(computed(15, &[2.5, 2.5]), [1.0]);
        assert_eq!(computed(15, &[2.5, 2.0]), [0.0]);
        // ifelse: `s1 s2 v1 v2`, s1 where v1 is at most v2.
        assert_eq!(computed(22, &[9.0, 10.0, 20.0, 1.0, 1.0]), [9.0, 10.0]);
        assert_eq!(computed(22, &[10.0, 20.0, 1.0, 2.0]), [10.0]);
        assert_eq!(computed(22, &[10.0, 20.0, 2.0, 1.0]), [20.0]);
        // random: xorshift's first state from 2 463 534 242 is 723 471 715,
        // 0x2B1F_4D63, so the first value is (0x2B1F + 1) / 65 536.
        assert_eq!(computed(23, &[5.0]), [5.0, 11_040.0 / 65_536.0]);
        // Short of its operands, an operator leaves the stack as it is.
        for (op, few) in [
            (9u8, &[][..]),
            (14, &[][..]),
            (10, &[1.0][..]),
            (12, &[1.0][..]),
            (27, &[][..]),
            (29, &[1.0][..]),
            (30, &[3.0][..]),
            (22, &[1.0, 2.0, 3.0][..]),
        ] {
            assert_eq!(computed(op, few), few, "12 {op}");
        }
        // Every other escaped code is none of them, and is the caller's.
        for op in [
            0u8, 1, 2, 6, 7, 8, 13, 16, 17, 19, 25, 31, 34, 35, 36, 37, 38, 255,
        ] {
            let mut stack = vec![1.0, 2.0];
            let mut transient = [0.0; TRANSIENT_ELEMENTS];
            let ran = compute(
                op,
                &mut stack,
                &mut transient,
                &mut Random::new(),
                MAX_OPERANDS,
            );
            assert_eq!(ran, None, "12 {op}");
            assert_eq!(stack, [1.0, 2.0], "12 {op} leaves the stack");
        }
    }

    /// `put` and `get` share one transient array across a run. An index is
    /// truncated toward zero, a negative one is element 0, and one past the
    /// array is ignored by `put` and read as 0 by `get`; either outside
    /// 5177's 0 to 31 is a bound met.
    #[test]
    fn put_and_get_keep_the_transient_array() {
        let mut transient = [0.0; TRANSIENT_ELEMENTS];
        let mut random = Random::new();
        let mut stack = vec![42.5, 31.0];
        let ran = compute(20, &mut stack, &mut transient, &mut random, MAX_OPERANDS);
        assert_eq!(ran, Some(None));
        assert!(stack.is_empty(), "put pops both");
        assert_eq!(transient[31], 42.5);
        let mut stack = vec![31.9];
        let ran = compute(21, &mut stack, &mut transient, &mut random, MAX_OPERANDS);
        assert_eq!((ran, stack), (Some(None), vec![42.5]));
        let mut stack = vec![7.0];
        let ran = compute(21, &mut stack, &mut transient, &mut random, MAX_OPERANDS);
        assert_eq!(
            (ran, stack),
            (Some(None), vec![0.0]),
            "an element no put set"
        );

        let before = transient;
        let mut stack = vec![1.0, 32.0];
        let ran = compute(20, &mut stack, &mut transient, &mut random, MAX_OPERANDS);
        assert_eq!(ran, Some(Some(Bound::TransientIndex)));
        assert!(stack.is_empty());
        assert_eq!(transient, before, "a put past the array writes nothing");
        let mut stack = vec![32.0];
        let ran = compute(21, &mut stack, &mut transient, &mut random, MAX_OPERANDS);
        assert_eq!((ran, stack), (Some(Some(Bound::TransientIndex)), vec![0.0]));

        let mut stack = vec![5.0, -3.0];
        let ran = compute(20, &mut stack, &mut transient, &mut random, MAX_OPERANDS);
        assert_eq!(ran, Some(Some(Bound::TransientIndex)));
        assert_eq!(transient[0], 5.0, "a negative index is element 0");
        let mut stack = vec![-0.5];
        let ran = compute(21, &mut stack, &mut transient, &mut random, MAX_OPERANDS);
        assert_eq!(
            (ran, stack),
            (Some(None), vec![5.0]),
            "-0.5 truncates to element 0, inside the array"
        );

        // Short of operands, each pops what there is.
        let mut stack = vec![3.0];
        let ran = compute(20, &mut stack, &mut transient, &mut random, MAX_OPERANDS);
        assert_eq!((ran, stack.len()), (Some(None), 0));
        let mut stack = Vec::new();
        let ran = compute(21, &mut stack, &mut transient, &mut random, MAX_OPERANDS);
        assert_eq!((ran, stack.len()), (Some(None), 0));
    }

    /// A `callsubr` or `callgsubr` operand the charstring computes calls the
    /// subroutine it computes. Every subroutine draws a line of its own
    /// length, so the outline names the one that ran.
    #[test]
    fn a_computed_subroutine_number_calls_that_subroutine() {
        let line = |dx: i16| [short(dx), short(0), vec![5, 11]].concat();
        let start = [short(0), short(0), vec![21]].concat();
        // `-100 3 add callsubr`: -97, which an INDEX of 200 biases by 107 to
        // local subroutine 10.
        let local = [start.clone(), short(-100), short(3), vec![12, 10, 10, 14]].concat();
        // `-53 2 mul callgsubr`: -106, which an INDEX of four biases to
        // global subroutine 1.
        let global = [start, short(-53), short(2), vec![12, 24, 29, 14]].concat();
        let fixture = Fixture {
            charstrings: vec![vec![14], local, global],
            private: private_dict(500, 400, true),
            subrs: (0..200).map(|i| line(100 + i)).collect(),
            gsubrs: (0..4).map(|i| line(-(50 + i))).collect(),
            ..Fixture::default()
        };
        let program = fixture.build();
        let cff = Cff::parse(&program).expect("a CFF font");
        let drawn = |glyph: u16| cff.outline(glyph).expect("the glyph").segments;
        assert_eq!(
            drawn(1),
            [
                Segment::MoveTo { x: 0.0, y: 0.0 },
                Segment::LineTo { x: 110.0, y: 0.0 },
                Segment::Close,
            ],
            "local subroutine 10's line"
        );
        assert_eq!(
            drawn(2),
            [
                Segment::MoveTo { x: 0.0, y: 0.0 },
                Segment::LineTo { x: -51.0, y: 0.0 },
                Segment::Close,
            ],
            "global subroutine 1's line"
        );
        assert_eq!((cff.first_bound(1), cff.first_bound(2)), (None, None));
    }

    /// `random` draws one fixed sequence, and each glyph's run starts it
    /// afresh: a charstring draws the same lines whichever glyph ran before
    /// it, through one machine or a fresh one.
    #[test]
    fn random_draws_one_sequence_that_each_glyph_restarts() {
        // `0 0 rmoveto`, then twice `random 256 mul 256 mul 0 rlineto`: lines
        // as long as the sequence's 16.16 numerators.
        let mut code = [short(0), short(0), vec![21]].concat();
        for _ in 0..2 {
            code.extend([12, 23]);
            code.extend(short(256));
            code.extend([12, 24]);
            code.extend(short(256));
            code.extend([12, 24]);
            code.extend(short(0));
            code.push(5);
        }
        code.push(14);
        let fixture = Fixture {
            charstrings: vec![vec![14], code.clone(), code],
            private: private_dict(500, 400, false),
            ..Fixture::default()
        };
        let program = fixture.build();
        let cff = Cff::parse(&program).expect("a CFF font");
        // The first two states from 2 463 534 242 are 0x2B1F_4D63 and
        // 0x94DA_CB7A: 11 040 and 38 107 over 65 536.
        let expected = [
            Segment::MoveTo { x: 0.0, y: 0.0 },
            Segment::LineTo {
                x: 11_040.0,
                y: 0.0,
            },
            Segment::LineTo {
                x: 49_147.0,
                y: 0.0,
            },
            Segment::Close,
        ];
        let mut machine = Machine::new();
        let mut outline = Outline::default();
        for glyph in [1u16, 2, 1, 2] {
            assert!(cff.outline_into(glyph, &mut outline, &mut machine));
            assert_eq!(outline.segments, expected, "glyph {glyph}, reused");
            assert_eq!(
                cff.outline(glyph).expect("the glyph").segments,
                expected,
                "glyph {glyph}, fresh"
            );
        }
    }

    /// The first bound a glyph's run meets is recorded by name, and CFF
    /// draws on past it.
    #[test]
    fn the_first_bound_a_run_meets_is_recorded() {
        let fixture = Fixture {
            charstrings: vec![
                vec![14],
                // 49 operands, one past the stack.
                [vec![139; 49], vec![14]].concat(),
                // 48 operands and `dup`, which has no room for the copy.
                [vec![139; 48], vec![12, 27, 14]].concat(),
                // `1 32 put`: past the transient array.
                vec![140, 171, 12, 20, 14],
                // `-1 get`: below it.
                vec![138, 12, 21, 14],
                // Subroutine 0 calls itself until the depth cap.
                vec![32, 10, 14],
                // `-106 callsubr`: subroutine 1, which an INDEX of one does
                // not hold.
                vec![33, 10, 14],
                // 64 001 operators, past the budget.
                [vec![5u8; 64_001], vec![14]].concat(),
                // `50 50 add 0 rlineto`, within every bound.
                vec![189, 189, 12, 10, 139, 5, 14],
            ],
            private: private_dict(500, 400, true),
            subrs: vec![vec![32, 10, 11]],
            ..Fixture::default()
        };
        let program = fixture.build();
        let cff = Cff::parse(&program).expect("a CFF font");
        let met: Vec<Option<Bound>> = (0..10).map(|glyph| cff.first_bound(glyph)).collect();
        assert_eq!(
            met,
            [
                None,
                Some(Bound::StackOverflow),
                Some(Bound::StackOverflow),
                Some(Bound::TransientIndex),
                Some(Bound::TransientIndex),
                Some(Bound::SubroutineDepth),
                Some(Bound::MissingSubroutine),
                Some(Bound::Budget),
                None,
                None,
            ]
        );
        assert_eq!(
            cff.outline(8).expect("glyph 8").segments,
            [Segment::LineTo { x: 100.0, y: 0.0 }]
        );
        assert!(cff.outline(1).is_some(), "a glyph past a bound still draws");
    }

    /// CFF2 has none of 5177's arithmetic (CFF2 chapter, 'CharString
    /// operators'), and its machine clears the stack at each.
    #[test]
    fn a_cff2_charstring_does_not_compute() {
        // `50 50 add 0 rlineto`: a line 100 long in CFF; in CFF2 `add`
        // clears both operands and `rlineto` finds one.
        let code = [189u8, 189, 12, 10, 139, 5];
        let global = Index::default();
        let private = Private::default();
        let draw = |flavour: Flavour| {
            let mut outline = Outline::default();
            let mut stack = Vec::new();
            let subroutines = Subroutines {
                global: &global,
                global_bias: EMPTY_INDEX_BIAS,
                private: &private,
            };
            let stop = Charstring::new(flavour, subroutines, Some(&mut outline), &mut stack, None)
                .run_glyph(&code);
            assert_eq!(stop, None, "{flavour:?} runs to the end");
            outline.segments
        };
        assert_eq!(draw(Flavour::Cff), [Segment::LineTo { x: 100.0, y: 0.0 }]);
        assert!(draw(Flavour::Cff2).is_empty());
    }

    // ---- FT-16.3: the Top DICT's and the Private DICT's style --------------

    /// `ItalicAngle` −12.5 as a DICT real: 30, then the nibbles of `-12.5`
    /// (`e` the minus, `a` the point, `f` the end), then the two-byte
    /// operator 12 2 (TN #5176 section 4, Table 5).
    const MINUS_TWELVE_AND_A_HALF: [u8; 6] = [30, 0xE1, 0x2A, 0x5F, 12, 2];

    /// FT-16.3: the Top DICT's `FontBBox`, `isFixedPitch`, `ItalicAngle`,
    /// `UnderlinePosition`, `UnderlineThickness` and `Weight`, and the
    /// Private DICT's `ForceBold` and `StdVW`, read as the operands written
    /// in the test; every absent one its Table 9 or Table 23 default; and the
    /// committed `three_glyphs.cff`, whose Top DICT states none of them.
    #[test]
    fn the_top_dicts_box_and_style_entries_are_read() {
        let mut top = entry(5, &[-50, -250, 1050, 900]);
        top.extend(entry(0x0C01, &[1]));
        top.extend_from_slice(&MINUS_TWELVE_AND_A_HALF);
        top.extend(entry(0x0C03, &[-75]));
        top.extend(entry(0x0C04, &[40]));
        // SID 384 is the standard string `Bold` (Appendix A).
        top.extend(entry(4, &[384]));
        let mut private = private_dict(600, 600, false);
        private.extend(entry(0x0C0E, &[1]));
        private.extend(entry(11, &[88]));
        let data = Fixture {
            top_extra: top,
            private,
            ..abc(Table::default())
        }
        .build();
        let cff = Cff::parse(&data).expect("the fixture parses");
        assert_eq!(
            cff.style(),
            Style {
                font_bbox: [-50.0, -250.0, 1050.0, 900.0],
                is_fixed_pitch: true,
                italic_angle: -12.5,
                underline_position: -75.0,
                underline_thickness: 40.0,
                weight: Some("Bold"),
                force_bold: true,
                std_vw: Some(88.0),
            }
        );

        // Nothing stated: every default Table 9 and Table 23 give.
        let defaults = Style {
            font_bbox: [0.0; 4],
            is_fixed_pitch: false,
            italic_angle: 0.0,
            underline_position: -100.0,
            underline_thickness: 50.0,
            weight: None,
            force_bold: false,
            std_vw: None,
        };
        let data = abc(Table::default()).build();
        assert_eq!(Cff::parse(&data).expect("abc parses").style(), defaults);

        // A box of three numbers is no box, a false pitch and a false
        // ForceBold are false, and a Weight that is no whole SID, or one past
        // the strings, names nothing; a Weight from the string INDEX reads it.
        let mut top = entry(5, &[1, 2, 3]);
        top.extend(entry(0x0C01, &[0]));
        top.extend(entry(4, &[391]));
        let mut private = private_dict(600, 600, false);
        private.extend(entry(0x0C0E, &[0]));
        let data = Fixture {
            strings: vec![b"Demibold".to_vec()],
            top_extra: top,
            private,
            ..abc(Table::default())
        }
        .build();
        let style = Cff::parse(&data).expect("parses").style();
        assert_eq!(style.font_bbox, [0.0; 4]);
        assert!(!style.is_fixed_pitch);
        assert!(!style.force_bold);
        assert_eq!(style.weight, Some("Demibold"));
        let data = Fixture {
            top_extra: entry(4, &[392]),
            ..abc(Table::default())
        }
        .build();
        assert_eq!(Cff::parse(&data).expect("parses").style().weight, None);
        let mut half = vec![30, 0x3A, 0x5F]; // 3.5
        half.push(4);
        let data = Fixture {
            top_extra: half,
            ..abc(Table::default())
        }
        .build();
        assert_eq!(Cff::parse(&data).expect("parses").style().weight, None);

        // A CID-keyed font reads its first Font DICT's Private DICT.
        let mut font = cid_fixture(vec![0, 0, 0, 1, 1]);
        font.fds[0].private.extend(entry(0x0C0E, &[1]));
        font.fds[0].private.extend(entry(11, &[70]));
        font.fds[1].private.extend(entry(11, &[99]));
        let data = font.build();
        let style = Cff::parse(&data).expect("the CID fixture parses").style();
        assert!(style.force_bold, "the first Font DICT's ForceBold");
        assert_eq!(style.std_vw, Some(70.0));

        // The committed seed is the program `three_glyph_program` builds:
        // its Top DICT holds CharStrings and Private alone, and its Private
        // DICT the two widths and Subrs, so every entry is its default.
        let seed = std::fs::read(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../fuzz/corpus/cff/three_glyphs.cff"),
        )
        .expect("the committed seed");
        assert_eq!(seed, three_glyph_program(), "the seed is the fixture");
        assert_eq!(Cff::parse(&seed).expect("it parses").style(), defaults);
    }
}
