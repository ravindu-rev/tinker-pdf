//! Static instances: a variable face at one instance, written as a face with
//! no variation tables.
//!
//! Design documentation: `docs/design/variable-fonts.md`, 'The static
//! instancer'; feature documentation: `docs/features/fonts.md`.
//!
//! OpenType 1.9.1's overview, 'Dynamic Generation of Static Instance Fonts',
//! leaves what such a face holds to the scenario that needs it; this one is
//! what a document embedding the instance, and any reader of that document,
//! needs. [`instantiate`] reads the instance's [`Location`] off the face's
//! design space and writes:
//!
//! - **`glyf` and `loca`**: each glyph's points as `gvar` moves them there,
//!   rounded to font units — `floor(v + ½)`, the numeric model's — and a
//!   composite's offsets likewise, each bounding box recomputed from the
//!   points it now holds, `loca` short where the new `glyf` fits it and long
//!   where it does not. Instructions, contour order, on-curve flags and
//!   `OVERLAP_SIMPLE` are kept.
//! - **`hmtx` and `vmtx`**: each advance from `HVAR` or `VVAR`, or from the
//!   phantom points where the face has neither, rounded; each left side
//!   bearing the glyph's new `xMin`, and each top side bearing its vertical
//!   origin — the third phantom point — less its new `yMax`. `hhea` and
//!   `vhea` take the long-metric count and the extents these give.
//! - **`OS/2`, `hhea`, `vhea`, `post` and `gasp`**: each value `MVAR` varies,
//!   rounded and held to its field's range, the last `gasp` range keeping
//!   0xFFFF; and the three fields `MVAR` leaves to the axes (its 'Value
//!   Tags' note): `usWeightClass` the `wght` value rounded into 1 to 1 000,
//!   `usWidthClass` the `wdth` value mapped through the `OS/2` chapter's
//!   table, interpolated and rounded into 1 to 9 as the `wdth` registration
//!   says, and `italicAngle` the `slnt` value, both counter-clockwise.
//!   `xAvgCharWidth`, which `OS/2` versions 3 on define as the mean non-zero
//!   advance, is the instance's.
//! - **`cvt `**: the `cvar` values, rounded; and where the face carries
//!   hinting, an IDEF for `GETVARIATION` (0x91) at the end of `prep` pushing
//!   the instance's normalised coordinates, with `maxp`'s
//!   `maxInstructionDefs` and `maxStackElements` raised to cover it, as the
//!   *TrueType Instruction Set*'s notes on the instruction ask of an instance.
//! - **`CFF2` becomes `CFF `**: each glyph's charstring re-encoded as Type 2
//!   at the instance by `Cff2::type2_charstring` — blends resolved,
//!   subroutines inlined, its width first, `endchar` added and every
//!   operator kept within Type 2's 48 operands, stems and masks kept — under
//!   a CFF that is name-keyed where the face has one Font DICT and `post`
//!   names every glyph apart, its charset those names, and CID-keyed with ROS
//!   `Adobe-Identity-0` and each glyph's CID its index otherwise (the
//!   overview's rule), each Private DICT written with its blended values. The
//!   CFF is read back before it is returned, every glyph held to the outline
//!   `Cff2` draws at the instance and to its width.
//! - **`head`**: the glyphs' union as the font box, `indexToLocFormat` the new
//!   `loca`'s, and `checkSumAdjustment` rewritten with every directory
//!   checksum; **`VORG`** its origins as `VVAR` moves them, and a CFF2 face's
//!   top side bearings measured from them, or from the origin the default
//!   instance has where there is no `VORG`.
//! - **Dropped**: `fvar`, `avar`, `gvar`, `cvar`, `HVAR`, `VVAR`, `MVAR`,
//!   `STAT`; `DSIG`, which signs bytes that changed; `hdmx` and `VDMX`, which
//!   describe no instance.
//! - **Copied as they are**: everything else — `cmap`, `name`, `fpgm`, `GSUB`,
//!   `GPOS` and `GDEF` with their variation data unapplied (FT-07a's), `kern`,
//!   and tables this reader does not know.
//!
//! A face that does not vary is its own instance and is returned as it is.
//! At the default location every delta is zero for a face whose regions all
//! peak away from it, so the instance keeps every default outline and metric.
//! A table a reader refuses refuses the instance, by name ([`InstanceError`]):
//! a writer has no default to fall back to that is not a different face.

use crate::cff::{Cff, Machine, STANDARD_STRINGS};
use crate::cff2::{Blends, Cff2, Cff2Error, Type2Error};
use crate::cff_subset::{dict_int, dict_op, dict_operand, write_index};
use crate::glyf::{self, Component};
use crate::outline::{Outline, Segment};
use crate::sfnt::Sfnt;
use crate::subset::assemble;
use crate::variation::cvar::cvt_at;
use crate::variation::gvar::Gvar;
use crate::variation::metrics::{metric_default, Hvar, Mvar, Vvar, METRIC_FIELDS};
use crate::variation::{tag_value, DesignSpace, Fixed, Instance, Location, Tag, VariationError};

const TAG_GLYF: Tag = *b"glyf";
const TAG_LOCA: Tag = *b"loca";
const TAG_HEAD: Tag = *b"head";
const TAG_HHEA: Tag = *b"hhea";
const TAG_HMTX: Tag = *b"hmtx";
const TAG_VHEA: Tag = *b"vhea";
const TAG_VMTX: Tag = *b"vmtx";
const TAG_MAXP: Tag = *b"maxp";
const TAG_OS2: Tag = *b"OS/2";
const TAG_POST: Tag = *b"post";
const TAG_GASP: Tag = *b"gasp";
const TAG_CVT: Tag = *b"cvt ";
const TAG_FPGM: Tag = *b"fpgm";
const TAG_PREP: Tag = *b"prep";
const TAG_VORG: Tag = *b"VORG";
const TAG_CFF2: Tag = *b"CFF2";
const TAG_CFF: Tag = *b"CFF ";
const TAG_NAME: Tag = *b"name";

/// The tables a static instance does not carry: the variation tables, `STAT`,
/// which names styles for a menu, `DSIG`, which signs bytes that changed, and
/// `hdmx` and `VDMX`, which describe no instance.
pub const DROPPED: &[Tag] = &[
    *b"fvar", *b"avar", *b"gvar", *b"cvar", *b"HVAR", *b"VVAR", *b"MVAR", *b"STAT", *b"DSIG",
    *b"hdmx", *b"VDMX",
];

/// How deep a composite's components are followed while its box is
/// measured, as the outline reader follows them.
const MAX_COMPONENT_DEPTH: u32 = 8;

/// How many component glyphs one box measures in all, as the outline reader
/// bounds the glyphs one outline draws.
const MAX_COMPONENTS: u32 = 256;

/// How many records of one composite are measured, as the reader draws them.
const MAX_RECORDS: usize = 64;

/// The `OS/2` chapter's `usWidthClass` table: each class's percentage of the
/// normal width, doubled so that every entry is whole.
const WIDTH_CLASSES: [i64; 9] = [100, 125, 150, 175, 200, 225, 250, 300, 400];

/// Why a static instance was not written. Each names the rule the face
/// broke; a caller embeds nothing rather than a face that is not the
/// instance asked for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum InstanceError {
    /// The bytes are no sfnt.
    NotAFont,
    /// The design space, or a variation table the instance reads, is one
    /// this reader refuses — or the instance names a named instance the face
    /// does not have.
    Variation(VariationError),
    /// One glyph's variation data is refused.
    Glyph {
        /// The glyph.
        glyph: u16,
        /// Why.
        error: VariationError,
    },
    /// A table the instance needs is missing or too short to read.
    MissingTable {
        /// The table.
        table: Tag,
    },
    /// A varied coordinate, the difference between two consecutive points,
    /// a composite's offset or a box edge does not fit the sixteen bits
    /// `glyf` stores it in.
    CoordinateRange {
        /// The glyph.
        glyph: u16,
    },
    /// The `CFF2` table is refused, or its variation store's axes are not
    /// the design space's.
    Cff2(Cff2Error),
    /// One glyph's CFF2 charstring is refused.
    Cff2Glyph {
        /// The glyph.
        glyph: u16,
        /// Why.
        error: Cff2Error,
    },
    /// A glyph's charstring at the instance carries an operand no Type 2
    /// charstring can: a whole number past sixteen bits, or a value past the
    /// 16.16 form's range.
    CharstringOperand {
        /// The glyph.
        glyph: u16,
    },
    /// More Font DICTs than a CFF's FDSelect, a byte a glyph, can name.
    TooManyFontDicts {
        /// How many the `CFF2` table holds.
        count: usize,
    },
    /// More glyphs than a CFF INDEX, counting in sixteen bits, holds.
    TooManyGlyphs {
        /// How many the `CFF2` table holds.
        count: usize,
    },
    /// The `CFF ` table written for the instance does not read back to the
    /// outline or the width the `CFF2` gives a glyph there.
    CffRereading {
        /// The glyph.
        glyph: u16,
    },
}

impl core::fmt::Display for InstanceError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            InstanceError::NotAFont => write!(f, "the program is not an sfnt"),
            InstanceError::Variation(error) => write!(f, "a variation table: {error}"),
            InstanceError::Glyph { glyph, error } => {
                write!(f, "glyph {glyph}'s variation data: {error}")
            }
            InstanceError::MissingTable { table } => write!(
                f,
                "the {} table is missing or too short",
                String::from_utf8_lossy(table)
            ),
            InstanceError::CoordinateRange { glyph } => write!(
                f,
                "glyph {glyph} at the instance has a coordinate past sixteen bits"
            ),
            InstanceError::Cff2(error) => write!(f, "the CFF2 table: {error}"),
            InstanceError::Cff2Glyph { glyph, error } => {
                write!(f, "glyph {glyph}'s CFF2 charstring: {error}")
            }
            InstanceError::CharstringOperand { glyph } => write!(
                f,
                "glyph {glyph} at the instance has an operand no Type 2 charstring carries"
            ),
            InstanceError::TooManyFontDicts { count } => write!(
                f,
                "the CFF2 table has {count} Font DICTs, more than a CFF's 256"
            ),
            InstanceError::TooManyGlyphs { count } => write!(
                f,
                "the CFF2 table has {count} glyphs, more than a CFF INDEX holds"
            ),
            InstanceError::CffRereading { glyph } => write!(
                f,
                "the instance's CFF does not draw glyph {glyph} as the CFF2 does"
            ),
        }
    }
}

impl std::error::Error for InstanceError {}

impl From<VariationError> for InstanceError {
    fn from(error: VariationError) -> InstanceError {
        InstanceError::Variation(error)
    }
}

fn be16(data: &[u8], at: usize) -> Option<u16> {
    let b = data.get(at..at.checked_add(2)?)?;
    Some(u16::from_be_bytes([b[0], b[1]]))
}

fn put16(data: &mut [u8], at: usize, value: u16) {
    if let Some(slot) = data.get_mut(at..at + 2) {
        slot.copy_from_slice(&value.to_be_bytes());
    }
}

fn put32(data: &mut [u8], at: usize, value: u32) {
    if let Some(slot) = data.get_mut(at..at + 4) {
        slot.copy_from_slice(&value.to_be_bytes());
    }
}

/// A rounded value held to `i16`.
fn clamp_i16(value: i64) -> i16 {
    // Held to the range first, so the conversion cannot fail.
    i16::try_from(value.clamp(i64::from(i16::MIN), i64::from(i16::MAX))).unwrap_or(0)
}

/// A rounded value held to `u16`.
fn clamp_u16(value: i64) -> u16 {
    u16::try_from(value.clamp(0, i64::from(u16::MAX))).unwrap_or(0)
}

/// Writes the static instance of `program` at `instance`.
///
/// # Errors
///
/// [`InstanceError`]: bytes that are no sfnt, a variation table or a
/// glyph's variation data a reader refuses, a named instance the face does
/// not have, a table the instance needs that is missing, a coordinate the
/// instance would move past sixteen bits, and outlines in `CFF2`.
pub fn instantiate(program: &[u8], instance: &Instance) -> Result<Vec<u8>, InstanceError> {
    let sfnt = Sfnt::parse(program).ok_or(InstanceError::NotAFont)?;
    let space = DesignSpace::read(&sfnt)?;
    let location = space.location(instance)?;
    if !space.is_variable() {
        // No axes: every coordinate is the default, and the face is its own
        // instance.
        return Ok(program.to_vec());
    }
    let user = space.user_coordinates(instance)?;
    let tables = Tables::read(&sfnt, &space)?;
    let glyph_count = sfnt
        .glyph_count()
        .ok_or(InstanceError::MissingTable { table: TAG_MAXP })?;

    // The outlines at the location: `glyf`'s, or `CFF2`'s.
    let outlines = if sfnt.table(tag_value(TAG_GLYF)).is_some() {
        Outlines::Glyf(GlyfInstance::read(
            &sfnt,
            tables.gvar.as_ref(),
            &location,
            glyph_count,
        )?)
    } else if let Some(table) = sfnt.table(tag_value(TAG_CFF2)) {
        Outlines::Cff2(Cff2Instance::read(table, &space, &location, glyph_count)?)
    } else {
        Outlines::None
    };
    let gvar = tables.gvar.as_ref();
    let vvar_scalars = tables.vvar.as_ref().map(|vvar| vvar.scalars(&location));
    // Where each glyph's vertical origin stands at the location: the third
    // phantom point of a glyph `gvar` moves, and for a CFF2 face its `VORG`
    // origin as `VVAR` moves it, or the default instance's origin.
    let vertical_origin = |glyph: u16| -> Result<Option<i64>, InstanceError> {
        match &outlines {
            Outlines::Glyf(_) if gvar.is_some() => {
                let p = glyf::phantom_points_at(&sfnt, glyph, gvar, &location)
                    .map_err(|error| InstanceError::Glyph { glyph, error })?;
                Ok(Some(p[2][1].round()))
            }
            Outlines::Cff2(cff2) => {
                cff2.vertical_origin(&sfnt, &tables, vvar_scalars.as_deref(), glyph)
            }
            _ => Ok(None),
        }
    };
    let metrics = Metrics::read(
        &sfnt,
        &tables,
        outlines.boxes(glyph_count),
        &vertical_origin,
        &location,
        glyph_count,
    )?;

    let mut out: Vec<(u32, Vec<u8>)> = Vec::new();
    let replaced = |tag: Tag| -> bool {
        DROPPED.contains(&tag)
            || matches!(
                &tag,
                b"glyf"
                    | b"loca"
                    | b"head"
                    | b"hhea"
                    | b"hmtx"
                    | b"vhea"
                    | b"vmtx"
                    | b"OS/2"
                    | b"post"
                    | b"gasp"
                    | b"cvt "
                    | b"prep"
                    | b"maxp"
                    | b"VORG"
                    | b"CFF2"
            )
            || (matches!(outlines, Outlines::Cff2(_)) && tag == TAG_CFF)
    };
    for (tag, data) in sfnt_tables(&sfnt) {
        if !replaced(tag) {
            out.push((tag_value(tag), data.to_vec()));
        }
    }

    // glyf, loca and head.
    let head = sfnt
        .table(tag_value(TAG_HEAD))
        .filter(|head| head.len() >= 54)
        .ok_or(InstanceError::MissingTable { table: TAG_HEAD })?;
    let mut head = head.to_vec();
    put32(&mut head, 8, 0); // checkSumAdjustment, which `assemble` fills in
    let mut hinted =
        sfnt.table(tag_value(TAG_FPGM)).is_some() || sfnt.table(tag_value(TAG_PREP)).is_some();
    let union = union_of(&outlines.boxes(glyph_count));
    match &outlines {
        Outlines::Glyf(glyphs) => {
            hinted |= glyphs.instructed;
            let (glyf, loca, long) = glyphs.encode();
            out.push((tag_value(TAG_GLYF), glyf));
            out.push((tag_value(TAG_LOCA), loca));
            put16(&mut head, 50, u16::from(long));
        }
        Outlines::Cff2(cff2) => {
            let cff = cff2.write(&sfnt, &space, instance, &metrics.widths(), union)?;
            out.push((tag_value(TAG_CFF), cff));
        }
        Outlines::None => {}
    }
    if !matches!(outlines, Outlines::None) {
        if let Some(union) = union {
            for (i, value) in union.iter().enumerate() {
                put16(&mut head, 36 + i * 2, *value as u16);
            }
        }
    }
    out.push((tag_value(TAG_HEAD), head));

    // hmtx and hhea, vmtx and vhea.
    metrics.write(&sfnt, &tables, &location, &mut out)?;

    // OS/2, post and gasp: MVAR's values and the axes' own fields.
    let axis_value = |tag: Tag| -> Option<Fixed> {
        let index = space.axis_index(tag)?;
        let axis = space.axes().get(index)?;
        user.get(index).map(|v| (*v).max(axis.min).min(axis.max))
    };
    if let Some(os2) = sfnt.table(tag_value(TAG_OS2)) {
        let mut os2 = os2.to_vec();
        tables.vary_fields(&sfnt, TAG_OS2, &location, &mut os2)?;
        if let Some(weight) = axis_value(*b"wght") {
            put16(&mut os2, 4, clamp_u16(weight.round()).clamp(1, 1_000));
        }
        if let Some(width) = axis_value(*b"wdth") {
            put16(&mut os2, 6, width_class(width));
        }
        if be16(&os2, 0).is_some_and(|version| version >= 3) {
            if let Some(average) = metrics.average_width() {
                put16(&mut os2, 2, average as u16);
            }
        }
        out.push((tag_value(TAG_OS2), os2));
    }
    if let Some(post) = sfnt.table(tag_value(TAG_POST)) {
        let mut post = post.to_vec();
        tables.vary_fields(&sfnt, TAG_POST, &location, &mut post)?;
        if let Some(slant) = axis_value(*b"slnt") {
            // A 16.16 Fixed both ways, so the axis value is the angle exactly.
            put32(
                &mut post,
                4,
                i32::try_from(slant.bits()).unwrap_or(0) as u32,
            );
        }
        out.push((tag_value(TAG_POST), post));
    }
    if let Some(gasp) = sfnt.table(tag_value(TAG_GASP)) {
        let mut gasp = gasp.to_vec();
        tables.vary_fields(&sfnt, TAG_GASP, &location, &mut gasp)?;
        out.push((tag_value(TAG_GASP), gasp));
    }

    // cvt, prep and maxp.
    if sfnt.table(tag_value(TAG_CVT)).is_some() {
        let values = cvt_at(&sfnt, &space, &location)?;
        let mut cvt = Vec::with_capacity(values.len() * 2);
        for value in values {
            cvt.extend_from_slice(&clamp_i16(value.round()).to_be_bytes());
        }
        out.push((tag_value(TAG_CVT), cvt));
    }
    let mut prep = sfnt.table(tag_value(TAG_PREP)).map(<[u8]>::to_vec);
    let mut maxp = sfnt.table(tag_value(TAG_MAXP)).map(<[u8]>::to_vec);
    if hinted {
        let axes = location.coordinates();
        let mut program = prep.take().unwrap_or_default();
        // PUSHB[000] 0x91, IDEF[], NPUSHW[] n and the coordinates, ENDF[]:
        // the instruction set's own form for an instance's GETVARIATION.
        program.extend_from_slice(&[0xB0, 0x91, 0x89, 0x41]);
        // `MAX_AXES` keeps the count inside a byte.
        program.push(u8::try_from(axes.len()).unwrap_or(u8::MAX));
        for coordinate in axes {
            program.extend_from_slice(&coordinate.to_be_bytes());
        }
        program.push(0x2D);
        prep = Some(program);
        if let Some(maxp) = maxp
            .as_mut()
            .filter(|m| m.len() >= 32 && be16(m, 0) == Some(1))
        {
            let defs = be16(maxp, 22).unwrap_or(0).saturating_add(1);
            let pushed = u16::try_from(axes.len()).unwrap_or(u16::MAX);
            let stack = be16(maxp, 24).unwrap_or(0).saturating_add(pushed);
            put16(maxp, 22, defs);
            put16(maxp, 24, stack);
        }
    }
    if let Some(prep) = prep {
        out.push((tag_value(TAG_PREP), prep));
    }
    if let Some(maxp) = maxp {
        out.push((tag_value(TAG_MAXP), maxp));
    }

    // VORG, where VVAR moves its origins.
    if let Some(vorg) = sfnt.table(tag_value(TAG_VORG)) {
        let varied = tables.varied_vorg(&sfnt, &location, glyph_count)?;
        out.push((tag_value(TAG_VORG), varied.unwrap_or_else(|| vorg.to_vec())));
    }

    out.sort_by_key(|(tag, _)| *tag);
    Ok(assemble(sfnt.version(), &out))
}

/// Every table of a face, by tag, in directory order, each once.
fn sfnt_tables<'a>(sfnt: &Sfnt<'a>) -> Vec<(Tag, &'a [u8])> {
    sfnt.tags()
        .into_iter()
        .filter_map(|tag| sfnt.table(tag_value(tag)).map(|data| (tag, data)))
        .collect()
}

/// The `usWidthClass` a `wdth` value maps to: the value placed among the
/// `OS/2` chapter's percentages, interpolated between the classes either
/// side and rounded, a half rounding up, then held to 1 to 9 — the `wdth`
/// registration's rule.
fn width_class(width: Fixed) -> u16 {
    // Twice the 16.16 value, against the doubled percentages.
    let value = i128::from(width.bits()) * 2;
    let at = |class: usize| i128::from(WIDTH_CLASSES[class]) << 16;
    if value <= at(0) {
        return 1;
    }
    if value >= at(8) {
        return 9;
    }
    for class in 0..8 {
        let (low, high) = (at(class), at(class + 1));
        if value <= high {
            // class + 1 + (value - low) / (high - low), rounded half up.
            let numerator = (value - low) * 2 + (high - low);
            let fraction = numerator.div_euclid((high - low) * 2);
            let class = i128::try_from(class).unwrap_or(0) + 1 + fraction;
            return u16::try_from(class.clamp(1, 9)).unwrap_or(5);
        }
    }
    9
}

/// The variation tables an instance reads, each parsed once.
struct Tables<'a> {
    gvar: Option<Gvar<'a>>,
    hvar: Option<Hvar<'a>>,
    vvar: Option<Vvar<'a>>,
    mvar: Option<Mvar<'a>>,
}

impl<'a> Tables<'a> {
    fn read(sfnt: &Sfnt<'a>, space: &DesignSpace) -> Result<Tables<'a>, InstanceError> {
        Ok(Tables {
            gvar: Gvar::read(sfnt, space)?,
            hvar: Hvar::read(sfnt, space)?,
            vvar: Vvar::read(sfnt, space)?,
            mvar: Mvar::read(sfnt, space)?,
        })
    }

    /// Writes every value `MVAR` varies in one table at the location, each
    /// its default plus its delta, rounded and held to its field's range;
    /// `gasp`'s last range, which is 0xFFFF whatever a record says, is left
    /// alone.
    ///
    /// # Errors
    ///
    /// The store's refusals for the index a record names.
    fn vary_fields(
        &self,
        sfnt: &Sfnt,
        table: Tag,
        location: &Location,
        data: &mut [u8],
    ) -> Result<(), InstanceError> {
        let Some(mvar) = &self.mvar else {
            return Ok(());
        };
        let scalars = mvar.scalars(location);
        let ranges = if table == TAG_GASP {
            usize::from(be16(data, 2).unwrap_or(0))
        } else {
            0
        };
        for field in METRIC_FIELDS.iter().filter(|field| field.table == table) {
            if table == TAG_GASP && (field.offset - 4) / 4 + 1 >= ranges {
                continue;
            }
            let (Some(default), Some(delta)) = (
                metric_default(sfnt, field.tag),
                mvar.delta(field.tag, &scalars)?,
            ) else {
                continue;
            };
            let value = Fixed::from_int(default).saturating_add(delta).round();
            let bits = if field.signed {
                clamp_i16(value) as u16
            } else {
                clamp_u16(value)
            };
            put16(data, field.offset, bits);
        }
        Ok(())
    }

    /// `VORG` with its origins as `VVAR` moves them, where `VVAR` maps
    /// origins: the default kept, and a record for every glyph whose origin
    /// at the location is another. `None` where nothing moves them — no
    /// `VVAR`, no origin map, or a `VORG` this reader does not read.
    ///
    /// # Errors
    ///
    /// The store's refusals for a glyph's origin.
    fn varied_vorg(
        &self,
        sfnt: &Sfnt,
        location: &Location,
        glyphs: u16,
    ) -> Result<Option<Vec<u8>>, InstanceError> {
        let (Some(vvar), Some(vorg)) = (self.vvar.as_ref(), sfnt.table(tag_value(TAG_VORG))) else {
            return Ok(None);
        };
        let Some(default) = be16(vorg, 4).map(|v| v as i16) else {
            return Ok(None);
        };
        let scalars = vvar.scalars(location);
        if vvar.origin_delta(0, &scalars)?.is_none() {
            return Ok(None);
        }
        let mut records: Vec<(u16, i16)> = Vec::new();
        for glyph in 0..glyphs {
            let Some(origin) = sfnt.vorg_origin(glyph) else {
                return Ok(None);
            };
            let delta = vvar.origin_delta(glyph, &scalars)?.unwrap_or(Fixed::ZERO);
            let moved = clamp_i16(
                Fixed::from_int(i32::from(origin))
                    .saturating_add(delta)
                    .round(),
            );
            if moved != default {
                records.push((glyph, moved));
            }
        }
        let mut out = Vec::with_capacity(8 + records.len() * 4);
        out.extend_from_slice(&1u16.to_be_bytes());
        out.extend_from_slice(&0u16.to_be_bytes());
        out.extend_from_slice(&default.to_be_bytes());
        // One record a glyph at most, and the glyph count is sixteen bits.
        out.extend_from_slice(
            &u16::try_from(records.len())
                .unwrap_or(u16::MAX)
                .to_be_bytes(),
        );
        for (glyph, origin) in records {
            out.extend_from_slice(&glyph.to_be_bytes());
            out.extend_from_slice(&origin.to_be_bytes());
        }
        Ok(Some(out))
    }
}

/// One glyph of the instance's `glyf`.
#[derive(Clone, Debug)]
enum Glyph {
    /// No outline.
    Empty,
    /// A simple glyph: its contours' last points, its points' on-curve
    /// flags — `OVERLAP_SIMPLE` kept on the first — its points rounded, and
    /// its instructions.
    Simple {
        ends: Vec<u16>,
        flags: Vec<u8>,
        points: Vec<(i32, i32)>,
        instructions: Vec<u8>,
    },
    /// A composite: its records with their offsets varied and rounded, and
    /// its instructions.
    Composite {
        records: Vec<Component>,
        instructions: Vec<u8>,
    },
    /// Data this reader draws nothing of — a simple glyph cut short, a
    /// composite with no whole record — kept as the face holds it.
    Opaque(Vec<u8>),
}

/// The instance's `glyf`: every glyph at the location, and each one's box.
struct GlyfInstance {
    glyphs: Vec<Glyph>,
    /// Each glyph's box, `None` for one that draws nothing.
    boxes: Vec<Option<[i16; 4]>>,
    /// Whether any glyph carries instructions.
    instructed: bool,
}

impl GlyfInstance {
    fn read(
        sfnt: &Sfnt,
        gvar: Option<&Gvar>,
        location: &Location,
        count: u16,
    ) -> Result<GlyfInstance, InstanceError> {
        let mut glyphs = Vec::with_capacity(usize::from(count));
        let mut instructed = false;
        for glyph in 0..count {
            let varied = varied_glyph(sfnt, gvar, location, glyph)?;
            match &varied {
                Glyph::Simple { instructions, .. } | Glyph::Composite { instructions, .. } => {
                    instructed |= !instructions.is_empty();
                }
                Glyph::Empty | Glyph::Opaque(_) => {}
            }
            glyphs.push(varied);
        }
        let mut instance = GlyfInstance {
            glyphs,
            boxes: Vec::new(),
            instructed,
        };
        let mut boxes = Vec::with_capacity(usize::from(count));
        for glyph in 0..count {
            boxes.push(instance.measure(glyph)?);
        }
        instance.boxes = boxes;
        Ok(instance)
    }

    /// A glyph's box: its points', or a composite's components' placed as
    /// the format places them, floored and ceiled to font units; `None` for
    /// a glyph that draws nothing.
    fn measure(&self, glyph: u16) -> Result<Option<[i16; 4]>, InstanceError> {
        let mut points = Vec::new();
        let mut budget = MAX_COMPONENTS;
        self.place(
            glyph,
            [0.0, 0.0, 1.0, 0.0, 0.0, 1.0],
            0,
            &mut budget,
            &mut points,
        );
        if points.is_empty() {
            return Ok(None);
        }
        let fold = |pick: fn(&(f64, f64)) -> f64, min: bool| {
            points.iter().map(pick).fold(
                if min {
                    f64::INFINITY
                } else {
                    f64::NEG_INFINITY
                },
                |acc, v| if min { acc.min(v) } else { acc.max(v) },
            )
        };
        let edge = |value: f64| -> Result<i16, InstanceError> {
            if (f64::from(i16::MIN)..=f64::from(i16::MAX)).contains(&value) {
                // Inside the range and integral, so the cast is exact.
                Ok(value as i16)
            } else {
                Err(InstanceError::CoordinateRange { glyph })
            }
        };
        Ok(Some([
            edge(fold(|p| p.0, true).floor())?,
            edge(fold(|p| p.1, true).floor())?,
            edge(fold(|p| p.0, false).ceil())?,
            edge(fold(|p| p.1, false).ceil())?,
        ]))
    }

    /// Appends the points a glyph draws, placed by `matrix`
    /// (`[dx, dy, a, b, c, d]`), within the outline reader's bounds.
    fn place(
        &self,
        glyph: u16,
        matrix: [f64; 6],
        depth: u32,
        budget: &mut u32,
        out: &mut Vec<(f64, f64)>,
    ) {
        if depth > MAX_COMPONENT_DEPTH || *budget == 0 {
            return;
        }
        *budget -= 1;
        let [dx, dy, a, b, c, d] = matrix;
        match self.glyphs.get(usize::from(glyph)) {
            Some(Glyph::Simple { points, .. }) => {
                for &(x, y) in points {
                    let (x, y) = (f64::from(x), f64::from(y));
                    out.push((a * x + c * y + dx, b * x + d * y + dy));
                }
            }
            Some(Glyph::Composite { records, .. }) => {
                // A matched point is numbered over this composite's own
                // components, from the first point its first one placed.
                let base = out.len();
                for record in records.iter().take(MAX_RECORDS) {
                    let [ca, cb, cc, cd] = record.transform.map(|v| f64::from(v) / 16_384.0);
                    let na = ca * a + cb * c;
                    let nb = ca * b + cb * d;
                    let nc = cc * a + cd * c;
                    let nd = cc * b + cd * d;
                    if record.offset_is_xy() {
                        let (ox, oy) = (
                            f64::from(record.arguments[0]),
                            f64::from(record.arguments[1]),
                        );
                        let child = [ox * a + oy * c + dx, ox * b + oy * d + dy, na, nb, nc, nd];
                        self.place(record.glyph, child, depth + 1, budget, out);
                    } else {
                        // Matched points: the child placed with its own 2×2
                        // and no offset, then moved so that its point
                        // `arguments[1]` lands on the composite's point
                        // `arguments[0]`, numbered over the components
                        // placed before it.
                        let mut child = Vec::new();
                        self.place(
                            record.glyph,
                            [dx, dy, na, nb, nc, nd],
                            depth + 1,
                            budget,
                            &mut child,
                        );
                        let parent = usize::try_from(record.arguments[0])
                            .ok()
                            .and_then(|p| p.checked_add(base));
                        let own = usize::try_from(record.arguments[1]).ok();
                        let shift = match (
                            parent.and_then(|p| out.get(p)),
                            own.and_then(|p| child.get(p)),
                        ) {
                            (Some(target), Some(source)) => {
                                (target.0 - source.0, target.1 - source.1)
                            }
                            _ => (0.0, 0.0),
                        };
                        out.extend(child.iter().map(|p| (p.0 + shift.0, p.1 + shift.1)));
                    }
                    if *budget == 0 {
                        return;
                    }
                }
            }
            _ => {}
        }
    }

    /// `glyf` and `loca`, and whether `loca` is the long form: short where
    /// the new `glyf`'s offsets halve into sixteen bits, each glyph padded to
    /// an even length so that they do.
    fn encode(&self) -> (Vec<u8>, Vec<u8>, bool) {
        let mut glyf: Vec<u8> = Vec::new();
        let mut offsets: Vec<usize> = Vec::with_capacity(self.glyphs.len() + 1);
        for (index, glyph) in self.glyphs.iter().enumerate() {
            offsets.push(glyf.len());
            let bounds = self.boxes.get(index).copied().flatten().unwrap_or([0; 4]);
            match glyph {
                Glyph::Empty => {}
                Glyph::Simple {
                    ends,
                    flags,
                    points,
                    instructions,
                } => encode_simple(&mut glyf, ends, flags, points, instructions, bounds),
                Glyph::Composite {
                    records,
                    instructions,
                } => encode_composite(&mut glyf, records, instructions, bounds),
                Glyph::Opaque(data) => glyf.extend_from_slice(data),
            }
            if glyf.len() % 2 != 0 {
                glyf.push(0);
            }
        }
        offsets.push(glyf.len());
        let long = glyf.len() / 2 > usize::from(u16::MAX);
        let mut loca = Vec::with_capacity(offsets.len() * if long { 4 } else { 2 });
        for offset in offsets {
            if long {
                // `glyf` is built in memory, so its length fits 32 bits for
                // any face `loca` can address.
                loca.extend_from_slice(&u32::try_from(offset).unwrap_or(u32::MAX).to_be_bytes());
            } else {
                loca.extend_from_slice(
                    &u16::try_from(offset / 2).unwrap_or(u16::MAX).to_be_bytes(),
                );
            }
        }
        (glyf, loca, long)
    }
}

/// One glyph at the location: its points or its offsets moved by `gvar` and
/// rounded.
fn varied_glyph(
    sfnt: &Sfnt,
    gvar: Option<&Gvar>,
    location: &Location,
    glyph: u16,
) -> Result<Glyph, InstanceError> {
    let Some(data) = glyf::glyph_data(sfnt, glyph) else {
        return Ok(Glyph::Empty);
    };
    if data.is_empty() {
        return Ok(Glyph::Empty);
    }
    let Some(contours) = be16(data, 0).map(|v| v as i16) else {
        return Ok(Glyph::Opaque(data.to_vec()));
    };
    let refused = |error: VariationError| InstanceError::Glyph { glyph, error };
    if contours < 0 {
        let (records, instructions) = glyf::composite_parts(data);
        if records.is_empty() {
            return Ok(Glyph::Opaque(data.to_vec()));
        }
        let deltas = match gvar {
            Some(gvar) => gvar
                .composite_deltas(glyph, records.len(), location)
                .map_err(refused)?,
            None => Vec::new(),
        };
        let mut varied = Vec::with_capacity(records.len());
        for (index, record) in records.iter().enumerate() {
            let mut record = *record;
            if record.offset_is_xy() {
                if let Some(delta) = deltas.get(index) {
                    for axis in 0..2 {
                        let moved = Fixed::from_int(record.arguments[axis])
                            .saturating_add(delta[axis])
                            .round();
                        record.arguments[axis] = i32::try_from(moved)
                            .ok()
                            .filter(|v| i16::try_from(*v).is_ok())
                            .ok_or(InstanceError::CoordinateRange { glyph })?;
                    }
                }
            }
            varied.push(record);
        }
        return Ok(Glyph::Composite {
            records: varied,
            instructions: instructions.to_vec(),
        });
    }
    if contours == 0 {
        return Ok(Glyph::Empty);
    }
    let Some(raw) = glyf::simple_glyph(sfnt, glyph) else {
        return Ok(Glyph::Opaque(data.to_vec()));
    };
    let deltas = match gvar {
        Some(gvar) => gvar.simple_deltas(glyph, &raw, location).map_err(refused)?,
        None => Vec::new(),
    };
    // The contours the outline reader draws, and the points they hold.
    let drawn = raw.contours();
    let Some(&(_, last)) = drawn.last() else {
        return Ok(Glyph::Empty);
    };
    let mut points = Vec::with_capacity(last + 1);
    for (index, point) in raw.points.iter().take(last + 1).enumerate() {
        let delta = deltas.get(index).copied().unwrap_or([Fixed::ZERO; 2]);
        let mut moved = [0i32; 2];
        for axis in 0..2 {
            let value = Fixed::from_int(point[axis])
                .saturating_add(delta[axis])
                .round();
            moved[axis] = i32::try_from(value)
                .ok()
                .filter(|v| i16::try_from(*v).is_ok())
                .ok_or(InstanceError::CoordinateRange { glyph })?;
        }
        points.push((moved[0], moved[1]));
    }
    // The difference between consecutive points is stored in sixteen bits.
    let mut previous = (0i32, 0i32);
    for point in &points {
        if i16::try_from(point.0 - previous.0).is_err()
            || i16::try_from(point.1 - previous.1).is_err()
        {
            return Err(InstanceError::CoordinateRange { glyph });
        }
        previous = *point;
    }
    let mut flags: Vec<u8> = raw.flags.iter().take(last + 1).map(|f| f & 0x01).collect();
    if let (Some(first), Some(original)) = (flags.first_mut(), raw.flags.first()) {
        *first |= original & 0x40; // OVERLAP_SIMPLE
    }
    Ok(Glyph::Simple {
        // A contour's last point was read from sixteen bits.
        ends: drawn
            .iter()
            .map(|(_, end)| u16::try_from(*end).unwrap_or(u16::MAX))
            .collect(),
        flags,
        points,
        instructions: raw.instructions.to_vec(),
    })
}

/// Appends a simple glyph (`glyf`, 'Simple glyph description'): each
/// coordinate the difference from the point before it, a byte with its sign
/// in the flag where it fits one, nothing where it is zero, and a word
/// otherwise; a flag repeated by `REPEAT` where the next ones are the same.
fn encode_simple(
    out: &mut Vec<u8>,
    ends: &[u16],
    on: &[u8],
    points: &[(i32, i32)],
    instructions: &[u8],
    bounds: [i16; 4],
) {
    // A contour count, the instruction length and every difference were
    // held to their widths when the glyph was varied.
    out.extend_from_slice(&i16::try_from(ends.len()).unwrap_or(i16::MAX).to_be_bytes());
    for value in bounds {
        out.extend_from_slice(&value.to_be_bytes());
    }
    for end in ends {
        out.extend_from_slice(&end.to_be_bytes());
    }
    out.extend_from_slice(
        &u16::try_from(instructions.len())
            .unwrap_or(u16::MAX)
            .to_be_bytes(),
    );
    out.extend_from_slice(instructions);

    let mut flags = Vec::with_capacity(points.len());
    let mut xs = Vec::new();
    let mut ys = Vec::new();
    let mut previous = (0i32, 0i32);
    for (index, point) in points.iter().enumerate() {
        let mut flag = on.get(index).copied().unwrap_or(1) & 0x41;
        for (axis, delta) in [(0usize, point.0 - previous.0), (1, point.1 - previous.1)] {
            let (short, same, bytes) = if axis == 0 {
                (0x02, 0x10, &mut xs)
            } else {
                (0x04, 0x20, &mut ys)
            };
            if delta == 0 {
                flag |= same;
            } else if delta.unsigned_abs() <= 255 {
                flag |= short;
                if delta > 0 {
                    flag |= same;
                }
                // At most 255, so the byte holds it.
                bytes.push(u8::try_from(delta.unsigned_abs()).unwrap_or(u8::MAX));
            } else {
                bytes.extend_from_slice(&i16::try_from(delta).unwrap_or(0).to_be_bytes());
            }
        }
        flags.push(flag);
        previous = *point;
    }
    let mut at = 0;
    while at < flags.len() {
        let flag = flags[at];
        let mut run = 1;
        while at + run < flags.len() && flags[at + run] == flag && run < 256 {
            run += 1;
        }
        if run > 1 {
            out.push(flag | 0x08);
            out.push(u8::try_from(run - 1).unwrap_or(u8::MAX));
        } else {
            out.push(flag);
        }
        at += run;
    }
    out.extend(xs);
    out.extend(ys);
}

/// Appends a composite glyph (`glyf`, 'Composite glyph description'): each
/// record with `MORE_COMPONENTS` but the last, its arguments in bytes where
/// both fit one and in words otherwise, its one transform as its flags
/// order them, and the instructions after the last record, which then
/// carries `WE_HAVE_INSTRUCTIONS`.
fn encode_composite(
    out: &mut Vec<u8>,
    records: &[Component],
    instructions: &[u8],
    bounds: [i16; 4],
) {
    out.extend_from_slice(&(-1i16).to_be_bytes());
    for value in bounds {
        out.extend_from_slice(&value.to_be_bytes());
    }
    for (index, record) in records.iter().enumerate() {
        let last = index + 1 == records.len();
        let mut flags = record.flags
            & !(Component::ARG_1_AND_2_ARE_WORDS
                | Component::MORE_COMPONENTS
                | Component::WE_HAVE_INSTRUCTIONS);
        // One transform, as the reader takes the first of the three flags.
        if flags & Component::WE_HAVE_A_SCALE != 0 {
            flags &= !(Component::WE_HAVE_AN_X_AND_Y_SCALE | Component::WE_HAVE_A_TWO_BY_TWO);
        } else if flags & Component::WE_HAVE_AN_X_AND_Y_SCALE != 0 {
            flags &= !Component::WE_HAVE_A_TWO_BY_TWO;
        }
        if !last {
            flags |= Component::MORE_COMPONENTS;
        }
        if last && !instructions.is_empty() {
            flags |= Component::WE_HAVE_INSTRUCTIONS;
        }
        let [a1, a2] = record.arguments;
        let bytes = if record.offset_is_xy() {
            i8::try_from(a1).is_ok() && i8::try_from(a2).is_ok()
        } else {
            u8::try_from(a1).is_ok() && u8::try_from(a2).is_ok()
        };
        if !bytes {
            flags |= Component::ARG_1_AND_2_ARE_WORDS;
        }
        out.extend_from_slice(&flags.to_be_bytes());
        out.extend_from_slice(&record.glyph.to_be_bytes());
        // Each argument was held to sixteen bits when it was varied, and a
        // point number was read from sixteen.
        if bytes {
            out.push(a1 as u8);
            out.push(a2 as u8);
        } else {
            out.extend_from_slice(&(a1 as u16).to_be_bytes());
            out.extend_from_slice(&(a2 as u16).to_be_bytes());
        }
        let [xscale, scale01, scale10, yscale] = record.transform;
        if flags & Component::WE_HAVE_A_SCALE != 0 {
            out.extend_from_slice(&xscale.to_be_bytes());
        } else if flags & Component::WE_HAVE_AN_X_AND_Y_SCALE != 0 {
            out.extend_from_slice(&xscale.to_be_bytes());
            out.extend_from_slice(&yscale.to_be_bytes());
        } else if flags & Component::WE_HAVE_A_TWO_BY_TWO != 0 {
            for value in [xscale, scale01, scale10, yscale] {
                out.extend_from_slice(&value.to_be_bytes());
            }
        }
    }
    if !instructions.is_empty() {
        out.extend_from_slice(
            &u16::try_from(instructions.len())
                .unwrap_or(u16::MAX)
                .to_be_bytes(),
        );
        out.extend_from_slice(instructions);
    }
}

/// The instance's horizontal and vertical metrics: each glyph's advance and
/// side bearing, by glyph.
struct Metrics {
    /// `(advance, left side bearing)`, where the face has `hhea` and `hmtx`.
    horizontal: Option<Vec<(u16, i16)>>,
    /// `(advance height, top side bearing)`, where it has `vhea` and `vmtx`.
    vertical: Option<Vec<(u16, i16)>>,
    /// Each glyph's box, where the instance has outlines it measured.
    boxes: Vec<Option<[i16; 4]>>,
}

impl Metrics {
    fn read(
        sfnt: &Sfnt,
        tables: &Tables,
        boxes: Vec<Option<[i16; 4]>>,
        vertical_origin: &dyn Fn(u16) -> Result<Option<i64>, InstanceError>,
        location: &Location,
        count: u16,
    ) -> Result<Metrics, InstanceError> {
        let gvar = tables.gvar.as_ref();
        let has = |tag: Tag| sfnt.table(tag_value(tag)).is_some();
        let phantoms = |glyph: u16| -> Result<[[Fixed; 2]; 4], InstanceError> {
            glyf::phantom_points_at(sfnt, glyph, gvar, location)
                .map_err(|error| InstanceError::Glyph { glyph, error })
        };

        let horizontal = if has(TAG_HHEA) && has(TAG_HMTX) && sfnt.advance(0).is_some() {
            let scalars = tables.hvar.as_ref().map(|hvar| hvar.scalars(location));
            let mut out = Vec::with_capacity(usize::from(count));
            for glyph in 0..count {
                let advance = match (&tables.hvar, &scalars) {
                    (Some(hvar), Some(scalars)) => {
                        let default = sfnt.advance(glyph).unwrap_or(0);
                        Fixed::from_int(i32::from(default))
                            .saturating_add(hvar.advance_delta(glyph, scalars)?)
                    }
                    _ if gvar.is_some() => {
                        let p = phantoms(glyph)?;
                        p[1][0].saturating_sub(p[0][0])
                    }
                    _ => Fixed::from_int(i32::from(sfnt.advance(glyph).unwrap_or(0))),
                };
                let lsb = match boxes.get(usize::from(glyph)).copied().flatten() {
                    Some(bounds) => bounds[0],
                    None => sfnt.left_side_bearing(glyph).unwrap_or(0),
                };
                out.push((clamp_u16(advance.round()), lsb));
            }
            Some(out)
        } else {
            None
        };

        let vertical = if has(TAG_VHEA) && has(TAG_VMTX) && sfnt.vertical_advance(0).is_some() {
            let scalars = tables.vvar.as_ref().map(|vvar| vvar.scalars(location));
            let mut out = Vec::with_capacity(usize::from(count));
            for glyph in 0..count {
                let p = if gvar.is_some() {
                    Some(phantoms(glyph)?)
                } else {
                    None
                };
                let height = match (&tables.vvar, &scalars, &p) {
                    (Some(vvar), Some(scalars), _) => {
                        let default = sfnt.vertical_advance(glyph).unwrap_or(0);
                        Fixed::from_int(i32::from(default))
                            .saturating_add(vvar.advance_delta(glyph, scalars)?)
                    }
                    (_, _, Some(p)) => p[2][1].saturating_sub(p[3][1]),
                    _ => Fixed::from_int(i32::from(sfnt.vertical_advance(glyph).unwrap_or(0))),
                };
                // Each top side bearing is measured from the glyph's vertical
                // origin at the location down to its top; a face whose
                // origins do not move keeps its own bearings.
                let top = boxes
                    .get(usize::from(glyph))
                    .copied()
                    .flatten()
                    .map_or(0, |b| i64::from(b[3]));
                let tsb = match vertical_origin(glyph)? {
                    Some(origin) => clamp_i16(origin - top),
                    None => sfnt.top_side_bearing(glyph).unwrap_or(0),
                };
                out.push((clamp_u16(height.round()), tsb));
            }
            Some(out)
        } else {
            None
        };
        Ok(Metrics {
            horizontal,
            vertical,
            boxes,
        })
    }

    /// Each glyph's width as a CFF charstring states it: its advance, or
    /// `None` for an advance of zero, which is the default width.
    fn widths(&self) -> Vec<Option<f64>> {
        self.horizontal.as_ref().map_or_else(Vec::new, |metrics| {
            metrics
                .iter()
                .map(|(advance, _)| (*advance != 0).then(|| f64::from(*advance)))
                .collect()
        })
    }

    /// The mean of the non-zero advances, rounded, a half rounding up — the
    /// `OS/2` chapter's `xAvgCharWidth` from version 3 on.
    fn average_width(&self) -> Option<i16> {
        let advances: Vec<i64> = self
            .horizontal
            .as_ref()?
            .iter()
            .map(|m| i64::from(m.0))
            .filter(|a| *a != 0)
            .collect();
        let n = i64::try_from(advances.len()).ok().filter(|n| *n > 0)?;
        let sum: i64 = advances.iter().sum();
        Some(clamp_i16((2 * sum + n).div_euclid(2 * n)))
    }

    /// Writes `hmtx` and `hhea`, and `vmtx` and `vhea`, where the face has
    /// them.
    ///
    /// # Errors
    ///
    /// `MVAR`'s refusals for the fields the headers carry.
    fn write(
        &self,
        sfnt: &Sfnt,
        tables: &Tables,
        location: &Location,
        out: &mut Vec<(u32, Vec<u8>)>,
    ) -> Result<(), InstanceError> {
        let pairs = [
            (TAG_HHEA, TAG_HMTX, &self.horizontal, 0usize),
            (TAG_VHEA, TAG_VMTX, &self.vertical, 1usize),
        ];
        for (header_tag, table_tag, metrics, axis) in pairs {
            let Some(header) = sfnt.table(tag_value(header_tag)) else {
                // No header to count the metrics with: the table, if there
                // is one, is copied as the face holds it.
                if let Some(table) = sfnt.table(tag_value(table_tag)) {
                    out.push((tag_value(table_tag), table.to_vec()));
                }
                continue;
            };
            let Some(metrics) = metrics else {
                out.push((tag_value(header_tag), header.to_vec()));
                if let Some(table) = sfnt.table(tag_value(table_tag)) {
                    out.push((tag_value(table_tag), table.to_vec()));
                }
                continue;
            };
            // The long metrics stop where the advances stop changing.
            let mut long = metrics.len();
            while long > 1 && metrics[long - 1].0 == metrics[long - 2].0 {
                long -= 1;
            }
            let mut table = Vec::with_capacity(long * 4 + (metrics.len() - long) * 2);
            for (index, (advance, bearing)) in metrics.iter().enumerate() {
                if index < long {
                    table.extend_from_slice(&advance.to_be_bytes());
                }
                table.extend_from_slice(&bearing.to_be_bytes());
            }
            let mut header = header.to_vec();
            tables.vary_fields(sfnt, header_tag, location, &mut header)?;
            // The extents are over the glyphs with contours, as both
            // chapters define them.
            let drawn: Vec<(usize, [i16; 4])> = self
                .boxes
                .iter()
                .enumerate()
                .filter_map(|(g, b)| b.map(|b| (g, b)))
                .collect();
            let advance_max = metrics.iter().map(|m| m.0).max().unwrap_or(0);
            let (start, end, extent): (Vec<i64>, Vec<i64>, Vec<i64>) = {
                let mut starts = Vec::new();
                let mut ends = Vec::new();
                let mut extents = Vec::new();
                for (glyph, bounds) in &drawn {
                    let (advance, bearing) = metrics[*glyph];
                    let (low, high) = if axis == 0 {
                        (i64::from(bounds[0]), i64::from(bounds[2]))
                    } else {
                        (i64::from(bounds[1]), i64::from(bounds[3]))
                    };
                    starts.push(i64::from(bearing));
                    let size = high - low;
                    ends.push(i64::from(advance) - i64::from(bearing) - size);
                    extents.push(i64::from(bearing) + size);
                }
                (starts, ends, extents)
            };
            put16(&mut header, 10, advance_max);
            if !drawn.is_empty() {
                put16(
                    &mut header,
                    12,
                    clamp_i16(start.iter().copied().min().unwrap_or(0)) as u16,
                );
                put16(
                    &mut header,
                    14,
                    clamp_i16(end.iter().copied().min().unwrap_or(0)) as u16,
                );
                put16(
                    &mut header,
                    16,
                    clamp_i16(extent.iter().copied().max().unwrap_or(0)) as u16,
                );
            }
            put16(&mut header, 34, u16::try_from(long).unwrap_or(u16::MAX));
            out.push((tag_value(header_tag), header));
            out.push((tag_value(table_tag), table));
        }
        Ok(())
    }
}

/// The instance's outlines: `glyf`'s, `CFF2`'s, or none — a face whose
/// outlines are in a table that does not vary, copied as it is.
enum Outlines<'a> {
    Glyf(GlyfInstance),
    Cff2(Cff2Instance<'a>),
    None,
}

impl Outlines<'_> {
    /// Each glyph's box at the location, `None` for one that draws nothing,
    /// one a glyph of `maxp`'s count.
    fn boxes(&self, count: u16) -> Vec<Option<[i16; 4]>> {
        let mut out = match self {
            Outlines::Glyf(glyphs) => glyphs.boxes.clone(),
            Outlines::Cff2(cff2) => cff2.boxes.clone(),
            Outlines::None => Vec::new(),
        };
        out.resize(usize::from(count), None);
        out
    }
}

/// The union of every box, or `None` where no glyph draws.
fn union_of(boxes: &[Option<[i16; 4]>]) -> Option<[i16; 4]> {
    let drawn: Vec<&[i16; 4]> = boxes.iter().flatten().collect();
    if drawn.is_empty() {
        return None;
    }
    Some([
        drawn.iter().map(|b| b[0]).min().unwrap_or(0),
        drawn.iter().map(|b| b[1]).min().unwrap_or(0),
        drawn.iter().map(|b| b[2]).max().unwrap_or(0),
        drawn.iter().map(|b| b[3]).max().unwrap_or(0),
    ])
}

/// A box in `f64` widened to hold one point more.
fn include(bounds: &mut Option<[f64; 4]>, x: f64, y: f64) {
    *bounds = Some(match *bounds {
        None => [x, y, x, y],
        Some([x0, y0, x1, y1]) => [x0.min(x), y0.min(y), x1.max(x), y1.max(y)],
    });
}

/// Where a quadratic's coordinate turns, as `t` in (0, 1), if it does.
fn quadratic_turn(p0: f64, c: f64, p1: f64) -> Option<f64> {
    let denominator = p0 - 2.0 * c + p1;
    if denominator == 0.0 {
        return None;
    }
    let t = (p0 - c) / denominator;
    (t > 0.0 && t < 1.0).then_some(t)
}

/// Where a cubic's coordinate turns, as up to two `t` in (0, 1): the roots
/// of its derivative, `a t² + b t + c` over the control polygon.
fn cubic_turns(p0: f64, p1: f64, p2: f64, p3: f64) -> [Option<f64>; 2] {
    let a = -p0 + 3.0 * p1 - 3.0 * p2 + p3;
    let b = 2.0 * (p0 - 2.0 * p1 + p2);
    let c = p1 - p0;
    let inside = |t: f64| (t > 0.0 && t < 1.0).then_some(t);
    if a == 0.0 {
        if b == 0.0 {
            return [None, None];
        }
        return [inside(-c / b), None];
    }
    let discriminant = b * b - 4.0 * a * c;
    if discriminant < 0.0 {
        return [None, None];
    }
    // `sqrt` is correctly rounded on every target (ruling 4).
    let root = discriminant.sqrt();
    [
        inside((-b + root) / (2.0 * a)),
        inside((-b - root) / (2.0 * a)),
    ]
}

/// An outline's tight box — its curves' turning points, not their control
/// points — as a CFF face's metrics measure it; `None` for an outline that
/// draws nothing.
fn outline_box(outline: &Outline) -> Option<[f64; 4]> {
    let mut bounds = None;
    let mut current = (0.0, 0.0);
    let mut start: Option<(f64, f64)> = None;
    for segment in &outline.segments {
        // A contour's start counts once the contour draws.
        let mut drawing = |bounds: &mut Option<[f64; 4]>| {
            if let Some((x, y)) = start.take() {
                include(bounds, x, y);
            }
        };
        match *segment {
            Segment::MoveTo { x, y } => {
                current = (x, y);
                start = Some((x, y));
            }
            Segment::LineTo { x, y } => {
                drawing(&mut bounds);
                include(&mut bounds, x, y);
                current = (x, y);
            }
            Segment::QuadTo { cx, cy, x, y } => {
                drawing(&mut bounds);
                let (x0, y0) = current;
                let at = |t: f64, a: f64, c: f64, b: f64| {
                    (1.0 - t) * (1.0 - t) * a + 2.0 * (1.0 - t) * t * c + t * t * b
                };
                if let Some(t) = quadratic_turn(x0, cx, x) {
                    include(&mut bounds, at(t, x0, cx, x), at(t, y0, cy, y));
                }
                if let Some(t) = quadratic_turn(y0, cy, y) {
                    include(&mut bounds, at(t, x0, cx, x), at(t, y0, cy, y));
                }
                include(&mut bounds, x, y);
                current = (x, y);
            }
            Segment::CurveTo {
                c1x,
                c1y,
                c2x,
                c2y,
                x,
                y,
            } => {
                drawing(&mut bounds);
                let (x0, y0) = current;
                let at = |t: f64, a: f64, b: f64, c: f64, d: f64| {
                    let s = 1.0 - t;
                    s * s * s * a + 3.0 * s * s * t * b + 3.0 * s * t * t * c + t * t * t * d
                };
                for t in cubic_turns(x0, c1x, c2x, x)
                    .into_iter()
                    .chain(cubic_turns(y0, c1y, c2y, y))
                    .flatten()
                {
                    include(&mut bounds, at(t, x0, c1x, c2x, x), at(t, y0, c1y, c2y, y));
                }
                include(&mut bounds, x, y);
                current = (x, y);
            }
            Segment::Close => {}
        }
    }
    bounds
}

/// A box floored and ceiled to font units, held to sixteen bits.
fn whole_box(bounds: [f64; 4], glyph: u16) -> Result<[i16; 4], InstanceError> {
    let edge = |value: f64| -> Result<i16, InstanceError> {
        if (f64::from(i16::MIN)..=f64::from(i16::MAX)).contains(&value) {
            // Inside the range and integral, so the cast is exact.
            Ok(value as i16)
        } else {
            Err(InstanceError::CoordinateRange { glyph })
        }
    };
    Ok([
        edge(bounds[0].floor())?,
        edge(bounds[1].floor())?,
        edge(bounds[2].ceil())?,
        edge(bounds[3].ceil())?,
    ])
}

/// The Private DICT operators a CFF carries that a CFF2 Private DICT can:
/// the alignment zones, the standard stems, the snaps, the blue scale, shift
/// and fuzz, and the language group and expansion factor.
const PRIVATE_OPERATORS: &[u16] = &[
    6, 7, 8, 9, 10, 11, 0x0C09, 0x0C0A, 0x0C0B, 0x0C0C, 0x0C0D, 0x0C11, 0x0C12,
];

/// A CFF Private DICT of blended entries: the operators both formats share,
/// with no `Subrs` — the charstrings call none — and no `vsindex`.
fn private_dict(entries: &[(u16, Vec<f64>)]) -> Vec<u8> {
    let mut out = Vec::new();
    for (op, operands) in entries {
        if !PRIVATE_OPERATORS.contains(op) {
            continue;
        }
        for value in operands {
            out.extend(dict_operand(*value));
        }
        out.extend(dict_op(*op));
    }
    out
}

/// A `name` table string by name ID: a Windows or Unicode record's
/// UTF-16BE, or a Macintosh Roman one's ASCII.
fn name_string(sfnt: &Sfnt, id: u16) -> Option<String> {
    let name = sfnt.table(tag_value(TAG_NAME))?;
    let count = usize::from(be16(name, 2)?);
    let storage = usize::from(be16(name, 4)?);
    let mut fallback = None;
    for record in 0..count {
        let at = 6 + record * 12;
        let (platform, encoding) = (be16(name, at)?, be16(name, at + 2)?);
        if be16(name, at + 6)? != id {
            continue;
        }
        let length = usize::from(be16(name, at + 8)?);
        let offset = usize::from(be16(name, at + 10)?);
        let Some(bytes) = name.get(storage + offset..storage + offset + length) else {
            continue;
        };
        if platform == 3 || platform == 0 {
            let units: Vec<u16> = bytes
                .chunks_exact(2)
                .map(|b| u16::from_be_bytes([b[0], b[1]]))
                .collect();
            if let Ok(text) = String::from_utf16(&units) {
                return Some(text);
            }
        } else if platform == 1 && encoding == 0 && bytes.is_ascii() {
            fallback = core::str::from_utf8(bytes).ok().map(str::to_string);
        }
    }
    fallback
}

/// The instance's PostScript name for its CFF's Name INDEX: a named
/// instance's own where `fvar` gives it one, else the face's name ID 6, kept
/// to the characters a PostScript name may hold and to 63 of them.
fn font_name(sfnt: &Sfnt, space: &DesignSpace, instance: &Instance) -> Vec<u8> {
    let named = match instance {
        Instance::Named(index) => space
            .instances()
            .get(usize::from(*index))
            .and_then(|named| named.postscript_name_id)
            .filter(|id| *id != 0xFFFF),
        Instance::User(_) => None,
    };
    let text = named
        .and_then(|id| name_string(sfnt, id))
        .or_else(|| name_string(sfnt, 6))
        .unwrap_or_default();
    let name: Vec<u8> = text
        .bytes()
        .filter(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
        .take(63)
        .collect();
    if name.is_empty() {
        b"Instance".to_vec()
    } else {
        name
    }
}

/// What a CFF table for an instance is made of.
struct CffParts<'p> {
    name: Vec<u8>,
    charstrings: Vec<Vec<u8>>,
    /// One Private DICT a Font DICT.
    privates: Vec<Vec<u8>>,
    /// Each glyph's Font DICT.
    fd_select: Vec<u8>,
    /// Each glyph's name, for a name-keyed CFF; `None` for a CID-keyed one.
    names: Option<Vec<&'p str>>,
    bbox: [i16; 4],
    matrix: [f64; 6],
}

/// The default `FontMatrix`, which a Top DICT need not state.
const DEFAULT_MATRIX: [f64; 6] = [0.001, 0.0, 0.0, 0.001, 0.0, 0.0];

/// A CFF table (Adobe Technical Note 5176): a four-byte header, the Name,
/// Top DICT, String and Global Subr INDEXes, the charset, FDSelect for a
/// CID-keyed font, the CharStrings INDEX, the Private DICTs and the FDArray
/// — every offset in the Top DICT a five-byte integer, so the DICT's length
/// is known before the offsets in it are. `None` past what an INDEX holds.
fn cff_table(parts: &CffParts) -> Option<Vec<u8>> {
    let count = parts.charstrings.len();
    let cid = parts.names.is_none();
    let mut strings: Vec<Vec<u8>> = Vec::new();
    let charset: Vec<u8> = match &parts.names {
        None => {
            // ROS's registry and ordering are the first strings, SIDs 391
            // and 392; every glyph after `.notdef` is the CID its index is,
            // one range.
            strings.push(b"Adobe".to_vec());
            strings.push(b"Identity".to_vec());
            if count >= 2 {
                let left = u16::try_from(count - 2).ok()?;
                let mut out = vec![2, 0, 1];
                out.extend_from_slice(&left.to_be_bytes());
                out
            } else {
                vec![0]
            }
        }
        Some(names) => {
            let mut out = vec![0];
            for name in names.iter().skip(1) {
                let sid = match STANDARD_STRINGS.iter().position(|s| s == name) {
                    Some(sid) => sid,
                    None => {
                        let own = match strings.iter().position(|s| s == name.as_bytes()) {
                            Some(at) => at,
                            None => {
                                strings.push(name.as_bytes().to_vec());
                                strings.len() - 1
                            }
                        };
                        STANDARD_STRINGS.len() + own
                    }
                };
                out.extend_from_slice(&u16::try_from(sid).ok()?.to_be_bytes());
            }
            out
        }
    };
    let fd_select: Option<Vec<u8>> = if cid {
        // Format 3: a range wherever the Font DICT changes, then the
        // sentinel.
        let mut ranges: Vec<(u16, u8)> = Vec::new();
        for (glyph, fd) in parts.fd_select.iter().enumerate() {
            if ranges.last().map(|r| r.1) != Some(*fd) {
                ranges.push((u16::try_from(glyph).ok()?, *fd));
            }
        }
        let mut out = vec![3];
        out.extend_from_slice(&u16::try_from(ranges.len()).ok()?.to_be_bytes());
        for (first, fd) in ranges {
            out.extend_from_slice(&first.to_be_bytes());
            out.push(fd);
        }
        out.extend_from_slice(&u16::try_from(count).ok()?.to_be_bytes());
        Some(out)
    } else {
        None
    };

    let name_index = write_index(&[parts.name.clone()])?;
    let string_index = write_index(&strings)?;
    let gsubrs = write_index(&[])?;
    let charstrings = write_index(&parts.charstrings)?;

    let top = |charset_at: usize,
               charstrings_at: usize,
               private: (usize, usize),
               fd_array_at: usize,
               fd_select_at: usize|
     -> Option<Vec<u8>> {
        let int = |value: usize| -> Option<[u8; 5]> { Some(dict_int(i32::try_from(value).ok()?)) };
        let mut out = Vec::new();
        if cid {
            out.extend(int(391)?);
            out.extend(int(392)?);
            out.extend(int(0)?);
            out.extend(dict_op(0x0C1E)); // ROS
            out.extend(int(count)?);
            out.extend(dict_op(0x0C22)); // CIDCount
        }
        for value in parts.bbox {
            out.extend(dict_int(i32::from(value)));
        }
        out.push(5); // FontBBox
        if parts.matrix != DEFAULT_MATRIX {
            for value in parts.matrix {
                out.extend(dict_operand(value));
            }
            out.extend(dict_op(0x0C07)); // FontMatrix
        }
        out.extend(int(charset_at)?);
        out.push(15); // charset
        out.extend(int(charstrings_at)?);
        out.push(17); // CharStrings
        if cid {
            out.extend(int(fd_array_at)?);
            out.extend(dict_op(0x0C24)); // FDArray
            out.extend(int(fd_select_at)?);
            out.extend(dict_op(0x0C25)); // FDSelect
        } else {
            out.extend(int(private.0)?);
            out.extend(int(private.1)?);
            out.push(18); // Private
        }
        Some(out)
    };
    let top_len = top(0, 0, (0, 0), 0, 0)?.len();
    let top_index_len = write_index(&[vec![0; top_len]])?.len();

    let mut at = 4 + name_index.len() + top_index_len + string_index.len() + gsubrs.len();
    let charset_at = at;
    at += charset.len();
    let fd_select_at = at;
    at += fd_select.as_ref().map_or(0, Vec::len);
    let charstrings_at = at;
    at += charstrings.len();
    let mut private_at: Vec<(usize, usize)> = Vec::with_capacity(parts.privates.len());
    for private in &parts.privates {
        private_at.push((private.len(), at));
        at += private.len();
    }
    let fd_array_at = at;
    let fd_array = if cid {
        let mut dicts = Vec::with_capacity(private_at.len());
        for (size, offset) in &private_at {
            let mut dict = Vec::new();
            dict.extend(dict_int(i32::try_from(*size).ok()?));
            dict.extend(dict_int(i32::try_from(*offset).ok()?));
            dict.push(18); // Private
            dicts.push(dict);
        }
        write_index(&dicts)?
    } else {
        Vec::new()
    };
    let top_dict = top(
        charset_at,
        charstrings_at,
        private_at.first().copied().unwrap_or((0, at)),
        fd_array_at,
        fd_select_at,
    )?;
    let top_index = write_index(&[top_dict])?;
    if top_index.len() != top_index_len {
        return None;
    }

    let mut out = Vec::with_capacity(at + fd_array.len());
    // Major 1, minor 0, a four-byte header, and offsets of four bytes.
    out.extend_from_slice(&[1, 0, 4, 4]);
    out.extend(name_index);
    out.extend(top_index);
    out.extend(string_index);
    out.extend(gsubrs);
    out.extend(charset);
    if let Some(fd_select) = fd_select {
        out.extend(fd_select);
    }
    out.extend(charstrings);
    for private in &parts.privates {
        out.extend_from_slice(private);
    }
    if out.len() != fd_array_at {
        return None;
    }
    out.extend(fd_array);
    Some(out)
}

/// A CFF2 face's outlines at the location, read once: each glyph's outline
/// and box there, and where its vertical origin stands at the default.
struct Cff2Instance<'a> {
    cff2: Cff2<'a>,
    blends: Blends,
    outlines: Vec<Outline>,
    /// One a glyph of `maxp`'s count, `None` past the `CFF2`'s glyphs.
    boxes: Vec<Option<[i16; 4]>>,
    /// Each glyph's top at the default instance.
    default_tops: Vec<Option<i64>>,
}

impl<'a> Cff2Instance<'a> {
    fn read(
        table: &'a [u8],
        space: &DesignSpace,
        location: &Location,
        count: u16,
    ) -> Result<Cff2Instance<'a>, InstanceError> {
        let cff2 = Cff2::parse(table).map_err(InstanceError::Cff2)?;
        let blends = cff2
            .blends_in(space, location)
            .map_err(InstanceError::Cff2)?;
        let glyphs = cff2.glyph_count();
        if glyphs > usize::from(u16::MAX) {
            return Err(InstanceError::TooManyGlyphs { count: glyphs });
        }
        let mut machine = Machine::new();
        let mut outlines = Vec::with_capacity(glyphs);
        let mut boxes = Vec::with_capacity(glyphs);
        let mut default_tops = Vec::with_capacity(glyphs);
        for index in 0..glyphs {
            // At most 65 535 glyphs, held above.
            let glyph = u16::try_from(index).unwrap_or(u16::MAX);
            let refused = |error: Cff2Error| InstanceError::Cff2Glyph { glyph, error };
            let mut outline = Outline::default();
            cff2.outline_with(glyph, &blends, &mut outline, &mut machine)
                .map_err(refused)?;
            boxes.push(match outline_box(&outline) {
                Some(bounds) => Some(whole_box(bounds, glyph)?),
                None => None,
            });
            let default = cff2.outline(glyph).map_err(refused)?;
            default_tops.push(outline_box(&default).map(|b| b[3].ceil() as i64));
            outlines.push(outline);
        }
        boxes.resize(boxes.len().max(usize::from(count)), None);
        default_tops.resize(default_tops.len().max(usize::from(count)), None);
        Ok(Cff2Instance {
            cff2,
            blends,
            outlines,
            boxes,
            default_tops,
        })
    }

    /// A glyph's vertical origin at the location: its `VORG` origin moved by
    /// `VVAR` where the face has a `VORG`, and otherwise the origin the
    /// default instance gives it — its top there plus its top side bearing —
    /// which the instance keeps; `None` where there is neither.
    fn vertical_origin(
        &self,
        sfnt: &Sfnt,
        tables: &Tables,
        scalars: Option<&[Fixed]>,
        glyph: u16,
    ) -> Result<Option<i64>, InstanceError> {
        if let Some(origin) = sfnt.vorg_origin(glyph) {
            let delta = match (&tables.vvar, scalars) {
                (Some(vvar), Some(scalars)) => {
                    vvar.origin_delta(glyph, scalars)?.unwrap_or(Fixed::ZERO)
                }
                _ => Fixed::ZERO,
            };
            return Ok(Some(
                Fixed::from_int(i32::from(origin))
                    .saturating_add(delta)
                    .round(),
            ));
        }
        let top = self.default_tops.get(usize::from(glyph)).copied().flatten();
        Ok(match (top, sfnt.top_side_bearing(glyph)) {
            (Some(top), Some(tsb)) => Some(top + i64::from(tsb)),
            _ => None,
        })
    }

    /// The instance's `CFF ` table: each glyph's charstring at the location
    /// as Type 2 with its width, each Private DICT blended, name-keyed or
    /// CID-keyed by the overview's rule — and read back, every glyph held to
    /// the outline and the width it has here.
    fn write(
        &self,
        sfnt: &Sfnt,
        space: &DesignSpace,
        instance: &Instance,
        widths: &[Option<f64>],
        union: Option<[i16; 4]>,
    ) -> Result<Vec<u8>, InstanceError> {
        let glyphs = self.cff2.glyph_count();
        let mut charstrings = Vec::with_capacity(glyphs);
        let mut fd_select = Vec::with_capacity(glyphs);
        let font_dicts = self.cff2.font_dict_count();
        if font_dicts > 256 {
            return Err(InstanceError::TooManyFontDicts { count: font_dicts });
        }
        for index in 0..glyphs {
            // At most 65 535 glyphs, held when the outlines were read.
            let glyph = u16::try_from(index).unwrap_or(u16::MAX);
            let width = widths.get(index).copied().flatten();
            let charstring = self
                .cff2
                .type2_charstring(glyph, &self.blends, width)
                .map_err(|error| match error {
                    Type2Error::Charstring(error) => InstanceError::Cff2Glyph { glyph, error },
                    Type2Error::Operand => InstanceError::CharstringOperand { glyph },
                })?;
            charstrings.push(charstring);
            let fd = self
                .cff2
                .font_dict_for(glyph)
                .map_err(|error| InstanceError::Cff2Glyph { glyph, error })?;
            // At most 256 Font DICTs, held above.
            fd_select.push(u8::try_from(fd).unwrap_or(u8::MAX));
        }
        let mut privates = Vec::with_capacity(font_dicts);
        for fd in 0..font_dicts {
            // At most 256.
            let fd = u16::try_from(fd).unwrap_or(u16::MAX);
            let entries = self
                .cff2
                .private_dict_at(fd, &self.blends)
                .map_err(InstanceError::Cff2)?;
            privates.push(private_dict(&entries));
        }
        // Name-keyed where one Font DICT and `post` names every glyph apart.
        let names = if font_dicts == 1 {
            sfnt.post_names().filter(|names| {
                names.len() == glyphs
                    && names
                        .iter()
                        .enumerate()
                        .all(|(i, name)| !names[..i].contains(name))
            })
        } else {
            None
        };
        let parts = CffParts {
            name: font_name(sfnt, space, instance),
            charstrings,
            privates,
            fd_select,
            names,
            bbox: union.unwrap_or([0; 4]),
            matrix: self.cff2.font_matrix(),
        };
        let cff = cff_table(&parts).ok_or(InstanceError::TooManyGlyphs { count: glyphs })?;
        self.verify(&cff, widths)?;
        Ok(cff)
    }

    /// The `CFF ` table read back: as many glyphs, each drawing the outline
    /// the `CFF2` draws at the location, through its own Font DICT's matrix,
    /// and advancing its width.
    fn verify(&self, cff: &[u8], widths: &[Option<f64>]) -> Result<(), InstanceError> {
        let read = Cff::parse(cff).ok_or(InstanceError::CffRereading { glyph: 0 })?;
        if read.glyph_count() != self.outlines.len() {
            return Err(InstanceError::CffRereading { glyph: 0 });
        }
        let mut machine = Machine::new();
        let mut outline = Outline::default();
        for (index, expected) in self.outlines.iter().enumerate() {
            // At most 65 535 glyphs.
            let glyph = u16::try_from(index).unwrap_or(u16::MAX);
            let width = widths.get(index).copied().flatten().unwrap_or(0.0);
            if !read.outline_into(glyph, &mut outline, &mut machine)
                || outline.segments != expected.segments
                || read.advance(glyph) != Some(width)
                || read.font_matrix_for(glyph) != self.cff2.font_matrix()
            {
                return Err(InstanceError::CffRereading { glyph });
            }
        }
        Ok(())
    }
}
