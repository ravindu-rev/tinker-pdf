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
//!
//! # The raw-point form, and a variable face's instances
//!
//! [`simple_glyph`] gives a simple glyph as its entry states it: the contour
//! end indices, the flags with `REPEAT` expanded, the absolute coordinates in
//! font units, and the four phantom points after them ([`phantom_points`]),
//! where `docs/design/hinting.md`'s zone 1 places them. It is the one form a
//! glyph's points are read in — [`outline`] resolves its implied midpoints,
//! and a variable face's `gvar` moves it (`docs/design/variable-fonts.md`).
//! [`outline_at`] adds a glyph's deltas at a [`Location`] to those points
//! before the midpoints are resolved, and to a composite's component offsets,
//! through the same contour reading, so at a location where nothing moves it
//! draws exactly what [`outline`] draws. [`phantom_points_at`] gives the
//! advance and side bearings at a location where a face has no `HVAR`.

use crate::outline::{Outline, Segment};
use crate::sfnt::Sfnt;
use crate::variation::gvar::Gvar;
use crate::variation::{Fixed, Location, VariationError};

/// How deep a composite glyph may nest before recursion is refused. The
/// hinter's walk of a composite is held to it as this reading is.
pub(crate) const MAX_COMPONENT_DEPTH: u32 = 8;

/// How many component glyphs one outline may draw in total.
///
/// A depth cap alone is not enough: a composite that lists sixty-four
/// components, each referring back to itself, stays within eight levels while
/// asking for 64^8 glyphs. The budget bounds the *work*, which is the thing
/// that actually has to be finite.
pub(crate) const MAX_COMPONENTS: u32 = 256;

/// How many records of one composite are drawn.
pub(crate) const MAX_RECORDS: u32 = 64;

const TAG_GLYF: u32 = 0x676C_7966;
const TAG_HHEA: u32 = 0x6868_6561;

/// The identity placement, `[dx, dy, a, b, c, d]`.
const IDENTITY: [f64; 6] = [0.0, 0.0, 1.0, 0.0, 0.0, 1.0];

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
    let (start, end) = glyph_slot(sfnt, glyph)?;
    // An empty range is a glyph with no outline, such as a space.
    (end > start).then_some((start, end))
}

/// The two `loca` entries that bound a glyph's data, empty or not: `None`
/// only for a face whose `head` or `loca` cannot say, or a glyph past the
/// end of `loca`.
fn glyph_slot(sfnt: &Sfnt, glyph: u16) -> Option<(usize, usize)> {
    let head = sfnt.table(0x6865_6164)?;
    // head.indexToLocFormat: 0 means 16-bit offsets stored halved.
    let long_format = i16_at(head, 50)? != 0;
    let loca = sfnt.table(0x6C6F_6361)?;

    let index = usize::from(glyph);
    Some(if long_format {
        (
            be32(loca, index * 4)? as usize,
            be32(loca, index * 4 + 4)? as usize,
        )
    } else {
        (
            usize::from(be16(loca, index * 2)?) * 2,
            usize::from(be16(loca, index * 2 + 2)?) * 2,
        )
    })
}

/// A glyph's bounding box as its `glyf` header states it, in font units.
///
/// The four `FWORD`s every glyph's data begins with after its contour
/// count, simple and composite alike, read as written: nothing here
/// recomputes them from the points, because the `vmtx` clause defines a
/// glyph's vertical origin against *this* `yMax` and a face's top side
/// bearings were computed from it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Bounds {
    /// The leftmost x.
    pub x_min: i16,
    /// The lowest y.
    pub y_min: i16,
    /// The rightmost x.
    pub x_max: i16,
    /// The highest y: the top the `vmtx` clause adds a top side bearing to.
    pub y_max: i16,
}

/// A glyph's bounds from its `glyf` header.
///
/// A glyph with no outline — a space, whose `loca` range is empty — has the
/// origin point for its box, all four zero, so the vertical origin the `vmtx`
/// clause builds from it is its top side bearing alone. `None` for a face with
/// no `glyf`, `loca` or `head`, a glyph past the end of `loca`, and a glyph
/// whose data is too short to hold the header.
#[must_use]
pub fn bounds(sfnt: &Sfnt, glyph: u16) -> Option<Bounds> {
    let glyf = sfnt.table(TAG_GLYF)?;
    let (start, end) = glyph_slot(sfnt, glyph)?;
    if end <= start {
        // `glyph_range` reads the same two entries as no outline, and an
        // outline with nothing in it has nothing to bound.
        return Some(Bounds::default());
    }
    let data = glyf.get(start..end)?;
    Some(Bounds {
        x_min: i16_at(data, 2)?,
        y_min: i16_at(data, 4)?,
        x_max: i16_at(data, 6)?,
        y_max: i16_at(data, 8)?,
    })
}

/// A glyph's four phantom points at the default instance, in font units:
/// where `docs/design/hinting.md`'s zone 1 places them after the contour
/// points, in the order *Instructing TrueType Glyphs* numbers them.
///
/// - the horizontal origin, `(xMin − lsb, 0)`, its left side bearing `hmtx`'s;
/// - the advance width point, `hmtx`'s advance past it on the same line;
/// - the vertical origin, `(0, yMax + tsb)`, its top side bearing `vmtx`'s;
/// - the advance height point, `vmtx`'s advance height below it.
///
/// `xMin` and `yMax` are the header's ([`bounds`]), zero for a glyph with no
/// outline. A face without `vmtx` metrics for the glyph takes `hhea`'s
/// ascender as the vertical origin and its descender as the advance height
/// point; a face without `hmtx` ones puts the horizontal origin at `x = 0`
/// with no advance. The two vertical points stand at `x = 0` and the two
/// horizontal ones at `y = 0`, this engine's choice where the specification
/// gives the points no other coordinate.
#[must_use]
pub fn phantom_points(sfnt: &Sfnt, glyph: u16) -> [[i32; 2]; 4] {
    let bounds = bounds(sfnt, glyph).unwrap_or_default();
    let origin = sfnt
        .left_side_bearing(glyph)
        .map_or(0, |lsb| i32::from(bounds.x_min) - i32::from(lsb));
    let advance = sfnt.advance(glyph).map_or(0, i32::from);
    let (top, height) = match (sfnt.top_side_bearing(glyph), sfnt.vertical_advance(glyph)) {
        (Some(tsb), Some(height)) => (i32::from(bounds.y_max) + i32::from(tsb), i32::from(height)),
        _ => {
            let hhea = sfnt.table(TAG_HHEA);
            let ascender = hhea.and_then(|t| i16_at(t, 4)).map_or(0, i32::from);
            let descender = hhea.and_then(|t| i16_at(t, 6)).map_or(0, i32::from);
            (ascender, ascender - descender)
        }
    };
    [
        [origin, 0],
        [origin + advance, 0],
        [0, top],
        [0, top - height],
    ]
}

/// A simple glyph in the raw-point form: what its `glyf` entry states, read
/// once for every reading of it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SimpleGlyph<'a> {
    /// Each contour's last point, as written.
    pub ends: Vec<u16>,
    /// Each contour point's flags, `REPEAT` expanded: one a contour point.
    pub flags: Vec<u8>,
    /// The contour points' positions in font units, absolute, and then the
    /// four phantom points ([`phantom_points`]): `flags.len() + 4` of them.
    pub points: Vec<[i32; 2]>,
    /// The glyph's instructions, which nothing here runs.
    pub instructions: &'a [u8],
}

impl SimpleGlyph<'_> {
    /// How many contour points there are: the points before the phantom
    /// ones.
    #[must_use]
    pub fn contour_points(&self) -> usize {
        self.flags.len()
    }

    /// The contours, each its first and last point, as [`outline`] reads
    /// them: in order, stopping at the first end that does not follow the
    /// one before it or that passes the points.
    #[must_use]
    pub fn contours(&self) -> Vec<(usize, usize)> {
        let count = self.flags.len();
        let mut out = Vec::with_capacity(self.ends.len());
        let mut first = 0usize;
        for &end in &self.ends {
            let end = usize::from(end);
            if end < first || end >= count {
                break;
            }
            out.push((first, end));
            first = end + 1;
        }
        out
    }
}

/// A simple glyph in the raw-point form, or a glyph with no outline as one
/// with no contour points and its four phantom points.
///
/// `None` for a composite, a glyph past the end of `loca`, a face with no
/// `glyf`, and a glyph whose data ends before its points do — the glyphs
/// [`outline`] draws nothing of.
#[must_use]
pub fn simple_glyph<'a>(sfnt: &Sfnt<'a>, glyph: u16) -> Option<SimpleGlyph<'a>> {
    let glyf = sfnt.table(TAG_GLYF)?;
    let (start, end) = glyph_slot(sfnt, glyph)?;
    let phantom = phantom_points(sfnt, glyph);
    if end <= start {
        return Some(SimpleGlyph {
            ends: Vec::new(),
            flags: Vec::new(),
            points: phantom.to_vec(),
            instructions: &[],
        });
    }
    let data = glyf.get(start..end)?;
    let contours = i16_at(data, 0)?;
    if contours < 0 {
        return None;
    }
    let parsed = parse_simple(data, contours as usize, &mut 0)?;
    Some(parsed.into_raw(phantom))
}

/// One component record of a composite glyph (`glyf`, 'Composite glyph
/// description').
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Component {
    /// The record's flags.
    pub flags: u16,
    /// The glyph it draws.
    pub glyph: u16,
    /// Its two arguments: a signed offset where `ARGS_ARE_XY_VALUES` is set,
    /// and the two unsigned point numbers to match where it is not.
    pub arguments: [i32; 2],
    /// Its own 2×2, `[xscale, scale01, scale10, yscale]` in F2Dot14 as
    /// written — a single scale on both axes, an x and a y scale, or the
    /// whole matrix — and the identity, `[16384, 0, 0, 16384]`, where it has
    /// none.
    pub transform: [i16; 4],
}

impl Component {
    /// `ARG_1_AND_2_ARE_WORDS`.
    pub const ARG_1_AND_2_ARE_WORDS: u16 = 0x0001;
    /// `ARGS_ARE_XY_VALUES`: the arguments are an offset.
    pub const ARGS_ARE_XY_VALUES: u16 = 0x0002;
    /// `ROUND_XY_TO_GRID`: a hinted offset is rounded to the grid.
    pub const ROUND_XY_TO_GRID: u16 = 0x0004;
    /// `WE_HAVE_A_SCALE`.
    pub const WE_HAVE_A_SCALE: u16 = 0x0008;
    /// `MORE_COMPONENTS`.
    pub const MORE_COMPONENTS: u16 = 0x0020;
    /// `WE_HAVE_AN_X_AND_Y_SCALE`.
    pub const WE_HAVE_AN_X_AND_Y_SCALE: u16 = 0x0040;
    /// `WE_HAVE_A_TWO_BY_TWO`.
    pub const WE_HAVE_A_TWO_BY_TWO: u16 = 0x0080;
    /// `WE_HAVE_INSTRUCTIONS`, on the last record: the composite's own
    /// program follows the records.
    pub const WE_HAVE_INSTRUCTIONS: u16 = 0x0100;
    /// `USE_MY_METRICS`: the composite's metrics are this component's.
    pub const USE_MY_METRICS: u16 = 0x0200;

    /// The identity 2×2, `[16384, 0, 0, 16384]` in F2Dot14: a record with no
    /// scale.
    pub const IDENTITY: [i16; 4] = [16_384, 0, 0, 16_384];

    /// Whether the arguments are an offset, rather than points to match.
    #[must_use]
    pub fn offset_is_xy(&self) -> bool {
        self.flags & Component::ARGS_ARE_XY_VALUES != 0
    }
}

/// Reads the component record at `at`, and where the record after it starts.
fn read_component(data: &[u8], at: usize) -> Option<(Component, usize)> {
    let (flags, glyph) = (be16(data, at)?, be16(data, at + 2)?);
    let mut at = at + 4;
    // ARG_1_AND_2_ARE_WORDS decides the argument width; ARGS_ARE_XY_VALUES
    // decides whether they are signed offsets or unsigned point numbers.
    let xy = flags & Component::ARGS_ARE_XY_VALUES != 0;
    let arguments = if flags & Component::ARG_1_AND_2_ARE_WORDS != 0 {
        let (a1, a2) = (be16(data, at)?, be16(data, at + 2)?);
        at += 4;
        if xy {
            [i32::from(a1 as i16), i32::from(a2 as i16)]
        } else {
            [i32::from(a1), i32::from(a2)]
        }
    } else {
        let (a1, a2) = (*data.get(at)?, *data.get(at + 1)?);
        at += 2;
        if xy {
            [i32::from(a1 as i8), i32::from(a2 as i8)]
        } else {
            [i32::from(a1), i32::from(a2)]
        }
    };
    let transform = if flags & Component::WE_HAVE_A_SCALE != 0 {
        let scale = i16_at(data, at)?;
        at += 2;
        [scale, 0, 0, scale]
    } else if flags & Component::WE_HAVE_AN_X_AND_Y_SCALE != 0 {
        let (x, y) = (i16_at(data, at)?, i16_at(data, at + 2)?);
        at += 4;
        [x, 0, 0, y]
    } else if flags & Component::WE_HAVE_A_TWO_BY_TWO != 0 {
        let matrix = [
            i16_at(data, at)?,
            i16_at(data, at + 2)?,
            i16_at(data, at + 4)?,
            i16_at(data, at + 6)?,
        ];
        at += 8;
        matrix
    } else {
        [16_384, 0, 0, 16_384]
    };
    Some((
        Component {
            flags,
            glyph,
            arguments,
            transform,
        },
        at,
    ))
}

/// Every record of a composite's data, in order: until one without
/// `MORE_COMPONENTS`, or the last whole one where the data ends first. A
/// record is at least six bytes, so the count is bounded by the data.
fn all_components(data: &[u8]) -> Vec<Component> {
    let mut records = Vec::new();
    let mut at = 10usize;
    while let Some((component, next)) = read_component(data, at) {
        records.push(component);
        if component.flags & Component::MORE_COMPONENTS == 0 {
            break;
        }
        at = next;
    }
    records
}

/// A composite glyph's component records, every one of them in order — the
/// order `gvar` numbers them in. `None` for a glyph that is not a
/// composite, past the end of `loca`, or in a face with no `glyf`.
#[must_use]
pub fn components(sfnt: &Sfnt, glyph: u16) -> Option<Vec<Component>> {
    composite_glyph(sfnt, glyph).map(|composite| composite.components)
}

/// A composite glyph as its `glyf` entry states it: its component records
/// in order, and the program that follows the last of them where that record
/// sets `WE_HAVE_INSTRUCTIONS` — its length a uint16 and then its bytes, cut
/// to the bytes the entry holds — or none.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompositeGlyph<'a> {
    /// The records, as [`components`] reads them.
    pub components: Vec<Component>,
    /// The composite's own instructions, which a hinter runs over the
    /// assembled points; empty where it carries none.
    pub instructions: &'a [u8],
}

/// A composite glyph's records and its own program; `None` as for
/// [`components`].
#[must_use]
pub fn composite_glyph<'a>(sfnt: &Sfnt<'a>, glyph: u16) -> Option<CompositeGlyph<'a>> {
    let glyf = sfnt.table(TAG_GLYF)?;
    let (start, end) = glyph_range(sfnt, glyph)?;
    let data = glyf.get(start..end)?;
    if i16_at(data, 0)? >= 0 {
        return None;
    }
    let mut components = Vec::new();
    let mut at = 10usize;
    let mut instructions: &'a [u8] = &[];
    while let Some((component, next)) = read_component(data, at) {
        components.push(component);
        at = next;
        if component.flags & Component::MORE_COMPONENTS == 0 {
            if component.flags & Component::WE_HAVE_INSTRUCTIONS != 0 {
                let length = usize::from(be16(data, at).unwrap_or(0));
                let program = at + 2;
                instructions = data
                    .get(program..program.saturating_add(length).min(data.len()))
                    .unwrap_or_default();
            }
            break;
        }
    }
    Some(CompositeGlyph {
        components,
        instructions,
    })
}

/// Reads one glyph's outline in font units.
#[must_use]
pub fn outline(sfnt: &Sfnt, glyph: u16) -> Option<Outline> {
    outline_with(sfnt, glyph, &mut 0)
}

/// [`outline`], and the steps it took (`docs/design/resource-exhaustion.md`,
/// "The counters"): a glyph appended for the glyph and each component, a
/// component record read, and for a simple glyph each contour end, each flag,
/// each coordinate on either axis and each point a contour is emitted from.
#[must_use]
pub fn outline_counted(sfnt: &Sfnt, glyph: u16) -> (Option<Outline>, u64) {
    let mut steps = 0u64;
    let outline = outline_with(sfnt, glyph, &mut steps);
    (outline, steps)
}

fn outline_with(sfnt: &Sfnt, glyph: u16, steps: &mut u64) -> Option<Outline> {
    let mut out = Outline::default();
    let mut budget = MAX_COMPONENTS;
    // With nothing to vary, nothing is refused: the result is always `Ok`.
    let _ = append(sfnt, glyph, IDENTITY, 0, &mut budget, &mut out, steps, None);
    Some(out)
}

/// One glyph's outline in font units at a location of its face's design
/// space: each glyph it draws moved by `gvar`'s deltas there before its
/// implied midpoints are resolved, and each component offset by its own —
/// [`outline`] where `gvar` is `None` or nothing moves.
///
/// # Errors
///
/// A `gvar` refusal for a glyph the outline draws ([`Gvar::glyph`],
/// [`Gvar::simple_deltas`], [`Gvar::composite_deltas`]), and
/// [`VariationError::GlyphDeltaWork`] where the glyphs it draws pass
/// [`crate::variation::gvar::MAX_GLYPH_DELTA_WORK`] between them. A caller
/// draws such a glyph at its default.
pub fn outline_at(
    sfnt: &Sfnt,
    glyph: u16,
    gvar: Option<&Gvar>,
    location: &Location,
) -> Result<Outline, VariationError> {
    outline_at_counted(sfnt, glyph, gvar, location).0
}

/// [`outline_at`], and the steps it took: [`outline_counted`]'s, a component
/// record read for each record a composite's numbering reads, and the point
/// deltas its glyphs' tables decode, a table times its points each.
#[must_use]
pub fn outline_at_counted(
    sfnt: &Sfnt,
    glyph: u16,
    gvar: Option<&Gvar>,
    location: &Location,
) -> (Result<Outline, VariationError>, u64) {
    let mut steps = 0u64;
    let mut out = Outline::default();
    let mut budget = MAX_COMPONENTS;
    let result = match gvar {
        None => append(
            sfnt,
            glyph,
            IDENTITY,
            0,
            &mut budget,
            &mut out,
            &mut steps,
            None,
        ),
        Some(gvar) => {
            let mut vary = Varying {
                gvar,
                location,
                work: 0,
            };
            let result = append(
                sfnt,
                glyph,
                IDENTITY,
                0,
                &mut budget,
                &mut out,
                &mut steps,
                Some(&mut vary),
            );
            steps = steps.saturating_add(vary.work);
            result
        }
    };
    (result.map(|()| out), steps)
}

/// A glyph's four phantom points at a location, in 16.16 font units: the
/// default ones ([`phantom_points`]) moved by the deltas `gvar` gives them,
/// which give the advances and side bearings where a face has no `HVAR` —
/// the advance width the first two's difference in x, the advance height the
/// last two's in y. A composite with a `USE_MY_METRICS` component takes that
/// component's phantom points, the last such component's where several
/// carry the flag, and its own deltas for them are not used (`gvar`, 'Point
/// numbers and processing for composite glyphs'); they are the component's
/// own, not moved by its offset.
///
/// # Errors
///
/// As [`outline_at`].
pub fn phantom_points_at(
    sfnt: &Sfnt,
    glyph: u16,
    gvar: Option<&Gvar>,
    location: &Location,
) -> Result<[[Fixed; 2]; 4], VariationError> {
    phantoms_at(sfnt, glyph, gvar, location, 0)
}

fn phantoms_at(
    sfnt: &Sfnt,
    glyph: u16,
    gvar: Option<&Gvar>,
    location: &Location,
    depth: u32,
) -> Result<[[Fixed; 2]; 4], VariationError> {
    let mut phantom =
        phantom_points(sfnt, glyph).map(|[x, y]| [Fixed::from_int(x), Fixed::from_int(y)]);
    let data = sfnt
        .table(TAG_GLYF)
        .zip(glyph_range(sfnt, glyph))
        .and_then(|(glyf, (start, end))| glyf.get(start..end));
    let (points, composite) = match data.and_then(|data| Some((data, i16_at(data, 0)?))) {
        Some((data, contours)) if contours < 0 => {
            let records = all_components(data);
            if depth < MAX_COMPONENT_DEPTH {
                if let Some(metrics) = records
                    .iter()
                    .rev()
                    .find(|c| c.flags & Component::USE_MY_METRICS != 0)
                {
                    return phantoms_at(sfnt, metrics.glyph, gvar, location, depth + 1);
                }
            }
            (records.len(), true)
        }
        // The points a simple glyph declares: one past its last contour's
        // end, which is where `gvar` numbers its phantom points from.
        Some((data, contours)) => {
            let last = usize::try_from(contours)
                .ok()
                .and_then(|n| n.checked_sub(1))
                .and_then(|n| be16(data, 10 + n * 2));
            (last.map_or(0, |end| usize::from(end) + 1), false)
        }
        // No outline: the four phantom points are all there is.
        None => (0, false),
    };
    let Some(gvar) = gvar else {
        return Ok(phantom);
    };
    let deltas = gvar.phantom_deltas(glyph, points, composite, location)?;
    for (point, delta) in phantom.iter_mut().zip(deltas) {
        point[0] = point[0].saturating_add(delta[0]);
        point[1] = point[1].saturating_add(delta[1]);
    }
    Ok(phantom)
}

/// What a glyph is varied by while its outline is drawn at a location, and
/// the delta work its glyphs have charged so far.
struct Varying<'v, 'a> {
    gvar: &'v Gvar<'a>,
    location: &'v Location,
    work: u64,
}

/// One step more, saturating.
fn step(steps: &mut u64, n: usize) {
    *steps = steps.saturating_add(u64::try_from(n).unwrap_or(u64::MAX));
}

/// Appends a glyph's contours, transformed by the given 2×3 matrix, written
/// `[dx, dy, a, b, c, d]` — at a location where `vary` says one, which is
/// the only way this refuses.
#[allow(clippy::too_many_arguments)]
fn append(
    sfnt: &Sfnt,
    glyph: u16,
    matrix: [f64; 6],
    depth: u32,
    budget: &mut u32,
    out: &mut Outline,
    steps: &mut u64,
    vary: Option<&mut Varying>,
) -> Result<(), VariationError> {
    let [dx, dy, a, b, c, d] = matrix;
    if depth > MAX_COMPONENT_DEPTH || *budget == 0 {
        return Ok(());
    }
    *budget -= 1;
    step(steps, 1);
    let Some(glyf) = sfnt.table(TAG_GLYF) else {
        return Ok(());
    };
    let Some((start, end)) = glyph_range(sfnt, glyph) else {
        return Ok(()); // No outline: a space, or a glyph past the end.
    };
    let Some(data) = glyf.get(start..end) else {
        return Ok(());
    };

    let Some(contours) = i16_at(data, 0) else {
        return Ok(());
    };

    let transform = |x: f64, y: f64| (a * x + c * y + dx, b * x + d * y + dy);

    if contours >= 0 {
        simple(
            sfnt,
            glyph,
            data,
            contours as usize,
            &transform,
            out,
            steps,
            vary,
        )
    } else {
        composite(sfnt, glyph, data, matrix, depth, budget, out, steps, vary)
    }
}

/// What a simple glyph's data states.
struct Parsed<'a> {
    ends: Vec<usize>,
    flags: Vec<u8>,
    xs: Vec<i32>,
    ys: Vec<i32>,
    instructions: &'a [u8],
}

impl<'a> Parsed<'a> {
    /// The raw-point form, the phantom points after the contour points.
    fn into_raw(self, phantom: [[i32; 2]; 4]) -> SimpleGlyph<'a> {
        let mut points: Vec<[i32; 2]> = self
            .xs
            .iter()
            .zip(&self.ys)
            .map(|(x, y)| [*x, *y])
            .collect();
        points.extend(phantom);
        SimpleGlyph {
            // Each end was read as sixteen bits.
            ends: self
                .ends
                .iter()
                .map(|e| u16::try_from(*e).unwrap_or(u16::MAX))
                .collect(),
            flags: self.flags,
            points,
            instructions: self.instructions,
        }
    }
}

/// Reads a simple glyph's contour ends, flags, coordinates and instructions,
/// counting [`outline_counted`]'s steps for each; `None` where the data ends
/// before its points do. A glyph of no contours has no points.
fn parse_simple<'a>(data: &'a [u8], contours: usize, steps: &mut u64) -> Option<Parsed<'a>> {
    // The header is 10 bytes, then one end-point index per contour.
    let mut ends = Vec::with_capacity(contours.min(256));
    for i in 0..contours {
        step(steps, 1);
        ends.push(usize::from(be16(data, 10 + i * 2)?));
    }
    let Some(&last) = ends.last() else {
        return Some(Parsed {
            ends,
            flags: Vec::new(),
            xs: Vec::new(),
            ys: Vec::new(),
            instructions: &[],
        });
    };
    let count = last + 1;
    if count > 1 << 16 {
        return None;
    }

    // The instructions: hinting bytecode, which is not interpreted.
    let instruction_len = usize::from(be16(data, 10 + contours * 2)?);
    let program = 10 + contours * 2 + 2;
    let mut at = program + instruction_len;

    // Flags, run-length encoded by the REPEAT bit: a step a flag, however
    // it was spelled.
    let mut flags = Vec::with_capacity(count);
    while flags.len() < count {
        let flag = *data.get(at)?;
        at += 1;
        flags.push(flag);
        if flag & 0x08 != 0 {
            let repeats = *data.get(at)?;
            at += 1;
            for _ in 0..repeats {
                if flags.len() >= count {
                    break;
                }
                flags.push(flag);
            }
        }
    }

    step(steps, flags.len());

    // Coordinates are deltas, one or two bytes by the flag bits.
    let mut xs = Vec::with_capacity(count);
    let mut x = 0i32;
    for &flag in &flags {
        if flag & 0x02 != 0 {
            let delta = *data.get(at)?;
            at += 1;
            x += if flag & 0x10 != 0 {
                i32::from(delta)
            } else {
                -i32::from(delta)
            };
        } else if flag & 0x10 == 0 {
            let delta = i16_at(data, at)?;
            at += 2;
            x += i32::from(delta);
        }
        xs.push(x);
    }

    step(steps, xs.len());

    let mut ys = Vec::with_capacity(count);
    let mut y = 0i32;
    for &flag in &flags {
        if flag & 0x04 != 0 {
            let delta = *data.get(at)?;
            at += 1;
            y += if flag & 0x20 != 0 {
                i32::from(delta)
            } else {
                -i32::from(delta)
            };
        } else if flag & 0x20 == 0 {
            let delta = i16_at(data, at)?;
            at += 2;
            y += i32::from(delta);
        }
        ys.push(y);
    }

    step(steps, ys.len());

    // The flags were read past the program, so it is inside the data.
    let instructions = data.get(program..program + instruction_len).unwrap_or(&[]);
    Some(Parsed {
        ends,
        flags,
        xs,
        ys,
        instructions,
    })
}

/// A simple glyph: contours of on- and off-curve points, each moved by its
/// delta where the glyph is drawn at a location.
#[allow(clippy::too_many_arguments)]
fn simple(
    sfnt: &Sfnt,
    glyph: u16,
    data: &[u8],
    contours: usize,
    transform: &dyn Fn(f64, f64) -> (f64, f64),
    out: &mut Outline,
    steps: &mut u64,
    vary: Option<&mut Varying>,
) -> Result<(), VariationError> {
    let Some(parsed) = parse_simple(data, contours, steps) else {
        return Ok(());
    };
    let Some(vary) = vary else {
        let (xs, ys) = (&parsed.xs, &parsed.ys);
        let position = |i: usize| Some((f64::from(*xs.get(i)?), f64::from(*ys.get(i)?)));
        emit_contours(
            &parsed.ends,
            &parsed.flags,
            &position,
            transform,
            out,
            steps,
        );
        return Ok(());
    };
    let ends = parsed.ends.clone();
    let raw = parsed.into_raw(phantom_points(sfnt, glyph));
    let deltas = vary
        .gvar
        .simple_deltas_charged(glyph, &raw, vary.location, &mut vary.work)?;
    let position = |i: usize| {
        let [x, y] = *raw.points.get(i)?;
        let [dx, dy] = *deltas.get(i)?;
        Some((
            Fixed::from_int(x).saturating_add(dx).to_f64(),
            Fixed::from_int(y).saturating_add(dy).to_f64(),
        ))
    };
    emit_contours(&ends, &raw.flags, &position, transform, out, steps);
    Ok(())
}

/// The outline of points already placed — the hinter's, in pixels
/// (`docs/design/hinting.md`, "The pipeline") — read by the contour reading
/// [`outline`] reads a glyph's own points with, its implied midpoints
/// included: each contour `ends` closes, over the first `on_curve.len()`
/// points, each on the curve or off it as `on_curve` says and at its place in
/// `positions`. A contour past the points, or whose end does not follow the
/// one before, ends the reading, as it ends [`outline`]'s.
pub(crate) fn outline_of(ends: &[usize], on_curve: &[bool], positions: &[(f64, f64)]) -> Outline {
    let flags: Vec<u8> = on_curve.iter().map(|&on| u8::from(on)).collect();
    let position = |i: usize| positions.get(i).copied();
    let mut out = Outline::default();
    emit_contours(ends, &flags, &position, &|x, y| (x, y), &mut out, &mut 0);
    out
}

/// Emits each contour in order, stopping at the first end that does not
/// follow the one before it or that passes the points.
fn emit_contours<P: Fn(usize) -> Option<(f64, f64)>>(
    ends: &[usize],
    flags: &[u8],
    position: &P,
    transform: &dyn Fn(f64, f64) -> (f64, f64),
    out: &mut Outline,
    steps: &mut u64,
) {
    let count = flags.len();
    let mut first = 0usize;
    for &end in ends {
        if end < first || end >= count {
            break;
        }
        emit_contour(flags, position, first, end, transform, out);
        step(steps, end + 1 - first);
        first = end + 1;
    }
}

/// Emits one closed contour, resolving implied on-curve midpoints.
fn emit_contour<P: Fn(usize) -> Option<(f64, f64)>>(
    flags: &[u8],
    position: &P,
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
        let (x, y) = position(index)?;
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

/// A composite glyph: other glyphs, each with a placement, under the matrix
/// `[dx, dy, a, b, c, d]` the glyph is drawn with. At a location, each
/// component placed by an offset has its offset moved by its delta — the
/// offset as written plus the delta, placed as the offset is — and one
/// placed by matching points ignores its delta (`gvar`, 'Point numbers and
/// processing for composite glyphs').
#[allow(clippy::too_many_arguments)]
fn composite(
    sfnt: &Sfnt,
    glyph: u16,
    data: &[u8],
    matrix: [f64; 6],
    depth: u32,
    budget: &mut u32,
    out: &mut Outline,
    steps: &mut u64,
    mut vary: Option<&mut Varying>,
) -> Result<(), VariationError> {
    let [dx, dy, a, b, c, d] = matrix;
    // At a location, every record is read first: a component's delta is its
    // place among all of them, and the phantom points' follow the last.
    let deltas = match vary.as_deref_mut() {
        Some(vary) => {
            let count = all_components(data).len();
            step(steps, count);
            Some(
                vary.gvar
                    .composite_deltas_charged(glyph, count, vary.location, &mut vary.work)?,
            )
        }
        None => None,
    };
    let mut at = 10usize;
    let mut guard = 0u32;
    let mut index = 0usize;

    loop {
        guard += 1;
        if guard > MAX_RECORDS {
            return Ok(());
        }
        step(steps, 1);
        let Some((component, next)) = read_component(data, at) else {
            return Ok(());
        };
        at = next;

        // A component's own 2×2, from F2Dot14.
        let [ca, cb, cc, cd] = component.transform.map(|v| f64::from(v) / 16384.0);

        // Point-matching placement is rare and needs both glyphs' points; an
        // offset of zero is a better answer than refusing the glyph.
        let (ox, oy) = if component.offset_is_xy() {
            let [x, y] = component.arguments;
            match deltas.as_ref().and_then(|deltas| deltas.get(index)) {
                Some([delta_x, delta_y]) => (
                    Fixed::from_int(x).saturating_add(*delta_x).to_f64(),
                    Fixed::from_int(y).saturating_add(*delta_y).to_f64(),
                ),
                None => (f64::from(x), f64::from(y)),
            }
        } else {
            (0.0, 0.0)
        };

        // Compose the component's transform with the one already in effect.
        let na = ca * a + cb * c;
        let nb = ca * b + cb * d;
        let nc = cc * a + cd * c;
        let nd = cc * b + cd * d;
        let ndx = ox * a + oy * c + dx;
        let ndy = ox * b + oy * d + dy;

        append(
            sfnt,
            component.glyph,
            [ndx, ndy, na, nb, nc, nd],
            depth + 1,
            budget,
            out,
            steps,
            vary.as_deref_mut(),
        )?;
        if *budget == 0 {
            return Ok(());
        }

        if component.flags & Component::MORE_COMPONENTS == 0 {
            return Ok(()); // No MORE_COMPONENTS.
        }
        index += 1;
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

        let tables: [(&[u8; 4], &[u8]); 3] = [(b"head", &head), (b"loca", &loca), (b"glyf", &glyf)];

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

    /// The header's box is read as written, not recomputed from the points:
    /// the triangle's points reach 100 on both axes and its header is made to
    /// say otherwise, so a reader that bounded the points would answer 100.
    #[test]
    fn the_bounds_are_the_headers_as_written() {
        let mut glyph = triangle();
        glyph[2..4].copy_from_slice(&(-12i16).to_be_bytes()); // xMin
        glyph[4..6].copy_from_slice(&(-34i16).to_be_bytes()); // yMin
        glyph[6..8].copy_from_slice(&456i16.to_be_bytes()); // xMax
        glyph[8..10].copy_from_slice(&789i16.to_be_bytes()); // yMax
        let data = font_with_glyph(&glyph);
        let sfnt = Sfnt::parse(&data).expect("a font");
        assert_eq!(
            bounds(&sfnt, 0),
            Some(Bounds {
                x_min: -12,
                y_min: -34,
                x_max: 456,
                y_max: 789,
            })
        );
    }

    /// A glyph whose `loca` range is empty has the origin point for its box,
    /// and a glyph past `loca` has no box at all.
    #[test]
    fn an_empty_glyph_is_bounded_at_its_origin_and_a_missing_one_not_at_all() {
        let triangle = triangle();
        let data = font_with_glyphs(&[triangle.as_slice(), &[][..]]);
        let sfnt = Sfnt::parse(&data).expect("a font");
        assert_eq!(bounds(&sfnt, 1), Some(Bounds::default()), "an empty glyph");
        assert_eq!(bounds(&sfnt, 2), None, "past the end of loca");
        assert_eq!(bounds(&sfnt, u16::MAX), None);
    }

    /// A glyph's data shorter than the ten bytes of its header, and a face
    /// with no `glyf` at all, answer `None` rather than a box made up.
    #[test]
    fn a_truncated_header_or_a_missing_table_has_no_bounds() {
        let data = font_with_glyph(&[0x00, 0x01, 0x00, 0x00]);
        let sfnt = Sfnt::parse(&data).expect("a font");
        assert_eq!(bounds(&sfnt, 0), None, "four bytes of a ten-byte header");

        let mut head = vec![0u8; 54];
        head[18..20].copy_from_slice(&1000u16.to_be_bytes());
        let mut face = Vec::new();
        face.extend_from_slice(&0x0001_0000u32.to_be_bytes());
        face.extend_from_slice(&1u16.to_be_bytes());
        face.extend_from_slice(&[0; 6]);
        face.extend_from_slice(b"head");
        face.extend_from_slice(&0u32.to_be_bytes());
        face.extend_from_slice(&28u32.to_be_bytes());
        face.extend_from_slice(&(head.len() as u32).to_be_bytes());
        face.extend_from_slice(&head);
        let sfnt = Sfnt::parse(&face).expect("a font with only head");
        assert_eq!(bounds(&sfnt, 0), None, "no glyf, no loca");
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

    /// The twelve committed Liberation faces, by name.
    const LIBERATION: [(&str, &[u8]); 12] = [
        (
            "LiberationMono-Bold",
            include_bytes!("../data/liberation/LiberationMono-Bold.ttf"),
        ),
        (
            "LiberationMono-BoldItalic",
            include_bytes!("../data/liberation/LiberationMono-BoldItalic.ttf"),
        ),
        (
            "LiberationMono-Italic",
            include_bytes!("../data/liberation/LiberationMono-Italic.ttf"),
        ),
        (
            "LiberationMono-Regular",
            include_bytes!("../data/liberation/LiberationMono-Regular.ttf"),
        ),
        (
            "LiberationSans-Bold",
            include_bytes!("../data/liberation/LiberationSans-Bold.ttf"),
        ),
        (
            "LiberationSans-BoldItalic",
            include_bytes!("../data/liberation/LiberationSans-BoldItalic.ttf"),
        ),
        (
            "LiberationSans-Italic",
            include_bytes!("../data/liberation/LiberationSans-Italic.ttf"),
        ),
        (
            "LiberationSans-Regular",
            include_bytes!("../data/liberation/LiberationSans-Regular.ttf"),
        ),
        (
            "LiberationSerif-Bold",
            include_bytes!("../data/liberation/LiberationSerif-Bold.ttf"),
        ),
        (
            "LiberationSerif-BoldItalic",
            include_bytes!("../data/liberation/LiberationSerif-BoldItalic.ttf"),
        ),
        (
            "LiberationSerif-Italic",
            include_bytes!("../data/liberation/LiberationSerif-Italic.ttf"),
        ),
        (
            "LiberationSerif-Regular",
            include_bytes!("../data/liberation/LiberationSerif-Regular.ttf"),
        ),
    ];

    /// **The unhinted outlines are unchanged** (FT-04b.9): every simple glyph
    /// of the committed faces, read once into the raw-point form the hinter
    /// reads ([`simple_glyph`]) and those points handed to the contour
    /// reading the hinter's answer goes through ([`outline_of`]), draws
    /// exactly the segments [`outline`] draws. One parse serves both
    /// readings, so the hinter's points are the points the unhinted outline
    /// is drawn from.
    #[test]
    fn the_raw_point_form_reads_every_committed_glyph_as_outline_does() {
        for (name, bytes) in LIBERATION {
            let sfnt = Sfnt::parse(bytes).expect("a committed face");
            let count = sfnt.glyph_count().expect("a glyph count");
            let (mut simple, mut composite) = (0usize, 0usize);
            for glyph in 0..count {
                let drawn = outline(&sfnt, glyph).expect("an outline");
                let Some(raw) = simple_glyph(&sfnt, glyph) else {
                    assert!(
                        components(&sfnt, glyph).is_some(),
                        "{name} glyph {glyph} is neither simple nor a composite"
                    );
                    composite += 1;
                    continue;
                };
                simple += 1;
                let points = raw.contour_points();
                assert_eq!(
                    raw.points.len(),
                    points + 4,
                    "{name} glyph {glyph}'s phantoms"
                );
                let ends: Vec<usize> = raw.contours().iter().map(|&(_, end)| end).collect();
                let on_curve: Vec<bool> = raw.flags.iter().map(|flag| flag & 1 != 0).collect();
                let positions: Vec<(f64, f64)> = raw.points[..points]
                    .iter()
                    .map(|&[x, y]| (f64::from(x), f64::from(y)))
                    .collect();
                assert_eq!(
                    outline_of(&ends, &on_curve, &positions).segments,
                    drawn.segments,
                    "{name} glyph {glyph}"
                );
            }
            assert!(simple > 0, "{name}: no simple glyph read");
            println!("RAN {name}: {simple} simple glyphs read alike, {composite} composites");
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
