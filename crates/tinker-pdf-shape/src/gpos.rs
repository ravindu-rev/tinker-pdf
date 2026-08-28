//! `GPOS`: the nine kinds of positioning, all of them integer.
//!
//! Types 7, 8 and 9 are not here. Contextual and chaining-contextual
//! positioning are the same rules as `GSUB`'s types 5 and 6 down to the byte,
//! so they live in `apply.rs`; the extension subtable is resolved before
//! anything in this file is reached. What is left is the six that move
//! glyphs.
//!
//! # Two kinds of movement, and why they are not the same
//!
//! A `ValueRecord` **adjusts**: it adds to a glyph's placement and advance,
//! and the result is final the moment it is written. An **anchor** attaches:
//! it says two points must coincide, and the number that satisfies that
//! depends on where the other glyph ends up — which no lookup can know while
//! lookups are still running.
//!
//! So the four attaching lookups (cursive, and the three mark ones) do not
//! write a final offset. They write the relationship into
//! [`crate::buffer::Buffer`] and it is resolved in one pass at the end, by
//! `propagate_attachments`. A shaper that resolved attachment where it found
//! it would place a mark correctly and then have the base moved out from
//! under it by the next lookup, which is the classic Arabic vowel-drift bug.
//!
//! # Device tables are parsed and not applied
//!
//! Every value record and every anchor may carry `Device` tables: per-pixel
//! corrections a designer hinted by hand. They are read, bounds-checked and
//! exposed through [`crate::common::Device`], and nothing here consults one,
//! because a device correction is a function of *pixels per em* and this
//! crate's whole output is in font design units — there is no ppem in the API
//! to evaluate one at. A consumer that rasterizes at a fixed size can apply
//! them; a consumer that lays out in design units must not, and would get a
//! different line at every zoom level if this crate did it for them.

use crate::apply::{apply_chain_context, apply_context, Applied, Runner, Skipper};
use crate::buffer::Buffer;
use crate::common::{ClassDef, Coverage, RIGHT_TO_LEFT};
use crate::gdef::GlyphClass;
use crate::read::Bytes;

/// Applies one `GPOS` subtable at one position.
pub(crate) fn apply<'a>(
    runner: &mut Runner<'a, '_>,
    buffer: &mut Buffer,
    kind: u16,
    data: Bytes<'a>,
    skipper: &Skipper<'a>,
    at: usize,
    depth: u32,
) -> Applied {
    match kind {
        1 => single(buffer, data, at),
        2 => pair(runner, buffer, data, skipper, at),
        3 => cursive(runner, buffer, data, skipper, at),
        4 => mark_to_base(runner, buffer, data, at),
        5 => mark_to_ligature(runner, buffer, data, at),
        6 => mark_to_mark(runner, buffer, data, skipper, at),
        7 => apply_context(runner, buffer, data, skipper, at, depth),
        8 => apply_chain_context(runner, buffer, data, skipper, at, depth),
        _ => Applied::Unsupported,
    }
}

/// One `ValueRecord`, in design units.
///
/// The four device-table offsets a record may also carry are deliberately not
/// held: see the module documentation. They are skipped over by
/// [`value_size`], which counts every set bit of the format rather than the
/// four it knows about — so a format bit this crate does not read still
/// advances the cursor by the right number of bytes, and the *next* record in
/// the array is still found.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Value {
    x_placement: i32,
    y_placement: i32,
    x_advance: i32,
    y_advance: i32,
}

const X_PLACEMENT: u16 = 0x0001;
const Y_PLACEMENT: u16 = 0x0002;
const X_ADVANCE: u16 = 0x0004;
const Y_ADVANCE: u16 = 0x0008;

/// How many bytes a value record in this format occupies.
///
/// Two per set bit, whatever the bit means. That is the specification's own
/// rule and it is what makes an unknown format bit survivable rather than
/// fatal: every field in a value record is two bytes wide, so the size is a
/// population count and never a table of known formats.
fn value_size(format: u16) -> usize {
    usize::try_from(format.count_ones())
        .unwrap_or(0)
        .saturating_mul(2)
}

/// Reads a value record, which is a struct whose *fields* are optional.
fn value(data: Bytes<'_>, at: usize, format: u16) -> Value {
    let mut out = Value::default();
    let mut cursor = at;
    let take = |out: &mut i32, cursor: &mut usize| {
        if let Some(found) = data.i16(*cursor) {
            *out = i32::from(found);
        }
        *cursor = cursor.saturating_add(2);
    };
    if format & X_PLACEMENT != 0 {
        take(&mut out.x_placement, &mut cursor);
    }
    if format & Y_PLACEMENT != 0 {
        take(&mut out.y_placement, &mut cursor);
    }
    if format & X_ADVANCE != 0 {
        take(&mut out.x_advance, &mut cursor);
    }
    if format & Y_ADVANCE != 0 {
        take(&mut out.y_advance, &mut cursor);
    }
    out
}

/// Adds a value record to a glyph.
fn adjust(buffer: &mut Buffer, at: usize, value: Value) -> bool {
    let Some(glyph) = buffer.glyph_mut(at) else {
        return false;
    };
    glyph.x_offset = glyph.x_offset.saturating_add(value.x_placement);
    glyph.y_offset = glyph.y_offset.saturating_add(value.y_placement);
    glyph.x_advance = glyph.x_advance.saturating_add(value.x_advance);
    glyph.y_advance = glyph.y_advance.saturating_add(value.y_advance);
    true
}

/// An `Anchor` table's coordinates, in design units.
///
/// Format 2's `anchorPoint` and format 3's device tables are read past rather
/// than applied — the first needs the glyph's outline, which this crate does
/// not have, and the second needs a pixel size, which its API does not carry.
/// Both formats still yield their design-unit coordinates, which is the
/// answer a design-unit shaper wants and the one every unhinted rasterizer
/// uses anyway.
fn anchor(data: Bytes<'_>) -> Option<(i32, i32)> {
    match data.u16(0)? {
        1..=3 => Some((i32::from(data.i16(2)?), i32::from(data.i16(4)?))),
        _ => None,
    }
}

/// Type 1: adjust one glyph, wherever it appears.
fn single(buffer: &mut Buffer, data: Bytes<'_>, at: usize) -> Applied {
    let Some(glyph) = buffer.glyph_id(at) else {
        return Applied::No;
    };
    let Some(coverage) = data.offset16(2).map(Coverage::new) else {
        return Applied::No;
    };
    let Some(index) = coverage.index_of(glyph) else {
        return Applied::No;
    };
    let Some(format) = data.u16(4) else {
        return Applied::No;
    };
    let record = match data.u16(0) {
        // One value for every covered glyph.
        Some(1) => value(data, 6, format),
        // One value each, in coverage order.
        Some(2) => {
            let count = usize::from(data.u16(6).unwrap_or(0));
            if usize::from(index) >= count {
                return Applied::No;
            }
            let Some(offset) = usize::from(index).checked_mul(value_size(format)) else {
                return Applied::No;
            };
            let Some(offset) = offset.checked_add(8) else {
                return Applied::No;
            };
            value(data, offset, format)
        }
        _ => return Applied::No,
    };
    adjust(buffer, at, record);
    Applied::Yes(at.saturating_add(1))
}

/// Type 2: adjust a glyph because of the one after it — kerning, and more.
///
/// The cursor ends at the *second* glyph when only the first was adjusted, so
/// that glyph can start the next pair; and past it when both were, because a
/// glyph already positioned as the second of a pair must not also be
/// positioned as the first of the next. That distinction is what makes `AVA`
/// kern twice rather than three times.
fn pair<'a>(
    runner: &mut Runner<'a, '_>,
    buffer: &mut Buffer,
    data: Bytes<'a>,
    skipper: &Skipper<'a>,
    at: usize,
) -> Applied {
    let Some(first) = buffer.glyph_id(at) else {
        return Applied::No;
    };
    let Some(coverage) = data.offset16(2).map(Coverage::new) else {
        return Applied::No;
    };
    let Some(index) = coverage.index_of(first) else {
        return Applied::No;
    };
    let Some(next) = runner.step_forward(buffer, skipper, at) else {
        return Applied::No;
    };
    let Some(second) = buffer.glyph_id(next) else {
        return Applied::No;
    };
    let Some(format1) = data.u16(4) else {
        return Applied::No;
    };
    let Some(format2) = data.u16(6) else {
        return Applied::No;
    };
    let (size1, size2) = (value_size(format1), value_size(format2));

    let found = match data.u16(0) {
        Some(1) => pair_format1(data, index, second, format1, format2, size1, size2),
        Some(2) => pair_format2(data, first, second, format1, format2, size1, size2),
        _ => None,
    };
    let Some((one, two)) = found else {
        return Applied::No;
    };
    if format1 != 0 {
        adjust(buffer, at, one);
    }
    if format2 != 0 {
        adjust(buffer, next, two);
        return Applied::Yes(next.saturating_add(1));
    }
    Applied::Yes(next)
}

/// Format 1: a list of second glyphs per first glyph.
fn pair_format1(
    data: Bytes<'_>,
    index: u16,
    second: u16,
    format1: u16,
    format2: u16,
    size1: usize,
    size2: usize,
) -> Option<(Value, Value)> {
    let count = usize::from(data.u16(8)?);
    if usize::from(index) >= count {
        return None;
    }
    let set = data.offset16_at(10, usize::from(index))?;
    let pairs = usize::from(set.u16(0)?);
    let stride = 2usize.checked_add(size1)?.checked_add(size2)?;
    // A linear scan rather than a binary search, and the reason is the shape
    // of the data rather than laziness: the specification requires the array
    // be ordered by `secondGlyph`, but a pair set is a handful of entries for
    // an ordinary glyph and the search would cost more in bounds checks than
    // it saved in comparisons. The wide case is format 2, which is a table
    // lookup and needs no search at all.
    for n in 0..pairs {
        let at = 2usize.checked_add(n.checked_mul(stride)?)?;
        if set.u16(at)? != second {
            continue;
        }
        let one = value(set, at.checked_add(2)?, format1);
        let two = value(set, at.checked_add(2)?.checked_add(size1)?, format2);
        return Some((one, two));
    }
    None
}

/// Format 2: a class matrix, which is how a face kerns a thousand pairs in a
/// few hundred bytes.
fn pair_format2(
    data: Bytes<'_>,
    first: u16,
    second: u16,
    format1: u16,
    format2: u16,
    size1: usize,
    size2: usize,
) -> Option<(Value, Value)> {
    let class1 = data
        .offset16(8)
        .map_or(0, |d| ClassDef::new(d).class_of(first));
    let class2 = data
        .offset16(10)
        .map_or(0, |d| ClassDef::new(d).class_of(second));
    let count1 = usize::from(data.u16(12)?);
    let count2 = usize::from(data.u16(14)?);
    if usize::from(class1) >= count1 || usize::from(class2) >= count2 {
        return None;
    }
    let stride = size1.checked_add(size2)?;
    let row = usize::from(class1).checked_mul(count2)?;
    let cell = row.checked_add(usize::from(class2))?.checked_mul(stride)?;
    let at = 16usize.checked_add(cell)?;
    let one = value(data, at, format1);
    let two = value(data, at.checked_add(size1)?, format2);
    Some((one, two))
}

/// Type 3: join one glyph's exit to the next one's entry.
///
/// This is what makes a Nastaliq or a formal Arabic face flow: each letter
/// carries an entry point and an exit point, and the pair is positioned so
/// that one lands on the other.
///
/// **Which glyph moves is the lookup's `RIGHT_TO_LEFT` flag**, and it is the
/// only place in the whole crate that bit is read. Clear, and the second
/// glyph is moved onto the first; set, and the first is moved onto the
/// second, so that the *last* glyph of a connected sequence is the one that
/// keeps its place on the baseline. The glyph that moves is attached to the
/// one that does not, so a chain of joined letters rises and falls as one.
///
/// # One divergence from the current specification, chosen deliberately
///
/// The glyph that moves is moved by **placement**, in both directions, and no
/// advance is touched. That is what Adobe's annotated specification says, in
/// the words attached to the fixture this is tested against: after `gpos3`
/// lookup 0 runs, *"occurrences of glyph 19 will have been moved, to have its
/// origin at (99, 99) relative to the origin of the new position of the
/// occurrence of glyph 18"*, and the deltas it then states leave every
/// following glyph exactly where it was.
///
/// The OpenType 1.9 text describes the line-direction half differently —
/// *"the layout engine adjusts the advance of the first glyph (in logical
/// order)"* — which places the joined pair identically and makes the run
/// **narrower**, because the shortened advance carries through to everything
/// after it. The two readings cannot both be satisfied: they differ by
/// exactly the overlap, on every glyph past the join.
///
/// This crate takes the fixture's reading, because ruling 13 makes the
/// fixture the thing that adjudicates and because a leaf that hands back the
/// advances its caller supplied is the more conservative of the two — a
/// consumer can shorten a run it was told the joins of, and cannot lengthen
/// one that was shortened for it.
///
/// ## Milestone 2 looked, and text-rendering-tests does not reach it
///
/// *Checked rather than assumed, and the answer is "not yet".* Milestone 2
/// vendored the corpus's CMAP, GSUB and GPOS sections and **none of them
/// contains a `GPOS` type 3 lookup at all**. The section called `GPOS-3` is a
/// trap for exactly this question: it is *Mark-to-Base Attachment for Ethiopic
/// Diacritics*, a type 4 lookup, and the numbering of the sections has nothing
/// to do with the numbering of the lookup types.
///
/// Scanning every face in the corpus rather than only the ones this milestone
/// runs, cursive attachment appears in three, and all three belong to later
/// milestones: `TestShapeAran.ttf` (section `SHARAN-1`, Arabic, milestone 4)
/// and `NotoSansKannada-Regular.ttf` with `TestShapeKndaV3.ttf` (the `SHKNDA`
/// sections, milestone 5). So the two readings are still both live, the aots
/// fixture is still the only thing adjudicating, and **`SHARAN-1` is the case
/// that will settle it** — named here so the next person does not have to
/// rediscover which fixture to look at.
fn cursive<'a>(
    runner: &mut Runner<'a, '_>,
    buffer: &mut Buffer,
    data: Bytes<'a>,
    skipper: &Skipper<'a>,
    at: usize,
) -> Applied {
    if data.u16(0) != Some(1) {
        return Applied::No;
    }
    let Some(glyph) = buffer.glyph_id(at) else {
        return Applied::No;
    };
    let Some(coverage) = data.offset16(2).map(Coverage::new) else {
        return Applied::No;
    };
    let Some(index) = coverage.index_of(glyph) else {
        return Applied::No;
    };
    let Some(entry) = entry_exit(data, index, 0) else {
        return Applied::No;
    };
    let Some(previous) = runner.step_backward(buffer, skipper, at) else {
        return Applied::No;
    };
    let Some(previous_glyph) = buffer.glyph_id(previous) else {
        return Applied::No;
    };
    let Some(previous_index) = coverage.index_of(previous_glyph) else {
        return Applied::No;
    };
    let Some(exit) = entry_exit(data, previous_index, 2) else {
        return Applied::No;
    };
    // The parent keeps its place; the child is moved so its anchor lands on
    // the parent's. `RIGHT_TO_LEFT` swaps which is which.
    let right_to_left = skipper.flags() & RIGHT_TO_LEFT != 0;
    let (parent, child, parent_anchor, child_anchor) = if right_to_left {
        (at, previous, entry, exit)
    } else {
        (previous, at, exit, entry)
    };
    place(buffer, child, parent, child_anchor, parent_anchor);
    Applied::Yes(at.saturating_add(1))
}

/// One `EntryExitRecord`'s anchor: `which` is 0 for the entry, 2 for the exit.
fn entry_exit(data: Bytes<'_>, index: u16, which: usize) -> Option<(i32, i32)> {
    let count = usize::from(data.u16(4)?);
    if usize::from(index) >= count {
        return None;
    }
    let at = 6usize
        .checked_add(usize::from(index).checked_mul(4)?)?
        .checked_add(which)?;
    anchor(data.offset16(at)?)
}

/// Type 4: put a mark on a base glyph.
fn mark_to_base(
    runner: &mut Runner<'_, '_>,
    buffer: &mut Buffer,
    data: Bytes<'_>,
    at: usize,
) -> Applied {
    if data.u16(0) != Some(1) {
        return Applied::No;
    }
    let Some(mark) = buffer.glyph_id(at) else {
        return Applied::No;
    };
    let Some(marks) = data.offset16(2).map(Coverage::new) else {
        return Applied::No;
    };
    let Some(mark_index) = marks.index_of(mark) else {
        return Applied::No;
    };
    let Some(base_at) = runner.previous_base(buffer, at) else {
        return Applied::No;
    };
    let Some(base) = buffer.glyph_id(base_at) else {
        return Applied::No;
    };
    let Some(bases) = data.offset16(4).map(Coverage::new) else {
        return Applied::No;
    };
    let Some(base_index) = bases.index_of(base) else {
        return Applied::No;
    };
    let Some(classes) = data.u16(6).map(usize::from) else {
        return Applied::No;
    };
    let Some((class, mark_anchor)) = mark_record(data, 8, mark_index, classes) else {
        return Applied::No;
    };
    let Some(array) = data.offset16(10) else {
        return Applied::No;
    };
    let count = usize::from(array.u16(0).unwrap_or(0));
    if usize::from(base_index) >= count {
        return Applied::No;
    }
    let Some(row) = usize::from(base_index).checked_mul(classes) else {
        return Applied::No;
    };
    let Some(slot) = row.checked_add(class) else {
        return Applied::No;
    };
    let Some(base_anchor) = array.offset16_at(2, slot).and_then(anchor) else {
        return Applied::No;
    };
    place(buffer, at, base_at, mark_anchor, base_anchor);
    Applied::Yes(at.saturating_add(1))
}

/// Type 5: put a mark on one *component* of a ligature.
///
/// Which component is the question this lookup exists to answer, and the
/// answer was decided back in `GSUB`: the ligature stamped every mark inside
/// it with the component it followed. A mark that carries this ligature's
/// serial uses that number; one that does not — a mark that arrived after the
/// ligature was formed, or was never part of it — goes on the **last**
/// component, which is the specification's own fallback and the reason a
/// vowel typed after a ligature lands at its end rather than at its start.
fn mark_to_ligature(
    runner: &mut Runner<'_, '_>,
    buffer: &mut Buffer,
    data: Bytes<'_>,
    at: usize,
) -> Applied {
    if data.u16(0) != Some(1) {
        return Applied::No;
    }
    let Some(mark) = buffer.glyph_id(at) else {
        return Applied::No;
    };
    let Some(marks) = data.offset16(2).map(Coverage::new) else {
        return Applied::No;
    };
    let Some(mark_index) = marks.index_of(mark) else {
        return Applied::No;
    };
    let Some(lig_at) = runner.previous_base(buffer, at) else {
        return Applied::No;
    };
    let Some(ligature) = buffer.glyph_id(lig_at) else {
        return Applied::No;
    };
    let Some(ligatures) = data.offset16(4).map(Coverage::new) else {
        return Applied::No;
    };
    let Some(lig_index) = ligatures.index_of(ligature) else {
        return Applied::No;
    };
    let Some(classes) = data.u16(6).map(usize::from) else {
        return Applied::No;
    };
    let Some((class, mark_anchor)) = mark_record(data, 8, mark_index, classes) else {
        return Applied::No;
    };
    let Some(array) = data.offset16(10) else {
        return Applied::No;
    };
    let count = usize::from(array.u16(0).unwrap_or(0));
    if usize::from(lig_index) >= count {
        return Applied::No;
    }
    let Some(attach) = array.offset16_at(2, usize::from(lig_index)) else {
        return Applied::No;
    };
    let components = usize::from(attach.u16(0).unwrap_or(0));
    if components == 0 {
        return Applied::No;
    }
    let mark_props = buffer.props(at);
    let lig_props = buffer.props(lig_at);
    let component = if mark_props.lig_id != 0
        && mark_props.lig_id == lig_props.lig_id
        && mark_props.lig_comp > 0
    {
        usize::from(mark_props.lig_comp)
            .min(components)
            .saturating_sub(1)
    } else {
        components.saturating_sub(1)
    };
    let Some(row) = component.checked_mul(classes) else {
        return Applied::No;
    };
    let Some(slot) = row.checked_add(class) else {
        return Applied::No;
    };
    let Some(lig_anchor) = attach.offset16_at(2, slot).and_then(anchor) else {
        return Applied::No;
    };
    place(buffer, at, lig_at, mark_anchor, lig_anchor);
    Applied::Yes(at.saturating_add(1))
}

/// Type 6: stack one mark on another.
///
/// The backward search here does **not** step over marks, unlike types 4 and
/// 5, because a mark is exactly what it is looking for. What it does check is
/// that the two marks belong together: two marks over the same base stack,
/// and two marks over different components of one ligature do not — which is
/// the difference between a Vietnamese circumflex-and-acute and two accents
/// drawn on top of each other.
fn mark_to_mark<'a>(
    runner: &mut Runner<'a, '_>,
    buffer: &mut Buffer,
    data: Bytes<'a>,
    skipper: &Skipper<'a>,
    at: usize,
) -> Applied {
    if data.u16(0) != Some(1) {
        return Applied::No;
    }
    let Some(mark) = buffer.glyph_id(at) else {
        return Applied::No;
    };
    let Some(marks) = data.offset16(2).map(Coverage::new) else {
        return Applied::No;
    };
    let Some(mark_index) = marks.index_of(mark) else {
        return Applied::No;
    };
    let Some(other_at) = runner.previous_mark(buffer, skipper, at) else {
        return Applied::No;
    };
    if buffer.props(other_at).class != GlyphClass::Mark {
        return Applied::No;
    }
    if !same_attachment(buffer, at, other_at) {
        return Applied::No;
    }
    let Some(other) = buffer.glyph_id(other_at) else {
        return Applied::No;
    };
    let Some(others) = data.offset16(4).map(Coverage::new) else {
        return Applied::No;
    };
    let Some(other_index) = others.index_of(other) else {
        return Applied::No;
    };
    let Some(classes) = data.u16(6).map(usize::from) else {
        return Applied::No;
    };
    let Some((class, mark_anchor)) = mark_record(data, 8, mark_index, classes) else {
        return Applied::No;
    };
    let Some(array) = data.offset16(10) else {
        return Applied::No;
    };
    let count = usize::from(array.u16(0).unwrap_or(0));
    if usize::from(other_index) >= count {
        return Applied::No;
    }
    let Some(row) = usize::from(other_index).checked_mul(classes) else {
        return Applied::No;
    };
    let Some(slot) = row.checked_add(class) else {
        return Applied::No;
    };
    let Some(other_anchor) = array.offset16_at(2, slot).and_then(anchor) else {
        return Applied::No;
    };
    place(buffer, at, other_at, mark_anchor, other_anchor);
    Applied::Yes(at.saturating_add(1))
}

/// Whether two marks hang off the same thing.
///
/// Same ligature serial and same component: they stack. Neither in a
/// ligature: they stack, because both are over the same base. Different
/// serials: they stack only if one of the two *is* a ligature in its own
/// right, which is the case a face makes when it ligates two marks and then
/// puts a third on the result.
fn same_attachment(buffer: &Buffer, one: usize, two: usize) -> bool {
    let a = buffer.props(one);
    let b = buffer.props(two);
    if a.lig_id == b.lig_id {
        a.lig_id == 0 || a.lig_comp == b.lig_comp
    } else {
        (a.lig_id != 0 && a.lig_comp == 0) || (b.lig_id != 0 && b.lig_comp == 0)
    }
}

/// One `MarkRecord`: which class the mark is in, and where its anchor is.
fn mark_record(
    data: Bytes<'_>,
    offset: usize,
    index: u16,
    classes: usize,
) -> Option<(usize, (i32, i32))> {
    let array = data.offset16(offset)?;
    let count = usize::from(array.u16(0)?);
    if usize::from(index) >= count {
        return None;
    }
    let at = 2usize.checked_add(usize::from(index).checked_mul(4)?)?;
    let class = usize::from(array.u16(at)?);
    if class >= classes {
        return None;
    }
    let anchor = anchor(array.offset16(at.checked_add(2)?)?)?;
    Some((class, anchor))
}

/// Records that the glyph at `at` must have its anchor meet another's.
///
/// All four attaching lookups end here: the three mark ones, and cursive,
/// whose exit and entry anchors are the same relationship with different
/// names.
///
/// The offset written is the difference between the two anchors and nothing
/// else. Everything that depends on where the other glyph *ends up* — its own
/// offset, and the advances the pen took between them — is added by
/// `propagate_attachments` once every lookup has run.
fn place(buffer: &mut Buffer, at: usize, to: usize, own: (i32, i32), other: (i32, i32)) {
    if let Some(glyph) = buffer.glyph_mut(at) {
        glyph.x_offset = other.0.saturating_sub(own.0);
        glyph.y_offset = other.1.saturating_sub(own.1);
    }
    buffer.attach(at, to);
}
