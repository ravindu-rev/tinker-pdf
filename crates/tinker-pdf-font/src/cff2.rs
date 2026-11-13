//! CFF2: the Compact Font Format 2 table (OpenType 1.9.1, the CFF2 chapter).
//!
//! Design documentation: `docs/design/variable-fonts.md`; feature
//! documentation: `docs/features/fonts.md`.
//!
//! A CFF2 table is a five-byte header, a Top DICT that is not in an INDEX,
//! the Global Subr INDEX after it, and, where the Top DICT points: the
//! CharStrings INDEX, the Font DICT INDEX — each Font DICT pointing at a
//! Private DICT, measured from the table, and the Private DICT at its local
//! subroutines, measured from itself — an FDSelect, in formats 0, 3 or 4, and
//! the variation store, an item variation store behind a 16-bit length.
//! INDEXes count in 32 bits; DICTs add `vsindex`, `blend` and
//! `VariationStoreOffset`, hold 513 operands, and take no 16.16 operand.
//!
//! **One interpreter.** The charstrings run through `cff.rs`'s Type 2
//! machine, parameterised by flavour: no width, no `endchar` or `return`, no
//! `put`, `get` or arithmetic, a stack of 513, and `vsindex` and `blend`. So
//! FT-04c's hint recording and `cff_subset.rs`'s stem count stay shared with
//! CFF's. A CFF2 program carries no charset, encoding or glyph names; a
//! caller selects glyphs through the face's `cmap`, or by CID as the glyph
//! index.
//!
//! **Every bound refuses by name** ([`Cff2Error`]), where CFF's reader stops
//! silently: an INDEX past 65 536 entries before its offsets are read, a
//! stack past 513, a `blend` short of its operands, subroutines nested past
//! ten, the operation budget, `vsindex` misplaced or out of range, `blend`
//! with no store, and Private DICTs that together read more bytes than the
//! table holds — which only a table overlapping its own DICTs reaches.

use std::collections::BTreeMap;

use crate::cff::{
    bias, dict_get, dict_token, Blend, Charstring, DictToken, Flavour, Index, IndexRefusal,
    Machine, Private, Stop, Subroutines, CFF2_MAX_OPERANDS,
};
use crate::outline::Outline;
use crate::variation::item_store::ItemVariationStore;
use crate::variation::VariationError;

/// `CharStringINDEXOffset`.
const CHARSTRINGS: u16 = 17;
/// `PrivateDICTOffset`, in a Font DICT.
const PRIVATE: u16 = 18;
/// `LocalSubrINDEXOffset`, in a Private DICT.
const SUBRS: u16 = 19;
/// `vsindex`, in a Private DICT.
const VSINDEX: u16 = 22;
/// `blend`, in a Private DICT.
const BLEND: u16 = 23;
/// `VariationStoreOffset`.
const VARIATION_STORE: u16 = 24;
/// `FontMatrix`.
const FONT_MATRIX: u16 = 0x0C07;
/// `FontDICTINDEXOffset`.
const FONT_DICTS: u16 = 0x0C24;
/// `FontDICTSelectOffset`.
const FD_SELECT: u16 = 0x0C25;

/// Which INDEX of a CFF2 table a refusal names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cff2Index {
    /// The CharStrings INDEX.
    CharStrings,
    /// The Global Subr INDEX.
    GlobalSubrs,
    /// The Font DICT INDEX.
    FontDicts,
    /// A Private DICT's Local Subr INDEX.
    LocalSubrs,
}

/// Why a CFF2 table, or one of its glyphs, was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Cff2Error {
    /// The header is cut short.
    Header,
    /// A major version other than 2.
    Version {
        /// The major version.
        major: u8,
        /// The minor version.
        minor: u8,
    },
    /// The Top DICT runs past the table.
    TopDict,
    /// The Top DICT names no CharStrings INDEX.
    NoCharStrings,
    /// The Top DICT names no Font DICT INDEX, or the INDEX is empty.
    NoFontDicts,
    /// An INDEX runs past the table.
    Index {
        /// Which.
        which: Cff2Index,
    },
    /// An INDEX counts more than 65 536 entries.
    IndexCount {
        /// Which.
        which: Cff2Index,
        /// The count it declares.
        count: u32,
    },
    /// A Font DICT's Private DICT runs past the table.
    PrivateDict {
        /// The Font DICT, by index.
        font_dict: u16,
    },
    /// The Private DICTs together read more bytes than the table holds.
    PrivateDictsOverlap,
    /// An FDSelect of a format the chapter does not define, cut short, out of
    /// order, or covering fewer glyphs than there are.
    FdSelect,
    /// A glyph whose FDSelect entry names a Font DICT past the INDEX.
    FontDict {
        /// The glyph.
        glyph: u16,
        /// The Font DICT it names.
        font_dict: u16,
    },
    /// The variation store runs past the table, or is refused.
    VariationStore(VariationError),
    /// A glyph past the CharStrings INDEX.
    NoSuchGlyph {
        /// The glyph.
        glyph: u16,
    },
    /// A charstring spent the operation budget.
    Budget,
    /// Subroutines nested past ten levels.
    SubroutineDepth,
    /// More than 513 operands on the stack.
    StackOverflow,
    /// A `blend` whose operands are fewer than `n + n·k + 1`, or whose `n`
    /// is not a whole number.
    BlendOperands,
    /// `blend` or `vsindex` in a table with no variation store.
    NoVariationStore,
    /// `vsindex` naming item variation data the store does not have.
    VsindexRange,
    /// `vsindex` after a `blend`, or given twice.
    VsindexMisplaced,
    /// `vsindex` with no operand.
    VsindexOperand,
}

impl From<Stop> for Cff2Error {
    fn from(stop: Stop) -> Cff2Error {
        match stop {
            Stop::Budget => Cff2Error::Budget,
            Stop::SubroutineDepth => Cff2Error::SubroutineDepth,
            Stop::StackOverflow => Cff2Error::StackOverflow,
            Stop::BlendOperands => Cff2Error::BlendOperands,
            Stop::NoVariationStore => Cff2Error::NoVariationStore,
            Stop::VsindexRange => Cff2Error::VsindexRange,
            Stop::VsindexMisplaced => Cff2Error::VsindexMisplaced,
            Stop::VsindexOperand => Cff2Error::VsindexOperand,
        }
    }
}

impl core::fmt::Display for Cff2Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Cff2Error::Header => write!(f, "the CFF2 header is cut short"),
            Cff2Error::Version { major, minor } => {
                write!(f, "CFF2 version {major}.{minor}, not 2")
            }
            Cff2Error::TopDict => write!(f, "the CFF2 Top DICT runs past the table"),
            Cff2Error::NoCharStrings => write!(f, "the CFF2 Top DICT names no CharStrings"),
            Cff2Error::NoFontDicts => write!(f, "the CFF2 table has no Font DICTs"),
            Cff2Error::Index { which } => write!(f, "the CFF2 {which:?} INDEX runs past the table"),
            Cff2Error::IndexCount { which, count } => write!(
                f,
                "the CFF2 {which:?} INDEX counts {count} entries, past 65536"
            ),
            Cff2Error::PrivateDict { font_dict } => write!(
                f,
                "Font DICT {font_dict}'s Private DICT runs past the table"
            ),
            Cff2Error::PrivateDictsOverlap => {
                write!(
                    f,
                    "the CFF2 Private DICTs read more bytes than the table holds"
                )
            }
            Cff2Error::FdSelect => write!(f, "the CFF2 FDSelect does not cover the glyphs"),
            Cff2Error::FontDict { glyph, font_dict } => write!(
                f,
                "glyph {glyph} names Font DICT {font_dict}, which is not there"
            ),
            Cff2Error::VariationStore(error) => write!(f, "the CFF2 variation store: {error}"),
            Cff2Error::NoSuchGlyph { glyph } => write!(f, "glyph {glyph} is past the CharStrings"),
            Cff2Error::Budget => write!(f, "a CFF2 charstring spent its operation budget"),
            Cff2Error::SubroutineDepth => {
                write!(f, "CFF2 subroutines nest past ten levels")
            }
            Cff2Error::StackOverflow => write!(f, "more than 513 CFF2 operands"),
            Cff2Error::BlendOperands => write!(f, "a CFF2 blend short of its operands"),
            Cff2Error::NoVariationStore => {
                write!(
                    f,
                    "a CFF2 blend or vsindex in a table with no variation store"
                )
            }
            Cff2Error::VsindexRange => write!(f, "a CFF2 vsindex names no item variation data"),
            Cff2Error::VsindexMisplaced => {
                write!(f, "a CFF2 vsindex after a blend, or given twice")
            }
            Cff2Error::VsindexOperand => write!(f, "a CFF2 vsindex with no operand"),
        }
    }
}

impl std::error::Error for Cff2Error {}

fn be16(data: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_be_bytes([*data.get(at)?, *data.get(at + 1)?]))
}

fn be32(data: &[u8], at: usize) -> Option<u32> {
    let b = data.get(at..at.checked_add(4)?)?;
    Some(u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
}

/// An INDEX, or the refusal naming it.
fn index_at(data: &[u8], at: usize, which: Cff2Index) -> Result<Index<'_>, Cff2Error> {
    match Index::parse_cff2(data, at) {
        Ok((index, _)) => Ok(index),
        Err(IndexRefusal::Truncated) => Err(Cff2Error::Index { which }),
        Err(IndexRefusal::Count(count)) => Err(Cff2Error::IndexCount { which, count }),
    }
}

/// A DICT operand as an offset or size into the table, or `None` for one
/// that is negative, fractional or absurd.
fn offset_of(value: f64) -> Option<usize> {
    (value >= 0.0 && value.trunc() == value && value <= f64::from(u32::MAX))
        .then_some(value as usize)
}

/// Reads CFF2 DICT data into `(operator, operands)` pairs (CFF2 chapter,
/// 'DICT data'). Operators run to 24; the stack holds 513 operands. Where
/// `blend` is given — a Private DICT — `vsindex` selects the item variation
/// data, before any `blend` and once, and `blend` resolves its operands in
/// place, leaving the results to the operator after it; the `vsindex` entry
/// is kept, with its one operand. Elsewhere the two bytes are operators like
/// any other, which a reader looking for its own keys passes over.
fn read_dict(data: &[u8], blend: Option<Blend<'_>>) -> Result<Vec<(u16, Vec<f64>)>, Stop> {
    let mut out = Vec::new();
    let mut stack: Vec<f64> = Vec::new();
    let mut vsindex = 0u16;
    let mut vsindex_set = false;
    let mut blended = false;
    let mut at = 0usize;
    while at < data.len() {
        let Some((token, next)) = dict_token(data, at, 24) else {
            break;
        };
        at = next;
        match (token, blend) {
            (Some(DictToken::Operand(value)), _) => {
                if stack.len() >= CFF2_MAX_OPERANDS {
                    return Err(Stop::StackOverflow);
                }
                stack.push(value);
            }
            (Some(DictToken::Operator(VSINDEX)), Some(blend)) => {
                if vsindex_set || blended {
                    return Err(Stop::VsindexMisplaced);
                }
                let value = stack.pop().ok_or(Stop::VsindexOperand)?;
                stack.clear();
                if blend.region_counts.is_empty() {
                    return Err(Stop::NoVariationStore);
                }
                if !(value >= 0.0
                    && value.trunc() == value
                    && value < blend.region_counts.len() as f64)
                {
                    return Err(Stop::VsindexRange);
                }
                vsindex = value as u16;
                vsindex_set = true;
                out.push((VSINDEX, vec![value]));
            }
            (Some(DictToken::Operator(BLEND)), Some(blend)) => {
                blend.apply(vsindex, &mut stack)?;
                blended = true;
            }
            (Some(DictToken::Operator(op)), _) => out.push((op, std::mem::take(&mut stack))),
            (None, _) => {}
        }
    }
    Ok(out)
}

/// One Font DICT: which distinct Private DICT a glyph runs under, and where
/// that DICT's bytes are, for reading its values again at an instance.
#[derive(Clone, Copy, Debug)]
struct FontDict<'a> {
    private: usize,
    /// Where the Private DICT starts in the table, which its
    /// `LocalSubrINDEXOffset` is measured from.
    offset: usize,
    dict: &'a [u8],
}

/// A parsed CFF2 table.
#[derive(Clone, Debug)]
pub struct Cff2<'a> {
    charstrings: Index<'a>,
    global_subrs: Index<'a>,
    global_bias: i32,
    font_dicts: Vec<FontDict<'a>>,
    /// Each distinct Private DICT, read once.
    privates: Vec<Private<'a>>,
    /// Each glyph's Font DICT; empty when the table has no FDSelect and every
    /// glyph is in Font DICT 0.
    fd_select: Vec<u16>,
    font_matrix: [f64; 6],
    store: Option<ItemVariationStore<'a>>,
    /// Each item variation data subtable's region count, by `vsindex`.
    region_counts: Vec<u16>,
}

impl<'a> Cff2<'a> {
    /// Parses a CFF2 table.
    ///
    /// # Errors
    ///
    /// A header, Top DICT, INDEX, Private DICT, FDSelect or variation store
    /// this reader refuses, by name ([`Cff2Error`]).
    pub fn parse(data: &'a [u8]) -> Result<Cff2<'a>, Cff2Error> {
        let (&major, &minor) = (
            data.first().ok_or(Cff2Error::Header)?,
            data.get(1).ok_or(Cff2Error::Header)?,
        );
        let header_size = usize::from(*data.get(2).ok_or(Cff2Error::Header)?);
        let top_size = usize::from(be16(data, 3).ok_or(Cff2Error::Header)?);
        if major != 2 {
            return Err(Cff2Error::Version { major, minor });
        }
        let top_end = header_size + top_size;
        let top_bytes = data.get(header_size..top_end).ok_or(Cff2Error::TopDict)?;
        let top = read_dict(top_bytes, None).map_err(Cff2Error::from)?;

        let global_subrs = index_at(data, top_end, Cff2Index::GlobalSubrs)?;
        let global_bias = bias(global_subrs.len());

        let charstrings_at = dict_get(&top, CHARSTRINGS)
            .and_then(<[f64]>::first)
            .and_then(|v| offset_of(*v))
            .ok_or(Cff2Error::NoCharStrings)?;
        let charstrings = index_at(data, charstrings_at, Cff2Index::CharStrings)?;

        // The variation store: a 16-bit length, then the store itself.
        let store = match dict_get(&top, VARIATION_STORE).and_then(<[f64]>::first) {
            Some(value) => {
                let past = Cff2Error::VariationStore(VariationError::StorePastEnd);
                let at = offset_of(*value).ok_or(past)?;
                let length = usize::from(be16(data, at).ok_or(past)?);
                let start = at.checked_add(2).ok_or(past)?;
                let bytes = data.get(start..start + length).ok_or(past)?;
                Some(ItemVariationStore::parse(bytes).map_err(Cff2Error::VariationStore)?)
            }
            None => None,
        };
        let mut region_counts = Vec::new();
        if let Some(store) = &store {
            for outer in 0..store.data_count() {
                let data = store.data(outer).map_err(Cff2Error::VariationStore)?;
                region_counts.push(data.map_or(0, |d| {
                    // A subtable's region count is sixteen bits wide.
                    u16::try_from(d.region_indexes().len()).unwrap_or(u16::MAX)
                }));
            }
        }

        let font_dicts_at = dict_get(&top, FONT_DICTS)
            .and_then(<[f64]>::first)
            .and_then(|v| offset_of(*v))
            .ok_or(Cff2Error::NoFontDicts)?;
        let font_dict_index = index_at(data, font_dicts_at, Cff2Index::FontDicts)?;
        if font_dict_index.len() == 0 {
            return Err(Cff2Error::NoFontDicts);
        }
        let blend = Blend {
            region_counts: &region_counts,
            scalars: None,
        };
        // Every Font DICT's Private DICT, and the distinct ones among them,
        // measured before any is read: the distinct ones, and the local
        // subroutines each points at, may not together read more bytes than
        // the table holds, which only a table overlapping its own DICTs
        // reaches — so the work and the memory are linear in the table.
        let mut font_dicts = Vec::with_capacity(font_dict_index.len());
        let mut distinct: BTreeMap<(usize, usize), usize> = BTreeMap::new();
        let mut read = 0usize;
        for number in 0..font_dict_index.len() {
            // The INDEX holds at most 65 536 entries.
            let font_dict = u16::try_from(number).unwrap_or(u16::MAX);
            let bytes = font_dict_index.get(number).ok_or(Cff2Error::Index {
                which: Cff2Index::FontDicts,
            })?;
            let entries = read_dict(bytes, None).map_err(Cff2Error::from)?;
            let (size, offset) = match dict_get(&entries, PRIVATE) {
                Some(&[size, offset, ..]) => (
                    offset_of(size).ok_or(Cff2Error::PrivateDict { font_dict })?,
                    offset_of(offset).ok_or(Cff2Error::PrivateDict { font_dict })?,
                ),
                _ => (0, 0),
            };
            let dict = data
                .get(offset..offset.saturating_add(size))
                .ok_or(Cff2Error::PrivateDict { font_dict })?;
            let next = distinct.len();
            let private = *distinct.entry((offset, size)).or_insert_with(|| {
                read = read.saturating_add(size);
                next
            });
            if read > data.len() {
                return Err(Cff2Error::PrivateDictsOverlap);
            }
            font_dicts.push(FontDict {
                private,
                offset,
                dict,
            });
        }
        let mut privates = vec![Private::default(); distinct.len()];
        let mut done = vec![false; distinct.len()];
        for font_dict in &font_dicts {
            let Some(slot) = done.get_mut(font_dict.private) else {
                continue;
            };
            if *slot {
                continue;
            }
            *slot = true;
            let (private, subrs_bytes) =
                read_private(data, font_dict.dict, font_dict.offset, blend)?;
            read = read.saturating_add(subrs_bytes);
            if read > data.len() {
                return Err(Cff2Error::PrivateDictsOverlap);
            }
            if let Some(target) = privates.get_mut(font_dict.private) {
                *target = private;
            }
        }

        let glyphs = charstrings.len();
        let fd_select = match dict_get(&top, FD_SELECT).and_then(<[f64]>::first) {
            Some(value) => {
                let at = offset_of(*value).ok_or(Cff2Error::FdSelect)?;
                read_fd_select(data, at, glyphs)?
            }
            None => Vec::new(),
        };

        let font_matrix = match dict_get(&top, FONT_MATRIX) {
            Some(m) if m.len() >= 6 => [m[0], m[1], m[2], m[3], m[4], m[5]],
            _ => [0.001, 0.0, 0.0, 0.001, 0.0, 0.0],
        };

        Ok(Cff2 {
            charstrings,
            global_subrs,
            global_bias,
            font_dicts,
            privates,
            fd_select,
            font_matrix,
            store,
            region_counts,
        })
    }

    /// How many glyphs the table holds.
    #[must_use]
    pub fn glyph_count(&self) -> usize {
        self.charstrings.len()
    }

    /// How many Font DICTs it holds.
    #[must_use]
    pub fn font_dict_count(&self) -> usize {
        self.font_dicts.len()
    }

    /// The font matrix, 1/1000 unless the Top DICT states another.
    #[must_use]
    pub fn font_matrix(&self) -> [f64; 6] {
        self.font_matrix
    }

    /// The variation store, where the table has one.
    #[must_use]
    pub fn variation_store(&self) -> Option<&ItemVariationStore<'a>> {
        self.store.as_ref()
    }

    /// The Font DICT a glyph runs under: its FDSelect entry, or 0 where the
    /// table has no FDSelect.
    ///
    /// # Errors
    ///
    /// [`Cff2Error::NoSuchGlyph`] past the glyphs, and
    /// [`Cff2Error::FontDict`] for an entry past the Font DICTs.
    pub fn font_dict_for(&self, glyph: u16) -> Result<u16, Cff2Error> {
        if usize::from(glyph) >= self.glyph_count() {
            return Err(Cff2Error::NoSuchGlyph { glyph });
        }
        let font_dict = self.fd_select.get(usize::from(glyph)).copied().unwrap_or(0);
        if usize::from(font_dict) >= self.font_dicts.len() {
            return Err(Cff2Error::FontDict { glyph, font_dict });
        }
        Ok(font_dict)
    }

    /// The `vsindex` a glyph's blends read unless its charstring names
    /// another: its Private DICT's, or 0.
    ///
    /// # Errors
    ///
    /// [`Cff2::font_dict_for`]'s.
    pub fn private_vsindex(&self, glyph: u16) -> Result<u16, Cff2Error> {
        Ok(self.private_for(glyph)?.vsindex)
    }

    /// The Private DICT a glyph runs under.
    fn private_for(&self, glyph: u16) -> Result<&Private<'a>, Cff2Error> {
        let font_dict = self.font_dict_for(glyph)?;
        self.font_dicts
            .get(usize::from(font_dict))
            .and_then(|fd| self.privates.get(fd.private))
            .ok_or(Cff2Error::FontDict { glyph, font_dict })
    }

    /// One glyph's outline at the default instance, in the font's own units:
    /// every `blend` keeps its defaults and drops its deltas.
    ///
    /// # Errors
    ///
    /// A glyph the table does not have or a Font DICT it does not hold, and
    /// every bound the charstring passes, by name.
    pub fn outline(&self, glyph: u16) -> Result<Outline, Cff2Error> {
        let mut outline = Outline::default();
        let mut machine = Machine::new();
        self.outline_into(glyph, &mut outline, &mut machine)?;
        Ok(outline)
    }

    /// [`Cff2::outline`], written into `outline` through `machine`'s stack,
    /// so a caller reading every glyph allocates both once. Nothing carries
    /// from one glyph to the next.
    ///
    /// # Errors
    ///
    /// As [`Cff2::outline`]; `outline` is left empty.
    pub fn outline_into(
        &self,
        glyph: u16,
        outline: &mut Outline,
        machine: &mut Machine,
    ) -> Result<(), Cff2Error> {
        self.run(glyph, outline, machine, None)
    }

    /// Runs a glyph under a `blend` reading `scalars`, the default's none.
    fn run(
        &self,
        glyph: u16,
        outline: &mut Outline,
        machine: &mut Machine,
        scalars: Option<&[Vec<f64>]>,
    ) -> Result<(), Cff2Error> {
        outline.segments.clear();
        let private = self.private_for(glyph)?;
        let charstring = self
            .charstrings
            .get(usize::from(glyph))
            .ok_or(Cff2Error::NoSuchGlyph { glyph })?;
        machine.stack.reserve(CFF2_MAX_OPERANDS);
        let blend = Blend {
            region_counts: &self.region_counts,
            scalars,
        };
        let stop = {
            let mut ctx = Charstring::new(
                Flavour::Cff2,
                Subroutines {
                    global: &self.global_subrs,
                    global_bias: self.global_bias,
                    private,
                },
                Some(&mut *outline),
                &mut machine.stack,
                Some(blend),
            );
            ctx.run_glyph(charstring)
        };
        if let Some(stop) = stop {
            outline.segments.clear();
            return Err(stop.into());
        }
        Ok(())
    }

    /// A Font DICT's Private DICT entries at the default instance: every
    /// `blend` keeps its defaults, and the `vsindex` entry is kept with its
    /// operand.
    ///
    /// # Errors
    ///
    /// [`Cff2Error::FontDict`] for a Font DICT past the INDEX, and the
    /// DICT's refusals.
    pub fn private_dict(&self, font_dict: u16) -> Result<Vec<(u16, Vec<f64>)>, Cff2Error> {
        self.private_entries(font_dict, None)
    }

    /// A Private DICT's entries under a `blend` reading `scalars`.
    fn private_entries(
        &self,
        font_dict: u16,
        scalars: Option<&[Vec<f64>]>,
    ) -> Result<Vec<(u16, Vec<f64>)>, Cff2Error> {
        let dict = self
            .font_dicts
            .get(usize::from(font_dict))
            .ok_or(Cff2Error::FontDict {
                glyph: 0,
                font_dict,
            })?
            .dict;
        let blend = Blend {
            region_counts: &self.region_counts,
            scalars,
        };
        read_dict(dict, Some(blend)).map_err(Cff2Error::from)
    }
}

/// Reads a Private DICT at the default instance for what running a
/// charstring needs: its local subroutines and their bias, and its
/// `vsindex` — and how many bytes its Local Subr INDEX spans, which the
/// table's read budget counts.
fn read_private<'a>(
    data: &'a [u8],
    dict: &'a [u8],
    offset: usize,
    blend: Blend<'_>,
) -> Result<(Private<'a>, usize), Cff2Error> {
    let entries = read_dict(dict, Some(blend)).map_err(Cff2Error::from)?;
    let mut private = Private::default();
    if let Some(&value) = dict_get(&entries, VSINDEX).and_then(<[f64]>::first) {
        // `read_dict` held it to the store's subtables, of which there are
        // at most 65 535.
        private.vsindex = value as u16;
    }
    let mut span = 0;
    if let Some(&value) = dict_get(&entries, SUBRS).and_then(<[f64]>::first) {
        let at = offset_of(value)
            .and_then(|relative| offset.checked_add(relative))
            .ok_or(Cff2Error::Index {
                which: Cff2Index::LocalSubrs,
            })?;
        let (subrs, end) = match Index::parse_cff2(data, at) {
            Ok(parsed) => parsed,
            Err(IndexRefusal::Truncated) => {
                return Err(Cff2Error::Index {
                    which: Cff2Index::LocalSubrs,
                })
            }
            Err(IndexRefusal::Count(count)) => {
                return Err(Cff2Error::IndexCount {
                    which: Cff2Index::LocalSubrs,
                    count,
                })
            }
        };
        span = end.saturating_sub(at);
        private.local_bias = bias(subrs.len());
        private.local_subrs = subrs;
    }
    Ok((private, span))
}

/// Reads FDSelect (CFF2 chapter, 'FontDICTSelect'): format 0, a Font DICT a
/// glyph; format 3, ranges of 16-bit firsts and 8-bit Font DICTs; format 4,
/// ranges of 32-bit firsts and 16-bit Font DICTs; each range format starting
/// at glyph 0, its firsts increasing, and a sentinel at or past the glyph
/// count.
fn read_fd_select(data: &[u8], at: usize, glyphs: usize) -> Result<Vec<u16>, Cff2Error> {
    let past = Cff2Error::FdSelect;
    // Inside the table, so no offset below can overflow.
    let format = *data.get(at).ok_or(past)?;
    let mut out = vec![0u16; glyphs];
    match format {
        0 => {
            let entries = data.get(at + 1..at + 1 + glyphs).ok_or(past)?;
            for (slot, fd) in out.iter_mut().zip(entries) {
                *slot = u16::from(*fd);
            }
        }
        3 | 4 => {
            let wide = format == 4;
            let (count, mut cursor) = if wide {
                (be32(data, at + 1).ok_or(past)? as usize, at + 5)
            } else {
                (usize::from(be16(data, at + 1).ok_or(past)?), at + 3)
            };
            // Each range holds a glyph at least, and its firsts increase.
            if count == 0 || count > glyphs.max(1) {
                return Err(past);
            }
            let record = if wide { 6 } else { 3 };
            let read = |cursor: usize| -> Option<(usize, u16)> {
                if wide {
                    Some((be32(data, cursor)? as usize, be16(data, cursor + 4)?))
                } else {
                    Some((
                        usize::from(be16(data, cursor)?),
                        u16::from(*data.get(cursor + 2)?),
                    ))
                }
            };
            let mut ranges = Vec::with_capacity(count);
            for _ in 0..count {
                ranges.push(read(cursor).ok_or(past)?);
                cursor += record;
            }
            let sentinel = if wide {
                be32(data, cursor).ok_or(past)? as usize
            } else {
                usize::from(be16(data, cursor).ok_or(past)?)
            };
            if ranges.first().map(|r| r.0) != Some(0) || sentinel < glyphs {
                return Err(past);
            }
            for (index, (first, fd)) in ranges.iter().enumerate() {
                let end = ranges.get(index + 1).map_or(sentinel, |next| next.0);
                if end <= *first {
                    return Err(past);
                }
                for slot in out.iter_mut().take(end.min(glyphs)).skip(*first) {
                    *slot = *fd;
                }
            }
        }
        _ => return Err(past),
    }
    Ok(out)
}
